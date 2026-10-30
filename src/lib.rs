//! Fogu — reference implementation of the `.fg` language. This crate is the
//! CLI; the language lives in the workspace crates it re-exports:
//!
//! * [`runtime`] (`fogu-core`) — lexer, parser, interpreter, JIT (Cranelift) and scheduler.
//! * [`native`] (`fogu-std`) — standard library (`fogu:*`), areas behind cargo features.
//! * [`pkg`] (`fogu-pkg`) — git-based package manager.
//! * [`toolkit`] — CLI, REPL and commands.

#[cfg(not(target_family = "wasm"))]
pub mod embed;
pub use fogu_core::{host, runtime};
pub use fogu_pkg as pkg;
pub use fogu_std::native;
pub use fogu_std::register;
pub mod toolkit;
