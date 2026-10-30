//! `fogu:surrealdb` — a SurrealDB client written on the runtime's networking
//! (no SurrealDB crate): its JSON-RPC protocol over WebSocket (`ws://`,
//! `wss://`: a session, live queries) or over HTTP (`http://`, `https://`:
//! `POST /rpc`, the session kept by the client).
//!
//! ```fogu
//! import surrealdb from "fogu:surrealdb"
//! const db = surrealdb.connect("ws://127.0.0.1:8000", { namespace: "app", database: "app", user: "root", pass: "root" })?
//! db.create("person:ana", { age: 30 })?
//! print(db.query("SELECT * FROM person WHERE age > $min", { min: 18 })?)   // one result per statement
//! const live = db.live("person")?
//! print(live.next(5s))                                                    // { action, record, result }
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, json_to_value, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e = HashMap::new();
    e.insert("connect".to_string(), nfn_var("connect", 0, connect));
    ModuleValue {
        name: "fogu:surrealdb".into(),
        exports: RefCell::new(e),
    }
}

fn to_json(v: &Value) -> Json {
    serde_json::from_str(&stringify_value(v)).unwrap_or(Json::Null)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// An RPC error object → its message.
fn rpc_error(e: &Json) -> String {
    match e.get("message").and_then(Json::as_str) {
        Some(m) => m.to_string(),
        None => e.to_string(),
    }
}

// ---------------------------------------------------------------------
// WebSocket transport
// ---------------------------------------------------------------------

type Slot = Arc<Mutex<Option<Result<Json, String>>>>;
type Inbox = Arc<Mutex<VecDeque<Json>>>;

#[derive(Default)]
struct WsShared {
    out: Mutex<Option<std::sync::mpsc::Sender<String>>>,
    pending: Mutex<HashMap<u64, Slot>>,
    lives: Mutex<HashMap<String, Inbox>>,
    closed: AtomicBool,
    error: Mutex<Option<String>>,
    next: AtomicU64,
}

impl WsShared {
    /// Sends a request; its answer lands in the returned slot.
    fn send(&self, method: &str, params: Json) -> Result<Slot, String> {
        if let Some(e) = lock(&self.error).clone() {
            return Err(e);
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let slot: Slot = Arc::default();
        lock(&self.pending).insert(id, Arc::clone(&slot));
        let msg = json!({ "id": id, "method": method, "params": params }).to_string();
        let sent = lock(&self.out).as_ref().is_some_and(|o| o.send(msg).is_ok());
        if !sent {
            lock(&self.pending).remove(&id);
            return Err("the connection is closed".into());
        }
        Ok(slot)
    }

    /// For the setup in `connect` (off the interpreter thread).
    fn call_blocking(&self, method: &str, params: Json, timeout: Duration) -> Result<Json, String> {
        let slot = self.send(method, params)?;
        let until = Instant::now() + timeout;
        loop {
            if let Some(r) = lock(&slot).take() {
                return r;
            }
            if Instant::now() >= until {
                return Err(format!("{}: no answer after {:?}", method, timeout));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn fail_all(&self, why: &str) {
        *lock(&self.error) = Some(why.to_string());
        for (_, slot) in lock(&self.pending).drain() {
            *lock(&slot) = Some(Err(why.to_string()));
        }
        sched::wake();
    }
}

fn ws_loop(mut conn: crate::native::ws_client::Dialed, shared: Arc<WsShared>, out: std::sync::mpsc::Receiver<String>) {
    use tungstenite::Message;
    conn.ws.get_ref().tcp().set_read_timeout(Some(Duration::from_millis(20))).ok();
    let mut last_ping = Instant::now();
    let why = loop {
        if shared.closed.load(Ordering::SeqCst) {
            let _ = conn.ws.close(None);
            let _ = conn.ws.flush();
            break "the connection is closed".to_string();
        }
        while let Ok(text) = out.try_recv() {
            if let Err(e) = conn.ws.send(Message::Text(text.into())) {
                shared.fail_all(&format!("connection lost ({})", e));
                return;
            }
        }
        // keep idle connections (and proxies) alive
        if last_ping.elapsed() > Duration::from_secs(30) {
            let _ = conn.ws.send(Message::Ping(Vec::new().into()));
            last_ping = Instant::now();
        }
        let msg = match conn.ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(e) => break format!("connection lost ({})", e),
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
            Message::Close(_) => break "the server closed the connection".to_string(),
            _ => continue,
        };
        let Ok(j) = serde_json::from_str::<Json>(&text) else { continue };
        match j.get("id").and_then(Json::as_u64) {
            Some(id) => {
                if let Some(slot) = lock(&shared.pending).remove(&id) {
                    *lock(&slot) = Some(match j.get("error") {
                        Some(e) if !e.is_null() => Err(rpc_error(e)),
                        _ => Ok(j.get("result").cloned().unwrap_or(Json::Null)),
                    });
                }
            }
            // a live query notification: { result: { id, action, record, result } }
            None => {
                let n = j.get("result").cloned().unwrap_or(Json::Null);
                if let Some(live) = n.get("id").and_then(Json::as_str) {
                    // kept even before `live()` has its answer (a LIVE SELECT run with query())
                    let inbox = Arc::clone(lock(&shared.lives).entry(live.to_string()).or_default());
                    lock(&inbox).push_back(n);
                }
            }
        }
        sched::wake();
    };
    shared.fail_all(&why);
}

// ---------------------------------------------------------------------
// HTTP transport
// ---------------------------------------------------------------------

struct Http {
    client: reqwest::Client,
    url: String,
}

/// What the HTTP transport sends with each request (it has no session).
#[derive(Clone, Default)]
struct Session {
    namespace: Option<String>,
    database: Option<String>,
    token: Option<String>,
    basic: Option<(String, String)>,
    vars: serde_json::Map<String, Json>,
}

async fn http_call(client: reqwest::Client, url: String, s: Session, method: String, params: Json, timeout: Duration) -> Result<Json, String> {
    let mut req = client
        .post(&url)
        .timeout(timeout)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .body(json!({ "id": 1, "method": method, "params": params }).to_string());
    if let Some(ns) = &s.namespace {
        req = req.header("surreal-ns", ns);
    }
    if let Some(db) = &s.database {
        req = req.header("surreal-db", db);
    }
    req = match (&s.token, &s.basic) {
        (Some(t), _) => req.bearer_auth(t),
        (None, Some((u, p))) => req.basic_auth(u, Some(p)),
        _ => req,
    };
    let res = req.send().await.map_err(|e| format!("{} ({})", e, url))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    let j: Json = serde_json::from_str(&text).map_err(|_| format!("HTTP {}: {}", status.as_u16(), text.chars().take(200).collect::<String>()))?;
    match j.get("error") {
        Some(e) if !e.is_null() => Err(rpc_error(e)),
        _ => Ok(j.get("result").cloned().unwrap_or(Json::Null)),
    }
}

// ---------------------------------------------------------------------
// connect
// ---------------------------------------------------------------------

enum Transport {
    Ws(Arc<WsShared>),
    Http(Arc<Http>),
}

struct Client {
    url: String,
    transport: Transport,
    session: Arc<Mutex<Session>>,
    timeout: Duration,
}

/// `connect(url?, { namespace, database, user, pass, auth, token, define, timeout = 10s })`
fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "surrealdb.connect")?;
    let (url, o) = match (args.first(), args.get(1)) {
        (Some(Value::Map(m)), None) => (None, m.borrow().clone()),
        (u, o) => (
            u.filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string()),
            match o {
                None | Some(Value::Nil) => HashMap::new(),
                Some(Value::Map(m)) => m.borrow().clone(),
                Some(other) => return Err(format!("surrealdb.connect: options must be a map, got '{}'", other.type_name())),
            },
        ),
    };
    let s = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let mut url = url.or_else(|| env("SURREAL_URL")).unwrap_or_else(|| "ws://127.0.0.1:8000".into());
    let scheme = url.split("://").next().unwrap_or("").to_ascii_lowercase();
    if !matches!(scheme.as_str(), "ws" | "wss" | "http" | "https") {
        return Err(format!("surrealdb.connect: the url must start with ws://, wss://, http:// or https://, got '{}'", url));
    }
    if !url.trim_end_matches('/').ends_with("/rpc") {
        url = format!("{}/rpc", url.trim_end_matches('/'));
    }
    let timeout = match o.get("timeout") {
        None | Some(Value::Nil) => Duration::from_secs(10),
        Some(t) => duration_arg(t, "surrealdb.connect timeout")?,
    };
    let namespace = s("namespace").or_else(|| s("ns")).or_else(|| env("SURREAL_NS"));
    let database = s("database").or_else(|| s("db")).or_else(|| env("SURREAL_DB"));
    let user = s("user").or_else(|| s("username")).or_else(|| env("SURREAL_USER"));
    let pass = s("pass").or_else(|| s("password")).or_else(|| env("SURREAL_PASS"));
    // signin: { user, pass } for a root user, or the raw `auth` map (ns, db, ac, record access vars…)
    let signin = match o.get("auth") {
        Some(v @ Value::Map(_)) => Some(to_json(v)),
        _ => match (&user, &pass) {
            (Some(u), Some(p)) => Some(json!({ "user": u, "pass": p })),
            _ => None,
        },
    };
    let token = s("token");
    let define = o.get("define").is_some_and(Value::is_truthy);
    let ws = scheme.starts_with("ws");
    let session = Session { namespace: namespace.clone(), database: database.clone(), token: token.clone(), ..Default::default() };
    let url2 = url.clone();
    let r = crate::native::rt::offload(move || -> Result<(Transport, Session), String> {
        let mut session = session;
        let call: Box<dyn Fn(&str, Json, &Session) -> Result<Json, String>>;
        let transport = if ws {
            let dialed = crate::native::ws_client::dial(&url2, &[], Some("json"), timeout).map_err(|e| e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(&e).to_string())?;
            let shared = Arc::new(WsShared::default());
            let (tx, rx) = std::sync::mpsc::channel();
            *lock(&shared.out) = Some(tx);
            let sh = Arc::clone(&shared);
            std::thread::Builder::new().name("fogu-surrealdb".into()).spawn(move || ws_loop(dialed, sh, rx)).map_err(|e| e.to_string())?;
            let sh = Arc::clone(&shared);
            call = Box::new(move |m, p, _| sh.call_blocking(m, p, timeout));
            Transport::Ws(shared)
        } else {
            let client = reqwest::Client::builder().build().map_err(|e| e.to_string())?;
            let http = Arc::new(Http { client, url: url2.clone() });
            let h = Arc::clone(&http);
            call = Box::new(move |m, p, s| crate::native::rt::tokio().block_on(http_call(h.client.clone(), h.url.clone(), s.clone(), m.to_string(), p, timeout)));
            Transport::Http(http)
        };
        if let Some(t) = &token {
            call("authenticate", json!([t]), &session)?;
        }
        if let Some(params) = signin {
            let t = call("signin", json!([params]), &session).map_err(|e| format!("signin: {}", e))?;
            session.token = t.as_str().map(String::from);
        }
        if define {
            if let Some(ns) = &namespace {
                let mut q = format!("DEFINE NAMESPACE IF NOT EXISTS `{}`; USE NS `{}`;", ns.replace('`', ""), ns.replace('`', ""));
                if let Some(db) = &database {
                    q.push_str(&format!(" DEFINE DATABASE IF NOT EXISTS `{}`;", db.replace('`', "")));
                }
                let r = call("query", json!([q]), &Session { database: None, ..session.clone() })?;
                statements(r, false).map_err(|e| format!("define: {}", e))?;
            }
        }
        if ws && (namespace.is_some() || database.is_some()) {
            call("use", json!([namespace, database]), &session)?;
        }
        Ok((transport, session))
    })?;
    let (transport, session) = r.map_err(|e| io_err!("surrealdb: {} ({})", e, url))?;
    Ok(NativeObjectRef::new(Client { url, transport, session: Arc::new(Mutex::new(session)), timeout }).to_value())
}

/// The results of a `query` (one per statement) → their values, or the first error.
fn statements(r: Json, raw: bool) -> Result<Json, String> {
    let Json::Array(list) = r else { return Ok(r) };
    if raw {
        return Ok(Json::Array(list));
    }
    let mut out = Vec::new();
    for (i, st) in list.into_iter().enumerate() {
        if st.get("status").and_then(Json::as_str) == Some("ERR") {
            let msg = st.get("result").map(|m| m.as_str().map(String::from).unwrap_or_else(|| m.to_string())).unwrap_or_default();
            return Err(format!("statement {}: {}", i + 1, msg));
        }
        out.push(st.get("result").cloned().unwrap_or(Json::Null));
    }
    Ok(Json::Array(out))
}

impl Client {
    /// Sends `method`; `finish` turns the answer into the Fogu result.
    fn request(&self, method: &str, params: Json, finish: impl Fn(Json) -> Result<Value, String> + 'static) -> Result<Poller, String> {
        let what = format!("surrealdb.{}", method);
        let timeout = self.timeout;
        match &self.transport {
            Transport::Ws(sh) => {
                let slot = sh.send(method, params).map_err(|e| io_err!("{}: {}", what, e))?;
                let until = Instant::now() + timeout;
                Ok(Box::new(move || {
                    let r = lock(&slot).take();
                    match r {
                        Some(Ok(j)) => Some(finish(j).map_err(|e| io_err!("{}: {}", what, e))),
                        Some(Err(e)) => Some(Err(io_err!("{}: {}", what, e))),
                        None if Instant::now() >= until => Some(Err(io_err!("{}: no answer after {:?}", what, timeout))),
                        None => {
                            sched::add_timer(Instant::now() + Duration::from_millis(20));
                            None
                        }
                    }
                }))
            }
            Transport::Http(h) => {
                let s = lock(&self.session).clone();
                let pending = crate::native::rt::spawn(http_call(h.client.clone(), h.url.clone(), s, method.to_string(), params, timeout));
                Ok(Box::new(move || {
                    Some(match pending.try_take()? {
                        Ok(j) => finish(j).map_err(|e| io_err!("{}: {}", what, e)),
                        Err(e) => Err(io_err!("{}: {}", what, e)),
                    })
                }))
            }
        }
    }

    fn is_ws(&self) -> bool {
        matches!(self.transport, Transport::Ws(_))
    }
}

fn arg_json(args: &[Value], i: usize) -> Option<Json> {
    args.get(i).filter(|v| !matches!(v, Value::Nil)).map(to_json)
}

fn value(j: Json) -> Result<Value, String> {
    Ok(json_to_value(&j))
}

impl FoguObject for Client {
    fn name(&self) -> &str {
        "SurrealDB"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let s = lock(&self.session);
        let opt = |v: &Option<String>| v.clone().map_or(Value::Nil, Value::str);
        Some(match field {
            "url" => Value::Str(self.url.clone().into()),
            "namespace" => opt(&s.namespace),
            "database" => opt(&s.database),
            "token" => opt(&s.token),
            "transport" => Value::Str(if self.is_ws() { "ws" } else { "http" }.into()),
            "closed" => Value::Bool(match &self.transport {
                Transport::Ws(sh) => sh.closed.load(Ordering::SeqCst) || lock(&sh.error).is_some(),
                Transport::Http(_) => false,
            }),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "close" => {
                if let Transport::Ws(sh) = &self.transport {
                    sh.closed.store(true, Ordering::SeqCst);
                    lock(&sh.out).take();
                }
                Ok(Value::Nil)
            }
            // listen(liveId) — notifications of a LIVE SELECT run with query()
            "listen" => {
                max_args(args, 1, "surrealdb.listen")?;
                let id = str_arg(args, 0, "surrealdb.listen")?;
                let Transport::Ws(sh) = &self.transport else {
                    return Err(io_err!("surrealdb.listen: live queries need a ws:// connection"));
                };
                Ok(live_value(sh, id))
            }
            other => Err(format!("method '{}' does not exist on SurrealDB ({})", other, self.methods().join(" "))),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let what = format!("surrealdb.{}", method);
        let thing = |i: usize| -> Result<String, String> { str_arg(args, i, &what) };
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // query(sql, vars?) → one value per statement; queryRaw keeps { status, result, time }
                "query" | "queryRaw" => {
                    max_args(args, 2, &what)?;
                    let sql = str_arg(args, 0, &what)?;
                    let mut vars = match arg_json(args, 1) {
                        Some(Json::Object(m)) => m,
                        None => serde_json::Map::new(),
                        Some(_) => return Err(format!("{}: vars must be a map", what)),
                    };
                    if !self.is_ws() {
                        for (k, v) in lock(&self.session).vars.iter() {
                            vars.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                    }
                    let raw = method == "queryRaw";
                    self.request("query", json!([sql, vars]), move |j| value(statements(j, raw)?))?
                }
                "select" | "delete" => {
                    max_args(args, 1, &what)?;
                    self.request(method, json!([thing(0)?]), value)?
                }
                "create" | "update" | "upsert" => {
                    max_args(args, 2, &what)?;
                    let mut p = vec![json!(thing(0)?)];
                    p.extend(arg_json(args, 1));
                    self.request(method, Json::Array(p), value)?
                }
                "merge" | "insert" | "insertRelation" => {
                    max_args(args, 2, &what)?;
                    let data = arg_json(args, 1).ok_or_else(|| format!("{}: missing data", what))?;
                    let rpc = if method == "insertRelation" { "insert_relation" } else { method };
                    self.request(rpc, json!([thing(0)?, data]), value)?
                }
                // patch(thing, [{ op, path, value }], diff = false)
                "patch" => {
                    max_args(args, 3, &what)?;
                    let ops = arg_json(args, 1).ok_or_else(|| format!("{}: missing the JSON Patch operations", what))?;
                    let diff = args.get(2).is_some_and(Value::is_truthy);
                    self.request("patch", json!([thing(0)?, ops, diff]), value)?
                }
                // relate(in, relation, out, data?)
                "relate" => {
                    max_args(args, 4, &what)?;
                    let mut p = vec![json!(thing(0)?), json!(thing(1)?), json!(thing(2)?)];
                    p.extend(arg_json(args, 3));
                    self.request("relate", Json::Array(p), value)?
                }
                // run(function, args?) — fn::name or a built-in
                "run" => {
                    max_args(args, 2, &what)?;
                    let a = arg_json(args, 1).unwrap_or(json!([]));
                    self.request("run", json!([thing(0)?, Json::Null, a]), value)?
                }
                "info" | "version" | "ping" => {
                    max_args(args, 0, &what)?;
                    self.request(method, json!([]), value)?
                }
                "use" => {
                    max_args(args, 2, &what)?;
                    let (ns, db) = (arg_json(args, 0), arg_json(args, 1));
                    {
                        let mut s = lock(&self.session);
                        if let Some(Json::String(n)) = &ns {
                            s.namespace = Some(n.clone());
                        }
                        if let Some(Json::String(d)) = &db {
                            s.database = Some(d.clone());
                        }
                    }
                    if !self.is_ws() {
                        return Ok(Some(ready(Ok(Value::Nil))));
                    }
                    self.request("use", json!([ns, db]), |_| Ok(Value::Nil))?
                }
                // signin(params) / signup(params) → the token (used from then on)
                "signin" | "signup" => {
                    max_args(args, 1, &what)?;
                    let p = arg_json(args, 0).ok_or_else(|| format!("{}: missing the credentials map", what))?;
                    let session = Arc::clone(&self.session);
                    self.request(method, json!([p]), move |j| {
                        let t = j.as_str().map(String::from).or_else(|| j.get("token").and_then(Json::as_str).map(String::from));
                        lock(&session).token = t.clone();
                        Ok(t.map_or(Value::Nil, Value::str))
                    })?
                }
                "authenticate" => {
                    max_args(args, 1, &what)?;
                    let t = thing(0)?;
                    lock(&self.session).token = Some(t.clone());
                    if !self.is_ws() {
                        return Ok(Some(ready(Ok(Value::Nil))));
                    }
                    self.request("authenticate", json!([t]), |_| Ok(Value::Nil))?
                }
                "invalidate" => {
                    max_args(args, 0, &what)?;
                    lock(&self.session).token = None;
                    if !self.is_ws() {
                        return Ok(Some(ready(Ok(Value::Nil))));
                    }
                    self.request("invalidate", json!([]), |_| Ok(Value::Nil))?
                }
                // set(name, value) / unset(name) — session variables ($name; RPC `let`/`unset`)
                "set" | "unset" => {
                    max_args(args, 2, &what)?;
                    let name = thing(0)?;
                    {
                        let mut s = lock(&self.session);
                        match method {
                            "set" => {
                                s.vars.insert(name.clone(), arg_json(args, 1).unwrap_or(Json::Null));
                            }
                            _ => {
                                s.vars.remove(&name);
                            }
                        }
                    }
                    if !self.is_ws() {
                        return Ok(Some(ready(Ok(Value::Nil))));
                    }
                    let (rpc, p) = if method == "set" { ("let", json!([name, arg_json(args, 1).unwrap_or(Json::Null)])) } else { ("unset", json!([name])) };
                    self.request(rpc, p, |_| Ok(Value::Nil))?
                }
                // live(table, { diff }) → Live
                "live" => {
                    max_args(args, 2, &what)?;
                    let Transport::Ws(sh) = &self.transport else {
                        return Err(io_err!("{}: live queries need a ws:// connection", what));
                    };
                    let diff = matches!(args.get(1), Some(Value::Map(m)) if m.borrow().get("diff").is_some_and(Value::is_truthy));
                    let sh2 = Arc::clone(sh);
                    // the inbox exists before the first notification can arrive
                    self.request("live", json!([thing(0)?, diff]), move |j| {
                        let id = j.as_str().ok_or("no live query id in the answer")?.to_string();
                        Ok(live_value(&sh2, id))
                    })?
                }
                _ => return Ok(None),
            }))
        })();
        r.transpose()
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "query", "queryRaw", "select", "create", "insert", "insertRelation", "update", "upsert", "merge", "patch", "delete", "relate", "run",
            "live", "listen", "use", "signin", "signup", "authenticate", "invalidate", "set", "unset", "info", "version", "ping", "close",
        ]
    }
}

fn ready(r: Result<Value, String>) -> Poller {
    let mut r = Some(r);
    Box::new(move || r.take())
}

// ---------------------------------------------------------------------
// live queries
// ---------------------------------------------------------------------

fn live_value(sh: &Arc<WsShared>, id: String) -> Value {
    let inbox: Inbox = Arc::clone(lock(&sh.lives).entry(id.clone()).or_default());
    NativeObjectRef::new(Live { id, inbox, shared: Arc::clone(sh), done: false }).to_value()
}

struct Live {
    id: String,
    inbox: Inbox,
    shared: Arc<WsShared>,
    done: bool,
}

impl Live {
    fn take(&mut self) -> Option<Value> {
        let n = lock(&self.inbox).pop_front()?;
        if n.get("action").and_then(Json::as_str) == Some("KILLED") {
            self.done = true;
        }
        let pick = |k: &str| json_to_value(n.get(k).unwrap_or(&Json::Null));
        Some(crate::native::util::map(vec![("action", pick("action")), ("record", pick("record")), ("result", pick("result"))]))
    }
}

impl FoguObject for Live {
    fn name(&self) -> &str {
        "SurrealLive"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "id" => Some(Value::Str(self.id.clone().into())),
            "done" => Some(Value::Bool(self.done)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "tryNext" => Ok(self.take().unwrap_or(Value::Nil)),
            other => Err(format!("method '{}' does not exist on SurrealLive (next tryNext kill)", other)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        match method {
            // next(timeout?) → { action, record, result }; nil after the timeout or once killed
            "next" => Some((|| {
                max_args(args, 1, "live.next")?;
                let until = match args.first() {
                    None | Some(Value::Nil) => None,
                    Some(t) => Some(Instant::now() + duration_arg(t, "live.next timeout")?),
                };
                if let Some(v) = self.take() {
                    return Ok(ready(Ok(v)));
                }
                if self.done {
                    return Ok(ready(Ok(Value::Nil)));
                }
                let (inbox, shared) = (Arc::clone(&self.inbox), Arc::clone(&self.shared));
                Ok(Box::new(move || {
                    if let Some(n) = lock(&inbox).pop_front() {
                        let pick = |k: &str| json_to_value(n.get(k).unwrap_or(&Json::Null));
                        return Some(Ok(crate::native::util::map(vec![("action", pick("action")), ("record", pick("record")), ("result", pick("result"))])));
                    }
                    if let Some(e) = lock(&shared.error).clone() {
                        return Some(Err(io_err!("surrealdb live: {}", e)));
                    }
                    match until {
                        Some(u) if Instant::now() >= u => Some(Ok(Value::Nil)),
                        Some(u) => {
                            sched::add_timer(u.min(Instant::now() + Duration::from_millis(50)));
                            None
                        }
                        None => {
                            sched::add_timer(Instant::now() + Duration::from_millis(50));
                            None
                        }
                    }
                }) as Poller)
            })()),
            "kill" => Some((|| {
                max_args(args, 0, "live.kill")?;
                let slot = self.shared.send("kill", json!([self.id])).map_err(|e| io_err!("live.kill: {}", e))?;
                let (lives, id) = (Arc::clone(&self.shared), self.id.clone());
                self.done = true;
                Ok(Box::new(move || {
                    let Some(r) = lock(&slot).take() else {
                        sched::add_timer(Instant::now() + Duration::from_millis(20));
                        return None;
                    };
                    lock(&lives.lives).remove(&id);
                    Some(r.map(|_| Value::Nil).map_err(|e| io_err!("live.kill: {}", e)))
                }) as Poller)
            })()),
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "kill"]
    }
}
