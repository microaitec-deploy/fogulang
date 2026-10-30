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
use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

use crate::native::math::nfn;
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
}

/// HTTP response without `Value` — every field is `Send`.
pub struct PlainResponse {
    pub status: i64,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// send this file (read by the server thread)
    pub file: Option<PathBuf>,
    /// streamed body: each item is a chunk; the response ends when the sender is dropped
    pub stream: Option<tokio::sync::mpsc::UnboundedReceiver<Bytes>>,
    /// accept a WebSocket upgrade: the plain socket goes through this channel
    pub upgrade: Option<std_mpsc::Sender<crate::native::websocket::WsIo>>,
}

impl PlainResponse {
    pub fn text(status: i64, body: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".into(), "text/plain; charset=utf-8".into());
        Self { status, headers, body: body.as_bytes().to_vec(), file: None, stream: None, upgrade: None }
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

/// Server settings decided before `listen`.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub host: String,
    pub max_body: usize,
    pub statics: Vec<StaticMount>,
    pub quiet: bool,
    /// Socket.IO served on the same port (`app.listen(port, { socketio: io })`)
    pub socketio: Option<crate::native::socketio::WebMount>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        ServerOptions { host: "127.0.0.1".into(), max_body: 10 * 1024 * 1024, statics: Vec::new(), quiet: false, socketio: None }
    }
}

/// Where the main thread answers a request (the server task awaits it).
pub type Reply = tokio::sync::oneshot::Sender<PlainResponse>;

/// Message from the server thread to the main thread.
struct ServerToMain {
    req: PlainRequest,
    reply: Reply,
}

/// Set by the server when it queues a request: the interpreter checks this
/// flag (one atomic load) instead of draining the channel on every call.
static WAITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    static PUMP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Marks the pump task as started; true the first time only.
pub fn start_pump() -> bool {
    !PUMP.with(|p| p.replace(true))
}

/// Did the server queue requests since they were last taken?
#[inline]
pub fn requests_waiting() -> bool {
    WAITING.load(std::sync::atomic::Ordering::Acquire)
}

// ================= MAIN THREAD GLOBAL STATE =================

thread_local! {
    static SERVER_HANDLER: RefCell<Option<Value>> = const { RefCell::new(None) };
    static SERVER_RX: RefCell<Option<std_mpsc::Receiver<ServerToMain>>> = const { RefCell::new(None) };
    static PENDING: RefCell<VecDeque<(PlainRequest, Reply)>> =
        const { RefCell::new(VecDeque::new()) };
}

// ================= API FOR THE INTERPRETER =================

/// Is a request waiting for the main thread?
pub fn has_pending_request() -> bool {
    WAITING.store(false, std::sync::atomic::Ordering::Release);
    SERVER_RX.with(|r| {
        if let Some(rx) = r.borrow().as_ref() {
            while let Ok(msg) = rx.try_recv() {
                PENDING.with(|q| q.borrow_mut().push_back((msg.req, msg.reply)));
            }
        }
    });
    PENDING.with(|q| !q.borrow().is_empty())
}

/// Returns the next pending request, already as a `Value::Object(Request)`.
pub fn take_pending_request() -> Option<(Value, Reply)> {
    WAITING.store(false, std::sync::atomic::Ordering::Release);
    SERVER_RX.with(|r| {
        let guard = r.borrow();
        if let Some(rx) = guard.as_ref() {
            while let Ok(msg) = rx.try_recv() {
                PENDING.with(|q| q.borrow_mut().push_back((msg.req, msg.reply)));
            }
        }
    });
    PENDING.with(|q| {
        let (plain, tx) = q.borrow_mut().pop_front()?;
        let value = NativeObjectRef::new(RequestValue {
            method: plain.method,
            path: plain.path,
            query: plain.query,
            headers: plain.headers,
            body: plain.body,
            upgrade: plain.upgrade,
            remote: plain.remote,
        })
        .to_value();
        Some((value, tx))
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

/// Registered handler (Fogu function).
pub fn current_handler() -> Option<Value> {
    SERVER_HANDLER.with(|h| h.borrow().clone())
}

/// Converts a response `Value` into a `PlainResponse` and sends it.
pub fn send_response(resp: Value, tx: Reply) {
    let plain = match resp {
        Value::Object(o) => {
            if let Some(p) = crate::native::web::take_special(&o) {
                let _ = tx.send(p);
                return;
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
                        Value::Str(s) => Some((k.clone(), s.to_string())),
                        _ => None,
                    })
                    .collect(),
                _ => HashMap::new(),
            };
            PlainResponse { status, headers, body, file: None, stream: None, upgrade: None }
        }
        _ => PlainResponse::text(500, "invalid response"),
    };
    let _ = tx.send(plain);
}

/// Native function with 1 required argument and 1 optional one (options).
pub(crate) fn nfn_opts(name: &str, f: fn(&[Value]) -> Result<Value, String>) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.to_string(),
        arity: 1,
        variadic: true,
        func: f,
        intercept: Default::default(),
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
    exports.insert("file".into(), crate::native::util::nfn_var("file", 1, hc::file));
    exports.insert(
        "serve".into(),
        // serve(8080 | "ip:port", handler)
        nfn("serve", 2, |args| {
            let mut opts = ServerOptions::default();
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
    SERVER_HANDLER.with(|h| h.borrow().is_some())
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
    let std_listener = std::net::TcpListener::bind(addr).map_err(|e| io_err!("http: cannot listen on {}: {}", addr, e))?;
    std_listener.set_nonblocking(true).map_err(|e| io_err!("http: {}", e))?;

    SERVER_HANDLER.with(|h| *h.borrow_mut() = Some(handler));
    let (tx_to_main, rx_from_server) = std_mpsc::channel::<ServerToMain>();
    SERVER_RX.with(|r| *r.borrow_mut() = Some(rx_from_server));
    // the interpreter's next call notices the flag and starts the pump task
    WAITING.store(true, std::sync::atomic::Ordering::Release);

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
                println!("[http] server listening on http://{}", addr);
            }
            let opts = std::sync::Arc::new(opts);
            loop {
                let (stream, remote) = match listener.accept().await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("http.serve: accept: {}", e);
                        continue;
                    }
                };
                let io = TokioIo::new(stream);
                let tx = tx_to_main.clone();
                let opts = std::sync::Arc::clone(&opts);
                tokio::spawn(async move {
                    let service = service_fn(move |req| handle_request(req, tx.clone(), std::sync::Arc::clone(&opts), remote));
                    let _ = http1::Builder::new().serve_connection(io, service).with_upgrades().await;
                });
            }
        });
    }
    Ok(Value::Nil)
}

fn full(b: impl Into<Bytes>) -> Body {
    Full::new(b.into()).boxed()
}

async fn handle_request(
    mut req: HyperRequest<Incoming>,
    tx: std_mpsc::Sender<ServerToMain>,
    opts: std::sync::Arc<ServerOptions>,
    remote: SocketAddr,
) -> Result<HyperResponse<Body>, Infallible> {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
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
        .filter_map(|(n, v)| v.to_str().ok().map(|v| (n.to_string(), v.to_string())))
        .collect();
    let upgrade = headers.get("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));

    // static files never reach the program
    if !upgrade && (method == "GET" || method == "HEAD") {
        if let Some(r) = serve_static(&opts.statics, &path, &headers, method == "HEAD", false).await {
            return Ok(r);
        }
    }

    let on_upgrade = if upgrade { Some(hyper::upgrade::on(&mut req)) } else { None };
    let body = match Limited::new(req.into_body(), opts.max_body).collect().await {
        Ok(c) => c.to_bytes().to_vec(),
        Err(_) => return Ok(plain_response(PlainResponse::text(413, "request body too large")).await),
    };
    let plain = PlainRequest {
        method: method.clone(),
        path: path.clone(),
        query,
        headers: headers.clone(),
        body,
        upgrade,
        remote: remote.to_string(),
    };
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<PlainResponse>();
    if tx.send(ServerToMain { req: plain, reply: reply_tx }).is_err() {
        return Ok(plain_response(PlainResponse::text(500, "server offline")).await);
    }
    WAITING.store(true, std::sync::atomic::Ordering::Release);
    // a main thread idling in the scheduler (Fogu.signal.wait) serves it now
    crate::runtime::sched::wake();
    // the Fogu handler runs on the main thread; this task just awaits the reply
    let resp = match tokio::time::timeout(Duration::from_secs(60), reply_rx).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => PlainResponse::text(500, "the handler did not answer"),
        Err(_) => PlainResponse::text(504, "timeout"),
    };

    // WebSocket: answer 101, then bridge the upgraded connection
    if let (Some(sock_tx), Some(on_upgrade)) = (resp.upgrade.as_ref().cloned(), on_upgrade) {
        if resp.status == 101 {
            let key = headers.get("sec-websocket-key").cloned().unwrap_or_default();
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
                if let Ok(upgraded) = on_upgrade.await {
                    let _ = sock_tx.send(Box::new(TokioIo::new(upgraded)));
                }
                crate::runtime::sched::wake();
            });
            return Ok(b.body(full(Bytes::new())).unwrap());
        }
    }

    // SPA fallback: an HTML page the program does not know
    if resp.status == 404 && method == "GET" && headers.get("accept").is_some_and(|a| a.contains("text/html")) {
        if let Some(r) = serve_static(&opts.statics, &path, &headers, false, true).await {
            return Ok(r);
        }
    }
    Ok(plain_response(resp).await)
}

async fn plain_response(resp: PlainResponse) -> HyperResponse<Body> {
    let code = StatusCode::from_u16(resp.status as u16).unwrap_or(StatusCode::OK);
    let mut builder = HyperResponse::builder().status(code);
    for (k, v) in &resp.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let body = if let Some(path) = &resp.file {
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
    } else if let Some(rx) = resp.stream {
        let stream = futures_lite::stream::unfold(rx, |mut rx| async move {
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

fn percent_decode(s: &str) -> String {
    crate::native::web::url_decode(&s.replace('+', "%2B"))
}

/// A file of a mount for a URL path (never outside the folder).
fn resolve_static(m: &StaticMount, path: &str) -> Option<PathBuf> {
    let prefix = m.prefix.trim_end_matches('/');
    let rest = if prefix.is_empty() {
        path
    } else if path == prefix {
        "/"
    } else {
        path.strip_prefix(prefix).filter(|r| r.starts_with('/'))?
    };
    let decoded = percent_decode(rest);
    let mut out = m.dir.clone();
    for part in decoded.split('/') {
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
    out.is_file().then_some(out)
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
        let builder = base.status(StatusCode::OK).header("content-type", mime_of(&file)).header("content-length", meta.len());
        if head {
            return Some(builder.body(full(Bytes::new())).unwrap());
        }
        let data = tokio::fs::read(&file).await.ok()?;
        return Some(builder.body(full(data)).unwrap());
    }
    None
}

/// RFC 7231 date (`Tue, 15 Nov 1994 08:12:31 GMT`).
fn httpdate(secs: u64) -> String {
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
    if start_pump() {
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
    let text = |code, msg: &str| NativeObjectRef::new(ResponseValue::text(code, msg)).to_value();
    while let Some((req, tx)) = take_pending_request() {
        let Some(handler) = current_handler() else {
            send_response(text(500, "no handler"), tx);
            continue;
        };
        let env = env.clone();
        let task = it.spawn_task(Box::new(move |it| {
            let resp = match it.call_value(handler, vec![req], env) {
                // Handler without an explicit response.
                Ok(Value::Nil) => text(404, "route not found"),
                Ok(v) => v,
                Err(e) => text(500, &e),
            };
            send_response(resp, tx);
            Ok(())
        }));
        if let Err(e) = task {
            eprintln!("[http] cannot start a handler: {}", e);
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
                let m: HashMap<String, Value> = self.headers.iter().map(|(k, v)| (k.clone(), Value::Str(v.clone().into()))).collect();
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
            "json" => crate::native::json::parse_value(&String::from_utf8_lossy(&self.body)),
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
    let lower = header.to_lowercase();
    let mut idx = 0;
    while let Some(pos) = lower[idx..].find(&format!("{}=", key)) {
        let start = idx + pos;
        // must be a whole parameter name
        let before = lower[..start].chars().last();
        if before.is_none_or(|c| c == ';' || c == ' ') {
            let v = &header[start + key.len() + 1..];
            return Some(if let Some(rest) = v.strip_prefix('"') {
                rest.split('"').next().unwrap_or("").to_string()
            } else {
                v.split(';').next().unwrap_or("").trim().to_string()
            });
        }
        idx = start + key.len();
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
    let mut out: HashMap<String, Value> = HashMap::new();
    let mut add = |name: String, v: Value| match out.get_mut(&name) {
        Some(Value::List(l)) => l.borrow_mut().push(v),
        Some(existing) => {
            let first = existing.clone();
            *existing = crate::native::util::list(vec![first, v]);
        }
        None => {
            out.insert(name, v);
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
            "json" => crate::native::json::parse_value(&String::from_utf8_lossy(&self.part.data)),
            // save(path): a folder keeps the original (sanitized) file name; returns the path
            "save" => {
                let target = crate::native::util::str_arg(args, 0, "save")?;
                let mut path = PathBuf::from(&target);
                if path.is_dir() || target.ends_with('/') || target.ends_with('\\') {
                    let clean: String = self
                        .part
                        .filename
                        .clone()
                        .unwrap_or_else(|| "upload".into())
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or("upload")
                        .chars()
                        .map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' })
                        .collect();
                    let clean = clean.trim_start_matches('.').to_string();
                    path = path.join(if clean.is_empty() { "upload".to_string() } else { clean });
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
                let m: HashMap<String, Value> = self.headers.iter().map(|(k, v)| (k.clone(), Value::Str(v.clone().into()))).collect();
                Some(Value::Map(Rc::new(RefCell::new(m))))
            }
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "json" => crate::native::json::parse_value(&self.body),
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
