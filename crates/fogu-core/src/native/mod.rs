//! The native object model and the built-ins every program has: str / list /
//! map / buffer methods, `fogu:math`, `fogu:json`, `fogu:test`, events, and the
//! shared I/O runtime. The rest of the standard library lives in `fogu-std`.

pub mod buffer;
pub mod collections;
pub mod context;
pub mod events;
pub mod function;
pub mod json;
pub mod math;
pub mod object;
#[cfg(not(target_family = "wasm"))]
pub mod rt;
#[cfg(target_family = "wasm")]
#[path = "rt_wasm.rs"]
pub mod rt;
pub mod test;
pub mod util;

#[allow(unused_imports)]
pub use context::NativeContext;
#[allow(unused_imports)]
pub use function::NativeFunction;
#[allow(unused_imports)]
pub use object::{FoguObject, NativeObjectRef};
