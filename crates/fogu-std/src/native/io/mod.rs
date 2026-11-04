//! Files, paths, processes, shell, sockets, archives and data formats (always built).

pub mod fs;
pub mod path;
pub mod process;
pub mod system;
pub mod shell;
#[cfg(not(target_family = "wasm"))]
pub mod watch;
pub mod zip;
pub mod formats;
pub mod json5;
pub mod markdown;
#[cfg(not(target_family = "wasm"))]
pub mod net;
#[cfg(not(target_family = "wasm"))]
pub mod ws_client;
