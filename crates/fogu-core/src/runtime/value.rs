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
    Map(Rc<RefCell<HashMap<String, Value>>>),
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
}

pub struct StructInstance {
    pub type_name: String,
    /// Module where the struct was declared (`_x` fields are only visible there).
    pub module: Option<Rc<str>>,
    pub fields: HashMap<String, Value>,
    pub embeds: Vec<String>,
    pub methods: HashMap<String, Rc<FnValue>>,
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
        match self {
            Value::Nil => "nil".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(n) => display_float(*n),
            Value::Str(s) => s.to_string(),
            Value::Duration(ns) => display_duration(*ns),
            Value::Range(a, b) => format!("{}..{}", a, b),
            Value::Buffer(b) => display_buffer(&b.borrow()),
            Value::List(l) => {
                let items: Vec<String> = l
                    .borrow()
                    .iter()
                    .map(|v| v.to_display_string())
                    .collect();
                format!("[{}]", items.join(", "))
            }
            Value::Map(m) => {
                let b = m.borrow();
                let mut keys: Vec<&String> = b.keys().collect();
                keys.sort();
                let items: Vec<String> = keys
                    .into_iter()
                    .map(|k| format!("{}: {}", k, b[k].to_display_string()))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            Value::Module(m) => format!("<module {}>", m.name),
            // a native object may describe itself with a `__display__` field
            Value::Object(o) => match o.get("__display__") {
                Some(Value::Str(s)) => s.into_string(),
                _ => format!("<native object {}>", o.name()),
            },
            Value::BoundMethod(b) => {
                format!("<bound {}::{}>", b.receiver.name(), b.method)
            }
            Value::StructBoundMethod(b) => {
                format!("<bound {}::{}>", b.receiver.borrow().type_name, b.method)
            }
            Value::Chan(c) => format!("<chan {} buffer={}>", c.name, c.buffer),
            Value::Struct(s) => {
                let b = s.borrow();
                let mut fields: Vec<String> = b
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_display_string()))
                    .collect();
                fields.sort();
                format!("{}{{{}}}", b.type_name, fields.join(", "))
            }
            Value::StructType(d) => format!("<struct {}>", d.decl.name),
            Value::Interface(i) => format!("<interface {}>", i.name),
            Value::Fn(f) => format!("<fn {}>", f.name),
            Value::NativeFn(f) => format!("<native fn {}>", f.name),
            Value::Error(e) => format!("error: {}", e.message),
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
    if n.is_finite() && n.fract() == 0.0 {
        format!("{:.1}", n)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod size_tests {
    #[test]
    fn value_is_24_bytes() {
        // every list element, map value and variable is a Value: keep it small
        assert_eq!(std::mem::size_of::<super::Value>(), 24);
    }
}
