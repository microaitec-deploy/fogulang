//! `fogu:web` module — mini framework inspired by ElysiaJS.
//!
//! ```fogu
//! import web from "fogu:web"
//!
//! const app = web.app()
//! app.use(web.logger())                           // native middleware
//! app.use(web.cors({ origin: "*" }))
//! app.use("/admin", (ctx) => {                    // scoped to /admin/*
//!     if ctx.headers["x-key"] != "s3cret" {
//!         return web.json({ error: "forbidden" }, { status: 403 })   // stops here
//!     }
//!     ctx.user = "root"                           // visible to the handler
//! })
//! app.get("/admin/stats", auth, (ctx) => ({ user: ctx.user }))  // route middleware
//! app.after((ctx, res) => { ctx.set.headers["x-served-by"] = "fogu" })
//! app.onError((ctx, err) => web.json({ error: err.message }, { status: 500 }))
//! app.listen(3000)
//! ```
//!
//! Request pipeline: global `use` middlewares (in order; a scoped one only
//! runs under its path prefix) → the route's own middlewares → the handler →
//! `after` hooks → native `after` (logger). A middleware returns `nil` to go
//! on, or any response value to answer right away. Errors (a runtime error or
//! a returned `error` value) go to the `onError` handlers, or become a 500.
//! `ctx.set.status` / `ctx.set.headers` adjust the final response.

use std::cell::{Cell, RefCell};
use ahash::AHashMap as HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched;
use crate::runtime::value::{ErrorValue, ModuleValue, Value, ERR_VALUE_PREFIX};

use crate::native::http as fogu_http;
use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{map, nfn_var};

// ================= MODULE =================

fn web_response(body: String, content_type: &str, args: &[Value]) -> Result<Value, String> {
    let (status, headers) = fogu_http::response_opts(content_type, args)?;
    Ok(NativeObjectRef::new(ResponseValue { status, headers, body, file: None }).to_value())
}

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    exports.insert(
        "app".into(),
        nfn("app", 0, |_| Ok(NativeObjectRef::new(AppValue::new()).to_value())),
    );

    // json/text/html(body, options?) — options: { status: 404, headers: {...} }
    exports.insert(
        "json".into(),
        fogu_http::nfn_opts("json", |args| {
            let body = crate::native::json::stringify_value(&args[0]);
            web_response(body, "application/json", args)
        }),
    );

    exports.insert(
        "text".into(),
        fogu_http::nfn_opts("text", |args| {
            let body = match &args[0] {
                Value::Str(s) => s.clone(),
                other => other.to_display_string().into(),
            };
            web_response(body.into_string(), "text/plain; charset=utf-8", args)
        }),
    );

    exports.insert(
        "html".into(),
        fogu_http::nfn_opts("html", |args| {
            let body = match &args[0] {
                Value::Str(s) => s.clone(),
                other => other.to_display_string().into(),
            };
            web_response(body.into_string(), "text/html; charset=utf-8", args)
        }),
    );

    // redirect(url, status = 302)
    exports.insert(
        "redirect".into(),
        nfn_var("redirect", 1, |args| {
            crate::native::util::max_args(args, 2, "redirect")?;
            let url = crate::native::util::str_arg(args, 0, "redirect")?;
            let status = match args.get(1) {
                None | Some(Value::Nil) => 302,
                Some(Value::Int(n)) if (300..400).contains(n) => *n,
                Some(other) => return Err(format!("redirect: status must be 3xx, got {}", other.to_display_string())),
            };
            let mut headers = HashMap::new();
            headers.insert("location".to_string(), url);
            Ok(NativeObjectRef::new(ResponseValue { status, headers, body: String::new(), file: None }).to_value())
        }),
    );

    exports.insert("cors".into(), nfn_var("cors", 0, cors));

    // file(path, { download, contentType, status }) — sent by the server thread
    exports.insert(
        "file".into(),
        nfn_var("file", 1, |args| {
            crate::native::util::max_args(args, 2, "file")?;
            let path = crate::native::util::str_arg(args, 0, "file")?;
            let mut status = 200;
            let mut headers = HashMap::new();
            if let Some(Value::Map(o)) = args.get(1) {
                let o = o.borrow();
                if let Some(Value::Str(name)) = o.get("download") {
                    headers.insert("content-disposition".to_string(), format!("attachment; filename=\"{}\"", name.replace('"', "")));
                }
                if let Some(Value::Str(ct)) = o.get("contentType") {
                    headers.insert("content-type".to_string(), ct.to_string());
                }
                if let Some(Value::Int(n)) = o.get("status") {
                    status = *n;
                }
            }
            if !std::path::Path::new(&path).is_file() {
                return Err(io_err!("web.file: {}: no such file", path));
            }
            Ok(NativeObjectRef::new(ResponseValue { status, headers, body: String::new(), file: Some(PathBuf::from(path)) }).to_value())
        }),
    );

    // sse({ retry, headers }) — a Server-Sent Events stream; return it from the handler
    exports.insert(
        "sse".into(),
        nfn_var("sse", 0, |args| {
            crate::native::util::max_args(args, 1, "sse")?;
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let mut headers = HashMap::new();
            headers.insert("content-type".to_string(), "text/event-stream; charset=utf-8".to_string());
            headers.insert("cache-control".to_string(), "no-cache".to_string());
            headers.insert("x-accel-buffering".to_string(), "no".to_string());
            if let Some(Value::Map(o)) = args.first() {
                let o = o.borrow();
                if let Some(Value::Int(ms)) = o.get("retry") {
                    let _ = tx.send(bytes::Bytes::from(format!("retry: {}\n\n", ms)));
                }
                if let Some(Value::Map(h)) = o.get("headers") {
                    for (k, v) in h.borrow().iter() {
                        headers.insert(k.to_lowercase(), v.to_display_string());
                    }
                }
            }
            Ok(NativeObjectRef::new(SseValue { tx, rx: Some(rx), headers, sent: 0 }).to_value())
        }),
    );
    exports.insert(
        "logger".into(),
        nfn("logger", 0, |_| Ok(NativeObjectRef::new(Logger).to_value())),
    );

    ModuleValue {
        name: "fogu:web".into(),
        exports: RefCell::new(exports),
    }
}

// ================= SEGMENTS =================

#[derive(Clone)]
enum Segment {
    Literal(String),
    Param(String),
    Wildcard,
}

fn parse_path(path: &str) -> Vec<Segment> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s == "*" {
                Segment::Wildcard
            } else if let Some(name) = s.strip_prefix(':') {
                Segment::Param(name.to_string())
            } else {
                Segment::Literal(s.to_string())
            }
        })
        .collect()
}

fn match_segments(pattern: &[Segment], path: &[&str]) -> Option<HashMap<String, Value>> {
    let mut params = HashMap::new();
    let mut pi = 0;
    for seg in pattern {
        match seg {
            Segment::Wildcard => return Some(params),
            Segment::Literal(lit) => {
                if pi >= path.len() || path[pi] != lit {
                    return None;
                }
                pi += 1;
            }
            Segment::Param(name) => {
                if pi >= path.len() {
                    return None;
                }
                params.insert(name.clone(), Value::Str(url_decode(path[pi]).into()));
                pi += 1;
            }
        }
    }
    if pi != path.len() {
        return None;
    }
    Some(params)
}

/// `/admin` matches `/admin` and `/admin/...`, not `/administrator`.
fn under_prefix(prefix: &str, path: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty()
        || path == prefix
        || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

pub(crate) use crate::native::util::url_decode;

/// `a=1&b=x%20y&flag` → `{ a: "1", b: "x y", flag: "" }`.
fn parse_query(q: &str) -> HashMap<String, Value> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (url_decode(k), Value::Str(url_decode(v).into()))
        })
        .collect()
}

// ================= APP =================

struct Route {
    method: String,
    segments: Vec<Segment>,
    middlewares: Vec<Value>,
    handler: Value,
}

#[derive(Default)]
struct AppState {
    routes: Vec<Route>,
    /// `(path prefix, middleware)`; the prefix is "" for global ones.
    middlewares: Vec<(String, Value)>,
    after: Vec<Value>,
    on_error: Vec<Value>,
    store: Rc<RefCell<HashMap<String, Value>>>,
    statics: Vec<fogu_http::StaticMount>,
    ws_routes: Vec<WsRoute>,
    /// fogu:topcoat shards (`POST /_tc/s/<name>`) and procedures (`POST /_tc/p/<name>`).
    shards: HashMap<String, Value>,
    procedures: HashMap<String, Value>,
}

/// Makes a shard rendered by a page reachable at `POST /_tc/s/<name>`.
pub fn register_shard(app: u64, name: &str, func: Value) {
    if let Some(st) = APPS.with(|a| a.borrow().get(&app).cloned()) {
        st.borrow_mut().shards.insert(name.to_string(), func);
    }
}

struct WsRoute {
    segments: Vec<Segment>,
    middlewares: Vec<Value>,
    handler: Value,
    protocols: Vec<String>,
}

thread_local! {
    /// Apps that are listening or have produced a handler, by id.
    static APPS: RefCell<HashMap<u64, Rc<RefCell<AppState>>>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

const DISPATCH: &str = "__web_dispatch__";

pub struct AppValue {
    id: u64,
    state: Rc<RefCell<AppState>>,
}

impl AppValue {
    pub fn new() -> Self {
        let id = NEXT_ID.with(|n| {
            let id = n.get();
            n.set(id + 1);
            id
        });
        Self { id, state: Rc::new(RefCell::new(AppState::default())) }
    }

    /// Another handle to the same app, so calls can be chained.
    fn same(&self) -> Value {
        NativeObjectRef::new(AppValue { id: self.id, state: Rc::clone(&self.state) }).to_value()
    }

    /// Registers the app and returns its dispatch function.
    fn handler(&self) -> Value {
        APPS.with(|a| a.borrow_mut().insert(self.id, Rc::clone(&self.state)));
        nfn(&format!("{}#{}", DISPATCH, self.id), 1, |_| Ok(Value::Nil))
    }
}

fn is_callable(v: &Value) -> bool {
    matches!(v, Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) | Value::StructBoundMethod(_))
        || matches!(v, Value::Object(o) if o.inner().borrow().methods().contains(&"handle"))
}

fn fn_arg(v: &Value, what: &str) -> Result<Value, String> {
    if is_callable(v) {
        Ok(v.clone())
    } else {
        Err(format!("{}: expected a function, got '{}'", what, v.type_name()))
    }
}

impl FoguObject for AppValue {
    fn name(&self) -> &str {
        "App"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "store" => Some(Value::Map(Rc::clone(&self.state.borrow().store))),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "all" => {
                if args.len() < 2 {
                    return Err(format!("{}(path, ...middlewares, handler) expects at least 2 arguments", method));
                }
                let path = match &args[0] {
                    Value::Str(s) => s.clone(),
                    other => return Err(format!("{}: path must be str, got '{}'", method, other.type_name())),
                };
                let mut fns = Vec::new();
                for a in &args[1..] {
                    fns.push(fn_arg(a, method)?);
                }
                let handler = fns.pop().unwrap();
                let m = if method == "all" { "*".to_string() } else { method.to_uppercase() };
                self.state.borrow_mut().routes.push(Route {
                    method: m,
                    segments: parse_path(&path),
                    middlewares: fns,
                    handler,
                });
                Ok(self.same())
            }

            // use(fn) · use("/prefix", fn)
            "use" => {
                let (prefix, f) = match args {
                    [f] => (String::new(), f),
                    [Value::Str(p), f] => (p.to_string(), f),
                    _ => return Err("use expects (fn) or (prefix, fn)".into()),
                };
                let f = fn_arg(f, "use")?;
                self.state.borrow_mut().middlewares.push((prefix, f));
                Ok(self.same())
            }

            "after" | "onError" => {
                if args.len() != 1 {
                    return Err(format!("{}(fn) expects 1 argument", method));
                }
                let f = fn_arg(&args[0], method)?;
                let mut st = self.state.borrow_mut();
                if method == "after" { st.after.push(f) } else { st.on_error.push(f) }
                drop(st);
                Ok(self.same())
            }

            "set" => {
                if args.len() != 2 {
                    return Err("set(key, value) expects 2 arguments".into());
                }
                match &args[0] {
                    Value::Str(k) => {
                        self.state.borrow().store.borrow_mut().insert(k.to_string(), args[1].clone());
                        Ok(self.same())
                    }
                    other => Err(format!("set: key must be str, got '{}'", other.type_name())),
                }
            }

            // listen(8080 | "ip:port", { host = "127.0.0.1", maxBody = 10mb, quiet })
            "listen" => {
                let mut opts = fogu_http::ServerOptions { statics: self.state.borrow().statics.clone(), ..Default::default() };
                match args.get(1) {
                    None | Some(Value::Nil) => {}
                    Some(Value::Map(o)) => {
                        let o = o.borrow();
                        if let Some(Value::Str(h)) = o.get("host") {
                            opts.host = h.to_string();
                        }
                        match o.get("maxBody") {
                            Some(Value::Int(n)) if *n > 0 => opts.max_body = *n as usize,
                            None | Some(Value::Nil) => {}
                            Some(other) => return Err(format!("listen: maxBody must be a number of bytes, got {}", other.to_display_string())),
                        }
                        opts.quiet = o.get("quiet").is_some_and(|v| v.is_truthy());
                        if let Some(io) = o.get("socketio") {
                            opts.socketio = Some(crate::native::socketio::web_mount(io)?);
                        }
                    }
                    Some(other) => return Err(format!("listen: options must be a map, got '{}'", other.type_name())),
                }
                // a host in "ip:port" wins over the `host` option
                let (host, port) = fogu_http::listen_addr(args.first(), &opts.host, "listen")?;
                opts.host = host;
                fogu_http::serve_from_native(port, self.handler(), opts)
            }

            // static(dir) · static(prefix, dir, { spa, index = "index.html", maxAge, dotfiles })
            "static" => {
                let (prefix, dir, o) = match args {
                    [Value::Str(d)] => ("/".to_string(), d.clone(), None),
                    [Value::Str(p), Value::Str(d)] => (p.to_string(), d.clone(), None),
                    [Value::Str(p), Value::Str(d), Value::Map(o)] => (p.to_string(), d.clone(), Some(o.borrow().clone())),
                    [Value::Str(d), Value::Map(o)] => ("/".to_string(), d.clone(), Some(o.borrow().clone())),
                    _ => return Err("static expects (dir), (prefix, dir) or (prefix, dir, options)".into()),
                };
                let dir = PathBuf::from(&dir);
                if !dir.is_dir() {
                    return Err(io_err!("static: {}: no such folder", dir.display()));
                }
                let o = o.unwrap_or_default();
                let max_age = match o.get("maxAge") {
                    None | Some(Value::Nil) => 0,
                    Some(Value::Int(n)) if *n >= 0 => *n as u64,
                    Some(Value::Duration(ns)) if *ns >= 0 => (*ns / 1_000_000_000) as u64,
                    Some(other) => return Err(format!("static: maxAge must be seconds or a duration, got {}", other.to_display_string())),
                };
                self.state.borrow_mut().statics.push(fogu_http::StaticMount {
                    prefix: if prefix.starts_with('/') { prefix } else { format!("/{}", prefix) },
                    dir: dir.canonicalize().unwrap_or(dir),
                    spa: o.get("spa").is_some_and(|v| v.is_truthy()),
                    index: match o.get("index") {
                        Some(Value::Str(i)) => i.to_string(),
                        _ => "index.html".into(),
                    },
                    max_age,
                    dotfiles: o.get("dotfiles").is_some_and(|v| v.is_truthy()),
                });
                Ok(self.same())
            }

            // ws(path, ...middlewares, handler(ws, ctx), { protocols }?)
            "ws" => {
                let mut rest: Vec<Value> = args.to_vec();
                let protocols = match rest.last() {
                    Some(Value::Map(o)) => {
                        let p = match o.borrow().get("protocols") {
                            Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
                            _ => Vec::new(),
                        };
                        rest.pop();
                        p
                    }
                    _ => Vec::new(),
                };
                if rest.len() < 2 {
                    return Err("ws(path, ...middlewares, handler) expects at least 2 arguments".into());
                }
                let path = match &rest[0] {
                    Value::Str(s) => s.clone(),
                    other => return Err(format!("ws: path must be str, got '{}'", other.type_name())),
                };
                let mut fns = Vec::new();
                for a in &rest[1..] {
                    fns.push(fn_arg(a, "ws")?);
                }
                let handler = fns.pop().unwrap();
                self.state.borrow_mut().ws_routes.push(WsRoute { segments: parse_path(&path), middlewares: fns, handler, protocols });
                Ok(self.same())
            }

            "handler" => Ok(self.handler()),

            // shard(tc.shard(...)) — also registered by the first page that renders it
            "shard" => {
                let (name, func) = match args {
                    [Value::Object(o)] if o.name() == "TopcoatShard" => (o.get("name").unwrap_or(Value::Nil).to_display_string(), o.get("handler").unwrap_or(Value::Nil)),
                    _ => return Err("shard expects a shard made with tc.shard(name, fn)".into()),
                };
                self.state.borrow_mut().shards.insert(name, func);
                Ok(self.same())
            }

            // procedure(name, fn(ctx, ...args)) — called from the browser with call(name, ...args)
            "procedure" => {
                let (name, f) = match args {
                    [Value::Str(n), f] => (n.clone(), fn_arg(f, "procedure")?),
                    _ => return Err("procedure expects (name, fn)".into()),
                };
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
                    return Err(format!("procedure: invalid name '{}' (letters, digits, _ - .)", name));
                }
                self.state.borrow_mut().procedures.insert(name.into_string(), f);
                Ok(self.same())
            }

            "routes" => {
                let names: Vec<Value> = self
                    .state
                    .borrow()
                    .routes
                    .iter()
                    .map(|r| {
                        Value::Str(format!(
                            "{} /{}",
                            r.method,
                            r.segments
                                .iter()
                                .map(|s| match s {
                                    Segment::Literal(l) => l.clone(),
                                    Segment::Param(p) => format!(":{}", p),
                                    Segment::Wildcard => "*".into(),
                                })
                                .collect::<Vec<_>>()
                                .join("/")
                        ).into())
                    })
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(names))))
            }

            other => Err(format!("method '{}' does not exist on App", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "get", "post", "put", "delete", "patch", "head", "options", "all", "use", "after", "onError", "set",
            "static", "ws", "handler", "listen", "routes", "shard", "procedure",
        ]
    }
}

// ================= DISPATCH =================

/// Is this native function an app's dispatch function?
pub fn is_dispatch(name: &str) -> bool {
    name.starts_with(DISPATCH)
}

/// Called by the interpreter for a `__web_dispatch__#<id>` function: runs the
/// request through the app's pipeline and returns a `Response`.
pub fn dispatch(
    name: &str,
    req: &Value,
    interp: &Interpreter,
    env: Rc<RefCell<Environment>>,
) -> Result<Value, String> {
    // `app.handler()` takes a plain map: make it the same Request object a real
    // HTTP request is, so ctx.req.json()/form()/bytes() behave alike in tests
    let as_request = test_request(req);
    let req = as_request.as_ref().unwrap_or(req);
    let id: u64 = name.rsplit('#').next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let state = APPS
        .with(|a| a.borrow().get(&id).cloned())
        .ok_or("web: this app has no handler (call app.handler() or app.listen())")?;

    let mut method = req_str(req, "method").unwrap_or_else(|| "GET".into()).to_uppercase();
    let full_path = req_str(req, "path").unwrap_or_else(|| "/".into());
    let (path, mut query) = match full_path.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (full_path.clone(), None),
    };
    // fogu:topcoat's browser runtime and stylesheet
    if let Some(r) = topcoat_asset(&method, &path) {
        return Ok(r);
    }
    if let Some(q) = req_str(req, "query") {
        query = Some(q);
    }
    let headers: HashMap<String, Value> = match req_field(req, "headers") {
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect(),
        _ => HashMap::new(),
    };
    let segments: Vec<&str> = path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let upgrade = matches!(req_field(req, "upgrade"), Some(Value::Bool(true)));

    // fogu:topcoat: a page rerun (POST of the signals, rendered as a GET),
    // a shard or a procedure
    let tc_mode = headers.get("x-topcoat").map(|v| v.to_display_string()).unwrap_or_default();
    let tc_body = || -> Value {
        match req_field(req, "body") {
            Some(Value::Str(b)) if !b.is_empty() => crate::native::json::parse_value(&b).unwrap_or(Value::Nil),
            _ => Value::Nil,
        }
    };
    let rerun = tc_mode == "rerun" && method == "POST";
    if rerun {
        method = "GET".into();
    }
    let special = match (method.as_str(), segments.as_slice()) {
        ("POST", ["_tc", "s", name]) => Some(TcCall::Shard(state.borrow().shards.get(*name).cloned())),
        ("POST", ["_tc", "p", name]) => Some(TcCall::Procedure(name.to_string(), state.borrow().procedures.get(*name).cloned())),
        _ => None,
    };

    // the route, its middlewares and the matching global middlewares
    let (route, global, after, on_error, store, ws_route) = {
        let st = state.borrow();
        let ws_route = if upgrade {
            st.ws_routes.iter().find_map(|r| {
                match_segments(&r.segments, &segments).map(|p| (r.handler.clone(), r.middlewares.clone(), p, r.protocols.clone()))
            })
        } else {
            None
        };
        let route = if upgrade {
            ws_route.as_ref().map(|w| (w.0.clone(), w.1.clone(), w.2.clone()))
        } else {
            st.routes.iter().find_map(|r| {
            if r.method != "*" && r.method != method && !(method == "HEAD" && r.method == "GET") {
                return None;
            }
            match_segments(&r.segments, &segments).map(|p| (r.handler.clone(), r.middlewares.clone(), p))
            })
        };
        let global: Vec<Value> = st
            .middlewares
            .iter()
            .filter(|(prefix, _)| under_prefix(prefix, &path))
            .map(|(_, f)| f.clone())
            .collect();
        (route, global, st.after.clone(), st.on_error.clone(), Rc::clone(&st.store), ws_route)
    };

    let set = map(vec![("status", Value::Nil), ("headers", map(vec![]))]);
    let ctx = map(vec![
        ("req", req.clone()),
        ("method", Value::Str(method.clone().into())),
        ("path", Value::Str(path.clone().into())),
        ("query", Value::Map(Rc::new(RefCell::new(query.as_deref().map(parse_query).unwrap_or_default())))),
        ("headers", Value::Map(Rc::new(RefCell::new(headers)))),
        ("body", req_field(req, "body").unwrap_or(Value::Str(String::new().into()))),
        ("params", map(vec![])),
        ("store", Value::Map(store)),
        ("set", set.clone()),
    ]);
    if let (Some((_, _, params)), Value::Map(c)) = (&route, &ctx) {
        c.borrow_mut().insert("params".into(), Value::Map(Rc::new(RefCell::new(params.clone()))));
    }
    let tc = crate::native::topcoat::ctx_state(&ctx).expect("ctx is a map");
    tc.borrow_mut().insert("app".into(), Value::Int(id as i64));
    tc.borrow_mut().insert("rerun".into(), Value::Bool(rerun));
    let tc_req = if rerun || special.is_some() { tc_body() } else { Value::Nil };
    if let Value::Map(b) = &tc_req {
        if let Some(Value::Map(sig)) = b.borrow().get("signals") {
            tc.borrow_mut().insert("input".into(), Value::Map(Rc::new(RefCell::new(sig.borrow().clone()))));
        }
        if let Some(p) = b.borrow().get("path") {
            tc.borrow_mut().insert("page".into(), p.clone());
        }
    }

    // `return value, error(...)` counts as the error
    let run = |f: &Value, args: Vec<Value>| invoke(interp, &env, f, args).map(unpair);

    // middlewares → handler
    let mut ran: Vec<Value> = Vec::new();
    let mut outcome: Result<Option<Value>, Value> = Ok(None);
    let route_mws = route.as_ref().map(|r| r.1.clone()).unwrap_or_default();
    for mw in global.iter().chain(route_mws.iter()) {
        ran.push(mw.clone());
        match run(mw, vec![ctx.clone()]) {
            Ok(Value::Nil) => {}
            Ok(e @ Value::Error(_)) => {
                outcome = Err(e);
                break;
            }
            Ok(v) => {
                outcome = Ok(Some(v));
                break;
            }
            Err(e) => {
                outcome = Err(e);
                break;
            }
        }
    }
    // WebSocket: accept (101) and run the handler in its own task
    if let (Ok(None), Some((handler, _, _, protocols))) = (&outcome, &ws_route) {
        let requested: Vec<String> = match field_of(&ctx, "headers") {
            Value::Map(h) => h
                .borrow()
                .get("sec-websocket-protocol")
                .map(|v| v.to_display_string().split(',').map(|p| p.trim().to_string()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let chosen = if protocols.is_empty() {
            requested.first().cloned()
        } else {
            requested.iter().find(|p| protocols.contains(p)).cloned()
        };
        let (tx, rx) = std_mpsc::channel::<crate::native::websocket::WsIo>();
        let mut headers = HashMap::new();
        if let Some(p) = &chosen {
            headers.insert("sec-websocket-protocol".to_string(), p.clone());
        }
        let raw_headers: Vec<(String, String)> = match field_of(&ctx, "headers") {
            Value::Map(h) => h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect(),
            _ => Vec::new(),
        };
        let host = raw_headers.iter().find(|(k, _)| k == "host").map(|(_, v)| v.clone()).unwrap_or_default();
        let url = format!("ws://{}{}", host, full_path);
        let (handler, ctx2, env2, path2) = (handler.clone(), ctx.clone(), Rc::clone(&env), full_path.clone());
        interp.spawn_task(Box::new(move |it| {
            // the server sends the upgraded connection (and wakes us) once the 101 is out
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut sock: Option<Option<crate::native::websocket::WsIo>> = None;
            let guard = sched::IoGuard::new();
            sched::wait_until(|| {
                if sock.is_none() {
                    match rx.try_recv() {
                        Ok(s) => sock = Some(Some(s)),
                        Err(std_mpsc::TryRecvError::Disconnected) => sock = Some(None),
                        Err(std_mpsc::TryRecvError::Empty) if Instant::now() >= deadline => sock = Some(None),
                        Err(std_mpsc::TryRecvError::Empty) => sched::add_timer(deadline),
                    }
                }
                sock.is_some()
            })?;
            drop(guard);
            let Some(Some(tcp)) = sock else { return Ok(()) };
            let conn = crate::native::websocket::server_conn(tcp, url, path2, raw_headers, chosen);
            if let Err(e) = it.call_value(handler, vec![conn, ctx2], env2) {
                eprintln!("[web] websocket handler: {}", e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e));
            }
            Ok(())
        }))?;
        outcome = Ok(Some(NativeObjectRef::new(WsAccept { tx: Some(tx), headers }).to_value()));
    }
    if upgrade && ws_route.is_none() {
        outcome = Ok(Some(text_response(404, "no websocket route")));
    }

    if let (Ok(None), Some(call)) = (&outcome, &special) {
        outcome = Ok(Some(match call {
            TcCall::Shard(None) | TcCall::Procedure(_, None) => text_response(404, "not found"),
            TcCall::Shard(Some(func)) => {
                let props = match &tc_req {
                    Value::Map(b) => b.borrow().get("props").cloned().unwrap_or_else(|| map(vec![])),
                    _ => map(vec![]),
                };
                match crate::native::topcoat::render_shard(interp, &env, func, &ctx, props) {
                    Ok(Ok(html)) => ndjson(200, crate::native::topcoat::frame(vec![("t", Value::Str("swap".into())), ("region", Value::Str(String::new().into())), ("html", Value::Str(html.into()))])),
                    Ok(Err(e)) => ndjson(200, error_frame(500, &err_message(&e))),
                    Err(e) => {
                        let msg = e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
                        eprintln!("[web] shard {}: {}", path, msg);
                        ndjson(200, error_frame(500, &msg))
                    }
                }
            }
            TcCall::Procedure(name, Some(func)) => {
                let mut cargs = vec![ctx.clone()];
                if let Value::List(l) = &tc_req {
                    cargs.extend(l.borrow().iter().cloned());
                }
                match run(func, cargs) {
                    Ok(Value::Error(e)) => json_response(200, &map(vec![("error", Value::Str(e.message.clone().into()))])),
                    // `return value, error(...)`
                    Ok(Value::List(l)) if l.borrow().len() == 2 && matches!(l.borrow()[1], Value::Error(_)) => {
                        json_response(200, &map(vec![("error", Value::Str(err_message(&l.borrow()[1]).into()))]))
                    }
                    Ok(v) => json_response(200, &map(vec![("ok", v)])),
                    Err(e) => {
                        eprintln!("[web] procedure {}: {}", name, err_message(&e));
                        json_response(500, &map(vec![("error", Value::Str(err_message(&e).into()))]))
                    }
                }
            }
        }));
    }

    if let Ok(None) = outcome {
        outcome = match &route {
            Some((handler, _, _)) => match run(handler, vec![ctx.clone()]) {
                Ok(e @ Value::Error(_)) => Err(e),
                Ok(v) => Ok(Some(v)),
                Err(e) => Err(e),
            },
            None => Ok(Some(text_response(404, "not found"))),
        };
    }

    // errors → onError (the first one that returns a response)
    let mut response = match outcome {
        Ok(v) => v.unwrap_or(Value::Nil),
        Err(err) => {
            let mut handled = None;
            for h in &on_error {
                match run(h, vec![ctx.clone(), err.clone()]) {
                    Ok(Value::Nil) | Ok(Value::Error(_)) => {}
                    Ok(v) => {
                        handled = Some(v);
                        break;
                    }
                    Err(_) => {}
                }
            }
            handled.unwrap_or_else(|| {
                let msg = err_message(&err);
                eprintln!("[web] {} {}: {}", method, path, msg);
                text_response(500, &format!("internal error: {}", msg))
            })
        }
    };
    response = topcoat_response(response, &ctx, rerun);
    response = normalize_response(response, &set);

    // after hooks may replace the response
    for h in &after {
        match run(h, vec![ctx.clone(), response.clone()]) {
            Ok(Value::Nil) | Ok(Value::Error(_)) => {}
            Ok(v) => response = normalize_response(v, &set),
            Err(e) => eprintln!("[web] after hook: {}", err_message(&e)),
        }
    }
    response = apply_set(response, &set);

    // native middlewares with an `after` step (logger), innermost first
    for mw in ran.iter().rev() {
        if let Value::Object(o) = mw {
            if o.inner().borrow().methods().contains(&"after") {
                if let Ok(v @ Value::Object(_)) = o.call("after", &[ctx.clone(), response.clone()]) {
                    response = v;
                }
            }
        }
    }
    Ok(response)
}

/// `[value, error]` (a function that did `return value, error(...)`) → the error.
/// (`[value, nil]` stays a list: it may be a real two-item answer.)
fn unpair(v: Value) -> Value {
    if let Value::List(l) = &v {
        let l = l.borrow();
        if l.len() == 2 && matches!(l[1], Value::Error(_)) {
            return l[1].clone();
        }
    }
    v
}

/// A `{ method, path, query, headers, body }` map → a `Request` object.
fn test_request(req: &Value) -> Option<Value> {
    let Value::Map(m) = req else { return None };
    let m = m.borrow();
    let text = |k: &str| m.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
    let full = text("path").unwrap_or_else(|| "/".into());
    let (path, mut query) = match full.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (full, None),
    };
    if let Some(q) = text("query") {
        query = Some(q);
    }
    let headers = match m.get("headers") {
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.to_lowercase(), v.to_display_string())).collect(),
        _ => HashMap::new(),
    };
    let body = match m.get("body") {
        None | Some(Value::Nil) => Vec::new(),
        // a map is sent as JSON, a list of bytes as bytes, anything else as text
        Some(v @ Value::Map(_)) => crate::native::json::stringify_value(v).into_bytes(),
        Some(v @ Value::List(_)) => crate::native::util::bytes_of(v, "handler body").unwrap_or_else(|_| crate::native::json::stringify_value(v).into_bytes()),
        Some(v) => v.to_display_string().into_bytes(),
    };
    Some(
        NativeObjectRef::new(fogu_http::RequestValue {
            method: text("method").unwrap_or_else(|| "GET".into()).to_uppercase(),
            path,
            query,
            headers,
            body,
            upgrade: matches!(m.get("upgrade"), Some(Value::Bool(true))),
            remote: text("remote").unwrap_or_else(|| "127.0.0.1:0".into()),
        })
        .to_value(),
    )
}

/// A fogu:topcoat request to a server function.
enum TcCall {
    Shard(Option<Value>),
    Procedure(String, Option<Value>),
}

fn ndjson(status: i64, body: String) -> Value {
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/x-ndjson".to_string());
    headers.insert("cache-control".to_string(), "no-store".to_string());
    NativeObjectRef::new(ResponseValue { status, headers, body, file: None }).to_value()
}

fn error_frame(status: i64, message: &str) -> String {
    crate::native::topcoat::frame(vec![("t", Value::Str("error".into())), ("status", Value::Int(status)), ("message", Value::Str(message.to_string().into()))])
}

/// `GET /_tc/runtime.js` and `/_tc/ui.css`.
fn topcoat_asset(method: &str, path: &str) -> Option<Value> {
    if method != "GET" && method != "HEAD" {
        return None;
    }
    let (body, ctype) = match path {
        "/_tc/runtime.js" => (crate::native::topcoat::RUNTIME_JS, "text/javascript; charset=utf-8"),
        "/_tc/ui.css" => (crate::native::topcoat::UI_CSS, "text/css; charset=utf-8"),
        "/_tc/shadcn.css" => (crate::native::topcoat::SHADCN_CSS, "text/css; charset=utf-8"),
        _ => return None,
    };
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), ctype.to_string());
    headers.insert("cache-control".to_string(), "public, max-age=3600".to_string());
    Some(NativeObjectRef::new(ResponseValue { status: 200, headers, body: body.to_string(), file: None }).to_value())
}

/// `Html` results become HTML responses carrying the page's signals; a rerun
/// answers with Topcoat frames (snapshot, redirect or error).
fn topcoat_response(resp: Value, ctx: &Value, rerun: bool) -> Value {
    let html = crate::native::topcoat::as_html(&resp).or_else(|| match (&resp, rerun) {
        (Value::Str(s), true) if content_type_of(s).starts_with("text/html") => Some(s.to_string()),
        _ => None,
    });
    if let Some(h) = html {
        let h = crate::native::topcoat::with_signals(h, ctx);
        if rerun {
            return ndjson(200, crate::native::topcoat::frame(vec![("t", Value::Str("snapshot".into())), ("html", Value::Str(h.into()))]));
        }
        let status = match field_of(&field_of(ctx, "set"), "status") {
            Value::Int(n) => n,
            _ => 200,
        };
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/html; charset=utf-8".to_string());
        return NativeObjectRef::new(ResponseValue { status, headers, body: h, file: None }).to_value();
    }
    if rerun {
        if let Value::Object(o) = &resp {
            if o.name() == "Response" {
                let status = match o.get("status") {
                    Some(Value::Int(n)) => n,
                    _ => 200,
                };
                let location = match o.get("headers") {
                    Some(Value::Map(h)) => h.borrow().get("location").map(|v| v.to_display_string()),
                    _ => None,
                };
                if let (300..=399, Some(loc)) = (status, location) {
                    return ndjson(200, crate::native::topcoat::frame(vec![("t", Value::Str("redirect".into())), ("location", Value::Str(loc.into()))]));
                }
                if status >= 400 {
                    let body = match o.get("body") {
                        Some(Value::Str(b)) => b,
                        _ => String::new().into(),
                    };
                    return ndjson(200, error_frame(status, &body));
                }
            }
        }
    }
    resp
}

/// Calls a Fogu function, or the `handle` method of a native middleware.
/// A runtime error comes back as `Err(error value)`.
fn invoke(interp: &Interpreter, env: &Rc<RefCell<Environment>>, f: &Value, args: Vec<Value>) -> Result<Value, Value> {
    let r = match f {
        Value::Object(o) => o.call("handle", &args),
        _ => interp.call_value(f.clone(), args, Rc::clone(env)),
    };
    r.map_err(|e| {
        let message = e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
        Value::Error(Rc::new(ErrorValue { message, cause: Value::Nil, trace: Vec::new() }))
    })
}

fn err_message(e: &Value) -> String {
    match e {
        Value::Error(e) => e.message.clone(),
        other => other.to_display_string(),
    }
}

/// Reads a request field from a `Request` object or a plain map.
fn req_field(v: &Value, field: &str) -> Option<Value> {
    match v {
        Value::Object(o) => o.get(field),
        Value::Map(m) => m.borrow().get(field).cloned(),
        _ => None,
    }
}

fn req_str(v: &Value, field: &str) -> Option<String> {
    match req_field(v, field) {
        Some(Value::Str(s)) => Some(s.into_string()),
        _ => None,
    }
}

fn content_type_of(s: &str) -> &'static str {
    // simple HTML: starts with `<` and contains a `>`
    if s.trim_start().starts_with('<') && s.contains('>') {
        "text/html; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    }
}

/// Any handler result → `Response` (`ctx.set.status` applies to plain values).
fn normalize_response(v: Value, set: &Value) -> Value {
    if let Value::Object(o) = &v {
        if matches!(o.name().as_str(), "Response" | "SseStream" | "WsAccept") {
            return v;
        }
    }
    let status = match field_of(set, "status") {
        Value::Int(n) => n,
        _ => 200,
    };
    let (ctype, body) = match v {
        Value::Nil => ("text/plain; charset=utf-8", String::new()),
        Value::Str(s) => (content_type_of(&s), s.to_string()),
        other => ("application/json", crate::native::json::stringify_value(&other)),
    };
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), ctype.to_string());
    let status = if status == 200 && body.is_empty() { 204 } else { status };
    NativeObjectRef::new(ResponseValue { status, headers, body, file: None }).to_value()
}

/// Adds `ctx.set.headers` to the response.
fn apply_set(resp: Value, set: &Value) -> Value {
    let extra = match field_of(set, "headers") {
        Value::Map(h) if !h.borrow().is_empty() => h,
        _ => return resp,
    };
    let Value::Object(o) = &resp else { return resp };
    // web responses, streams and accepts are changed in place
    if matches!(o.name().as_str(), "Response" | "SseStream" | "WsAccept") {
        let inner = o.inner();
        let mut b = inner.borrow_mut();
        let any = b.as_any_mut();
        let target = if let Some(r) = any.downcast_mut::<ResponseValue>() {
            Some(&mut r.headers)
        } else if let Some(s) = any.downcast_mut::<SseValue>() {
            Some(&mut s.headers)
        } else {
            any.downcast_mut::<WsAccept>().map(|w| &mut w.headers)
        };
        if let Some(h) = target {
            for (k, v) in extra.borrow().iter() {
                h.insert(k.to_lowercase(), v.to_display_string());
            }
            drop(b);
            return resp;
        }
    }
    let status = match o.get("status") {
        Some(Value::Int(n)) => n,
        _ => 200,
    };
    let body = match o.get("body") {
        Some(Value::Str(s)) => s,
        _ => String::new().into(),
    };
    let mut headers: HashMap<String, String> = match o.get("headers") {
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect(),
        _ => HashMap::new(),
    };
    for (k, v) in extra.borrow().iter() {
        headers.insert(k.to_lowercase(), v.to_display_string());
    }
    NativeObjectRef::new(ResponseValue { status, headers, body: body.into_string(), file: None }).to_value()
}

fn field_of(v: &Value, k: &str) -> Value {
    match v {
        Value::Map(m) => m.borrow().get(k).cloned().unwrap_or(Value::Nil),
        _ => Value::Nil,
    }
}

pub fn text_response(status: i64, body: &str) -> Value {
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain; charset=utf-8".to_string());
    NativeObjectRef::new(ResponseValue { status, headers, body: body.to_string(), file: None }).to_value()
}

pub fn json_response(status: i64, body: &Value) -> Value {
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    let body = crate::native::json::stringify_value(body);
    NativeObjectRef::new(ResponseValue { status, headers, body, file: None }).to_value()
}

/// `ctx.set.headers[name] = value` from native middleware.
pub fn set_header(ctx: &Value, name: &str, value: &str) {
    if let Value::Map(h) = field_of(&field_of(ctx, "set"), "headers") {
        h.borrow_mut().insert(name.to_string(), Value::Str(value.to_string().into()));
    }
}

// ================= NATIVE MIDDLEWARES =================

/// `web.cors({ origin, methods, headers, credentials, maxAge })`.
struct Cors {
    origin: String,
    methods: String,
    headers: String,
    credentials: bool,
    max_age: Option<i64>,
}

fn cors(args: &[Value]) -> Result<Value, String> {
    crate::native::util::max_args(args, 1, "cors")?;
    let opts = match args.first() {
        None | Some(Value::Nil) => HashMap::new(),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(other) => return Err(format!("cors(options?) expects a map, got '{}'", other.type_name())),
    };
    let s = |k: &str, d: &str| -> String {
        match opts.get(k) {
            Some(Value::Str(s)) => s.to_string(),
            Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect::<Vec<_>>().join(", "),
            _ => d.to_string(),
        }
    };
    Ok(NativeObjectRef::new(Cors {
        origin: s("origin", "*"),
        methods: s("methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS"),
        headers: s("headers", "Content-Type, Authorization"),
        credentials: opts.get("credentials").is_some_and(|v| v.is_truthy()),
        max_age: match opts.get("maxAge") {
            Some(Value::Int(n)) => Some(*n),
            _ => None,
        },
    })
    .to_value())
}

impl FoguObject for Cors {
    fn name(&self) -> &str {
        "Middleware"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        if method != "handle" {
            return Err(format!("method '{}' does not exist on cors middleware", method));
        }
        let ctx = args.first().cloned().unwrap_or(Value::Nil);
        // with a list of origins, echo the request origin when allowed
        let origin = if self.origin.contains(',') {
            let req_origin = field_of(&field_of(&ctx, "headers"), "origin").to_display_string();
            if self.origin.split(',').any(|o| o.trim() == req_origin) { req_origin } else { String::new() }
        } else {
            self.origin.clone()
        };
        if !origin.is_empty() {
            set_header(&ctx, "access-control-allow-origin", &origin);
            if origin != "*" {
                set_header(&ctx, "vary", "Origin");
            }
        }
        if self.credentials {
            set_header(&ctx, "access-control-allow-credentials", "true");
        }
        if field_of(&ctx, "method").to_display_string() == "OPTIONS" {
            set_header(&ctx, "access-control-allow-methods", &self.methods);
            set_header(&ctx, "access-control-allow-headers", &self.headers);
            if let Some(age) = self.max_age {
                set_header(&ctx, "access-control-max-age", &age.to_string());
            }
            // preflight: answered here
            let mut headers = HashMap::new();
            headers.insert("content-length".to_string(), "0".to_string());
            return Ok(NativeObjectRef::new(ResponseValue { status: 204, headers, body: String::new(), file: None }).to_value());
        }
        Ok(Value::Nil)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["handle"]
    }
}

/// `web.logger()`: prints `GET /path 200 1.2ms` when the response is ready.
struct Logger;

impl FoguObject for Logger {
    fn name(&self) -> &str {
        "Middleware"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let ctx = args.first().cloned().unwrap_or(Value::Nil);
        match method {
            "handle" => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros() as i64)
                    .unwrap_or(0);
                if let Value::Map(c) = &ctx {
                    c.borrow_mut().insert("__start_us".into(), Value::Int(now));
                }
                Ok(Value::Nil)
            }
            "after" => {
                let start = match field_of(&ctx, "__start_us") {
                    Value::Int(n) => n,
                    _ => 0,
                };
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros() as i64)
                    .unwrap_or(0);
                let status = match args.get(1) {
                    Some(Value::Object(o)) => o.get("status").map(|s| s.to_display_string()).unwrap_or_default(),
                    _ => String::new(),
                };
                println!(
                    "{} {} {} {:.1}ms",
                    field_of(&ctx, "method").to_display_string(),
                    field_of(&ctx, "path").to_display_string(),
                    status,
                    (now - start) as f64 / 1000.0
                );
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on logger middleware", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["handle", "after"]
    }
}

// ================= TYPES =================

pub struct ResponseValue {
    pub status: i64,
    pub headers: HashMap<String, String>,
    pub body: String,
    /// `web.file`: the server thread sends this file
    pub file: Option<PathBuf>,
}

/// The payload of special responses for the server thread (see `http::send_response`).
pub fn take_special(o: &NativeObjectRef) -> Option<fogu_http::PlainResponse> {
    // only these types can be downcast (others do not implement `as_any`)
    if !matches!(o.name().as_str(), "Response" | "SseStream" | "WsAccept") {
        return None;
    }
    let inner = o.inner();
    let mut b = inner.borrow_mut();
    let any = b.as_any_mut();
    if let Some(r) = any.downcast_mut::<ResponseValue>() {
        let file = r.file.clone()?;
        return Some(fogu_http::PlainResponse { status: r.status, headers: r.headers.clone(), body: Vec::new(), file: Some(file), stream: None, upgrade: None });
    }
    if let Some(s) = any.downcast_mut::<SseValue>() {
        let rx = s.rx.take()?;
        return Some(fogu_http::PlainResponse { status: 200, headers: s.headers.clone(), body: Vec::new(), file: None, stream: Some(rx), upgrade: None });
    }
    if let Some(w) = any.downcast_mut::<WsAccept>() {
        let tx = w.tx.take()?;
        return Some(fogu_http::PlainResponse { status: 101, headers: w.headers.clone(), body: Vec::new(), file: None, stream: None, upgrade: Some(tx) });
    }
    None
}

/// `web.sse()`: `send(data, { event, id })`, `comment(text)`, `close()`, field `open`.
pub struct SseValue {
    tx: tokio::sync::mpsc::UnboundedSender<bytes::Bytes>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>>,
    headers: HashMap<String, String>,
    sent: i64,
}

impl FoguObject for SseValue {
    fn name(&self) -> &str {
        "SseStream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "open" => Value::Bool(!self.tx.is_closed()),
            "sent" => Value::Int(self.sent),
            "status" => Value::Int(200),
            "headers" => map(self.headers.iter().map(|(k, v)| (k.as_str(), Value::Str(v.clone().into()))).collect()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let push = |s: &mut Self, text: String| -> Result<Value, String> {
            s.tx.send(bytes::Bytes::from(text)).map_err(|_| io_err!("sse: the client disconnected"))?;
            Ok(Value::Nil)
        };
        match method {
            // send(data, { event, id }): str as is, other values as JSON
            "send" => {
                crate::native::util::max_args(args, 2, "send")?;
                let data = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    Some(v) => crate::native::json::stringify_value(v).into(),
                    None => String::new().into(),
                };
                let mut out = String::new();
                if let Some(Value::Map(o)) = args.get(1) {
                    let o = o.borrow();
                    if let Some(e) = o.get("event").filter(|v| !matches!(v, Value::Nil)) {
                        out.push_str(&format!("event: {}\n", e.to_display_string().replace('\n', " ")));
                    }
                    if let Some(i) = o.get("id").filter(|v| !matches!(v, Value::Nil)) {
                        out.push_str(&format!("id: {}\n", i.to_display_string().replace('\n', " ")));
                    }
                }
                for line in data.split('\n') {
                    out.push_str(&format!("data: {}\n", line.trim_end_matches('\r')));
                }
                out.push('\n');
                self.sent += 1;
                push(self, out)
            }
            // comment(text): keeps idle connections alive
            "comment" => {
                let text = match args.first() {
                    Some(v) => v.to_display_string(),
                    None => String::new(),
                };
                push(self, format!(": {}\n\n", text.replace('\n', " ")))
            }
            "close" => {
                let (tx, _) = tokio::sync::mpsc::unbounded_channel();
                self.tx = tx; // dropping the sender ends the response
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on SseStream (send comment close)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["send", "comment", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The 101 answer of a WebSocket route (internal).
pub struct WsAccept {
    tx: Option<std_mpsc::Sender<crate::native::websocket::WsIo>>,
    headers: HashMap<String, String>,
}

impl FoguObject for WsAccept {
    fn name(&self) -> &str {
        "WsAccept"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "status" => Some(Value::Int(101)),
            "headers" => Some(map(self.headers.iter().map(|(k, v)| (k.as_str(), Value::Str(v.clone().into()))).collect())),
            _ => None,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
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
                let mut m = HashMap::new();
                for (k, v) in &self.headers {
                    m.insert(k.clone(), Value::Str(v.clone().into()));
                }
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
    use super::{parse_query, under_prefix, url_decode};
    use crate::runtime::value::Value;

    #[test]
    fn helpers() {
        assert_eq!(url_decode("a%20b+c%2F"), "a b c/");
        assert_eq!(url_decode("100%"), "100%");
        assert!(under_prefix("/admin", "/admin"));
        assert!(under_prefix("/admin/", "/admin/x"));
        assert!(!under_prefix("/admin", "/administrator"));
        let q = parse_query("a=1&b=x%20y&flag");
        assert!(matches!(q.get("b"), Some(Value::Str(s)) if s == "x y"));
        assert!(matches!(q.get("flag"), Some(Value::Str(s)) if s.is_empty()));
    }
}
