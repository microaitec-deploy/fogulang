//! Small helpers shared by the native modules.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::{NativeFnValue, Value};

/// Native function with at least `min` arguments (the rest are optional).
pub fn nfn_var(name: &str, min: usize, f: fn(&[Value]) -> Result<Value, String>) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.into(),
        arity: min,
        variadic: true,
        func: f,
        intercept: Default::default(),
    }))
}

/// Argument `i` as a string (`what` names the function in the error).
pub fn str_arg(args: &[Value], i: usize, what: &str) -> Result<String, String> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(other) => Err(format!(
            "{}: argument {} must be str, got '{}'",
            what,
            i + 1,
            other.type_name()
        )),
        None => Err(format!("{}: missing argument {}", what, i + 1)),
    }
}

/// Optional argument `i` (missing or `nil` → `None`).
pub fn opt_arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !matches!(v, Value::Nil))
}

/// Fails when more than `max` arguments were passed.
pub fn max_args(args: &[Value], max: usize, what: &str) -> Result<(), String> {
    if args.len() > max {
        return Err(format!("{} expects at most {} arguments, got {}", what, max, args.len()));
    }
    Ok(())
}

pub fn list(items: Vec<Value>) -> Value {
    Value::List(Rc::new(RefCell::new(items)))
}

pub fn str_list<I: IntoIterator<Item = String>>(items: I) -> Value {
    list(items.into_iter().map(Value::str).collect())
}

pub fn map(pairs: Vec<(&str, Value)>) -> Value {
    let m: HashMap<String, Value> = pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::Map(Rc::new(RefCell::new(m)))
}

/// A duration (`250ms`) or a number of milliseconds, as a `std` duration.
pub fn duration_arg(v: &Value, what: &str) -> Result<std::time::Duration, String> {
    match v {
        Value::Duration(ns) if *ns >= 0 => Ok(std::time::Duration::from_nanos(*ns as u64)),
        Value::Int(ms) if *ms >= 0 => Ok(std::time::Duration::from_millis(*ms as u64)),
        other => Err(format!(
            "{} expects a duration (e.g. 500ms) or int ms >= 0, got '{}'",
            what,
            other.type_name()
        )),
    }
}

/// Paths are shown with forward slashes on every platform.
pub fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Runs blocking `work` (network I/O) on the worker pool and returns a
/// poller for [`FoguObject::start_async`](crate::native::object::FoguObject::start_async):
/// the other `fg` tasks keep running meanwhile, and the scheduler is woken
/// when it finishes. `finish` turns the result into a Fogu value on the main
/// thread.
pub fn thread_poller<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    finish: impl FnOnce(T) -> Result<Value, String> + 'static,
) -> crate::native::object::Poller {
    let pending = crate::native::rt::spawn_blocking(work);
    let mut finish = Some(finish);
    Box::new(move || match pending.try_take()? {
        Ok(v) => Some(finish.take().expect("polled after completion")(v)),
        Err(e) => Some(Err(e)),
    })
}

/// A `serde_json` value as a Fogu value (objects keep their key order).
pub fn json_to_value(j: &serde_json::Value) -> Value {
    crate::native::json::parse_value(&j.to_string()).unwrap_or(Value::Nil)
}

/// Sleeps without blocking the other `fg` tasks: they run while this one
/// (or the main program) waits.
pub fn sleep_coop(d: std::time::Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + d;
    crate::runtime::sched::add_timer(deadline);
    crate::runtime::sched::wait_until(|| {
        let done = std::time::Instant::now() >= deadline;
        if !done {
            crate::runtime::sched::add_timer(deadline);
        }
        done
    })
}

/// Bytes from a str (UTF-8) or a list of ints.
pub fn bytes_of(v: &Value, what: &str) -> Result<Vec<u8>, String> {
    match v {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::Buffer(b) => Ok(b.borrow().clone()),
        Value::List(l) => l
            .borrow()
            .iter()
            .map(|b| match b {
                Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                _ => Err(format!("{}: byte lists must hold ints 0..255", what)),
            })
            .collect(),
        other => Err(format!("{}: expected str or a list of bytes, got '{}'", what, other.type_name())),
    }
}

pub fn bytes_value(b: Vec<u8>) -> Value {
    Value::buffer(b)
}

/// `%XX` and `+` decoding (URLs, form data).
pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The process id (0 on WebAssembly, which has no processes).
pub fn pid() -> u32 {
    if cfg!(target_family = "wasm") { 0 } else { std::process::id() }
}

/// The HTTP client of the package manager, shared with the runtime.
pub use fogu_pkg::net::error_chain;
#[cfg(not(target_family = "wasm"))]
pub use fogu_pkg::net::blocking_client;
