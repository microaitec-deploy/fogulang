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
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
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

/// `tcpListen(addr)` → TcpListener
fn tcp_listen(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "tcpListen")?;
    let addr = str_arg(args, 0, "tcpListen(addr)")?;
    let a = addr.clone();
    let listener = rt::block_on(async move { tokio::net::TcpListener::bind(a).await })?
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
fn opt_timeout(v: Option<&Value>, what: &str) -> Result<Option<Duration>, String> {
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
fn op<T, F>(what: &'static str, timeout: Option<Duration>, fut: F, finish: impl FnOnce(T) -> Value + 'static) -> Poller
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
    let mut finish = Some(finish);
    Box::new(move || {
        Some(match pending.try_take()? {
            Ok(v) => Ok(finish.take().expect("polled after completion")(v)),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Err(io_err!("{}: timed out", what)),
            Err(e) => Err(io_err!("{}: {}", what, e)),
        })
    })
}

fn closed(what: &str) -> Option<Result<Poller, String>> {
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

pub struct TcpStreamValue {
    reader: Option<Arc<AsyncMutex<BufReader<OwnedReadHalf>>>>,
    writer: Option<Arc<AsyncMutex<OwnedWriteHalf>>>,
    peer: String,
    local: String,
    timeout: Option<Duration>,
}

impl TcpStreamValue {
    fn wrap(s: tokio::net::TcpStream) -> Value {
        s.set_nodelay(true).ok();
        let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
        let local = s.local_addr().map(|a| a.to_string()).unwrap_or_default();
        let (r, w) = s.into_split();
        NativeObjectRef::new(TcpStreamValue {
            reader: Some(Arc::new(AsyncMutex::new(BufReader::with_capacity(16 * 1024, r)))),
            writer: Some(Arc::new(AsyncMutex::new(w))),
            peer,
            local,
            timeout: None,
        })
        .to_value()
    }
}

impl FoguObject for TcpStreamValue {
    fn name(&self) -> &str {
        "TcpStream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "peer" | "peerAddr" => Some(Value::Str(self.peer.as_str().into())),
            "localAddr" => Some(Value::Str(self.local.as_str().into())),
            "open" => Some(Value::Bool(self.writer.is_some())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "peer" | "peerAddr" => Ok(Value::Str(self.peer.as_str().into())),
            "close" => {
                self.reader = None;
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
                "method '{}' does not exist on TcpStream (read readExact readLine readText readAll write writeLine close setTimeout)",
                other
            )),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let timeout = self.timeout;
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
}

// ================= UDP SOCKET =================

pub struct UdpSocketValue {
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
        let Some(s) = self.sock.clone() else { return closed(method) };
        let timeout = self.timeout;
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
