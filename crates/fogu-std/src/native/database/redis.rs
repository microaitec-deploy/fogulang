//! `fogu:redis` module — Redis / Valkey / KeyDB client (RESP2 over TCP or TLS).
//!
//! ```fogu
//! import redis from "fogu:redis"
//!
//! const r = redis.connect("redis://:secret@127.0.0.1:6379/0")?   // rediss:// for TLS
//! r.set("visits", 0)?
//! r.incr("visits")?
//! r.set("session:1", "ana", { ex: 3600 })?
//! r.hset("user:1", { name: "Ana", age: 30 })?
//! print(r.hgetall("user:1")?)                      // {age: 30, name: Ana} (values are str)
//! print(r.zrange("ranking", 0, -1, { withScores: true })?)
//! r.setJson("cfg", { theme: "dark" })?; print(r.getJson("cfg")?.theme)
//! print(r.pipeline([["INCR", "a"], ["INCR", "a"]])?)   // [1, 2]
//!
//! const sub = r.subscribe("news")?                 // its own connection
//! r.publish("news", "hello")?
//! print(sub.next(1s)?)                             // {channel: news, message: hello}
//! ```
//!
//! Every Redis command is available as a method (`r.lpush(...)`,
//! `r.zrangebyscore(...)`, `r.xadd(...)`), or through `r.command("CMD", ...)`.
//! Commands run on a connection thread and wait cooperatively. Redis errors
//! (`WRONGTYPE`, `NOAUTH`…) and network failures are `error` values.

use std::cell::RefCell;
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, list, map, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 0, connect));
    ModuleValue {
        name: "fogu:redis".into(),
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
    username: Option<String>,
    password: Option<String>,
    db: i64,
    tls: bool,
    timeout: Duration,
    name: Option<String>,
}

fn get_s(m: &HashMap<String, Value>, k: &str) -> Option<String> {
    match m.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

/// `redis://[user:pass@]host[:port][/db]`, `rediss://…` (TLS), or a map
/// `{ host, port, username, password, db, tls, timeout, name }`.
fn parse_cfg(arg: Option<&Value>) -> Result<Cfg, String> {
    let mut cfg = Cfg {
        host: "127.0.0.1".into(),
        port: 6379,
        username: None,
        password: None,
        db: 0,
        tls: false,
        timeout: Duration::from_secs(5),
        name: None,
    };
    let url = match arg {
        None | Some(Value::Nil) => std::env::var("REDIS_URL").ok(),
        Some(Value::Str(s)) => Some(s.to_string()),
        Some(Value::Map(m)) => {
            let m = m.borrow();
            if let Some(u) = get_s(&m, "url") {
                cfg = parse_url(&u, cfg)?;
            }
            if let Some(h) = get_s(&m, "host") {
                cfg.host = h;
            }
            match m.get("port") {
                Some(Value::Int(p)) if (1..=65535).contains(p) => cfg.port = *p as u16,
                None | Some(Value::Nil) => {}
                Some(other) => return Err(format!("redis.connect: invalid port {}", other.to_display_string())),
            }
            cfg.username = get_s(&m, "username").or(cfg.username);
            cfg.password = get_s(&m, "password").or(cfg.password);
            if let Some(Value::Int(db)) = m.get("db") {
                cfg.db = *db;
            }
            if let Some(Value::Bool(t)) = m.get("tls") {
                cfg.tls = *t;
            }
            if let Some(t) = m.get("timeout").filter(|v| !matches!(v, Value::Nil)) {
                cfg.timeout = duration_arg(t, "redis.connect: timeout")?;
            }
            cfg.name = get_s(&m, "name");
            return Ok(cfg);
        }
        Some(other) => return Err(format!("redis.connect expects a url or a map, got '{}'", other.type_name())),
    };
    match url {
        Some(u) => parse_url(&u, cfg),
        None => Ok(cfg),
    }
}

fn parse_url(url: &str, mut cfg: Cfg) -> Result<Cfg, String> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| format!("redis.connect: invalid url '{}'", url))?;
    cfg.tls = match scheme {
        "redis" | "valkey" => false,
        "rediss" | "valkeys" => true,
        other => return Err(format!("redis.connect: unknown scheme '{}' (redis, rediss)", other)),
    };
    let (main, _query) = rest.split_once('?').unwrap_or((rest, ""));
    let (hostpart, db) = match main.split_once('/') {
        Some((h, d)) => (h, d),
        None => (main, ""),
    };
    if !db.is_empty() {
        cfg.db = db.parse().map_err(|_| format!("redis.connect: invalid database '{}'", db))?;
    }
    let (auth, hostport) = match hostpart.rfind('@') {
        Some(i) => (Some(&hostpart[..i]), &hostpart[i + 1..]),
        None => (None, hostpart),
    };
    if let Some(a) = auth {
        let (u, p) = a.split_once(':').unwrap_or(("", a));
        if !u.is_empty() {
            cfg.username = Some(crate::native::util::url_decode(u));
        }
        if !p.is_empty() {
            cfg.password = Some(crate::native::util::url_decode(p));
        }
    }
    if let Some((h, p)) = hostport.rsplit_once(':').filter(|(h, _)| !h.ends_with(']') || h.starts_with('[')) {
        if !h.is_empty() {
            cfg.host = h.trim_matches(['[', ']']).to_string();
        }
        cfg.port = p.parse().map_err(|_| format!("redis.connect: invalid port '{}'", p))?;
    } else if !hostport.is_empty() {
        cfg.host = hostport.trim_matches(['[', ']']).to_string();
    }
    Ok(cfg)
}

// ---------------------------------------------------------------------
// RESP
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Resp {
    Simple(String),
    Error(String),
    Int(i64),
    Bulk(Option<Vec<u8>>),
    Array(Option<Vec<Resp>>),
}

fn encode(args: &[Vec<u8>]) -> Vec<u8> {
    let mut out = format!("*{}\r\n", args.len()).into_bytes();
    for a in args {
        out.extend(format!("${}\r\n", a.len()).into_bytes());
        out.extend(a);
        out.extend(b"\r\n");
    }
    out
}

fn read_resp<R: BufRead>(r: &mut R) -> std::io::Result<Resp> {
    let mut line = Vec::new();
    r.read_until(b'\n', &mut line)?;
    if line.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
    }
    while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        line.pop();
    }
    let (kind, rest) = (line[0], String::from_utf8_lossy(&line[1..]).into_owned());
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad reply: {}", rest));
    Ok(match kind {
        b'+' => Resp::Simple(rest),
        b'-' => Resp::Error(rest),
        b':' => Resp::Int(rest.parse().map_err(|_| bad())?),
        b'$' => {
            let n: i64 = rest.parse().map_err(|_| bad())?;
            if n < 0 {
                Resp::Bulk(None)
            } else {
                let mut buf = vec![0u8; n as usize + 2];
                r.read_exact(&mut buf)?;
                buf.truncate(n as usize);
                Resp::Bulk(Some(buf))
            }
        }
        b'*' => {
            let n: i64 = rest.parse().map_err(|_| bad())?;
            if n < 0 {
                Resp::Array(None)
            } else {
                let mut items = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    items.push(read_resp(r)?);
                }
                Resp::Array(Some(items))
            }
        }
        _ => return Err(bad()),
    })
}

// ---------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------

enum Stream {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

/// Opens a connection, authenticates and selects the database. Also returns
/// a handle to shut the socket down from another thread.
fn open(cfg: &Cfg) -> Result<(BufReader<Stream>, TcpStream), String> {
    let target = format!("{}:{}", cfg.host, cfg.port);
    let addr = target
        .to_socket_addrs()
        .map_err(|e| io_err!("redis: cannot resolve {}: {}", target, e))?
        .next()
        .ok_or_else(|| io_err!("redis: cannot resolve {}", target))?;
    let tcp = TcpStream::connect_timeout(&addr, cfg.timeout).map_err(|e| io_err!("redis: cannot connect to {}: {}", target, e))?;
    tcp.set_nodelay(true).ok();
    let handle = tcp.try_clone().map_err(|e| io_err!("redis: {}", e))?;
    let stream = if cfg.tls {
        let connector = native_tls::TlsConnector::new().map_err(|e| io_err!("redis: TLS: {}", e))?;
        Stream::Tls(Box::new(connector.connect(&cfg.host, tcp).map_err(|e| io_err!("redis: TLS handshake: {}", e))?))
    } else {
        Stream::Plain(tcp)
    };
    let mut conn = BufReader::new(stream);
    let mut hello: Vec<Vec<Vec<u8>>> = Vec::new();
    if let Some(p) = &cfg.password {
        let mut auth = vec![b"AUTH".to_vec()];
        if let Some(u) = &cfg.username {
            auth.push(u.as_bytes().to_vec());
        }
        auth.push(p.as_bytes().to_vec());
        hello.push(auth);
    }
    if cfg.db != 0 {
        hello.push(vec![b"SELECT".to_vec(), cfg.db.to_string().into_bytes()]);
    }
    if let Some(n) = &cfg.name {
        hello.push(vec![b"CLIENT".to_vec(), b"SETNAME".to_vec(), n.as_bytes().to_vec()]);
    }
    for cmd in hello {
        conn.get_mut().write_all(&encode(&cmd)).map_err(|e| io_err!("redis: {}", e))?;
        if let Resp::Error(e) = read_resp(&mut conn).map_err(|e| io_err!("redis: {}", e))? {
            return Err(io_err!("redis: {}", e));
        }
    }
    Ok((conn, handle))
}

type Job = (Vec<Vec<Vec<u8>>>, Sender<Result<Vec<Resp>, String>>);

/// The connection thread: runs batches of commands in order; reconnects on
/// the next batch after a network failure.
fn worker(cfg: Cfg, first: (BufReader<Stream>, TcpStream), rx: Receiver<Job>, handle: Arc<Mutex<Option<TcpStream>>>) {
    let mut conn = Some(first.0);
    *handle.lock().unwrap() = Some(first.1);
    while let Ok((cmds, reply)) = rx.recv() {
        if conn.is_none() {
            match open(&cfg) {
                Ok((c, h)) => {
                    conn = Some(c);
                    *handle.lock().unwrap() = Some(h);
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                    continue;
                }
            }
        }
        let c = conn.as_mut().unwrap();
        let result = (|| -> std::io::Result<Vec<Resp>> {
            let mut buf = Vec::new();
            for cmd in &cmds {
                buf.extend(encode(cmd));
            }
            c.get_mut().write_all(&buf)?;
            let mut out = Vec::with_capacity(cmds.len());
            for _ in &cmds {
                out.push(read_resp(c)?);
            }
            Ok(out)
        })();
        let result = result.map_err(|e| {
            conn = None;
            io_err!("redis: {}", e)
        });
        let _ = reply.send(result);
    }
}

struct Conn {
    tx: Sender<Job>,
    handle: Arc<Mutex<Option<TcpStream>>>,
    cfg: Cfg,
    closed: std::cell::Cell<bool>,
}

impl Conn {
    /// Sends a batch; the poller yields the replies.
    fn send(self: &Rc<Self>, cmds: Vec<Vec<Vec<u8>>>, finish: impl FnOnce(Vec<Resp>) -> Result<Value, String> + 'static) -> Result<Poller, String> {
        if self.closed.get() {
            return Err(io_err!("redis: the connection is closed"));
        }
        let (tx, rx) = channel();
        self.tx.send((cmds, tx)).map_err(|_| io_err!("redis: the connection is closed"))?;
        let mut finish = Some(finish);
        Ok(Box::new(move || match rx.try_recv() {
            Ok(Ok(replies)) => Some(finish.take().unwrap()(replies)),
            Ok(Err(e)) => Some(Err(e)),
            Err(TryRecvError::Empty) => {
                sched::add_timer(Instant::now() + Duration::from_millis(1));
                None
            }
            Err(TryRecvError::Disconnected) => Some(Err(io_err!("redis: the connection is closed"))),
        }))
    }
}

fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "redis.connect")?;
    let cfg = parse_cfg(args.first())?;
    let first = open(&cfg)?;
    let (tx, rx) = channel::<Job>();
    let handle = Arc::new(Mutex::new(None));
    {
        let (cfg, handle) = (cfg.clone(), Arc::clone(&handle));
        std::thread::spawn(move || worker(cfg, first, rx, handle));
    }
    Ok(NativeObjectRef::new(RedisValue { conn: Rc::new(Conn { tx, handle, cfg, closed: std::cell::Cell::new(false) }) }).to_value())
}

// ---------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------

/// Command arguments: maps become `k v k v`, lists are flattened, other
/// values become text (maps/lists inside lists as JSON).
fn push_arg(out: &mut Vec<Vec<u8>>, v: &Value, what: &str) -> Result<(), String> {
    match v {
        Value::Str(s) => out.push(s.as_bytes().to_vec()),
        Value::Int(n) => out.push(n.to_string().into_bytes()),
        Value::Float(f) => out.push(f.to_string().into_bytes()),
        Value::Bool(b) => out.push(if *b { b"1".to_vec() } else { b"0".to_vec() }),
        Value::Duration(ns) => out.push((ns / 1_000_000_000).to_string().into_bytes()),
        Value::Map(m) => {
            let m = m.borrow();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for k in keys {
                out.push(k.as_bytes().to_vec());
                push_scalar(out, &m[k]);
            }
        }
        Value::List(l) => {
            for item in l.borrow().iter() {
                push_scalar(out, item);
            }
        }
        Value::Nil => return Err(format!("{}: nil is not a valid argument", what)),
        other => out.push(other.to_display_string().into_bytes()),
    }
    Ok(())
}

fn push_scalar(out: &mut Vec<Vec<u8>>, v: &Value) {
    match v {
        Value::Map(_) | Value::List(_) => out.push(stringify_value(v).into_bytes()),
        Value::Str(s) => out.push(s.as_bytes().to_vec()),
        Value::Bool(b) => out.push(if *b { b"1".to_vec() } else { b"0".to_vec() }),
        Value::Nil => out.push(Vec::new()),
        other => out.push(other.to_display_string().into_bytes()),
    }
}

fn to_value(r: Resp) -> Value {
    match r {
        Resp::Simple(s) => Value::Str(s.into()),
        Resp::Error(e) => Value::Error(Rc::new(crate::runtime::value::ErrorValue {
            message: format!("redis: {}", e),
            cause: Value::Nil,
            trace: Vec::new(),
        })),
        Resp::Int(n) => Value::Int(n),
        Resp::Bulk(None) | Resp::Array(None) => Value::Nil,
        Resp::Bulk(Some(b)) => Value::Str(String::from_utf8_lossy(&b).into_owned().into()),
        Resp::Array(Some(items)) => list(items.into_iter().map(to_value).collect()),
    }
}

/// A single reply: a Redis error becomes an `error` value.
fn one(r: Resp) -> Result<Value, String> {
    match r {
        Resp::Error(e) => Err(io_err!("redis: {}", e)),
        other => Ok(to_value(other)),
    }
}

/// `[k, v, k, v]` → map
fn pairs_map(r: Resp) -> Result<Value, String> {
    match one(r)? {
        Value::List(l) => {
            let items = l.borrow();
            let mut m = HashMap::new();
            for pair in items.chunks(2) {
                if let [k, v] = pair {
                    m.insert(k.to_display_string(), v.clone());
                }
            }
            Ok(Value::Map(Rc::new(RefCell::new(m))))
        }
        other => Ok(other),
    }
}

fn opts(args: &[Value], i: usize) -> HashMap<String, Value> {
    match args.get(i) {
        Some(Value::Map(m)) => m.borrow().clone(),
        _ => HashMap::new(),
    }
}

pub struct RedisValue {
    conn: Rc<Conn>,
}

impl FoguObject for RedisValue {
    fn name(&self) -> &str {
        "Redis"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "host" => Value::Str(self.conn.cfg.host.clone().into()),
            "port" => Value::Int(self.conn.cfg.port as i64),
            "db" => Value::Int(self.conn.cfg.db),
            "closed" => Value::Bool(self.conn.closed.get()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "close" | "quit" => {
                max_args(args, 0, "close")?;
                self.conn.closed.set(true);
                if let Some(s) = self.conn.handle.lock().unwrap().take() {
                    let _ = s.shutdown(Shutdown::Both);
                }
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Redis", other)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method == "close" || method == "quit" {
            return None;
        }
        Some(self.command(method, args))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "get", "set", "del", "incr", "expire", "ttl", "keys", "hset", "hget", "hgetall", "lpush", "rpush", "lrange",
            "sadd", "smembers", "zadd", "zrange", "publish", "subscribe", "psubscribe", "pipeline", "transaction",
            "command", "getJson", "setJson", "close", "(any Redis command)",
        ]
    }
}

impl RedisValue {
    fn command(&self, method: &str, args: &[Value]) -> Result<Poller, String> {
        let conn = &self.conn;
        let mut cmd: Vec<Vec<u8>> = Vec::new();
        match method {
            "command" | "cmd" => {
                let name = str_arg(args, 0, method)?;
                cmd.push(name.into_bytes());
                for a in &args[1..] {
                    push_arg(&mut cmd, a, method)?;
                }
                conn.send(vec![cmd], |mut r| one(r.remove(0)))
            }
            // set(key, value, { ex, px, exat, nx, xx, keepTtl, get })
            "set" | "setJson" => {
                if args.len() < 2 {
                    return Err(format!("{}(key, value, options?) expects at least 2 arguments", method));
                }
                max_args(args, 3, method)?;
                cmd.push(b"SET".to_vec());
                cmd.push(str_arg(args, 0, method)?.into_bytes());
                if method == "setJson" {
                    cmd.push(stringify_value(&args[1]).into_bytes());
                } else {
                    push_scalar(&mut cmd, &args[1]);
                }
                let o = opts(args, 2);
                for (k, flag) in [("ex", "EX"), ("px", "PX"), ("exat", "EXAT"), ("pxat", "PXAT")] {
                    match o.get(k) {
                        Some(Value::Int(n)) => cmd.extend([flag.as_bytes().to_vec(), n.to_string().into_bytes()]),
                        Some(Value::Duration(ns)) => {
                            let n = if flag.starts_with('P') { ns / 1_000_000 } else { ns / 1_000_000_000 };
                            cmd.extend([flag.as_bytes().to_vec(), n.to_string().into_bytes()]);
                        }
                        _ => {}
                    }
                }
                for (k, flag) in [("nx", "NX"), ("xx", "XX"), ("keepTtl", "KEEPTTL"), ("get", "GET")] {
                    if o.get(k).is_some_and(|v| v.is_truthy()) {
                        cmd.push(flag.as_bytes().to_vec());
                    }
                }
                let get = o.get("get").is_some_and(|v| v.is_truthy());
                // SET … NX answers nil when the key exists: report it as false
                conn.send(vec![cmd], move |mut r| {
                    Ok(match r.remove(0) {
                        Resp::Error(e) => return Err(io_err!("redis: {}", e)),
                        Resp::Bulk(None) if !get => Value::Bool(false),
                        Resp::Simple(s) if s == "OK" && !get => Value::Bool(true),
                        other => to_value(other),
                    })
                })
            }
            "getJson" => {
                max_args(args, 1, "getJson")?;
                cmd.push(b"GET".to_vec());
                cmd.push(str_arg(args, 0, "getJson")?.into_bytes());
                conn.send(vec![cmd], |mut r| match one(r.remove(0))? {
                    Value::Str(s) => parse_value(&s).map_err(|e| io_err!("redis.getJson: {}", e)),
                    other => Ok(other),
                })
            }
            // zrange(key, start, stop, { withScores, rev, byScore }) → [member] or [{ member, score }]
            "zrange" => {
                if args.len() < 3 {
                    return Err("zrange(key, start, stop, options?) expects at least 3 arguments".into());
                }
                cmd.push(b"ZRANGE".to_vec());
                for a in &args[..3] {
                    push_arg(&mut cmd, a, "zrange")?;
                }
                let o = opts(args, 3);
                for (k, flag) in [("byScore", "BYSCORE"), ("rev", "REV"), ("withScores", "WITHSCORES")] {
                    if o.get(k).is_some_and(|v| v.is_truthy()) {
                        cmd.push(flag.as_bytes().to_vec());
                    }
                }
                let scores = o.get("withScores").is_some_and(|v| v.is_truthy());
                conn.send(vec![cmd], move |mut r| {
                    let v = one(r.remove(0))?;
                    if !scores {
                        return Ok(v);
                    }
                    let Value::List(l) = v else { return Ok(v) };
                    let items = l.borrow();
                    Ok(list(
                        items
                            .chunks(2)
                            .map(|p| {
                                let score = p.get(1).map(|s| s.to_display_string().parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0);
                                map(vec![("member", p[0].clone()), ("score", Value::Float(score))])
                            })
                            .collect(),
                    ))
                })
            }
            // pipeline([[cmd, ...args], …]) → replies (errors as error values)
            // transaction([...]) → MULTI … EXEC
            "pipeline" | "transaction" => {
                max_args(args, 1, method)?;
                let items = match args.first() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => return Err(format!("{} expects a list of commands", method)),
                };
                let mut cmds = Vec::new();
                if method == "transaction" {
                    cmds.push(vec![b"MULTI".to_vec()]);
                }
                for it in &items {
                    let mut c = Vec::new();
                    match it {
                        Value::List(parts) => {
                            for p in parts.borrow().iter() {
                                push_arg(&mut c, p, method)?;
                            }
                        }
                        Value::Str(s) => c.extend(s.split_whitespace().map(|w| w.as_bytes().to_vec())),
                        _ => return Err(format!("{}: each command is a list like [\"SET\", \"k\", \"v\"]", method)),
                    }
                    if c.is_empty() {
                        return Err(format!("{}: empty command", method));
                    }
                    cmds.push(c);
                }
                if method == "transaction" {
                    cmds.push(vec![b"EXEC".to_vec()]);
                }
                let tx = method == "transaction";
                conn.send(cmds, move |replies| {
                    if tx {
                        // the EXEC reply holds the results; a queued error aborts it
                        for r in &replies[..replies.len() - 1] {
                            if let Resp::Error(e) = r {
                                return Err(io_err!("redis: transaction aborted: {}", e));
                            }
                        }
                        return match replies.into_iter().last().unwrap() {
                            Resp::Array(None) => Err(io_err!("redis: transaction aborted (WATCH)")),
                            other => one(other),
                        };
                    }
                    Ok(list(replies.into_iter().map(to_value).collect()))
                })
            }
            "subscribe" | "psubscribe" => {
                let mut names = Vec::new();
                for a in args {
                    push_arg(&mut names, a, method)?;
                }
                if names.is_empty() {
                    return Err(format!("{} expects at least one channel", method));
                }
                let cfg = conn.cfg.clone();
                let pattern = method == "psubscribe";
                Ok(crate::native::util::thread_poller(
                    move || Subscription::start(cfg, pattern, names),
                    |s| Ok(NativeObjectRef::new(s).to_value()),
                ))
            }
            _ => {
                // any other method is a Redis command: hgetall, lpush, xadd…
                let name = method.to_uppercase();
                if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err(format!("'{}' is not a Redis command", method));
                }
                cmd.push(name.clone().into_bytes());
                for a in args {
                    push_arg(&mut cmd, a, method)?;
                }
                let as_map = matches!(name.as_str(), "HGETALL" | "CONFIG");
                let as_bool = matches!(name.as_str(), "EXISTS" | "SISMEMBER" | "HEXISTS" | "EXPIRE" | "PEXPIRE" | "PERSIST" | "SETNX" | "HSETNX" | "MSETNX")
                    && args.len() <= 2;
                conn.send(vec![cmd], move |mut r| {
                    let reply = r.remove(0);
                    if as_map {
                        return pairs_map(reply);
                    }
                    match (as_bool, one(reply)?) {
                        (true, Value::Int(n)) => Ok(Value::Bool(n > 0)),
                        (_, v) => Ok(v),
                    }
                })
            }
        }
    }
}

// ---------------------------------------------------------------------
// Pub/Sub
// ---------------------------------------------------------------------

#[derive(Default)]
struct Inbox {
    messages: VecDeque<(String, String, Option<String>)>,
    error: Option<String>,
}

pub struct Subscription {
    inbox: Arc<Mutex<Inbox>>,
    handle: TcpStream,
    channels: Vec<String>,
    pattern: bool,
    closed: bool,
}

impl Subscription {
    fn start(cfg: Cfg, pattern: bool, names: Vec<Vec<u8>>) -> Result<Subscription, String> {
        let (mut conn, handle) = open(&cfg)?;
        let verb: &[u8] = if pattern { b"PSUBSCRIBE" } else { b"SUBSCRIBE" };
        let mut cmd = vec![verb.to_vec()];
        cmd.extend(names.iter().cloned());
        conn.get_mut().write_all(&encode(&cmd)).map_err(|e| io_err!("redis: {}", e))?;
        // one confirmation per channel
        for _ in &names {
            match read_resp(&mut conn).map_err(|e| io_err!("redis: {}", e))? {
                Resp::Error(e) => return Err(io_err!("redis: {}", e)),
                Resp::Array(Some(_)) => {}
                other => return Err(io_err!("redis: unexpected reply {:?}", other)),
            }
        }
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        {
            let inbox = Arc::clone(&inbox);
            std::thread::spawn(move || loop {
                match read_resp(&mut conn) {
                    Ok(Resp::Array(Some(items))) => {
                        let text = |r: &Resp| match r {
                            Resp::Bulk(Some(b)) => String::from_utf8_lossy(b).into_owned(),
                            Resp::Simple(s) => s.clone(),
                            _ => String::new(),
                        };
                        let kind = items.first().map(text).unwrap_or_default();
                        let msg = match (kind.as_str(), items.len()) {
                            ("message", 3) => Some((text(&items[1]), text(&items[2]), None)),
                            ("pmessage", 4) => Some((text(&items[2]), text(&items[3]), Some(text(&items[1])))),
                            _ => None,
                        };
                        if let Some(m) = msg {
                            inbox.lock().unwrap().messages.push_back(m);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        inbox.lock().unwrap().error.get_or_insert(e.to_string());
                        return;
                    }
                }
            });
        }
        Ok(Subscription {
            inbox,
            handle,
            channels: names.iter().map(|n| String::from_utf8_lossy(n).into_owned()).collect(),
            pattern,
            closed: false,
        })
    }
}

fn message_value(m: (String, String, Option<String>)) -> Value {
    let mut pairs = vec![("channel", Value::Str(m.0.into())), ("message", Value::Str(m.1.into()))];
    if let Some(p) = m.2 {
        pairs.push(("pattern", Value::Str(p.into())));
    }
    map(pairs)
}

impl FoguObject for Subscription {
    fn name(&self) -> &str {
        "RedisSubscription"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "channels" => crate::native::util::str_list(self.channels.iter().cloned()),
            "pattern" => Value::Bool(self.pattern),
            "pending" => Value::Int(self.inbox.lock().unwrap().messages.len() as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "tryNext" => {
                max_args(args, 0, "tryNext")?;
                Ok(self.inbox.lock().unwrap().messages.pop_front().map_or(Value::Nil, message_value))
            }
            "close" => {
                self.closed = true;
                let _ = self.handle.shutdown(Shutdown::Both);
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on RedisSubscription (next tryNext close)", other)),
        }
    }

    /// `next(timeout?)` → `{ channel, message, pattern? }`, nil on timeout.
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
        if self.closed {
            return Some(Err(io_err!("redis: the subscription is closed")));
        }
        let inbox = Arc::clone(&self.inbox);
        Some(Ok(Box::new(move || {
            let mut b = inbox.lock().unwrap();
            if let Some(m) = b.messages.pop_front() {
                return Some(Ok(message_value(m)));
            }
            if let Some(e) = &b.error {
                return Some(Err(io_err!("redis: subscription closed: {}", e)));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            sched::add_timer(Instant::now() + Duration::from_millis(2));
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "close"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let base = parse_cfg(None).unwrap();
        let c = parse_url("rediss://user:p%40ss@cache.example.com:6380/2", base.clone()).unwrap();
        assert_eq!((c.host.as_str(), c.port, c.db, c.tls), ("cache.example.com", 6380, 2, true));
        assert_eq!((c.username.as_deref(), c.password.as_deref()), (Some("user"), Some("p@ss")));
        let c = parse_url("redis://:secret@localhost", base).unwrap();
        assert_eq!((c.host.as_str(), c.port, c.password.as_deref(), c.username), ("localhost", 6379, Some("secret"), None));
    }

    #[test]
    fn resp() {
        assert_eq!(encode(&[b"GET".to_vec(), b"k".to_vec()]), b"*2\r\n$3\r\nGET\r\n$1\r\nk\r\n");
        let data = b"*3\r\n:1\r\n$-1\r\n$5\r\nhello\r\n-ERR bad\r\n";
        let mut r = BufReader::new(&data[..]);
        assert_eq!(
            read_resp(&mut r).unwrap(),
            Resp::Array(Some(vec![Resp::Int(1), Resp::Bulk(None), Resp::Bulk(Some(b"hello".to_vec()))]))
        );
        assert_eq!(read_resp(&mut r).unwrap(), Resp::Error("ERR bad".into()));
    }
}
