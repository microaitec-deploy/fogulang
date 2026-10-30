//! `fogu:sql` module — multi-database SQL with SQLx (`Any` driver).
//!
//! Supported URLs:
//!   - "sqlite://file.db"                       → SQLite (created if missing)
//!   - "sqlite::memory:"                        → in-memory SQLite
//!   - "postgres://user:pass@host/db"           → PostgreSQL
//!   - "mysql://user:pass@host/db"              → MySQL / MariaDB
//!
//! Usage:
//! ```fogu
//! import sql from "fogu:sql"
//!
//! let db = sql.open("sqlite://demo.db")?
//! db.run("CREATE TABLE IF NOT EXISTS t (id INTEGER, name TEXT)")
//! db.run("INSERT INTO t (id, name) VALUES (?, ?)", [1, "Fogu"])
//! let rows = db.query("SELECT * FROM t")
//! for r in rows { print(r.id, r.name) }
//! db.close()
//! ```

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use sqlx::any::{AnyPoolOptions, AnyRow};
use sqlx::{AnyPool, Column, Row, TypeInfo, ValueRef};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;

// ---------- Module ----------

pub fn build() -> ModuleValue {
    sqlx::any::install_default_drivers();

    let mut exports = HashMap::new();

    exports.insert(
        "open".into(),
        nfn("open", 1, |args| {
            let url = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(format!(
                        "open(url) expects str, got '{}'",
                        other.type_name()
                    ))
                }
            };

            let (pool, url) = connect(&url).map_err(|e| io_err!("sql.open: {}", e))?;
            Ok(NativeObjectRef::new(SqlValue::new(pool, url)).to_value())
        }),
    );

    ModuleValue {
        name: "fogu:sql".into(),
        exports: RefCell::new(exports),
    }
}

/// Opens a single persistent connection (see below) on the shared runtime.
/// Returns the pool and the effective URL.
pub(crate) fn connect(url: &str) -> Result<(AnyPool, String), String> {
    sqlx::any::install_default_drivers();
    let mut url = url.to_string();
    // SQLite: append `?mode=rwc` (read-write-create) so that the file is
    // created if it does not exist (sqlite::memory: does not need it).
    if url.starts_with("sqlite:") && !url.contains("mode=") && !url.contains("memory") {
        url.push_str(if url.contains('?') { "&mode=rwc" } else { "?mode=rwc" });
    }
    // A single persistent connection: with several pooled connections
    // `begin()` and the following statements could run on different
    // connections (breaking transactions), and each `sqlite::memory:`
    // connection would see its own empty database.
    let u = url.clone();
    let pool = rt::block_on(async move {
        AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect(&u)
            .await
    })?
    .map_err(|e| format!("{} at '{}'", e, url))?;
    Ok((pool, url))
}

/// A parameter that can travel to the runtime's threads.
#[derive(Clone)]
enum Param {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

fn to_params(values: &[Value]) -> Vec<Param> {
    values
        .iter()
        .map(|v| match v {
            Value::Nil => Param::Null,
            Value::Bool(b) => Param::Bool(*b),
            Value::Int(n) => Param::Int(*n),
            Value::Float(f) => Param::Float(*f),
            Value::Str(s) => Param::Str(s.to_string()),
            other => Param::Str(other.to_display_string()),
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Op {
    /// rows affected and last insert id
    Execute,
    Query,
    One,
    /// several statements, no parameters
    Raw,
}

enum Out {
    Done(u64, Option<i64>),
    Rows(Vec<AnyRow>),
    Row(Option<AnyRow>),
}

async fn run_op(pool: AnyPool, op: Op, sql: String, params: Vec<Param>) -> Result<Out, sqlx::Error> {
    if let Op::Raw = op {
        let r = sqlx::raw_sql(&sql).execute(&pool).await?;
        return Ok(Out::Done(r.rows_affected(), r.last_insert_id()));
    }
    let mut q = sqlx::query(&sql);
    for p in &params {
        q = match p {
            Param::Null => q.bind(Option::<i64>::None),
            Param::Bool(b) => q.bind(*b),
            Param::Int(n) => q.bind(*n),
            Param::Float(f) => q.bind(*f),
            Param::Str(s) => q.bind(s.clone()),
        };
    }
    Ok(match op {
        Op::Query => Out::Rows(q.fetch_all(&pool).await?),
        Op::One => Out::Row(q.fetch_optional(&pool).await?),
        _ => {
            let r = q.execute(&pool).await?;
            Out::Done(r.rows_affected(), r.last_insert_id())
        }
    })
}

/// A connection used by other native modules (fogu:orm). Synchronous: the
/// ORM calls it from inside its own methods.
pub(crate) struct SqlConn {
    pool: Option<AnyPool>,
}

impl SqlConn {
    pub(crate) fn open(url: &str) -> Result<SqlConn, String> {
        let (pool, _) = connect(url)?;
        Ok(SqlConn { pool: Some(pool) })
    }

    fn run(&self, op: Op, sql: &str, params: &[Value]) -> Result<Out, String> {
        let pool = self.pool.clone().ok_or("the database connection is closed")?;
        rt::tokio().block_on(run_op(pool, op, sql.to_string(), to_params(params))).map_err(|e| e.to_string())
    }

    /// Runs a statement; returns (rows affected, last insert id).
    pub(crate) fn execute(&self, sql: &str, params: &[Value]) -> Result<(u64, Option<i64>), String> {
        match self.run(Op::Execute, sql, params)? {
            Out::Done(n, id) => Ok((n, id)),
            _ => Ok((0, None)),
        }
    }

    /// Runs a query; returns the rows as maps.
    pub(crate) fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Value>, String> {
        match self.run(Op::Query, sql, params)? {
            Out::Rows(rows) => rows.into_iter().map(row_to_map).collect(),
            _ => Ok(Vec::new()),
        }
    }

    pub(crate) fn close(&mut self) {
        if let Some(pool) = self.pool.take() {
            rt::tokio().block_on(pool.close());
        }
    }
}

// ---------- SQL object ----------

pub struct SqlValue {
    pool: Option<AnyPool>,
    url: String,
    /// Between `begin()` and `commit()`/`rollback()` statements run without
    /// yielding, so another task cannot slip into the transaction.
    in_tx: bool,
    /// Statements running on the runtime; `begin()` waits for them, so none
    /// of them ends up inside the transaction.
    in_flight: Arc<AtomicUsize>,
}

impl SqlValue {
    pub fn new(pool: AnyPool, url: String) -> Self {
        Self { pool: Some(pool), url, in_tx: false, in_flight: Arc::new(AtomicUsize::new(0)) }
    }

    fn pool(&self) -> Result<AnyPool, String> {
        self.pool.clone().ok_or_else(|| "sql: database is closed".to_string())
    }

    /// The statement a method runs: (op, sql, params, name for errors).
    fn statement(method: &str, args: &[Value]) -> Result<Option<(Op, String, Vec<Param>)>, String> {
        let (op, with_params) = match method {
            "run" | "execute" => (Op::Execute, true),
            "query" => (Op::Query, true),
            "one" => (Op::One, true),
            "exec" => (Op::Raw, false),
            _ => return Ok(None),
        };
        if with_params {
            if args.is_empty() {
                return Err(format!("{}(sql [, params]) expects 1 or 2 arguments", method));
            }
        } else if args.len() != 1 {
            return Err(format!("{}(sql) expects 1 argument", method));
        }
        let sql = str_arg(&args[0], method)?;
        let params = if with_params { to_params(&params_arg(args.get(1))?) } else { Vec::new() };
        Ok(Some((op, sql, params)))
    }
}

/// The result of a statement as a Fogu value.
fn finish(method: &str, r: Result<Out, sqlx::Error>) -> Result<Value, String> {
    let what = if method == "execute" { "run" } else { method };
    match r.map_err(|e| io_err!("sql.{}: {}", what, e))? {
        Out::Done(n, _) if method != "exec" => Ok(Value::Int(n as i64)),
        Out::Done(..) => Ok(Value::Nil),
        Out::Rows(rows) => {
            let rows = rows.into_iter().map(row_to_map).collect::<Result<Vec<_>, _>>()?;
            Ok(Value::List(Rc::new(RefCell::new(rows))))
        }
        Out::Row(row) => Ok(row.map(row_to_map).transpose()?.unwrap_or(Value::Nil)),
    }
}

impl FoguObject for SqlValue {
    fn name(&self) -> &str {
        "Sql"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "url" => Some(Value::Str(self.url.clone().into())),
            "driver" => Some(Value::Str(driver_name(&self.url).into())),
            _ => None,
        }
    }

    /// Statements run on the shared runtime while other `fg` tasks keep
    /// going — except inside a transaction, or with no other task to run.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if self.in_tx || !rt::concurrent() {
            return None;
        }
        let (op, sql, params) = match Self::statement(method, args) {
            Ok(Some(s)) => s,
            Ok(None) => return None,
            Err(e) => return Some(Err(e)),
        };
        let pool = match self.pool() {
            Ok(p) => p,
            Err(e) => return Some(Err(e)),
        };
        let in_flight = Arc::clone(&self.in_flight);
        in_flight.fetch_add(1, Ordering::SeqCst);
        let pending = rt::spawn(async move {
            let r = run_op(pool, op, sql, params).await;
            in_flight.fetch_sub(1, Ordering::SeqCst);
            r
        });
        let method = method.to_string();
        Some(Ok(Box::new(move || pending.try_take().map(|r| finish(&method, r)))))
    }

    /// Statements of other tasks finish before a transaction starts.
    fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        if method != "begin" {
            return None;
        }
        let in_flight = Arc::clone(&self.in_flight);
        Some(Box::new(move || in_flight.load(Ordering::SeqCst) == 0))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        if let Some((op, sql, params)) = Self::statement(method, args)? {
            let pool = self.pool()?;
            return finish(method, rt::tokio().block_on(run_op(pool, op, sql, params)));
        }
        match method {
            "begin" | "commit" | "rollback" => {
                let pool = self.pool()?;
                let sql = method.to_uppercase();
                rt::tokio()
                    .block_on(run_op(pool, Op::Execute, sql, Vec::new()))
                    .map_err(|e| io_err!("sql.{}: {}", method, e))?;
                self.in_tx = method == "begin";
                Ok(Value::Nil)
            }
            "close" => {
                if let Some(pool) = self.pool.take() {
                    rt::tokio().block_on(pool.close());
                }
                self.in_tx = false;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Sql", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "run", "execute", "query", "one", "exec",
            "begin", "commit", "rollback", "close",
        ]
    }
}

// ---------- Helpers ----------

/// The URL scheme: `sqlite://x.db` and `sqlite::memory:` → `sqlite`.
fn driver_name(url: &str) -> String {
    url.split(':').next().filter(|s| !s.is_empty()).unwrap_or("?").to_string()
}

fn str_arg(v: &Value, ctx: &str) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(format!(
            "{}: expected str, got '{}'",
            ctx,
            other.type_name()
        )),
    }
}

fn params_arg(v: Option<&Value>) -> Result<Vec<Value>, String> {
    let v = match v {
        Some(v) if !matches!(v, Value::Nil) => v,
        _ => return Ok(Vec::new()),
    };

    match v {
        Value::List(l) => Ok(l.borrow().clone()),
        other => Err(format!(
            "parameters must be a list, got '{}'",
            other.type_name()
        )),
    }
}

fn row_to_map(row: AnyRow) -> Result<Value, String> {
    let mut map = HashMap::new();
    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let v = value_ref_to_value(&row, i)?;
        map.insert(name, v);
    }
    Ok(Value::Map(Rc::new(RefCell::new(map))))
}

fn value_ref_to_value(row: &AnyRow, i: usize) -> Result<Value, String> {
    let vr = row
        .try_get_raw(i)
        .map_err(|e| io_err!("sql: column {}: {}", i, e))?;

    if vr.is_null() {
        return Ok(Value::Nil);
    }

    // `type_info()` returns a `Cow` owned by the temporary `vr`.
    // We bind it to an intermediate variable before calling `.name()`.
    let type_info = vr.type_info();
    let type_name = type_info.name();

    match type_name {
        "BOOL" | "BOOLEAN" => row
            .try_get::<bool, _>(i)
            .map(Value::Bool)
            .map_err(|e| io_err!("sql: bool: {}", e)),
        "INT2" | "SMALLINT" | "TINYINT" => row
            .try_get::<i16, _>(i)
            .map(|n| Value::Int(n as i64))
            .map_err(|e| io_err!("sql: int2: {}", e)),
        "INT4" | "INTEGER" | "INT" | "MEDIUMINT" => row
            .try_get::<i32, _>(i)
            .map(|n| Value::Int(n as i64))
            .map_err(|e| io_err!("sql: int4: {}", e)),
        "INT8" | "BIGINT" => row
            .try_get::<i64, _>(i)
            .map(Value::Int)
            .map_err(|e| io_err!("sql: int8: {}", e)),
        "FLOAT4" | "REAL" => row
            .try_get::<f32, _>(i)
            .map(|n| Value::Float(n as f64))
            .map_err(|e| io_err!("sql: float4: {}", e)),
        "FLOAT8" | "DOUBLE" | "DOUBLE PRECISION" => row
            .try_get::<f64, _>(i)
            .map(Value::Float)
            .map_err(|e| io_err!("sql: float8: {}", e)),
        "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "NAME" | "CLOB" => row
            .try_get::<String, _>(i)
            .map(Value::str)
            .map_err(|e| io_err!("sql: text: {}", e)),
        _ => row.try_get::<String, _>(i).map(Value::str).map_err(|e| {
            io_err!(
                "sql: unsupported type '{}' (column {}): {}",
                type_name, i, e
            )
        }),
    }
}