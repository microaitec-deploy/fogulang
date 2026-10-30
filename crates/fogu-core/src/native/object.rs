use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use crate::native::events::EventEmitterApi;
use crate::runtime::value::Value;

/// Trait that every native object must implement.
///
/// `name` — type name, used by `typeof` and for debugging.
/// `get` — reads a field (returns None if it does not exist).
/// `set` — writes a field. Returns Err if not applicable.
/// `call` — calls a method. `None` means "not a method".
pub trait FoguObject {
    fn name(&self) -> &str;

    /// Field access, e.g. `obj.field`.
    /// Returns `None` to signal "unknown field".
    fn get(&self, _field: &str) -> Option<Value> {
        None
    }

    /// Field write, e.g. `obj.field = v`.
    /// Refused by default.
    fn set(&mut self, _field: &str, _value: Value) -> Result<(), String> {
        Err("object does not support writing fields".into())
    }

    /// Method call, e.g. `obj.method(args)`.
    /// Returns `Ok(Value)` if the method exists; `Err` if it does not or fails.
    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on {}", method, self.name()))
    }

    /// Condition a blocking method must wait for *before* it is called
    /// (e.g. `Mutex.lock` waits until the mutex is free). The interpreter
    /// waits cooperatively — yielding to other `fg` tasks — without holding a
    /// borrow of the object, so another task may change it in the meantime.
    /// The closure must only capture shared state (`Rc<Cell<…>>`).
    fn wait_for(&self, _method: &str) -> Option<Box<dyn Fn() -> bool>> {
        None
    }

    /// Asynchronous method (e.g. a network request): starts the operation and
    /// returns a poller, or `None` when `method` is an ordinary method. The
    /// interpreter calls the poller — without holding a borrow of the object,
    /// letting the other `fg` tasks run in between — until it returns the
    /// result. A poller that is not ready should register a short timer
    /// (`sched::add_timer`) so the scheduler keeps polling.
    fn start_async(&mut self, _method: &str, _args: &[Value]) -> Option<Result<Poller, String>> {
        None
    }

    /// Known methods (used for diagnostics).
    fn methods(&self) -> Vec<&'static str> {
        Vec::new()
    }

     fn as_any(&self) -> &dyn std::any::Any {
        // Default: not downcastable. Concrete implementations may
        // override it.
        unimplemented!("as_any not implemented — implement Any for the type")
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        unimplemented!()
    }
}

/// Result of an asynchronous method, polled until it is `Some`.
pub type Poller = Box<dyn FnMut() -> Option<Result<Value, String>>>;

/// `Rc<RefCell<dyn FoguObject>>` wrapper with helpers.
#[derive(Clone)]
pub struct NativeObjectRef {
    inner: Rc<RefCell<dyn FoguObject>>,
}

impl NativeObjectRef {
    pub fn new<O: FoguObject + 'static>(obj: O) -> Self {
        Self {
            inner: Rc::new(RefCell::new(obj)),
        }
    }

    pub fn from_rc(inner: Rc<RefCell<dyn FoguObject>>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> Rc<RefCell<dyn FoguObject>> {
        Rc::clone(&self.inner)
    }

    pub fn get(&self, field: &str) -> Option<Value> {
        self.inner.borrow().get(field)
    }

    pub fn set(&self, field: &str, v: Value) -> Result<(), String> {
        self.inner.borrow_mut().set(field, v)
    }

    pub fn call(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        self.inner.borrow_mut().call(method, args)
    }

    pub fn name(&self) -> String {
        self.inner.borrow().name().to_string()
    }

    /// See [`FoguObject::start_async`]; the borrow ends before returning.
    pub fn start_async(&self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        self.inner.borrow_mut().start_async(method, args)
    }

    /// See [`FoguObject::wait_for`]; the borrow ends before returning.
    pub fn wait_for(&self, method: &str) -> Option<Box<dyn Fn() -> bool>> {
        self.inner.borrow().wait_for(method)
    }

    /// Builds a `Value::Object` from this wrapper.
    pub fn to_value(&self) -> Value {
        Value::Object(self.clone())
    }

    /// Helper map for debugging.
    pub fn debug_map(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("type".into(), self.name());
        m
    }

        /// Runs a function with `&EventEmitterValue` (downcast).
    pub fn with_emitter<R>(&self, f: impl FnOnce(&dyn EventEmitterApi) -> R)
        -> Result<R, String>
    {
        // We need to access `inner` — but it is a `dyn FoguObject`.
        // Downcasting requires `Any`. Simplification: `FoguObject` has an
        // `as_any` method that returns
        // `&dyn Any`.
        let inner = self.inner.borrow();
        let any = inner.as_any();
        match any.downcast_ref::<crate::native::events::EventEmitterValue>() {
            Some(e) => Ok(f(e)),
            None => Err("object is not an EventEmitter".into()),
        }
    }

    pub fn with_emitter_mut<R>(&self, f: impl FnOnce(&mut dyn EventEmitterApi) -> R)
        -> Result<R, String>
    {
        let mut inner = self.inner.borrow_mut();
        let any = inner.as_any_mut();
        match any.downcast_mut::<crate::native::events::EventEmitterValue>() {
            Some(e) => Ok(f(e)),
            None => Err("object is not an EventEmitter".into()),
        }
    }
}

impl std::fmt::Debug for NativeObjectRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<native object {}>", self.name())
    }
}