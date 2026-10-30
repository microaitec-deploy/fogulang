use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

pub struct NativeContext {
    globals: HashMap<String, Value>,
    /// Native modules, built the first time they are imported (startup stays fast).
    modules: RefCell<HashMap<String, Slot>>,
}

enum Slot {
    Built(Rc<ModuleValue>),
    Lazy(fn() -> ModuleValue),
}

impl NativeContext {
    pub fn new() -> Self {
        let mut ctx = Self {
            globals: HashMap::new(),
            modules: RefCell::new(HashMap::new()),
        };
        ctx.install_core();
        if let Some(host) = crate::host::get() {
            host.install(&mut ctx);
        }
        ctx
    }

    pub fn define_global(
        &mut self,
        name: &str,
        arity: usize,
        f: fn(&[Value]) -> Result<Value, String>,
    ) {
        self.globals.insert(
            name.to_string(),
            Value::NativeFn(Rc::new(NativeFnValue {
                name: name.to_string(),
                arity,
                variadic: false,
                func: f,
                intercept: Default::default(),
            })),
        );
    }

    pub fn define_global_variadic(
        &mut self,
        name: &str,
        min_arity: usize,
        f: fn(&[Value]) -> Result<Value, String>,
    ) {
        self.globals.insert(
            name.to_string(),
            Value::NativeFn(Rc::new(NativeFnValue {
                name: name.to_string(),
                arity: min_arity,
                variadic: true,
                func: f,
                intercept: Default::default(),
            })),
        );
    }

    pub fn define_module(&mut self, name: &str, module: ModuleValue) {
        self.modules.get_mut().insert(name.to_string(), Slot::Built(Rc::new(module)));
    }

    /// Defines a global value (e.g. the `Fogu` object).
    pub fn define_value(&mut self, name: &str, value: Value) {
        self.globals.insert(name.to_string(), value);
    }

    /// Registers a module that is built on its first import.
    pub fn define_lazy(&mut self, name: &str, build: fn() -> ModuleValue) {
        self.modules.get_mut().insert(name.to_string(), Slot::Lazy(build));
    }

    pub fn globals(&self) -> Vec<(String, Value)> {
        self.globals
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn get_module(&self, name: &str) -> Option<Rc<ModuleValue>> {
        let build = match self.modules.borrow().get(name)? {
            Slot::Built(m) => return Some(Rc::clone(m)),
            Slot::Lazy(f) => *f,
        };
        let m = Rc::new(build());
        self.modules.borrow_mut().insert(name.to_string(), Slot::Built(Rc::clone(&m)));
        Some(m)
    }

    /// The language globals and the core modules; the host adds the rest.
    fn install_core(&mut self) {
        self.define_lazy("math", crate::native::math::build);
        self.define_lazy("json", crate::native::json::build);
        self.define_lazy("events", crate::native::events::build);
        self.define_lazy("test", crate::native::test::build);

        self.define_global_variadic("print", 0, |args| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| v.to_display_string())
                .collect();
            println!("{}", parts.join(" "));
            Ok(Value::Nil)
        });

        self.define_global("len", 1, |args| match &args[0] {
            Value::Str(s) => Ok(Value::Int(s.char_len() as i64)),
            Value::List(l) => Ok(Value::Int(l.borrow().len() as i64)),
            Value::Buffer(b) => Ok(Value::Int(b.borrow().len() as i64)),
            Value::Map(m) => Ok(Value::Int(m.borrow().len() as i64)),
            Value::Chan(c) => Ok(Value::Int(c.queue.borrow().len() as i64)),
            other => Err(format!("'len' not applicable to '{}'", other.type_name())),
        });

        // close(ch): closes a channel; receives drain the buffer and then
        // end; sends start failing.
        self.define_global("close", 1, |args| match &args[0] {
            Value::Chan(c) => {
                c.closed.set(true);
                crate::runtime::sched::note_progress();
                Ok(Value::Nil)
            }
            other => Err(format!("'close' expects a channel, got '{}'", other.type_name())),
        });

        // timeout(d): channel that receives `nil` after `d` (spec §36).
        self.define_global("timeout", 1, |args| {
            let ns = match &args[0] {
                Value::Duration(ns) if *ns >= 0 => *ns as u64,
                Value::Int(ms) if *ms >= 0 => *ms as u64 * 1_000_000,
                other => {
                    return Err(format!(
                        "timeout expects a duration (e.g. 500ms) or int ms >= 0, got '{}'",
                        other.type_name()
                    ))
                }
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_nanos(ns);
            Ok(Value::Chan(Rc::new(crate::runtime::value::ChanValue::timeout(deadline))))
        });

        // bytes(str | size | [byte] | buffer, encoding = "utf8") → buffer
        self.define_global_variadic("bytes", 0, crate::native::buffer::bytes);

        self.define_global("typeof", 1, |args| {
            Ok(Value::Str(args[0].type_name().to_string().into()))
        });

        self.define_global("str", 1, |args| {
            Ok(Value::Str(args[0].to_display_string().into()))
        });

        self.define_global("int", 1, |args| match &args[0] {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::Float(f) => Ok(Value::Int(*f as i64)),
            Value::Str(s) => s
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| format!("cannot convert '{}' to int", s)),
            Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
            other => Err(format!(
                "cannot convert '{}' to int",
                other.type_name()
            )),
        });

        self.define_global("float", 1, |args| match &args[0] {
            Value::Float(f) => Ok(Value::Float(*f)),
            Value::Int(n) => Ok(Value::Float(*n as f64)),
            Value::Str(s) => s
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| format!("cannot convert '{}' to float", s)),
            other => Err(format!(
                "cannot convert '{}' to float",
                other.type_name()
            )),
        });

        crate::native::collections::install(self);

        self.define_global("clock", 0, |_| {
            let d = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs_f64();
            Ok(Value::Float(d))
        });
    }
}

impl Default for NativeContext {
    fn default() -> Self {
        Self::new()
    }
}