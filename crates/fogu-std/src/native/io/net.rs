//! `fogu:net` module — TCP and UDP sockets on the shared tokio runtime.
//!
//! ```fogu
//! import net from "fogu:net"
//!
//! const server = net.tcpListen("127.0.0.1:9000")?
//! fg fn() {
//!     for conn in server {                          // accept() in a loop
//!         fg fn() {
//!             while true {
//!                 const line = conn.readLine()?     // nil at end of stream
//!                 if line == nil { break }
//!                 conn.writeLine("echo: " + line)?
//!             }
//!             conn.close()
//!         }()
//!     }
//! }()
//!
//! const c = net.tcpConnect("127.0.0.1:9000")?
//! c.writeLine("hi")?
//! print(c.readLine()?)                              // echo: hi
//! ```
//!
//! Every operation that waits (`accept`, `read*`, `write*`, `recv*`,
//! `tcpConnect`) runs on the runtime: the calling task waits cooperatively and
//! the other `fg` tasks keep running; completion wakes the scheduler (no
//! polling). Data is read as a `buffer` (`read`, `readExact`, `readAll`,
//! `recv`) or as text (`readLine`, `readText`); `write`/`send` take a str, a
//! buffer or a list of byte ints. Network failures are `error` values;
//! `setTimeout(d)` bounds every later operation (an int is milliseconds).

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex as AsyncMutex;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;
use crate::native::util::{bytes_of, duration_arg, list, max_args, nfn_var, str_arg};

// ================= MODULE =================

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("tcpListen".into(), nfn_var("tcpListen", 1, tcp_listen));
    e.insert("tcpConnect".into(), nfn_var("tcpConnect", 1, tcp_connect));
    e.insert("udpBind".into(), nfn_var("udpBind", 1, udp_bind));
    e.insert("resolve".into(), nfn_var("resolve", 1, resolve));
    ModuleValue {
        name: "fogu:net".into(),
        exports: RefCell::new(e),
    }
}

/// `tcpListen(addr, { reusePort, backlog })` → TcpListener
///
/// `reusePort: true` (Linux, macOS, BSD) lets several listeners — one per
/// `sync.Worker`, each on its own core — take the same port; the kernel
/// spreads the incoming connections over them.
fn tcp_listen(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "tcpListen")?;
    let addr = str_arg(args, 0, "tcpListen(addr)")?;
    let (mut reuse_port, mut backlog) = (false, 1024u32);
    match args.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            for (k, v) in m.borrow().iter() {
                match (k.as_str(), v) {
                    ("reusePort", Value::Bool(b)) => reuse_port = *b,
                    ("backlog", Value::Int(n)) if *n > 0 && *n <= u32::MAX as i64 => backlog = *n as u32,
                    ("reusePort" | "backlog", other) => {
                        return Err(format!("tcpListen: invalid '{}': {}", k, other.to_display_string()));
                    }
                    (other, _) => return Err(format!("tcpListen: unknown option '{}' (reusePort backlog)", other)),
                }
            }
        }
        Some(other) => return Err(format!("tcpListen(addr, opts): options must be a map, got '{}'", other.type_name())),
    }
    #[cfg(not(unix))]
    if reuse_port {
        return Err(io_err!("tcpListen: reusePort is not available on this system"));
    }
    let a = addr.clone();
    let listener = rt::block_on(async move {
        let sa = tokio::net::lookup_host(a.as_str())
            .await?
            .next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no address"))?;
        let sock = if sa.is_ipv4() { tokio::net::TcpSocket::new_v4()? } else { tokio::net::TcpSocket::new_v6()? };
        // as the standard listener does: a restarted server can bind at once
        #[cfg(unix)]
        sock.set_reuseaddr(true)?;
        #[cfg(unix)]
        if reuse_port {
            sock.set_reuseport(true)?;
        }
        sock.bind(sa)?;
        sock.listen(backlog)
    })?
    .map_err(|e| io_err!("tcpListen: {} at {}", e, addr))?;
    let local = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| "?".into());
    Ok(NativeObjectRef::new(TcpListenerValue { listener: Some(Arc::new(listener)), local, timeout: None }).to_value())
}

/// `tcpConnect(addr, timeout?)` → TcpStream
fn tcp_connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "tcpConnect")?;
    let addr = str_arg(args, 0, "tcpConnect(addr)")?;
    let timeout = opt_timeout(args.get(1), "tcpConnect")?.unwrap_or(Duration::from_secs(30));
    let a = addr.clone();
    let stream = rt::block_on(async move { tokio::time::timeout(timeout, tokio::net::TcpStream::connect(a)).await })?
        .map_err(|_| io_err!("tcpConnect: timed out connecting to {}", addr))?
        .map_err(|e| io_err!("tcpConnect: {} at {}", e, addr))?;
    Ok(TcpStreamValue::wrap(stream))
}

/// `udpBind(addr)` → UdpSocket
fn udp_bind(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "udpBind")?;
    let addr = str_arg(args, 0, "udpBind(addr)")?;
    let a = addr.clone();
    let sock = rt::block_on(async move { tokio::net::UdpSocket::bind(a).await })?
        .map_err(|e| io_err!("udpBind: {} at {}", e, addr))?;
    let local = sock.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| "?".into());
    #[cfg(target_os = "linux")]
    {
        let raw = sock.into_std().map_err(|e| io_err!("udpBind: {} at {}", e, addr))?;
        return Ok(NativeObjectRef::new(UdpSocketValue { raw: Some(std::rc::Rc::new(RawUdp(raw))), sock: None, local, timeout: None }).to_value());
    }
    #[cfg(not(target_os = "linux"))]
    Ok(NativeObjectRef::new(UdpSocketValue { sock: Some(Arc::new(sock)), local, timeout: None }).to_value())
}

/// `resolve(host)` → list of IP strings
fn resolve(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "resolve")?;
    let host = str_arg(args, 0, "resolve(host)")?;
    let h = host.clone();
    let addrs: Vec<SocketAddr> = rt::block_on(async move { tokio::net::lookup_host((h.as_str(), 0)).await.map(|a| a.collect()) })?
        .map_err(|e| io_err!("resolve: {}: {}", host, e))?;
    let mut ips: Vec<String> = Vec::new();
    for a in addrs {
        let ip = a.ip().to_string();
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    Ok(list(ips.into_iter().map(|s| Value::Str(s.into())).collect()))
}

// ---------- helpers ----------

/// An optional timeout argument: a duration, or an int of milliseconds.
pub(crate) fn opt_timeout(v: Option<&Value>, what: &str) -> Result<Option<Duration>, String> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Int(ms)) if *ms >= 0 => Ok(Some(Duration::from_millis(*ms as u64))),
        Some(v) => duration_arg(v, what).map(Some),
    }
}

/// `n` (the size of a read) from an optional int argument.
fn size_arg(args: &[Value], what: &str, default: usize) -> Result<usize, String> {
    match args.first() {
        None | Some(Value::Nil) => Ok(default),
        Some(Value::Int(n)) if *n > 0 => Ok(*n as usize),
        Some(other) => Err(format!("{}: the size must be an int > 0, got {}", what, other.to_display_string())),
    }
}

/// Runs `fut` on the runtime (bounded by `timeout`) and turns its result into
/// a value on the interpreter thread.
pub(crate) fn op<T, F>(what: &'static str, timeout: Option<Duration>, fut: F, finish: impl FnOnce(T) -> Value + 'static) -> Poller
where
    T: Send + 'static,
    F: std::future::Future<Output = std::io::Result<T>> + Send + 'static,
{
    let pending = rt::spawn(async move {
        match timeout {
            Some(d) => tokio::time::timeout(d, fut).await.unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into())),
            None => fut.await,
        }
    });
    // nothing but the end of this work makes the result available
    fogu_core::runtime::sched::offer_gate(pending.gate());
    let mut finish = Some(finish);
    Box::new(move || {
        Some(match pending.try_take()? {
            Ok(v) => Ok(finish.take().expect("polled after completion")(v)),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Err(io_err!("{}: timed out", what)),
            Err(e) => Err(io_err!("{}: {}", what, e)),
        })
    })
}

/// A result that is already there (the operation needed no waiting).
#[cfg(target_os = "linux")]
fn done(v: Result<Value, String>) -> Poller {
    let mut v = Some(v);
    Box::new(move || v.take())
}

#[cfg(target_os = "linux")]
thread_local! {
    /// Operations in a row that completed without waiting (see [`fast_turn`]).
    static FAST_RUN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Where the direct reads land before the bytes are copied where they go.
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// `false` on every 64th operation in a row that needed no waiting, so a
/// connection that always has data cannot keep the other tasks from running.
#[cfg(target_os = "linux")]
fn fast_turn() -> bool {
    FAST_RUN.with(|c| {
        let n = c.get() + 1;
        c.set(if n >= 64 { 0 } else { n });
        n < 64
    })
}

/// An operation done on the interpreter thread itself (Linux): `attempt` is
/// made of non-blocking system calls on `fd` (it keeps what it has done so
/// far between calls); when the socket is not ready the task parks until the
/// scheduler's reactor sees it ready, and `attempt` runs again. No hand-over
/// to the I/O threads and back — that costs far more than the calls. With a
/// `timeout`, the operation fails once it has waited that long.
#[cfg(target_os = "linux")]
fn direct<T: 'static>(
    what: &'static str,
    write: bool,
    fd: std::os::fd::RawFd,
    timeout: Option<Duration>,
    mut attempt: impl FnMut() -> std::io::Result<T> + 'static,
    finish: impl FnOnce(T) -> Value + 'static,
) -> Poller {
    use fogu_core::runtime::{reactor, sched};
    let deadline = timeout.map(|d| std::time::Instant::now() + d);
    let mut gate: Option<sched::Gate> = None;
    let mut finish = Some(finish);
    // every 64th operation in a row lets the other tasks run first
    let mut turn = fast_turn();
    let mut poller: Poller = Box::new(move || {
        if !turn {
            turn = true;
            return None;
        }
        if gate.as_ref().is_some_and(|g| !g.load(std::sync::atomic::Ordering::Acquire)) {
            return match deadline {
                Some(d) if std::time::Instant::now() >= d => Some(Err(io_err!("{}: timed out", what))),
                Some(d) => {
                    sched::add_timer(d);
                    None
                }
                None => None,
            };
        }
        match attempt() {
            Ok(v) => Some(Ok(finish.take().expect("polled after completion")(v))),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                gate = if write { reactor::writable(fd) } else { reactor::readable(fd) };
                match (&gate, deadline) {
                    (None, _) => Some(Err(io_err!("{}: cannot wait for the socket", what))),
                    // only the socket ends this wait: the task is not asked before
                    (Some(g), None) => {
                        sched::offer_gate(sched::Gate::clone(g));
                        None
                    }
                    // the deadline may end it too: the task is asked when it passes
                    (Some(_), Some(d)) => {
                        sched::add_timer(d);
                        None
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => None,
            Err(e) => Some(Err(io_err!("{}: {}", what, e))),
        }
    });
    // usually ready at once
    match poller() {
        Some(r) => done(r),
        None => poller,
    }
}

/// A plain TCP connection on Linux: the socket itself (not registered with
/// the I/O threads' runtime, which would be woken by every packet) and the
/// bytes received but not consumed yet (what `readLine` read past its line).
#[cfg(target_os = "linux")]
struct RawTcp {
    sock: std::net::TcpStream,
    buf: RefCell<Vec<u8>>,
    /// The last receive took everything the socket had (and the reactor
    /// watches it since): unless the reactor saw more arrive, the next one
    /// would find nothing.
    drained: std::cell::Cell<bool>,
}

#[cfg(target_os = "linux")]
impl Drop for RawTcp {
    fn drop(&mut self) {
        fogu_core::runtime::reactor::forget(self.fd());
    }
}

/// A UDP socket used directly (Linux).
#[cfg(target_os = "linux")]
struct RawUdp(std::net::UdpSocket);

#[cfg(target_os = "linux")]
impl Drop for RawUdp {
    fn drop(&mut self) {
        fogu_core::runtime::reactor::forget(std::os::fd::AsRawFd::as_raw_fd(&self.0));
    }
}

#[cfg(target_os = "linux")]
impl RawTcp {
    fn fd(&self) -> std::os::fd::RawFd {
        std::os::fd::AsRawFd::as_raw_fd(&self.sock)
    }

    /// Receives what the socket has (at most 64 KiB) into `buf`; 0 at the
    /// end of the stream, `WouldBlock` when there is nothing yet.
    ///
    /// `first`: the first receive of an operation. A socket known to be
    /// drained, with nothing new seen by the reactor, is not even asked:
    /// that system call would only answer "nothing yet". (Later receives of
    /// the same operation come after the reactor reported the socket ready.)
    fn fill(&self, first: &mut bool) -> std::io::Result<usize> {
        use fogu_core::runtime::reactor;
        if std::mem::take(first) && self.drained.get() && !reactor::take_readable(self.fd()) {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        SCRATCH.with(|s| {
            let mut s = s.borrow_mut();
            if s.len() < 64 * 1024 {
                s.resize(64 * 1024, 0);
            }
            let got = match reactor::recv(self.fd(), &mut s[..]) {
                Ok(got) => got,
                Err(e) => {
                    self.drained.set(false);
                    return Err(e);
                }
            };
            // less than asked for: the socket is empty now. Watched from
            // here on (if it was not), so anything that arrives is noticed.
            self.drained.set(got > 0 && got < s.len() && reactor::watch(self.fd()));
            self.buf.borrow_mut().extend_from_slice(&s[..got]);
            Ok(got)
        })
    }

    /// The first `n` unconsumed bytes.
    fn take(&self, n: usize) -> Vec<u8> {
        let mut buf = self.buf.borrow_mut();
        if n >= buf.len() {
            return std::mem::take(&mut *buf);
        }
        let rest = buf.split_off(n);
        std::mem::replace(&mut *buf, rest)
    }

    /// The operations of a stream, done directly.
    fn start(self: &std::rc::Rc<Self>, method: &str, args: &[Value], timeout: Option<Duration>) -> Option<Result<Poller, String>> {
        use fogu_core::runtime::reactor;
        let raw = std::rc::Rc::clone(self);
        let fd = raw.fd();
        match method {
            "write" | "writeLine" => {
                if let Err(e) = max_args(args, 1, method) {
                    return Some(Err(e));
                }
                let mut data = match args.first().map(|v| bytes_of(v, method)) {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => return Some(Err(e)),
                    None => return Some(Err(format!("{}(data) expects 1 argument", method))),
                };
                if method == "writeLine" {
                    data.push(b'\n');
                }
                let n = data.len() as i64;
                let what = if method == "write" { "write" } else { "writeLine" };
                let mut sent = 0;
                Some(Ok(direct(what, true, fd, timeout, move || {
                    let _alive = &raw;
                    while sent < data.len() {
                        sent += reactor::send(fd, &data[sent..])?;
                    }
                    Ok(())
                }, move |()| Value::Int(n))))
            }
            "read" | "readText" => {
                let n = match max_args(args, 1, method).and_then(|_| size_arg(args, method, 64 * 1024)) {
                    Ok(n) => n,
                    Err(e) => return Some(Err(e)),
                };
                let text = method == "readText";
                let mut first = true;
                Some(Ok(direct(if text { "readText" } else { "read" }, false, fd, timeout, move || {
                    if raw.buf.borrow().is_empty() && raw.fill(&mut first)? == 0 {
                        return Ok(Vec::new());
                    }
                    Ok(raw.take(n))
                }, move |b: Vec<u8>| {
                    if b.is_empty() {
                        Value::Nil
                    } else if text {
                        Value::Str(String::from_utf8_lossy(&b).into())
                    } else {
                        Value::buffer(b)
                    }
                })))
            }
            "readExact" => {
                let n = match max_args(args, 1, method).and_then(|_| match args.first() {
                    Some(Value::Int(n)) if *n >= 0 => Ok(*n as usize),
                    _ => Err("readExact(n) expects an int >= 0".into()),
                }) {
                    Ok(n) => n,
                    Err(e) => return Some(Err(e)),
                };
                let mut first = true;
                Some(Ok(direct("readExact", false, fd, timeout, move || {
                    while raw.buf.borrow().len() < n {
                        if raw.fill(&mut first)? == 0 {
                            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "early eof"));
                        }
                    }
                    Ok(raw.take(n))
                }, Value::buffer)))
            }
            "readAll" => {
                if let Err(e) = max_args(args, 0, method) {
                    return Some(Err(e));
                }
                let mut first = true;
                Some(Ok(direct("readAll", false, fd, timeout, move || {
                    while raw.fill(&mut first)? > 0 {}
                    Ok(raw.take(usize::MAX))
                }, Value::buffer)))
            }
            // a line without its \n / \r\n; nil at end of stream
            "readLine" => {
                if let Err(e) = max_args(args, 0, method) {
                    return Some(Err(e));
                }
                // bytes of `buf` already searched for the end of the line
                let mut seen = 0;
                let mut first = true;
                Some(Ok(direct("readLine", false, fd, timeout, move || {
                    loop {
                        let end = raw.buf.borrow()[seen..].iter().position(|&b| b == b'\n').map(|i| seen + i + 1);
                        if let Some(end) = end {
                            return Ok(Some(raw.take(end)));
                        }
                        seen = raw.buf.borrow().len();
                        if raw.fill(&mut first)? == 0 {
                            // the end of the stream ends the last line too
                            let rest = raw.take(usize::MAX);
                            return Ok(if rest.is_empty() { None } else { Some(rest) });
                        }
                    }
                }, |line: Option<Vec<u8>>| {
                    let Some(mut b) = line else { return Value::Nil };
                    if b.last() == Some(&b'\n') {
                        b.pop();
                        if b.last() == Some(&b'\r') {
                            b.pop();
                        }
                    }
                    Value::Str(String::from_utf8_lossy(&b).into())
                })))
            }
            _ => None,
        }
    }
}

pub(crate) fn closed(what: &str) -> Option<Result<Poller, String>> {
    Some(Err(io_err!("{}: the socket is closed", what)))
}

// ================= TCP LISTENER =================

pub struct TcpListenerValue {
    listener: Option<Arc<tokio::net::TcpListener>>,
    local: String,
    timeout: Option<Duration>,
}

impl FoguObject for TcpListenerValue {
    fn name(&self) -> &str {
        "TcpListener"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "addr" | "localAddr" => Some(Value::Str(self.local.as_str().into())),
            "port" => Some(Value::Int(self.local.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0))),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "close" => {
                self.listener = None;
                Ok(Value::Nil)
            }
            "setTimeout" => {
                max_args(args, 1, "setTimeout")?;
                self.timeout = opt_timeout(args.first(), "setTimeout")?;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on TcpListener (accept close setTimeout)", other)),
        }
    }

    /// `accept(timeout?)` → TcpStream
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "accept" {
            return None;
        }
        let Some(l) = self.listener.clone() else { return closed("accept") };
        let timeout = match (max_args(args, 1, "accept"), opt_timeout(args.first(), "accept")) {
            (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
            (_, Ok(t)) => t.or(self.timeout),
        };
        Some(Ok(op("accept", timeout, async move { l.accept().await.map(|(s, _)| s) }, TcpStreamValue::wrap)))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["accept", "close", "setTimeout"]
    }
}

// ================= TCP STREAM =================

/// The read side of a stream: a plain TCP half (which can be reunited with its
/// write half, for STARTTLS) or any other reader (a TLS stream's half).
pub enum StreamRead {
    Tcp(OwnedReadHalf),
    Boxed(Box<dyn AsyncRead + Send + Unpin>),
}

/// The write side of a stream (see [`StreamRead`]).
pub enum StreamWrite {
    Tcp(OwnedWriteHalf),
    Boxed(Box<dyn AsyncWrite + Send + Unpin>),
}

impl AsyncRead for StreamRead {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamRead::Tcp(r) => Pin::new(r).poll_read(cx, buf),
            StreamRead::Boxed(r) => Pin::new(r).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for StreamWrite {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            StreamWrite::Tcp(w) => Pin::new(w).poll_write(cx, buf),
            StreamWrite::Boxed(w) => Pin::new(w).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamWrite::Tcp(w) => Pin::new(w).poll_flush(cx),
            StreamWrite::Boxed(w) => Pin::new(w).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamWrite::Tcp(w) => Pin::new(w).poll_shutdown(cx),
            StreamWrite::Boxed(w) => Pin::new(w).poll_shutdown(cx),
        }
    }
}

/// A connected stream: `TcpStream` (plain TCP) or `TlsStream` (`fogu:tls`),
/// with the same methods; a TLS stream adds read-only fields (`extra`).
pub struct TcpStreamValue {
    /// Linux, plain TCP: the socket used directly (then `reader` and `writer` are empty).
    #[cfg(target_os = "linux")]
    raw: Option<std::rc::Rc<RawTcp>>,
    reader: Option<Arc<AsyncMutex<BufReader<StreamRead>>>>,
    writer: Option<Arc<AsyncMutex<StreamWrite>>>,
    peer: String,
    local: String,
    timeout: Option<Duration>,
    kind: &'static str,
    extra: Vec<(&'static str, Value)>,
}

impl TcpStreamValue {
    fn wrap(s: tokio::net::TcpStream) -> Value {
        s.set_nodelay(true).ok();
        let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
        let local = s.local_addr().map(|a| a.to_string()).unwrap_or_default();
        // Linux: out of the runtime of the I/O threads, into this thread's reactor
        #[cfg(target_os = "linux")]
        {
            let raw = s.into_std().ok().map(|sock| std::rc::Rc::new(RawTcp { sock, buf: RefCell::new(Vec::new()), drained: std::cell::Cell::new(false) }));
            return NativeObjectRef::new(TcpStreamValue {
                raw,
                reader: None,
                writer: None,
                peer,
                local,
                timeout: None,
                kind: "TcpStream",
                extra: Vec::new(),
            })
            .to_value();
        }
        #[allow(unreachable_code)]
        let (r, w) = s.into_split();
        Self::from_halves(StreamRead::Tcp(r), StreamWrite::Tcp(w), "TcpStream", peer, local, Vec::new())
    }

    /// Wraps any stream (e.g. a TLS stream over TCP) as an object of type
    /// `kind` with the methods of `TcpStream` and the fields in `extra`.
    pub fn wrap_io<S>(s: S, kind: &'static str, peer: String, local: String, extra: Vec<(&'static str, Value)>) -> Value
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let (r, w) = tokio::io::split(s);
        Self::from_halves(StreamRead::Boxed(Box::new(r)), StreamWrite::Boxed(Box::new(w)), kind, peer, local, extra)
    }

    fn from_halves(r: StreamRead, w: StreamWrite, kind: &'static str, peer: String, local: String, extra: Vec<(&'static str, Value)>) -> Value {
        NativeObjectRef::new(TcpStreamValue {
            #[cfg(target_os = "linux")]
            raw: None,
            reader: Some(Arc::new(AsyncMutex::new(BufReader::with_capacity(16 * 1024, r)))),
            writer: Some(Arc::new(AsyncMutex::new(w))),
            peer,
            local,
            timeout: None,
            kind,
            extra,
        })
        .to_value()
    }

    /// Takes the plain TCP socket out of this object (which becomes closed),
    /// to start TLS on it (STARTTLS). Fails when the stream is already TLS,
    /// closed, busy in another task, or holds data read but not consumed.
    pub fn take_tcp(&mut self, what: &str) -> Result<tokio::net::TcpStream, String> {
        if self.kind != "TcpStream" {
            return Err(format!("{}: the connection is already a {}", what, self.kind));
        }
        #[cfg(target_os = "linux")]
        if let Some(raw) = self.raw.take() {
            let raw = match std::rc::Rc::try_unwrap(raw) {
                Ok(raw) => raw,
                Err(shared) => {
                    self.raw = Some(shared);
                    return Err(io_err!("{}: the connection is in use by another task", what));
                }
            };
            let pending = raw.buf.borrow().len();
            if pending > 0 {
                self.raw = Some(std::rc::Rc::new(raw));
                return Err(io_err!("{}: {} bytes were received but not read yet", what, pending));
            }
            let _runtime = rt::tokio().enter();
            // a duplicate of the descriptor: `raw` stops watching and closes its own
            return raw.sock.try_clone().and_then(tokio::net::TcpStream::from_std).map_err(|e| io_err!("{}: {}", what, e));
        }
        let (Some(r), Some(w)) = (self.reader.take(), self.writer.take()) else {
            return Err(io_err!("{}: the socket is closed", what));
        };
        let (r, w) = match (Arc::try_unwrap(r), Arc::try_unwrap(w)) {
            (Ok(r), Ok(w)) => (r.into_inner(), w.into_inner()),
            (r, w) => {
                // restore what we took: an operation of another task holds it
                self.reader = Some(r.map(|m| Arc::new(m)).unwrap_or_else(|a| a));
                self.writer = Some(w.map(|m| Arc::new(m)).unwrap_or_else(|a| a));
                return Err(io_err!("{}: the connection is in use by another task", what));
            }
        };
        if !r.buffer().is_empty() {
            let n = r.buffer().len();
            self.reader = Some(Arc::new(AsyncMutex::new(r)));
            self.writer = Some(Arc::new(AsyncMutex::new(w)));
            return Err(io_err!("{}: {} bytes were received but not read yet", what, n));
        }
        match (r.into_inner(), w) {
            (StreamRead::Tcp(r), StreamWrite::Tcp(w)) => r.reunite(w).map_err(|e| io_err!("{}: {}", what, e)),
            _ => Err(io_err!("{}: not a plain TCP connection", what)),
        }
    }
}

impl FoguObject for TcpStreamValue {
    fn name(&self) -> &str {
        self.kind
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "peer" | "peerAddr" => Some(Value::Str(self.peer.as_str().into())),
            "localAddr" => Some(Value::Str(self.local.as_str().into())),
            #[cfg(target_os = "linux")]
            "open" => Some(Value::Bool(self.writer.is_some() || self.raw.is_some())),
            #[cfg(not(target_os = "linux"))]
            "open" => Some(Value::Bool(self.writer.is_some())),
            _ => self.extra.iter().find(|(k, _)| *k == field).map(|(_, v)| v.clone()),
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "peer" | "peerAddr" => Ok(Value::Str(self.peer.as_str().into())),
            "close" => {
                self.reader = None;
                #[cfg(target_os = "linux")]
                if let Some(raw) = self.raw.take() {
                    // sends FIN; the descriptor closes with the last reference
                    let _ = raw.sock.shutdown(std::net::Shutdown::Write);
                }
                if let Some(w) = self.writer.take() {
                    // flushes and sends FIN without making the program wait
                    rt::tokio().spawn(async move {
                        let _ = w.lock().await.shutdown().await;
                    });
                }
                Ok(Value::Nil)
            }
            "setTimeout" => {
                max_args(args, 1, "setTimeout")?;
                self.timeout = opt_timeout(args.first(), "setTimeout")?;
                Ok(Value::Nil)
            }
            other => Err(format!(
                "method '{}' does not exist on {} (read readExact readLine readText readAll write writeLine close setTimeout)",
                other, self.kind
            )),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let timeout = self.timeout;
        #[cfg(target_os = "linux")]
        if let Some(raw) = &self.raw {
            return raw.start(method, args, timeout);
        }
        match method {
            // write(data) / writeLine(data) → bytes written
            "write" | "writeLine" => {
                let Some(w) = self.writer.clone() else { return closed(method) };
                if let Err(e) = max_args(args, 1, method) {
                    return Some(Err(e));
                }
                let mut data = match args.first().map(|v| bytes_of(v, method)) {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => return Some(Err(e)),
                    None => return Some(Err(format!("{}(data) expects 1 argument", method))),
                };
                if method == "writeLine" {
                    data.push(b'\n');
                }
                let n = data.len() as i64;
                let what = if method == "write" { "write" } else { "writeLine" };
                Some(Ok(op(what, timeout, async move { w.lock().await.write_all(&data).await }, move |()| Value::Int(n))))
            }
            // read(n = 64 KiB) → buffer with what is available (at most n), nil at end of stream
            "read" | "readExact" | "readAll" | "readLine" | "readText" => {
                let Some(r) = self.reader.clone() else { return closed(method) };
                let res = match method {
                    "read" | "readText" => max_args(args, 1, method).and_then(|_| size_arg(args, method, 64 * 1024)),
                    "readExact" => max_args(args, 1, method).and_then(|_| match args.first() {
                        Some(Value::Int(n)) if *n >= 0 => Ok(*n as usize),
                        _ => Err("readExact(n) expects an int >= 0".into()),
                    }),
                    _ => max_args(args, 0, method).map(|_| 0),
                };
                let n = match res {
                    Ok(n) => n,
                    Err(e) => return Some(Err(e)),
                };
                Some(Ok(match method {
                    "read" => op("read", timeout, async move {
                        let mut buf = vec![0u8; n];
                        let got = r.lock().await.read(&mut buf).await?;
                        buf.truncate(got);
                        Ok(buf)
                    }, |b: Vec<u8>| if b.is_empty() { Value::Nil } else { Value::buffer(b) }),
                    "readText" => op("readText", timeout, async move {
                        let mut buf = vec![0u8; n];
                        let got = r.lock().await.read(&mut buf).await?;
                        buf.truncate(got);
                        Ok(buf)
                    }, |b: Vec<u8>| if b.is_empty() { Value::Nil } else { Value::Str(String::from_utf8_lossy(&b).into()) }),
                    "readExact" => op("readExact", timeout, async move {
                        let mut buf = vec![0u8; n];
                        r.lock().await.read_exact(&mut buf).await?;
                        Ok(buf)
                    }, Value::buffer),
                    "readAll" => op("readAll", timeout, async move {
                        let mut buf = Vec::new();
                        r.lock().await.read_to_end(&mut buf).await?;
                        Ok(buf)
                    }, Value::buffer),
                    // a line without its \n / \r\n; nil at end of stream
                    _ => op("readLine", timeout, async move {
                        let mut buf = Vec::new();
                        let got = r.lock().await.read_until(b'\n', &mut buf).await?;
                        Ok((got, buf))
                    }, |(got, mut b): (usize, Vec<u8>)| {
                        if got == 0 {
                            return Value::Nil;
                        }
                        if b.last() == Some(&b'\n') {
                            b.pop();
                            if b.last() == Some(&b'\r') {
                                b.pop();
                            }
                        }
                        Value::Str(String::from_utf8_lossy(&b).into())
                    }),
                }))
            }
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["read", "readExact", "readLine", "readText", "readAll", "write", "writeLine", "close", "setTimeout", "peer", "peerAddr"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ================= UDP SOCKET =================

/// The operations of a UDP socket, done directly (Linux). `Ok(None)`: this
/// one needs a name lookup first.
#[cfg(target_os = "linux")]
fn udp_direct(raw: &std::rc::Rc<RawUdp>, method: &str, args: &[Value], timeout: Option<Duration>) -> Result<Option<Poller>, String> {
    use fogu_core::runtime::reactor;
    let fd = std::os::fd::AsRawFd::as_raw_fd(&raw.0);
    let sock = std::rc::Rc::clone(raw);
    /// A non-blocking receive of at most `n` bytes.
    fn receive<T>(n: usize, recv: impl FnOnce(&mut [u8]) -> std::io::Result<(usize, T)>) -> std::io::Result<(Vec<u8>, T)> {
        SCRATCH.with(|s| {
            let mut s = s.borrow_mut();
            let want = n.min(64 * 1024);
            if s.len() < want {
                s.resize(want, 0);
            }
            let (got, extra) = recv(&mut s[..want])?;
            Ok((s[..got].to_vec(), extra))
        })
    }
    Ok(Some(match method {
        "sendTo" => {
            max_args(args, 2, "sendTo")?;
            let addr = str_arg(args, 0, "sendTo(addr, data)")?;
            let Ok(to) = addr.parse::<SocketAddr>() else { return Ok(None) };
            let data = bytes_of(args.get(1).ok_or("sendTo(addr, data) expects 2 arguments")?, "sendTo")?;
            direct("sendTo", true, fd, timeout, move || {
                let _alive = &sock;
                reactor::send_to(fd, &data, to)
            }, |n| Value::Int(n as i64))
        }
        "send" => {
            max_args(args, 1, "send")?;
            let data = bytes_of(args.first().ok_or("send(data) expects 1 argument")?, "send")?;
            direct("send", true, fd, timeout, move || {
                let _alive = &sock;
                reactor::send(fd, &data)
            }, |n| Value::Int(n as i64))
        }
        "connect" => {
            max_args(args, 1, "connect")?;
            let addr = str_arg(args, 0, "connect(addr)")?;
            let Ok(to) = addr.parse::<SocketAddr>() else { return Ok(None) };
            // for a datagram socket this only records the peer
            let r = sock.0.connect(to).map(|()| Value::Nil).map_err(|e| io_err!("connect: {}", e));
            done(r)
        }
        "recv" => {
            let n = size_arg(args, "recv", 65536)?;
            direct("recv", false, fd, timeout, move || {
                let _alive = &sock;
                receive(n, |buf| reactor::recv(fd, buf).map(|got| (got, ()))).map(|(data, ())| data)
            }, Value::buffer)
        }
        // recvFrom(n = 65536) → UdpPacket { data, from, len }
        _ => {
            let n = size_arg(args, "recvFrom", 65536)?;
            direct("recvFrom", false, fd, timeout, move || {
                let _alive = &sock;
                receive(n, |buf| reactor::recv_from(fd, buf))
            }, |(data, from)| NativeObjectRef::new(UdpPacketValue { data, from: from.to_string() }).to_value())
        }
    }))
}

pub struct UdpSocketValue {
    /// Linux: the socket used directly (then `sock` is empty).
    #[cfg(target_os = "linux")]
    raw: Option<std::rc::Rc<RawUdp>>,
    sock: Option<Arc<tokio::net::UdpSocket>>,
    local: String,
    timeout: Option<Duration>,
}

impl FoguObject for UdpSocketValue {
    fn name(&self) -> &str {
        "UdpSocket"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "addr" | "localAddr" => Some(Value::Str(self.local.as_str().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "localAddr" | "addr" => Ok(Value::Str(self.local.as_str().into())),
            "close" => {
                self.sock = None;
                #[cfg(target_os = "linux")]
                {
                    self.raw = None;
                }
                Ok(Value::Nil)
            }
            "setTimeout" => {
                max_args(args, 1, "setTimeout")?;
                self.timeout = opt_timeout(args.first(), "setTimeout")?;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on UdpSocket (sendTo recvFrom connect send recv close setTimeout)", other)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if !matches!(method, "sendTo" | "send" | "recvFrom" | "recv" | "connect") {
            return None;
        }
        let timeout = self.timeout;
        #[cfg(target_os = "linux")]
        if let Some(raw) = self.raw.clone() {
            match udp_direct(&raw, method, args, timeout) {
                Ok(Some(p)) => return Some(Ok(p)),
                Err(e) => return Some(Err(e)),
                // a host name to look up: through the I/O threads, on a
                // duplicate of the socket that lasts as long as the operation
                Ok(None) => {
                    let _runtime = rt::tokio().enter();
                    match raw.0.try_clone().and_then(tokio::net::UdpSocket::from_std) {
                        Ok(s) => self.sock = Some(Arc::new(s)),
                        Err(e) => return Some(Err(io_err!("{}: {}", method, e))),
                    }
                }
            }
        }
        let Some(s) = self.sock.clone() else { return closed(method) };
        #[cfg(target_os = "linux")]
        if self.raw.is_some() {
            self.sock = None;
        }
        let r: Result<Poller, String> = (|| {
            Ok(match method {
                "sendTo" => {
                    max_args(args, 2, "sendTo")?;
                    let addr = str_arg(args, 0, "sendTo(addr, data)")?;
                    let data = bytes_of(args.get(1).ok_or("sendTo(addr, data) expects 2 arguments")?, "sendTo")?;
                    op("sendTo", timeout, async move { s.send_to(&data, addr).await }, |n| Value::Int(n as i64))
                }
                "send" => {
                    max_args(args, 1, "send")?;
                    let data = bytes_of(args.first().ok_or("send(data) expects 1 argument")?, "send")?;
                    op("send", timeout, async move { s.send(&data).await }, |n| Value::Int(n as i64))
                }
                "connect" => {
                    max_args(args, 1, "connect")?;
                    let addr = str_arg(args, 0, "connect(addr)")?;
                    op("connect", timeout, async move { s.connect(addr).await }, |()| Value::Nil)
                }
                "recv" => {
                    let n = size_arg(args, "recv", 65536)?;
                    op("recv", timeout, async move {
                        let mut buf = vec![0u8; n];
                        let got = s.recv(&mut buf).await?;
                        buf.truncate(got);
                        Ok(buf)
                    }, Value::buffer)
                }
                // recvFrom(n = 65536) → UdpPacket { data, from, len }
                _ => {
                    let n = size_arg(args, "recvFrom", 65536)?;
                    op("recvFrom", timeout, async move {
                        let mut buf = vec![0u8; n];
                        let (got, from) = s.recv_from(&mut buf).await?;
                        buf.truncate(got);
                        Ok((buf, from.to_string()))
                    }, |(data, from)| NativeObjectRef::new(UdpPacketValue { data, from }).to_value())
                }
            })
        })();
        Some(r)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["sendTo", "recvFrom", "connect", "send", "recv", "close", "setTimeout", "localAddr"]
    }
}

// ================= UDP PACKET =================

pub struct UdpPacketValue {
    data: Vec<u8>,
    from: String,
}

impl FoguObject for UdpPacketValue {
    fn name(&self) -> &str {
        "UdpPacket"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "data" => Some(Value::buffer(self.data.clone())),
            "text" => Some(Value::Str(String::from_utf8_lossy(&self.data).into())),
            "from" | "fromAddr" | "addr" => Some(Value::Str(self.from.as_str().into())),
            "len" => Some(Value::Int(self.data.len() as i64)),
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        Vec::new()
    }
}
