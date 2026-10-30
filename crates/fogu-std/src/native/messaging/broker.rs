//! `fogu:broker` module (also `fogu:mq`) — the Fogu broker inside the
//! runtime: key/value store with TTLs, channels (rooms), pub/sub with
//! wildcard topics, RPC (`act`/`call`), API keys with scoped permissions,
//! and clusters of brokers. It speaks the FoguMQ wire protocol, so the
//! Bun, Go, Rust and Python clients — and `fogu:fogumq` — talk to it.
//!
//! ```fogu
//! import broker from "fogu:broker"
//!
//! const b = broker.start("127.0.0.1:7222", { token: "s3cret", db: "mq.db" })?
//! print(b.url)                                    // fogu://s3cret@127.0.0.1:7222
//!
//! const c = b.client()                            // inline client: no socket, same API as fogumq
//! c.kv.set("visits", 0)?
//! const room = c.channel("chat")
//! const svc = room.act("add")?                    // RPC handler
//! fg fn() {
//!     while true {
//!         const [r, err] = svc.next()                // an error once the broker closes
//!         if err != nil { break }
//!         r.reply(r.params.a + r.params.b)
//!     }
//! }()
//! print(room.call("add", { a: 1, b: 2 })?)        // 3
//! const sub = room.sub("news")?                   // = subscribe
//! room.pub("news", { id: 1 })?                    // = publish
//! print(sub.next(1s))
//!
//! // a cluster: messages, RPC routes and KV writes travel between the nodes
//! const n2 = broker.start("127.0.0.1:7223", { token: "s3cret", node: "n2", peers: ["127.0.0.1:7222"] })?
//! const remote = broker.connect("fogu://s3cret@127.0.0.1:7223")?   // = fogumq.connect
//! ```
//!
//! Cluster links: a node dials the `peers` it is given (one link per pair is
//! enough, either side may list the other) and authenticates with `secret`
//! (default: the root token). Channel messages and publications reach the
//! members/subscribers of every node; `call` goes to whichever node has the
//! method registered; KV writes made while the nodes are linked are copied
//! to every node (last write wins, `incr` is atomic per node; older keys are
//! not caught up).

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde_json::{json, Value as Json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{duration_arg, json_to_value, max_args, nfn_var, opt_arg};

// ---------------------------------------------------------------------
// protocol
// ---------------------------------------------------------------------

const MAGIC: [u8; 2] = [b'F', b'O'];
const VERSION: u8 = 1;
const HEADER: usize = 17;
const MAX_PAYLOAD: usize = 64 << 20;

const OP_PING: u8 = 0x01;
const OP_PONG: u8 = 0x02;
const OP_GET: u8 = 0x10;
const OP_SET: u8 = 0x11;
const OP_DEL: u8 = 0x12;
const OP_EXISTS: u8 = 0x13;
const OP_INCR: u8 = 0x20;
const OP_DECR: u8 = 0x21;
const OP_JOIN: u8 = 0x30;
const OP_LEAVE: u8 = 0x31;
const OP_CHANNEL_MSG: u8 = 0x32;
const OP_CHANNEL_LIST: u8 = 0x33;
const OP_MEMBERS: u8 = 0x34;
const OP_SUBSCRIBE: u8 = 0x40;
const OP_PUBLISH: u8 = 0x41;
const OP_AUTH: u8 = 0x50;
const OP_CALL: u8 = 0x60;
const OP_ACT: u8 = 0x61;
const OP_RES: u8 = 0x62;
const OP_APIKEY_CREATE: u8 = 0x70;
const OP_APIKEY_DELETE: u8 = 0x71;
const OP_APIKEY_LIST: u8 = 0x72;
const OP_APIKEY_GET: u8 = 0x73;
const OP_ERROR: u8 = 0xFF;
// between the nodes of a cluster
const OP_PEER_HELLO: u8 = 0x80;
const OP_PEER_PUBLISH: u8 = 0x81;
const OP_PEER_CHANNEL: u8 = 0x82;
const OP_PEER_ACTIONS: u8 = 0x83;
const OP_PEER_CALL: u8 = 0x84;
const OP_PEER_RES: u8 = 0x85;
const OP_PEER_KV: u8 = 0x86;

const ST_OK: u8 = 0x00;
const ST_ERROR: u8 = 0x02;
const ST_INVALID: u8 = 0x03;
const ST_UNAUTHORIZED: u8 = 0x04;
const ST_FORBIDDEN: u8 = 0x05;

#[derive(Clone, Debug)]
struct Frame {
    op: u8,
    id: u64,
    payload: Bytes,
}

impl Frame {
    fn new(op: u8, id: u64, payload: impl Into<Bytes>) -> Frame {
        Frame { op, id, payload: payload.into() }
    }

    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(HEADER + self.payload.len());
        b.extend_from_slice(&MAGIC);
        b.push(VERSION);
        b.push(self.op);
        b.push(0);
        b.extend_from_slice(&self.id.to_be_bytes());
        b.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        b.extend_from_slice(&self.payload);
        b
    }

    fn json(&self) -> Json {
        serde_json::from_slice(&self.payload).unwrap_or(Json::Null)
    }
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Frame> {
    let mut h = [0u8; HEADER];
    r.read_exact(&mut h).await?;
    if h[..2] != MAGIC || h[2] != VERSION {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad frame header"));
    }
    let len = u32::from_be_bytes([h[13], h[14], h[15], h[16]]) as usize;
    if len > MAX_PAYLOAD {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).await?;
    Ok(Frame { op: h[3], id: u64::from_be_bytes(h[5..13].try_into().expect("8 bytes")), payload: payload.into() })
}

/// An answer: status byte + JSON body.
fn ok_frame(id: u64, op: u8, body: Json) -> Frame {
    let mut p = vec![ST_OK];
    p.extend_from_slice(body.to_string().as_bytes());
    Frame::new(op, id, p)
}

fn err_frame(id: u64, status: u8, msg: &str) -> Frame {
    let mut p = vec![status];
    p.extend_from_slice(json!({ "error": msg }).to_string().as_bytes());
    Frame::new(OP_ERROR, id, p)
}

/// Dot-segmented wildcards: `*` one segment (or everything alone), `**` the rest.
fn matches_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    fn walk(p: &[&str], v: &[&str]) -> bool {
        match (p.first(), v.first()) {
            (None, None) => true,
            (Some(&"**"), _) => true,
            (Some(_), None) | (None, Some(_)) => false,
            (Some(&"*"), Some(_)) => walk(&p[1..], &v[1..]),
            (Some(a), Some(b)) => a == b && walk(&p[1..], &v[1..]),
        }
    }
    let p: Vec<&str> = pattern.split('.').collect();
    let v: Vec<&str> = value.split('.').collect();
    walk(&p, &v)
}

fn has_access(patterns: &[String], value: &str) -> bool {
    patterns.iter().any(|p| matches_pattern(p, value))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(subtle::ConstantTimeEq::ct_eq(a.as_bytes(), b.as_bytes()))
}

// ---------------------------------------------------------------------
// storage: KV and API keys (in memory, optionally persisted with SurrealKV)
// ---------------------------------------------------------------------

/// Key prefixes in the SurrealKV store.
const T_KV: &str = "kv/";
const T_KEYS: &str = "key/";

type DbWrite = (String, Option<String>);

/// The SurrealKV store of a broker: writes go, in order, to one writer task
/// that commits them in batches.
struct Db {
    tx: UnboundedSender<DbWrite>,
    done: std::sync::mpsc::Receiver<()>,
}

async fn db_writer(tree: surrealkv::Tree, mut rx: UnboundedReceiver<DbWrite>, done: std::sync::mpsc::Sender<()>) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while batch.len() < 1000 {
            match rx.try_recv() {
                Ok(w) => batch.push(w),
                Err(_) => break,
            }
        }
        let r = async {
            let mut t = tree.begin()?;
            for (k, v) in &batch {
                match v {
                    Some(v) => t.set(k.as_bytes(), v.as_bytes())?,
                    None => t.delete(k.as_bytes())?,
                }
            }
            t.commit().await
        }
        .await;
        if let Err(e) = r {
            eprintln!("[broker] db write failed: {}", e);
        }
    }
    let _ = tree.close().await;
    let _ = done.send(());
}

/// Every `(key, value)` of the store whose key starts with `prefix` (prefix removed).
fn db_scan(tree: &surrealkv::Tree, prefix: &str) -> Result<Vec<(String, String)>, String> {
    use surrealkv::LSMIterator;
    let t = tree.begin().map_err(|e| io_err!("broker: db: {}", e))?;
    let mut end = prefix.as_bytes().to_vec();
    if let Some(last) = end.last_mut() {
        *last += 1;
    }
    let mut it = t.range(prefix.as_bytes(), end.as_slice()).map_err(|e| io_err!("broker: db: {}", e))?;
    it.seek_first().map_err(|e| io_err!("broker: db: {}", e))?;
    let mut out = Vec::new();
    while it.valid() {
        let k = String::from_utf8_lossy(&it.key().user_key()[prefix.len()..]).into_owned();
        let v = String::from_utf8_lossy(&it.value().map_err(|e| io_err!("broker: db: {}", e))?).into_owned();
        out.push((k, v));
        it.next().map_err(|e| io_err!("broker: db: {}", e))?;
    }
    Ok(out)
}

/// A store written by earlier versions (a redb file): its rows, read once.
fn read_redb(path: &str) -> Result<Vec<(String, String, String)>, String> {
    const KV: TableDefinition<&str, &str> = TableDefinition::new("kv");
    const KEYS: TableDefinition<&str, &str> = TableDefinition::new("apikeys");
    let db = redb::Database::open(path).map_err(|e| io_err!("broker: db {}: {}", path, e))?;
    let r = db.begin_read().map_err(|e| io_err!("broker: db: {}", e))?;
    let mut out = Vec::new();
    for (table, prefix) in [(KV, T_KV), (KEYS, T_KEYS)] {
        let Ok(t) = r.open_table(table) else { continue };
        for item in t.iter().map_err(|e| io_err!("broker: db: {}", e))? {
            let (k, v) = item.map_err(|e| io_err!("broker: db: {}", e))?;
            out.push((prefix.to_string(), k.value().to_string(), v.value().to_string()));
        }
    }
    Ok(out)
}

struct Store {
    kv: Mutex<HashMap<String, (Json, Option<u64>)>>,
    keys: Mutex<HashMap<String, Json>>,
    /// taken (closed) by `Broker.close`
    db: Mutex<Option<Db>>,
}

impl Store {
    fn open(path: Option<&str>) -> Result<Store, String> {
        let s = Store { kv: Mutex::default(), keys: Mutex::default(), db: Mutex::new(None) };
        let Some(path) = path else { return Ok(s) };
        // a redb file of an earlier version: moved aside and copied into SurrealKV
        let migrate = if std::path::Path::new(path).is_file() {
            let rows = read_redb(path)?;
            let backup = format!("{}.redb.bak", path);
            std::fs::rename(path, &backup).map_err(|e| io_err!("broker: db {}: {}", path, e))?;
            eprintln!("[broker] {}: converted to SurrealKV (the redb file is kept as {})", path, backup);
            rows
        } else {
            Vec::new()
        };
        let rt = crate::native::rt::tokio();
        let tree = {
            let _g = rt.enter();
            surrealkv::TreeBuilder::new().with_path(path.into()).build().map_err(|e| io_err!("broker: db {}: {}", path, e))?
        };
        if !migrate.is_empty() {
            let r: Result<(), surrealkv::Error> = rt.block_on(async {
                let mut t = tree.begin()?;
                for (prefix, k, v) in &migrate {
                    t.set(format!("{}{}", prefix, k).as_bytes(), v.as_bytes())?;
                }
                t.commit().await
            });
            r.map_err(|e| io_err!("broker: db {}: {}", path, e))?;
        }
        {
            let mut kv = s.kv.lock().unwrap_or_else(|e| e.into_inner());
            for (k, v) in db_scan(&tree, T_KV)? {
                if let Ok(j) = serde_json::from_str::<Json>(&v) {
                    let exp = j.get("expires_at").and_then(Json::as_u64);
                    kv.insert(k, (j.get("data").cloned().unwrap_or(Json::Null), exp));
                }
            }
            let mut keys = s.keys.lock().unwrap_or_else(|e| e.into_inner());
            for (k, v) in db_scan(&tree, T_KEYS)? {
                if let Ok(j) = serde_json::from_str::<Json>(&v) {
                    keys.insert(k, j);
                }
            }
        }
        let (tx, rx) = unbounded_channel();
        let (done_tx, done) = std::sync::mpsc::channel();
        rt.spawn(db_writer(tree, rx, done_tx));
        crate::native::signal::arm_cleanup();
        *s.db.lock().unwrap_or_else(|e| e.into_inner()) = Some(Db { tx, done });
        Ok(s)
    }

    /// Writes what is queued and closes the store.
    fn close_db(&self) {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(Db { tx, done }) = db {
            drop(tx);
            let _ = done.recv_timeout(Duration::from_secs(10));
        }
    }

    fn persist(&self, prefix: &str, key: &str, value: Option<&str>) {
        let guard = self.db.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = guard.as_ref() {
            let _ = db.tx.send((format!("{}{}", prefix, key), value.map(String::from)));
        }
    }

    fn get(&self, key: &str) -> Option<Json> {
        let mut kv = self.kv.lock().unwrap_or_else(|e| e.into_inner());
        match kv.get(key) {
            Some((_, Some(exp))) if now_secs() > *exp => {
                kv.remove(key);
                drop(kv);
                self.persist(T_KV, key, None);
                None
            }
            Some((v, _)) => Some(v.clone()),
            None => None,
        }
    }

    fn set(&self, key: &str, value: Json, ttl: Option<u64>) {
        let exp = ttl.map(|t| now_secs() + t);
        self.persist(T_KV, key, Some(&json!({ "data": value, "expires_at": exp }).to_string()));
        self.kv.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), (value, exp));
    }

    fn del(&self, key: &str) -> bool {
        let had = self.kv.lock().unwrap_or_else(|e| e.into_inner()).remove(key).is_some();
        if had {
            self.persist(T_KV, key, None);
        }
        had
    }

    fn incr(&self, key: &str, delta: i64) -> Result<i64, String> {
        // under one lock: atomic on this node
        let mut kv = self.kv.lock().unwrap_or_else(|e| e.into_inner());
        let cur = match kv.get(key) {
            Some((_, Some(exp))) if now_secs() > *exp => 0,
            Some((v, _)) => v.as_i64().ok_or("value is not an integer")?,
            None => 0,
        };
        let new = cur.checked_add(delta).ok_or("integer overflow")?;
        kv.insert(key.to_string(), (json!(new), None));
        drop(kv);
        self.persist(T_KV, key, Some(&json!({ "data": new, "expires_at": null }).to_string()));
        Ok(new)
    }

    fn purge(&self) -> usize {
        let now = now_secs();
        let gone: Vec<String> = {
            let mut kv = self.kv.lock().unwrap_or_else(|e| e.into_inner());
            let gone: Vec<String> = kv.iter().filter(|(_, (_, e))| e.is_some_and(|e| now > e)).map(|(k, _)| k.clone()).collect();
            for k in &gone {
                kv.remove(k);
            }
            gone
        };
        for k in &gone {
            self.persist(T_KV, k, None);
        }
        gone.len()
    }

    fn key_by_token(&self, token: &str) -> Option<Json> {
        self.keys.lock().unwrap_or_else(|e| e.into_inner()).values().find(|k| k["token"].as_str().is_some_and(|t| ct_eq(t, token))).cloned()
    }

    fn put_key(&self, k: Json) {
        let id = k["id"].as_str().unwrap_or_default().to_string();
        self.persist(T_KEYS, &id, Some(&k.to_string()));
        self.keys.lock().unwrap_or_else(|e| e.into_inner()).insert(id, k);
    }

    fn del_key(&self, id: &str) -> bool {
        let had = self.keys.lock().unwrap_or_else(|e| e.into_inner()).remove(id).is_some();
        if had {
            self.persist(T_KEYS, id, None);
        }
        had
    }
}

/// What a client sees of a key (the token only when it is created).
fn key_view(k: &Json, with_token: bool) -> Json {
    let mut v = json!({ "id": k["id"], "name": k["name"], "perms": k["perms"], "created_at": k["created_at"] });
    if with_token {
        v["token"] = k["token"].clone();
    }
    v
}

// ---------------------------------------------------------------------
// the broker
// ---------------------------------------------------------------------

type Tx = UnboundedSender<Frame>;

#[derive(Clone)]
enum Role {
    Root,
    Key { channels: Vec<String>, kv: Vec<String>, topics: Vec<String>, rpc: Vec<String> },
}

impl Role {
    fn can(&self, what: &str, name: &str) -> bool {
        match self {
            Role::Root => true,
            Role::Key { channels, kv, topics, rpc } => has_access(
                match what {
                    "channel" => channels,
                    "kv" => kv,
                    "topic" => topics,
                    _ => rpc,
                },
                name,
            ),
        }
    }
}

enum Ev {
    Join { channel: String, cid: String, alias: String, tx: Tx },
    Leave { channel: String, cid: String },
    /// `sender` is None for messages from another node
    ChannelMsg { channel: String, sender: Option<String>, env: Bytes },
    ChannelList { tx: Tx, id: u64 },
    Members { channel: String, tx: Tx, id: u64 },
    Subscribe { topic: String, cid: String, tx: Tx },
    Publish { topic: String, env: Bytes, local_only: bool },
    Act { method: String, cid: String, tx: Tx },
    Call { id: u64, method: String, params: Json, tx: Tx },
    Res { brq: u64, payload: Bytes },
    Gone { cid: String },
    KvRepl(Json),
    PeerUp { node: String, link: u64, tx: Tx },
    PeerDown { node: String, link: u64 },
    FromPeer { node: String, frame: Frame },
    Stats { reply: std::sync::mpsc::Sender<Json> },
    Inspect { reply: std::sync::mpsc::Sender<Json> },
}

enum Pending {
    Local { tx: Tx, id: u64 },
    Remote { origin: String, call_id: u64 },
}

struct Core {
    token: String,
    node: String,
    secret: String,
    store: Store,
    tx: UnboundedSender<Ev>,
    clients: AtomicUsize,
    next_link: AtomicU64,
    closed: AtomicBool,
    /// set on close: every connection and peer link ends
    stop: tokio::sync::watch::Sender<bool>,
    log: bool,
    /// for `inspect()`: start time (ms since the epoch) and traffic counters
    started: u64,
    messages: AtomicU64,
    publications: AtomicU64,
    calls: AtomicU64,
}

impl Core {
    fn log(&self, msg: impl FnOnce() -> String) {
        if self.log {
            eprintln!("[broker {}] {}", self.node, msg());
        }
    }
}

async fn broker_loop(core: Arc<Core>, mut rx: UnboundedReceiver<Ev>) {
    let mut channels: HashMap<String, Vec<(String, String, Tx)>> = HashMap::new();
    let mut topics: HashMap<String, Vec<(String, Tx)>> = HashMap::new();
    let mut actions: HashMap<String, (String, Tx)> = HashMap::new();
    let mut remote_actions: HashMap<String, String> = HashMap::new();
    let mut links: HashMap<String, (u64, Tx)> = HashMap::new();
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    // ids of forwarded calls: far from the ids clients give their own requests
    // (they count from 1), so a client never mistakes a call for a reply
    let mut next = 1u64 << 48;

    let announce = |actions: &HashMap<String, (String, Tx)>, to: &[&Tx], node: &str| {
        let mut methods: Vec<&String> = actions.keys().collect();
        methods.sort();
        let f = Frame::new(OP_PEER_ACTIONS, 0, json!({ "node": node, "methods": methods }).to_string());
        for tx in to {
            let _ = tx.send(f.clone());
        }
    };
    let to_peers = |links: &HashMap<String, (u64, Tx)>, f: Frame| {
        for (_, tx) in links.values() {
            let _ = tx.send(f.clone());
        }
    };

    while let Some(ev) = rx.recv().await {
        match ev {
            Ev::Join { channel, cid, alias, tx } => {
                let m = channels.entry(channel).or_default();
                if !m.iter().any(|(id, _, _)| *id == cid) {
                    m.push((cid, alias, tx));
                }
            }
            Ev::Leave { channel, cid } => {
                if let Some(m) = channels.get_mut(&channel) {
                    m.retain(|(id, _, _)| *id != cid);
                    if m.is_empty() {
                        channels.remove(&channel);
                    }
                }
            }
            Ev::ChannelMsg { channel, sender, env } => {
                core.messages.fetch_add(1, Ordering::Relaxed);
                let members = channels.get(&channel);
                // a client must be a member to talk in a channel
                if let Some(s) = &sender {
                    if !members.is_some_and(|m| m.iter().any(|(id, _, _)| id == s)) {
                        continue;
                    }
                    to_peers(&links, Frame::new(OP_PEER_CHANNEL, 0, env.clone()));
                }
                for (_, _, tx) in members.into_iter().flatten() {
                    let _ = tx.send(Frame::new(OP_CHANNEL_MSG, 0, env.clone()));
                }
            }
            Ev::ChannelList { tx, id } => {
                let mut l: Vec<Json> = channels.iter().map(|(n, m)| json!({ "name": n, "member_count": m.len() })).collect();
                l.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                let _ = tx.send(ok_frame(id, OP_CHANNEL_LIST, json!({ "channels": l })));
            }
            Ev::Members { channel, tx, id } => {
                let l: Vec<Json> = channels.get(&channel).into_iter().flatten().map(|(c, a, _)| json!({ "client_id": c, "alias": a })).collect();
                let _ = tx.send(ok_frame(id, OP_MEMBERS, json!({ "members": l })));
            }
            Ev::Subscribe { topic, cid, tx } => {
                let l = topics.entry(topic).or_default();
                if !l.iter().any(|(id, _)| *id == cid) {
                    l.push((cid, tx));
                }
            }
            Ev::Publish { topic, env, local_only } => {
                core.publications.fetch_add(1, Ordering::Relaxed);
                for (pattern, subs) in &topics {
                    if matches_pattern(pattern, &topic) || matches_pattern(&topic, pattern) {
                        for (_, tx) in subs {
                            let _ = tx.send(Frame::new(OP_PUBLISH, 0, env.clone()));
                        }
                    }
                }
                if !local_only {
                    to_peers(&links, Frame::new(OP_PEER_PUBLISH, 0, env));
                }
            }
            Ev::Act { method, cid, tx } => {
                actions.insert(method, (cid, tx));
                let txs: Vec<&Tx> = links.values().map(|(_, t)| t).collect();
                announce(&actions, &txs, &core.node);
            }
            Ev::Call { id, method, params, tx } => {
                core.calls.fetch_add(1, Ordering::Relaxed);
                let brq = next;
                next += 1;
                if let Some((_, handler)) = actions.get(&method) {
                    pending.insert(brq, Pending::Local { tx: tx.clone(), id });
                    let f = Frame::new(OP_CALL, brq, json!({ "method": method, "params": params }).to_string());
                    if handler.send(f).is_err() {
                        pending.remove(&brq);
                        let _ = tx.send(err_frame(id, ST_ERROR, "RPC handler disconnected"));
                    }
                } else if let Some((_, link)) = remote_actions.get(&method).and_then(|n| links.get(n)) {
                    pending.insert(brq, Pending::Local { tx, id });
                    let _ = link.send(Frame::new(OP_PEER_CALL, brq, json!({ "method": method, "params": params, "origin": core.node }).to_string()));
                } else {
                    let _ = tx.send(err_frame(id, ST_ERROR, &format!("no handler for '{}'", method)));
                }
            }
            Ev::Res { brq, payload } => match pending.remove(&brq) {
                Some(Pending::Local { tx, id }) => {
                    let _ = tx.send(Frame::new(OP_RES, id, payload));
                }
                Some(Pending::Remote { origin, call_id }) => {
                    if let Some((_, link)) = links.get(&origin) {
                        let _ = link.send(Frame::new(OP_PEER_RES, call_id, payload));
                    }
                }
                None => {}
            },
            Ev::Gone { cid } => {
                for m in channels.values_mut() {
                    m.retain(|(id, _, _)| *id != cid);
                }
                channels.retain(|_, m| !m.is_empty());
                for l in topics.values_mut() {
                    l.retain(|(id, _)| *id != cid);
                }
                topics.retain(|_, l| !l.is_empty());
                let before = actions.len();
                actions.retain(|_, (id, _)| *id != cid);
                if actions.len() != before {
                    let txs: Vec<&Tx> = links.values().map(|(_, t)| t).collect();
                    announce(&actions, &txs, &core.node);
                }
            }
            Ev::KvRepl(op) => to_peers(&links, Frame::new(OP_PEER_KV, 0, op.to_string())),
            Ev::PeerUp { node, link, tx } => {
                core.log(|| format!("peer {} up", node));
                announce(&actions, &[&tx], &core.node);
                links.insert(node, (link, tx));
            }
            Ev::PeerDown { node, link } => {
                if links.get(&node).is_some_and(|(l, _)| *l == link) {
                    core.log(|| format!("peer {} down", node));
                    links.remove(&node);
                    remote_actions.retain(|_, n| *n != node);
                }
            }
            Ev::FromPeer { node, frame } => match frame.op {
                OP_PEER_PUBLISH => {
                    let topic = frame.json()["topic"].as_str().unwrap_or_default().to_string();
                    for (pattern, subs) in &topics {
                        if matches_pattern(pattern, &topic) || matches_pattern(&topic, pattern) {
                            for (_, tx) in subs {
                                let _ = tx.send(Frame::new(OP_PUBLISH, 0, frame.payload.clone()));
                            }
                        }
                    }
                }
                OP_PEER_CHANNEL => {
                    let ch = frame.json()["channel"].as_str().unwrap_or_default().to_string();
                    for (_, _, tx) in channels.get(&ch).into_iter().flatten() {
                        let _ = tx.send(Frame::new(OP_CHANNEL_MSG, 0, frame.payload.clone()));
                    }
                }
                OP_PEER_ACTIONS => {
                    let j = frame.json();
                    remote_actions.retain(|_, n| *n != node);
                    for m in j["methods"].as_array().into_iter().flatten().filter_map(Json::as_str) {
                        remote_actions.entry(m.to_string()).or_insert_with(|| node.clone());
                    }
                }
                OP_PEER_CALL => {
                    let j = frame.json();
                    let method = j["method"].as_str().unwrap_or_default().to_string();
                    let origin = j["origin"].as_str().unwrap_or(&node).to_string();
                    match actions.get(&method) {
                        Some((_, handler)) => {
                            let brq = next;
                            next += 1;
                            pending.insert(brq, Pending::Remote { origin, call_id: frame.id });
                            let _ = handler.send(Frame::new(OP_CALL, brq, json!({ "method": method, "params": j["params"] }).to_string()));
                        }
                        None => {
                            if let Some((_, link)) = links.get(&origin) {
                                let body = json!({ "result": null, "error": format!("no handler for '{}'", method) });
                                let _ = link.send(Frame::new(OP_PEER_RES, frame.id, body.to_string()));
                            }
                        }
                    }
                }
                OP_PEER_RES => {
                    if let Some(Pending::Local { tx, id }) = pending.remove(&frame.id) {
                        let _ = tx.send(Frame::new(OP_RES, id, frame.payload));
                    }
                }
                OP_PEER_KV => {
                    let j = frame.json();
                    let key = j["key"].as_str().unwrap_or_default();
                    match j["op"].as_str() {
                        Some("set") => core.store.set(key, j["value"].clone(), j["ttl"].as_u64()),
                        Some("del") => {
                            core.store.del(key);
                        }
                        _ => {}
                    }
                }
                _ => {}
            },
            Ev::Inspect { reply } => {
                let mut chans: Vec<Json> = channels
                    .iter()
                    .map(|(name, m)| json!({ "name": name, "members": m.iter().map(|(_, alias, _)| alias.clone()).collect::<Vec<_>>() }))
                    .collect();
                chans.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                let mut tops: Vec<Json> = topics.iter().map(|(t, s)| json!({ "pattern": t, "subscribers": s.len() })).collect();
                tops.sort_by(|a, b| a["pattern"].as_str().cmp(&b["pattern"].as_str()));
                let mut acts: Vec<&String> = actions.keys().collect();
                acts.sort();
                let mut ra: Vec<Json> = remote_actions.iter().map(|(m, n)| json!({ "method": m, "node": n })).collect();
                ra.sort_by(|a, b| a["method"].as_str().cmp(&b["method"].as_str()));
                let mut peers: Vec<&String> = links.keys().collect();
                peers.sort();
                let _ = reply.send(json!({
                    "node": core.node,
                    "started": core.started,
                    "clients": core.clients.load(Ordering::SeqCst),
                    "channels": chans,
                    "topics": tops,
                    "actions": acts,
                    "remoteActions": ra,
                    "peers": peers,
                    "keys": core.store.kv.lock().map(|k| k.len()).unwrap_or(0),
                    "apiKeys": core.store.keys.lock().map(|k| k.len()).unwrap_or(0),
                    "pendingCalls": pending.len(),
                    "messages": core.messages.load(Ordering::Relaxed),
                    "publications": core.publications.load(Ordering::Relaxed),
                    "calls": core.calls.load(Ordering::Relaxed),
                    "persistent": core.store.db.lock().map(|d| d.is_some()).unwrap_or(false),
                }));
            }
            Ev::Stats { reply } => {
                let mut peers: Vec<&String> = links.keys().collect();
                peers.sort();
                let mut ra: Vec<&String> = remote_actions.keys().collect();
                ra.sort();
                let mut acts: Vec<&String> = actions.keys().collect();
                acts.sort();
                let _ = reply.send(json!({
                    "node": core.node,
                    "clients": core.clients.load(Ordering::SeqCst),
                    "channels": channels.len(),
                    "topics": topics.len(),
                    "actions": acts,
                    "remoteActions": ra,
                    "peers": peers,
                    "keys": core.store.kv.lock().map(|k| k.len()).unwrap_or(0),
                }));
            }
        }
    }
}

/// One connection (TCP, inline pipe or another node).
async fn serve_conn<R, W>(core: Arc<Core>, mut rd: R, mut wr: W, peer: String)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut out) = unbounded_channel::<Frame>();
    let writer = tokio::spawn(async move {
        while let Some(f) = out.recv().await {
            if wr.write_all(&f.encode()).await.is_err() {
                break;
            }
        }
        let _ = wr.shutdown().await;
    });
    let cid = uuid::Uuid::new_v4().to_string();
    let alias = cid[..8].to_string();
    let mut role: Option<Role> = None;
    core.clients.fetch_add(1, Ordering::SeqCst);
    core.log(|| format!("client {} connected ({})", alias, peer));

    let mut stop = core.stop.subscribe();
    loop {
        let f = tokio::select! {
            r = read_frame(&mut rd) => r.ok(),
            _ = stop.changed() => None,
        };
        let Some(f) = f else { break };
        // another node of the cluster
        if f.op == OP_PEER_HELLO && role.is_none() {
            let j = f.json();
            let node = j["node"].as_str().unwrap_or_default().to_string();
            if !j["secret"].as_str().is_some_and(|s| ct_eq(s, &core.secret)) || node.is_empty() || node == core.node {
                let _ = tx.send(err_frame(f.id, ST_UNAUTHORIZED, "cluster: bad secret or node"));
                break;
            }
            let _ = tx.send(Frame::new(OP_PEER_HELLO, f.id, json!({ "node": core.node }).to_string()));
            core.clients.fetch_sub(1, Ordering::SeqCst);
            peer_link(core, rd, tx, node).await;
            drop(writer);
            return;
        }
        if f.op == OP_AUTH {
            if role.is_some() {
                let _ = tx.send(err_frame(f.id, ST_ERROR, "already authenticated"));
                continue;
            }
            let token = f.json()["token"].as_str().unwrap_or_default().to_string();
            if ct_eq(&token, &core.token) {
                role = Some(Role::Root);
                let _ = tx.send(ok_frame(f.id, OP_AUTH, json!({ "result": { "role": "root" } })));
            } else if let Some(k) = core.store.key_by_token(&token) {
                let list = |n: &str| k["perms"][n].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                role = Some(Role::Key { channels: list("channels"), kv: list("kv"), topics: list("topics"), rpc: list("rpc") });
                let _ = tx.send(ok_frame(f.id, OP_AUTH, json!({ "result": { "role": "api_key" } })));
            } else {
                let _ = tx.send(err_frame(f.id, ST_UNAUTHORIZED, "invalid token"));
            }
            continue;
        }
        let Some(r) = &role else {
            let _ = tx.send(err_frame(f.id, ST_UNAUTHORIZED, "not authenticated"));
            continue;
        };
        if let Err(e) = handle(&core, r, &cid, &alias, &tx, f.clone()) {
            let _ = tx.send(err_frame(f.id, ST_INVALID, &e));
        }
    }
    let _ = core.tx.send(Ev::Gone { cid });
    core.clients.fetch_sub(1, Ordering::SeqCst);
    core.log(|| format!("client {} disconnected", alias));
    drop(tx);
    let _ = writer.await;
}

/// The frames of an authenticated client.
fn handle(core: &Arc<Core>, role: &Role, cid: &str, alias: &str, tx: &Tx, f: Frame) -> Result<(), String> {
    let j = || -> Result<Json, String> { serde_json::from_slice(&f.payload).map_err(|e| format!("invalid JSON: {}", e)) };
    let s = |v: &Json, k: &str| v[k].as_str().map(String::from).ok_or_else(|| format!("missing '{}'", k));
    let forbid = |what: &str, name: &str| err_frame(f.id, ST_FORBIDDEN, &format!("no {} access to '{}'", what, name));
    let send = |fr: Frame| {
        let _ = tx.send(fr);
    };
    let ev = |e: Ev| {
        let _ = core.tx.send(e);
    };
    match f.op {
        OP_PING => send(Frame::new(OP_PONG, f.id, vec![ST_OK])),
        OP_PONG | OP_ERROR => {}
        OP_GET | OP_SET | OP_DEL | OP_EXISTS | OP_INCR | OP_DECR => {
            let p = j()?;
            let key = s(&p, "key")?;
            if !role.can("kv", &key) {
                send(forbid("kv", &key));
                return Ok(());
            }
            match f.op {
                OP_GET => send(ok_frame(f.id, OP_GET, json!({ "value": core.store.get(&key).unwrap_or(Json::Null) }))),
                OP_SET => {
                    let ttl = p["ttl"].as_u64();
                    core.store.set(&key, p["value"].clone(), ttl);
                    ev(Ev::KvRepl(json!({ "op": "set", "key": key, "value": p["value"], "ttl": ttl })));
                    send(ok_frame(f.id, OP_SET, json!({ "result": "ok" })));
                }
                OP_DEL => {
                    let n = core.store.del(&key) as usize;
                    ev(Ev::KvRepl(json!({ "op": "del", "key": key })));
                    send(ok_frame(f.id, OP_DEL, json!({ "count": n })));
                }
                OP_EXISTS => send(ok_frame(f.id, OP_EXISTS, json!({ "exists": core.store.get(&key).is_some() }))),
                _ => {
                    let d = p["delta"].as_i64().unwrap_or(1);
                    let v = core.store.incr(&key, if f.op == OP_INCR { d } else { -d })?;
                    ev(Ev::KvRepl(json!({ "op": "set", "key": key, "value": v })));
                    send(ok_frame(f.id, f.op, json!({ "value": v })));
                }
            }
        }
        OP_JOIN | OP_LEAVE | OP_CHANNEL_MSG | OP_MEMBERS => {
            let p = j()?;
            let channel = s(&p, "channel")?;
            if f.op != OP_LEAVE && !role.can("channel", &channel) {
                send(forbid("channel", &channel));
                return Ok(());
            }
            match f.op {
                OP_JOIN => {
                    let a = p["alias"].as_str().unwrap_or(alias).to_string();
                    ev(Ev::Join { channel: channel.clone(), cid: cid.into(), alias: a, tx: tx.clone() });
                    send(ok_frame(f.id, OP_JOIN, json!({ "result": { "channel": channel, "status": "joined" } })));
                }
                OP_LEAVE => {
                    ev(Ev::Leave { channel: channel.clone(), cid: cid.into() });
                    send(ok_frame(f.id, OP_LEAVE, json!({ "result": { "channel": channel, "status": "left" } })));
                }
                OP_CHANNEL_MSG => {
                    let env = json!({ "channel": channel, "sender_id": cid, "alias": alias, "data": p["data"], "node": core.node });
                    ev(Ev::ChannelMsg { channel: channel.clone(), sender: Some(cid.into()), env: env.to_string().into() });
                    send(ok_frame(f.id, OP_CHANNEL_MSG, json!({ "result": { "channel": channel, "status": "sent" } })));
                }
                _ => ev(Ev::Members { channel, tx: tx.clone(), id: f.id }),
            }
        }
        OP_CHANNEL_LIST => ev(Ev::ChannelList { tx: tx.clone(), id: f.id }),
        OP_SUBSCRIBE | OP_PUBLISH => {
            let p = j()?;
            let topic = s(&p, "topic")?;
            if !role.can("topic", &topic) {
                send(forbid("topic", &topic));
                return Ok(());
            }
            if f.op == OP_SUBSCRIBE {
                ev(Ev::Subscribe { topic: topic.clone(), cid: cid.into(), tx: tx.clone() });
                send(ok_frame(f.id, OP_SUBSCRIBE, json!({ "result": { "topic": topic, "status": "subscribed" } })));
            } else {
                let env = json!({ "topic": topic, "data": p["data"] });
                ev(Ev::Publish { topic, env: env.to_string().into(), local_only: false });
                send(ok_frame(f.id, OP_PUBLISH, json!({ "result": "published" })));
            }
        }
        OP_ACT | OP_CALL => {
            let p = j()?;
            let method = s(&p, "method")?;
            if !role.can("rpc", &method) {
                send(forbid("rpc", &method));
                return Ok(());
            }
            if f.op == OP_ACT {
                ev(Ev::Act { method: method.clone(), cid: cid.into(), tx: tx.clone() });
                send(ok_frame(f.id, OP_ACT, json!({ "result": { "method": method, "status": "registered" } })));
            } else {
                ev(Ev::Call { id: f.id, method, params: p["params"].clone(), tx: tx.clone() });
            }
        }
        OP_RES => ev(Ev::Res { brq: f.id, payload: f.payload }),
        OP_APIKEY_CREATE | OP_APIKEY_DELETE | OP_APIKEY_LIST | OP_APIKEY_GET => {
            if !matches!(role, Role::Root) {
                send(err_frame(f.id, ST_FORBIDDEN, "only root can manage API keys"));
                return Ok(());
            }
            match f.op {
                OP_APIKEY_CREATE => {
                    let p = j()?;
                    let arr = |k: &str| p[k].as_array().cloned().unwrap_or_default();
                    let key = json!({
                        "id": uuid::Uuid::new_v4().to_string(),
                        "token": format!("fk_{}", uuid::Uuid::new_v4().simple()),
                        "name": s(&p, "name")?,
                        "perms": { "channels": arr("channels"), "kv": arr("kv"), "topics": arr("topics"), "rpc": arr("rpc") },
                        "created_at": now_secs(),
                    });
                    core.store.put_key(key.clone());
                    send(ok_frame(f.id, OP_APIKEY_CREATE, json!({ "api_key": key_view(&key, true) })));
                }
                OP_APIKEY_DELETE => {
                    let id = s(&j()?, "id")?;
                    if core.store.del_key(&id) {
                        send(ok_frame(f.id, OP_APIKEY_DELETE, json!({ "result": { "id": id, "status": "deleted" } })));
                    } else {
                        send(err_frame(f.id, ST_ERROR, &format!("API key '{}' not found", id)));
                    }
                }
                OP_APIKEY_LIST => {
                    let mut keys: Vec<Json> = core.store.keys.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect();
                    keys.sort_by_key(|k| k["created_at"].as_u64().unwrap_or(0));
                    send(ok_frame(f.id, OP_APIKEY_LIST, json!({ "api_keys": keys.iter().map(|k| key_view(k, false)).collect::<Vec<_>>() })));
                }
                _ => {
                    let id = s(&j()?, "id")?;
                    let k = core.store.keys.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned();
                    match k {
                        Some(k) => send(ok_frame(f.id, OP_APIKEY_GET, json!({ "api_key": key_view(&k, false) }))),
                        None => send(err_frame(f.id, ST_ERROR, &format!("API key '{}' not found", id))),
                    }
                }
            }
        }
        other => send(err_frame(f.id, ST_ERROR, &format!("unexpected opcode 0x{:02x}", other))),
    }
    Ok(())
}

/// A link to another node: its frames go to the broker loop; ours go through `tx`.
async fn peer_link<R: AsyncRead + Unpin>(core: Arc<Core>, mut rd: R, tx: Tx, node: String) {
    let link = core.next_link.fetch_add(1, Ordering::SeqCst);
    let _ = core.tx.send(Ev::PeerUp { node: node.clone(), link, tx });
    let mut stop = core.stop.subscribe();
    loop {
        let frame = tokio::select! {
            r = read_frame(&mut rd) => r.ok(),
            _ = stop.changed() => None,
        };
        let Some(frame) = frame else { break };
        if core.tx.send(Ev::FromPeer { node: node.clone(), frame }).is_err() {
            break;
        }
    }
    let _ = core.tx.send(Ev::PeerDown { node, link });
}

/// Keeps a link to `addr` open (reconnecting) while the broker runs.
async fn dial_peer(core: Arc<Core>, addr: String) {
    let mut wait = Duration::from_millis(200);
    while !core.closed.load(Ordering::SeqCst) {
        if let Ok(tcp) = tokio::net::TcpStream::connect(&addr).await {
            let _ = tcp.set_nodelay(true);
            let (mut rd, mut wr) = tcp.into_split();
            let hello = Frame::new(OP_PEER_HELLO, 1, json!({ "node": core.node, "secret": core.secret }).to_string());
            if wr.write_all(&hello.encode()).await.is_ok() {
                if let Ok(Ok(reply)) = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut rd)).await {
                    if reply.op == OP_PEER_HELLO {
                        let node = reply.json()["node"].as_str().unwrap_or_default().to_string();
                        let (tx, mut out) = unbounded_channel::<Frame>();
                        let writer = tokio::spawn(async move {
                            while let Some(f) = out.recv().await {
                                if wr.write_all(&f.encode()).await.is_err() {
                                    break;
                                }
                            }
                        });
                        wait = Duration::from_millis(200);
                        peer_link(Arc::clone(&core), rd, tx, node).await;
                        writer.abort();
                    } else {
                        core.log(|| format!("peer {} refused the link: {}", addr, String::from_utf8_lossy(&reply.payload)));
                    }
                }
            }
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(5));
    }
}

// ---------------------------------------------------------------------
// Fogu values
// ---------------------------------------------------------------------

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    let start = nfn_var("start", 0, start);
    e.insert("start".into(), start.clone());
    e.insert("broker".into(), start);
    // connect(url, opts) → a client of a remote broker (same as fogu:fogumq)
    e.insert("connect".into(), nfn_var("connect", 1, crate::native::fogumq::connect));
    ModuleValue {
        name: "fogu:broker".into(),
        exports: RefCell::new(e),
    }
}

/// start(addr = "127.0.0.1:7222", { token, db, node, peers, secret, log }) → Broker
fn start(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "broker.start")?;
    let (addr, o) = match (a.first(), a.get(1)) {
        (Some(Value::Map(m)), None) => ("127.0.0.1:7222".to_string(), Some(m.clone())),
        (None, _) => ("127.0.0.1:7222".to_string(), None),
        (Some(Value::Int(p)), o) => (format!("127.0.0.1:{}", p), o.and_then(|v| if let Value::Map(m) = v { Some(m.clone()) } else { None })),
        (Some(v), o) => (v.to_display_string(), o.and_then(|v| if let Value::Map(m) = v { Some(m.clone()) } else { None })),
    };
    let o: HashMap<String, Value> = o.map(|m| m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    for k in o.keys() {
        if !["token", "db", "node", "peers", "secret", "log", "purgeEvery"].contains(&k.as_str()) {
            return Err(format!("broker.start: unknown option '{}' (token db node peers secret log purgeEvery)", k));
        }
    }
    let token = match o.get("token") {
        Some(t) => t.to_display_string(),
        None => std::env::var("FOGU_ROOT_TOKEN").ok().filter(|t| !t.is_empty()).unwrap_or_else(|| format!("fr_{}", uuid::Uuid::new_v4().simple())),
    };
    let peers: Vec<String> = match o.get("peers") {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        Some(v) => vec![v.to_display_string()],
    };
    let purge = match o.get("purgeEvery") {
        Some(v) => duration_arg(v, "broker.start purgeEvery")?,
        None => Duration::from_secs(60),
    };
    let store = Store::open(o.get("db").map(|v| v.to_display_string()).as_deref())?;
    let std_l = std::net::TcpListener::bind(&addr).map_err(|e| io_err!("broker: cannot listen on {}: {}", addr, e))?;
    std_l.set_nonblocking(true).map_err(|e| io_err!("broker: {}", e))?;
    let local = std_l.local_addr().map_err(|e| io_err!("broker: {}", e))?;
    let node = o.get("node").map(|v| v.to_display_string()).unwrap_or_else(|| local.to_string());
    let (tx, rx) = unbounded_channel();
    let core = Arc::new(Core {
        secret: o.get("secret").map(|v| v.to_display_string()).unwrap_or_else(|| token.clone()),
        token,
        node,
        store,
        tx,
        clients: AtomicUsize::new(0),
        next_link: AtomicU64::new(1),
        closed: AtomicBool::new(false),
        stop: tokio::sync::watch::channel(false).0,
        log: o.get("log").is_some_and(Value::is_truthy),
        started: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
        messages: AtomicU64::new(0),
        publications: AtomicU64::new(0),
        calls: AtomicU64::new(0),
    });
    OPEN.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::clone(&core));
    let rt = crate::native::rt::tokio();
    let listener = {
        let _g = rt.enter();
        tokio::net::TcpListener::from_std(std_l).map_err(|e| io_err!("broker: {}", e))?
    };
    let mut tasks = vec![rt.spawn(broker_loop(Arc::clone(&core), rx))];
    let c = Arc::clone(&core);
    tasks.push(rt.spawn(async move {
        loop {
            let Ok((tcp, peer)) = listener.accept().await else { continue };
            let _ = tcp.set_nodelay(true);
            let (rd, wr) = tcp.into_split();
            tokio::spawn(serve_conn(Arc::clone(&c), rd, wr, peer.to_string()));
        }
    }));
    let c = Arc::clone(&core);
    tasks.push(rt.spawn(async move {
        let mut t = tokio::time::interval(purge);
        t.tick().await;
        loop {
            t.tick().await;
            let n = c.store.purge();
            if n > 0 {
                c.log(|| format!("TTL purge: {} keys removed", n));
            }
        }
    }));
    for p in peers.into_iter().filter(|p| *p != local.to_string() && *p != addr) {
        tasks.push(rt.spawn(dial_peer(Arc::clone(&core), p)));
    }
    Ok(NativeObjectRef::new(BrokerValue { core, tasks, addr: local }).to_value())
}

pub struct BrokerValue {
    core: Arc<Core>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    addr: std::net::SocketAddr,
}

/// The brokers still running, closed when the program exits (their stores are flushed).
static OPEN: Mutex<Vec<Arc<Core>>> = Mutex::new(Vec::new());

pub fn close_all() {
    let open: Vec<Arc<Core>> = OPEN.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect();
    for core in open {
        core.closed.store(true, Ordering::SeqCst);
        let _ = core.stop.send(true);
        core.store.close_db();
    }
}

impl BrokerValue {
    fn close(&mut self) {
        OPEN.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| !Arc::ptr_eq(c, &self.core));
        self.core.closed.store(true, Ordering::SeqCst);
        let _ = self.core.stop.send(true);
        for t in self.tasks.drain(..) {
            t.abort();
        }
        self.core.store.close_db();
    }
}

impl Drop for BrokerValue {
    fn drop(&mut self) {
        self.close();
    }
}

/// The std side of an inline client: frames from the broker arrive as chunks.
struct PipeReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct PipeWriter {
    tx: UnboundedSender<Vec<u8>>,
}

impl Write for PipeWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.is_empty() {
            return Ok(0);
        }
        self.tx.send(b.to_vec()).map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broker closed"))?;
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl BrokerValue {
    /// An inline client: the FoguMQ protocol over an in-memory pipe.
    fn client(&self, args: &[Value]) -> Result<Value, String> {
        max_args(args, 1, "client")?;
        let mut token = self.core.token.clone();
        let mut request_timeout = Duration::from_secs(5);
        if let Some(Value::Map(m)) = opt_arg(args, 0) {
            let m = m.borrow();
            if let Some(t) = m.get("token") {
                token = t.to_display_string();
            }
            if let Some(t) = m.get("requestTimeout") {
                request_timeout = duration_arg(t, "client: requestTimeout")?;
            }
        }
        if self.core.closed.load(Ordering::SeqCst) {
            return Err(io_err!("broker: closed"));
        }
        let rt = crate::native::rt::tokio();
        let (broker_end, client_end) = tokio::io::duplex(1 << 20);
        let (brd, bwr) = tokio::io::split(broker_end);
        rt.spawn(serve_conn(Arc::clone(&self.core), brd, bwr, "inline".into()));
        let (mut crd, mut cwr) = tokio::io::split(client_end);
        let (to_client, from_broker) = std::sync::mpsc::channel::<Vec<u8>>();
        rt.spawn(async move {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match crd.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if to_client.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let (to_broker, mut from_client) = unbounded_channel::<Vec<u8>>();
        rt.spawn(async move {
            // an empty chunk closes the pipe
            while let Some(b) = from_client.recv().await {
                if b.is_empty() || cwr.write_all(&b).await.is_err() {
                    break;
                }
            }
            let _ = cwr.shutdown().await;
        });
        let closer = to_broker.clone();
        crate::native::fogumq::open(
            Box::new(PipeReader { rx: from_broker, buf: Vec::new(), pos: 0 }),
            Box::new(PipeWriter { tx: to_broker }),
            Arc::new(move || {
                let _ = closer.send(Vec::new());
            }),
            &token,
            format!("fogu://inline@{}", self.addr),
            Duration::from_secs(10),
            request_timeout,
        )
    }

    fn inspect(&self) -> Result<Value, String> {
        let (reply, rx) = std::sync::mpsc::channel();
        self.core.tx.send(Ev::Inspect { reply }).map_err(|_| io_err!("broker: closed"))?;
        let j = crate::native::rt::offload(move || rx.recv_timeout(Duration::from_secs(5)))?.map_err(|_| io_err!("broker: no answer"))?;
        let mut v = json_to_value(&j);
        if let Value::Map(m) = &mut v {
            m.borrow_mut().insert("address".into(), Value::Str(self.addr.to_string().into()));
        }
        Ok(v)
    }

    /// `keys({ prefix, limit = 200 })` → `[{ key, value, ttl }]` (ttl in seconds, nil = none), sorted.
    fn keys(&self, args: &[Value]) -> Result<Value, String> {
        max_args(args, 1, "keys")?;
        let (prefix, limit) = match opt_arg(args, 0) {
            Some(Value::Map(m)) => {
                let m = m.borrow();
                let p = m.get("prefix").filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string()).unwrap_or_default();
                let l = match m.get("limit") {
                    Some(Value::Int(n)) if *n > 0 => *n as usize,
                    _ => 200,
                };
                (p, l)
            }
            Some(Value::Str(p)) => (p.to_string(), 200),
            _ => (String::new(), 200),
        };
        let now = now_secs();
        let kv = self.core.store.kv.lock().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<(&String, &(Json, Option<u64>))> =
            kv.iter().filter(|(k, (_, exp))| k.starts_with(&prefix) && exp.is_none_or(|e| e > now)).collect();
        items.sort_by(|a, b| a.0.cmp(b.0));
        let out: Vec<Value> = items
            .into_iter()
            .take(limit)
            .map(|(k, (v, exp))| {
                crate::native::util::map(vec![
                    ("key", Value::Str(k.clone().into())),
                    ("value", json_to_value(v)),
                    ("ttl", exp.map_or(Value::Nil, |e| Value::Int(e.saturating_sub(now) as i64))),
                ])
            })
            .collect();
        Ok(crate::native::util::list(out))
    }

    fn stats(&self) -> Result<Value, String> {
        let (reply, rx) = std::sync::mpsc::channel();
        self.core.tx.send(Ev::Stats { reply }).map_err(|_| io_err!("broker: closed"))?;
        let j = crate::native::rt::offload(move || rx.recv_timeout(Duration::from_secs(5)))?.map_err(|_| io_err!("broker: no answer"))?;
        Ok(json_to_value(&j))
    }
}

impl FoguObject for BrokerValue {
    fn name(&self) -> &str {
        "Broker"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "address" => Value::Str(self.addr.to_string().into()),
            "port" => Value::Int(self.addr.port() as i64),
            "node" => Value::Str(self.core.node.clone().into()),
            "token" => Value::Str(self.core.token.clone().into()),
            "url" => Value::Str(format!("fogu://{}@{}", self.core.token, self.addr).into()),
            "closed" => Value::Bool(self.core.closed.load(Ordering::SeqCst)),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "client" => self.client(args),
            "stats" => {
                max_args(args, 0, "stats")?;
                self.stats()
            }
            "inspect" => {
                max_args(args, 0, "inspect")?;
                self.inspect()
            }
            "keys" => self.keys(args),
            "close" => {
                max_args(args, 0, "close")?;
                self.close();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Broker (client stats inspect keys admin close)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["client", "stats", "inspect", "keys", "admin", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(matches_pattern("chat.*.test", "chat.room1.test"));
        assert!(!matches_pattern("chat.*.test", "chat.a.b.test"));
        assert!(matches_pattern("chat.**", "chat.a.b.c"));
        assert!(matches_pattern("*", "anything"));
        assert!(!matches_pattern("chat.*", "news.x"));
    }

    #[test]
    fn redb_files_are_converted() {
        let dir = std::env::temp_dir().join(format!("fogu-broker-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mq.db");
        {
            const KV: TableDefinition<&str, &str> = TableDefinition::new("kv");
            const KEYS: TableDefinition<&str, &str> = TableDefinition::new("apikeys");
            let db = redb::Database::create(&path).unwrap();
            let w = db.begin_write().unwrap();
            w.open_table(KV).unwrap().insert("visits", r#"{"data":7,"expires_at":null}"#).unwrap();
            w.open_table(KEYS).unwrap().insert("k1", r#"{"id":"k1","name":"app","token":"t","perms":{}}"#).unwrap();
            w.commit().unwrap();
        }
        let p = path.to_str().unwrap();
        let s = Store::open(Some(p)).unwrap();
        assert_eq!(s.get("visits"), Some(json!(7)));
        assert!(s.key_by_token("t").is_some());
        s.set("more", json!("x"), None);
        s.close_db();
        assert!(path.is_dir() && dir.join("mq.db.redb.bak").is_file());
        // reopened from SurrealKV
        let s = Store::open(Some(p)).unwrap();
        assert_eq!(s.get("more"), Some(json!("x")));
        assert_eq!(s.get("visits"), Some(json!(7)));
        s.close_db();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frames() {
        let f = Frame::new(OP_SET, 42, b"{}".to_vec());
        let b = f.encode();
        assert_eq!(&b[..2], b"FO");
        assert_eq!(b.len(), HEADER + 2);
    }
}
