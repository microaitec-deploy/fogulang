//! `fogu:orm` module — a Prisma-style ORM over SQL (SQLite, PostgreSQL,
//! MySQL/MariaDB), native in Rust.
//!
//! ```fogu
//! import { create_client } from "fogu:orm"
//!
//! const db = create_client({
//!     schema: {
//!         provider: "sqlite",
//!         url: "sqlite://app.db",
//!         models: {
//!             User: {
//!                 fields: {
//!                     id:    { type: "Int", primary: true, autoincrement: true },
//!                     email: { type: "String", required: true, unique: true },
//!                 },
//!                 relations: { posts: { model: "Post", kind: "one-to-many", foreignKey: "authorId" } }
//!             },
//!             Post: { … relations: { author: { model: "User", kind: "many-to-one", foreignKey: "authorId" } } }
//!         }
//!     },
//!     migrate: "auto",
//! })?
//! const u = db.user.create({ data: { email: "a@x.com" } })?
//! for u in db.user.findMany({ where: { email: { contains: "@x" } }, include: { posts: true } })? { … }
//! ```
//!
//! Models are reached as `db.<model in camelCase>` (`User` → `db.user`) and
//! offer `create createMany findMany findFirst findUnique update updateMany
//! upsert delete deleteMany count`. Relations (`one-to-many`, `many-to-one`,
//! `one-to-one`) are loaded with one extra query per relation level (no N+1).
//! Database errors are operational errors (`error` values); a malformed query
//! (unknown field or operator) stops the program.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::interpreter::values_equal;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::sql::SqlConn;
use crate::native::util::{list, map, max_args, nfn_var};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("create_client".into(), nfn_var("create_client", 1, create_client));
    e.insert("createClient".into(), nfn_var("createClient", 1, create_client));
    ModuleValue {
        name: "fogu:orm".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum FieldType {
    Int,
    BigInt,
    String,
    Bool,
    Float,
    DateTime,
    Json,
}

#[derive(Clone)]
struct Field {
    name: String,
    ty: FieldType,
    primary: bool,
    autoincrement: bool,
    required: bool,
    unique: bool,
    default: Option<Value>,
}

#[derive(Clone, Copy, PartialEq)]
enum RelKind {
    OneToMany,
    ManyToOne,
    OneToOne,
}

#[derive(Clone)]
struct Relation {
    model: String,
    kind: RelKind,
    foreign_key: String,
    references: Option<String>,
}

#[derive(Clone)]
struct Model {
    name: String,
    fields: Vec<Field>,
    relations: BTreeMap<String, Relation>,
}

impl Model {
    fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
    fn pk(&self) -> &Field {
        self.fields.iter().find(|f| f.primary).unwrap_or(&self.fields[0])
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Dialect {
    Sqlite,
    Postgres,
    Mysql,
}

fn get<'a>(m: &'a HashMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(k).filter(|v| !matches!(v, Value::Nil))
}

fn as_map(v: &Value, what: &str) -> Result<Rc<RefCell<HashMap<String, Value>>>, String> {
    match v {
        Value::Map(m) => Ok(Rc::clone(m)),
        other => Err(format!("{} must be a map, got '{}'", what, other.type_name())),
    }
}

fn truthy(v: Option<&Value>) -> bool {
    v.is_some_and(|v| v.is_truthy())
}

fn parse_models(models: &HashMap<String, Value>) -> Result<HashMap<String, Model>, String> {
    let mut out = HashMap::new();
    for (name, def) in models {
        let def = as_map(def, &format!("model {}", name))?;
        let def = def.borrow();
        let fdefs = as_map(get(&def, "fields").ok_or(format!("model {} has no fields", name))?, "fields")?;
        let mut fields = Vec::new();
        let mut names: Vec<String> = fdefs.borrow().keys().cloned().collect();
        names.sort();
        for fname in names {
            let fd = as_map(&fdefs.borrow()[&fname], &format!("{}.{}", name, fname))?;
            let fd = fd.borrow();
            let tyname = match get(&fd, "type") {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err(format!("{}.{}: missing \"type\"", name, fname)),
            };
            let ty = match tyname.as_str() {
                "Int" | "Integer" => FieldType::Int,
                "BigInt" => FieldType::BigInt,
                "String" | "Text" => FieldType::String,
                "Bool" | "Boolean" => FieldType::Bool,
                "Float" | "Double" | "Decimal" => FieldType::Float,
                "DateTime" | "Date" => FieldType::DateTime,
                "Json" => FieldType::Json,
                other => return Err(format!("{}.{}: unknown type '{}'", name, fname, other)),
            };
            let primary = truthy(get(&fd, "primary")) || truthy(get(&fd, "id"));
            fields.push(Field {
                name: fname.clone(),
                ty,
                primary,
                autoincrement: truthy(get(&fd, "autoincrement")),
                required: truthy(get(&fd, "required")) || primary,
                unique: truthy(get(&fd, "unique")),
                default: get(&fd, "defaultValue").or_else(|| get(&fd, "default")).cloned(),
            });
        }
        // primary key first, then the rest in alphabetical order
        fields.sort_by_key(|f| !f.primary);
        if fields.is_empty() {
            return Err(format!("model {} has no fields", name));
        }
        let mut relations = BTreeMap::new();
        if let Some(rdefs) = get(&def, "relations") {
            let rdefs = as_map(rdefs, "relations")?;
            for (rname, rd) in rdefs.borrow().iter() {
                let rd = as_map(rd, &format!("{}.{}", name, rname))?;
                let rd = rd.borrow();
                let s = |k: &str| match get(&rd, k) {
                    Some(Value::Str(v)) => Some(v.to_string()),
                    _ => None,
                };
                let model = s("model").ok_or(format!("relation {}.{}: missing \"model\"", name, rname))?;
                let kind = match s("kind").as_deref() {
                    Some("one-to-many") | Some("oneToMany") | Some("hasMany") => RelKind::OneToMany,
                    Some("many-to-one") | Some("manyToOne") | Some("belongsTo") => RelKind::ManyToOne,
                    Some("one-to-one") | Some("oneToOne") | Some("hasOne") => RelKind::OneToOne,
                    other => return Err(format!("relation {}.{}: unknown kind {:?}", name, rname, other)),
                };
                let foreign_key =
                    s("foreignKey").ok_or(format!("relation {}.{}: missing \"foreignKey\"", name, rname))?;
                relations.insert(
                    rname.clone(),
                    Relation { model, kind, foreign_key, references: s("references") },
                );
            }
        }
        out.insert(name.clone(), Model { name: name.clone(), fields, relations });
    }
    // relations must point to existing models and fields
    for m in out.values() {
        for (rname, r) in &m.relations {
            let target = out
                .get(&r.model)
                .ok_or(format!("relation {}.{}: unknown model '{}'", m.name, rname, r.model))?;
            let fk_owner = if fk_on_self(m, r) { m } else { target };
            if fk_owner.field(&r.foreign_key).is_none() {
                return Err(format!(
                    "relation {}.{}: '{}' is not a field of {}",
                    m.name, rname, r.foreign_key, fk_owner.name
                ));
            }
        }
    }
    Ok(out)
}

/// Is the foreign key a field of the model that declares the relation?
fn fk_on_self(m: &Model, r: &Relation) -> bool {
    match r.kind {
        RelKind::ManyToOne => true,
        RelKind::OneToMany => false,
        RelKind::OneToOne => m.field(&r.foreign_key).is_some(),
    }
}

// ---------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------

struct Inner {
    conn: RefCell<SqlConn>,
    dialect: Dialect,
    models: HashMap<String, Model>,
    log: bool,
}

fn create_client(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "create_client")?;
    let cfg = as_map(&a[0], "create_client(config)")?;
    let cfg = cfg.borrow();
    let schema = as_map(get(&cfg, "schema").unwrap_or(&a[0]), "schema")?;
    let schema = schema.borrow();
    let url = match get(&schema, "url").or_else(|| get(&cfg, "url")) {
        Some(Value::Str(u)) => u.clone(),
        _ => return Err("create_client: schema.url is required".into()),
    };
    let provider = match get(&schema, "provider") {
        Some(Value::Str(p)) => p.clone(),
        _ => url.split(':').next().unwrap_or("").to_string().into(),
    };
    let dialect = match provider.as_str() {
        "sqlite" => Dialect::Sqlite,
        "postgres" | "postgresql" => Dialect::Postgres,
        "mysql" | "mariadb" => Dialect::Mysql,
        other => return Err(format!("create_client: unknown provider '{}' (sqlite, postgresql, mysql)", other)),
    };
    let models = parse_models(&as_map(
        get(&schema, "models").ok_or("create_client: schema.models is required")?,
        "models",
    )?
    .borrow())?;
    let log = truthy(get(&cfg, "log"));
    let migrate = match get(&cfg, "migrate") {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Bool(true)) => "auto".into(),
        _ => "none".into(),
    };
    let conn = SqlConn::open(&url).map_err(|e| io_err!("orm: cannot connect: {}", e))?;
    let inner = Rc::new(Inner { conn: RefCell::new(conn), dialect, models, log });
    if migrate == "auto" {
        migrate_all(&inner)?;
    }
    Ok(NativeObjectRef::new(ClientValue { inner }).to_value())
}

/// `User` → `user`, `BlogPost` → `blogPost`.
fn accessor(model: &str) -> String {
    let mut c = model.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub struct ClientValue {
    inner: Rc<Inner>,
}

impl FoguObject for ClientValue {
    fn name(&self) -> &str {
        "OrmClient"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let model = self.inner.models.keys().find(|m| accessor(m) == field || *m == field)?;
        Some(NativeObjectRef::new(ModelValue { inner: Rc::clone(&self.inner), model: model.clone() }).to_value())
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "disconnect" | "close" => {
                self.inner.conn.borrow_mut().close();
                Ok(Value::Nil)
            }
            "migrate" => migrate_all(&self.inner).map(|_| Value::Nil),
            // raw SQL escape hatches
            "query" | "queryRaw" => {
                let (sql, params) = raw_args(args, method)?;
                self.inner.query(&sql, &params).map(list)
            }
            "execute" | "executeRaw" => {
                let (sql, params) = raw_args(args, method)?;
                self.inner.exec(&sql, &params).map(|(n, _)| Value::Int(n as i64))
            }
            "begin" | "commit" | "rollback" => {
                let sql = match (method, self.inner.dialect) {
                    ("begin", Dialect::Mysql) => "START TRANSACTION",
                    ("begin", _) => "BEGIN",
                    ("commit", _) => "COMMIT",
                    _ => "ROLLBACK",
                };
                self.inner.exec(sql, &[]).map(|_| Value::Nil)
            }
            other => Err(format!(
                "method '{}' does not exist on the ORM client (models: {})",
                other,
                {
                    let mut m: Vec<String> = self.inner.models.keys().map(|k| accessor(k)).collect();
                    m.sort();
                    m.join(", ")
                }
            )),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["disconnect", "migrate", "query", "execute", "begin", "commit", "rollback"]
    }
}

fn raw_args(args: &[Value], what: &str) -> Result<(String, Vec<Value>), String> {
    let sql = crate::native::util::str_arg(args, 0, what)?;
    let params = match args.get(1) {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::List(l)) => l.borrow().clone(),
        Some(other) => return Err(format!("{}: params must be a list, got '{}'", what, other.type_name())),
    };
    Ok((sql, params))
}

impl Inner {
    fn logq(&self, sql: &str, params: &[Value]) {
        if self.log {
            let p: Vec<String> = params.iter().map(|v| v.to_display_string()).collect();
            println!("[orm] {}  -- [{}]", sql, p.join(", "));
        }
    }

    fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Value>, String> {
        self.logq(sql, params);
        self.conn.borrow().query(sql, params).map_err(|e| io_err!("orm: {}", e))
    }

    fn exec(&self, sql: &str, params: &[Value]) -> Result<(u64, Option<i64>), String> {
        self.logq(sql, params);
        self.conn.borrow().execute(sql, params).map_err(|e| io_err!("orm: {}", e))
    }

    fn q(&self, ident: &str) -> String {
        match self.dialect {
            Dialect::Mysql => format!("`{}`", ident.replace('`', "``")),
            _ => format!("\"{}\"", ident.replace('"', "\"\"")),
        }
    }

    fn model(&self, name: &str) -> &Model {
        &self.models[name]
    }
}

// ---------------------------------------------------------------------
// Migrations
// ---------------------------------------------------------------------

fn sql_type(d: Dialect, f: &Field) -> String {
    let t = match (f.ty, d) {
        (FieldType::Int, Dialect::Mysql) => "INT",
        (FieldType::Int, _) => "INTEGER",
        (FieldType::BigInt, _) => "BIGINT",
        // MySQL cannot index TEXT without a length
        (FieldType::String, Dialect::Mysql) if f.unique || f.primary => "VARCHAR(255)",
        (FieldType::String, _) => "TEXT",
        (FieldType::Bool, Dialect::Sqlite) => "INTEGER",
        (FieldType::Bool, _) => "BOOLEAN",
        (FieldType::Float, Dialect::Sqlite) => "REAL",
        (FieldType::Float, Dialect::Postgres) => "DOUBLE PRECISION",
        (FieldType::Float, Dialect::Mysql) => "DOUBLE",
        (FieldType::DateTime, Dialect::Mysql) => "VARCHAR(40)",
        (FieldType::DateTime, _) => "TEXT",
        (FieldType::Json, _) => "TEXT",
    };
    t.to_string()
}

fn literal(d: Dialect, v: &Value) -> Option<String> {
    match v {
        Value::Int(n) => Some(n.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Bool(b) if d == Dialect::Sqlite => Some(if *b { "1".into() } else { "0".into() }),
        Value::Bool(b) => Some(if *b { "TRUE".into() } else { "FALSE".into() }),
        Value::Str(s) if s != "now" && s != "uuid" && s != "cuid" => Some(format!("'{}'", s.replace('\'', "''"))),
        _ => None,
    }
}

fn column_ddl(inner: &Inner, f: &Field, adding: bool) -> String {
    let d = inner.dialect;
    let mut s = format!("{} ", inner.q(&f.name));
    if f.primary && f.autoincrement {
        s.push_str(match (d, f.ty) {
            (Dialect::Sqlite, _) => "INTEGER PRIMARY KEY AUTOINCREMENT",
            (Dialect::Postgres, FieldType::BigInt) => "BIGSERIAL PRIMARY KEY",
            (Dialect::Postgres, _) => "SERIAL PRIMARY KEY",
            (Dialect::Mysql, FieldType::BigInt) => "BIGINT AUTO_INCREMENT PRIMARY KEY",
            (Dialect::Mysql, _) => "INT AUTO_INCREMENT PRIMARY KEY",
        });
        return s;
    }
    s.push_str(&sql_type(d, f));
    if f.primary {
        s.push_str(" PRIMARY KEY");
    }
    let default = f.default.as_ref().and_then(|v| literal(d, v));
    // a column added later cannot be NOT NULL without a default
    if f.required && !f.primary && (!adding || default.is_some()) {
        s.push_str(" NOT NULL");
    }
    if f.unique && !f.primary && !adding {
        s.push_str(" UNIQUE");
    }
    if let Some(dv) = default {
        if !(d == Dialect::Mysql && f.ty == FieldType::String) {
            s.push_str(&format!(" DEFAULT {}", dv));
        }
    }
    s
}

/// Creates missing tables (with their foreign keys) and adds missing columns.
fn migrate_all(inner: &Inner) -> Result<(), String> {
    // tables referenced by a many-to-one relation are created first
    let mut order: Vec<String> = Vec::new();
    let mut names: Vec<&String> = inner.models.keys().collect();
    names.sort();
    fn visit(inner: &Inner, name: &str, order: &mut Vec<String>, stack: &mut HashSet<String>) {
        if order.iter().any(|n| n == name) || !stack.insert(name.to_string()) {
            return;
        }
        let m = inner.model(name);
        for r in m.relations.values() {
            if fk_on_self(m, r) {
                visit(inner, &r.model, order, stack);
            }
        }
        order.push(name.to_string());
    }
    for n in names {
        visit(inner, n, &mut order, &mut HashSet::new());
    }

    for name in &order {
        let m = inner.model(name);
        let mut cols: Vec<String> = m.fields.iter().map(|f| column_ddl(inner, f, false)).collect();
        for r in m.relations.values() {
            if fk_on_self(m, r) && order.iter().position(|n| n == &r.model) <= order.iter().position(|n| n == name) {
                let target = inner.model(&r.model);
                let refs = r.references.clone().unwrap_or_else(|| target.pk().name.clone());
                cols.push(format!(
                    "FOREIGN KEY ({}) REFERENCES {} ({})",
                    inner.q(&r.foreign_key),
                    inner.q(&target.name),
                    inner.q(&refs)
                ));
            }
        }
        let ddl = format!("CREATE TABLE IF NOT EXISTS {} ({})", inner.q(&m.name), cols.join(", "));
        inner.exec(&ddl, &[])?;

        // columns added to the schema after the table was created
        let existing: HashSet<String> = existing_columns(inner, &m.name)?;
        for f in &m.fields {
            if !existing.contains(&f.name) && !existing.contains(&f.name.to_lowercase()) {
                let sql = format!("ALTER TABLE {} ADD COLUMN {}", inner.q(&m.name), column_ddl(inner, f, true));
                inner.exec(&sql, &[])?;
            }
        }
    }
    Ok(())
}

fn existing_columns(inner: &Inner, table: &str) -> Result<HashSet<String>, String> {
    let rows = match inner.dialect {
        Dialect::Sqlite => inner.query(&format!("PRAGMA table_info({})", inner.q(table)), &[])?,
        Dialect::Postgres => inner.query(
            "SELECT column_name::text AS name FROM information_schema.columns WHERE table_name = $1",
            &[Value::Str(table.to_string().into())],
        )?,
        Dialect::Mysql => inner.query(
            "SELECT column_name AS name FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = ?",
            &[Value::Str(table.to_string().into())],
        )?,
    };
    Ok(rows
        .iter()
        .filter_map(|r| match r {
            Value::Map(m) => m.borrow().iter().find(|(k, _)| k.eq_ignore_ascii_case("name")).map(|(_, v)| v.to_display_string()),
            _ => None,
        })
        .collect())
}

// ---------------------------------------------------------------------
// Query building
// ---------------------------------------------------------------------

/// SQL text + parameters, with the placeholder style of the dialect.
struct Sql<'a> {
    inner: &'a Inner,
    params: Vec<Value>,
}

impl<'a> Sql<'a> {
    fn new(inner: &'a Inner) -> Self {
        Sql { inner, params: Vec::new() }
    }
    fn ph(&mut self, v: Value) -> String {
        self.params.push(v);
        match self.inner.dialect {
            Dialect::Postgres => format!("${}", self.params.len()),
            _ => "?".into(),
        }
    }
}

/// Converts a Fogu value to what is stored for a field.
fn to_db(f: &Field, v: &Value) -> Result<Value, String> {
    if matches!(v, Value::Nil) {
        return Ok(Value::Nil);
    }
    let bad = || format!("field '{}' ({:?}) cannot store '{}'", f.name, f.ty, v.type_name());
    Ok(match f.ty {
        FieldType::Int | FieldType::BigInt => match v {
            Value::Int(_) => v.clone(),
            _ => return Err(bad()),
        },
        FieldType::Float => match v {
            Value::Float(_) => v.clone(),
            Value::Int(n) => Value::Float(*n as f64),
            _ => return Err(bad()),
        },
        FieldType::Bool => match v {
            Value::Bool(_) => v.clone(),
            _ => return Err(bad()),
        },
        FieldType::String => match v {
            Value::Str(_) => v.clone(),
            _ => return Err(bad()),
        },
        FieldType::DateTime => match v {
            Value::Str(_) => v.clone(),
            Value::Int(ms) => Value::Str(iso(*ms).into()),
            _ => return Err(bad()),
        },
        FieldType::Json => Value::Str(crate::native::json::stringify_value(v).into()),
    })
}

/// Converts a stored value back (0/1 → bool, JSON text → value…).
fn from_db(f: &Field, v: Value) -> Value {
    match (f.ty, v) {
        (FieldType::Bool, Value::Int(n)) => Value::Bool(n != 0),
        (FieldType::Float, Value::Int(n)) => Value::Float(n as f64),
        (FieldType::Json, Value::Str(s)) => crate::native::json::parse_value(&s).unwrap_or(Value::Str(s)),
        (_, v) => v,
    }
}

fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        m,
        d,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        millis
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `where` → SQL condition (empty string = no condition).
fn where_sql(sql: &mut Sql, m: &Model, w: &Value) -> Result<String, String> {
    let w = match w {
        Value::Nil => return Ok(String::new()),
        Value::Map(w) => w.borrow().clone(),
        other => return Err(format!("where must be a map, got '{}'", other.type_name())),
    };
    let mut keys: Vec<&String> = w.keys().collect();
    keys.sort();
    let mut parts = Vec::new();
    for k in keys {
        let v = &w[k];
        match k.as_str() {
            "AND" | "OR" => {
                let items = match v {
                    Value::List(l) => l.borrow().clone(),
                    other => vec![other.clone()],
                };
                let mut sub = Vec::new();
                for it in &items {
                    let c = where_sql(sql, m, it)?;
                    if !c.is_empty() {
                        sub.push(format!("({})", c));
                    }
                }
                if !sub.is_empty() {
                    parts.push(format!("({})", sub.join(if k == "AND" { " AND " } else { " OR " })));
                } else if k == "OR" {
                    parts.push("1 = 0".into());
                }
            }
            "NOT" => {
                let c = where_sql(sql, m, v)?;
                if !c.is_empty() {
                    parts.push(format!("NOT ({})", c));
                }
            }
            field => {
                let f = m
                    .field(field)
                    .ok_or_else(|| format!("unknown field '{}' on model {}", field, m.name))?;
                parts.push(field_cond(sql, f, v)?);
            }
        }
    }
    Ok(parts.join(" AND "))
}

fn field_cond(sql: &mut Sql, f: &Field, v: &Value) -> Result<String, String> {
    let col = sql.inner.q(&f.name);
    let ops = match v {
        Value::Map(ops) => ops.borrow().clone(),
        Value::Nil => return Ok(format!("{} IS NULL", col)),
        other => {
            let p = sql.ph(to_db(f, other)?);
            return Ok(format!("{} = {}", col, p));
        }
    };
    let insensitive = matches!(ops.get("mode"), Some(Value::Str(s)) if s == "insensitive");
    let mut keys: Vec<&String> = ops.keys().collect();
    keys.sort();
    let mut parts = Vec::new();
    for op in keys {
        let x = &ops[op];
        let c = match op.as_str() {
            "mode" => continue,
            "equals" if matches!(x, Value::Nil) => format!("{} IS NULL", col),
            "equals" => format!("{} = {}", col, sql.ph(to_db(f, x)?)),
            "not" => match x {
                Value::Nil => format!("{} IS NOT NULL", col),
                Value::Map(_) => format!("NOT ({})", field_cond(sql, f, x)?),
                _ => format!("({} <> {} OR {} IS NULL)", col, sql.ph(to_db(f, x)?), col),
            },
            "lt" | "lte" | "gt" | "gte" => {
                let sym = match op.as_str() {
                    "lt" => "<",
                    "lte" => "<=",
                    "gt" => ">",
                    _ => ">=",
                };
                format!("{} {} {}", col, sym, sql.ph(to_db(f, x)?))
            }
            "in" | "notIn" => {
                let items = match x {
                    Value::List(l) => l.borrow().clone(),
                    other => return Err(format!("'{}' expects a list, got '{}'", op, other.type_name())),
                };
                if items.is_empty() {
                    if op == "in" { "1 = 0".into() } else { "1 = 1".into() }
                } else {
                    let mut phs = Vec::new();
                    for it in &items {
                        phs.push(sql.ph(to_db(f, it)?));
                    }
                    format!("{} {} ({})", col, if op == "in" { "IN" } else { "NOT IN" }, phs.join(", "))
                }
            }
            "contains" | "startsWith" | "endsWith" => {
                let s = match x {
                    Value::Str(s) => s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"),
                    other => return Err(format!("'{}' expects str, got '{}'", op, other.type_name())),
                };
                let pat = match op.as_str() {
                    "contains" => format!("%{}%", s),
                    "startsWith" => format!("{}%", s),
                    _ => format!("%{}", s),
                };
                let p = sql.ph(Value::Str(pat.into()));
                if insensitive {
                    format!("LOWER({}) LIKE LOWER({}) ESCAPE '\\'", col, p)
                } else {
                    format!("{} LIKE {} ESCAPE '\\'", col, p)
                }
            }
            other => {
                return Err(format!(
                    "unknown filter '{}' on '{}' (equals not in notIn lt lte gt gte contains startsWith endsWith mode)",
                    other, f.name
                ))
            }
        };
        parts.push(c);
    }
    Ok(if parts.is_empty() { "1 = 1".into() } else { parts.join(" AND ") })
}

fn order_sql(inner: &Inner, m: &Model, o: Option<&Value>) -> Result<String, String> {
    let items = match o {
        None | Some(Value::Nil) => return Ok(String::new()),
        Some(Value::List(l)) => l.borrow().clone(),
        Some(other) => vec![other.clone()],
    };
    let mut parts = Vec::new();
    for it in items {
        let mm = as_map(&it, "orderBy")?;
        let mut keys: Vec<(String, Value)> = mm.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, dir) in keys {
            if m.field(&k).is_none() {
                return Err(format!("orderBy: unknown field '{}' on model {}", k, m.name));
            }
            let d = match dir.to_display_string().to_lowercase().as_str() {
                "asc" => "ASC",
                "desc" => "DESC",
                other => return Err(format!("orderBy: '{}' must be \"asc\" or \"desc\"", other)),
            };
            parts.push(format!("{} {}", inner.q(&k), d));
        }
    }
    Ok(if parts.is_empty() { String::new() } else { format!(" ORDER BY {}", parts.join(", ")) })
}

// ---------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------

pub struct ModelValue {
    inner: Rc<Inner>,
    model: String,
}

/// The arguments map of a model method (`{}` when omitted).
fn args_map(args: &[Value], what: &str) -> Result<HashMap<String, Value>, String> {
    max_args(args, 1, what)?;
    match args.first() {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{} expects a map of options, got '{}'", what, other.type_name())),
    }
}

fn record(m: &Model, row: Value) -> Value {
    let Value::Map(r) = row else { return row };
    let r = r.borrow();
    let pairs: HashMap<String, Value> = m
        .fields
        .iter()
        .map(|f| {
            let v = r
                .get(&f.name)
                .or_else(|| r.iter().find(|(k, _)| k.eq_ignore_ascii_case(&f.name)).map(|(_, v)| v))
                .cloned()
                .unwrap_or(Value::Nil);
            (f.name.clone(), from_db(f, v))
        })
        .collect();
    Value::Map(Rc::new(RefCell::new(pairs)))
}

fn field_of(rec: &Value, name: &str) -> Value {
    match rec {
        Value::Map(m) => m.borrow().get(name).cloned().unwrap_or(Value::Nil),
        _ => Value::Nil,
    }
}

impl ModelValue {
    fn m(&self) -> &Model {
        self.inner.model(&self.model)
    }

    /// SELECT … with where / orderBy / take / skip, then includes and select.
    /// `extra` restricts the rows to `field IN (values)` (relation loading);
    /// those rows keep every field until they are attached to their parents.
    fn find_many(&self, model: &str, opts: &HashMap<String, Value>, extra: Option<(String, Vec<Value>)>) -> Result<Vec<Value>, String> {
        let nested = extra.is_some();
        let inner = &*self.inner;
        let m = inner.model(model);
        let mut sql = Sql::new(inner);
        let mut cond = where_sql(&mut sql, m, opts.get("where").unwrap_or(&Value::Nil))?;
        if let Some((fk, values)) = extra {
            let f = m.field(&fk).unwrap();
            let mut phs = Vec::new();
            for v in &values {
                phs.push(sql.ph(to_db(f, v)?));
            }
            let c = if phs.is_empty() { "1 = 0".to_string() } else { format!("{} IN ({})", inner.q(&fk), phs.join(", ")) };
            cond = if cond.is_empty() { c } else { format!("({}) AND {}", cond, c) };
        }
        let mut text = format!("SELECT * FROM {}", inner.q(&m.name));
        if !cond.is_empty() {
            text.push_str(&format!(" WHERE {}", cond));
        }
        text.push_str(&order_sql(inner, m, opts.get("orderBy"))?);
        let int_opt = |k: &str| -> Result<Option<i64>, String> {
            match opts.get(k) {
                None | Some(Value::Nil) => Ok(None),
                Some(Value::Int(n)) if *n >= 0 => Ok(Some(*n)),
                Some(other) => Err(format!("{} must be an int >= 0, got {}", k, other.to_display_string())),
            }
        };
        let (take, skip) = (int_opt("take")?, int_opt("skip")?);
        if take.is_some() || skip.is_some() {
            let t = take.unwrap_or(match inner.dialect {
                Dialect::Mysql => i64::MAX,
                _ => -1,
            });
            if inner.dialect == Dialect::Postgres && take.is_none() {
                text.push_str(&format!(" OFFSET {}", skip.unwrap_or(0)));
            } else {
                text.push_str(&format!(" LIMIT {} OFFSET {}", t, skip.unwrap_or(0)));
            }
        }
        let rows: Vec<Value> = inner.query(&text, &sql.params)?.into_iter().map(|r| record(m, r)).collect();
        let rows = self.with_relations(model, rows, opts)?;
        if !nested {
            apply_select(self.inner.model(model), &rows, opts)?;
        }
        Ok(rows)
    }

    /// Loads `include` (and the relation keys inside `select`).
    fn with_relations(&self, model: &str, rows: Vec<Value>, opts: &HashMap<String, Value>) -> Result<Vec<Value>, String> {
        let m = self.inner.model(model).clone();
        let mut wanted: Vec<(String, Value)> = Vec::new();
        for key in ["include", "select"] {
            if let Some(Value::Map(inc)) = opts.get(key) {
                for (rname, spec) in inc.borrow().iter() {
                    if m.relations.contains_key(rname) && spec.is_truthy() {
                        wanted.push((rname.clone(), spec.clone()));
                    } else if key == "include" && spec.is_truthy() {
                        return Err(format!("include: '{}' is not a relation of model {}", rname, m.name));
                    }
                }
            }
        }
        wanted.sort_by(|a, b| a.0.cmp(&b.0));
        for (rname, spec) in wanted {
            let r = m.relations[&rname].clone();
            let sub: HashMap<String, Value> = match &spec {
                Value::Map(s) => s.borrow().clone(),
                _ => HashMap::new(),
            };
            let target = self.inner.model(&r.model).clone();
            if fk_on_self(&m, &r) {
                // many-to-one: this.fk → target.references
                let refs = r.references.clone().unwrap_or_else(|| target.pk().name.clone());
                let keys: Vec<Value> = dedup(rows.iter().map(|x| field_of(x, &r.foreign_key)).filter(|v| !matches!(v, Value::Nil)));
                let found = self.find_many(&r.model, &sub, Some((refs.clone(), keys)))?;
                for row in &rows {
                    let fk = field_of(row, &r.foreign_key);
                    let hit = found.iter().find(|t| values_equal(&field_of(t, &refs), &fk)).cloned().unwrap_or(Value::Nil);
                    set(row, &rname, hit);
                }
                apply_select(&target, &found, &sub)?;
            } else {
                // one-to-many / one-to-one: target.fk → this.references
                let refs = r.references.clone().unwrap_or_else(|| m.pk().name.clone());
                let keys: Vec<Value> = dedup(rows.iter().map(|x| field_of(x, &refs)).filter(|v| !matches!(v, Value::Nil)));
                let found = self.find_many(&r.model, &sub, Some((r.foreign_key.clone(), keys)))?;
                for row in &rows {
                    let key = field_of(row, &refs);
                    let mine: Vec<Value> = found.iter().filter(|t| values_equal(&field_of(t, &r.foreign_key), &key)).cloned().collect();
                    let v = if r.kind == RelKind::OneToMany {
                        list(mine)
                    } else {
                        mine.into_iter().next().unwrap_or(Value::Nil)
                    };
                    set(row, &rname, v);
                }
                apply_select(&target, &found, &sub)?;
            }
        }
        Ok(rows)
    }

    fn data(&self, opts: &HashMap<String, Value>, key: &str) -> Result<HashMap<String, Value>, String> {
        match opts.get(key) {
            Some(Value::Map(d)) => Ok(d.borrow().clone()),
            _ => Err(format!("{}.{}: '{}' must be a map", accessor(&self.model), "create", key)),
        }
    }

    fn insert(&self, data: HashMap<String, Value>) -> Result<Value, String> {
        let inner = &*self.inner;
        let m = self.m();
        for k in data.keys() {
            if m.field(k).is_none() {
                return Err(format!("unknown field '{}' on model {}", k, m.name));
            }
        }
        let mut cols = Vec::new();
        let mut sql = Sql::new(inner);
        let mut phs = Vec::new();
        for f in &m.fields {
            let v = match data.get(&f.name) {
                Some(v) if !matches!(v, Value::Nil) => v.clone(),
                _ => match &f.default {
                    Some(Value::Str(s)) if s == "now" => Value::Str(iso(now_ms()).into()),
                    Some(Value::Str(s)) if s == "uuid" || s == "cuid" => Value::Str(uuid::Uuid::new_v4().to_string().into()),
                    Some(d) => d.clone(),
                    None if f.autoincrement => continue,
                    None if f.required => {
                        return Err(format!("{}.create: missing required field '{}'", accessor(&m.name), f.name))
                    }
                    None => continue,
                },
            };
            cols.push(inner.q(&f.name));
            phs.push(sql.ph(to_db(f, &v)?));
        }
        let table = inner.q(&m.name);
        let head = if cols.is_empty() {
            match inner.dialect {
                Dialect::Mysql => format!("INSERT INTO {} () VALUES ()", table),
                _ => format!("INSERT INTO {} DEFAULT VALUES", table),
            }
        } else {
            format!("INSERT INTO {} ({}) VALUES ({})", table, cols.join(", "), phs.join(", "))
        };
        match inner.dialect {
            Dialect::Mysql => {
                let (_, last) = inner.exec(&head, &sql.params)?;
                let pk = m.pk();
                let key = data.get(&pk.name).cloned().filter(|v| !matches!(v, Value::Nil)).or(last.map(Value::Int)).unwrap_or(Value::Nil);
                self.by_pk(&m.name, &key)?.ok_or_else(|| io_err!("orm: the inserted row was not found"))
            }
            _ => {
                let rows = inner.query(&format!("{} RETURNING *", head), &sql.params)?;
                rows.into_iter().next().map(|r| record(m, r)).ok_or_else(|| io_err!("orm: insert returned no row"))
            }
        }
    }

    fn by_pk(&self, model: &str, key: &Value) -> Result<Option<Value>, String> {
        let inner = &*self.inner;
        let m = inner.model(model);
        let pk = m.pk();
        let mut sql = Sql::new(inner);
        let p = sql.ph(to_db(pk, key)?);
        let text = format!("SELECT * FROM {} WHERE {} = {}", inner.q(&m.name), inner.q(&pk.name), p);
        Ok(inner.query(&text, &sql.params)?.into_iter().next().map(|r| record(m, r)))
    }

    /// SET clause, supporting `{ increment, decrement, multiply, divide, set }`.
    fn set_sql(&self, sql: &mut Sql, data: &HashMap<String, Value>) -> Result<String, String> {
        let m = self.m();
        let mut keys: Vec<&String> = data.keys().collect();
        keys.sort();
        let mut parts = Vec::new();
        for k in keys {
            let f = m.field(k).ok_or_else(|| format!("unknown field '{}' on model {}", k, m.name))?;
            let col = sql.inner.q(&f.name);
            let v = &data[k];
            let expr = match v {
                Value::Map(op) if f.ty != FieldType::Json => {
                    let op = op.borrow();
                    let (name, x) = op.iter().next().ok_or_else(|| format!("empty update operation on '{}'", k))?;
                    let p = sql.ph(to_db(f, x)?);
                    match name.as_str() {
                        "set" => p,
                        "increment" => format!("{} + {}", col, p),
                        "decrement" => format!("{} - {}", col, p),
                        "multiply" => format!("{} * {}", col, p),
                        "divide" => format!("{} / {}", col, p),
                        other => return Err(format!("unknown update operation '{}' on '{}'", other, k)),
                    }
                }
                other => sql.ph(to_db(f, other)?),
            };
            parts.push(format!("{} = {}", col, expr));
        }
        if parts.is_empty() {
            return Err("update: 'data' is empty".into());
        }
        Ok(parts.join(", "))
    }

    fn first_pk(&self, opts: &HashMap<String, Value>) -> Result<Option<Value>, String> {
        let mut o = HashMap::new();
        o.insert("where".to_string(), opts.get("where").cloned().unwrap_or(Value::Nil));
        o.insert("take".to_string(), Value::Int(1));
        let rows = self.find_many(&self.model, &o, None)?;
        Ok(rows.first().map(|r| field_of(r, &self.m().pk().name)))
    }

    fn update_one(&self, pk: &Value, data: &HashMap<String, Value>) -> Result<(), String> {
        let inner = &*self.inner;
        let m = self.m();
        let mut sql = Sql::new(inner);
        let set = self.set_sql(&mut sql, data)?;
        let p = sql.ph(to_db(m.pk(), pk)?);
        let text = format!("UPDATE {} SET {} WHERE {} = {}", inner.q(&m.name), set, inner.q(&m.pk().name), p);
        inner.exec(&text, &sql.params).map(|_| ())
    }

    fn one_with(&self, rec: Value, opts: &HashMap<String, Value>) -> Result<Value, String> {
        let rows = self.with_relations(&self.model, vec![rec], opts)?;
        apply_select(self.m(), &rows, opts)?;
        Ok(rows.into_iter().next().unwrap_or(Value::Nil))
    }
}

/// `select`: keeps only the chosen fields (and the loaded relations).
fn apply_select(m: &Model, rows: &[Value], opts: &HashMap<String, Value>) -> Result<(), String> {
    if let Some(Value::Map(sel)) = opts.get("select") {
        let keep: HashSet<String> = sel.borrow().iter().filter(|(_, v)| v.is_truthy()).map(|(k, _)| k.clone()).collect();
        for k in &keep {
            if m.field(k).is_none() && !m.relations.contains_key(k) {
                return Err(format!("select: unknown field '{}' on model {}", k, m.name));
            }
        }
        for row in rows {
            if let Value::Map(r) = row {
                r.borrow_mut().retain(|k, _| keep.contains(k));
            }
        }
    }
    Ok(())
}

fn dedup(it: impl Iterator<Item = Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for v in it {
        if !out.iter().any(|x| values_equal(x, &v)) {
            out.push(v);
        }
    }
    out
}

fn set(row: &Value, k: &str, v: Value) {
    if let Value::Map(m) = row {
        m.borrow_mut().insert(k.to_string(), v);
    }
}

fn count_map(n: u64) -> Value {
    map(vec![("count", Value::Int(n as i64))])
}

impl FoguObject for ModelValue {
    fn name(&self) -> &str {
        "OrmModel"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "name").then(|| Value::Str(self.model.clone().into()))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let what = format!("{}.{}", accessor(&self.model), method);
        let opts = args_map(args, &what)?;
        let not_found = || io_err!("{}: record not found", what);
        match method {
            "findMany" => self.find_many(&self.model.clone(), &opts, None).map(list),
            "findFirst" | "findUnique" => {
                let mut o = opts.clone();
                o.insert("take".into(), Value::Int(1));
                Ok(self.find_many(&self.model.clone(), &o, None)?.into_iter().next().unwrap_or(Value::Nil))
            }
            "count" => {
                let inner = &*self.inner;
                let m = self.m();
                let mut sql = Sql::new(inner);
                let cond = where_sql(&mut sql, m, opts.get("where").unwrap_or(&Value::Nil))?;
                let mut text = format!("SELECT COUNT(*) AS n FROM {}", inner.q(&m.name));
                if !cond.is_empty() {
                    text.push_str(&format!(" WHERE {}", cond));
                }
                let rows = inner.query(&text, &sql.params)?;
                Ok(rows.first().map(|r| field_of(r, "n")).unwrap_or(Value::Int(0)))
            }
            "create" => {
                let rec = self.insert(self.data(&opts, "data")?)?;
                self.one_with(rec, &opts)
            }
            "createMany" => {
                let items = match opts.get("data") {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => return Err(format!("{}: 'data' must be a list of maps", what)),
                };
                for it in &items {
                    self.insert(as_map(it, "data item")?.borrow().clone())?;
                }
                Ok(count_map(items.len() as u64))
            }
            "update" => {
                let data = self.data(&opts, "data")?;
                let pk = self.first_pk(&opts)?.ok_or_else(not_found)?;
                self.update_one(&pk, &data)?;
                let rec = self.by_pk(&self.model, &pk)?.ok_or_else(not_found)?;
                self.one_with(rec, &opts)
            }
            "updateMany" => {
                let data = self.data(&opts, "data")?;
                let inner = &*self.inner;
                let m = self.m();
                let mut sql = Sql::new(inner);
                let set = self.set_sql(&mut sql, &data)?;
                let cond = where_sql(&mut sql, m, opts.get("where").unwrap_or(&Value::Nil))?;
                let mut text = format!("UPDATE {} SET {}", inner.q(&m.name), set);
                if !cond.is_empty() {
                    text.push_str(&format!(" WHERE {}", cond));
                }
                inner.exec(&text, &sql.params).map(|(n, _)| count_map(n))
            }
            "upsert" => {
                let rec = match self.first_pk(&opts)? {
                    Some(pk) => {
                        let data = self.data(&opts, "update")?;
                        if !data.is_empty() {
                            self.update_one(&pk, &data)?;
                        }
                        self.by_pk(&self.model, &pk)?.ok_or_else(not_found)?
                    }
                    None => self.insert(self.data(&opts, "create")?)?,
                };
                self.one_with(rec, &opts)
            }
            "delete" => {
                let pk = self.first_pk(&opts)?.ok_or_else(not_found)?;
                let rec = self.by_pk(&self.model, &pk)?.ok_or_else(not_found)?;
                let rec = self.one_with(rec, &opts)?;
                let inner = &*self.inner;
                let m = self.m();
                let mut sql = Sql::new(inner);
                let p = sql.ph(to_db(m.pk(), &pk)?);
                inner.exec(
                    &format!("DELETE FROM {} WHERE {} = {}", inner.q(&m.name), inner.q(&m.pk().name), p),
                    &sql.params,
                )?;
                Ok(rec)
            }
            "deleteMany" => {
                let inner = &*self.inner;
                let m = self.m();
                let mut sql = Sql::new(inner);
                let cond = where_sql(&mut sql, m, opts.get("where").unwrap_or(&Value::Nil))?;
                let mut text = format!("DELETE FROM {}", inner.q(&m.name));
                if !cond.is_empty() {
                    text.push_str(&format!(" WHERE {}", cond));
                }
                inner.exec(&text, &sql.params).map(|(n, _)| count_map(n))
            }
            other => Err(format!(
                "method '{}' does not exist on model {} (create createMany findMany findFirst findUnique update updateMany upsert delete deleteMany count)",
                other, self.model
            )),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "create", "createMany", "findMany", "findFirst", "findUnique", "update", "updateMany", "upsert",
            "delete", "deleteMany", "count",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{accessor, iso};

    #[test]
    fn helpers() {
        assert_eq!(accessor("User"), "user");
        assert_eq!(accessor("BlogPost"), "blogPost");
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
    }
}
