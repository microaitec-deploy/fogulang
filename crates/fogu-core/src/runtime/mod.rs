//! Runtime of the Fogu language.

pub mod array;
pub mod ast;
pub mod environment;
pub mod fnsrc;
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

pub use interpreter::vm;
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
    fnsrc::register(path, parser.take_fn_spans());

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
    interp.enter_main(path);
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
/// may be used inside tests). Groups (`describe`), hooks and `only` follow
/// `native/test.rs`.
pub fn run_test_file(path: &Path, filter: Option<&str>) -> Result<Vec<TestOutcome>, String> {
    use crate::native::test::{self as t, Item};
    use crate::runtime::value::Value;
    t::reset(); // drop leftovers of a failed load
    let (interp, program, filename) = load(path)?;
    interp
        .interpret(&program)
        .map_err(|e| located(&interp, &filename, e))?;
    // the bodies of the groups register their tests (nested groups too)
    while let Some((body, args)) = t::next_body() {
        interp
            .call_value(body, args, Rc::clone(interp.globals_ref()))
            .map_err(|e| located(&interp, &filename, e))?;
    }
    t::end_bodies();
    let (tests, groups) = t::take();

    // `only` anywhere: only those tests (and the tests of `only` groups) run
    let in_only_group = |mut g: usize| loop {
        if groups[g].only {
            break true;
        }
        match groups[g].parent {
            Some(p) => g = p,
            None => break false,
        }
    };
    let any_only = tests.iter().any(|c| c.only) || groups.iter().any(|g| g.only);
    let skipped_group = |mut g: usize| loop {
        if groups[g].skip {
            break true;
        }
        match groups[g].parent {
            Some(p) => g = p,
            None => break false,
        }
    };
    let selected: Vec<bool> = tests
        .iter()
        .map(|c| filter.is_none_or(|f| c.name.contains(f)) && (!any_only || c.only || in_only_group(c.group)))
        .collect();
    let runnable = |c: &crate::native::test::TestCase| !c.skip && !c.todo && !skipped_group(c.group);

    // what a function call gave: an error, an error value, or a failed task
    let outcome = |r: Result<Value, String>, failures: u64| -> Option<String> {
        sched::shutdown();
        match r {
            Err(e) => Some(located(&interp, &filename, e)),
            Ok(v) => match v.into_result() {
                Err(e) => Some(format!("returned an error: {}", e.message)),
                Ok(_) => match sched::task_failures() {
                    (n, Some(e)) if n != failures => Some(e),
                    _ => None,
                },
            },
        }
    };
    let call = |f: &Value, args: Vec<Value>| -> Option<String> {
        let failures = sched::task_failures().0;
        let r = interp.call_value(f.clone(), args, Rc::clone(interp.globals_ref()));
        outcome(r, failures)
    };
    // does group `g` hold a selected test that runs?
    fn has_runnable(g: usize, groups: &[crate::native::test::Group], keep: &dyn Fn(usize) -> bool) -> bool {
        groups[g].items.iter().any(|it| match *it {
            Item::Test(i) => keep(i),
            Item::Group(c) => has_runnable(c, groups, keep),
        })
    }
    let keep = |i: usize| selected[i] && runnable(&tests[i]);
    // the hooks of `g` and its parents: outer first
    let chain = |g: usize| {
        let mut c = vec![g];
        while let Some(p) = groups[*c.last().unwrap()].parent {
            c.push(p);
        }
        c.reverse();
        c
    };

    let mut out = Vec::new();
    // a depth-first walk in declaration order; `None` = the group's beforeAll failed
    fn walk(
        g: usize,
        groups: &[crate::native::test::Group],
        tests: &[crate::native::test::TestCase],
        selected: &[bool],
        keep: &dyn Fn(usize) -> bool,
        ctx: &mut dyn FnMut(Step) -> Option<String>,
        out: &mut Vec<TestOutcome>,
        failed_setup: Option<&str>,
    ) {
        let runs = has_runnable(g, groups, keep);
        let mut setup_error: Option<String> = failed_setup.map(str::to_string);
        if runs && setup_error.is_none() {
            for f in &groups[g].before_all {
                if let Some(e) = ctx(Step::Hook(f)) {
                    setup_error = Some(prefixed(&e, "beforeAll failed: "));
                    break;
                }
            }
        }
        for it in &groups[g].items {
            match *it {
                Item::Test(i) => {
                    let c = &tests[i];
                    if !selected[i] {
                        continue;
                    }
                    if !keep(i) {
                        let name = if c.todo { format!("{} (todo)", c.name) } else { c.name.clone() };
                        out.push(TestOutcome { name, status: TestStatus::Skipped, millis: 0 });
                        continue;
                    }
                    if let Some(e) = &setup_error {
                        out.push(TestOutcome { name: c.name.clone(), status: TestStatus::Failed(e.clone()), millis: 0 });
                        continue;
                    }
                    let started = std::time::Instant::now();
                    let error = ctx(Step::Test(i));
                    let status = match error {
                        Some(e) => TestStatus::Failed(e),
                        None => TestStatus::Passed,
                    };
                    out.push(TestOutcome { name: c.name.clone(), status, millis: started.elapsed().as_millis() });
                }
                Item::Group(c) => walk(c, groups, tests, selected, keep, ctx, out, setup_error.as_deref()),
            }
        }
        // cleanup runs even when tests (or the setup) failed
        if runs {
            for f in &groups[g].after_all {
                if let Some(e) = ctx(Step::Hook(f)) {
                    let name = if groups[g].name.is_empty() { "afterAll".to_string() } else { format!("{} > afterAll", groups[g].name) };
                    out.push(TestOutcome { name, status: TestStatus::Failed(e), millis: 0 });
                }
            }
        }
    }
    enum Step<'a> {
        Hook(&'a Value),
        Test(usize),
    }
    let mut ctx = |step: Step| -> Option<String> {
        match step {
            Step::Hook(f) => call(f, Vec::new()),
            Step::Test(i) => {
                let c = &tests[i];
                let groups_of = chain(c.group);
                // beforeEach: outer groups first; a failure skips the test body
                let mut error = None;
                for &g in &groups_of {
                    for f in &groups[g].before_each {
                        if error.is_none() {
                            error = call(f, Vec::new()).map(|e| prefixed(&e, "beforeEach failed: "));
                        }
                    }
                }
                if error.is_none() {
                    error = call(&c.func, c.args.clone());
                }
                // afterEach: inner groups first, always
                for &g in groups_of.iter().rev() {
                    for f in &groups[g].after_each {
                        if let Some(e) = call(f, Vec::new()) {
                            error.get_or_insert(prefixed(&e, "afterEach failed: "));
                        }
                    }
                }
                error
            }
        }
    };
    walk(0, &groups, &tests, &selected, &keep, &mut ctx, &mut out, None);
    Ok(out)
}

/// `what` put before the message of a failure, after its location when it
/// has one (`file.fg:3: message` → `file.fg:3: beforeAll failed: message`).
fn prefixed(e: &str, what: &str) -> String {
    const MARK: &str = "runtime error: ";
    if let Some(i) = e.find(MARK) {
        let at = i + MARK.len();
        return format!("{}{}{}", &e[..at], what, &e[at..]);
    }
    let first = e.lines().next().unwrap_or("");
    // the location ends at the first ":<digits>: "
    let bytes = first.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j + 1 < bytes.len() && bytes[j] == b':' && bytes[j + 1] == b' ' {
                return format!("{}{}{}", &e[..j + 2], what, &e[j + 2..]);
            }
        }
        i += 1;
    }
    format!("{}{}", what, e)
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
