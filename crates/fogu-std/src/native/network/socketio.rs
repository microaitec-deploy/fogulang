//! `fogu:socketio` module — Socket.IO (v5 protocol) servers and clients,
//! compatible with the official JavaScript, Python, Go… libraries. The server
//! is socketioxide (WebSocket and long polling); the client is our own
//! (Engine.IO v4 + Socket.IO v5 over the runtime's WebSocket, `fogu:websocket`).
//!
//! ```fogu
//! import socketio from "fogu:socketio"
//!
//! const io = socketio.server()
//! io.on("connection", fn(socket) {
//!     socket.join("lobby")
//!     socket.on("chat", fn(msg, ack) {
//!         io.to("lobby").emit("chat", msg)          // everyone in the room
//!         if ack != nil { ack({ ok: true }) }       // the client used emitWithAck
//!     })
//!     socket.on("disconnect", fn(reason) { print(socket.id, "left:", reason) })
//! })
//! io.listen(3000)                                   // standalone…
//! // …or on a fogu:web app, same port: app.listen(3000, { socketio: io })
//!
//! const c = socketio.connect("http://localhost:3000")?
//! c.on("chat", fn(msg) { print("got", msg) })
//! c.emit("chat", "hello")
//! print(c.emitWithAck("chat", "hi", { timeout: 2s })?)
//! ```
//!
//! Handlers run as `fg` tasks, in the order the messages arrived; the
//! program must stay alive meanwhile (a `time.delay` loop, `wait`, a
//! channel…), as with the other servers. `socket.on("disconnect")`,
//! `c.on("connect" | "disconnect" | "error")` are the lifecycle events.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use socketioxide::extract::{AckSender, Event, SocketRef, TryData};

use socketioxide::socket::DisconnectReason;
use socketioxide::SocketIo;

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched::{self, IoGuard};
use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, json_to_value, list, map, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    // served by `intercept` (they need the interpreter to start their task)
    for (name, min) in [("server", 0), ("connect", 1)] {
        e.insert(name.into(), nfn_var(&format!("socketio.{}", name), min, |_| Err("socketio: internal call".into())));
    }
    ModuleValue {
        name: "fogu:socketio".into(),
        exports: RefCell::new(e),
    }
}

/// `socketio.server` / `socketio.connect` (called by the interpreter before any other call).
pub fn intercept(callee: &Value, args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Option<Result<Value, String>> {
    let Value::NativeFn(f) = callee else { return None };
    match f.name.as_str() {
        "socketio.server" => Some(server(args, interp, env)),
        "socketio.connect" => Some(connect(args, interp, env)),
        _ => None,
    }
}

fn to_json(v: &Value) -> serde_json::Value {
    serde_json::from_str(&crate::native::json::stringify_value(v)).unwrap_or(serde_json::Value::Null)
}

fn opts(v: Option<&Value>, what: &str) -> Result<HashMap<String, Value>, String> {
    match v {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

/// Messages from the network threads, handled in order by the program's task.
struct Queue<J> {
    jobs: Mutex<VecDeque<J>>,
    closed: AtomicBool,
}

impl<J> Queue<J> {
    fn new() -> Arc<Self> {
        Arc::new(Queue { jobs: Mutex::new(VecDeque::new()), closed: AtomicBool::new(false) })
    }

    fn push(&self, j: J) {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).push_back(j);
        sched::wake();
    }

    fn pop(&self) -> Option<J> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }
}

/// The task that runs the Fogu handlers: waits for jobs, handles each one
/// (in order), stops when the queue is closed.
fn start_pump<J: 'static>(interp: &Interpreter, env: &Rc<RefCell<Environment>>, q: Arc<Queue<J>>, handle: impl Fn(&Interpreter, &Rc<RefCell<Environment>>, J) + 'static) -> Result<(), String> {
    let env = Rc::clone(env);
    interp.spawn_task(Box::new(move |it| {
        // yield once first: the code that created the server/client goes on
        // and registers its handlers before any message is handled
        let mut first = true;
        sched::wait_until(|| !std::mem::take(&mut first))?;
        loop {
            let mut job = None;
            sched::wait_until(|| {
                job = q.pop();
                job.is_some() || q.closed.load(Ordering::SeqCst)
            })?;
            match job {
                Some(j) => handle(it, &env, j),
                None => return Ok(()),
            }
        }
    }))
}

/// Calls a handler in its own task (a slow one does not hold the others).
fn run_handler(it: &Interpreter, env: &Rc<RefCell<Environment>>, f: Value, args: Vec<Value>) {
    let env = Rc::clone(env);
    let _ = it.spawn_task(Box::new(move |it| it.call_value(f, args, env).map(|_| ())));
}

// ---------------------------------------------------------------------
// server
// ---------------------------------------------------------------------

enum SJob {
    Connect { ns: String, socket: SocketRef, auth: serde_json::Value },
    Event { sid: String, seq: u64, event: String, args: Vec<serde_json::Value>, ack: Option<AckSender> },
    Disconnect { sid: String, reason: String },
}

/// Every argument of an event (the JSON text minus the event name), and
/// whether the client waits for an ack.
struct RawArgs {
    args: Vec<serde_json::Value>,
    wants_ack: bool,
    /// the message's number on its socket (socketioxide runs handlers as
    /// separate tokio tasks: the numbers restore the order they arrived in)
    seq: u64,
}

/// Next message number of each socket (by sid).
static SEQS: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

impl<A: socketioxide::adapter::Adapter> socketioxide::handler::FromMessageParts<A> for RawArgs {
    type Error = std::convert::Infallible;
    fn from_message_parts(
        s: &Arc<socketioxide::socket::Socket<A>>,
        v: &mut socketioxide_core::Value,
        ack_id: &Option<i64>,
    ) -> Result<Self, std::convert::Infallible> {
        let mut args: Vec<serde_json::Value> = v.as_str().and_then(|s| serde_json::from_str(s.as_str()).ok()).unwrap_or_default();
        if !args.is_empty() {
            args.remove(0); // the event name
        }
        // extraction runs in order, before the handler is spawned
        let seq = {
            let mut m = SEQS.lock().unwrap_or_else(|e| e.into_inner());
            let c = m.get_or_insert_with(HashMap::new).entry(s.id.to_string()).or_insert(0);
            *c += 1;
            *c - 1
        };
        Ok(RawArgs { args, wants_ack: ack_id.is_some(), seq })
    }
}

type Svc = socketioxide::service::SocketIoService<engineioxide::service::NotFoundService>;

/// What `fogu:web` needs to serve Socket.IO on its own port.
#[derive(Clone)]
pub struct WebMount {
    pub path: String,
    svc: Svc,
}

impl std::fmt::Debug for WebMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WebMount({})", self.path)
    }
}

struct ServerState {
    io: SocketIo,
    mount: WebMount,
    queue: Arc<Queue<SJob>>,
    /// namespace → "connection" handlers
    conn: RefCell<HashMap<String, Vec<Value>>>,
    sockets: RefCell<HashMap<String, NativeObjectRef>>,
    listener: RefCell<Option<tokio::task::JoinHandle<()>>>,
    port: Cell<Option<u16>>,
    /// keeps the program from being taken for deadlocked while serving
    guard: RefCell<Option<IoGuard>>,
    /// per socket: the next message number, and messages that came early
    order: RefCell<HashMap<String, (u64, std::collections::BTreeMap<u64, SJob>)>>,
}

impl ServerState {
    /// Accepts connections on `ns` (once).
    fn open_ns(&self, ns: &str) {
        if self.conn.borrow().contains_key(ns) {
            return;
        }
        self.conn.borrow_mut().insert(ns.to_string(), Vec::new());
        let q = Arc::clone(&self.queue);
        let name = ns.to_string();
        self.io.ns(ns.to_string(), async move |s: SocketRef, auth: TryData<serde_json::Value>| {
            // every event goes through the queue, so none is lost before the
            // Fogu "connection" handler has registered its `socket.on`s
            let qe = Arc::clone(&q);
            s.on_fallback(async move |s: SocketRef, Event(event): Event, raw: RawArgs, ack: AckSender| {
                let ack = raw.wants_ack.then_some(ack);
                qe.push(SJob::Event { sid: s.id.to_string(), seq: raw.seq, event, args: raw.args, ack });
            });
            let qd = Arc::clone(&q);
            s.on_disconnect(async move |s: SocketRef, reason: DisconnectReason| {
                qd.push(SJob::Disconnect { sid: s.id.to_string(), reason: reason.to_string() });
            });
            q.push(SJob::Connect { ns: name.clone(), socket: s, auth: auth.0.unwrap_or(serde_json::Value::Null) });
        });
        self.serving();
    }

    fn serving(&self) {
        if self.guard.borrow().is_none() && (self.port.get().is_some() || !self.conn.borrow().is_empty()) {
            *self.guard.borrow_mut() = Some(IoGuard::new());
        }
    }

    fn close(&self) {
        if let Some(t) = self.listener.borrow_mut().take() {
            t.abort();
        }
        let io = self.io.clone();
        crate::native::rt::tokio().spawn(async move { io.close().await });
        self.queue.closed.store(true, Ordering::SeqCst);
        self.guard.borrow_mut().take();
        sched::wake();
    }
}

fn server(args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    max_args(args, 1, "socketio.server")?;
    let o = opts(args.first(), "socketio.server")?;
    for k in o.keys() {
        if !["path", "pingInterval", "pingTimeout", "ackTimeout", "maxPayload"].contains(&k.as_str()) {
            return Err(format!("socketio.server: unknown option '{}' (path pingInterval pingTimeout ackTimeout maxPayload)", k));
        }
    }
    let path = o.get("path").map(|v| v.to_display_string()).unwrap_or_else(|| "/socket.io".into());
    let mut b = SocketIo::builder().req_path(path.clone());
    if let Some(v) = o.get("pingInterval") {
        b = b.ping_interval(duration_arg(v, "pingInterval")?);
    }
    if let Some(v) = o.get("pingTimeout") {
        b = b.ping_timeout(duration_arg(v, "pingTimeout")?);
    }
    if let Some(v) = o.get("ackTimeout") {
        b = b.ack_timeout(duration_arg(v, "ackTimeout")?);
    }
    if let Some(Value::Int(n)) = o.get("maxPayload") {
        b = b.max_payload(*n as u64);
    }
    let (svc, io) = b.build_svc();
    let st = Rc::new(ServerState {
        io,
        mount: WebMount { path: path.trim_end_matches('/').to_string(), svc },
        queue: Queue::new(),
        conn: RefCell::new(HashMap::new()),
        sockets: RefCell::new(HashMap::new()),
        listener: RefCell::new(None),
        order: RefCell::new(HashMap::new()),
        port: Cell::new(None),
        guard: RefCell::new(None),
    });
    let s2 = Rc::clone(&st);
    start_pump(interp, env, Arc::clone(&st.queue), move |it, env, job| handle_server_job(&s2, it, env, job))?;
    Ok(NativeObjectRef::new(SioServer { st }).to_value())
}

/// Delivers a socket's events in the order they arrived.
fn handle_server_job(st: &Rc<ServerState>, it: &Interpreter, env: &Rc<RefCell<Environment>>, job: SJob) {
    match job {
        SJob::Event { ref sid, seq, .. } => {
            let sid = sid.clone();
            let ready: Vec<SJob> = {
                let mut order = st.order.borrow_mut();
                let (next, early) = order.entry(sid).or_default();
                early.insert(seq, job);
                let mut out = Vec::new();
                while let Some(j) = early.remove(next) {
                    out.push(j);
                    *next += 1;
                }
                out
            };
            for j in ready {
                dispatch_job(st, it, env, j);
            }
        }
        SJob::Disconnect { ref sid, .. } => {
            // what is still buffered goes first
            let rest: Vec<SJob> = st.order.borrow_mut().remove(sid).map(|(_, early)| early.into_values().collect()).unwrap_or_default();
            if let Some(m) = SEQS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                m.remove(sid);
            }
            for j in rest {
                dispatch_job(st, it, env, j);
            }
            dispatch_job(st, it, env, job);
        }
        job => dispatch_job(st, it, env, job),
    }
}

fn dispatch_job(st: &Rc<ServerState>, it: &Interpreter, env: &Rc<RefCell<Environment>>, job: SJob) {
    match job {
        SJob::Connect { ns, socket, auth } => {
            let sid = socket.id.to_string();
            let sock = NativeObjectRef::new(SioSocket { st: Rc::clone(st), sock: socket, handlers: Rc::default(), auth: json_to_value(&auth) });
            st.sockets.borrow_mut().insert(sid, sock.clone());
            let hs = st.conn.borrow().get(&ns).cloned().unwrap_or_default();
            for h in hs {
                run_handler(it, env, h, vec![sock.to_value()]);
            }
        }
        SJob::Event { sid, event, args, ack, .. } => {
            let Some(sock) = st.sockets.borrow().get(&sid).cloned() else { return };
            let hs = {
                let inner = sock.inner();
                let b = inner.borrow();
                b.as_any().downcast_ref::<SioSocket>().and_then(|s| s.handlers.borrow().get(&event).cloned()).unwrap_or_default()
            };
            // one parameter per argument, then `ack` when the client waits for one
            let mut args: Vec<Value> = args.iter().map(json_to_value).collect();
            let ack = ack.map(|a| NativeObjectRef::new(Ack { sender: RefCell::new(Some(a)) }));
            if let Some(a) = &ack {
                args.push(Value::bound(a.clone(), "send"));
            }
            for h in hs {
                run_handler(it, env, h, args.clone());
            }
        }
        SJob::Disconnect { sid, reason } => {
            let Some(sock) = st.sockets.borrow_mut().remove(&sid) else { return };
            let hs = {
                let inner = sock.inner();
                let b = inner.borrow();
                b.as_any().downcast_ref::<SioSocket>().and_then(|s| s.handlers.borrow().get("disconnect").cloned()).unwrap_or_default()
            };
            for h in hs {
                run_handler(it, env, h, vec![Value::Str(reason.clone().into())]);
            }
        }
    }
}

/// The `ack` function given to a handler when the client asked for one.
pub struct Ack {
    sender: RefCell<Option<AckSender>>,
}

impl FoguObject for Ack {
    fn name(&self) -> &str {
        "SocketAck"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "send" => {
                let data = args_of(args);
                match self.sender.borrow_mut().take() {
                    Some(s) => s.send(&data).map(|_| Value::Nil).map_err(|e| io_err!("socketio: ack: {}", e)),
                    None => Err("socketio: ack already sent".into()),
                }
            }
            other => Err(format!("method '{}' does not exist on SocketAck", other)),
        }
    }
}

/// Where a broadcast goes: everyone in a namespace, or the others of a socket.
#[derive(Clone)]
enum Origin {
    Io(String),
    Socket(SocketRef),
}

/// `io.to(room)`, `socket.to(room)`, `socket.broadcast`…: rooms + exceptions, then `emit`.
pub struct Broadcast {
    io: SocketIo,
    origin: Origin,
    rooms: Vec<String>,
    except: Vec<String>,
}

fn rooms_arg(args: &[Value], what: &str) -> Result<Vec<String>, String> {
    match args.first() {
        Some(Value::List(l)) => Ok(l.borrow().iter().map(|v| v.to_display_string()).collect()),
        Some(v) => Ok(vec![v.to_display_string()]),
        None => Err(format!("{}(room | [rooms])", what)),
    }
}

impl Broadcast {
    fn emit(&self, event: String, data: Args) -> Result<(), String> {
        let op = match &self.origin {
            Origin::Io(ns) => match self.io.of(ns) {
                Some(op) => op,
                None => return Err(io_err!("socketio: no namespace '{}'", ns)),
            },
            Origin::Socket(s) => s.broadcast(),
        };
        let op = if self.rooms.is_empty() { op } else { op.to(self.rooms.clone()) };
        let op = if self.except.is_empty() { op } else { op.except(self.except.clone()) };
        crate::native::rt::tokio().spawn(async move {
            let _ = op.emit(&event, &data).await;
        });
        Ok(())
    }
}

impl FoguObject for Broadcast {
    fn name(&self) -> &str {
        "SocketBroadcast"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "to" | "in" => {
                let mut b = self.clone_self();
                b.rooms.extend(rooms_arg(args, method)?);
                Ok(NativeObjectRef::new(b).to_value())
            }
            "except" => {
                let mut b = self.clone_self();
                b.except.extend(rooms_arg(args, method)?);
                Ok(NativeObjectRef::new(b).to_value())
            }
            "emit" => {
                let (event, data) = emit_args(args, "emit")?;
                self.emit(event, data)?;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on SocketBroadcast (to except emit)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["to", "except", "emit"]
    }
}

impl Broadcast {
    fn clone_self(&self) -> Broadcast {
        Broadcast { io: self.io.clone(), origin: self.origin.clone(), rooms: self.rooms.clone(), except: self.except.clone() }
    }
}

/// The arguments of an event. Socket.IO tells several arguments from one
/// list by how they are serialized: as a tuple.
#[derive(Clone)]
struct Args(Vec<serde_json::Value>);

impl serde::Serialize for Args {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        match self.0.as_slice() {
            [] => s.serialize_unit(),
            [one] => one.serialize(s),
            many => {
                let mut t = s.serialize_tuple(many.len())?;
                for v in many {
                    t.serialize_element(v)?;
                }
                t.end()
            }
        }
    }
}

fn args_of(vs: &[Value]) -> Args {
    Args(vs.iter().map(to_json).collect())
}

/// `(event, ...args)` → the event and its arguments.
fn emit_args(args: &[Value], what: &str) -> Result<(String, Args), String> {
    let event = str_arg(args, 0, what)?;
    Ok((event, args_of(&args[1..])))
}

pub struct SioServer {
    st: Rc<ServerState>,
}

/// The mount of a `socketio.server()` value (for `app.listen(port, { socketio })`).
pub fn web_mount(v: &Value) -> Result<WebMount, String> {
    if let Value::Object(o) = v {
        if let Some(s) = o.inner().borrow().as_any().downcast_ref::<SioServer>() {
            return Ok(s.st.mount.clone());
        }
    }
    Err(format!("listen: 'socketio' must be a socketio.server(), got '{}'", v.type_name()))
}

fn ns_name(v: &Value) -> String {
    let s = v.to_display_string();
    if s.starts_with('/') { s } else { format!("/{}", s) }
}

impl FoguObject for SioServer {
    fn name(&self) -> &str {
        "SocketIoServer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "port" => Some(self.st.port.get().map(|p| Value::Int(p as i64)).unwrap_or(Value::Nil)),
            "path" => Some(Value::Str(self.st.mount.path.clone().into())),
            "sockets" => Some(Value::Int(self.st.sockets.borrow().len() as i64)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // on("connection", fn(socket)) — the default namespace "/"
            "on" => {
                max_args(args, 2, "on")?;
                let ev = str_arg(args, 0, "on")?;
                if ev != "connection" && ev != "connect" {
                    return Err(format!("on: the server only has the \"connection\" event, got '{}' (use socket.on in it)", ev));
                }
                self.st.open_ns("/");
                self.st.conn.borrow_mut().entry("/".into()).or_default().push(args[1].clone());
                Ok(Value::Nil)
            }
            // of("/admin") → a namespace (on, emit, to)
            "of" => {
                max_args(args, 1, "of")?;
                let ns = ns_name(args.first().unwrap_or(&Value::Str("/".into())));
                Ok(NativeObjectRef::new(SioNamespace { st: Rc::clone(&self.st), ns }).to_value())
            }
            "emit" => {
                let (event, data) = emit_args(args, "emit")?;
                self.st.open_ns("/");
                Broadcast { io: self.st.io.clone(), origin: Origin::Io("/".into()), rooms: vec![], except: vec![] }.emit(event, data)?;
                Ok(Value::Nil)
            }
            "to" | "in" | "except" => {
                self.st.open_ns("/");
                Broadcast { io: self.st.io.clone(), origin: Origin::Io("/".into()), rooms: vec![], except: vec![] }.call(method, args)
            }
            // listen(port | "host:port") → the port (a standalone server)
            "listen" => {
                max_args(args, 1, "listen")?;
                if self.st.listener.borrow().is_some() {
                    return Err("listen: already listening".into());
                }
                let addr = match args.first() {
                    Some(Value::Int(p)) => format!("0.0.0.0:{}", p),
                    Some(Value::Str(s)) if s.starts_with(':') => format!("0.0.0.0{}", s),
                    Some(Value::Str(s)) => s.to_string(),
                    _ => return Err("listen(port | \"host:port\")".into()),
                };
                let std_l = std::net::TcpListener::bind(&addr).map_err(|e| io_err!("socketio: cannot listen on {}: {}", addr, e))?;
                std_l.set_nonblocking(true).map_err(|e| io_err!("socketio: {}", e))?;
                let port = std_l.local_addr().map_err(|e| io_err!("socketio: {}", e))?.port();
                let rt = crate::native::rt::tokio();
                let l = {
                    let _g = rt.enter();
                    tokio::net::TcpListener::from_std(std_l).map_err(|e| io_err!("socketio: {}", e))?
                };
                let svc = self.st.mount.svc.clone();
                *self.st.listener.borrow_mut() = Some(rt.spawn(async move {
                    loop {
                        let Ok((tcp, _)) = l.accept().await else { continue };
                        let svc = svc.clone();
                        tokio::spawn(async move {
                            let _ = hyper::server::conn::http1::Builder::new()
                                .serve_connection(hyper_util::rt::TokioIo::new(tcp), svc)
                                .with_upgrades()
                                .await;
                        });
                    }
                }));
                self.st.port.set(Some(port));
                self.st.serving();
                Ok(Value::Int(port as i64))
            }
            "close" => {
                self.st.close();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on SocketIoServer ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["on", "of", "emit", "to", "except", "listen", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct SioNamespace {
    st: Rc<ServerState>,
    ns: String,
}

impl FoguObject for SioNamespace {
    fn name(&self) -> &str {
        "SocketIoNamespace"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "name").then(|| Value::Str(self.ns.clone().into()))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        self.st.open_ns(&self.ns);
        let b = Broadcast { io: self.st.io.clone(), origin: Origin::Io(self.ns.clone()), rooms: vec![], except: vec![] };
        match method {
            "on" => {
                max_args(args, 2, "on")?;
                let ev = str_arg(args, 0, "on")?;
                if ev != "connection" && ev != "connect" {
                    return Err(format!("on: a namespace only has the \"connection\" event, got '{}'", ev));
                }
                self.st.conn.borrow_mut().entry(self.ns.clone()).or_default().push(args[1].clone());
                Ok(Value::Nil)
            }
            "emit" => {
                let (event, data) = emit_args(args, "emit")?;
                b.emit(event, data)?;
                Ok(Value::Nil)
            }
            "to" | "in" | "except" => b.clone_self().call(method, args),
            other => Err(format!("method '{}' does not exist on SocketIoNamespace (on emit to except)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["on", "emit", "to", "except"]
    }
}

/// One connected client, on the server.
pub struct SioSocket {
    st: Rc<ServerState>,
    sock: SocketRef,
    handlers: Rc<RefCell<HashMap<String, Vec<Value>>>>,
    auth: Value,
}

impl FoguObject for SioSocket {
    fn name(&self) -> &str {
        "Socket"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let parts = self.sock.req_parts();
        Some(match field {
            "id" => Value::Str(self.sock.id.to_string().into()),
            "ns" | "namespace" => Value::Str(self.sock.ns().to_string().into()),
            "connected" => Value::Bool(self.sock.connected()),
            "rooms" => list(self.sock.rooms().into_iter().map(|r| Value::Str(r.to_string().into())).collect()),
            "auth" => self.auth.clone(),
            "handshake" => map(vec![
                ("headers", {
                    let mut pairs: Vec<(String, Value)> =
                        parts.headers.iter().map(|(k, v)| (k.as_str().to_string(), Value::Str(String::from_utf8_lossy(v.as_bytes()).into_owned().into()))).collect();
                    pairs.sort_by(|a, b| a.0.cmp(&b.0));
                    Value::Map(Rc::new(RefCell::new(pairs.into_iter().collect())))
                }),
                ("query", Value::Str(parts.uri.query().unwrap_or("").to_string().into())),
                ("auth", self.auth.clone()),
            ]),
            "broadcast" => NativeObjectRef::new(Broadcast { io: self.st.io.clone(), origin: Origin::Socket(self.sock.clone()), rooms: vec![], except: vec![] }).to_value(),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // on(event, fn(...args, ack?)) · on("disconnect", fn(reason))
            "on" => {
                max_args(args, 2, "on")?;
                let ev = str_arg(args, 0, "on")?;
                self.handlers.borrow_mut().entry(ev).or_default().push(args.get(1).cloned().ok_or("on(event, fn)")?);
                Ok(Value::Nil)
            }
            "off" => {
                max_args(args, 1, "off")?;
                self.handlers.borrow_mut().remove(&str_arg(args, 0, "off")?);
                Ok(Value::Nil)
            }
            // emit(event, ...args): to this client
            "emit" => {
                let (event, data) = emit_args(args, "emit")?;
                self.sock.emit(&event, &data).map_err(|e| io_err!("socketio: emit: {}", e))?;
                Ok(Value::Nil)
            }
            "join" => {
                self.sock.join(rooms_arg(args, "join")?);
                Ok(Value::Nil)
            }
            "leave" => {
                self.sock.leave(rooms_arg(args, "leave")?);
                Ok(Value::Nil)
            }
            // to(room).emit(...): the others in the room (not this socket)
            "to" | "in" | "except" => {
                Broadcast { io: self.st.io.clone(), origin: Origin::Socket(self.sock.clone()), rooms: vec![], except: vec![] }.call(method, args)
            }
            "disconnect" => {
                let _ = self.sock.clone().disconnect();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Socket ({})", other, self.methods().join(" "))),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "emitWithAck" {
            return None;
        }
        // emitWithAck(event, data, { timeout }) → the client's answer
        Some((|| {
            let (event, data, timeout) = ack_args(args)?;
            let stream = self.sock.emit_with_ack::<_, serde_json::Value>(&event, &data).map_err(|e| io_err!("socketio: emit: {}", e))?;
            let pending = crate::native::rt::spawn(async move {
                let r = match timeout {
                    Some(t) => tokio::time::timeout(t, stream).await.map_err(|_| "timeout".to_string())?,
                    None => stream.await,
                };
                r.map_err(|e| e.to_string())
            });
            let poller: Poller = Box::new(move || {
                let r = pending.try_take()?;
                Some(r.map(|v| json_to_value(&v)).map_err(|e| io_err!("socketio: emitWithAck: {}", e)))
            });
            Ok(poller)
        })())
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["on", "off", "emit", "emitWithAck", "join", "leave", "to", "except", "disconnect"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn ack_args(args: &[Value]) -> Result<(String, serde_json::Value, Option<Duration>), String> {
    max_args(args, 3, "emitWithAck")?;
    let event = str_arg(args, 0, "emitWithAck")?;
    let data = args.get(1).map(to_json).unwrap_or(serde_json::Value::Null);
    let timeout = match args.get(2) {
        Some(Value::Map(m)) => m.borrow().get("timeout").map(|t| duration_arg(t, "timeout")).transpose()?,
        _ => None,
    };
    Ok((event, data, timeout))
}

/// Serves a request of `fogu:web` under the Socket.IO path.
pub async fn serve_http(m: &WebMount, req: hyper::Request<hyper::body::Incoming>) -> hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible>> {
    use http_body_util::BodyExt;
    use hyper::service::Service;
    let resp = match m.svc.call(req).await {
        Ok(r) => r,
        Err(_) => {
            return hyper::Response::builder()
                .status(500)
                .body(http_body_util::Full::new(bytes::Bytes::new()).boxed())
                .expect("valid response")
        }
    };
    let (parts, body) = resp.into_parts();
    // polling answers are short; a WebSocket upgrade has an empty body
    let bytes = body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
    hyper::Response::from_parts(parts, http_body_util::Full::new(bytes).boxed())
}

// ---------------------------------------------------------------------
// client
// ---------------------------------------------------------------------

enum CJob {
    Event { event: String, args: Vec<serde_json::Value> },
}

/// What the I/O thread and the program share.
#[derive(Default)]
struct ClientShared {
    connected: AtomicBool,
    closed: AtomicBool,
    /// why the last connection attempt failed (CONNECT_ERROR, network…)
    error: Mutex<Option<String>>,
    /// ack id → its arguments once they arrived
    acks: Mutex<HashMap<u64, Option<Vec<serde_json::Value>>>>,
}

pub struct SioClient {
    out: Option<std::sync::mpsc::Sender<String>>,
    shared: Arc<ClientShared>,
    handlers: Rc<RefCell<HashMap<String, Vec<Value>>>>,
    queue: Arc<Queue<CJob>>,
    nsp: String,
    next_ack: u64,
    url: String,
    guard: Option<IoGuard>,
}

/// `"/admin"` → `"/admin,"`; the main namespace has no prefix.
fn ns_prefix(ns: &str) -> String {
    if ns == "/" { String::new() } else { format!("{},", ns) }
}

/// A Socket.IO packet (after the Engine.IO `4`): `<type>[<ns>,][<ack id>][json]`.
fn parse_packet(p: &str) -> (char, String, Option<u64>, serde_json::Value) {
    let mut chars = p.chars();
    let ty = chars.next().unwrap_or(' ');
    let mut rest = chars.as_str();
    let mut ns = "/".to_string();
    if rest.starts_with('/') {
        let end = rest.find(',').unwrap_or(rest.len());
        ns = rest[..end].to_string();
        rest = rest.get(end + 1..).unwrap_or("");
    }
    let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let id = rest[..digits].parse().ok();
    let data = serde_json::from_str(&rest[digits..]).unwrap_or(serde_json::Value::Null);
    (ty, ns, id, data)
}

struct Dial {
    /// `ws(s)://host[:port]<path>/?EIO=4&transport=websocket`
    ws_url: String,
    headers: Vec<(String, String)>,
    nsp: String,
    auth: serde_json::Value,
    reconnect: bool,
    timeout: Duration,
}

/// The connection thread: Engine.IO v4 over WebSocket, Socket.IO v5 packets,
/// heartbeats and reconnection.
fn engine_loop(d: Dial, shared: Arc<ClientShared>, queue: Arc<Queue<CJob>>, out: std::sync::mpsc::Receiver<String>) {
    use tungstenite::Message;
    let mut backoff = Duration::from_millis(500);
    let push = |event: &str, args: Vec<serde_json::Value>| queue.push(CJob::Event { event: event.into(), args });
    let fail = |e: String| {
        *shared.error.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
        sched::wake();
    };
    'connections: while !shared.closed.load(Ordering::SeqCst) {
        let mut conn = match crate::native::ws_client::dial(&d.ws_url, &d.headers, None, d.timeout) {
            Ok(c) => c,
            Err(e) => {
                fail(e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(&e).to_string());
                if !d.reconnect {
                    return;
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(5));
                continue;
            }
        };
        // short reads: the loop also sends what the program queued
        conn.ws.get_ref().tcp().set_read_timeout(Some(Duration::from_millis(20))).ok();
        let mut limit = d.timeout;
        let mut last_seen = std::time::Instant::now();
        let mut opened = false;
        let reason = loop {
            if shared.closed.load(Ordering::SeqCst) {
                let _ = conn.ws.send(Message::Text(format!("41{}", ns_prefix(&d.nsp))));
                let _ = conn.ws.close(None);
                let _ = conn.ws.flush();
                return;
            }
            while let Ok(text) = out.try_recv() {
                if conn.ws.send(Message::Text(text)).is_err() {
                    break;
                }
            }
            if last_seen.elapsed() > limit {
                break "ping timeout".to_string();
            }
            let msg = match conn.ws.read() {
                Ok(m) => m,
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(e) => break format!("transport close ({})", e),
            };
            last_seen = std::time::Instant::now();
            let Message::Text(t) = msg else {
                if matches!(msg, Message::Close(_)) {
                    break "transport close".to_string();
                }
                continue;
            };
            let t = t.as_str();
            match t.as_bytes().first() {
                // open: { sid, pingInterval, pingTimeout } → join the namespace
                Some(b'0') => {
                    let o: serde_json::Value = serde_json::from_str(&t[1..]).unwrap_or_default();
                    let ms = |k: &str, d: u64| o[k].as_u64().unwrap_or(d);
                    limit = Duration::from_millis(ms("pingInterval", 25_000) + ms("pingTimeout", 20_000));
                    let auth = if d.auth.is_null() { String::new() } else { d.auth.to_string() };
                    let _ = conn.ws.send(Message::Text(format!("40{}{}", ns_prefix(&d.nsp), auth)));
                    opened = true;
                }
                // ping → pong
                Some(b'2') => {
                    let _ = conn.ws.send(Message::Text("3".into()));
                }
                Some(b'1') => break "transport close".to_string(),
                Some(b'4') if opened => {
                    let (ty, ns, id, data) = parse_packet(&t[1..]);
                    if ns != d.nsp {
                        continue;
                    }
                    match ty {
                        '0' => {
                            backoff = Duration::from_millis(500);
                            shared.connected.store(true, Ordering::SeqCst);
                            *shared.error.lock().unwrap_or_else(|p| p.into_inner()) = None;
                            push("connect", vec![]);
                            sched::wake();
                        }
                        '4' => {
                            let msg = data["message"].as_str().map(String::from).unwrap_or_else(|| data.to_string());
                            fail(format!("connection refused: {}", msg));
                            push("error", vec![data]);
                            if !d.reconnect {
                                return;
                            }
                            break "connect error".to_string();
                        }
                        '2' => {
                            let mut arr = match data {
                                serde_json::Value::Array(a) => a,
                                _ => continue,
                            };
                            if arr.is_empty() {
                                continue;
                            }
                            let ev = arr.remove(0);
                            if let Some(name) = ev.as_str() {
                                push(name, arr);
                            }
                        }
                        '3' => {
                            if let Some(id) = id {
                                let args = match data {
                                    serde_json::Value::Array(a) => a,
                                    v => vec![v],
                                };
                                if let Some(slot) = shared.acks.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&id) {
                                    *slot = Some(args);
                                }
                                sched::wake();
                            }
                        }
                        // the server ended this client's session: no reconnection
                        '1' => {
                            shared.connected.store(false, Ordering::SeqCst);
                            push("disconnect", vec![serde_json::Value::String("io server disconnect".into())]);
                            shared.closed.store(true, Ordering::SeqCst);
                            queue.closed.store(true, Ordering::SeqCst);
                            sched::wake();
                            return;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        };
        let was = shared.connected.swap(false, Ordering::SeqCst);
        if was {
            push("disconnect", vec![serde_json::Value::String(reason.clone())]);
        }
        if !d.reconnect || shared.closed.load(Ordering::SeqCst) {
            if !was {
                fail(reason);
            }
            break 'connections;
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

/// connect(url, { namespace, auth, headers, path = "/socket.io", reconnect = true, timeout = 10s }) → SocketIoClient
fn connect(args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    max_args(args, 2, "socketio.connect")?;
    let url = str_arg(args, 0, "socketio.connect")?;
    let o = opts(args.get(1), "socketio.connect")?;
    let (scheme, rest) = match url.split_once("://") {
        Some(("http" | "ws", r)) => ("ws", r),
        Some(("https" | "wss", r)) => ("wss", r),
        _ => return Err(format!("socketio.connect: expected an http(s):// or ws(s):// url, got '{}'", url)),
    };
    let hostport = rest.split(['/', '?']).next().unwrap_or("");
    // like the JavaScript client: the url's path is the namespace
    let url_path = rest[hostport.len()..].split('?').next().unwrap_or("").trim_end_matches('/');
    let nsp = match o.get("namespace") {
        Some(ns) => ns_name(ns),
        None if !url_path.is_empty() => url_path.to_string(),
        None => "/".into(),
    };
    let path = o.get("path").map(|v| v.to_display_string()).unwrap_or_else(|| "/socket.io".into());
    if let Some(t) = o.get("transport") {
        if t.to_display_string() != "websocket" {
            return Err(format!("socketio.connect: only the \"websocket\" transport is supported, got '{}'", t.to_display_string()));
        }
    }
    let headers: Vec<(String, String)> = match o.get("headers") {
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect(),
        _ => Vec::new(),
    };
    let timeout = match o.get("timeout") {
        Some(t) => duration_arg(t, "socketio.connect timeout")?,
        None => Duration::from_secs(10),
    };
    let dial = Dial {
        ws_url: format!("{}://{}{}/?EIO=4&transport=websocket", scheme, hostport, path.trim_end_matches('/')),
        headers,
        nsp: nsp.clone(),
        auth: o.get("auth").map(to_json).unwrap_or(serde_json::Value::Null),
        reconnect: o.get("reconnect").is_none_or(Value::is_truthy),
        timeout,
    };
    let queue: Arc<Queue<CJob>> = Queue::new();
    let shared = Arc::new(ClientShared::default());
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    {
        let (s, q) = (Arc::clone(&shared), Arc::clone(&queue));
        std::thread::Builder::new()
            .name("fogu-socketio".into())
            .spawn(move || engine_loop(dial, s, q, rx))
            .map_err(|e| io_err!("socketio.connect: {}", e))?;
    }
    // connected once the server accepted the namespace (emits before that are lost)
    let deadline = std::time::Instant::now() + timeout;
    let s2 = Arc::clone(&shared);
    let _guard = IoGuard::new();
    sched::wait_until(move || {
        let failed = !s2.connected.load(Ordering::SeqCst) && s2.error.lock().map(|e| e.is_some()).unwrap_or(false);
        let done = s2.connected.load(Ordering::SeqCst) || failed || std::time::Instant::now() >= deadline;
        if !done {
            sched::add_timer(std::time::Instant::now() + Duration::from_millis(10));
        }
        done
    })?;
    if !shared.connected.load(Ordering::SeqCst) {
        shared.closed.store(true, Ordering::SeqCst);
        let why = shared.error.lock().unwrap_or_else(|p| p.into_inner()).clone().unwrap_or_else(|| format!("not connected after {:?}", timeout));
        return Err(io_err!("socketio.connect: {}: {}", url, why));
    }
    let handlers: Rc<RefCell<HashMap<String, Vec<Value>>>> = Rc::default();
    let h2 = Rc::clone(&handlers);
    start_pump(interp, env, Arc::clone(&queue), move |it, env, job| {
        let CJob::Event { event, args } = job;
        let hs = h2.borrow().get(&event).cloned().unwrap_or_default();
        let args: Vec<Value> = args.iter().map(json_to_value).collect();
        for h in hs {
            run_handler(it, env, h, args.clone());
        }
    })?;
    Ok(NativeObjectRef::new(SioClient { out: Some(tx), shared, handlers, queue, nsp, next_ack: 0, url, guard: Some(IoGuard::new()) }).to_value())
}

impl SioClient {
    fn send(&self, packet: String) -> Result<(), String> {
        if !self.shared.connected.load(Ordering::SeqCst) {
            return Err(io_err!("socketio: the client is disconnected"));
        }
        match &self.out {
            Some(o) if o.send(packet).is_ok() => Ok(()),
            _ => Err(io_err!("socketio: the client is disconnected")),
        }
    }

    /// `42[ns,][id]["event", ...args]`
    fn event_packet(&self, event: String, args: &[Value], ack: Option<u64>) -> String {
        let mut arr = vec![serde_json::Value::String(event)];
        arr.extend(args.iter().map(to_json));
        format!("42{}{}{}", ns_prefix(&self.nsp), ack.map(|i| i.to_string()).unwrap_or_default(), serde_json::Value::Array(arr))
    }

    fn close(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        self.shared.connected.store(false, Ordering::SeqCst);
        self.out.take();
        self.queue.closed.store(true, Ordering::SeqCst);
        self.guard.take();
        sched::wake();
    }
}

impl Drop for SioClient {
    fn drop(&mut self) {
        self.close();
    }
}

impl FoguObject for SioClient {
    fn name(&self) -> &str {
        "SocketIoClient"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "connected" => Some(Value::Bool(self.shared.connected.load(Ordering::SeqCst))),
            "url" => Some(Value::Str(self.url.clone().into())),
            "namespace" => Some(Value::Str(self.nsp.clone().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "on" => {
                max_args(args, 2, "on")?;
                let ev = str_arg(args, 0, "on")?;
                self.handlers.borrow_mut().entry(ev).or_default().push(args.get(1).cloned().ok_or("on(event, fn)")?);
                Ok(Value::Nil)
            }
            "off" => {
                max_args(args, 1, "off")?;
                self.handlers.borrow_mut().remove(&str_arg(args, 0, "off")?);
                Ok(Value::Nil)
            }
            // emit(event, ...args)
            "emit" => {
                let event = str_arg(args, 0, "emit")?;
                let packet = self.event_packet(event, &args[1..], None);
                self.send(packet)?;
                Ok(Value::Nil)
            }
            "disconnect" | "close" => {
                self.close();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on SocketIoClient ({})", other, self.methods().join(" "))),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "emitWithAck" {
            return None;
        }
        // emitWithAck(event, data, { timeout = 5s }) → the server's answer
        Some((|| {
            max_args(args, 3, "emitWithAck")?;
            let event = str_arg(args, 0, "emitWithAck")?;
            let data: Vec<Value> = args.get(1).cloned().into_iter().collect();
            let timeout = match args.get(2) {
                Some(Value::Map(m)) => m.borrow().get("timeout").map(|t| duration_arg(t, "timeout")).transpose()?,
                _ => None,
            }
            .unwrap_or(Duration::from_secs(5));
            self.next_ack += 1;
            let id = self.next_ack;
            self.shared.acks.lock().unwrap_or_else(|p| p.into_inner()).insert(id, None);
            let packet = self.event_packet(event, &data, Some(id));
            if let Err(e) = self.send(packet) {
                self.shared.acks.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                return Err(e);
            }
            let shared = Arc::clone(&self.shared);
            let deadline = std::time::Instant::now() + timeout;
            let poller: Poller = Box::new(move || {
                let mut acks = shared.acks.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(Some(v)) = acks.get(&id) {
                    let v = v.clone();
                    acks.remove(&id);
                    return Some(Ok(match v.len() {
                        0 => Value::Nil,
                        1 => json_to_value(&v[0]),
                        _ => list(v.iter().map(json_to_value).collect()),
                    }));
                }
                if std::time::Instant::now() >= deadline {
                    acks.remove(&id);
                    return Some(Err(io_err!("socketio: emitWithAck: timeout")));
                }
                sched::add_timer(std::time::Instant::now() + Duration::from_millis(20));
                None
            });
            Ok(poller)
        })())
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["on", "off", "emit", "emitWithAck", "disconnect"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
