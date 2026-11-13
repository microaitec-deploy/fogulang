//! `fogu:sync` module — synchronization between `fg` tasks (spec §37).
//!
//! ```fogu
//! import sync from "fogu:sync"
//!
//! const mu = sync.Mutex()
//! const wg = sync.WaitGroup()
//! let total = 0
//! for i in 0..3 {
//!     wg.add(1)
//!     fg fn() {
//!         mu.lock()
//!         total += 1
//!         mu.unlock()
//!         wg.done()
//!     }()
//! }
//! wg.wait()
//!
//! const sem = sync.Semaphore(2)        // at most two tasks inside at a time
//! fg fn() { sem.acquire(); work(); sem.release() }()
//! ```
//!
//! `fg` tasks share one thread (values are not thread-safe). To use other
//! cores, a **worker** runs a program of its own, with its own interpreter,
//! on another OS thread — on the CPU you choose:
//!
//! ```fogu
//! const w = sync.Worker("./prime.fg", { core: 2 })?   // or code: sync.Worker("fn sq(n) { return n * n }")
//! print(w.call("countPrimes", 2000000)?)            // runs on CPU 2; other tasks keep running meanwhile
//! w.close()
//! ```
//!
//! Arguments and results cross as JSON (numbers, texts, bools, nil, lists and
//! maps): the two programs share nothing else.
//!
//! Tasks are cooperative, so blocking operations (`lock`, `wait`) wait by
//! yielding to the other tasks: the interpreter asks the object for its wait
//! condition (`FoguObject::wait_for`) and only then calls the method. Waiting
//! forever is reported as a deadlock.

use std::cell::Cell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{max_args, nfn_var};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("Mutex".into(), nfn("Mutex", 0, |_| {
        Ok(NativeObjectRef::new(MutexValue { locked: Rc::new(Cell::new(false)) }).to_value())
    }));
    e.insert("WaitGroup".into(), nfn("WaitGroup", 0, |_| {
        Ok(NativeObjectRef::new(WaitGroupValue { count: Rc::new(Cell::new(0)) }).to_value())
    }));
    // Semaphore(n): n permits; acquire() waits for one
    e.insert("Semaphore".into(), nfn_var("Semaphore", 1, |a| {
        max_args(a, 1, "sync.Semaphore")?;
        let n = match a[0] {
            Value::Int(n) if n >= 1 => n,
            ref other => return Err(format!("sync.Semaphore(n) expects an int >= 1, got {}", other.to_display_string())),
        };
        Ok(NativeObjectRef::new(SemaphoreValue { permits: Rc::new(Cell::new(n)), size: n }).to_value())
    }));
    e.insert("Once".into(), nfn("Once", 0, |_| Ok(NativeObjectRef::new(OnceValue { done: false }).to_value())));
    e.insert("Atomic".into(), nfn_var("Atomic", 0, |a| {
        max_args(a, 1, "sync.Atomic")?;
        Ok(NativeObjectRef::new(AtomicValue { value: a.first().cloned().unwrap_or(Value::Int(0)) }).to_value())
    }));
    // Worker(file | code, { core }): its own interpreter on its own OS thread
    #[cfg(not(target_family = "wasm"))]
    e.insert("Worker".into(), nfn_var("Worker", 1, worker::new));
    ModuleValue {
        name: "fogu:sync".into(),
        exports: std::cell::RefCell::new(e),
    }
}

fn no_args(args: &[Value], what: &str) -> Result<(), String> {
    if !args.is_empty() {
        return Err(format!("{} expects no arguments", what));
    }
    Ok(())
}

// ---------- Mutex ----------

pub struct MutexValue {
    locked: Rc<Cell<bool>>,
}

impl FoguObject for MutexValue {
    fn name(&self) -> &str {
        "Mutex"
    }
    fn get(&self, field: &str) -> Option<Value> {
        (field == "locked").then(|| Value::Bool(self.locked.get()))
    }
    fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        let locked = Rc::clone(&self.locked);
        (method == "lock").then(|| Box::new(move || !locked.get()) as Box<dyn Fn() -> bool>)
    }
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // the interpreter already waited until the mutex was free
            "lock" => {
                no_args(args, "lock()")?;
                self.locked.set(true);
                Ok(Value::Nil)
            }
            "tryLock" => {
                no_args(args, "tryLock()")?;
                if self.locked.get() {
                    return Ok(Value::Bool(false));
                }
                self.locked.set(true);
                Ok(Value::Bool(true))
            }
            "unlock" => {
                no_args(args, "unlock()")?;
                if !self.locked.get() {
                    return Err("unlock of a mutex that is not locked".into());
                }
                self.locked.set(false);
                sched::note_progress();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Mutex", other)),
        }
    }
    fn methods(&self) -> Vec<&'static str> {
        vec!["lock", "tryLock", "unlock"]
    }
}

/// A `sync.Mutex` over an existing flag (`fogu:vm` contexts expose theirs).
pub(crate) fn mutex_over(locked: Rc<Cell<bool>>) -> Value {
    NativeObjectRef::new(MutexValue { locked }).to_value()
}

// ---------- Semaphore ----------

pub struct SemaphoreValue {
    permits: Rc<Cell<i64>>,
    size: i64,
}

/// The permits counter of a `sync.Semaphore` value (for `fogu:vm`).
pub(crate) fn semaphore_permits(v: &Value) -> Option<Rc<Cell<i64>>> {
    match v {
        Value::Object(o) => o.inner().borrow().as_any().downcast_ref::<SemaphoreValue>().map(|s| Rc::clone(&s.permits)),
        _ => None,
    }
}

impl FoguObject for SemaphoreValue {
    fn name(&self) -> &str {
        "Semaphore"
    }
    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "available" => Some(Value::Int(self.permits.get())),
            "size" => Some(Value::Int(self.size)),
            _ => None,
        }
    }
    fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        let permits = Rc::clone(&self.permits);
        (method == "acquire").then(|| Box::new(move || permits.get() > 0) as Box<dyn Fn() -> bool>)
    }
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // the interpreter already waited for a free permit
            "acquire" => {
                no_args(args, "acquire()")?;
                self.permits.set(self.permits.get() - 1);
                Ok(Value::Nil)
            }
            "tryAcquire" => {
                no_args(args, "tryAcquire()")?;
                if self.permits.get() <= 0 {
                    return Ok(Value::Bool(false));
                }
                self.permits.set(self.permits.get() - 1);
                Ok(Value::Bool(true))
            }
            "release" => {
                no_args(args, "release()")?;
                if self.permits.get() >= self.size {
                    return Err("release of a semaphore with no permit taken".into());
                }
                self.permits.set(self.permits.get() + 1);
                sched::note_progress();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Semaphore", other)),
        }
    }
    fn methods(&self) -> Vec<&'static str> {
        vec!["acquire", "tryAcquire", "release"]
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------- WaitGroup ----------

pub struct WaitGroupValue {
    count: Rc<Cell<i64>>,
}

impl FoguObject for WaitGroupValue {
    fn name(&self) -> &str {
        "WaitGroup"
    }
    fn get(&self, field: &str) -> Option<Value> {
        (field == "count").then(|| Value::Int(self.count.get()))
    }
    fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        let count = Rc::clone(&self.count);
        (method == "wait").then(|| Box::new(move || count.get() <= 0) as Box<dyn Fn() -> bool>)
    }
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "add" => {
                let n = match args.first() {
                    None => 1,
                    Some(Value::Int(n)) => *n,
                    Some(other) => return Err(format!("add expects int, got '{}'", other.type_name())),
                };
                let c = self.count.get() + n;
                if c < 0 {
                    return Err("WaitGroup counter would become negative".into());
                }
                self.count.set(c);
                sched::note_progress();
                Ok(Value::Nil)
            }
            "done" => {
                no_args(args, "done()")?;
                let c = self.count.get() - 1;
                if c < 0 {
                    return Err("WaitGroup.done() called more times than add()".into());
                }
                self.count.set(c);
                sched::note_progress();
                Ok(Value::Nil)
            }
            // the interpreter already waited until the counter reached zero
            "wait" => {
                no_args(args, "wait()")?;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on WaitGroup", other)),
        }
    }
    fn methods(&self) -> Vec<&'static str> {
        vec!["add", "done", "wait"]
    }
}

// ---------- Once ----------

/// `once.do(fn)` runs `fn` only the first time. Calling the function needs the
/// interpreter, which intercepts `do` and asks the object with `claim()`.
pub struct OnceValue {
    done: bool,
}

impl FoguObject for OnceValue {
    fn name(&self) -> &str {
        "Once"
    }
    fn get(&self, field: &str) -> Option<Value> {
        (field == "done").then(|| Value::Bool(self.done))
    }
    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            // true only the first time
            "claim" => {
                let first = !self.done;
                self.done = true;
                Ok(Value::Bool(first))
            }
            "do" => Err("Once.do must be handled by the interpreter — please report a bug".into()),
            other => Err(format!("method '{}' does not exist on Once", other)),
        }
    }
    fn methods(&self) -> Vec<&'static str> {
        vec!["do"]
    }
}

// ---------- Atomic ----------

pub struct AtomicValue {
    value: Value,
}

impl FoguObject for AtomicValue {
    fn name(&self) -> &str {
        "Atomic"
    }
    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "get" | "load" => Ok(self.value.clone()),
            "set" | "store" => {
                self.value = args.first().cloned().unwrap_or(Value::Nil);
                sched::note_progress();
                Ok(Value::Nil)
            }
            "add" => {
                let n = match args.first() {
                    None => 1,
                    Some(Value::Int(n)) => *n,
                    Some(other) => return Err(format!("add expects int, got '{}'", other.type_name())),
                };
                let cur = match &self.value {
                    Value::Int(c) => *c,
                    other => return Err(format!("add needs an int Atomic, it holds '{}'", other.type_name())),
                };
                let v = cur.checked_add(n).ok_or(crate::runtime::ops::ERR_OVERFLOW)?;
                self.value = Value::Int(v);
                sched::note_progress();
                Ok(Value::Int(v))
            }
            "compareAndSwap" => {
                if args.len() != 2 {
                    return Err("compareAndSwap(old, new) expects 2 arguments".into());
                }
                if crate::runtime::interpreter::values_equal(&self.value, &args[0]) {
                    self.value = args[1].clone();
                    sched::note_progress();
                    Ok(Value::Bool(true))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            other => Err(format!("method '{}' does not exist on Atomic", other)),
        }
    }
    fn methods(&self) -> Vec<&'static str> {
        vec!["get", "set", "add", "compareAndSwap"]
    }
}

// ---------- Worker ----------

/// `sync.Worker`: a Fogu program on another OS thread, with its own
/// interpreter (so its own globals, modules and `fg` tasks). The caller's
/// values never cross the thread: a call sends its arguments as JSON text and
/// gets the result back the same way.
#[cfg(not(target_family = "wasm"))]
mod worker {
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Arc, Mutex};

    use crate::native::object::{FoguObject, NativeObjectRef, Poller};
    use crate::native::util::max_args;
    use crate::runtime::sched::{self, IoGuard};
    use crate::runtime::value::{Value, ERR_VALUE_PREFIX};
    use crate::runtime::{Interpreter, Lexer, Parser, MAIN_STACK};

    type Slot = Arc<Mutex<Option<Result<String, String>>>>;

    /// A call: the function, its arguments (a JSON array), where the result
    /// goes and the guard that wakes the caller's scheduler when it is there.
    struct Call {
        name: String,
        args: String,
        slot: Slot,
        guard: IoGuard,
    }

    pub struct WorkerValue {
        tx: Option<Sender<Call>>,
        core: Option<usize>,
        source: String,
        /// Made from a function (`sync.Worker(fn)`): the global of the worker
        /// that `call(...args)` runs.
        entry: Option<String>,
    }

    /// The global names a program can use without declaring them (this
    /// thread's, computed once: a fresh interpreter has the same ones).
    fn builtin_names() -> Rc<std::collections::HashSet<String>> {
        thread_local! {
            static NAMES: std::cell::OnceCell<Rc<std::collections::HashSet<String>>> = const { std::cell::OnceCell::new() };
        }
        NAMES.with(|n| Rc::clone(n.get_or_init(|| Rc::new(Interpreter::new().builtin_names()))))
    }

    /// `fn name(` with exactly this name: the text is a declaration.
    fn declares(src: &str, name: &str) -> bool {
        let Some(rest) = src.strip_prefix("fn") else { return false };
        let rest = rest.trim_start();
        !name.is_empty() && rest.len() > name.len() && rest.starts_with(name) && {
            let next = rest[name.len()..].chars().next().unwrap_or(' ');
            !(next.is_alphanumeric() || next == '_')
        } && rest.len() < src.len()
    }

    /// The source of `name` as a top-level declaration of the worker.
    fn as_global(name: &str, src: &str) -> String {
        if declares(src, name) {
            src.to_string()
        } else {
            format!("const {} = {}", name, src)
        }
    }

    /// The text of a constant as Fogu source.
    fn literal(v: &Value) -> Option<String> {
        Some(match v {
            Value::Nil => "nil".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) if f.is_finite() => crate::runtime::value::display_float(*f),
            Value::Str(s) => {
                let mut out = String::with_capacity(s.len() + 2);
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        '\0' => out.push_str("\\0"),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            _ => return None,
        })
    }

    /// The name a resolver error says is not defined.
    fn undefined_name(e: &str) -> Option<String> {
        let i = e.find("variable '")? + "variable '".len();
        let j = e[i..].find("' is not defined")?;
        Some(e[i..i + j].to_string())
    }

    /// `sync.Worker(fn)`: the worker's program, built from the function's
    /// source — what crosses threads is code, never the function value or
    /// the environment it captured. Whatever the function uses besides its
    /// parameters and the builtins must be reproducible in the worker: a
    /// `fogu:` module (imported again there), a top-level function of a file
    /// (its source is added, with what it uses), a constant of a top-level
    /// scope (written as a literal). A local variable it captures, or any
    /// other value of this thread, is an error that names it — checked here,
    /// before any thread starts. Returns (program, the global to call).
    fn from_function(f: &crate::runtime::value::FnValue) -> Result<(String, String), String> {
        let what = if f.name == "<anonymous>" { "the function".to_string() } else { format!("'{}'", f.name) };
        let fail = |msg: String| io_err!("sync.Worker({}): {}", if f.name == "<anonymous>" { "fn" } else { &f.name }, msg);
        let src = crate::runtime::fnsrc::source(&f.body).ok_or_else(|| {
            fail(format!(
                "the source of {} is not available: only functions defined in a file can start a worker (or pass a file or code)",
                what
            ))
        })?;
        let entry = if declares(&src, &f.name) { f.name.clone() } else { "__worker_main".to_string() };
        let main = as_global(&entry, &src);
        let builtins = builtin_names();
        // what the function needs, dependencies first (a declaration must
        // come before the code that runs it)
        let mut deps: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let mut code = deps.join("\n");
            code.push('\n');
            code.push_str(&main);
            code.push('\n');
            let program = Lexer::new(&code)
                .tokenize()
                .and_then(|t| Parser::new(t).parse_program())
                .map_err(|e| fail(format!("its source does not parse on its own: {}", e)))?;
            let err = match crate::runtime::resolver::check(&program, &builtins) {
                Ok(_) => return Ok((code, entry)),
                Err(e) => e,
            };
            let Some(name) = undefined_name(&err) else {
                return Err(fail(err));
            };
            if !seen.insert(name.clone()) {
                return Err(fail(format!("cannot make '{}' available in the worker", name)));
            }
            let found = f.closure.as_ref().and_then(|env| env.borrow().lookup_scoped(&name));
            let line = match found {
                None => return Err(fail(format!("it uses '{}', which is not defined", name))),
                Some((_, _, false)) => {
                    return Err(fail(format!(
                        "it captures the local variable '{}' of this thread, which cannot cross threads: pass it as an argument (w.call(..., {}))",
                        name, name
                    )))
                }
                Some((Value::Module(m), _, true)) if m.name.starts_with("fogu:") => {
                    format!("import {} from \"{}\"", name, m.name)
                }
                Some((Value::Fn(g), _, true)) => match crate::runtime::fnsrc::source(&g.body) {
                    Some(gsrc) => as_global(&name, &gsrc),
                    None => return Err(fail(format!("it calls '{}', whose source is not available", name))),
                },
                Some((_, true, true)) => {
                    return Err(fail(format!(
                        "it uses the variable '{}' of this thread, which can change: pass it as an argument",
                        name
                    )))
                }
                Some((v, false, true)) => match literal(&v) {
                    Some(text) => format!("const {} = {}", name, text),
                    None => {
                        return Err(fail(format!(
                            "it uses '{}' (a {}) from this thread: pass what it needs as arguments, or import it in the worker",
                            name,
                            v.type_name()
                        )))
                    }
                },
            };
            deps.insert(0, line);
        }
    }

    fn plain(e: String) -> String {
        e.strip_prefix(ERR_VALUE_PREFIX).map(str::to_string).unwrap_or(e)
    }

    /// `sync.Worker(file | code | fn, { core })`.
    pub fn new(a: &[Value]) -> Result<Value, String> {
        max_args(a, 2, "sync.Worker")?;
        // from a function: its program is built (and checked) before the thread starts
        let built;
        let mut entry = None;
        let mut fn_dir = None;
        let src: &str = match &a[0] {
            Value::Str(s) => s,
            Value::Fn(f) => {
                let (code, name) = from_function(f)?;
                entry = Some(name);
                fn_dir = crate::runtime::fnsrc::file(&f.body).and_then(|p| p.parent().map(Path::to_path_buf));
                built = code;
                &built
            }
            Value::NativeFn(f) => {
                return Err(format!(
                    "sync.Worker: '{}' is a built-in function, which has no source to send: call it from a function of a file",
                    f.name
                ))
            }
            other => return Err(format!("sync.Worker(file | code | fn, opts) expects a str or a function, got '{}'", other.type_name())),
        };
        let core = match a.get(1) {
            None | Some(Value::Nil) => None,
            Some(Value::Map(m)) => match m.borrow().get("core") {
                None | Some(Value::Nil) => None,
                Some(Value::Int(n)) if *n >= 0 => Some(*n as usize),
                Some(other) => return Err(format!("sync.Worker: 'core' must be an int >= 0, got {}", other.to_display_string())),
            },
            Some(other) => return Err(format!("sync.Worker: options must be a map, got '{}'", other.type_name())),
        };
        if let Some(c) = core {
            let n = crate::native::rt::machine_cores();
            if c >= n {
                return Err(io_err!("sync.Worker: core {} does not exist (this machine has {}: 0..{})", c, n, n - 1));
            }
        }
        // a path to a .fg file, or the code itself
        let (code, file, dir) = if let Some(name) = &entry {
            (src.to_string(), format!("<worker fn {}>", name), fn_dir)
        } else if src.ends_with(".fg") && !src.contains('\n') {
            let path = PathBuf::from(src);
            let text = match crate::runtime::vfs::read(&path) {
                Some(t) => t,
                None => std::fs::read_to_string(&path).map_err(|e| io_err!("sync.Worker: {}: {}", src, e))?,
            };
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf);
            (text, src.to_string(), dir)
        } else {
            (src.to_string(), "<worker>".to_string(), None)
        };
        let dir = dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let (tx, rx) = channel::<Call>();
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        // until it has started the worker is an operation in flight (it wakes this thread)
        let starting = IoGuard::new();
        let name = file.clone();
        std::thread::Builder::new()
            .name("fogu-worker".into())
            .stack_size(MAIN_STACK)
            .spawn(move || {
                sched::init_main_stack(MAIN_STACK);
                // while it starts, the creator's guard is this thread's own work
                sched::set_served(1);
                if let Some(c) = core {
                    if let Err(e) = crate::native::rt::pin_current_thread(c) {
                        let _ = ready_tx.send(Err(e));
                        drop(starting);
                        return;
                    }
                }
                let interp = Interpreter::new();
                interp.set_base_dir(&dir);
                interp.set_main_file(&name);
                // the program: its top level runs once, here
                let loaded = Lexer::new(&code)
                    .tokenize()
                    .and_then(|t| Parser::new(t).parse_program())
                    .map_err(|e| format!("{}: {}", name, e))
                    .and_then(|p| {
                        interp.embed_eval(&p).map(|_| ()).map_err(|e| {
                            let (f, line) = interp.error_location();
                            format!("{}:{}: {}", f, line, plain(e))
                        })
                    });
                let ok = loaded.is_ok();
                let _ = ready_tx.send(loaded);
                drop(starting); // wakes the creator, after the answer is there
                sched::set_served(0);
                if !ok {
                    return;
                }
                let globals = interp.globals_env();
                // one call at a time, until the Worker value is dropped or closed
                for call in rx {
                    // this call's own guard is not "something still coming" for this thread
                    sched::set_served(1);
                    let run = std::panic::AssertUnwindSafe(|| {
                        let f = interp.global(&call.name).ok_or_else(|| format!("the worker has no function '{}'", call.name))?;
                        let args = match crate::native::json::parse_value(&call.args)? {
                            Value::List(l) => l.borrow().clone(),
                            _ => Vec::new(),
                        };
                        match interp.call_value(f, args, Rc::clone(&globals)) {
                            Ok(Value::Error(e)) => Err(e.message.clone()),
                            Ok(v) => Ok(crate::native::json::stringify_value(&v)),
                            Err(e) => Err(plain(e)),
                        }
                    });
                    // a panic in native code must still answer the caller
                    let out = std::panic::catch_unwind(run).unwrap_or_else(|_| Err("the worker panicked".to_string()));
                    sched::set_served(0);
                    *call.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(out);
                    drop(call.guard); // wakes the caller, after the result is in place
                }
                sched::shutdown();
            })
            .map_err(|e| io_err!("sync.Worker: cannot start the thread: {}", e))?;
        // the worker's top level may take a while (or need something this
        // program serves): wait for it without stopping the other tasks
        let mut started = None;
        sched::wait_until(|| {
            if started.is_none() {
                started = match ready_rx.try_recv() {
                    Ok(r) => Some(r),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("the worker thread stopped while starting".to_string())),
                };
            }
            started.is_some()
        })?;
        if let Some(Err(e)) = started {
            return Err(io_err!("sync.Worker: {}", e));
        }
        Ok(NativeObjectRef::new(WorkerValue { tx: Some(tx), core, source: file, entry }).to_value())
    }

    impl FoguObject for WorkerValue {
        fn name(&self) -> &str {
            "Worker"
        }

        fn get(&self, field: &str) -> Option<Value> {
            match field {
                "core" => Some(self.core.map(|c| Value::Int(c as i64)).unwrap_or(Value::Nil)),
                "source" => Some(Value::Str(self.source.clone().into())),
                "closed" => Some(Value::Bool(self.tx.is_none())),
                _ => None,
            }
        }

        fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
            match method {
                // close(): the worker ends after the call it is running
                "close" => {
                    max_args(args, 0, "close")?;
                    self.tx = None;
                    Ok(Value::Nil)
                }
                other => Err(format!("method '{}' does not exist on Worker (call close)", other)),
            }
        }

        /// `call(name, ...args)`: the caller waits without holding the
        /// object, so its other tasks (and other calls) keep running.
        fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
            if method != "call" {
                return None;
            }
            let r = (|| {
                // a worker made from a function runs it with every argument
                let (name, rest) = match (&self.entry, args.first()) {
                    (Some(entry), _) => (entry.clone(), args),
                    (None, Some(Value::Str(s))) => (s.to_string(), &args[1..]),
                    (None, _) => return Err("Worker.call(name, ...args) expects the name of a function of the worker".to_string()),
                };
                let tx = self.tx.as_ref().ok_or_else(|| io_err!("Worker.call: the worker is closed"))?;
                let slot: Slot = Arc::new(Mutex::new(None));
                let call = Call {
                    name: name.clone(),
                    args: crate::native::json::stringify_value(&crate::native::util::list(rest.to_vec())),
                    slot: Arc::clone(&slot),
                    guard: IoGuard::new(),
                };
                tx.send(call).map_err(|_| io_err!("Worker.call: the worker stopped"))?;
                Ok(Box::new(move || {
                    let out = slot.lock().unwrap_or_else(|p| p.into_inner()).take()?;
                    Some(match out {
                        Ok(json) => crate::native::json::parse_value(&json),
                        Err(e) => Err(io_err!("Worker.call {}: {}", name, e)),
                    })
                }) as Poller)
            })();
            Some(r)
        }

        fn methods(&self) -> Vec<&'static str> {
            vec!["call", "close"]
        }
    }
}
