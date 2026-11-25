//! The HTTP/1.1 server of `fogu:http` / `fogu:web` on the interpreter thread
//! (Linux, plain HTTP without static mounts or Socket.IO; the other servers
//! run on the I/O threads, see `http.rs`).
//!
//! A request never leaves the thread that runs its handler: the listener and
//! every connection are sockets of this thread's reactor (`runtime/reactor.rs`),
//! each connection is one task that reads a request, calls the handler and
//! writes the answer. There is no queue between threads, no wake-up of
//! another thread and no allocation freed by a thread that did not make it.
//! A program that wants more cores starts one server per `sync.Worker` with
//! `reusePort` (the kernel spreads the connections).
//!
//! Limits are the ones of the other server: `maxHeaderBytes` (431),
//! `headerTimeout` (the connection is closed; also the idle keep-alive time),
//! `maxBody` (413), `readTimeout` (408), `handlerTimeout` (504). Deadlines
//! are kept by one sweeper task per thread, not one timer per request.
//! See `docs/architecture/direct-http.md`.

use std::cell::{Cell, RefCell};
use std::io::{ErrorKind, Read};
use std::mem::MaybeUninit;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ahash::AHashMap as HashMap;

use crate::native::http::{self, BodyStream, RequestValue, ServerOptions};
use crate::native::object::NativeObjectRef;
use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::value::Value;
use crate::runtime::{reactor, sched};

/// What one receive reads at most.
const CHUNK: usize = 64 * 1024;

/// Can this server run on the interpreter thread? (`FOGU_HTTP=threads`
/// keeps every server on the I/O threads.)
pub fn eligible(opts: &ServerOptions) -> bool {
    static THREADS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    opts.tls.is_none()
        && opts.socketio.is_none()
        && opts.statics.is_empty()
        && !*THREADS.get_or_init(|| std::env::var("FOGU_HTTP").is_ok_and(|v| v == "threads"))
}

struct Server {
    id: u64,
    listener: TcpListener,
    opts: Rc<ServerOptions>,
}

/// Where a connection is, for the sweeper.
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// waiting for a request line and headers (`headerTimeout`)
    Head,
    /// reading a body (`readTimeout`)
    Body,
    /// the handler runs (`handlerTimeout`)
    Handler,
    /// writing an answer
    Write,
    /// sending a streamed body: the sweeper notices a client that left
    Stream,
}

/// What the sweeper sees of a connection.
struct Shared {
    sock: TcpStream,
    phase: Cell<Phase>,
    deadline: Cell<Option<Instant>>,
    /// the deadline passed (the sweeper already acted on the socket)
    expired: Cell<bool>,
    /// the client closed the connection during a streamed body
    gone: Cell<bool>,
    /// the gate of the streamed body being sent
    stream: RefCell<Option<sched::Gate>>,
}

impl Shared {
    fn arm(&self, phase: Phase, timeout: Option<Duration>) {
        self.phase.set(phase);
        self.deadline.set(timeout.map(|t| Instant::now() + t));
    }
}

thread_local! {
    /// Servers bound but not accepting yet (they start at the next call of the interpreter).
    static QUEUED: RefCell<Vec<Server>> = const { RefCell::new(Vec::new()) };
    /// The open connections of this thread, by descriptor.
    static CONNS: RefCell<HashMap<RawFd, Rc<Shared>>> = RefCell::new(HashMap::new());
    /// A sweeper task is running.
    static SWEEPING: Cell<bool> = const { Cell::new(false) };
    /// How often the sweeper looks: a quarter of the shortest timeout, 10 ms to 1 s.
    static TICK: Cell<Duration> = const { Cell::new(Duration::from_secs(1)) };
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// The `date` header of the current second.
    static DATE: RefCell<(u64, String)> = const { RefCell::new((0, String::new())) };
}

/// A bound listener to serve from this thread (`start_queued` starts it).
pub fn queue(id: u64, listener: TcpListener, opts: ServerOptions) {
    let shortest = [Some(opts.header_timeout), Some(opts.read_timeout), opts.handler_timeout].into_iter().flatten().min().unwrap_or(Duration::from_secs(1));
    let tick = (shortest / 4).clamp(Duration::from_millis(10), Duration::from_secs(1));
    TICK.with(|t| t.set(t.get().min(tick)));
    QUEUED.with(|q| q.borrow_mut().push(Server { id, listener, opts: Rc::new(opts) }));
}

/// Starts the accept task of every queued server.
pub fn start_queued(it: &Interpreter, env: &Rc<RefCell<Environment>>) {
    let servers = QUEUED.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for server in servers {
        let env = Rc::clone(env);
        if let Err(e) = it.spawn_task(Box::new(move |it| accept_loop(server, it, env))) {
            eprintln!("[http] cannot start the server task: {}", e);
        }
    }
}

fn open(gate: &sched::Gate) -> bool {
    gate.load(Ordering::Acquire)
}

/// Parks the task for `d`.
fn pause(d: Duration) -> Result<(), String> {
    let gate = sched::new_local_gate();
    sched::open_at(Instant::now() + d, &gate);
    sched::wait_until_on(Some(&gate), || open(&gate))
}

fn accept_loop(server: Server, it: &Interpreter, env: Rc<RefCell<Environment>>) -> Result<(), String> {
    let fd = server.listener.as_raw_fd();
    loop {
        match server.listener.accept() {
            Ok((sock, remote)) => {
                // (answers are written in one piece: nothing to gain from Nagle's delay)
                if sock.set_nonblocking(true).is_err() || sock.set_nodelay(true).is_err() {
                    continue;
                }
                let fd = sock.as_raw_fd();
                let shared = Rc::new(Shared {
                    sock,
                    phase: Cell::new(Phase::Head),
                    deadline: Cell::new(None),
                    expired: Cell::new(false),
                    gone: Cell::new(false),
                    stream: RefCell::new(None),
                });
                CONNS.with(|c| c.borrow_mut().insert(fd, Rc::clone(&shared)));
                let conn = Conn {
                    shared,
                    fd,
                    server: server.id,
                    opts: Rc::clone(&server.opts),
                    remote: remote.to_string(),
                    buf: Vec::new(),
                    out: Vec::new(),
                    drained: false,
                    watched: false,
                };
                let env = Rc::clone(&env);
                if let Err(e) = it.spawn_task(Box::new(move |it| {
                    let mut conn = conn;
                    // (an error here is the scheduler shutting down)
                    let _ = conn.serve(it, &env);
                    Ok(())
                })) {
                    eprintln!("[http] cannot start a connection: {}", e);
                    continue;
                }
                if !SWEEPING.with(|s| s.replace(true)) {
                    let started = it.spawn_task(Box::new(|_| {
                        while sweep() {
                            pause(TICK.with(|t| t.get()))?;
                        }
                        Ok(())
                    }));
                    if started.is_err() {
                        SWEEPING.with(|s| s.set(false));
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                let Some(gate) = reactor::readable(fd) else {
                    eprintln!("http.serve: cannot wait for connections");
                    return Ok(());
                };
                sched::wait_until_on(Some(&gate), || open(&gate))?;
            }
            Err(e) if matches!(e.kind(), ErrorKind::Interrupted | ErrorKind::ConnectionAborted) => {}
            Err(e) => {
                // out of descriptors…: let connections finish before trying again
                eprintln!("http.serve: accept: {}", e);
                pause(Duration::from_millis(50))?;
            }
        }
    }
}

/// Acts on the connections whose deadline passed; false when none is open
/// (the sweeper ends, the next connection starts another).
fn sweep() -> bool {
    let now = Instant::now();
    let any = CONNS.with(|c| {
        let c = c.borrow();
        for (fd, s) in c.iter() {
            let phase = s.phase.get();
            if phase == Phase::Stream {
                let mut b = [0u8; 1];
                let left = match s.sock.peek(&mut b) {
                    Ok(n) => n == 0,
                    Err(e) => !matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted),
                };
                if left {
                    s.gone.set(true);
                    if let Some(g) = s.stream.borrow().as_ref() {
                        g.store(true, Ordering::Release);
                    }
                }
            } else if s.deadline.get().is_some_and(|d| now >= d) {
                s.deadline.set(None);
                s.expired.set(true);
                if phase == Phase::Handler {
                    // the handler still runs (in the connection's task): the
                    // client is answered here and the connection ends
                    let mut out = Vec::new();
                    text_response(&mut out, 504, "timeout");
                    let _ = reactor::send(*fd, &out);
                    let _ = s.sock.shutdown(Shutdown::Both);
                } else {
                    // wakes the task waiting for the request: it reads the end of the stream
                    let _ = s.sock.shutdown(Shutdown::Read);
                }
            }
        }
        !c.is_empty()
    });
    if !any {
        SWEEPING.with(|s| s.set(false));
    }
    any
}

// ---------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------

enum BodyKind {
    Len(usize),
    Chunked,
}

/// A request line and its headers.
struct Head {
    method: String,
    path: String,
    query: Option<String>,
    headers: HashMap<String, String>,
    upgrade: bool,
    ws_key: Option<String>,
    expect: bool,
    body: BodyKind,
    meta: Meta,
}

/// What the answer needs to know of its request.
#[derive(Clone, Copy)]
struct Meta {
    /// no body in the answer (HEAD)
    head_only: bool,
    /// the client keeps the connection
    keep: bool,
    http10: bool,
}

enum Parsed {
    Partial,
    Done(Box<Head>, usize),
    Bad(u16, String),
}

fn parse_head(data: &[u8], opts: &ServerOptions) -> Parsed {
    let too_large = || Parsed::Bad(431, String::new());
    let mut slots = [const { MaybeUninit::<httparse::Header>::uninit() }; 100];
    let mut r = httparse::Request::new(&mut []);
    let used = match r.parse_with_uninit_headers(data, &mut slots) {
        Ok(httparse::Status::Complete(n)) if n <= opts.max_header => n,
        Ok(httparse::Status::Complete(_)) | Err(httparse::Error::TooManyHeaders) => return too_large(),
        Ok(httparse::Status::Partial) if data.len() >= opts.max_header => return too_large(),
        Ok(httparse::Status::Partial) => return Parsed::Partial,
        Err(_) => return Parsed::Bad(400, "bad request".into()),
    };
    let method = r.method.unwrap_or("GET");
    let http10 = r.version == Some(0);
    // origin form (`/path?query`), or absolute form (`http://host/path?query`)
    let mut target = r.path.unwrap_or("/");
    if !target.starts_with('/') {
        if let Some(i) = target.find("://") {
            target = target[i + 3..].find('/').map_or("/", |j| &target[i + 3 + j..]);
        }
    }
    let (raw_path, query) = match target.split_once('?') {
        Some((p, q)) => (p, Some(q.to_string())),
        None => (target, None),
    };
    // one canonical path for everything that looks at it (see `canonical_path`)
    let path = match http::canonical_path(raw_path) {
        Ok(p) => p,
        Err(why) => return Parsed::Bad(400, format!("bad request: {}", why)),
    };
    let mut headers = HashMap::with_capacity(r.headers.len());
    let (mut len, mut encoded, mut chunked) = (None::<usize>, false, false);
    let (mut keep, mut upgrade, mut expect) = (!http10, false, false);
    for h in r.headers.iter() {
        let name = h.name.to_ascii_lowercase();
        // values with bytes >= 0x80 are decoded as UTF-8 (lossy), as the client does
        let value = match std::str::from_utf8(h.value) {
            Ok(v) => v.to_string(),
            Err(_) => String::from_utf8_lossy(h.value).into_owned(),
        };
        match name.as_str() {
            "content-length" => {
                let v = value.trim();
                let n = if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) { v.parse::<usize>().ok() } else { None };
                match n {
                    Some(n) if len.is_none_or(|l| l == n) => len = Some(n),
                    _ => return Parsed::Bad(400, "bad request".into()),
                }
            }
            "transfer-encoding" => {
                encoded = true;
                chunked = value.rsplit(',').next().is_some_and(|t| t.trim().eq_ignore_ascii_case("chunked"));
            }
            "connection" => {
                for t in value.split(',') {
                    let t = t.trim();
                    if t.eq_ignore_ascii_case("close") {
                        keep = false;
                    } else if t.eq_ignore_ascii_case("keep-alive") {
                        keep = true;
                    }
                }
            }
            "upgrade" => upgrade = value.eq_ignore_ascii_case("websocket"),
            "expect" => expect = value.eq_ignore_ascii_case("100-continue"),
            _ => {}
        }
        headers.insert(name, value);
    }
    let body = if encoded {
        // a length next to an encoding is how requests are smuggled past a
        // proxy: such a request is refused, not interpreted
        if !chunked || len.is_some() || http10 {
            return Parsed::Bad(400, "bad request".into());
        }
        BodyKind::Chunked
    } else {
        BodyKind::Len(len.unwrap_or(0))
    };
    if matches!(body, BodyKind::Len(n) if n > opts.max_body) {
        return Parsed::Bad(413, "request body too large".into());
    }
    let ws_key = if upgrade { headers.get("sec-websocket-key").cloned() } else { None };
    let meta = Meta { head_only: method == "HEAD", keep, http10 };
    Parsed::Done(Box::new(Head { method: method.to_string(), path, query, headers, upgrade, ws_key, expect, body, meta }), used)
}

/// Where a chunked body is.
#[derive(Clone, Copy)]
enum Chunk {
    /// in the size line: value so far, digits read, inside an extension
    Size(usize, u8, bool),
    SizeLf(usize),
    Data(usize),
    DataCr,
    DataLf,
    /// in the trailers: is the current line empty so far?
    Trailer(bool),
    TrailerLf(bool),
}

/// Collects a request body from the bytes received.
struct BodyReader {
    body: Vec<u8>,
    /// bytes still expected (a body with a length)
    left: usize,
    chunk: Option<Chunk>,
    /// bytes of chunk framing and trailers read (bounded)
    framing: usize,
    max: usize,
}

impl BodyReader {
    fn new(kind: &BodyKind, max: usize) -> BodyReader {
        match kind {
            BodyKind::Len(n) => BodyReader { body: Vec::new(), left: *n, chunk: None, framing: 0, max },
            BodyKind::Chunked => BodyReader { body: Vec::new(), left: 0, chunk: Some(Chunk::Size(0, 0, false)), framing: 0, max },
        }
    }

    /// Takes what belongs to the body from `data`: how many bytes, and
    /// whether the body is complete.
    fn feed(&mut self, data: &[u8]) -> Result<(usize, bool), (u16, &'static str)> {
        let Some(mut state) = self.chunk else {
            let n = self.left.min(data.len());
            if n > 0 {
                if self.body.is_empty() {
                    self.body.reserve_exact(self.left);
                }
                self.body.extend_from_slice(&data[..n]);
                self.left -= n;
            }
            return Ok((n, self.left == 0));
        };
        const BAD: (u16, &str) = (400, "bad request");
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            if let Chunk::Data(left) = state {
                let n = left.min(data.len() - i);
                self.body.extend_from_slice(&data[i..i + n]);
                i += n;
                state = if n == left { Chunk::DataCr } else { Chunk::Data(left - n) };
                continue;
            }
            i += 1;
            self.framing += 1;
            if self.framing > 64 * 1024 + self.body.len() / 4 {
                return Err(BAD);
            }
            state = match (state, b) {
                (Chunk::Size(v, d, false), b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F') if d < 15 => Chunk::Size(v * 16 + (b as char).to_digit(16).unwrap_or(0) as usize, d + 1, false),
                (Chunk::Size(v, d, _), b';' | b' ' | b'\t') if d > 0 => Chunk::Size(v, d, true),
                (Chunk::Size(v, d, _), b'\r') if d > 0 => Chunk::SizeLf(v),
                (Chunk::Size(v, d, true), _) if b != b'\n' => Chunk::Size(v, d, true),
                (Chunk::SizeLf(0), b'\n') => Chunk::Trailer(true),
                (Chunk::SizeLf(v), b'\n') => {
                    if self.body.len().saturating_add(v) > self.max {
                        return Err((413, "request body too large"));
                    }
                    Chunk::Data(v)
                }
                (Chunk::DataCr, b'\r') => Chunk::DataLf,
                (Chunk::DataLf, b'\n') => Chunk::Size(0, 0, false),
                (Chunk::Trailer(empty), b'\r') => Chunk::TrailerLf(empty),
                (Chunk::Trailer(_), _) if b != b'\n' => Chunk::Trailer(false),
                (Chunk::TrailerLf(true), b'\n') => {
                    self.chunk = Some(state);
                    return Ok((i, true));
                }
                (Chunk::TrailerLf(false), b'\n') => Chunk::Trailer(true),
                _ => return Err(BAD),
            };
        }
        self.chunk = Some(state);
        Ok((i, false))
    }
}

/// How reading a request ended when no request came out of it.
enum Fail {
    /// nothing to answer: the client left, or was too slow with its headers
    Close,
    Status(u16, String),
}

enum Step {
    More,
    Head(Box<Head>, BodyReader, bool),
    Fail(Fail),
}

// ---------------------------------------------------------------------
// A connection
// ---------------------------------------------------------------------

struct Conn {
    shared: Rc<Shared>,
    fd: RawFd,
    server: u64,
    opts: Rc<ServerOptions>,
    remote: String,
    /// received and not consumed yet (the start of a request, or the next one)
    buf: Vec<u8>,
    /// the answer being written
    out: Vec<u8>,
    /// the last receive emptied the socket and the reactor watches it: unless
    /// the reactor saw more arrive, the next one would find nothing
    drained: bool,
    watched: bool,
}

impl Drop for Conn {
    fn drop(&mut self) {
        // before the socket closes (with the last `Shared`): its number may be reused
        reactor::forget(self.fd);
        CONNS.with(|c| c.borrow_mut().remove(&self.fd));
    }
}

impl Conn {
    fn wait_readable(&self) -> Result<bool, String> {
        let Some(gate) = reactor::readable(self.fd) else { return Ok(false) };
        sched::wait_until_on(Some(&gate), || open(&gate))?;
        Ok(true)
    }

    /// Receives once and hands the bytes to `f` (which must not wait: they
    /// are in a buffer the whole thread shares). `first`: the first receive
    /// of an operation, skipped when the socket is known to be empty.
    fn recv_with<R>(&mut self, first: &mut bool, f: impl FnOnce(&mut Conn, &[u8]) -> R) -> std::io::Result<R> {
        if std::mem::take(first) && self.drained && !reactor::take_readable(self.fd) {
            return Err(ErrorKind::WouldBlock.into());
        }
        SCRATCH.with(|s| {
            let mut s = s.borrow_mut();
            if s.len() < CHUNK {
                s.resize(CHUNK, 0);
            }
            let got = match reactor::recv(self.fd, &mut s[..]) {
                Ok(got) => got,
                Err(e) => {
                    self.drained = false;
                    return Err(e);
                }
            };
            if got > 0 && got < s.len() && !self.watched {
                self.watched = reactor::watch(self.fd);
            }
            self.drained = got > 0 && got < s.len() && self.watched;
            Ok(f(self, &s[..got]))
        })
    }

    /// A head was parsed; `rest` is what came after it.
    fn begin_body(&mut self, head: Box<Head>, rest: &[u8]) -> Step {
        let mut reader = BodyReader::new(&head.body, self.opts.max_body);
        match reader.feed(rest) {
            Ok((used, done)) => {
                // what follows the body is the next request
                self.buf.extend_from_slice(&rest[used..]);
                Step::Head(head, reader, done)
            }
            Err((code, msg)) => Step::Fail(Fail::Status(code, msg.into())),
        }
    }

    /// Looks for a request in the unconsumed bytes.
    fn on_buffer(&mut self) -> Step {
        let buf = std::mem::take(&mut self.buf);
        match parse_head(&buf, &self.opts) {
            Parsed::Partial => {
                self.buf = buf;
                Step::More
            }
            Parsed::Done(head, used) => self.begin_body(head, &buf[used..]),
            Parsed::Bad(code, msg) => Step::Fail(Fail::Status(code, msg)),
        }
    }

    /// New bytes arrived: usually a whole request, parsed where it was received.
    fn on_data(&mut self, data: &[u8]) -> Step {
        if !self.buf.is_empty() {
            self.buf.extend_from_slice(data);
            return self.on_buffer();
        }
        match parse_head(data, &self.opts) {
            Parsed::Partial => {
                self.buf.extend_from_slice(data);
                Step::More
            }
            Parsed::Done(head, used) => self.begin_body(head, &data[used..]),
            Parsed::Bad(code, msg) => Step::Fail(Fail::Status(code, msg)),
        }
    }

    /// The next request of the connection and its body.
    fn read_request(&mut self) -> Result<Result<(Box<Head>, Vec<u8>), Fail>, String> {
        self.shared.arm(Phase::Head, Some(self.opts.header_timeout));
        let mut first = true;
        let mut step = if self.buf.is_empty() { Step::More } else { self.on_buffer() };
        let (head, mut reader, done) = loop {
            match step {
                Step::Head(head, reader, done) => break (head, reader, done),
                Step::Fail(f) => return Ok(Err(f)),
                Step::More => {}
            }
            step = match self.recv_with(&mut first, |me, data| if data.is_empty() { Step::Fail(Fail::Close) } else { me.on_data(data) }) {
                Ok(step) => step,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if !self.wait_readable()? {
                        return Ok(Err(Fail::Close));
                    }
                    Step::More
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => Step::More,
                Err(_) => Step::Fail(Fail::Close),
            };
        };
        if !done {
            if head.expect {
                self.out.clear();
                self.out.extend_from_slice(b"HTTP/1.1 100 Continue\r\n\r\n");
                if !self.flush()? {
                    return Ok(Err(Fail::Close));
                }
            }
            self.shared.arm(Phase::Body, Some(self.opts.read_timeout));
            loop {
                let fed = self.recv_with(&mut first, |me, data| {
                    if data.is_empty() {
                        return Err(if me.shared.expired.get() { Fail::Status(408, "request timeout".into()) } else { Fail::Close });
                    }
                    match reader.feed(data) {
                        Ok((used, done)) => {
                            me.buf.extend_from_slice(&data[used..]);
                            Ok(done)
                        }
                        Err((code, msg)) => Err(Fail::Status(code, msg.into())),
                    }
                });
                match fed {
                    Ok(Ok(true)) => break,
                    Ok(Ok(false)) => {}
                    Ok(Err(f)) => return Ok(Err(f)),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        if !self.wait_readable()? {
                            return Ok(Err(Fail::Close));
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => return Ok(Err(Fail::Close)),
                }
            }
        }
        Ok(Ok((head, reader.body)))
    }

    /// Serves the connection until it ends.
    fn serve(&mut self, it: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<(), String> {
        let mut served = 0u32;
        loop {
            let (head, body) = match self.read_request()? {
                Ok(r) => r,
                Err(Fail::Close) => return Ok(()),
                Err(Fail::Status(code, msg)) => {
                    self.shared.arm(Phase::Write, None);
                    self.out.clear();
                    text_response(&mut self.out, code, &msg);
                    if self.flush()? {
                        self.linger();
                    }
                    return Ok(());
                }
            };
            let head = *head;
            let meta = head.meta;
            let (upgrade, ws_key) = (head.upgrade, head.ws_key);
            let req = NativeObjectRef::new(RequestValue {
                method: head.method,
                path: head.path,
                query: head.query,
                headers: head.headers,
                body,
                upgrade,
                remote: self.remote.clone(),
                static_guard: false,
            })
            .to_value();
            let Some(handler) = http::handler_of(self.server) else {
                self.out.clear();
                text_response(&mut self.out, 500, "internal error");
                self.flush()?;
                return Ok(());
            };
            self.shared.arm(Phase::Handler, self.opts.handler_timeout);
            let resp = http::answer(it, handler, req, Rc::clone(env));
            // too late: the sweeper answered 504 and ended the connection
            if self.shared.expired.get() {
                return Ok(());
            }
            self.shared.arm(Phase::Write, None);
            if !self.respond(resp, meta, upgrade, ws_key)? {
                return Ok(());
            }
            // a client that pipelines without end does not keep the thread
            served += 1;
            if served % 32 == 0 && !self.buf.is_empty() {
                sched::yield_now()?;
            }
        }
    }

    /// Sends everything in `data`; false when the client is gone.
    fn send_all(&self, data: &[u8]) -> Result<bool, String> {
        let mut sent = 0;
        while sent < data.len() {
            match reactor::send(self.fd, &data[sent..]) {
                Ok(n) => sent += n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    let Some(gate) = reactor::writable(self.fd) else { return Ok(false) };
                    sched::wait_until_on(Some(&gate), || open(&gate))?;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Ok(false),
            }
        }
        Ok(true)
    }

    /// Sends `out` and empties it; false when the client is gone.
    fn flush(&mut self) -> Result<bool, String> {
        let mut out = std::mem::take(&mut self.out);
        let sent = self.send_all(&out);
        // (one large answer does not keep its buffer for the connection's life)
        if out.capacity() > 2 * CHUNK {
            out = Vec::new();
        } else {
            out.clear();
        }
        self.out = out;
        sent
    }

    /// After an answer that ends the connection with input unread: closing
    /// then would reset the connection before the client reads the answer.
    fn linger(&mut self) {
        let _ = self.shared.sock.shutdown(Shutdown::Write);
        SCRATCH.with(|s| {
            let mut s = s.borrow_mut();
            if s.len() < CHUNK {
                s.resize(CHUNK, 0);
            }
            for _ in 0..16 {
                if !matches!(reactor::recv(self.fd, &mut s[..]), Ok(n) if n > 0) {
                    break;
                }
            }
        });
    }

    /// The head and the body of an answer with a known body, into `out`.
    fn fill(&mut self, status: i64, headers: &HashMap<String, String>, body: &[u8], meta: Meta) -> bool {
        self.out.clear();
        let (keep, bodyless) = write_head(&mut self.out, status, headers, meta, Some(body.len() as u64));
        if !bodyless {
            self.out.extend_from_slice(body);
        }
        keep
    }

    fn text(&mut self, status: i64, msg: &str, meta: Meta) -> Result<bool, String> {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain; charset=utf-8".to_string());
        let keep = self.fill(status, &headers, msg.as_bytes(), meta);
        Ok(self.flush()? && keep)
    }

    /// Writes the answer; true when the connection serves another request.
    fn respond(&mut self, resp: Value, meta: Meta, upgrade: bool, ws_key: Option<String>) -> Result<bool, String> {
        // the usual answer is written straight from the response object
        if let Value::Object(o) = &resp {
            if o.with_name(|n| n == "Response") {
                let inner = o.inner();
                let filled = {
                    let b = inner.borrow();
                    let any = b.as_any();
                    if let Some(r) = any.downcast_ref::<crate::native::web::ResponseValue>() {
                        if r.file.is_none() { Some(self.fill(r.status, &r.headers, r.body.as_bytes(), meta)) } else { None }
                    } else {
                        any.downcast_ref::<http::ResponseValue>().map(|r| self.fill(r.status, &r.headers, r.body.as_bytes(), meta))
                    }
                };
                if let Some(keep) = filled {
                    return Ok(self.flush()? && keep);
                }
            }
        }
        let mut plain = http::plain_of(resp);
        if let Some(path) = plain.file.take() {
            return self.send_file(path, plain.status, plain.headers, meta);
        }
        if let Some(stream) = plain.stream.take() {
            return self.send_stream(stream, plain.status, &plain.headers, meta);
        }
        if let (Some(sock_tx), true, 101) = (plain.upgrade.take(), upgrade, plain.status) {
            return self.upgrade(sock_tx, ws_key.unwrap_or_default(), &plain.headers);
        }
        // (a guarded static file: this server has no static mounts)
        if plain.serve_static {
            return self.text(404, "not found", meta);
        }
        let keep = self.fill(plain.status, &plain.headers, &plain.body, meta);
        Ok(self.flush()? && keep)
    }

    /// `web.file`: read off this thread, a piece at a time.
    fn send_file(&mut self, path: PathBuf, status: i64, mut headers: HashMap<String, String>, meta: Meta) -> Result<bool, String> {
        let opened = crate::native::rt::offload(move || -> std::io::Result<(std::fs::File, u64, PathBuf)> {
            let file = std::fs::File::open(&path)?;
            let info = file.metadata()?;
            if info.is_dir() {
                return Err(ErrorKind::NotFound.into());
            }
            Ok((file, info.len(), path))
        })?;
        let Ok((mut file, len, path)) = opened else { return self.text(404, "file not found", meta) };
        if !headers.contains_key("content-type") {
            headers.insert("content-type".into(), http::mime_of(&path).into());
        }
        self.out.clear();
        let (keep, bodyless) = write_head(&mut self.out, status, &headers, meta, Some(len));
        let mut left = if bodyless { 0 } else { len };
        while left > 0 {
            let want = left.min(CHUNK as u64) as usize;
            let (f, piece, got) = crate::native::rt::offload(move || {
                let mut piece = vec![0u8; want];
                let got = file.read(&mut piece);
                (file, piece, got)
            })?;
            file = f;
            match got {
                Ok(n) if n > 0 => {
                    self.out.extend_from_slice(&piece[..n]);
                    left -= n as u64;
                    if !self.flush()? {
                        return Ok(false);
                    }
                }
                // shorter than announced (truncated meanwhile): only closing says so
                _ => {
                    self.flush()?;
                    return Ok(false);
                }
            }
        }
        Ok(self.flush()? && keep)
    }

    /// A streamed body (`web.sse`, `web.stream`, a readable): each chunk is
    /// sent as it is queued.
    fn send_stream(&mut self, stream: BodyStream, status: i64, headers: &HashMap<String, String>, meta: Meta) -> Result<bool, String> {
        let BodyStream { mut rx, ready } = stream;
        self.out.clear();
        let (keep, bodyless) = write_head(&mut self.out, status, headers, meta, None);
        let chunked = !meta.http10;
        *self.shared.stream.borrow_mut() = Some(sched::Gate::clone(&ready));
        self.shared.arm(Phase::Stream, None);
        // (a producer on another thread is work in flight, not a deadlock)
        let _producing = sched::IoGuard::new();
        let mut ended = false;
        let mut alive = true;
        while alive && !ended {
            ready.store(false, Ordering::Release);
            loop {
                match rx.try_recv() {
                    Ok(chunk) => {
                        if !bodyless && !chunk.is_empty() {
                            if chunked {
                                self.out.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
                            }
                            self.out.extend_from_slice(&chunk);
                            if chunked {
                                self.out.extend_from_slice(b"\r\n");
                            }
                        }
                        // (a long run of chunks goes out in pieces)
                        if self.out.len() >= CHUNK {
                            break;
                        }
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                        if self.out.is_empty() {
                            sched::wait_until_on(Some(&ready), || open(&ready))?;
                        }
                        break;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
            }
            if ended && chunked && !bodyless {
                self.out.extend_from_slice(b"0\r\n\r\n");
            }
            if !self.out.is_empty() {
                alive = self.flush()?;
            }
            alive = alive && !self.shared.gone.get();
        }
        // closes the channel: the producer's next write says the client left
        drop(rx);
        *self.shared.stream.borrow_mut() = None;
        Ok(alive && keep && chunked)
    }

    /// An accepted WebSocket upgrade: the 101, then the socket goes to the
    /// WebSocket code (which runs on the I/O threads).
    fn upgrade(&mut self, sock_tx: std::sync::mpsc::Sender<crate::native::websocket::WsIo>, key: String, headers: &HashMap<String, String>) -> Result<bool, String> {
        let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
        self.out.clear();
        self.out.extend_from_slice(b"HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: ");
        self.out.extend_from_slice(accept.as_bytes());
        self.out.extend_from_slice(b"\r\n");
        if let Some(p) = headers.get("sec-websocket-protocol").filter(|p| !p.bytes().any(|b| b == b'\r' || b == b'\n')) {
            self.out.extend_from_slice(b"sec-websocket-protocol: ");
            self.out.extend_from_slice(p.as_bytes());
            self.out.extend_from_slice(b"\r\n");
        }
        self.out.extend_from_slice(b"\r\n");
        if !self.flush()? {
            return Ok(false);
        }
        // a duplicate of the socket for the I/O threads; this thread's
        // descriptor closes with the connection (and leaves the reactor first)
        reactor::forget(self.fd);
        let early = std::mem::take(&mut self.buf);
        let _runtime = crate::native::rt::tokio().enter();
        if let Ok(sock) = self.shared.sock.try_clone().and_then(tokio::net::TcpStream::from_std) {
            let io: crate::native::websocket::WsIo = if early.is_empty() { Box::new(sock) } else { Box::new(Prefixed { early, at: 0, io: sock }) };
            let _ = sock_tx.send(io);
        }
        Ok(false)
    }
}

/// A socket whose first bytes were already received (what a client sent
/// right behind its upgrade request).
struct Prefixed {
    early: Vec<u8>,
    at: usize,
    io: tokio::net::TcpStream,
}

impl tokio::io::AsyncRead for Prefixed {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<std::io::Result<()>> {
        if self.at < self.early.len() {
            let n = (self.early.len() - self.at).min(buf.remaining());
            buf.put_slice(&self.early[self.at..self.at + n]);
            self.at += n;
            return std::task::Poll::Ready(Ok(()));
        }
        std::pin::Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for Prefixed {
    fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, data: &[u8]) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.io).poll_write(cx, data)
    }

    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

// ---------------------------------------------------------------------
// Writing an answer
// ---------------------------------------------------------------------

fn reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        422 => "Unprocessable Entity",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => match code / 100 {
            1 => "Informational",
            2 => "Success",
            3 => "Redirection",
            4 => "Client Error",
            _ => "Server Error",
        },
    }
}

fn push_num(out: &mut Vec<u8>, mut n: u64) {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[i..]);
}

fn push_date(out: &mut Vec<u8>) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    DATE.with(|d| {
        let mut d = d.borrow_mut();
        if d.0 != secs || d.1.is_empty() {
            *d = (secs, http::httpdate(secs));
        }
        out.extend_from_slice(b"date: ");
        out.extend_from_slice(d.1.as_bytes());
        out.extend_from_slice(b"\r\n");
    });
}

/// A header name the program gave is sent only if it is a token, a value
/// only without line breaks: a header can never inject another one.
fn sendable(name: &str, value: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        && !value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
}

/// Writes the status line and the headers. `len`: the size of the body, or
/// `None` for a streamed one. Returns whether the connection stays open and
/// whether the answer has no body (HEAD, 1xx, 204, 304).
fn write_head(out: &mut Vec<u8>, status: i64, headers: &HashMap<String, String>, meta: Meta, len: Option<u64>) -> (bool, bool) {
    let code = if (100..1000).contains(&status) { status as u16 } else { 200 };
    out.extend_from_slice(b"HTTP/1.1 ");
    push_num(out, code as u64);
    out.push(b' ');
    out.extend_from_slice(reason(code).as_bytes());
    out.extend_from_slice(b"\r\n");
    let mut keep = meta.keep;
    let mut dated = false;
    let mut given_len = None;
    for (k, v) in headers {
        if !sendable(k, v) || k.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        if k.eq_ignore_ascii_case("content-length") {
            given_len = v.trim().parse::<u64>().ok();
            continue;
        }
        if k.eq_ignore_ascii_case("connection") {
            if v.to_ascii_lowercase().contains("close") {
                keep = false;
            }
            continue;
        }
        dated = dated || k.eq_ignore_ascii_case("date");
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(v.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    let empty = code < 200 || code == 204 || code == 304;
    match len {
        Some(n) if !empty => {
            // a HEAD answer may announce the size its GET would have
            let n = if meta.head_only { given_len.unwrap_or(n) } else { n };
            out.extend_from_slice(b"content-length: ");
            push_num(out, n);
            out.extend_from_slice(b"\r\n");
        }
        Some(_) => {}
        // HTTP/1.0 has no chunks: the end of the connection ends the body
        None if meta.http10 => keep = false,
        None => out.extend_from_slice(b"transfer-encoding: chunked\r\n"),
    }
    if !dated {
        push_date(out);
    }
    if !keep {
        out.extend_from_slice(b"connection: close\r\n");
    } else if meta.http10 {
        out.extend_from_slice(b"connection: keep-alive\r\n");
    }
    out.extend_from_slice(b"\r\n");
    (keep, empty || meta.head_only)
}

/// A whole plain-text answer that ends the connection.
fn text_response(out: &mut Vec<u8>, code: u16, msg: &str) {
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain; charset=utf-8".to_string());
    write_head(out, code as i64, &headers, Meta { head_only: false, keep: false, http10: false }, Some(msg.len() as u64));
    out.extend_from_slice(msg.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(text: &str) -> Parsed {
        parse_head(text.as_bytes(), &ServerOptions::default())
    }

    #[test]
    fn request_heads() {
        let Parsed::Done(h, used) = head("GET /a/../b?x=1 HTTP/1.1\r\nHost: x\r\nX-A: 1\r\n\r\nrest") else { panic!() };
        assert_eq!((h.method.as_str(), h.path.as_str(), h.query.as_deref(), used), ("GET", "/b", Some("x=1"), 45));
        assert_eq!(h.headers.get("x-a").map(String::as_str), Some("1"));
        assert!(h.meta.keep && !h.meta.http10 && !h.meta.head_only);
        assert!(matches!(h.body, BodyKind::Len(0)));
        // absolute form, HTTP/1.0, Connection
        let Parsed::Done(h, _) = head("HEAD http://h:1/p HTTP/1.0\r\n\r\n") else { panic!() };
        assert!(h.path == "/p" && !h.meta.keep && h.meta.http10 && h.meta.head_only);
        let Parsed::Done(h, _) = head("GET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\n") else { panic!() };
        assert!(h.meta.keep);
        let Parsed::Done(h, _) = head("GET / HTTP/1.1\r\nConnection: close\r\n\r\n") else { panic!() };
        assert!(!h.meta.keep);
        assert!(matches!(head("GET / HTTP/1.1\r\nHost: x\r\n"), Parsed::Partial));
        // refused: a broken line, an escaped separator, a length and an encoding together
        assert!(matches!(head("GET\r\n\r\n"), Parsed::Bad(400, _)));
        assert!(matches!(head("GET /a%2Fb HTTP/1.1\r\n\r\n"), Parsed::Bad(400, _)));
        assert!(matches!(head("POST / HTTP/1.1\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n"), Parsed::Bad(400, _)));
        assert!(matches!(head("POST / HTTP/1.1\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\n"), Parsed::Bad(400, _)));
        assert!(matches!(head("POST / HTTP/1.1\r\nContent-Length: +3\r\n\r\n"), Parsed::Bad(400, _)));
        assert!(matches!(head("POST / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n"), Parsed::Bad(400, _)));
        // limits
        let small = ServerOptions { max_header: 8192, max_body: 10, ..ServerOptions::default() };
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(9000));
        assert!(matches!(parse_head(long.as_bytes(), &small), Parsed::Bad(431, _)));
        assert!(matches!(parse_head(&long.as_bytes()[..8500], &small), Parsed::Bad(431, _)));
        assert!(matches!(parse_head(b"POST / HTTP/1.1\r\nContent-Length: 11\r\n\r\n", &small), Parsed::Bad(413, _)));
    }

    #[test]
    fn bodies() {
        let mut r = BodyReader::new(&BodyKind::Len(5), 100);
        assert_eq!(r.feed(b"ab").unwrap(), (2, false));
        assert_eq!(r.feed(b"cdeNEXT").unwrap(), (3, true));
        assert_eq!(r.body, b"abcde");
        // chunked, fed a byte at a time and in one piece
        let text = b"4\r\nWiki\r\n6;ext=1\r\npedia \r\nE\r\nin \r\n\r\nchunks.\r\n0\r\nX-T: 1\r\n\r\nNEXT";
        let mut whole = BodyReader::new(&BodyKind::Chunked, 100);
        assert_eq!(whole.feed(text).unwrap(), (text.len() - 4, true));
        assert_eq!(whole.body, b"Wikipedia in \r\n\r\nchunks.");
        let mut bytes = BodyReader::new(&BodyKind::Chunked, 100);
        let mut done = false;
        for b in &text[..text.len() - 4] {
            assert!(!done);
            done = bytes.feed(std::slice::from_ref(b)).unwrap().1;
        }
        assert!(done);
        assert_eq!(bytes.body, whole.body);
        // too large, and broken framing
        assert_eq!(BodyReader::new(&BodyKind::Chunked, 3).feed(b"4\r\nWiki\r\n").unwrap_err().0, 413);
        assert_eq!(BodyReader::new(&BodyKind::Chunked, 100).feed(b"zz\r\n").unwrap_err().0, 400);
        assert_eq!(BodyReader::new(&BodyKind::Chunked, 100).feed(b"1\r\nab").unwrap_err().0, 400);
        assert_eq!(BodyReader::new(&BodyKind::Chunked, 100).feed(b"\r\n").unwrap_err().0, 400);
    }

    #[test]
    fn answers() {
        let meta = Meta { head_only: false, keep: true, http10: false };
        let mut headers = HashMap::new();
        headers.insert("x-a".to_string(), "1".to_string());
        headers.insert("x-bad".to_string(), "1\r\nset-cookie: a=b".to_string());
        headers.insert("bad name".to_string(), "1".to_string());
        headers.insert("content-length".to_string(), "99".to_string());
        let mut out = Vec::new();
        assert_eq!(write_head(&mut out, 200, &headers, meta, Some(2)), (true, false));
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n") && text.ends_with("\r\n\r\n"), "{text}");
        assert!(text.contains("x-a: 1\r\n") && text.contains("content-length: 2\r\n") && text.contains("date: "), "{text}");
        assert!(!text.contains("cookie") && !text.contains("bad name") && !text.contains("99") && !text.contains("connection"), "{text}");
        // HEAD keeps the announced size; 204 has none; a stream is chunked (closed for HTTP/1.0)
        let mut out = Vec::new();
        assert_eq!(write_head(&mut out, 200, &headers, Meta { head_only: true, ..meta }, Some(0)), (true, true));
        assert!(String::from_utf8(out).unwrap().contains("content-length: 99\r\n"));
        let mut out = Vec::new();
        assert_eq!(write_head(&mut out, 204, &HashMap::new(), meta, Some(0)), (true, true));
        assert!(!String::from_utf8(out).unwrap().contains("content-length"));
        let mut out = Vec::new();
        assert_eq!(write_head(&mut out, 200, &HashMap::new(), meta, None), (true, false));
        assert!(String::from_utf8(out).unwrap().contains("transfer-encoding: chunked\r\n"));
        let mut out = Vec::new();
        assert_eq!(write_head(&mut out, 200, &HashMap::new(), Meta { http10: true, ..meta }, None), (false, false));
        assert!(String::from_utf8(out).unwrap().contains("connection: close\r\n"));
        let mut out = Vec::new();
        text_response(&mut out, 431, "x");
        assert!(String::from_utf8(out).unwrap().starts_with("HTTP/1.1 431 Request Header Fields Too Large\r\n"));
    }
}
