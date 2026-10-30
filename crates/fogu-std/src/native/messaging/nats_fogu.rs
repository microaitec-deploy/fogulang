//! `nats.fogu(config)` / `nc.fogu()` — the high-level layer over a NATS
//! connection: KV stores (JetStream buckets) with watches, pub/sub with
//! handlers, services (request/reply with auth) and commands.
//!
//! ```fogu
//! import nats from "fogu:nats"
//!
//! const f = nats.fogu("nats://127.0.0.1:4222")?
//! f.store.createKV("config", fn(e) { print(e.operation, e.key, e.value) })?   // watch: return false stops
//! const kv = f.store.kv("config")
//! kv.set("theme", { dark: true })?
//! print(kv.get("theme")?, kv.create("theme", 1)?, kv.keys()?)                // {dark: true} false [theme]
//!
//! f.sub({ topic: "orders.*" }, fn(data) { print("order", data) })
//! f.pub({ topic: "orders.new" }, { id: 1 })
//!
//! f.service("math.double", fn(x) { return x * 2 }, fn(h) { return h.get("token") == "s3cret" })
//! print(f.command("math.double", 21, { token: "s3cret" })?)                    // 42
//! print(f.command("math.double", 21)?)                                         // {data: nil, error: unauthorized}
//! ```
//!
//! Data: nil is sent empty, a string as is, anything else as JSON; received
//! data is parsed as JSON when it can be, else it stays text. A service
//! runs on subject `service:<name>`; a failing auth answers
//! `{ error: "unauthorized", data: nil }`, a failing handler
//! `{ error: "internal_error", data: nil }` (the error is printed).
//! Handlers run as `fg` tasks.

use std::cell::RefCell;
use std::collections::HashSet;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value as J};

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched;
use crate::runtime::value::Value;

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{duration_arg, list, map, max_args, str_arg};
use super::{nuid, Conn, Msg, NatsValue};

// ---------------------------------------------------------------------
// data
// ---------------------------------------------------------------------

fn to_bytes(v: Option<&Value>) -> Vec<u8> {
    super::payload(v)
}

fn from_bytes(b: &[u8]) -> Value {
    if b.is_empty() {
        return Value::Nil;
    }
    let raw = String::from_utf8_lossy(b);
    crate::native::json::parse_value(&raw).unwrap_or_else(|_| Value::Str(raw.into_owned().into()))
}

fn err_payload(code: &str) -> Vec<u8> {
    json!({ "error": code, "data": null }).to_string().into_bytes()
}

fn headers_of(v: Option<&Value>) -> Vec<(String, String)> {
    match v {
        Some(Value::Map(m)) => {
            let mut h: Vec<(String, String)> = m.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect();
            h.sort();
            h
        }
        _ => Vec::new(),
    }
}

/// Waits (cooperatively) for the answer of a request.
fn request(conn: &Rc<Conn>, subject: &str, headers: &[(String, String)], data: &[u8], timeout: Duration) -> Result<Msg, String> {
    let p = conn.request(subject, headers, data, timeout)?;
    let mut out = None;
    sched::wait_until(|| {
        if out.is_none() {
            out = p.poll();
        }
        out.is_some()
    })?;
    out.expect("ready")
}

/// A JetStream API call → its JSON (an `error` field becomes Err((code, text))).
fn js(conn: &Rc<Conn>, subject: &str, body: J) -> Result<J, (i64, String)> {
    let data = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
    let m = request(conn, subject, &[], &data, conn.timeout).map_err(|e| (0, e))?;
    if m.status == Some(503) {
        return Err((503, io_err!("nats: JetStream is not enabled on the server")));
    }
    let j: J = serde_json::from_slice(&m.data).map_err(|e| (0, io_err!("nats: invalid JetStream answer: {}", e)))?;
    if j["error"].is_object() {
        let code = j["error"]["err_code"].as_i64().unwrap_or(0);
        return Err((code, io_err!("nats: jetstream: {} ({})", j["error"]["description"].as_str().unwrap_or("error"), code)));
    }
    Ok(j)
}

fn check_key(key: &str) -> Result<(), String> {
    let ok = !key.is_empty()
        && !key.starts_with('.')
        && !key.ends_with('.')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || "-/_=.".contains(c));
    if ok { Ok(()) } else { Err(format!("nats.kv: invalid key '{}' (letters, digits and - / _ = .)", key)) }
}

// ---------------------------------------------------------------------
// state shared by the objects of one `fogu()`
// ---------------------------------------------------------------------

struct State {
    conn: RefCell<Rc<Conn>>,
    /// the `Nats` value (`f.connection`)
    nats: RefCell<Value>,
    /// the config used by `connect()` without arguments
    config: Option<Value>,
    stores: RefCell<HashSet<String>>,
    /// subscriptions with a handler (sid); emptied by close/connect
    sids: RefCell<Vec<u64>>,
    /// KV watches: bucket → (sid, consumer name)
    watches: RefCell<HashMap<String, (u64, String)>>,
}

impl State {
    fn conn(&self) -> Rc<Conn> {
        Rc::clone(&self.conn.borrow())
    }

    fn require_store(&self, name: &str) -> Result<(), String> {
        if self.stores.borrow().contains(name) {
            Ok(())
        } else {
            Err(io_err!("nats.kv: store \"{}\" was not created; call createKV(\"{}\") first", name, name))
        }
    }

    fn stop_all(&self) {
        let conn = self.conn();
        for (_, (sid, consumer)) in self.watches.borrow_mut().drain() {
            conn.unsubscribe(sid);
            let _ = consumer;
        }
        for sid in self.sids.borrow_mut().drain(..) {
            conn.unsubscribe(sid);
        }
    }
}

/// Runs `handle` for every message of `sid`, as an `fg` task, until the
/// subscription ends (unsubscribed, connection closed) or `handle` says stop.
fn consume(interp: &Interpreter, conn: Rc<Conn>, sid: u64, handle: impl Fn(&Interpreter, Msg) -> bool + 'static) -> Result<(), String> {
    interp.spawn_task(Box::new(move |it| loop {
        let mut msg = None;
        let mut gone = false;
        sched::wait_until(|| {
            let sh = conn.shared.lock().unwrap();
            gone = sh.closed.is_some() || !sh.subs.contains_key(&sid);
            drop(sh);
            msg = conn.pop(sid);
            if msg.is_none() && !gone {
                // messages come from the reader thread
                sched::add_timer(std::time::Instant::now() + Duration::from_millis(2));
            }
            msg.is_some() || gone
        })?;
        match msg {
            Some(m) => {
                if !handle(it, m) {
                    conn.unsubscribe(sid);
                    return Ok(());
                }
            }
            None => return Ok(()),
        }
    }))
}

/// Calls a Fogu handler; its error (or error value) is printed and returned.
fn call(it: &Interpreter, env: &Rc<RefCell<Environment>>, f: &Value, args: Vec<Value>, what: &str) -> Result<Value, String> {
    let r = it.call_value(f.clone(), args, Rc::clone(env));
    let err = match &r {
        Err(e) => Some(e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(e).to_string()),
        Ok(Value::Error(e)) => Some(e.message.clone()),
        Ok(Value::List(l)) if l.borrow().len() == 2 && matches!(l.borrow()[1], Value::Error(_)) => {
            let Value::Error(e) = &l.borrow()[1] else { unreachable!() };
            Some(e.message.clone())
        }
        _ => None,
    };
    match err {
        Some(e) => {
            eprintln!("[{}] {}", what, e);
            Err(e)
        }
        None => r,
    }
}

// ---------------------------------------------------------------------
// constructors
// ---------------------------------------------------------------------

/// `nats.fogu(config)`: connects and wraps.
pub fn module_fogu(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "nats.fogu")?;
    let nats = super::connect(args)?;
    wrap(nats, args.first().cloned())
}

/// `nc.fogu()` on an open connection.
pub fn wrap(nats: Value, config: Option<Value>) -> Result<Value, String> {
    let conn = conn_of(&nats)?;
    let st = Rc::new(State {
        conn: RefCell::new(conn),
        nats: RefCell::new(nats),
        config,
        stores: RefCell::new(HashSet::new()),
        sids: RefCell::new(Vec::new()),
        watches: RefCell::new(HashMap::new()),
    });
    Ok(NativeObjectRef::new(Fogu { st }).to_value())
}

fn conn_of(v: &Value) -> Result<Rc<Conn>, String> {
    if let Value::Object(o) = v {
        if let Some(n) = o.inner().borrow().as_any().downcast_ref::<NatsValue>() {
            return Ok(Rc::clone(&n.conn));
        }
    }
    Err(format!("nats.fogu: expected a nats connection, got '{}'", v.type_name()))
}

pub struct Fogu {
    st: Rc<State>,
}

pub struct Store {
    st: Rc<State>,
}

pub struct Kv {
    st: Rc<State>,
    bucket: String,
}

/// The `headers` given to a service's auth function: `get(name)`, `has(name)`.
pub struct Headers {
    h: Vec<(String, String)>,
}

impl FoguObject for Headers {
    fn name(&self) -> &str {
        "NatsHeaders"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let name = str_arg(args, 0, method)?;
        let found = self.h.iter().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|(_, v)| v.clone());
        match method {
            "get" => Ok(found.map(Value::str).unwrap_or(Value::Nil)),
            "has" => Ok(Value::Bool(found.is_some())),
            other => Err(format!("method '{}' does not exist on NatsHeaders (get has)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["get", "has"]
    }
}

impl FoguObject for Fogu {
    fn name(&self) -> &str {
        "NatsFogu"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "store" => Some(NativeObjectRef::new(Store { st: Rc::clone(&self.st) }).to_value()),
            "connection" => Some(self.st.nats.borrow().clone()),
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["sub", "pub", "service", "command", "connect", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FoguObject for Store {
    fn name(&self) -> &str {
        "NatsFoguStore"
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["kv", "createKV"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl FoguObject for Kv {
    fn name(&self) -> &str {
        "NatsKv"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "bucket").then(|| Value::Str(self.bucket.clone().into()))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["create", "set", "get", "del", "keys"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The calls of these objects (they wait and run handlers: they need the interpreter).
pub fn intercept(callee: &Value, args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Option<Result<Value, String>> {
    let Value::BoundMethod(bm) = callee else { return None };
    let (receiver, method) = (&bm.receiver, &bm.method);
    if !matches!(receiver.name().as_str(), "NatsFogu" | "NatsFoguStore" | "NatsKv") {
        return None;
    }
    let st = {
        let inner = receiver.inner();
        let b = inner.borrow();
        let any = b.as_any();
        if let Some(f) = any.downcast_ref::<Fogu>() {
            (Rc::clone(&f.st), None, "fogu")
        } else if let Some(s) = any.downcast_ref::<Store>() {
            (Rc::clone(&s.st), None, "store")
        } else if let Some(k) = any.downcast_ref::<Kv>() {
            (Rc::clone(&k.st), Some(k.bucket.clone()), "kv")
        } else {
            return None;
        }
    };
    let (st, bucket, kind) = st;
    Some(match kind {
        "fogu" => fogu_call(&st, method, args, interp, env),
        "store" => store_call(&st, method, args, interp, env),
        _ => kv_call(&st, &bucket.expect("bucket"), method, args),
    })
}

fn topic_of(v: Option<&Value>, what: &str) -> Result<String, String> {
    match v {
        Some(Value::Map(m)) => match m.borrow().get("topic") {
            Some(Value::Str(t)) if !t.is_empty() => Ok(t.to_string()),
            _ => Err(format!("nats.{} requires {{ topic }}", what)),
        },
        Some(Value::Str(t)) => Ok(t.to_string()),
        _ => Err(format!("nats.{} requires {{ topic }}", what)),
    }
}

fn is_fn(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) | Value::StructBoundMethod(_)))
}

fn fogu_call(st: &Rc<State>, method: &str, args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    let conn = st.conn();
    match method {
        // sub({ topic }, fn(data), { queue })
        "sub" => {
            max_args(args, 3, "sub")?;
            let topic = topic_of(args.first(), "sub")?;
            if !is_fn(args.get(1)) {
                return Err("nats.sub requires a handler".into());
            }
            let queue = match args.get(2) {
                Some(Value::Map(m)) => m.borrow().get("queue").map(|v| v.to_display_string()),
                _ => None,
            };
            let sid = conn.subscribe(&topic, queue.as_deref(), None)?;
            st.sids.borrow_mut().push(sid);
            let (f, env) = (args[1].clone(), Rc::clone(env));
            consume(interp, Rc::clone(&conn), sid, move |it, m| {
                let _ = call(it, &env, &f, vec![from_bytes(&m.data)], "nats handler error");
                true
            })?;
            Ok(Value::Nil)
        }
        // pub({ topic }, data)
        "pub" => {
            max_args(args, 2, "pub")?;
            let topic = topic_of(args.first(), "pub")?;
            conn.publish(&topic, None, &[], &to_bytes(args.get(1)))?;
            Ok(Value::Nil)
        }
        // service(name, fn(input), auth?(headers), { queue })
        "service" => {
            max_args(args, 4, "service")?;
            let name = str_arg(args, 0, "service")?;
            if name.is_empty() {
                return Err("nats.service requires a name".into());
            }
            if !is_fn(args.get(1)) {
                return Err("nats.service requires a handler".into());
            }
            let auth = args.get(2).filter(|v| !matches!(v, Value::Nil)).cloned();
            let queue = match args.get(3) {
                Some(Value::Map(m)) => m.borrow().get("queue").map(|v| v.to_display_string()),
                _ => None,
            };
            let sid = conn.subscribe(&format!("service:{}", name), queue.as_deref(), None)?;
            st.sids.borrow_mut().push(sid);
            let (f, env, c2) = (args[1].clone(), Rc::clone(env), Rc::clone(&conn));
            consume(interp, Rc::clone(&conn), sid, move |it, m| {
                let Some(reply) = m.reply.clone() else { return true };
                let answer = |data: Vec<u8>| {
                    let _ = c2.publish(&reply, None, &[], &data);
                };
                if let Some(a) = &auth {
                    let h = NativeObjectRef::new(Headers { h: m.headers.clone() }).to_value();
                    match call(it, &env, a, vec![h], &format!("nats:service:{} auth error", name)) {
                        Ok(v) if v.is_truthy() => {}
                        _ => {
                            answer(err_payload("unauthorized"));
                            return true;
                        }
                    }
                }
                match call(it, &env, &f, vec![from_bytes(&m.data)], &format!("nats:service:{} exec error", name)) {
                    Ok(Value::Nil) => answer(b"{}".to_vec()),
                    Ok(v) => answer(to_bytes(Some(&v))),
                    Err(_) => answer(err_payload("internal_error")),
                }
                true
            })?;
            Ok(Value::Nil)
        }
        // command(name, data = {}, headers = {}, { timeout = 5s }) → the service's answer
        "command" => {
            max_args(args, 4, "command")?;
            let name = str_arg(args, 0, "command")?;
            let data = args.get(1).cloned().unwrap_or_else(|| map(vec![]));
            let headers = headers_of(args.get(2));
            let timeout = match args.get(3) {
                Some(Value::Map(m)) => match m.borrow().get("timeout") {
                    Some(Value::Int(ms)) => Duration::from_millis(*ms as u64),
                    Some(v) => duration_arg(v, "command timeout")?,
                    None => Duration::from_secs(5),
                },
                _ => Duration::from_secs(5),
            };
            let m = request(&conn, &format!("service:{}", name), &headers, &to_bytes(Some(&data)), timeout)?;
            if m.status == Some(503) {
                return Err(io_err!("nats: no service '{}'", name));
            }
            Ok(from_bytes(&m.data))
        }
        // connect(newConfig?): a new connection (handlers of the old one stop)
        "connect" => {
            max_args(args, 1, "connect")?;
            let cfg = args.first().cloned().or_else(|| st.config.clone());
            let nats = super::connect(&cfg.into_iter().collect::<Vec<_>>())?;
            st.stop_all();
            let old = st.conn();
            let _ = old.write(b"");
            old.shared.lock().unwrap().closed.get_or_insert("reconnected".into());
            *st.conn.borrow_mut() = conn_of(&nats)?;
            *st.nats.borrow_mut() = nats.clone();
            st.stores.borrow_mut().clear();
            Ok(nats)
        }
        // close(): stops watches and subscriptions, then closes
        "close" => {
            max_args(args, 0, "close")?;
            st.stop_all();
            let nats = st.nats.borrow().clone();
            if let Value::Object(o) = nats {
                let _ = o.inner().borrow_mut().call("close", &[]);
            }
            Ok(Value::Nil)
        }
        other => Err(format!("method '{}' does not exist on NatsFogu (sub pub service command connect close store connection)", other)),
    }
}

fn stream_name(bucket: &str) -> String {
    format!("KV_{}", bucket)
}

fn store_call(st: &Rc<State>, method: &str, args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    let conn = st.conn();
    match method {
        "kv" => {
            max_args(args, 1, "kv")?;
            Ok(NativeObjectRef::new(Kv { st: Rc::clone(st), bucket: str_arg(args, 0, "kv")? }).to_value())
        }
        // createKV(name, fn?(entry), { history = 5, ttl, maxBytes }) → the store's kv
        "createKV" => {
            max_args(args, 3, "createKV")?;
            let name = str_arg(args, 0, "createKV")?;
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(format!("nats.createKV: invalid bucket name '{}'", name));
            }
            let (handler, cfg) = match (args.get(1), args.get(2)) {
                (Some(Value::Map(m)), None) => (None, Some(m.borrow().clone())),
                (f, c) => (f.filter(|v| !matches!(v, Value::Nil)).cloned(), c.and_then(|v| if let Value::Map(m) = v { Some(m.borrow().clone()) } else { None })),
            };
            let cfg = cfg.unwrap_or_default();
            let history = match cfg.get("history") {
                Some(Value::Int(n)) if (1..=64).contains(n) => *n,
                None => 5,
                Some(other) => return Err(format!("nats.createKV: history must be 1..64, got {}", other.to_display_string())),
            };
            let max_age = match cfg.get("ttl") {
                Some(v) => duration_arg(v, "createKV ttl")?.as_nanos() as i64,
                None => 0,
            };
            let stream = stream_name(&name);
            let config = json!({
                "name": stream, "subjects": [format!("$KV.{}.>", name)], "retention": "limits",
                "max_msgs_per_subject": history, "max_msgs": -1, "max_bytes": -1, "max_age": max_age,
                "max_msg_size": -1, "storage": "file", "discard": "new", "num_replicas": 1,
                "duplicate_window": 120_000_000_000i64, "allow_rollup_hdrs": true, "deny_delete": true,
                "deny_purge": false, "allow_direct": true,
            });
            match js(&conn, &format!("$JS.API.STREAM.CREATE.{}", stream), config) {
                Ok(_) => {}
                // already there (maybe with another config): use it
                Err((10058, _)) => {
                    js(&conn, &format!("$JS.API.STREAM.INFO.{}", stream), J::Null).map_err(|(_, e)| e)?;
                }
                Err((_, e)) => return Err(e),
            }
            st.stores.borrow_mut().insert(name.clone());
            if let Some(f) = handler {
                watch(st, &name, f, interp, env)?;
            }
            Ok(NativeObjectRef::new(Kv { st: Rc::clone(st), bucket: name }).to_value())
        }
        other => Err(format!("method '{}' does not exist on NatsFoguStore (kv createKV)", other)),
    }
}

/// A push consumer over the bucket: the last value of each key, then every change.
fn watch(st: &Rc<State>, bucket: &str, f: Value, interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<(), String> {
    let conn = st.conn();
    if let Some((sid, _)) = st.watches.borrow_mut().remove(bucket) {
        conn.unsubscribe(sid);
    }
    let inbox = format!("_INBOX.{}", nuid());
    let sid = conn.subscribe(&inbox, None, None)?;
    let stream = stream_name(bucket);
    let consumer = format!("fogu_watch_{}", nuid());
    let body = json!({
        "stream_name": stream,
        "config": {
            "name": consumer, "deliver_subject": inbox, "deliver_policy": "last_per_subject",
            "filter_subject": format!("$KV.{}.>", bucket), "ack_policy": "none", "replay_policy": "instant",
            "inactive_threshold": 60_000_000_000i64, "mem_storage": true, "num_replicas": 1,
        },
    });
    if let Err((_, e)) = js(&conn, &format!("$JS.API.CONSUMER.CREATE.{}.{}", stream, consumer), body) {
        conn.unsubscribe(sid);
        return Err(e);
    }
    st.watches.borrow_mut().insert(bucket.to_string(), (sid, consumer.clone()));
    let prefix = format!("$KV.{}.", bucket);
    let (env, b2, st2) = (Rc::clone(env), bucket.to_string(), Rc::clone(st));
    consume(interp, Rc::clone(&conn), sid, move |it, m| {
        let Some(key) = m.subject.strip_prefix(&prefix) else { return true };
        let op = m.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("KV-Operation")).map(|(_, v)| v.clone()).unwrap_or_else(|| "PUT".into());
        let revision = m.reply.as_deref().and_then(super::js_meta).map(|(_, _, s)| s).unwrap_or(0);
        let entry = map(vec![
            ("bucket", Value::Str(b2.clone().into())),
            ("key", Value::Str(key.to_string().into())),
            ("value", if op == "PUT" { from_bytes(&m.data) } else { Value::Nil }),
            ("operation", Value::Str(op.into())),
            ("revision", Value::Int(revision)),
        ]);
        match call(it, &env, &f, vec![entry], &format!("nats:kv:{} watch error", b2)) {
            Ok(Value::Bool(false)) => {
                st2.watches.borrow_mut().remove(&b2);
                false
            }
            _ => true,
        }
    })
}

/// The latest entry of a key: (revision, deleted?, data), or None.
fn last(conn: &Rc<Conn>, bucket: &str, key: &str) -> Result<Option<(i64, bool, Vec<u8>)>, String> {
    let subject = format!("$JS.API.STREAM.MSG.GET.{}", stream_name(bucket));
    match js(conn, &subject, json!({ "last_by_subj": format!("$KV.{}.{}", bucket, key) })) {
        Ok(j) => {
            let b64 = base64::engine::general_purpose::STANDARD;
            let msg = &j["message"];
            let data = msg["data"].as_str().and_then(|d| b64.decode(d).ok()).unwrap_or_default();
            let hdrs = msg["hdrs"].as_str().and_then(|h| b64.decode(h).ok()).map(|h| String::from_utf8_lossy(&h).into_owned()).unwrap_or_default();
            let deleted = hdrs.lines().any(|l| {
                let l = l.to_ascii_lowercase();
                l.starts_with("kv-operation:") && (l.contains("del") || l.contains("purge"))
            });
            Ok(Some((msg["seq"].as_i64().unwrap_or(0), deleted, data)))
        }
        Err((10037, _)) => Ok(None),
        Err((_, e)) => Err(e),
    }
}

/// A JetStream publish to the bucket (the answer is the PubAck).
fn put(conn: &Rc<Conn>, bucket: &str, key: &str, headers: &[(String, String)], data: &[u8]) -> Result<Result<i64, (i64, String)>, String> {
    let m = request(conn, &format!("$KV.{}.{}", bucket, key), headers, data, conn.timeout)?;
    if m.status == Some(503) {
        return Err(io_err!("nats.kv: bucket '{}' does not exist", bucket));
    }
    let j: J = serde_json::from_slice(&m.data).map_err(|e| io_err!("nats.kv: invalid answer: {}", e))?;
    if j["error"].is_object() {
        return Ok(Err((j["error"]["err_code"].as_i64().unwrap_or(0), j["error"]["description"].as_str().unwrap_or("error").to_string())));
    }
    Ok(Ok(j["seq"].as_i64().unwrap_or(0)))
}

fn kv_call(st: &Rc<State>, bucket: &str, method: &str, args: &[Value]) -> Result<Value, String> {
    st.require_store(bucket)?;
    let conn = st.conn();
    match method {
        // create(key, data) → true, or false when the key exists
        "create" => {
            max_args(args, 2, "create")?;
            let key = str_arg(args, 0, "create")?;
            check_key(&key)?;
            let data = to_bytes(args.get(1));
            // only if there is no live value (a deleted one may be replaced)
            let expected = match last(&conn, bucket, &key)? {
                Some((_, false, _)) => return Ok(Value::Bool(false)),
                Some((rev, true, _)) => rev,
                None => 0,
            };
            let h = vec![("Nats-Expected-Last-Subject-Sequence".to_string(), expected.to_string())];
            match put(&conn, bucket, &key, &h, &data)? {
                Ok(_) => Ok(Value::Bool(true)),
                Err((10071, _)) => Ok(Value::Bool(false)),
                Err((code, e)) => Err(io_err!("nats.kv: {} ({})", e, code)),
            }
        }
        "set" => {
            max_args(args, 2, "set")?;
            let key = str_arg(args, 0, "set")?;
            check_key(&key)?;
            put(&conn, bucket, &key, &[], &to_bytes(args.get(1)))?.map_err(|(c, e)| io_err!("nats.kv: {} ({})", e, c))?;
            Ok(Value::Nil)
        }
        "get" => {
            max_args(args, 1, "get")?;
            let key = str_arg(args, 0, "get")?;
            check_key(&key)?;
            Ok(match last(&conn, bucket, &key)? {
                Some((_, false, data)) => from_bytes(&data),
                _ => Value::Nil,
            })
        }
        "del" => {
            max_args(args, 1, "del")?;
            let key = str_arg(args, 0, "del")?;
            check_key(&key)?;
            put(&conn, bucket, &key, &[("KV-Operation".to_string(), "DEL".to_string())], &[])?.map_err(|(c, e)| io_err!("nats.kv: {} ({})", e, c))?;
            Ok(Value::Nil)
        }
        // keys() → the keys with a live value, sorted
        "keys" => {
            max_args(args, 0, "keys")?;
            let prefix = format!("$KV.{}.", bucket);
            let info = js(&conn, &format!("$JS.API.STREAM.INFO.{}", stream_name(bucket)), json!({ "subjects_filter": format!("{}>", prefix) })).map_err(|(_, e)| e)?;
            let mut keys: Vec<String> = info["state"]["subjects"].as_object().map(|o| o.keys().filter_map(|s| s.strip_prefix(&prefix).map(String::from)).collect()).unwrap_or_default();
            keys.sort();
            let mut live = Vec::new();
            for k in keys {
                if matches!(last(&conn, bucket, &k)?, Some((_, false, _))) {
                    live.push(Value::Str(k.into()));
                }
            }
            Ok(list(live))
        }
        other => Err(format!("method '{}' does not exist on NatsKv (create set get del keys)", other)),
    }
}
