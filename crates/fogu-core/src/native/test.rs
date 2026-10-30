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

use std::cell::RefCell;
use ahash::AHashMap as HashMap;

use crate::runtime::interpreter::values_equal;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::str_arg;

pub struct TestCase {
    pub name: String,
    pub func: Value,
    pub skip: bool,
}

thread_local! {
    static TESTS: RefCell<Vec<TestCase>> = const { RefCell::new(Vec::new()) };
}

/// Removes and returns the tests registered so far.
pub fn take_tests() -> Vec<TestCase> {
    TESTS.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

fn register(args: &[Value], skip: bool, what: &str) -> Result<Value, String> {
    let name = str_arg(args, 0, what)?;
    let func = args[1].clone();
    if !matches!(func, Value::Fn(_) | Value::NativeFn(_)) {
        return Err(format!("{}: the second argument must be a function, got '{}'", what, func.type_name()));
    }
    TESTS.with(|t| t.borrow_mut().push(TestCase { name, func, skip }));
    Ok(Value::Nil)
}

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("test".into(), nfn("test", 2, |a| register(a, false, "test")));
    e.insert("skip".into(), nfn("skip", 2, |a| register(a, true, "skip")));
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
