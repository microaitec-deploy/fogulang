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
use std::time::{Duration, Instant};

use socketioxide::adapter::{Adapter, LocalAdapter};
use socketioxide::extract::{AckSender, Event, SocketRef, TryData};
use socketioxide::handler::ConnectHandler;

use socketioxide::socket::DisconnectReason;
use socketioxide::SocketIo;

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched::{self, IoGuard};
use crate::runtime::value::Str;
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

fn opts(v: Option<&Value>, what: &str) -> Result<HashMap<Str, Value>, String> {
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

/// The Redis adapter: several servers (processes, machines) share rooms and broadcasts.
type RedisA = socketioxide_redis::RedisAdapter<socketioxide::adapter::Emitter>;

/// What depends on the adapter: opening a namespace (a remote adapter subscribes to
/// its channels first) and the type of the HTTP service.
trait Kind: Adapter + Sized + 'static {
    fn open<C: ConnectHandler<Self, T>, T: Send + Sync + 'static>(io: &SocketIo<Self>, ns: String, c: C);
    fn svc(svc: socketioxide::service::SocketIoService<engineioxide::service::NotFoundService, Self>) -> Svc;
}

impl Kind for LocalAdapter {
    fn open<C: ConnectHandler<Self, T>, T: Send + Sync + 'static>(io: &SocketIo<Self>, ns: String, c: C) {
        io.ns(ns, c)
    }
    fn svc(svc: socketioxide::service::SocketIoService<engineioxide::service::NotFoundService, Self>) -> Svc {
        Svc::Local(svc)
    }
}

impl Kind for RedisA {
    fn open<C: ConnectHandler<Self, T>, T: Send + Sync + 'static>(io: &SocketIo<Self>, ns: String, c: C) {
        use socketioxide_core::adapter::Spawnable;
        let _rt = crate::native::rt::tokio().enter();
        io.ns(ns, c).spawn();
    }
    fn svc(svc: socketioxide::service::SocketIoService<engineioxide::service::NotFoundService, Self>) -> Svc {
        Svc::Redis(svc)
    }
}

enum SJob<A: Adapter> {
    /// `io.use` middlewares to run before a connection is accepted
    Use { ns: String, socket: SocketRef<A>, auth: serde_json::Value, reply: tokio::sync::oneshot::Sender<Result<(), String>> },
    Connect { ns: String, socket: SocketRef<A>, auth: serde_json::Value },
    Event { sid: String, seq: u64, event: String, args: Vec<serde_json::Value>, ack: Option<AckSender<A>> },
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

type NotFound = engineioxide::service::NotFoundService;

/// The HTTP service of a server, by adapter.
#[derive(Clone)]
enum Svc {
    Local(socketioxide::service::SocketIoService<NotFound>),
    Redis(socketioxide::service::SocketIoService<NotFound, RedisA>),
}

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

// --- connection state recovery ---------------------------------------

/// A broadcast, kept for the sockets that were away when it left.
struct Logged {
    offset: String,
    at: Instant,
    ns: String,
    rooms: Vec<String>,
    except: Vec<String>,
    event: String,
    args: Vec<serde_json::Value>,
}

/// A socket that lost its connection: what it had, for `maxDisconnection`.
struct Session {
    sid: String,
    ns: String,
    rooms: Vec<String>,
    /// `socket.data` as JSON (filled in by the program's task)
    data: serde_json::Value,
    /// when it connected (a client that never got a broadcast has no offset)
    since: Instant,
    at: Instant,
}

/// What a recovered socket gets back when its connection is accepted.
struct Restore {
    rooms: Vec<String>,
    data: serde_json::Value,
    missed: Vec<(String, Vec<serde_json::Value>)>,
}

/// Socket.IO's connection state recovery (the official servers' `connectionStateRecovery`):
/// each socket gets a private id (the CONNECT packet's `pid`), each broadcast an offset
/// (its last argument). A client that comes back within `maxDisconnection` with its pid and
/// the last offset it saw gets its rooms, its `socket.data` and the broadcasts it missed.
struct Recovery {
    max: Duration,
    skip_middlewares: bool,
    nonce: String,
    next: u64,
    log: VecDeque<Logged>,
    /// pid → the session of a socket that is away
    sessions: HashMap<String, Session>,
    /// sid → (pid, connected at) of the live sockets
    live: HashMap<String, (String, Instant)>,
    /// new sid → what to give back to it
    restoring: HashMap<String, Restore>,
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

impl Recovery {
    fn new(max: Duration, skip_middlewares: bool) -> Recovery {
        Recovery {
            max,
            skip_middlewares,
            nonce: random_hex(4),
            next: 0,
            log: VecDeque::new(),
            sessions: HashMap::new(),
            live: HashMap::new(),
            restoring: HashMap::new(),
        }
    }

    fn prune(&mut self) {
        let now = Instant::now();
        while self.log.front().is_some_and(|e| now.duration_since(e.at) > self.max) {
            self.log.pop_front();
        }
        let max = self.max;
        self.sessions.retain(|_, s| now.duration_since(s.at) <= max);
    }

    /// Keeps a broadcast; returns its offset.
    fn record(&mut self, ns: String, rooms: Vec<String>, except: Vec<String>, event: &str, args: &[serde_json::Value]) -> String {
        self.prune();
        self.next += 1;
        let offset = format!("{}-{:x}", self.nonce, self.next);
        self.log.push_back(Logged { offset: offset.clone(), at: Instant::now(), ns, rooms, except, event: event.to_string(), args: args.to_vec() });
        offset
    }

    /// At the connection: a known pid (and offset) gives the session back. Returns the
    /// socket's pid and whether it was recovered.
    fn admit(&mut self, sid: &str, ns: &str, auth: &serde_json::Value) -> (String, bool) {
        self.prune();
        let now = Instant::now();
        let pid = auth.get("pid").and_then(|v| v.as_str());
        let offset = auth.get("offset").and_then(|v| v.as_str());
        if let Some(pid) = pid {
            if self.sessions.get(pid).is_some_and(|s| s.ns == ns) {
                // the broadcasts after the last one the client saw (all of them since it
                // connected when it saw none); an offset no longer kept loses the session
                let from = match offset {
                    Some(o) => self.log.iter().position(|e| e.offset == o).map(|i| i + 1),
                    None => {
                        let since = self.sessions[pid].since;
                        Some(self.log.iter().position(|e| e.at >= since).unwrap_or(self.log.len()))
                    }
                };
                if let Some(from) = from {
                    let s = self.sessions.remove(pid).expect("session");
                    let mut mine = s.rooms.clone();
                    mine.push(s.sid.clone());
                    let missed = self
                        .log
                        .iter()
                        .skip(from)
                        .filter(|e| e.ns == s.ns && (e.rooms.is_empty() || e.rooms.iter().any(|r| mine.contains(r))) && !e.except.iter().any(|r| mine.contains(r)))
                        .map(|e| {
                            let mut args = e.args.clone();
                            args.push(serde_json::Value::String(e.offset.clone()));
                            (e.event.clone(), args)
                        })
                        .collect();
                    let rooms = s.rooms.into_iter().filter(|r| *r != s.sid).collect();
                    self.restoring.insert(sid.to_string(), Restore { rooms, data: s.data, missed });
                    self.live.insert(sid.to_string(), (pid.to_string(), s.since));
                    return (pid.to_string(), true);
                }
            }
        }
        let pid = random_hex(16);
        self.live.insert(sid.to_string(), (pid.clone(), now));
        (pid, false)
    }

    /// At a disconnection the client did not ask for: the session is kept.
    fn away(&mut self, sid: &str, ns: &str, rooms: Vec<String>, recoverable: bool) {
        let Some((pid, since)) = self.live.remove(sid) else { return };
        self.restoring.remove(sid);
        if recoverable {
            self.sessions.insert(pid, Session { sid: sid.to_string(), ns: ns.to_string(), rooms, data: serde_json::Value::Null, since, at: Instant::now() });
        }
    }
}

type Shared<T> = Arc<Mutex<T>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------

struct ServerState<A: Kind> {
    io: SocketIo<A>,
    mount: WebMount,
    queue: Arc<Queue<SJob<A>>>,
    /// namespace → "connection" handlers
    conn: RefCell<HashMap<String, Vec<Value>>>,
    sockets: RefCell<HashMap<String, NativeObjectRef>>,
    listener: RefCell<Option<tokio::task::JoinHandle<()>>>,
    port: Cell<Option<u16>>,
    /// keeps the program from being taken for deadlocked while serving
    guard: RefCell<Option<IoGuard>>,
    /// per socket: the next message number, and messages that came early
    order: RefCell<HashMap<String, (u64, std::collections::BTreeMap<u64, SJob<A>>)>>,
    /// namespace → `use` middlewares, and whether it has any (read by the network side)
    uses: RefCell<HashMap<String, Vec<Value>>>,
    has_uses: RefCell<HashMap<String, Arc<AtomicBool>>>,
    /// sockets a middleware saw, until their connection (so `socket.data` carries over)
    admitted: RefCell<HashMap<String, NativeObjectRef>>,
    /// connection state recovery, when the `recovery` option is on
    recovery: Option<Shared<Recovery>>,
    /// "redis" or "local"
    adapter: &'static str,
}

impl<A: Kind> ServerState<A> {
    /// Accepts connections on `ns` (once).
    fn open_ns(&self, ns: &str) {
        if self.conn.borrow().contains_key(ns) {
            return;
        }
        self.conn.borrow_mut().insert(ns.to_string(), Vec::new());
        let q = Arc::clone(&self.queue);
        let name = ns.to_string();
        // `io.use`: the network side asks the program (and waits) before connecting;
        // without middlewares it does not ask at all
        let flag = Arc::clone(self.has_uses.borrow_mut().entry(ns.to_string()).or_default());
        let (qm, ns_m, rec_m) = (Arc::clone(&self.queue), ns.to_string(), self.recovery.clone());
        let middleware = move |s: SocketRef<A>, auth: TryData<serde_json::Value>| {
            let (q, flag, ns, rec) = (Arc::clone(&qm), Arc::clone(&flag), ns_m.clone(), rec_m.clone());
            async move {
                let auth = auth.0.unwrap_or(serde_json::Value::Null);
                if let Some(rec) = &rec {
                    // the pid goes in the CONNECT packet; a recovered socket skips the
                    // middlewares (unless `skipMiddlewares: false`), as in Socket.IO
                    let (pid, recovered, skip) = {
                        let mut r = lock(rec);
                        let (pid, recovered) = r.admit(&s.id.to_string(), &ns, &auth);
                        (pid, recovered, r.skip_middlewares)
                    };
                    s.set_recovery_pid(pid);
                    if recovered && skip {
                        return Ok::<(), String>(());
                    }
                }
                if !flag.load(Ordering::SeqCst) {
                    return Ok::<(), String>(());
                }
                let (tx, rx) = tokio::sync::oneshot::channel();
                let sid = s.id.to_string();
                q.push(SJob::Use { ns, socket: s, auth, reply: tx });
                let verdict = match rx.await {
                    Ok(r) => r,
                    Err(_) => Err("the server is closing".to_string()),
                };
                if verdict.is_err() {
                    if let Some(rec) = &rec {
                        lock(rec).away(&sid, "", vec![], false);
                    }
                }
                verdict
            }
        };
        let rec_h = self.recovery.clone();
        let handler = async move |s: SocketRef<A>, auth: TryData<serde_json::Value>| {
            // every event goes through the queue, so none is lost before the
            // Fogu "connection" handler has registered its `socket.on`s
            let qe = Arc::clone(&q);
            s.on_fallback(async move |s: SocketRef<A>, Event(event): Event, raw: RawArgs, ack: AckSender<A>| {
                let ack = raw.wants_ack.then_some(ack);
                qe.push(SJob::Event { sid: s.id.to_string(), seq: raw.seq, event, args: raw.args, ack });
            });
            let qd = Arc::clone(&q);
            let rec_d = rec_h.clone();
            s.on_disconnect(async move |s: SocketRef<A>, reason: DisconnectReason| {
                if let Some(rec) = &rec_d {
                    // the rooms are still there: the socket leaves them after this handler
                    let recoverable = matches!(reason, DisconnectReason::TransportClose | DisconnectReason::TransportError | DisconnectReason::HeartbeatTimeout | DisconnectReason::ClosingServer);
                    let rooms = s.rooms().into_iter().map(|r| r.to_string()).collect();
                    lock(rec).away(&s.id.to_string(), s.ns(), rooms, recoverable);
                }
                qd.push(SJob::Disconnect { sid: s.id.to_string(), reason: reason.to_string() });
            });
            q.push(SJob::Connect { ns: name.clone(), socket: s, auth: auth.0.unwrap_or(serde_json::Value::Null) });
        };
        A::open(&self.io, ns.to_string(), handler.with(middleware));
        self.serving();
    }

    /// `io.use(fn)` / `io.of(ns).use(fn)`.
    fn add_use(&self, ns: &str, f: Value) {
        self.uses.borrow_mut().entry(ns.to_string()).or_default().push(f);
        self.open_ns(ns);
        if let Some(flag) = self.has_uses.borrow().get(ns) {
            flag.store(true, Ordering::SeqCst);
        }
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

    fn broadcast(&self, origin: Origin<A>) -> Broadcast<A> {
        Broadcast { io: self.io.clone(), origin, rooms: vec![], except: vec![], recovery: self.recovery.clone() }
    }
}

const SERVER_OPTIONS: &str = "path pingInterval pingTimeout ackTimeout maxPayload recovery redis redisPrefix";

fn server(args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    max_args(args, 1, "socketio.server")?;
    let o = opts(args.first(), "socketio.server")?;
    for k in o.keys() {
        if !SERVER_OPTIONS.split(' ').any(|n| n == k.as_str()) {
            return Err(format!("socketio.server: unknown option '{}' ({})", k, SERVER_OPTIONS));
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
    // one limit for both transports: polling payloads and WebSocket messages
    match o.get("maxPayload") {
        None | Some(Value::Nil) => {}
        Some(Value::Int(n)) if *n > 0 => {
            b = b.max_payload(*n as u64).ws_max_message_size(*n as usize).ws_max_frame_size(*n as usize);
        }
        Some(other) => return Err(format!("socketio.server: maxPayload must be a number of bytes > 0, got {}", other.to_display_string())),
    }
    // recovery: true | { maxDisconnection = 2m, skipMiddlewares = true }
    let recovery = match o.get("recovery") {
        None | Some(Value::Nil) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(Recovery::new(Duration::from_secs(120), true)),
        Some(Value::Map(m)) => {
            let m = m.borrow();
            for k in m.keys() {
                if !["maxDisconnection", "skipMiddlewares"].contains(&k.as_str()) {
                    return Err(format!("socketio.server: unknown recovery option '{}' (maxDisconnection skipMiddlewares)", k));
                }
            }
            let max = m.get("maxDisconnection").map(|v| duration_arg(v, "maxDisconnection")).transpose()?.unwrap_or(Duration::from_secs(120));
            let skip = m.get("skipMiddlewares").is_none_or(Value::is_truthy);
            Some(Recovery::new(max, skip))
        }
        Some(other) => return Err(format!("socketio.server: recovery must be true or a map, got '{}'", other.type_name())),
    }
    .map(|r| Arc::new(Mutex::new(r)));
    match o.get("redis") {
        None | Some(Value::Nil) => {
            if o.contains_key("redisPrefix") {
                return Err("socketio.server: redisPrefix needs the redis option".into());
            }
            let (svc, io) = b.build_svc();
            finish_server(io, LocalAdapter::svc(svc), path, recovery, "local", interp, env)
        }
        Some(Value::Str(url)) => {
            use socketioxide_redis::drivers::redis::redis_client as redis;
            // the adapter reads its channels as RESP3 pushes
            let mut url = url.to_string();
            if !url.contains("protocol=") {
                url.push(if url.contains('?') { '&' } else { '?' });
                url.push_str("protocol=resp3");
            }
            let client = redis::Client::open(url.as_str()).map_err(|e| io_err!("socketio.server: redis: {}", e))?;
            let mut config = socketioxide_redis::RedisAdapterConfig::new();
            if let Some(p) = o.get("redisPrefix") {
                config = config.with_prefix(p.to_display_string());
            }
            let ctr = crate::native::rt::block_on(async move { socketioxide_redis::RedisAdapterCtr::new_with_redis_config(&client, config).await })?
                .map_err(|e| io_err!("socketio.server: redis: {}", e))?;
            let (svc, io) = b.with_adapter::<RedisA>(ctr).build_svc();
            finish_server(io, RedisA::svc(svc), path, recovery, "redis", interp, env)
        }
        Some(other) => Err(format!("socketio.server: redis must be a url (\"redis://host:6379\"), got '{}'", other.type_name())),
    }
}

fn finish_server<A: Kind>(io: SocketIo<A>, svc: Svc, path: String, recovery: Option<Shared<Recovery>>, adapter: &'static str, interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    let st = Rc::new(ServerState {
        io,
        mount: WebMount { path: path.trim_end_matches('/').to_string(), svc },
        queue: Queue::new(),
        conn: RefCell::new(HashMap::new()),
        sockets: RefCell::new(HashMap::new()),
        listener: RefCell::new(None),
        order: RefCell::new(HashMap::new()),
        uses: RefCell::new(HashMap::new()),
        has_uses: RefCell::new(HashMap::new()),
        admitted: RefCell::new(HashMap::new()),
        port: Cell::new(None),
        guard: RefCell::new(None),
        recovery,
        adapter,
    });
    let s2 = Rc::clone(&st);
    start_pump(interp, env, Arc::clone(&st.queue), move |it, env, job| handle_server_job(&s2, it, env, job))?;
    Ok(NativeObjectRef::new(SioServer { st }).to_value())
}

/// Delivers a socket's events in the order they arrived.
fn handle_server_job<A: Kind>(st: &Rc<ServerState<A>>, it: &Interpreter, env: &Rc<RefCell<Environment>>, job: SJob<A>) {
    match job {
        SJob::Event { ref sid, seq, .. } => {
            let sid = sid.clone();
            let ready: Vec<SJob<A>> = {
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
            let rest: Vec<SJob<A>> = st.order.borrow_mut().remove(sid).map(|(_, early)| early.into_values().collect()).unwrap_or_default();
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

fn new_socket<A: Kind>(st: &Rc<ServerState<A>>, socket: SocketRef<A>, auth: &serde_json::Value) -> NativeObjectRef {
    NativeObjectRef::new(SioSocket { st: Rc::clone(st), sock: socket, handlers: Rc::default(), auth: json_to_value(auth), data: map(vec![]), recovered: false })
}

fn with_socket<A: Kind, R>(sock: &NativeObjectRef, f: impl FnOnce(&SioSocket<A>) -> R) -> Option<R> {
    let inner = sock.inner();
    let b = inner.borrow();
    b.as_any().downcast_ref::<SioSocket<A>>().map(f)
}

fn dispatch_job<A: Kind>(st: &Rc<ServerState<A>>, it: &Interpreter, env: &Rc<RefCell<Environment>>, job: SJob<A>) {
    match job {
        // the middlewares, in order, in a task of their own: an error refuses the connection
        SJob::Use { ns, socket, auth, reply } => {
            let sid = socket.id.to_string();
            let sock = new_socket(st, socket, &auth);
            let uses = st.uses.borrow().get(&ns).cloned().unwrap_or_default();
            let (st2, env) = (Rc::clone(st), Rc::clone(env));
            let _ = it.spawn_task(Box::new(move |it| {
                let mut verdict = Ok(());
                for f in uses {
                    let r = it.call_value(f, vec![sock.to_value()], Rc::clone(&env));
                    let r = match r {
                        Ok(v) => v.into_result().map(|_| ()).map_err(|e| e.message.clone()),
                        Err(e) => {
                            eprintln!("[socketio] middleware: {}", e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(&e));
                            Err("server error".to_string())
                        }
                    };
                    if r.is_err() {
                        verdict = r;
                        break;
                    }
                }
                if verdict.is_ok() {
                    st2.admitted.borrow_mut().insert(sid, sock);
                }
                let _ = reply.send(verdict);
                Ok(())
            }));
        }
        SJob::Connect { ns, socket, auth } => {
            let sid = socket.id.to_string();
            let sock = st.admitted.borrow_mut().remove(&sid).unwrap_or_else(|| new_socket(st, socket, &auth));
            // a recovered socket: its rooms, its data, then the broadcasts it missed
            let restore = st.recovery.as_ref().and_then(|r| lock(r).restoring.remove(&sid));
            if let Some(r) = restore {
                let inner = sock.inner();
                let mut b = inner.borrow_mut();
                if let Some(s) = b.as_any_mut().downcast_mut::<SioSocket<A>>() {
                    s.recovered = true;
                    if !r.data.is_null() {
                        s.data = json_to_value(&r.data);
                    }
                    if !r.rooms.is_empty() {
                        s.sock.join(r.rooms);
                    }
                    let _rt = crate::native::rt::tokio().enter();
                    for (event, args) in r.missed {
                        let _ = s.sock.emit(&event, &Args(args));
                    }
                }
            }
            st.sockets.borrow_mut().insert(sid, sock.clone());
            let hs = st.conn.borrow().get(&ns).cloned().unwrap_or_default();
            for h in hs {
                run_handler(it, env, h, vec![sock.to_value()]);
            }
        }
        SJob::Event { sid, event, args, ack, .. } => {
            // reserved names: only the server raises them (a client sending
            // "disconnect" must not run the cleanup of a live socket)
            if matches!(event.as_str(), "connect" | "connect_error" | "disconnect" | "disconnecting") {
                return;
            }
            let Some(sock) = st.sockets.borrow().get(&sid).cloned() else { return };
            let hs = with_socket::<A, _>(&sock, |s| s.handlers.borrow().get(&event).cloned()).flatten().unwrap_or_default();
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
            // a kept session carries `socket.data` (the network side kept the rooms)
            if let Some(rec) = &st.recovery {
                if let Some(data) = with_socket::<A, _>(&sock, |s| to_json(&s.data)) {
                    let mut r = lock(rec);
                    if let Some(s) = r.sessions.values_mut().find(|s| s.sid == sid) {
                        s.data = data;
                    }
                }
            }
            let hs = with_socket::<A, _>(&sock, |s| s.handlers.borrow().get("disconnect").cloned()).flatten().unwrap_or_default();
            for h in hs {
                run_handler(it, env, h, vec![Value::Str(reason.clone().into())]);
            }
        }
    }
}

/// The `ack` function given to a handler when the client asked for one.
struct Ack<A: Adapter> {
    sender: RefCell<Option<AckSender<A>>>,
}

impl<A: Kind> FoguObject for Ack<A> {
    fn name(&self) -> &str {
        "SocketAck"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        // socketioxide spawns tokio tasks (acks, disconnects, broadcasts): it
        // must run inside the shared runtime's context, or it panics
        let _rt = crate::native::rt::tokio().enter();
        match method {
            "send" => {
                let data = args_of(args);
                match self.sender.borrow_mut().take() {
                    Some(s) => s.send(&data).map(|_| Value::Nil).map_err(|e| io_err!("socketio: ack: {}", e)),
                    None => Err("socketio: ack already sent".into()),
                }
            }
            other => Err(format!("method '{}' does not exist on SocketAck (send)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["send"]
    }
}

/// Where a broadcast goes: everyone in a namespace, or the others of a socket.
enum Origin<A: Adapter> {
    Io(String),
    Socket(SocketRef<A>),
}

impl<A: Adapter> Clone for Origin<A> {
    fn clone(&self) -> Self {
        match self {
            Origin::Io(ns) => Origin::Io(ns.clone()),
            Origin::Socket(s) => Origin::Socket(s.clone()),
        }
    }
}

/// `io.to(room)`, `socket.to(room)`, `socket.broadcast`…: rooms + exceptions, then `emit`.
struct Broadcast<A: Kind> {
    io: SocketIo<A>,
    origin: Origin<A>,
    rooms: Vec<String>,
    except: Vec<String>,
    recovery: Option<Shared<Recovery>>,
}

fn rooms_arg(args: &[Value], what: &str) -> Result<Vec<String>, String> {
    match args.first() {
        Some(Value::List(l)) => Ok(l.borrow().iter().map(|v| v.to_display_string()).collect()),
        Some(v) => Ok(vec![v.to_display_string()]),
        None => Err(format!("{}(room | [rooms])", what)),
    }
}

impl<A: Kind> Broadcast<A> {
    fn emit(&self, event: String, mut data: Args) -> Result<(), String> {
        let op = match &self.origin {
            Origin::Io(ns) => match self.io.of(ns) {
                Some(op) => op,
                None => return Err(io_err!("socketio: no namespace '{}'", ns)),
            },
            Origin::Socket(s) => s.broadcast(),
        };
        // with recovery, each broadcast is kept and carries its offset as the last argument
        if let Some(rec) = &self.recovery {
            let (ns, mut except) = match &self.origin {
                Origin::Io(ns) => (ns.clone(), self.except.clone()),
                Origin::Socket(s) => (s.ns().to_string(), self.except.clone()),
            };
            if let Origin::Socket(s) = &self.origin {
                except.push(s.id.to_string());
            }
            let offset = lock(rec).record(ns, self.rooms.clone(), except, &event, &data.0);
            data.0.push(serde_json::Value::String(offset));
        }
        let op = if self.rooms.is_empty() { op } else { op.to(self.rooms.clone()) };
        let op = if self.except.is_empty() { op } else { op.except(self.except.clone()) };
        // one queue drained by one task: broadcasts leave in the order they
        // were made (a task per emit let them overtake each other)
        broadcast_queue().send(Box::pin(async move {
            let _ = op.emit(&event, &data).await;
        })).map_err(|_| io_err!("socketio: the server is closed"))?;
        Ok(())
    }

    fn clone_self(&self) -> Broadcast<A> {
        Broadcast { io: self.io.clone(), origin: self.origin.clone(), rooms: self.rooms.clone(), except: self.except.clone(), recovery: self.recovery.clone() }
    }
}

type Job = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// The process-wide queue of broadcast emits, run one after another.
fn broadcast_queue() -> &'static tokio::sync::mpsc::UnboundedSender<Job> {
    static Q: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<Job>> = std::sync::OnceLock::new();
    Q.get_or_init(|| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
        crate::native::rt::tokio().spawn(async move {
            while let Some(job) = rx.recv().await {
                job.await;
            }
        });
        tx
    })
}

impl<A: Kind> FoguObject for Broadcast<A> {
    fn name(&self) -> &str {
        "SocketBroadcast"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        // socketioxide spawns tokio tasks (acks, disconnects, broadcasts): it
        // must run inside the shared runtime's context, or it panics
        let _rt = crate::native::rt::tokio().enter();
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

struct SioServer<A: Kind> {
    st: Rc<ServerState<A>>,
}

/// The mount of a `socketio.server()` value (for `app.listen(port, { socketio })`).
pub fn web_mount(v: &Value) -> Result<WebMount, String> {
    if let Value::Object(o) = v {
        let inner = o.inner();
        let b = inner.borrow();
        if let Some(s) = b.as_any().downcast_ref::<SioServer<LocalAdapter>>() {
            return Ok(s.st.mount.clone());
        }
        if let Some(s) = b.as_any().downcast_ref::<SioServer<RedisA>>() {
            return Ok(s.st.mount.clone());
        }
    }
    Err(format!("listen: 'socketio' must be a socketio.server(), got '{}'", v.type_name()))
}

fn ns_name(v: &Value) -> String {
    let s = v.to_display_string();
    if s.starts_with('/') { s } else { format!("/{}", s) }
}

impl<A: Kind> FoguObject for SioServer<A> {
    fn name(&self) -> &str {
        "SocketIoServer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "port" => Some(self.st.port.get().map(|p| Value::Int(p as i64)).unwrap_or(Value::Nil)),
            "path" => Some(Value::Str(self.st.mount.path.clone().into())),
            "sockets" => Some(Value::Int(self.st.sockets.borrow().len() as i64)),
            "adapter" => Some(Value::Str(self.st.adapter.into())),
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
            // use(fn(socket)): runs before "connection"; returning an error refuses the
            // socket (the client gets connect_error with its message)
            "use" => {
                max_args(args, 1, "use")?;
                let f = args.first().cloned().unwrap_or(Value::Nil);
                if !matches!(f, Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) | Value::StructBoundMethod(_)) {
                    return Err(format!("use: expects a function, got '{}'", f.type_name()));
                }
                let _rt = crate::native::rt::tokio().enter();
                self.st.add_use("/", f);
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
                let _rt = crate::native::rt::tokio().enter();
                self.st.broadcast(Origin::Io("/".into())).emit(event, data)?;
                Ok(Value::Nil)
            }
            "to" | "in" | "except" => {
                self.st.open_ns("/");
                self.st.broadcast(Origin::Io("/".into())).call(method, args)
            }
            // listen(port | "host:port", { tls: { cert, key } }?) → the port (a standalone server)
            "listen" => {
                max_args(args, 2, "listen")?;
                let tls = match args.get(1) {
                    None | Some(Value::Nil) => None,
                    Some(Value::Map(m)) => {
                        let m = m.borrow();
                        if let Some(k) = m.keys().find(|k| k.as_str() != "tls") {
                            return Err(format!("listen: unknown option '{}' (tls)", k));
                        }
                        match m.get("tls") {
                            None | Some(Value::Nil) => None,
                            Some(v) => Some(tokio_rustls::TlsAcceptor::from(crate::native::tls::server_config(v, "socketio listen", &["http/1.1"])?)),
                        }
                    }
                    Some(other) => return Err(format!("listen: options must be a map, got '{}'", other.type_name())),
                };
                if self.st.listener.borrow().is_some() {
                    return Err("listen: already listening".into());
                }
                let addr = match args.first() {
                    Some(Value::Int(p)) => format!("127.0.0.1:{}", p), // like app.listen: the network only when asked ("0.0.0.0:port")
                    Some(Value::Str(s)) if s.starts_with(':') => format!("127.0.0.1{}", s),
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
                        let (svc, tls) = (svc.clone(), tls.clone());
                        tokio::spawn(async move {
                            let io: crate::native::websocket::WsIo = match tls {
                                None => Box::new(tcp),
                                Some(t) => match tokio::time::timeout(Duration::from_secs(10), t.accept(tcp)).await {
                                    Ok(Ok(s)) => Box::new(s),
                                    _ => return,
                                },
                            };
                            let io = hyper_util::rt::TokioIo::new(io);
                            let b = hyper::server::conn::http1::Builder::new();
                            let _ = match svc {
                                Svc::Local(s) => b.serve_connection(io, s).with_upgrades().await,
                                Svc::Redis(s) => b.serve_connection(io, s).with_upgrades().await,
                            };
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
        vec!["on", "use", "of", "emit", "to", "except", "listen", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct SioNamespace<A: Kind> {
    st: Rc<ServerState<A>>,
    ns: String,
}

impl<A: Kind> FoguObject for SioNamespace<A> {
    fn name(&self) -> &str {
        "SocketIoNamespace"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "name").then(|| Value::Str(self.ns.clone().into()))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        // socketioxide spawns tokio tasks (acks, disconnects, broadcasts): it
        // must run inside the shared runtime's context, or it panics
        let _rt = crate::native::rt::tokio().enter();
        self.st.open_ns(&self.ns);
        let b = self.st.broadcast(Origin::Io(self.ns.clone()));
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
            "use" => {
                max_args(args, 1, "use")?;
                let f = args.first().cloned().unwrap_or(Value::Nil);
                if !matches!(f, Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) | Value::StructBoundMethod(_)) {
                    return Err(format!("use: expects a function, got '{}'", f.type_name()));
                }
                self.st.add_use(&self.ns, f);
                Ok(Value::Nil)
            }
            "emit" => {
                let (event, data) = emit_args(args, "emit")?;
                b.emit(event, data)?;
                Ok(Value::Nil)
            }
            "to" | "in" | "except" => b.clone_self().call(method, args),
            other => Err(format!("method '{}' does not exist on SocketIoNamespace (on use emit to except)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["on", "use", "emit", "to", "except"]
    }
}

/// One connected client, on the server.
struct SioSocket<A: Kind> {
    st: Rc<ServerState<A>>,
    sock: SocketRef<A>,
    handlers: Rc<RefCell<HashMap<String, Vec<Value>>>>,
    auth: Value,
    /// `socket.data`: what the program keeps about the socket (set by a middleware…)
    data: Value,
    /// connection state recovery gave this socket its previous session back
    recovered: bool,
}

impl<A: Kind> FoguObject for SioSocket<A> {
    fn name(&self) -> &str {
        "Socket"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let parts = self.sock.req_parts();
        Some(match field {
            "id" => Value::Str(self.sock.id.to_string().into()),
            "ns" | "namespace" => Value::Str(self.sock.ns().to_string().into()),
            "connected" => Value::Bool(self.sock.connected()),
            "recovered" => Value::Bool(self.recovered),
            "rooms" => list(self.sock.rooms().into_iter().map(|r| Value::Str(r.to_string().into())).collect()),
            "auth" => self.auth.clone(),
            "data" => self.data.clone(),
            "handshake" => map(vec![
                ("headers", {
                    let mut pairs: Vec<(String, Value)> =
                        parts.headers.iter().map(|(k, v)| (k.as_str().to_string(), Value::Str(String::from_utf8_lossy(v.as_bytes()).into_owned().into()))).collect();
                    pairs.sort_by(|a, b| a.0.cmp(&b.0));
                    Value::Map(Rc::new(RefCell::new(pairs.into_iter().map(|(k, v)| (Str::from(k), v)).collect())))
                }),
                ("query", Value::Str(parts.uri.query().unwrap_or("").to_string().into())),
                ("auth", self.auth.clone()),
            ]),
            "broadcast" => NativeObjectRef::new(self.st.broadcast(Origin::Socket(self.sock.clone()))).to_value(),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        // socketioxide spawns tokio tasks (acks, disconnects, broadcasts): it
        // must run inside the shared runtime's context, or it panics
        let _rt = crate::native::rt::tokio().enter();
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
            "to" | "in" | "except" => self.st.broadcast(Origin::Socket(self.sock.clone())).call(method, args),
            "disconnect" => {
                let _ = self.sock.clone().disconnect();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Socket ({})", other, self.methods().join(" "))),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        // socketioxide spawns tokio tasks (acks, disconnects, broadcasts): it
        // must run inside the shared runtime's context, or it panics
        let _rt = crate::native::rt::tokio().enter();
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

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
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

/// A response of the Socket.IO service, its body read (polling answers are short;
/// a WebSocket upgrade has an empty body).
async fn read_response<B: hyper::body::Body<Data = bytes::Bytes>, E>(
    r: Result<hyper::Response<B>, E>,
) -> hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible>> {
    use http_body_util::BodyExt;
    let Ok(resp) = r else {
        return hyper::Response::builder()
            .status(500)
            .body(http_body_util::Full::new(bytes::Bytes::new()).boxed())
            .expect("valid response");
    };
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
    hyper::Response::from_parts(parts, http_body_util::Full::new(bytes).boxed())
}

/// Serves a request of `fogu:web` under the Socket.IO path.
pub async fn serve_http(m: &WebMount, req: hyper::Request<hyper::body::Incoming>) -> hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible>> {
    use hyper::service::Service;
    match &m.svc {
        Svc::Local(s) => read_response(s.call(req).await).await,
        Svc::Redis(s) => read_response(s.call(req).await).await,
    }
}

// ---------------------------------------------------------------------
// client
// ---------------------------------------------------------------------

enum CJob {
    /// `tail`: the last argument may be a recovery offset, left out for a handler
    /// that does not take it
    Event { event: String, args: Vec<serde_json::Value>, tail: bool },
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
    /// the socket's id (the CONNECT packet's `sid`)
    id: Mutex<Option<String>>,
    /// connection state recovery: the private id the server gave, the offset of the
    /// last broadcast received, and whether the last connection got the session back
    pid: Mutex<Option<String>>,
    offset: Mutex<Option<String>>,
    recovered: AtomicBool,
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
    let push = |event: &str, args: Vec<serde_json::Value>| queue.push(CJob::Event { event: event.into(), args, tail: false });
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
                    // a server with recovery gave a pid: it goes back with the last offset
                    let mut auth = d.auth.clone();
                    if let Some(pid) = lock(&shared.pid).clone() {
                        if !auth.is_object() {
                            auth = serde_json::json!({});
                        }
                        auth["pid"] = serde_json::Value::String(pid);
                        if let Some(o) = lock(&shared.offset).clone() {
                            auth["offset"] = serde_json::Value::String(o);
                        }
                    }
                    let auth = if auth.is_null() { String::new() } else { auth.to_string() };
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
                            *lock(&shared.id) = data["sid"].as_str().map(String::from);
                            let pid = data["pid"].as_str().map(String::from);
                            let mut old = lock(&shared.pid);
                            shared.recovered.store(pid.is_some() && *old == pid, Ordering::SeqCst);
                            if *old != pid {
                                *lock(&shared.offset) = None;
                            }
                            *old = pid;
                            drop(old);
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
                            // with recovery, a broadcast's last argument is its offset (as in the
                            // JavaScript client, a string last argument is taken for one)
                            let tail = lock(&shared.pid).is_some() && arr.last().is_some_and(|v| v.is_string());
                            if tail {
                                *lock(&shared.offset) = arr.last().and_then(|v| v.as_str().map(String::from));
                            }
                            if let Some(name) = ev.as_str() {
                                queue.push(CJob::Event { event: name.into(), args: arr, tail });
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
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.to_string(), v.to_display_string())).collect(),
        _ => Vec::new(),
    };
    let timeout = match o.get("timeout") {
        Some(t) => duration_arg(t, "socketio.connect timeout")?,
        None => Duration::from_secs(10),
    };
    // the url's query goes with the handshake (`socket.handshake.query`), as the JavaScript client does
    let url_query = rest.split_once('?').map(|(_, q)| q).filter(|q| !q.is_empty()).map(|q| format!("&{}", q)).unwrap_or_default();
    let dial = Dial {
        ws_url: format!("{}://{}{}/?EIO=4&transport=websocket{}", scheme, hostport, path.trim_end_matches('/'), url_query),
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
        let CJob::Event { event, args, tail } = job;
        let hs = h2.borrow().get(&event).cloned().unwrap_or_default();
        let args: Vec<Value> = args.iter().map(json_to_value).collect();
        for h in hs {
            let takes_all = match &h {
                Value::Fn(f) => f.params.len() >= args.len() || f.params.last().is_some_and(|p| p.starts_with("...")),
                _ => true,
            };
            let args = if tail && !takes_all { args[..args.len() - 1].to_vec() } else { args.clone() };
            run_handler(it, env, h, args);
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
            "id" => Some(lock(&self.shared.id).clone().map(|s| Value::Str(s.into())).unwrap_or(Value::Nil)),
            "recovered" => Some(Value::Bool(self.shared.recovered.load(Ordering::SeqCst))),
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
