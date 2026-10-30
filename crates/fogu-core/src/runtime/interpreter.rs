use std::cell::{Cell, RefCell};
use ahash::AHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::ast::*;
use super::environment::Environment;
use super::jit::{Jit, LoopKind, LoopOutcome};
use super::module::ModuleCache;
use super::ops;
use super::sched;
use crate::native::NativeObjectRef;
use super::value::{
    BoundMethodValue, ChanValue, ErrorValue, FnValue, ModuleValue, NativeFnValue, Str, StructBoundMethodValue,
    StructInstance, StructTypeValue, Value,
    ERR_VALUE_PREFIX,
};


pub enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
}

pub struct Interpreter {
    globals: Rc<RefCell<Environment>>,
    module_cache: ModuleCache,
    base_dir: RefCell<PathBuf>,
    jit: RefCell<Jit>,
    jit_enabled: bool,
    jit_loop_threshold: u32,
    cur_line: Cell<u32>,
    /// File of the code being executed (for errors and `error` traces).
    cur_file: RefCell<Rc<str>>,
    pending: RefCell<Option<Value>>,
    rng: Cell<u64>,
    /// In the REPL redeclaring a name is allowed (replaces the previous one).
    redeclare_ok: Cell<bool>,
    /// `fogu:vm` timeouts and cancellations, per `fg` task: that task's
    /// loops and calls stop once its deadline passes (and it does not use the
    /// JIT meanwhile, since compiled loops cannot be interrupted). Other tasks
    /// are not affected.
    deadlines: RefCell<HashMap<usize, Deadline>>,
    /// `deadlines.len()`: the common case (none) costs one load.
    deadline_count: Cell<usize>,
    /// The scheduler's task-switch hook is installed (see `install_switch_hook`).
    switch_hook: Cell<bool>,
}

impl Drop for Interpreter {
    fn drop(&mut self) {
        if self.switch_hook.get() {
            sched::clear_switch_hook(self as *const Interpreter as usize);
        }
    }
}

/// When a `fogu:vm` run must stop, and why.
#[derive(Clone, Copy)]
pub struct Deadline {
    pub at: std::time::Instant,
    pub timeout: std::time::Duration,
    pub cancelled: bool,
}

/// Sentinel used by `?` to unwind to the end of the function.
const PROPAGATE: &str = "\u{0}propagate";
/// Sentinel of `?.` on `nil`: unwinds to the enclosing `OptChain`.
const OPT_NIL: &str = "\u{0}optnil";

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        let module_cache = ModuleCache::new();
        let globals = Environment::new();
        {
            let mut g = globals.borrow_mut();
            for (name, value) in module_cache.natives().globals() {
                g.define_builtin(name, value);
            }
        }
        let jit = Jit::new();
        let jit_enabled = jit.enabled();
        let jit_loop_threshold = jit.loop_threshold();
        Self {
            globals,
            module_cache,
            base_dir: RefCell::new(PathBuf::from(".")),
            jit: RefCell::new(jit),
            jit_enabled,
            jit_loop_threshold,
            cur_line: Cell::new(0),
            cur_file: RefCell::new(Rc::from("")),
            pending: RefCell::new(None),
            redeclare_ok: Cell::new(false),
            deadlines: RefCell::new(HashMap::new()),
            deadline_count: Cell::new(0),
            switch_hook: Cell::new(false),
            rng: Cell::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0x9E37_79B9_7F4A_7C15)
                    | 1,
            ),
        }
    }

    /// File and line of the last executed statement (for errors).
    pub fn error_location(&self) -> (String, u32) {
        (self.cur_file.borrow().to_string(), self.cur_line.get())
    }

    pub fn set_main_file(&self, f: &str) {
        *self.cur_file.borrow_mut() = Rc::from(f);
        self.globals.borrow_mut().set_module(f);
    }

    /// `_x` fields and methods of structs are private to the module where the
    /// struct was declared (spec §26).
    fn check_private(
        &self,
        target: &Value,
        field: &str,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<(), String> {
        if !field.starts_with('_') {
            return Ok(());
        }
        let Value::Struct(s) = target else {
            return Ok(());
        };
        let s = s.borrow();
        if s.module.is_some() && s.module != env.borrow().module_name() {
            return Err(format!(
                "'{}' is private to the module where '{}' was declared",
                field, s.type_name
            ));
        }
        Ok(())
    }

    /// Declares a name in the `env` scope (error if it already exists there).
    fn declare(
        &self,
        env: &Rc<RefCell<Environment>>,
        name: &str,
        value: Value,
        mutable: bool,
    ) -> Result<(), String> {
        let mut e = env.borrow_mut();
        if self.redeclare_ok.get() {
            e.define(name.to_string(), value, mutable);
            Ok(())
        } else {
            e.declare(name.to_string(), value, mutable)
        }
    }

    /// Enters the file (module) where the code of an environment was defined.
    /// Returns the previous position, to be restored with [`Self::leave`] on
    /// success; on error the innermost position (the error's) is kept.
    fn enter(&self, module: Option<&Rc<str>>) -> (Option<Rc<str>>, u32) {
        let line = self.cur_line.get();
        let file = module.map(|m| self.cur_file.replace(Rc::clone(m)));
        (file, line)
    }

    fn leave(&self, saved: (Option<Rc<str>>, u32)) {
        if let Some(f) = saved.0 {
            *self.cur_file.borrow_mut() = f;
        }
        self.cur_line.set(saved.1);
    }

    /// Runs `f` (which may yield to other `fg` tasks) and then restores the
    /// current position, which the other tasks may have changed.
    /// Each `fg` task keeps its own current file and line: the scheduler
    /// saves them when a task yields and restores them when it resumes.
    fn install_switch_hook(&self) {
        if self.switch_hook.replace(true) {
            return;
        }
        let me = self as *const Interpreter as usize;
        sched::set_switch_hook(
            me,
            Rc::new(move || {
                // SAFETY: as for `fg`: the interpreter outlives its tasks, and
                // the hook is removed when it is dropped.
                let it = unsafe { &*(me as *const Interpreter) };
                let file = it.cur_file.borrow().clone();
                let line = it.cur_line.get();
                Box::new(move || {
                    let it = unsafe { &*(me as *const Interpreter) };
                    *it.cur_file.borrow_mut() = file;
                    it.cur_line.set(line);
                })
            }),
        );
    }

    fn keep_loc<T>(&self, f: impl FnOnce() -> T) -> T {
        let file = self.cur_file.borrow().clone();
        let line = self.cur_line.get();
        let r = f();
        *self.cur_file.borrow_mut() = file;
        self.cur_line.set(line);
        r
    }

    /// Result of a native function: operational errors (marked with
    /// `io_err!`) become `error` values with the current position in the trace.
    fn native_result(&self, r: Result<Value, String>) -> Result<Value, String> {
        match r {
            Err(e) if e.starts_with(ERR_VALUE_PREFIX) => {
                Ok(Value::Error(Rc::new(ErrorValue {
                    message: e[ERR_VALUE_PREFIX.len()..].to_string(),
                    cause: Value::Nil,
                    trace: vec![format!("{}:{}", self.cur_file.borrow(), self.cur_line.get())],
                })))
            }
            other => other,
        }
    }

    /// `import(spec)` at run time: relative paths are resolved from the file of
    /// the code that runs it (like a static import). The module is loaded once
    /// and shared with static imports; a failure is an `error` value.
    fn dyn_import(&self, spec: &str, env: &Rc<RefCell<Environment>>) -> Value {
        let from_dir = env
            .borrow()
            .module_name()
            .and_then(|m| {
                let p = std::path::Path::new(&*m);
                let dir = p.parent().map(|d| if d.as_os_str().is_empty() { std::path::Path::new(".") } else { d });
                dir.map(|d| d.to_path_buf()).filter(|d| d.is_dir() || super::vfs::contains(d))
            })
            .unwrap_or_else(|| self.base_dir.borrow().clone());
        match self.keep_loc(|| self.module_cache.resolve(spec, &from_dir, self)) {
            Ok(module) => Value::Module(module),
            Err(e) => {
                let msg = e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
                Value::Error(Rc::new(ErrorValue {
                    message: format!("import(\"{}\"): {}", spec, msg),
                    cause: Value::Nil,
                    trace: vec![format!("{}:{}", self.cur_file.borrow(), self.cur_line.get())],
                }))
            }
        }
    }

    /// Turns the `?` sentinel into a readable message.
    fn finish_err(&self, e: String) -> String {
        if let Some(msg) = e.strip_prefix(ERR_VALUE_PREFIX) {
            return msg.to_string();
        }
        if e == PROPAGATE {
            let msg = match self.pending.borrow_mut().take() {
                Some(Value::List(l)) => l
                    .borrow()
                    .get(1)
                    .map(|v| v.to_display_string())
                    .unwrap_or_default(),
                Some(v) => v.to_display_string(),
                None => "error".into(),
            };
            format!("{} (propagated by '?' outside a function)", msg)
        } else {
            e
        }
    }

    fn sync_jit_line(&self) {
        let l = self.jit.borrow().last_line();
        if l > 0 {
            self.cur_line.set(l);
        }
    }

    fn next_rand(&self) -> u64 {
        let mut x = self.rng.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.set(x);
        x
    }

    pub fn set_base_dir(&self, dir: &Path) {
        *self.base_dir.borrow_mut() = dir.to_path_buf();
    }

    /// Global native names (for static name checking).
    pub fn builtin_names(&self) -> std::collections::HashSet<String> {
        self.globals.borrow().names().into_iter().collect()
    }

    pub fn globals_ref(&self) -> &Rc<RefCell<Environment>> {
        &self.globals
    }

    /// Prints JIT statistics (`--jit-stats` flag).
    pub fn jit_report(&self) -> String {
        self.jit.borrow().report()
    }

    pub fn interpret(&self, program: &[Stmt]) -> Result<(), String> {
        let exports = Rc::new(ModuleValue {
            name: "<main>".into(),
            exports: RefCell::new(HashMap::new()),
        });
        let env = Rc::clone(&self.globals);
        let result = (|| -> Result<(), String> {
            for stmt in program {
                match self.exec_in_module(stmt, env.clone(), &exports)? {
                    Flow::Normal => {}
                    Flow::Return(_) => return Err("'return' outside a function".into()),
                    Flow::Break => return Err("'break' outside a loop".into()),
                    Flow::Continue => return Err("'continue' outside a loop".into()),
                }
            }
            Ok(())
        })();
        sched::shutdown();
        result.map_err(|e| self.finish_err(e))
    }

    /// A global variable (for an embedding host: `crate::embed`).
    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.borrow().get(name)
    }

    /// Defines (or replaces) a global variable.
    pub fn set_global(&self, name: &str, v: Value) {
        self.globals.borrow_mut().define(name.to_string(), v, true);
    }

    /// The global scope (calls made by an embedding host run in it).
    pub fn globals_env(&self) -> Rc<RefCell<Environment>> {
        Rc::clone(&self.globals)
    }

    /// Runs one REPL line; if it ends in an expression, returns its value.
    /// The `fg` tasks it started end with it.
    pub fn repl_eval(&self, program: &[Stmt]) -> Result<Option<Value>, String> {
        self.eval_global(program, true)
    }

    /// Like `repl_eval`, but tasks, servers and handlers keep running
    /// afterwards (an embedding host: `crate::embed`).
    pub fn embed_eval(&self, program: &[Stmt]) -> Result<Option<Value>, String> {
        self.eval_global(program, false)
    }

    fn eval_global(&self, program: &[Stmt], shutdown: bool) -> Result<Option<Value>, String> {
        self.redeclare_ok.set(true);
        let exports = Rc::new(ModuleValue {
            name: "<repl>".into(),
            exports: RefCell::new(HashMap::new()),
        });
        let env = Rc::clone(&self.globals);
        let mut last = None;
        let n = program.len();
        let result = (|| -> Result<(), String> {
            for (i, stmt) in program.iter().enumerate() {
                if let (true, Stmt::Expr(e)) = (i + 1 == n, stmt) {
                    last = Some(self.eval(e, env.clone())?);
                    continue;
                }
                match self.exec_in_module(stmt, env.clone(), &exports)? {
                    Flow::Normal => {}
                    Flow::Return(_) => return Err("'return' outside a function".into()),
                    Flow::Break => return Err("'break' outside a loop".into()),
                    Flow::Continue => return Err("'continue' outside a loop".into()),
                }
            }
            Ok(())
        })();
        if shutdown {
            sched::shutdown();
        }
        result.map_err(|e| self.finish_err(e))?;
        Ok(last.filter(|v| !matches!(v, Value::Nil)))
    }

    /// The deadline of the running task, if it is inside a `fogu:vm` run with one.
    pub fn deadline(&self) -> Option<Deadline> {
        if self.deadline_count.get() == 0 {
            return None;
        }
        self.deadlines.borrow().get(&sched::task_id()).copied()
    }

    /// Sets (or clears) the deadline of the task `task` (`sched::task_id()`).
    pub fn set_deadline(&self, task: usize, d: Option<Deadline>) {
        let mut m = self.deadlines.borrow_mut();
        match d {
            Some(d) => {
                m.insert(task, d);
            }
            None => {
                m.remove(&task);
            }
        }
        self.deadline_count.set(m.len());
    }

    /// The deadline of another task (`fogu:vm` cancellation).
    pub fn deadline_of(&self, task: usize) -> Option<Deadline> {
        self.deadlines.borrow().get(&task).copied()
    }

    fn no_deadline(&self) -> bool {
        self.deadline_count.get() == 0 || self.deadline().is_none()
    }

    fn check_deadline(&self) -> Result<(), String> {
        match self.deadline() {
            Some(d) if d.cancelled => Err("script execution cancelled".into()),
            Some(d) if std::time::Instant::now() >= d.at => Err(format!(
                "script execution timed out after {}",
                Value::Duration(d.timeout.as_nanos() as i64).to_display_string()
            )),
            _ => Ok(()),
        }
    }

    /// Environment for a `fogu:vm` context: only the global natives
    /// (`isolated`), or a child of this program's globals.
    pub fn vm_env(&self, isolated: bool, file: &str) -> Rc<RefCell<Environment>> {
        let parent = if isolated {
            let g = Environment::new();
            for (name, value) in self.module_cache.natives().globals() {
                g.borrow_mut().define_builtin(name, value);
            }
            g
        } else {
            Rc::clone(&self.globals)
        };
        let env = Environment::new_module(parent);
        env.borrow_mut().set_module(file);
        env
    }

    /// Runs `fogu:vm` code in `env` and returns the value of its last
    /// expression. Errors come back formatted (`file:line: runtime error: …`).
    pub fn vm_run(
        &self,
        program: &[Stmt],
        env: Rc<RefCell<Environment>>,
        file: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<Value, String> {
        let exports = Rc::new(ModuleValue {
            name: file.into(),
            exports: RefCell::new(HashMap::new()),
        });
        let prev_file = self.cur_file.replace(Rc::from(file));
        let prev_line = self.cur_line.get();
        let task = sched::task_id();
        let prev_deadline = self.deadline();
        if let Some(d) = timeout {
            let at = std::time::Instant::now() + d;
            // a nested timeout cannot outlive the outer one
            if prev_deadline.is_none_or(|outer| at < outer.at) {
                let cancelled = prev_deadline.is_some_and(|p| p.cancelled);
                self.set_deadline(task, Some(Deadline { at, timeout: d, cancelled }));
            }
        }
        let n = program.len();
        let mut last = Value::Nil;
        let result = (|| -> Result<(), String> {
            for (i, stmt) in program.iter().enumerate() {
                if let (true, Stmt::Expr(e)) = (i + 1 == n, stmt) {
                    last = self.eval(e, env.clone())?;
                    continue;
                }
                match self.exec_in_module(stmt, env.clone(), &exports)? {
                    Flow::Normal => {}
                    Flow::Return(_) => return Err("'return' outside a function".into()),
                    Flow::Break => return Err("'break' outside a loop".into()),
                    Flow::Continue => return Err("'continue' outside a loop".into()),
                }
            }
            Ok(())
        })();
        let result = result.map_err(|e| {
            let msg = self.finish_err(e);
            let (f, line) = self.error_location();
            if line > 0 {
                format!("{}:{}: runtime error: {}", f, line, msg)
            } else {
                format!("{}: runtime error: {}", f, msg)
            }
        });
        // keep a cancellation that arrived meanwhile
        let cancelled = self.deadline().is_some_and(|d| d.cancelled);
        self.set_deadline(task, prev_deadline.map(|d| Deadline { cancelled: d.cancelled || cancelled, ..d }));
        *self.cur_file.borrow_mut() = prev_file;
        self.cur_line.set(prev_line);
        result.map(|_| last)
    }

    pub fn eval_module(
        &self,
        program: &[Stmt],
        env: Rc<RefCell<Environment>>,
        path: &Path,
        exports: Rc<ModuleValue>,
    ) -> Result<(), String> {
        let prev_dir = self.base_dir.borrow().clone();
        let prev_file = self.cur_file.replace(Rc::from(super::module::display_path(path)));
        let prev_line = self.cur_line.get();
        if let Some(dir) = path.parent() {
            *self.base_dir.borrow_mut() = dir.to_path_buf();
        }

        let result = (|| -> Result<(), String> {
            for stmt in program {
                match self.exec_in_module(stmt, env.clone(), &exports)? {
                    Flow::Normal => {}
                    Flow::Return(_) => return Err("'return' outside a function".into()),
                    Flow::Break => return Err("'break' outside a loop".into()),
                    Flow::Continue => return Err("'continue' outside a loop".into()),
                }
            }
            Ok(())
        })();

        *self.base_dir.borrow_mut() = prev_dir;
        if result.is_ok() {
            *self.cur_file.borrow_mut() = prev_file;
            self.cur_line.set(prev_line);
        }
        result.map_err(|e| self.finish_err(e))
    }

    fn exec_in_module(
        &self,
        stmt: &Stmt,
        env: Rc<RefCell<Environment>>,
        exports: &Rc<ModuleValue>,
    ) -> Result<Flow, String> {
        match stmt {
            Stmt::Import { kind, path } => {
                let from_dir = self.base_dir.borrow().clone();
                let module = self.module_cache.resolve(path, &from_dir, self)?;
                self.module_cache.bind_import(kind, module, &env)?;
                Ok(Flow::Normal)
            }
            Stmt::Export(inner) => match &**inner {
                ExportKind::Fn { name, params, body } => {
                    let f = Value::Fn(Rc::new(FnValue {
                        name: name.clone(),
                        params: params.clone(),
                        body: Rc::clone(body),
                        closure: Some(Rc::clone(&env)),
                        module: env.borrow().module_name(),
                    }));
                    self.declare(&env, name, f.clone(), false)?;
                    exports.exports.borrow_mut().insert(name.clone(), f);
                    Ok(Flow::Normal)
                }
                ExportKind::Let {
                    name,
                    mutable,
                    init,
                    ty,
                } => {
                    let value = self.eval(init, env.clone())?;
                    if let Some(t) = ty {
                        self.check_var(name, &value, t, &env)?;
                    }
                    self.declare(&env, name, value.clone(), *mutable)?;
                    exports.exports.borrow_mut().insert(name.clone(), value);
                    Ok(Flow::Normal)
                }
                ExportKind::Interface(decl) => {
                    let v = Value::Interface(Rc::new(decl.clone()));
                    self.declare(&env, &decl.name, v.clone(), false)?;
                    exports.exports.borrow_mut().insert(decl.name.clone(), v);
                    Ok(Flow::Normal)
                }
                ExportKind::Struct(decl) => {
                    let t = Value::StructType(Rc::new(StructTypeValue {
                        decl: decl.clone(),
                        env: Rc::clone(&env),
                    }));
                    self.declare(&env, &decl.name, t.clone(), false)?;
                    exports.exports.borrow_mut().insert(decl.name.clone(), t);
                    Ok(Flow::Normal)
                }
            },
            other => self.exec(other, env),
        }
    }

    /// Like `eval`: the heavier statements have their own `#[inline(never)]`
    /// methods, so nested blocks and calls keep small frames.
    fn exec(&self, stmt: &Stmt, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        match stmt {
            Stmt::Line(n) => {
                self.cur_line.set(*n);
                Ok(Flow::Normal)
            }

            Stmt::LetList { names, mutable, init } => self.exec_let_list(names, mutable, init, env),

            Stmt::Let {
                name,
                mutable,
                init,
                ty,
            } => self.exec_let(name, mutable, init, ty, env),

            Stmt::CheckType { name, ty } => self.exec_check_type(name, ty, env),

            Stmt::ReturnType(_) => Ok(Flow::Normal),

            Stmt::Interface(decl) => self.exec_interface(decl, env),

            Stmt::Expr(expr) => {
                self.eval(expr, env)?;
                Ok(Flow::Normal)
            }

            Stmt::Block(stmts) => self.exec_scoped_block(stmts, env),

            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.eval(cond, env.clone())?;
                if c.is_truthy() {
                    self.exec(then_branch, env)
                } else if let Some(else_b) = else_branch {
                    self.exec(else_b, env)
                } else {
                    Ok(Flow::Normal)
                }
            }

            Stmt::While { id, cond, body } => self.exec_while(*id, cond, body, env),

            Stmt::For {
                id,
                var,
                var2,
                iter,
                body,
            } => self.exec_for_stmt(id, var, var2, iter, body, env),

            Stmt::Break => Ok(Flow::Break),

            Stmt::Continue => Ok(Flow::Continue),

            Stmt::Return(expr) => {
                let value = match expr {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Nil,
                };
                Ok(Flow::Return(value))
            }

            Stmt::FnDecl { name, params, body } => self.exec_fn_decl(name, params, body, env),

            Stmt::Struct(decl) => self.exec_struct(decl, env),

            Stmt::MatchStmt { subject, arms } => self.exec_match_stmt(subject, arms, env),

            Stmt::Select { cases, default } => self.exec_select(cases, default.as_deref(), env),

            Stmt::SendStmt { chan, value } => self.exec_send(chan, value, env),

            Stmt::Import { .. } => Err("'import' is only allowed at the top of a module".into()),

            Stmt::Export(_) => Err("'export' is only allowed at the top of a module".into()),
        }
    }

    #[inline(never)]
    fn exec_let_list(&self, names: &[String], mutable: &bool, init: &Expr, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let v = self.eval(init, env.clone())?;
        let items: Vec<Value> = match &v {
            Value::List(l) => l.borrow().clone(),
            // `const [v, err] = f()` also works with functions that
            // return a single value or an `error` (e.g. native functions).
            Value::Error(_) => vec![Value::Nil, v.clone()],
            other => vec![other.clone(), Value::Nil],
        };
        for (i, n) in names.iter().enumerate() {
            if n == "_" {
                continue;
            }
            let x = items.get(i).cloned().unwrap_or(Value::Nil);
            self.declare(&env, n, x, *mutable)?;
        }
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_let(&self, name: &str, mutable: &bool, init: &Expr, ty: &Option<TypeExpr>, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let value = self.eval(init, env.clone())?;
        if let Some(t) = ty {
            self.check_var(name, &value, t, &env)?;
        }
        self.declare(&env, name, value, *mutable)?;
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_check_type(&self, name: &str, ty: &TypeExpr, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let v = env.borrow().get(name).unwrap_or(Value::Nil);
        if !self.type_matches(&v, ty, &env)? {
            return Err(format!(
                "type mismatch: parameter '{}' expects {}, got {}",
                name,
                ty,
                describe(&v)
            ));
        }
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_interface(&self, decl: &InterfaceDecl, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let v = Value::Interface(Rc::new(decl.clone()));
        self.declare(&env, &decl.name, v, false)?;
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_scoped_block(&self, stmts: &[Stmt], env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let block_env = Environment::with_parent(env);
        for s in stmts {
            match self.exec(s, block_env.clone())? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_for_stmt(&self, id: &u32, var: &str, var2: &Option<String>, iter: &Expr, body: &Stmt, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let iterable = self.eval(iter, env.clone())?;
        match var2 {
            None => self.exec_for(*id, var, iterable, body, env),
            Some(var2) => self.exec_for_pairs(var, var2, iterable, body, env),
        }
    }

    #[inline(never)]
    fn exec_fn_decl(&self, name: &str, params: &[String], body: &Block, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let f = Value::Fn(Rc::new(FnValue {
            name: name.to_string(),
            params: params.to_vec(),
            body: Rc::clone(body),
            closure: Some(Rc::clone(&env)),
            module: env.borrow().module_name(),
        }));
        self.declare(&env, name, f, false)?;
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_struct(&self, decl: &StructDecl, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let t = Value::StructType(Rc::new(StructTypeValue {
            decl: decl.clone(),
            env: Rc::clone(&env),
        }));
        self.declare(&env, &decl.name, t, false)?;
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_match_stmt(&self, subject: &Expr, arms: &[MatchArm], env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let v = self.eval(subject, env.clone())?;
        let _ = self.eval_match(&v, arms, env)?;
        Ok(Flow::Normal)
    }

    #[inline(never)]
    fn exec_send(&self, chan: &Expr, value: &Expr, env: Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let c = self.eval(chan, env.clone())?;
        let v = self.eval(value, env)?;
        match c {
            Value::Chan(ch) => {
                self.keep_loc(|| chan_send(&ch, v))?;
                Ok(Flow::Normal)
            }
            other => Err(format!(
                "'<-' expects a channel, got '{}'",
                other.type_name()
            )),
        }
    }

    // ---------- while ----------

    fn exec_while(
        &self,
        id: u32,
        cond: &Expr,
        body: &Stmt,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Flow, String> {
        let mut iters: u32 = 0;
        let mut scope = LoopScope::default();
        loop {
            // On-stack replacement (OSR): after a few interpreted
            // iterations, try to compile the whole loop. The state lives in
            // `env`, so the switch is exact.
            self.check_deadline()?;
            if self.jit_enabled && self.no_deadline() {
                iters = iters.saturating_add(1);
                if iters == self.jit_loop_threshold {
                    let out = self
                        .jit
                        .borrow_mut()
                        .run_loop(id, LoopKind::While { cond, body }, &env);
                    match out {
                        LoopOutcome::Done => return Ok(Flow::Normal),
                        LoopOutcome::Err(e) => {
                            self.sync_jit_line();
                            return Err(e);
                        }
                        // a speculation failed before any effect of the
                        // iteration: it is simply re-run here
                        LoopOutcome::Skip | LoopOutcome::Deopt(_) => {}
                    }
                }
            }

            let c = self.eval(cond, env.clone())?;
            if !c.is_truthy() {
                break;
            }
            match scope.run(self, body, &env)? {
                Flow::Normal | Flow::Continue => {}
                Flow::Break => break,
                Flow::Return(v) => return Ok(Flow::Return(v)),
            }
        }
        Ok(Flow::Normal)
    }

    // ---------- for ----------

    fn exec_for(
        &self,
        id: u32,
        var: &str,
        iterable: Value,
        body: &Stmt,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Flow, String> {
        // Body of one iteration; returns Some(flow) if the loop must stop.
        // The scopes of an iteration are reused by the next one unless a
        // closure captured them.
        let mut iter_env = Environment::with_parent(env.clone());
        let mut scope = LoopScope::default();
        let var_sym = super::symbol::intern(var);
        macro_rules! run_body {
            ($item:expr) => {{
                self.check_deadline()?;
                if scope.captured(&iter_env) {
                    iter_env = Environment::with_parent(env.clone());
                    scope = LoopScope::default();
                }
                // the loop variable's slot is reused: no String allocation per iteration
                iter_env.borrow_mut().set_own_sym(var_sym, $item);
                match scope.run(self, body, &iter_env)? {
                    Flow::Normal | Flow::Continue => None,
                    Flow::Break => Some(Flow::Normal),
                    Flow::Return(v) => Some(Flow::Return(v)),
                }
            }};
        }

        match iterable {
            Value::Range(start, end) => {
                let mut resume = start;
                if self.jit_enabled
                    && self.no_deadline()
                    && end.saturating_sub(start) >= self.jit_loop_threshold as i64
                {
                    let out = self.jit.borrow_mut().run_loop(
                        id,
                        LoopKind::For {
                            var,
                            body,
                            lo: start,
                            hi: end,
                        },
                        &env,
                    );
                    match out {
                        LoopOutcome::Done => return Ok(Flow::Normal),
                        LoopOutcome::Err(e) => {
                            self.sync_jit_line();
                            return Err(e);
                        }
                        LoopOutcome::Skip => {}
                        // the native code stopped before any effect of
                        // iteration `at`: continue from it
                        LoopOutcome::Deopt(at) => resume = at,
                    }
                }
                let mut i = if resume > start { resume } else { start };
                while i < end {
                    if let Some(f) = run_body!(Value::Int(i)) {
                        return Ok(f);
                    }
                    i += 1;
                }
                Ok(Flow::Normal)
            }

            Value::List(list) => {
                // a snapshot: the body may modify the list (values are Rc clones, cheap)
                let snapshot: Vec<Value> = list.borrow().clone();
                let mut from = 0usize;
                if self.jit_enabled && self.no_deadline() && snapshot.len() >= self.jit_loop_threshold as usize {
                    let out = self.jit.borrow_mut().run_loop(
                        id,
                        LoopKind::ForList { var, body, items: &snapshot },
                        &env,
                    );
                    match out {
                        LoopOutcome::Done => return Ok(Flow::Normal),
                        LoopOutcome::Err(e) => {
                            self.sync_jit_line();
                            return Err(e);
                        }
                        LoopOutcome::Skip => {}
                        LoopOutcome::Deopt(at) => from = at.max(0) as usize,
                    }
                }
                for item in snapshot.into_iter().skip(from) {
                    if let Some(f) = run_body!(item) {
                        return Ok(f);
                    }
                }
                Ok(Flow::Normal)
            }

            Value::Buffer(b) => {
                let snapshot: Vec<u8> = b.borrow().clone();
                for byte in snapshot {
                    if let Some(f) = run_body!(Value::Int(byte as i64)) {
                        return Ok(f);
                    }
                }
                Ok(Flow::Normal)
            }

            Value::Chan(ch) => {
                loop {
                    let v = match self.keep_loc(|| chan_recv(&ch)) {
                        Ok(v) => v,
                        Err(RecvError::Closed) => break,
                        Err(RecvError::Other(e)) => return Err(e),
                    };
                    if let Some(f) = run_body!(v) {
                        return Ok(f);
                    }
                }
                Ok(Flow::Normal)
            }

            Value::Str(s) => {
                for ch in s.chars() {
                    if let Some(f) = run_body!(Value::Str(ch.into())) {
                        return Ok(f);
                    }
                }
                Ok(Flow::Normal)
            }

            Value::Map(m) => {
                // Deterministic (alphabetical) key order.
                let mut keys: Vec<String> = m.borrow().keys().cloned().collect();
                keys.sort();
                for k in keys {
                    if let Some(f) = run_body!(Value::Str(k.into())) {
                        return Ok(f);
                    }
                }
                Ok(Flow::Normal)
            }

            Value::Object(o) if o.name() == "TcpListener" => {
                loop {
                    let conn = self.call_native_method(&o, "accept", &[])?;
                    if let Some(f) = run_body!(conn) {
                        return Ok(f);
                    }
                }
            }

            other => Err(format!(
                "value of type '{}' is not iterable",
                other.type_name()
            )),
        }
    }

    /// `for k, v in map` (key, value) and `for i, x in list|str` (index, item).
    fn exec_for_pairs(
        &self,
        var: &str,
        var2: &str,
        iterable: Value,
        body: &Stmt,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Flow, String> {
        let pairs: Vec<(Value, Value)> = match &iterable {
            Value::Map(m) => {
                let b = m.borrow();
                let mut keys: Vec<&String> = b.keys().collect();
                keys.sort();
                keys.into_iter()
                    .map(|k| (Value::Str(k.clone().into()), b[k].clone()))
                    .collect()
            }
            Value::List(l) => l
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, v)| (Value::Int(i as i64), v.clone()))
                .collect(),
            Value::Buffer(b) => b
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, x)| (Value::Int(i as i64), Value::Int(*x as i64)))
                .collect(),
            Value::Str(s) => s
                .chars()
                .enumerate()
                .map(|(i, c)| (Value::Int(i as i64), Value::Str(c.to_string().into())))
                .collect(),
            other => {
                return Err(format!(
                    "'for {}, {} in' expects a map, list or str, got '{}'",
                    var,
                    var2,
                    other.type_name()
                ))
            }
        };
        let mut iter_env = Environment::with_parent(env.clone());
        let mut scope = LoopScope::default();
        let (sym1, sym2) = (super::symbol::intern(var), super::symbol::intern(var2));
        for (a, b) in pairs {
            self.check_deadline()?;
            if scope.captured(&iter_env) {
                iter_env = Environment::with_parent(env.clone());
                scope = LoopScope::default();
            }
            {
                let mut e = iter_env.borrow_mut();
                e.set_own_sym(sym1, a);
                e.set_own_sym(sym2, b);
            }
            match scope.run(self, body, &iter_env)? {
                Flow::Normal | Flow::Continue => {}
                Flow::Break => break,
                Flow::Return(v) => return Ok(Flow::Return(v)),
            }
        }
        Ok(Flow::Normal)
    }

    // ---------- select ----------

    fn exec_select(
        &self,
        cases: &[SelectCase],
        default: Option<&[Stmt]>,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Flow, String> {
        // The channels and the values to send are evaluated only once.
        enum Ready {
            Recv(Rc<ChanValue>),
            Send(Rc<ChanValue>, Value),
        }
        let mut evaluated: Vec<Ready> = Vec::with_capacity(cases.len());
        for case in cases {
            match case {
                SelectCase::Recv { chan, .. } => match self.eval(chan, env.clone())? {
                    Value::Chan(c) => evaluated.push(Ready::Recv(c)),
                    other => {
                        return Err(format!(
                            "select: '<-' expects a channel, got '{}'",
                            other.type_name()
                        ))
                    }
                },
                SelectCase::Send { chan, value, .. } => {
                    let c = self.eval(chan, env.clone())?;
                    let v = self.eval(value, env.clone())?;
                    match c {
                        Value::Chan(c) => evaluated.push(Ready::Send(c, v)),
                        other => {
                            return Err(format!(
                                "select: '<-' expects a channel, got '{}'",
                                other.type_name()
                            ))
                        }
                    }
                }
            }
        }

        let is_ready = |r: &Ready| -> bool {
            match r {
                Ready::Recv(c) => {
                    c.poll_timer();
                    !c.queue.borrow().is_empty() || c.closed.get()
                }
                Ready::Send(c, _) => {
                    c.closed.get() || c.queue.borrow().len() < c.capacity()
                }
            }
        };

        loop {
            // Uniform choice among the ready cases (like Go).
            let ready: Vec<usize> = (0..evaluated.len())
                .filter(|&i| is_ready(&evaluated[i]))
                .collect();
            if !ready.is_empty() {
                let idx = ready[(self.next_rand() % ready.len() as u64) as usize];
                let r = &evaluated[idx];
                match (&cases[idx], r) {
                    (SelectCase::Recv { var, body, .. }, Ready::Recv(ch)) => {
                        let v = match chan_recv(ch) {
                            Ok(v) => v,
                            Err(RecvError::Closed) => Value::Nil,
                            Err(RecvError::Other(e)) => return Err(e),
                        };
                        let case_env = Environment::with_parent(env.clone());
                        case_env.borrow_mut().define(var.clone(), v, false);
                        return self.exec_block(body, case_env);
                    }
                    (SelectCase::Send { body, .. }, Ready::Send(ch, v)) => {
                        chan_push(ch, v.clone())?;
                        return self.exec_block(body, env);
                    }
                    _ => unreachable!(),
                }
            }

            if let Some(d) = default {
                return self.exec_block(d, env);
            }
            if evaluated.is_empty() {
                return Err("select without cases".into());
            }

            // Block until some case is ready.
            self.keep_loc(|| sched::wait_until(|| evaluated.iter().any(|r| is_ready(r))))?;
        }
    }

    fn exec_block(
        &self,
        stmts: &[Stmt],
        env: Rc<RefCell<Environment>>,
    ) -> Result<Flow, String> {
        for s in stmts {
            match self.exec(s, env.clone())? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    // ---------- match ----------

    fn eval_match(
        &self,
        subject: &Value,
        arms: &[MatchArm],
        env: Rc<RefCell<Environment>>,
    ) -> Result<Value, String> {
        for arm in arms {
            if let Some(binds) = self.match_pattern(&arm.pattern, subject, env.clone())? {
                let arm_env = Environment::with_parent(env.clone());
                for (k, v) in binds {
                    arm_env.borrow_mut().define(k, v, false);
                }
                return self.eval(&arm.body, arm_env);
            }
        }
        Err(ops::ERR_MATCH.into())
    }

    fn match_pattern(
        &self,
        pattern: &Pattern,
        value: &Value,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Option<Vec<(String, Value)>>, String> {
        match pattern {
            Pattern::Wildcard => Ok(Some(Vec::new())),
            Pattern::Literal(expr) => {
                let expected = self.eval(expr, env)?;
                if values_equal(&expected, value) {
                    Ok(Some(Vec::new()))
                } else {
                    Ok(None)
                }
            }
            Pattern::Ident(name) => Ok(Some(vec![(name.clone(), value.clone())])),
            Pattern::IdentBind { name, guard } => {
                let bind_env = Environment::with_parent(env.clone());
                bind_env
                    .borrow_mut()
                    .define(name.clone(), value.clone(), false);
                if let Some(g) = guard {
                    let v = self.eval(g, bind_env)?;
                    if v.is_truthy() {
                        Ok(Some(vec![(name.clone(), value.clone())]))
                    } else {
                        Ok(None)
                    }
                } else {
                    Ok(Some(vec![(name.clone(), value.clone())]))
                }
            }
            Pattern::Or(pats) => {
                for p in pats {
                    if let Some(b) = self.match_pattern(p, value, env.clone())? {
                        return Ok(Some(b));
                    }
                }
                Ok(None)
            }
        }
    }

    // ---------- eval ----------

    /// A small dispatcher: every non-trivial kind of expression has its own
    /// `#[inline(never)]` method, so the recursion through `eval` only pays
    /// for the locals of the kinds actually being evaluated (the native stack
    /// of a parked `fg` task is mostly these frames).
    fn eval(&self, expr: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        match expr {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(n) => Ok(Value::Float(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Duration(ns) => Ok(Value::Duration(*ns)),
            Expr::DynImport(spec) => {
                let spec = match self.eval(spec, env.clone())? {
                    Value::Str(s) => s.to_string(),
                    other => return Err(format!("import() expects a module path (str), got {}", other.type_name())),
                };
                Ok(self.dyn_import(&spec, &env))
            }
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nil => Ok(Value::Nil),
            Expr::Template(parts) => self.eval_template(parts, env),
            Expr::OptAccess { target, field } => self.eval_opt_access(target, field, env),
            Expr::OptChain(inner) => self.eval_opt_chain(inner, env),
            Expr::Ident(name) => env.borrow().get_sym(name.sym).ok_or_else(|| undefined(name)),
            Expr::List(elems) => self.eval_list(elems, env),
            Expr::Map(entries) => self.eval_map(entries, env),
            Expr::Assign { target, op, value } => self.eval_assign(target, *op, value, env),
            Expr::Unary { op, right } => self.eval_unary(op, right, env),
            Expr::Logical { left, op, right } => self.eval_logical(left, op, right, env),
            Expr::Range { start, end } => self.eval_range(start, end, env),
            Expr::Binary { left, op, right } => self.eval_binary_expr(left, op, right, env),
            Expr::Index { target, index } => self.eval_index_expr(target, index, env),
            Expr::Access { target, field } => self.eval_access_expr(target, field, env),
            Expr::Fn { params, body } => self.eval_fn_literal(params, body, env),
            Expr::Fg { call } => self.eval_fg(call, env),
            Expr::Chan { buffer } => self.eval_chan(buffer, env),
            Expr::Recv { chan } => self.eval_recv(chan, env),
            Expr::Match { subject, arms } => self.eval_match_expr(subject, arms, env),
            Expr::New { name, args } => self.eval_new(name, args, env),
            Expr::Call { callee, args } => self.eval_call(callee, args, env),
            Expr::Try { expr } => {
                let v = self.eval(expr, env)?;
                self.propagate(v)
            }
        }
    }

    #[inline(never)]
    fn eval_opt_chain(&self, inner: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        match self.eval(inner, env) {
            Err(e) if e == OPT_NIL => Ok(Value::Nil),
            r => r,
        }
    }

    #[inline(never)]
    fn eval_fn_literal(&self, params: &[String], body: &Block, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        Ok(Value::Fn(Rc::new(FnValue {
            name: "<anonymous>".into(),
            params: params.to_vec(),
            body: Rc::clone(body),
            closure: Some(Rc::clone(&env)),
            module: env.borrow().module_name(),
        })))
    }

    #[inline(never)]
    fn eval_template(&self, parts: &[Expr], env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let mut out = String::new();
        for p in parts {
            match self.eval(p, env.clone())? {
                Value::Str(s) => out.push_str(&s),
                v => out.push_str(&v.to_display_string()),
            }
        }
        Ok(Value::Str(out.into()))
    }

    #[inline(never)]
    fn eval_opt_access(&self, target: &Expr, field: &str, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        match self.eval(target, env.clone())? {
        Value::Nil => Err(OPT_NIL.into()),
        t => {
            self.check_private(&t, field, &env)?;
            self.eval_access(t, field)
        }
    }
    }

    #[inline(never)]
    fn eval_list(&self, elems: &[Expr], env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let mut out = Vec::with_capacity(elems.len());
        for e in elems {
            out.push(self.eval(e, env.clone())?);
        }
        Ok(Value::List(Rc::new(RefCell::new(out))))
    }

    #[inline(never)]
    fn eval_map(&self, entries: &[(String, Expr)], env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let mut map = HashMap::new();
        for (k, v) in entries {
            map.insert(k.clone(), self.eval(v, env.clone())?);
        }
        Ok(Value::Map(Rc::new(RefCell::new(map))))
    }

    #[inline(never)]
    fn eval_unary(&self, op: &UnaryOp, right: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let r = self.eval(right, env)?;
        match op {
            UnaryOp::Neg => match r {
                Value::Int(n) => n
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or_else(|| ops::ERR_OVERFLOW.to_string()),
                Value::Float(n) => Ok(Value::Float(-n)),
                Value::Duration(n) => n
                    .checked_neg()
                    .map(Value::Duration)
                    .ok_or_else(|| ops::ERR_OVERFLOW.to_string()),
                other => Err(format!(
                    "operator '-' not applicable to '{}'",
                    other.type_name()
                )),
            },
            UnaryOp::Not => Ok(Value::Bool(!r.is_truthy())),
        }
    }

    #[inline(never)]
    fn eval_logical(&self, left: &Expr, op: &LogicalOp, right: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let l = self.eval(left, env.clone())?;
        match op {
            LogicalOp::And => {
                if !l.is_truthy() {
                    Ok(l)
                } else {
                    self.eval(right, env)
                }
            }
            LogicalOp::Or => {
                if l.is_truthy() {
                    Ok(l)
                } else {
                    self.eval(right, env)
                }
            }
            LogicalOp::Coalesce => {
                if matches!(l, Value::Nil) {
                    self.eval(right, env)
                } else {
                    Ok(l)
                }
            }
        }
    }

    #[inline(never)]
    fn eval_range(&self, start: &Expr, end: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let s = self.eval(start, env.clone())?;
        let e = self.eval(end, env)?;
        match (s, e) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Range(a, b)),
            _ => Err("range '..' requires integers".into()),
        }
    }

    #[inline(never)]
    fn eval_binary_expr(&self, left: &Expr, op: &BinaryOp, right: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let l = self.eval(left, env.clone())?;
        let r = self.eval(right, env.clone())?;
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
            if let Some(eq) = self.struct_equals(&l, &r, env)? {
                return Ok(Value::Bool(eq == (*op == BinaryOp::Eq)));
            }
        }
        eval_binary(l, *op, r)
    }

    #[inline(never)]
    fn eval_index_expr(&self, target: &Expr, index: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let t = self.eval(target, env.clone())?;
        let i = self.eval(index, env)?;
        eval_index(t, i)
    }

    #[inline(never)]
    fn eval_access_expr(&self, target: &Expr, field: &str, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let t = self.eval(target, env.clone())?;
        self.check_private(&t, field, &env)?;
        self.eval_access(t, field)
    }

    #[inline(never)]
    fn eval_fg(&self, call: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let Expr::Call { callee, args } = call else {
            return Err("'fg' requires a function call".into());
        };
        // Callee and arguments are evaluated now, in the current task.
        let callee_val = self.eval(callee, env.clone())?;
        let mut arg_vals = Vec::with_capacity(args.len());
        for a in args {
            arg_vals.push(self.eval(a, env.clone())?);
        }
        self.install_switch_hook();
        let me = self as *const Interpreter as usize;
        let genv = env.clone();
        self.keep_loc(|| sched::spawn(Box::new(move || {
            // SAFETY: the interpreter lives until the end of the program;
            // pending tasks are discarded in `shutdown`.
            let it = unsafe { &*(me as *const Interpreter) };
            it.call_value(callee_val, arg_vals, genv).map(|_| ())
        })))?;
        Ok(Value::Nil)
    }

    #[inline(never)]
    fn eval_chan(&self, buffer: &Option<Box<Expr>>, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let buf = match buffer {
            Some(b) => match self.eval(b, env)? {
                Value::Int(n) if n >= 0 => n as usize,
                other => {
                    return Err(format!(
                        "chan buffer must be an int >= 0, got '{}'",
                        other.type_name()
                    ))
                }
            },
            None => 0,
        };
        Ok(Value::Chan(Rc::new(ChanValue::new(buf))))
    }

    #[inline(never)]
    fn eval_recv(&self, chan: &Expr, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let c = self.eval(chan, env)?;
        match c {
            Value::Chan(ch) => match self.keep_loc(|| chan_recv(&ch)) {
                Ok(v) => Ok(v),
                Err(RecvError::Closed) => Ok(Value::Nil),
                Err(RecvError::Other(e)) => Err(e),
            },
            other => Err(format!(
                "'<-' expects a channel, got '{}'",
                other.type_name()
            )),
        }
    }

    #[inline(never)]
    fn eval_match_expr(&self, subject: &Expr, arms: &[MatchArm], env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let v = self.eval(subject, env.clone())?;
        self.eval_match(&v, arms, env)
    }

    #[inline(never)]
    fn eval_new(&self, name: &str, args: &[Expr], env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        let t = env
            .borrow()
            .get(name)
            .ok_or_else(|| format!("struct '{}' is not defined", name))?;
        let tv = match t {
            Value::StructType(d) => d,
            other => {
                return Err(format!(
                    "'{}' is not a struct type (it is '{}')",
                    name,
                    other.type_name()
                ))
            }
        };

        let mut arg_vals = Vec::with_capacity(args.len());
        for a in args {
            arg_vals.push(self.eval(a, env.clone())?);
        }

        let inst = self.instantiate_struct(&tv, arg_vals, false)?;
        Ok(Value::Struct(inst))
    }

    #[inline(never)]
    fn eval_call(&self, callee: &Expr, args: &[Expr], env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        match callee {
            Expr::Access { target, field } => {
                let recv = self.eval(target, env.clone())?;
                return self.call_method(recv, field, args, env);
            }
            // `target?.method(...)`: if the target is nil the chain is
            // nil (the arguments are not even evaluated).
            Expr::OptAccess { target, field } => {
                let recv = self.eval(target, env.clone())?;
                if matches!(recv, Value::Nil) {
                    return Err(OPT_NIL.into());
                }
                return self.call_method(recv, field, args, env);
            }
            _ => {}
        }
        let callee_val = self.eval(callee, env.clone())?;
        let mut arg_vals = Vec::with_capacity(args.len());
        for a in args {
            arg_vals.push(self.eval(a, env.clone())?);
        }
        self.call_value(callee_val, arg_vals, env)
    }

    /// `a == b` between structs uses the `equals(other)` method of the left
    /// side, if it exists (spec §12). `None` = normal equality.
    fn struct_equals(
        &self,
        l: &Value,
        r: &Value,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Option<bool>, String> {
        let Value::Struct(s) = l else {
            return Ok(None);
        };
        if !s.borrow().methods.contains_key("equals") {
            return Ok(None);
        }
        let m = Value::struct_bound(Rc::clone(s), "equals");
        Ok(Some(self.call_value(m, vec![r.clone()], env)?.is_truthy()))
    }

    /// `recv.field(args)`: built-in list/map/str methods (`xs.push(1)`)
    /// or a call of the field's value.
    fn call_method(
        &self,
        recv: Value,
        field: &str,
        args: &[Expr],
        env: Rc<RefCell<Environment>>,
    ) -> Result<Value, String> {
        let is_builtin_recv = match &recv {
            Value::List(_) | Value::Str(_) | Value::Buffer(_) => true,
            Value::Map(m) => !m.borrow().contains_key(field),
            _ => false,
        };
        let mut arg_vals = Vec::with_capacity(args.len());
        for a in args {
            arg_vals.push(self.eval(a, env.clone())?);
        }
        self.check_private(&recv, field, &env)?;
        if is_builtin_recv {
            if let Some(r) = crate::native::collections::call_method(self, &recv, field, &arg_vals) {
                return r;
            }
            return Err(format!(
                "'{}' has no method '{}'",
                recv.type_name(),
                field
            ));
        }
        let callee_val = self.eval_access(recv, field)?;
        self.call_value(callee_val, arg_vals, env)
    }

    // ---------- assignment ----------

    #[inline(never)]
    fn eval_assign(
        &self,
        target: &Expr,
        op: Option<BinaryOp>,
        value: &Expr,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Value, String> {
        match target {
            Expr::Ident(name) => {
                // `s += x` and `s = s + x`: the operands, in the usual order
                let append = match (op, value) {
                    (Some(bop), _) => Some((bop, value)),
                    (None, Expr::Binary { left, op: BinaryOp::Add, right }) if matches!(&**left, Expr::Ident(l) if l == name) => Some((BinaryOp::Add, &**right)),
                    _ => None,
                };
                let v = match append {
                    None => self.eval(value, env.clone())?,
                    Some((bop, rhs_expr)) => {
                        let cur = env
                            .borrow()
                            .get_sym(name.sym)
                            .ok_or_else(|| format!("variable '{}' is not defined", name))?;
                        let rhs = self.eval(rhs_expr, env.clone())?;
                        if let (BinaryOp::Add, Value::Str(_), Value::Str(r)) = (bop, &cur, &rhs) {
                            return self.append_in_place(name, cur, r, &env);
                        }
                        eval_binary(cur, bop, rhs)?
                    }
                };
                env.borrow_mut().assign_sym(name.sym, v.clone())?;
                Ok(v)
            }

            Expr::Access { target: obj, field } => {
                let o = self.eval(obj, env.clone())?;
                self.check_private(&o, field, &env)?;
                let v = match op {
                    None => self.eval(value, env)?,
                    Some(bop) => {
                        let cur = self.eval_access(o.clone(), field)?;
                        let rhs = self.eval(value, env)?;
                        eval_binary(cur, bop, rhs)?
                    }
                };
                self.assign_field(&o, field, v.clone())?;
                Ok(v)
            }

            Expr::Index { target: obj, index } => {
                let o = self.eval(obj, env.clone())?;
                let i = self.eval(index, env.clone())?;
                let v = match op {
                    None => self.eval(value, env)?,
                    Some(bop) => {
                        let cur = eval_index(o.clone(), i.clone())?;
                        let rhs = self.eval(value, env)?;
                        eval_binary(cur, bop, rhs)?
                    }
                };
                assign_index(&o, i, v.clone())?;
                Ok(v)
            }

            _ => Err("invalid assignment target".into()),
        }
    }

    /// Runs an asynchronous native method (`start_async`) to completion,
    /// waiting cooperatively; `None` when `method` is an ordinary one.
    fn poll_async(&self, receiver: &NativeObjectRef, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        let mut poller = match receiver.start_async(method, args)? {
            Ok(p) => p,
            Err(e) => return Some(Err(e)),
        };
        let mut out = None;
        let waited = self.keep_loc(|| {
            sched::wait_until(|| {
                if out.is_none() {
                    out = poller();
                }
                out.is_some()
            })
        });
        Some(waited.and_then(|()| out.expect("ready")))
    }

    /// A native method called by the interpreter itself (`for conn in
    /// listener`): asynchronous ones wait cooperatively.
    fn call_native_method(&self, receiver: &NativeObjectRef, method: &str, args: &[Value]) -> Result<Value, String> {
        match self.poll_async(receiver, method, args) {
            Some(r) => r,
            None => receiver.call(method, args),
        }
    }

    /// `name = cur + rhs` for strings, growing the text in place when the
    /// variable holds the only other reference (amortized O(1) appends
    /// instead of copying the whole string every time).
    fn append_in_place(&self, name: &str, cur: Value, rhs: &Str, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
        let Value::Str(mut s) = cur else { unreachable!() };
        env.borrow_mut().update(name, |slot| {
            // the variable still holds the value read above: let go of its reference
            if matches!(slot, Value::Str(old) if Str::ptr_eq(old, &s)) {
                *slot = Value::Nil;
            }
            s.push_str(rhs);
            *slot = Value::Str(s.clone());
            Value::Str(s)
        })
    }

    fn assign_field(&self, target: &Value, field: &str, v: Value) -> Result<(), String> {
        match target {
            Value::Map(m) => {
                m.borrow_mut().insert(field.to_string(), v);
                Ok(())
            }
            Value::Struct(s) => {
                let mut b = s.borrow_mut();
                if !b.fields.contains_key(field) {
                    return Err(format!(
                        "field '{}' does not exist on '{}'",
                        field, b.type_name
                    ));
                }
                b.fields.insert(field.to_string(), v);
                Ok(())
            }
            Value::Object(o) => o.set(field, v),
            Value::Module(_) => Err(format!(
                "cannot assign to '{}': module exports are read-only",
                field
            )),
            other => Err(format!(
                "assignment '.{}' not applicable to '{}'",
                field,
                other.type_name()
            )),
        }
    }

    // ---------- call ----------

    /// Starts an `fg` task that runs `f` (used by native modules that must
    /// call Fogu code without blocking the caller, e.g. WebSocket handlers).
    pub fn spawn_task(&self, f: Box<dyn FnOnce(&Interpreter) -> Result<(), String>>) -> Result<(), String> {
        self.install_switch_hook();
        let me = self as *const Interpreter as usize;
        sched::spawn(Box::new(move || {
            // SAFETY: as for `fg`: the interpreter lives until the end of the
            // program and pending tasks are discarded in `shutdown`.
            let it = unsafe { &*(me as *const Interpreter) };
            f(it)
        }))
    }

    pub fn call_value(
        &self,
        callee_val: Value,
        arg_vals: Vec<Value>,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Value, String> {
        // the standard library serves some calls itself (servers, `fogu:vm`…);
        // it only intercepts native functions and native objects' methods, so
        // calls of Fogu functions and struct methods skip that chain
        if let Some(host) = crate::host::get() {
            let interceptable = match &callee_val {
                Value::NativeFn(f) => match f.intercept.get() {
                    0 => {
                        let may = host.may_intercept_native(f);
                        f.intercept.set(if may { 2 } else { 1 });
                        may
                    }
                    n => n == 2,
                },
                Value::BoundMethod(_) => true,
                _ => false,
            };
            if interceptable {
                match host.intercept(&callee_val, &arg_vals, self, &env) {
                    Some(crate::host::Handled::Native(r)) => return self.native_result(r),
                    Some(crate::host::Handled::Direct(r)) => return r,
                    None => {}
                }
            }
            host.before_call(self, &env);
        }

        // === EventEmitter.emit interception ===
        self.call_value_rest(callee_val, arg_vals, env)
    }

    fn call_value_rest(
        &self,
        callee_val: Value,
        arg_vals: Vec<Value>,
        env: Rc<RefCell<Environment>>,
    ) -> Result<Value, String> {
        // each kind of callee has its own `#[inline(never)]` method: the
        // recursion of Fogu calls only carries the frame of the kind in use
        match callee_val {
            Value::Fn(f) => self.call_fn(f, arg_vals),
            Value::NativeFn(f) => self.call_native_fn(&f, arg_vals),
            Value::BoundMethod(b) if b.method == "emit" && b.receiver.name() == "EventEmitter" => {
                self.emit_event(&b.receiver, arg_vals)
            }
            // a method value is usually fresh (`obj.m(...)`): take its parts without copying
            Value::BoundMethod(b) => {
                let b = Rc::try_unwrap(b).unwrap_or_else(|b| BoundMethodValue { receiver: b.receiver.clone(), method: b.method.clone() });
                self.call_bound(b.receiver, b.method, arg_vals, env)
            }
            Value::StructBoundMethod(b) => {
                let b = Rc::try_unwrap(b).unwrap_or_else(|b| StructBoundMethodValue { receiver: Rc::clone(&b.receiver), method: b.method.clone() });
                self.call_struct_method(b.receiver, b.method, arg_vals)
            }
            other => Err(format!(
                "value of type '{}' is not callable",
                other.type_name()
            )),
        }
    }

    /// `EventEmitter.emit(event, ...)` calls the listeners.
    #[inline(never)]
    fn emit_event(&self, receiver: &NativeObjectRef, arg_vals: Vec<Value>) -> Result<Value, String> {
        if arg_vals.is_empty() {
            return Err("emit(event, ...) expects at least 1 argument".into());
        }
        let event = match &arg_vals[0] {
            Value::Str(s) => s.clone(),
            other => {
                return Err(format!(
                    "emit: the event name must be str, got '{}'",
                    other.type_name()
                ))
            }
        };
        let call_args: Vec<Value> = arg_vals[1..].to_vec();

        let snap: Vec<(Value, u64, bool)> =
            receiver.with_emitter(|e| e.get_listeners(&event))?;

        if snap.is_empty() {
            return Ok(Value::Bool(false));
        }

        for (func, id, once) in &snap {
            // a `once` listener is removed before it runs (as in Node), so an
            // emit of the same event from inside it does not run it again;
            // `on` listeners stay
            if *once {
                receiver.with_emitter_mut(|e| {
                    e.remove_listener_by_id(&event, *id);
                })?;
            }
            self.call_value(func.clone(), call_args.clone(), self.globals.clone())?;
        }

        return Ok(Value::Bool(true));
    }

    #[inline(never)]
    fn call_fn(&self, f: Rc<FnValue>, arg_vals: Vec<Value>) -> Result<Value, String> {
        check_arity("function", &f.name, &f.params, arg_vals.len())?;
        let saved = self.enter(f.module.as_ref());

        // Try native execution (hot numeric functions).
        if self.jit_enabled && self.no_deadline() {
            let r = self.jit.borrow_mut().call_fn(&f, &arg_vals);
            if let Some(r) = r {
                if r.is_err() {
                    self.sync_jit_line();
                } else {
                    self.leave(saved);
                }
                return r;
            }
        }

        if !sched::stack_ok() {
            return Err(ops::ERR_STACK.into());
        }
        if let Err(e) = self.check_deadline() {
            self.leave(saved);
            return Err(e);
        }

        let parent = f.closure.clone().unwrap_or_else(|| self.globals.clone());
        let fn_env = Environment::with_parent(parent);
        bind_params(&mut fn_env.borrow_mut(), &f.params, arg_vals);
        let r = self.run_body(&f.name, &f.body, fn_env);
        if r.is_ok() {
            self.leave(saved);
        }
        r
    }

    #[inline(never)]
    fn call_native_fn(&self, f: &NativeFnValue, arg_vals: Vec<Value>) -> Result<Value, String> {
        if f.variadic {
            if arg_vals.len() < f.arity {
                return Err(format!(
                    "native function '{}' expects at least {} arguments, got {}",
                    f.name,
                    f.arity,
                    arg_vals.len()
                ));
            }
        } else if arg_vals.len() != f.arity {
            return Err(format!(
                "native function '{}' expects {} arguments, got {}",
                f.name,
                f.arity,
                arg_vals.len()
            ));
        }
        self.native_result((f.func)(&arg_vals))
    }

    #[inline(never)]
    fn call_bound(&self, receiver: NativeObjectRef, method: String, arg_vals: Vec<Value>, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
        // methods that need the interpreter (`Agent.run`, `GrpcServer.serve`)
        if let Some(r) = crate::host::get().and_then(|h| h.intercept_method(&receiver, &method, &arg_vals, self, &env)) {
            return self.native_result(r);
        }
        // blocking methods (`Mutex.lock`, `WaitGroup.wait`) first wait
        // cooperatively, without holding a borrow of the object
        if let Some(ready) = receiver.wait_for(&method) {
            self.keep_loc(|| sched::wait_until(|| ready()))?;
        }
        // asynchronous methods (network requests) are polled cooperatively
        if let Some(r) = self.poll_async(&receiver, &method, &arg_vals) {
            return self.native_result(r);
        }
        // `Once.do(fn)` calls a Fogu function: only the first time
        if method == "do" && receiver.name() == "Once" {
            let f = arg_vals.into_iter().next().ok_or("Once.do(fn) expects a function")?;
            return match receiver.call("claim", &[])? {
                Value::Bool(true) => self.call_value(f, Vec::new(), env),
                _ => Ok(Value::Nil),
            };
        }
        self.native_result(receiver.call(&method, &arg_vals))
    }

    #[inline(never)]
    fn call_struct_method(&self, receiver: Rc<RefCell<StructInstance>>, method: String, arg_vals: Vec<Value>) -> Result<Value, String> {
        let f = {
            let b = receiver.borrow();
            b.methods
                .get(&method)
                .cloned()
                .ok_or_else(|| format!("method '{}' does not exist", method))?
        };
        check_arity("method", &method, &f.params, arg_vals.len())?;
        if !sched::stack_ok() {
            return Err(ops::ERR_STACK.into());
        }
        let saved = self.enter(f.module.as_ref());
        let fn_env = Environment::with_parent(
            f.closure.clone().unwrap_or_else(|| self.globals.clone()),
        );
        {
            let mut e = fn_env.borrow_mut();
            e.define("this".into(), Value::Struct(Rc::clone(&receiver)), false);
            bind_params(&mut e, &f.params, arg_vals);
        }
        let r = self.run_body(&f.name, &f.body, fn_env);
        if r.is_ok() {
            self.leave(saved);
        }
        r
    }

    fn run_body(
        &self,
        name: &str,
        body: &[Stmt],
        fn_env: Rc<RefCell<Environment>>,
    ) -> Result<Value, String> {
        let saved_line = self.cur_line.get();
        let mut out = Ok(Value::Nil);
        for stmt in body {
            match self.exec(stmt, fn_env.clone()) {
                Ok(Flow::Normal) => {}
                Ok(Flow::Return(v)) => {
                    out = Ok(v);
                    break;
                }
                Ok(Flow::Break) => return Err("'break' outside a loop".into()),
                Ok(Flow::Continue) => return Err("'continue' outside a loop".into()),
                Err(e) if e == PROPAGATE => {
                    // `?`: the function returns the saved (nil, error) pair —
                    // or just the error when its declared return type is not
                    // a tuple (`fn main(): error`), keeping the return type.
                    let pair = self.pending.borrow_mut().take().unwrap_or(Value::Nil);
                    out = Ok(match (body.first(), &pair) {
                        (Some(Stmt::ReturnType(t)), Value::List(l)) if !matches!(t, TypeExpr::Tuple(_)) => {
                            l.borrow().get(1).cloned().unwrap_or(Value::Nil)
                        }
                        _ => pair,
                    });
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        if let (Ok(v), Some(Stmt::ReturnType(ty))) = (&out, body.first()) {
            if !self.type_matches(v, ty, &fn_env)? {
                return Err(format!(
                    "type mismatch: function '{}' must return {}, got {}",
                    name,
                    ty,
                    describe(v)
                ));
            }
        }
        self.cur_line.set(saved_line);
        out
    }

    /// `let x: T = v` — the value must match the annotation.
    fn check_var(
        &self,
        name: &str,
        v: &Value,
        ty: &TypeExpr,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<(), String> {
        if self.type_matches(v, ty, env)? {
            return Ok(());
        }
        Err(format!(
            "type mismatch: variable '{}' is declared as {}, got {}",
            name,
            ty,
            describe(v)
        ))
    }

    /// Does `v` match the annotation `ty`? Struct and interface names are
    /// looked up in `env`; generic arguments are not checked (spec §27).
    fn type_matches(
        &self,
        v: &Value,
        ty: &TypeExpr,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<bool, String> {
        Ok(match ty {
            TypeExpr::Any => true,
            TypeExpr::Named(n) | TypeExpr::Generic(n, _) => self.named_type_matches(v, n, env)?,
            TypeExpr::Optional(t) => matches!(v, Value::Nil) || self.type_matches(v, t, env)?,
            TypeExpr::Union(ts) => {
                for t in ts {
                    if self.type_matches(v, t, env)? {
                        return Ok(true);
                    }
                }
                false
            }
            TypeExpr::Tuple(ts) => match v {
                Value::List(l) => {
                    let items = l.borrow().clone();
                    if items.len() != ts.len() {
                        return Ok(false);
                    }
                    // `return nil, err`: with an error, the other values are free
                    if matches!(items.last(), Some(Value::Error(_)))
                        && ts.last().is_some_and(TypeExpr::has_error)
                    {
                        return Ok(true);
                    }
                    for (item, t) in items.iter().zip(ts) {
                        if !self.type_matches(item, t, env)? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
        })
    }

    fn named_type_matches(
        &self,
        v: &Value,
        name: &str,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<bool, String> {
        Ok(match name {
            "int" => matches!(v, Value::Int(_)),
            // int → float promotion (spec §10)
            "float" => matches!(v, Value::Float(_) | Value::Int(_)),
            "str" | "string" => matches!(v, Value::Str(_)),
            "bool" => matches!(v, Value::Bool(_)),
            "nil" | "void" => matches!(v, Value::Nil),
            "duration" => matches!(v, Value::Duration(_)),
            "list" | "tuple" => matches!(v, Value::List(_)),
            "buffer" => matches!(v, Value::Buffer(_)),
            "map" => matches!(v, Value::Map(_)),
            "range" => matches!(v, Value::Range(..)),
            "chan" => matches!(v, Value::Chan(_)),
            // like Go, `error` also accepts nil (no error)
            "error" => matches!(v, Value::Error(_) | Value::Nil),
            "fn" => matches!(
                v,
                Value::Fn(_)
                    | Value::NativeFn(_)
                    | Value::BoundMethod(_)
                    | Value::StructBoundMethod(_)
            ),
            "struct" => matches!(v, Value::Struct(_)),
            "object" => matches!(v, Value::Object(_)),
            other => match env.borrow().get(other) {
                Some(Value::StructType(t)) => match v {
                    Value::Struct(s) => s.borrow().type_name == t.decl.name,
                    _ => false,
                },
                Some(Value::Interface(i)) => satisfies(v, &i),
                Some(_) => return Err(format!("'{}' is not a type", other)),
                None => return Err(format!("unknown type '{}'", other)),
            },
        })
    }

    /// Semantics of `expr?` (spec §30).
    fn propagate(&self, v: Value) -> Result<Value, String> {
        let (value, err) = match &v {
            Value::Error(_) => (None, Some(v.clone())),
            Value::List(l) => {
                let b = l.borrow();
                if b.len() == 2 && matches!(b[1], Value::Error(_) | Value::Nil) {
                    match &b[1] {
                        Value::Error(_) => (None, Some(b[1].clone())),
                        _ => (Some(b[0].clone()), None),
                    }
                } else {
                    (Some(v.clone()), None)
                }
            }
            _ => (Some(v.clone()), None),
        };
        if let Some(Value::Error(e)) = err {
            let mut trace = e.trace.clone();
            trace.push(format!("{}:{}", self.cur_file.borrow(), self.cur_line.get()));
            let ne = Value::Error(Rc::new(ErrorValue {
                message: e.message.clone(),
                cause: e.cause.clone(),
                trace,
            }));
            *self.pending.borrow_mut() =
                Some(Value::List(Rc::new(RefCell::new(vec![Value::Nil, ne]))));
            return Err(PROPAGATE.into());
        }
        Ok(value.unwrap_or(Value::Nil))
    }

    // ---------- structs ----------

    /// Instantiates a struct. Methods and default values resolve names in
    /// the environment where the struct was *declared*.
    fn instantiate_struct(
        &self,
        tv: &StructTypeValue,
        args: Vec<Value>,
        embedded: bool,
    ) -> Result<Rc<RefCell<StructInstance>>, String> {
        let decl = &tv.decl;
        let denv = &tv.env;
        let mut fields: HashMap<String, Value> = HashMap::new();
        let mut methods: HashMap<String, Rc<FnValue>> = HashMap::new();

        // 1) embeds (fields and methods of the embed become visible outside)
        for embed_name in &decl.embeds {
            let t = denv
                .borrow()
                .get(embed_name)
                .ok_or_else(|| format!("embed '{}' is not defined", embed_name))?;
            let inner_tv = match t {
                Value::StructType(d) => d,
                other => {
                    return Err(format!(
                        "'{}' is not a struct type (it is '{}')",
                        embed_name,
                        other.type_name()
                    ))
                }
            };
            let inner = self.instantiate_struct(&inner_tv, Vec::new(), true)?;
            {
                let ib = inner.borrow();
                for (k, v) in ib.fields.iter() {
                    fields.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (m, f) in ib.methods.iter() {
                    methods.entry(m.clone()).or_insert_with(|| Rc::clone(f));
                }
            }
            fields.insert(embed_name.clone(), Value::Struct(inner));
        }

        // 2) own fields
        for f in &decl.fields {
            let v = match &f.default {
                Some(e) => self.eval(e, denv.clone())?,
                None => Value::Nil,
            };
            fields.insert(f.name.clone(), v);
        }

        // 3) own methods
        for (mname, params, body) in &decl.methods {
            methods.insert(
                mname.clone(),
                Rc::new(FnValue {
                    name: format!("{}.{}", decl.name, mname),
                    params: params.clone(),
                    body: Rc::clone(body),
                    closure: Some(Rc::clone(denv)),
                    module: denv.borrow().module_name(),
                }),
            );
        }

        let inst = Rc::new(RefCell::new(StructInstance {
            type_name: decl.name.clone(),
            module: denv.borrow().module_name(),
            fields,
            embeds: decl.embeds.clone(),
            methods,
        }));

        // 4) constructor
        match &decl.constructor {
            Some((params, _)) if embedded && !params.is_empty() => {
                // Embed whose constructor requires arguments: keeps the
                // default values.
            }
            Some((params, body)) => {
                check_arity("constructor of", &decl.name, params, args.len())?;
                let module = denv.borrow().module_name();
                let saved = self.enter(module.as_ref());
                let ctor_env = Environment::with_parent(denv.clone());
                {
                    let mut e = ctor_env.borrow_mut();
                    e.define("this".into(), Value::Struct(Rc::clone(&inst)), false);
                    bind_params(&mut e, params, args);
                }
                for stmt in body.iter() {
                    match self.exec(stmt, ctor_env.clone())? {
                        Flow::Normal => {}
                        Flow::Return(_) => break,
                        Flow::Break => return Err("'break' outside a loop".into()),
                        Flow::Continue => return Err("'continue' outside a loop".into()),
                    }
                }
                self.leave(saved);
            }
            None => {
                if !args.is_empty() {
                    return Err(format!(
                        "struct '{}' has no constructor, but {} arguments were passed",
                        decl.name,
                        args.len()
                    ));
                }
            }
        }
        Ok(inst)
    }

    fn eval_access(&self, target: Value, field: &str) -> Result<Value, String> {
        match target {
            Value::Map(m) => Ok(m.borrow().get(field).cloned().unwrap_or(Value::Nil)),
            Value::Module(m) => {
                if field.starts_with('_') {
                    return Err(format!(
                        "'{}' is private to module '{}' (names starting with '_')",
                        field, m.name
                    ));
                }
                Ok(m.exports.borrow().get(field).cloned().unwrap_or(Value::Nil))
            }
            Value::Object(o) => {
                if let Some(v) = o.get(field) {
                    return Ok(v);
                }
                Ok(Value::bound(o, field))
            }
            Value::Struct(s) => {
                if s.borrow().methods.contains_key(field) {
                    return Ok(Value::struct_bound(Rc::clone(&s), field));
                }
                if let Some(v) = s.borrow().fields.get(field) {
                    return Ok(v.clone());
                }
                Err(format!(
                    "field or method '{}' does not exist on '{}'",
                    field,
                    s.borrow().type_name
                ))
            }
            Value::Error(e) => match field {
                "message" => Ok(Value::Str(e.message.clone().into())),
                "cause" => Ok(e.cause.clone()),
                "trace" => Ok(Value::List(Rc::new(RefCell::new(
                    e.trace.iter().map(|t| Value::Str(t.clone().into())).collect(),
                )))),
                _ => Err(format!("error has no field '{}'", field)),
            },
            Value::Duration(ns) => match field {
                "ns" => Ok(Value::Int(ns)),
                "us" => Ok(Value::Int(ns / 1_000)),
                "ms" => Ok(Value::Int(ns / 1_000_000)),
                "seconds" => Ok(Value::Float(ns as f64 / 1e9)),
                "minutes" => Ok(Value::Float(ns as f64 / 6e10)),
                "hours" => Ok(Value::Float(ns as f64 / 3.6e12)),
                _ => Err(format!(
                    "duration has no field '{}' (use ns, us, ms, seconds, minutes, hours)",
                    field
                )),
            },
            other => Err(format!(
                "access '.{}' not applicable to '{}'",
                field,
                other.type_name()
            )),
        }
    }
}

// ---------- Types ----------

/// Type of a value for error messages (struct name for instances).
fn describe(v: &Value) -> String {
    match v {
        Value::Struct(s) => s.borrow().type_name.clone(),
        other => other.type_name().to_string(),
    }
}

/// Structural interfaces (spec §25): a struct (or a map of functions)
/// satisfies an interface when it has every required method.
fn satisfies(v: &Value, iface: &InterfaceDecl) -> bool {
    match v {
        Value::Struct(s) => {
            let s = s.borrow();
            iface.methods.iter().all(|(m, _)| s.methods.contains_key(m))
        }
        Value::Map(m) => {
            let m = m.borrow();
            iface.methods.iter().all(|(name, _)| {
                matches!(m.get(name), Some(Value::Fn(_) | Value::NativeFn(_)))
            })
        }
        _ => false,
    }
}

// ---------- Parameters ----------

/// Error if more arguments than parameters are passed (except with
/// `...rest`). Missing arguments are `nil` (or their default value).
fn check_arity(what: &str, name: &str, params: &[String], n: usize) -> Result<(), String> {
    if rest_param(params).is_none() && n > params.len() {
        return Err(format!(
            "{} '{}' expects at most {} arguments, got {}",
            what,
            name,
            params.len(),
            n
        ));
    }
    Ok(())
}

/// Binds the arguments to the parameters; `...rest` receives the list of the rest.
fn bind_params(e: &mut Environment, params: &[String], mut args: Vec<Value>) {
    let rest = rest_param(params);
    let fixed = if rest.is_some() { params.len() - 1 } else { params.len() };
    let extra = if args.len() > fixed { args.split_off(fixed) } else { Vec::new() };
    for (i, p) in params[..fixed].iter().enumerate() {
        let v = args.get(i).cloned().unwrap_or(Value::Nil);
        e.define(p.clone(), v, true);
    }
    if let Some(r) = rest {
        e.define(r.to_string(), Value::List(Rc::new(RefCell::new(extra))), true);
    }
}

// ---------- Channels ----------

pub enum RecvError {
    Closed,
    Other(String),
}

/// Pushes without waiting (used when it is known there is room).
fn chan_push(ch: &Rc<ChanValue>, v: Value) -> Result<(), String> {
    if ch.closed.get() {
        return Err("send on a closed channel".into());
    }
    ch.queue.borrow_mut().push_back(v);
    ch.sent.set(ch.sent.get() + 1);
    sched::note_progress();
    Ok(())
}

/// Blocking (cooperative) send.
pub fn chan_send(ch: &Rc<ChanValue>, v: Value) -> Result<(), String> {
    // WebAssembly: a task cannot be suspended, so sends never wait (see sched_wasm.rs)
    if cfg!(target_family = "wasm") {
        return chan_push(ch, v);
    }
    sched::wait_until(|| ch.closed.get() || ch.queue.borrow().len() < ch.capacity())?;
    chan_push(ch, v)?;
    if ch.buffer == 0 {
        // Rendezvous: wait for the receiver to take the value.
        let my_seq = ch.sent.get();
        sched::wait_until(|| ch.recvd.get() >= my_seq || ch.closed.get())?;
    }
    Ok(())
}

/// Blocking (cooperative) receive.
pub fn chan_recv(ch: &Rc<ChanValue>) -> Result<Value, RecvError> {
    sched::wait_until(|| {
        ch.poll_timer();
        !ch.queue.borrow().is_empty() || ch.closed.get()
    })
    .map_err(RecvError::Other)?;
    let v = ch.queue.borrow_mut().pop_front();
    match v {
        Some(v) => {
            ch.recvd.set(ch.recvd.get() + 1);
            sched::note_progress();
            Ok(v)
        }
        None => Err(RecvError::Closed),
    }
}

// ---------- Indexing ----------

#[cold]
#[inline(never)]
fn undefined(name: &str) -> String {
    format!("variable '{}' is not defined", name)
}

fn norm_index(i: i64, len: usize) -> Result<usize, String> {
    let len_i = len as i64;
    let idx = if i < 0 { i + len_i } else { i };
    if idx < 0 || idx >= len_i {
        return Err(format!("index {} out of bounds (length {})", i, len));
    }
    Ok(idx as usize)
}

fn eval_index(target: Value, index: Value) -> Result<Value, String> {
    match (target, index) {
        (Value::List(l), Value::Int(i)) => {
            let v = l.borrow();
            let idx = norm_index(i, v.len())?;
            Ok(v[idx].clone())
        }
        (Value::Map(m), Value::Str(k)) => Ok(m.borrow().get(k.as_str()).cloned().unwrap_or(Value::Nil)),
        (Value::Module(m), Value::Str(k)) => {
            Ok(m.exports.borrow().get(k.as_str()).cloned().unwrap_or(Value::Nil))
        }
        (Value::Buffer(b), Value::Int(i)) => {
            let b = b.borrow();
            let idx = norm_index(i, b.len())?;
            Ok(Value::Int(b[idx] as i64))
        }
        // O(1) on ASCII text (the char count is cached in the `Str`)
        (Value::Str(s), Value::Int(i)) => {
            let idx = norm_index(i, s.char_len())?;
            Ok(Value::Str(s.char_at(idx).unwrap_or_default().into()))
        }
        (t, i) => Err(format!(
            "cannot index '{}' with '{}'",
            t.type_name(),
            i.type_name()
        )),
    }
}

fn assign_index(target: &Value, index: Value, v: Value) -> Result<(), String> {
    match (target, index) {
        (Value::List(l), Value::Int(i)) => {
            let mut b = l.borrow_mut();
            let idx = norm_index(i, b.len())?;
            b[idx] = v;
            Ok(())
        }
        (Value::Map(m), Value::Str(k)) => {
            m.borrow_mut().insert(k.into_string(), v);
            Ok(())
        }
        (Value::Buffer(b), Value::Int(i)) => {
            let byte = match v {
                Value::Int(n) if (0..=255).contains(&n) => n as u8,
                other => return Err(format!("a buffer holds bytes (ints 0..255), got {}", other.to_display_string())),
            };
            let mut b = b.borrow_mut();
            let idx = norm_index(i, b.len())?;
            b[idx] = byte;
            Ok(())
        }
        (t, i) => Err(format!(
            "cannot assign to index '{}' of '{}'",
            i.type_name(),
            t.type_name()
        )),
    }
}

// ---------- Arithmetic and comparison ----------

fn eval_binary(l: Value, op: BinaryOp, r: Value) -> Result<Value, String> {
    use BinaryOp::*;

    match op {
        Eq => return Ok(Value::Bool(values_equal(&l, &r))),
        Ne => return Ok(Value::Bool(!values_equal(&l, &r))),
        Lt | Le | Gt | Ge => return compare_values(l, op, r),
        _ => {}
    }

    match (l, r) {
        (Value::Int(a), Value::Int(b)) => {
            let res = match op {
                Add => a.checked_add(b).ok_or(ops::CODE_OVERFLOW),
                Sub => a.checked_sub(b).ok_or(ops::CODE_OVERFLOW),
                Mul => a.checked_mul(b).ok_or(ops::CODE_OVERFLOW),
                Div => ops::int_div(a, b),
                Rem => ops::int_rem(a, b),
                Pow => ops::int_pow(a, b),
                _ => unreachable!(),
            };
            res.map(Value::Int)
                .map_err(|c| ops::code_message(c).to_string())
        }

        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(match op {
            Add => a + b,
            Sub => a - b,
            Mul => a * b,
            Div => a / b,
            Rem => a % b,
            Pow => a.powf(b),
            _ => unreachable!(),
        })),

        (Value::Int(a), Value::Float(b)) => eval_binary(Value::Float(a as f64), op, Value::Float(b)),
        (Value::Float(a), Value::Int(b)) => eval_binary(Value::Float(a), op, Value::Float(b as f64)),

        (Value::Str(a), Value::Str(b)) => match op {
            Add => Ok(Value::Str(a + &b)),
            _ => Err("operator not applicable to strings".into()),
        },

        (Value::Duration(a), Value::Duration(b)) => match op {
            Add => a.checked_add(b).map(Value::Duration).ok_or_else(|| ops::ERR_OVERFLOW.into()),
            Sub => a.checked_sub(b).map(Value::Duration).ok_or_else(|| ops::ERR_OVERFLOW.into()),
            Rem => ops::int_rem(a, b)
                .map(Value::Duration)
                .map_err(|c| ops::code_message(c).to_string()),
            // duration / duration = how many times it fits (float)
            Div if b == 0 => Err(ops::ERR_DIV_ZERO.into()),
            Div => Ok(Value::Float(a as f64 / b as f64)),
            _ => Err("operator not applicable between 'duration' and 'duration'".into()),
        },
        (Value::Duration(d), Value::Int(n)) => match op {
            Mul => d.checked_mul(n).map(Value::Duration).ok_or_else(|| ops::ERR_OVERFLOW.into()),
            Div => ops::int_div(d, n)
                .map(Value::Duration)
                .map_err(|c| ops::code_message(c).to_string()),
            _ => Err("operator not applicable between 'duration' and 'int'".into()),
        },
        (Value::Int(n), Value::Duration(d)) if op == Mul => {
            d.checked_mul(n).map(Value::Duration).ok_or_else(|| ops::ERR_OVERFLOW.into())
        }
        (Value::Duration(d), Value::Float(f)) => match op {
            Mul => scale_duration(d, f),
            Div if f == 0.0 => Err(ops::ERR_DIV_ZERO.into()),
            Div => scale_duration(d, 1.0 / f),
            _ => Err("operator not applicable between 'duration' and 'float'".into()),
        },
        (Value::Float(f), Value::Duration(d)) if op == Mul => scale_duration(d, f),

        (Value::List(a), Value::List(b)) => match op {
            Add => {
                let mut result = a.borrow().clone();
                result.extend(b.borrow().iter().cloned());
                Ok(Value::List(Rc::new(RefCell::new(result))))
            }
            _ => Err("operator not applicable to lists".into()),
        },

        (a, b) => Err(format!(
            "operator not applicable between '{}' and '{}'",
            a.type_name(),
            b.type_name()
        )),
    }
}

fn scale_duration(d: i64, f: f64) -> Result<Value, String> {
    let v = (d as f64 * f).round();
    if !v.is_finite() || v.abs() >= i64::MAX as f64 {
        return Err(ops::ERR_OVERFLOW.into());
    }
    Ok(Value::Duration(v as i64))
}

fn compare_values(l: Value, op: BinaryOp, r: Value) -> Result<Value, String> {
    use BinaryOp::*;
    use std::cmp::Ordering;

    let ordering: Option<Ordering> = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
        (Value::Duration(a), Value::Duration(b)) => Some(a.cmp(b)),
        _ => {
            return Err(format!(
                "cannot compare '{}' with '{}'",
                l.type_name(),
                r.type_name()
            ));
        }
    };

    // With NaN, all ordered comparisons are false (IEEE 754).
    let Some(ord) = ordering else {
        return Ok(Value::Bool(false));
    };

    Ok(Value::Bool(match op {
        Lt => ord == Ordering::Less,
        Le => ord != Ordering::Greater,
        Gt => ord == Ordering::Greater,
        Ge => ord != Ordering::Less,
        _ => unreachable!(),
    }))
}

/// `==` semantics (structural for lists and maps, identity for the rest).
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
        (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Duration(x), Value::Duration(y)) => x == y,
        (Value::Range(a1, b1), Value::Range(a2, b2)) => a1 == a2 && b1 == b2,
        (Value::List(x), Value::List(y)) => {
            if Rc::ptr_eq(x, y) {
                return true;
            }
            let xb = x.borrow();
            let yb = y.borrow();
            xb.len() == yb.len() && xb.iter().zip(yb.iter()).all(|(a, b)| values_equal(a, b))
        }
        (Value::Buffer(x), Value::Buffer(y)) => Rc::ptr_eq(x, y) || *x.borrow() == *y.borrow(),
        (Value::Map(x), Value::Map(y)) => {
            if Rc::ptr_eq(x, y) {
                return true;
            }
            let xb = x.borrow();
            let yb = y.borrow();
            xb.len() == yb.len()
                && xb
                    .iter()
                    .all(|(k, v)| yb.get(k).map_or(false, |other| values_equal(v, other)))
        }
        (Value::Struct(x), Value::Struct(y)) => Rc::ptr_eq(x, y),
        (Value::Error(x), Value::Error(y)) => Rc::ptr_eq(x, y),
        (Value::Chan(x), Value::Chan(y)) => Rc::ptr_eq(x, y),
        (Value::Fn(x), Value::Fn(y)) => Rc::ptr_eq(x, y),
        (Value::NativeFn(x), Value::NativeFn(y)) => Rc::ptr_eq(x, y),
        (Value::Module(x), Value::Module(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// The block scope of a loop body, reused across iterations: allocating two
/// environments per iteration was most of the cost of a simple loop.
#[derive(Default)]
struct LoopScope {
    block: Option<Rc<RefCell<Environment>>>,
}

impl LoopScope {
    /// Did something keep the previous iteration's scopes (a closure or a
    /// task that captured the loop variable or a local of the body)? Then the
    /// next iteration needs fresh ones. `iter_env` (the loop variables) is held
    /// by the loop itself and, as their parent, by the reused body block — so
    /// it is free when its count is exactly that.
    fn captured(&self, iter_env: &Rc<RefCell<Environment>>) -> bool {
        let expected = match &self.block {
            Some(b) if Rc::strong_count(b) > 1 => return true,
            Some(_) => 2,
            None => 1,
        };
        Rc::strong_count(iter_env) > expected
    }

    /// Prepares `slot` (a child of `parent`) for a new iteration: emptied if
    /// nothing else holds it, else replaced (a closure or task captured the
    /// previous iteration's bindings). Returns true when it was replaced.
    fn recycle(slot: &mut Rc<RefCell<Environment>>, parent: &Rc<RefCell<Environment>>) -> bool {
        if Rc::strong_count(slot) == 1 {
            slot.borrow_mut().reset();
            false
        } else {
            *slot = Environment::with_parent(Rc::clone(parent));
            true
        }
    }

    /// Runs one iteration of `body` in `env`; a block body gets the reused scope.
    fn run(&mut self, it: &Interpreter, body: &Stmt, env: &Rc<RefCell<Environment>>) -> Result<Flow, String> {
        let Stmt::Block(stmts) = body else {
            return it.exec(body, Rc::clone(env));
        };
        let block = match &mut self.block {
            Some(b) => {
                LoopScope::recycle(b, env);
                Rc::clone(b)
            }
            None => {
                let b = Environment::with_parent(Rc::clone(env));
                self.block = Some(Rc::clone(&b));
                b
            }
        };
        for st in stmts {
            match it.exec(st, Rc::clone(&block))? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }
}
