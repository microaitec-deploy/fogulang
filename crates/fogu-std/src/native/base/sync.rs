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
