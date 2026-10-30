//! What the standard library plugs into the core. `fogu-std` registers its
//! [`Host`] once at startup (`fogu_std::register()`); without one, programs
//! only have the core built-ins.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::native::{NativeContext, NativeObjectRef};
use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::value::Value;

/// The result of a call the host served itself.
pub enum Handled {
    /// A native result: `error` values reach the program as values.
    Native(Result<Value, String>),
    /// Returned as is (an app handler of `fogu:web`).
    Direct(Result<Value, String>),
}

pub trait Host: Sync {
    /// Registers the globals and the `fogu:*` modules.
    fn install(&self, ctx: &mut NativeContext);

    /// A call that needs the interpreter (`fogu:vm`, servers that run Fogu
    /// handlers, `cron.tick`…); `None` lets the interpreter call it normally.
    fn intercept(&self, callee: &Value, args: &[Value], it: &Interpreter, env: &Rc<RefCell<Environment>>) -> Option<Handled>;

    /// May [`intercept`](Self::intercept) handle calls of this native function?
    /// Answered once per function (by name) and cached: the others — `print`,
    /// `str`, `len`, most module functions — skip `intercept` on every call.
    fn may_intercept_native(&self, f: &crate::runtime::value::NativeFnValue) -> bool {
        let _ = f;
        true
    }

    /// A method of a native object that needs the interpreter (`Agent.run`,
    /// `GrpcServer.serve`).
    fn intercept_method(
        &self,
        receiver: &NativeObjectRef,
        method: &str,
        args: &[Value],
        it: &Interpreter,
        env: &Rc<RefCell<Environment>>,
    ) -> Option<Result<Value, String>>;

    /// Runs before every call (a server serves the requests it queued); it
    /// must be cheap when there is nothing to do.
    fn before_call(&self, it: &Interpreter, env: &Rc<RefCell<Environment>>);
}

static HOST: OnceLock<&'static dyn Host> = OnceLock::new();

/// Registers the host (the first registration wins).
pub fn set(host: &'static dyn Host) {
    let _ = HOST.set(host);
}

pub fn get() -> Option<&'static dyn Host> {
    HOST.get().copied()
}
