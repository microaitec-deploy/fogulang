//! Fogu core: the language itself — lexer, parser, resolver, interpreter,
//! Cranelift JIT, scheduler of `fg` tasks — plus the value and native object
//! model and the built-in methods of str / list / map / buffer.
//!
//! The standard library (`fogu-std`) plugs in through [`host`]: it registers
//! the globals and `fogu:*` modules, and the calls it serves itself.

/// Like `format!`, but the error reaches the Fogu program as an `error` value
/// (operational errors: IO, network, SQL, invalid JSON).
#[macro_export]
macro_rules! io_err {
    ($($t:tt)*) => {
        $crate::runtime::value::err_value(format!($($t)*))
    };
}

pub mod host;
pub mod native;
pub mod runtime;

pub use fogu_pkg as pkg;
