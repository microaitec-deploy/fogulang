//! Numeric JIT based on Cranelift.
//!
//! The interpreter remains the semantic reference. The JIT compiles to
//! native code only the *numeric* subset of the language (int/float/bool,
//! arithmetic, `if`/`while`/`for a..b`, `match`, calls to other compilable
//! functions, `print` of scalars). Everything else simply "gives up"
//! (bails) and stays in the interpreter.
//!
//! Two entry points:
//!
//! * [`Jit::call_fn`] — functions called with int/float/bool arguments.
//!   They are specialized by argument types; the return type is inferred
//!   (int, float or bool).
//! * [`Jit::run_loop`] — hot `while` / `for a..b` loops. The free numeric
//!   variables are loaded from the environment into a slot vector, the loop
//!   runs natively and the modified variables are written back. It works
//!   as on-stack replacement (OSR).
//!
//! Preserved semantics: integer overflow, division by zero, `**` with a
//! negative exponent and non-exhaustive `match` produce the same errors as
//! the interpreter (see `ops.rs`).

use std::cell::RefCell;
use std::collections::HashSet;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{self, types, AbiParam, FuncRef, InstBuilder, MemFlagsData, Signature, TrapCode, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use super::ast::*;
use super::environment::{redefine_epoch, Environment};
use super::ops;
use super::sched;
use super::value::{display_float, FnValue, Value};

// ======================================================================
// Configuration
// ======================================================================

#[derive(Clone, Debug)]
pub struct JitConfig {
    pub enabled: bool,
    /// 0 = silent, 1 = log compilations/bails, 2 = + CLIF.
    pub debug: u8,
    /// Number of interpreted calls before trying to compile a function.
    pub call_threshold: u32,
    /// Number of interpreted iterations before trying to compile a loop.
    pub loop_threshold: u32,
}

impl JitConfig {
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok();
        JitConfig {
            enabled: get("FOGU_JIT").map_or(true, |v| v != "0"),
            debug: get("FOGU_JIT_DEBUG").and_then(|v| v.parse().ok()).unwrap_or(0),
            call_threshold: get("FOGU_JIT_CALLS").and_then(|v| v.parse().ok()).unwrap_or(2),
            loop_threshold: get("FOGU_JIT_LOOPS").and_then(|v| v.parse().ok()).unwrap_or(200),
        }
    }
}

static CONFIG: OnceLock<JitConfig> = OnceLock::new();
static STATS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Requests a JIT report at the end of the run (`--jit-stats`).
pub fn request_stats() {
    STATS.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn stats_requested() -> bool {
    STATS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Sets the global configuration (CLI flags). Only the first call counts.
pub fn configure(cfg: JitConfig) {
    let _ = CONFIG.set(cfg);
}

fn config() -> JitConfig {
    CONFIG.get().cloned().unwrap_or_else(JitConfig::from_env)
}

// ======================================================================
// Types
// ======================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Ty {
    Int,
    Float,
    Bool,
    /// Result of `print(...)` (nil). Never stored in variables.
    Unit,
    /// A list held by a loop variable (a pointer to its `RefCell<Vec<Value>>`,
    /// kept alive by the environment): `xs.push(…)`, `xs.len()` and, with a
    /// speculated element type, `xs[i]`. The variable can't be reassigned.
    List,
    /// A map held by a loop variable (a pointer to its `RefCell<HashMap>`):
    /// `m[k] = v`, `m.len()` and, with a speculated value type, `m[k]`.
    Map,
    /// A text that native code only reads: a pointer to a `Value::Str` owned
    /// by the list a `for x in list` loop walks (a snapshot, never modified).
    Str,
}

impl Ty {
    fn clif(self) -> ir::Type {
        match self {
            Ty::Int | Ty::List | Ty::Map | Ty::Str => types::I64,
            Ty::Float => types::F64,
            Ty::Bool | Ty::Unit => types::I8,
        }
    }
    fn code(self) -> u64 {
        match self {
            Ty::Int => 1,
            Ty::Float => 2,
            Ty::Bool => 3,
            // never part of a function key: these only live in loop slots
            Ty::Unit | Ty::List | Ty::Map | Ty::Str => 0,
        }
    }
    fn from_code(c: u64) -> Ty {
        match c {
            1 => Ty::Int,
            2 => Ty::Float,
            3 => Ty::Bool,
            _ => Ty::Unit,
        }
    }
    fn of_value(v: &Value) -> Option<Ty> {
        match v {
            Value::Int(_) => Some(Ty::Int),
            Value::Float(_) => Some(Ty::Float),
            Value::Bool(_) => Some(Ty::Bool),
            _ => None,
        }
    }
}

#[derive(Debug)]
enum Bail {
    Msg(String),
    /// The type returned by a `return` is not the assumed candidate.
    Ret(Ty),
    /// Failed because it depends on the assumed return type of a function
    /// still being compiled (mutual recursion); not definitive.
    Guess(String),
}

type R<T> = Result<T, Bail>;

fn bail<T>(m: impl Into<String>) -> R<T> {
    Err(Bail::Msg(m.into()))
}

fn bail_msg(b: &Bail) -> String {
    match b {
        Bail::Msg(m) => m.clone(),
        Bail::Ret(t) => format!("inconsistent return type ({:?})", t),
        Bail::Guess(m) => m.clone(),
    }
}

// ======================================================================
// Execution context shared with native code
// ======================================================================

#[repr(C)]
struct JitCtx {
    err: i64,
    /// (unused: kept so the offsets below stay as they are)
    _reserved: i64,
    /// Lowest address the stack pointer may reach (0 = unknown: no limit).
    stack_limit: i64,
    /// Line of the error that left the native code (0 = the caller's line).
    line: i64,
    /// Set by a helper whose speculation failed (wrong type, missing key,
    /// index out of range): the loop leaves and the interpreter redoes the
    /// iteration (`LoopOutcome::Deopt`).
    deopt: i64,
    /// Where the interpreter resumes: the index of the iteration being redone.
    resume: i64,
}
const OFF_LINE: i32 = 24;
const OFF_DEOPT: i32 = 32;
const OFF_RESUME: i32 = 40;
const OFF_ERR: i32 = 0;

// ---------- helpers called by native code ----------

extern "C" fn h_ipow(ctx: *mut JitCtx, a: i64, b: i64) -> i64 {
    match ops::int_pow(a, b) {
        Ok(v) => v,
        Err(c) => {
            unsafe { (*ctx).err = c };
            0
        }
    }
}
extern "C" fn h_fpow(a: f64, b: f64) -> f64 {
    a.powf(b)
}
extern "C" fn h_fmod(a: f64, b: f64) -> f64 {
    a % b
}

/// The `fogu:math` functions the JIT compiles (see `gen_math`).
const JIT_MATH: &[&str] = &[
    "sqrt", "abs", "floor", "ceil", "trunc", "round", "pow", "min", "max", "lerp", "random", "cbrt", "sin", "cos",
    "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "exp", "exp2", "expm1",
    "log", "log2", "log10", "log1p", "hypot", "fract", "degrees", "radians",
];

macro_rules! math_fns {
    ($($f:ident => $e:expr),* $(,)?) => {$(
        extern "C" fn $f(x: f64) -> f64 {
            ($e)(x)
        }
    )*};
}

math_fns!(
    m_cbrt => f64::cbrt, m_sin => f64::sin, m_cos => f64::cos, m_tan => f64::tan, m_asin => f64::asin,
    m_acos => f64::acos, m_atan => f64::atan, m_sinh => f64::sinh, m_cosh => f64::cosh, m_tanh => f64::tanh,
    m_asinh => f64::asinh, m_acosh => f64::acosh, m_atanh => f64::atanh, m_exp => f64::exp, m_exp2 => f64::exp2,
    m_expm1 => f64::exp_m1, m_ln => f64::ln, m_log2 => f64::log2, m_log10 => f64::log10, m_log1p => f64::ln_1p,
    m_fract => f64::fract, m_degrees => f64::to_degrees, m_radians => f64::to_radians, m_round => f64::round,
);

extern "C" fn m_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}
extern "C" fn m_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
extern "C" fn m_hypot(x: f64, y: f64) -> f64 {
    x.hypot(y)
}
extern "C" fn m_logb(x: f64, b: f64) -> f64 {
    x.log(b)
}
extern "C" fn m_random() -> f64 {
    crate::native::math::next_f64()
}

/// The one-float math function `name` called with `n` arguments.
fn math_unary(name: &str, n: usize) -> Option<extern "C" fn(f64) -> f64> {
    Some(match name {
        "cbrt" => m_cbrt,
        "sin" => m_sin,
        "cos" => m_cos,
        "tan" => m_tan,
        "asin" => m_asin,
        "acos" => m_acos,
        "atan" => m_atan,
        "sinh" => m_sinh,
        "cosh" => m_cosh,
        "tanh" => m_tanh,
        "asinh" => m_asinh,
        "acosh" => m_acosh,
        "atanh" => m_atanh,
        "exp" => m_exp,
        "exp2" => m_exp2,
        "expm1" => m_expm1,
        "log" if n == 1 => m_ln,
        "log2" => m_log2,
        "log10" => m_log10,
        "log1p" => m_log1p,
        "fract" => m_fract,
        "degrees" => m_degrees,
        "radians" => m_radians,
        _ => return None,
    })
}

fn math_binary(name: &str) -> Option<extern "C" fn(f64, f64) -> f64> {
    Some(match name {
        "pow" => m_pow,
        "atan2" => m_atan2,
        "hypot" => m_hypot,
        "log" => m_logb,
        _ => return None,
    })
}
extern "C" fn h_print_i64(v: i64) {
    print!("{}", v);
}
extern "C" fn h_print_f64(v: f64) {
    print!("{}", display_float(v));
}
extern "C" fn h_print_bool(v: i64) {
    print!("{}", v != 0);
}
extern "C" fn h_print_str(p: *const u8, n: i64) {
    let s = unsafe { std::slice::from_raw_parts(p, n as usize) };
    print!("{}", String::from_utf8_lossy(s));
}
extern "C" fn h_print_sep() {
    print!(" ");
}
/// `xs.push(v)` from native loop code; `list` comes from a loop slot.
extern "C" fn h_list_push_i64(list: i64, v: i64) {
    // SAFETY: the pointer was taken from a list the environment keeps alive
    // for the whole loop, and native code holds no borrow of it
    let l = unsafe { &*(list as *const RefCell<Vec<Value>>) };
    l.borrow_mut().push(Value::Int(v));
}
extern "C" fn h_list_push_f64(list: i64, v: f64) {
    let l = unsafe { &*(list as *const RefCell<Vec<Value>>) };
    l.borrow_mut().push(Value::Float(v));
}
extern "C" fn h_list_push_bool(list: i64, v: i64) {
    let l = unsafe { &*(list as *const RefCell<Vec<Value>>) };
    l.borrow_mut().push(Value::Bool(v & 0xff != 0));
}
// Text built by native code for `xs.push(<string expression>)`: a `String`
// behind a raw pointer, created by `h_sb_new` and consumed by `h_list_push_sb`.
extern "C" fn h_sb_new() -> i64 {
    Box::into_raw(Box::new(String::with_capacity(16))) as i64
}
fn sb<'a>(p: i64) -> &'a mut String {
    // SAFETY: `p` comes from `h_sb_new` and is used by one builder at a time
    unsafe { &mut *(p as *mut String) }
}
extern "C" fn h_sb_bytes(b: i64, p: i64, n: i64) {
    // SAFETY: (p, n) is a string literal interned for the life of the program
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(p as *const u8, n as usize)) };
    sb(b).push_str(s);
}
extern "C" fn h_sb_i64(b: i64, v: i64) {
    use std::fmt::Write;
    let _ = write!(sb(b), "{}", v);
}
extern "C" fn h_sb_f64(b: i64, v: f64) {
    sb(b).push_str(&display_float(v));
}
extern "C" fn h_sb_bool(b: i64, v: i64) {
    sb(b).push_str(if v & 0xff != 0 { "true" } else { "false" });
}
extern "C" fn h_list_push_sb(list: i64, b: i64) {
    // SAFETY: `b` comes from `h_sb_new` and is consumed exactly once here
    let text = unsafe { Box::from_raw(b as *mut String) };
    let l = unsafe { &*(list as *const RefCell<Vec<Value>>) };
    l.borrow_mut().push(Value::Str((*text).into()));
}
// Values built by native code for `xs.push({ … })` / `xs.push([ … ])`: each one
// is a boxed `Value` (raw pointer) handed to exactly one consumer, which takes it.
fn vbox(v: Value) -> i64 {
    Box::into_raw(Box::new(v)) as i64
}
fn vtake(p: i64) -> Value {
    // SAFETY: `p` comes from `vbox` and is taken exactly once
    *unsafe { Box::from_raw(p as *mut Value) }
}
extern "C" fn h_v_int(v: i64) -> i64 {
    vbox(Value::Int(v))
}
extern "C" fn h_v_float(v: f64) -> i64 {
    vbox(Value::Float(v))
}
extern "C" fn h_v_bool(v: i64) -> i64 {
    vbox(Value::Bool(v & 0xff != 0))
}
extern "C" fn h_v_str(b: i64) -> i64 {
    // SAFETY: `b` comes from `h_sb_new` and is consumed exactly once here
    let text = unsafe { Box::from_raw(b as *mut String) };
    vbox(Value::Str((*text).into()))
}
extern "C" fn h_v_list(cap: i64) -> i64 {
    vbox(Value::List(Rc::new(RefCell::new(Vec::with_capacity(cap as usize)))))
}
extern "C" fn h_v_list_add(l: i64, v: i64) {
    // SAFETY: `l` is a boxed list from `h_v_list`, still owned by native code
    if let Value::List(x) = unsafe { &*(l as *const Value) } {
        x.borrow_mut().push(vtake(v));
    }
}
extern "C" fn h_v_map(cap: i64) -> i64 {
    vbox(Value::Map(Rc::new(RefCell::new(HashMap::with_capacity(cap as usize)))))
}
extern "C" fn h_v_map_set(m: i64, kp: i64, kn: i64, v: i64) {
    // SAFETY: (kp, kn) is a key interned for the life of the program; `m` is a
    // boxed map from `h_v_map`, still owned by native code
    let key = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(kp as *const u8, kn as usize)) };
    if let Value::Map(x) = unsafe { &*(m as *const Value) } {
        x.borrow_mut().insert(key.to_string(), vtake(v));
    }
}
extern "C" fn h_list_push_v(list: i64, v: i64) {
    let l = unsafe { &*(list as *const RefCell<Vec<Value>>) };
    l.borrow_mut().push(vtake(v));
}
extern "C" fn h_print_nl() {
    println!();
}

// ---- speculation: lists, maps and texts inside loops ----
//
// A helper that reads a value the loop assumed a type for checks it; when the
// assumption fails it sets `ctx.deopt` and the loop leaves before any side
// effect of the iteration, so the interpreter redoes that iteration exactly.

fn deopt(ctx: *mut JitCtx) {
    unsafe { (*ctx).deopt = 1 };
}
fn sval<'a>(p: i64) -> &'a Value {
    // SAFETY: `p` points into a list snapshot owned by the loop (never modified)
    unsafe { &*(p as *const Value) }
}
/// Appends the text of a `Ty::Str` value to a builder.
extern "C" fn h_sb_str(b: i64, p: i64) {
    if let Value::Str(s) = sval(p) {
        sb(b).push_str(s.as_str());
    }
}
extern "C" fn h_print_sv(p: i64) {
    if let Value::Str(s) = sval(p) {
        print!("{}", s.as_str());
    }
}
extern "C" fn h_str_len(p: i64) -> i64 {
    match sval(p) {
        Value::Str(s) => s.char_len() as i64,
        _ => 0,
    }
}
extern "C" fn h_list_len(list: i64) -> i64 {
    let l = unsafe { &*(list as *const RefCell<Vec<Value>>) };
    l.borrow().len() as i64
}
extern "C" fn h_map_len(map: i64) -> i64 {
    let m = unsafe { &*(map as *const RefCell<HashMap<String, Value>>) };
    m.borrow().len() as i64
}
/// Element `idx` of the snapshot `items` (a `*const Value` array), typed.
fn elem<'a>(items: i64, idx: i64) -> &'a Value {
    // SAFETY: the loop runs 0..len over a snapshot the caller keeps alive
    unsafe { &*(items as *const Value).add(idx as usize) }
}
extern "C" fn h_elem_i64(ctx: *mut JitCtx, items: i64, idx: i64) -> i64 {
    match elem(items, idx) {
        Value::Int(n) => *n,
        _ => {
            deopt(ctx);
            0
        }
    }
}
extern "C" fn h_elem_f64(ctx: *mut JitCtx, items: i64, idx: i64) -> f64 {
    match elem(items, idx) {
        Value::Float(x) => *x,
        _ => {
            deopt(ctx);
            0.0
        }
    }
}
extern "C" fn h_elem_bool(ctx: *mut JitCtx, items: i64, idx: i64) -> i64 {
    match elem(items, idx) {
        Value::Bool(b) => *b as i64,
        _ => {
            deopt(ctx);
            0
        }
    }
}
extern "C" fn h_elem_str(ctx: *mut JitCtx, items: i64, idx: i64) -> i64 {
    let v = elem(items, idx);
    match v {
        Value::Str(_) => v as *const Value as i64,
        _ => {
            deopt(ctx);
            0
        }
    }
}
/// `xs[i]` with the interpreter's index rules; anything unexpected deopts
/// (the interpreter then raises its own error or returns the real value).
fn list_at(list: i64, i: i64) -> Option<Value> {
    let l = unsafe { &*(list as *const RefCell<Vec<Value>>) };
    let v = l.borrow();
    let len = v.len() as i64;
    let idx = if i < 0 { i + len } else { i };
    if idx < 0 || idx >= len {
        return None;
    }
    Some(v[idx as usize].clone())
}
extern "C" fn h_list_get_i64(ctx: *mut JitCtx, list: i64, i: i64) -> i64 {
    match list_at(list, i) {
        Some(Value::Int(n)) => n,
        _ => {
            deopt(ctx);
            0
        }
    }
}
extern "C" fn h_list_get_f64(ctx: *mut JitCtx, list: i64, i: i64) -> f64 {
    match list_at(list, i) {
        Some(Value::Float(x)) => x,
        _ => {
            deopt(ctx);
            0.0
        }
    }
}
extern "C" fn h_list_get_bool(ctx: *mut JitCtx, list: i64, i: i64) -> i64 {
    match list_at(list, i) {
        Some(Value::Bool(b)) => b as i64,
        _ => {
            deopt(ctx);
            0
        }
    }
}
/// `m[key]`: the key is a builder (consumed); a missing key or another type deopts.
fn map_at(map: i64, b: i64) -> Option<Value> {
    // SAFETY: `b` comes from `h_sb_new` and is consumed exactly once here
    let key = unsafe { Box::from_raw(b as *mut String) };
    let m = unsafe { &*(map as *const RefCell<HashMap<String, Value>>) };
    let r = m.borrow().get(key.as_str()).cloned();
    r
}
extern "C" fn h_map_get_i64(ctx: *mut JitCtx, map: i64, b: i64) -> i64 {
    match map_at(map, b) {
        Some(Value::Int(n)) => n,
        _ => {
            deopt(ctx);
            0
        }
    }
}
extern "C" fn h_map_get_f64(ctx: *mut JitCtx, map: i64, b: i64) -> f64 {
    match map_at(map, b) {
        Some(Value::Float(x)) => x,
        _ => {
            deopt(ctx);
            0.0
        }
    }
}
extern "C" fn h_map_get_bool(ctx: *mut JitCtx, map: i64, b: i64) -> i64 {
    match map_at(map, b) {
        Some(Value::Bool(x)) => x as i64,
        _ => {
            deopt(ctx);
            0
        }
    }
}
/// `m[key] = v`: the key is a builder (consumed).
fn map_set(map: i64, b: i64, v: Value) {
    // SAFETY: `b` comes from `h_sb_new` and is consumed exactly once here
    let key = unsafe { Box::from_raw(b as *mut String) };
    let m = unsafe { &*(map as *const RefCell<HashMap<String, Value>>) };
    m.borrow_mut().insert(*key, v);
}
extern "C" fn h_map_set_i64(map: i64, b: i64, v: i64) {
    map_set(map, b, Value::Int(v));
}
extern "C" fn h_map_set_f64(map: i64, b: i64, v: f64) {
    map_set(map, b, Value::Float(v));
}
extern "C" fn h_map_set_bool(map: i64, b: i64, v: i64) {
    map_set(map, b, Value::Bool(v & 0xff != 0));
}
extern "C" fn h_map_set_sb(map: i64, b: i64, text: i64) {
    // SAFETY: `text` comes from `h_sb_new` and is consumed exactly once here
    let t = unsafe { Box::from_raw(text as *mut String) };
    map_set(map, b, Value::Str((*t).into()));
}
extern "C" fn h_map_set_v(map: i64, b: i64, v: i64) {
    map_set(map, b, vtake(v));
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Helper {
    Ipow,
    Fpow,
    Fmod,
    PrintI64,
    PrintF64,
    PrintBool,
    PrintStr,
    PrintSep,
    PrintNl,
    ListPushI64,
    ListPushF64,
    ListPushBool,
    SbNew,
    SbBytes,
    SbI64,
    SbF64,
    SbBool,
    ListPushSb,
    VInt,
    VFloat,
    VBool,
    VStr,
    VList,
    VListAdd,
    VMap,
    VMapSet,
    ListPushV,
    SbStr,
    PrintSv,
    StrLen,
    ListLen,
    MapLen,
    ElemI64,
    ElemF64,
    ElemBool,
    ElemStr,
    ListGetI64,
    ListGetF64,
    ListGetBool,
    MapGetI64,
    MapGetF64,
    MapGetBool,
    MapSetI64,
    MapSetF64,
    MapSetBool,
    MapSetSb,
    MapSetV,
}

impl Helper {
    const ALL: &[Helper] = &[
        Helper::Ipow,
        Helper::Fpow,
        Helper::Fmod,
        Helper::PrintI64,
        Helper::PrintF64,
        Helper::PrintBool,
        Helper::PrintStr,
        Helper::PrintSep,
        Helper::PrintNl,
        Helper::ListPushI64,
        Helper::ListPushF64,
        Helper::ListPushBool,
        Helper::SbNew,
        Helper::SbBytes,
        Helper::SbI64,
        Helper::SbF64,
        Helper::SbBool,
        Helper::ListPushSb,
        Helper::VInt,
        Helper::VFloat,
        Helper::VBool,
        Helper::VStr,
        Helper::VList,
        Helper::VListAdd,
        Helper::VMap,
        Helper::VMapSet,
        Helper::ListPushV,
        Helper::SbStr,
        Helper::PrintSv,
        Helper::StrLen,
        Helper::ListLen,
        Helper::MapLen,
        Helper::ElemI64,
        Helper::ElemF64,
        Helper::ElemBool,
        Helper::ElemStr,
        Helper::ListGetI64,
        Helper::ListGetF64,
        Helper::ListGetBool,
        Helper::MapGetI64,
        Helper::MapGetF64,
        Helper::MapGetBool,
        Helper::MapSetI64,
        Helper::MapSetF64,
        Helper::MapSetBool,
        Helper::MapSetSb,
        Helper::MapSetV,
    ];
    fn name(self) -> &'static str {
        match self {
            Helper::Ipow => "fogu_ipow",
            Helper::Fpow => "fogu_fpow",
            Helper::Fmod => "fogu_fmod",
            Helper::PrintI64 => "fogu_print_i64",
            Helper::PrintF64 => "fogu_print_f64",
            Helper::PrintBool => "fogu_print_bool",
            Helper::PrintStr => "fogu_print_str",
            Helper::PrintSep => "fogu_print_sep",
            Helper::PrintNl => "fogu_print_nl",
            Helper::ListPushI64 => "fogu_list_push_i64",
            Helper::ListPushF64 => "fogu_list_push_f64",
            Helper::ListPushBool => "fogu_list_push_bool",
            Helper::SbNew => "fogu_sb_new",
            Helper::SbBytes => "fogu_sb_bytes",
            Helper::SbI64 => "fogu_sb_i64",
            Helper::SbF64 => "fogu_sb_f64",
            Helper::SbBool => "fogu_sb_bool",
            Helper::ListPushSb => "fogu_list_push_sb",
            Helper::VInt => "fogu_v_int",
            Helper::VFloat => "fogu_v_float",
            Helper::VBool => "fogu_v_bool",
            Helper::VStr => "fogu_v_str",
            Helper::VList => "fogu_v_list",
            Helper::VListAdd => "fogu_v_list_add",
            Helper::VMap => "fogu_v_map",
            Helper::VMapSet => "fogu_v_map_set",
            Helper::ListPushV => "fogu_list_push_v",
            Helper::SbStr => "fogu_sb_str",
            Helper::PrintSv => "fogu_print_sv",
            Helper::StrLen => "fogu_str_len",
            Helper::ListLen => "fogu_list_len",
            Helper::MapLen => "fogu_map_len",
            Helper::ElemI64 => "fogu_elem_i64",
            Helper::ElemF64 => "fogu_elem_f64",
            Helper::ElemBool => "fogu_elem_bool",
            Helper::ElemStr => "fogu_elem_str",
            Helper::ListGetI64 => "fogu_list_get_i64",
            Helper::ListGetF64 => "fogu_list_get_f64",
            Helper::ListGetBool => "fogu_list_get_bool",
            Helper::MapGetI64 => "fogu_map_get_i64",
            Helper::MapGetF64 => "fogu_map_get_f64",
            Helper::MapGetBool => "fogu_map_get_bool",
            Helper::MapSetI64 => "fogu_map_set_i64",
            Helper::MapSetF64 => "fogu_map_set_f64",
            Helper::MapSetBool => "fogu_map_set_bool",
            Helper::MapSetSb => "fogu_map_set_sb",
            Helper::MapSetV => "fogu_map_set_v",
        }
    }
    fn addr(self) -> *const u8 {
        match self {
            Helper::Ipow => h_ipow as *const u8,
            Helper::Fpow => h_fpow as *const u8,
            Helper::Fmod => h_fmod as *const u8,
            Helper::PrintI64 => h_print_i64 as *const u8,
            Helper::PrintF64 => h_print_f64 as *const u8,
            Helper::PrintBool => h_print_bool as *const u8,
            Helper::PrintStr => h_print_str as *const u8,
            Helper::PrintSep => h_print_sep as *const u8,
            Helper::PrintNl => h_print_nl as *const u8,
            Helper::ListPushI64 => h_list_push_i64 as *const u8,
            Helper::ListPushF64 => h_list_push_f64 as *const u8,
            Helper::ListPushBool => h_list_push_bool as *const u8,
            Helper::SbNew => h_sb_new as *const u8,
            Helper::SbBytes => h_sb_bytes as *const u8,
            Helper::SbI64 => h_sb_i64 as *const u8,
            Helper::SbF64 => h_sb_f64 as *const u8,
            Helper::SbBool => h_sb_bool as *const u8,
            Helper::ListPushSb => h_list_push_sb as *const u8,
            Helper::VInt => h_v_int as *const u8,
            Helper::VFloat => h_v_float as *const u8,
            Helper::VBool => h_v_bool as *const u8,
            Helper::VStr => h_v_str as *const u8,
            Helper::VList => h_v_list as *const u8,
            Helper::VListAdd => h_v_list_add as *const u8,
            Helper::VMap => h_v_map as *const u8,
            Helper::VMapSet => h_v_map_set as *const u8,
            Helper::ListPushV => h_list_push_v as *const u8,
            Helper::SbStr => h_sb_str as *const u8,
            Helper::PrintSv => h_print_sv as *const u8,
            Helper::StrLen => h_str_len as *const u8,
            Helper::ListLen => h_list_len as *const u8,
            Helper::MapLen => h_map_len as *const u8,
            Helper::ElemI64 => h_elem_i64 as *const u8,
            Helper::ElemF64 => h_elem_f64 as *const u8,
            Helper::ElemBool => h_elem_bool as *const u8,
            Helper::ElemStr => h_elem_str as *const u8,
            Helper::ListGetI64 => h_list_get_i64 as *const u8,
            Helper::ListGetF64 => h_list_get_f64 as *const u8,
            Helper::ListGetBool => h_list_get_bool as *const u8,
            Helper::MapGetI64 => h_map_get_i64 as *const u8,
            Helper::MapGetF64 => h_map_get_f64 as *const u8,
            Helper::MapGetBool => h_map_get_bool as *const u8,
            Helper::MapSetI64 => h_map_set_i64 as *const u8,
            Helper::MapSetF64 => h_map_set_f64 as *const u8,
            Helper::MapSetBool => h_map_set_bool as *const u8,
            Helper::MapSetSb => h_map_set_sb as *const u8,
            Helper::MapSetV => h_map_set_v as *const u8,
        }
    }
    fn params_ret(self) -> (Vec<ir::Type>, Option<ir::Type>) {
        use types::{F64, I64};
        match self {
            Helper::Ipow => (vec![I64, I64, I64], Some(I64)),
            Helper::Fpow | Helper::Fmod => (vec![F64, F64], Some(F64)),
            Helper::PrintI64 | Helper::PrintBool => (vec![I64], None),
            Helper::PrintF64 => (vec![F64], None),
            Helper::PrintStr => (vec![I64, I64], None),
            Helper::PrintSep | Helper::PrintNl => (vec![], None),
            Helper::ListPushI64 | Helper::ListPushBool => (vec![I64, I64], None),
            Helper::ListPushF64 => (vec![I64, F64], None),
            Helper::SbNew => (vec![], Some(I64)),
            Helper::SbBytes => (vec![I64, I64, I64], None),
            Helper::SbI64 | Helper::SbBool | Helper::ListPushSb => (vec![I64, I64], None),
            Helper::SbF64 => (vec![I64, F64], None),
            Helper::VInt | Helper::VBool | Helper::VStr | Helper::VList | Helper::VMap => (vec![I64], Some(I64)),
            Helper::VFloat => (vec![F64], Some(I64)),
            Helper::VListAdd | Helper::ListPushV => (vec![I64, I64], None),
            Helper::VMapSet => (vec![I64, I64, I64, I64], None),
            Helper::SbStr => (vec![I64, I64], None),
            Helper::PrintSv => (vec![I64], None),
            Helper::StrLen | Helper::ListLen | Helper::MapLen => (vec![I64], Some(I64)),
            Helper::ElemI64 | Helper::ElemBool | Helper::ElemStr => (vec![I64, I64, I64], Some(I64)),
            Helper::ElemF64 => (vec![I64, I64, I64], Some(F64)),
            Helper::ListGetI64 | Helper::ListGetBool | Helper::MapGetI64 | Helper::MapGetBool => (vec![I64, I64, I64], Some(I64)),
            Helper::ListGetF64 | Helper::MapGetF64 => (vec![I64, I64, I64], Some(F64)),
            Helper::MapSetI64 | Helper::MapSetBool | Helper::MapSetSb | Helper::MapSetV => (vec![I64, I64, I64], None),
            Helper::MapSetF64 => (vec![I64, I64, F64], None),
        }
    }
}

// ======================================================================
// Keys and cache entries
// ======================================================================

// ======================================================================
// Deadlines (`fogu:vm` timeouts) in native code
// ======================================================================
//
// Native code does not return to the interpreter between statements, so it
// cannot look at the clock the way the interpreter does. Instead every
// compiled function compares the stack pointer with a limit on entry (its
// stack-overflow check), and code compiled for a run with a deadline does
// the same at every loop iteration. When the deadline of the running native
// code passes, a watchdog thread raises that limit above any address: the
// next check fails, the native code unwinds through its error path and the
// interpreter reports the timeout. Code that runs without a deadline pays
// nothing for this.

/// After a fallible typed native ([`super::value::TypedNative`]): its error,
/// if it left one, becomes the error of the native code.
extern "C" fn typed_check(ctx: *mut JitCtx) {
    if let Some(msg) = crate::native::util::take_typed_error() {
        // SAFETY: called by compiled code with its own context; the
        // interpreter that runs this native code is below us on the stack.
        unsafe {
            (*ctx).err = ops::custom_code(&msg);
            // the error was raised by Fogu code the native function ran (a
            // callback): its line is the line of the error
            if let Some(it) = super::interpreter::native_interpreter() {
                (*ctx).line = (*it).current_line() as i64;
            }
        }
    }
}

/// The stack limit of one JIT, and the deadline of the native code running
/// on it (if it has one).
pub struct Watch {
    /// The deadline (see [`deadline_ns`]) while native code with a deadline
    /// runs. The watchdog raises the limit holding this lock, and the
    /// interpreter arms / disarms holding it: a deadline can never stop the
    /// native code of another run.
    armed: std::sync::Mutex<Option<u64>>,
    /// Lowest address the stack pointer may reach (-1: stop now).
    limit: std::sync::atomic::AtomicI64,
}

/// "Stop now": above every address when compared unsigned.
const LIMIT_STOP: i64 = -1;

struct Watchdog {
    watches: std::sync::Mutex<Vec<std::sync::Weak<Watch>>>,
    /// The thread sleeps here while no deadline is armed.
    parked: std::sync::Mutex<bool>,
    wake: std::sync::Condvar,
}

fn watchdog() -> &'static Watchdog {
    static W: OnceLock<Watchdog> = OnceLock::new();
    W.get_or_init(|| Watchdog {
        watches: std::sync::Mutex::new(Vec::new()),
        parked: std::sync::Mutex::new(false),
        wake: std::sync::Condvar::new(),
    })
}

fn clock_base() -> Instant {
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

/// A deadline as native code is armed with it: nanoseconds on the JIT's clock.
pub fn deadline_ns(at: Instant) -> u64 {
    (at.saturating_duration_since(clock_base()).as_nanos() as u64).max(1)
}

/// Registers a JIT with the watchdog (starting its thread the first time).
fn watch_register(w: &std::sync::Arc<Watch>) {
    let wd = watchdog();
    wd.watches.lock().unwrap_or_else(|p| p.into_inner()).push(std::sync::Arc::downgrade(w));
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        let _ = std::thread::Builder::new().name("fogu-jit-watchdog".into()).spawn(watchdog_loop);
    });
}

/// Wakes the watchdog if it is parked (a deadline was armed).
fn watch_wake() {
    let wd = watchdog();
    let mut parked = wd.parked.lock().unwrap_or_else(|p| p.into_inner());
    if *parked {
        *parked = false;
        wd.wake.notify_all();
    }
}

fn watchdog_loop() {
    let wd = watchdog();
    let mut idle = 0u32;
    loop {
        let mut any = false;
        {
            let mut ws = wd.watches.lock().unwrap_or_else(|p| p.into_inner());
            ws.retain(|w| w.strong_count() > 0);
            let now = clock_base().elapsed().as_nanos() as u64;
            for w in ws.iter().filter_map(std::sync::Weak::upgrade) {
                let armed = w.armed.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(d) = *armed {
                    any = true;
                    if now >= d {
                        w.limit.store(LIMIT_STOP, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        }
        if any {
            idle = 0;
            std::thread::sleep(std::time::Duration::from_micros(500));
            continue;
        }
        idle += 1;
        if idle < 200 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        // nothing armed for a while: sleep until the next deadline is armed.
        // The decision is taken holding `parked`: a deadline armed since the
        // scan above either is seen by the scan below, or finds `parked` set
        // (its `watch_wake` waits for this lock) and wakes this thread.
        idle = 0;
        let mut parked = wd.parked.lock().unwrap_or_else(|p| p.into_inner());
        *parked = true;
        let armed_now = {
            let ws = wd.watches.lock().unwrap_or_else(|p| p.into_inner());
            ws.iter().filter_map(std::sync::Weak::upgrade).any(|w| w.armed.lock().unwrap_or_else(|p| p.into_inner()).is_some())
        };
        if armed_now {
            *parked = false;
            continue;
        }
        while *parked {
            parked = wd.wake.wait(parked).unwrap_or_else(|p| p.into_inner());
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct FnKey {
    ptr: usize,
    mask: u64,
    n: u8,
    /// Compiled for a run with a deadline: its loops check the limit too.
    checked: bool,
}

impl FnKey {
    fn tys(&self) -> Vec<Ty> {
        (0..self.n as usize)
            .map(|i| Ty::from_code((self.mask >> (2 * i)) & 3))
            .collect()
    }
}

const MAX_ARGS: usize = 24;

enum Spec {
    InProgress { id: FuncId, ret: Ty },
    Done { id: FuncId, ret: Ty, thunk: Option<usize>, _keep: Block },
    Failed(String),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct LoopKey {
    id: u32,
    /// 0 `while`, 1 `for a..b`, 2 `for x in list`
    kind: u8,
    slots: Vec<(String, Ty, bool)>,
    /// Speculated element type of each list slot / value type of each map slot.
    spec: Vec<(String, Ty)>,
    /// Speculated element type of the list a `for x in list` loop walks.
    elem: Option<Ty>,
    /// Compiled for a run with a deadline (see [`Watch`]).
    checked: bool,
}

enum LoopEntry {
    Done { addr: usize, assigned: Vec<bool> },
    Failed,
}

#[derive(Default)]
struct Stats {
    fns_compiled: u32,
    loops_compiled: u32,
    bails: Vec<String>,
    compile_time_us: u128,
    native_calls: u64,
    native_loops: u64,
    /// Loops that left early because a speculation failed.
    deopts: u64,
}

// ======================================================================
// Compiler state
// ======================================================================

/// A function of at most this many IR instructions may be inlined.
const INLINE_MAX_INSTS: usize = 120;
/// Instructions one function may receive from inlined callees.
const INLINE_BUDGET: usize = 480;

/// Tells Cranelift which calls to inline: the direct calls to functions whose
/// body is in `bodies`, while the budget lasts.
struct Inliner<'a> {
    bodies: &'a HashMap<u32, ir::Function>,
    budget: usize,
}

impl cranelift_codegen::inline::Inline for Inliner<'_> {
    fn inline(
        &mut self,
        caller: &ir::Function,
        _inst: ir::Inst,
        opcode: ir::Opcode,
        callee: FuncRef,
        _args: &[ir::Value],
    ) -> cranelift_codegen::inline::InlineCommand<'_> {
        use cranelift_codegen::inline::InlineCommand;
        if opcode != ir::Opcode::Call {
            return InlineCommand::KeepCall;
        }
        let ir::ExternalName::User(name) = caller.dfg.ext_funcs[callee].name else {
            return InlineCommand::KeepCall;
        };
        let name = &caller.params.user_named_funcs()[name];
        if name.namespace != 0 {
            return InlineCommand::KeepCall;
        }
        match self.bodies.get(&name.index) {
            Some(f) if f.dfg.num_insts() <= self.budget => {
                self.budget -= f.dfg.num_insts();
                InlineCommand::Inline { callee: std::borrow::Cow::Borrowed(f), visit_callee: false }
            }
            _ => InlineCommand::KeepCall,
        }
    }
}

/// Makes every conditional branch that can leave a loop do so through its
/// first target (`brif c, body, exit` becomes `brif c == 0, exit, body`).
///
/// Cranelift's optimizer walks the blocks in an order that follows the
/// second target of a branch first, and it only knows it is inside a loop
/// until it meets a block outside of it: with the exit as second target, the
/// rest of the loop body was optimized as if it were outside any loop. Then
/// nothing loop-invariant was moved out, and values computed before the loop
/// but used only inside it were computed again on every iteration instead.
fn order_branches(func: &mut ir::Function) {
    use cranelift_codegen::cursor::{Cursor, FuncCursor};
    use cranelift_codegen::dominator_tree::DominatorTree;
    use cranelift_codegen::ir::condcodes::CondCode;
    use cranelift_codegen::flowgraph::ControlFlowGraph;
    use cranelift_codegen::loop_analysis::LoopAnalysis;

    let cfg = ControlFlowGraph::with_function(func);
    let domtree = DominatorTree::with_function(func, &cfg);
    let mut loops = LoopAnalysis::new();
    loops.compute(func, &cfg, &domtree);
    if loops.loops().next().is_none() {
        return;
    }
    let mut leaving = Vec::new();
    for block in func.layout.blocks() {
        let Some(inst) = func.layout.last_inst(block) else { continue };
        if let ir::InstructionData::Brif { arg, blocks, .. } = &func.dfg.insts[inst] {
            let then_b = blocks[0].block(&func.dfg.value_lists);
            let else_b = blocks[1].block(&func.dfg.value_lists);
            if loops.loop_level(else_b).level() < loops.loop_level(then_b).level() {
                leaving.push((inst, *arg));
            }
        }
    }
    for (inst, cond) in leaving {
        // the opposite comparison when the condition is one (so the branch
        // still tests the flags directly), `cond == 0` otherwise
        let compare = match func.dfg.value_def(cond) {
            ir::ValueDef::Result(def, _) => match func.dfg.insts[def] {
                ir::InstructionData::IntCompare { cond: cc, args, .. } => Some((Ok(cc.complement()), args)),
                ir::InstructionData::FloatCompare { cond: cc, args, .. } => Some((Err(cc.complement()), args)),
                _ => None,
            },
            _ => None,
        };
        let mut cur = FuncCursor::new(func).at_inst(inst);
        let not = match compare {
            Some((Ok(cc), [a, b])) => cur.ins().icmp(cc, a, b),
            Some((Err(cc), [a, b])) => cur.ins().fcmp(cc, a, b),
            None => cur.ins().icmp_imm_s(IntCC::Equal, cond, 0),
        };
        if let ir::InstructionData::Brif { arg, blocks, .. } = &mut cur.func.dfg.insts[inst] {
            *arg = not;
            blocks.swap(0, 1);
        }
    }
}

/// The multiplier and shift that turn a signed division by the constant `d`
/// (not 0, 1, -1 or a power of two) into a multiplication (Hacker's Delight,
/// 10-1): `q = mulhi(n, m)`, corrected by `n` when the signs of `d` and `m`
/// differ, shifted right by the shift, plus its own sign bit.
fn div_magic(d: i64) -> (i64, u32) {
    const TWO63: u64 = 1 << 63;
    let ad = d.unsigned_abs();
    let t = TWO63 + ((d as u64) >> 63);
    let anc = t - 1 - t % ad;
    let mut p = 63u32;
    let mut q1 = TWO63 / anc;
    let mut r1 = TWO63 - q1 * anc;
    let mut q2 = TWO63 / ad;
    let mut r2 = TWO63 - q2 * ad;
    loop {
        p += 1;
        q1 = q1.wrapping_mul(2);
        r1 = r1.wrapping_mul(2);
        if r1 >= anc {
            q1 = q1.wrapping_add(1);
            r1 = r1.wrapping_sub(anc);
        }
        q2 = q2.wrapping_mul(2);
        r2 = r2.wrapping_mul(2);
        if r2 >= ad {
            q2 = q2.wrapping_add(1);
            r2 = r2.wrapping_sub(ad);
        }
        let delta = ad - r2;
        if !(q1 < delta || (q1 == delta && r1 == 0)) {
            break;
        }
    }
    let m = q2.wrapping_add(1) as i64;
    (if d < 0 { m.wrapping_neg() } else { m }, p - 64)
}

struct Inner {
    /// Address of the stack limit native code checks ([`Watch::limit`]).
    limit_addr: usize,
    module: JITModule,
    helper_ids: HashMap<Helper, FuncId>,
    specs: HashMap<FnKey, Spec>,
    /// Functions being compiled and the in-progress ones they depend on.
    building: Vec<(FnKey, HashSet<FnKey>)>,
    /// Provisional specializations (depend on a guess not yet confirmed).
    tent: HashMap<FnKey, HashSet<FnKey>>,
    guess_fails: HashMap<FnKey, u32>,
    loops: HashMap<LoopKey, LoopEntry>,
    ctx: Box<JitCtx>,
    epoch: u64,
    counter: u32,
    debug: u8,
    /// Address of the `math` native fn -> name ("sqrt", "abs", ...).
    math_fns: HashMap<usize, &'static str>,
    strs: HashMap<String, &'static str>,
    stats: Stats,
    /// IR of the small compiled functions (by `FuncId`), before any inlining:
    /// what [`Inner::inline_calls`] copies into their callers.
    bodies: HashMap<u32, ir::Function>,
}

impl Inner {
    fn new(debug: u8, limit_addr: usize) -> Result<Inner, String> {
        let mut flags = settings::builder();
        flags.set("use_colocated_libcalls", "false").map_err(|e| e.to_string())?;
        flags.set("is_pic", "false").map_err(|e| e.to_string())?;
        flags.set("opt_level", "speed").map_err(|e| e.to_string())?;
        // the IR verifier is a development aid (kept in debug builds, where
        // the tests run): in a release it is time added to every compilation
        if !cfg!(debug_assertions) {
            flags.set("enable_verifier", "false").map_err(|e| e.to_string())?;
        }
        // nothing unwinds through compiled code (it reports errors through
        // the context), so no unwind tables are needed
        flags.set("unwind_info", "false").map_err(|e| e.to_string())?;
        let isa = cranelift_native::builder()
            .map_err(|e| e.to_string())?
            .finish(settings::Flags::new(flags))
            .map_err(|e| e.to_string())?;
        let mut jb = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        for &h in Helper::ALL {
            jb.symbol(h.name(), h.addr());
        }
        let mut module = JITModule::new(jb);

        let mut helper_ids = HashMap::new();
        for &h in Helper::ALL {
            let (ps, r) = h.params_ret();
            let mut sig = module.make_signature();
            for p in ps {
                sig.params.push(AbiParam::new(p));
            }
            if let Some(r) = r {
                sig.returns.push(AbiParam::new(r));
            }
            let id = module
                .declare_function(h.name(), Linkage::Import, &sig)
                .map_err(|e| e.to_string())?;
            helper_ids.insert(h, id);
        }

        let mut math_fns = HashMap::new();
        let m = crate::native::math::build();
        let mut seen = HashSet::new();
        for &name in JIT_MATH {
            if let Some(Value::NativeFn(nf)) = m.exports.borrow().get(name) {
                // two functions folded into one address by the linker can't be told apart
                if !seen.insert(nf.func as usize) {
                    math_fns.remove(&(nf.func as usize));
                    continue;
                }
                math_fns.insert(nf.func as usize, name);
            }
        }

        Ok(Inner {
            module,
            helper_ids,
            specs: HashMap::new(),
            building: Vec::new(),
            tent: HashMap::new(),
            guess_fails: HashMap::new(),
            loops: HashMap::new(),
            ctx: Box::new(JitCtx { err: 0, _reserved: 0, stack_limit: 0, line: 0, deopt: 0, resume: 0 }),
            limit_addr,
            epoch: redefine_epoch(),
            counter: 0,
            debug,
            math_fns,
            strs: HashMap::new(),
            stats: Stats::default(),
            bodies: HashMap::new(),
        })
    }

    fn log(&self, msg: impl AsRef<str>) {
        if self.debug >= 1 {
            eprintln!("[jit] {}", msg.as_ref());
        }
    }

    fn intern(&mut self, s: &str) -> &'static str {
        if let Some(x) = self.strs.get(s) {
            return x;
        }
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        self.strs.insert(s.to_string(), leaked);
        leaked
    }

    fn fn_sig(&self, tys: &[Ty], ret: Ty) -> Signature {
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // ctx
        for t in tys {
            sig.params.push(AbiParam::new(t.clif()));
        }
        sig.returns.push(AbiParam::new(ret.clif()));
        sig
    }

    // ------------------------------------------------------------------
    // Functions
    // ------------------------------------------------------------------

    /// Returns (id, return type) of the specialization, compiling it if
    /// needed. `caller` identifies the function being compiled (only direct
    /// recursion is supported).
    fn specialize(
        &mut self,
        f: &Rc<FnValue>,
        key: FnKey,
        caller: Option<FnKey>,
    ) -> R<(FuncId, Ty)> {
        if let Some(e) = self.specs.get(&key) {
            return match e {
                Spec::Done { id, ret, .. } => {
                    let r = (*id, *ret);
                    // Using a still-provisional specialization inherits its dependencies.
                    if let Some(deps) = self.tent.get(&key).cloned() {
                        if let Some(top) = self.building.last_mut() {
                            for d in deps {
                                if d != top.0 {
                                    top.1.insert(d);
                                }
                            }
                        }
                    }
                    Ok(r)
                }
                Spec::Failed(m) => bail(m.clone()),
                Spec::InProgress { id, ret } => {
                    let r = (*id, *ret);
                    if caller != Some(key) {
                        // mutual recursion: depends on the type assumed for `key`
                        if let Some(top) = self.building.last_mut() {
                            top.1.insert(key);
                        }
                    }
                    Ok(r)
                }
            };
        }

        let tys = key.tys();
        let t0 = Instant::now();
        let mut last = Bail::Msg("no viable return type".into());
        let mut guessed = false;
        for cand in [Ty::Int, Ty::Float, Ty::Bool] {
            self.counter += 1;
            let name = format!("{}_{}", f.name.replace(|c: char| !c.is_alphanumeric(), "_"), self.counter);
            let sig = self.fn_sig(&tys, cand);
            let id = self
                .module
                .declare_function(&name, Linkage::Local, &sig)
                .map_err(|e| Bail::Msg(e.to_string()))?;
            self.specs.insert(key, Spec::InProgress { id, ret: cand });
            self.building.push((key, HashSet::new()));
            let res = self.build_function(f, &tys, id, cand, key, sig);
            let (_, mut refs) = self.building.pop().expect("compilation stack");
            refs.remove(&key);
            match res {
                Ok(()) => {
                    self.specs.insert(
                        key,
                        Spec::Done { id, ret: cand, thunk: None, _keep: Rc::clone(&f.body) },
                    );
                    self.stats.fns_compiled += 1;
                    let us = t0.elapsed().as_micros();
                    self.stats.compile_time_us += us;
                    self.log(format!(
                        "function '{}'({}) -> {:?} compiled in {}µs",
                        f.name,
                        tys.iter().map(|t| format!("{:?}", t)).collect::<Vec<_>>().join(", "),
                        cand,
                        us
                    ));
                    // The provisional ones that depended on `key` now depend
                    // only on what `key` still assumes.
                    let dependents: Vec<FnKey> = self
                        .tent
                        .iter()
                        .filter(|(_, d)| d.contains(&key))
                        .map(|(k, _)| *k)
                        .collect();
                    for k in dependents {
                        let d = self.tent.get_mut(&k).unwrap();
                        d.remove(&key);
                        d.extend(refs.iter().copied().filter(|r| *r != k));
                        if d.is_empty() {
                            self.tent.remove(&k);
                        }
                    }
                    if !refs.is_empty() {
                        self.tent.insert(key, refs.clone());
                        if let Some(top) = self.building.last_mut() {
                            for d in refs {
                                if d != top.0 {
                                    top.1.insert(d);
                                }
                            }
                        }
                    }
                    return Ok((id, cand));
                }
                Err(b) => {
                    // Everything compiled assuming the wrong type of `key` is dropped.
                    self.drop_dependents(key);
                    match b {
                        Bail::Ret(_) => {
                            last = Bail::Ret(cand);
                        }
                        Bail::Guess(_) => {
                            guessed = true;
                            last = b;
                        }
                        other if !refs.is_empty() => {
                            guessed = true;
                            last = Bail::Guess(bail_msg(&other));
                        }
                        other => {
                            let m = bail_msg(&other);
                            self.log(format!("function '{}' not compiled: {}", f.name, m));
                            self.stats.bails.push(format!("fn {}: {}", f.name, m));
                            self.specs.insert(key, Spec::Failed(m.clone()));
                            return Err(other);
                        }
                    }
                }
            }
        }
        let m = bail_msg(&last);
        if guessed {
            // It may have failed only because of an ancestor's guess: do not
            // memoize right away (but give up after 3 attempts).
            let n = self.guess_fails.entry(key).or_insert(0);
            *n += 1;
            if *n < 3 {
                self.specs.remove(&key);
                return Err(Bail::Guess(m));
            }
        }
        self.log(format!("function '{}' not compiled: {}", f.name, m));
        self.stats.bails.push(format!("fn {}: {}", f.name, m));
        self.specs.insert(key, Spec::Failed(m.clone()));
        bail(m)
    }

    fn drop_dependents(&mut self, key: FnKey) {
        let dead: Vec<FnKey> = self
            .tent
            .iter()
            .filter(|(_, d)| d.contains(&key))
            .map(|(k, _)| *k)
            .collect();
        for k in dead {
            self.tent.remove(&k);
            self.specs.remove(&k);
        }
    }

    fn build_function(
        &mut self,
        f: &Rc<FnValue>,
        tys: &[Ty],
        id: FuncId,
        ret: Ty,
        me: FnKey,
        sig: Signature,
    ) -> R<()> {
        let env = match &f.closure {
            Some(e) => Rc::clone(e),
            None => return bail("function without a closure environment"),
        };
        if !always_returns(&f.body) {
            return bail("the function may end without 'return' (would return nil)");
        }

        let mut ctx = Context::new();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, id.as_u32());
        let mut fbc = FunctionBuilderContext::new();
        let cfg = self.module.target_config();
        {
            let b = FunctionBuilder::new(&mut ctx.func, &mut fbc);
            let mut g = Gen::new(self, b, env, Some(ret), Some(me));
            g.fn_name = f.name.clone();
            g.checked = me.checked;
            g.prologue_fn(&f.params, tys)?;
            for s in f.body.iter() {
                g.stmt(s)?;
            }
            g.finish(cfg);
        }
        order_branches(&mut ctx.func);
        self.inline_calls(&mut ctx, Some(id))?;
        self.define(id, &mut ctx)
    }

    /// Copies the body of small compiled functions into their call sites in
    /// `ctx` (one level deep), so a call costs nothing: no frame, no stack
    /// check per call, and the optimizer sees caller and callee together. A
    /// function that calls itself gets one copy of its own body per call site,
    /// which removes most of the calls of a recursion.
    fn inline_calls(&mut self, ctx: &mut Context, me: Option<FuncId>) -> R<()> {
        if let Some(id) = me {
            if ctx.func.dfg.num_insts() <= INLINE_MAX_INSTS {
                self.bodies.insert(id.as_u32(), ctx.func.clone());
            }
        }
        if self.bodies.is_empty() {
            return Ok(());
        }
        let mut inl = Inliner { bodies: &self.bodies, budget: INLINE_BUDGET };
        ctx.inline(&mut inl)
            .map(|_| ())
            .map_err(|e| Bail::Msg(format!("internal JIT error (inlining): {:?}", e)))
    }

    fn define(&mut self, id: FuncId, ctx: &mut Context) -> R<()> {
        if self.debug >= 2 {
            eprintln!("{}", ctx.func.display());
        }
        if self.debug >= 3 {
            ctx.set_disasm(true);
        }
        let t0 = Instant::now();
        self.module
            .define_function(id, ctx)
            .map_err(|e| Bail::Msg(format!("internal JIT error: {:?}", e)))?;
        let code_us = t0.elapsed().as_micros();
        if self.debug >= 3 {
            eprintln!("; optimized\n{}", ctx.func.display());
            if let Some(code) = ctx.compiled_code() {
                eprintln!("{}", code.vcode.as_deref().unwrap_or(""));
            }
        }
        if self.debug >= 1 {
            eprintln!("[jit] machine code in {}µs", code_us);
        }
        self.module.clear_context(ctx);
        Ok(())
    }

    /// Generates (once) a `(ctx, args*, out*)` thunk to call the function
    /// from Rust with a uniform ABI.
    fn ensure_thunk(&mut self, key: FnKey) -> R<usize> {
        let (id, ret, existing) = match self.specs.get(&key) {
            Some(Spec::Done { id, ret, thunk, .. }) => (*id, *ret, *thunk),
            _ => return bail("missing specialization"),
        };
        if let Some(t) = existing {
            return Ok(t);
        }
        let tys = key.tys();

        let mut sig = self.module.make_signature();
        for _ in 0..3 {
            sig.params.push(AbiParam::new(types::I64));
        }
        self.counter += 1;
        let tid = self
            .module
            .declare_function(&format!("thunk_{}", self.counter), Linkage::Local, &sig)
            .map_err(|e| Bail::Msg(e.to_string()))?;

        let mut ctx = Context::new();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(1, tid.as_u32());
        let mut fbc = FunctionBuilderContext::new();
        let cfg = self.module.target_config();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbc);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            let ps: Vec<ir::Value> = b.block_params(entry).to_vec();
            let (cptr, args, out) = (ps[0], ps[1], ps[2]);
            let mut call_args = vec![cptr];
            for (i, t) in tys.iter().enumerate() {
                let v = b.ins().load(t.clif(), MemFlagsData::trusted(), args, (8 * i) as i32);
                call_args.push(v);
            }
            let fr = self.module.declare_func_in_func(id, b.func);
            let inst = b.ins().call(fr, &call_args);
            let r = b.inst_results(inst)[0];
            b.ins().store(MemFlagsData::trusted(), r, out, 0);
            b.ins().return_(&[]);
            b.seal_all_blocks();
            b.finalize(cfg);
        }
        let _ = ret;
        self.define(tid, &mut ctx)?;
        self.module
            .finalize_definitions()
            .map_err(|e| Bail::Msg(format!("error finalizing: {:?}", e)))?;
        let addr = self.module.get_finalized_function(tid) as usize;
        if let Some(Spec::Done { thunk, .. }) = self.specs.get_mut(&key) {
            *thunk = Some(addr);
        }
        Ok(addr)
    }

    // ------------------------------------------------------------------
    // Loops
    // ------------------------------------------------------------------

    fn compile_loop(
        &mut self,
        kind: &LoopKind,
        env: &Rc<RefCell<Environment>>,
        key: &LoopKey,
    ) -> R<(usize, Vec<bool>)> {
        let slots = &key.slots;
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // ctx
        sig.params.push(AbiParam::new(types::I64)); // slots*
        sig.returns.push(AbiParam::new(types::I64));
        self.counter += 1;
        let id = self
            .module
            .declare_function(&format!("loop_{}", self.counter), Linkage::Local, &sig)
            .map_err(|e| Bail::Msg(e.to_string()))?;

        let mut ctx = Context::new();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(2, id.as_u32());
        let mut fbc = FunctionBuilderContext::new();
        let cfg = self.module.target_config();
        let assigned;
        {
            let b = FunctionBuilder::new(&mut ctx.func, &mut fbc);
            let mut g = Gen::new(self, b, Rc::clone(env), None, None);
            g.checked = key.checked;
            g.spec = key.spec.iter().cloned().collect();
            g.elem = key.elem;
            let slots_ptr = g.prologue_loop(slots)?;
            match kind {
                LoopKind::While { cond, body } => g.gen_while(cond, body)?,
                LoopKind::For { var, body, .. } => {
                    let n = slots.len();
                    let lo = g.b.ins().load(types::I64, MemFlagsData::trusted(), slots_ptr, (8 * n) as i32);
                    let hi = g.b.ins().load(types::I64, MemFlagsData::trusted(), slots_ptr, (8 * (n + 1)) as i32);
                    g.gen_for_range(var, lo, hi, body)?;
                }
                LoopKind::ForList { var, body, .. } => {
                    let n = slots.len();
                    let items = g.b.ins().load(types::I64, MemFlagsData::trusted(), slots_ptr, (8 * n) as i32);
                    let len = g.b.ins().load(types::I64, MemFlagsData::trusted(), slots_ptr, (8 * (n + 1)) as i32);
                    g.gen_for_list(var, items, len, body)?;
                }
            }
            g.epilogue_loop(slots_ptr, slots);
            assigned = g.slots_assigned.clone();
            g.finish(cfg);
        }
        order_branches(&mut ctx.func);
        self.inline_calls(&mut ctx, None)?;
        self.define(id, &mut ctx)?;
        self.module
            .finalize_definitions()
            .map_err(|e| Bail::Msg(format!("error finalizing: {:?}", e)))?;
        Ok((self.module.get_finalized_function(id) as usize, assigned))
    }
}

// ======================================================================
// Auxiliary syntactic analyses
// ======================================================================

/// Does a numeric JIT type satisfy a type annotation?
fn annotation_ok(ty: &TypeExpr, t: Ty) -> bool {
    matches!(annotation_check(ty, t), Ann::Ok)
}

/// What a type annotation asks of a value of a numeric JIT type.
enum Ann {
    /// Always satisfied.
    Ok,
    /// Never (or not decidable here): the interpreter runs it and reports.
    No,
    /// A sized integer (`int8`…): satisfied when the int is in `lo..=hi`.
    Range(i64, i64, String, ops::Sized),
}

fn annotation_check(ty: &TypeExpr, t: Ty) -> Ann {
    match ty {
        TypeExpr::Any => Ann::Ok,
        TypeExpr::Named(n) => match (n.as_str(), t, ops::Sized::from_name(n)) {
            ("int", Ty::Int, _) | ("float", Ty::Float | Ty::Int, _) | ("bool", Ty::Bool, _) => Ann::Ok,
            (_, Ty::Float | Ty::Int, Some(k)) if k.is_float() => Ann::Ok,
            (_, Ty::Int, Some(ops::Sized::I64)) => Ann::Ok,
            (_, Ty::Int, Some(k)) => match k.range() {
                Some((lo, hi)) => Ann::Range(lo, hi, n.clone(), k),
                None => Ann::No,
            },
            _ => Ann::No,
        },
        // numbers are never nil
        TypeExpr::Optional(inner) => annotation_check(inner, t),
        TypeExpr::Union(ts) if ts.iter().any(|x| annotation_ok(x, t)) => Ann::Ok,
        _ => Ann::No,
    }
}

/// Does the block always end with `return` (on every path)?
fn always_returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_always_returns)
}

fn stmt_always_returns(s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(_)) => true,
        Stmt::Block(b) => always_returns(b),
        Stmt::If { then_branch, else_branch: Some(e), .. } => {
            stmt_always_returns(then_branch) && stmt_always_returns(e)
        }
        _ => false,
    }
}

/// All mentioned identifiers (over-approximation of a loop's free
/// variables).
fn collect_names(out: &mut HashSet<String>, s: &Stmt) {
    fn ex(out: &mut HashSet<String>, e: &Expr) {
        match e {
            Expr::Ident(n) => {
                out.insert(n.text.clone());
            }
            Expr::List(v) => v.iter().for_each(|x| ex(out, x)),
            Expr::Map(v) => v.iter().for_each(|(_, x)| ex(out, x)),
            Expr::Unary { right, .. } => ex(out, right),
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
                ex(out, left);
                ex(out, right);
            }
            Expr::Range { start, end } => {
                ex(out, start);
                ex(out, end);
            }
            Expr::Index { target, index } => {
                ex(out, target);
                ex(out, index);
            }
            Expr::Access { target, .. } | Expr::OptAccess { target, .. } => ex(out, target),
            Expr::Template(parts) => parts.iter().for_each(|x| ex(out, x)),
            Expr::DynImport(spec) => ex(out, spec),
            Expr::OptChain(inner) => ex(out, inner),
            Expr::Call { callee, args } => {
                ex(out, callee);
                args.iter().for_each(|x| ex(out, x));
            }
            Expr::Assign { target, value, .. } => {
                ex(out, target);
                ex(out, value);
            }
            Expr::New { args, .. } => args.iter().for_each(|x| ex(out, x)),
            Expr::Fg { call } => ex(out, call),
            Expr::Chan { buffer } => {
                if let Some(b) = buffer {
                    ex(out, b)
                }
            }
            Expr::Recv { chan } => ex(out, chan),
            Expr::Match { subject, arms } => {
                ex(out, subject);
                for a in arms {
                    pat(out, &a.pattern);
                    ex(out, &a.body);
                }
            }
            Expr::Try { expr } => ex(out, expr),
            _ => {}
        }
    }
    fn pat(out: &mut HashSet<String>, p: &Pattern) {
        match p {
            Pattern::Literal(e) => ex(out, e),
            Pattern::IdentBind { guard: Some(g), .. } => ex(out, g),
            Pattern::Or(v) => v.iter().for_each(|x| pat(out, x)),
            _ => {}
        }
    }
    match s {
        Stmt::Let { init, .. } => ex(out, init),
        Stmt::CheckType { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::Expr(e) => ex(out, e),
        Stmt::Block(b) => b.iter().for_each(|x| collect_names(out, x)),
        Stmt::If { cond, then_branch, else_branch } => {
            ex(out, cond);
            collect_names(out, then_branch);
            if let Some(e) = else_branch {
                collect_names(out, e);
            }
        }
        Stmt::While { cond, body, .. } => {
            ex(out, cond);
            collect_names(out, body);
        }
        Stmt::For { iter, body, .. } => {
            ex(out, iter);
            collect_names(out, body);
        }
        Stmt::Return(Some(e)) => ex(out, e),
        Stmt::MatchStmt { subject, arms } => {
            ex(out, subject);
            for a in arms {
                pat(out, &a.pattern);
                ex(out, &a.body);
            }
        }
        _ => {}
    }
}

// ======================================================================
// Code generator
// ======================================================================

#[derive(Clone, Copy)]
struct Local {
    var: Variable,
    ty: Ty,
    mutable: bool,
    slot: Option<usize>,
}

enum Callee {
    User(Rc<FnValue>),
    Math(&'static str),
    Print,
    ToInt,
    ToFloat,
    /// `int8(x)` … `float32(x)`: the name and its sized type.
    Sized(&'static str, ops::Sized),
    /// A native function with a typed entry: called directly.
    Typed(String, super::value::TypedNative),
}

struct Gen<'a, 'f> {
    inner: &'a mut Inner,
    b: FunctionBuilder<'f>,
    /// Integer values known to lie in a range (`x % 7`, `x & 255`, and what
    /// is computed from them): arithmetic that cannot overflow needs no check.
    ranges: HashMap<ir::Value, (i64, i64)>,
    ctx_ptr: ir::Value,
    scopes: Vec<HashMap<String, Local>>,
    env: Rc<RefCell<Environment>>,
    /// (`continue` target, `break` target)
    loops: Vec<(ir::Block, ir::Block)>,
    ret: Option<Ty>,
    /// A sized return annotation: (lo, hi, error code) checked at each `return`.
    ret_range: Option<(i64, i64, i64)>,
    /// The function being compiled (for its error messages).
    fn_name: String,
    /// The code may run with a deadline: every loop iteration checks the limit.
    checked: bool,
    me: Option<FnKey>,
    err_blocks: HashMap<i64, ir::Block>,
    prop_block: Option<ir::Block>,
    /// The line being executed, kept in a register: it is only written to
    /// the context when an error leaves the native code (0 = before the
    /// first statement: the caller's line stands).
    line_var: Variable,
    helper_refs: HashMap<Helper, FuncRef>,
    dead: bool,
    slots_assigned: Vec<bool>,
    /// For loops: slot pointer and which ones to also save on error.
    wb: Option<(ir::Value, Vec<(usize, String)>)>,
    // ---- speculation (loops only) ----
    /// Speculated element type per list slot / value type per map slot.
    spec: HashMap<String, Ty>,
    /// Speculated element type of a `for x in list` loop.
    elem: Option<Ty>,
    /// The slot array (loops).
    slots_ptr: Option<ir::Value>,
    /// Mutable slots: (slot, its variable, its value at the start of the
    /// current iteration of the compiled loop) — restored on deopt.
    commits: Vec<(usize, Variable, Variable)>,
    /// The iteration the interpreter resumes at after a deopt.
    resume_var: Option<Variable>,
    deopt_block: Option<ir::Block>,
    /// The next loop compiled is the loop itself (not one nested in its body).
    top_loop: bool,
    /// A side effect of the current iteration was emitted (push, map set,
    /// print, a call): from here on no speculation may fail.
    effects: bool,
    /// Guards emitted so far.
    guards: usize,
}

impl<'a, 'f> Gen<'a, 'f> {
    fn new(
        inner: &'a mut Inner,
        mut b: FunctionBuilder<'f>,
        env: Rc<RefCell<Environment>>,
        ret: Option<Ty>,
        me: Option<FnKey>,
    ) -> Self {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let ctx_ptr = b.block_params(entry)[0];
        let line_var = b.declare_var(types::I64);
        let z = b.ins().iconst(types::I64, 0);
        b.def_var(line_var, z);
        Gen {
            inner,
            b,
            ctx_ptr,
            scopes: vec![HashMap::new()],
            env,
            loops: Vec::new(),
            ret,
            ret_range: None,
            fn_name: String::new(),
            checked: false,
            me,
            err_blocks: HashMap::new(),
            prop_block: None,
            line_var,
            helper_refs: HashMap::new(),
            dead: false,
            slots_assigned: Vec::new(),
            wb: None,
            spec: HashMap::new(),
            elem: None,
            slots_ptr: None,
            commits: Vec::new(),
            resume_var: None,
            deopt_block: None,
            top_loop: false,
            effects: false,
            guards: 0,
            ranges: HashMap::new(),
        }
    }

    // ---------- prologue / epilogue ----------

    fn prologue_fn(&mut self, params: &[String], tys: &[Ty]) -> R<()> {
        let entry = self.b.current_block().unwrap();
        let ps: Vec<ir::Value> = self.b.block_params(entry).to_vec();
        for (i, name) in params.iter().enumerate() {
            let ty = tys[i];
            let var = self.b.declare_var(ty.clif());
            self.b.def_var(var, ps[i + 1]);
            self.scopes[0].insert(name.clone(), Local { var, ty, mutable: true, slot: None });
        }
        // the stack pointer against the limit of this thread / task: one
        // load and one compare per call, nothing to undo on return
        self.limit_check();
        Ok(())
    }

    fn prologue_loop(&mut self, slots: &[(String, Ty, bool)]) -> R<ir::Value> {
        let entry = self.b.current_block().unwrap();
        let slots_ptr = self.b.block_params(entry)[1];
        self.slots_assigned = vec![false; slots.len()];
        for (i, (name, ty, mutable)) in slots.iter().enumerate() {
            let v = self.b.ins().load(ty.clif(), MemFlagsData::trusted(), slots_ptr, (8 * i) as i32);
            let var = self.b.declare_var(ty.clif());
            self.b.def_var(var, v);
            self.scopes[0].insert(
                name.clone(),
                Local { var, ty: *ty, mutable: *mutable, slot: Some(i) },
            );
            if *mutable {
                let c = self.b.declare_var(ty.clif());
                self.b.def_var(c, v);
                self.commits.push((i, var, c));
            }
        }
        let r = self.b.declare_var(types::I64);
        let z = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(r, z);
        self.resume_var = Some(r);
        self.slots_ptr = Some(slots_ptr);
        self.top_loop = true;
        Ok(slots_ptr)
    }

    /// Start of an iteration of the compiled loop: remembers the variables
    /// (a deopt restores them) and where the interpreter would resume.
    fn iteration_start(&mut self, idx: Option<ir::Value>) {
        if !self.top_loop {
            return;
        }
        self.top_loop = false;
        for (_, var, c) in self.commits.clone() {
            let v = self.b.use_var(var);
            self.b.def_var(c, v);
        }
        if let Some(r) = self.resume_var {
            let i = match idx {
                Some(i) => i,
                None => self.b.ins().iconst(types::I64, 0),
            };
            self.b.def_var(r, i);
        }
    }

    /// A nested loop's body ran side effects *and* guards: a guard of a later
    /// nested iteration would run after effects of an earlier one.
    fn nested_loop_check(&self, effects_before: bool, guards_before: usize) -> R<()> {
        if !effects_before && self.effects && self.guards > guards_before {
            return bail("speculation inside a nested loop with side effects");
        }
        Ok(())
    }

    /// Something observable happened in this iteration.
    fn effect(&mut self) {
        self.effects = true;
    }

    /// A helper that may fail its speculation is about to run: allowed only
    /// in a loop and before any side effect of the iteration.
    fn guard_ok(&self) -> R<()> {
        if self.resume_var.is_none() {
            return bail("speculation outside a compiled loop");
        }
        if self.effects {
            return bail("speculation after a side effect in the iteration");
        }
        Ok(())
    }

    /// After such a helper: leave the loop when it deopted.
    fn guard_branch(&mut self) {
        self.guards += 1;
        let blk = match self.deopt_block {
            Some(b) => b,
            None => {
                let b = self.b.create_block();
                self.b.set_cold_block(b);
                self.deopt_block = Some(b);
                b
            }
        };
        let d = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx_ptr, OFF_DEOPT);
        let cont = self.b.create_block();
        self.b.ins().brif(d, blk, &[], cont, &[]);
        self.b.switch_to_block(cont);
    }

    fn epilogue_loop(&mut self, slots_ptr: ir::Value, slots: &[(String, Ty, bool)]) {
        for (i, (name, _, _)) in slots.iter().enumerate() {
            if self.slots_assigned[i] {
                let l = self.scopes[0][name];
                let v = self.b.use_var(l.var);
                self.b.ins().store(MemFlagsData::trusted(), v, slots_ptr, (8 * i) as i32);
            }
        }
        let wb: Vec<(usize, String)> = slots
            .iter()
            .enumerate()
            .filter(|(i, _)| self.slots_assigned[*i])
            .map(|(i, (n, _, _))| (i, n.clone()))
            .collect();
        self.wb = Some((slots_ptr, wb));
        let z = self.b.ins().iconst(types::I64, 0);
        self.b.ins().return_(&[z]);
        self.dead = true;
    }

    /// Saves the modified loop variables into the slots (also on error).
    fn write_back(&mut self) {
        if let Some((ptr, list)) = self.wb.clone() {
            for (i, name) in list {
                let l = self.scopes[0][&name];
                let v = self.b.use_var(l.var);
                self.b.ins().store(MemFlagsData::trusted(), v, ptr, (8 * i) as i32);
            }
        }
    }

    fn zero_ret(&mut self) -> ir::Value {
        match self.ret {
            Some(Ty::Float) => self.b.ins().f64const(0.0),
            Some(Ty::Bool) | Some(Ty::Unit) => self.b.ins().iconst(types::I8, 0),
            _ => self.b.ins().iconst(types::I64, 0),
        }
    }

    fn finish(mut self, cfg: cranelift_codegen::isa::TargetFrontendConfig) {
        if !self.dead {
            self.b.ins().trap(TrapCode::INTEGER_OVERFLOW);
        }
        // error blocks (cold)
        let errs: Vec<(i64, ir::Block)> = self.err_blocks.iter().map(|(k, v)| (*k, *v)).collect();
        for (code, blk) in errs {
            self.b.switch_to_block(blk);
            let line = self.b.block_params(blk)[0];
            self.b.ins().store(MemFlagsData::trusted(), line, self.ctx_ptr, OFF_LINE);
            let c = self.b.ins().iconst(types::I64, code);
            self.b.ins().store(MemFlagsData::trusted(), c, self.ctx_ptr, OFF_ERR);
            self.write_back();
            let z = self.zero_ret();
            self.b.ins().return_(&[z]);
        }
        if let Some(blk) = self.prop_block {
            // an error of a callee: its line stands, unless it failed before
            // its first statement (or it is a helper, which records none)
            self.b.switch_to_block(blk);
            let mine = self.b.block_params(blk)[0];
            let theirs = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx_ptr, OFF_LINE);
            let line = self.b.ins().select(theirs, theirs, mine);
            self.b.ins().store(MemFlagsData::trusted(), line, self.ctx_ptr, OFF_LINE);
            self.write_back();
            let z = self.zero_ret();
            self.b.ins().return_(&[z]);
        }
        if let Some(blk) = self.deopt_block {
            // the variables as they were when the iteration started, and where to resume
            self.b.switch_to_block(blk);
            if let Some(ptr) = self.slots_ptr {
                for (i, _, c) in self.commits.clone() {
                    let v = self.b.use_var(c);
                    self.b.ins().store(MemFlagsData::trusted(), v, ptr, (8 * i) as i32);
                }
            }
            if let Some(r) = self.resume_var {
                let v = self.b.use_var(r);
                self.b.ins().store(MemFlagsData::trusted(), v, self.ctx_ptr, OFF_RESUME);
            }
            let one = self.b.ins().iconst(types::I64, 1);
            self.b.ins().return_(&[one]);
        }
        self.b.seal_all_blocks();
        self.b.finalize(cfg);
    }

    // ---------- errors ----------

    fn err_block(&mut self, code: i64) -> ir::Block {
        if let Some(b) = self.err_blocks.get(&code) {
            return *b;
        }
        let b = self.b.create_block();
        self.b.append_block_param(b, types::I64);
        self.b.set_cold_block(b);
        self.err_blocks.insert(code, b);
        b
    }

    /// The current line, as the argument of an error block.
    fn line_arg(&mut self) -> ir::BlockArg {
        ir::BlockArg::Value(self.b.use_var(self.line_var))
    }

    /// The stack pointer against the limit of this thread / task: a stack
    /// overflow, or a deadline that passed (the watchdog raised the limit).
    fn limit_check(&mut self) {
        let sp = self.b.ins().get_stack_pointer(types::I64);
        let at = self.b.ins().iconst(types::I64, self.inner.limit_addr as i64);
        // an atomic load: another thread (the watchdog) writes the limit, so
        // the optimizer must not reuse a value read earlier in the function
        // (a loop nested in a loop would otherwise never see the change)
        let lim = self.b.ins().atomic_load(types::I64, MemFlagsData::trusted(), at);
        let over = self.b.ins().icmp(IntCC::UnsignedLessThan, sp, lim);
        self.trap_if(over, ops::CODE_STACK);
    }

    /// Start of a loop iteration: where code with a deadline can be stopped.
    fn cancel_point(&mut self) {
        if self.checked {
            self.limit_check();
        }
    }

    /// Raises the error `code` unless `lo <= v <= hi` (one unsigned compare).
    fn range_guard(&mut self, v: ir::Value, lo: i64, hi: i64, code: i64) {
        let d = if lo == 0 { v } else { self.b.ins().iadd_imm_s(v, lo.wrapping_neg()) };
        let span = self.b.ins().iconst(types::I64, hi.wrapping_sub(lo));
        let out = self.b.ins().icmp(IntCC::UnsignedGreaterThan, d, span);
        self.trap_if(out, code);
    }

    /// The value of an integer constant (`iconst`), if `v` is one.
    fn const_of(&self, v: ir::Value) -> Option<i64> {
        match self.b.func.dfg.value_def(v) {
            ir::ValueDef::Result(inst, _) => match self.b.func.dfg.insts[inst] {
                ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } => Some(imm.bits()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The value of a float constant (`f64const`), if `v` is one.
    fn float_const(&self, v: ir::Value) -> Option<f64> {
        match self.b.func.dfg.value_def(v) {
            ir::ValueDef::Result(inst, _) => match self.b.func.dfg.insts[inst] {
                ir::InstructionData::UnaryIeee64 { opcode: ir::Opcode::F64const, imm } => Some(imm.as_f64()),
                _ => None,
            },
            _ => None,
        }
    }

    /// `a / d` or `a % d` (truncated, like the interpreter) for a constant
    /// `d` other than 0, -1 and `i64::MIN`: no division instruction and
    /// nothing to check (neither a zero divisor nor an overflow can happen).
    fn div_const(&mut self, div: bool, a: ir::Value, d: i64) -> ir::Value {
        if d == 1 {
            return if div { a } else { self.b.ins().iconst(types::I64, 0) };
        }
        let ad = d.unsigned_abs();
        let q = if ad.is_power_of_two() {
            let k = ad.trailing_zeros() as i64;
            // rounds toward zero: negative values get 2^k - 1 added first
            let sign = self.b.ins().sshr_imm_s(a, 63);
            let bias = self.b.ins().ushr_imm_s(sign, 64 - k);
            let t = self.b.ins().iadd(a, bias);
            if !div {
                let low = self.b.ins().band_imm_s(t, (ad - 1) as i64);
                return self.b.ins().isub(low, bias);
            }
            let q = self.b.ins().sshr_imm_s(t, k);
            if d < 0 { self.b.ins().ineg(q) } else { q }
        } else {
            let (m, s) = div_magic(d);
            let mc = self.b.ins().iconst(types::I64, m);
            let mut q = self.b.ins().smulhi(a, mc);
            if d > 0 && m < 0 {
                q = self.b.ins().iadd(q, a);
            } else if d < 0 && m > 0 {
                q = self.b.ins().isub(q, a);
            }
            if s > 0 {
                q = self.b.ins().sshr_imm_s(q, s as i64);
            }
            let sign = self.b.ins().ushr_imm_s(q, 63);
            self.b.ins().iadd(q, sign)
        };
        if div {
            return q;
        }
        let qd = self.b.ins().imul_imm_s(q, d);
        self.b.ins().isub(a, qd)
    }

    fn trap_if(&mut self, cond: ir::Value, code: i64) {
        let eb = self.err_block(code);
        let cont = self.b.create_block();
        let line = self.line_arg();
        self.b.ins().brif(cond, eb, &[line], cont, &[]);
        self.b.switch_to_block(cont);
    }

    fn check_err(&mut self) {
        let pb = match self.prop_block {
            Some(b) => b,
            None => {
                let b = self.b.create_block();
                self.b.append_block_param(b, types::I64);
                self.b.set_cold_block(b);
                self.prop_block = Some(b);
                b
            }
        };
        let e = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx_ptr, OFF_ERR);
        let cont = self.b.create_block();
        let line = self.line_arg();
        self.b.ins().brif(e, pb, &[line], cont, &[]);
        self.b.switch_to_block(cont);
    }

    fn helper(&mut self, h: Helper) -> FuncRef {
        if let Some(r) = self.helper_refs.get(&h) {
            return *r;
        }
        let id = self.inner.helper_ids[&h];
        let r = self.inner.module.declare_func_in_func(id, self.b.func);
        self.helper_refs.insert(h, r);
        r
    }

    // ---------- scopes ----------

    fn lookup_local(&self, name: &str) -> Option<Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn declare_local(&mut self, name: &str, ty: Ty, mutable: bool, v: ir::Value) -> R<()> {
        if ty == Ty::Unit {
            return bail("a nil value cannot be stored in a variable");
        }
        let var = self.b.declare_var(ty.clif());
        self.b.def_var(var, v);
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), Local { var, ty, mutable, slot: None });
        Ok(())
    }

    // ---------- statements ----------

    fn revive(&mut self) {
        if self.dead {
            let nb = self.b.create_block();
            self.b.switch_to_block(nb);
            self.dead = false;
        }
    }

    fn block_stmts(&mut self, stmts: &[Stmt]) -> R<()> {
        self.scopes.push(HashMap::new());
        let mut r = Ok(());
        for s in stmts {
            r = self.stmt(s);
            if r.is_err() {
                break;
            }
        }
        self.scopes.pop();
        r
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        self.revive();
        match s {
            Stmt::Line(n) => {
                let l = self.b.ins().iconst(types::I64, *n as i64);
                self.b.def_var(self.line_var, l);
                Ok(())
            }
            Stmt::Let { name, mutable, init, ty } => {
                let (v, t) = self.expr(init)?;
                if let Some(ty) = ty {
                    match annotation_check(ty, t) {
                        Ann::Ok => {}
                        Ann::No => return bail(format!("'{}' does not match its annotation {}", name, ty)),
                        Ann::Range(lo, hi, n, k) => {
                            let msg = format!(
                                "type mismatch: variable '{}' is declared as {}, got {}",
                                name,
                                ty,
                                ops::out_of_range(&n, k)
                            );
                            self.range_guard(v, lo, hi, ops::custom_code(&msg));
                        }
                    }
                }
                self.declare_local(name, t, *mutable, v)
            }
            // Specializations are per argument type, so a matching
            // annotation costs nothing; otherwise the interpreter reports it.
            Stmt::CheckType { name, ty } => {
                let (v, t) = self.ident(name)?;
                match annotation_check(ty, t) {
                    Ann::Ok => {}
                    Ann::No => return bail(format!("parameter '{}' does not match its annotation {}", name, ty)),
                    Ann::Range(lo, hi, n, k) => {
                        let msg = format!(
                            "type mismatch: parameter '{}' expects {}, got {}",
                            name,
                            ty,
                            ops::out_of_range(&n, k)
                        );
                        self.range_guard(v, lo, hi, ops::custom_code(&msg));
                    }
                }
                Ok(())
            }
            Stmt::ReturnType(ty) => match self.ret.map(|r| annotation_check(ty, r)) {
                Some(Ann::Ok) => Ok(()),
                Some(Ann::Range(lo, hi, n, k)) => {
                    let msg = format!(
                        "type mismatch: function '{}' must return {}, got {}",
                        self.fn_name,
                        ty,
                        ops::out_of_range(&n, k)
                    );
                    self.ret_range = Some((lo, hi, ops::custom_code(&msg)));
                    Ok(())
                }
                _ => bail(format!("return type does not match the annotation {}", ty)),
            },
            Stmt::Expr(e) => {
                if self.map_set(e)? || self.list_push(e)? {
                    return Ok(());
                }
                self.expr(e)?;
                Ok(())
            }
            Stmt::Block(b) => self.block_stmts(b),
            Stmt::If { cond, then_branch, else_branch } => {
                let then_b = self.b.create_block();
                let merge = self.b.create_block();
                let else_b = if else_branch.is_some() { self.b.create_block() } else { merge };
                self.branch_on(cond, then_b, else_b)?;

                self.b.switch_to_block(then_b);
                self.dead = false;
                self.stmt(then_branch)?;
                if !self.dead {
                    self.b.ins().jump(merge, &[]);
                }
                if let Some(e) = else_branch {
                    self.b.switch_to_block(else_b);
                    self.dead = false;
                    self.stmt(e)?;
                    if !self.dead {
                        self.b.ins().jump(merge, &[]);
                    }
                }
                self.b.switch_to_block(merge);
                self.dead = false;
                Ok(())
            }
            Stmt::While { cond, body, .. } => self.gen_while(cond, body),
            Stmt::For { var, var2, iter, body, .. } => {
                if var2.is_some() {
                    return bail("'for a, b in' not supported");
                }
                let Expr::Range { start, end } = iter else {
                    return bail("'for' is only supported over a..b ranges");
                };
                let (lo, lt) = self.expr(start)?;
                let (hi, ht) = self.expr(end)?;
                if lt != Ty::Int || ht != Ty::Int {
                    return bail("range bounds must be int");
                }
                self.gen_for_range(var, lo, hi, body)
            }
            Stmt::Break => match self.loops.last() {
                Some(&(_, brk)) => {
                    self.b.ins().jump(brk, &[]);
                    self.dead = true;
                    Ok(())
                }
                None => bail("'break' outside a loop"),
            },
            Stmt::Continue => match self.loops.last() {
                Some(&(cont, _)) => {
                    self.b.ins().jump(cont, &[]);
                    self.dead = true;
                    Ok(())
                }
                None => bail("'continue' outside a loop"),
            },
            Stmt::Return(e) => {
                let Some(want) = self.ret else {
                    return bail("'return' inside a compiled loop");
                };
                let Some(e) = e else {
                    return bail("'return' without a value");
                };
                let (v, t) = self.expr(e)?;
                if t != want {
                    return Err(Bail::Ret(t));
                }
                if let Some((lo, hi, code)) = self.ret_range {
                    self.range_guard(v, lo, hi, code);
                }
                self.b.ins().return_(&[v]);
                self.dead = true;
                Ok(())
            }
            Stmt::MatchStmt { subject, arms } => {
                self.gen_match(subject, arms, false)?;
                Ok(())
            }
            other => bail(format!("unsupported statement: {}", stmt_name(other))),
        }
    }

    /// Jumps to `t` when the condition holds and to `f` when it does not.
    /// `&&` and `||` become the branches themselves (as the interpreter
    /// evaluates them: the right side only when needed) instead of a boolean
    /// that is built and then tested.
    fn branch_on(&mut self, e: &Expr, t: ir::Block, f: ir::Block) -> R<()> {
        match e {
            Expr::Logical { left, op: LogicalOp::And, right } => {
                let rhs = self.b.create_block();
                self.branch_on(left, rhs, f)?;
                self.b.switch_to_block(rhs);
                self.branch_on(right, t, f)
            }
            Expr::Logical { left, op: LogicalOp::Or, right } => {
                let rhs = self.b.create_block();
                self.branch_on(left, t, rhs)?;
                self.b.switch_to_block(rhs);
                self.branch_on(right, t, f)
            }
            _ => {
                let c = self.cond(e)?;
                self.b.ins().brif(c, t, &[], f, &[]);
                Ok(())
            }
        }
    }

    /// The range an integer value is known to lie in, if any.
    fn range_of(&self, v: ir::Value) -> Option<(i64, i64)> {
        self.const_of(v).map(|c| (c, c)).or_else(|| self.ranges.get(&v).copied())
    }

    fn cond(&mut self, e: &Expr) -> R<ir::Value> {
        let (v, t) = self.expr(e)?;
        if t != Ty::Bool {
            return bail(format!("non-boolean condition ({:?})", t));
        }
        Ok(v)
    }

    fn gen_while(&mut self, cond: &Expr, body: &Stmt) -> R<()> {
        let header = self.b.create_block();
        let body_b = self.b.create_block();
        let exit = self.b.create_block();
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        self.dead = false;
        self.branch_on(cond, body_b, exit)?;

        self.b.switch_to_block(body_b);
        self.cancel_point();
        let nested = !self.top_loop;
        let (eb, gb) = (self.effects, self.guards);
        self.iteration_start(None);
        self.loops.push((header, exit));
        let r = self.stmt(body);
        self.loops.pop();
        r?;
        if nested {
            self.nested_loop_check(eb, gb)?;
        }
        if !self.dead {
            self.b.ins().jump(header, &[]);
        }
        self.b.switch_to_block(exit);
        self.dead = false;
        Ok(())
    }

    fn gen_for_range(&mut self, var: &str, lo: ir::Value, hi: ir::Value, body: &Stmt) -> R<()> {
        let idx = self.b.declare_var(types::I64);
        self.b.def_var(idx, lo);
        let header = self.b.create_block();
        let body_b = self.b.create_block();
        let incr = self.b.create_block();
        let exit = self.b.create_block();
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        let i = self.b.use_var(idx);
        let c = self.b.ins().icmp(IntCC::SignedLessThan, i, hi);
        self.b.ins().brif(c, body_b, &[], exit, &[]);

        self.b.switch_to_block(body_b);
        self.dead = false;
        self.cancel_point();
        self.scopes.push(HashMap::new());
        let cur = self.b.use_var(idx);
        if let (Some(first), Some(end)) = (self.const_of(lo), self.const_of(hi)) {
            // inside the body, `first <= var < end`
            if first < end {
                self.ranges.insert(cur, (first, end - 1));
            }
        }
        let nested = !self.top_loop;
        let (eb, gb) = (self.effects, self.guards);
        self.iteration_start(Some(cur));
        self.declare_local(var, Ty::Int, false, cur)?;
        self.loops.push((incr, exit));
        let r = self.stmt(body);
        self.loops.pop();
        self.scopes.pop();
        r?;
        if nested {
            self.nested_loop_check(eb, gb)?;
        }
        if !self.dead {
            self.b.ins().jump(incr, &[]);
        }

        self.b.switch_to_block(incr);
        let i = self.b.use_var(idx);
        let n = self.b.ins().iadd_imm_s(i, 1);
        self.b.def_var(idx, n);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.dead = false;
        Ok(())
    }

    /// `for var in list` over the snapshot `items[0..len]`; each element is
    /// checked against the speculated type (a mismatch deopts).
    fn gen_for_list(&mut self, var: &str, items: ir::Value, len: ir::Value, body: &Stmt) -> R<()> {
        let Some(elem) = self.elem else {
            return bail("no speculated element type");
        };
        let idx = self.b.declare_var(types::I64);
        let z = self.b.ins().iconst(types::I64, 0);
        self.b.def_var(idx, z);
        let header = self.b.create_block();
        let body_b = self.b.create_block();
        let incr = self.b.create_block();
        let exit = self.b.create_block();
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        let i = self.b.use_var(idx);
        let c = self.b.ins().icmp(IntCC::SignedLessThan, i, len);
        self.b.ins().brif(c, body_b, &[], exit, &[]);

        self.b.switch_to_block(body_b);
        self.dead = false;
        self.cancel_point();
        self.scopes.push(HashMap::new());
        let cur = self.b.use_var(idx);
        let nested = !self.top_loop;
        let (eb, gb) = (self.effects, self.guards);
        self.iteration_start(Some(cur));
        let h = match elem {
            Ty::Int => Helper::ElemI64,
            Ty::Float => Helper::ElemF64,
            Ty::Bool => Helper::ElemBool,
            Ty::Str => Helper::ElemStr,
            _ => return bail("element type the JIT does not compile"),
        };
        self.guard_ok()?;
        let fr = self.helper(h);
        let call = self.b.ins().call(fr, &[self.ctx_ptr, items, cur]);
        let mut v = self.b.inst_results(call)[0];
        self.guard_branch();
        if elem == Ty::Bool {
            v = self.b.ins().ireduce(types::I8, v);
        }
        self.declare_local(var, elem, false, v)?;
        self.loops.push((incr, exit));
        let r = self.stmt(body);
        self.loops.pop();
        self.scopes.pop();
        r?;
        if nested {
            self.nested_loop_check(eb, gb)?;
        }
        if !self.dead {
            self.b.ins().jump(incr, &[]);
        }

        self.b.switch_to_block(incr);
        let i = self.b.use_var(idx);
        let n = self.b.ins().iadd_imm_s(i, 1);
        self.b.def_var(idx, n);
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(exit);
        self.dead = false;
        Ok(())
    }

    // ---------- match ----------

    /// Compares `a == b` with the semantics of `values_equal`.
    fn equality(&mut self, a: ir::Value, at: Ty, b: ir::Value, bt: Ty) -> R<ir::Value> {
        Ok(match (at, bt) {
            (Ty::Int, Ty::Int) | (Ty::Bool, Ty::Bool) => self.b.ins().icmp(IntCC::Equal, a, b),
            (Ty::Float, Ty::Float) => self.b.ins().fcmp(FloatCC::Equal, a, b),
            (Ty::Int, Ty::Float) => {
                let a = self.b.ins().fcvt_from_sint(types::F64, a);
                self.b.ins().fcmp(FloatCC::Equal, a, b)
            }
            (Ty::Float, Ty::Int) => {
                let b = self.b.ins().fcvt_from_sint(types::F64, b);
                self.b.ins().fcmp(FloatCC::Equal, a, b)
            }
            (Ty::Unit, _) | (_, Ty::Unit) => return bail("comparison with nil"),
            (Ty::Str | Ty::List | Ty::Map, _) | (_, Ty::Str | Ty::List | Ty::Map) => {
                return bail("comparison of texts, lists or maps is not compiled")
            }
            // different types (bool vs number): never equal
            _ => self.b.ins().iconst(types::I8, 0),
        })
    }

    fn gen_match(&mut self, subject: &Expr, arms: &[MatchArm], want_value: bool) -> R<(ir::Value, Ty)> {
        let (sv, st) = self.expr(subject)?;
        if matches!(st, Ty::Str | Ty::List | Ty::Map) {
            return bail("match on a text, list or map is not compiled");
        }
        let done = self.b.create_block();
        let mut result: Option<(Variable, Ty)> = None;

        for arm in arms {
            let next = self.b.create_block();
            let body_b = self.b.create_block();
            let mut binds: Vec<(String, Local)> = Vec::new();

            match &arm.pattern {
                Pattern::Wildcard => {
                    self.b.ins().jump(body_b, &[]);
                }
                Pattern::Ident(name) => {
                    let var = self.b.declare_var(st.clif());
                    self.b.def_var(var, sv);
                    binds.push((name.clone(), Local { var, ty: st, mutable: false, slot: None }));
                    self.b.ins().jump(body_b, &[]);
                }
                Pattern::IdentBind { name, guard } => {
                    let var = self.b.declare_var(st.clif());
                    self.b.def_var(var, sv);
                    let local = Local { var, ty: st, mutable: false, slot: None };
                    match guard {
                        None => {
                            self.b.ins().jump(body_b, &[]);
                        }
                        Some(g) => {
                            self.scopes.push(HashMap::new());
                            self.scopes.last_mut().unwrap().insert(name.clone(), local);
                            let c = self.cond(g);
                            self.scopes.pop();
                            let c = c?;
                            self.b.ins().brif(c, body_b, &[], next, &[]);
                        }
                    }
                    binds.push((name.clone(), local));
                }
                Pattern::Literal(e) => {
                    let (pv, pt) = self.expr(e)?;
                    let eq = self.equality(sv, st, pv, pt)?;
                    self.b.ins().brif(eq, body_b, &[], next, &[]);
                }
                Pattern::Or(pats) => {
                    for p in pats {
                        let Pattern::Literal(e) = p else {
                            if matches!(p, Pattern::Wildcard) {
                                self.b.ins().jump(body_b, &[]);
                                let nb = self.b.create_block();
                                self.b.switch_to_block(nb);
                                continue;
                            }
                            return bail("'|' patterns only support literals");
                        };
                        let (pv, pt) = self.expr(e)?;
                        let eq = self.equality(sv, st, pv, pt)?;
                        let nb = self.b.create_block();
                        self.b.ins().brif(eq, body_b, &[], nb, &[]);
                        self.b.switch_to_block(nb);
                    }
                    self.b.ins().jump(next, &[]);
                }
            }

            self.b.switch_to_block(body_b);
            self.scopes.push(HashMap::new());
            for (n, l) in binds {
                self.scopes.last_mut().unwrap().insert(n, l);
            }
            let body = self.expr(&arm.body);
            self.scopes.pop();
            let (bv, bt) = body?;
            if want_value {
                match result {
                    None => {
                        if bt == Ty::Unit {
                            return bail("match arm returns nil");
                        }
                        let var = self.b.declare_var(bt.clif());
                        result = Some((var, bt));
                    }
                    Some((_, rt)) if rt != bt => return bail("match arms with different types"),
                    _ => {}
                }
                self.b.def_var(result.unwrap().0, bv);
            }
            self.b.ins().jump(done, &[]);
            self.b.switch_to_block(next);
        }

        // no arm matched
        let eb = self.err_block(ops::CODE_MATCH);
        let line = self.line_arg();
        self.b.ins().jump(eb, &[line]);
        self.b.switch_to_block(done);
        self.dead = false;
        if want_value {
            match result {
                Some((var, ty)) => {
                    let v = self.b.use_var(var);
                    Ok((v, ty))
                }
                None => bail("match without arms"),
            }
        } else {
            let z = self.b.ins().iconst(types::I8, 0);
            Ok((z, Ty::Unit))
        }
    }

    // ---------- expressions ----------

    fn to_f64(&mut self, v: ir::Value) -> ir::Value {
        self.b.ins().fcvt_from_sint(types::F64, v)
    }

    fn expr(&mut self, e: &Expr) -> R<(ir::Value, Ty)> {
        match e {
            Expr::Int(n) => Ok((self.b.ins().iconst(types::I64, *n), Ty::Int)),
            Expr::Float(f) => Ok((self.b.ins().f64const(*f), Ty::Float)),
            Expr::Bool(v) => Ok((self.b.ins().iconst(types::I8, *v as i64), Ty::Bool)),
            Expr::Ident(name) => self.ident(name),

            Expr::Unary { op, right } => {
                let (v, t) = self.expr(right)?;
                match (op, t) {
                    (UnaryOp::Neg, Ty::Int) => {
                        let z = self.b.ins().iconst(types::I64, 0);
                        let (r, of) = self.b.ins().ssub_overflow(z, v);
                        self.trap_if(of, ops::CODE_OVERFLOW);
                        Ok((r, Ty::Int))
                    }
                    (UnaryOp::Neg, Ty::Float) => Ok((self.b.ins().fneg(v), Ty::Float)),
                    (UnaryOp::Not, Ty::Bool) => Ok((self.b.ins().bxor_imm_s(v, 1), Ty::Bool)),
                    (UnaryOp::BitNot, Ty::Int) => Ok((self.b.ins().bnot(v), Ty::Int)),
                    // int/float are always truthy (only false/nil are falsy)
                    (UnaryOp::Not, Ty::Int) | (UnaryOp::Not, Ty::Float) => {
                        Ok((self.b.ins().iconst(types::I8, 0), Ty::Bool))
                    }
                    _ => bail("unary operator not supported for this type"),
                }
            }

            Expr::Binary { left, op, right } => {
                let (l, lt) = self.expr(left)?;
                let (r, rt) = self.expr(right)?;
                self.binop(*op, l, lt, r, rt)
            }

            Expr::Logical { left, op, right } => match op {
                LogicalOp::Coalesce => {
                    // numeric values are never nil
                    let (l, lt) = self.expr(left)?;
                    if lt == Ty::Unit {
                        return bail("'??' on nil");
                    }
                    Ok((l, lt))
                }
                LogicalOp::And | LogicalOp::Or => {
                    let (l, lt) = self.expr(left)?;
                    if lt != Ty::Bool {
                        return bail("'&&'/'||' require boolean operands");
                    }
                    let res = self.b.declare_var(types::I8);
                    self.b.def_var(res, l);
                    let rhs_b = self.b.create_block();
                    let done = self.b.create_block();
                    if *op == LogicalOp::And {
                        self.b.ins().brif(l, rhs_b, &[], done, &[]);
                    } else {
                        self.b.ins().brif(l, done, &[], rhs_b, &[]);
                    }
                    self.b.switch_to_block(rhs_b);
                    let (r, rt) = self.expr(right)?;
                    if rt != Ty::Bool {
                        return bail("'&&'/'||' require boolean operands");
                    }
                    self.b.def_var(res, r);
                    self.b.ins().jump(done, &[]);
                    self.b.switch_to_block(done);
                    Ok((self.b.use_var(res), Ty::Bool))
                }
            },

            Expr::Assign { target, op, value } => {
                let Expr::Ident(name) = &**target else {
                    return bail("assignment to a field/index is not supported");
                };
                let Some(local) = self.lookup_local(name) else {
                    return bail(format!("assignment to non-local variable '{}'", name));
                };
                if !local.mutable {
                    return bail(format!("assignment to const '{}'", name));
                }
                let cur = self.b.use_var(local.var);
                let (rv, rt) = self.expr(value)?;
                let (nv, nt) = match op {
                    None => (rv, rt),
                    Some(bop) => self.binop(*bop, cur, local.ty, rv, rt)?,
                };
                if nt != local.ty {
                    return bail(format!("variable '{}' would change type", name));
                }
                self.b.def_var(local.var, nv);
                if let Some(i) = local.slot {
                    self.slots_assigned[i] = true;
                }
                Ok((nv, nt))
            }

            Expr::Access { target, field } => {
                // module constants: math.PI
                if let Expr::Ident(m) = &**target {
                    if self.lookup_local(m).is_none() {
                        let b = self.env.borrow().lookup(m);
                        if let Some(b) = b {
                            if let (Value::Module(mv), true, true) = (&b.value, b.stable, !b.mutable) {
                                let v = mv.exports.borrow().get(field).cloned();
                                if let Some(v) = v {
                                    return self.const_value(&v);
                                }
                            }
                        }
                    }
                }
                bail("field access not supported")
            }

            Expr::Call { callee, args } => self.call(callee, args),

            Expr::Index { target, index } => self.index_read(target, index),

            Expr::Match { subject, arms } => self.gen_match(subject, arms, true),

            other => bail(format!("unsupported expression: {}", expr_name(other))),
        }
    }

    fn const_value(&mut self, v: &Value) -> R<(ir::Value, Ty)> {
        match v {
            Value::Int(n) => Ok((self.b.ins().iconst(types::I64, *n), Ty::Int)),
            Value::Float(f) => Ok((self.b.ins().f64const(*f), Ty::Float)),
            Value::Bool(x) => Ok((self.b.ins().iconst(types::I8, *x as i64), Ty::Bool)),
            _ => bail("non-numeric constant"),
        }
    }

    /// `xs.push(a, b…)` as a statement, `xs` a list loop variable and the
    /// arguments numbers: calls the runtime per value. `false` = not this form.
    fn list_push(&mut self, e: &Expr) -> R<bool> {
        let Expr::Call { callee, args } = e else { return Ok(false) };
        let Expr::Access { target, field } = &**callee else { return Ok(false) };
        let Expr::Ident(name) = &**target else { return Ok(false) };
        if field != "push" {
            return Ok(false);
        }
        let Some(l) = self.lookup_local(name) else { return Ok(false) };
        if l.ty != Ty::List {
            return Ok(false);
        }
        let list = self.b.use_var(l.var);
        for a in args {
            if matches!(a, Expr::List(_) | Expr::Map(_)) {
                let v = self.value_box(a)?;
                let fr = self.helper(Helper::ListPushV);
                self.b.ins().call(fr, &[list, v]);
                continue;
            }
            if self.is_stringish(a) {
                let fr = self.helper(Helper::SbNew);
                let call = self.b.ins().call(fr, &[]);
                let b = self.b.inst_results(call)[0];
                self.str_parts(a, b)?;
                let fr = self.helper(Helper::ListPushSb);
                self.b.ins().call(fr, &[list, b]);
                continue;
            }
            let (v, t) = self.expr(a)?;
            let h = match t {
                Ty::Int => Helper::ListPushI64,
                Ty::Float => Helper::ListPushF64,
                Ty::Bool => {
                    let v = self.b.ins().uextend(types::I64, v);
                    let fr = self.helper(Helper::ListPushBool);
                    self.b.ins().call(fr, &[list, v]);
                    continue;
                }
                Ty::Unit | Ty::List | Ty::Map | Ty::Str => return bail("push of a value the JIT does not compile"),
            };
            let fr = self.helper(h);
            self.b.ins().call(fr, &[list, v]);
        }
        self.effect();
        Ok(true)
    }

    /// `m[key] = value` as a statement, `m` a map loop variable and the key
    /// a text: calls the runtime. `false` = not this form.
    fn map_set(&mut self, e: &Expr) -> R<bool> {
        let Expr::Assign { target, op, value } = e else { return Ok(false) };
        let Expr::Index { target: t, index } = &**target else { return Ok(false) };
        let Expr::Ident(name) = &**t else { return Ok(false) };
        let Some(l) = self.lookup_local(name) else { return Ok(false) };
        if l.ty != Ty::Map {
            return Ok(false);
        }
        if op.is_some() {
            return bail("compound assignment to a map entry");
        }
        let map = self.b.use_var(l.var);
        // the value first (its guards may deopt; the key builder would leak)
        enum V {
            Boxed(ir::Value),
            Text(ir::Value),
            Num(ir::Value, Ty),
        }
        let val = if matches!(&**value, Expr::List(_) | Expr::Map(_)) {
            V::Boxed(self.value_box(value)?)
        } else if self.is_stringish(value) {
            let b = self.new_builder();
            self.str_parts(value, b)?;
            V::Text(b)
        } else {
            let (v, t) = self.expr(value)?;
            V::Num(v, t)
        };
        let key = self.key_builder(index)?;
        match val {
            V::Boxed(v) => {
                let fr = self.helper(Helper::MapSetV);
                self.b.ins().call(fr, &[map, key, v]);
            }
            V::Text(b) => {
                let fr = self.helper(Helper::MapSetSb);
                self.b.ins().call(fr, &[map, key, b]);
            }
            V::Num(v, t) => {
                let (h, v) = match t {
                    Ty::Int => (Helper::MapSetI64, v),
                    Ty::Float => (Helper::MapSetF64, v),
                    Ty::Bool => (Helper::MapSetBool, self.b.ins().uextend(types::I64, v)),
                    _ => return bail("map value the JIT does not compile"),
                };
                let fr = self.helper(h);
                self.b.ins().call(fr, &[map, key, v]);
            }
        }
        self.effect();
        Ok(true)
    }

    fn new_builder(&mut self) -> ir::Value {
        let fr = self.helper(Helper::SbNew);
        let call = self.b.ins().call(fr, &[]);
        self.b.inst_results(call)[0]
    }

    /// A map key as a text builder (consumed by the map helpers).
    fn key_builder(&mut self, e: &Expr) -> R<ir::Value> {
        if !self.is_stringish(e) {
            return bail("map key is not a text the JIT builds");
        }
        let b = self.new_builder();
        self.str_parts(e, b)?;
        Ok(b)
    }

    /// `xs[i]` / `m[k]` on a loop variable, with the speculated type of its
    /// elements; the helper deopts when the value is not of that type.
    fn index_read(&mut self, target: &Expr, index: &Expr) -> R<(ir::Value, Ty)> {
        let Expr::Ident(name) = target else {
            return bail("index of an expression is not supported");
        };
        let Some(l) = self.lookup_local(name) else {
            return bail("index of a non-local value is not supported");
        };
        let want = match self.spec.get(name.text.as_str()) {
            Some(t) => *t,
            None => return bail(format!("no speculated element type for '{}'", name)),
        };
        let (h, args) = match l.ty {
            Ty::List => {
                let (i, it) = self.expr(index)?;
                if it != Ty::Int {
                    return bail("list index must be an int");
                }
                let list = self.b.use_var(l.var);
                let h = match want {
                    Ty::Int => Helper::ListGetI64,
                    Ty::Float => Helper::ListGetF64,
                    Ty::Bool => Helper::ListGetBool,
                    _ => return bail("element type the JIT does not compile"),
                };
                (h, vec![self.ctx_ptr, list, i])
            }
            Ty::Map => {
                let key = self.key_builder(index)?;
                let map = self.b.use_var(l.var);
                let h = match want {
                    Ty::Int => Helper::MapGetI64,
                    Ty::Float => Helper::MapGetF64,
                    Ty::Bool => Helper::MapGetBool,
                    _ => return bail("value type the JIT does not compile"),
                };
                (h, vec![self.ctx_ptr, map, key])
            }
            _ => return bail("index of a value the JIT does not compile"),
        };
        self.guard_ok()?;
        let fr = self.helper(h);
        let call = self.b.ins().call(fr, &args);
        let mut v = self.b.inst_results(call)[0];
        self.guard_branch();
        if want == Ty::Bool {
            v = self.b.ins().ireduce(types::I8, v);
        }
        Ok((v, want))
    }

    /// `xs.len()`, `m.len()`, `s.len()` on a loop variable.
    fn local_method(&mut self, l: Local, name: &str, method: &str, args: &[Expr]) -> R<(ir::Value, Ty)> {
        let h = match (l.ty, method, args.len()) {
            (Ty::List, "len", 0) => Helper::ListLen,
            (Ty::Map, "len", 0) => Helper::MapLen,
            (Ty::Str, "len", 0) => Helper::StrLen,
            _ => return bail(format!("'{}.{}' on a loop variable is not compiled", name, method)),
        };
        let v = self.b.use_var(l.var);
        let fr = self.helper(h);
        let call = self.b.ins().call(fr, &[v]);
        Ok((self.b.inst_results(call)[0], Ty::Int))
    }

    /// Does `e` produce text (so `push` must build a string)? Literals,
    /// templates, `str(…)` and `+` with any of those.
    fn is_stringish(&self, e: &Expr) -> bool {
        match e {
            Expr::Str(_) | Expr::Template(_) => true,
            Expr::Ident(n) => matches!(self.lookup_local(n), Some(l) if l.ty == Ty::Str),
            Expr::Call { callee, .. } => self.is_str_builtin(callee),
            Expr::Binary { op: BinaryOp::Add, left, right } => self.is_stringish(left) || self.is_stringish(right),
            _ => false,
        }
    }

    /// `str` here is the global built-in (not a local or a redefinition).
    fn is_str_builtin(&self, callee: &Expr) -> bool {
        let Expr::Ident(n) = callee else { return false };
        if n != "str" || self.lookup_local(n).is_some() {
            return false;
        }
        matches!(
            self.env.borrow().lookup(n),
            Some(b) if b.stable && !b.mutable && matches!(&b.value, Value::NativeFn(f) if f.name == "str" && f.arity == 1 && !f.variadic)
        )
    }

    /// Appends the text of `e` to the builder `b`: literals, numbers (as `str`
    /// and templates show them) and `+` of texts. Anything else bails, so the
    /// interpreter keeps its own behavior (and errors, e.g. `"a" + 1`).
    fn str_parts(&mut self, e: &Expr, b: ir::Value) -> R<()> {
        match e {
            Expr::Str(lit) => {
                let st = self.inner.intern(lit);
                let p = self.b.ins().iconst(types::I64, st.as_ptr() as i64);
                let n = self.b.ins().iconst(types::I64, st.len() as i64);
                let fr = self.helper(Helper::SbBytes);
                self.b.ins().call(fr, &[b, p, n]);
                Ok(())
            }
            Expr::Binary { op: BinaryOp::Add, left, right } if self.is_stringish(left) && self.is_stringish(right) => {
                self.str_parts(left, b)?;
                self.str_parts(right, b)
            }
            Expr::Ident(n) if matches!(self.lookup_local(n), Some(l) if l.ty == Ty::Str) => {
                let (p, _) = self.ident(n)?;
                let fr = self.helper(Helper::SbStr);
                self.b.ins().call(fr, &[b, p]);
                Ok(())
            }
            Expr::Template(parts) => {
                for p in parts {
                    if matches!(p, Expr::Str(_)) || self.is_stringish(p) {
                        self.str_parts(p, b)?;
                    } else {
                        self.number_text(p, b)?;
                    }
                }
                Ok(())
            }
            Expr::Call { callee, args } if self.is_str_builtin(callee) && args.len() == 1 => match &args[0] {
                Expr::Str(_) => self.str_parts(&args[0], b),
                a => self.number_text(a, b),
            },
            _ => bail("string expression not supported"),
        }
    }

    /// A boxed `Value` built from a list or map literal (nested ones too), a
    /// text or a number; anything else bails.
    fn value_box(&mut self, e: &Expr) -> R<ir::Value> {
        let call1 = |g: &mut Self, h: Helper, args: &[ir::Value]| -> ir::Value {
            let fr = g.helper(h);
            let c = g.b.ins().call(fr, args);
            g.b.inst_results(c)[0]
        };
        match e {
            Expr::List(items) => {
                let cap = self.b.ins().iconst(types::I64, items.len() as i64);
                let l = call1(self, Helper::VList, &[cap]);
                for it in items {
                    let v = self.value_box(it)?;
                    let fr = self.helper(Helper::VListAdd);
                    self.b.ins().call(fr, &[l, v]);
                }
                Ok(l)
            }
            Expr::Map(entries) => {
                let cap = self.b.ins().iconst(types::I64, entries.len() as i64);
                let m = call1(self, Helper::VMap, &[cap]);
                for (k, v) in entries {
                    let vb = self.value_box(v)?;
                    let key = self.inner.intern(k);
                    let kp = self.b.ins().iconst(types::I64, key.as_ptr() as i64);
                    let kn = self.b.ins().iconst(types::I64, key.len() as i64);
                    let fr = self.helper(Helper::VMapSet);
                    self.b.ins().call(fr, &[m, kp, kn, vb]);
                }
                Ok(m)
            }
            e if self.is_stringish(e) => {
                let fr = self.helper(Helper::SbNew);
                let c = self.b.ins().call(fr, &[]);
                let b = self.b.inst_results(c)[0];
                self.str_parts(e, b)?;
                Ok(call1(self, Helper::VStr, &[b]))
            }
            e => {
                let (v, t) = self.expr(e)?;
                match t {
                    Ty::Int => Ok(call1(self, Helper::VInt, &[v])),
                    Ty::Float => Ok(call1(self, Helper::VFloat, &[v])),
                    Ty::Bool => {
                        let v = self.b.ins().uextend(types::I64, v);
                        Ok(call1(self, Helper::VBool, &[v]))
                    }
                    Ty::Unit | Ty::List | Ty::Map | Ty::Str => bail("value the JIT does not compile"),
                }
            }
        }
    }

    /// The text of a numeric expression, as `str` shows it.
    fn number_text(&mut self, e: &Expr, b: ir::Value) -> R<()> {
        let (v, t) = self.expr(e)?;
        let (h, v) = match t {
            Ty::Int => (Helper::SbI64, v),
            Ty::Float => (Helper::SbF64, v),
            Ty::Bool => (Helper::SbBool, self.b.ins().uextend(types::I64, v)),
            Ty::Unit | Ty::List | Ty::Map | Ty::Str => return bail("text of a value the JIT does not compile"),
        };
        let fr = self.helper(h);
        self.b.ins().call(fr, &[b, v]);
        Ok(())
    }

    fn ident(&mut self, name: &str) -> R<(ir::Value, Ty)> {
        if let Some(l) = self.lookup_local(name) {
            if matches!(l.ty, Ty::List | Ty::Map) {
                return bail(format!("'{}' (a list or map) used as a value", name));
            }
            return Ok((self.b.use_var(l.var), l.ty));
        }
        let binding = self.env.borrow().lookup(name);
        match binding {
            Some(b) if b.stable && !b.mutable => self.const_value(&b.value),
            Some(_) => bail(format!("variable '{}' is not a global constant", name)),
            None => bail(format!("variable '{}' is not defined", name)),
        }
    }

    // ---------- operators ----------

    fn binop(
        &mut self,
        op: BinaryOp,
        a: ir::Value,
        at: Ty,
        b: ir::Value,
        bt: Ty,
    ) -> R<(ir::Value, Ty)> {
        use BinaryOp::*;
        match op {
            Eq => return Ok((self.equality(a, at, b, bt)?, Ty::Bool)),
            Ne => {
                let e = self.equality(a, at, b, bt)?;
                return Ok((self.b.ins().bxor_imm_s(e, 1), Ty::Bool));
            }
            BitAnd | BitOr | BitXor | Shl | Shr => {
                if (at, bt) != (Ty::Int, Ty::Int) {
                    // the interpreter reports it
                    return bail(format!("bit operator between {:?} and {:?}", at, bt));
                }
                let v = match op {
                    BitAnd => {
                        let r = self.b.ins().band(a, b);
                        // `x & mask` with a non-negative mask is in 0..=mask
                        let mask = [a, b].iter().filter_map(|v| self.const_of(*v)).filter(|c| *c >= 0).min();
                        if let Some(mask) = mask {
                            self.ranges.insert(r, (0, mask));
                        }
                        r
                    }
                    BitOr => self.b.ins().bor(a, b),
                    BitXor => self.b.ins().bxor(a, b),
                    Shl => {
                        let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, b, 0);
                        self.trap_if(neg, ops::CODE_SHIFT);
                        // the machine shift takes the count modulo 64: 64 or more is 0
                        let r = self.b.ins().ishl(a, b);
                        let lim = self.b.ins().iconst(types::I64, 63);
                        let big = self.b.ins().icmp(IntCC::UnsignedGreaterThan, b, lim);
                        let z = self.b.ins().iconst(types::I64, 0);
                        self.b.ins().select(big, z, r)
                    }
                    _ => {
                        let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, b, 0);
                        self.trap_if(neg, ops::CODE_SHIFT);
                        let lim = self.b.ins().iconst(types::I64, 63);
                        let n = self.b.ins().umin(b, lim);
                        self.b.ins().sshr(a, n)
                    }
                };
                return Ok((v, Ty::Int));
            }
            _ => {}
        }

        // int -> float promotion
        let (a, b, ty) = match (at, bt) {
            (Ty::Int, Ty::Int) => (a, b, Ty::Int),
            (Ty::Float, Ty::Float) => (a, b, Ty::Float),
            (Ty::Int, Ty::Float) => (self.to_f64(a), b, Ty::Float),
            (Ty::Float, Ty::Int) => (a, self.to_f64(b), Ty::Float),
            _ => return bail(format!("operator not supported between {:?} and {:?}", at, bt)),
        };

        match op {
            Lt | Le | Gt | Ge => {
                let v = if ty == Ty::Int {
                    let cc = match op {
                        Lt => IntCC::SignedLessThan,
                        Le => IntCC::SignedLessThanOrEqual,
                        Gt => IntCC::SignedGreaterThan,
                        _ => IntCC::SignedGreaterThanOrEqual,
                    };
                    self.b.ins().icmp(cc, a, b)
                } else {
                    let cc = match op {
                        Lt => FloatCC::LessThan,
                        Le => FloatCC::LessThanOrEqual,
                        Gt => FloatCC::GreaterThan,
                        _ => FloatCC::GreaterThanOrEqual,
                    };
                    self.b.ins().fcmp(cc, a, b)
                };
                Ok((v, Ty::Bool))
            }
            Add | Sub | Mul | Div | Rem | Pow => {
                if ty == Ty::Int {
                    Ok((self.int_arith(op, a, b), Ty::Int))
                } else {
                    Ok((self.float_arith(op, a, b), Ty::Float))
                }
            }
            Eq | Ne | BitAnd | BitOr | BitXor | Shl | Shr => unreachable!(),
        }
    }

    fn int_arith(&mut self, op: BinaryOp, a: ir::Value, b: ir::Value) -> ir::Value {
        use BinaryOp::*;
        // both operands in known ranges and no result outside of an int: the
        // plain instruction, with nothing to check
        if let (Add | Sub | Mul, Some((al, ah)), Some((bl, bh))) = (op, self.range_of(a), self.range_of(b)) {
            let (al, ah, bl, bh) = (al as i128, ah as i128, bl as i128, bh as i128);
            let (lo, hi) = match op {
                Add => (al + bl, ah + bh),
                Sub => (al - bh, ah - bl),
                _ => {
                    let p = [al * bl, al * bh, ah * bl, ah * bh];
                    (p.iter().copied().min().unwrap_or(0), p.iter().copied().max().unwrap_or(0))
                }
            };
            if lo >= i64::MIN as i128 && hi <= i64::MAX as i128 {
                let r = match op {
                    Add => self.b.ins().iadd(a, b),
                    Sub => self.b.ins().isub(a, b),
                    _ => self.b.ins().imul(a, b),
                };
                self.ranges.insert(r, (lo as i64, hi as i64));
                return r;
            }
        }
        match op {
            Add => {
                let (r, of) = self.b.ins().sadd_overflow(a, b);
                self.trap_if(of, ops::CODE_OVERFLOW);
                r
            }
            Sub => {
                let (r, of) = self.b.ins().ssub_overflow(a, b);
                self.trap_if(of, ops::CODE_OVERFLOW);
                r
            }
            Mul => {
                let (r, of) = self.b.ins().smul_overflow(a, b);
                self.trap_if(of, ops::CODE_OVERFLOW);
                r
            }
            Div | Rem if self.const_of(b).is_some_and(|d| d != 0 && d != -1 && d != i64::MIN) => {
                let d = self.const_of(b).unwrap_or(1);
                let known = self.range_of(a);
                let r = self.div_const(op == Div, a, d);
                if op == Rem {
                    // a remainder is smaller than the divisor and has the
                    // sign of the dividend
                    let m = d.abs() - 1;
                    let range = match known {
                        Some((lo, _)) if lo >= 0 => (0, m),
                        Some((_, hi)) if hi <= 0 => (-m, 0),
                        _ => (-m, m),
                    };
                    self.ranges.insert(r, range);
                }
                r
            }
            Div | Rem => {
                let zero = self.b.ins().icmp_imm_s(IntCC::Equal, b, 0);
                self.trap_if(zero, ops::CODE_DIV_ZERO);
                let m1 = self.b.ins().icmp_imm_s(IntCC::Equal, b, -1);
                let mn = self.b.ins().icmp_imm_s(IntCC::Equal, a, i64::MIN);
                let both = self.b.ins().band(m1, mn);
                self.trap_if(both, ops::CODE_OVERFLOW);
                if op == Div {
                    self.b.ins().sdiv(a, b)
                } else {
                    self.b.ins().srem(a, b)
                }
            }
            Pow => {
                let fr = self.helper(Helper::Ipow);
                let inst = self.b.ins().call(fr, &[self.ctx_ptr, a, b]);
                let r = self.b.inst_results(inst)[0];
                self.check_err();
                r
            }
            _ => unreachable!(),
        }
    }

    fn float_arith(&mut self, op: BinaryOp, a: ir::Value, b: ir::Value) -> ir::Value {
        use BinaryOp::*;
        match op {
            Add => self.b.ins().fadd(a, b),
            Sub => self.b.ins().fsub(a, b),
            // `2.0 * x` is exactly `x + x`, without a constant to load
            Mul if self.float_const(a) == Some(2.0) => self.b.ins().fadd(b, b),
            Mul if self.float_const(b) == Some(2.0) => self.b.ins().fadd(a, a),
            Mul => self.b.ins().fmul(a, b),
            Div => self.b.ins().fdiv(a, b),
            Rem => {
                let fr = self.helper(Helper::Fmod);
                let inst = self.b.ins().call(fr, &[a, b]);
                self.b.inst_results(inst)[0]
            }
            Pow => {
                let fr = self.helper(Helper::Fpow);
                let inst = self.b.ins().call(fr, &[a, b]);
                self.b.inst_results(inst)[0]
            }
            _ => unreachable!(),
        }
    }

    // ---------- calls ----------

    fn resolve_callee(&mut self, callee: &Expr) -> R<Callee> {
        // simple name or module.function
        let (value, via_module) = match callee {
            Expr::Ident(name) => {
                if self.lookup_local(name).is_some() {
                    return bail(format!("'{}' is a numeric variable, not a function", name));
                }
                let b = self.env.borrow().lookup(name);
                match b {
                    Some(b) if b.stable && !b.mutable => (b.value, false),
                    Some(_) => {
                        return bail(format!(
                            "'{}' is not an immutable global binding (cannot be resolved at compile time)",
                            name
                        ))
                    }
                    None => return bail(format!("function '{}' is not defined", name)),
                }
            }
            Expr::Access { target, field } => {
                let Expr::Ident(m) = &**target else {
                    return bail("method call not supported");
                };
                if self.lookup_local(m).is_some() {
                    return bail("call on a local variable");
                }
                let b = self.env.borrow().lookup(m);
                match b {
                    Some(b) if b.stable && !b.mutable => match &b.value {
                        Value::Module(mv) => match mv.exports.borrow().get(field) {
                            Some(v) => (v.clone(), true),
                            None => return bail(format!("module without '{}'", field)),
                        },
                        _ => return bail("method call not supported"),
                    },
                    _ => return bail("module not resolvable at compile time"),
                }
            }
            _ => return bail("unsupported callee"),
        };

        match value {
            Value::Fn(f) => Ok(Callee::User(f)),
            Value::NativeFn(nf) => {
                if let Some(n) = self.inner.math_fns.get(&(nf.func as usize)) {
                    return Ok(Callee::Math(n));
                }
                if let Some(t) = nf.typed {
                    return Ok(Callee::Typed(nf.name.clone(), t));
                }
                if !via_module {
                    match (nf.name.as_str(), nf.arity, nf.variadic) {
                        ("print", 0, true) => return Ok(Callee::Print),
                        ("int", 1, false) => return Ok(Callee::ToInt),
                        ("float", 1, false) => return Ok(Callee::ToFloat),
                        (n, 1, false) => {
                            if let Some(name) = ops::SIZED_NAMES.iter().find(|s| **s == n) {
                                return Ok(Callee::Sized(name, ops::Sized::from_name(name).unwrap()));
                            }
                        }
                        _ => {}
                    }
                }
                bail(format!("native function '{}' not supported", nf.name))
            }
            _ => bail("value is not callable"),
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> R<(ir::Value, Ty)> {
        if let Expr::Access { target, field } = callee {
            if let Expr::Ident(n) = &**target {
                if let Some(l) = self.lookup_local(n) {
                    return self.local_method(l, n, field, args);
                }
            }
        }
        let c = self.resolve_callee(callee)?;
        match c {
            Callee::Print => self.gen_print(args),
            Callee::ToInt | Callee::ToFloat => {
                if args.len() != 1 {
                    return bail("int()/float() expect 1 argument");
                }
                let (v, t) = self.expr(&args[0])?;
                if matches!(c, Callee::ToInt) {
                    match t {
                        Ty::Int => Ok((v, Ty::Int)),
                        Ty::Float => Ok((self.b.ins().fcvt_to_sint_sat(types::I64, v), Ty::Int)),
                        Ty::Bool => Ok((self.b.ins().uextend(types::I64, v), Ty::Int)),
                        Ty::Unit => bail("int(nil)"),
                        Ty::List | Ty::Map | Ty::Str => bail("int() of a value the JIT does not compile"),
                    }
                } else {
                    match t {
                        Ty::Float => Ok((v, Ty::Float)),
                        Ty::Int => Ok((self.to_f64(v), Ty::Float)),
                        _ => bail("float() of bool/nil not supported"),
                    }
                }
            }
            Callee::Sized(name, kind) => {
                if args.len() != 1 {
                    return bail("a conversion expects 1 argument");
                }
                let (v, t) = self.expr(&args[0])?;
                if kind.is_float() {
                    let f = match t {
                        Ty::Float => v,
                        Ty::Int => self.to_f64(v),
                        _ => return bail("float conversion of bool/nil not supported"),
                    };
                    return Ok(match kind {
                        ops::Sized::F32 => {
                            let s = self.b.ins().fdemote(types::F32, f);
                            (self.b.ins().fpromote(types::F64, s), Ty::Float)
                        }
                        _ => (f, Ty::Float),
                    });
                }
                let n = match t {
                    Ty::Int => v,
                    Ty::Float => self.b.ins().fcvt_to_sint_sat(types::I64, v),
                    // 0 or 1: already inside every range
                    Ty::Bool => return Ok((self.b.ins().uextend(types::I64, v), Ty::Int)),
                    _ => return bail("integer conversion of a value the JIT does not compile"),
                };
                // Go's conversion: the low bits, sign- or zero-extended
                let narrow = |g: &mut Self, ty: types::Type, signed: bool| {
                    let r = g.b.ins().ireduce(ty, n);
                    if signed { g.b.ins().sextend(types::I64, r) } else { g.b.ins().uextend(types::I64, r) }
                };
                Ok((
                    match kind {
                        ops::Sized::I8 => narrow(self, types::I8, true),
                        ops::Sized::I16 => narrow(self, types::I16, true),
                        ops::Sized::I32 => narrow(self, types::I32, true),
                        ops::Sized::U8 => narrow(self, types::I8, false),
                        ops::Sized::U16 => narrow(self, types::I16, false),
                        ops::Sized::U32 => narrow(self, types::I32, false),
                        ops::Sized::U64 => {
                            let code = ops::custom_code(&ops::err_negative(name));
                            self.range_guard(n, 0, i64::MAX, code);
                            n
                        }
                        _ => n,
                    },
                    Ty::Int,
                ))
            }
            Callee::Typed(name, t) => {
                use super::value::Num;
                if args.len() != t.params.len() {
                    return bail(format!("'{}' expects {} argument(s)", name, t.params.len()));
                }
                let mut sig = self.inner.module.make_signature();
                let mut vals = Vec::with_capacity(args.len());
                for (a, p) in args.iter().zip(t.params) {
                    let (v, at) = self.expr(a)?;
                    let (v, ct) = match (p, at) {
                        (Num::Int, Ty::Int) => (v, types::I64),
                        (Num::Float, Ty::Float) => (v, types::F64),
                        (Num::Float, Ty::Int) => (self.to_f64(v), types::F64),
                        (Num::Bool, Ty::Bool) => (v, types::I8),
                        _ => return bail(format!("'{}': argument type not covered by its typed entry", name)),
                    };
                    sig.params.push(AbiParam::new(ct));
                    vals.push(v);
                }
                let (ct, rt) = match t.ret {
                    Num::Int => (types::I64, Ty::Int),
                    Num::Float => (types::F64, Ty::Float),
                    Num::Bool => (types::I8, Ty::Bool),
                };
                sig.returns.push(AbiParam::new(ct));
                // it may read a clock or change state: an effect of the iteration
                self.effect();
                let sr = self.b.import_signature(sig);
                let f = self.b.ins().iconst(types::I64, t.entry as i64);
                let inst = self.b.ins().call_indirect(sr, f, &vals);
                let r = self.b.inst_results(inst)[0];
                if t.fallible {
                    // the entry may have left an error: raise it here
                    let mut csig = self.inner.module.make_signature();
                    csig.params.push(AbiParam::new(types::I64));
                    let csr = self.b.import_signature(csig);
                    let cf = self.b.ins().iconst(types::I64, typed_check as *const () as usize as i64);
                    self.b.ins().call_indirect(csr, cf, &[self.ctx_ptr]);
                    self.check_err();
                }
                Ok((r, rt))
            }
            Callee::Math(name) => self.gen_math(name, args),
            Callee::User(f) => {
                if rest_param(&f.params).is_some() {
                    return bail(format!("'{}' is variadic", f.name));
                }
                if f.params.len() != args.len() {
                    return bail(format!(
                        "'{}' expects {} arguments, got {}",
                        f.name,
                        f.params.len(),
                        args.len()
                    ));
                }
                if args.len() > MAX_ARGS {
                    return bail("too many arguments");
                }
                let mut vals = vec![self.ctx_ptr];
                let mut mask = 0u64;
                for (i, a) in args.iter().enumerate() {
                    let (v, t) = self.expr(a)?;
                    if t == Ty::Unit {
                        return bail("nil argument");
                    }
                    mask |= t.code() << (2 * i);
                    vals.push(v);
                }
                let key = FnKey { ptr: Rc::as_ptr(&f.body) as usize, mask, n: args.len() as u8, checked: self.checked };
                let (id, ret) = self.inner.specialize(&f, key, self.me)?;
                let fr = self.inner.module.declare_func_in_func(id, self.b.func);
                // the callee may print: no speculation may fail after it
                self.effect();
                let inst = self.b.ins().call(fr, &vals);
                let r = self.b.inst_results(inst)[0];
                self.check_err();
                Ok((r, ret))
            }
        }
    }

    /// A number as a float (ints are converted), or a bail.
    fn math_f64(&mut self, name: &str, e: &Expr) -> R<ir::Value> {
        let (v, t) = self.expr(e)?;
        match t {
            Ty::Int => Ok(self.to_f64(v)),
            Ty::Float => Ok(v),
            _ => bail(format!("math.{} requires numbers", name)),
        }
    }

    /// Calls a Rust `extern "C" fn(f64...) -> f64` by address.
    fn call_f64(&mut self, addr: usize, args: &[ir::Value]) -> ir::Value {
        let mut sig = self.inner.module.make_signature();
        for _ in args {
            sig.params.push(AbiParam::new(types::F64));
        }
        sig.returns.push(AbiParam::new(types::F64));
        let sr = self.b.import_signature(sig);
        let f = self.b.ins().iconst(types::I64, addr as i64);
        let inst = self.b.ins().call_indirect(sr, f, args);
        self.b.inst_results(inst)[0]
    }

    fn gen_math(&mut self, name: &str, args: &[Expr]) -> R<(ir::Value, Ty)> {
        let want = match name {
            "random" => 0,
            "pow" | "atan2" | "hypot" | "min" | "max" => 2,
            "lerp" => 3,
            "log" | "round" => args.len().clamp(1, 2),
            _ => 1,
        };
        if args.len() != want {
            return bail(format!("math.{} expects {} argument(s)", name, want));
        }
        if let Some(f) = math_unary(name, args.len()) {
            let x = self.math_f64(name, &args[0])?;
            return Ok((self.call_f64(f as usize, &[x]), Ty::Float));
        }
        if let Some(f) = math_binary(name) {
            let x = self.math_f64(name, &args[0])?;
            let y = self.math_f64(name, &args[1])?;
            return Ok((self.call_f64(f as usize, &[x, y]), Ty::Float));
        }
        match name {
            "sqrt" => {
                let f = self.math_f64(name, &args[0])?;
                Ok((self.b.ins().sqrt(f), Ty::Float))
            }
            "abs" => {
                let (a, at) = self.expr(&args[0])?;
                match at {
                    Ty::Int => {
                        let is_min = self.b.ins().icmp_imm_s(IntCC::Equal, a, i64::MIN);
                        self.trap_if(is_min, ops::CODE_OVERFLOW);
                        Ok((self.b.ins().iabs(a), Ty::Int))
                    }
                    Ty::Float => Ok((self.b.ins().fabs(a), Ty::Float)),
                    _ => bail("abs requires a number"),
                }
            }
            "floor" | "ceil" | "trunc" | "round" => {
                let (a, at) = self.expr(&args[0])?;
                match at {
                    Ty::Int => Ok((a, Ty::Int)),
                    Ty::Float => {
                        let r = match name {
                            "floor" => self.b.ins().floor(a),
                            "ceil" => self.b.ins().ceil(a),
                            "trunc" => self.b.ins().trunc(a),
                            // halves away from zero, like f64::round (`nearest` rounds to even)
                            _ => self.call_f64(m_round as extern "C" fn(f64) -> f64 as usize, &[a]),
                        };
                        Ok((self.b.ins().fcvt_to_sint_sat(types::I64, r), Ty::Int))
                    }
                    _ => bail("floor/ceil/trunc/round require a number"),
                }
            }
            "min" | "max" => {
                let (a, at) = self.expr(&args[0])?;
                let (b, bt) = self.expr(&args[1])?;
                if (at, bt) != (Ty::Int, Ty::Int) {
                    return bail("math.min/max: only ints are compiled");
                }
                let r = if name == "min" { self.b.ins().smin(a, b) } else { self.b.ins().smax(a, b) };
                Ok((r, Ty::Int))
            }
            "lerp" => {
                let x = self.math_f64(name, &args[0])?;
                let y = self.math_f64(name, &args[1])?;
                let t = self.math_f64(name, &args[2])?;
                let d = self.b.ins().fsub(y, x);
                let m = self.b.ins().fmul(d, t);
                Ok((self.b.ins().fadd(x, m), Ty::Float))
            }
            "random" => {
                // the generator's state changes: an iteration can't be redone after it
                self.effect();
                Ok((self.call_f64(m_random as extern "C" fn() -> f64 as usize, &[]), Ty::Float))
            }
            _ => bail("unknown math function"),
        }
    }

    fn gen_print(&mut self, args: &[Expr]) -> R<(ir::Value, Ty)> {
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                let fr = self.helper(Helper::PrintSep);
                self.b.ins().call(fr, &[]);
            }
            if let Expr::Str(s) = a {
                let st = self.inner.intern(s);
                let p = self.b.ins().iconst(types::I64, st.as_ptr() as i64);
                let n = self.b.ins().iconst(types::I64, st.len() as i64);
                let fr = self.helper(Helper::PrintStr);
                self.b.ins().call(fr, &[p, n]);
                continue;
            }
            let (v, t) = self.expr(a)?;
            match t {
                Ty::Int => {
                    let fr = self.helper(Helper::PrintI64);
                    self.b.ins().call(fr, &[v]);
                }
                Ty::Float => {
                    let fr = self.helper(Helper::PrintF64);
                    self.b.ins().call(fr, &[v]);
                }
                Ty::Bool => {
                    let w = self.b.ins().uextend(types::I64, v);
                    let fr = self.helper(Helper::PrintBool);
                    self.b.ins().call(fr, &[w]);
                }
                Ty::Str => {
                    let fr = self.helper(Helper::PrintSv);
                    self.b.ins().call(fr, &[v]);
                }
                Ty::Unit => return bail("print of nil"),
                Ty::List | Ty::Map => return bail("print of a list or map"),
            }
        }
        let fr = self.helper(Helper::PrintNl);
        self.b.ins().call(fr, &[]);
        self.effect();
        Ok((self.b.ins().iconst(types::I8, 0), Ty::Unit))
    }
}

fn stmt_name(s: &Stmt) -> &'static str {
    match s {
        Stmt::Line(_) => "line",
        Stmt::LetList { .. } => "let-list",
        Stmt::Let { .. } => "let",
        Stmt::CheckType { .. } => "type check",
        Stmt::ReturnType(_) => "return type",
        Stmt::Interface(_) => "interface",
        Stmt::Expr(_) => "expr",
        Stmt::Block(_) => "block",
        Stmt::If { .. } => "if",
        Stmt::While { .. } => "while",
        Stmt::For { .. } => "for",
        Stmt::Break => "break",
        Stmt::Continue => "continue",
        Stmt::Return(_) => "return",
        Stmt::FnDecl { .. } => "fn",
        Stmt::Import { .. } => "import",
        Stmt::Export(_) => "export",
        Stmt::Struct(_) => "struct",
        Stmt::Select { .. } => "select",
        Stmt::MatchStmt { .. } => "match",
        Stmt::SendStmt { .. } => "send",
    }
}

fn expr_name(e: &Expr) -> &'static str {
    match e {
        Expr::Int(_) => "int",
        Expr::Float(_) => "float",
        Expr::Str(_) => "str",
        Expr::Bool(_) => "bool",
        Expr::Nil => "nil",
        Expr::Ident(_) => "ident",
        Expr::List(_) => "list",
        Expr::Map(_) => "map",
        Expr::Unary { .. } => "unary",
        Expr::Binary { .. } => "binary",
        Expr::Logical { .. } => "logical",
        Expr::Range { .. } => "range",
        Expr::Index { .. } => "index",
        Expr::Access { .. } => "access",
        Expr::Call { .. } => "call",
        Expr::Assign { .. } => "assignment",
        Expr::New { .. } => "new",
        Expr::Fn { .. } => "anonymous fn",
        Expr::Fg { .. } => "fg",
        Expr::Chan { .. } => "chan",
        Expr::Recv { .. } => "recv",
        Expr::Match { .. } => "match",
        Expr::Try { .. } => "?",
        Expr::OptAccess { .. } | Expr::OptChain(_) => "?.",
        Expr::Template(_) => "template",
        Expr::Duration(_) => "duration",
        Expr::DynImport(_) => "import()",
    }
}

// ======================================================================
// Public API
// ======================================================================

pub enum LoopKind<'a> {
    While { cond: &'a Expr, body: &'a Stmt },
    For { var: &'a str, body: &'a Stmt, lo: i64, hi: i64 },
    /// `for var in list`, over the interpreter's snapshot of the list.
    ForList { var: &'a str, body: &'a Stmt, items: &'a [Value] },
}

pub enum LoopOutcome {
    /// Not compilable (or JIT disabled): continue in the interpreter.
    Skip,
    /// The loop ran to completion in native code.
    Done,
    /// A speculation failed before any side effect of iteration `n` (the
    /// range value or the list index; 0 for `while`): the variables are
    /// written back and the interpreter continues from that iteration.
    Deopt(i64),
    Err(String),
}

pub struct Jit {
    cfg: JitConfig,
    inner: Option<Inner>,
    init_failed: bool,
    counts: HashMap<usize, u32>,
    loop_names: HashMap<u32, Rc<Vec<String>>>,
    /// The stack limit of the native code, shared with the watchdog.
    watch: std::sync::Arc<Watch>,
    /// The watchdog knows `watch` (from the first run with a deadline on).
    watched: bool,
}

impl Jit {
    pub fn new() -> Self {
        Jit {
            cfg: config(),
            inner: None,
            init_failed: false,
            counts: HashMap::new(),
            loop_names: HashMap::new(),
            watch: std::sync::Arc::new(Watch {
                armed: std::sync::Mutex::new(None),
                limit: std::sync::atomic::AtomicI64::new(0),
            }),
            watched: false,
        }
    }

    /// Before native code runs: its stack limit and, with a deadline, the
    /// watchdog that will stop it.
    fn enter(&mut self, deadline: Option<u64>) {
        // native frames may go down to the limit of this thread / task (it
        // already leaves a margin for the helpers and the error path)
        self.watch.limit.store(sched::stack_limit() as i64, std::sync::atomic::Ordering::Relaxed);
        if let Some(d) = deadline {
            if !self.watched {
                self.watched = true;
                watch_register(&self.watch);
            }
            *self.watch.armed.lock().unwrap_or_else(|p| p.into_inner()) = Some(d);
            watch_wake();
        }
    }

    /// After native code ran: was it stopped by its deadline?
    fn leave(&mut self, deadline: Option<u64>) -> bool {
        if deadline.is_none() {
            return false;
        }
        *self.watch.armed.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.watch.limit.load(std::sync::atomic::Ordering::SeqCst) == LIMIT_STOP
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    pub fn loop_threshold(&self) -> u32 {
        self.cfg.loop_threshold.max(1)
    }

    fn inner(&mut self) -> Option<&mut Inner> {
        if self.init_failed {
            return None;
        }
        if let Some(i) = &self.inner {
            if i.epoch != redefine_epoch() {
                // A global binding was redefined: compiled code may have
                // assumed the old value. Start from scratch.
                let dbg = i.debug;
                if dbg >= 1 {
                    eprintln!("[jit] global binding redefined: cache invalidated");
                }
                self.inner = None;
            }
        }
        if self.inner.is_none() {
            let t0 = Instant::now();
            match Inner::new(self.cfg.debug, &self.watch.limit as *const _ as usize) {
                Ok(i) => {
                    if self.cfg.debug >= 1 {
                        eprintln!("[jit] ready in {}µs", t0.elapsed().as_micros());
                    }
                    self.inner = Some(i)
                }
                Err(e) => {
                    if self.cfg.debug >= 1 {
                        eprintln!("[jit] unavailable on this system: {}", e);
                    }
                    self.init_failed = true;
                    return None;
                }
            }
        }
        self.inner.as_mut()
    }

    fn set_limits(inner: &mut Inner) {
        inner.ctx.err = 0;
        inner.ctx.deopt = 0;
        inner.ctx.resume = 0;
        inner.ctx.line = 0;
    }

    /// Last line recorded by native code (for error messages).
    pub fn last_line(&self) -> u32 {
        self.inner.as_ref().map(|i| i.ctx.line as u32).unwrap_or(0)
    }

    /// Tries to run the function natively. `None` = use the interpreter.
    ///
    /// `deadline` ([`deadline_ns`]) is the deadline of the running `fogu:vm`
    /// code, if it has one: the native code is stopped when it passes (the
    /// error is then [`ops::ERR_CANCEL`]).
    pub fn call_fn(&mut self, f: &Rc<FnValue>, args: &[Value], deadline: Option<u64>) -> Option<Result<Value, String>> {
        if !self.cfg.enabled
            || args.len() != f.params.len()
            || args.len() > MAX_ARGS
            || rest_param(&f.params).is_some()
        {
            return None;
        }
        let mut mask = 0u64;
        for (i, a) in args.iter().enumerate() {
            mask |= Ty::of_value(a)?.code() << (2 * i);
        }
        let key = FnKey { ptr: Rc::as_ptr(&f.body) as usize, mask, n: args.len() as u8, checked: deadline.is_some() };

        // Only compile after a few interpreted calls.
        let c = self.counts.entry(key.ptr).or_insert(0);
        *c += 1;
        if *c < self.cfg.call_threshold {
            return None;
        }

        // Already rejected: answer before entering the (stack-hungry)
        // compile path, so tasks calling it keep a small stack.
        if let Some(i) = &self.inner {
            if i.epoch == redefine_epoch() && matches!(i.specs.get(&key), Some(Spec::Failed(_))) {
                return None;
            }
        }
        let inner = self.inner()?;
        if let Err(_) = inner.specialize(f, key, None) {
            return None;
        }
        let addr = match inner.ensure_thunk(key) {
            Ok(a) => a,
            Err(b) => {
                let m = bail_msg(&b);
                inner.log(format!("thunk of '{}' failed: {}", f.name, m));
                inner.specs.insert(key, Spec::Failed(m));
                return None;
            }
        };
        let ret = match inner.specs.get(&key) {
            Some(Spec::Done { ret, .. }) => *ret,
            _ => return None,
        };

        let mut argbuf = [0u64; MAX_ARGS];
        for (i, a) in args.iter().enumerate() {
            argbuf[i] = match a {
                Value::Int(n) => *n as u64,
                Value::Float(x) => x.to_bits(),
                Value::Bool(b) => *b as u64,
                _ => return None,
            };
        }
        let mut out: u64 = 0;
        Self::set_limits(inner);
        inner.stats.native_calls += 1;
        let ctx: *mut JitCtx = &mut *inner.ctx;
        self.enter(deadline);
        let thunk: extern "C" fn(*mut JitCtx, *const u64, *mut u64) =
            unsafe { std::mem::transmute(addr) };
        thunk(ctx, argbuf.as_ptr(), &mut out);
        let stopped = self.leave(deadline);

        // SAFETY: `ctx` is the context of `self.inner`, which `enter` / `leave` do not touch.
        let err = unsafe { (*ctx).err };
        if err != 0 {
            if stopped && err == ops::CODE_STACK {
                return Some(Err(ops::ERR_CANCEL.to_string()));
            }
            return Some(Err(ops::code_message(err).to_string()));
        }
        Some(Ok(match ret {
            Ty::Int => Value::Int(out as i64),
            Ty::Float => Value::Float(f64::from_bits(out)),
            Ty::Bool => Value::Bool(out & 0xff != 0),
            // functions never return lists, maps or texts (they are only loop slots)
            Ty::Unit | Ty::List | Ty::Map | Ty::Str => Value::Nil,
        }))
    }

    /// Tries to compile and run the loop from the current state of `env`.
    pub fn run_loop(
        &mut self,
        id: u32,
        kind: LoopKind,
        env: &Rc<RefCell<Environment>>,
        deadline: Option<u64>,
    ) -> LoopOutcome {
        if !self.cfg.enabled {
            return LoopOutcome::Skip;
        }
        let names = match self.loop_names.get(&id) {
            Some(n) => Rc::clone(n),
            None => {
                let mut set = HashSet::new();
                match &kind {
                    LoopKind::While { cond, body } => {
                        let tmp = Stmt::Expr((*cond).clone());
                        collect_names(&mut set, &tmp);
                        collect_names(&mut set, body);
                    }
                    LoopKind::For { body, .. } | LoopKind::ForList { body, .. } => collect_names(&mut set, body),
                }
                let mut v: Vec<String> = set.into_iter().collect();
                v.sort();
                let rc = Rc::new(v);
                self.loop_names.insert(id, Rc::clone(&rc));
                rc
            }
        };

        // free numeric variables that must go through slots
        let mut slots: Vec<(String, Ty, bool)> = Vec::new();
        let mut init: Vec<u64> = Vec::new();
        let mut spec: Vec<(String, Ty)> = Vec::new();
        for n in names.iter() {
            if let Some(b) = env.borrow().lookup(n) {
                // a list or map: its pointer (the environment keeps it alive); the
                // type of its first element/value is what the loop speculates
                if let Value::List(l) = &b.value {
                    slots.push((n.clone(), Ty::List, false));
                    init.push(Rc::as_ptr(l) as u64);
                    if let Some(t) = l.borrow().first().and_then(Ty::of_value) {
                        spec.push((n.clone(), t));
                    }
                    continue;
                }
                if let Value::Map(m) = &b.value {
                    slots.push((n.clone(), Ty::Map, false));
                    init.push(Rc::as_ptr(m) as u64);
                    if let Some(t) = m.borrow().values().next().and_then(Ty::of_value) {
                        spec.push((n.clone(), t));
                    }
                    continue;
                }
                if let Some(t) = Ty::of_value(&b.value) {
                    if b.mutable || !b.stable {
                        slots.push((n.clone(), t, b.mutable));
                        init.push(match &b.value {
                            Value::Int(x) => *x as u64,
                            Value::Float(x) => x.to_bits(),
                            Value::Bool(x) => *x as u64,
                            _ => 0,
                        });
                    }
                }
            }
        }
        let mut elem = None;
        match &kind {
            LoopKind::For { lo, hi, .. } => {
                init.push(*lo as u64);
                init.push(*hi as u64);
            }
            LoopKind::ForList { items, .. } => {
                elem = match items.first() {
                    Some(Value::Str(_)) => Some(Ty::Str),
                    Some(v) => Ty::of_value(v),
                    None => None,
                };
                if elem.is_none() {
                    return LoopOutcome::Skip;
                }
                init.push(items.as_ptr() as u64);
                init.push(items.len() as u64);
            }
            LoopKind::While { .. } => {}
        }

        let key = LoopKey {
            id,
            kind: match kind {
                LoopKind::While { .. } => 0,
                LoopKind::For { .. } => 1,
                LoopKind::ForList { .. } => 2,
            },
            slots: slots.clone(),
            spec,
            elem,
            checked: deadline.is_some(),
        };

        let Some(inner) = self.inner() else {
            return LoopOutcome::Skip;
        };

        if !inner.loops.contains_key(&key) {
            let t0 = Instant::now();
            let entry = match inner.compile_loop(&kind, env, &key) {
                Ok((addr, assigned)) => {
                    let us = t0.elapsed().as_micros();
                    inner.stats.loops_compiled += 1;
                    inner.stats.compile_time_us += us;
                    inner.log(format!("loop #{} compiled in {}µs ({} variables)", id, us, slots.len()));
                    LoopEntry::Done { addr, assigned }
                }
                Err(b) => {
                    let m = bail_msg(&b);
                    inner.log(format!("loop #{} not compiled: {}", id, m));
                    inner.stats.bails.push(format!("loop #{}: {}", id, m));
                    LoopEntry::Failed
                }
            };
            inner.loops.insert(key.clone(), entry);
        }

        let (addr, assigned) = match inner.loops.get(&key) {
            Some(LoopEntry::Done { addr, assigned }) => (*addr, assigned.clone()),
            _ => return LoopOutcome::Skip,
        };

        Self::set_limits(inner);
        inner.stats.native_loops += 1;
        let ctx: *mut JitCtx = &mut *inner.ctx;
        self.enter(deadline);
        let f: extern "C" fn(*mut JitCtx, *mut u64) -> i64 = unsafe { std::mem::transmute(addr) };
        let rc = f(ctx, init.as_mut_ptr());
        let stopped = self.leave(deadline);
        let Some(inner) = self.inner.as_mut() else {
            return LoopOutcome::Skip;
        };

        // On error (and on deopt) the modified variables are also written back
        // (the native code already wrote them to the slots before leaving).
        let failed = (inner.ctx.err != 0).then(|| {
            if stopped && inner.ctx.err == ops::CODE_STACK {
                ops::ERR_CANCEL.to_string()
            } else {
                ops::code_message(inner.ctx.err).to_string()
            }
        });
        for (i, (name, ty, _)) in slots.iter().enumerate() {
            if assigned[i] {
                let v = match ty {
                    Ty::Int => Value::Int(init[i] as i64),
                    Ty::Float => Value::Float(f64::from_bits(init[i])),
                    Ty::Bool => Value::Bool(init[i] & 0xff != 0),
                    // list/map slots are never assigned (they can't be reassigned in the loop)
                    Ty::Unit | Ty::List | Ty::Map | Ty::Str => Value::Nil,
                };
                if let Err(e) = env.borrow_mut().assign(name, v) {
                    return LoopOutcome::Err(e);
                }
            }
        }
        if let Some(e) = failed {
            return LoopOutcome::Err(e);
        }
        if rc == 1 {
            let at = inner.ctx.resume;
            inner.stats.deopts += 1;
            inner.log(format!("loop #{} left native code at iteration {} (speculation failed)", id, at));
            return LoopOutcome::Deopt(at);
        }
        LoopOutcome::Done
    }

    pub fn report(&self) -> String {
        let Some(i) = &self.inner else {
            return "[jit] no compilation was needed".into();
        };
        let s = &i.stats;
        let mut out = format!(
            "[jit] compiled functions: {} | compiled loops: {} | native calls: {} | native loops: {} | deopts: {} | compile time: {:.2} ms",
            s.fns_compiled,
            s.loops_compiled,
            s.native_calls,
            s.native_loops,
            s.deopts,
            s.compile_time_us as f64 / 1000.0
        );
        let mut seen = HashSet::new();
        for b in &s.bails {
            if seen.insert(b.clone()) {
                out.push_str(&format!("\n[jit]   gave up: {}", b));
            }
        }
        out
    }
}

impl Default for Jit {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod div_magic_tests {
    use super::div_magic;

    /// What the code `div_const` emits computes, for a divisor that is not a
    /// power of two.
    fn div(n: i64, d: i64) -> i64 {
        let (m, s) = div_magic(d);
        let mut q = ((n as i128 * m as i128) >> 64) as i64;
        if d > 0 && m < 0 {
            q = q.wrapping_add(n);
        } else if d < 0 && m > 0 {
            q = q.wrapping_sub(n);
        }
        q >>= s;
        q.wrapping_add(((q as u64) >> 63) as i64)
    }

    #[test]
    fn matches_the_division_instruction() {
        let mut divisors: Vec<i64> = vec![3, 5, 6, 7, 9, 10, 11, 12, 25, 100, 125, 641, 1000, 65537, 1_000_000_007];
        divisors.extend([i64::MAX, i64::MAX - 1, (1 << 62) + 1, (1 << 62) - 1, (1 << 33) + 3]);
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as i64
        };
        for _ in 0..200 {
            let d = next() >> (next() & 63);
            if d != 0 && d != -1 && d != i64::MIN && !d.unsigned_abs().is_power_of_two() {
                divisors.push(d);
            }
        }
        let mut values: Vec<i64> = vec![0, 1, -1, 2, -2, i64::MAX, i64::MIN, i64::MAX - 1, i64::MIN + 1];
        for _ in 0..2000 {
            values.push(next() >> (next() & 63));
        }
        for &d in divisors.iter().chain(divisors.iter().map(|d| -*d).collect::<Vec<_>>().iter()) {
            for &n in &values {
                for n in [n, n.wrapping_mul(d), n.wrapping_mul(d).wrapping_add(1), n.wrapping_mul(d).wrapping_sub(1)] {
                    assert_eq!(div(n, d), n.wrapping_div(d), "{} / {}", n, d);
                }
            }
        }
    }
}
