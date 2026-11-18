use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::fmt;
use std::rc::Rc;
use std::time::Instant;

use super::ast::{Block, InterfaceDecl, StructDecl};
use super::environment::Environment;
pub use super::fstr::Str;
use crate::native::object::NativeObjectRef;

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Str),
    /// Duration in nanoseconds (spec §7.7).
    Duration(i64),
    Range(i64, i64),
    List(Rc<RefCell<Vec<Value>>>),
    /// Mutable byte sequence (`bytes(...)`, network/file data).
    Buffer(Rc<RefCell<Vec<u8>>>),
    Map(Rc<RefCell<HashMap<Str, Value>>>),
    Module(Rc<ModuleValue>),
    Object(NativeObjectRef),
    /// `obj.method` of a native object, not called yet (behind an `Rc` so the
    /// variant stays small: `Value` is 24 bytes).
    BoundMethod(Rc<BoundMethodValue>),
    /// `instance.method` of a struct, not called yet.
    StructBoundMethod(Rc<StructBoundMethodValue>),
    Chan(Rc<ChanValue>),
    Struct(Rc<RefCell<StructInstance>>),
    StructType(Rc<StructTypeValue>),
    /// A declared `interface` (usable in type annotations).
    Interface(Rc<InterfaceDecl>),
    Fn(Rc<FnValue>),
    NativeFn(Rc<NativeFnValue>),
    Error(Rc<ErrorValue>),
}

/// The interned names of a parameter list (for [`FnValue::param_syms`]).
pub fn param_syms(params: &[String]) -> Vec<super::symbol::Sym> {
    params.iter().map(|p| super::symbol::intern(p)).collect()
}

pub struct BoundMethodValue {
    pub receiver: NativeObjectRef,
    pub method: String,
}

pub struct StructBoundMethodValue {
    pub receiver: Rc<RefCell<StructInstance>>,
    pub method: String,
}

impl Value {
    pub fn bound(receiver: NativeObjectRef, method: impl Into<String>) -> Value {
        Value::BoundMethod(Rc::new(BoundMethodValue { receiver, method: method.into() }))
    }

    pub fn struct_bound(receiver: Rc<RefCell<StructInstance>>, method: impl Into<String>) -> Value {
        Value::StructBoundMethod(Rc::new(StructBoundMethodValue { receiver, method: method.into() }))
    }
}

pub struct ModuleValue {
    pub name: String,
    pub exports: RefCell<HashMap<String, Value>>,
}

/// `error` value (spec §29): message, optional cause and trace.
pub struct ErrorValue {
    pub message: String,
    pub cause: Value,
    pub trace: Vec<String>,
}

pub struct FnValue {
    pub name: String,
    pub params: Vec<String>,
    /// The parameters' interned names: a call binds them without hashing text.
    pub param_syms: Vec<super::symbol::Sym>,
    pub body: Block,
    pub closure: Option<Rc<RefCell<Environment>>>,
    /// File (module) where the function was defined — used in errors.
    pub module: Option<Rc<str>>,
}

pub struct NativeFnValue {
    pub name: String,
    pub arity: usize,
    pub variadic: bool,
    pub func: fn(&[Value]) -> Result<Value, String>,
    /// Cache of [`Host::may_intercept_native`](crate::host::Host::may_intercept_native)
    /// for this function: 0 = not asked yet, 1 = never intercepted, 2 = may be.
    pub intercept: std::cell::Cell<u8>,
    /// The typed entry, when the function only takes and returns numbers:
    /// compiled code calls it directly, with no `Value` built (see [`TypedNative`]).
    pub typed: Option<TypedNative>,
}

/// A number as native code passes it: in a register, with no tag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Num {
    /// `int`: an `i64`.
    Int,
    /// `float`: an `f64` (an `int` argument is converted, as `fn(x: float)` does).
    Float,
    /// `bool`: a `u8` (0 or 1).
    Bool,
}

/// The typed entry of a native function: `entry` is an `extern "C" fn` whose
/// parameters are `params` (`i64` / `f64` / `u8`) and whose result is `ret`.
/// It must compute exactly what `func` computes for those argument types.
/// An entry that may fail is `fallible`: it reports the error with
/// `native::util::set_typed_error` (and returns anything) and compiled code
/// checks for it after the call; the others can never fail.
#[derive(Clone, Copy, Debug)]
pub struct TypedNative {
    pub params: &'static [Num],
    pub ret: Num,
    pub entry: usize,
    pub fallible: bool,
}

/// Cooperative channel (single-thread). `buffer == 0` is a rendezvous: the
/// sender only continues after the value has been consumed.
pub struct ChanValue {
    pub queue: RefCell<VecDeque<Value>>,
    pub buffer: usize,
    pub closed: Cell<bool>,
    /// Number of values already sent / received (for the rendezvous).
    pub sent: Cell<u64>,
    pub recvd: Cell<u64>,
    pub name: String,
    /// `timeout(d)` channels: deadline at which they receive a value.
    pub timer: Cell<Option<Instant>>,
}

impl ChanValue {
    pub fn new(buffer: usize) -> Self {
        Self {
            queue: RefCell::new(VecDeque::new()),
            buffer,
            closed: Cell::new(false),
            sent: Cell::new(0),
            recvd: Cell::new(0),
            name: "chan".into(),
            timer: Cell::new(None),
        }
    }

    /// Channel that receives `nil` once the deadline passes (spec §36: `timeout(d)`).
    pub fn timeout(deadline: Instant) -> Self {
        let mut c = Self::new(1);
        c.name = "timeout".into();
        c.timer = Cell::new(Some(deadline));
        super::sched::add_timer(deadline);
        c
    }

    /// If the deadline of a `timeout` channel has passed, delivers the value.
    pub fn poll_timer(&self) {
        if let Some(d) = self.timer.get() {
            if Instant::now() >= d {
                self.timer.set(None);
                self.queue.borrow_mut().push_back(Value::Nil);
                self.sent.set(self.sent.get() + 1);
                super::sched::note_progress();
            }
        }
    }

    /// Effective queue capacity (an unbuffered channel holds 1 item in
    /// transit and then waits for the receiver).
    pub fn capacity(&self) -> usize {
        self.buffer.max(1)
    }
}

/// A declared `struct`, with the environment where it was defined (methods
/// resolve free names there, not where it is instantiated).
pub struct StructTypeValue {
    pub decl: StructDecl,
    pub env: Rc<RefCell<Environment>>,
    /// What its instances share, built by the first `new` (see [`StructLayout`]).
    pub layout: RefCell<Option<Rc<StructLayout>>>,
}

/// What every instance of a struct type shares: the names of its fields (each
/// one a slot of the instance, in this order) and its methods. An instance is
/// then one vector of values; reading `p.x` is an index once the slot of `x`
/// is known, and `new` neither hashes field names nor builds methods.
/// See `docs/architecture/struct-layouts.md`.
pub struct StructLayout {
    /// Unique in the process: what inline caches compare.
    pub id: u64,
    pub type_name: String,
    /// Module where the struct was declared (`_x` fields are only visible there).
    pub module: Option<Rc<str>>,
    pub embeds: Vec<String>,
    /// Field names, in slot order.
    pub names: Vec<String>,
    /// Field name → slot.
    pub index: HashMap<String, u32>,
    pub methods: HashMap<String, Rc<FnValue>>,
    /// The constructor as a method (`this` + its parameters), when there is one.
    pub ctor: Option<Rc<FnValue>>,
    /// The layouts of the embedded types this one was built from.
    pub embed_ids: Vec<u64>,
}

/// A new [`StructLayout::id`].
pub fn next_layout_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A struct value: its type's layout and one value per field.
/// It reads as its layout (`type_name`, `module`, `embeds`, `methods`).
pub struct StructInstance {
    pub layout: Rc<StructLayout>,
    pub values: Vec<Value>,
}

impl std::ops::Deref for StructInstance {
    type Target = StructLayout;
    fn deref(&self) -> &StructLayout {
        &self.layout
    }
}

impl StructInstance {
    /// The value of the field `name`, if the struct has it.
    #[inline]
    pub fn field(&self, name: &str) -> Option<&Value> {
        self.layout.index.get(name).map(|i| &self.values[*i as usize])
    }

    #[inline]
    pub fn field_mut(&mut self, name: &str) -> Option<&mut Value> {
        match self.layout.index.get(name) {
            Some(i) => Some(&mut self.values[*i as usize]),
            None => None,
        }
    }

    /// Every field with its value, in slot order.
    pub fn fields(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.layout.names.iter().zip(self.values.iter())
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_display_string())
    }
}

impl Value {
    /// A buffer value owning `bytes`.
    pub fn buffer(bytes: Vec<u8>) -> Value {
        Value::Buffer(Rc::new(RefCell::new(bytes)))
    }

    /// A str value from anything that converts into `Str` (`String`, `&str`, `char`…).
    pub fn str(s: impl Into<Str>) -> Value {
        Value::Str(s.into())
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Duration(_) => "duration",
            Value::Range(_, _) => "range",
            Value::List(_) => "list",
            Value::Buffer(_) => "buffer",
            Value::Map(_) => "map",
            Value::Module(_) => "module",
            Value::Object(_) => "object",
            Value::BoundMethod(_) => "fn",
            Value::StructBoundMethod(_) => "fn",
            Value::Chan(_) => "chan",
            Value::Struct(_) => "struct",
            Value::StructType(_) => "struct-type",
            Value::Interface(_) => "interface",
            Value::Fn(_) | Value::NativeFn(_) => "fn",
            Value::Error(_) => "error",
        }
    }

    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::Bool(false) | Value::Nil)
    }

    pub fn to_display_string(&self) -> String {
        if let Value::Str(s) = self {
            return s.to_string();
        }
        let mut out = String::new();
        self.write_display(&mut out, &mut WalkPath::default());
        out
    }

    /// Writes the display form of `self` into `out` (one buffer for the whole
    /// value: linear in its size). A list, map or struct met again inside
    /// itself is written `[...]`, `{...}` or `T{...}` (a cycle).
    pub fn write_display(&self, out: &mut String, path: &mut WalkPath) {
        use std::fmt::Write;
        match self {
            Value::Nil => out.push_str("nil"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(n) => {
                let _ = write!(out, "{}", n);
            }
            Value::Float(n) => out.push_str(&display_float(*n)),
            Value::Str(s) => out.push_str(s),
            Value::Duration(ns) => out.push_str(&display_duration(*ns)),
            Value::Range(a, b) => {
                let _ = write!(out, "{}..{}", a, b);
            }
            Value::Buffer(b) => out.push_str(&display_buffer(&b.borrow())),
            Value::List(l) => {
                let p = Rc::as_ptr(l) as usize;
                // a cycle, or nesting deeper than the stack of this task allows
                if !super::sched::stack_ok() || !path.enter(p) {
                    out.push_str("[...]");
                    return;
                }
                out.push('[');
                for (i, v) in l.borrow().iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.write_display(out, path);
                }
                out.push(']');
                path.leave();
            }
            Value::Map(m) => {
                let p = Rc::as_ptr(m) as usize;
                if !super::sched::stack_ok() || !path.enter(p) {
                    out.push_str("{...}");
                    return;
                }
                let b = m.borrow();
                let mut keys: Vec<&Str> = b.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(k);
                    out.push_str(": ");
                    b[k].write_display(out, path);
                }
                out.push('}');
                path.leave();
            }
            Value::Module(m) => {
                let _ = write!(out, "<module {}>", m.name);
            }
            // a native object may describe itself with a `__display__` field
            Value::Object(o) => match o.get("__display__") {
                Some(Value::Str(s)) => out.push_str(&s),
                _ => {
                    let _ = write!(out, "<native object {}>", o.name());
                }
            },
            Value::BoundMethod(b) => {
                let _ = write!(out, "<bound {}::{}>", b.receiver.name(), b.method);
            }
            Value::StructBoundMethod(b) => {
                let _ = write!(out, "<bound {}::{}>", b.receiver.borrow().type_name, b.method);
            }
            Value::Chan(c) => {
                if c.name == "chan" {
                    let _ = write!(out, "<chan buffer={}>", c.buffer);
                } else {
                    let _ = write!(out, "<chan {} buffer={}>", c.name, c.buffer);
                }
            }
            Value::Struct(s) => {
                let b = s.borrow();
                let p = Rc::as_ptr(s) as usize;
                if !super::sched::stack_ok() || !path.enter(p) {
                    let _ = write!(out, "{}{{...}}", b.type_name);
                    return;
                }
                // ordered as the text `name: value` sorts (what it always was)
                let mut fields: Vec<(&String, &Value)> = b.fields().collect();
                fields.sort_by(|x, y| x.0.bytes().chain(Some(b':')).cmp(y.0.bytes().chain(Some(b':'))));
                out.push_str(&b.type_name);
                out.push('{');
                for (i, (k, v)) in fields.into_iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(k);
                    out.push_str(": ");
                    v.write_display(out, path);
                }
                out.push('}');
                path.leave();
            }
            Value::StructType(d) => {
                let _ = write!(out, "<struct {}>", d.decl.name);
            }
            Value::Interface(i) => {
                let _ = write!(out, "<interface {}>", i.name);
            }
            Value::Fn(f) => {
                let _ = write!(out, "<fn {}>", f.name);
            }
            Value::NativeFn(f) => {
                let _ = write!(out, "<native fn {}>", f.name);
            }
            Value::Error(e) => {
                out.push_str("error: ");
                out.push_str(&e.message);
            }
        }
    }

    /// The address of a list, map or struct (what [`WalkPath`] tracks).
    pub fn container_addr(&self) -> Option<usize> {
        match self {
            Value::List(l) => Some(Rc::as_ptr(l) as usize),
            Value::Map(m) => Some(Rc::as_ptr(m) as usize),
            Value::Struct(s) => Some(Rc::as_ptr(s) as usize),
            _ => None,
        }
    }
}

/// The lists, maps and structs on the current path of a recursive walk over a
/// value (display, equality, copies, serialization): meeting one again means
/// the value contains itself. A short path is a vector; a deep one (a very
/// nested value) also gets a set, so the check stays O(1).
#[derive(Default)]
pub struct WalkPath {
    stack: Vec<usize>,
    set: Option<rustc_hash::FxHashSet<usize>>,
}

impl WalkPath {
    const SMALL: usize = 32;

    /// Enters the container at `addr`; false (and nothing entered) when it is
    /// already on the path: a cycle.
    pub fn enter(&mut self, addr: usize) -> bool {
        match &mut self.set {
            Some(s) => {
                if !s.insert(addr) {
                    return false;
                }
            }
            None => {
                if self.stack.contains(&addr) {
                    return false;
                }
            }
        }
        self.stack.push(addr);
        if self.set.is_none() && self.stack.len() > Self::SMALL {
            self.set = Some(self.stack.iter().copied().collect());
        }
        true
    }

    /// Leaves the container entered last.
    pub fn leave(&mut self) {
        if let Some(a) = self.stack.pop() {
            if let Some(s) = &mut self.set {
                s.remove(&a);
            }
        }
    }
}

/// Prefix of *operational* native errors (IO, network, SQL, invalid JSON):
/// they reach the program as an `error` value instead of stopping execution,
/// so they can be handled with `?` or `const [v, err] = ...` (spec §28).
pub const ERR_VALUE_PREFIX: &str = "\u{1}error-value:";

/// Marks a message as an operational error (see [`ERR_VALUE_PREFIX`]).
pub fn err_value(msg: impl Into<String>) -> String {
    format!("{}{}", ERR_VALUE_PREFIX, msg.into())
}

/// `1500ms`, `2s`, `5m`, `1h`: the largest unit that divides the value.
pub fn display_duration(ns: i64) -> String {
    const UNITS: [(i64, &str); 6] = [
        (3_600_000_000_000, "h"),
        (60_000_000_000, "m"),
        (1_000_000_000, "s"),
        (1_000_000, "ms"),
        (1_000, "us"),
        (1, "ns"),
    ];
    if ns == 0 {
        return "0s".into();
    }
    for (mult, unit) in UNITS {
        if ns % mult == 0 {
            return format!("{}{}", ns / mult, unit);
        }
    }
    unreachable!()
}

/// Float formatting shared with the JIT (native `print`).
/// `<buffer 5 bytes: 68 65 6c 6c 6f>` (the first 32 bytes).
pub fn display_buffer(b: &[u8]) -> String {
    let shown: Vec<String> = b.iter().take(32).map(|x| format!("{:02x}", x)).collect();
    let more = if b.len() > 32 { " …" } else { "" };
    let unit = if b.len() == 1 { "byte" } else { "bytes" };
    if b.is_empty() {
        return "<buffer 0 bytes>".into();
    }
    format!("<buffer {} {}: {}{}>", b.len(), unit, shown.join(" "), more)
}

pub fn display_float(n: f64) -> String {
    if exponent_form(n) {
        format!("{:e}", n)
    } else if n.is_finite() && n.fract() == 0.0 {
        format!("{:.1}", n)
    } else {
        n.to_string()
    }
}

/// Very large and very small floats are written with an exponent (`1e300`,
/// `5e-324`), as JavaScript does (`>= 1e21`, `< 1e-7`), instead of hundreds
/// of digits. Shared by `print`/`str` and `json.stringify`.
pub fn exponent_form(n: f64) -> bool {
    let a = n.abs();
    n.is_finite() && a != 0.0 && !(1e-7..1e21).contains(&a)
}

#[cfg(test)]
mod size_tests {
    #[test]
    fn value_is_24_bytes() {
        // every list element, map value and variable is a Value: keep it small
        assert_eq!(std::mem::size_of::<super::Value>(), 24);
    }
}
