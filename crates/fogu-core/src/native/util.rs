//! Small helpers shared by the native modules.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::{NativeFnValue, Str, Value};
pub use crate::runtime::value::{Num, TypedNative};

/// Native function with at least `min` arguments (the rest are optional).
pub fn nfn_var(name: &str, min: usize, f: fn(&[Value]) -> Result<Value, String>) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.into(),
        arity: min,
        variadic: true,
        func: f,
        intercept: Default::default(),
        typed: None,
    }))
}

/// `f` with a typed entry for compiled code (see [`TypedNative`]): `entry` is
/// an `extern "C" fn` over plain numbers that computes the same as `f`.
pub fn with_typed(f: Value, typed: TypedNative) -> Value {
    match f {
        Value::NativeFn(nf) => Value::NativeFn(Rc::new(NativeFnValue {
            name: nf.name.clone(),
            arity: nf.arity,
            variadic: nf.variadic,
            func: nf.func,
            intercept: Default::default(),
            typed: Some(typed),
        })),
        other => other,
    }
}

thread_local! {
    /// The error of the last call of a fallible typed entry ([`TypedNative`]).
    static TYPED_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Called by a fallible typed entry that failed: compiled code raises `msg`
/// as soon as the entry returns.
pub fn set_typed_error(msg: String) {
    TYPED_ERROR.with(|e| *e.borrow_mut() = Some(msg));
}

/// The error a fallible typed entry left, if any.
pub fn take_typed_error() -> Option<String> {
    TYPED_ERROR.with(|e| e.borrow_mut().take())
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
    let m: HashMap<Str, Value> = pairs.into_iter().map(|(k, v)| (Str::from(k), v)).collect();
    Value::Map(Rc::new(RefCell::new(m)))
}

/// A duration (`250ms`) or a number of milliseconds, as a `std` duration.
pub fn duration_arg(v: &Value, what: &str) -> Result<std::time::Duration, String> {
    match v {
        Value::Duration(ns) if *ns >= 0 => Ok(std::time::Duration::from_nanos(*ns as u64)),
        Value::Int(ms) if *ms >= 0 => Ok(std::time::Duration::from_millis(*ms as u64)),
        Value::Duration(_) | Value::Int(_) => Err(format!("{}: the duration must be >= 0, got {}", what, v.to_display_string())),
        other => Err(format!(
            "{} expects a duration (e.g. 500ms) or int ms >= 0, got '{}'",
            what,
            other.type_name()
        )),
    }
}

/// The instant `d` from now; a duration too long for the clock waits "forever"
/// (a hundred years) instead of overflowing it.
pub fn deadline_after(d: std::time::Duration) -> std::time::Instant {
    let now = std::time::Instant::now();
    now.checked_add(d).unwrap_or_else(|| now + std::time::Duration::from_secs(100 * 365 * 86_400))
}

/// The path of `rel` under the folder `root`, for a name that comes from outside
/// the program (a query, a header, an upload): `..`, drive letters and NUL are
/// refused, a leading separator is dropped, and what exists of the result must
/// still be inside the folder once symbolic links are followed.
pub fn contained(root: &str, rel: &str) -> Result<std::path::PathBuf, String> {
    let mut out = std::path::PathBuf::from(if root.is_empty() { "." } else { root });
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(format!("'{}' leaves '{}'", rel, root)),
            p if p.contains(':') || p.contains('\0') => return Err(format!("'{}' is not a name under '{}'", rel, root)),
            p => out.push(p),
        }
    }
    let base = std::path::Path::new(if root.is_empty() { "." } else { root }).canonicalize().map_err(|e| format!("{}: {}", root, e))?;
    // (a file still to be created: its nearest existing folder decides)
    let mut probe = out.as_path();
    let real = loop {
        match probe.canonicalize() {
            Ok(real) => break real,
            Err(_) => match probe.parent() {
                Some(up) if !up.as_os_str().is_empty() => probe = up,
                _ => break base.clone(),
            },
        }
    };
    if !real.starts_with(&base) {
        return Err(format!("'{}' leaves '{}'", rel, root));
    }
    Ok(out)
}

/// True when `p` is the folder `base` or something under it, with `..` and
/// symbolic links resolved (what does not exist yet is resolved by name).
pub fn is_inside(base: &str, p: &str) -> bool {
    fn real(p: &std::path::Path) -> Option<std::path::PathBuf> {
        let abs = std::path::absolute(p).ok()?;
        // lexical first: `a/../b` must not need `a` to exist
        let mut flat = std::path::PathBuf::new();
        for c in abs.components() {
            match c {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    flat.pop();
                }
                other => flat.push(other.as_os_str()),
            }
        }
        // then the links of the part that exists
        let mut rest = Vec::new();
        let mut probe = flat.clone();
        loop {
            if let Ok(done) = probe.canonicalize() {
                let mut done = done;
                for name in rest.iter().rev() {
                    done.push(name);
                }
                return Some(done);
            }
            let name = probe.file_name()?.to_os_string();
            rest.push(name);
            if !probe.pop() {
                return None;
            }
        }
    }
    match (real(std::path::Path::new(base)), real(std::path::Path::new(p))) {
        (Some(base), Some(p)) => p.starts_with(base),
        _ => false,
    }
}

/// A connection URL (or a message that holds one) as it may be shown: the
/// password of `scheme://user:password@host` and of a `password=` parameter
/// becomes `***`. Error messages and logs never carry the secret.
pub fn redact_url(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        // the authority ends at the first of these
        let end = tail.find(['/', '?', '#', ' ', '\'', '"', ')']).unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rfind('@') {
            Some(a) => match authority[..a].find(':') {
                Some(c) => {
                    out.push_str(&authority[..c + 1]);
                    out.push_str("***");
                    out.push_str(&authority[a..]);
                }
                None => out.push_str(authority),
            },
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    for key in ["password=", "passwd=", "pwd=", "sslpassword="] {
        let mut from = 0;
        while let Some(i) = out[from..].to_ascii_lowercase().find(key) {
            let start = from + i + key.len();
            // (a parameter, not the end of a longer name)
            let named = from + i == 0 || !out.as_bytes()[from + i - 1].is_ascii_alphanumeric();
            let end = out[start..].find(['&', ' ', '\'', '"', ';', ')']).map_or(out.len(), |e| start + e);
            if named {
                out.replace_range(start..end, "***");
                from = start + 3;
            } else {
                from = start;
            }
        }
    }
    out
}

/// Paths are shown with forward slashes on every platform.
pub fn path_str(p: &std::path::Path) -> String {
    let shown = p.to_string_lossy().replace('\\', "/");
    // (a canonical path on Windows is `\\?\C:\…`: programs and people expect `C:/…`)
    match shown.strip_prefix("//?/") {
        Some(plain) if !plain.starts_with("UNC/") => plain.to_string(),
        _ => shown,
    }
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
    // converted directly: a round trip through JSON text turned the whole value into nil when
    // a number did not fit in an int (a u64) or the nesting was deep
    use serde_json::Value as J;
    match j {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            // beyond the int range (or a fraction): a float, like JSON.parse in JavaScript
            None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        J::String(s) => Value::str(s.as_str()),
        J::Array(a) => list(a.iter().map(json_to_value).collect()),
        J::Object(o) => {
            let m: HashMap<Str, Value> = o.iter().map(|(k, v)| (Str::from(k.as_str()), json_to_value(v))).collect();
            Value::Map(Rc::new(RefCell::new(m)))
        }
    }
}

thread_local! {
    /// Until when each task may wait: the deadline of the `fogu:vm` run (or of
    /// the test) it is in. Kept by `Interpreter::set_deadline`.
    static WAIT_LIMITS: std::cell::RefCell<HashMap<usize, (std::time::Instant, std::time::Duration)>> = std::cell::RefCell::new(HashMap::new());
    /// `WAIT_LIMITS.len()`: the usual case (none) costs one load.
    static WAIT_LIMIT_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Sets (or clears) the instant after which the task `task` must not be left
/// sleeping, with the timeout it comes from (for the message).
pub fn set_wait_limit(task: usize, limit: Option<(std::time::Instant, std::time::Duration)>) {
    WAIT_LIMITS.with(|m| {
        let mut m = m.borrow_mut();
        match limit {
            Some(l) => {
                m.insert(task, l);
            }
            None => {
                m.remove(&task);
            }
        }
        WAIT_LIMIT_COUNT.with(|c| c.set(m.len()));
    });
}

fn wait_limit() -> Option<(std::time::Instant, std::time::Duration)> {
    if WAIT_LIMIT_COUNT.with(|c| c.get()) == 0 {
        return None;
    }
    WAIT_LIMITS.with(|m| m.borrow().get(&crate::runtime::sched::task_id()).copied())
}

/// Sleeps without blocking the other `fg` tasks: they run while this one
/// (or the main program) waits.
pub fn sleep_coop(d: std::time::Duration) -> Result<(), String> {
    use crate::runtime::sched;
    // a zero sleep still lets the others run once (`time.delay(0ms)` yields)
    if d.is_zero() {
        return sched::yield_now();
    }
    let deadline = std::time::Instant::now()
        .checked_add(d)
        .ok_or_else(|| format!("sleep: a duration of {} s is too large", d.as_secs()))?;
    // code with a timeout (`fogu:vm`, `fogu test --timeout`) sleeps no longer than it
    if let Some((limit, timeout)) = wait_limit().filter(|(limit, _)| *limit < deadline) {
        sleep_until(limit)?;
        return Err(format!(
            "script execution timed out after {}",
            Value::Duration(timeout.as_nanos() as i64).to_display_string()
        ));
    }
    sleep_until(deadline)
}

/// Windows wakes sleeping threads on a 15.6 ms tick unless a program asks for 1 ms
/// (§8: 100 × `time.delay(1ms)` took 1.5 s). Asked once, by the first sleep of the
/// program; the process keeps it until it ends.
#[cfg(windows)]
fn fine_timers() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn timeBeginPeriod(period: u32) -> u32;
        }
        // SAFETY: a plain winmm call with a constant
        unsafe {
            timeBeginPeriod(1);
        }
    });
}

fn sleep_until(deadline: std::time::Instant) -> Result<(), String> {
    use crate::runtime::sched;
    #[cfg(windows)]
    fine_timers();
    // a task parks behind a gate opened at its deadline: waking costs nothing
    // before, and tasks wake in the order of their deadlines
    if !cfg!(target_family = "wasm") && sched::in_task() {
        let gate = sched::new_local_gate();
        let key = sched::open_at(deadline, &gate);
        let r = sched::wait_until_on(Some(&gate), || gate.load(std::sync::atomic::Ordering::Acquire));
        sched::cancel_open_at(key);
        return r;
    }
    crate::runtime::sched::add_timer(deadline);
    crate::runtime::sched::wait_until(|| {
        let done = std::time::Instant::now() >= deadline;
        if !done {
            crate::runtime::sched::add_timer(deadline);
        }
        done
    })
}

/// The error of an allocation the system refused: a size that comes from the
/// program (or its input) stops it with this message, never aborts the process.
pub fn alloc_error(what: &str, n: impl std::fmt::Display) -> String {
    format!("{}: cannot allocate {} bytes (out of memory)", what, n)
}

/// `n` zero bytes, or [`alloc_error`] when the memory cannot be had.
pub fn zeroed_bytes(n: usize, what: &str) -> Result<Vec<u8>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let layout = std::alloc::Layout::array::<u8>(n).map_err(|_| alloc_error(what, n))?;
    // SAFETY: a non-empty layout of `n` bytes from the global allocator, all
    // zeroed: exactly what a `Vec<u8>` of length and capacity `n` owns.
    unsafe {
        let p = std::alloc::alloc_zeroed(layout);
        if p.is_null() {
            return Err(alloc_error(what, n));
        }
        Ok(Vec::from_raw_parts(p, n, n))
    }
}

/// `s` repeated `n` times, or [`alloc_error`] when the text would not fit.
pub fn repeat_str(s: &str, n: usize, what: &str) -> Result<String, String> {
    if s.is_empty() || n == 0 {
        return Ok(String::new());
    }
    let total = s.len().checked_mul(n).ok_or_else(|| alloc_error(what, format!("{} × {}", s.len(), n)))?;
    let mut out = String::new();
    out.try_reserve_exact(total).map_err(|_| alloc_error(what, total))?;
    out.push_str(s);
    while out.len() < total {
        // a prefix of whole copies: `out` is k copies and what is missing a multiple of `s`
        let take = (total - out.len()).min(out.len());
        // SAFETY: `take` ends between two copies of `s`, so the bytes stay UTF-8.
        unsafe { out.as_mut_vec().extend_from_within(..take) };
    }
    Ok(out)
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
        other => Err(format!("{}: expected str, buffer or a list of bytes, got '{}'", what, other.type_name())),
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
pub use fogu_pkg::net::{blocking_client, redirect_policy};
