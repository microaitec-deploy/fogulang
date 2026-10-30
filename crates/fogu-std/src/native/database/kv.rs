//! `fogu:kv` module — embedded key-value store backed by `redb`.
//!
//! Usage:
//! ```fogu
//! import kv from "fogu:kv"
//!
//! let db = kv.open("data.redb")?
//! db.set("name", "Fogu")
//! print(db.get("name"))
//! db.delete("name")
//! db.close()
//! ```

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata,
    TableDefinition,
};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};

// Single table for all key-value pairs.
// Key: string; Value: string (prefixed with \u{0}<type>:).
const TABLE: TableDefinition<&str, &str> = TableDefinition::new("kv");

// ---------- Module ----------

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    exports.insert(
        "open".into(),
        nfn("open", 1, |args| {
            let path = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(format!(
                        "open(path) expects str, got '{}'",
                        other.type_name()
                    ))
                }
            };
            let db = Database::create(&path).map_err(|e| {
                io_err!("kv.open: error opening '{}': {}", path, e)
            })?;
            Ok(NativeObjectRef::new(KvValue::new(db, path.into_string())).to_value())
        }),
    );

    ModuleValue {
        name: "fogu:kv".into(),
        exports: RefCell::new(exports),
    }
}

// ---------- KV object ----------

pub struct KvValue {
    db: Option<Database>,
    path: String,
}

impl KvValue {
    pub fn new(db: Database, path: String) -> Self {
        Self {
            db: Some(db),
            path,
        }
    }

    fn encode(v: &Value) -> Result<String, String> {
        match v {
            Value::Nil => Ok("\u{0}nil".into()),
            Value::Bool(b) => Ok(format!("\u{0}bool:{}", b)),
            Value::Int(n) => Ok(format!("\u{0}int:{}", n)),
            Value::Float(f) => Ok(format!("\u{0}float:{}", f)),
            Value::Str(s) => Ok(format!("\u{0}str:{}", s)),
            Value::List(l) => {
                let mut parts = Vec::new();
                for item in l.borrow().iter() {
                    parts.push(json_value(item)?);
                }
                Ok(format!("\u{0}list:[{}]", parts.join(",")))
            }
            // maps (and any nesting) are stored as JSON
            Value::Map(_) => Ok(format!("\u{0}json:{}", crate::native::json::stringify_value(v))),
            other => Err(format!(
                "kv: unsupported type '{}'",
                other.type_name()
            )),
        }
    }

    fn decode(s: &str) -> Result<Value, String> {
        if let Some(rest) = s.strip_prefix("\u{0}") {
            if rest == "nil" {
                return Ok(Value::Nil);
            }
            if let Some(b) = rest.strip_prefix("bool:") {
                return Ok(Value::Bool(b == "true"));
            }
            if let Some(n) = rest.strip_prefix("int:") {
                return n
                    .parse::<i64>()
                    .map(Value::Int)
                    .map_err(|e| io_err!("kv: invalid int: {}", e));
            }
            if let Some(f) = rest.strip_prefix("float:") {
                return f
                    .parse::<f64>()
                    .map(Value::Float)
                    .map_err(|e| io_err!("kv: invalid float: {}", e));
            }
            if let Some(s) = rest.strip_prefix("str:") {
                return Ok(Value::Str(s.to_string().into()));
            }
            if let Some(l) = rest.strip_prefix("list:") {
                return json_parse_list(l);
            }
            if let Some(j) = rest.strip_prefix("json:") {
                return crate::native::json::parse_value(j);
            }
        }
        // Old or corrupted format — treat as a raw string.
        Ok(Value::Str(s.to_string().into()))
    }
}

impl FoguObject for KvValue {
    fn name(&self) -> &str {
        "Kv"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "path" => Some(Value::Str(self.path.clone().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "get" => {
                if args.len() != 1 {
                    return Err("get(key) expects 1 argument".into());
                }
                let key = str_key(&args[0], "get")?;
                let db = self.db_ref()?;

                let read_txn = db
                    .begin_read()
                    .map_err(|e| io_err!("kv.get: {}", e))?;
                let table = match read_txn.open_table(TABLE) {
                    Ok(t) => t,
                    Err(_) => return Ok(Value::Nil),
                };
                match table.get(key.as_str()) {
                    Ok(Some(v)) => KvValue::decode(v.value()),
                    Ok(None) => Ok(Value::Nil),
                    Err(e) => Err(format!("kv.get: {}", e)),
                }
            }

            "set" => {
                if args.len() != 2 {
                    return Err("set(key, value) expects 2 arguments".into());
                }
                let key = str_key(&args[0], "set")?;
                let encoded = KvValue::encode(&args[1])?;
                let db = self.db_ref()?;

                let write_txn = db
                    .begin_write()
                    .map_err(|e| io_err!("kv.set: {}", e))?;
                {
                    let mut table = write_txn
                        .open_table(TABLE)
                        .map_err(|e| io_err!("kv.set: {}", e))?;
                    table
                        .insert(key.as_str(), encoded.as_str())
                        .map_err(|e| io_err!("kv.set: {}", e))?;
                }
                write_txn
                    .commit()
                    .map_err(|e| io_err!("kv.set commit: {}", e))?;
                Ok(Value::Nil)
            }

            "delete" => {
                if args.len() != 1 {
                    return Err("delete(key) expects 1 argument".into());
                }
                let key = str_key(&args[0], "delete")?;
                let db = self.db_ref()?;

                let write_txn = db
                    .begin_write()
                    .map_err(|e| io_err!("kv.delete: {}", e))?;
                {
                    let mut table = write_txn
                        .open_table(TABLE)
                        .map_err(|e| io_err!("kv.delete: {}", e))?;
                    table
                        .remove(key.as_str())
                        .map_err(|e| io_err!("kv.delete: {}", e))?;
                }
                write_txn
                    .commit()
                    .map_err(|e| io_err!("kv.delete commit: {}", e))?;
                Ok(Value::Nil)
            }

            "has" => {
                if args.len() != 1 {
                    return Err("has(key) expects 1 argument".into());
                }
                let key = str_key(&args[0], "has")?;
                let db = self.db_ref()?;

                let read_txn = db
                    .begin_read()
                    .map_err(|e| io_err!("kv.has: {}", e))?;
                let table = match read_txn.open_table(TABLE) {
                    Ok(t) => t,
                    Err(_) => return Ok(Value::Bool(false)),
                };
                let exists = table
                    .get(key.as_str())
                    .map_err(|e| io_err!("kv.has: {}", e))?
                    .is_some();
                Ok(Value::Bool(exists))
            }

            "keys" => {
                let db = self.db_ref()?;
                let read_txn = db
                    .begin_read()
                    .map_err(|e| io_err!("kv.keys: {}", e))?;
                let table = match read_txn.open_table(TABLE) {
                    Ok(t) => t,
                    Err(_) => {
                        return Ok(Value::List(Rc::new(RefCell::new(Vec::new()))))
                    }
                };
                let mut keys = Vec::new();
                for entry in table.iter().map_err(|e| io_err!("kv.keys: {}", e))? {
                    let (k, _) = entry.map_err(|e| io_err!("kv.keys: {}", e))?;
                    keys.push(Value::Str(k.value().to_string().into()));
                }
                Ok(Value::List(Rc::new(RefCell::new(keys))))
            }

            "clear" => {
                let db = self.db_ref()?;
                let write_txn = db
                    .begin_write()
                    .map_err(|e| io_err!("kv.clear: {}", e))?;
                {
                    write_txn
                        .delete_table(TABLE)
                        .map_err(|e| io_err!("kv.clear: {}", e))?;
                    // Reopen the empty table to make sure it exists.
                    let _ = write_txn
                        .open_table(TABLE)
                        .map_err(|e| io_err!("kv.clear: {}", e))?;
                }
                write_txn
                    .commit()
                    .map_err(|e| io_err!("kv.clear commit: {}", e))?;
                Ok(Value::Nil)
            }

            "len" => {
                let db = self.db_ref()?;
                let read_txn = db
                    .begin_read()
                    .map_err(|e| io_err!("kv.len: {}", e))?;
                let table = match read_txn.open_table(TABLE) {
                    Ok(t) => t,
                    Err(_) => return Ok(Value::Int(0)),
                };
                let n = table
                    .len()
                    .map_err(|e| io_err!("kv.len: {}", e))?;
                Ok(Value::Int(n as i64))
            }

            "close" => {
                self.db = None;
                Ok(Value::Nil)
            }

            other => Err(format!("method '{}' does not exist on Kv", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["get", "set", "delete", "has", "keys", "clear", "len", "close"]
    }
}

impl KvValue {
    fn db_ref(&self) -> Result<&Database, String> {
        self.db
            .as_ref()
            .ok_or_else(|| "kv: database is closed".to_string())
    }
}

// ---------- Helpers ----------

fn str_key(v: &Value, ctx: &str) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(format!(
            "{}: key must be str, got '{}'",
            ctx,
            other.type_name()
        )),
    }
}

// ---------- Serializing lists as JSON ----------

fn json_value(v: &Value) -> Result<String, String> {
    match v {
        Value::Nil => Ok("null".into()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Int(n) => Ok(n.to_string()),
        Value::Float(f) => Ok(f.to_string()),
        Value::Str(s) => Ok(format!("{:?}", s)),
        Value::List(l) => {
            let mut parts = Vec::new();
            for item in l.borrow().iter() {
                parts.push(json_value(item)?);
            }
            Ok(format!("[{}]", parts.join(",")))
        }
        other => Err(format!(
            "kv: type '{}' not supported in lists",
            other.type_name()
        )),
    }
}

fn json_parse_list(s: &str) -> Result<Value, String> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Err("kv: malformed list".into());
    }
    let inner = &s[1..s.len() - 1];
    if inner.trim().is_empty() {
        return Ok(Value::List(Rc::new(RefCell::new(Vec::new()))));
    }
    let mut items = Vec::new();
    for part in split_json_list(inner) {
        let t = part.trim();
        if t == "null" {
            items.push(Value::Nil);
        } else if t == "true" {
            items.push(Value::Bool(true));
        } else if t == "false" {
            items.push(Value::Bool(false));
        } else if t.starts_with('"') && t.ends_with('"') {
            let unquoted = &t[1..t.len() - 1];
            items.push(Value::Str(
                unquoted.replace("\\\"", "\"").replace("\\\\", "\\").into(),
            ));
        } else if t.starts_with('[') {
            items.push(json_parse_list(t)?);
        } else if let Ok(i) = t.parse::<i64>() {
            items.push(Value::Int(i));
        } else if let Ok(f) = t.parse::<f64>() {
            items.push(Value::Float(f));
        } else {
            return Err(format!("kv: unknown JSON value: {}", t));
        }
    }
    Ok(Value::List(Rc::new(RefCell::new(items))))
}

fn split_json_list(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut in_str = false;
    let mut escape = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match c {
            '\\' if in_str => escape = true,
            '"' => in_str = !in_str,
            '[' if !in_str => depth += 1,
            ']' if !in_str => depth -= 1,
            ',' if !in_str && depth == 0 => {
                parts.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].to_string());
    parts
}