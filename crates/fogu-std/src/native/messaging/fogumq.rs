//! `fogu:fogumq` module — native client for the Fogu broker: key/value
//! store, channels, pub/sub, RPC and API keys.
//!
//! ```fogu
//! import fogumq from "fogu:fogumq"
//!
//! const mq = fogumq.connect("fogu://token@host:7222")?
//! mq.kv.set("visits", 0)?
//! print(mq.kv.incr("visits")?)                    // 1
//!
//! const room = mq.channel("chat")
//! room.join("ana")?
//! const inbox = room.onMessage()                  // channel messages
//! room.send({ text: "hi" })?
//! print(inbox.next(2s))                           // nil after 2s without messages
//!
//! const sub = room.subscribe("news")?             // topic "chat/news"
//! room.publish("news", { id: 1 })?
//! print(sub.next(2s))
//!
//! const svc = room.act("add")?                    // RPC server side
//! fg fn() { const r = svc.next(); r.reply(r.params.a + r.params.b) }()
//! print(room.call("add", { a: 1, b: 2 })?)        // 3
//! ```
//!
//! Frames are read by a background thread; methods that wait for the server
//! wait cooperatively, so other `fg` tasks keep running — a task can answer
//! the calls another one makes. Network and server failures are `error`
//! values.
//!
//! Frame: `'F' 'O' version opcode flags request_id:u64 len:u32 payload`
//! (big endian, 17-byte header). Payloads are JSON; replies start with a
//! status byte.

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use ahash::AHashMap as HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, json_to_value, list, max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    ModuleValue {
        name: "fogu:fogumq".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------

const OP_PING: u8 = 0x01;
const OP_PONG: u8 = 0x02;
const OP_AUTH: u8 = 0x50;
const OP_ERROR: u8 = 0xFF;
const OP_GET: u8 = 0x10;
const OP_SET: u8 = 0x11;
const OP_DEL: u8 = 0x12;
const OP_EXISTS: u8 = 0x13;
const OP_INCR: u8 = 0x20;
const OP_DECR: u8 = 0x21;
const OP_JOIN: u8 = 0x30;
const OP_LEAVE: u8 = 0x31;
const OP_CHANNEL_MSG: u8 = 0x32;
const OP_CHANNEL_LIST: u8 = 0x33;
const OP_MEMBERS: u8 = 0x34;
const OP_SUBSCRIBE: u8 = 0x40;
const OP_PUBLISH: u8 = 0x41;
const OP_CALL: u8 = 0x60;
const OP_ACT: u8 = 0x61;
const OP_RES: u8 = 0x62;
const OP_APIKEY_CREATE: u8 = 0x70;
const OP_APIKEY_DELETE: u8 = 0x71;
const OP_APIKEY_LIST: u8 = 0x72;
const OP_APIKEY_GET: u8 = 0x73;

const ST_OK: u8 = 0x00;
const ST_NOT_FOUND: u8 = 0x01;
const ST_FORBIDDEN: u8 = 0x05;

const HEADER_SIZE: usize = 17;
const MAGIC: [u8; 2] = [0x46, 0x4F];
const VERSION: u8 = 0x01;
const MAX_PAYLOAD: usize = 64 << 20;

fn encode_frame(opcode: u8, request_id: u64, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(HEADER_SIZE + payload.len());
    b.extend_from_slice(&MAGIC);
    b.push(VERSION);
    b.push(opcode);
    b.push(0); // flags
    b.extend_from_slice(&request_id.to_be_bytes());
    b.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    b.extend_from_slice(payload);
    b
}

fn status_name(s: u8) -> &'static str {
    match s {
        0x00 => "ok",
        0x01 => "not found",
        0x02 => "error",
        0x03 => "invalid",
        0x04 => "unauthorized",
        ST_FORBIDDEN => "forbidden",
        _ => "unknown",
    }
}

/// `{"error": "..."}` → its message; anything else as text.
fn error_text(body: &[u8]) -> String {
    let raw = String::from_utf8_lossy(body).into_owned();
    serde_json::from_str::<Json>(&raw)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or(raw)
}

// ---------------------------------------------------------------------
// URL: fogu://[user:]token@host:port
// ---------------------------------------------------------------------

#[derive(Debug)]
struct Url {
    token: String,
    host: String,
    port: u16,
}

fn parse_url(raw: &str) -> Result<Url, String> {
    let bad = || format!("fogumq.connect: invalid url '{}' (expected fogu://token@host:port)", raw);
    let rest = raw.strip_prefix("fogu://").ok_or_else(bad)?;
    let (cred, host_port) = rest.rsplit_once('@').ok_or_else(bad)?;
    // an optional `user:` before the token is ignored
    let token = match cred.split_once(':') {
        Some((user, tok)) if !user.is_empty() && !user.contains('@') => tok,
        _ => cred,
    };
    let (host, port) = host_port.rsplit_once(':').ok_or_else(bad)?;
    if token.is_empty() || host.is_empty() || host.contains(':') {
        return Err(bad());
    }
    let port = port.parse().map_err(|_| format!("fogumq.connect: invalid port '{}'", port))?;
    Ok(Url { token: crate::native::util::url_decode(token), host: host.to_string(), port })
}

// ---------------------------------------------------------------------
// Connection (shared with the reader thread)
// ---------------------------------------------------------------------

/// A reply: `(status, body)` or `(message, status)`.
type Reply = Result<(u8, Json), (String, Option<u8>)>;

/// What a registration receives.
enum Item {
    Msg(Json),
    Call { id: u64, method: String, params: Json },
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// `channel.onMessage()`: messages of a channel.
    Listener,
    /// `channel.subscribe(topic)`: publications of a topic.
    Subscription,
    /// `channel.act(method)`: RPC calls for a method.
    Service,
}

struct Registration {
    kind: Kind,
    key: String,
    queue: VecDeque<Item>,
}

#[derive(Default)]
struct Shared {
    connected: bool,
    authenticated: bool,
    is_root: bool,
    /// Why the connection ended.
    closed: Option<String>,
    /// Requests waiting for an answer, and the answers that arrived.
    waiting: HashSet<u64>,
    replies: HashMap<u64, Reply>,
    regs: HashMap<u32, Registration>,
    next_reg: u32,
    /// RPC method → the registration serving it.
    actions: HashMap<String, u32>,
}

impl Shared {
    fn reply(&mut self, id: u64, r: Reply) {
        if self.waiting.contains(&id) {
            self.replies.insert(id, r);
        }
    }

    fn push(&mut self, kind: Kind, key: Option<&str>, data: &Json) {
        for r in self.regs.values_mut() {
            if r.kind == kind && key.is_none_or(|k| k == r.key) {
                r.queue.push_back(Item::Msg(data.clone()));
            }
        }
    }

    fn register(&mut self, kind: Kind, key: &str) -> u32 {
        self.next_reg += 1;
        let id = self.next_reg;
        self.regs.insert(id, Registration { kind, key: key.to_string(), queue: VecDeque::new() });
        if kind == Kind::Service {
            if let Some(old) = self.actions.insert(key.to_string(), id) {
                self.regs.remove(&old);
            }
        }
        id
    }

    fn unregister(&mut self, id: u32) {
        if let Some(r) = self.regs.remove(&id) {
            if r.kind == Kind::Service && self.actions.get(&r.key) == Some(&id) {
                self.actions.remove(&r.key);
            }
        }
    }
}

/// Where frames are written: a TCP socket, or the in-memory pipe of a
/// `fogu:mq` inline client.
type Writer = Arc<Mutex<Box<dyn Write + Send>>>;
/// Closes the transport (unblocks the reader thread).
type Shutdown_ = Arc<dyn Fn() + Send + Sync>;

struct Conn {
    shared: Arc<Mutex<Shared>>,
    writer: Writer,
    shutdown: Shutdown_,
    next_id: Cell<u64>,
    timeout: Duration,
    url: String,
}

fn lock(s: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

fn write_to(w: &Writer, frame: &[u8]) -> std::io::Result<()> {
    w.lock().unwrap_or_else(|p| p.into_inner()).write_all(frame)
}

fn closed_err(reason: &Option<String>) -> String {
    io_err!("fogumq: not connected{}", reason.as_ref().map(|r| format!(" ({})", r)).unwrap_or_default())
}

impl Conn {
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    fn next_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1).max(1));
        id
    }

    fn require_auth(&self) -> Result<(), String> {
        let s = self.lock();
        if !s.connected {
            return Err(closed_err(&s.closed));
        }
        if !s.authenticated {
            return Err(io_err!("fogumq: not authenticated"));
        }
        Ok(())
    }

    fn require_root(&self) -> Result<(), String> {
        self.require_auth()?;
        if !self.lock().is_root {
            return Err(io_err!("fogumq: root-only operation"));
        }
        Ok(())
    }

    fn write(&self, opcode: u8, id: u64, payload: &[u8]) -> Result<(), String> {
        {
            let s = self.lock();
            if !s.connected {
                return Err(closed_err(&s.closed));
            }
        }
        write_to(&self.writer, &encode_frame(opcode, id, payload)).map_err(|e| io_err!("fogumq: write failed: {}", e))
    }

    /// Sends a frame that expects an answer; returns its request id.
    fn send(&self, opcode: u8, payload: Option<&Json>) -> Result<u64, String> {
        let id = self.next_id();
        let body = payload.map(|v| v.to_string().into_bytes()).unwrap_or_default();
        self.lock().waiting.insert(id);
        if let Err(e) = self.write(opcode, id, &body) {
            self.lock().waiting.remove(&id);
            return Err(e);
        }
        Ok(id)
    }

    /// The answer to a request, if it arrived (or the connection ended).
    fn take_reply(&self, id: u64) -> Option<Result<(u8, Json), String>> {
        let mut s = self.lock();
        if let Some(r) = s.replies.remove(&id) {
            s.waiting.remove(&id);
            return Some(r.map_err(|(msg, status)| match status {
                Some(st) => io_err!("fogumq: server error: {} (status {})", msg, status_name(st)),
                None => io_err!("fogumq: server error: {}", msg),
            }));
        }
        if !s.connected {
            s.waiting.remove(&id);
            return Some(Err(closed_err(&s.closed)));
        }
        None
    }

    fn forget(&self, id: u64) {
        let mut s = self.lock();
        s.waiting.remove(&id);
        s.replies.remove(&id);
    }

    fn close(&self) {
        let was = {
            let mut s = self.lock();
            let was = s.connected;
            s.connected = false;
            s.authenticated = false;
            s.is_root = false;
            s.closed.get_or_insert("closed by the client".into());
            was
        };
        if was {
            (self.shutdown)();
        }
    }
}

/// Keeps the scheduler polling while a network operation is pending.
fn poll_again() {
    sched::add_timer(Instant::now() + Duration::from_millis(2));
}

/// Sends one request and waits cooperatively for the answer; `finish`
/// turns the answer's body into the result.
fn request(
    conn: Rc<Conn>,
    opcode: u8,
    payload: Option<Json>,
    timeout: Duration,
    finish: impl FnOnce(u8, Json) -> Result<Value, String> + 'static,
) -> Result<Poller, String> {
    conn.require_auth()?;
    let id = conn.send(opcode, payload.as_ref())?;
    let deadline = Instant::now() + timeout;
    let mut finish = Some(finish);
    Ok(Box::new(move || match conn.take_reply(id) {
        Some(Ok((status, body))) => Some((finish.take().expect("polled after completion"))(status, body)),
        Some(Err(e)) => Some(Err(e)),
        None if Instant::now() >= deadline => {
            conn.forget(id);
            Some(Err(io_err!("fogumq: timeout after {:?}", timeout)))
        }
        None => {
            poll_again();
            None
        }
    }))
}

/// Reader thread: splits the stream into frames and dispatches them.
fn read_loop(mut stream: Box<dyn Read + Send>, shared: Arc<Mutex<Shared>>, writer: Writer) {
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut chunk = vec![0u8; 64 * 1024];
    let reason = 'read: loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) => break "closed by the server".to_string(),
            Ok(n) => n,
            Err(e) => break e.to_string(),
        };
        buf.extend_from_slice(&chunk[..n]);
        while buf.len() >= HEADER_SIZE {
            if buf[..2] != MAGIC {
                break 'read "bad frame (invalid magic)".to_string();
            }
            let len = u32::from_be_bytes([buf[13], buf[14], buf[15], buf[16]]) as usize;
            if len > MAX_PAYLOAD {
                break 'read format!("bad frame length {}", len);
            }
            if buf.len() < HEADER_SIZE + len {
                break;
            }
            let opcode = buf[3];
            let id = u64::from_be_bytes(buf[5..13].try_into().unwrap());
            let payload: Vec<u8> = buf[HEADER_SIZE..HEADER_SIZE + len].to_vec();
            buf.drain(..HEADER_SIZE + len);
            dispatch(&shared, &writer, opcode, id, &payload);
        }
        if !lock(&shared).connected {
            break "closed".to_string();
        }
    };
    let mut s = lock(&shared);
    s.connected = false;
    s.authenticated = false;
    s.is_root = false;
    s.closed.get_or_insert(reason);
}

fn dispatch(shared: &Mutex<Shared>, writer: &Writer, opcode: u8, id: u64, payload: &[u8]) {
    let mut s = lock(shared);
    let pending = s.waiting.contains(&id);
    match opcode {
        OP_PING => {
            drop(s);
            let _ = write_to(writer, &encode_frame(OP_PONG, id, &[]));
            return;
        }
        OP_PONG => {
            s.reply(id, Ok((ST_OK, Json::Null)));
            return;
        }
        // a push unless it is the status-prefixed ack of our own send
        OP_CHANNEL_MSG if !(pending && payload.first().is_some_and(|b| *b <= ST_FORBIDDEN)) => {
            let envelope: Json = serde_json::from_slice(payload).unwrap_or(Json::Null);
            if let Some(ch) = envelope.get("channel").and_then(|v| v.as_str()) {
                let ch = ch.to_string();
                s.push(Kind::Listener, Some(&ch), &envelope);
            }
            return;
        }
        OP_PUBLISH if !pending => {
            let data: Json = serde_json::from_slice(payload).unwrap_or(Json::Null);
            // routed by its topic when the server says it, else to every subscription
            let topic = data.get("topic").and_then(|t| t.as_str()).map(String::from);
            let known = topic.as_ref().is_some_and(|t| s.regs.values().any(|r| r.kind == Kind::Subscription && &r.key == t));
            s.push(Kind::Subscription, topic.as_deref().filter(|_| known), &data);
            return;
        }
        // always a call to one of our actions (answers to our calls are RES/ERROR)
        OP_CALL => {
            let call: Json = serde_json::from_slice(payload).unwrap_or(Json::Null);
            let method = call.get("method").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let params = call.get("params").cloned().unwrap_or(Json::Null);
            match s.actions.get(&method).copied() {
                Some(reg) => {
                    if let Some(r) = s.regs.get_mut(&reg) {
                        r.queue.push_back(Item::Call { id, method, params });
                    }
                }
                None => {
                    drop(s);
                    let body = json!({ "result": null, "error": format!("handler '{}' not found", method) });
                    let _ = write_to(writer, &encode_frame(OP_RES, id, body.to_string().as_bytes()));
                }
            }
            return;
        }
        OP_RES => {
            let body: Json = serde_json::from_slice(payload).unwrap_or(Json::Null);
            let err = body.get("error").and_then(|v| v.as_str()).filter(|e| !e.is_empty()).map(String::from);
            let r = match err {
                Some(e) => Err((e, None)),
                None => Ok((ST_OK, body.get("result").cloned().unwrap_or(body))),
            };
            s.reply(id, r);
            return;
        }
        OP_ERROR => {
            let status = payload.first().copied();
            let msg = error_text(if payload.len() > 1 { &payload[1..] } else { payload });
            let msg = if msg.is_empty() { "unknown error".to_string() } else { msg };
            s.reply(id, Err((msg, status)));
            return;
        }
        _ => {}
    }
    // an ordinary reply: status byte + JSON body
    if !pending {
        return;
    }
    let Some((&status, body)) = payload.split_first() else {
        s.reply(id, Err(("protocol error: empty payload".into(), None)));
        return;
    };
    let r = match status {
        ST_OK | ST_NOT_FOUND => {
            let v = if body.is_empty() {
                Json::Null
            } else {
                serde_json::from_slice(body).unwrap_or_else(|_| Json::String(String::from_utf8_lossy(body).into_owned()))
            };
            Ok((status, v))
        }
        _ => Err((error_text(body), Some(status))),
    };
    s.reply(id, r);
}

// ---------------------------------------------------------------------
// connect
// ---------------------------------------------------------------------

/// `connect(url, { timeout, requestTimeout }?)`
pub(crate) fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "fogumq.connect")?;
    let url = parse_url(&str_arg(args, 0, "fogumq.connect")?)?;
    let mut connect_timeout = Duration::from_secs(30);
    let mut request_timeout = Duration::from_secs(5);
    match opt_arg(args, 1) {
        None => {}
        Some(Value::Map(m)) => {
            let m = m.borrow();
            if let Some(v) = m.get("timeout") {
                connect_timeout = duration_arg(v, "fogumq.connect: timeout")?;
            }
            if let Some(v) = m.get("requestTimeout") {
                request_timeout = duration_arg(v, "fogumq.connect: requestTimeout")?;
            }
        }
        Some(other) => return Err(format!("fogumq.connect: options must be a map, got '{}'", other.type_name())),
    }

    let target = format!("{}:{}", url.host, url.port);
    let addr = target
        .to_socket_addrs()
        .map_err(|e| io_err!("fogumq: cannot resolve {}: {}", target, e))?
        .next()
        .ok_or_else(|| io_err!("fogumq: cannot resolve {}", target))?;
    let stream = TcpStream::connect_timeout(&addr, connect_timeout)
        .map_err(|e| io_err!("fogumq: cannot connect to {}: {}", target, e))?;
    stream.set_nodelay(true).ok();
    let reader = stream.try_clone().map_err(|e| io_err!("fogumq: {}", e))?;
    let closer = stream.try_clone().map_err(|e| io_err!("fogumq: {}", e))?;
    open(
        Box::new(reader),
        Box::new(stream),
        Arc::new(move || {
            let _ = closer.shutdown(Shutdown::Both);
        }),
        &url.token,
        format!("fogu://{}:{}", url.host, url.port),
        connect_timeout,
        request_timeout,
    )
}

/// A client over any transport: starts the reader thread and authenticates.
pub(crate) fn open(
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    shutdown: Shutdown_,
    token: &str,
    url: String,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<Value, String> {
    let writer: Writer = Arc::new(Mutex::new(writer));
    let shared = Arc::new(Mutex::new(Shared { connected: true, ..Default::default() }));
    {
        let (s, w) = (Arc::clone(&shared), Arc::clone(&writer));
        std::thread::spawn(move || read_loop(reader, s, w));
    }
    let conn = Conn { shared, writer, shutdown, next_id: Cell::new(1), timeout: request_timeout, url };

    // AUTH: waits (blocking) for the answer
    let id = conn.send(OP_AUTH, Some(&json!({ "token": token })))?;
    let deadline = Instant::now() + connect_timeout;
    let auth = loop {
        if let Some(r) = conn.take_reply(id) {
            break r;
        }
        if Instant::now() >= deadline {
            conn.forget(id);
            break Err(io_err!("fogumq: no answer to AUTH after {:?}", connect_timeout));
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    match auth {
        Ok((_, body)) => {
            let mut s = conn.lock();
            s.authenticated = true;
            s.is_root = body.pointer("/result/role").and_then(|v| v.as_str()) == Some("root");
        }
        Err(e) => {
            conn.close();
            return Err(e);
        }
    }
    Ok(NativeObjectRef::new(Client { conn: Rc::new(conn) }).to_value())
}

// ---------------------------------------------------------------------
// Values ↔ JSON
// ---------------------------------------------------------------------

fn to_json(v: &Value) -> Json {
    match v {
        Value::Nil => Json::Null,
        other => serde_json::from_str(&stringify_value(other)).unwrap_or(Json::Null),
    }
}

fn arg_json(args: &[Value], i: usize) -> Json {
    args.get(i).map(to_json).unwrap_or(Json::Null)
}

/// `body.key` as a Fogu value (nil when missing).
fn field(body: &Json, key: &str) -> Value {
    body.get(key).map(json_to_value).unwrap_or(Value::Nil)
}

/// `body.key` when present, else the whole body.
fn field_or_body(body: Json, key: &str) -> Value {
    json_to_value(body.get(key).unwrap_or(&body))
}

fn array_field(body: &Json, key: &str) -> Value {
    match body.get(key) {
        Some(v @ Json::Array(_)) => json_to_value(v),
        _ => list(Vec::new()),
    }
}

/// `start_async` boilerplate: `Ok(None)` means "not an async method".
fn async_result(r: Result<Option<Poller>, String>) -> Option<Result<Poller, String>> {
    match r {
        Ok(Some(p)) => Some(Ok(p)),
        Ok(None) => None,
        Err(e) => Some(Err(e)),
    }
}

// ---------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------

pub struct Client {
    conn: Rc<Conn>,
}

impl FoguObject for Client {
    fn name(&self) -> &str {
        "FoguMQ"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let conn = Rc::clone(&self.conn);
        let s = self.conn.lock();
        Some(match field {
            "connected" => Value::Bool(s.connected),
            "authenticated" => Value::Bool(s.authenticated),
            "isRoot" => Value::Bool(s.is_root),
            "url" => Value::Str(self.conn.url.clone().into()),
            "kv" => NativeObjectRef::new(Kv { conn }).to_value(),
            "apikeys" => NativeObjectRef::new(ApiKeys { conn }).to_value(),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "channel" => {
                max_args(args, 1, "channel")?;
                let name = str_arg(args, 0, "channel")?;
                Ok(NativeObjectRef::new(Channel { conn: Rc::clone(&self.conn), name }).to_value())
            }
            "close" | "disconnect" => {
                max_args(args, 0, method)?;
                self.conn.close();
                Ok(Value::Nil)
            }
            other => Err(format!(
                "method '{}' does not exist on FoguMQ (ping channel close disconnect; fields kv apikeys)",
                other
            )),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "ping" {
            return None;
        }
        if let Err(e) = max_args(args, 0, "ping") {
            return Some(Err(e));
        }
        Some(request(Rc::clone(&self.conn), OP_PING, None, Duration::from_secs(5), |_, _| Ok(Value::Nil)))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["ping", "channel", "close", "disconnect"]
    }
}

// ---------------------------------------------------------------------
// Key/value
// ---------------------------------------------------------------------

struct Kv {
    conn: Rc<Conn>,
}

impl FoguObject for Kv {
    fn name(&self) -> &str {
        "FoguKV"
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let conn = Rc::clone(&self.conn);
        let to = conn.timeout;
        let what = format!("kv.{}", method);
        async_result((|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // get(key) → value or nil
                "get" => {
                    max_args(args, 1, &what)?;
                    let p = json!({ "key": str_arg(args, 0, &what)? });
                    request(conn, OP_GET, Some(p), to, |_, d| Ok(field(&d, "value")))?
                }
                // set(key, value, ttl?) → true
                "set" => {
                    max_args(args, 3, &what)?;
                    let key = str_arg(args, 0, &what)?;
                    let p = match opt_arg(args, 2) {
                        None => json!({ "key": key, "value": arg_json(args, 1) }),
                        Some(Value::Int(t)) if *t >= 0 => json!({ "key": key, "value": arg_json(args, 1), "ttl": t }),
                        Some(other) => return Err(format!("{}: ttl must be an int >= 0, got {}", what, other.to_display_string())),
                    };
                    request(conn, OP_SET, Some(p), to, |_, _| Ok(Value::Bool(true)))?
                }
                // del(key) → bool (whether something was removed)
                "del" => {
                    max_args(args, 1, &what)?;
                    let p = json!({ "key": str_arg(args, 0, &what)? });
                    request(conn, OP_DEL, Some(p), to, |_, d| {
                        Ok(Value::Bool(d.get("count").and_then(|v| v.as_u64()).unwrap_or(0) > 0))
                    })?
                }
                "exists" => {
                    max_args(args, 1, &what)?;
                    let p = json!({ "key": str_arg(args, 0, &what)? });
                    request(conn, OP_EXISTS, Some(p), to, |_, d| {
                        Ok(Value::Bool(d.get("exists").and_then(|v| v.as_bool()).unwrap_or(false)))
                    })?
                }
                // incr(key, delta = 1) / decr(key, delta = 1) → the new value
                "incr" | "decr" => {
                    max_args(args, 2, &what)?;
                    let key = str_arg(args, 0, &what)?;
                    let delta = match opt_arg(args, 1) {
                        None => 1,
                        Some(Value::Int(n)) => *n,
                        Some(other) => return Err(format!("{}: delta must be an int, got '{}'", what, other.type_name())),
                    };
                    let op = if method == "incr" { OP_INCR } else { OP_DECR };
                    request(conn, op, Some(json!({ "key": key, "delta": delta })), to, |_, d| {
                        Ok(Value::Int(d.get("value").and_then(|v| v.as_i64()).unwrap_or(0)))
                    })?
                }
                _ => return Ok(None),
            }))
        })())
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on FoguKV (get set del exists incr decr)", method))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["get", "set", "del", "exists", "incr", "decr"]
    }
}

// ---------------------------------------------------------------------
// API keys (root only)
// ---------------------------------------------------------------------

struct ApiKeys {
    conn: Rc<Conn>,
}

impl FoguObject for ApiKeys {
    fn name(&self) -> &str {
        "FoguApiKeys"
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let conn = Rc::clone(&self.conn);
        let to = conn.timeout;
        let what = format!("apikeys.{}", method);
        async_result((|| -> Result<Option<Poller>, String> {
            if !matches!(method, "create" | "delete" | "list" | "get") {
                return Ok(None);
            }
            conn.require_root()?;
            Ok(Some(match method {
                // create(name, { channels, kv, topics, rpc }?) → the new key
                "create" => {
                    max_args(args, 2, &what)?;
                    let name = str_arg(args, 0, &what)?;
                    let perms = match opt_arg(args, 1) {
                        None => Json::Object(Default::default()),
                        Some(v @ Value::Map(_)) => to_json(v),
                        Some(other) => return Err(format!("{}: permissions must be a map, got '{}'", what, other.type_name())),
                    };
                    let list_of = |k: &str| perms.get(k).cloned().unwrap_or_else(|| json!([]));
                    let p = json!({
                        "name": name, "channels": list_of("channels"),
                        "kv": list_of("kv"), "topics": list_of("topics"), "rpc": list_of("rpc"),
                    });
                    request(conn, OP_APIKEY_CREATE, Some(p), to, |_, d| Ok(field_or_body(d, "api_key")))?
                }
                "delete" => {
                    max_args(args, 1, &what)?;
                    let p = json!({ "id": str_arg(args, 0, &what)? });
                    request(conn, OP_APIKEY_DELETE, Some(p), to, |_, d| Ok(field_or_body(d, "result")))?
                }
                "get" => {
                    max_args(args, 1, &what)?;
                    let p = json!({ "id": str_arg(args, 0, &what)? });
                    request(conn, OP_APIKEY_GET, Some(p), to, |_, d| Ok(field_or_body(d, "api_key")))?
                }
                _ => {
                    max_args(args, 0, &what)?;
                    request(conn, OP_APIKEY_LIST, None, to, |_, d| Ok(array_field(&d, "api_keys")))?
                }
            }))
        })())
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on FoguApiKeys (create delete list get)", method))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["create", "delete", "list", "get"]
    }
}

// ---------------------------------------------------------------------
// Channel: membership, messages, pub/sub and RPC
// ---------------------------------------------------------------------

struct Channel {
    conn: Rc<Conn>,
    name: String,
}

impl Channel {
    /// `name/sub`: topics and RPC methods live under the channel.
    fn ns(&self, sub: &str) -> String {
        format!("{}/{}", self.name, sub)
    }

    fn inbox(&self, kind: Kind, key: &str) -> Inbox {
        let reg = self.conn.lock().register(kind, key);
        Inbox { conn: Rc::clone(&self.conn), reg, kind, key: key.to_string() }
    }
}

impl FoguObject for Channel {
    fn name(&self) -> &str {
        "FoguChannel"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "name" => Some(Value::Str(self.name.clone().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // onMessage() → Listener with the messages sent to this channel
            "onMessage" => {
                max_args(args, 0, "onMessage")?;
                Ok(NativeObjectRef::new(self.inbox(Kind::Listener, &self.name)).to_value())
            }
            other => Err(format!(
                "method '{}' does not exist on FoguChannel (join leave send onMessage members list subscribe publish act call)",
                other
            )),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let conn = Rc::clone(&self.conn);
        let to = conn.timeout;
        let name = self.name.clone();
        async_result((|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // join(alias?)
                "join" => {
                    max_args(args, 1, "join")?;
                    let p = match opt_arg(args, 0) {
                        None => json!({ "channel": name }),
                        Some(_) => json!({ "channel": name, "alias": str_arg(args, 0, "join")? }),
                    };
                    request(conn, OP_JOIN, Some(p), to, |_, d| Ok(field_or_body(d, "result")))?
                }
                "leave" => {
                    max_args(args, 0, "leave")?;
                    request(conn, OP_LEAVE, Some(json!({ "channel": name })), to, |_, d| Ok(field_or_body(d, "result")))?
                }
                // send(data) → true
                "send" => {
                    max_args(args, 1, "send")?;
                    let p = json!({ "channel": name, "data": arg_json(args, 0) });
                    request(conn, OP_CHANNEL_MSG, Some(p), to, |_, _| Ok(Value::Bool(true)))?
                }
                "members" => {
                    max_args(args, 0, "members")?;
                    request(conn, OP_MEMBERS, Some(json!({ "channel": name })), to, |_, d| Ok(array_field(&d, "members")))?
                }
                // list() → every channel on the server
                "list" => {
                    max_args(args, 0, "list")?;
                    request(conn, OP_CHANNEL_LIST, None, to, |_, d| Ok(array_field(&d, "channels")))?
                }
                // subscribe(topic) → Subscription
                "subscribe" | "sub" => {
                    max_args(args, 1, "subscribe")?;
                    let key = self.ns(&str_arg(args, 0, "subscribe")?);
                    let inbox = self.inbox(Kind::Subscription, &key);
                    let reg = inbox.reg;
                    let c2 = Rc::clone(&conn);
                    let mut inbox = Some(inbox);
                    let mut poll = request(conn, OP_SUBSCRIBE, Some(json!({ "topic": key })), to, move |_, _| {
                        Ok(NativeObjectRef::new(inbox.take().unwrap()).to_value())
                    })
                    .inspect_err(|_| c2.lock().unregister(reg))?;
                    Box::new(move || {
                        let r = poll()?;
                        if r.is_err() {
                            c2.lock().unregister(reg);
                        }
                        Some(r)
                    })
                }
                // publish(topic, data) → true
                "publish" | "pub" => {
                    max_args(args, 2, "publish")?;
                    let p = json!({ "topic": self.ns(&str_arg(args, 0, "publish")?), "data": arg_json(args, 1) });
                    request(conn, OP_PUBLISH, Some(p), to, |_, _| Ok(Value::Bool(true)))?
                }
                // act(method) → Service that receives the calls
                "act" => {
                    max_args(args, 1, "act")?;
                    let key = self.ns(&str_arg(args, 0, "act")?);
                    let inbox = self.inbox(Kind::Service, &key);
                    let reg = inbox.reg;
                    let c2 = Rc::clone(&conn);
                    let mut inbox = Some(inbox);
                    let mut poll = request(conn, OP_ACT, Some(json!({ "method": key })), to, move |_, _| {
                        Ok(NativeObjectRef::new(inbox.take().unwrap()).to_value())
                    })
                    .inspect_err(|_| c2.lock().unregister(reg))?;
                    Box::new(move || {
                        let r = poll()?;
                        if r.is_err() {
                            c2.lock().unregister(reg);
                        }
                        Some(r)
                    })
                }
                // call(method, params?, timeout?) → the result
                "call" => {
                    max_args(args, 3, "call")?;
                    let key = self.ns(&str_arg(args, 0, "call")?);
                    let timeout = match opt_arg(args, 2) {
                        Some(v) => duration_arg(v, "call: timeout")?,
                        None => to,
                    };
                    let p = json!({ "method": key, "params": arg_json(args, 1) });
                    request(conn, OP_CALL, Some(p), timeout, |_, d| Ok(json_to_value(&d)))?
                }
                _ => return Ok(None),
            }))
        })())
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["join", "leave", "send", "onMessage", "members", "list", "subscribe", "sub", "publish", "pub", "act", "call"]
    }
}

// ---------------------------------------------------------------------
// Listener / Subscription / Service: a registration with a queue
// ---------------------------------------------------------------------

struct Inbox {
    conn: Rc<Conn>,
    reg: u32,
    kind: Kind,
    key: String,
}

impl Inbox {
    fn type_name(&self) -> &'static str {
        match self.kind {
            Kind::Listener => "Listener",
            Kind::Subscription => "Subscription",
            Kind::Service => "Service",
        }
    }

    fn pop(&self) -> Option<Item> {
        self.conn.lock().regs.get_mut(&self.reg).and_then(|r| r.queue.pop_front())
    }

    fn wrap(&self, item: Item) -> Value {
        match item {
            Item::Msg(v) => json_to_value(&v),
            Item::Call { id, method, params } => {
                NativeObjectRef::new(Request { conn: Rc::clone(&self.conn), id, method, params, answered: false }).to_value()
            }
        }
    }

    fn closed(&self) -> Option<String> {
        let s = self.conn.lock();
        if !s.regs.contains_key(&self.reg) {
            Some(io_err!("fogumq: {} '{}' was closed", self.type_name(), self.key))
        } else if !s.connected {
            Some(closed_err(&s.closed))
        } else {
            None
        }
    }
}

impl FoguObject for Inbox {
    fn name(&self) -> &str {
        self.type_name()
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "key" => Some(Value::Str(self.key.clone().into())),
            "pending" => {
                Some(Value::Int(self.conn.lock().regs.get(&self.reg).map_or(0, |r| r.queue.len()) as i64))
            }
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // non-blocking: the next item or nil
            "tryNext" => {
                max_args(args, 0, "tryNext")?;
                match self.pop() {
                    Some(i) => Ok(self.wrap(i)),
                    None => self.closed().map_or(Ok(Value::Nil), Err),
                }
            }
            // stops receiving (locally: the protocol has no unsubscribe)
            "close" => {
                max_args(args, 0, "close")?;
                self.conn.lock().unregister(self.reg);
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on {} (next tryNext close)", other, self.type_name())),
        }
    }

    /// `next(timeout?)`: waits for the next item; nil after the timeout.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        let deadline = match (max_args(args, 1, "next"), opt_arg(args, 0)) {
            (Err(e), _) => return Some(Err(e)),
            (_, None) => None,
            (_, Some(v)) => match duration_arg(v, "next") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
        };
        let me = Inbox { conn: Rc::clone(&self.conn), reg: self.reg, kind: self.kind, key: self.key.clone() };
        Some(Ok(Box::new(move || {
            if let Some(i) = me.pop() {
                return Some(Ok(me.wrap(i)));
            }
            if let Some(e) = me.closed() {
                return Some(Err(e));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            poll_again();
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "close"]
    }
}

/// An RPC call received by `act`: answer it with `reply` or `fail`.
struct Request {
    conn: Rc<Conn>,
    id: u64,
    method: String,
    params: Json,
    answered: bool,
}

impl FoguObject for Request {
    fn name(&self) -> &str {
        "Request"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "method" => Some(Value::Str(self.method.clone().into())),
            "params" => Some(json_to_value(&self.params)),
            "id" => Some(Value::Int(self.id as i64)),
            "answered" => Some(Value::Bool(self.answered)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let body = match method {
            "reply" => {
                max_args(args, 1, "reply")?;
                json!({ "result": arg_json(args, 0), "error": null })
            }
            "fail" => {
                max_args(args, 1, "fail")?;
                let msg = match args.first() {
                    Some(Value::Error(e)) => e.message.clone(),
                    Some(v) => v.to_display_string(),
                    None => "handler error".into(),
                };
                json!({ "result": null, "error": msg })
            }
            other => return Err(format!("method '{}' does not exist on Request (reply fail)", other)),
        };
        if self.answered {
            return Err(format!("{}: this request was already answered", method));
        }
        self.answered = true;
        self.conn.write(OP_RES, self.id, body.to_string().as_bytes()).map(|_| Value::Nil)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["reply", "fail"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames() {
        let f = encode_frame(OP_SET, 7, b"{}");
        assert_eq!(f.len(), HEADER_SIZE + 2);
        assert_eq!(&f[..5], &[0x46, 0x4F, VERSION, OP_SET, 0]);
        assert_eq!(u64::from_be_bytes(f[5..13].try_into().unwrap()), 7);
        assert_eq!(u32::from_be_bytes(f[13..17].try_into().unwrap()), 2);
    }

    #[test]
    fn urls() {
        let u = parse_url("fogu://tok-en@10.0.0.1:7300").unwrap();
        assert_eq!((u.token.as_str(), u.host.as_str(), u.port), ("tok-en", "10.0.0.1", 7300));
        let u = parse_url("fogu://admin:secret@localhost:7222").unwrap();
        assert_eq!((u.token.as_str(), u.host.as_str()), ("secret", "localhost"));
        assert!(parse_url("fogu://localhost:7222").is_err());
        assert!(parse_url("fogumq://t@h:1").is_err());
        assert!(parse_url("fogu://t@h:port").is_err());
    }

    #[test]
    fn routing() {
        let mut s = Shared::default();
        let a = s.register(Kind::Listener, "chat");
        let b = s.register(Kind::Listener, "other");
        s.push(Kind::Listener, Some("chat"), &json!({ "channel": "chat" }));
        assert_eq!(s.regs[&a].queue.len(), 1);
        assert_eq!(s.regs[&b].queue.len(), 0);
        let x = s.register(Kind::Service, "c/add");
        let y = s.register(Kind::Service, "c/add");
        assert!(!s.regs.contains_key(&x));
        assert_eq!(s.actions["c/add"], y);
        s.unregister(y);
        assert!(!s.actions.contains_key("c/add"));
    }
}
