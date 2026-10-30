//! Language-level modules of the standard library (always built): time, sync,
//! cron, log, regex, crypto, vm, the `Fogu` object, signals, CLI helpers, FFI.

pub mod time;
pub mod sync;
pub mod cron;
pub mod log;
pub mod regex;
pub mod crypto;
pub mod vm;
pub mod fogu_global;
#[cfg(not(target_family = "wasm"))]
pub mod host;
#[cfg(not(target_family = "wasm"))]
pub mod signal;
#[cfg(not(target_family = "wasm"))]
pub mod cli;
#[cfg(not(target_family = "wasm"))]
pub mod ffi;
