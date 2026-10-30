//! The JIT on WebAssembly: Cranelift cannot generate code there, so every
//! function and loop runs in the interpreter (same API as `jit.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use super::ast::{Expr, Stmt};
use super::environment::Environment;
use super::value::{FnValue, Value};

#[derive(Clone, Debug)]
pub struct JitConfig {
    pub enabled: bool,
    pub debug: u8,
    pub call_threshold: u32,
    pub loop_threshold: u32,
}

impl JitConfig {
    pub fn from_env() -> Self {
        JitConfig { enabled: false, debug: 0, call_threshold: u32::MAX, loop_threshold: u32::MAX }
    }
}

pub fn request_stats() {}

pub fn stats_requested() -> bool {
    false
}

pub fn configure(_cfg: JitConfig) {}

#[allow(dead_code)]
pub enum LoopKind<'a> {
    While { cond: &'a Expr, body: &'a Stmt },
    For { var: &'a str, body: &'a Stmt, lo: i64, hi: i64 },
}

#[allow(dead_code)]
pub enum LoopOutcome {
    Skip,
    Done,
    Err(String),
}

pub struct Jit;

impl Default for Jit {
    fn default() -> Self {
        Jit
    }
}

impl Jit {
    pub fn new() -> Self {
        Jit
    }

    pub fn enabled(&self) -> bool {
        false
    }

    pub fn loop_threshold(&self) -> u32 {
        u32::MAX
    }

    pub fn last_line(&self) -> u32 {
        0
    }

    pub fn call_fn(&mut self, _f: &Rc<FnValue>, _args: &[Value]) -> Option<Result<Value, String>> {
        None
    }

    pub fn run_loop(&mut self, _id: u32, _kind: LoopKind, _env: &Rc<RefCell<Environment>>) -> LoopOutcome {
        LoopOutcome::Skip
    }

    pub fn report(&self) -> String {
        "[jit] not available on WebAssembly".into()
    }
}
