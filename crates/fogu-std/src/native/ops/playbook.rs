//! `fogu:playbook` module and `fogu play` — configuration management in the
//! style of Ansible: an inventory of hosts, playbooks of idempotent tasks,
//! run over SSH (the system's OpenSSH client) or on the local machine.
//!
//! ```yaml
//! # site.yml
//! - name: Web servers
//!   hosts: web
//!   become: true
//!   vars: { app: shop, port: 8080 }
//!   tasks:
//!     - name: App folder
//!       file: { path: "/srv/{{ app }}", state: directory, mode: "0755" }
//!     - name: Config
//!       copy: { content: "port={{ port }}\n", dest: "/srv/{{ app }}/app.conf" }
//!       notify: restart app
//!     - name: Packages
//!       package: { name: [curl, nginx], state: present }
//!       when: facts.os == 'linux'
//!   handlers:
//!     - name: restart app
//!       service: { name: nginx, state: restarted }
//! ```
//!
//! ```fogu
//! import playbook from "fogu:playbook"
//! const r = playbook.run("site.yml", { inventory: "inventory.yml", check: true })?
//! print(r.ok, r.recap)
//! ```
//!
//! Modules: `shell command copy template file lineinfile package service
//! debug set_fact fail wait_for`. Task keys: `name when loop register notify
//! ignore_errors changed_when check_mode become tags`. Templates: `{{ expr }}` with
//! filters `default upper lower trim length join first last replace split int
//! float string bool to_json basename dirname`.

use std::collections::{BTreeMap, HashSet};
use ahash::AHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value as J};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::util::{map, max_args, nfn_var};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("run".into(), nfn_var("run", 1, run_fn));
    e.insert("inventory".into(), nfn_var("inventory", 1, |a| {
        max_args(a, 1, "playbook.inventory")?;
        let inv = load_inventory(&to_json(&a[0]))?;
        Ok(crate::native::util::json_to_value(&inv.describe()))
    }));
    e.insert("render".into(), nfn_var("render", 2, |a| {
        max_args(a, 2, "playbook.render")?;
        let text = crate::native::util::str_arg(a, 0, "playbook.render")?;
        render(&text, &to_json(&a[1])).map(Value::str).map_err(|e| io_err!("playbook.render: {}", e))
    }));
    ModuleValue {
        name: "fogu:playbook".into(),
        exports: std::cell::RefCell::new(e),
    }
}

fn to_json(v: &Value) -> J {
    serde_json::from_str(&crate::native::json::stringify_value(v)).unwrap_or(J::Null)
}

/// A str is a YAML/JSON file to read; anything else is the data itself.
fn load_data(v: &J, what: &str) -> Result<J, String> {
    match v {
        J::String(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| io_err!("{}: {}: {}", what, path, e))?;
            serde_yaml::from_str::<J>(&text).map_err(|e| io_err!("{}: {}: {}", what, path, e))
        }
        other => Ok(other.clone()),
    }
}

// =====================================================================
// Expressions and templates ({{ … }}, when:)
// =====================================================================

#[derive(Debug, Clone, PartialEq)]
enum Tk {
    Ident(String),
    Str(String),
    Num(f64),
    Op(String),
}

fn tokenize(src: &str) -> Result<Vec<Tk>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '\'' || ch == '"' {
            let q = ch;
            let mut s = String::new();
            i += 1;
            while i < c.len() && c[i] != q {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                }
                s.push(c[i]);
                i += 1;
            }
            if i >= c.len() {
                return Err(format!("unterminated string in '{}'", src));
            }
            i += 1;
            out.push(Tk::Str(s));
        } else if ch.is_ascii_digit() {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            let t: String = c[st..i].iter().collect();
            out.push(Tk::Num(t.parse().map_err(|_| format!("bad number '{}'", t))?));
        } else if ch.is_alphabetic() || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tk::Ident(c[st..i].iter().collect()));
        } else {
            let two: String = c[i..(i + 2).min(c.len())].iter().collect();
            let op = if ["==", "!=", "<=", ">="].contains(&two.as_str()) { two } else { ch.to_string() };
            if !["==", "!=", "<=", ">=", "<", ">", "(", ")", "[", "]", ".", ",", "|", "+", "-", "~", "*", "/", "%"].contains(&op.as_str()) {
                return Err(format!("unexpected '{}' in '{}'", op, src));
            }
            i += op.len();
            out.push(Tk::Op(op));
        }
    }
    Ok(out)
}

/// `None` is "undefined".
type V = Option<J>;

struct Parser<'a> {
    t: Vec<Tk>,
    i: usize,
    vars: &'a J,
}

fn truthy(v: &V) -> bool {
    match v {
        None | Some(J::Null) => false,
        Some(J::Bool(b)) => *b,
        Some(J::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(J::String(s)) => !s.is_empty() && !matches!(s.to_lowercase().as_str(), "false" | "no" | "0"),
        Some(J::Array(a)) => !a.is_empty(),
        Some(J::Object(o)) => !o.is_empty(),
    }
}

fn text_of(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Null => String::new(),
        other => other.to_string(),
    }
}

fn num_of(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::String(s) => s.trim().parse().ok(),
        J::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

fn number(f: f64) -> J {
    if f.fract() == 0.0 && f.abs() < 9e15 {
        J::from(f as i64)
    } else {
        serde_json::Number::from_f64(f).map_or(J::Null, J::Number)
    }
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tk> {
        self.t.get(self.i)
    }
    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tk::Op(o)) if o == op)
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tk::Ident(o)) if o == w)
    }
    fn expect_op(&mut self, op: &str) -> Result<(), String> {
        if self.is_op(op) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}'", op))
        }
    }

    fn expr(&mut self) -> Result<V, String> {
        let mut l = self.and()?;
        while self.is_word("or") {
            self.i += 1;
            let r = self.and()?;
            l = Some(J::Bool(truthy(&l) || truthy(&r)));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<V, String> {
        let mut l = self.not()?;
        while self.is_word("and") {
            self.i += 1;
            let r = self.not()?;
            l = Some(J::Bool(truthy(&l) && truthy(&r)));
        }
        Ok(l)
    }

    fn not(&mut self) -> Result<V, String> {
        if self.is_word("not") {
            self.i += 1;
            let v = self.not()?;
            return Ok(Some(J::Bool(!truthy(&v))));
        }
        self.cmp()
    }

    fn cmp(&mut self) -> Result<V, String> {
        let l = self.concat()?;
        // is [not] defined / undefined / none
        if self.is_word("is") {
            self.i += 1;
            let neg = if self.is_word("not") {
                self.i += 1;
                true
            } else {
                false
            };
            let test = match self.peek() {
                Some(Tk::Ident(w)) => w.clone(),
                _ => return Err("expected a test after 'is'".into()),
            };
            self.i += 1;
            let r = match test.as_str() {
                "defined" => l.is_some(),
                "undefined" => l.is_none(),
                "none" => matches!(l, Some(J::Null)),
                "true" => l == Some(J::Bool(true)),
                "false" => l == Some(J::Bool(false)),
                other => return Err(format!("unknown test '{}'", other)),
            };
            return Ok(Some(J::Bool(r != neg)));
        }
        let op = match self.peek() {
            Some(Tk::Op(o)) if ["==", "!=", "<", ">", "<=", ">="].contains(&o.as_str()) => o.clone(),
            Some(Tk::Ident(w)) if w == "in" => "in".into(),
            Some(Tk::Ident(w)) if w == "not" && matches!(self.t.get(self.i + 1), Some(Tk::Ident(n)) if n == "in") => {
                self.i += 1;
                "not in".into()
            }
            _ => return Ok(l),
        };
        self.i += 1;
        let r = self.concat()?;
        let (a, b) = (l.unwrap_or(J::Null), r.unwrap_or(J::Null));
        let res = match op.as_str() {
            "==" => equal(&a, &b),
            "!=" => !equal(&a, &b),
            "in" | "not in" => {
                let found = match &b {
                    J::Array(items) => items.iter().any(|x| equal(x, &a)),
                    J::String(s) => s.contains(&text_of(&a)),
                    J::Object(o) => o.contains_key(&text_of(&a)),
                    _ => false,
                };
                found == (op == "in")
            }
            _ => {
                let ord = match (num_of(&a), num_of(&b)) {
                    (Some(x), Some(y)) if !matches!((&a, &b), (J::String(_), J::String(_))) => x.partial_cmp(&y),
                    _ => Some(text_of(&a).cmp(&text_of(&b))),
                };
                let ord = ord.ok_or("cannot compare")?;
                match op.as_str() {
                    "<" => ord.is_lt(),
                    ">" => ord.is_gt(),
                    "<=" => ord.is_le(),
                    _ => ord.is_ge(),
                }
            }
        };
        Ok(Some(J::Bool(res)))
    }

    /// `~` joins texts; `+`/`-` on numbers
    /// `*`, `/`, `%` on numbers
    fn term(&mut self) -> Result<V, String> {
        let mut l = self.filtered()?;
        loop {
            let op = match self.peek() {
                Some(Tk::Op(o)) if o == "*" || o == "/" || o == "%" => o.clone(),
                _ => return Ok(l),
            };
            self.i += 1;
            let r = self.filtered()?;
            let a = num_of(&l.unwrap_or(J::Null)).ok_or_else(|| format!("'{}' needs numbers", op))?;
            let b = num_of(&r.unwrap_or(J::Null)).ok_or_else(|| format!("'{}' needs numbers", op))?;
            if (op == "/" || op == "%") && b == 0.0 {
                return Err("division by zero".into());
            }
            l = Some(number(match op.as_str() {
                "*" => a * b,
                "/" => a / b,
                _ => a % b,
            }));
        }
    }

    fn concat(&mut self) -> Result<V, String> {
        let mut l = self.term()?;
        loop {
            let op = match self.peek() {
                Some(Tk::Op(o)) if o == "~" || o == "+" || o == "-" => o.clone(),
                _ => return Ok(l),
            };
            self.i += 1;
            let r = self.term()?;
            let (a, b) = (l.unwrap_or(J::Null), r.unwrap_or(J::Null));
            l = Some(match op.as_str() {
                "~" => J::String(text_of(&a) + &text_of(&b)),
                "+" => match (&a, &b) {
                    (J::String(x), J::String(y)) => J::String(format!("{}{}", x, y)),
                    (J::Array(x), J::Array(y)) => J::Array(x.iter().chain(y).cloned().collect()),
                    _ => number(num_of(&a).ok_or("'+' needs numbers")? + num_of(&b).ok_or("'+' needs numbers")?),
                },
                _ => number(num_of(&a).ok_or("'-' needs numbers")? - num_of(&b).ok_or("'-' needs numbers")?),
            });
        }
    }

    fn filtered(&mut self) -> Result<V, String> {
        let mut v = self.primary()?;
        while self.is_op("|") {
            self.i += 1;
            let name = match self.peek() {
                Some(Tk::Ident(n)) => n.clone(),
                _ => return Err("expected a filter name after '|'".into()),
            };
            self.i += 1;
            let mut args = Vec::new();
            if self.is_op("(") {
                self.i += 1;
                while !self.is_op(")") {
                    args.push(self.expr()?.unwrap_or(J::Null));
                    if !self.is_op(",") {
                        break;
                    }
                    self.i += 1;
                }
                self.expect_op(")")?;
            }
            v = filter(&name, v, &args)?;
        }
        Ok(v)
    }

    fn primary(&mut self) -> Result<V, String> {
        let tk = self.peek().cloned().ok_or("unexpected end of expression")?;
        self.i += 1;
        let mut v: V = match tk {
            Tk::Str(s) => Some(J::String(s)),
            Tk::Num(n) => Some(number(n)),
            Tk::Op(o) if o == "(" => {
                let v = self.expr()?;
                self.expect_op(")")?;
                v
            }
            Tk::Op(o) if o == "[" => {
                let mut items = Vec::new();
                while !self.is_op("]") {
                    items.push(self.expr()?.unwrap_or(J::Null));
                    if !self.is_op(",") {
                        break;
                    }
                    self.i += 1;
                }
                self.expect_op("]")?;
                Some(J::Array(items))
            }
            Tk::Op(o) if o == "-" => {
                let v = self.primary()?.unwrap_or(J::Null);
                Some(number(-num_of(&v).ok_or("'-' needs a number")?))
            }
            Tk::Ident(w) => match w.as_str() {
                "true" | "True" => Some(J::Bool(true)),
                "false" | "False" => Some(J::Bool(false)),
                "none" | "None" | "null" => Some(J::Null),
                name => self.vars.get(name).cloned(),
            },
            Tk::Op(o) => return Err(format!("unexpected '{}'", o)),
        };
        // .field and [index]
        loop {
            if self.is_op(".") {
                self.i += 1;
                let key = match self.peek() {
                    Some(Tk::Ident(k)) => k.clone(),
                    Some(Tk::Num(n)) => (*n as i64).to_string(),
                    _ => return Err("expected a name after '.'".into()),
                };
                self.i += 1;
                v = index(v, &J::String(key));
            } else if self.is_op("[") {
                self.i += 1;
                let k = self.expr()?.unwrap_or(J::Null);
                self.expect_op("]")?;
                v = index(v, &k);
            } else {
                return Ok(v);
            }
        }
    }
}

fn index(v: V, k: &J) -> V {
    match (v?, k) {
        (J::Object(o), k) => o.get(&text_of(k)).cloned(),
        (J::Array(a), k) => {
            let i = num_of(k)? as i64;
            let i = if i < 0 { a.len() as i64 + i } else { i };
            a.get(usize::try_from(i).ok()?).cloned()
        }
        _ => None,
    }
}

fn equal(a: &J, b: &J) -> bool {
    match (num_of(a), num_of(b)) {
        (Some(x), Some(y)) if matches!(a, J::Number(_)) || matches!(b, J::Number(_)) => x == y,
        _ => a == b || (matches!((a, b), (J::String(_), _) | (_, J::String(_))) && text_of(a) == text_of(b) && !matches!((a, b), (J::Null, _) | (_, J::Null))),
    }
}

fn filter(name: &str, v: V, args: &[J]) -> Result<V, String> {
    let need = |v: V| v.ok_or_else(|| format!("filter '{}' applied to an undefined value", name));
    Ok(Some(match name {
        "default" | "d" => match v {
            None => args.first().cloned().unwrap_or(J::String(String::new())),
            Some(J::Null) if args.get(1).is_some_and(|b| b == &J::Bool(true)) => args[0].clone(),
            Some(x) => x,
        },
        "upper" => J::String(text_of(&need(v)?).to_uppercase()),
        "lower" => J::String(text_of(&need(v)?).to_lowercase()),
        "trim" => J::String(text_of(&need(v)?).trim().to_string()),
        "string" => J::String(text_of(&need(v)?)),
        "int" => J::from(num_of(&need(v)?).unwrap_or(0.0) as i64),
        "float" => number(num_of(&need(v)?).unwrap_or(0.0)),
        "bool" => J::Bool(truthy(&v)),
        "length" | "count" => J::from(match need(v)? {
            J::Array(a) => a.len(),
            J::Object(o) => o.len(),
            J::String(s) => s.chars().count(),
            _ => 0,
        }),
        "join" => {
            let sep = args.first().map(text_of).unwrap_or_default();
            match need(v)? {
                J::Array(a) => J::String(a.iter().map(text_of).collect::<Vec<_>>().join(&sep)),
                other => J::String(text_of(&other)),
            }
        }
        "first" | "last" => match need(v)? {
            J::Array(a) => (if name == "first" { a.first() } else { a.last() }).cloned().unwrap_or(J::Null),
            J::String(s) => J::String(if name == "first" { s.chars().next() } else { s.chars().last() }.map(String::from).unwrap_or_default()),
            other => other,
        },
        "replace" => J::String(text_of(&need(v)?).replace(&args.first().map(text_of).unwrap_or_default(), &args.get(1).map(text_of).unwrap_or_default())),
        "split" => {
            let s = text_of(&need(v)?);
            let parts: Vec<J> = match args.first() {
                Some(sep) => s.split(&text_of(sep)).map(|x| J::String(x.to_string())).collect(),
                None => s.split_whitespace().map(|x| J::String(x.to_string())).collect(),
            };
            J::Array(parts)
        }
        "to_json" => J::String(need(v)?.to_string()),
        "basename" => J::String(text_of(&need(v)?).rsplit('/').next().unwrap_or("").to_string()),
        "dirname" => {
            let s = text_of(&need(v)?);
            J::String(s.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default())
        }
        other => return Err(format!("unknown filter '{}'", other)),
    }))
}

fn eval(expr: &str, vars: &J) -> Result<V, String> {
    let t = tokenize(expr)?;
    let mut p = Parser { t, i: 0, vars };
    let v = p.expr()?;
    if p.i != p.t.len() {
        return Err(format!("unexpected '{:?}' in '{}'", p.t[p.i], expr));
    }
    Ok(v)
}

/// `{{ expr }}` in a text. An undefined value is an error (use `| default`).
fn render(text: &str, vars: &J) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or_else(|| format!("unclosed '{{{{' in '{}'", text))?;
        let expr = after[..end].trim();
        match eval(expr, vars)? {
            Some(v) => out.push_str(&text_of(&v)),
            None => return Err(format!("'{}' is undefined", expr)),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Renders every string inside a value; `"{{ expr }}"` alone keeps the
/// value's type (a list stays a list).
fn render_value(v: &J, vars: &J) -> Result<J, String> {
    Ok(match v {
        J::String(s) => {
            let t = s.trim();
            if t.starts_with("{{") && t.ends_with("}}") && t[2..t.len() - 2].find("{{").is_none() {
                eval(t[2..t.len() - 2].trim(), vars)?.ok_or_else(|| format!("'{}' is undefined", t[2..t.len() - 2].trim()))?
            } else {
                J::String(render(s, vars)?)
            }
        }
        J::Array(a) => J::Array(a.iter().map(|x| render_value(x, vars)).collect::<Result<_, _>>()?),
        J::Object(o) => J::Object(o.iter().map(|(k, x)| Ok((k.clone(), render_value(x, vars)?))).collect::<Result<_, String>>()?),
        other => other.clone(),
    })
}

/// `when:` — an expression (without braces), a bool, or a list (all true).
fn condition(v: &J, vars: &J) -> Result<bool, String> {
    match v {
        J::Bool(b) => Ok(*b),
        J::Array(items) => {
            for i in items {
                if !condition(i, vars)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        J::String(s) => {
            let s = s.trim();
            let s = s.strip_prefix("{{").and_then(|x| x.strip_suffix("}}")).unwrap_or(s);
            Ok(truthy(&eval(s, vars)?))
        }
        J::Null => Ok(true),
        other => Ok(truthy(&Some(other.clone()))),
    }
}

// =====================================================================
// Inventory
// =====================================================================

#[derive(Clone)]
struct Host {
    name: String,
    local: bool,
    target: Option<crate::native::ssh::Target>,
    vars: Map<String, J>,
    become_password: Option<String>,
}

struct Inventory {
    hosts: Vec<Host>,
    groups: BTreeMap<String, Vec<String>>,
    group_vars: BTreeMap<String, Map<String, J>>,
    vars: Map<String, J>,
}

impl Inventory {
    fn describe(&self) -> J {
        json!({
            "hosts": self.hosts.iter().map(|h| json!({
                "name": h.name,
                "connection": if h.local { "local" } else { "ssh" },
                "address": h.target.as_ref().map(|t| t.display()),
            })).collect::<Vec<_>>(),
            "groups": self.groups,
        })
    }

    /// Hosts matching a pattern: `all`, a host, a group, a list, `a:b`, `!x`.
    fn select(&self, pattern: &J) -> Result<Vec<String>, String> {
        let parts: Vec<String> = match pattern {
            J::String(s) => s.split([':', ',']).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
            J::Array(a) => a.iter().map(text_of).collect(),
            _ => return Err("a play needs 'hosts'".into()),
        };
        let mut out: Vec<String> = Vec::new();
        let mut exclude: HashSet<String> = HashSet::new();
        for p in parts {
            let (neg, p) = match p.strip_prefix('!') {
                Some(x) => (true, x.to_string()),
                None => (false, p),
            };
            let names: Vec<String> = if p == "all" || p == "*" {
                self.hosts.iter().map(|h| h.name.clone()).collect()
            } else if let Some(g) = self.groups.get(&p) {
                g.clone()
            } else if self.hosts.iter().any(|h| h.name == p) {
                vec![p.clone()]
            } else if p == "localhost" {
                vec!["localhost".into()]
            } else {
                return Err(format!("no host or group '{}' in the inventory", p));
            };
            if neg {
                exclude.extend(names);
            } else {
                for n in names {
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
        out.retain(|n| !exclude.contains(n));
        Ok(out)
    }

    fn host(&self, name: &str) -> Host {
        self.hosts.iter().find(|h| h.name == name).cloned().unwrap_or_else(|| Host {
            name: name.to_string(),
            local: true,
            target: None,
            vars: Map::new(),
            become_password: None,
        })
    }

    /// Variables of a host: inventory vars → its groups' vars → its own.
    fn host_vars(&self, name: &str) -> Map<String, J> {
        let mut v = self.vars.clone();
        for (g, members) in &self.groups {
            if members.iter().any(|m| m == name) {
                if let Some(gv) = self.group_vars.get(g) {
                    v.extend(gv.clone());
                }
            }
        }
        v.extend(self.host(name).vars);
        v
    }
}

fn load_inventory(v: &J) -> Result<Inventory, String> {
    let data = match v {
        J::Null => json!({ "hosts": { "localhost": { "connection": "local" } } }),
        other => load_data(other, "inventory")?,
    };
    let get_str = |m: &Map<String, J>, keys: &[&str]| keys.iter().find_map(|k| m.get(*k).filter(|v| !v.is_null()).map(text_of));
    let mut inv = Inventory { hosts: Vec::new(), groups: BTreeMap::new(), group_vars: BTreeMap::new(), vars: Map::new() };
    if let Some(J::Object(v)) = data.get("vars") {
        inv.vars = v.clone();
    }
    match data.get("hosts") {
        Some(J::Object(hosts)) => {
            for (name, h) in hosts {
                let m = h.as_object().cloned().unwrap_or_default();
                let local = get_str(&m, &["connection", "ansible_connection"]).as_deref() == Some("local");
                let target = if local {
                    None
                } else {
                    let addr = get_str(&m, &["host", "address", "ansible_host"]).unwrap_or_else(|| name.clone());
                    let port = m.get("port").or_else(|| m.get("ansible_port")).and_then(num_of).map(|p| p as u16);
                    let mut t = crate::native::ssh::Target::new(
                        &addr,
                        get_str(&m, &["user", "ansible_user"]),
                        port,
                        get_str(&m, &["key", "ansible_ssh_private_key_file"]),
                        get_str(&m, &["password", "ansible_password"]),
                    );
                    if let Some(J::Bool(false)) = m.get("strictHostKey") {
                        t.strict = "no".into();
                    }
                    Some(t)
                };
                let mut vars = match m.get("vars") {
                    Some(J::Object(v)) => v.clone(),
                    _ => Map::new(),
                };
                for (k, v) in &m {
                    if !matches!(k.as_str(), "vars" | "password" | "ansible_password" | "become_password") {
                        vars.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
                inv.hosts.push(Host {
                    name: name.clone(),
                    local,
                    target,
                    vars,
                    become_password: get_str(&m, &["become_password", "ansible_become_password"]),
                });
            }
        }
        Some(J::Array(names)) => {
            for n in names {
                let name = text_of(n);
                inv.hosts.push(Host {
                    target: Some(crate::native::ssh::Target::new(&name, None, None, None, None)),
                    name,
                    local: false,
                    vars: Map::new(),
                    become_password: None,
                });
            }
        }
        None => {}
        Some(_) => return Err(io_err!("inventory: 'hosts' must be a map of hosts")),
    }
    if let Some(J::Object(groups)) = data.get("groups") {
        for (g, def) in groups {
            let (members, vars) = match def {
                J::Array(a) => (a.iter().map(text_of).collect::<Vec<_>>(), Map::new()),
                J::Object(o) => (
                    o.get("hosts").and_then(J::as_array).map(|a| a.iter().map(text_of).collect()).unwrap_or_default(),
                    o.get("vars").and_then(J::as_object).cloned().unwrap_or_default(),
                ),
                _ => return Err(io_err!("inventory: group '{}' must be a list of hosts or {{ hosts, vars }}", g)),
            };
            for m in &members {
                if !inv.hosts.iter().any(|h| &h.name == m) {
                    return Err(io_err!("inventory: group '{}' names an unknown host '{}'", g, m));
                }
            }
            inv.groups.insert(g.clone(), members);
            inv.group_vars.insert(g.clone(), vars);
        }
    }
    Ok(inv)
}

// =====================================================================
// Connections
// =====================================================================

struct Out {
    rc: i64,
    stdout: String,
    stderr: String,
}

/// Quote for POSIX sh.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

enum Conn {
    Local,
    Ssh(crate::native::ssh::Target),
}

impl Conn {
    /// `Err` = unreachable.
    fn run(&self, cmd: &str, stdin: Option<&[u8]>, sudo: Option<&Become>) -> Result<Out, String> {
        match self {
            Conn::Local => {
                let (rc, o, e) = if cfg!(windows) || sudo.is_none() {
                    crate::native::shell::run_line(cmd, None, &[], stdin.map(<[u8]>::to_vec).unwrap_or_default())?
                } else {
                    let full = format!("sudo -n sh -c {}", q(cmd));
                    crate::native::shell::run_line(&full, None, &[], stdin.map(<[u8]>::to_vec).unwrap_or_default())?
                };
                Ok(Out { rc: rc as i64, stdout: String::from_utf8_lossy(&o).into(), stderr: String::from_utf8_lossy(&e).into() })
            }
            Conn::Ssh(t) => {
                let full = match sudo {
                    None => cmd.to_string(),
                    Some(b) if b.password.is_some() => format!("sudo -S -p '' sh -c {}", q(cmd)),
                    Some(_) => format!("sudo -n sh -c {}", q(cmd)),
                };
                // the sudo password goes first on stdin
                let input: Option<Vec<u8>> = match (sudo.and_then(|b| b.password.clone()), stdin) {
                    (Some(pw), Some(data)) => Some([format!("{}\n", pw).as_bytes(), data].concat()),
                    (Some(pw), None) => Some(format!("{}\n", pw).into_bytes()),
                    (None, data) => data.map(<[u8]>::to_vec),
                };
                let (rc, o, e) = t.run_bytes(&full, input.as_deref())?;
                if rc == 255 {
                    return Err(String::from_utf8_lossy(&e).trim().to_string());
                }
                Ok(Out { rc, stdout: String::from_utf8_lossy(&o).into(), stderr: String::from_utf8_lossy(&e).into() })
            }
        }
    }

    fn read(&self, path: &str, sudo: Option<&Become>) -> Result<Option<Vec<u8>>, String> {
        match self {
            Conn::Local => match std::fs::read(path) {
                Ok(d) => Ok(Some(d)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("{}: {}", path, e)),
            },
            Conn::Ssh(_) => {
                let r = self.run(&format!("if [ -f {p} ]; then cat {p}; else exit 3; fi", p = q(path)), None, sudo)?;
                match r.rc {
                    0 => Ok(Some(r.stdout.into_bytes())),
                    3 => Ok(None),
                    _ => Err(format!("{}: {}", path, r.stderr.trim())),
                }
            }
        }
    }

    fn write(&self, path: &str, data: &[u8], mode: Option<&str>, sudo: Option<&Become>) -> Result<(), String> {
        match self {
            Conn::Local => {
                if let Some(dir) = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", path, e))?;
                }
                std::fs::write(path, data).map_err(|e| format!("{}: {}", path, e))?;
                if let Some(m) = mode {
                    set_mode_local(path, m)?;
                }
                Ok(())
            }
            Conn::Ssh(_) => {
                let chmod = mode.map(|m| format!(" && chmod {} {}", q(m), q(path))).unwrap_or_default();
                let cmd = format!("mkdir -p \"$(dirname {p})\" && cat > {p}{c}", p = q(path), c = chmod);
                let r = self.run(&cmd, Some(data), sudo)?;
                if r.rc != 0 {
                    return Err(format!("{}: {}", path, r.stderr.trim()));
                }
                Ok(())
            }
        }
    }

    /// `"dir"`, `"file"` or `None`
    fn stat(&self, path: &str, sudo: Option<&Become>) -> Result<Option<&'static str>, String> {
        match self {
            Conn::Local => Ok(std::fs::metadata(path).ok().map(|m| if m.is_dir() { "dir" } else { "file" })),
            Conn::Ssh(_) => {
                let r = self.run(&format!("if [ -d {p} ]; then echo dir; elif [ -e {p} ]; then echo file; else echo none; fi", p = q(path)), None, sudo)?;
                Ok(match r.stdout.trim() {
                    "dir" => Some("dir"),
                    "file" => Some("file"),
                    _ => None,
                })
            }
        }
    }

    /// Current permission bits (`"0644"`), when the platform has them.
    fn mode(&self, path: &str, sudo: Option<&Become>) -> Result<Option<String>, String> {
        match self {
            Conn::Local => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    return Ok(std::fs::metadata(path).ok().map(|m| format!("{:04o}", m.permissions().mode() & 0o7777)));
                }
                #[allow(unreachable_code)]
                Ok(None)
            }
            Conn::Ssh(_) => {
                let r = self.run(&format!("stat -c %a {p} 2>/dev/null || stat -f %Lp {p}", p = q(path)), None, sudo)?;
                Ok((r.rc == 0).then(|| format!("{:0>4}", r.stdout.trim())))
            }
        }
    }
}

fn set_mode_local(path: &str, mode: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bits = u32::from_str_radix(mode.trim_start_matches("0o"), 8).map_err(|_| format!("invalid mode '{}'", mode))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).map_err(|e| format!("{}: {}", path, e))?;
    }
    let _ = (path, mode);
    Ok(())
}

#[derive(Clone)]
struct Become {
    password: Option<String>,
}

// =====================================================================
// Tasks
// =====================================================================

const MODULES: &[&str] = &[
    "shell", "command", "copy", "template", "file", "lineinfile", "package", "service", "debug", "set_fact", "fail",
    "wait_for",
];

#[derive(Debug, Clone, PartialEq)]
enum Status {
    Ok,
    Changed,
    Skipped,
    Failed(String),
    Unreachable(String),
}

struct TaskResult {
    status: Status,
    /// registered value: `{ changed, failed, rc, stdout, stderr, msg… }`
    data: Map<String, J>,
    /// text shown after the status (debug messages)
    show: Option<String>,
}

impl TaskResult {
    fn new(status: Status) -> Self {
        TaskResult { status, data: Map::new(), show: None }
    }
}

struct Ctx<'a> {
    conn: &'a Conn,
    sudo: Option<Become>,
    check: bool,
    vars: &'a J,
    facts: &'a Map<String, J>,
    base_dir: &'a Path,
}

fn arg_str(args: &J, k: &str) -> Option<String> {
    args.get(k).filter(|v| !v.is_null()).map(text_of)
}

fn sha(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data).iter().map(|b| format!("{:02x}", b)).collect()
}

/// Writes a file when its content (or mode) differs; returns whether it changed.
fn ensure_content(c: &Ctx, dest: &str, content: &[u8], mode: Option<&str>) -> Result<bool, String> {
    let b = c.sudo.as_ref();
    let current = c.conn.read(dest, b)?;
    let same = current.as_deref().is_some_and(|cur| sha(cur) == sha(content));
    let mode_ok = match mode {
        Some(m) => c.conn.mode(dest, b)?.is_none_or(|cur| cur.trim_start_matches('0') == m.trim_start_matches('0')),
        None => true,
    };
    if same && mode_ok {
        return Ok(false);
    }
    if !c.check {
        c.conn.write(dest, content, mode, b)?;
    }
    Ok(true)
}

fn run_module(module: &str, args: &J, c: &Ctx) -> Result<TaskResult, String> {
    let b = c.sudo.as_ref();
    let unreachable = |e: String| TaskResult::new(Status::Unreachable(e));
    let failed = |m: String| TaskResult::new(Status::Failed(m));
    let changed = |yes: bool| TaskResult::new(if yes { Status::Changed } else { Status::Ok });
    Ok(match module {
        "shell" | "command" => {
            let (cmd, chdir) = match args {
                J::String(s) => (s.clone(), None),
                J::Object(_) => (arg_str(args, "cmd").or_else(|| arg_str(args, "_raw")).ok_or(format!("{}: needs 'cmd'", module))?, arg_str(args, "chdir")),
                _ => return Err(format!("{}: expected a command", module)),
            };
            // creates/removes make it idempotent
            if let Some(p) = arg_str(args, "creates") {
                if c.conn.stat(&p, b).map_err(|e| e.to_string())?.is_some() {
                    return Ok(TaskResult::new(Status::Ok));
                }
            }
            if let Some(p) = arg_str(args, "removes") {
                if c.conn.stat(&p, b).map_err(|e| e.to_string())?.is_none() {
                    return Ok(TaskResult::new(Status::Ok));
                }
            }
            if c.check {
                let mut r = TaskResult::new(Status::Skipped);
                r.show = Some("skipped in check mode".into());
                return Ok(r);
            }
            let full = match chdir {
                Some(d) => format!("cd {} && {}", q(&d), cmd),
                None => cmd,
            };
            let stdin = arg_str(args, "stdin").map(String::into_bytes);
            match c.conn.run(&full, stdin.as_deref(), b) {
                Err(e) => unreachable(e),
                Ok(o) => {
                    let mut r = if o.rc == 0 { TaskResult::new(Status::Changed) } else { failed(format!("non-zero return code {}: {}", o.rc, o.stderr.trim())) };
                    r.data.insert("rc".into(), J::from(o.rc));
                    r.data.insert("stdout".into(), J::String(o.stdout.trim_end_matches(['\n', '\r']).to_string()));
                    r.data.insert("stderr".into(), J::String(o.stderr.trim_end_matches(['\n', '\r']).to_string()));
                    r.data.insert(
                        "stdout_lines".into(),
                        J::Array(o.stdout.lines().map(|l| J::String(l.to_string())).collect()),
                    );
                    r
                }
            }
        }
        "copy" | "template" => {
            let dest = arg_str(args, "dest").ok_or(format!("{}: needs 'dest'", module))?;
            let raw: Vec<u8> = match (arg_str(args, "content"), arg_str(args, "src")) {
                (Some(content), _) => content.into_bytes(),
                (None, Some(src)) => {
                    let p = c.base_dir.join(&src);
                    std::fs::read(&p).map_err(|e| format!("{}: {}: {}", module, p.display(), e))?
                }
                _ => return Err(format!("{}: needs 'src' or 'content'", module)),
            };
            let data = if module == "template" {
                render(&String::from_utf8_lossy(&raw), c.vars)?.into_bytes()
            } else {
                raw
            };
            let mode = arg_str(args, "mode");
            match ensure_content(c, &dest, &data, mode.as_deref()) {
                Ok(ch) => changed(ch),
                Err(e) => failed(e),
            }
        }
        "file" => {
            let path = arg_str(args, "path").or_else(|| arg_str(args, "dest")).ok_or("file: needs 'path'")?;
            let state = arg_str(args, "state").unwrap_or_else(|| "file".into());
            let mode = arg_str(args, "mode");
            let cur = match c.conn.stat(&path, b) {
                Ok(s) => s,
                Err(e) => return Ok(unreachable(e)),
            };
            let mut did = false;
            match state.as_str() {
                "directory" => {
                    if cur == Some("file") {
                        return Ok(failed(format!("{} exists and is not a directory", path)));
                    }
                    if cur.is_none() {
                        did = true;
                        if !c.check {
                            match c.conn {
                                Conn::Local => std::fs::create_dir_all(&path).map_err(|e| format!("{}: {}", path, e))?,
                                Conn::Ssh(_) => {
                                    let r = c.conn.run(&format!("mkdir -p {}", q(&path)), None, b)?;
                                    if r.rc != 0 {
                                        return Ok(failed(r.stderr.trim().to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
                "absent" => {
                    if cur.is_some() {
                        did = true;
                        if !c.check {
                            match c.conn {
                                Conn::Local => {
                                    let r = if cur == Some("dir") { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
                                    r.map_err(|e| format!("{}: {}", path, e))?;
                                }
                                Conn::Ssh(_) => {
                                    let r = c.conn.run(&format!("rm -rf {}", q(&path)), None, b)?;
                                    if r.rc != 0 {
                                        return Ok(failed(r.stderr.trim().to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
                "touch" => {
                    did = cur.is_none();
                    if did && !c.check {
                        c.conn.write(&path, b"", mode.as_deref(), b)?;
                    }
                }
                "file" => {
                    if cur.is_none() {
                        return Ok(failed(format!("{} does not exist (use state: touch to create it)", path)));
                    }
                }
                other => return Err(format!("file: unknown state '{}' (directory absent touch file)", other)),
            }
            // permissions
            if let (Some(m), true) = (&mode, state != "absent") {
                let current = if c.check && did { None } else { c.conn.mode(&path, b)? };
                if current.is_some_and(|cur| cur.trim_start_matches('0') != m.trim_start_matches('0')) {
                    did = true;
                    if !c.check {
                        match c.conn {
                            Conn::Local => set_mode_local(&path, m)?,
                            Conn::Ssh(_) => {
                                c.conn.run(&format!("chmod {} {}", q(m), q(&path)), None, b)?;
                            }
                        }
                    }
                }
            }
            changed(did)
        }
        "lineinfile" => {
            let path = arg_str(args, "path").or_else(|| arg_str(args, "dest")).ok_or("lineinfile: needs 'path'")?;
            let state = arg_str(args, "state").unwrap_or_else(|| "present".into());
            let line = arg_str(args, "line");
            let re = match arg_str(args, "regexp") {
                Some(r) => Some(::regex::Regex::new(&r).map_err(|e| format!("lineinfile: invalid regexp: {}", e))?),
                None => None,
            };
            let create = args.get("create").is_some_and(|v| truthy(&Some(v.clone())));
            let current = match c.conn.read(&path, b) {
                Ok(Some(d)) => String::from_utf8_lossy(&d).into_owned(),
                Ok(None) if create || state == "absent" => String::new(),
                Ok(None) => return Ok(failed(format!("{} does not exist (use create: true)", path))),
                Err(e) => return Ok(failed(e)),
            };
            let mut lines: Vec<String> = current.lines().map(String::from).collect();
            let matches = |l: &str| match &re {
                Some(r) => r.is_match(l),
                None => line.as_deref() == Some(l),
            };
            if state == "absent" {
                lines.retain(|l| !matches(l));
            } else {
                let line = line.clone().ok_or("lineinfile: needs 'line'")?;
                match lines.iter().rposition(|l| matches(l)) {
                    Some(i) => lines[i] = line,
                    None => {
                        if !lines.iter().any(|l| *l == line) {
                            lines.push(line)
                        }
                    }
                }
            }
            let mut out = lines.join("\n");
            if !out.is_empty() {
                out.push('\n');
            }
            if out == current {
                changed(false)
            } else {
                if !c.check {
                    c.conn.write(&path, out.as_bytes(), arg_str(args, "mode").as_deref(), b)?;
                }
                changed(true)
            }
        }
        "package" => {
            let names: Vec<String> = match args.get("name") {
                Some(J::Array(a)) => a.iter().map(text_of).collect(),
                Some(J::String(s)) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                _ => return Err("package: needs 'name'".into()),
            };
            let state = arg_str(args, "state").unwrap_or_else(|| "present".into());
            let mgr = arg_str(args, "use").or_else(|| c.facts.get("pkg_mgr").map(text_of)).filter(|m| !m.is_empty()).ok_or("package: no package manager found (gather facts or set 'use')")?;
            let (installed, install, remove): (&str, &str, &str) = match mgr.as_str() {
                "apt" => ("dpkg-query -W -f='${Status}' {} 2>/dev/null | grep -q 'install ok installed'", "DEBIAN_FRONTEND=noninteractive apt-get install -y", "DEBIAN_FRONTEND=noninteractive apt-get remove -y"),
                "dnf" => ("rpm -q {} >/dev/null 2>&1", "dnf install -y", "dnf remove -y"),
                "yum" => ("rpm -q {} >/dev/null 2>&1", "yum install -y", "yum remove -y"),
                "zypper" => ("rpm -q {} >/dev/null 2>&1", "zypper -n install", "zypper -n remove"),
                "apk" => ("apk info -e {} >/dev/null 2>&1", "apk add", "apk del"),
                "pacman" => ("pacman -Q {} >/dev/null 2>&1", "pacman -S --noconfirm", "pacman -R --noconfirm"),
                other => return Err(format!("package: unknown package manager '{}'", other)),
            };
            let mut todo = Vec::new();
            for n in &names {
                let r = match c.conn.run(&installed.replace("{}", &q(n)), None, None) {
                    Ok(r) => r,
                    Err(e) => return Ok(unreachable(e)),
                };
                let present = r.rc == 0;
                if (state == "absent") == present {
                    todo.push(n.clone());
                }
            }
            if todo.is_empty() {
                return Ok(changed(false));
            }
            if !c.check {
                if mgr == "apt" && args.get("update_cache").is_some_and(|v| truthy(&Some(v.clone()))) {
                    c.conn.run("apt-get update", None, b)?;
                }
                let verb = if state == "absent" { remove } else { install };
                let quoted: Vec<String> = todo.iter().map(|n| q(n)).collect();
                let r = c.conn.run(&format!("{} {}", verb, quoted.join(" ")), None, b)?;
                if r.rc != 0 {
                    return Ok(failed(format!("{}: {}", mgr, r.stderr.trim().lines().last().unwrap_or(""))));
                }
            }
            let mut r = changed(true);
            r.data.insert("packages".into(), J::Array(todo.into_iter().map(J::String).collect()));
            r
        }
        "service" => {
            let name = arg_str(args, "name").ok_or("service: needs 'name'")?;
            let state = arg_str(args, "state");
            let enabled = args.get("enabled").map(|v| truthy(&Some(v.clone())));
            let mut did = false;
            let active = c.conn.run(&format!("systemctl is-active --quiet {}", q(&name)), None, b).map_err(|e| e.to_string())?.rc == 0;
            let action = match state.as_deref() {
                Some("started") if !active => Some("start"),
                Some("stopped") if active => Some("stop"),
                Some("restarted") => Some("restart"),
                Some("reloaded") => Some("reload"),
                Some("started" | "stopped") | None => None,
                Some(other) => return Err(format!("service: unknown state '{}' (started stopped restarted reloaded)", other)),
            };
            if let Some(a) = action {
                did = true;
                if !c.check {
                    let r = c.conn.run(&format!("systemctl {} {}", a, q(&name)), None, b)?;
                    if r.rc != 0 {
                        return Ok(failed(r.stderr.trim().to_string()));
                    }
                }
            }
            if let Some(want) = enabled {
                let is = c.conn.run(&format!("systemctl is-enabled --quiet {}", q(&name)), None, b)?.rc == 0;
                if is != want {
                    did = true;
                    if !c.check {
                        let r = c.conn.run(&format!("systemctl {} {}", if want { "enable" } else { "disable" }, q(&name)), None, b)?;
                        if r.rc != 0 {
                            return Ok(failed(r.stderr.trim().to_string()));
                        }
                    }
                }
            }
            changed(did)
        }
        "debug" => {
            let msg = match args {
                J::String(s) => s.clone(),
                J::Object(_) => match (args.get("msg"), args.get("var")) {
                    (Some(m), _) => text_of(m),
                    (None, Some(v)) => {
                        let name = text_of(v);
                        let val = eval(&name, c.vars)?.map(|v| v.to_string()).unwrap_or_else(|| "undefined".into());
                        format!("{} = {}", name, val)
                    }
                    _ => "Hello world!".into(),
                },
                other => text_of(other),
            };
            let mut r = TaskResult::new(Status::Ok);
            r.data.insert("msg".into(), J::String(msg.clone()));
            r.show = Some(msg);
            r
        }
        "set_fact" => {
            let mut r = TaskResult::new(Status::Ok);
            if let J::Object(o) = args {
                r.data = o.clone();
            }
            r.data.insert("_set_fact".into(), J::Bool(true));
            r
        }
        "fail" => failed(match args {
            J::String(s) => s.clone(),
            _ => arg_str(args, "msg").unwrap_or_else(|| "failed as requested".into()),
        }),
        "wait_for" => {
            let timeout = args.get("timeout").and_then(num_of).unwrap_or(30.0);
            let deadline = Instant::now() + Duration::from_secs_f64(timeout);
            let host = arg_str(args, "host").unwrap_or_else(|| "127.0.0.1".into());
            loop {
                let ready = if let Some(port) = args.get("port").and_then(num_of) {
                    match c.conn {
                        Conn::Local => std::net::TcpStream::connect_timeout(
                            &format!("{}:{}", host, port as u16).parse().map_err(|_| "wait_for: invalid host".to_string())?,
                            Duration::from_millis(500),
                        )
                        .is_ok(),
                        Conn::Ssh(_) => {
                            let cmd = format!("(command -v nc >/dev/null && nc -z {h} {p}) || (exec 3<>/dev/tcp/{h}/{p}) 2>/dev/null", h = host, p = port as u16);
                            c.conn.run(&cmd, None, None)?.rc == 0
                        }
                    }
                } else if let Some(p) = arg_str(args, "path") {
                    c.conn.stat(&p, b)?.is_some()
                } else {
                    return Err("wait_for: needs 'port' or 'path'".into());
                };
                if ready {
                    break TaskResult::new(Status::Ok);
                }
                if Instant::now() >= deadline {
                    break failed(format!("timeout after {}s", timeout));
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        other => return Err(format!("unknown module '{}' ({})", other, MODULES.join(" "))),
    })
}

// =====================================================================
// Facts
// =====================================================================

fn gather_facts(conn: &Conn, name: &str) -> Result<Map<String, J>, String> {
    let mut f = Map::new();
    if matches!(conn, Conn::Local) && cfg!(windows) {
        f.insert("os".into(), J::String("windows".into()));
        f.insert("arch".into(), J::String(std::env::consts::ARCH.into()));
        f.insert("hostname".into(), J::String(std::env::var("COMPUTERNAME").unwrap_or_else(|_| name.to_string())));
        f.insert("distribution".into(), J::String("windows".into()));
        f.insert("pkg_mgr".into(), J::String(String::new()));
        return Ok(f);
    }
    let script = "uname -s; uname -m; uname -n; echo ---; cat /etc/os-release 2>/dev/null; echo ---; \
                  for m in apt-get dnf yum zypper apk pacman; do command -v $m >/dev/null 2>&1 && echo $m && break; done; \
                  echo ---; command -v systemctl >/dev/null 2>&1 && echo systemd; echo ---; id -un";
    let o = conn.run(script, None, None)?;
    let parts: Vec<&str> = o.stdout.split("---").collect();
    let head: Vec<&str> = parts.first().map(|p| p.lines().filter(|l| !l.is_empty()).collect()).unwrap_or_default();
    f.insert("os".into(), J::String(head.first().map(|s| s.to_lowercase()).unwrap_or_default()));
    f.insert("arch".into(), J::String(head.get(1).map(|s| s.to_string()).unwrap_or_default()));
    f.insert("hostname".into(), J::String(head.get(2).map(|s| s.to_string()).unwrap_or_default()));
    let mut osr: HashMap<String, String> = HashMap::new();
    for l in parts.get(1).copied().unwrap_or("").lines() {
        if let Some((k, v)) = l.split_once('=') {
            osr.insert(k.to_string(), v.trim_matches('"').to_string());
        }
    }
    f.insert("distribution".into(), J::String(osr.get("ID").cloned().unwrap_or_default()));
    f.insert("distribution_version".into(), J::String(osr.get("VERSION_ID").cloned().unwrap_or_default()));
    let pkg = parts.get(2).map(|p| p.trim()).unwrap_or("");
    f.insert("pkg_mgr".into(), J::String(if pkg == "apt-get" { "apt".into() } else { pkg.to_string() }));
    f.insert("service_mgr".into(), J::String(parts.get(3).map(|p| p.trim()).unwrap_or("").to_string()));
    f.insert("user".into(), J::String(parts.get(4).map(|p| p.trim()).unwrap_or("").to_string()));
    Ok(f)
}

// =====================================================================
// Running playbooks
// =====================================================================

#[derive(Default, Clone)]
struct Stats {
    ok: i64,
    changed: i64,
    unreachable: i64,
    failed: i64,
    skipped: i64,
    ignored: i64,
}

struct HostState {
    name: String,
    conn: Conn,
    become_password: Option<String>,
    vars: Map<String, J>,
    facts: Map<String, J>,
    runtime: Map<String, J>,
    dead: bool,
    notified: Vec<String>,
    stats: Stats,
}

impl HostState {
    fn all_vars(&self, play_vars: &Map<String, J>, extra: &Map<String, J>, groups: &BTreeMap<String, Vec<String>>) -> J {
        let mut v = self.vars.clone();
        v.insert("facts".into(), J::Object(self.facts.clone()));
        v.insert("ansible_facts".into(), J::Object(self.facts.clone()));
        v.extend(play_vars.clone());
        v.extend(extra.clone());
        v.extend(self.runtime.clone());
        v.insert("inventory_hostname".into(), J::String(self.name.clone()));
        v.insert("groups".into(), json!(groups));
        J::Object(v)
    }
}

#[derive(Clone)]
struct Options {
    check: bool,
    limit: Option<Vec<String>>,
    tags: Vec<String>,
    skip_tags: Vec<String>,
    forks: usize,
    extra: Map<String, J>,
    verbose: bool,
    quiet: bool,
    color: bool,
}

fn banner(title: &str) -> String {
    let stars = 72usize.saturating_sub(title.chars().count() + 1).max(3);
    format!("\n{} {}", title, "*".repeat(stars))
}

fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}

/// Tasks of a play with their kind: normal task or handler.
fn task_name(t: &J, module: &str) -> String {
    t.get("name").map(text_of).unwrap_or_else(|| module.to_string())
}

fn module_of(task: &J) -> Result<(String, J), String> {
    let obj = task.as_object().ok_or("a task must be a map")?;
    for m in MODULES {
        if let Some(a) = obj.get(*m) {
            return Ok((m.to_string(), a.clone()));
        }
    }
    // ansible.builtin.copy → copy
    for (k, a) in obj {
        if let Some(short) = k.strip_prefix("ansible.builtin.") {
            if MODULES.contains(&short) {
                return Ok((short.to_string(), a.clone()));
            }
        }
    }
    Err(format!("task '{}' has no known module ({})", task.get("name").map(text_of).unwrap_or_default(), MODULES.join(" ")))
}

fn tags_of(v: Option<&J>) -> Vec<String> {
    match v {
        Some(J::String(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
        Some(J::Array(a)) => a.iter().map(text_of).collect(),
        _ => Vec::new(),
    }
}

struct Run {
    output: Vec<String>,
    stats: BTreeMap<String, Stats>,
}

/// Runs one task on every live host (in parallel); prints the results in host order.
#[allow(clippy::too_many_arguments)]
fn run_task(
    task: &J,
    hosts: &mut [HostState],
    play_vars: &Map<String, J>,
    play_become: Option<&J>,
    o: &Options,
    groups: &BTreeMap<String, Vec<String>>,
    base_dir: &Path,
    out: &mut Vec<String>,
    only: Option<&HashSet<String>>,
) -> Result<(), String> {
    let (module, raw_args) = module_of(task)?;
    let name = task_name(task, &module);
    out.push(banner(&format!("TASK [{}]", name)));
    let forks = o.forks.max(1);
    let indexes: Vec<usize> = (0..hosts.len())
        .filter(|i| !hosts[*i].dead && only.is_none_or(|s| s.contains(&hosts[*i].name)))
        .collect();
    let mut lines: Vec<(usize, Vec<String>, Vec<(Status, Map<String, J>)>)> = Vec::new();
    for chunk in indexes.chunks(forks) {
        let results: Vec<(usize, Vec<String>, Vec<(Status, Map<String, J>)>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|&i| {
                    let h = &hosts[i];
                    let vars = h.all_vars(play_vars, &o.extra, groups);
                    let (task, module, raw_args) = (task.clone(), module.clone(), raw_args.clone());
                    let become_flag = task.get("become").or(play_become).is_some_and(|v| truthy(&Some(v.clone())));
                    let sudo = become_flag.then(|| Become { password: h.become_password.clone() });
                    scope.spawn(move || run_on_host(&h.name, &h.conn, &h.facts, &task, &module, &raw_args, &vars, sudo, o, base_dir))
                })
                .collect();
            handles.into_iter().zip(chunk).map(|(hd, &i)| {
                let (l, r) = hd.join().unwrap_or_else(|_| (vec![format!("fatal: [{}]: internal error", hosts[i].name)], vec![(Status::Failed("internal error".into()), Map::new())]));
                (i, l, r)
            }).collect()
        });
        lines.extend(results);
    }
    lines.sort_by_key(|(i, _, _)| *i);
    let notify = tags_of(task.get("notify"));
    let ignore = task.get("ignore_errors").is_some_and(|v| truthy(&Some(v.clone())));
    let register = task.get("register").map(text_of);
    for (i, ls, results) in lines {
        out.extend(ls);
        let h = &mut hosts[i];
        let mut any_changed = false;
        let mut any_failed = false;
        let mut regs: Vec<J> = Vec::new();
        for (status, data) in &results {
            match status {
                Status::Changed => any_changed = true,
                Status::Failed(_) => any_failed = true,
                Status::Unreachable(_) => {
                    h.stats.unreachable += 1;
                    h.dead = true;
                }
                _ => {}
            }
            if data.get("_set_fact").is_some() {
                for (k, v) in data {
                    if k != "_set_fact" {
                        h.runtime.insert(k.clone(), v.clone());
                    }
                }
            }
            regs.push(J::Object(data.clone()));
        }
        if h.dead {
            continue;
        }
        let skipped = results.iter().all(|(s, _)| *s == Status::Skipped);
        if any_failed {
            if ignore {
                out.push("...ignoring".into());
                h.stats.ignored += 1;
                h.stats.ok += 1;
            } else {
                h.stats.failed += 1;
                h.dead = true;
            }
        } else if skipped {
            h.stats.skipped += 1;
        } else if any_changed {
            h.stats.changed += 1;
            h.stats.ok += 1;
            for n in &notify {
                if !h.notified.contains(n) {
                    h.notified.push(n.clone());
                }
            }
        } else {
            h.stats.ok += 1;
        }
        if let Some(reg) = &register {
            let value = if task.get("loop").is_some() || task.get("with_items").is_some() {
                json!({ "results": regs, "changed": any_changed, "failed": any_failed })
            } else {
                regs.into_iter().next().unwrap_or(json!({}))
            };
            h.runtime.insert(reg.clone(), value);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_on_host(
    host: &str,
    conn: &Conn,
    facts: &Map<String, J>,
    task: &J,
    module: &str,
    raw_args: &J,
    vars: &J,
    sudo: Option<Become>,
    o: &Options,
    base_dir: &Path,
) -> (Vec<String>, Vec<(Status, Map<String, J>)>) {
    let mut lines = Vec::new();
    let mut results = Vec::new();
    let paint_status = |s: &Status, label: String| -> String {
        let (code, word) = match s {
            Status::Ok => ("32", "ok"),
            Status::Changed => ("33", "changed"),
            Status::Skipped => ("36", "skipping"),
            Status::Failed(_) => ("31", "fatal"),
            Status::Unreachable(_) => ("31", "fatal"),
        };
        paint(o.color, code, &format!("{}: {}", word, label))
    };
    // loop items
    let items: Option<Vec<J>> = match task.get("loop").or_else(|| task.get("with_items")) {
        None => None,
        Some(l) => match render_value(l, vars) {
            Ok(J::Array(a)) => Some(a),
            Ok(other) => Some(vec![other]),
            Err(e) => {
                let s = Status::Failed(e.clone());
                lines.push(format!("{} => {}", paint_status(&s, format!("[{}]", host)), e));
                results.push((s, Map::new()));
                return (lines, results);
            }
        },
    };
    let runs: Vec<Option<J>> = match items {
        Some(list) => list.into_iter().map(Some).collect(),
        None => vec![None],
    };
    for item in runs {
        let mut scope = vars.clone();
        if let (Some(it), J::Object(m)) = (&item, &mut scope) {
            m.insert("item".into(), it.clone());
        }
        let label = match &item {
            Some(it) => format!("[{}] => (item={})", host, text_of(it)),
            None => format!("[{}]", host),
        };
        // when
        match task.get("when").map(|w| condition(w, &scope)) {
            Some(Ok(false)) => {
                lines.push(paint_status(&Status::Skipped, label));
                results.push((Status::Skipped, Map::new()));
                continue;
            }
            Some(Err(e)) => {
                let s = Status::Failed(format!("when: {}", e));
                lines.push(format!("{} => when: {}", paint_status(&s, label), e));
                results.push((s, Map::new()));
                continue;
            }
            _ => {}
        }
        let args = match render_value(raw_args, &scope) {
            Ok(a) => a,
            Err(e) => {
                let s = Status::Failed(e.clone());
                lines.push(format!("{} => {}", paint_status(&s, label), e));
                results.push((s, Map::new()));
                continue;
            }
        };
        // check_mode: false runs the task even in check mode (e.g. to read state)
        let check = o.check && task.get("check_mode").is_none_or(|v| truthy(&Some(v.clone())));
        let ctx = Ctx { conn, sudo: sudo.clone(), check, vars: &scope, facts, base_dir };
        let mut r = match run_module(module, &args, &ctx) {
            Ok(r) => r,
            Err(e) => TaskResult::new(Status::Failed(e)),
        };
        // changed_when overrides the change report
        if let Some(cw) = task.get("changed_when") {
            let mut s2 = scope.clone();
            if let J::Object(m) = &mut s2 {
                m.insert("result".into(), J::Object(r.data.clone()));
            }
            if matches!(r.status, Status::Ok | Status::Changed) {
                r.status = if condition(cw, &s2).unwrap_or(true) { Status::Changed } else { Status::Ok };
            }
        }
        r.data.insert("changed".into(), J::Bool(r.status == Status::Changed));
        r.data.insert("failed".into(), J::Bool(matches!(r.status, Status::Failed(_))));
        r.data.insert("skipped".into(), J::Bool(r.status == Status::Skipped));
        if let Some(it) = &item {
            r.data.insert("item".into(), it.clone());
        }
        let line = match &r.status {
            Status::Failed(m) | Status::Unreachable(m) => format!("{}: {}", paint_status(&r.status, label.clone()), if matches!(r.status, Status::Unreachable(_)) { format!("UNREACHABLE! => {}", m) } else { format!("FAILED! => {}", m) }),
            s => {
                let mut l = paint_status(s, label.clone());
                if let Some(show) = &r.show {
                    l.push_str(&format!(" => {}", show));
                } else if o.verbose {
                    if let Some(J::String(out)) = r.data.get("stdout") {
                        if !out.is_empty() {
                            l.push_str(&format!(" => {}", out));
                        }
                    }
                }
                l
            }
        };
        lines.push(line);
        let unreachable = matches!(r.status, Status::Unreachable(_));
        results.push((r.status, r.data));
        if unreachable {
            break;
        }
    }
    (lines, results)
}

/// Runs plays; returns the printed lines and the recap.
fn run_plays(plays: &J, inv: &Inventory, o: &Options, base_dir: &Path) -> Result<Run, String> {
    let plays = match plays {
        J::Array(a) => a.clone(),
        J::Object(_) => vec![plays.clone()],
        _ => return Err(io_err!("playbook: expected a list of plays")),
    };
    let mut out: Vec<String> = Vec::new();
    let mut all_stats: BTreeMap<String, Stats> = BTreeMap::new();
    let flush = |out: &mut Vec<String>| {
        if !o.quiet {
            for l in out.drain(..) {
                println!("{}", l);
            }
        }
    };
    for play in &plays {
        let pname = play.get("name").map(text_of).unwrap_or_else(|| text_of(play.get("hosts").unwrap_or(&J::Null)));
        let mut names = inv.select(play.get("hosts").unwrap_or(&J::Null)).map_err(|e| io_err!("playbook: {}", e))?;
        if let Some(limit) = &o.limit {
            let allowed: HashSet<String> = limit.iter().flat_map(|l| inv.select(&J::String(l.clone())).unwrap_or_default()).collect();
            names.retain(|n| allowed.contains(n));
        }
        out.push(banner(&format!("PLAY [{}]", pname)));
        let play_vars = play.get("vars").and_then(J::as_object).cloned().unwrap_or_default();
        let mut hosts: Vec<HostState> = names
            .iter()
            .map(|n| {
                let h = inv.host(n);
                HostState {
                    name: n.clone(),
                    conn: match &h.target {
                        Some(t) if !h.local => Conn::Ssh(t.clone()),
                        _ => Conn::Local,
                    },
                    become_password: h.become_password.clone(),
                    vars: inv.host_vars(n),
                    facts: Map::new(),
                    runtime: Map::new(),
                    dead: false,
                    notified: Vec::new(),
                    stats: all_stats.get(n).cloned().unwrap_or_default(),
                }
            })
            .collect();
        if hosts.is_empty() {
            out.push("skipping: no hosts matched".into());
            flush(&mut out);
            continue;
        }
        // facts
        if play.get("gather_facts").is_none_or(|v| truthy(&Some(v.clone()))) {
            out.push(banner("TASK [Gathering Facts]"));
            let facts: Vec<Result<Map<String, J>, String>> = std::thread::scope(|s| {
                let hs: Vec<_> = hosts.iter().map(|h| s.spawn(move || gather_facts(&h.conn, &h.name))).collect();
                hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("internal error".into()))).collect()
            });
            for (h, f) in hosts.iter_mut().zip(facts) {
                match f {
                    Ok(f) => {
                        h.facts = f;
                        h.stats.ok += 1;
                        out.push(paint(o.color, "32", &format!("ok: [{}]", h.name)));
                    }
                    Err(e) => {
                        h.dead = true;
                        h.stats.unreachable += 1;
                        out.push(paint(o.color, "31", &format!("fatal: [{}]: UNREACHABLE! => {}", h.name, e)));
                    }
                }
            }
            flush(&mut out);
        }
        let play_become = play.get("become");
        let play_tags = tags_of(play.get("tags"));
        let tasks = play.get("tasks").and_then(J::as_array).cloned().unwrap_or_default();
        for task in &tasks {
            let mut tags = tags_of(task.get("tags"));
            tags.extend(play_tags.clone());
            if !o.tags.is_empty() && !tags.iter().any(|t| o.tags.contains(t) || t == "always") {
                continue;
            }
            if tags.iter().any(|t| o.skip_tags.contains(t)) {
                continue;
            }
            if hosts.iter().all(|h| h.dead) {
                break;
            }
            run_task(task, &mut hosts, &play_vars, play_become, o, &inv.groups, base_dir, &mut out, None).map_err(|e| io_err!("playbook: {}", e))?;
            flush(&mut out);
        }
        // handlers, in their order, for the hosts that notified them
        let handlers = play.get("handlers").and_then(J::as_array).cloned().unwrap_or_default();
        for handler in &handlers {
            let hname = handler.get("name").map(text_of).unwrap_or_default();
            let listen = tags_of(handler.get("listen"));
            let who: HashSet<String> = hosts
                .iter()
                .filter(|h| !h.dead && h.notified.iter().any(|n| *n == hname || listen.contains(n)))
                .map(|h| h.name.clone())
                .collect();
            if who.is_empty() {
                continue;
            }
            let mut renamed = handler.clone();
            if let J::Object(m) = &mut renamed {
                m.insert("name".into(), J::String(format!("handler: {}", hname)));
            }
            run_task(&renamed, &mut hosts, &play_vars, play_become, o, &inv.groups, base_dir, &mut out, Some(&who)).map_err(|e| io_err!("playbook: {}", e))?;
            flush(&mut out);
        }
        for h in hosts {
            all_stats.insert(h.name.clone(), h.stats);
        }
    }
    // recap
    out.push(banner("PLAY RECAP"));
    let width = all_stats.keys().map(|k| k.chars().count()).max().unwrap_or(4);
    for (h, s) in &all_stats {
        let color = if s.failed > 0 || s.unreachable > 0 { "31" } else if s.changed > 0 { "33" } else { "32" };
        out.push(format!(
            "{}{} : ok={:<4} changed={:<4} unreachable={:<4} failed={:<4} skipped={:<4} ignored={}",
            paint(o.color, color, h),
            " ".repeat(width - h.chars().count()),
            s.ok,
            s.changed,
            s.unreachable,
            s.failed,
            s.skipped,
            s.ignored
        ));
    }
    flush(&mut out);
    Ok(Run { output: out, stats: all_stats })
}

fn recap_value(stats: &BTreeMap<String, Stats>) -> Value {
    let ok = stats.values().all(|s| s.failed == 0 && s.unreachable == 0);
    let failed: Vec<String> = stats.iter().filter(|(_, s)| s.failed > 0 || s.unreachable > 0).map(|(h, _)| h.clone()).collect();
    let recap: Vec<(&str, Value)> = stats
        .iter()
        .map(|(h, s)| {
            (
                h.as_str(),
                map(vec![
                    ("ok", Value::Int(s.ok)),
                    ("changed", Value::Int(s.changed)),
                    ("unreachable", Value::Int(s.unreachable)),
                    ("failed", Value::Int(s.failed)),
                    ("skipped", Value::Int(s.skipped)),
                    ("ignored", Value::Int(s.ignored)),
                ]),
            )
        })
        .collect();
    map(vec![("ok", Value::Bool(ok)), ("failed", crate::native::util::str_list(failed)), ("recap", map(recap))])
}

fn options_from(o: &Map<String, J>) -> Options {
    let list = |k: &str| -> Vec<String> { tags_of(o.get(k)) };
    Options {
        check: o.get("check").is_some_and(|v| truthy(&Some(v.clone()))),
        limit: o.get("limit").map(|_| list("limit")),
        tags: list("tags"),
        skip_tags: list("skipTags"),
        forks: o.get("forks").and_then(num_of).map_or(5, |f| f as usize),
        extra: o.get("vars").and_then(J::as_object).cloned().unwrap_or_default(),
        verbose: o.get("verbose").is_some_and(|v| truthy(&Some(v.clone()))),
        quiet: o.get("quiet").is_some_and(|v| truthy(&Some(v.clone()))),
        color: false,
    }
}

/// `playbook.run(playbook, { inventory, vars, check, limit, tags, skipTags, forks, verbose, quiet })`
fn run_fn(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "playbook.run")?;
    let source = to_json(&args[0]);
    let base_dir = match &source {
        J::String(p) => Path::new(p).parent().map(Path::to_path_buf).unwrap_or_default(),
        _ => PathBuf::from("."),
    };
    let plays = load_data(&source, "playbook")?;
    let opts = match args.get(1) {
        Some(v @ Value::Map(_)) => to_json(v).as_object().cloned().unwrap_or_default(),
        _ => Map::new(),
    };
    let inv = load_inventory(opts.get("inventory").unwrap_or(&J::Null))?;
    let o = options_from(&opts);
    // hosts run in parallel threads; the call returns when the playbook ends
    let run = run_plays(&plays, &inv, &o, &base_dir)?;
    Ok(recap_value(&run.stats))
}

// =====================================================================
// fogu play
// =====================================================================

/// `fogu play <playbook.yml> [-i inventory] [--check] [--limit x] [--tags a,b]
/// [--skip-tags c] [-e key=value]... [-f forks] [-v]`
pub fn play_cli(args: &[String]) -> i32 {
    let mut book: Option<String> = None;
    let mut opts = Map::new();
    let mut extra = Map::new();
    let mut it = args.iter();
    let mut inventory: Option<String> = None;
    while let Some(a) = it.next() {
        let mut val = |what: &str| -> Option<String> {
            let v = it.next().cloned();
            if v.is_none() {
                eprintln!("error: {} expects a value", what);
            }
            v
        };
        match a.as_str() {
            "-i" | "--inventory" => match val(a) {
                Some(v) => inventory = Some(v),
                None => return 2,
            },
            "--check" | "-C" => {
                opts.insert("check".into(), J::Bool(true));
            }
            "-v" | "--verbose" => {
                opts.insert("verbose".into(), J::Bool(true));
            }
            "-l" | "--limit" | "-t" | "--tags" | "--skip-tags" | "-f" | "--forks" | "-e" | "--extra-vars" => {
                let Some(v) = val(a) else { return 2 };
                match a.as_str() {
                    "-l" | "--limit" => {
                        opts.insert("limit".into(), J::String(v));
                    }
                    "-t" | "--tags" => {
                        opts.insert("tags".into(), J::String(v));
                    }
                    "--skip-tags" => {
                        opts.insert("skipTags".into(), J::String(v));
                    }
                    "-f" | "--forks" => {
                        opts.insert("forks".into(), J::from(v.parse::<i64>().unwrap_or(5)));
                    }
                    _ => {
                        // key=value or JSON/YAML
                        match v.split_once('=') {
                            Some((k, val)) if !v.trim_start().starts_with('{') => {
                                let parsed = serde_yaml::from_str::<J>(val).unwrap_or(J::String(val.to_string()));
                                extra.insert(k.to_string(), parsed);
                            }
                            _ => match serde_yaml::from_str::<J>(&v) {
                                Ok(J::Object(m)) => extra.extend(m),
                                _ => {
                                    eprintln!("error: -e expects key=value or a map");
                                    return 2;
                                }
                            },
                        }
                    }
                }
            }
            s if s.starts_with('-') => {
                eprintln!("error: unknown option {}", s);
                return 2;
            }
            s => book = Some(s.to_string()),
        }
    }
    let Some(book) = book else {
        eprintln!("usage: fogu play <playbook.yml> [-i inventory.yml] [--check] [-l hosts] [-t tags] [-e key=value] [-v]");
        return 2;
    };
    let strip = |e: String| e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).map(String::from).unwrap_or(e);
    let plays = match load_data(&J::String(book.clone()), "playbook") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {}", strip(e));
            return 2;
        }
    };
    let inv = match load_inventory(&inventory.map(J::String).unwrap_or(J::Null)) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {}", strip(e));
            return 2;
        }
    };
    opts.insert("vars".into(), J::Object(extra));
    let mut o = options_from(&opts);
    o.color = std::env::var_os("NO_COLOR").is_none() && std::io::IsTerminal::is_terminal(&std::io::stdout());
    let base = Path::new(&book).parent().map(Path::to_path_buf).unwrap_or_default();
    match run_plays(&plays, &inv, &o, &base) {
        Ok(r) => {
            let _ = r.output;
            if r.stats.values().any(|s| s.failed > 0 || s.unreachable > 0) {
                2
            } else {
                0
            }
        }
        Err(e) => {
            eprintln!("error: {}", strip(e));
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_and_conditions() {
        let v = json!({ "app": "shop", "port": 8080, "list": ["a", "b"], "facts": { "os": "linux" }, "empty": "" });
        assert_eq!(render("{{ app }}:{{ port }}", &v).unwrap(), "shop:8080");
        assert_eq!(render("{{ list | join(',') | upper }}", &v).unwrap(), "A,B");
        assert_eq!(render("{{ missing | default('x') }} {{ list | length }}", &v).unwrap(), "x 2");
        assert!(render("{{ missing }}", &v).is_err());
        assert_eq!(render_value(&json!("{{ list }}"), &v).unwrap(), json!(["a", "b"]));
        assert!(condition(&json!("facts.os == 'linux' and port > 80"), &v).unwrap());
        assert!(condition(&json!("'a' in list and missing is not defined"), &v).unwrap());
        assert!(!condition(&json!("not app or empty"), &v).unwrap());
        assert!(condition(&json!(["port >= 8080", "app != 'web'"]), &v).unwrap());
        assert_eq!(render("{{ port + 1 }} {{ app ~ '-' ~ port }} {{ list[-1] }}", &v).unwrap(), "8081 shop-8080 b");
    }

    #[test]
    fn inventory_selection() {
        let inv = load_inventory(&json!({
            "vars": { "env": "prod" },
            "hosts": { "a": { "host": "10.0.0.1", "user": "u" }, "b": {}, "me": { "connection": "local" } },
            "groups": { "web": { "hosts": ["a", "b"], "vars": { "port": 80 } }, "db": ["b"] }
        }))
        .unwrap();
        assert_eq!(inv.select(&json!("web")).unwrap(), vec!["a", "b"]);
        assert_eq!(inv.select(&json!("web:!db")).unwrap(), vec!["a"]);
        assert_eq!(inv.select(&json!("all")).unwrap().len(), 3);
        assert!(inv.select(&json!("nope")).is_err());
        assert_eq!(inv.host_vars("a").get("port"), Some(&json!(80)));
        assert_eq!(inv.host_vars("me").get("env"), Some(&json!("prod")));
        assert!(inv.host("me").local);
    }
}
