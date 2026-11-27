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
//!
//! let pg = sql.open("postgres://u:p@host/db", { pool: 8 })?   // 8 connections
//! pg.begin()?            // this task's transaction, on a connection of its own
//! pg.run("UPDATE ...")   // other tasks keep using the rest of the pool
//! pg.commit()?
//! ```

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::sync::Arc;

use sqlx::any::{AnyPoolOptions, AnyRow};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{AnyPool, Column, Row, SqlitePool, TypeInfo, ValueRef};

use crate::runtime::sched;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;

// ---------- Module ----------

pub fn build() -> ModuleValue {
    sqlx::any::install_default_drivers();

    let mut exports = HashMap::new();

    // open(url, { pool = 1 }) → Sql
    exports.insert(
        "open".into(),
        crate::native::util::nfn_var("open", 1, |args| {
            crate::native::util::max_args(args, 2, "sql.open")?;
            let url = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(format!(
                        "open(url) expects str, got '{}'",
                        other.type_name()
                    ))
                }
            };
            let size = pool_option(args.get(1), "sql.open")?;
            let (pool, url) = connect(&url, size).map_err(|e| io_err!("sql.open: {}", e))?;
            Ok(NativeObjectRef::new(SqlValue::new(pool, url)).to_value())
        }),
    );

    ModuleValue {
        name: "fogu:sql".into(),
        exports: RefCell::new(exports),
    }
}

/// `{ pool: n }` (1 to 256; 1 when absent).
pub(crate) fn pool_option(v: Option<&Value>, what: &str) -> Result<u32, String> {
    let n = match v {
        None | Some(Value::Nil) => return Ok(1),
        Some(Value::Map(m)) => match m.borrow().get("pool") {
            None | Some(Value::Nil) => return Ok(1),
            Some(Value::Int(n)) => *n,
            Some(other) => return Err(format!("{}: 'pool' must be an int, got '{}'", what, other.type_name())),
        },
        Some(other) => return Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    };
    if !(1..=256).contains(&n) {
        return Err(format!("{}: 'pool' must be between 1 and 256, got {}", what, n));
    }
    Ok(n as u32)
}

/// A connection pool: SQLite through its own driver (the `Any` driver cannot
/// read BOOLEAN, DATE, DATETIME nor BLOB columns), the others through `Any`
/// (`pg`: PostgreSQL, whose NULL parameters are written into the statement).
#[derive(Clone)]
pub(crate) struct Pool {
    kind: PoolKind,
    /// How many connections it may open.
    size: u32,
}

#[derive(Clone)]
enum PoolKind {
    Sqlite(SqlitePool),
    Any(AnyPool, bool),
}

impl Pool {
    async fn close(&self) {
        match &self.kind {
            PoolKind::Sqlite(p) => p.close().await,
            PoolKind::Any(p, _) => p.close().await,
        }
    }
}

/// A connection taken out of the pool by a transaction, until its
/// `commit()` / `rollback()`: every statement of the task that opened it runs
/// on it, while other tasks go on with the rest of the pool.
pub(crate) enum Held {
    Sqlite(sqlx::pool::PoolConnection<sqlx::Sqlite>),
    Any(sqlx::pool::PoolConnection<sqlx::Any>, bool),
}

pub(crate) type Tx = Arc<tokio::sync::Mutex<Held>>;

/// Where a statement runs.
#[derive(Clone)]
enum Target {
    Pool(Pool),
    Tx(Tx),
}

/// Opens a pool of `size` connections on the shared runtime (an in-memory
/// SQLite database is one connection: each would see its own empty database).
/// Returns the pool and the effective URL.
pub(crate) fn connect(url: &str, size: u32) -> Result<(Pool, String), String> {
    sqlx::any::install_default_drivers();
    let url = url.to_string();
    if url.starts_with("sqlite:") {
        use std::str::FromStr;
        let size = if url.contains(":memory:") || url.contains("mode=memory") { 1 } else { size };
        // the file is created if it does not exist
        let opts = SqliteConnectOptions::from_str(&url).map_err(|e| crate::native::util::redact_url(&format!("{} at '{}'", e, url)))?.create_if_missing(true);
        let pool = rt::block_on(async move {
            SqlitePoolOptions::new()
                .max_connections(size)
                .min_connections(1)
                .idle_timeout(None)
                .max_lifetime(None)
                .connect_with(opts)
                .await
        })?
        .map_err(|e| crate::native::util::redact_url(&format!("{} at '{}'", e, url)))?;
        return Ok((Pool { kind: PoolKind::Sqlite(pool), size }, url));
    }
    let u = url.clone();
    let pg = url.starts_with("postgres:") || url.starts_with("postgresql:");
    let pool = rt::block_on(async move {
        AnyPoolOptions::new()
            .max_connections(size)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect(&u)
            .await
    })?
    .map_err(|e| crate::native::util::redact_url(&format!("{} at '{}'", e, url)))?;
    Ok((Pool { kind: PoolKind::Any(pool, pg), size }, url))
}

/// A parameter that can travel to the runtime's threads.
#[derive(Clone, Debug, PartialEq)]
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

/// PostgreSQL: a NULL parameter is bound with a type (the `Any` driver says
/// BIGINT), which a TEXT, BYTEA or BOOLEAN column refuses ("incorrect binary
/// data format"). Each `$n` that is NULL is written as `NULL` in the
/// statement instead, and the later placeholders are renumbered. Quoted
/// text, quoted names and comments are left alone.
fn inline_nulls(sql: &str, params: Vec<Param>) -> (String, Vec<Param>) {
    if !params.iter().any(|p| *p == Param::Null) {
        return (sql.to_string(), params);
    }
    // the new number of each parameter (None: inlined)
    let mut renumber = Vec::with_capacity(params.len());
    let mut next = 0usize;
    for p in &params {
        if *p == Param::Null {
            renumber.push(None);
        } else {
            next += 1;
            renumber.push(Some(next));
        }
    }
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len() + 8);
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        match b[i] {
            q @ (b'\'' | b'"') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                i += 1;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'$' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                let start = i;
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                let n: usize = sql[start + 1..i].parse().unwrap_or(0);
                if let Some(slot) = n.checked_sub(1).and_then(|k| renumber.get(k)) {
                    out.push_str(&sql[last..start]);
                    match slot {
                        None => out.push_str("NULL"),
                        Some(k) => {
                            out.push('$');
                            out.push_str(&k.to_string());
                        }
                    }
                    last = i;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&sql[last.min(sql.len())..]);
    (out, params.into_iter().filter(|p| *p != Param::Null).collect())
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

type DbFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, sqlx::Error>> + Send + 'a>>;

// (boxed with their lifetime: sqlx's executors on `&mut` connections are not
// `Send` for every lifetime, which a plain `async fn` would ask)
fn sqlite_on<'a>(c: &'a mut sqlx::SqliteConnection, op: Op, sql: &'a str, params: &'a [Param]) -> DbFuture<'a, Out> {
    Box::pin(async move {
    let out = if let Op::Raw = op {
        let r = sqlx::Executor::execute(&mut *c, sqlx::raw_sql(sql)).await?;
        Out::Done(r.rows_affected(), Some(r.last_insert_rowid()))
    } else {
        let q = bind_all!(sqlx::query(sql), params);
        match op {
            Op::Query => Out::Rows(q.fetch_all(&mut *c).await?.iter().map(sqlite_row).collect()),
            Op::One => Out::Row(q.fetch_optional(&mut *c).await?.as_ref().map(sqlite_row)),
            _ => {
                let r = q.execute(&mut *c).await?;
                Out::Done(r.rows_affected(), Some(r.last_insert_rowid()))
            }
        }
    };
    if matches!(op, Op::Raw | Op::Execute) && is_ddl(sql) {
        use sqlx::Connection;
        let _ = c.clear_cached_statements().await;
    }
    Ok(out)
    })
}

fn any_on<'a>(c: &'a mut sqlx::AnyConnection, pg: bool, op: Op, sql: &'a str, params: Vec<Param>) -> DbFuture<'a, Out> {
    Box::pin(async move {
    if let Op::Raw = op {
        let r = sqlx::Executor::execute(&mut *c, sqlx::raw_sql(sql)).await?;
        return Ok(Out::Done(r.rows_affected(), r.last_insert_id()));
    }
    let (sql, params) = if pg { inline_nulls(sql, params) } else { (sql.to_string(), params) };
    let q = bind_all!(sqlx::query(&sql), &params);
    let out = match op {
        Op::Query => Out::Rows(q.fetch_all(&mut *c).await?.iter().map(any_row).collect::<Result<_, _>>()?),
        Op::One => Out::Row(q.fetch_optional(&mut *c).await?.as_ref().map(any_row).transpose()?),
        _ => {
            let r = q.execute(&mut *c).await?;
            Out::Done(r.rows_affected(), r.last_insert_id())
        }
    };
    if matches!(op, Op::Execute) && is_ddl(&sql) {
        use sqlx::Connection;
        let _ = c.clear_cached_statements().await;
    }
    Ok(out)
    })
}

fn raw_on<'a>(held: &'a mut Held, sql: &'a str) -> DbFuture<'a, ()> {
    Box::pin(async move {
        match held {
            Held::Sqlite(c) => sqlx::Executor::execute(&mut **c, sqlx::raw_sql(sql)).await.map(|_| ()),
            Held::Any(c, _) => sqlx::Executor::execute(&mut **c, sqlx::raw_sql(sql)).await.map(|_| ()),
        }
    })
}

async fn run_op(t: Target, op: Op, sql: String, params: Vec<Param>) -> Result<Out, sqlx::Error> {
    match t {
        Target::Pool(Pool { kind: PoolKind::Sqlite(p), .. }) => sqlite_on(&mut *p.acquire().await?, op, &sql, &params).await,
        Target::Pool(Pool { kind: PoolKind::Any(p, pg), .. }) => any_on(&mut *p.acquire().await?, pg, op, &sql, params).await,
        Target::Tx(tx) => match &mut *tx.lock().await {
            Held::Sqlite(c) => sqlite_on(c, op, &sql, &params).await,
            Held::Any(c, pg) => {
                let pg = *pg;
                any_on(c, pg, op, &sql, params).await
            }
        },
    }
}

/// `begin()`: a connection of the pool, inside a new transaction.
async fn begin_tx(pool: Pool, sql: &'static str) -> Result<Tx, sqlx::Error> {
    let mut held = match pool.kind {
        PoolKind::Sqlite(p) => Held::Sqlite(p.acquire().await?),
        PoolKind::Any(p, pg) => Held::Any(p.acquire().await?, pg),
    };
    raw_on(&mut held, sql).await?;
    Ok(Arc::new(tokio::sync::Mutex::new(held)))
}

/// `commit()` / `rollback()`: the connection goes back to the pool afterwards.
async fn end_tx(tx: Tx, sql: &'static str) -> Result<(), sqlx::Error> {
    let mut g = tx.lock().await;
    raw_on(&mut g, sql).await
}

/// The open transactions of a database, by task (`sched::task_id`).
#[derive(Clone, Default)]
pub(crate) struct Txs(Rc<RefCell<HashMap<usize, Tx>>>);

impl Txs {
    fn mine(&self) -> Option<Tx> {
        let txs = self.0.borrow();
        if txs.is_empty() {
            return None;
        }
        txs.get(&sched::task_id()).cloned()
    }

    fn target(&self, pool: &Pool) -> Target {
        match self.mine() {
            Some(tx) => Target::Tx(tx),
            None => Target::Pool(pool.clone()),
        }
    }

    /// With one connection, another task's transaction holds it: wait
    /// (cooperatively, without a deadline) instead of queueing in the pool.
    fn wait_free(&self, pool_size: u32) -> Option<Box<dyn Fn() -> bool>> {
        if pool_size > 1 || self.0.borrow().is_empty() {
            return None;
        }
        let txs = Rc::clone(&self.0);
        let free = move || {
            let t = txs.borrow();
            t.is_empty() || t.contains_key(&sched::task_id())
        };
        if free() { None } else { Some(Box::new(free)) }
    }
}

/// A connection pool used by other native modules (fogu:orm), from code that
/// holds no object borrowed: statements wait cooperatively, so other tasks
/// run meanwhile. A transaction belongs to the task that began it, on its own
/// connection.
pub(crate) struct SqlConn {
    pool: RefCell<Option<Pool>>,
    txs: Txs,
}

fn sql_err(e: sqlx::Error) -> String {
    e.to_string()
}

impl SqlConn {
    pub(crate) fn open(url: &str, size: u32) -> Result<SqlConn, String> {
        let (pool, _) = connect(url, size)?;
        Ok(SqlConn { pool: RefCell::new(Some(pool)), txs: Txs::default() })
    }

    fn pool(&self) -> Result<Pool, String> {
        self.pool.borrow().clone().ok_or_else(|| "the database connection is closed".to_string())
    }

    fn run(&self, op: Op, sql: &str, params: &[Value]) -> Result<Out, String> {
        let pool = self.pool()?;
        if let Some(wait) = self.txs.wait_free(pool.size) {
            sched::wait_until(wait)?;
        }
        let target = self.txs.target(&pool);
        rt::block_on(run_op(target, op, sql.to_string(), to_params(params)))?.map_err(sql_err)
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

    /// `BEGIN` on a connection of its own, for the calling task.
    pub(crate) fn begin(&self, sql: &'static str) -> Result<(), String> {
        if self.txs.mine().is_some() {
            return Err("a transaction is already open in this task".into());
        }
        let pool = self.pool()?;
        if let Some(wait) = self.txs.wait_free(pool.size) {
            sched::wait_until(wait)?;
        }
        let task = sched::task_id();
        let tx = rt::block_on(begin_tx(pool, sql))?.map_err(sql_err)?;
        self.txs.0.borrow_mut().insert(task, tx);
        Ok(())
    }

    /// `COMMIT` / `ROLLBACK` of the calling task's transaction.
    pub(crate) fn end(&self, sql: &'static str) -> Result<(), String> {
        let tx = self.txs.0.borrow_mut().remove(&sched::task_id()).ok_or("no transaction is open in this task")?;
        rt::block_on(end_tx(tx, sql))?.map_err(sql_err)
    }

    /// Whether the calling task has a transaction open.
    pub(crate) fn in_tx(&self) -> bool {
        self.txs.mine().is_some()
    }

    pub(crate) fn close(&self) {
        self.txs.0.borrow_mut().clear();
        let pool = self.pool.borrow_mut().take();
        if let Some(pool) = pool {
            let _ = rt::block_on(async move { pool.close().await });
        }
    }
}

// ---------- SQL object ----------

pub struct SqlValue {
    pool: Option<Pool>,
    url: String,
    /// Open transactions, each on its own connection, by task.
    txs: Txs,
}

impl SqlValue {
    pub(crate) fn new(pool: Pool, url: String) -> Self {
        Self { pool: Some(pool), url, txs: Txs::default() }
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

    fn tx_sql(&self, method: &str) -> &'static str {
        match method {
            "begin" if self.url.starts_with("mysql:") || self.url.starts_with("mariadb:") => "START TRANSACTION",
            "begin" => "BEGIN",
            "commit" => "COMMIT",
            _ => "ROLLBACK",
        }
    }

    /// The work of `begin` / `commit` / `rollback` for the current task, as a future.
    #[allow(clippy::type_complexity)]
    fn tx_op(&self, method: &str) -> Result<std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Tx>, sqlx::Error>> + Send>>, String> {
        let sql = self.tx_sql(method);
        if method == "begin" {
            if self.txs.mine().is_some() {
                return Err(io_err!("sql.begin: a transaction is already open in this task"));
            }
            let pool = self.pool()?;
            return Ok(Box::pin(async move { begin_tx(pool, sql).await.map(Some) }));
        }
        let tx = self.txs.0.borrow_mut().remove(&sched::task_id()).ok_or_else(|| io_err!("sql.{}: no transaction is open in this task", method))?;
        Ok(Box::pin(async move { end_tx(tx, sql).await.map(|_| None) }))
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

/// A transaction step's outcome: the new transaction is the task's.
fn finish_tx(method: &str, txs: &Txs, task: usize, r: Result<Option<Tx>, sqlx::Error>) -> Result<Value, String> {
    match r.map_err(|e| io_err!("sql.{}: {}", method, e))? {
        Some(tx) => {
            txs.0.borrow_mut().insert(task, tx);
        }
        None => {}
    }
    Ok(Value::Nil)
}

impl FoguObject for SqlValue {
    fn name(&self) -> &str {
        "Sql"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "url" => Some(Value::Str(self.url.clone().into())),
            "driver" => Some(Value::Str(driver_name(&self.url).into())),
            "pool" => Some(Value::Int(self.pool.as_ref().map_or(0, |p| p.size as i64))),
            "inTransaction" => Some(Value::Bool(self.txs.mine().is_some())),
            _ => None,
        }
    }

    /// Statements run on the shared runtime while other `fg` tasks keep
    /// going (inside a transaction too: on its own connection) — or inline
    /// when no other task could run meanwhile.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if !rt::concurrent() {
            return None;
        }
        if matches!(method, "begin" | "commit" | "rollback") {
            let fut = match self.tx_op(method) {
                Ok(f) => f,
                Err(e) => return Some(Err(e)),
            };
            let (txs, task, method) = (self.txs.clone(), sched::task_id(), method.to_string());
            let pending = rt::spawn(fut);
            return Some(Ok(Box::new(move || pending.try_take().map(|r| finish_tx(&method, &txs, task, r)))));
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
        let target = self.txs.target(&pool);
        let pending = rt::spawn(run_op(target, op, sql, params));
        let method = method.to_string();
        Some(Ok(Box::new(move || pending.try_take().map(|r| finish(&method, r)))))
    }

    /// With one connection, the statements of other tasks wait while a
    /// task's transaction is open (the common case costs one check).
    fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        if !matches!(method, "run" | "execute" | "query" | "one" | "exec" | "begin") {
            return None;
        }
        self.txs.wait_free(self.pool.as_ref().map_or(1, |p| p.size))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        if let Some((op, sql, params)) = Self::statement(method, args)? {
            let target = self.txs.target(&self.pool()?);
            return finish(method, rt::tokio().block_on(run_op(target, op, sql, params)));
        }
        match method {
            "begin" | "commit" | "rollback" => {
                let fut = self.tx_op(method)?;
                let r = rt::tokio().block_on(fut);
                finish_tx(method, &self.txs, sched::task_id(), r)
            }
            "close" => {
                self.txs.0.borrow_mut().clear();
                if let Some(pool) = self.pool.take() {
                    rt::tokio().block_on(pool.close());
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nulls_are_written_into_postgres_statements() {
        let p = vec![Param::Int(1), Param::Null, Param::Str("x".into()), Param::Null];
        let (sql, rest) = inline_nulls("INSERT INTO t VALUES ($1, $2, $3, $4) -- $2\n RETURNING '$2', \"$3\"", p);
        assert_eq!(sql, "INSERT INTO t VALUES ($1, NULL, $2, NULL) -- $2\n RETURNING '$2', \"$3\"");
        assert_eq!(rest, vec![Param::Int(1), Param::Str("x".into())]);
        let (sql, rest) = inline_nulls("SELECT $1 WHERE a = $1 OR b = $2", vec![Param::Null, Param::Int(5)]);
        assert_eq!(sql, "SELECT NULL WHERE a = NULL OR b = $1");
        assert_eq!(rest, vec![Param::Int(5)]);
        let (sql, _) = inline_nulls("SELECT $10, $1", (0..10).map(|i| if i == 0 { Param::Null } else { Param::Int(i) }).collect());
        assert_eq!(sql, "SELECT $9, NULL");
        assert_eq!(inline_nulls("SELECT $1", vec![Param::Int(1)]).0, "SELECT $1");
    }
}
