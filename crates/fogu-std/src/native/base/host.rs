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

use std::cell::{Cell, RefCell};
use std::ffi::CString;

use crate::runtime::value::Value;

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{max_args, str_arg};

/// `void callback(const char *name, const char *json, void *user)`
pub type HostCallback = extern "C" fn(*const std::os::raw::c_char, *const std::os::raw::c_char, *mut std::ffi::c_void);

thread_local! {
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
