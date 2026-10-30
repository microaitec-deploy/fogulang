//! `fogu:websocket` module — WebSocket client (`ws://`, `wss://`) and a
//! standalone server. `fogu:web` uses the same connection type for
//! `app.ws(path, handler)`.
//!
//! ```fogu
//! import websocket from "fogu:websocket"
//!
//! const ws = websocket.connect("wss://echo.example.com/socket")?
//! ws.send("hello")                                 // str → text, buffer / [bytes] → binary, map → JSON text
//! const msg = ws.next(5s)?                         // { type: "text", data } · nil on timeout or close
//! ws.close()
//!
//! const server = websocket.listen(8080)?
//! fg fn() {
//!     while true {
//!         const conn = server.accept()?
//!         fg fn() { while true { const m = conn.next()?; if m == nil { break }; conn.send(m.data) } }()
//!     }
//! }()
//! ```
//!
//! Every connection is a task on the shared tokio runtime (no thread per
//! connection); `next` and `accept` wait cooperatively and are woken by the
//! runtime when a message or a client arrives (no polling). Network failures
//! are `error` values; a closed connection makes `next` return nil (check
//! `conn.open`, `conn.closeCode`).

use std::cell::RefCell;
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_tungstenite::WebSocketStream;

use crate::runtime::sched::{self, IoGuard};
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;
use crate::native::util::{bytes_of, duration_arg, map, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    e.insert("listen".into(), nfn_var("listen", 1, listen));
    ModuleValue {
        name: "fogu:websocket".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Async connections (fogu:websocket, fogu:web)
// ---------------------------------------------------------------------

/// Any byte stream a WebSocket can run on: a TCP socket, TLS, or an HTTP
/// connection upgraded by the `fogu:web` server.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub type WsIo = Box<dyn Io>;

#[derive(Debug)]
enum Incoming {
    Text(Bytes),
    Binary(Bytes),
}

#[derive(Default)]
struct State {
    inbox: VecDeque<Incoming>,
    open: bool,
    close_code: Option<u16>,
    close_reason: String,
    error: Option<String>,
}

enum Out {
    Msg(Message),
    Close(u16, String),
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// The task of a connection: sends what the program queued, reads what
/// arrives; wakes the scheduler whenever there is something for the program.
async fn run_conn<S: Io + 'static>(ws: WebSocketStream<S>, mut rx: UnboundedReceiver<Out>, state: Arc<Mutex<State>>) {
    let (mut sink, mut stream) = ws.split();
    let finish = |code: Option<u16>, reason: String, error: Option<String>| {
        let mut s = lock(&state);
        s.open = false;
        if let Some(c) = code {
            s.close_code.get_or_insert(c);
            s.close_reason = reason;
        }
        if s.close_code.is_none() {
            if let Some(e) = error {
                s.error.get_or_insert(e);
            }
        }
        drop(s);
        sched::wake();
    };
    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(Out::Msg(m)) => {
                    // queue everything already waiting, then flush once
                    let mut r = sink.feed(m).await;
                    while r.is_ok() {
                        match rx.try_recv() {
                            Ok(Out::Msg(m)) => r = sink.feed(m).await,
                            Ok(Out::Close(code, reason)) => {
                                let _ = sink.flush().await;
                                close(&mut sink, &mut stream, code, reason).await;
                                return finish(Some(code), String::new(), None);
                            }
                            Err(_) => break,
                        }
                    }
                    if let Err(e) = r.and(sink.flush().await) {
                        return finish(None, String::new(), Some(e.to_string()));
                    }
                }
                Some(Out::Close(code, reason)) => {
                    close(&mut sink, &mut stream, code, reason).await;
                    return finish(Some(code), String::new(), None);
                }
                // the program dropped the connection
                None => {
                    let _ = sink.close().await;
                    return finish(Some(1000), String::new(), None);
                }
            },
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(t))) => push(&state, Incoming::Text(t.into())),
                Some(Ok(Message::Binary(b))) => push(&state, Incoming::Binary(b)),
                Some(Ok(Message::Close(frame))) => {
                    let code = frame.as_ref().map_or(1005, |f| u16::from(f.code));
                    let reason = frame.map(|f| f.reason.to_string()).unwrap_or_default();
                    let _ = sink.flush().await;
                    return finish(Some(code), reason, None);
                }
                Some(Ok(_)) => {} // ping/pong: answered by tungstenite on the next flush
                Some(Err(e)) => return finish(Some(1006), String::new(), Some(e.to_string())),
                None => return finish(Some(1006), String::new(), None),
            },
        }
    }
}

fn push(state: &Mutex<State>, m: Incoming) {
    lock(state).inbox.push_back(m);
    sched::wake();
}

/// Sends a close frame and waits (briefly) for the peer's.
async fn close<S, R>(sink: &mut S, stream: &mut R, code: u16, reason: String)
where
    S: futures_util::Sink<Message> + Unpin,
    R: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let frame = CloseFrame { code: CloseCode::from(code), reason: reason.into() };
    let _ = sink.send(Message::Close(Some(frame))).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(m)) = stream.next().await {
            if m.is_close() {
                break;
            }
        }
    })
    .await;
}

pub struct ConnValue {
    tx: UnboundedSender<Out>,
    state: Arc<Mutex<State>>,
    url: String,
    path: String,
    headers: Vec<(String, String)>,
    protocol: Option<String>,
}

fn config() -> WebSocketConfig {
    // tungstenite zero-fills `read_buffer_size` bytes before every socket read
    // (128 KiB by default): with small messages that was most of the CPU time.
    // Large messages still work, through more reads.
    WebSocketConfig::default()
        .max_message_size(Some(64 << 20))
        .max_frame_size(Some(16 << 20))
        .read_buffer_size(8 * 1024)
}

/// Starts the task of an open WebSocket.
fn conn_value<S: Io + 'static>(ws: WebSocketStream<S>, url: String, path: String, headers: Vec<(String, String)>, protocol: Option<String>) -> Value {
    let state = Arc::new(Mutex::new(State { open: true, ..Default::default() }));
    let (tx, rx) = unbounded_channel();
    rt::tokio().spawn(run_conn(ws, rx, Arc::clone(&state)));
    NativeObjectRef::new(ConnValue { tx, state, url, path, headers, protocol }).to_value()
}

/// A server connection on a stream whose HTTP upgrade was already done
/// (`fogu:web` routes).
pub fn server_conn(io: WsIo, url: String, path: String, headers: Vec<(String, String)>, protocol: Option<String>) -> Value {
    let ws = {
        let _rt = rt::tokio().enter();
        futures_lite::future::block_on(WebSocketStream::from_raw_socket(io, Role::Server, Some(config())))
    };
    conn_value(ws, url, path, headers, protocol)
}

fn outgoing(v: Option<&Value>) -> Message {
    match v {
        None | Some(Value::Nil) => Message::text(""),
        Some(Value::Str(s)) => Message::text(s.as_str()),
        Some(Value::Buffer(b)) => Message::binary(b.borrow().clone()),
        Some(l @ Value::List(_)) => match bytes_of(l, "send") {
            Ok(b) => Message::binary(b),
            Err(_) => Message::text(stringify_value(l)),
        },
        Some(other) => Message::text(stringify_value(other)),
    }
}

fn incoming_value(m: Incoming) -> Value {
    match m {
        Incoming::Text(t) => NativeObjectRef::new(MsgValue { binary: false, data: t }).to_value(),
        Incoming::Binary(b) => NativeObjectRef::new(MsgValue { binary: true, data: b }).to_value(),
    }
}

/// Holds the scheduler while a `next`/`accept` waits: the runtime wakes it
/// when something arrives, so waiting is not a deadlock and costs no polling.
/// With a deadline, a timer ends the wait.
fn wait_poller(deadline: Option<Instant>, mut ready: impl FnMut() -> Option<Result<Value, String>> + 'static) -> Poller {
    let mut guard = Some(IoGuard::new());
    Box::new(move || {
        if let Some(r) = ready() {
            guard.take();
            return Some(r);
        }
        match deadline {
            Some(d) if Instant::now() >= d => {
                guard.take();
                Some(Ok(Value::Nil))
            }
            Some(d) => {
                sched::add_timer(d);
                None
            }
            None => None,
        }
    })
}

impl FoguObject for ConnValue {
    fn name(&self) -> &str {
        "WebSocket"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let s = lock(&self.state);
        Some(match field {
            "open" => Value::Bool(s.open),
            "url" => Value::Str(self.url.as_str().into()),
            "path" => Value::Str(self.path.as_str().into()),
            "protocol" => self.protocol.as_deref().map_or(Value::Nil, |p| Value::Str(p.into())),
            "headers" => map(self.headers.iter().map(|(k, v)| (k.as_str(), Value::Str(v.as_str().into()))).collect()),
            "closeCode" => s.close_code.map_or(Value::Nil, |c| Value::Int(c as i64)),
            "closeReason" => Value::Str(s.close_reason.as_str().into()),
            "pending" => Value::Int(s.inbox.len() as i64),
            "error" => s.error.as_deref().map_or(Value::Nil, |e| Value::Str(e.into())),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let check_open = |s: &Self| -> Result<(), String> {
            let st = lock(&s.state);
            if !st.open {
                return Err(io_err!(
                    "websocket: the connection is closed{}",
                    st.error.as_ref().map(|e| format!(" ({})", e)).unwrap_or_default()
                ));
            }
            Ok(())
        };
        match method {
            // send(data): str → text frame, buffer / list of byte ints → binary, other → JSON text
            "send" => {
                max_args(args, 1, "send")?;
                check_open(self)?;
                self.tx.send(Out::Msg(outgoing(args.first()))).map_err(|_| io_err!("websocket: the connection is closed"))?;
                Ok(Value::Nil)
            }
            "ping" => {
                max_args(args, 0, "ping")?;
                check_open(self)?;
                let _ = self.tx.send(Out::Msg(Message::Ping(Bytes::new())));
                Ok(Value::Nil)
            }
            "tryNext" => {
                max_args(args, 0, "tryNext")?;
                Ok(lock(&self.state).inbox.pop_front().map_or(Value::Nil, incoming_value))
            }
            // close(code = 1000, reason = "")
            "close" => {
                max_args(args, 2, "close")?;
                let code = match args.first() {
                    Some(Value::Int(c)) if (1000..5000).contains(c) => *c as u16,
                    None | Some(Value::Nil) => 1000,
                    Some(other) => return Err(format!("close: code must be 1000..4999, got {}", other.to_display_string())),
                };
                let reason = match args.get(1) {
                    Some(Value::Str(r)) => r.to_string(),
                    _ => String::new(),
                };
                let _ = self.tx.send(Out::Close(code, reason));
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on WebSocket (send next tryNext ping close)", other)),
        }
    }

    /// `next(timeout?)` → `{ type, data }`, or nil on timeout / when closed.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        let deadline = match (max_args(args, 1, "next"), args.first()) {
            (Err(e), _) => return Some(Err(e)),
            (_, None | Some(Value::Nil)) => None,
            (_, Some(v)) => match duration_arg(v, "next") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
        };
        let state = Arc::clone(&self.state);
        Some(Ok(wait_poller(deadline, move || {
            let mut s = lock(&state);
            if let Some(m) = s.inbox.pop_front() {
                return Some(Ok(incoming_value(m)));
            }
            if !s.open {
                return Some(match &s.error {
                    Some(e) => Err(io_err!("websocket: {}", e)),
                    None => Ok(Value::Nil),
                });
            }
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["send", "next", "tryNext", "ping", "close"]
    }
}

pub struct MsgValue {
    binary: bool,
    data: Bytes,
}

impl FoguObject for MsgValue {
    fn name(&self) -> &str {
        "WebSocketMessage"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "type" => Value::Str(if self.binary { "binary" } else { "text" }.into()),
            // text frames: str; binary frames: buffer
            "data" if self.binary => Value::buffer(self.data.to_vec()),
            "data" | "text" => Value::Str(String::from_utf8_lossy(&self.data).into()),
            "bytes" => Value::buffer(self.data.to_vec()),
            "size" => Value::Int(self.data.len() as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "json" => parse_value(&String::from_utf8_lossy(&self.data)),
            other => Err(format!("method '{}' does not exist on WebSocketMessage (json)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["json"]
    }
}

// ---------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------

/// `connect(url, { headers, protocols, timeout })`
fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "websocket.connect")?;
    let url = str_arg(args, 0, "websocket.connect")?;
    let o: HashMap<String, Value> = match args.get(1) {
        None | Some(Value::Nil) => HashMap::new(),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(other) => return Err(format!("websocket.connect: options must be a map, got '{}'", other.type_name())),
    };
    let timeout = match o.get("timeout") {
        None | Some(Value::Nil) => Duration::from_secs(10),
        Some(v) => duration_arg(v, "websocket.connect: timeout")?,
    };
    let headers: Vec<(String, String)> = match o.get("headers") {
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect(),
        _ => Vec::new(),
    };
    let protocols = match o.get("protocols") {
        Some(Value::List(p)) => Some(p.borrow().iter().map(|v| v.to_display_string()).collect::<Vec<_>>().join(", ")),
        _ => None,
    };
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = url.as_str().into_client_request().map_err(|e| format!("websocket.connect: {}", e))?;
    for (k, v) in &headers {
        let name = tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(k.as_bytes()).map_err(|_| format!("websocket.connect: invalid header '{}'", k))?;
        let value = tokio_tungstenite::tungstenite::http::HeaderValue::from_str(v).map_err(|_| format!("websocket.connect: invalid value for '{}'", k))?;
        req.headers_mut().insert(name, value);
    }
    if let Some(p) = &protocols {
        req.headers_mut().insert("Sec-WebSocket-Protocol", p.parse().map_err(|_| "websocket.connect: invalid protocols")?);
    }
    let path = req.uri().path_and_query().map(|p| p.to_string()).unwrap_or_else(|| "/".into());
    let u = url.clone();
    let dialed = rt::block_on(async move {
        tokio::time::timeout(timeout, tokio_tungstenite::connect_async_with_config(req, Some(config()), true)).await
    })?;
    let (ws, resp) = match dialed {
        Err(_) => return Err(io_err!("websocket: timed out connecting to {}", u)),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(r))) => return Err(io_err!("websocket: handshake rejected ({})", r.status())),
        Ok(Err(e)) => return Err(io_err!("websocket: cannot connect to {}: {}", u, e)),
        Ok(Ok(c)) => c,
    };
    let protocol = resp.headers().get("sec-websocket-protocol").and_then(|v| v.to_str().ok()).map(String::from);
    let resp_headers = resp.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
    Ok(conn_value(ws, url, path, resp_headers, protocol))
}

// ---------------------------------------------------------------------
// Standalone server
// ---------------------------------------------------------------------

type AcceptedConn = (WebSocketStream<tokio::net::TcpStream>, String, Vec<(String, String)>);

#[derive(Default)]
struct Accepted {
    queue: VecDeque<AcceptedConn>,
    error: Option<String>,
}

/// `listen(port, { host = "127.0.0.1" })` → Server
fn listen(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "websocket.listen")?;
    let port = match args.first() {
        Some(Value::Int(p)) if (0..=65535).contains(p) => *p as u16,
        _ => return Err("websocket.listen expects a port (0..65535)".into()),
    };
    let host = match args.get(1) {
        Some(Value::Map(m)) => match m.borrow().get("host") {
            Some(Value::Str(h)) => h.to_string(),
            _ => "127.0.0.1".into(),
        },
        _ => "127.0.0.1".into(),
    };
    let h = host.clone();
    let listener = rt::block_on(async move { tokio::net::TcpListener::bind((h.as_str(), port)).await })?
        .map_err(|e| io_err!("websocket: cannot listen on {}:{}: {}", host, port, e))?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let accepted = Arc::new(Mutex::new(Accepted::default()));
    let acc = Arc::clone(&accepted);
    rt::tokio().spawn(async move {
        loop {
            let tcp = match listener.accept().await {
                Ok((tcp, _)) => tcp,
                Err(e) => {
                    acc.lock().unwrap_or_else(|e| e.into_inner()).error = Some(e.to_string());
                    sched::wake();
                    return;
                }
            };
            tcp.set_nodelay(true).ok();
            let acc = Arc::clone(&acc);
            // the handshake of one client must not hold the others
            tokio::spawn(async move {
                let mut path = String::new();
                let mut headers = Vec::new();
                let cb = |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
                    path = req.uri().to_string();
                    headers = req.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
                    Ok(resp)
                };
                let hs = tokio_tungstenite::accept_hdr_async_with_config(tcp, cb, Some(config()));
                if let Ok(Ok(ws)) = tokio::time::timeout(Duration::from_secs(10), hs).await {
                    acc.lock().unwrap_or_else(|e| e.into_inner()).queue.push_back((ws, path, headers));
                    sched::wake();
                }
            });
        }
    });
    Ok(NativeObjectRef::new(ServerValue { accepted, port, host }).to_value())
}

pub struct ServerValue {
    accepted: Arc<Mutex<Accepted>>,
    port: u16,
    host: String,
}

impl FoguObject for ServerValue {
    fn name(&self) -> &str {
        "WebSocketServer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "port" => Value::Int(self.port as i64),
            "url" => Value::Str(format!("ws://{}:{}", self.host, self.port).into()),
            _ => return None,
        })
    }

    /// `accept(timeout?)` → WebSocket, nil on timeout.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "accept" {
            return None;
        }
        let deadline = match (max_args(args, 1, "accept"), args.first()) {
            (Err(e), _) => return Some(Err(e)),
            (_, None | Some(Value::Nil)) => None,
            (_, Some(v)) => match duration_arg(v, "accept") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
        };
        let accepted = Arc::clone(&self.accepted);
        let base = format!("ws://{}:{}", self.host, self.port);
        Some(Ok(wait_poller(deadline, move || {
            let mut a = accepted.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((ws, path, headers)) = a.queue.pop_front() {
                let url = format!("{}{}", base, path);
                let protocol = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-protocol")).map(|(_, v)| v.clone());
                return Some(Ok(conn_value(ws, url, path, headers, protocol)));
            }
            a.error.as_ref().map(|e| Err(io_err!("websocket: {}", e)))
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["accept"]
    }
}
