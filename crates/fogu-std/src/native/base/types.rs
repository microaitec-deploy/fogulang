//! `fogu:types` — runtime type checks, lenient conversions and schemas.
//!
//! ```fogu
//! import types from "fogu:types"
//!
//! print(types.isInt(3), types.of(time.Date()))          // true Date
//! const [n, err] = types.toInt("42")                     // 42 (an error value for "x")
//! const User = { name: "str", "age?": { "$type": "int", min: 0 }, tags: "[str]" }
//! const [u, bad] = types.check(json.parse(body)?, User)  // u, or an error with the path
//! const q = types.coerce({ page: "2" }, { page: "int" })? // { page: 2 }
//! ```
//!
//! A **spec** is one of:
//! - a type name: `any nil bool int float number str list map buffer fn error duration range
//!   struct object chan`, or the name of a struct type / native object (`"User"`, `"Date"`);
//!   `"T?"` also accepts nil, `"A|B"` either, `"[T]"` a list of T, `"{T}"` a map with T values;
//! - a list `[spec]`: a list whose elements match `spec`;
//! - a map `{ field: spec }`: a map (or struct) with those fields; `"field?"` is optional;
//! - a map with `"$type"`: constraints on a value — `min max` (numbers, or lengths of str/list/map),
//!   `minLen maxLen`, `pattern` (a regex the whole text must match), `oneOf` (allowed values),
//!   `nullable`, `fields`/`items` (as above), `strict` (no unknown fields).

use std::cell::RefCell;
use std::rc::Rc;
use ahash::AHashMap as HashMap;

use crate::runtime::value::{ModuleValue, StructInstance, Value};

use crate::native::util::{list, max_args, nfn_var};

type R = Result<Value, String>;

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    // isInt isStr … and toInt toFloat … (see `named!` at the end)
    install(&mut e);
    e.insert("of".into(), nfn_var("of", 1, |a| {
        max_args(a, 1, "types.of")?;
        Ok(Value::Str(of(&a[0]).into()))
    }));
    e.insert("is".into(), nfn_var("is", 2, |a| {
        max_args(a, 2, "types.is")?;
        let mut errs = Vec::new();
        check(&a[0], &a[1], "", &mut errs, false, &mut None)?;
        Ok(Value::Bool(errs.is_empty()))
    }));
    e.insert("check".into(), nfn_var("check", 2, |a| {
        max_args(a, 2, "types.check")?;
        let mut errs = Vec::new();
        check(&a[0], &a[1], "", &mut errs, false, &mut None)?;
        match errs.first() {
            None => Ok(a[0].clone()),
            Some(e) => Err(io_err!("{}", e)),
        }
    }));
    e.insert("validate".into(), nfn_var("validate", 2, |a| {
        max_args(a, 2, "types.validate")?;
        let mut errs = Vec::new();
        check(&a[0], &a[1], "", &mut errs, true, &mut None)?;
        Ok(list(errs.into_iter().map(|e| Value::Str(e.into())).collect()))
    }));
    e.insert("assert".into(), nfn_var("assert", 2, |a| {
        max_args(a, 2, "types.assert")?;
        let mut errs = Vec::new();
        check(&a[0], &a[1], "", &mut errs, false, &mut None)?;
        match errs.first() {
            None => Ok(a[0].clone()),
            Some(e) => Err(format!("types.assert: {}", e)),
        }
    }));
    e.insert("coerce".into(), nfn_var("coerce", 2, |a| {
        max_args(a, 2, "types.coerce")?;
        let mut errs = Vec::new();
        let mut out = Some(Value::Nil);
        check(&a[0], &a[1], "", &mut errs, false, &mut out)?;
        match errs.first() {
            None => Ok(out.unwrap_or(Value::Nil)),
            Some(e) => Err(io_err!("{}", e)),
        }
    }));
    e.insert("equal".into(), nfn_var("equal", 2, |a| {
        max_args(a, 2, "types.equal")?;
        Ok(Value::Bool(deep_equal(&a[0], &a[1])))
    }));
    e.insert("clone".into(), nfn_var("clone", 1, |a| {
        max_args(a, 1, "types.clone")?;
        Ok(deep_clone(&a[0]))
    }));
    ModuleValue {
        name: "fogu:types".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Predicates and conversions
// ---------------------------------------------------------------------

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Nil => true,
        Value::Str(s) => s.is_empty(),
        Value::List(l) => l.borrow().is_empty(),
        Value::Map(m) => m.borrow().is_empty(),
        Value::Buffer(b) => b.borrow().is_empty(),
        Value::Range(a, b) => a >= b,
        _ => false,
    }
}

/// `types.of`: the struct type or native object name, else `typeof`.
pub fn of(v: &Value) -> String {
    match v {
        Value::Struct(s) => s.borrow().type_name.clone(),
        Value::Object(o) => o.name(),
        Value::StructType(t) => t.decl.name.clone(),
        other => other.type_name().to_string(),
    }
}

fn to_int(v: &Value) -> Option<Value> {
    Some(Value::Int(match v {
        Value::Int(n) => *n,
        Value::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.2e18 => *f as i64,
        Value::Bool(b) => i64::from(*b),
        Value::Str(s) => {
            let t = s.trim().replace('_', "");
            if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                i64::from_str_radix(h, 16).ok()?
            } else if let Some(b) = t.strip_prefix("0b") {
                i64::from_str_radix(b, 2).ok()?
            } else {
                match t.parse::<i64>() {
                    Ok(n) => n,
                    Err(_) => {
                        let f = t.parse::<f64>().ok()?;
                        if f.fract() != 0.0 || !f.is_finite() {
                            return None;
                        }
                        f as i64
                    }
                }
            }
        }
        _ => return None,
    }))
}

fn to_float(v: &Value) -> Option<Value> {
    Some(Value::Float(match v {
        Value::Int(n) => *n as f64,
        Value::Float(f) => *f,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Str(s) => s.trim().replace('_', "").parse::<f64>().ok()?,
        _ => return None,
    }))
}

fn to_bool(v: &Value) -> Option<Value> {
    Some(Value::Bool(match v {
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::Nil => false,
        Value::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" | "on" => true,
            "false" | "0" | "no" | "n" | "off" | "" => false,
            _ => return None,
        },
        _ => return None,
    }))
}

fn to_list(v: &Value) -> Option<Value> {
    Some(match v {
        Value::List(_) => v.clone(),
        Value::Nil => list(Vec::new()),
        Value::Str(s) => list(s.chars().map(|c| Value::Str(c.to_string().into())).collect()),
        Value::Range(a, b) => list((*a..*b).map(Value::Int).collect()),
        Value::Buffer(b) => list(b.borrow().iter().map(|x| Value::Int(*x as i64)).collect()),
        Value::Map(m) => {
            let m = m.borrow();
            list(m.iter().map(|(k, v)| list(vec![Value::Str(k.clone().into()), v.clone()])).collect())
        }
        other => list(vec![other.clone()]),
    })
}

fn to_map(v: &Value) -> Option<Value> {
    Some(match v {
        Value::Map(_) => v.clone(),
        Value::Nil => new_map(HashMap::new()),
        Value::Struct(s) => {
            let s = s.borrow();
            let mut keys: Vec<&String> = s.fields.keys().collect();
            keys.sort();
            new_map(keys.into_iter().map(|k| (k.clone(), s.fields[k].clone())).collect())
        }
        Value::List(l) => {
            let mut out = HashMap::new();
            for p in l.borrow().iter() {
                let Value::List(pair) = p else { return None };
                let pair = pair.borrow();
                match pair.as_slice() {
                    [k, v] => {
                        out.insert(k.to_display_string(), v.clone());
                    }
                    _ => return None,
                }
            }
            new_map(out)
        }
        _ => return None,
    })
}

fn new_map(m: HashMap<String, Value>) -> Value {
    Value::Map(Rc::new(RefCell::new(m.into_iter().collect())))
}

// ---------------------------------------------------------------------
// Deep equality and copies
// ---------------------------------------------------------------------

fn deep_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::List(x), Value::List(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| deep_equal(a, b))
        }
        (Value::Map(x), Value::Map(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| deep_equal(v, w)))
        }
        (Value::Struct(x), Value::Struct(y)) => {
            if Rc::ptr_eq(x, y) {
                return true;
            }
            let (x, y) = (x.borrow(), y.borrow());
            x.type_name == y.type_name
                && x.fields.len() == y.fields.len()
                && x.fields.iter().all(|(k, v)| y.fields.get(k).is_some_and(|w| deep_equal(v, w)))
        }
        _ => crate::runtime::interpreter::values_equal(a, b),
    }
}

fn deep_clone(v: &Value) -> Value {
    match v {
        Value::List(l) => list(l.borrow().iter().map(deep_clone).collect()),
        Value::Map(m) => Value::Map(Rc::new(RefCell::new(m.borrow().iter().map(|(k, v)| (k.clone(), deep_clone(v))).collect()))),
        Value::Buffer(b) => Value::buffer(b.borrow().clone()),
        Value::Struct(s) => {
            let s = s.borrow();
            Value::Struct(Rc::new(RefCell::new(StructInstance {
                type_name: s.type_name.clone(),
                module: s.module.clone(),
                fields: s.fields.iter().map(|(k, v)| (k.clone(), deep_clone(v))).collect(),
                embeds: s.embeds.clone(),
                methods: s.methods.clone(),
            })))
        }
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------

fn at(path: &str) -> String {
    if path.is_empty() { "value".into() } else { path.to_string() }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_string() } else { format!("{}.{}", path, key) }
}

fn describe(v: &Value) -> String {
    match v {
        Value::Str(s) if s.chars().count() <= 20 => format!("str \"{}\"", s),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => format!("{} {}", v.type_name(), v.to_display_string()),
        other => of(other),
    }
}

/// Checks `v` against `spec`, adding messages to `errs` (all of them when
/// `all`, else stopping at the first). With `out`, builds the coerced value
/// (texts read as the numbers or bools the spec asks for). A malformed spec is
/// a programming error (`Err`).
fn check(v: &Value, spec: &Value, path: &str, errs: &mut Vec<String>, all: bool, out: &mut Option<Value>) -> Result<(), String> {
    if !all && !errs.is_empty() {
        return Ok(());
    }
    match spec {
        Value::Str(s) => check_name(v, s.trim(), path, errs, all, out),
        Value::List(l) => {
            let l = l.borrow();
            let [item] = l.as_slice() else {
                return Err(format!("types: a list spec has one element (the item spec), got {}", l.len()));
            };
            check_list(v, item, path, errs, all, out)
        }
        Value::Map(m) => {
            let m = m.borrow();
            if m.contains_key("$type") {
                check_constrained(v, &m, path, errs, all, out)
            } else {
                check_fields(v, &m, false, path, errs, all, out)
            }
        }
        Value::StructType(t) => {
            if !matches!(v, Value::Struct(s) if s.borrow().type_name == t.decl.name) {
                errs.push(format!("{}: expected {}, got {}", at(path), t.decl.name, describe(v)));
            } else if let Some(o) = out {
                *o = v.clone();
            }
            Ok(())
        }
        other => Err(format!("types: invalid spec of type '{}' (a type name, [spec] or {{ field: spec }})", other.type_name())),
    }
}

fn check_name(v: &Value, s: &str, path: &str, errs: &mut Vec<String>, all: bool, out: &mut Option<Value>) -> Result<(), String> {
    if let Some(inner) = s.strip_suffix('?') {
        if matches!(v, Value::Nil) {
            if let Some(o) = out {
                *o = Value::Nil;
            }
            return Ok(());
        }
        return check_name(v, inner, path, errs, all, out);
    }
    if s.contains('|') && !s.starts_with('[') && !s.starts_with('{') {
        for alt in s.split('|') {
            let mut e = Vec::new();
            let mut o = out.as_ref().map(|_| Value::Nil);
            check_name(v, alt.trim(), path, &mut e, false, &mut o)?;
            if e.is_empty() {
                if let (Some(dst), Some(o)) = (out.as_mut(), o) {
                    *dst = o;
                }
                return Ok(());
            }
        }
        errs.push(format!("{}: expected {}, got {}", at(path), s, describe(v)));
        return Ok(());
    }
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return check_list(v, &Value::Str(inner.trim().into()), path, errs, all, out);
    }
    if let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        let Value::Map(m) = v else {
            errs.push(format!("{}: expected map, got {}", at(path), describe(v)));
            return Ok(());
        };
        let item = Value::Str(inner.trim().into());
        let mut keys: Vec<String> = m.borrow().keys().cloned().collect();
        keys.sort();
        let mut built = HashMap::new();
        for k in keys {
            let x = m.borrow()[&k].clone();
            let mut o = out.as_ref().map(|_| Value::Nil);
            check(&x, &item, &join(path, &k), errs, all, &mut o)?;
            built.insert(k, o.unwrap_or(x));
        }
        if let Some(o) = out {
            *o = new_map(built);
        }
        return Ok(());
    }
    let coerced = |v: &Value| -> Option<Value> {
        let Value::Str(_) = v else { return None };
        out.as_ref()?;
        match s {
            "int" => to_int(v),
            "float" | "number" => to_float(v).map(|f| match (s, to_int(v)) {
                ("number", Some(i)) => i,
                _ => f,
            }),
            "bool" => to_bool(v),
            _ => None,
        }
    };
    let ok = match s {
        "any" => true,
        "nil" => matches!(v, Value::Nil),
        "bool" => matches!(v, Value::Bool(_)),
        "int" => matches!(v, Value::Int(_)),
        "float" => matches!(v, Value::Float(_) | Value::Int(_)),
        "number" => matches!(v, Value::Int(_) | Value::Float(_)),
        "str" => matches!(v, Value::Str(_)),
        "list" => matches!(v, Value::List(_)),
        "map" => matches!(v, Value::Map(_)),
        "buffer" => matches!(v, Value::Buffer(_)),
        "fn" => v.type_name() == "fn",
        "error" => matches!(v, Value::Error(_)),
        "duration" => matches!(v, Value::Duration(_)),
        "range" => matches!(v, Value::Range(..)),
        "struct" => matches!(v, Value::Struct(_)),
        "object" => matches!(v, Value::Object(_)),
        "chan" => matches!(v, Value::Chan(_)),
        "" => return Err("types: empty type name in a spec".into()),
        name => of(v) == name,
    };
    if ok {
        if let Some(o) = out {
            // a float spec turns ints into floats
            *o = match (s, v) {
                ("float", Value::Int(n)) => Value::Float(*n as f64),
                _ => v.clone(),
            };
        }
    } else if let Some(c) = coerced(v) {
        if let Some(o) = out {
            *o = c;
        }
    } else {
        errs.push(format!("{}: expected {}, got {}", at(path), s, describe(v)));
    }
    Ok(())
}

fn check_list(v: &Value, item: &Value, path: &str, errs: &mut Vec<String>, all: bool, out: &mut Option<Value>) -> Result<(), String> {
    let Value::List(l) = v else {
        errs.push(format!("{}: expected list, got {}", at(path), describe(v)));
        return Ok(());
    };
    let items: Vec<Value> = l.borrow().clone();
    let mut built = Vec::with_capacity(items.len());
    for (i, x) in items.iter().enumerate() {
        let mut o = out.as_ref().map(|_| Value::Nil);
        check(x, item, &format!("{}[{}]", at(path), i), errs, all, &mut o)?;
        built.push(o.unwrap_or_else(|| x.clone()));
    }
    if let Some(o) = out {
        *o = list(built);
    }
    Ok(())
}

/// The fields of a map or a struct.
fn fields_of(v: &Value) -> Option<HashMap<String, Value>> {
    match v {
        Value::Map(m) => Some(m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        Value::Struct(s) => Some(s.borrow().fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        _ => None,
    }
}

fn check_fields(
    v: &Value,
    spec: &HashMap<String, Value>,
    strict: bool,
    path: &str,
    errs: &mut Vec<String>,
    all: bool,
    out: &mut Option<Value>,
) -> Result<(), String> {
    let Some(fields) = fields_of(v) else {
        errs.push(format!("{}: expected map, got {}", at(path), describe(v)));
        return Ok(());
    };
    let mut built: HashMap<String, Value> = fields.clone();
    let mut known = Vec::new();
    for (key, fspec) in sorted(spec) {
        let (name, optional) = match key.strip_suffix('?') {
            Some(n) => (n.to_string(), true),
            None => (key.clone(), false),
        };
        known.push(name.clone());
        match fields.get(&name) {
            None | Some(Value::Nil) if optional => {}
            None => errs.push(format!("{}: missing", join(path, &name))),
            Some(x) => {
                let mut o = out.as_ref().map(|_| Value::Nil);
                check(x, &fspec, &join(path, &name), errs, all, &mut o)?;
                if let Some(o) = o {
                    built.insert(name, o);
                }
            }
        }
    }
    if strict {
        let mut extra: Vec<&String> = fields.keys().filter(|k| !known.contains(k)).collect();
        extra.sort();
        for k in extra {
            errs.push(format!("{}: unknown field", join(path, k)));
        }
    }
    if let Some(o) = out {
        *o = match v {
            Value::Struct(_) => v.clone(),
            _ => new_map(built),
        };
    }
    Ok(())
}

/// The entries of a map spec in a stable order.
fn sorted(m: &HashMap<String, Value>) -> Vec<(String, Value)> {
    let mut v: Vec<(String, Value)> = m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn check_constrained(
    v: &Value,
    m: &HashMap<String, Value>,
    path: &str,
    errs: &mut Vec<String>,
    all: bool,
    out: &mut Option<Value>,
) -> Result<(), String> {
    const KEYS: &[&str] = &["$type", "min", "max", "minLen", "maxLen", "pattern", "oneOf", "nullable", "fields", "items", "strict"];
    for k in m.keys() {
        if !KEYS.contains(&k.as_str()) {
            return Err(format!("types: unknown constraint '{}' ({})", k, KEYS.join(" ")));
        }
    }
    if matches!(v, Value::Nil) && matches!(m.get("nullable"), Some(Value::Bool(true))) {
        if let Some(o) = out {
            *o = Value::Nil;
        }
        return Ok(());
    }
    let before = errs.len();
    let strict = matches!(m.get("strict"), Some(Value::Bool(true)));
    match (m.get("fields"), m.get("items")) {
        (Some(Value::Map(f)), _) => check_fields(v, &*f.borrow(), strict, path, errs, all, out)?,
        (_, Some(item)) => check_list(v, item, path, errs, all, out)?,
        _ => {
            let t = m.get("$type").cloned().unwrap_or(Value::Str("any".into()));
            check(v, &t, path, errs, all, out)?;
        }
    }
    if errs.len() > before {
        return Ok(());
    }
    // constraints apply to the (coerced) value
    let x = out.clone().filter(|o| !matches!(o, Value::Nil)).unwrap_or_else(|| v.clone());
    let p = at(path);
    let size = match &x {
        Value::Str(s) => Some(s.chars().count() as f64),
        Value::List(l) => Some(l.borrow().len() as f64),
        Value::Map(mm) => Some(mm.borrow().len() as f64),
        Value::Buffer(b) => Some(b.borrow().len() as f64),
        _ => None,
    };
    let measure = match &x {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => size,
    };
    let num = |k: &str| -> Result<Option<f64>, String> {
        match m.get(k) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Int(n)) => Ok(Some(*n as f64)),
            Some(Value::Float(f)) => Ok(Some(*f)),
            Some(o) => Err(format!("types: '{}' must be a number, got '{}'", k, o.type_name())),
        }
    };
    let what = if size.is_some() && !matches!(x, Value::Int(_) | Value::Float(_)) { "length " } else { "" };
    if let (Some(lo), Some(mv)) = (num("min")?, measure) {
        if mv < lo {
            errs.push(format!("{}: {}must be >= {}, got {}", p, what, fmt_num(lo), fmt_num(mv)));
        }
    }
    if let (Some(hi), Some(mv)) = (num("max")?, measure) {
        if mv > hi {
            errs.push(format!("{}: {}must be <= {}, got {}", p, what, fmt_num(hi), fmt_num(mv)));
        }
    }
    if let (Some(lo), Some(n)) = (num("minLen")?, size) {
        if n < lo {
            errs.push(format!("{}: length must be >= {}, got {}", p, fmt_num(lo), fmt_num(n)));
        }
    }
    if let (Some(hi), Some(n)) = (num("maxLen")?, size) {
        if n > hi {
            errs.push(format!("{}: length must be <= {}, got {}", p, fmt_num(hi), fmt_num(n)));
        }
    }
    if let Some(pat) = m.get("pattern") {
        let Value::Str(pat) = pat else {
            return Err("types: 'pattern' must be a str (a regex)".into());
        };
        let re = regex::Regex::new(&format!("^(?:{})$", pat)).map_err(|e| format!("types: invalid pattern: {}", e))?;
        match &x {
            Value::Str(s) if re.is_match(s) => {}
            Value::Str(_) => errs.push(format!("{}: does not match /{}/", p, pat)),
            other => errs.push(format!("{}: expected str, got {}", p, describe(other))),
        }
    }
    if let Some(one) = m.get("oneOf") {
        let Value::List(l) = one else {
            return Err("types: 'oneOf' must be a list".into());
        };
        let l = l.borrow();
        if !l.iter().any(|a| deep_equal(a, &x)) {
            let opts: Vec<String> = l.iter().map(|a| a.to_display_string()).collect();
            errs.push(format!("{}: must be one of {}, got {}", p, opts.join(", "), x.to_display_string()));
        }
    }
    Ok(())
}

fn fmt_num(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { f.to_string() }
}

/// The native functions of predicates and conversions: one `fn` each (they
/// are plain function pointers, so each name gets its own wrapper).
macro_rules! named {
    ($($name:literal => $kind:ident $f:expr),* $(,)?) => {
        pub fn install(e: &mut HashMap<String, Value>) {
            $(
                {
                    fn wrapper(a: &[Value]) -> R {
                        named!(@$kind a, $name, $f)
                    }
                    e.insert($name.into(), nfn_var($name, 1, wrapper));
                }
            )*
        }
    };
    (@pred $a:ident, $name:literal, $f:expr) => {{
        max_args($a, 1, concat!("types.", $name))?;
        let f: fn(&Value) -> bool = $f;
        Ok(Value::Bool(f(&$a[0])))
    }};
    (@conv $a:ident, $name:literal, $f:expr) => {{
        max_args($a, 2, concat!("types.", $name))?;
        let f: fn(&Value) -> Option<Value> = $f;
        match (f(&$a[0]), $a.get(1)) {
            (Some(v), _) => Ok(v),
            // a default value instead of an error
            (None, Some(d)) => Ok(d.clone()),
            (None, None) => Err(io_err!("{}: cannot convert {} to {}", concat!("types.", $name), describe(&$a[0]), &$name[2..].to_lowercase())),
        }
    }};
}

named! {
    "isNil" => pred |v| matches!(v, Value::Nil),
    "isBool" => pred |v| matches!(v, Value::Bool(_)),
    "isInt" => pred |v| matches!(v, Value::Int(_)),
    "isFloat" => pred |v| matches!(v, Value::Float(_)),
    "isNumber" => pred |v| matches!(v, Value::Int(_) | Value::Float(_)),
    "isStr" => pred |v| matches!(v, Value::Str(_)),
    "isList" => pred |v| matches!(v, Value::List(_)),
    "isMap" => pred |v| matches!(v, Value::Map(_)),
    "isBuffer" => pred |v| matches!(v, Value::Buffer(_)),
    "isFn" => pred |v| v.type_name() == "fn",
    "isError" => pred |v| matches!(v, Value::Error(_)),
    "isDuration" => pred |v| matches!(v, Value::Duration(_)),
    "isRange" => pred |v| matches!(v, Value::Range(..)),
    "isStruct" => pred |v| matches!(v, Value::Struct(_)),
    "isObject" => pred |v| matches!(v, Value::Object(_)),
    "isChan" => pred |v| matches!(v, Value::Chan(_)),
    "isEmpty" => pred is_empty,
    "toInt" => conv to_int,
    "toFloat" => conv to_float,
    "toBool" => conv to_bool,
    "toStr" => conv |v| Some(Value::Str(v.to_display_string().into())),
    "toList" => conv to_list,
    "toMap" => conv to_map,
}
