use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    exports.insert(
        "EventEmitter".into(),
        nfn("EventEmitter", 0, |_args| {
            Ok(NativeObjectRef::new(EventEmitterValue::new()).to_value())
        }),
    );

    ModuleValue {
        name: "fogu:events".into(),
        exports: RefCell::new(exports),
    }
}

// ---------- Public API for the interpreter ----------

pub trait EventEmitterApi {
    fn add_listener(&mut self, event: &str, func: Value, once: bool) -> Result<(), String>;
    /// The listeners of `event`: (function, id, registered with `once`).
    fn get_listeners(&self, event: &str) -> Vec<(Value, u64, bool)>;
    fn remove_listener_by_id(&mut self, event: &str, id: u64);
    fn remove_all_listeners(&mut self, event: &str);
    fn clear_all(&mut self);
    fn event_names(&self) -> Vec<String>;
    fn count(&self, event: &str) -> i64;
}

#[derive(Default)]
pub struct EventEmitterValue {
    listeners: HashMap<String, Vec<Listener>>,
    next_id: u64,
}

struct Listener {
    func: Value,
    once: bool,
    id: u64,
}

impl EventEmitterValue {
    pub fn new() -> Self {
        Self {
            listeners: HashMap::new(),
            next_id: 1,
        }
    }
}

impl EventEmitterApi for EventEmitterValue {
    fn add_listener(&mut self, event: &str, func: Value, once: bool) -> Result<(), String> {
        match &func {
            Value::Fn(_)
            | Value::NativeFn(_)
            | Value::BoundMethod(_)
            | Value::StructBoundMethod(_) => {}
            other => {
                return Err(format!(
                    "listener must be a function, got '{}'",
                    other.type_name()
                ))
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.listeners
            .entry(event.to_string())
            .or_default()
            .push(Listener { func, once, id });
        Ok(())
    }

    fn get_listeners(&self, event: &str) -> Vec<(Value, u64, bool)> {
        self.listeners
            .get(event)
            .map(|l| l.iter().map(|x| (x.func.clone(), x.id, x.once)).collect())
            .unwrap_or_default()
    }

    fn remove_listener_by_id(&mut self, event: &str, id: u64) {
        if let Some(list) = self.listeners.get_mut(event) {
            list.retain(|l| l.id != id);
            if list.is_empty() {
                self.listeners.remove(event);
            }
        }
    }

    fn remove_all_listeners(&mut self, event: &str) {
        self.listeners.remove(event);
    }

    fn clear_all(&mut self) {
        self.listeners.clear();
    }

    fn event_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.listeners.keys().cloned().collect();
        v.sort();
        v
    }

    fn count(&self, event: &str) -> i64 {
        self.listeners.get(event).map(|l| l.len() as i64).unwrap_or(0)
    }
}

impl FoguObject for EventEmitterValue {
    fn name(&self) -> &str {
        "EventEmitter"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "on" => {
                if args.len() != 2 {
                    return Err("on(event, fn) expects 2 arguments".into());
                }
                let event = str_arg(&args[0], "on")?;
                self.add_listener(&event, args[1].clone(), false)?;
                Ok(Value::Nil)
            }
            "once" => {
                if args.len() != 2 {
                    return Err("once(event, fn) expects 2 arguments".into());
                }
                let event = str_arg(&args[0], "once")?;
                self.add_listener(&event, args[1].clone(), true)?;
                Ok(Value::Nil)
            }
            "off" | "removeListener" => {
                if args.is_empty() {
                    return Err("off(event) expects at least 1 argument".into());
                }
                let event = str_arg(&args[0], "off")?;
                self.remove_all_listeners(&event);
                Ok(Value::Nil)
            }
            "removeAll" | "clear" => {
                self.clear_all();
                Ok(Value::Nil)
            }
            "listeners" => {
                if args.len() != 1 {
                    return Err("listeners(event) expects 1 argument".into());
                }
                let event = str_arg(&args[0], "listeners")?;
                Ok(Value::Int(self.count(&event)))
            }
            "eventNames" => {
                let names = self.event_names();
                let vals: Vec<Value> = names.into_iter().map(Value::str).collect();
                Ok(Value::List(Rc::new(RefCell::new(vals))))
            }
            "hasListeners" => {
                if args.len() != 1 {
                    return Err("hasListeners(event) expects 1 argument".into());
                }
                let event = str_arg(&args[0], "hasListeners")?;
                Ok(Value::Bool(self.count(&event) > 0))
            }
            // `emit` **is handled in the interpreter** so that it can call
            // Fogu functions. Here we return an explicit error so we are
            // never called by accident.
            "emit" => Err(
                "emit must be handled by the interpreter — please report a bug".into()
            ),
            other => Err(format!("method '{}' does not exist on EventEmitter", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "on", "once", "off", "removeListener", "removeAll",
            "listeners", "eventNames", "hasListeners", "emit",
        ]
    }


     fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn str_arg(v: &Value, ctx: &str) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(format!(
            "{}: expected str, got '{}'",
            ctx,
            other.type_name()
        )),
    }
}