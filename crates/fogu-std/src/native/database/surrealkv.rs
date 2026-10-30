//! `fogu:surrealkv` — SurrealKV, an embedded, versioned LSM key-value store
//! with MVCC transactions (snapshot isolation) and encryption at rest.
//!
//! ```fogu
//! import surrealkv from "fogu:surrealkv"
//! const db = surrealkv.open("data/app", { versions: true })?
//! db.set("user:1", { name: "Ana" })?
//! print(db.get("user:1"))                       // {name: Ana}
//! print(db.scan({ prefix: "user:" }))           // [{key: user:1, value: {name: Ana}}]
//! print(db.history("user:1"))                   // [{value: …, at: <ms>, deleted: false}]
//! const tx = db.begin()
//! tx.set("a", 1)
//! tx.commit()?                                  // error on a write conflict
//! db.close()
//! ```
//!
//! Keys are strings; values are any Fogu value, stored as JSON. Timestamps are
//! milliseconds since the epoch (like `time.now()`); SurrealKV keeps nanoseconds.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::sync::{Arc, Mutex};

use surrealkv::{Durability, HistoryOptions, LSMIterator, SoftwareKeyManager, Transaction, Tree, TreeBuilder};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, list, map, max_args, nfn_var, str_arg, str_list};

pub fn build() -> ModuleValue {
    let mut e = HashMap::new();
    e.insert("open".to_string(), nfn_var("open", 1, open));
    ModuleValue {
        name: "fogu:surrealkv".into(),
        exports: RefCell::new(e),
    }
}

fn opts(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn flag(o: &HashMap<String, Value>, k: &str) -> bool {
    o.get(k).is_some_and(Value::is_truthy)
}

fn skv_err(what: &str, e: surrealkv::Error) -> String {
    match e {
        surrealkv::Error::TransactionWriteConflict => {
            io_err!("{}: write conflict (another transaction changed the same keys); retry it", what)
        }
        e => io_err!("{}: {}", what, e),
    }
}

/// `open(path, { versions, retention, sync, key, keyFile, vlog })`
fn open(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "surrealkv.open")?;
    let path = str_arg(args, 0, "surrealkv.open")?;
    let o = opts(args, 1, "surrealkv.open")?;
    let versions = flag(&o, "versions");
    let retention = match o.get("retention") {
        None | Some(Value::Nil) => 0,
        Some(v) => duration_arg(v, "surrealkv.open retention")?.as_nanos() as u64,
    };
    let mut b = TreeBuilder::new().with_path(path.clone().into()).with_versioning(versions, retention);
    if flag(&o, "vlog") {
        b = b.with_enable_vlog(true);
    }
    let keys = match (o.get("key"), o.get("keyFile")) {
        (Some(k), _) if !matches!(k, Value::Nil) => {
            Some(SoftwareKeyManager::from_hex(k.to_display_string().trim()).map_err(|e| format!("surrealkv.open: key: {}", e))?)
        }
        (_, Some(f)) if !matches!(f, Value::Nil) => {
            let f = f.to_display_string();
            Some(SoftwareKeyManager::from_key_file(&f).map_err(|e| io_err!("surrealkv.open: keyFile {}: {}", f, e))?)
        }
        _ => None,
    };
    if let Some(k) = keys {
        b = b.with_encryption(Arc::new(k), surrealkv::CipherPreference::Auto);
    }
    // the background flush/compaction tasks run on the shared runtime
    let tree = {
        let _rt = crate::native::rt::tokio().enter();
        b.build().map_err(|e| io_err!("surrealkv.open: {}: {}", path, e))?
    };
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    OPEN.lock().unwrap_or_else(|e| e.into_inner()).push((id, tree.clone()));
    crate::native::signal::arm_cleanup();
    Ok(NativeObjectRef::new(Store { tree: Some(tree), id, path, versions, sync: flag(&o, "sync") }).to_value())
}

/// The open stores, closed (flushed) when the program exits.
static OPEN: Mutex<Vec<(u64, Tree)>> = Mutex::new(Vec::new());
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn unregister(id: u64) {
    OPEN.lock().unwrap_or_else(|e| e.into_inner()).retain(|(i, _)| *i != id);
}

/// Closes the stores the program left open (called by `native::cleanup`).
pub fn close_all() {
    let open: Vec<Tree> = OPEN.lock().unwrap_or_else(|e| e.into_inner()).drain(..).map(|(_, t)| t).collect();
    for t in open {
        let _ = crate::native::rt::tokio().block_on(t.close());
    }
}

// ---------- values, keys, timestamps ----------

fn encode(v: &Value) -> String {
    stringify_value(v)
}

fn decode(b: &[u8]) -> Value {
    let s = String::from_utf8_lossy(b);
    parse_value(&s).unwrap_or_else(|_| Value::Str(s.into_owned().into()))
}

fn key_arg(args: &[Value], i: usize, what: &str) -> Result<String, String> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(Value::Int(n)) => Ok(n.to_string()),
        Some(other) => Err(format!("{}: the key must be a str, got '{}'", what, other.type_name())),
        None => Err(format!("{}: missing key", what)),
    }
}

/// A time as ms since the epoch: an int, or a `time.date()` object.
fn ms_arg(v: &Value, what: &str) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        Value::Object(o) => match o.get("ms") {
            Some(Value::Int(n)) => Ok(n),
            _ => Err(format!("{}: expected a time in ms or a Date, got '{}'", what, o.name())),
        },
        other => Err(format!("{}: expected a time in ms or a Date, got '{}'", what, other.type_name())),
    }
}

/// The first key after every key that starts with `prefix`.
fn prefix_end(prefix: &str) -> Option<Vec<u8>> {
    let mut b = prefix.as_bytes().to_vec();
    while let Some(last) = b.pop() {
        if last < 0xff {
            b.push(last + 1);
            return Some(b);
        }
    }
    None
}

// ---------- reads shared by the store and transactions ----------

struct Scan {
    start: Option<Vec<u8>>,
    end: Option<Vec<u8>>,
    limit: Option<usize>,
    reverse: bool,
    keys_only: bool,
}

fn scan_opts(o: &HashMap<String, Value>) -> Result<Scan, String> {
    let s = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
    let (mut start, mut end) = (None, None);
    if let Some(p) = s("prefix") {
        start = Some(p.as_bytes().to_vec());
        end = prefix_end(&p);
    }
    if let Some(v) = s("start") {
        start = Some(v.into_bytes());
    }
    if let Some(v) = s("end") {
        end = Some(v.into_bytes());
    }
    let limit = match o.get("limit") {
        None | Some(Value::Nil) => None,
        Some(Value::Int(n)) if *n >= 0 => Some(*n as usize),
        Some(other) => return Err(format!("scan: limit must be an int >= 0, got '{}'", other.to_display_string())),
    };
    Ok(Scan { start, end, limit, reverse: flag(o, "reverse"), keys_only: flag(o, "keys") })
}

fn scan(tx: &Transaction, s: &Scan, what: &str) -> Result<Vec<(String, Option<Value>)>, String> {
    let mut out = Vec::new();
    if s.limit == Some(0) {
        return Ok(out);
    }
    let start = s.start.clone().unwrap_or_default();
    let end = s.end.clone().unwrap_or_else(|| vec![0xff; 256]);
    let mut it = tx.range(start, end).map_err(|e| skv_err(what, e))?;
    if s.reverse { it.seek_last() } else { it.seek_first() }.map_err(|e| skv_err(what, e))?;
    while it.valid() {
        let k = String::from_utf8_lossy(it.key().user_key()).into_owned();
        let v = if s.keys_only { None } else { Some(decode(&it.value().map_err(|e| skv_err(what, e))?)) };
        out.push((k, v));
        if s.limit.is_some_and(|l| out.len() >= l) {
            break;
        }
        if s.reverse { it.prev() } else { it.next() }.map_err(|e| skv_err(what, e))?;
    }
    Ok(out)
}

fn rows(items: Vec<(String, Option<Value>)>) -> Value {
    list(
        items
            .into_iter()
            .map(|(k, v)| match v {
                Some(v) => map(vec![("key", Value::Str(k.into())), ("value", v)]),
                None => Value::Str(k.into()),
            })
            .collect(),
    )
}

/// The read methods of a transaction (also used, on a fresh one, by the store).
fn read(tx: &Transaction, method: &str, args: &[Value], versions: bool) -> Option<Result<Value, String>> {
    let what = format!("surrealkv.{}", method);
    let r = (|| match method {
        "get" => {
            max_args(args, 1, &what)?;
            let k = key_arg(args, 0, &what)?;
            Ok(tx.get(k.as_bytes()).map_err(|e| skv_err(&what, e))?.map_or(Value::Nil, |v| decode(&v)))
        }
        "has" => {
            max_args(args, 1, &what)?;
            let k = key_arg(args, 0, &what)?;
            Ok(Value::Bool(tx.get(k.as_bytes()).map_err(|e| skv_err(&what, e))?.is_some()))
        }
        // scan({ prefix, start, end, limit, reverse, keys }) → [{ key, value }]
        "scan" => {
            max_args(args, 1, &what)?;
            let s = scan_opts(&opts(args, 0, &what)?)?;
            Ok(rows(scan(tx, &s, &what)?))
        }
        "keys" | "count" => {
            max_args(args, 1, &what)?;
            let mut o = HashMap::new();
            if let Some(p) = args.first().filter(|v| !matches!(v, Value::Nil)) {
                o.insert("prefix".to_string(), p.clone());
            }
            let mut s = scan_opts(&o)?;
            s.keys_only = true;
            let found = scan(tx, &s, &what)?;
            Ok(if method == "count" { Value::Int(found.len() as i64) } else { str_list(found.into_iter().map(|(k, _)| k)) })
        }
        // getAt(key, at) — the value as of a time (needs { versions: true })
        "getAt" => {
            max_args(args, 2, &what)?;
            let k = key_arg(args, 0, &what)?;
            let at = ms_arg(args.get(1).unwrap_or(&Value::Nil), &what)?;
            if !versions {
                return Err(format!("{}: the store was opened without {{ versions: true }}", what));
            }
            let ns = (at.max(0) as u64).saturating_mul(1_000_000).saturating_add(999_999);
            Ok(tx.get_at(k.as_bytes(), ns).map_err(|e| skv_err(&what, e))?.map_or(Value::Nil, |v| decode(&v)))
        }
        // history(key, { limit, deleted }) → [{ value, at, deleted }], newest first
        "history" => {
            max_args(args, 2, &what)?;
            let k = key_arg(args, 0, &what)?;
            let o = opts(args, 1, &what)?;
            if !versions {
                return Err(format!("{}: the store was opened without {{ versions: true }}", what));
            }
            let mut ho = HistoryOptions::new().with_tombstones(flag(&o, "deleted"));
            let limit = match o.get("limit") {
                Some(Value::Int(n)) if *n > 0 => Some(*n as usize),
                _ => None,
            };
            if let Some(l) = limit {
                ho = ho.with_limit(l);
            }
            let mut end = k.as_bytes().to_vec();
            end.push(0);
            let mut it = tx.history_with_options(k.as_bytes(), end.as_slice(), &ho).map_err(|e| skv_err(&what, e))?;
            it.seek_first().map_err(|e| skv_err(&what, e))?;
            let mut out = Vec::new();
            while it.valid() && limit.is_none_or(|l| out.len() < l) {
                let key = it.key();
                let deleted = key.is_tombstone();
                let at = (key.timestamp() / 1_000_000) as i64;
                let value = if deleted { Value::Nil } else { decode(&it.value().map_err(|e| skv_err(&what, e))?) };
                out.push(map(vec![("value", value), ("at", Value::Int(at)), ("deleted", Value::Bool(deleted))]));
                it.next().map_err(|e| skv_err(&what, e))?;
            }
            Ok(list(out))
        }
        _ => Err(String::new()),
    })();
    match r {
        Err(e) if e.is_empty() => None,
        r => Some(r),
    }
}

/// The write methods of a transaction.
fn write(tx: &mut Transaction, method: &str, args: &[Value], versions: bool) -> Option<Result<(), String>> {
    let what = format!("surrealkv.{}", method);
    let r = (|| match method {
        // set(key, value, { at }) — `at` writes a version for that time (versioned stores)
        "set" => {
            max_args(args, 3, &what)?;
            let k = key_arg(args, 0, &what)?;
            let v = encode(args.get(1).ok_or_else(|| format!("{}: missing value", what))?);
            let o = opts(args, 2, &what)?;
            match o.get("at").filter(|v| !matches!(v, Value::Nil)) {
                Some(at) => {
                    if !versions {
                        return Err(format!("{}: {{ at }} needs a store opened with {{ versions: true }}", what));
                    }
                    let ns = (ms_arg(at, &what)?.max(0) as u64).saturating_mul(1_000_000);
                    tx.set_at(k.as_bytes(), v.as_bytes(), ns).map_err(|e| skv_err(&what, e))
                }
                None => tx.set(k.as_bytes(), v.as_bytes()).map_err(|e| skv_err(&what, e)),
            }
        }
        // delete keeps the history of a versioned store (a tombstone); purge erases every version
        "delete" | "purge" => {
            max_args(args, 1, &what)?;
            let k = key_arg(args, 0, &what)?;
            if versions && method == "delete" {
                tx.soft_delete(k.as_bytes()).map_err(|e| skv_err(&what, e))
            } else {
                tx.delete(k.as_bytes()).map_err(|e| skv_err(&what, e))
            }
        }
        // deleteRange(start, end) — keys in [start, end)
        "deleteRange" => {
            max_args(args, 2, &what)?;
            let (a, b) = (key_arg(args, 0, &what)?, key_arg(args, 1, &what)?);
            tx.delete_range(a.as_bytes(), b.as_bytes()).map_err(|e| skv_err(&what, e))
        }
        "deletePrefix" => {
            max_args(args, 1, &what)?;
            let p = key_arg(args, 0, &what)?;
            let end = prefix_end(&p).unwrap_or_else(|| vec![0xff; 256]);
            tx.delete_range(p.as_bytes(), end.as_slice()).map_err(|e| skv_err(&what, e))
        }
        _ => Err(String::new()),
    })();
    match r {
        Err(e) if e.is_empty() => None,
        r => Some(r),
    }
}

/// Commits on the shared runtime, waited for cooperatively (never inside
/// `call`: other tasks may use the same store meanwhile).
fn commit(mut tx: Transaction, sync: bool, what: &str) -> Poller {
    if sync {
        tx.set_durability(Durability::Immediate);
    }
    let pending = crate::native::rt::spawn(async move { tx.commit().await });
    let what = what.to_string();
    Box::new(move || Some(pending.try_take()?.map(|_| Value::Nil).map_err(|e| skv_err(&what, e))))
}

fn ready(r: Result<Value, String>) -> Poller {
    let mut r = Some(r);
    Box::new(move || r.take())
}

// ---------- Store ----------

struct Store {
    tree: Option<Tree>,
    id: u64,
    path: String,
    versions: bool,
    sync: bool,
}

impl Store {
    fn tree(&self) -> Result<&Tree, String> {
        self.tree.as_ref().ok_or_else(|| io_err!("surrealkv: the store is closed"))
    }

    fn begin(&self) -> Result<Transaction, String> {
        self.tree()?.begin().map_err(|e| skv_err("surrealkv.begin", e))
    }
}

impl FoguObject for Store {
    fn name(&self) -> &str {
        "SurrealKV"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "path" => Some(Value::Str(self.path.clone().into())),
            "versions" => Some(Value::Bool(self.versions)),
            "closed" => Some(Value::Bool(self.tree.is_none())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let what = format!("surrealkv.{}", method);
        match method {
            // begin({ readOnly }) → Txn
            "begin" => {
                max_args(args, 1, &what)?;
                let o = opts(args, 0, &what)?;
                let tx = if flag(&o, "readOnly") {
                    self.tree()?.begin_with_mode(surrealkv::Mode::ReadOnly).map_err(|e| skv_err(&what, e))?
                } else {
                    self.begin()?
                };
                return Ok(NativeObjectRef::new(Txn { tx: Some(tx), versions: self.versions, sync: self.sync }).to_value());
            }
            // checkpoint(dir) — a consistent copy of the database · restore(dir)
            "checkpoint" | "restore" => {
                max_args(args, 1, &what)?;
                let dir = str_arg(args, 0, &what)?;
                let t = self.tree()?;
                let meta = if method == "checkpoint" { t.create_checkpoint(&dir) } else { t.restore_from_checkpoint(&dir) };
                meta.map_err(|e| io_err!("{}: {}: {}", what, dir, e))?;
                return Ok(Value::Str(dir.into()));
            }
            "flush" => {
                max_args(args, 0, &what)?;
                self.tree()?.flush_wal(true).map_err(|e| skv_err(&what, e))?;
                return Ok(Value::Nil);
            }
            _ => {}
        }
        if self.methods().contains(&method) {
            let tx = self.begin()?;
            if let Some(r) = read(&tx, method, args, self.versions) {
                return r;
            }
        }
        Err(format!("method '{}' does not exist on SurrealKV ({})", method, self.methods().join(" ")))
    }

    /// The methods that commit (or close) wait without holding the object.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let what = format!("surrealkv.{}", method);
        let r = (|| match method {
            "close" => {
                let Some(t) = self.tree.take() else { return Ok(Some(ready(Ok(Value::Nil)))) };
                unregister(self.id);
                let pending = crate::native::rt::spawn(async move { t.close().await });
                Ok(Some(Box::new(move || Some(pending.try_take()?.map(|_| Value::Nil).map_err(|e| skv_err(&what, e)))) as Poller))
            }
            // batch({ set: { k: v }, delete: [keys] }) — one atomic commit
            "batch" => {
                max_args(args, 1, &what)?;
                let o = opts(args, 0, &what)?;
                let mut tx = self.begin()?;
                if let Some(Value::Map(m)) = o.get("set") {
                    for (k, v) in m.borrow().iter() {
                        tx.set(k.as_bytes(), encode(v).as_bytes()).map_err(|e| skv_err(&what, e))?;
                    }
                }
                match o.get("delete") {
                    None | Some(Value::Nil) => {}
                    Some(Value::List(l)) => {
                        for k in l.borrow().iter() {
                            let k = k.to_display_string();
                            if self.versions { tx.soft_delete(k.as_bytes()) } else { tx.delete(k.as_bytes()) }.map_err(|e| skv_err(&what, e))?;
                        }
                    }
                    Some(other) => return Err(format!("{}: delete must be a list of keys, got '{}'", what, other.type_name())),
                }
                Ok(Some(commit(tx, self.sync, &what)))
            }
            _ => {
                if !matches!(method, "set" | "delete" | "purge" | "deleteRange" | "deletePrefix") {
                    return Ok(None);
                }
                let mut tx = self.begin()?;
                match write(&mut tx, method, args, self.versions) {
                    Some(r) => r.map(|_| Some(commit(tx, self.sync, &what))),
                    None => Ok(None),
                }
            }
        })();
        r.transpose()
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "get", "set", "has", "delete", "purge", "deleteRange", "deletePrefix", "scan", "keys", "count", "getAt", "history", "batch",
            "begin", "checkpoint", "restore", "flush", "close",
        ]
    }
}

// ---------- Txn ----------

struct Txn {
    tx: Option<Transaction>,
    versions: bool,
    sync: bool,
}

impl FoguObject for Txn {
    fn name(&self) -> &str {
        "SurrealKVTxn"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "open" => Some(Value::Bool(self.tx.is_some())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let what = format!("surrealkv.tx.{}", method);
        match method {
            "rollback" => {
                if let Some(mut tx) = self.tx.take() {
                    tx.rollback();
                }
                return Ok(Value::Nil);
            }
            _ => {}
        }
        if !self.methods().contains(&method) {
            return Err(format!("method '{}' does not exist on SurrealKVTxn ({})", method, self.methods().join(" ")));
        }
        let versions = self.versions;
        let tx = self.tx.as_mut().ok_or_else(|| io_err!("{}: the transaction is already finished", what))?;
        if let Some(r) = read(tx, method, args, versions) {
            return r;
        }
        match write(tx, method, args, versions) {
            Some(r) => r.map(|_| Value::Nil),
            None => Err(format!("method '{}' does not exist on SurrealKVTxn", method)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "commit" {
            return None;
        }
        let what = "surrealkv.tx.commit";
        Some((|| {
            max_args(args, 0, what)?;
            let tx = self.tx.take().ok_or_else(|| io_err!("{}: the transaction is already finished", what))?;
            Ok(commit(tx, self.sync, what))
        })())
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["get", "set", "has", "delete", "purge", "deleteRange", "deletePrefix", "scan", "keys", "count", "getAt", "history", "commit", "rollback"]
    }
}
