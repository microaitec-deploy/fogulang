//! `Fogu.host` — talking to the program that embeds this runtime (the C API
//! of `ffi/`, see `crate::embed`).
//!
//! ```fogu
//! if Fogu.host.embedded {
//!     Fogu.host.emit("progress", { done: 3, total: 10 })   // calls the host's callback
//! }
//! ```
//!
//! `emit(name, data)` serializes `data` as JSON and calls the callback the
//! host registered (`fogu_on_host`) on the runtime's thread; it returns
//! false when there is none. The callback owns the two strings and frees
//! them with `fogu_free_string` (so an asynchronous host can read them later). Events in the other direction arrive through
//! `Fogu.signal.on(name, fn)` (the host calls `fogu_emit`).
//!
//! The host can also give the program **functions** (`fogu_register`): a
//! global that, when called, passes its arguments as a JSON array to a C
//! function and returns what it answers (`fogu_string(json)`, `fogu_error(msg)`
//! — an error value — or NULL for nil), synchronously, on the runtime's thread.

use std::cell::{Cell, RefCell};
use std::ffi::CString;

use crate::runtime::value::Value;

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{max_args, str_arg};

/// `void callback(const char *name, const char *json, void *user)`
pub type HostCallback = extern "C" fn(*const std::os::raw::c_char, *const std::os::raw::c_char, *mut std::ffi::c_void);

/// `char *fn(const char *args_json, void *user)`: a function the host registered.
pub type HostFn = extern "C" fn(*const std::os::raw::c_char, *mut std::ffi::c_void) -> *mut std::os::raw::c_char;

/// The first byte of a string made by `fogu_error`.
pub const ERROR_MARK: u8 = 1;

thread_local! {
    static FUNCS: RefCell<Vec<(HostFn, usize, String)>> = const { RefCell::new(Vec::new()) };
    static CALLBACK: RefCell<Option<(HostCallback, usize)>> = const { RefCell::new(None) };
    static EMBEDDED: Cell<bool> = const { Cell::new(false) };
}

/// Called by the embedding runtime on its own thread.
pub fn set_callback(cb: Option<(HostCallback, usize)>) {
    CALLBACK.with(|c| *c.borrow_mut() = cb);
}

pub fn set_embedded(on: bool) {
    EMBEDDED.with(|e| e.set(on));
}

/// A Fogu function that calls the host's `cb` (its id travels in the name: `name#hostfn<id>`).
pub fn register(name: &str, cb: HostFn, user: usize) -> Value {
    let id = FUNCS.with(|f| {
        let mut f = f.borrow_mut();
        f.push((cb, user, name.to_string()));
        f.len() - 1
    });
    crate::native::util::nfn_var(&format!("{}#hostfn{}", name, id), 0, |_| Err("Fogu.host: internal error: call through call_host_fn".into()))
}

/// Is this native function one the host registered?
pub fn host_fn_id(name: &str) -> Option<usize> {
    name.rsplit_once("#hostfn").and_then(|(_, id)| id.parse().ok())
}

/// Calls host function `id` with the arguments as a JSON array.
pub fn call_host_fn(id: usize, args: &[Value]) -> Result<Value, String> {
    let (cb, user, name) = FUNCS.with(|f| f.borrow().get(id).cloned()).ok_or("Fogu.host: unknown host function")?;
    let json = crate::native::json::stringify_value(&crate::native::util::list(args.to_vec()));
    let json = CString::new(json).map_err(|_| format!("{}: an argument has a NUL byte", name))?;
    let r = cb(json.as_ptr(), user as *mut std::ffi::c_void);
    if r.is_null() {
        return Ok(Value::Nil);
    }
    // SAFETY: the host returns a string made by fogu_string/fogu_error (a CString of this library)
    let text = unsafe { CString::from_raw(r) }.into_string().map_err(|_| format!("{}: the host returned invalid UTF-8", name))?;
    match text.as_bytes().first() {
        Some(&ERROR_MARK) => Err(io_err!("{}", &text[1..])),
        _ => crate::native::json::parse_value(&text).map_err(|e| format!("{}: the host returned invalid JSON: {}", name, e)),
    }
}

pub fn value() -> Value {
    NativeObjectRef::new(HostObj).to_value()
}

pub struct HostObj;

impl FoguObject for HostObj {
    fn name(&self) -> &str {
        "FoguHost"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "embedded").then(|| Value::Bool(EMBEDDED.with(|e| e.get())))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "emit" => {
                max_args(args, 2, "Fogu.host.emit")?;
                let name = str_arg(args, 0, "Fogu.host.emit")?;
                let data = crate::native::json::stringify_value(args.get(1).unwrap_or(&Value::Nil));
                let Some((cb, user)) = CALLBACK.with(|c| *c.borrow()) else { return Ok(Value::Bool(false)) };
                let (n, d) = (CString::new(name).map_err(|_| "Fogu.host.emit: name has a NUL byte")?, CString::new(data).unwrap_or_default());
                // the host owns both strings (fogu_free_string): it may read them later
                cb(n.into_raw(), d.into_raw(), user as *mut std::ffi::c_void);
                Ok(Value::Bool(true))
            }
            other => Err(format!("method '{}' does not exist on Fogu.host (emit)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["emit"]
    }
}
