//! `fogu:http` module — HTTP server and client with real concurrency.
//!
//! Golden rule: **nothing containing a `Value` crosses a thread**.
//! The channels between the server thread and the main thread only carry
//! `PlainRequest` / `PlainResponse`, which are `Send`.
//!
//! The server thread also does the work that needs no Fogu code: static
//! files (with the SPA fallback), file responses, streamed responses (SSE)
//! and the WebSocket upgrade (bridged to a plain socket for `fogu:websocket`).

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request as HyperRequest, Response as HyperResponse, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched;
use crate::runtime::value::Str;
use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};

// ================= "PLAIN" TYPES (Send) =================

/// HTTP request without `Value` — every field is `Send`.
#[derive(Clone)]
pub struct PlainRequest {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// `Upgrade: websocket`
    pub upgrade: bool,
    pub remote: String,
    /// a static file a middleware guards: the program runs the middlewares
    /// and answers `serve_static` when they let it through
    pub static_guard: bool,
}

/// HTTP response without `Value` — every field is `Send`.
pub struct PlainResponse {
    pub status: i64,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// send this file (read by the server thread)
    pub file: Option<PathBuf>,
    /// streamed body: each item is a chunk; the response ends when the sender is dropped
    pub stream: Option<BodyStream>,
    /// accept a WebSocket upgrade: the plain socket goes through this channel
    pub upgrade: Option<std_mpsc::Sender<crate::native::websocket::WsIo>>,
    /// the middlewares let a guarded static file through: the server thread
    /// serves it (with these headers added)
    pub serve_static: bool,
}

/// The receiving end of a streamed body. `ready` is set (see [`StreamReady`])
/// whenever a chunk was queued or the sending end went away, so a server
/// that sends from the interpreter thread parks behind it instead of asking
/// the channel on every pass.
pub struct BodyStream {
    pub rx: tokio::sync::mpsc::UnboundedReceiver<Bytes>,
    pub ready: sched::Gate,
}

/// The sending end's side of [`BodyStream::ready`]: `signal` after a send;
/// dropping it (the stream ended) signals too.
pub struct StreamReady(sched::Gate);

impl StreamReady {
    pub fn signal(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
        sched::wake();
    }
}

impl Drop for StreamReady {
    fn drop(&mut self) {
        self.signal();
    }
}

/// A channel for a streamed body.
pub fn body_stream() -> (tokio::sync::mpsc::UnboundedSender<Bytes>, std::sync::Arc<StreamReady>, BodyStream) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let ready = sched::Gate::default();
    (tx, std::sync::Arc::new(StreamReady(sched::Gate::clone(&ready))), BodyStream { rx, ready })
}

impl PlainResponse {
    pub fn text(status: i64, body: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".into(), "text/plain; charset=utf-8".into());
        Self { status, headers, body: body.as_bytes().to_vec(), file: None, stream: None, upgrade: None, serve_static: false }
    }
}

/// A folder served by the server thread (`app.static`).
#[derive(Clone, Debug)]
pub struct StaticMount {
    /// URL prefix, e.g. `/` or `/assets`
    pub prefix: String,
    pub dir: PathBuf,
    /// serve `index.html` for unknown HTML routes (React, Vue… builds)
    pub spa: bool,
    pub index: String,
    /// `Cache-Control: max-age` in seconds (`index.html` always revalidates)
    pub max_age: u64,
    pub dotfiles: bool,
}

/// The path prefixes of an app's middlewares ("" for a global one), shared
/// with the server thread: a static file under one of them is not served
/// before the middlewares said so (`app.use("/private", auth)` protects
/// `app.static("/private", dir)` too).
pub type Guards = std::sync::Arc<std::sync::RwLock<Vec<String>>>;

/// `/admin` covers `/admin` and `/admin/...`, not `/administrator`; "" covers everything.
pub fn under_prefix(prefix: &str, path: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty() || path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

/// Server settings decided before `listen`.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub host: String,
    /// `maxBody`: the largest request body (413 beyond it)
    pub max_body: usize,
    /// `maxHeaderBytes`: the request line and headers together (431 beyond it)
    pub max_header: usize,
    /// `headerTimeout`: a client must send a request's headers within this
    /// (also how long an idle keep-alive connection stays open)
    pub header_timeout: Duration,
    /// `readTimeout`: then its whole body within this (408 beyond it)
    pub read_timeout: Duration,
    /// `handlerTimeout`: the program must answer within this (504); `None` = no limit
    pub handler_timeout: Option<Duration>,
    pub statics: Vec<StaticMount>,
    /// the app's middleware prefixes (see [`Guards`])
    pub guards: Guards,
    pub quiet: bool,
    /// Socket.IO served on the same port (`app.listen(port, { socketio: io })`)
    pub socketio: Option<crate::native::socketio::WebMount>,
    /// HTTPS: the `tls: { cert, key }` option (`fogu:tls`)
    pub tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    /// `reusePort`: several listeners share the port (one server per
    /// `sync.Worker`); the kernel spreads the connections over them
    pub reuse_port: bool,
}

impl Default for ServerOptions {
    fn default() -> Self {
        ServerOptions {
            host: "127.0.0.1".into(),
            max_body: 10 * 1024 * 1024,
            max_header: 64 * 1024,
            header_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(60),
            handler_timeout: Some(Duration::from_secs(60)),
            statics: Vec::new(),
            guards: Guards::default(),
            quiet: false,
            socketio: None,
            tls: None,
            reuse_port: false,
        }
    }
}

/// The bytes of a size written as text: `"4096"`, `"512kb"`, `"10 MB"`, `"1gb"`
/// (powers of 1024, any case).
pub(crate) fn size_of(text: &str) -> Option<i64> {
    let t = text.trim().to_ascii_lowercase();
    let digits = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let n: i64 = t[..digits].parse().ok()?;
    let unit = match t[digits..].trim() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        _ => return None,
    };
    n.checked_mul(unit)
}

/// The options `http.serve` and `app.listen` share.
pub const SERVER_OPTIONS: &str = "tls quiet reusePort maxBody maxHeaderBytes headerTimeout readTimeout handlerTimeout";

impl ServerOptions {
    /// Reads one of [`SERVER_OPTIONS`]; false for a key it does not know.
    pub fn set(&mut self, key: &str, v: &Value, what: &str) -> Result<bool, String> {
        // a number of bytes, or a size written out: "512kb", "10mb", "1gb"
        let bytes = |v: &Value, min: i64| {
            let n = match v {
                Value::Int(n) => Some(*n),
                Value::Str(s) => size_of(s),
                _ => None,
            };
            match n {
                Some(n) if n >= min => Ok(n as usize),
                _ => Err(format!("{}: {} must be a number of bytes >= {} (or a size such as \"10mb\"), got {}", what, key, min, v.to_display_string())),
            }
        };
        match key {
            "maxBody" => self.max_body = bytes(v, 1)?,
            // hyper refuses a read buffer under 8 KB
            "maxHeaderBytes" => self.max_header = bytes(v, 8192)?,
            "headerTimeout" | "readTimeout" | "handlerTimeout" => {
                let d = crate::native::util::duration_arg(v, &format!("{}: {}", what, key))?;
                match key {
                    "headerTimeout" if !d.is_zero() => self.header_timeout = d,
                    "readTimeout" if !d.is_zero() => self.read_timeout = d,
                    "handlerTimeout" => self.handler_timeout = (!d.is_zero()).then_some(d),
                    _ => return Err(format!("{}: {} must be more than 0", what, key)),
                }
            }
            "quiet" => self.quiet = v.is_truthy(),
            "reusePort" => match v {
                Value::Bool(b) => self.reuse_port = *b,
                other => return Err(format!("{}: reusePort must be a bool, got {}", what, other.to_display_string())),
            },
            "tls" => {
                if !matches!(v, Value::Nil) {
                    self.tls = Some(crate::native::tls::server_config(v, what, &["http/1.1"])?);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// Where the main thread answers a request (the server task awaits it).
pub type Reply = tokio::sync::oneshot::Sender<PlainResponse>;

/// Message from the server thread to the main thread.
struct ServerToMain {
    /// which server (each `http.serve` / `app.listen` has its own handler)
    server: u64,
    req: PlainRequest,
    reply: Reply,
}

/// Set by a server when it queues a request for its thread: the interpreter
/// checks this flag (one atomic load) instead of draining the channel on
/// every call. One per interpreter thread that has a server — a process with
/// a server per `sync.Worker` must not have them clear each other's flag (a
/// request would wait for the next one to be noticed).
type Waiting = std::sync::Arc<std::sync::atomic::AtomicBool>;

thread_local! {
    static PUMP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// This thread's flag, once it has a server (its request queue lives here).
    static WAITING: RefCell<Option<Waiting>> = const { RefCell::new(None) };
}

/// Marks the pump task as started; true the first time only.
pub fn start_pump() -> bool {
    !PUMP.with(|p| p.replace(true))
}

/// Did the server queue requests since they were last taken?
#[inline]
pub fn requests_waiting() -> bool {
    WAITING.with(|w| w.borrow().as_ref().is_some_and(|f| f.load(std::sync::atomic::Ordering::SeqCst)))
}

// ================= MAIN THREAD GLOBAL STATE =================

thread_local! {
    /// The handler of every server this thread started, by server id: a
    /// second `http.serve` / `app.listen` adds one, it never replaces the first.
    static SERVER_HANDLERS: RefCell<HashMap<u64, Value>> = RefCell::new(HashMap::new());
    static NEXT_SERVER: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    /// One queue for all the servers of this thread: each message names its server.
    static SERVER_TX: RefCell<Option<std_mpsc::Sender<ServerToMain>>> = const { RefCell::new(None) };
    static SERVER_RX: RefCell<Option<std_mpsc::Receiver<ServerToMain>>> = const { RefCell::new(None) };
    static PENDING: RefCell<VecDeque<(u64, PlainRequest, Reply)>> =
        const { RefCell::new(VecDeque::new()) };
}

/// The requests queued so far are about to be read: clear this thread's flag
/// first (a request queued while they are read sets it again).
fn clear_waiting() {
    WAITING.with(|w| {
        if let Some(f) = w.borrow().as_ref() {
            f.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    });
}

// ================= API FOR THE INTERPRETER =================

/// Is a request waiting for the main thread?
pub fn has_pending_request() -> bool {
    clear_waiting();
    SERVER_RX.with(|r| {
        if let Some(rx) = r.borrow().as_ref() {
            while let Ok(msg) = rx.try_recv() {
                PENDING.with(|q| q.borrow_mut().push_back((msg.server, msg.req, msg.reply)));
            }
        }
    });
    PENDING.with(|q| !q.borrow().is_empty())
}

/// Returns the next pending request (its server and the request, already as
/// a `Value::Object(Request)`).
pub fn take_pending_request() -> Option<(u64, Value, Reply)> {
    clear_waiting();
    SERVER_RX.with(|r| {
        let guard = r.borrow();
        if let Some(rx) = guard.as_ref() {
            while let Ok(msg) = rx.try_recv() {
                PENDING.with(|q| q.borrow_mut().push_back((msg.server, msg.req, msg.reply)));
            }
        }
    });
    PENDING.with(|q| {
        let (server, plain, tx) = q.borrow_mut().pop_front()?;
        let value = NativeObjectRef::new(RequestValue {
            method: plain.method,
            path: plain.path,
            query: plain.query,
            headers: plain.headers,
            body: plain.body,
            upgrade: plain.upgrade,
            remote: plain.remote,
            static_guard: plain.static_guard,
        })
        .to_value();
        Some((server, value, tx))
    })
}

/// Entry point for `web.rs`: a server whose handler is a native dispatcher.
pub fn serve_from_native(port: u16, handler: Value, opts: ServerOptions) -> Result<Value, String> {
    start_server(port, handler, opts)
}

/// Where to listen: an int port (on `default_host`) or a string
/// `"ip:port"`, `"host:port"`, `"[::1]:port"`, `":port"` or `"port"`.
pub fn listen_addr(v: Option<&Value>, default_host: &str, what: &str) -> Result<(String, u16), String> {
    let bad = |got: String| format!("{}: expected a port 1..65535 or \"ip:port\", got {}", what, got);
    let port_of = |p: &str| p.parse::<u16>().ok().filter(|p| *p > 0);
    match v {
        Some(Value::Int(p)) if (1..=65535).contains(p) => Ok((default_host.to_string(), *p as u16)),
        Some(Value::Str(s)) => {
            let s = s.trim();
            if let Some(p) = port_of(s) {
                return Ok((default_host.to_string(), p));
            }
            let (host, port) = s.rsplit_once(':').ok_or_else(|| bad(format!("'{}'", s)))?;
            let port = port_of(port).ok_or_else(|| bad(format!("'{}'", s)))?;
            let host = host.trim_start_matches('[').trim_end_matches(']');
            if host.contains(':') && !s.starts_with('[') {
                return Err(bad(format!("'{}' (write IPv6 hosts as [::1]:port)", s)));
            }
            Ok((if host.is_empty() { default_host.to_string() } else { host.to_string() }, port))
        }
        Some(other) => Err(bad(other.to_display_string())),
        None => Err(format!("{} expects a port or \"ip:port\"", what)),
    }
}

/// `host` (an IP or a name) and `port` as a socket address.
fn socket_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    use std::net::ToSocketAddrs;
    (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .ok_or_else(|| format!("serve: invalid host '{}'", host))
}

/// The handler (Fogu function) of a server.
pub fn handler_of(server: u64) -> Option<Value> {
    SERVER_HANDLERS.with(|h| h.borrow().get(&server).cloned())
}

/// Converts a response `Value` into a `PlainResponse` and sends it.
pub fn send_response(resp: Value, tx: Reply) {
    let _ = tx.send(plain_of(resp));
}

/// What the server sends for a response `Value`.
pub fn plain_of(resp: Value) -> PlainResponse {
    match resp {
        Value::Object(o) => {
            if let Some(p) = crate::native::web::take_special(&o) {
                return p;
            }
            let status = match o.get("status") {
                Some(Value::Int(n)) => n,
                _ => 200,
            };
            let body = match o.get("body") {
                Some(Value::Str(s)) => s.as_bytes().to_vec(),
                _ => Vec::new(),
            };
            let headers = match o.get("headers") {
                Some(Value::Map(m)) => m
                    .borrow()
                    .iter()
                    .filter_map(|(k, v)| match v {
                        Value::Str(s) => Some((k.to_string(), s.to_string())),
                        _ => None,
                    })
                    .collect(),
                _ => HashMap::new(),
            };
            PlainResponse { status, headers, body, file: None, stream: None, upgrade: None, serve_static: false }
        }
        _ => PlainResponse::text(500, "invalid response"),
    }
}

/// Native function with 1 required argument and 1 optional one (options).
pub(crate) fn nfn_opts(name: &str, f: fn(&[Value]) -> Result<Value, String>) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.to_string(),
        arity: 1,
        variadic: true,
        func: f,
        intercept: Default::default(),
        typed: None,
    }))
}

/// Reads `(body, options?)` of `text/json/html`: returns the status and the
/// headers (`content_type` + those in `options.headers`). Options: `{ status, headers }`.
pub(crate) fn response_opts(content_type: &str, args: &[Value]) -> Result<(i64, HashMap<String, String>), String> {
    if args.len() > 2 {
        return Err(format!("expects (body, options?), got {} arguments", args.len()));
    }
    let mut status: i64 = 200;
    let mut headers: HashMap<String, String> = HashMap::new();
    headers.insert("content-type".into(), content_type.into());
    match args.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            let m = m.borrow();
            match m.get("status") {
                None | Some(Value::Nil) => {}
                Some(Value::Int(n)) if (100..=599).contains(n) => status = *n,
                Some(other) => {
                    return Err(format!(
                        "status must be an int between 100 and 599, got '{}'",
                        other.to_display_string()
                    ))
                }
            }
            match m.get("headers") {
                None | Some(Value::Nil) => {}
                Some(Value::Map(h)) => {
                    for (k, v) in h.borrow().iter() {
                        headers.insert(k.to_lowercase(), v.to_display_string());
                    }
                }
                Some(other) => return Err(format!("headers must be a map, got '{}'", other.type_name())),
            }
        }
        Some(other) => {
            return Err(format!("response options must be a map {{status, headers}}, got '{}'", other.type_name()))
        }
    }
    Ok((status, headers))
}

fn response(body: String, content_type: &str, args: &[Value]) -> Result<Value, String> {
    let (status, headers) = response_opts(content_type, args)?;
    Ok(NativeObjectRef::new(ResponseValue { status, headers, body }).to_value())
}

// ================= MODULE =================

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    // text/json/html(body, options?) — options: { status: 404, headers: {...} }
    exports.insert(
        "text".into(),
        nfn_opts("text", |args| {
            let body = match &args[0] {
                Value::Str(s) => s.clone(),
                other => other.to_display_string().into(),
            };
            response(body.into_string(), "text/plain; charset=utf-8", args)
        }),
    );
    exports.insert(
        "json".into(),
        nfn_opts("json", |args| {
            let body = crate::native::json::stringify_value(&args[0]);
            response(body, "application/json", args)
        }),
    );
    exports.insert(
        "html".into(),
        nfn_opts("html", |args| {
            let body = match &args[0] {
                Value::Str(s) => s.clone(),
                other => return Err(format!("html expects str, got '{}'", other.type_name())),
            };
            response(body.into_string(), "text/html; charset=utf-8", args)
        }),
    );
    // the client (http_client.rs): get/delete/head(url, opts?) · post/put/patch(url, body?, opts?)
    use crate::native::http_client as hc;
    exports.insert("get".into(), crate::native::util::nfn_var("get", 1, |a| hc::no_body("GET", a)));
    exports.insert("delete".into(), crate::native::util::nfn_var("delete", 1, |a| hc::no_body("DELETE", a)));
    exports.insert("head".into(), crate::native::util::nfn_var("head", 1, |a| hc::no_body("HEAD", a)));
    exports.insert("post".into(), crate::native::util::nfn_var("post", 1, |a| hc::with_body("POST", a)));
    exports.insert("put".into(), crate::native::util::nfn_var("put", 1, |a| hc::with_body("PUT", a)));
    exports.insert("patch".into(), crate::native::util::nfn_var("patch", 1, |a| hc::with_body("PATCH", a)));
    exports.insert("request".into(), crate::native::util::nfn_var("request", 1, hc::request));
    exports.insert("download".into(), crate::native::util::nfn_var("download", 2, hc::download));
    exports.insert("stream".into(), crate::native::util::nfn_var("stream", 1, hc::stream));
    exports.insert("file".into(), crate::native::util::nfn_var("file", 1, hc::file));
    exports.insert(
        "serve".into(),
        // serve(8080 | "ip:port", handler, { tls: { cert, key }, quiet, maxBody }?)
        crate::native::util::nfn_var("serve", 2, |args| {
            crate::native::util::max_args(args, 3, "serve")?;
            let mut opts = ServerOptions::default();
            match args.get(2) {
                None | Some(Value::Nil) => {}
                Some(Value::Map(o)) => {
                    let o = o.borrow();
                    let mut keys: Vec<&Str> = o.keys().collect();
                    keys.sort();
                    for k in keys {
                        let v = &o[k];
                        if matches!(v, Value::Nil) {
                            continue;
                        }
                        if !opts.set(k, v, "serve")? {
                            return Err(format!("serve: unknown option '{}' ({})", k, SERVER_OPTIONS));
                        }
                    }
                }
                Some(other) => return Err(format!("serve: options must be a map, got '{}'", other.type_name())),
            }
            let (host, port) = listen_addr(args.first(), &opts.host, "serve")?;
            opts.host = host;
            start_server(port, args[1].clone(), opts)
        }),
    );
    ModuleValue {
        name: "fogu:http".into(),
        exports: RefCell::new(exports),
    }
}

/// Is an HTTP server running in this program (its requests are served by calls)?
pub fn serving() -> bool {
    SERVER_HANDLERS.with(|h| !h.borrow().is_empty())
}

// ================= SERVER =================

type Body = BoxBody<Bytes, Infallible>;

fn start_server(port: u16, handler: Value, opts: ServerOptions) -> Result<Value, String> {
    match &handler {
        Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) => {}
        other => return Err(format!("serve: handler must be a function, got '{}'", other.type_name())),
    }
    // bind here, so a busy port is reported to the program
    let addr = socket_addr(&opts.host, port)?;
    let std_listener = if opts.reuse_port {
        // one listener per worker on the same port: the kernel spreads the connections
        #[cfg(unix)]
        {
            let _runtime = crate::native::rt::tokio().enter();
            let bind = || -> std::io::Result<std::net::TcpListener> {
                let sock = if addr.is_ipv4() { tokio::net::TcpSocket::new_v4()? } else { tokio::net::TcpSocket::new_v6()? };
                sock.set_reuseaddr(true)?;
                sock.set_reuseport(true)?;
                sock.bind(addr)?;
                sock.listen(1024)?.into_std()
            };
            bind().map_err(|e| io_err!("http: cannot listen on {}: {}", addr, e))?
        }
        #[cfg(not(unix))]
        return Err(io_err!("http: reusePort is not available on this system"));
    } else {
        std::net::TcpListener::bind(addr).map_err(|e| io_err!("http: cannot listen on {}: {}", addr, e))?
    };
    std_listener.set_nonblocking(true).map_err(|e| io_err!("http: {}", e))?;

    let server = NEXT_SERVER.with(|n| n.replace(n.get() + 1));
    SERVER_HANDLERS.with(|h| h.borrow_mut().insert(server, handler));
    // plain HTTP on Linux is served by the interpreter thread itself, on its
    // reactor (see `http_direct`): no other thread touches a request
    #[cfg(target_os = "linux")]
    if crate::native::http_direct::eligible(&opts) {
        if !opts.quiet {
            println!("[http] server listening on http://{}", addr);
        }
        crate::native::http_direct::queue(server, std_listener, opts);
        // the interpreter's next call notices the flag and starts the server's task
        let waiting: Waiting = WAITING.with(|w| std::sync::Arc::clone(w.borrow_mut().get_or_insert_with(Default::default)));
        waiting.store(true, std::sync::atomic::Ordering::SeqCst);
        return Ok(Value::Nil);
    }
    // the first server creates this thread's queue; the next ones share it
    let tx_to_main = SERVER_TX.with(|t| {
        t.borrow_mut()
            .get_or_insert_with(|| {
                let (tx, rx) = std_mpsc::channel::<ServerToMain>();
                SERVER_RX.with(|r| *r.borrow_mut() = Some(rx));
                tx
            })
            .clone()
    });
    // the interpreter's next call notices the flag and starts the pump task
    let waiting: Waiting = WAITING.with(|w| std::sync::Arc::clone(w.borrow_mut().get_or_insert_with(Default::default)));
    waiting.store(true, std::sync::atomic::Ordering::SeqCst);

    // the server runs on the shared I/O runtime
    {
        crate::native::rt::tokio().spawn(async move {
            let listener = match TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("http.serve: {}", e);
                    return;
                }
            };
            if !opts.quiet {
                println!("[http] server listening on {}://{}", if opts.tls.is_some() { "https" } else { "http" }, addr);
            }
            let tls = opts.tls.clone().map(tokio_rustls::TlsAcceptor::from);
            let opts = std::sync::Arc::new(opts);
            loop {
                let (stream, remote) = match listener.accept().await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("http.serve: accept: {}", e);
                        continue;
                    }
                };
                let tx = tx_to_main.clone();
                let opts = std::sync::Arc::clone(&opts);
                let tls = tls.clone();
                let waiting = std::sync::Arc::clone(&waiting);
                tokio::spawn(async move {
                    // slow clients (slowloris) cannot hold a connection: the
                    // headers of each request must arrive within headerTimeout
                    // (which also closes idle keep-alive connections), and
                    // they are limited to maxHeaderBytes (431)
                    let mut builder = http1::Builder::new();
                    builder.timer(hyper_util::rt::TokioTimer::new()).header_read_timeout(opts.header_timeout).max_buf_size(opts.max_header);
                    let waiting = std::sync::Arc::clone(&waiting);
                    let service = service_fn(move |req| handle_request(req, server, tx.clone(), std::sync::Arc::clone(&opts), remote, std::sync::Arc::clone(&waiting)));
                    match tls {
                        // HTTPS: the handshake first (a client that stalls is dropped after 10s)
                        Some(acceptor) => {
                            let Ok(Ok(s)) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await else { return };
                            let _ = builder.serve_connection(TokioIo::new(s), service).with_upgrades().await;
                        }
                        None => {
                            let _ = builder.serve_connection(TokioIo::new(stream), service).with_upgrades().await;
                        }
                    }
                });
            }
        });
    }
    Ok(Value::Nil)
}

fn full(b: impl Into<Bytes>) -> Body {
    Full::new(b.into()).boxed()
}

/// The canonical form of a request path, computed once before anything
/// (static files, Socket.IO, middlewares, the router, the handler) looks at
/// it, so they all decide on the same resource:
///
/// - percent escapes are decoded exactly once (`%252F` becomes the literal
///   text `%2F`, never `/`); `+` stays `+` (it means a space only in queries);
/// - an escaped `/` or `\` (`%2F`, `%5C`, any case), a raw `\`, a NUL or
///   another control character, an invalid escape and bytes that are not
///   UTF-8 once decoded make the request invalid (400): a segment can never
///   smuggle a separator;
/// - repeated slashes collapse, `.` segments go away and `..` removes the
///   previous segment (also when written `%2e%2e`); a `..` that would climb
///   above the root makes the request invalid;
/// - the result starts with `/` and keeps a trailing `/` if the request had one.
pub fn canonical_path(raw: &str) -> Result<String, &'static str> {
    let b = raw.as_bytes();
    // fast path: nothing to decode or collapse
    let plain = b.first() == Some(&b'/')
        && !b.iter().any(|&c| c == b'%' || c == b'\\' || c < 0x20 || c == 0x7f)
        && !raw.contains("//")
        && !raw.split('/').any(|s| s == "." || s == "..");
    if plain {
        return Ok(raw.to_string());
    }
    let mut decoded: Vec<u8> = Vec::with_capacity(b.len() + 1);
    if b.first() != Some(&b'/') {
        decoded.push(b'/');
    }
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%' {
            let (Some(h), Some(l)) = (b.get(i + 1).and_then(|&x| hex(x)), b.get(i + 2).and_then(|&x| hex(x))) else {
                return Err("invalid percent escape in the path");
            };
            let v = h * 16 + l;
            if v == b'/' || v == b'\\' {
                return Err("encoded '/' or '\\' in the path");
            }
            if v < 0x20 || v == 0x7f {
                return Err("control character in the path");
            }
            decoded.push(v);
            i += 3;
            continue;
        }
        if c == b'\\' {
            return Err("'\\' in the path");
        }
        if c < 0x20 || c == 0x7f {
            return Err("control character in the path");
        }
        decoded.push(c);
        i += 1;
    }
    let text = String::from_utf8(decoded).map_err(|_| "the path is not UTF-8")?;
    let mut out: Vec<&str> = Vec::new();
    for seg in text.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if out.pop().is_none() {
                    return Err("the path climbs above the root");
                }
            }
            s => out.push(s),
        }
    }
    let mut path = String::with_capacity(text.len());
    for s in &out {
        path.push('/');
        path.push_str(s);
    }
    let trailing = text.ends_with('/') || text.ends_with("/.") || text.ends_with("/..");
    if path.is_empty() || trailing {
        path.push('/');
    }
    Ok(path)
}

async fn handle_request(
    mut req: HyperRequest<Incoming>,
    server: u64,
    tx: std_mpsc::Sender<ServerToMain>,
    opts: std::sync::Arc<ServerOptions>,
    remote: SocketAddr,
    waiting: Waiting,
) -> Result<HyperResponse<Body>, Infallible> {
    let method = req.method().to_string();
    // one canonical path for everything below (see `canonical_path`)
    let path = match canonical_path(req.uri().path()) {
        Ok(p) => p,
        Err(why) => return Ok(plain_response(PlainResponse::text(400, &format!("bad request: {}", why))).await),
    };
    // Socket.IO (polling and the WebSocket upgrade) never reaches the program
    if let Some(sio) = &opts.socketio {
        if path == sio.path || path.starts_with(&format!("{}/", sio.path)) {
            return Ok(crate::native::socketio::serve_http(sio, req).await);
        }
    }
    let query = req.uri().query().map(|q| q.to_string());
    let headers: HashMap<String, String> = req
        .headers()
        .iter()
        // values with bytes >= 0x80 are decoded as UTF-8 (lossy), as the client does
        .map(|(n, v)| (n.to_string(), v.to_str().map(str::to_string).unwrap_or_else(|_| String::from_utf8_lossy(v.as_bytes()).into_owned())))
        .collect();
    let upgrade = headers.get("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));

    // static files never reach the program, unless a middleware guards them
    let mut static_guard = false;
    if !upgrade && (method == "GET" || method == "HEAD") && !opts.statics.is_empty() {
        let guarded = {
            let g = opts.guards.read().unwrap_or_else(|e| e.into_inner());
            !g.is_empty() && g.iter().any(|p| under_prefix(p, &path))
        };
        if !guarded {
            if let Some(r) = serve_static(&opts.statics, &path, &headers, method == "HEAD", false).await {
                return Ok(r);
            }
        } else {
            static_guard = opts.statics.iter().any(|m| resolve_static(m, &path).is_some());
        }
    }

    let on_upgrade = if upgrade { Some(hyper::upgrade::on(&mut req)) } else { None };
    let body = match tokio::time::timeout(opts.read_timeout, Limited::new(req.into_body(), opts.max_body).collect()).await {
        Ok(Ok(c)) => c.to_bytes().to_vec(),
        Ok(Err(_)) => return Ok(plain_response(PlainResponse::text(413, "request body too large")).await),
        Err(_) => return Ok(plain_response(PlainResponse::text(408, "request timeout")).await),
    };
    // what the answer may still need of the request: the headers for a static
    // file or a WebSocket accept — kept only when the server has static
    // mounts or the request asks for an upgrade (not one copy per request)
    let accepts_html = method == "GET" && headers.get("accept").is_some_and(|a| a.contains("text/html"));
    // (a file answer may be asked for in part)
    let range = headers.get("range").map(|r| (r.clone(), headers.get("if-range").cloned()));
    let kept = if upgrade || !opts.statics.is_empty() { headers.clone() } else { HashMap::new() };
    let plain = PlainRequest {
        method: method.clone(),
        path: path.clone(),
        query,
        headers,
        body,
        upgrade,
        remote: remote.to_string(),
        static_guard,
    };
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<PlainResponse>();
    if tx.send(ServerToMain { server, req: plain, reply: reply_tx }).is_err() {
        return Ok(plain_response(PlainResponse::text(500, "server offline")).await);
    }
    // SeqCst on both sides: the flag is cleared *before* the queue is read, so
    // a request queued while it is read always leaves the flag set
    waiting.store(true, std::sync::atomic::Ordering::SeqCst);
    // a main thread idling in the scheduler (Fogu.signal.wait) serves it now
    crate::runtime::sched::wake();
    // the Fogu handler runs on the main thread; this task just awaits the reply
    let answer = async {
        match opts.handler_timeout {
            Some(t) => tokio::time::timeout(t, reply_rx).await.map_err(|_| ()),
            None => Ok(reply_rx.await),
        }
    };
    let resp = match answer.await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => PlainResponse::text(500, "internal error"),
        Err(_) => PlainResponse::text(504, "timeout"),
    };

    // a guarded static file the middlewares let through
    if resp.serve_static {
        if let Some(mut r) = serve_static(&opts.statics, &path, &kept, method == "HEAD", false).await {
            for (k, v) in &resp.headers {
                if let (Ok(k), Ok(v)) = (hyper::header::HeaderName::from_bytes(k.as_bytes()), hyper::header::HeaderValue::from_str(v)) {
                    r.headers_mut().insert(k, v);
                }
            }
            return Ok(r);
        }
        return Ok(plain_response(PlainResponse::text(404, "not found")).await);
    }

    // WebSocket: answer 101, then bridge the upgraded connection
    if let (Some(sock_tx), Some(on_upgrade)) = (resp.upgrade.as_ref().cloned(), on_upgrade) {
        if resp.status == 101 {
            let key = kept.get("sec-websocket-key").cloned().unwrap_or_default();
            let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
            let mut b = HyperResponse::builder()
                .status(StatusCode::SWITCHING_PROTOCOLS)
                .header("upgrade", "websocket")
                .header("connection", "Upgrade")
                .header("sec-websocket-accept", accept);
            if let Some(p) = resp.headers.get("sec-websocket-protocol") {
                b = b.header("sec-websocket-protocol", p.as_str());
            }
            // the upgraded connection itself goes to the WebSocket task (fogu:websocket)
            tokio::spawn(async move {
                // the socket (or the closed channel of a failed upgrade) is
                // visible before the waiting task is woken
                match on_upgrade.await {
                    Ok(upgraded) => {
                        let _ = sock_tx.send(Box::new(TokioIo::new(upgraded)));
                    }
                    Err(_) => drop(sock_tx),
                }
                crate::runtime::sched::wake();
            });
            return Ok(b.body(full(Bytes::new())).unwrap());
        }
    }

    // SPA fallback: an HTML page the program does not know
    if resp.status == 404 && accepts_html {
        if let Some(r) = serve_static(&opts.statics, &path, &kept, false, true).await {
            return Ok(r);
        }
    }
    if resp.file.is_some() {
        return Ok(file_response(resp, range.as_ref().map(|(r, i)| (r.as_str(), i.as_deref())), method == "HEAD").await);
    }
    Ok(plain_response(resp).await)
}

/// What of a file a request asks for (RFC 7233; several ranges at once are
/// answered with the whole file, which the RFC allows).
#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum Slice {
    Whole,
    /// first and last byte
    Part(u64, u64),
    /// nothing of the file is in the range (416)
    Outside,
}

pub(crate) fn file_slice(range: Option<(&str, Option<&str>)>, len: u64, etag: &str, modified: &str) -> Slice {
    let Some((range, if_range)) = range else { return Slice::Whole };
    // `If-Range`: the range only counts for the version the client has
    if if_range.is_some_and(|v| v.trim() != etag && v.trim() != modified) {
        return Slice::Whole;
    }
    let Some(spec) = range.trim().strip_prefix("bytes=") else { return Slice::Whole };
    let Some((a, b)) = spec.trim().split_once('-') else { return Slice::Whole };
    let (a, b) = (a.trim(), b.trim());
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
    if a.is_empty() {
        // the last `b` bytes
        if !digits(b) {
            return Slice::Whole;
        }
        return match b.parse::<u64>() {
            Ok(0) => Slice::Outside,
            Ok(n) if len > 0 => Slice::Part(len.saturating_sub(n), len - 1),
            Ok(_) => Slice::Outside,
            Err(_) => Slice::Whole,
        };
    }
    if !digits(a) || !(b.is_empty() || digits(b)) {
        return Slice::Whole;
    }
    let Ok(first) = a.parse::<u64>() else { return Slice::Outside };
    if first >= len {
        return Slice::Outside;
    }
    let last = if b.is_empty() { len - 1 } else { b.parse::<u64>().unwrap_or(u64::MAX).min(len - 1) };
    if last < first {
        return Slice::Whole;
    }
    Slice::Part(first, last)
}

/// The validators of a file answer: (etag, last-modified).
pub(crate) fn file_validators(meta: &std::fs::Metadata) -> (String, String) {
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    (format!("\"{:x}-{:x}\"", meta.len(), mtime), httpdate(mtime))
}

/// The status, the headers a file answer adds and the bytes to send (first, count).
pub(crate) fn file_plan(status: i64, headers: &mut HashMap<String, String>, range: Option<(&str, Option<&str>)>, meta: &std::fs::Metadata) -> (i64, u64, u64) {
    let len = meta.len();
    // (an answer with a status of its own — an error page — is sent whole)
    if status != 200 {
        return (status, 0, len);
    }
    let (etag, modified) = file_validators(meta);
    let slice = file_slice(range, len, headers.get("etag").map_or(etag.as_str(), |e| e.as_str()), &modified);
    headers.entry("accept-ranges".into()).or_insert_with(|| "bytes".into());
    headers.entry("etag".into()).or_insert(etag);
    headers.entry("last-modified".into()).or_insert(modified);
    match slice {
        Slice::Whole => (200, 0, len),
        Slice::Part(a, b) => {
            headers.insert("content-range".into(), format!("bytes {}-{}/{}", a, b, len));
            (206, a, b - a + 1)
        }
        Slice::Outside => {
            headers.insert("content-range".into(), format!("bytes */{}", len));
            (416, 0, 0)
        }
    }
}

/// `web.file`: the file, or the part of it the request asks for, a chunk at a
/// time (64 KiB): a large file does not have to fit in memory.
async fn file_response(mut resp: PlainResponse, range: Option<(&str, Option<&str>)>, head: bool) -> HyperResponse<Body> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let not_found = || HyperResponse::builder().status(StatusCode::NOT_FOUND).header("content-type", "text/plain; charset=utf-8").body(full("file not found")).unwrap();
    let Some(path) = resp.file.take() else { return not_found() };
    let Ok(mut file) = tokio::fs::File::open(&path).await else { return not_found() };
    let Ok(meta) = file.metadata().await else { return not_found() };
    if meta.is_dir() {
        return not_found();
    }
    let (status, first, count) = file_plan(resp.status, &mut resp.headers, range, &meta);
    let mut builder = HyperResponse::builder().status(StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK));
    for (k, v) in &resp.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    if !resp.headers.contains_key("content-type") {
        builder = builder.header("content-type", mime_of(&path));
    }
    builder = builder.header("content-length", count);
    if head || count == 0 || (first > 0 && file.seek(std::io::SeekFrom::Start(first)).await.is_err()) {
        return builder.body(full(Bytes::new())).unwrap_or_else(|_| HyperResponse::new(full(Bytes::new())));
    }
    let chunks = futures_lite::stream::unfold((file, count), |(mut file, left)| async move {
        if left == 0 {
            return None;
        }
        let mut buf = vec![0u8; left.min(64 * 1024) as usize];
        match file.read(&mut buf).await {
            Ok(0) | Err(_) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, Infallible>(Frame::data(Bytes::from(buf))), (file, left - n as u64)))
            }
        }
    });
    builder.body(BodyExt::boxed(StreamBody::new(chunks))).unwrap_or_else(|_| HyperResponse::new(full(Bytes::new())))
}

async fn plain_response(resp: PlainResponse) -> HyperResponse<Body> {
    let code = StatusCode::from_u16(resp.status as u16).unwrap_or(StatusCode::OK);
    let mut builder = HyperResponse::builder().status(code);
    for (k, v) in &resp.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let body = if let Some(path) = &resp.file {
        // (file answers go through `file_response`; this is the same file, whole)
        match tokio::fs::read(path).await {
            Ok(data) => {
                if !resp.headers.contains_key("content-type") {
                    builder = builder.header("content-type", mime_of(path));
                }
                full(data)
            }
            Err(_) => {
                return HyperResponse::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(full("file not found"))
                    .unwrap();
            }
        }
    } else if let Some(body) = resp.stream {
        let stream = futures_lite::stream::unfold(body.rx, |mut rx| async move {
            rx.recv().await.map(|b| (Ok::<_, Infallible>(Frame::data(b)), rx))
        });
        BodyExt::boxed(StreamBody::new(stream))
    } else {
        full(resp.body)
    };
    builder.body(body).unwrap_or_else(|_| HyperResponse::new(full(Bytes::new())))
}

// ================= STATIC FILES =================

pub fn mime_of(p: &Path) -> &'static str {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// A file of a mount for a URL path (never outside the folder). `path` is
/// already canonical (`canonical_path`: decoded once, no `..`): it is not
/// decoded again.
fn resolve_static(m: &StaticMount, path: &str) -> Option<PathBuf> {
    let prefix = m.prefix.trim_end_matches('/');
    let rest = if prefix.is_empty() {
        path
    } else if path == prefix {
        "/"
    } else {
        path.strip_prefix(prefix).filter(|r| r.starts_with('/'))?
    };
    let mut out = m.dir.clone();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.starts_with('.') && !m.dotfiles => return None,
            p if p.contains('\\') || p.contains(':') => return None,
            p => out.push(p),
        }
    }
    if out.is_dir() {
        out.push(&m.index);
    }
    if !out.is_file() {
        return None;
    }
    // the name asked for is not always the file's real name: a Windows 8.3
    // short name (`ENV~1` for `.env`) or a symlink can reach a dotfile or
    // leave the folder, so the check is repeated on the real path
    if !m.dotfiles {
        let real = out.canonicalize().ok()?;
        let inside = real.strip_prefix(&m.dir).ok()?;
        if inside.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
            return None;
        }
    }
    Some(out)
}

async fn serve_static(mounts: &[StaticMount], path: &str, headers: &HashMap<String, String>, head: bool, spa_only: bool) -> Option<HyperResponse<Body>> {
    for m in mounts {
        let file = if spa_only {
            if !m.spa {
                continue;
            }
            let prefix = m.prefix.trim_end_matches('/');
            if !prefix.is_empty() && path != prefix && !path.starts_with(&format!("{}/", prefix)) {
                continue;
            }
            // a missing asset (with an extension) is a real 404
            let last = path.rsplit('/').next().unwrap_or("");
            if last.contains('.') {
                continue;
            }
            let index = m.dir.join(&m.index);
            if !index.is_file() {
                continue;
            }
            index
        } else {
            match resolve_static(m, path) {
                Some(f) => f,
                None => continue,
            }
        };
        let meta = tokio::fs::metadata(&file).await.ok()?;
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let etag = format!("\"{:x}-{:x}\"", meta.len(), mtime);
        let is_html = file.extension().is_some_and(|e| e == "html" || e == "htm");
        let cache = if is_html || m.max_age == 0 { "no-cache".to_string() } else { format!("public, max-age={}", m.max_age) };
        let base = HyperResponse::builder()
            .header("etag", etag.as_str())
            .header("cache-control", cache)
            .header("last-modified", httpdate(mtime))
            .header("x-content-type-options", "nosniff");
        if headers.get("if-none-match").is_some_and(|v| v.split(',').any(|t| t.trim() == etag)) {
            return Some(base.status(StatusCode::NOT_MODIFIED).body(full(Bytes::new())).unwrap());
        }
        let base = base.header("content-type", mime_of(&file)).header("accept-ranges", "bytes");
        let range = headers.get("range").map(|r| (r.as_str(), headers.get("if-range").map(String::as_str)));
        let (first, count) = match file_slice(range, meta.len(), &etag, &httpdate(mtime)) {
            Slice::Whole => (0, meta.len()),
            Slice::Part(a, b) => {
                let builder = base.status(StatusCode::PARTIAL_CONTENT).header("content-range", format!("bytes {}-{}/{}", a, b, meta.len())).header("content-length", b - a + 1);
                if head {
                    return Some(builder.body(full(Bytes::new())).unwrap());
                }
                use tokio::io::{AsyncReadExt, AsyncSeekExt};
                let mut f = tokio::fs::File::open(&file).await.ok()?;
                f.seek(std::io::SeekFrom::Start(a)).await.ok()?;
                let mut data = Vec::with_capacity((b - a + 1) as usize);
                f.take(b - a + 1).read_to_end(&mut data).await.ok()?;
                return Some(builder.body(full(data)).unwrap());
            }
            Slice::Outside => {
                return Some(base.status(StatusCode::RANGE_NOT_SATISFIABLE).header("content-range", format!("bytes */{}", meta.len())).header("content-length", 0).body(full(Bytes::new())).unwrap());
            }
        };
        let _ = first;
        let builder = base.status(StatusCode::OK).header("content-length", count);
        if head {
            return Some(builder.body(full(Bytes::new())).unwrap());
        }
        let data = tokio::fs::read(&file).await.ok()?;
        return Some(builder.body(full(data)).unwrap());
    }
    None
}

/// RFC 7231 date (`Tue, 15 Nov 1994 08:12:31 GMT`).
pub(crate) fn httpdate(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days.rem_euclid(7)) as usize];
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m - 1) as usize];
    format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT", weekday, d, month, y, rem / 3600, rem % 3600 / 60, rem % 60)
}

// ================= CLIENT =================

pub use crate::native::util::{blocking_client, error_chain};

/// Answers the requests the server queued (called before interpreter calls
/// and by `Fogu.signal.wait`). The handlers
/// run here, on the interpreter thread, each in its own task: one that
/// waits (a query, an HTTP call) does not hold the others.
pub fn serve_pending(it: &Interpreter, env: &Rc<RefCell<Environment>>) {
    // The first time: a task that serves requests as soon as the server
    // queues them (it wakes the scheduler), even while the program waits
    // in `time.delay` or on a channel. A running server is waiting for
    // work, not deadlocked.
    #[cfg(target_os = "linux")]
    crate::native::http_direct::start_queued(it, env);
    // (only servers on the I/O threads queue requests: the others need no pump)
    if SERVER_TX.with(|t| t.borrow().is_some()) && start_pump() {
        let env = env.clone();
        let started = it.spawn_task(Box::new(move |it| {
            let _serving = sched::IoGuard::new();
            loop {
                sched::wait_until(requests_waiting)?;
                serve_pending(it, &env);
            }
        }));
        if let Err(e) = started {
            eprintln!("[http] cannot start the server task: {}", e);
        }
    }
    if !SERVER_TX.with(|t| t.borrow().is_some()) {
        clear_waiting();
        return;
    }
    let text = |code, msg: &str| NativeObjectRef::new(ResponseValue::text(code, msg)).to_value();
    while let Some((server, req, tx)) = take_pending_request() {
        // nobody waits for the answer any more (the client left, or the server
        // already answered 504): running the handler would only add load
        if tx.is_closed() {
            continue;
        }
        let Some(handler) = handler_of(server) else {
            send_response(text(500, "internal error"), tx);
            continue;
        };
        let env = env.clone();
        let task = it.spawn_task(Box::new(move |it| {
            send_response(answer(it, handler, req, env), tx);
            Ok(())
        }));
        if let Err(e) = task {
            eprintln!("[http] cannot start a handler: {}", e);
        }
    }
}

/// Runs a request's handler and turns what it returns into a response value.
pub fn answer(it: &Interpreter, handler: Value, req: Value, env: Rc<RefCell<Environment>>) -> Value {
    let text = |code, msg: &str| NativeObjectRef::new(ResponseValue::text(code, msg)).to_value();
    // "GET /path", for the log: built only when something is logged
    let logged = req.clone();
    let what = move || match &logged {
        Value::Object(o) => {
            let f = |k| o.get(k).map(|v| v.to_display_string()).unwrap_or_default();
            format!("{} {}", f("method"), f("path"))
        }
        _ => String::new(),
    };
    match call_contained(it, handler, vec![req], env) {
        // Handler without an explicit response.
        Ok(Value::Nil) => text(404, "route not found"),
        // `return value, error(...)` or an `error` value: logged, never sent
        Ok(Value::Error(e)) => {
            eprintln!("[http] {}: {}", what(), e.message);
            text(500, "internal error")
        }
        Ok(Value::List(l)) if l.borrow().len() == 2 && matches!(l.borrow()[1], Value::Error(_)) => {
            let msg = match &l.borrow()[1] {
                Value::Error(e) => e.message.clone(),
                _ => String::new(),
            };
            eprintln!("[http] {}: {}", what(), msg);
            text(500, "internal error")
        }
        // a str is text, other values JSON (as in fogu:web)
        Ok(Value::Str(s)) => text(200, &s),
        Ok(v @ Value::Object(_)) => v,
        Ok(v) => {
            let mut r = ResponseValue::text(200, &crate::native::json::stringify_value(&v));
            r.headers.insert("content-type".into(), "application/json".into());
            NativeObjectRef::new(r).to_value()
        }
        // the message (a runtime error, a panic) may hold internal
        // details: it goes to the log, the client gets a generic 500
        Err(e) => {
            eprintln!("[http] {}: {}", what(), e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(&e));
            text(500, "internal error")
        }
    }
}

/// Calls a request handler and contains a Rust panic in it: the panic
/// becomes `Err` (the request gets a 500) instead of unwinding through the
/// scheduler and killing the server. This is containment for panics nobody
/// knows about yet, not the fix of a known one. What a handler shares with
/// others stays usable: `RefCell` borrows are released while unwinding, and
/// the scheduler restores the interpreter's position at the next switch.
pub fn call_contained(it: &Interpreter, f: Value, args: Vec<Value>, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.call_value(f, args, env))) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".into());
            Err(format!("internal panic: {}", msg))
        }
    }
}

// ================= FOGU TYPES =================

pub struct RequestValue {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub upgrade: bool,
    pub remote: String,
    /// see [`PlainRequest::static_guard`]
    pub static_guard: bool,
}

impl FoguObject for RequestValue {
    fn name(&self) -> &str {
        "Request"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "method" => Some(Value::Str(self.method.clone().into())),
            "path" => Some(Value::Str(self.path.clone().into())),
            "query" => Some(self.query.clone().map_or(Value::Nil, Value::str)),
            "body" => Some(Value::Str(String::from_utf8_lossy(&self.body).into_owned().into())),
            "size" => Some(Value::Int(self.body.len() as i64)),
            "upgrade" => Some(Value::Bool(self.upgrade)),
            "remote" => Some(Value::Str(self.remote.clone().into())),
            "headers" => {
                let m: HashMap<Str, Value> = self.headers.iter().map(|(k, v)| (k.as_str().into(), Value::Str(v.clone().into()))).collect();
                Some(Value::Map(Rc::new(RefCell::new(m))))
            }
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "header" => {
                if args.len() != 1 {
                    return Err("header(name) expects 1 argument".into());
                }
                match &args[0] {
                    Value::Str(k) => Ok(self.headers.get(&k.to_lowercase()).cloned().map(Value::str).unwrap_or(Value::Nil)),
                    other => Err(format!("header expects str, got '{}'", other.type_name())),
                }
            }
            "json" => crate::native::json::parse_value(&String::from_utf8_lossy(&self.body)).map_err(|e| io_err!("json: {}", e)),
            "bytes" => Ok(crate::native::util::list(self.body.iter().map(|b| Value::Int(*b as i64)).collect())),
            // form() → { field: value | UploadedFile | [..] } for multipart and urlencoded bodies
            "form" => {
                let ct = self.headers.get("content-type").cloned().unwrap_or_default();
                form_value(&ct, &self.body)
            }
            other => Err(format!("method '{}' does not exist on Request (header json bytes form)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["header", "json", "bytes", "form"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ================= FORMS AND UPLOADS =================

#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// `name="x"; filename="a.txt"` parameters of a Content-Disposition header.
fn disposition_param(header: &str, key: &str) -> Option<String> {
    // compared byte by byte, ignoring ASCII case only: lowercasing the whole
    // header changes the length of some characters (`İ`), and the indices
    // found in the lowercase copy would not be valid in the original
    let (h, k) = (header.as_bytes(), key.as_bytes());
    let n = k.len() + 1;
    let mut start = 0;
    while start + n <= h.len() {
        let hit = h[start..start + k.len()].eq_ignore_ascii_case(k) && h[start + k.len()] == b'=';
        // must be a whole parameter name
        if hit && (start == 0 || matches!(h[start - 1], b';' | b' ' | b'\t')) {
            // `key=` is ASCII, so `start + n` is a char boundary
            let v = &header[start + n..];
            return Some(if let Some(rest) = v.strip_prefix('"') {
                rest.split('"').next().unwrap_or("").to_string()
            } else {
                v.split(';').next().unwrap_or("").trim().to_string()
            });
        }
        start += 1;
    }
    None
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<Part>, String> {
    let delim = format!("--{}", boundary).into_bytes();
    let mut parts = Vec::new();
    let mut pos = find(body, &delim, 0).ok_or("form: multipart boundary not found")? + delim.len();
    loop {
        if body.get(pos..pos + 2) == Some(b"--") {
            break;
        }
        // skip the CRLF after the boundary
        if body.get(pos..pos + 2) == Some(b"\r\n") {
            pos += 2;
        }
        let head_end = find(body, b"\r\n\r\n", pos).ok_or("form: malformed part headers")?;
        let head = String::from_utf8_lossy(&body[pos..head_end]).to_string();
        let data_start = head_end + 4;
        let next = find(body, &[b"\r\n".as_slice(), &delim].concat(), data_start).ok_or("form: unterminated part")?;
        let mut name = String::new();
        let mut filename = None;
        let mut content_type = None;
        for line in head.split("\r\n") {
            let (k, v) = line.split_once(':').unwrap_or((line, ""));
            match k.trim().to_lowercase().as_str() {
                "content-disposition" => {
                    name = disposition_param(v, "name").unwrap_or_default();
                    filename = disposition_param(v, "filename");
                }
                "content-type" => content_type = Some(v.trim().to_string()),
                _ => {}
            }
        }
        parts.push(Part { name, filename, content_type, data: body[data_start..next].to_vec() });
        pos = next + 2 + delim.len();
    }
    Ok(parts)
}

fn form_value(content_type: &str, body: &[u8]) -> Result<Value, String> {
    let mut out: HashMap<Str, Value> = HashMap::new();
    let mut add = |name: String, v: Value| match out.get_mut(name.as_str()) {
        Some(Value::List(l)) => l.borrow_mut().push(v),
        Some(existing) => {
            let first = existing.clone();
            *existing = crate::native::util::list(vec![first, v]);
        }
        None => {
            out.insert(name.into(), v);
        }
    };
    let ct = content_type.to_lowercase();
    if ct.starts_with("multipart/form-data") {
        let boundary = content_type
            .split(';')
            .find_map(|p| p.trim().strip_prefix("boundary=").or_else(|| p.trim().strip_prefix("BOUNDARY=")))
            .map(|b| b.trim_matches('"').to_string())
            .ok_or_else(|| io_err!("form: multipart without boundary"))?;
        for p in parse_multipart(body, &boundary).map_err(|e| io_err!("{}", e))? {
            let v = match &p.filename {
                Some(_) => NativeObjectRef::new(UploadedFile { part: p.clone() }).to_value(),
                None => Value::Str(String::from_utf8_lossy(&p.data).into_owned().into()),
            };
            add(p.name, v);
        }
    } else {
        for pair in String::from_utf8_lossy(body).split('&').filter(|s| !s.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            add(crate::native::web::url_decode(k), Value::Str(crate::native::web::url_decode(v).into()));
        }
    }
    Ok(Value::Map(Rc::new(RefCell::new(out))))
}

/// A file sent in a multipart form.
pub struct UploadedFile {
    part: Part,
}

impl FoguObject for UploadedFile {
    fn name(&self) -> &str {
        "UploadedFile"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "field" => Value::Str(self.part.name.clone().into()),
            "filename" | "name" => Value::Str(self.part.filename.clone().unwrap_or_default().into()),
            "contentType" | "type" => self.part.content_type.clone().map_or(Value::Nil, Value::str),
            "size" => Value::Int(self.part.data.len() as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "text" => Ok(Value::Str(String::from_utf8_lossy(&self.part.data).into_owned().into())),
            "bytes" => Ok(crate::native::util::list(self.part.data.iter().map(|b| Value::Int(*b as i64)).collect())),
            "json" => crate::native::json::parse_value(&String::from_utf8_lossy(&self.part.data)).map_err(|e| io_err!("json: {}", e)),
            // save(path): a folder keeps the original (sanitized) file name; returns the path.
            // A path written out is the program's own, but the client's file name often
            // ends up in it: it never climbs with `..`
            "save" => {
                let target = crate::native::util::str_arg(args, 0, "save")?;
                let mut path = PathBuf::from(&target);
                if path.is_dir() || target.ends_with('/') || target.ends_with('\\') {
                    path = path.join(safe_file_name(self.part.filename.as_deref().unwrap_or("upload")));
                } else if target.split(['/', '\\']).any(|part| part == "..") || target.contains('\0') {
                    return Err(io_err!("save: {}: '..' is not allowed in a path (save into a folder: save(\"uploads/\"))", target));
                }
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| io_err!("save: {}: {}", dir.display(), e))?;
                }
                std::fs::write(&path, &self.part.data).map_err(|e| io_err!("save: {}: {}", path.display(), e))?;
                Ok(Value::Str(crate::native::util::path_str(&path).into()))
            }
            other => Err(format!("method '{}' does not exist on UploadedFile (text bytes json save)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["text", "bytes", "json", "save"]
    }
}

/// The name an uploaded file is saved under: the last component of what the
/// client sent, without anything a file system treats specially — separators,
/// drive and stream marks, a leading dot, the trailing dots and spaces Windows
/// drops (`a.txt.` is `a.txt` there) and its device names (`NUL`, `CON`, `COM1`…).
pub(crate) fn safe_file_name(given: &str) -> String {
    let last = given.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last.chars().map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' }).collect();
    let clean = clean.trim_start_matches('.').trim_end_matches(['.', ' ']).trim_start();
    if clean.is_empty() {
        return "upload".to_string();
    }
    let stem = clean.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if device { format!("_{}", clean) } else { clean.to_string() }
}

pub struct ResponseValue {
    pub status: i64,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl ResponseValue {
    pub fn text(status: i64, body: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".into(), "text/plain; charset=utf-8".into());
        Self { status, headers, body: body.to_string() }
    }
}

impl FoguObject for ResponseValue {
    fn name(&self) -> &str {
        "Response"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "status" => Some(Value::Int(self.status)),
            "body" => Some(Value::Str(self.body.clone().into())),
            "headers" => {
                let m: HashMap<Str, Value> = self.headers.iter().map(|(k, v)| (k.as_str().into(), Value::Str(v.clone().into()))).collect();
                Some(Value::Map(Rc::new(RefCell::new(m))))
            }
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "json" => crate::native::json::parse_value(&self.body).map_err(|e| io_err!("json: {}", e)),
            other => Err(format!("method '{}' does not exist on Response", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["json"]
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_addresses() {
        let at = |v: Value| listen_addr(Some(&v), "127.0.0.1", "listen");
        let s = |x: &str| Value::Str(x.into());
        assert_eq!(at(Value::Int(8080)).unwrap(), ("127.0.0.1".into(), 8080));
        assert_eq!(at(s("8080")).unwrap(), ("127.0.0.1".into(), 8080));
        assert_eq!(at(s(":8080")).unwrap(), ("127.0.0.1".into(), 8080));
        assert_eq!(at(s("0.0.0.0:80")).unwrap(), ("0.0.0.0".into(), 80));
        assert_eq!(at(s("[::1]:3000")).unwrap(), ("::1".into(), 3000));
        assert_eq!(at(s("localhost:3000")).unwrap(), ("localhost".into(), 3000));
        for bad in [s("0.0.0.0"), s("h:0"), s("h:70000"), s("::1:80"), Value::Int(0), Value::Bool(true)] {
            assert!(at(bad).is_err());
        }
        assert_eq!(socket_addr("::1", 80).unwrap().to_string(), "[::1]:80");
    }

    #[test]
    fn multipart() {
        let body = b"--XyZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nHello\r\n--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a b.txt\"\r\nContent-Type: text/plain\r\n\r\nline1\r\nline2\r\n--XyZ--\r\n";
        let parts = parse_multipart(body, "XyZ").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].name.as_str(), parts[0].data.as_slice()), ("title", b"Hello".as_slice()));
        assert_eq!(parts[1].filename.as_deref(), Some("a b.txt"));
        assert_eq!(parts[1].data, b"line1\r\nline2");
        assert_eq!(disposition_param(" form-data; name=\"f\"; filename=\"x\"", "name").as_deref(), Some("f"));
    }

    #[test]
    fn statics() {
        let dir = std::env::temp_dir().join("fogu-static-test");
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<h1>x</h1>").unwrap();
        std::fs::write(dir.join("assets/app.js"), "x").unwrap();
        let m = StaticMount { prefix: "/".into(), dir: dir.clone(), spa: true, index: "index.html".into(), max_age: 0, dotfiles: false };
        assert!(resolve_static(&m, "/").unwrap().ends_with("index.html"));
        assert!(resolve_static(&m, "/assets/app.js").is_some());
        assert!(resolve_static(&m, "/../secret").is_none());
        assert!(resolve_static(&m, "/assets/%2e%2e/index.html").is_none());
        let m2 = StaticMount { prefix: "/static".into(), ..m };
        assert!(resolve_static(&m2, "/static/assets/app.js").is_some());
        assert!(resolve_static(&m2, "/staticx/assets/app.js").is_none());
        assert_eq!(httpdate(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
    }
}
