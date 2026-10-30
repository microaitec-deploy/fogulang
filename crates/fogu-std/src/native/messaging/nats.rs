//! `fogu:nats` module — NATS client (core protocol over TCP) with JetStream
//! streams, publish acknowledgements and pull consumers.
//!
//! ```fogu
//! import nats from "fogu:nats"
//!
//! const nc = nats.connect("nats://127.0.0.1:4222")?
//! const sub = nc.subscribe("orders.*", { queue: "workers" })
//! nc.publish("orders.created", { id: 1 })
//! const m = sub.next(2s)?                         // nil after 2s
//! print(m.subject, m.json().id)
//!
//! const svc = nc.subscribe("math.double")
//! fg fn() { const req = svc.next()?; req.respond(str(int(req.data) * 2)) }()
//! print(nc.request("math.double", "21")?.data)    // 42
//!
//! const js = nc.jetstream()
//! js.addStream({ name: "ORDERS", subjects: ["orders.>"] })?
//! print(js.publish("orders.new", { id: 2 })?.seq)
//! const c = js.consumer("ORDERS", { durable: "billing" })?
//! for msg in c.fetch(10, 1s)? { print(msg.data); msg.ack() }
//! ```
//!
//! Frames are read by a background thread; waiting methods (`next`,
//! `request`, `flush`, JetStream calls) wait cooperatively. Network and
//! server failures are `error` values.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as J};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, list, map, max_args, nfn_var, str_arg};

#[path = "nats_fogu.rs"]
pub(crate) mod fogu;

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 0, connect));
    // fogu(config) → the high-level layer (KV stores, sub/pub, services, commands)
    e.insert("fogu".into(), nfn_var("fogu", 0, fogu::module_fogu));
    ModuleValue {
        name: "fogu:nats".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Cfg {
    host: String,
    port: u16,
    user: Option<String>,
    pass: Option<String>,
    token: Option<String>,
    name: String,
    timeout: Duration,
}

fn get_s(m: &HashMap<String, Value>, k: &str) -> Option<String> {
    match m.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

fn parse_url(url: &str, cfg: &mut Cfg) -> Result<(), String> {
    let rest = url.strip_prefix("nats://").or_else(|| url.strip_prefix("tcp://")).unwrap_or(url);
    if url.starts_with("tls://") || url.starts_with("wss://") || url.starts_with("ws://") {
        return Err(format!("nats.connect: '{}': only nats:// (plain TCP) is supported", url));
    }
    let rest = rest.trim_end_matches('/');
    let (auth, hostport) = match rest.rfind('@') {
        Some(i) => (Some(&rest[..i]), &rest[i + 1..]),
        None => (None, rest),
    };
    if let Some(a) = auth {
        match a.split_once(':') {
            Some((u, p)) => {
                cfg.user = Some(crate::native::util::url_decode(u));
                cfg.pass = Some(crate::native::util::url_decode(p));
            }
            None => cfg.token = Some(crate::native::util::url_decode(a)),
        }
    }
    match hostport.rsplit_once(':') {
        Some((h, p)) => {
            if !h.is_empty() {
                cfg.host = h.to_string();
            }
            cfg.port = p.parse().map_err(|_| format!("nats.connect: invalid port '{}'", p))?;
        }
        None if !hostport.is_empty() => cfg.host = hostport.to_string(),
        None => {}
    }
    Ok(())
}

/// `nats://[user:pass@|token@]host[:port]` or `{ url, user, pass, token, name, timeout }`.
fn parse_cfg(arg: Option<&Value>) -> Result<Cfg, String> {
    let mut cfg = Cfg {
        host: "127.0.0.1".into(),
        port: 4222,
        user: None,
        pass: None,
        token: None,
        name: "fogu".into(),
        timeout: Duration::from_secs(5),
    };
    match arg {
        None | Some(Value::Nil) => {
            if let Ok(u) = std::env::var("NATS_URL") {
                parse_url(&u, &mut cfg)?;
            }
        }
        Some(Value::Str(u)) => parse_url(u, &mut cfg)?,
        Some(Value::Map(m)) => {
            let m = m.borrow();
            if let Some(u) = get_s(&m, "url").or_else(|| get_s(&m, "servers")) {
                parse_url(&u, &mut cfg)?;
            }
            cfg.user = get_s(&m, "user").or(cfg.user.take());
            cfg.pass = get_s(&m, "pass").or_else(|| get_s(&m, "password")).or(cfg.pass.take());
            cfg.token = get_s(&m, "token").or(cfg.token.take());
            if let Some(n) = get_s(&m, "name") {
                cfg.name = n;
            }
            if let Some(t) = m.get("timeout").filter(|v| !matches!(v, Value::Nil)) {
                cfg.timeout = duration_arg(t, "nats.connect: timeout")?;
            }
        }
        Some(other) => return Err(format!("nats.connect expects a url or a map, got '{}'", other.type_name())),
    }
    Ok(cfg)
}

// ---------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Msg {
    subject: String,
    reply: Option<String>,
    headers: Vec<(String, String)>,
    /// `NATS/1.0 503` → 503 (no responders), 404/408 (JetStream fetch end)
    status: Option<u16>,
    description: String,
    data: Vec<u8>,
}

#[derive(Default)]
struct Shared {
    subs: HashMap<u64, VecDeque<Msg>>,
    pongs: u64,
    closed: Option<String>,
    last_error: Option<String>,
}

fn parse_headers(block: &[u8]) -> (Option<u16>, String, Vec<(String, String)>) {
    let text = String::from_utf8_lossy(block);
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut status = None;
    let mut description = String::new();
    if let Some(rest) = first.strip_prefix("NATS/1.0") {
        let rest = rest.trim();
        if !rest.is_empty() {
            let (code, desc) = rest.split_once(' ').unwrap_or((rest, ""));
            status = code.parse().ok();
            description = desc.to_string();
        }
    }
    let headers = lines
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect();
    (status, description, headers)
}

fn header_block(headers: &[(String, String)]) -> Vec<u8> {
    let mut s = String::from("NATS/1.0\r\n");
    for (k, v) in headers {
        s.push_str(&format!("{}: {}\r\n", k, v));
    }
    s.push_str("\r\n");
    s.into_bytes()
}

fn reader(mut r: BufReader<TcpStream>, shared: Arc<Mutex<Shared>>, writer: Arc<Mutex<TcpStream>>) {
    let reason = loop {
        let mut line = String::new();
        match r.read_line(&mut line) {
            Ok(0) => break "closed by the server".to_string(),
            Ok(_) => {}
            Err(e) => break e.to_string(),
        }
        let line = line.trim_end();
        let mut parts = line.split_whitespace();
        let op = parts.next().unwrap_or_default().to_uppercase();
        let args: Vec<&str> = parts.collect();
        match op.as_str() {
            "PING" => {
                let _ = writer.lock().unwrap().write_all(b"PONG\r\n");
            }
            "PONG" => shared.lock().unwrap().pongs += 1,
            "+OK" | "INFO" => {}
            "-ERR" => {
                let msg = line[4..].trim().trim_matches('\'').to_string();
                shared.lock().unwrap().last_error = Some(msg);
            }
            "MSG" | "HMSG" => {
                // MSG <subject> <sid> [reply] <size> · HMSG <subject> <sid> [reply] <hdr size> <total size>
                let headers = op == "HMSG";
                let need = if headers { 4 } else { 3 };
                if args.len() < need {
                    break format!("bad message line: {}", line);
                }
                let total: usize = args[args.len() - 1].parse().unwrap_or(0);
                let hdr: usize = if headers { args[args.len() - 2].parse().unwrap_or(0) } else { 0 };
                let reply = if args.len() > need { Some(args[2].to_string()) } else { None };
                let mut buf = vec![0u8; total + 2];
                if let Err(e) = r.read_exact(&mut buf) {
                    break e.to_string();
                }
                buf.truncate(total);
                let (status, description, hs) = if headers { parse_headers(&buf[..hdr]) } else { (None, String::new(), Vec::new()) };
                let msg = Msg {
                    subject: args[0].to_string(),
                    reply,
                    headers: hs,
                    status,
                    description,
                    data: buf[hdr..].to_vec(),
                };
                if let Ok(sid) = args[1].parse::<u64>() {
                    let mut s = shared.lock().unwrap();
                    if let Some(q) = s.subs.get_mut(&sid) {
                        q.push_back(msg);
                    }
                }
            }
            _ => {}
        }
    };
    shared.lock().unwrap().closed.get_or_insert(reason);
}

struct Conn {
    shared: Arc<Mutex<Shared>>,
    writer: Arc<Mutex<TcpStream>>,
    next_sid: Cell<u64>,
    pings: Cell<u64>,
    cfg: Cfg,
    timeout: Duration,
}

fn nuid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

impl Conn {
    fn write(&self, data: &[u8]) -> Result<(), String> {
        if let Some(r) = &self.shared.lock().unwrap().closed {
            return Err(io_err!("nats: connection closed ({})", r));
        }
        self.writer.lock().unwrap().write_all(data).map_err(|e| io_err!("nats: {}", e))
    }

    fn publish(&self, subject: &str, reply: Option<&str>, headers: &[(String, String)], data: &[u8]) -> Result<(), String> {
        check_subject(subject)?;
        let reply = reply.map(|r| format!(" {}", r)).unwrap_or_default();
        let mut buf = if headers.is_empty() {
            format!("PUB {}{} {}\r\n", subject, reply, data.len()).into_bytes()
        } else {
            let hb = header_block(headers);
            let mut b = format!("HPUB {}{} {} {}\r\n", subject, reply, hb.len(), hb.len() + data.len()).into_bytes();
            b.extend(hb);
            b
        };
        buf.extend_from_slice(data);
        buf.extend_from_slice(b"\r\n");
        self.write(&buf)
    }

    fn subscribe(&self, subject: &str, queue: Option<&str>, max: Option<u64>) -> Result<u64, String> {
        check_subject(subject)?;
        let sid = self.next_sid.get() + 1;
        self.next_sid.set(sid);
        self.shared.lock().unwrap().subs.insert(sid, VecDeque::new());
        let q = queue.map(|q| format!(" {}", q)).unwrap_or_default();
        let mut cmd = format!("SUB {}{} {}\r\n", subject, q, sid);
        if let Some(m) = max {
            cmd.push_str(&format!("UNSUB {} {}\r\n", sid, m));
        }
        self.write(cmd.as_bytes())?;
        Ok(sid)
    }

    fn unsubscribe(&self, sid: u64) {
        self.shared.lock().unwrap().subs.remove(&sid);
        let _ = self.write(format!("UNSUB {}\r\n", sid).as_bytes());
    }

    fn pop(&self, sid: u64) -> Option<Msg> {
        self.shared.lock().unwrap().subs.get_mut(&sid).and_then(|q| q.pop_front())
    }

    fn closed(&self) -> Option<String> {
        self.shared.lock().unwrap().closed.clone().map(|r| io_err!("nats: connection closed ({})", r))
    }

    /// Request/reply: the poller yields the reply message.
    fn request(self: &Rc<Self>, subject: &str, headers: &[(String, String)], data: &[u8], timeout: Duration) -> Result<RequestPoll, String> {
        let inbox = format!("_INBOX.{}", nuid());
        let sid = self.subscribe(&inbox, None, Some(1))?;
        if let Err(e) = self.publish(subject, Some(&inbox), headers, data) {
            self.unsubscribe(sid);
            return Err(e);
        }
        Ok(RequestPoll { conn: Rc::clone(self), sid, deadline: Instant::now() + timeout, subject: subject.to_string() })
    }
}

struct RequestPoll {
    conn: Rc<Conn>,
    sid: u64,
    deadline: Instant,
    subject: String,
}

impl RequestPoll {
    fn poll(&self) -> Option<Result<Msg, String>> {
        if let Some(m) = self.conn.pop(self.sid) {
            self.conn.shared.lock().unwrap().subs.remove(&self.sid);
            return Some(Ok(m));
        }
        if let Some(e) = self.conn.closed() {
            return Some(Err(e));
        }
        if Instant::now() >= self.deadline {
            self.conn.unsubscribe(self.sid);
            return Some(Err(io_err!("nats: request to '{}' timed out", self.subject)));
        }
        poll_again();
        None
    }
}

fn poll_again() {
    sched::add_timer(Instant::now() + Duration::from_millis(1));
}

fn check_subject(s: &str) -> Result<(), String> {
    if s.is_empty() || s.contains(char::is_whitespace) {
        return Err(format!("nats: invalid subject '{}'", s));
    }
    Ok(())
}

fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "nats.connect")?;
    let cfg = parse_cfg(args.first())?;
    let target = format!("{}:{}", cfg.host, cfg.port);
    let addr = target
        .to_socket_addrs()
        .map_err(|e| io_err!("nats: cannot resolve {}: {}", target, e))?
        .next()
        .ok_or_else(|| io_err!("nats: cannot resolve {}", target))?;
    let tcp = TcpStream::connect_timeout(&addr, cfg.timeout).map_err(|e| io_err!("nats: cannot connect to {}: {}", target, e))?;
    tcp.set_nodelay(true).ok();
    let mut r = BufReader::new(tcp.try_clone().map_err(|e| io_err!("nats: {}", e))?);
    // options go on the handle that reads (a try_clone is a separate handle on Windows)
    r.get_ref().set_read_timeout(Some(cfg.timeout)).ok();
    let mut w = tcp.try_clone().map_err(|e| io_err!("nats: {}", e))?;
    // INFO → CONNECT + PING → PONG (or -ERR)
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| io_err!("nats: handshake: {}", e))?;
    if !line.starts_with("INFO") {
        return Err(io_err!("nats: unexpected greeting: {}", line.trim()));
    }
    let info: J = serde_json::from_str(line[4..].trim()).unwrap_or(J::Null);
    if info["tls_required"].as_bool() == Some(true) {
        return Err(io_err!("nats: the server requires TLS, which fogu:nats does not support yet"));
    }
    let mut c = json!({
        "verbose": false, "pedantic": false, "lang": "fogu", "version": env!("CARGO_PKG_VERSION"),
        "protocol": 1, "headers": true, "no_responders": true, "name": cfg.name,
    });
    if let Some(u) = &cfg.user {
        c["user"] = u.clone().into();
        c["pass"] = cfg.pass.clone().unwrap_or_default().into();
    }
    if let Some(t) = &cfg.token {
        c["auth_token"] = t.clone().into();
    }
    w.write_all(format!("CONNECT {}\r\nPING\r\n", c).as_bytes()).map_err(|e| io_err!("nats: {}", e))?;
    loop {
        let mut l = String::new();
        r.read_line(&mut l).map_err(|e| io_err!("nats: handshake: {}", e))?;
        let l = l.trim();
        if l == "PONG" {
            break;
        }
        if let Some(err) = l.strip_prefix("-ERR") {
            return Err(io_err!("nats: {}", err.trim().trim_matches('\'')));
        }
        if l.is_empty() {
            return Err(io_err!("nats: connection closed during the handshake"));
        }
    }
    r.get_ref().set_read_timeout(None).ok();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let writer = Arc::new(Mutex::new(w));
    {
        let (s, wr) = (Arc::clone(&shared), Arc::clone(&writer));
        std::thread::spawn(move || reader(r, s, wr));
    }
    let timeout = cfg.timeout;
    Ok(NativeObjectRef::new(NatsValue {
        conn: Rc::new(Conn { shared, writer, next_sid: Cell::new(0), pings: Cell::new(0), cfg, timeout }),
        info,
    })
    .to_value())
}

// ---------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------

fn payload(v: Option<&Value>) -> Vec<u8> {
    match v {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(other) => stringify_value(other).into_bytes(),
    }
}

fn headers_opt(o: &HashMap<String, Value>) -> Vec<(String, String)> {
    match o.get("headers") {
        Some(Value::Map(h)) => {
            let mut v: Vec<(String, String)> = h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect();
            v.sort();
            v
        }
        _ => Vec::new(),
    }
}

fn opts(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn timeout_opt(o: &HashMap<String, Value>, default: Duration, what: &str) -> Result<Duration, String> {
    match o.get("timeout") {
        None | Some(Value::Nil) => Ok(default),
        Some(t) => duration_arg(t, what),
    }
}

pub struct NatsValue {
    conn: Rc<Conn>,
    info: J,
}

impl FoguObject for NatsValue {
    fn name(&self) -> &str {
        "Nats"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "connected" => Value::Bool(self.conn.shared.lock().unwrap().closed.is_none()),
            "host" => Value::Str(self.conn.cfg.host.clone().into()),
            "port" => Value::Int(self.conn.cfg.port as i64),
            "server" => crate::native::util::json_to_value(&json!({
                "id": self.info["server_id"], "name": self.info["server_name"], "version": self.info["version"],
                "jetstream": self.info["jetstream"].as_bool().unwrap_or(false), "maxPayload": self.info["max_payload"],
            })),
            "lastError" => self.conn.shared.lock().unwrap().last_error.clone().map_or(Value::Nil, Value::str),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // publish(subject, data?, { headers, reply })
            "publish" => {
                max_args(args, 3, "publish")?;
                let subject = str_arg(args, 0, "publish")?;
                let o = opts(args, 2, "publish")?;
                let reply = get_s(&o, "reply");
                self.conn.publish(&subject, reply.as_deref(), &headers_opt(&o), &payload(args.get(1))).map(|_| Value::Nil)
            }
            // subscribe(subject, { queue, max }) → Subscription
            "subscribe" => {
                max_args(args, 2, "subscribe")?;
                let subject = str_arg(args, 0, "subscribe")?;
                let o = opts(args, 1, "subscribe")?;
                let max = match o.get("max") {
                    Some(Value::Int(n)) if *n > 0 => Some(*n as u64),
                    _ => None,
                };
                let queue = get_s(&o, "queue");
                let sid = self.conn.subscribe(&subject, queue.as_deref(), max)?;
                Ok(NativeObjectRef::new(SubValue { conn: Rc::clone(&self.conn), sid, subject, queue }).to_value())
            }
            "jetstream" => {
                max_args(args, 1, "jetstream")?;
                let o = opts(args, 0, "jetstream")?;
                Ok(NativeObjectRef::new(JsValue {
                    conn: Rc::clone(&self.conn),
                    prefix: get_s(&o, "prefix").unwrap_or_else(|| "$JS.API".into()),
                    timeout: timeout_opt(&o, self.conn.timeout, "jetstream: timeout")?,
                })
                .to_value())
            }
            // fogu() → the high-level layer over this connection
            "fogu" => {
                max_args(args, 0, "fogu")?;
                let nats = NativeObjectRef::new(NatsValue { conn: Rc::clone(&self.conn), info: self.info.clone() }).to_value();
                fogu::wrap(nats, None)
            }
            "close" => {
                max_args(args, 0, "close")?;
                let mut w = self.conn.writer.lock().unwrap();
                let _ = w.flush();
                let _ = w.shutdown(Shutdown::Both);
                self.conn.shared.lock().unwrap().closed.get_or_insert("closed by the client".into());
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Nats (publish subscribe request flush jetstream close)", other)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // request(subject, data?, { timeout, headers }) → Message
                "request" => {
                    max_args(args, 3, "request")?;
                    let subject = str_arg(args, 0, "request")?;
                    let o = opts(args, 2, "request")?;
                    let p = self.conn.request(&subject, &headers_opt(&o), &payload(args.get(1)), timeout_opt(&o, self.conn.timeout, "request: timeout")?)?;
                    let conn = Rc::clone(&self.conn);
                    Box::new(move || {
                        p.poll().map(|r| {
                            r.and_then(|m| {
                                if m.status == Some(503) {
                                    return Err(io_err!("nats: no responders for '{}'", subject));
                                }
                                Ok(msg_value(&conn, m))
                            })
                        })
                    })
                }
                // flush(): waits until the server processed everything sent
                "flush" => {
                    max_args(args, 0, "flush")?;
                    let target = {
                        let n = self.conn.pings.get() + 1;
                        self.conn.pings.set(n);
                        n
                    };
                    self.conn.write(b"PING\r\n")?;
                    let conn = Rc::clone(&self.conn);
                    let deadline = Instant::now() + conn.timeout;
                    Box::new(move || {
                        if conn.shared.lock().unwrap().pongs >= target {
                            return Some(Ok(Value::Nil));
                        }
                        if let Some(e) = conn.closed() {
                            return Some(Err(e));
                        }
                        if Instant::now() >= deadline {
                            return Some(Err(io_err!("nats: flush timed out")));
                        }
                        poll_again();
                        None
                    })
                }
                _ => return Ok(None),
            }))
        })();
        match r {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["publish", "subscribe", "request", "flush", "jetstream", "fogu", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct SubValue {
    conn: Rc<Conn>,
    sid: u64,
    subject: String,
    queue: Option<String>,
}

impl FoguObject for SubValue {
    fn name(&self) -> &str {
        "NatsSubscription"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "subject" => Value::Str(self.subject.clone().into()),
            "queue" => self.queue.clone().map_or(Value::Nil, Value::str),
            "pending" => Value::Int(self.conn.shared.lock().unwrap().subs.get(&self.sid).map_or(0, |q| q.len()) as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "tryNext" => {
                max_args(args, 0, "tryNext")?;
                Ok(self.conn.pop(self.sid).map_or(Value::Nil, |m| msg_value(&self.conn, m)))
            }
            "unsubscribe" | "close" => {
                max_args(args, 0, method)?;
                self.conn.unsubscribe(self.sid);
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on NatsSubscription (next tryNext unsubscribe)", other)),
        }
    }

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
        let (conn, sid) = (Rc::clone(&self.conn), self.sid);
        Some(Ok(Box::new(move || {
            if let Some(m) = conn.pop(sid) {
                return Some(Ok(msg_value(&conn, m)));
            }
            if let Some(e) = conn.closed() {
                return Some(Err(e));
            }
            if !conn.shared.lock().unwrap().subs.contains_key(&sid) {
                return Some(Ok(Value::Nil));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            poll_again();
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "unsubscribe"]
    }
}

/// A received message: `subject data reply headers seq` + `json() respond()`
/// and, for JetStream, `ack() nak() term() inProgress()`.
pub struct MsgValue {
    conn: Rc<Conn>,
    msg: Msg,
    acked: bool,
}

fn msg_value(conn: &Rc<Conn>, msg: Msg) -> Value {
    NativeObjectRef::new(MsgValue { conn: Rc::clone(conn), msg, acked: false }).to_value()
}

/// `$JS.ACK.<stream>.<consumer>.<delivered>.<stream seq>.<consumer seq>.<ts>.<pending>`
fn js_meta(reply: &str) -> Option<(String, i64, i64)> {
    let p: Vec<&str> = reply.split('.').collect();
    if p.len() < 9 || p[0] != "$JS" || p[1] != "ACK" {
        return None;
    }
    let off = p.len() - 9;
    Some((p[2 + off].to_string(), p[4 + off].parse().ok()?, p[5 + off].parse().ok()?))
}

impl FoguObject for MsgValue {
    fn name(&self) -> &str {
        "NatsMessage"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let m = &self.msg;
        Some(match field {
            "subject" => Value::Str(m.subject.clone().into()),
            "data" => Value::Str(String::from_utf8_lossy(&m.data).into_owned().into()),
            "reply" => m.reply.clone().map_or(Value::Nil, Value::str),
            "headers" => map(m.headers.iter().map(|(k, v)| (k.as_str(), Value::Str(v.clone().into()))).collect()),
            "size" => Value::Int(m.data.len() as i64),
            "status" => m.status.map_or(Value::Nil, |c| Value::Int(c as i64)),
            "description" => Value::Str(m.description.clone().into()),
            "seq" => m.reply.as_deref().and_then(js_meta).map_or(Value::Nil, |(_, _, s)| Value::Int(s)),
            "delivered" => m.reply.as_deref().and_then(js_meta).map_or(Value::Nil, |(_, d, _)| Value::Int(d)),
            "stream" => m.reply.as_deref().and_then(js_meta).map_or(Value::Nil, |(s, _, _)| Value::Str(s.into())),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let ack = |s: &mut Self, body: &str, final_ack: bool| -> Result<Value, String> {
            let reply = s.msg.reply.clone().ok_or_else(|| format!("{}: this message has no reply subject", method))?;
            if s.acked {
                return Err(format!("{}: the message was already acknowledged", method));
            }
            s.conn.publish(&reply, None, &[], body.as_bytes())?;
            if final_ack {
                s.acked = true;
            }
            Ok(Value::Nil)
        };
        match method {
            "json" => parse_value(&String::from_utf8_lossy(&self.msg.data)),
            // respond(data, { headers }) answers a request
            "respond" => {
                max_args(args, 2, "respond")?;
                let reply = self.msg.reply.clone().ok_or("respond: this message is not a request")?;
                let o = opts(args, 1, "respond")?;
                self.conn.publish(&reply, None, &headers_opt(&o), &payload(args.first())).map(|_| Value::Nil)
            }
            "ack" => ack(self, "+ACK", true),
            "nak" => {
                // nak(delay?) asks for redelivery
                let body = match args.first() {
                    Some(v) if !matches!(v, Value::Nil) => {
                        format!("-NAK {{\"delay\":{}}}", duration_arg(v, "nak")?.as_nanos())
                    }
                    _ => "-NAK".into(),
                };
                ack(self, &body, true)
            }
            "term" => ack(self, "+TERM", true),
            "inProgress" => ack(self, "+WPI", false),
            other => Err(format!("method '{}' does not exist on NatsMessage (json respond ack nak term inProgress)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["json", "respond", "ack", "nak", "term", "inProgress"]
    }
}

// ---------------------------------------------------------------------
// JetStream
// ---------------------------------------------------------------------

pub struct JsValue {
    conn: Rc<Conn>,
    prefix: String,
    timeout: Duration,
}

fn to_json(v: &Value) -> J {
    serde_json::from_str(&stringify_value(v)).unwrap_or(J::Null)
}

/// A JetStream API call: JSON request → JSON answer (an `error` field becomes an error value).
fn js_api(conn: &Rc<Conn>, subject: String, body: J, timeout: Duration, finish: impl FnOnce(J) -> Result<Value, String> + 'static) -> Result<Poller, String> {
    let data = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
    let p = conn.request(&subject, &[], &data, timeout)?;
    let mut finish = Some(finish);
    Ok(Box::new(move || {
        let r = p.poll()?;
        Some(r.and_then(|m| {
            if m.status == Some(503) {
                return Err(io_err!("nats: JetStream is not enabled on the server"));
            }
            let j: J = serde_json::from_slice(&m.data).map_err(|e| io_err!("nats: invalid JetStream answer: {}", e))?;
            if j["error"].is_object() {
                return Err(io_err!(
                    "nats: jetstream: {} ({})",
                    j["error"]["description"].as_str().unwrap_or("error"),
                    j["error"]["err_code"].as_i64().or_else(|| j["error"]["code"].as_i64()).unwrap_or(0)
                ));
            }
            finish.take().unwrap()(j)
        }))
    }))
}

fn json_value(j: &J) -> Value {
    crate::native::util::json_to_value(j)
}

impl FoguObject for JsValue {
    fn name(&self) -> &str {
        "JetStream"
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let conn = Rc::clone(&self.conn);
        let (prefix, timeout) = (self.prefix.clone(), self.timeout);
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // addStream({ name, subjects, storage, max_msgs, max_age… }) → info
                "addStream" | "updateStream" => {
                    max_args(args, 1, method)?;
                    let cfg = match args.first() {
                        Some(v @ Value::Map(_)) => to_json(v),
                        _ => return Err(format!("{} expects a config map with a 'name'", method)),
                    };
                    let name = cfg["name"].as_str().ok_or_else(|| format!("{}: the config needs a 'name'", method))?.to_string();
                    let verb = if method == "addStream" { "CREATE" } else { "UPDATE" };
                    js_api(&conn, format!("{}.STREAM.{}.{}", prefix, verb, name), cfg, timeout, |j| Ok(json_value(&j)))?
                }
                "streamInfo" | "deleteStream" | "purgeStream" => {
                    max_args(args, 1, method)?;
                    let name = str_arg(args, 0, method)?;
                    let verb = match method {
                        "streamInfo" => "INFO",
                        "deleteStream" => "DELETE",
                        _ => "PURGE",
                    };
                    let info = method == "streamInfo";
                    js_api(&conn, format!("{}.STREAM.{}.{}", prefix, verb, name), J::Null, timeout, move |j| {
                        Ok(if info { json_value(&j) } else { Value::Nil })
                    })?
                }
                // publish(subject, data, { msgId, headers, timeout }) → { stream, seq, duplicate }
                "publish" => {
                    max_args(args, 3, "publish")?;
                    let subject = str_arg(args, 0, "publish")?;
                    let o = opts(args, 2, "publish")?;
                    let mut headers = headers_opt(&o);
                    if let Some(id) = get_s(&o, "msgId") {
                        headers.push(("Nats-Msg-Id".into(), id));
                    }
                    let p = conn.request(&subject, &headers, &payload(args.get(1)), timeout_opt(&o, timeout, "publish: timeout")?)?;
                    Box::new(move || {
                        let r = p.poll()?;
                        Some(r.and_then(|m| {
                            if m.status == Some(503) {
                                return Err(io_err!("nats: no stream listens to '{}'", subject));
                            }
                            let j: J = serde_json::from_slice(&m.data).map_err(|e| io_err!("nats: invalid publish ack: {}", e))?;
                            if j["error"].is_object() {
                                return Err(io_err!("nats: jetstream: {}", j["error"]["description"].as_str().unwrap_or("error")));
                            }
                            Ok(map(vec![
                                ("stream", Value::Str(j["stream"].as_str().unwrap_or_default().into())),
                                ("seq", Value::Int(j["seq"].as_i64().unwrap_or(0))),
                                ("duplicate", Value::Bool(j["duplicate"].as_bool().unwrap_or(false))),
                            ]))
                        }))
                    })
                }
                // consumer(stream, { durable, filter, ackWait, deliver = "all", maxDeliver }) → Consumer
                "consumer" => {
                    max_args(args, 2, "consumer")?;
                    let stream = str_arg(args, 0, "consumer")?;
                    let o = opts(args, 1, "consumer")?;
                    let durable = get_s(&o, "durable").unwrap_or_else(|| format!("fogu_{}", &nuid()[..8]));
                    let mut c = json!({ "durable_name": durable, "name": durable, "ack_policy": "explicit",
                        "deliver_policy": get_s(&o, "deliver").unwrap_or_else(|| "all".into()) });
                    if let Some(f) = get_s(&o, "filter") {
                        c["filter_subject"] = f.into();
                    }
                    if let Some(w) = o.get("ackWait").filter(|v| !matches!(v, Value::Nil)) {
                        c["ack_wait"] = (duration_arg(w, "consumer: ackWait")?.as_nanos() as u64).into();
                    }
                    if let Some(Value::Int(n)) = o.get("maxDeliver") {
                        c["max_deliver"] = (*n).into();
                    }
                    let body = json!({ "stream_name": stream, "config": c });
                    let c2 = Rc::clone(&conn);
                    let (s2, d2, p2) = (stream.clone(), durable.clone(), prefix.clone());
                    js_api(&conn, format!("{}.CONSUMER.CREATE.{}.{}", prefix, stream, durable), body, timeout, move |_| {
                        Ok(NativeObjectRef::new(ConsumerValue { conn: c2, stream: s2, name: d2, prefix: p2, timeout }).to_value())
                    })?
                }
                "deleteConsumer" => {
                    max_args(args, 2, "deleteConsumer")?;
                    let stream = str_arg(args, 0, "deleteConsumer")?;
                    let name = str_arg(args, 1, "deleteConsumer")?;
                    js_api(&conn, format!("{}.CONSUMER.DELETE.{}.{}", prefix, stream, name), J::Null, timeout, |_| Ok(Value::Nil))?
                }
                _ => return Ok(None),
            }))
        })();
        match r {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["addStream", "updateStream", "streamInfo", "deleteStream", "purgeStream", "publish", "consumer", "deleteConsumer"]
    }
}

pub struct ConsumerValue {
    conn: Rc<Conn>,
    stream: String,
    name: String,
    prefix: String,
    timeout: Duration,
}

impl FoguObject for ConsumerValue {
    fn name(&self) -> &str {
        "NatsConsumer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "stream" => Value::Str(self.stream.clone().into()),
            "name" => Value::Str(self.name.clone().into()),
            _ => return None,
        })
    }

    /// `fetch(batch = 1, expires = 5s)` → up to `batch` messages (maybe fewer).
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "fetch" {
            return None;
        }
        let r = (|| -> Result<Poller, String> {
            max_args(args, 2, "fetch")?;
            let batch = match args.first() {
                None | Some(Value::Nil) => 1,
                Some(Value::Int(n)) if *n > 0 => *n as usize,
                Some(other) => return Err(format!("fetch: batch must be an int > 0, got {}", other.to_display_string())),
            };
            let expires = match args.get(1) {
                None | Some(Value::Nil) => self.timeout,
                Some(v) => duration_arg(v, "fetch: expires")?,
            };
            let inbox = format!("_INBOX.{}", nuid());
            let sid = self.conn.subscribe(&inbox, None, None)?;
            let req = json!({ "batch": batch, "expires": expires.as_nanos() as u64 });
            self.conn.publish(
                &format!("{}.CONSUMER.MSG.NEXT.{}.{}", self.prefix, self.stream, self.name),
                Some(&inbox),
                &[],
                req.to_string().as_bytes(),
            )?;
            let conn = Rc::clone(&self.conn);
            let deadline = Instant::now() + expires + Duration::from_millis(500);
            let mut got: Vec<Msg> = Vec::new();
            Ok(Box::new(move || {
                let mut done = false;
                while let Some(m) = conn.pop(sid) {
                    match m.status {
                        // 404 no messages, 408 request expired, 409 consumer changes
                        Some(404) | Some(408) | Some(409) => {
                            done = true;
                            break;
                        }
                        Some(100) => continue,
                        _ => got.push(m),
                    }
                    if got.len() >= batch {
                        done = true;
                        break;
                    }
                }
                if let Some(e) = conn.closed() {
                    return Some(Err(e));
                }
                if done || Instant::now() >= deadline {
                    conn.unsubscribe(sid);
                    let msgs: Vec<Value> = std::mem::take(&mut got).into_iter().map(|m| msg_value(&conn, m)).collect();
                    return Some(Ok(list(msgs)));
                }
                poll_again();
                None
            }))
        })();
        Some(r)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["fetch"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_headers() {
        let c = parse_cfg(Some(&Value::Str("nats://ana:s3cret@nats.example.com:4333".into()))).unwrap();
        assert_eq!((c.host.as_str(), c.port, c.user.as_deref(), c.pass.as_deref()), ("nats.example.com", 4333, Some("ana"), Some("s3cret")));
        let c = parse_cfg(Some(&Value::Str("nats://tok@localhost".into()))).unwrap();
        assert_eq!((c.token.as_deref(), c.port), (Some("tok"), 4222));
        let (status, desc, h) = parse_headers(b"NATS/1.0 503\r\n\r\n");
        assert_eq!((status, desc.as_str(), h.len()), (Some(503), "", 0));
        let (status, _, h) = parse_headers(&header_block(&[("X-Id".into(), "7".into())]));
        assert_eq!((status, h[0].1.as_str()), (None, "7"));
        assert_eq!(js_meta("$JS.ACK.ORDERS.billing.1.42.42.1700000000000000000.0"), Some(("ORDERS".into(), 1, 42)));
    }
}
