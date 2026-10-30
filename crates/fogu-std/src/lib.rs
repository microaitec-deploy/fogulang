//! Fogu standard library: the `fogu:*` modules beyond the core built-ins,
//! grouped by area in [`native`]. Every area except `base` and `io` is a cargo
//! feature. Call [`register`] once at startup (the CLI and the C API do).

#[macro_use]
extern crate fogu_core;

pub use fogu_core::{pkg, runtime};

mod host;
pub mod native;

/// Plugs the standard library into the core (idempotent).
pub fn register() {
    fogu_core::host::set(&host::Std);
}
