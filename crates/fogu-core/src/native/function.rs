use std::rc::Rc;

use crate::runtime::value::{NativeFnValue, Value};

/// Signature of a native function.
///
/// Receives the already evaluated arguments and returns a `Value` or an error.
pub type NativeFnPtr = fn(&[Value]) -> Result<Value, String>;

/// Ergonomic constructor for a `NativeFnValue`.
pub struct NativeFunction;

impl NativeFunction {
    /// Creates a native function with a fixed arity.
    pub fn fixed(
        name: &str,
        arity: usize,
        f: NativeFnPtr,
    ) -> Rc<NativeFnValue> {
        Rc::new(NativeFnValue {
            name: name.to_string(),
            arity,
            variadic: false,
            func: f,
            intercept: Default::default(),
        })
    }

    /// Creates a variadic native function.
    pub fn variadic(
        name: &str,
        min_arity: usize,
        f: NativeFnPtr,
    ) -> Rc<NativeFnValue> {
        Rc::new(NativeFnValue {
            name: name.to_string(),
            arity: min_arity,
            variadic: true,
            func: f,
            intercept: Default::default(),
        })
    }

    /// Shortcut: returns the `Value::NativeFn` ready to register.
    pub fn value_fixed(name: &str, arity: usize, f: NativeFnPtr) -> Value {
        Value::NativeFn(Self::fixed(name, arity, f))
    }

    pub fn value_variadic(name: &str, min_arity: usize, f: NativeFnPtr) -> Value {
        Value::NativeFn(Self::variadic(name, min_arity, f))
    }
}