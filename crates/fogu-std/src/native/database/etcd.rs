//! `fogu:etcd` module — etcd v3 client over gRPC, without a client crate:
//! the official `.proto` files (`native/proto/etcd`, Apache-2.0) are compiled
//! at run time by protox and the calls go through tonic (as `fogu:grpc`) on
//! the shared runtime.
//!
//! ```fogu
//! import etcd from "fogu:etcd"
//!
//! const db = etcd.connect()?                          // $ETCDCTL_ENDPOINTS or http://127.0.0.1:2379
//! db.put("/app/name", "Fogu")?
//! print(db.get("/app/name")?.kvs[0].value)            // Fogu
//! print(db.get("/app/", { prefix: true })?.count)
//! const lease = db.grant(30s)?
//! db.put("/app/token", "xyz", { lease: lease.id })?
//! const w = db.watch("/app/", { prefix: true })?      // w.next(timeout?) → { type, kv, prevKv }
//! const ka = db.keepAlive(lease.id)?                  // ka.next() → { id, ttl }
//! ```
//!
//! Keys and values are text. Network and server failures are error values;
//! wrong arguments stop the program. Several endpoints are balanced (and a
//! failing one is skipped); `{ user, password }` authenticates.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use prost_reflect::{DescriptorPool, DynamicMessage, SerializeOptions};
use serde_json::{json, Value as Json};

use crate::runtime::sched::{self, IoGuard};
use crate::runtime::value::{ModuleValue, Value};

use crate::native::grpc::RawCodec;
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, json_to_value, list, max_args, nfn_var, str_arg};

const PROTOS: &[(&str, &str)] = &[
    ("rpc.proto", include_str!("proto/etcd/rpc.proto")),
    ("kv.proto", include_str!("proto/etcd/kv.proto")),
    ("auth.proto", include_str!("proto/etcd/auth.proto")),
];

fn pool() -> Result<&'static DescriptorPool, String> {
    static POOL: OnceLock<Result<DescriptorPool, String>> = OnceLock::new();
    POOL.get_or_init(|| {
        let sources = PROTOS.iter().map(|(n, s)| (n.to_string(), s.to_string())).collect();
        crate::native::protobuf::compile_sources(sources, "rpc.proto")
    })
    .as_ref()
    .map_err(|e| io_err!("etcd: {}", e))
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

fn b64_bytes(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// A proto3-JSON value → the message's bytes.
fn encode(msg: &str, j: &Json) -> Result<Vec<u8>, String> {
    let desc = pool()?.get_message_by_name(msg).ok_or_else(|| format!("etcd: no message {}", msg))?;
    let m = DynamicMessage::deserialize(desc, j.clone()).map_err(|e| format!("etcd: {}: {}", msg, e))?;
    Ok(crate::native::protobuf::encode_message(&m))
}

/// Bytes → proto3 JSON (field names in camelCase, 64-bit numbers as numbers, defaults included).
fn decode(msg: &str, b: &[u8]) -> Result<Json, String> {
    let desc = pool()?.get_message_by_name(msg).ok_or_else(|| format!("etcd: no message {}", msg))?;
    let m = crate::native::protobuf::decode_message(desc, b)?;
    let opts = SerializeOptions::new().stringify_64_bit_integers(false).skip_default_fields(false);
    m.serialize_with_options(serde_json::value::Serializer, &opts).map_err(|e| io_err!("etcd: {}", e))
}

fn text_of(j: &Json) -> String {
    j.as_str()
        .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn num(j: &Json) -> i64 {
    j.as_i64().or_else(|| j.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

/// A `mvccpb.KeyValue` as `{ key, value, createRevision, modRevision, version, lease }`.
fn kv(j: &Json) -> Json {
    if j.is_null() {
        return Json::Null;
    }
    json!({
        "key": text_of(&j["key"]),
        "value": text_of(&j["value"]),
        "createRevision": num(&j["createRevision"]),
        "modRevision": num(&j["modRevision"]),
        "version": num(&j["version"]),
        "lease": num(&j["lease"]),
    })
}

fn kvs(j: &Json) -> Json {
    Json::Array(j.as_array().map(|a| a.iter().map(kv).collect()).unwrap_or_default())
}

fn revision(j: &Json) -> i64 {
    num(&j["header"]["revision"])
}

// ---------------------------------------------------------------------
// module and configuration
// ---------------------------------------------------------------------

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 0, connect));
    e.insert("defaultEndpoints".into(), nfn_var("defaultEndpoints", 0, |_| Ok(list(default_endpoints().into_iter().map(Value::str).collect()))));
    ModuleValue {
        name: "fogu:etcd".into(),
        exports: RefCell::new(e),
    }
}

fn default_endpoints() -> Vec<String> {
    if let Ok(env) = std::env::var("ETCDCTL_ENDPOINTS") {
        let v: Vec<String> = env.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if !v.is_empty() {
            return v;
        }
    }
    vec!["http://127.0.0.1:2379".into()]
}

#[derive(Clone)]
struct Cfg {
    endpoints: Vec<String>,
    user: Option<String>,
    password: Option<String>,
    tls: bool,
    timeout: Duration,
    connect_timeout: Duration,
}

fn get_s(m: &HashMap<String, Value>, k: &str) -> Option<String> {
    match m.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

/// `connect("http://host:2379" | { endpoints, endpoint, user, password, tls, timeout = 15s, connectTimeout = 5s })`
fn parse_cfg(arg: Option<&Value>) -> Result<Cfg, String> {
    let m: HashMap<String, Value> = match arg {
        None | Some(Value::Nil) => HashMap::new(),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(Value::Str(s)) => HashMap::from([("endpoint".to_string(), Value::Str(s.clone()))]),
        Some(other) => return Err(format!("etcd.connect expects an options map or a url, got '{}'", other.type_name())),
    };
    let mut endpoints: Vec<String> = match m.get("endpoints") {
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        _ => get_s(&m, "endpoint").into_iter().collect(),
    };
    if endpoints.is_empty() {
        endpoints = default_endpoints();
    }
    let tls = m.get("tls").is_some_and(Value::is_truthy);
    for e in endpoints.iter_mut() {
        if !e.starts_with("http://") && !e.starts_with("https://") {
            e.insert_str(0, if tls { "https://" } else { "http://" });
        }
    }
    let user = get_s(&m, "user").or_else(|| std::env::var("ETCDCTL_USER").ok().filter(|u| !u.is_empty()));
    let (user, password) = match user {
        Some(u) => match u.split_once(':') {
            Some((u, p)) => (Some(u.to_string()), Some(p.to_string())),
            None => (Some(u), get_s(&m, "password").or_else(|| std::env::var("ETCDCTL_PASSWORD").ok())),
        },
        None => (None, None),
    };
    let dur = |k: &str, d: Duration| -> Result<Duration, String> {
        match m.get(k) {
            None | Some(Value::Nil) => Ok(d),
            Some(t) => duration_arg(t, &format!("etcd.connect: {}", k)),
        }
    };
    Ok(Cfg {
        tls: tls || endpoints.iter().any(|e| e.starts_with("https://")),
        endpoints,
        user,
        password,
        timeout: dur("timeout", Duration::from_secs(15))?,
        connect_timeout: dur("connectTimeout", Duration::from_secs(5))?,
    })
}

/// What every call needs (Send: it goes to the I/O runtime).
#[derive(Clone)]
struct Conn {
    channel: tonic::transport::Channel,
    cfg: Cfg,
    token: Arc<Mutex<Option<String>>>,
}

fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "etcd.connect")?;
    let cfg = parse_cfg(args.first())?;
    pool()?;
    let mut eps = Vec::new();
    for e in &cfg.endpoints {
        let mut ep = tonic::transport::Endpoint::from_shared(e.clone())
            .map_err(|err| format!("etcd.connect: invalid endpoint '{}': {}", e, err))?
            .connect_timeout(cfg.connect_timeout)
            .tcp_nodelay(true)
            .keep_alive_while_idle(true);
        if e.starts_with("https://") {
            ep = ep.tls_config(tonic::transport::ClientTlsConfig::new().with_native_roots()).map_err(|err| io_err!("etcd.connect: TLS: {}", err))?;
        }
        eps.push(ep);
    }
    let channel = {
        let _g = crate::native::rt::tokio().enter();
        tonic::transport::Channel::balance_list(eps.into_iter())
    };
    let conn = Conn { channel, cfg: cfg.clone(), token: Arc::default() };
    // check the connection (and authenticate) once, like etcdctl
    let c2 = conn.clone();
    crate::native::rt::block_on(async move { c2.unary("/etcdserverpb.Maintenance/Status", "etcdserverpb.StatusRequest", "etcdserverpb.StatusResponse", json!({})).await })??;
    Ok(NativeObjectRef::new(EtcdValue { conn: Some(conn), cfg }).to_value())
}

fn status_text(s: &tonic::Status) -> String {
    if s.message().is_empty() { format!("{:?}", s.code()) } else { s.message().to_string() }
}

impl Conn {
    async fn auth_token(&self) -> Result<Option<String>, String> {
        let Some(user) = self.cfg.user.clone() else { return Ok(None) };
        if let Some(t) = self.token.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(Some(t));
        }
        let body = encode("etcdserverpb.AuthenticateRequest", &json!({ "name": user, "password": self.cfg.password.clone().unwrap_or_default() }))?;
        let out = self.raw("/etcdserverpb.Auth/Authenticate", body, None).await?;
        let token = decode("etcdserverpb.AuthenticateResponse", &out)?["token"].as_str().unwrap_or_default().to_string();
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        Ok(Some(token))
    }

    async fn raw(&self, path: &'static str, body: Vec<u8>, token: Option<String>) -> Result<Vec<u8>, String> {
        let mut g = tonic::client::Grpc::new(self.channel.clone());
        let what = path.rsplit('/').next().unwrap_or(path);
        let call = async {
            g.ready().await.map_err(|e| io_err!("etcd: cannot connect to {}: {}", self.cfg.endpoints.join(","), e))?;
            let mut req = tonic::Request::new(body);
            if let Some(t) = token {
                req.metadata_mut().insert("token", t.parse().map_err(|_| io_err!("etcd: invalid auth token"))?);
            }
            let r = g.unary(req, http::uri::PathAndQuery::from_static(path), RawCodec).await.map_err(|s| {
                if s.code() == tonic::Code::Unavailable {
                    io_err!("etcd.{}: {} ({})", what, status_text(&s), self.cfg.endpoints.join(", "))
                } else {
                    io_err!("etcd.{}: {}", what, status_text(&s))
                }
            })?;
            Ok::<Vec<u8>, String>(r.into_inner())
        };
        tokio::time::timeout(self.cfg.timeout, call).await.map_err(|_| io_err!("etcd.{}: timeout after {:?}", what, self.cfg.timeout))?
    }

    /// One call: request JSON → response JSON.
    async fn unary(&self, path: &'static str, req: &str, resp: &str, body: Json) -> Result<Json, String> {
        let bytes = encode(req, &body)?;
        let token = self.auth_token().await?;
        let out = match self.raw(path, bytes.clone(), token).await {
            // an expired token: log in again once
            Err(e) if self.cfg.user.is_some() && e.contains("token") => {
                self.token.lock().unwrap_or_else(|e| e.into_inner()).take();
                let t = self.auth_token().await?;
                self.raw(path, bytes, t).await?
            }
            r => r?,
        };
        decode(resp, &out)
    }
}

// ---------------------------------------------------------------------
// requests
// ---------------------------------------------------------------------

fn opts(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn truthy(m: &HashMap<String, Value>, k: &str) -> bool {
    m.get(k).is_some_and(Value::is_truthy)
}

fn int_of(m: &HashMap<String, Value>, k: &str) -> Option<i64> {
    match m.get(k) {
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    }
}

fn int_arg(args: &[Value], i: usize, what: &str) -> Result<i64, String> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(format!("{}: expected int, got '{}'", what, other.type_name())),
        None => Err(format!("{}: missing argument", what)),
    }
}

/// The end of the range of keys that start with `prefix`.
fn prefix_end(prefix: &str) -> Vec<u8> {
    let mut b = prefix.as_bytes().to_vec();
    for i in (0..b.len()).rev() {
        if b[i] < 0xff {
            b[i] += 1;
            b.truncate(i + 1);
            return b;
        }
    }
    vec![0]
}

/// `range_end` from `{ prefix }` / `{ rangeEnd }`.
fn range_end(key: &str, o: &HashMap<String, Value>) -> Option<String> {
    if truthy(o, "prefix") {
        Some(b64_bytes(&prefix_end(key)))
    } else {
        get_s(o, "rangeEnd").map(|r| b64(&r))
    }
}

fn range_request(key: &str, o: &HashMap<String, Value>) -> Json {
    let mut r = json!({ "key": b64(key) });
    if let Some(e) = range_end(key, o) {
        r["rangeEnd"] = e.into();
    }
    for (k, f) in [("limit", "limit"), ("revision", "revision"), ("minModRevision", "minModRevision"), ("maxModRevision", "maxModRevision"), ("minCreateRevision", "minCreateRevision"), ("maxCreateRevision", "maxCreateRevision")] {
        if let Some(n) = int_of(o, k) {
            r[f] = n.into();
        }
    }
    for (k, f) in [("keysOnly", "keysOnly"), ("countOnly", "countOnly"), ("serializable", "serializable")] {
        if truthy(o, k) {
            r[f] = true.into();
        }
    }
    if let Some(s) = get_s(o, "sort") {
        r["sortOrder"] = (if matches!(s.as_str(), "desc" | "descend" | "descending") { "DESCEND" } else { "ASCEND" }).into();
        r["sortTarget"] = (match get_s(o, "sortTarget").as_deref() {
            Some("version") => "VERSION",
            Some("create" | "createRevision") => "CREATE",
            Some("mod" | "modRevision") => "MOD",
            Some("value") => "VALUE",
            _ => "KEY",
        })
        .into();
    }
    r
}

fn put_request(key: &str, value: &str, o: &HashMap<String, Value>) -> Json {
    let mut r = json!({ "key": b64(key), "value": b64(value) });
    if let Some(l) = int_of(o, "lease") {
        r["lease"] = l.into();
    }
    for k in ["prevKv", "ignoreValue", "ignoreLease"] {
        if truthy(o, k) {
            r[k] = true.into();
        }
    }
    r
}

fn delete_request(key: &str, o: &HashMap<String, Value>) -> Json {
    let mut r = json!({ "key": b64(key) });
    if let Some(e) = range_end(key, o) {
        r["rangeEnd"] = e.into();
    }
    if truthy(o, "prevKv") {
        r["prevKv"] = true.into();
    }
    r
}

fn get_result(j: &Json) -> Json {
    json!({ "kvs": kvs(&j["kvs"]), "count": num(&j["count"]), "more": j["more"].as_bool().unwrap_or(false), "revision": revision(j) })
}

fn compares(v: Option<&Value>) -> Result<Vec<Json>, String> {
    let arr = match v {
        None | Some(Value::Nil) => return Ok(Vec::new()),
        Some(Value::List(l)) => l.borrow().clone(),
        Some(other) => return Err(format!("txn.compare must be a list, got '{}'", other.type_name())),
    };
    let mut out = Vec::new();
    for item in arr.iter() {
        let Value::Map(m) = item else { return Err("txn.compare items must be maps".into()) };
        let m = m.borrow().clone();
        let key = get_s(&m, "key").ok_or("txn.compare: missing 'key'")?;
        let op = get_s(&m, "op").unwrap_or_else(|| "=".into());
        let result = match op.as_str() {
            "=" | "==" | "equal" => "EQUAL",
            "!=" | "notEqual" => "NOT_EQUAL",
            ">" | "greater" => "GREATER",
            "<" | "less" => "LESS",
            _ => return Err(format!("txn.compare: unknown op '{}'", op)),
        };
        let mut c = json!({ "key": b64(&key), "result": result });
        if let Some(e) = range_end(&key, &m) {
            c["rangeEnd"] = e.into();
        }
        match get_s(&m, "field").as_deref().unwrap_or("value") {
            "value" => {
                c["target"] = "VALUE".into();
                c["value"] = b64(&get_s(&m, "value").unwrap_or_default()).into();
            }
            "version" => {
                c["target"] = "VERSION".into();
                c["version"] = int_of(&m, "version").unwrap_or(0).into();
            }
            "createRevision" => {
                c["target"] = "CREATE".into();
                c["createRevision"] = int_of(&m, "revision").unwrap_or(0).into();
            }
            "modRevision" => {
                c["target"] = "MOD".into();
                c["modRevision"] = int_of(&m, "revision").unwrap_or(0).into();
            }
            "lease" => {
                c["target"] = "LEASE".into();
                c["lease"] = int_of(&m, "lease").unwrap_or(0).into();
            }
            other => return Err(format!("txn.compare: unknown field '{}'", other)),
        }
        out.push(c);
    }
    Ok(out)
}

fn txn_ops(v: Option<&Value>) -> Result<Vec<Json>, String> {
    let arr = match v {
        None | Some(Value::Nil) => return Ok(Vec::new()),
        Some(Value::List(l)) => l.borrow().clone(),
        Some(other) => return Err(format!("txn op list must be a list, got '{}'", other.type_name())),
    };
    let mut out = Vec::new();
    for item in arr.iter() {
        let Value::Map(m) = item else { return Err("txn op items must be maps".into()) };
        let m = m.borrow().clone();
        let key = get_s(&m, "key").ok_or("txn op: missing 'key'")?;
        out.push(match get_s(&m, "type").as_deref().unwrap_or("put") {
            "put" => json!({ "requestPut": put_request(&key, &get_s(&m, "value").unwrap_or_default(), &m) }),
            "get" => json!({ "requestRange": range_request(&key, &m) }),
            "delete" => json!({ "requestDeleteRange": delete_request(&key, &m) }),
            other => return Err(format!("txn op: unknown type '{}'", other)),
        });
    }
    Ok(out)
}

fn txn_result(j: &Json) -> Json {
    let responses: Vec<Json> = j["responses"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    if !r["responseRange"].is_null() {
                        json!({ "type": "get", "kvs": kvs(&r["responseRange"]["kvs"]), "count": num(&r["responseRange"]["count"]) })
                    } else if !r["responsePut"].is_null() {
                        json!({ "type": "put", "revision": revision(&r["responsePut"]) })
                    } else if !r["responseDeleteRange"].is_null() {
                        json!({ "type": "delete", "deleted": num(&r["responseDeleteRange"]["deleted"]) })
                    } else {
                        json!({ "type": "txn" })
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "succeeded": j["succeeded"].as_bool().unwrap_or(false), "responseCount": responses.len(), "revision": revision(j), "responses": responses })
}

// ---------------------------------------------------------------------
// the Etcd value
// ---------------------------------------------------------------------

pub struct EtcdValue {
    conn: Option<Conn>,
    cfg: Cfg,
}

/// A unary call on the I/O runtime, waited for cooperatively.
fn call(conn: &Conn, path: &'static str, req: &'static str, resp: &'static str, body: Json, finish: fn(&Json) -> Json) -> Poller {
    let c = conn.clone();
    let pending = crate::native::rt::spawn(async move { c.unary(path, req, resp, body).await });
    Box::new(move || {
        let r = pending.try_take()?;
        Some(r.map(|j| json_to_value(&finish(&j))))
    })
}

impl FoguObject for EtcdValue {
    fn name(&self) -> &str {
        "Etcd"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "endpoints" => crate::native::util::str_list(self.cfg.endpoints.clone()),
            "user" => self.cfg.user.clone().map_or(Value::Nil, Value::str),
            "tls" => Value::Bool(self.cfg.tls),
            "closed" => Value::Bool(self.conn.is_none()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let conn = self.conn.clone().ok_or_else(|| io_err!("etcd: the connection is closed"));
        match method {
            "close" => {
                self.conn = None;
                Ok(Value::Nil)
            }
            // watch(key, { prefix, rangeEnd, prevKv, revision, progressNotify }) → Watch
            "watch" => {
                max_args(args, 2, "watch")?;
                let key = str_arg(args, 0, "watch")?;
                let o = opts(args, 1, "watch")?;
                let mut create = json!({ "key": b64(&key) });
                if let Some(e) = range_end(&key, &o) {
                    create["rangeEnd"] = e.into();
                }
                if let Some(r) = int_of(&o, "revision") {
                    create["startRevision"] = r.into();
                }
                for k in ["prevKv", "progressNotify"] {
                    if truthy(&o, k) {
                        create[k] = true.into();
                    }
                }
                let first = encode("etcdserverpb.WatchRequest", &json!({ "createRequest": create }))?;
                Ok(NativeObjectRef::new(Stream::start(conn?, "/etcdserverpb.Watch/Watch", vec![first], None, watch_events)).to_value())
            }
            // keepAlive(leaseId) → KeepAlive: refreshes the lease until closed
            "keepAlive" => {
                max_args(args, 1, "keepAlive")?;
                let id = int_arg(args, 0, "keepAlive")?;
                let req = encode("etcdserverpb.LeaseKeepAliveRequest", &json!({ "ID": id }))?;
                Ok(NativeObjectRef::new(Stream::start(conn?, "/etcdserverpb.Lease/LeaseKeepAlive", vec![req.clone()], Some(req), keepalive_events)).to_value())
            }
            other => Err(format!("method '{}' does not exist on Etcd ({})", other, self.methods().join(" "))),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let conn = match &self.conn {
            Some(c) => c,
            None if matches!(method, "close" | "watch" | "keepAlive") => return None,
            None => return Some(Err(io_err!("etcd: the connection is closed"))),
        };
        let out: Result<Option<Poller>, String> = (|| {
            Ok(Some(match method {
                // get(key, { prefix, rangeEnd, limit, revision, keysOnly, countOnly, serializable, sort, sortTarget, min/max…Revision })
                "get" => {
                    max_args(args, 2, "get")?;
                    let key = str_arg(args, 0, "get")?;
                    let o = opts(args, 1, "get")?;
                    call(conn, "/etcdserverpb.KV/Range", "etcdserverpb.RangeRequest", "etcdserverpb.RangeResponse", range_request(&key, &o), get_result)
                }
                // put(key, value, { lease, prevKv, ignoreValue, ignoreLease }) → { revision, prevKv }
                "put" => {
                    max_args(args, 3, "put")?;
                    let key = str_arg(args, 0, "put")?;
                    let value = match args.get(1) {
                        Some(Value::Str(s)) => s.clone(),
                        Some(v) => v.to_display_string().into(),
                        None => return Err("put(key, value, options?)".into()),
                    };
                    let o = opts(args, 2, "put")?;
                    call(conn, "/etcdserverpb.KV/Put", "etcdserverpb.PutRequest", "etcdserverpb.PutResponse", put_request(&key, &value, &o), |j| json!({ "revision": revision(j), "prevKv": kv(&j["prevKv"]) }))
                }
                // delete(key, { prefix, rangeEnd, prevKv }) → { deleted, prevKvs }
                "delete" => {
                    max_args(args, 2, "delete")?;
                    let key = str_arg(args, 0, "delete")?;
                    let o = opts(args, 1, "delete")?;
                    call(conn, "/etcdserverpb.KV/DeleteRange", "etcdserverpb.DeleteRangeRequest", "etcdserverpb.DeleteRangeResponse", delete_request(&key, &o), |j| {
                        json!({ "deleted": num(&j["deleted"]), "prevKvs": kvs(&j["prevKvs"]), "revision": revision(j) })
                    })
                }
                // grant(ttl) → { id, ttl }
                "grant" => {
                    max_args(args, 1, "grant")?;
                    let ttl = match args.first() {
                        Some(Value::Int(s)) => (*s).max(1),
                        Some(v) => duration_arg(v, "grant: ttl")?.as_secs().max(1) as i64,
                        None => return Err("grant(ttl) expects a duration".into()),
                    };
                    call(conn, "/etcdserverpb.Lease/LeaseGrant", "etcdserverpb.LeaseGrantRequest", "etcdserverpb.LeaseGrantResponse", json!({ "TTL": ttl }), |j| json!({ "id": num(&j["ID"]), "ttl": num(&j["TTL"]) }))
                }
                "revoke" => {
                    max_args(args, 1, "revoke")?;
                    let id = int_arg(args, 0, "revoke")?;
                    call(conn, "/etcdserverpb.Lease/LeaseRevoke", "etcdserverpb.LeaseRevokeRequest", "etcdserverpb.LeaseRevokeResponse", json!({ "ID": id }), |_| Json::Null)
                }
                // ttl(leaseId, { keys }) → { ttl, grantedTtl, granted, keys }
                "ttl" => {
                    max_args(args, 2, "ttl")?;
                    let id = int_arg(args, 0, "ttl")?;
                    let keys = truthy(&opts(args, 1, "ttl")?, "keys");
                    call(conn, "/etcdserverpb.Lease/LeaseTimeToLive", "etcdserverpb.LeaseTimeToLiveRequest", "etcdserverpb.LeaseTimeToLiveResponse", json!({ "ID": id, "keys": keys }), |j| {
                        let ttl = num(&j["TTL"]);
                        let keys: Vec<Json> = j["keys"].as_array().map(|a| a.iter().map(|k| Json::String(text_of(k))).collect()).unwrap_or_default();
                        json!({ "ttl": ttl, "grantedTtl": num(&j["grantedTTL"]), "granted": ttl > 0, "keys": keys })
                    })
                }
                // txn({ compare, success, failure }) → { succeeded, responseCount, revision, responses }
                "txn" => {
                    max_args(args, 1, "txn")?;
                    let Some(Value::Map(spec)) = args.first() else { return Err("txn expects a map { compare, success, failure }".into()) };
                    let spec = spec.borrow().clone();
                    let body = json!({ "compare": compares(spec.get("compare"))?, "success": txn_ops(spec.get("success"))?, "failure": txn_ops(spec.get("failure"))? });
                    call(conn, "/etcdserverpb.KV/Txn", "etcdserverpb.TxnRequest", "etcdserverpb.TxnResponse", body, txn_result)
                }
                "status" => {
                    max_args(args, 0, "status")?;
                    call(conn, "/etcdserverpb.Maintenance/Status", "etcdserverpb.StatusRequest", "etcdserverpb.StatusResponse", json!({}), |j| {
                        json!({ "version": j["version"], "dbSize": num(&j["dbSize"]), "leader": num(&j["leader"]), "raftIndex": num(&j["raftIndex"]), "raftTerm": num(&j["raftTerm"]), "revision": revision(j), "isLearner": j["isLearner"] })
                    })
                }
                "memberList" => {
                    max_args(args, 0, "memberList")?;
                    call(conn, "/etcdserverpb.Cluster/MemberList", "etcdserverpb.MemberListRequest", "etcdserverpb.MemberListResponse", json!({}), |j| {
                        Json::Array(
                            j["members"]
                                .as_array()
                                .map(|a| a.iter().map(|m| json!({ "id": num(&m["ID"]), "name": m["name"], "peerUrls": m["peerURLs"], "clientUrls": m["clientURLs"], "isLearner": m["isLearner"] })).collect())
                                .unwrap_or_default(),
                        )
                    })
                }
                // compact(revision, { physical })
                "compact" => {
                    max_args(args, 2, "compact")?;
                    let rev = int_arg(args, 0, "compact")?;
                    let physical = truthy(&opts(args, 1, "compact")?, "physical");
                    call(conn, "/etcdserverpb.KV/Compact", "etcdserverpb.CompactionRequest", "etcdserverpb.CompactionResponse", json!({ "revision": rev, "physical": physical }), |_| Json::Null)
                }
                _ => return Ok(None),
            }))
        })();
        match out {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["get", "put", "delete", "grant", "revoke", "ttl", "keepAlive", "watch", "txn", "status", "memberList", "compact", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------
// streams: watch and keepAlive
// ---------------------------------------------------------------------

/// A watch response → its events (an error when the server cancels the watch).
fn watch_events(b: &[u8]) -> Result<(Vec<Json>, bool), String> {
    let j = decode("etcdserverpb.WatchResponse", b)?;
    if j["canceled"].as_bool() == Some(true) {
        let why = j["cancelReason"].as_str().unwrap_or("").to_string();
        if why.is_empty() {
            return Ok((Vec::new(), true));
        }
        return Err(io_err!("etcd.watch: canceled: {}", why));
    }
    let events = j["events"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| json!({ "type": if e["type"] == "DELETE" { "delete" } else { "put" }, "kv": kv(&e["kv"]), "prevKv": kv(&e["prevKv"]), "revision": revision(&j) }))
                .collect()
        })
        .unwrap_or_default();
    Ok((events, false))
}

/// A keep-alive response → `{ id, ttl }` (a lease that expired ends the stream).
fn keepalive_events(b: &[u8]) -> Result<(Vec<Json>, bool), String> {
    let j = decode("etcdserverpb.LeaseKeepAliveResponse", b)?;
    let ttl = num(&j["TTL"]);
    Ok((vec![json!({ "id": num(&j["ID"]), "ttl": ttl })], ttl <= 0))
}

type Parse = fn(&[u8]) -> Result<(Vec<Json>, bool), String>;

/// A bidirectional call kept open; its messages are read with `next()`.
type Inbox = Arc<Mutex<Option<Receiver<Result<Json, String>>>>>;

pub struct Stream {
    rx: Inbox,
    task: Option<tokio::task::JoinHandle<()>>,
    kind: &'static str,
}

impl Stream {
    /// `first` are sent at once; `repeat` is sent again every third of the
    /// lease's TTL (keep-alive).
    fn start(conn: Conn, path: &'static str, first: Vec<Vec<u8>>, repeat: Option<Vec<u8>>, parse: Parse) -> Stream {
        let (tx, rx) = channel::<Result<Json, String>>();
        let kind = if repeat.is_some() { "KeepAlive" } else { "Watch" };
        let guard = IoGuard::new();
        let task = crate::native::rt::tokio().spawn(async move {
            let _guard = guard;
            let send = |r: Result<Json, String>| {
                let ok = tx.send(r).is_ok();
                sched::wake();
                ok
            };
            let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
            for m in first {
                let _ = out_tx.send(m);
            }
            let token = match conn.auth_token().await {
                Ok(t) => t,
                Err(e) => {
                    send(Err(e));
                    return;
                }
            };
            let mut g = tonic::client::Grpc::new(conn.channel.clone());
            if let Err(e) = g.ready().await {
                send(Err(io_err!("etcd: cannot connect: {}", e)));
                return;
            }
            let mut req = tonic::Request::new(tokio_stream::wrappers::UnboundedReceiverStream::new(out_rx));
            if let Some(t) = token.and_then(|t| t.parse().ok()) {
                req.metadata_mut().insert("token", t);
            }
            let mut stream = match g.streaming(req, http::uri::PathAndQuery::from_static(path), RawCodec).await {
                Ok(r) => r.into_inner(),
                Err(s) => {
                    send(Err(io_err!("etcd: {}", status_text(&s))));
                    return;
                }
            };
            let mut every = Duration::from_secs(1);
            let mut next_ping = Instant::now() + every;
            loop {
                let wait = if repeat.is_some() { next_ping.saturating_duration_since(Instant::now()) } else { Duration::from_secs(3600) };
                tokio::select! {
                    m = stream.message() => match m {
                        Ok(Some(b)) => match parse(&b) {
                            Ok((items, end)) => {
                                for it in items {
                                    // a keep-alive's ttl sets the pace of the next ones
                                    if let Some(ttl) = it["ttl"].as_i64() {
                                        every = Duration::from_secs((ttl / 3).max(1) as u64);
                                        next_ping = Instant::now() + every;
                                    }
                                    if !send(Ok(it)) {
                                        return;
                                    }
                                }
                                if end {
                                    return;
                                }
                            }
                            Err(e) => {
                                send(Err(e));
                                return;
                            }
                        },
                        Ok(None) => return,
                        Err(s) => {
                            send(Err(io_err!("etcd: {}", status_text(&s))));
                            return;
                        }
                    },
                    _ = tokio::time::sleep(wait) => {
                        if let Some(r) = &repeat {
                            if out_tx.send(r.clone()).is_err() {
                                return;
                            }
                            next_ping = Instant::now() + every;
                        }
                    }
                }
            }
        });
        Stream { rx: Arc::new(Mutex::new(Some(rx))), task: Some(task), kind }
    }

    fn stop(&mut self) {
        self.rx.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop();
    }
}

impl FoguObject for Stream {
    fn name(&self) -> &str {
        self.kind
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "closed").then(|| Value::Bool(self.rx.lock().map(|r| r.is_none()).unwrap_or(true)))
    }

    // next(timeout?) → the next event (nil after the timeout, or when the stream ended)
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        if let Err(e) = max_args(args, 1, "next") {
            return Some(Err(e));
        }
        let deadline = match args.first() {
            Some(v) => match duration_arg(v, "next: timeout") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
            None => None,
        };
        let slot = Arc::clone(&self.rx);
        Some(Ok(Box::new(move || {
            let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
            let Some(rx) = g.as_ref() else { return Some(Ok(Value::Nil)) };
            match rx.try_recv() {
                Ok(Ok(j)) => Some(Ok(json_to_value(&j))),
                Ok(Err(e)) => Some(Err(e)),
                Err(TryRecvError::Disconnected) => {
                    g.take();
                    Some(Ok(Value::Nil))
                }
                Err(TryRecvError::Empty) => {
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Some(Ok(Value::Nil));
                    }
                    sched::add_timer(Instant::now() + Duration::from_millis(5));
                    None
                }
            }
        })))
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "close" | "cancel" => {
                self.stop();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on {} (next close)", other, self.kind)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
