//! C API of the Fogu runtime (`libfogu_rt`). See `fogu.h` for the contract.
//!
//! Every call that returns `char *` returns a JSON object — `{"ok":true,
//! "value":<json>}` or `{"ok":false,"error":"..."}` — that the caller frees
//! with `fogu_free_string`. Calls on one runtime are serialized (each waits
//! for the previous one); several runtimes may live in one process.

use std::ffi::{c_char, c_void, CStr, CString};

use fogu::embed::Runtime;

/// An opaque handle.
pub struct FoguRuntime(Runtime);

fn out(r: Result<String, String>) -> *mut c_char {
    let s = match r {
        Ok(v) => format!("{{\"ok\":true,\"value\":{}}}", v),
        Err(e) => format!("{{\"ok\":false,\"error\":{}}}", serde_json_string(&e)),
    };
    CString::new(s).unwrap_or_default().into_raw()
}

fn serde_json_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// # Safety
/// `p` is NULL or a NUL-terminated string.
unsafe fn text(p: *const c_char) -> Result<String, String> {
    if p.is_null() {
        return Ok(String::new());
    }
    unsafe { CStr::from_ptr(p) }.to_str().map(str::to_string).map_err(|_| "argument is not UTF-8".to_string())
}

/// # Safety
/// `rt` comes from `fogu_new` and was not freed.
unsafe fn with(rt: *mut FoguRuntime, f: impl FnOnce(&Runtime) -> Result<String, String>) -> *mut c_char {
    if rt.is_null() {
        return out(Err("NULL runtime".into()));
    }
    out(f(unsafe { &(*rt).0 }))
}

/// The version of the runtime (static string, do not free).
#[unsafe(no_mangle)]
pub extern "C" fn fogu_version() -> *const c_char {
    static V: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    V.as_ptr() as *const c_char
}

/// Starts a runtime; NULL if it could not start.
#[unsafe(no_mangle)]
pub extern "C" fn fogu_new() -> *mut FoguRuntime {
    match Runtime::new() {
        Ok(r) => Box::into_raw(Box::new(FoguRuntime(r))),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Stops the runtime (its tasks and servers end) and frees it.
///
/// # Safety
/// `rt` comes from `fogu_new`; it must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_free(rt: *mut FoguRuntime) {
    if !rt.is_null() {
        drop(unsafe { Box::from_raw(rt) });
    }
}

/// Frees a string returned by this library.
///
/// # Safety
/// `s` comes from this library (or is NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Evaluates code in the global scope → the value of its last expression.
///
/// # Safety
/// Valid runtime and NUL-terminated UTF-8 `code`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_eval(rt: *mut FoguRuntime, code: *const c_char) -> *mut c_char {
    unsafe { with(rt, |r| r.eval(&text(code)?)) }
}

/// Starts code as a background task (servers, loops) and returns at once.
///
/// # Safety
/// Valid runtime and NUL-terminated UTF-8 `code`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_spawn(rt: *mut FoguRuntime, code: *const c_char) -> *mut c_char {
    unsafe { with(rt, |r| r.spawn(&text(code)?)) }
}

/// Runs a .fg file in the global scope.
///
/// # Safety
/// Valid runtime and NUL-terminated UTF-8 `path`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_run_file(rt: *mut FoguRuntime, path: *const c_char) -> *mut c_char {
    unsafe { with(rt, |r| r.run_file(&text(path)?)) }
}

/// Calls a global function; `args_json` is a JSON array (or NULL).
///
/// # Safety
/// Valid runtime and NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_call(rt: *mut FoguRuntime, name: *const c_char, args_json: *const c_char) -> *mut c_char {
    unsafe { with(rt, |r| r.call(&text(name)?, &text(args_json)?)) }
}

/// Reads a global variable.
///
/// # Safety
/// Valid runtime and NUL-terminated UTF-8 `name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_get(rt: *mut FoguRuntime, name: *const c_char) -> *mut c_char {
    unsafe { with(rt, |r| r.get(&text(name)?)) }
}

/// Defines a global variable from JSON.
///
/// # Safety
/// Valid runtime and NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_set(rt: *mut FoguRuntime, name: *const c_char, value_json: *const c_char) -> *mut c_char {
    unsafe { with(rt, |r| r.set(&text(name)?, &text(value_json)?)) }
}

/// Fires the program's `Fogu.signal.on(name, fn(data))` handlers.
///
/// # Safety
/// Valid runtime and NUL-terminated UTF-8 strings (`data_json` may be NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_emit(rt: *mut FoguRuntime, name: *const c_char, data_json: *const c_char) -> *mut c_char {
    unsafe { with(rt, |r| r.emit(&text(name)?, &text(data_json)?)) }
}

/// Sets the callback behind `Fogu.host.emit(name, data)` (NULL removes it).
/// It runs on the runtime's thread; `user` is passed back unchanged; the
/// callback owns `name` and `json` (free them with `fogu_free_string`).
///
/// # Safety
/// Valid runtime; `cb` stays callable while it is set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fogu_on_host(
    rt: *mut FoguRuntime,
    cb: Option<extern "C" fn(*const c_char, *const c_char, *mut c_void)>,
    user: *mut c_void,
) -> *mut c_char {
    unsafe { with(rt, |r| r.on_host(cb, user as usize)) }
}
