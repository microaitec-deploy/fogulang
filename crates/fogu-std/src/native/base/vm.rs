//! `fogu:vm` module — runs Fogu source code at run time, in the style of
//! Node's `node:vm`.
//!
//! ```fogu
//! import vm from "fogu:vm"
//!
//! print(vm.runInNewContext("x * 2 + y", { x: 20, y: 2 })?)    // 42
//!
//! const ctx = vm.createContext({ count: 0 })
//! vm.runInContext("count += 1; let msg = `count is ${count}`", ctx)?
//! print(ctx.sandbox.count, ctx.msg)                          // 1 count is 1
//!
//! const script = vm.Script("n * n", { filename: "square.fg" })?
//! for n in 1..4 { print(script.runInNewContext({ n: n })?) }
//!
//! vm.runInNewContext("while true {}", nil, { timeout: 50ms })   // error value
//!
//! // concurrency: each spawn runs in its own `fg` task
//! const sem = sync.Semaphore(4)                              // at most 4 at a time
//! const tasks = []
//! for n in 1..10 { tasks.push(script.spawn({ n: n }, { semaphore: sem, timeout: 1s })) }
//! print(vm.all(tasks)?)                                      // [1, 4, 9, ...]
//! const t = vm.spawn("while true { time.delay(10ms) }", { time: time })
//! t.cancel(); print(t.wait())                                // error: script execution cancelled
//! ```
//!
//! Timeouts and cancellations belong to the task that runs the code: other
//! tasks (and their JIT) are not affected. A context is used by one run at a
//! time: concurrent runs on it wait their turn (its lock is `ctx.mutex`, a
//! `sync.Mutex` the program can take too); a run may nest in its own context.
//!
//! The code runs on the same interpreter (its functions can be called from
//! the program and vice versa). A new context only sees the global natives
//! (`print`, `len`, `Fogu`…) plus the keys of its sandbox map; it may still
//! `import` modules. Like `node:vm`, it is not a security boundary.
//!
//! The value of the last expression is returned; syntax, name, runtime and
//! timeout errors come back as `error` values.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::time::Duration;

use std::cell::Cell;

use crate::runtime::ast::Stmt;
use crate::runtime::interpreter::Deadline;
use crate::runtime::sched;
use crate::runtime::environment::Environment;
use crate::runtime::value::{ModuleValue, Value};
use crate::runtime::{resolver, Interpreter, Lexer, Parser};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{duration_arg, nfn_var, str_arg};

const DEFAULT_FILE: &str = "<vm>";

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    for (name, min) in [
        ("runInNewContext", 1),
        ("runInContext", 2),
        ("runInThisContext", 1),
        ("createContext", 0),
        ("Script", 1),
        ("compileFunction", 1),
        ("spawn", 1),
        ("all", 1),
    ] {
        // the calls are served by `intercept` (they need the interpreter)
        e.insert(name.into(), nfn_var(&format!("vm.{}", name), min, |_| Err("vm: internal call".into())));
    }
    e.insert("isContext".into(), nfn_var("isContext", 1, |a| Ok(Value::Bool(is_context(&a[0])))));
    ModuleValue {
        name: "fogu:vm".into(),
        exports: RefCell::new(e),
    }
}

fn is_context(v: &Value) -> bool {
    matches!(v, Value::Object(o) if o.name() == "VmContext")
}

// ---------------------------------------------------------------------
// Options and compilation
// ---------------------------------------------------------------------

struct Opts {
    filename: String,
    timeout: Option<Duration>,
    /// `spawn`: a `sync.Semaphore` limiting how many run at once.
    semaphore: Option<Rc<Cell<i64>>>,
    /// `vm.spawn(code, sandbox, { context })`: run in that context instead.
    context: Option<NativeObjectRef>,
}

/// `options` is a map `{ filename, timeout }` or, as in Node, a filename.
fn opts(v: Option<&Value>, what: &str) -> Result<Opts, String> {
    let mut o = Opts { filename: DEFAULT_FILE.into(), timeout: None, semaphore: None, context: None };
    match v {
        None | Some(Value::Nil) => {}
        Some(Value::Str(s)) => o.filename = s.to_string(),
        Some(Value::Map(m)) => {
            let m = m.borrow();
            if let Some(Value::Str(f)) = m.get("filename") {
                o.filename = f.to_string();
            }
            match m.get("timeout") {
                None | Some(Value::Nil) => {}
                Some(t) => o.timeout = Some(duration_arg(t, &format!("{}: timeout", what))?),
            }
            match m.get("semaphore") {
                None | Some(Value::Nil) => {}
                Some(v) => {
                    o.semaphore = Some(
                        crate::native::sync::semaphore_permits(v).ok_or_else(|| format!("{}: 'semaphore' must be a sync.Semaphore", what))?,
                    )
                }
            }
            match m.get("context") {
                None | Some(Value::Nil) => {}
                Some(c) => o.context = Some(context_arg(Some(c), what)?),
            }
        }
        Some(other) => return Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
    Ok(o)
}

fn sandbox_arg(v: Option<&Value>, what: &str) -> Result<Option<Rc<RefCell<HashMap<String, Value>>>>, String> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Map(m)) => Ok(Some(Rc::clone(m))),
        Some(other) => Err(format!("{}: the sandbox must be a map, got '{}'", what, other.type_name())),
    }
}

fn parse(code: &str, file: &str) -> Result<Rc<Vec<Stmt>>, String> {
    let tokens = Lexer::new(code).tokenize().map_err(|e| io_err!("{}: {}", file, e))?;
    let program = Parser::new(tokens).parse_program().map_err(|e| io_err!("{}: {}", file, e))?;
    Ok(Rc::new(program))
}

/// Static checks against the names visible in `env`, then the run.
fn run(interp: &Interpreter, program: &[Stmt], env: &Rc<RefCell<Environment>>, o: &Opts) -> Result<Value, String> {
    let names = env.borrow().all_names().into_iter().collect();
    resolver::check(program, &names).map_err(|e| io_err!("{}: {}", o.filename, e))?;
    interp.vm_run(program, Rc::clone(env), &o.filename, o.timeout).map_err(|e| io_err!("{}", e))
}

// ---------------------------------------------------------------------
// Contexts
// ---------------------------------------------------------------------

/// A context made by `createContext`: its variables live between runs and
/// are copied to the sandbox map after each one.
pub struct ContextValue {
    env: Rc<RefCell<Environment>>,
    sandbox: Rc<RefCell<HashMap<String, Value>>>,
    lock: CtxLock,
}

/// One run at a time per context. The flag is shared with `ctx.mutex` (a
/// `sync.Mutex`); the owner task may nest runs.
#[derive(Clone, Default)]
struct CtxLock {
    locked: Rc<Cell<bool>>,
    owner: Rc<Cell<Option<usize>>>,
    depth: Rc<Cell<u32>>,
}

impl CtxLock {
    fn acquire(&self) -> Result<(), String> {
        let me = sched::task_id();
        if self.owner.get() == Some(me) {
            self.depth.set(self.depth.get() + 1);
            return Ok(());
        }
        let locked = Rc::clone(&self.locked);
        sched::wait_until(move || !locked.get())?;
        self.locked.set(true);
        self.owner.set(Some(me));
        self.depth.set(1);
        Ok(())
    }

    fn release(&self) {
        let d = self.depth.get().saturating_sub(1);
        self.depth.set(d);
        if d == 0 {
            self.owner.set(None);
            self.locked.set(false);
            sched::note_progress();
        }
    }
}

impl ContextValue {
    fn new(interp: &Interpreter, sandbox: Option<Rc<RefCell<HashMap<String, Value>>>>) -> Self {
        Self {
            env: interp.vm_env(true, DEFAULT_FILE),
            sandbox: sandbox.unwrap_or_default(),
            lock: CtxLock::default(),
        }
    }

    /// Sandbox keys → variables (the program may have changed the map).
    fn pull(&self) {
        let mut env = self.env.borrow_mut();
        for (k, v) in self.sandbox.borrow().iter() {
            match env.get_own(k) {
                Some((_, mutable)) => env.define(k.clone(), v.clone(), mutable),
                None => env.define_shadowable(k.clone(), v.clone()),
            }
        }
    }

    /// Variables → sandbox keys.
    fn push(&self) {
        let env = self.env.borrow();
        let mut sb = self.sandbox.borrow_mut();
        for name in env.names() {
            if let Some((v, _)) = env.get_own(&name) {
                sb.insert(name, v);
            }
        }
    }
}

impl FoguObject for ContextValue {
    fn name(&self) -> &str {
        "VmContext"
    }

    /// `ctx.sandbox` (the map) or any variable of the context.
    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "sandbox" => return Some(Value::Map(Rc::clone(&self.sandbox))),
            "mutex" => return Some(crate::native::sync::mutex_over(Rc::clone(&self.lock.locked))),
            "busy" => return Some(Value::Bool(self.lock.locked.get())),
            _ => {}
        }
        self.env.borrow().get_own(field).map(|(v, _)| v)
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on VmContext (run)", method))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["run", "spawn"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn run_in_context(interp: &Interpreter, program: &[Stmt], ctx: &NativeObjectRef, o: &Opts) -> Result<Value, String> {
    let (env, sandbox, lock) = {
        let inner = ctx.inner();
        let b = inner.borrow();
        let c = b.as_any().downcast_ref::<ContextValue>().ok_or("runInContext: not a vm context")?;
        (Rc::clone(&c.env), Rc::clone(&c.sandbox), c.lock.clone())
    };
    // concurrent runs on one context wait their turn
    lock.acquire()?;
    let c = ContextValue { env, sandbox, lock };
    c.pull();
    c.env.borrow_mut().set_module(&o.filename);
    let r = run(interp, program, &c.env, o);
    // copied back even after an error (what ran so far is kept)
    c.push();
    c.lock.release();
    r
}

fn run_in_new_context(
    interp: &Interpreter,
    program: &[Stmt],
    sandbox: Option<Rc<RefCell<HashMap<String, Value>>>>,
    o: &Opts,
) -> Result<Value, String> {
    let ctx = ContextValue::new(interp, sandbox);
    ctx.pull();
    ctx.env.borrow_mut().set_module(&o.filename);
    let r = run(interp, program, &ctx.env, o);
    ctx.push();
    r
}

thread_local! {
    /// Shared scope of `runInThisContext` (its declarations persist, as
    /// Node's global ones do).
    static THIS_ENV: RefCell<Option<Rc<RefCell<Environment>>>> = const { RefCell::new(None) };
}

fn run_in_this_context(interp: &Interpreter, program: &[Stmt], o: &Opts) -> Result<Value, String> {
    let env = THIS_ENV.with(|t| Rc::clone(t.borrow_mut().get_or_insert_with(|| interp.vm_env(false, DEFAULT_FILE))));
    env.borrow_mut().set_module(&o.filename);
    run(interp, program, &env, o)
}

fn context_arg(v: Option<&Value>, what: &str) -> Result<NativeObjectRef, String> {
    match v {
        Some(Value::Object(o)) if o.name() == "VmContext" => Ok(o.clone()),
        Some(other) => Err(format!("{}: expected a context of vm.createContext, got '{}'", what, other.type_name())),
        None => Err(format!("{}: expected a context of vm.createContext", what)),
    }
}

// ---------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------

/// Code compiled once by `vm.Script`, run many times.
pub struct ScriptValue {
    program: Rc<Vec<Stmt>>,
    source: String,
    filename: String,
}

impl FoguObject for ScriptValue {
    fn name(&self) -> &str {
        "VmScript"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "source" => Value::Str(self.source.clone().into()),
            "filename" => Value::Str(self.filename.clone().into()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!(
            "method '{}' does not exist on VmScript (runInContext runInNewContext runInThisContext)",
            method
        ))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["runInContext", "runInNewContext", "runInThisContext", "spawn"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------

/// What a spawned run executes.
enum Target {
    New(Option<Rc<RefCell<HashMap<String, Value>>>>),
    Context(NativeObjectRef),
}

#[derive(Default)]
struct TaskState {
    result: RefCell<Option<Result<Value, String>>>,
    /// `sched::task_id()` of the task while it runs.
    task: Cell<Option<usize>>,
    cancelled: Cell<bool>,
}

/// A run started by `spawn`: `wait()` for its value, `cancel()` to stop it.
pub struct TaskValue {
    state: Rc<TaskState>,
    filename: String,
}

impl FoguObject for TaskValue {
    fn name(&self) -> &str {
        "VmTask"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let r = self.state.result.borrow();
        Some(match field {
            "done" => Value::Bool(r.is_some()),
            "ok" => Value::Bool(matches!(&*r, Some(Ok(_)))),
            "result" => match &*r {
                Some(Ok(v)) => v.clone(),
                _ => Value::Nil,
            },
            "error" => match &*r {
                Some(Err(e)) => Value::Str(e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(e).to_string().into()),
                _ => Value::Nil,
            },
            "filename" => Value::Str(self.filename.clone().into()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on VmTask (wait cancel)", method))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["wait", "cancel"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Starts `program` in a new `fg` task.
fn spawn(interp: &Interpreter, program: Rc<Vec<Stmt>>, target: Target, o: Opts) -> Result<Value, String> {
    let state = Rc::new(TaskState::default());
    let st = Rc::clone(&state);
    let filename = o.filename.clone();
    interp.spawn_task(Box::new(move |it| {
        st.task.set(Some(sched::task_id()));
        let r = (|| {
            if let Some(permits) = &o.semaphore {
                let p = Rc::clone(permits);
                sched::wait_until(move || p.get() > 0)?;
                permits.set(permits.get() - 1);
            }
            // cancelled before it started
            if st.cancelled.get() {
                return Err(io_err!("{}: runtime error: script execution cancelled", o.filename));
            }
            let r = match &target {
                Target::New(sandbox) => run_in_new_context(it, &program, sandbox.clone(), &o),
                Target::Context(ctx) => run_in_context(it, &program, ctx, &o),
            };
            if let Some(permits) = &o.semaphore {
                permits.set(permits.get() + 1);
                sched::note_progress();
            }
            r
        })();
        st.task.set(None);
        *st.result.borrow_mut() = Some(r);
        sched::note_progress();
        Ok(())
    }))?;
    Ok(NativeObjectRef::new(TaskValue { state, filename }).to_value())
}

fn task_state(v: &NativeObjectRef) -> Result<Rc<TaskState>, String> {
    let inner = v.inner();
    let b = inner.borrow();
    b.as_any().downcast_ref::<TaskValue>().map(|t| Rc::clone(&t.state)).ok_or_else(|| "not a vm task".into())
}

/// Waits (cooperatively) for a task: its value, or its error.
fn wait_task(state: &Rc<TaskState>) -> Result<Value, String> {
    let s = Rc::clone(state);
    sched::wait_until(move || s.result.borrow().is_some())?;
    match state.result.borrow().as_ref().expect("done") {
        Ok(v) => Ok(v.clone()),
        Err(e) => Err(e.clone()),
    }
}

fn cancel_task(interp: &Interpreter, state: &Rc<TaskState>) -> bool {
    if state.result.borrow().is_some() {
        return false;
    }
    state.cancelled.set(true);
    if let Some(task) = state.task.get() {
        let d = interp.deadline_of(task);
        interp.set_deadline(
            task,
            Some(Deadline {
                at: d.map_or_else(std::time::Instant::now, |d| d.at),
                timeout: d.map_or(Duration::ZERO, |d| d.timeout),
                cancelled: true,
            }),
        );
    }
    sched::note_progress();
    true
}

// ---------------------------------------------------------------------
// Dispatch (called by the interpreter before any other call)
// ---------------------------------------------------------------------

pub fn intercept(callee: &Value, args: &[Value], interp: &Interpreter) -> Option<Result<Value, String>> {
    match callee {
        Value::NativeFn(f) => {
            let name = f.name.strip_prefix("vm.")?;
            Some(module_call(name, args, interp))
        }
        Value::BoundMethod(bm) => match bm.receiver.name().as_str() {
            "VmScript" | "VmContext" | "VmTask" => Some(object_call(&bm.receiver, &bm.method, args, interp)),
            _ => None,
        },
        _ => None,
    }
}

fn module_call(name: &str, args: &[Value], interp: &Interpreter) -> Result<Value, String> {
    let what = format!("vm.{}", name);
    let max = |n: usize| -> Result<(), String> {
        if args.len() > n {
            return Err(format!("{} expects at most {} arguments, got {}", what, n, args.len()));
        }
        Ok(())
    };
    match name {
        // runInNewContext(code, sandbox?, options?)
        "runInNewContext" => {
            max(3)?;
            let code = str_arg(args, 0, &what)?;
            let sandbox = sandbox_arg(args.get(1), &what)?;
            let o = opts(args.get(2), &what)?;
            run_in_new_context(interp, &parse(&code, &o.filename)?, sandbox, &o)
        }
        // runInContext(code, context, options?)
        "runInContext" => {
            max(3)?;
            let code = str_arg(args, 0, &what)?;
            let ctx = context_arg(args.get(1), &what)?;
            let o = opts(args.get(2), &what)?;
            run_in_context(interp, &parse(&code, &o.filename)?, &ctx, &o)
        }
        // runInThisContext(code, options?)
        "runInThisContext" => {
            max(2)?;
            let code = str_arg(args, 0, &what)?;
            let o = opts(args.get(1), &what)?;
            run_in_this_context(interp, &parse(&code, &o.filename)?, &o)
        }
        // spawn(code, sandbox?, { timeout, filename, semaphore, context }) → VmTask
        "spawn" => {
            max(3)?;
            let code = str_arg(args, 0, &what)?;
            let sandbox = sandbox_arg(args.get(1), &what)?;
            let mut o = opts(args.get(2), &what)?;
            let program = parse(&code, &o.filename)?;
            let target = match o.context.take() {
                Some(c) => Target::Context(c),
                None => Target::New(sandbox),
            };
            spawn(interp, program, target, o)
        }
        // all([tasks]) → their values, in order (the first error, if any fails)
        "all" => {
            max(1)?;
            let tasks = match args.first() {
                Some(Value::List(l)) => l.borrow().clone(),
                _ => return Err(format!("{}: expects a list of vm tasks", what)),
            };
            let mut out = Vec::with_capacity(tasks.len());
            for t in &tasks {
                let Value::Object(o) = t else { return Err(format!("{}: expected vm tasks, got '{}'", what, t.type_name())) };
                out.push(wait_task(&task_state(o)?)?);
            }
            Ok(crate::native::util::list(out))
        }
        // createContext(sandbox?)
        "createContext" => {
            max(1)?;
            let sandbox = sandbox_arg(args.first(), &what)?;
            let ctx = ContextValue::new(interp, sandbox);
            ctx.pull();
            Ok(NativeObjectRef::new(ctx).to_value())
        }
        // Script(code, options?)
        "Script" => {
            max(2)?;
            let source = str_arg(args, 0, &what)?;
            let o = opts(args.get(1), &what)?;
            let program = parse(&source, &o.filename)?;
            Ok(NativeObjectRef::new(ScriptValue { program, source, filename: o.filename }).to_value())
        }
        // compileFunction(code, params = [], { context, filename })
        "compileFunction" => {
            max(3)?;
            let code = str_arg(args, 0, &what)?;
            let params: Vec<String> = match args.get(1) {
                None | Some(Value::Nil) => Vec::new(),
                Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
                Some(other) => return Err(format!("{}: params must be a list, got '{}'", what, other.type_name())),
            };
            let o = opts(args.get(2), &what)?;
            // the body starts on the first line, so the lines of errors match
            let src = format!("(fn({}) {{{}\n}})", params.join(", "), code);
            let program = parse(&src, &o.filename)?;
            let ctx = match args.get(2) {
                Some(Value::Map(m)) => m.borrow().get("context").cloned(),
                _ => None,
            };
            match ctx {
                Some(Value::Nil) | None => run_in_new_context(interp, &program, None, &o),
                Some(c) => run_in_context(interp, &program, &context_arg(Some(&c), &what)?, &o),
            }
        }
        other => Err(format!("vm.{} is not a function", other)),
    }
}

fn object_call(receiver: &NativeObjectRef, method: &str, args: &[Value], interp: &Interpreter) -> Result<Value, String> {
    if receiver.name() == "VmTask" {
        let state = task_state(receiver)?;
        return match method {
            "wait" => {
                if !args.is_empty() {
                    return Err("wait() expects no arguments".into());
                }
                wait_task(&state)
            }
            // cancel() → true if it was still running
            "cancel" => Ok(Value::Bool(cancel_task(interp, &state))),
            _ => receiver.call(method, args),
        };
    }
    if receiver.name() == "VmContext" {
        // ctx.run(code, options?) — runInContext with the context first; ctx.spawn(code, options?)
        if method != "run" && method != "spawn" {
            return receiver.call(method, args);
        }
        let code = str_arg(args, 0, method)?;
        let o = opts(args.get(1), method)?;
        let program = parse(&code, &o.filename)?;
        if method == "spawn" {
            return spawn(interp, program, Target::Context(receiver.clone()), o);
        }
        return run_in_context(interp, &program, receiver, &o);
    }
    let (program, filename) = {
        let inner = receiver.inner();
        let b = inner.borrow();
        let s = b.as_any().downcast_ref::<ScriptValue>().ok_or("not a vm script")?;
        (Rc::clone(&s.program), s.filename.clone())
    };
    let with_file = |v: Option<&Value>| -> Result<Opts, String> {
        let mut o = opts(v, method)?;
        if !matches!(v, Some(Value::Str(_))) && !matches!(v, Some(Value::Map(m)) if m.borrow().contains_key("filename")) {
            o.filename = filename.clone();
        }
        Ok(o)
    };
    match method {
        "runInContext" => {
            let ctx = context_arg(args.first(), method)?;
            run_in_context(interp, &program, &ctx, &with_file(args.get(1))?)
        }
        "runInNewContext" => {
            let sandbox = sandbox_arg(args.first(), method)?;
            run_in_new_context(interp, &program, sandbox, &with_file(args.get(1))?)
        }
        "runInThisContext" => run_in_this_context(interp, &program, &with_file(args.first())?),
        // spawn(sandbox | context?, options?) → VmTask
        "spawn" => {
            let o = with_file(args.get(1))?;
            let target = match args.first() {
                Some(Value::Object(c)) if c.name() == "VmContext" => Target::Context(c.clone()),
                v => Target::New(sandbox_arg(v, method)?),
            };
            spawn(interp, program, target, o)
        }
        other => receiver.call(other, args),
    }
}
