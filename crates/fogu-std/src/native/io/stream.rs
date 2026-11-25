//! Streams: data read or written a piece at a time, so a program handles a
//! file, a download or a response of any size in constant memory — the role
//! of Node's and Bun's streams, with Fogu's conventions (errors are values,
//! `for` walks a readable, nothing runs behind the program's back).
//!
//! ```fogu
//! import fs from "fogu:fs"
//!
//! const src = fs.createReadStream("big.log", { lines: true })?
//! for line in src { if line.contains("ERROR") { print(line) } }
//!
//! const out = fs.createWriteStream("copy.bin")?
//! fs.createReadStream("big.bin")?.pipe(out)?      // bytes copied, off this thread
//! ```
//!
//! A **readable** (`ReadStream` from `fs.createReadStream`, `HttpStream` from
//! `http.stream`) has `read(n?)` → buffer · nil (the end) · error,
//! `readLine()` → str · nil · error, `readAll()`, `pipe(writable)` and
//! `close()`; `for chunk in r` gives buffers (or lines, with `{ lines: true }`)
//! until the end — an error ends the loop and stays in `r.error`.
//!
//! A **writable** (`WriteStream` from `fs.createWriteStream`, `ResponseStream`
//! from `web.stream`) has `write(data)` → bytes · error, `writeLine(data)`
//! and `close()` (or `end()`).
//!
//! Waiting never blocks the other `fg` tasks: reads and writes that would wait
//! run on the pool (a read whose bytes are already buffered returns at once),
//! and `pipe` copies on the pool without the bytes ever becoming Fogu values.
//! A `ResponseStream` applies backpressure: `write` waits while more than its
//! `highWaterMark` (1 MiB) is queued for a slow client, so memory stays flat.

use std::cell::Cell;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;
use crate::native::util::bytes_of;
use crate::runtime::value::Value;

/// The size of a chunk when `read()` has no argument, and of `pipe`'s copies.
pub const CHUNK: usize = 64 * 1024;

// ---------------------------------------------------------------- readables

/// What a readable reads from: anything that can move to the pool, behind a
/// buffer this module owns (so it can tell, without reading, whether the
/// next answer is already there).
pub type Source = BufReader<Box<dyn Read + Send>>;

/// A source over `r`, with a buffer of `cap` bytes.
pub fn source(r: impl Read + Send + 'static, cap: usize) -> Source {
    BufReader::with_capacity(cap, Box::new(r) as Box<dyn Read + Send>)
}

struct ReadState {
    /// `None` once closed (or ended).
    src: Option<Source>,
    /// Bytes still allowed (`end` of `createReadStream`), `None` = no limit.
    remaining: Option<u64>,
    read: u64,
}

impl ReadState {
    /// Reads up to `n` bytes (`None` = the end).
    fn chunk(&mut self, n: usize) -> std::io::Result<Option<Vec<u8>>> {
        let n = match self.remaining {
            Some(0) => return Ok(None),
            Some(r) => n.min(r as usize),
            None => n,
        };
        let Some(src) = self.src.as_mut() else { return Ok(None) };
        let mut buf = vec![0u8; n];
        let mut got = 0;
        // one read of what is there; a second only when nothing came yet
        while got == 0 {
            match src.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(k) => got += k,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        if got == 0 {
            self.src = None;
            return Ok(None);
        }
        buf.truncate(got);
        self.account(got);
        Ok(Some(buf))
    }

    /// A line without its `\n` / `\r\n` (`None` = the end).
    fn line(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        if self.remaining == Some(0) {
            return Ok(None);
        }
        let Some(src) = self.src.as_mut() else { return Ok(None) };
        let mut line = Vec::new();
        let n = match self.remaining {
            Some(r) => src.take(r).read_until(b'\n', &mut line)?,
            None => src.read_until(b'\n', &mut line)?,
        };
        if n == 0 {
            self.src = None;
            return Ok(None);
        }
        self.account(n);
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }

    fn all(&mut self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(c) = self.chunk(CHUNK)? {
            out.extend_from_slice(&c);
        }
        Ok(out)
    }

    fn account(&mut self, n: usize) {
        self.read += n as u64;
        if let Some(r) = self.remaining.as_mut() {
            *r -= (n as u64).min(*r);
        }
    }

    /// Can `line()` / `chunk()` answer without waiting (the bytes are in
    /// the buffer)? Then the call stays on this thread. Only looks at the
    /// buffer: reading here could wait on the network on the interpreter's
    /// thread (and the data may come from a task of this very thread).
    fn buffered(&self, want_line: bool) -> bool {
        match self.src.as_ref() {
            Some(s) => {
                let b = s.buffer();
                !b.is_empty() && (!want_line || b.contains(&b'\n'))
            }
            None => true,
        }
    }
}

/// A readable stream (`ReadStream`, `HttpStream`).
pub struct ReadStreamValue {
    kind: &'static str,
    state: Arc<Mutex<ReadState>>,
    /// `for x in stream` gives lines (str) instead of buffers.
    lines: bool,
    /// The error that ended a `for` over this stream.
    error: Rc<Cell<Option<String>>>,
    /// Fields of the source (`path`, `status`, `headers`…).
    fields: Vec<(&'static str, Value)>,
    /// A `read` / `pipe` running on the pool.
    busy: Arc<AtomicBool>,
}

impl ReadStreamValue {
    pub fn new(kind: &'static str, src: Source, remaining: Option<u64>, lines: bool, fields: Vec<(&'static str, Value)>) -> Value {
        NativeObjectRef::new(ReadStreamValue {
            kind,
            state: Arc::new(Mutex::new(ReadState { src: Some(src), remaining, read: 0 })),
            lines,
            error: Rc::new(Cell::new(None)),
            fields,
            busy: Arc::new(AtomicBool::new(false)),
        })
        .to_value()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ReadState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs `op` on the pool (or here, when the bytes are already buffered)
    /// and makes the value on this thread.
    fn run<T: Send + 'static>(
        &self,
        what: &'static str,
        want_line: bool,
        op: impl FnOnce(&mut ReadState) -> std::io::Result<T> + Send + 'static,
        finish: impl FnOnce(T) -> Value + 'static,
    ) -> Result<Poller, String> {
        if self.busy.load(Ordering::Acquire) {
            return Err(format!("{}.{}: another task is reading this stream", self.kind, what));
        }
        let kind = self.kind;
        let wrap = move |r: std::io::Result<T>| -> Result<T, String> {
            r.map_err(|e| io_err!("{}.{}: {}", kind, what, e))
        };
        let inline = self.lock().buffered(want_line);
        if inline {
            let r = op(&mut self.lock());
            let mut v = Some(wrap(r).map(finish));
            return Ok(Box::new(move || v.take()));
        }
        let state = Arc::clone(&self.state);
        let busy = Arc::clone(&self.busy);
        busy.store(true, Ordering::Release);
        let pending = rt::spawn_blocking(move || {
            let r = op(&mut state.lock().unwrap_or_else(|e| e.into_inner()));
            busy.store(false, Ordering::Release);
            r
        });
        fogu_core::runtime::sched::offer_gate(pending.gate());
        let mut finish = Some(finish);
        Ok(Box::new(move || {
            let r = pending.try_take()?;
            Some(wrap(r).map(|v| finish.take().expect("polled after completion")(v)))
        }))
    }
}

fn opt_buffer(v: Option<Vec<u8>>) -> Value {
    v.map(Value::buffer).unwrap_or(Value::Nil)
}

fn opt_text(v: Option<Vec<u8>>) -> Value {
    match v {
        Some(b) => Value::Str(String::from_utf8_lossy(&b).into_owned().into()),
        None => Value::Nil,
    }
}

fn size_arg(args: &[Value], what: &str) -> Result<usize, String> {
    match args.first() {
        None | Some(Value::Nil) => Ok(CHUNK),
        Some(Value::Int(n)) if *n > 0 => Ok(*n as usize),
        Some(other) => Err(format!("{}: the size must be an int > 0, got {}", what, other.to_display_string())),
    }
}

impl FoguObject for ReadStreamValue {
    fn name(&self) -> &str {
        self.kind
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "bytesRead" => Some(Value::Int(self.lock().read as i64)),
            "open" => Some(Value::Bool(self.lock().src.is_some())),
            "lines" => Some(Value::Bool(self.lines)),
            "error" => {
                let e = self.error.take();
                self.error.set(e.clone());
                Some(match e {
                    Some(m) => Value::Error(Rc::new(crate::runtime::value::ErrorValue { message: m, cause: Value::Nil, trace: Vec::new() })),
                    None => Value::Nil,
                })
            }
            _ => self.fields.iter().find(|(k, _)| *k == field).map(|(_, v)| v.clone()),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        Some(match method {
            "read" => {
                let n = match size_arg(args, "read") {
                    Ok(n) => n,
                    Err(e) => return Some(Err(e)),
                };
                self.run("read", false, move |s| s.chunk(n), opt_buffer)
            }
            "readLine" => self.run("readLine", true, |s| s.line(), opt_text),
            "readAll" => self.run("readAll", false, |s| s.all(), Value::buffer),
            "readText" => self.run("readText", false, |s| s.all(), |b| Value::Str(String::from_utf8_lossy(&b).into_owned().into())),
            // the next item of `for x in stream`: a buffer, or a line
            "next" => {
                if self.lines {
                    self.run("next", true, |s| s.line(), opt_text)
                } else {
                    self.run("next", false, |s| s.chunk(CHUNK), opt_buffer)
                }
            }
            "pipe" => {
                let Some(sink) = args.first().and_then(Sink::of) else {
                    return Some(Err(format!(
                        "{}.pipe(dst): dst must be a WriteStream (fs.createWriteStream) or a ResponseStream (web.stream); for other destinations write each chunk: for c in src {{ dst.write(c)? }}",
                        self.kind
                    )));
                };
                if self.busy.load(Ordering::Acquire) {
                    return Some(Err(format!("{}.pipe: another task is reading this stream", self.kind)));
                }
                let state = Arc::clone(&self.state);
                let busy = Arc::clone(&self.busy);
                busy.store(true, Ordering::Release);
                let kind = self.kind;
                let pending = rt::spawn_blocking(move || {
                    let r = pipe(&mut state.lock().unwrap_or_else(|e| e.into_inner()), &sink);
                    busy.store(false, Ordering::Release);
                    r
                });
                fogu_core::runtime::sched::offer_gate(pending.gate());
                Ok(Box::new(move || {
                    let r = pending.try_take()?;
                    Some(r.map(|n| Value::Int(n as i64)).map_err(|e| io_err!("{}.pipe: {}", kind, e)))
                }))
            }
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "close" | "destroy" => {
                self.lock().src = None;
                Ok(Value::Nil)
            }
            // the `for` loop records the error that ended it
            "__fail" => {
                self.error.set(args.first().map(|v| match v {
                    Value::Error(e) => e.message.clone(),
                    other => other.to_display_string(),
                }));
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on {} (read readLine readAll readText pipe close)", other, self.kind)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["read", "readLine", "readAll", "readText", "pipe", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------- writables

/// Where `pipe` (and a writable) puts bytes. Everything in it can move to the pool.
#[derive(Clone)]
pub enum Sink {
    File(Arc<Mutex<Option<BufWriter<File>>>>),
    #[cfg(all(feature = "network", not(target_family = "wasm")))]
    Response(ResponseTx),
}

impl Sink {
    /// The sink of a writable stream value, if it is one.
    fn of(v: &Value) -> Option<Sink> {
        let Value::Object(o) = v else { return None };
        let rc = o.inner();
        let inner = rc.borrow();
        let any = inner.as_any();
        if let Some(w) = any.downcast_ref::<WriteStreamValue>() {
            return Some(Sink::File(Arc::clone(&w.file)));
        }
        #[cfg(all(feature = "network", not(target_family = "wasm")))]
        if let Some(r) = any.downcast_ref::<ResponseStreamValue>() {
            return Some(Sink::Response(r.tx.clone()));
        }
        None
    }

    /// Writes `data` (on the pool: may wait for a slow client).
    fn put(&self, data: Vec<u8>) -> std::io::Result<()> {
        match self {
            Sink::File(f) => match f.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                Some(w) => w.write_all(&data),
                None => Err(std::io::Error::other("the stream is closed")),
            },
            #[cfg(all(feature = "network", not(target_family = "wasm")))]
            Sink::Response(tx) => {
                while tx.full() {
                    if tx.closed() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                tx.send(data)
            }
        }
    }

    fn flush(&self) -> std::io::Result<()> {
        match self {
            Sink::File(f) => match f.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                Some(w) => w.flush(),
                None => Ok(()),
            },
            #[cfg(all(feature = "network", not(target_family = "wasm")))]
            Sink::Response(_) => Ok(()),
        }
    }
}

/// Copies what is left of `src` into `sink`: the bytes are read and written
/// on the pool, never as Fogu values.
fn pipe(src: &mut ReadState, sink: &Sink) -> std::io::Result<u64> {
    let mut total = 0u64;
    while let Some(chunk) = src.chunk(CHUNK)? {
        total += chunk.len() as u64;
        sink.put(chunk)?;
    }
    sink.flush()?;
    Ok(total)
}

/// A writable file (`fs.createWriteStream`).
pub struct WriteStreamValue {
    file: Arc<Mutex<Option<BufWriter<File>>>>,
    path: String,
    written: Arc<AtomicUsize>,
}

impl WriteStreamValue {
    pub fn new(file: File, path: String) -> Value {
        NativeObjectRef::new(WriteStreamValue {
            file: Arc::new(Mutex::new(Some(BufWriter::with_capacity(CHUNK, file)))),
            path,
            written: Arc::new(AtomicUsize::new(0)),
        })
        .to_value()
    }
}

impl FoguObject for WriteStreamValue {
    fn name(&self) -> &str {
        "WriteStream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "path" => Some(Value::Str(self.path.as_str().into())),
            "bytesWritten" => Some(Value::Int(self.written.load(Ordering::Relaxed) as i64)),
            "open" => Some(Value::Bool(self.file.lock().unwrap_or_else(|e| e.into_inner()).is_some())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // a buffered write: the data goes to the file's buffer (64 KiB),
            // written out when it is full, at flush() and at close()
            "write" | "writeLine" => {
                crate::native::util::max_args(args, 1, method)?;
                let mut data = match args.first() {
                    Some(v) => bytes_of(v, method)?,
                    None => return Err(format!("{}(data) expects 1 argument", method)),
                };
                if method == "writeLine" {
                    data.push(b'\n');
                }
                let mut f = self.file.lock().unwrap_or_else(|e| e.into_inner());
                let Some(w) = f.as_mut() else { return Err(io_err!("WriteStream.{}: the stream is closed", method)) };
                w.write_all(&data).map_err(|e| io_err!("WriteStream.{}: {}: {}", method, self.path, e))?;
                self.written.fetch_add(data.len(), Ordering::Relaxed);
                Ok(Value::Int(data.len() as i64))
            }
            "flush" => {
                let mut f = self.file.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(w) = f.as_mut() {
                    w.flush().map_err(|e| io_err!("WriteStream.flush: {}: {}", self.path, e))?;
                }
                Ok(Value::Nil)
            }
            "close" | "end" => {
                if let Some(v) = args.first() {
                    self.call("write", std::slice::from_ref(v))?;
                }
                let taken = self.file.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(mut w) = taken {
                    w.flush().map_err(|e| io_err!("WriteStream.close: {}: {}", self.path, e))?;
                }
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on WriteStream (write writeLine flush close end)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["write", "writeLine", "flush", "close", "end"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// --------------------------------------------------------- response bodies

/// The sending side of a streamed HTTP response body, with the bytes still
/// queued for the client: each chunk, once the server has written it to the
/// socket, gives its size back (`Queued`), so a writer can wait while too
/// much is queued (backpressure) instead of filling memory.
#[cfg(all(feature = "network", not(target_family = "wasm")))]
#[derive(Clone)]
pub struct ResponseTx {
    tx: tokio::sync::mpsc::UnboundedSender<bytes::Bytes>,
    queued: Arc<AtomicUsize>,
    limit: usize,
}

/// A chunk on its way to the client; dropping it (sent, or the client left)
/// takes its bytes off the queue and wakes the interpreter.
#[cfg(all(feature = "network", not(target_family = "wasm")))]
struct Queued {
    data: Vec<u8>,
    queued: Arc<AtomicUsize>,
}

#[cfg(all(feature = "network", not(target_family = "wasm")))]
impl AsRef<[u8]> for Queued {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

#[cfg(all(feature = "network", not(target_family = "wasm")))]
impl Drop for Queued {
    fn drop(&mut self) {
        self.queued.fetch_sub(self.data.len(), Ordering::AcqRel);
        fogu_core::runtime::sched::wake();
    }
}

#[cfg(all(feature = "network", not(target_family = "wasm")))]
impl ResponseTx {
    pub fn new(limit: usize) -> (ResponseTx, tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (ResponseTx { tx, queued: Arc::new(AtomicUsize::new(0)), limit }, rx)
    }

    fn full(&self) -> bool {
        self.queued.load(Ordering::Acquire) >= self.limit
    }

    fn closed(&self) -> bool {
        self.tx.is_closed()
    }

    fn send(&self, data: Vec<u8>) -> std::io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.queued.fetch_add(data.len(), Ordering::AcqRel);
        let chunk = bytes::Bytes::from_owner(Queued { data, queued: Arc::clone(&self.queued) });
        // a refused chunk is dropped here, which takes it off the queue
        self.tx.send(chunk).map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the client disconnected"))
    }
}

/// `web.stream({ status, headers, contentType, highWaterMark })`: a response
/// whose body the handler writes a piece at a time. Return it from the
/// handler; write to it from an `fg` task (or `pipe` a readable into it).
#[cfg(all(feature = "network", not(target_family = "wasm")))]
pub struct ResponseStreamValue {
    pub tx: ResponseTx,
    /// Taken by the server when the handler returns the stream.
    pub rx: Option<tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>>,
    pub status: i64,
    pub headers: ahash::AHashMap<String, String>,
    written: usize,
}

#[cfg(all(feature = "network", not(target_family = "wasm")))]
impl ResponseStreamValue {
    pub fn new(status: i64, headers: ahash::AHashMap<String, String>, limit: usize) -> Value {
        let (tx, rx) = ResponseTx::new(limit);
        NativeObjectRef::new(ResponseStreamValue { tx, rx: Some(rx), status, headers, written: 0 }).to_value()
    }
}

#[cfg(all(feature = "network", not(target_family = "wasm")))]
impl FoguObject for ResponseStreamValue {
    fn name(&self) -> &str {
        "ResponseStream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "open" => Value::Bool(!self.tx.closed()),
            "bytesWritten" => Value::Int(self.written as i64),
            "queued" => Value::Int(self.tx.queued.load(Ordering::Relaxed) as i64),
            "status" => Value::Int(self.status),
            "headers" => crate::native::util::map(self.headers.iter().map(|(k, v)| (k.as_str(), Value::Str(v.clone().into()))).collect()),
            _ => return None,
        })
    }

    /// `write` waits (cooperatively) while the client is behind.
    fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        if !matches!(method, "write" | "writeLine" | "end") {
            return None;
        }
        let tx = self.tx.clone();
        Some(Box::new(move || !tx.full() || tx.closed()))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "write" | "writeLine" => {
                crate::native::util::max_args(args, 1, method)?;
                let mut data = match args.first() {
                    Some(v) => bytes_of(v, method)?,
                    None => return Err(format!("{}(data) expects 1 argument", method)),
                };
                if method == "writeLine" {
                    data.push(b'\n');
                }
                let n = data.len();
                self.tx.send(data).map_err(|e| io_err!("ResponseStream.{}: {}", method, e))?;
                self.written += n;
                Ok(Value::Int(n as i64))
            }
            "close" | "end" => {
                if let Some(v) = args.first() {
                    self.call("write", std::slice::from_ref(v))?;
                }
                // dropping the sender ends the response
                let (tx, _) = tokio::sync::mpsc::unbounded_channel();
                self.tx.tx = tx;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on ResponseStream (write writeLine close end)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["write", "writeLine", "close", "end"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------- fs entry points

/// `fs.createReadStream(path, { start, end, lines, bufferSize })`.
pub fn create_read_stream(args: &[Value]) -> Result<Value, String> {
    crate::native::util::max_args(args, 2, "fs.createReadStream")?;
    let path = crate::native::util::str_arg(args, 0, "fs.createReadStream")?;
    let (mut start, mut end, mut lines, mut cap) = (0u64, None, false, CHUNK);
    match args.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            let m = m.borrow();
            let int = |k: &str| -> Result<Option<u64>, String> {
                match m.get(k) {
                    None | Some(Value::Nil) => Ok(None),
                    Some(Value::Int(n)) if *n >= 0 => Ok(Some(*n as u64)),
                    Some(other) => Err(format!("fs.createReadStream: {} must be an int >= 0, got {}", k, other.to_display_string())),
                }
            };
            start = int("start")?.unwrap_or(0);
            end = int("end")?;
            if let Some(b) = int("bufferSize")? {
                cap = (b as usize).clamp(4096, 16 * 1024 * 1024);
            }
            lines = matches!(m.get("lines"), Some(Value::Bool(true)));
        }
        Some(other) => return Err(format!("fs.createReadStream: options must be a map, got '{}'", other.type_name())),
    }
    let mut f = File::open(&path).map_err(|e| io_err!("fs.createReadStream: {}: {}", path, e))?;
    if f.metadata().map(|m| m.is_dir()).unwrap_or(false) {
        return Err(io_err!("fs.createReadStream: {}: is a directory", path));
    }
    if start > 0 {
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(start)).map_err(|e| io_err!("fs.createReadStream: {}: {}", path, e))?;
    }
    // `end` is inclusive, as in Node
    let remaining = end.map(|e| (e + 1).saturating_sub(start));
    let src = source(f, cap);
    Ok(ReadStreamValue::new("ReadStream", src, remaining, lines, vec![("path", Value::Str(path.into()))]))
}

/// `fs.createWriteStream(path, { append })`.
pub fn create_write_stream(args: &[Value]) -> Result<Value, String> {
    crate::native::util::max_args(args, 2, "fs.createWriteStream")?;
    let path = crate::native::util::str_arg(args, 0, "fs.createWriteStream")?;
    let append = match args.get(1) {
        None | Some(Value::Nil) => false,
        Some(Value::Map(m)) => matches!(m.borrow().get("append"), Some(Value::Bool(true))),
        Some(other) => return Err(format!("fs.createWriteStream: options must be a map, got '{}'", other.type_name())),
    };
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(&path)
        .map_err(|e| io_err!("fs.createWriteStream: {}: {}", path, e))?;
    Ok(WriteStreamValue::new(f, path))
}

/// A readable stream returned by a handler (`return fs.createReadStream(p)`),
/// as the body of the response: a pool thread copies it into the response a
/// chunk at a time, waiting for the client like `ResponseStream.write` does.
/// Returns the body and the content type the source suggests.
#[cfg(all(feature = "network", not(target_family = "wasm")))]
pub fn readable_body(o: &NativeObjectRef) -> Option<(tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>, Option<String>)> {
    let rc = o.inner();
    let inner = rc.borrow();
    let r = inner.as_any().downcast_ref::<ReadStreamValue>()?;
    let ctype = match r.fields.iter().find(|(k, _)| *k == "path") {
        Some((_, Value::Str(p))) => Some(crate::native::http::mime_of(std::path::Path::new(p.as_str())).to_string()),
        _ => match r.fields.iter().find(|(k, _)| *k == "headers") {
            Some((_, Value::Map(h))) => h.borrow().get("content-type").map(|v| v.to_display_string()),
            _ => None,
        },
    };
    let (tx, rx) = ResponseTx::new(1 << 20);
    let state = Arc::clone(&r.state);
    let busy = Arc::clone(&r.busy);
    busy.store(true, Ordering::Release);
    std::thread::Builder::new()
        .name("fogu-stream".into())
        .spawn(move || {
            let _ = pipe(&mut state.lock().unwrap_or_else(|e| e.into_inner()), &Sink::Response(tx));
            busy.store(false, Ordering::Release);
        })
        .ok()?;
    Some((rx, ctype))
}

/// Is `o` a readable stream (`for x in o` reads it)?
pub fn is_readable(name: &str) -> bool {
    matches!(name, "ReadStream" | "HttpStream")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(text: &str, remaining: Option<u64>) -> ReadState {
        let src = source(std::io::Cursor::new(text.as_bytes().to_vec()), 8);
        ReadState { src: Some(src), remaining, read: 0 }
    }

    #[test]
    fn chunks_lines_and_limits() {
        let mut s = state("one\r\ntwo\nthree", None);
        assert_eq!(s.line().unwrap().unwrap(), b"one");
        assert_eq!(s.line().unwrap().unwrap(), b"two");
        assert_eq!(s.line().unwrap().unwrap(), b"three");
        assert!(s.line().unwrap().is_none());
        assert_eq!(s.read, 14);

        let mut s = state("0123456789", Some(4));
        assert_eq!(s.chunk(100).unwrap().unwrap(), b"0123");
        assert!(s.chunk(100).unwrap().is_none());

        let mut s = state("abcdefghij", None);
        assert_eq!(s.all().unwrap(), b"abcdefghij");
    }

    #[cfg(all(feature = "network", not(target_family = "wasm")))]
    #[test]
    fn response_queue_gives_bytes_back() {
        let (tx, mut rx) = ResponseTx::new(10);
        tx.send(vec![1; 6]).unwrap();
        tx.send(vec![2; 6]).unwrap();
        assert!(tx.full());
        let first = rx.try_recv().unwrap();
        assert_eq!(first.len(), 6);
        drop(first);
        assert!(!tx.full());
        // the client left: what was queued is dropped, and a new chunk is refused
        drop(rx);
        assert_eq!(tx.queued.load(Ordering::Relaxed), 0);
        assert!(tx.send(vec![3]).is_err());
        assert_eq!(tx.queued.load(Ordering::Relaxed), 0);
    }
}
