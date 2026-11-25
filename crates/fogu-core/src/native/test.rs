//! `fogu:test` module — unit tests run by `fogu test` (spec §63).
//!
//! ```fogu
//! import { test, expect } from "fogu:test"
//! import { sum } from "./util.fg"
//!
//! test("adds two integers", () => {
//!     expect(sum(1, 2)).toBe(3)
//!     expect([1, 2]).not.toContain(3)
//! })
//! ```
//!
//! `test(name, fn)` only registers the test; `fogu test` loads each
//! `*.test.fg` file and then runs the registered functions. A failed
//! expectation is an error that stops the test.
//!
//! As in Bun: `describe(name, fn)` groups tests; `beforeAll`, `afterAll`,
//! `beforeEach` and `afterEach` attach to the group they are declared in
//! (the file, at the top level); `test.each(rows)(name, fn)` and
//! `describe.each` run one test (or group) per row; `test.skip`,
//! `test.only`, `test.todo`, `test.skipIf(cond)`, `describe.skip`,
//! `describe.only`. Order: a group's `beforeAll` before its first test, its
//! `afterAll` after its last one; the `beforeEach` of the outer groups first,
//! the `afterEach` of the inner ones first. Cleanup always runs: `afterEach`
//! after a failed test (or a failed `beforeEach`), `afterAll` after failures.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;

use crate::runtime::interpreter::values_equal;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::str_arg;

// ---------------------------------------------------------------- registry
//
// Loading a test file only registers: `test(...)` adds a test to the group
// being declared, `describe(name, fn)` adds a group whose body runs later
// (by the runner, which has the interpreter), the hooks attach to the group
// being declared. The runner then walks the tree in declaration order.

/// A registered test.
pub struct TestCase {
    /// Its full name: the names of its groups and its own, joined by " > ".
    pub name: String,
    pub func: Value,
    /// The arguments of a `test.each` row.
    pub args: Vec<Value>,
    pub skip: bool,
    pub only: bool,
    pub todo: bool,
    pub group: usize,
}

/// A `describe` group (0 is the file itself).
pub struct Group {
    pub name: String,
    pub parent: Option<usize>,
    /// The body, until the runner has run it (`describe(name, fn)`).
    pub body: Option<(Value, Vec<Value>)>,
    pub skip: bool,
    pub only: bool,
    pub before_all: Vec<Value>,
    pub after_all: Vec<Value>,
    pub before_each: Vec<Value>,
    pub after_each: Vec<Value>,
    /// Tests and groups, in declaration order.
    pub items: Vec<Item>,
}

#[derive(Clone, Copy)]
pub enum Item {
    Test(usize),
    Group(usize),
}

#[derive(Default)]
struct Registry {
    tests: Vec<TestCase>,
    groups: Vec<Group>,
    current: usize,
}

fn root() -> Group {
    Group {
        name: String::new(),
        parent: None,
        body: None,
        skip: false,
        only: false,
        before_all: Vec::new(),
        after_all: Vec::new(),
        before_each: Vec::new(),
        after_each: Vec::new(),
        items: Vec::new(),
    }
}

thread_local! {
    static REG: RefCell<Registry> = RefCell::new(Registry { tests: Vec::new(), groups: vec![root()], current: 0 });
}

/// Forgets everything registered (a new file).
pub fn reset() {
    REG.with(|r| *r.borrow_mut() = Registry { tests: Vec::new(), groups: vec![root()], current: 0 });
}

/// The next `describe` body to run, with its group made the current one.
pub fn next_body() -> Option<(Value, Vec<Value>)> {
    REG.with(|r| {
        let mut r = r.borrow_mut();
        let g = r.groups.iter().position(|g| g.body.is_some())?;
        r.current = g;
        r.groups[g].body.take()
    })
}

/// Back to the file's own level once the bodies have run.
pub fn end_bodies() {
    REG.with(|r| r.borrow_mut().current = 0);
}

/// Takes what was registered: the tests and the groups.
pub fn take() -> (Vec<TestCase>, Vec<Group>) {
    REG.with(|r| {
        let mut r = r.borrow_mut();
        (std::mem::take(&mut r.tests), std::mem::replace(&mut r.groups, vec![root()]))
    })
}

/// The full name of a test declared in the current group.
fn full_name(r: &Registry, name: &str) -> String {
    let mut parts = vec![name.to_string()];
    let mut g = Some(r.current);
    while let Some(i) = g {
        if i == 0 {
            break;
        }
        parts.push(r.groups[i].name.clone());
        g = r.groups[i].parent;
    }
    parts.reverse();
    parts.join(" > ")
}

fn function(v: &Value, what: &str) -> Result<Value, String> {
    match v {
        Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) => Ok(v.clone()),
        other => Err(format!("{}: expects a function, got '{}'", what, other.type_name())),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Skip,
    Only,
    Todo,
}

fn add_test(name: String, func: Value, args: Vec<Value>, mode: Mode) {
    REG.with(|r| {
        let mut r = r.borrow_mut();
        let group = r.current;
        let name = full_name(&r, &name);
        let id = r.tests.len();
        r.tests.push(TestCase { name, func, args, skip: mode == Mode::Skip, only: mode == Mode::Only, todo: mode == Mode::Todo, group });
        r.groups[group].items.push(Item::Test(id));
    });
}

fn add_group(name: String, body: Value, args: Vec<Value>, mode: Mode) {
    REG.with(|r| {
        let mut r = r.borrow_mut();
        let parent = r.current;
        let id = r.groups.len();
        let mut g = root();
        g.name = name;
        g.parent = Some(parent);
        g.body = Some((body, args));
        g.skip = mode == Mode::Skip;
        g.only = mode == Mode::Only;
        r.groups.push(g);
        r.groups[parent].items.push(Item::Group(id));
    });
}

fn add_hook(kind: &str, f: Value) {
    REG.with(|r| {
        let mut r = r.borrow_mut();
        let g = r.current;
        let g = &mut r.groups[g];
        match kind {
            "beforeAll" => g.before_all.push(f),
            "afterAll" => g.after_all.push(f),
            "beforeEach" => g.before_each.push(f),
            _ => g.after_each.push(f),
        }
    });
}

/// `test.each` / `describe.each` names: `%s %d %i %f %j %o %p` take the
/// row's values in order, `%#` is the row's index, `%%` a percent sign, and
/// `$key` a field of a map row.
fn format_name(template: &str, row: &Value, index: usize) -> String {
    let values: Vec<Value> = match row {
        Value::List(l) => l.borrow().clone(),
        Value::Tuple(t) => t.to_vec(),
        other => vec![other.clone()],
    };
    let mut out = String::new();
    let mut next = values.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek().copied() {
                Some('%') => {
                    chars.next();
                    out.push('%');
                }
                Some('#') => {
                    chars.next();
                    out.push_str(&index.to_string());
                }
                Some('s' | 'd' | 'i' | 'f' | 'j' | 'o' | 'p') => {
                    let k = chars.next().unwrap_or('s');
                    let v = next.next().cloned().unwrap_or(Value::Nil);
                    match k {
                        'j' | 'o' | 'p' => out.push_str(&crate::native::json::stringify_value(&v)),
                        'd' | 'i' => match v {
                            Value::Float(f) => out.push_str(&(f as i64).to_string()),
                            other => out.push_str(&other.to_display_string()),
                        },
                        _ => out.push_str(&v.to_display_string()),
                    }
                }
                _ => out.push('%'),
            }
        } else if c == '$' && matches!(row, Value::Map(_)) && chars.peek().is_some_and(|c| c.is_alphanumeric() || *c == '_') {
            let mut key = String::new();
            while let Some(&k) = chars.peek() {
                if k.is_alphanumeric() || k == '_' {
                    key.push(k);
                    chars.next();
                } else {
                    break;
                }
            }
            let v = match row {
                Value::Map(m) => m.borrow().get(key.as_str()).cloned().unwrap_or(Value::Nil),
                _ => Value::Nil,
            };
            out.push_str(&v.to_display_string());
        } else {
            out.push(c);
        }
    }
    out
}

/// The arguments a row gives its test: the values of a list (or tuple) row,
/// or the row itself (a map, a single value).
fn row_args(row: &Value) -> Vec<Value> {
    match row {
        Value::List(l) => l.borrow().clone(),
        Value::Tuple(t) => t.to_vec(),
        other => vec![other.clone()],
    }
}

/// The object behind `test`, `describe` and the hooks: they are its methods,
/// bound (`test(...)` calls `test`), and the variants are its fields
/// (`test.each`, `test.skip`, `describe.only` …).
struct TestApi {
    /// Which callable this is: "test" or "describe" (the fields depend on it).
    of: &'static str,
}

/// The value returned by `test.each(rows)`: called with `(name, fn)`, it
/// registers a test per row.
struct Each {
    rows: Vec<Value>,
    group: bool,
    mode: Mode,
}

fn api(of: &'static str) -> NativeObjectRef {
    NativeObjectRef::new(TestApi { of })
}

impl FoguObject for TestApi {
    fn name(&self) -> &str {
        "TestApi"
    }

    fn get(&self, field: &str) -> Option<Value> {
        // test.skip / test.only / test.todo / test.each, describe.skip / .only / .each
        let known = match (self.of, field) {
            ("test", "skip" | "only" | "todo" | "each" | "skipIf" | "if") => true,
            ("describe", "skip" | "only" | "each") => true,
            _ => false,
        };
        known.then(|| Value::bound(api(self.of), &format!("{}.{}", self.of, field)))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let name_fn = |what: &str| -> Result<(String, Value), String> {
            let name = str_arg(args, 0, what)?;
            let f = function(args.get(1).unwrap_or(&Value::Nil), what)?;
            Ok((name, f))
        };
        match method {
            "test" | "it" | "test.skip" | "test.only" | "test.todo" => {
                let mode = match method {
                    "test.skip" => Mode::Skip,
                    "test.only" => Mode::Only,
                    "test.todo" => Mode::Todo,
                    _ => Mode::Normal,
                };
                if mode == Mode::Todo && args.len() < 2 {
                    let name = str_arg(args, 0, method)?;
                    add_test(name, Value::Nil, Vec::new(), Mode::Todo);
                    return Ok(Value::Nil);
                }
                let (name, f) = name_fn(method)?;
                add_test(name, f, Vec::new(), mode);
                Ok(Value::Nil)
            }
            // test.skipIf(cond)(name, fn) · test.if(cond)(name, fn)
            "test.skipIf" | "test.if" => {
                let cond = args.first().is_some_and(|v| v.is_truthy());
                let skip = if method == "test.skipIf" { cond } else { !cond };
                let mode = if skip { Mode::Skip } else { Mode::Normal };
                Ok(Value::bound(NativeObjectRef::new(Each { rows: Vec::new(), group: false, mode }), "one"))
            }
            "describe" | "describe.skip" | "describe.only" => {
                let mode = match method {
                    "describe.skip" => Mode::Skip,
                    "describe.only" => Mode::Only,
                    _ => Mode::Normal,
                };
                let (name, f) = name_fn(method)?;
                add_group(name, f, Vec::new(), mode);
                Ok(Value::Nil)
            }
            "test.each" | "describe.each" => {
                let rows = match args.first() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    Some(Value::Array(a)) => a.borrow().to_values(),
                    Some(other) => return Err(format!("{}(rows): rows must be a list, got '{}'", method, other.type_name())),
                    None => return Err(format!("{}(rows) expects a list of rows", method)),
                };
                Ok(Value::bound(NativeObjectRef::new(Each { rows, group: method == "describe.each", mode: Mode::Normal }), "run"))
            }
            "beforeAll" | "afterAll" | "beforeEach" | "afterEach" => {
                let f = function(args.first().unwrap_or(&Value::Nil), method)?;
                add_hook(method, f);
                Ok(Value::Nil)
            }
            other => Err(format!("'{}' is not part of fogu:test", other)),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl FoguObject for Each {
    fn name(&self) -> &str {
        "TestEach"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let what = if self.group { "describe.each(rows)" } else { "test.each(rows)" };
        let template = str_arg(args, 0, what)?;
        let f = function(args.get(1).unwrap_or(&Value::Nil), what)?;
        if method == "one" {
            add_test(template, f, Vec::new(), self.mode);
            return Ok(Value::Nil);
        }
        for (i, row) in self.rows.iter().enumerate() {
            let name = format_name(&template, row, i);
            if self.group {
                add_group(name, f.clone(), row_args(row), self.mode);
            } else {
                add_test(name, f.clone(), row_args(row), self.mode);
            }
        }
        Ok(Value::Nil)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("test".into(), Value::bound(api("test"), "test"));
    e.insert("it".into(), Value::bound(api("test"), "it"));
    e.insert("describe".into(), Value::bound(api("describe"), "describe"));
    for hook in ["beforeAll", "afterAll", "beforeEach", "afterEach"] {
        e.insert(hook.into(), Value::bound(api("test"), hook));
    }
    e.insert("skip".into(), Value::bound(api("test"), "test.skip"));
    e.insert("expect".into(), nfn("expect", 1, |a| {
        Ok(NativeObjectRef::new(Expectation { value: a[0].clone(), negate: false }).to_value())
    }));
    ModuleValue {
        name: "fogu:test".into(),
        exports: RefCell::new(e),
    }
}

/// Value shown in failure messages (strings are quoted).
fn repr(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{:?}", s),
        other => other.to_display_string(),
    }
}

pub struct Expectation {
    value: Value,
    negate: bool,
}

impl Expectation {
    /// Checks `ok` (inverted by `.not`); `what` describes the expectation.
    fn check(&self, ok: bool, what: String) -> Result<Value, String> {
        if ok != self.negate {
            return Ok(Value::Nil);
        }
        let not = if self.negate { "not " } else { "" };
        Err(format!("expected {} {}{}", repr(&self.value), not, what))
    }

    fn arg<'a>(args: &'a [Value], method: &str) -> Result<&'a Value, String> {
        args.first().ok_or_else(|| format!("{}(…) expects an argument", method))
    }

    fn number(v: &Value, method: &str) -> Result<f64, String> {
        match v {
            Value::Int(n) => Ok(*n as f64),
            Value::Float(f) => Ok(*f),
            Value::Duration(ns) => Ok(*ns as f64),
            other => Err(format!("{}: expected a number, got '{}'", method, other.type_name())),
        }
    }
}

impl FoguObject for Expectation {
    fn name(&self) -> &str {
        "Expectation"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "not").then(|| {
            NativeObjectRef::new(Expectation { value: self.value.clone(), negate: !self.negate }).to_value()
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let v = &self.value;
        match method {
            "toBe" | "toEqual" => {
                let x = Self::arg(args, method)?;
                self.check(values_equal(v, x), format!("to be {}", repr(x)))
            }
            "toBeTruthy" => self.check(v.is_truthy(), "to be truthy".into()),
            "toBeFalsy" => self.check(!v.is_truthy(), "to be falsy".into()),
            "toBeNil" => self.check(matches!(v, Value::Nil), "to be nil".into()),
            "toBeError" => self.check(matches!(v, Value::Error(_)), "to be an error".into()),
            "toBeType" => {
                let t = str_arg(args, 0, "toBeType")?;
                self.check(v.type_name() == t, format!("to have type {} (it is {})", t, v.type_name()))
            }
            "toContain" => {
                let x = Self::arg(args, method)?;
                let ok = match (v, x) {
                    (Value::List(l), _) => l.borrow().iter().any(|i| values_equal(i, x)),
                    (Value::Str(s), Value::Str(sub)) => s.contains(sub.as_str()),
                    (Value::Map(m), Value::Str(k)) => m.borrow().contains_key(k.as_str()),
                    _ => false,
                };
                self.check(ok, format!("to contain {}", repr(x)))
            }
            "toHaveLength" => {
                let n = match Self::arg(args, method)? {
                    Value::Int(n) => *n,
                    other => return Err(format!("toHaveLength expects int, got '{}'", other.type_name())),
                };
                let len = match v {
                    Value::List(l) => l.borrow().len() as i64,
                    Value::Map(m) => m.borrow().len() as i64,
                    Value::Str(s) => s.chars().count() as i64,
                    other => return Err(format!("toHaveLength: '{}' has no length", other.type_name())),
                };
                self.check(len == n, format!("to have length {} (it has {})", n, len))
            }
            "toBeGreaterThan" | "toBeLessThan" => {
                let a = Self::number(v, method)?;
                let b = Self::number(Self::arg(args, method)?, method)?;
                let (ok, word) = if method == "toBeGreaterThan" { (a > b, "greater") } else { (a < b, "less") };
                self.check(ok, format!("to be {} than {}", word, repr(&args[0])))
            }
            "toBeCloseTo" => {
                let a = Self::number(v, method)?;
                let b = Self::number(Self::arg(args, method)?, method)?;
                let eps = match args.get(1) {
                    Some(e) => Self::number(e, method)?,
                    None => 1e-9,
                };
                self.check((a - b).abs() <= eps, format!("to be close to {} (±{})", repr(&args[0]), eps))
            }
            other => Err(format!("method '{}' does not exist on Expectation", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "toBe", "toEqual", "toBeTruthy", "toBeFalsy", "toBeNil", "toBeError", "toBeType",
            "toContain", "toHaveLength", "toBeGreaterThan", "toBeLessThan", "toBeCloseTo",
        ]
    }
}
