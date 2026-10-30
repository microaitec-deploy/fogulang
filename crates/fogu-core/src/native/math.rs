use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    exports.insert("PI".into(), Value::Float(std::f64::consts::PI));
    exports.insert("E".into(), Value::Float(std::f64::consts::E));

    exports.insert(
        "abs".into(),
        nfn("abs", 1, |args| match &args[0] {
            Value::Int(n) => n
                .checked_abs()
                .map(Value::Int)
                .ok_or_else(|| crate::runtime::ops::ERR_OVERFLOW.to_string()),
            Value::Float(n) => Ok(Value::Float(n.abs())),
            other => Err(format!("'abs' not applicable to '{}'", other.type_name())),
        }),
    );

    exports.insert(
        "sqrt".into(),
        nfn("sqrt", 1, |args| match &args[0] {
            Value::Int(n) => Ok(Value::Float((*n as f64).sqrt())),
            Value::Float(n) => Ok(Value::Float(n.sqrt())),
            other => Err(format!("'sqrt' not applicable to '{}'", other.type_name())),
        }),
    );

    exports.insert(
        "pow".into(),
        nfn("pow", 2, |args| {
            let a = num(&args[0])?;
            let b = num(&args[1])?;
            Ok(Value::Float(a.powf(b)))
        }),
    );

    exports.insert(
        "floor".into(),
        nfn("floor", 1, |args| match &args[0] {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::Float(n) => Ok(Value::Int(n.floor() as i64)),
            other => Err(format!("'floor' not applicable to '{}'", other.type_name())),
        }),
    );

    exports.insert(
        "ceil".into(),
        nfn("ceil", 1, |args| match &args[0] {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::Float(n) => Ok(Value::Int(n.ceil() as i64)),
            other => Err(format!("'ceil' not applicable to '{}'", other.type_name())),
        }),
    );

    ModuleValue {
        name: "fogu:math".into(),
        exports: RefCell::new(exports),
    }
}

pub fn nfn(
    name: &str,
    arity: usize,
    f: fn(&[Value]) -> Result<Value, String>,
) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.into(),
        arity,
        variadic: false,
        func: f,
        intercept: Default::default(),
    }))
}

fn num(v: &Value) -> Result<f64, String> {
    match v {
        Value::Int(n) => Ok(*n as f64),
        Value::Float(n) => Ok(*n),
        other => Err(format!("expected a number, got '{}'", other.type_name())),
    }
}