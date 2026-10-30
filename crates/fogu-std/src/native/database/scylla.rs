//! `fogu:scylla` module — ScyllaDB / Apache Cassandra client (CQL), built on
//! the official `scylla` driver.
//!
//! ```fogu
//! import scylla from "fogu:scylla"
//!
//! const db = scylla.connect({ hosts: ["127.0.0.1:9042"] })?
//! db.execute("CREATE KEYSPACE IF NOT EXISTS app WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}")?
//! db.use("app")?
//! db.execute("CREATE TABLE IF NOT EXISTS users (id int PRIMARY KEY, name text, tags set<text>)")?
//! db.execute("INSERT INTO users (id, name, tags) VALUES (?, ?, ?)", [1, "Ana", ["admin"]])?
//! for u in db.query("SELECT * FROM users WHERE id = ?", [1])? { print(u.name, u.tags) }
//! db.close()
//! ```
//!
//! Statements with parameters are prepared (and cached per query text); each
//! Fogu value is converted to the CQL type of its column (`int`, `bigint`,
//! `text`, `uuid`, `timestamp`, `list<…>`, `map<…>`, UDTs…). Network and
//! query failures are operational errors (`error` values).

use ahash::AHashMap as HashMap;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::errors::TranslationError;
use scylla::frame::response::result::{CollectionType, ColumnType, NativeType};
use scylla::policies::address_translator::{AddressTranslator, UntranslatedPeer};
use scylla::response::query_result::QueryResult;
use scylla::statement::prepared::PreparedStatement;
use scylla::value::{
    Counter, CqlDate, CqlDuration, CqlTime, CqlTimestamp, CqlTimeuuid, CqlValue, Row,
};
use tokio::runtime::Runtime;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{list, map, max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    ModuleValue {
        name: "fogu:scylla".into(),
        exports: std::cell::RefCell::new(e),
    }
}

/// `connect("host:9042")` or `connect({ hosts, keyspace, user, password })`.
fn connect(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "scylla.connect")?;
    let mut hosts: Vec<String> = Vec::new();
    let mut keyspace: Option<String> = None;
    let mut auth: Option<(String, String)> = None;
    let mut single_node: Option<bool> = None;
    let mut address_map: HashMap<SocketAddr, SocketAddr> = HashMap::new();
    match &a[0] {
        Value::Str(h) => hosts.push(h.to_string()),
        Value::Map(m) => {
            let m = m.borrow();
            match m.get("hosts") {
                Some(Value::List(l)) => {
                    for h in l.borrow().iter() {
                        hosts.push(h.to_display_string());
                    }
                }
                Some(Value::Str(h)) => hosts.push(h.to_string()),
                _ => {}
            }
            if let Some(Value::Str(h)) = m.get("host") {
                hosts.push(h.to_string());
            }
            if let Some(Value::Str(k)) = m.get("keyspace") {
                keyspace = Some(k.to_string());
            }
            if let (Some(Value::Str(u)), Some(Value::Str(p))) = (m.get("user"), m.get("password")) {
                auth = Some((u.to_string(), p.to_string()));
            }
            if let Some(Value::Bool(b)) = m.get("singleNode") {
                single_node = Some(*b);
            }
            if let Some(Value::Map(am)) = m.get("addressMap") {
                for (from, to) in am.borrow().iter() {
                    let parse = |s: &str| -> Result<SocketAddr, String> {
                        s.to_socket_addrs()
                            .ok()
                            .and_then(|mut a| a.next())
                            .ok_or_else(|| format!("scylla.connect: invalid address '{}' in addressMap", s))
                    };
                    address_map.insert(parse(from)?, parse(&to.to_display_string())?);
                }
            }
        }
        other => {
            return Err(format!(
                "scylla.connect expects \"host:port\" or {{hosts, keyspace, user, password}}, got '{}'",
                other.type_name()
            ))
        }
    }
    if hosts.is_empty() {
        return Err("scylla.connect: no hosts given".into());
    }
    let hosts: Vec<String> = hosts
        .into_iter()
        .map(|h| if h.contains(':') { h } else { format!("{}:9042", h) })
        .collect();

    let rt = crate::native::rt::tokio();
    // A single loopback host (e.g. Docker publishing 9042) cannot reach the
    // internal addresses the node advertises: route every node through it.
    let first: Option<SocketAddr> = hosts[0].to_socket_addrs().ok().and_then(|mut a| a.next());
    let single = single_node.unwrap_or(hosts.len() == 1 && first.is_some_and(|a| a.ip().is_loopback()));
    let mut b = SessionBuilder::new().known_nodes(&hosts);
    if single || !address_map.is_empty() {
        b = b.address_translator(Arc::new(Translator {
            map: address_map,
            fallback: if single { first } else { None },
        }));
    }
    if let Some((u, p)) = auth {
        b = b.user(u, p);
    }
    if let Some(k) = &keyspace {
        b = b.use_keyspace(k, false);
    }
    let session = rt
        .block_on(b.build())
        .map_err(|e| io_err!("scylla.connect: {}: {}", hosts.join(", "), e))?;
    Ok(NativeObjectRef::new(ScyllaValue {
        rt: Some(rt),
        session: Some(session),
        prepared: HashMap::new(),
        hosts,
    })
    .to_value())
}

/// `addressMap` entries, then (single-node mode) the given host.
struct Translator {
    map: HashMap<SocketAddr, SocketAddr>,
    fallback: Option<SocketAddr>,
}

#[async_trait::async_trait]
impl AddressTranslator for Translator {
    async fn translate_address(&self, peer: &UntranslatedPeer) -> Result<SocketAddr, TranslationError> {
        let addr = peer.untranslated_address();
        Ok(self.map.get(&addr).copied().or(self.fallback).unwrap_or(addr))
    }
}

pub struct ScyllaValue {
    /// the shared runtime (the session's background tasks live there)
    rt: Option<&'static Runtime>,
    session: Option<Session>,
    prepared: HashMap<String, PreparedStatement>,
    hosts: Vec<String>,
}

impl ScyllaValue {
    fn parts(&self) -> Result<(&Runtime, &Session), String> {
        match (&self.rt, &self.session) {
            (Some(rt), Some(s)) => Ok((*rt, s)),
            _ => Err("scylla: the session is closed".into()),
        }
    }

    /// Runs a statement; with parameters it is prepared (cached) and each
    /// value is converted to the CQL type of its column.
    fn run(&mut self, what: &str, args: &[Value]) -> Result<QueryResult, String> {
        if args.is_empty() || args.len() > 2 {
            return Err(format!("{}(cql [, params]) expects 1 or 2 arguments", what));
        }
        let cql = str_arg(args, 0, what)?;
        let params: Vec<Value> = match opt_arg(args, 1) {
            None => Vec::new(),
            Some(Value::List(l)) => l.borrow().clone(),
            Some(other) => return Err(format!("{}: params must be a list, got '{}'", what, other.type_name())),
        };
        if params.is_empty() {
            let (rt, s) = self.parts()?;
            return rt
                .block_on(s.query_unpaged(cql.as_str(), &[] as &[CqlValue]))
                .map_err(|e| io_err!("scylla.{}: {}", what, e));
        }
        if !self.prepared.contains_key(&cql) {
            let (rt, s) = self.parts()?;
            let p = rt
                .block_on(s.prepare(cql.as_str()))
                .map_err(|e| io_err!("scylla.{}: {}", what, e))?;
            self.prepared.insert(cql.clone(), p);
        }
        let (rt, s) = self.parts()?;
        let p = &self.prepared[&cql];
        let specs = p.get_variable_col_specs();
        if specs.len() != params.len() {
            return Err(format!(
                "scylla.{}: the statement has {} placeholders but {} values were given",
                what,
                specs.len(),
                params.len()
            ));
        }
        let mut values: Vec<Option<CqlValue>> = Vec::with_capacity(params.len());
        for (v, spec) in params.iter().zip(specs.iter()) {
            values.push(
                to_cql(v, spec.typ()).map_err(|e| format!("scylla.{}: column '{}': {}", what, spec.name(), e))?,
            );
        }
        rt.block_on(s.execute_unpaged(p, values))
            .map_err(|e| io_err!("scylla.{}: {}", what, e))
    }

    fn rows(res: QueryResult, what: &str) -> Result<Vec<Value>, String> {
        let rows = match res.into_rows_result() {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()), // INSERT/UPDATE/DDL: no rows
        };
        let names: Vec<String> = rows.column_specs().iter().map(|c| c.name().to_string()).collect();
        let mut out = Vec::new();
        for row in rows.rows::<Row>().map_err(|e| io_err!("scylla.{}: {}", what, e))? {
            let row = row.map_err(|e| io_err!("scylla.{}: {}", what, e))?;
            let pairs: Vec<(&str, Value)> = names
                .iter()
                .zip(row.columns)
                .map(|(n, v)| (n.as_str(), v.map(from_cql).unwrap_or(Value::Nil)))
                .collect();
            out.push(map(pairs));
        }
        Ok(out)
    }
}

impl FoguObject for ScyllaValue {
    fn name(&self) -> &str {
        "Scylla"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "hosts" => Some(list(self.hosts.iter().cloned().map(Value::str).collect())),
            "keyspace" => Some(
                self.session
                    .as_ref()
                    .and_then(|s| s.get_keyspace())
                    .map(|k| Value::Str(k.to_string().into()))
                    .unwrap_or(Value::Nil),
            ),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // statements without results (DDL, INSERT, UPDATE, DELETE)
            "execute" => self.run("execute", args).map(|_| Value::Nil),
            "query" => {
                let res = self.run("query", args)?;
                Ok(list(Self::rows(res, "query")?))
            }
            "one" => {
                let res = self.run("one", args)?;
                Ok(Self::rows(res, "one")?.into_iter().next().unwrap_or(Value::Nil))
            }
            "use" => {
                let ks = str_arg(args, 0, "use")?;
                let (rt, s) = self.parts()?;
                rt.block_on(s.use_keyspace(ks.clone(), false))
                    .map(|_| Value::Nil)
                    .map_err(|e| io_err!("scylla.use: {}: {}", ks, e))
            }
            "close" => {
                self.shutdown();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Scylla", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["execute", "query", "one", "use", "close"]
    }
}

impl ScyllaValue {
    /// Drops the session inside the runtime (its drop may spawn tasks).
    fn shutdown(&mut self) {
        self.prepared.clear();
        if let Some(rt) = self.rt.take() {
            let _g = rt.enter();
            self.session = None;
        }
    }
}

impl Drop for ScyllaValue {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------- Fogu → CQL ----------

fn int_of(v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("expected int, got '{}'", other.type_name())),
    }
}

fn narrow<T: TryFrom<i64>>(v: &Value, ty: &str) -> Result<T, String> {
    let n = int_of(v)?;
    T::try_from(n).map_err(|_| format!("{} does not fit in a CQL {}", n, ty))
}

fn str_of(v: &Value) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(format!("expected str, got '{}'", other.type_name())),
    }
}

fn items(v: &Value) -> Result<Vec<Value>, String> {
    match v {
        Value::List(l) => Ok(l.borrow().clone()),
        other => Err(format!("expected list, got '{}'", other.type_name())),
    }
}

fn to_cql(v: &Value, ty: &ColumnType) -> Result<Option<CqlValue>, String> {
    if matches!(v, Value::Nil) {
        return Ok(None);
    }
    let some = |c: CqlValue| Ok(Some(c));
    match ty {
        ColumnType::Native(n) => match n {
            NativeType::Int => some(CqlValue::Int(narrow(v, "int")?)),
            NativeType::BigInt => some(CqlValue::BigInt(int_of(v)?)),
            NativeType::SmallInt => some(CqlValue::SmallInt(narrow(v, "smallint")?)),
            NativeType::TinyInt => some(CqlValue::TinyInt(narrow(v, "tinyint")?)),
            NativeType::Counter => some(CqlValue::Counter(Counter(int_of(v)?))),
            NativeType::Double => match v {
                Value::Float(f) => some(CqlValue::Double(*f)),
                Value::Int(n) => some(CqlValue::Double(*n as f64)),
                other => Err(format!("expected a number, got '{}'", other.type_name())),
            },
            NativeType::Float => match v {
                Value::Float(f) => some(CqlValue::Float(*f as f32)),
                Value::Int(n) => some(CqlValue::Float(*n as f32)),
                other => Err(format!("expected a number, got '{}'", other.type_name())),
            },
            NativeType::Boolean => match v {
                Value::Bool(b) => some(CqlValue::Boolean(*b)),
                other => Err(format!("expected bool, got '{}'", other.type_name())),
            },
            NativeType::Text => some(CqlValue::Text(str_of(v)?)),
            NativeType::Ascii => some(CqlValue::Ascii(str_of(v)?)),
            NativeType::Uuid => {
                let s = str_of(v)?;
                let u = uuid::Uuid::parse_str(&s).map_err(|e| format!("invalid uuid '{}': {}", s, e))?;
                some(CqlValue::Uuid(u))
            }
            NativeType::Timeuuid => {
                let s = str_of(v)?;
                let u = uuid::Uuid::parse_str(&s).map_err(|e| format!("invalid timeuuid '{}': {}", s, e))?;
                some(CqlValue::Timeuuid(CqlTimeuuid::from(u)))
            }
            // milliseconds since the Unix epoch (time.now(), Date.getTime())
            NativeType::Timestamp => some(CqlValue::Timestamp(CqlTimestamp(int_of(v)?))),
            NativeType::Date => match v {
                Value::Str(s) => some(CqlValue::Date(CqlDate(parse_date(s)?))),
                other => Err(format!("expected a date \"YYYY-MM-DD\", got '{}'", other.type_name())),
            },
            NativeType::Time => match v {
                Value::Duration(ns) => some(CqlValue::Time(CqlTime(*ns))),
                Value::Int(ns) => some(CqlValue::Time(CqlTime(*ns))),
                other => Err(format!("expected a duration since midnight, got '{}'", other.type_name())),
            },
            NativeType::Duration => match v {
                Value::Duration(ns) => some(CqlValue::Duration(CqlDuration { months: 0, days: 0, nanoseconds: *ns })),
                other => Err(format!("expected a duration, got '{}'", other.type_name())),
            },
            NativeType::Inet => {
                let s = str_of(v)?;
                let ip: IpAddr = s.parse().map_err(|e| format!("invalid ip '{}': {}", s, e))?;
                some(CqlValue::Inet(ip))
            }
            NativeType::Blob => match v {
                Value::Str(s) => some(CqlValue::Blob(s.as_bytes().to_vec())),
                Value::List(l) => {
                    let mut bytes = Vec::new();
                    for b in l.borrow().iter() {
                        bytes.push(narrow::<u8>(b, "byte")?);
                    }
                    some(CqlValue::Blob(bytes))
                }
                other => Err(format!("expected str or list<int> for a blob, got '{}'", other.type_name())),
            },
            other => Err(format!("CQL type {:?} is not supported as a parameter yet", other)),
        },
        ColumnType::Collection { typ, .. } => match typ {
            CollectionType::List(t) => {
                let xs = items(v)?.iter().map(|x| to_cql(x, t).map(|c| c.unwrap_or(CqlValue::Empty))).collect::<Result<_, _>>()?;
                some(CqlValue::List(xs))
            }
            CollectionType::Set(t) => {
                let xs = items(v)?.iter().map(|x| to_cql(x, t).map(|c| c.unwrap_or(CqlValue::Empty))).collect::<Result<_, _>>()?;
                some(CqlValue::Set(xs))
            }
            CollectionType::Map(kt, vt) => match v {
                Value::Map(m) => {
                    let mut entries: Vec<(&String, &Value)> = Vec::new();
                    let m = m.borrow();
                    let mut keys: Vec<&String> = m.keys().collect();
                    keys.sort();
                    for k in keys {
                        entries.push((k, &m[k]));
                    }
                    let mut out = Vec::new();
                    for (k, val) in entries {
                        // map keys are strings in Fogu; numeric CQL keys are parsed
                        let key = match kt.as_ref() {
                            ColumnType::Native(NativeType::Int | NativeType::BigInt | NativeType::SmallInt | NativeType::TinyInt) => {
                                let n: i64 = k.parse().map_err(|_| format!("map key '{}' is not an integer", k))?;
                                to_cql(&Value::Int(n), kt)?
                            }
                            _ => to_cql(&Value::Str(k.clone().into()), kt)?,
                        };
                        out.push((key.unwrap_or(CqlValue::Empty), to_cql(val, vt)?.unwrap_or(CqlValue::Empty)));
                    }
                    some(CqlValue::Map(out))
                }
                other => Err(format!("expected map, got '{}'", other.type_name())),
            },
            other => Err(format!("CQL collection {:?} is not supported as a parameter yet", other)),
        },
        ColumnType::Vector { typ, .. } => {
            let xs = items(v)?.iter().map(|x| to_cql(x, typ).map(|c| c.unwrap_or(CqlValue::Empty))).collect::<Result<_, _>>()?;
            some(CqlValue::Vector(xs))
        }
        ColumnType::Tuple(types) => {
            let xs = items(v)?;
            if xs.len() != types.len() {
                return Err(format!("expected a tuple of {} values, got {}", types.len(), xs.len()));
            }
            let vals = xs.iter().zip(types).map(|(x, t)| to_cql(x, t)).collect::<Result<_, _>>()?;
            some(CqlValue::Tuple(vals))
        }
        ColumnType::UserDefinedType { definition, .. } => match v {
            Value::Map(m) => {
                let m = m.borrow();
                let mut fields = Vec::new();
                for (fname, ftype) in &definition.field_types {
                    let fv = m.get(fname.as_ref()).cloned().unwrap_or(Value::Nil);
                    fields.push((fname.to_string(), to_cql(&fv, ftype)?));
                }
                some(CqlValue::UserDefinedType {
                    keyspace: definition.keyspace.to_string(),
                    name: definition.name.to_string(),
                    fields,
                })
            }
            other => Err(format!("expected a map for the UDT {}, got '{}'", definition.name, other.type_name())),
        },
        other => Err(format!("CQL type {:?} is not supported as a parameter yet", other)),
    }
}

// ---------- CQL → Fogu ----------

fn from_cql(v: CqlValue) -> Value {
    match v {
        CqlValue::Int(n) => Value::Int(n as i64),
        CqlValue::BigInt(n) => Value::Int(n),
        CqlValue::SmallInt(n) => Value::Int(n as i64),
        CqlValue::TinyInt(n) => Value::Int(n as i64),
        CqlValue::Counter(c) => Value::Int(c.0),
        CqlValue::Float(f) => Value::Float(f as f64),
        CqlValue::Double(f) => Value::Float(f),
        CqlValue::Boolean(b) => Value::Bool(b),
        CqlValue::Text(s) | CqlValue::Ascii(s) => Value::Str(s.into()),
        CqlValue::Uuid(u) => Value::Str(u.to_string().into()),
        CqlValue::Timeuuid(u) => Value::Str(u.to_string().into()),
        CqlValue::Timestamp(t) => Value::Int(t.0),
        CqlValue::Date(d) => Value::Str(format_date(d.0).into()),
        CqlValue::Time(t) => Value::Duration(t.0),
        CqlValue::Duration(d) if d.months == 0 => {
            Value::Duration(d.days as i64 * 86_400_000_000_000 + d.nanoseconds)
        }
        CqlValue::Duration(d) => Value::Str(format!("{}mo{}d{}ns", d.months, d.days, d.nanoseconds).into()),
        CqlValue::Inet(ip) => Value::Str(ip.to_string().into()),
        CqlValue::Blob(b) => list(b.into_iter().map(|x| Value::Int(x as i64)).collect()),
        CqlValue::List(xs) | CqlValue::Set(xs) | CqlValue::Vector(xs) => {
            list(xs.into_iter().map(from_cql).collect())
        }
        CqlValue::Map(pairs) => {
            let m: HashMap<String, Value> = pairs
                .into_iter()
                .map(|(k, v)| (from_cql(k).to_display_string(), from_cql(v)))
                .collect();
            Value::Map(std::rc::Rc::new(std::cell::RefCell::new(m)))
        }
        CqlValue::Tuple(xs) => list(xs.into_iter().map(|x| x.map(from_cql).unwrap_or(Value::Nil)).collect()),
        CqlValue::UserDefinedType { fields, .. } => {
            let m: HashMap<String, Value> = fields
                .into_iter()
                .map(|(k, v)| (k, v.map(from_cql).unwrap_or(Value::Nil)))
                .collect();
            Value::Map(std::rc::Rc::new(std::cell::RefCell::new(m)))
        }
        CqlValue::Empty => Value::Nil,
        // decimal / varint: shown as text (no arbitrary-precision type in Fogu)
        other => Value::Str(format!("{:?}", other).into()),
    }
}

// ---------- dates (CQL: days since 1970-01-01 + 2^31) ----------

const DATE_OFFSET: i64 = 1 << 31;

fn format_date(d: u32) -> String {
    let z = d as i64 - DATE_OFFSET + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn parse_date(s: &str) -> Result<u32, String> {
    let bad = || format!("invalid date '{}' (expected YYYY-MM-DD)", s);
    let mut it = s.splitn(3, '-');
    let y: i64 = it.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?;
    let m: i64 = it.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?;
    let d: i64 = it.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u32::try_from(days + DATE_OFFSET).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        for s in ["1970-01-01", "2026-10-10", "2000-02-29", "1969-12-31"] {
            assert_eq!(format_date(parse_date(s).unwrap()), s);
        }
        assert_eq!(parse_date("1970-01-01").unwrap() as i64, DATE_OFFSET);
    }
}
