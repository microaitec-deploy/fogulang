//! Runtime of the Fogu language.

pub mod ast;
pub mod environment;
#[cfg(not(target_family = "wasm"))]
pub mod ffi_stub;
pub mod fstr;
pub mod interpreter;
#[cfg(not(target_family = "wasm"))]
pub mod jit;
#[cfg(target_family = "wasm")]
#[path = "jit_wasm.rs"]
pub mod jit;
pub mod lexer;
pub mod module;
pub mod ops;
pub mod parser;
#[cfg(target_os = "linux")]
pub mod reactor;
pub mod resolver;
#[cfg(not(target_family = "wasm"))]
pub mod sched;
pub mod symbol;
pub mod vfs;
#[cfg(target_family = "wasm")]
#[path = "sched_wasm.rs"]
pub mod sched;
pub mod token;
pub mod value;

pub use interpreter::Interpreter;
pub use lexer::Lexer;
pub use parser::Parser;

use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Stack size of the interpreter thread. It is virtual memory: it is only
/// actually used as recursion grows.
pub const MAIN_STACK: usize = 1024 * 1024 * 1024;

/// Reads, parses and checks a file; returns the prepared interpreter and
/// program (nothing has run yet).
fn load(path: &Path) -> Result<(Interpreter, Vec<ast::Stmt>, String), String> {
    let source = match vfs::read(path) {
        Some(s) => s,
        None => std::fs::read_to_string(path).map_err(|e| format!("error reading '{}': {}", path.display(), e))?,
    };

    let filename = vfs::display(path);
    mark("source read");

    let mut lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| format!("{}: {}", filename, e))?;

    let mut parser = Parser::new(tokens);
    let program = parser
        .parse_program()
        .map_err(|e| format!("{}: {}", filename, e))?;

    mark("parsed");
    let interp = Interpreter::new();
    mark("interpreter created");
    resolver::check(&program, &interp.builtin_names())
        .map_err(|e| format!("{}: {}", filename, e))?;
    mark("resolved");

    let base = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    interp.set_base_dir(&base);
    interp.set_main_file(&filename);
    crate::pkg::set_main_dir(&base);
    Ok((interp, program, filename))
}

/// `file:line: runtime error: msg` for an error raised while running.
fn located(interp: &Interpreter, filename: &str, e: String) -> String {
    let (file, line) = interp.error_location();
    if line > 0 {
        format!("{}:{}: runtime error: {}", file, line, e)
    } else {
        format!("{}: runtime error: {}", filename, e)
    }
}

pub fn run_file(path: &Path) -> Result<(), String> {
    let (interp, program, filename) = load(path)?;
    let result = interp
        .interpret(&program)
        .map_err(|e| located(&interp, &filename, e));
    if jit::stats_requested() {
        eprintln!("{}", interp.jit_report());
    }
    mark("program ended");
    if LAST_RUN.load(std::sync::atomic::Ordering::Relaxed) {
        // the process ends after this run: giving every value of the program
        // back to the allocator one by one first would only delay the exit
        std::mem::forget(program);
        std::mem::forget(interp);
    }
    result
}

/// With the `startup-trace` feature and `FOGU_T0` set to the launcher's
/// `CLOCK_MONOTONIC` in nanoseconds: prints the milliseconds since then and
/// the CPU time of the process, to see where a start spends its time.
/// Without the feature (the default) it is nothing at all.
#[cfg(all(feature = "startup-trace", unix))]
pub fn mark(what: &str) {
    if let Some(t0) = std::env::var("FOGU_T0").ok().and_then(|s| s.parse::<i128>().ok()) {
        let clock = |id| {
            let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            // SAFETY: `ts` is a valid timespec to fill
            unsafe { libc::clock_gettime(id, &mut ts) };
            ts.tv_sec as i128 * 1_000_000_000 + ts.tv_nsec as i128
        };
        let now = clock(libc::CLOCK_MONOTONIC);
        let cpu = clock(libc::CLOCK_PROCESS_CPUTIME_ID);
        eprintln!("[start] {:8.3} ms  cpu {:7.3}  {}", (now - t0) as f64 / 1e6, cpu as f64 / 1e6, what);
    }
}

#[cfg(not(all(feature = "startup-trace", unix)))]
#[inline(always)]
pub fn mark(_what: &str) {}

/// The process exits right after the program it is running (see [`last_run`]).
static LAST_RUN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Tells the runtime that the process exits when the program ends, so
/// [`run_file`] may leave the program's memory to the operating system
/// instead of freeing it (the `fogu` command does this; an embedder that
/// runs several programs in one process must not).
pub fn last_run() {
    LAST_RUN.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Result of one test registered with `fogu:test`.
pub enum TestStatus {
    Passed,
    Failed(String),
    Skipped,
}

pub struct TestOutcome {
    pub name: String,
    pub status: TestStatus,
    pub millis: u128,
}

/// Loads a `*.test.fg` file (registering its tests) and runs every test in
/// a fresh interpreter. `filter` keeps only the tests whose name contains it.
/// A test fails when it raises an error or returns an `error` value (so `?`
/// may be used inside tests).
pub fn run_test_file(path: &Path, filter: Option<&str>) -> Result<Vec<TestOutcome>, String> {
    crate::native::test::take_tests(); // drop leftovers of a failed load
    let (interp, program, filename) = load(path)?;
    interp
        .interpret(&program)
        .map_err(|e| located(&interp, &filename, e))?;

    let mut out = Vec::new();
    for t in crate::native::test::take_tests() {
        if filter.is_some_and(|f| !t.name.contains(f)) {
            continue;
        }
        if t.skip {
            out.push(TestOutcome { name: t.name, status: TestStatus::Skipped, millis: 0 });
            continue;
        }
        let started = std::time::Instant::now();
        let r = interp.call_value(t.func, Vec::new(), Rc::clone(interp.globals_ref()));
        sched::shutdown();
        let status = match r {
            Err(e) => TestStatus::Failed(located(&interp, &filename, e)),
            Ok(value::Value::Error(e)) => TestStatus::Failed(format!("returned an error: {}", e.message)),
            Ok(value::Value::List(l)) if matches!(l.borrow().last(), Some(value::Value::Error(_))) => {
                let msg = match l.borrow().last() {
                    Some(value::Value::Error(e)) => e.message.clone(),
                    _ => String::new(),
                };
                TestStatus::Failed(format!("returned an error: {}", msg))
            }
            Ok(_) => TestStatus::Passed,
        };
        out.push(TestOutcome { name: t.name, status, millis: started.elapsed().as_millis() });
    }
    Ok(out)
}

/// Runs a standalone source string (`fogu -e`).
pub fn run_source(source: &str, label: &str) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().map_err(|e| format!("{}: {}", label, e))?;
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program().map_err(|e| format!("{}: {}", label, e))?;
    let interp = Interpreter::new();
    resolver::check(&program, &interp.builtin_names()).map_err(|e| format!("{}: {}", label, e))?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    interp.set_base_dir(&cwd);
    interp.set_main_file(label);
    crate::pkg::set_main_dir(&cwd);
    let result = interp.interpret(&program).map_err(|e| {
        let (file, line) = interp.error_location();
        format!("{}:{}: runtime error: {}", file, line, e)
    });
    if jit::stats_requested() {
        eprintln!("{}", interp.jit_report());
    }
    result
}
