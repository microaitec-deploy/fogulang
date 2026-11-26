//! `fogu:sql` module — multi-database SQL with SQLx: the native SQLite
//! driver for `sqlite:` URLs (every column type, BLOBs), the `Any` driver
//! for PostgreSQL and MySQL.
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

use std::cell::{Cell, RefCell};
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use sqlx::any::{AnyPoolOptions, AnyRow};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{AnyPool, Column, Row, SqlitePool, TypeInfo, ValueRef};

use crate::runtime::sched;

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

/// A connection pool: SQLite through its own driver (the `Any` driver cannot
/// read BOOLEAN, DATE, DATETIME nor BLOB columns), the others through `Any`.
#[derive(Clone)]
pub(crate) enum Pool {
    Sqlite(SqlitePool),
    Any(AnyPool),
}

impl Pool {
    async fn close(&self) {
        match self {
            Pool::Sqlite(p) => p.close().await,
            Pool::Any(p) => p.close().await,
        }
    }
}

/// Opens a single persistent connection (see below) on the shared runtime.
/// Returns the pool and the effective URL.
pub(crate) fn connect(url: &str) -> Result<(Pool, String), String> {
    sqlx::any::install_default_drivers();
    let url = url.to_string();
    // A single persistent connection: with several pooled connections
    // `begin()` and the following statements could run on different
    // connections (breaking transactions), and each `sqlite::memory:`
    // connection would see its own empty database.
    if url.starts_with("sqlite:") {
        use std::str::FromStr;
        // the file is created if it does not exist
        let opts = SqliteConnectOptions::from_str(&url).map_err(|e| crate::native::util::redact_url(&format!("{} at '{}'", e, url)))?.create_if_missing(true);
        let pool = rt::block_on(async move {
            SqlitePoolOptions::new()
                .max_connections(1)
                .min_connections(1)
                .idle_timeout(None)
                .max_lifetime(None)
                .connect_with(opts)
                .await
        })?
        .map_err(|e| crate::native::util::redact_url(&format!("{} at '{}'", e, url)))?;
        return Ok((Pool::Sqlite(pool), url));
    }
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
    .map_err(|e| crate::native::util::redact_url(&format!("{} at '{}'", e, url)))?;
    Ok((Pool::Any(pool), url))
}

/// A parameter that can travel to the runtime's threads.
#[derive(Clone)]
enum Param {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// a `buffer`: stored as a BLOB, never as its printed form
    Bytes(Vec<u8>),
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
            Value::Buffer(_) => Param::Bytes(crate::native::util::bytes_of(v, "sql").unwrap_or_default()),
            // lists and maps as JSON text (not the way `print` shows them)
            Value::List(_) | Value::Map(_) => Param::Str(crate::native::json::stringify_value(v)),
            other => Param::Str(other.to_display_string()),
        })
        .collect()
}

/// A column value read on the runtime's threads (a row is `Send`, a `Value` is not).
enum Cell2 {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Blob(Vec<u8>),
}

type PlainRow = Vec<(String, Cell2)>;

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
    Rows(Vec<PlainRow>),
    Row(Option<PlainRow>),
}

/// Statements that change the schema: the cached prepared statements (and
/// the column types they remember) are dropped after them.
fn is_ddl(sql: &str) -> bool {
    sql.split(';').any(|st| {
        let w = st.split_whitespace().next().unwrap_or("");
        ["CREATE", "DROP", "ALTER"].iter().any(|k| w.eq_ignore_ascii_case(k))
    })
}

macro_rules! bind_all {
    ($q:expr, $params:expr) => {{
        let mut q = $q;
        for p in $params {
            q = match p {
                Param::Null => q.bind(Option::<i64>::None),
                Param::Bool(b) => q.bind(*b),
                Param::Int(n) => q.bind(*n),
                Param::Float(f) => q.bind(*f),
                Param::Str(s) => q.bind(s.clone()),
                Param::Bytes(b) => q.bind(b.clone()),
            };
        }
        q
    }};
}

async fn run_op(pool: Pool, op: Op, sql: String, params: Vec<Param>) -> Result<Out, sqlx::Error> {
    match pool {
        Pool::Sqlite(pool) => {
            let out = if let Op::Raw = op {
                let r = sqlx::raw_sql(&sql).execute(&pool).await?;
                Out::Done(r.rows_affected(), Some(r.last_insert_rowid()))
            } else {
                let q = bind_all!(sqlx::query(&sql), &params);
                match op {
                    Op::Query => Out::Rows(q.fetch_all(&pool).await?.iter().map(sqlite_row).collect()),
                    Op::One => Out::Row(q.fetch_optional(&pool).await?.as_ref().map(sqlite_row)),
                    _ => {
                        let r = q.execute(&pool).await?;
                        Out::Done(r.rows_affected(), Some(r.last_insert_rowid()))
                    }
                }
            };
            if matches!(op, Op::Raw | Op::Execute) && is_ddl(&sql) {
                if let Ok(mut c) = pool.acquire().await {
                    use sqlx::Connection;
                    let _ = c.clear_cached_statements().await;
                }
            }
            Ok(out)
        }
        Pool::Any(pool) => {
            if let Op::Raw = op {
                let r = sqlx::raw_sql(&sql).execute(&pool).await?;
                return Ok(Out::Done(r.rows_affected(), r.last_insert_id()));
            }
            let q = bind_all!(sqlx::query(&sql), &params);
            Ok(match op {
                Op::Query => Out::Rows(q.fetch_all(&pool).await?.iter().map(any_row).collect::<Result<_, _>>()?),
                Op::One => Out::Row(q.fetch_optional(&pool).await?.as_ref().map(any_row).transpose()?),
                _ => {
                    let r = q.execute(&pool).await?;
                    Out::Done(r.rows_affected(), r.last_insert_id())
                }
            })
        }
    }
}

/// A connection used by other native modules (fogu:orm). Synchronous: the
/// ORM calls it from inside its own methods.
pub(crate) struct SqlConn {
    pool: Option<Pool>,
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
            Out::Rows(rows) => Ok(rows.into_iter().map(row_to_map).collect()),
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
    pool: Option<Pool>,
    url: String,
    /// Between `begin()` and `commit()`/`rollback()` the owner's statements
    /// run without yielding.
    in_tx: bool,
    /// The task that opened the transaction (`sched::task_id`). A transaction
    /// belongs to it: the statements of other tasks wait (cooperatively) for
    /// its `commit()` / `rollback()` instead of running inside it.
    tx_owner: Rc<Cell<Option<usize>>>,
    /// Statements running on the runtime; `begin()` waits for them, so none
    /// of them ends up inside the transaction.
    in_flight: Arc<AtomicUsize>,
}

impl SqlValue {
    pub(crate) fn new(pool: Pool, url: String) -> Self {
        Self { pool: Some(pool), url, in_tx: false, tx_owner: Rc::new(Cell::new(None)), in_flight: Arc::new(AtomicUsize::new(0)) }
    }

    fn pool(&self) -> Result<Pool, String> {
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
            let rows = rows.into_iter().map(row_to_map).collect::<Vec<_>>();
            Ok(Value::List(Rc::new(RefCell::new(rows))))
        }
        Out::Row(row) => Ok(row.map(row_to_map).unwrap_or(Value::Nil)),
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

    /// Statements of other tasks finish before a transaction starts, and
    /// wait while another task's transaction is open.
    fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        if !matches!(method, "run" | "execute" | "query" | "one" | "exec" | "begin" | "commit" | "rollback") {
            return None;
        }
        let owner = Rc::clone(&self.tx_owner);
        let free = move || owner.get().is_none_or(|o| o == sched::task_id());
        if method != "begin" {
            // the common case (no transaction) costs one load
            if self.tx_owner.get().is_none() {
                return None;
            }
            return Some(Box::new(free));
        }
        let in_flight = Arc::clone(&self.in_flight);
        Some(Box::new(move || free() && in_flight.load(Ordering::SeqCst) == 0))
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
                self.tx_owner.set(self.in_tx.then(sched::task_id));
                Ok(Value::Nil)
            }
            "close" => {
                if let Some(pool) = self.pool.take() {
                    rt::tokio().block_on(pool.close());
                }
                self.in_tx = false;
                self.tx_owner.set(None);
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

fn row_to_map(row: PlainRow) -> Value {
    let mut map = HashMap::new();
    for (name, cell) in row {
        let v = match cell {
            Cell2::Null => Value::Nil,
            Cell2::Bool(b) => Value::Bool(b),
            Cell2::Int(n) => Value::Int(n),
            Cell2::Float(f) => Value::Float(f),
            Cell2::Text(t) => Value::str(t),
            Cell2::Blob(b) => Value::buffer(b),
        };
        map.insert(name.into(), v);
    }
    Value::Map(Rc::new(RefCell::new(map)))
}

/// A SQLite row: each value is read by what it really holds (SQLite's
/// storage class: INTEGER, REAL, TEXT, BLOB), not by the declared type of its
/// column, which SQLite does not enforce. An integer in a BOOLEAN / BOOL
/// column is a `bool`; DATE / DATETIME values are the text (or number) stored.
fn sqlite_row(row: &SqliteRow) -> PlainRow {
    row.columns()
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let storage = match row.try_get_raw(i) {
                Ok(v) if !v.is_null() => Some(v.type_info().name().to_string()),
                _ => None,
            };
            let cell = match storage.as_deref() {
                None | Some("NULL") => Cell2::Null,
                Some("INTEGER" | "BOOLEAN" | "NUMERIC") => {
                    let n = row.try_get_unchecked::<i64, _>(i).unwrap_or(0);
                    let decl = col.type_info().name().to_ascii_uppercase();
                    if decl == "BOOLEAN" || decl == "BOOL" {
                        Cell2::Bool(n != 0)
                    } else {
                        Cell2::Int(n)
                    }
                }
                Some("REAL") => Cell2::Float(row.try_get_unchecked::<f64, _>(i).unwrap_or(0.0)),
                Some("BLOB") => Cell2::Blob(row.try_get_unchecked::<Vec<u8>, _>(i).unwrap_or_default()),
                Some(_) => match row.try_get_unchecked::<String, _>(i) {
                    Ok(t) => Cell2::Text(t),
                    Err(_) => Cell2::Blob(row.try_get_unchecked::<Vec<u8>, _>(i).unwrap_or_default()),
                },
            };
            (col.name().to_string(), cell)
        })
        .collect()
}

/// A PostgreSQL / MySQL row through the `Any` driver.
fn any_row(row: &AnyRow) -> Result<PlainRow, sqlx::Error> {
    let mut out = Vec::with_capacity(row.columns().len());
    for (i, col) in row.columns().iter().enumerate() {
        out.push((col.name().to_string(), any_value(row, i)?));
    }
    Ok(out)
}

fn any_value(row: &AnyRow, i: usize) -> Result<Cell2, sqlx::Error> {
    let type_name = {
        let vr = row.try_get_raw(i)?;
        if vr.is_null() {
            return Ok(Cell2::Null);
        }
        vr.type_info().name().to_string()
    };
    Ok(match type_name.as_str() {
        "BOOL" | "BOOLEAN" => Cell2::Bool(row.try_get::<bool, _>(i)?),
        "INT2" | "SMALLINT" | "TINYINT" => Cell2::Int(row.try_get::<i16, _>(i)? as i64),
        "INT4" | "INTEGER" | "INT" | "MEDIUMINT" => Cell2::Int(row.try_get::<i32, _>(i)? as i64),
        "INT8" | "BIGINT" => Cell2::Int(row.try_get::<i64, _>(i)?),
        "FLOAT4" | "REAL" => Cell2::Float(row.try_get::<f32, _>(i)? as f64),
        "FLOAT8" | "DOUBLE" | "DOUBLE PRECISION" => Cell2::Float(row.try_get::<f64, _>(i)?),
        "BLOB" | "BYTEA" | "VARBINARY" | "BINARY" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => Cell2::Blob(row.try_get::<Vec<u8>, _>(i)?),
        _ => match row.try_get::<String, _>(i) {
            Ok(t) => Cell2::Text(t),
            Err(_) => Cell2::Blob(row.try_get::<Vec<u8>, _>(i)?),
        },
    })
}
