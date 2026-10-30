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
    /// kept alive by the environment). Only `xs.push(number)` compiles; any
    /// other use bails, and the variable can't be reassigned in the loop.
    List,
}

impl Ty {
    fn clif(self) -> ir::Type {
        match self {
            Ty::Int | Ty::List => types::I64,
            Ty::Float => types::F64,
            Ty::Bool | Ty::Unit => types::I8,
        }
    }
    fn code(self) -> u64 {
        match self {
            Ty::Int => 1,
            Ty::Float => 2,
            Ty::Bool => 3,
            // never part of a function key: lists only live in loop slots
            Ty::Unit | Ty::List => 0,
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
    depth: i64,
    depth_limit: i64,
    line: i64,
}
const OFF_LINE: i32 = 24;
const OFF_ERR: i32 = 0;
const OFF_DEPTH: i32 = 8;
const OFF_LIMIT: i32 = 16;

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
}

impl Helper {
    const ALL: [Helper; 27] = [
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
        }
    }
}

// ======================================================================
// Keys and cache entries
// ======================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct FnKey {
    ptr: usize,
    mask: u64,
    n: u8,
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
    is_for: bool,
    slots: Vec<(String, Ty, bool)>,
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
}

// ======================================================================
// Compiler state
// ======================================================================

struct Inner {
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
}

impl Inner {
    fn new(debug: u8) -> Result<Inner, String> {
        let mut flags = settings::builder();
        flags.set("use_colocated_libcalls", "false").map_err(|e| e.to_string())?;
        flags.set("is_pic", "false").map_err(|e| e.to_string())?;
        flags.set("opt_level", "speed").map_err(|e| e.to_string())?;
        let isa = cranelift_native::builder()
            .map_err(|e| e.to_string())?
            .finish(settings::Flags::new(flags))
            .map_err(|e| e.to_string())?;
        let mut jb = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        for h in Helper::ALL {
            jb.symbol(h.name(), h.addr());
        }
        let mut module = JITModule::new(jb);

        let mut helper_ids = HashMap::new();
        for h in Helper::ALL {
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
        for name in ["sqrt", "abs", "floor", "ceil", "pow"] {
            if let Some(Value::NativeFn(nf)) = m.exports.borrow().get(name) {
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
            ctx: Box::new(JitCtx { err: 0, depth: 0, depth_limit: 0, line: 0 }),
            epoch: redefine_epoch(),
            counter: 0,
            debug,
            math_fns,
            strs: HashMap::new(),
            stats: Stats::default(),
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
            g.prologue_fn(&f.params, tys)?;
            for s in f.body.iter() {
                g.stmt(s)?;
            }
            g.finish(cfg);
        }
        self.define(id, &mut ctx)
    }

    fn define(&mut self, id: FuncId, ctx: &mut Context) -> R<()> {
        if self.debug >= 2 {
            eprintln!("{}", ctx.func.display());
        }
        self.module
            .define_function(id, ctx)
            .map_err(|e| Bail::Msg(format!("internal JIT error: {:?}", e)))?;
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
        slots: &[(String, Ty, bool)],
    ) -> R<(usize, Vec<bool>)> {
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
            let slots_ptr = g.prologue_loop(slots)?;
            match kind {
                LoopKind::While { cond, body } => g.gen_while(cond, body)?,
                LoopKind::For { var, body, .. } => {
                    let n = slots.len();
                    let lo = g.b.ins().load(types::I64, MemFlagsData::trusted(), slots_ptr, (8 * n) as i32);
                    let hi = g.b.ins().load(types::I64, MemFlagsData::trusted(), slots_ptr, (8 * (n + 1)) as i32);
                    g.gen_for_range(var, lo, hi, body)?;
                }
            }
            g.epilogue_loop(slots_ptr, slots);
            assigned = g.slots_assigned.clone();
            g.finish(cfg);
        }
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
    match ty {
        TypeExpr::Any => true,
        TypeExpr::Named(n) => matches!(
            (n.as_str(), t),
            ("int", Ty::Int) | ("float", Ty::Float) | ("float", Ty::Int) | ("bool", Ty::Bool)
        ),
        // numbers are never nil
        TypeExpr::Optional(inner) => annotation_ok(inner, t),
        TypeExpr::Union(ts) => ts.iter().any(|x| annotation_ok(x, t)),
        _ => false,
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
}

struct Gen<'a, 'f> {
    inner: &'a mut Inner,
    b: FunctionBuilder<'f>,
    ctx_ptr: ir::Value,
    scopes: Vec<HashMap<String, Local>>,
    env: Rc<RefCell<Environment>>,
    /// (`continue` target, `break` target)
    loops: Vec<(ir::Block, ir::Block)>,
    ret: Option<Ty>,
    me: Option<FnKey>,
    err_blocks: HashMap<i64, ir::Block>,
    prop_block: Option<ir::Block>,
    helper_refs: HashMap<Helper, FuncRef>,
    dead: bool,
    slots_assigned: Vec<bool>,
    /// For loops: slot pointer and which ones to also save on error.
    wb: Option<(ir::Value, Vec<(usize, String)>)>,
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
        Gen {
            inner,
            b,
            ctx_ptr,
            scopes: vec![HashMap::new()],
            env,
            loops: Vec::new(),
            ret,
            me,
            err_blocks: HashMap::new(),
            prop_block: None,
            helper_refs: HashMap::new(),
            dead: false,
            slots_assigned: Vec::new(),
            wb: None,
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
        // depth += 1; if depth > limit -> stack error
        let d = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx_ptr, OFF_DEPTH);
        let d1 = self.b.ins().iadd_imm_s(d, 1);
        self.b.ins().store(MemFlagsData::trusted(), d1, self.ctx_ptr, OFF_DEPTH);
        let lim = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx_ptr, OFF_LIMIT);
        let over = self.b.ins().icmp(IntCC::SignedGreaterThan, d1, lim);
        self.trap_if(over, ops::CODE_STACK);
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
        }
        Ok(slots_ptr)
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
            let c = self.b.ins().iconst(types::I64, code);
            self.b.ins().store(MemFlagsData::trusted(), c, self.ctx_ptr, OFF_ERR);
            self.write_back();
            let z = self.zero_ret();
            self.b.ins().return_(&[z]);
        }
        if let Some(blk) = self.prop_block {
            self.b.switch_to_block(blk);
            self.write_back();
            let z = self.zero_ret();
            self.b.ins().return_(&[z]);
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
        self.b.set_cold_block(b);
        self.err_blocks.insert(code, b);
        b
    }

    fn trap_if(&mut self, cond: ir::Value, code: i64) {
        let eb = self.err_block(code);
        let cont = self.b.create_block();
        self.b.ins().brif(cond, eb, &[], cont, &[]);
        self.b.switch_to_block(cont);
    }

    fn check_err(&mut self) {
        let pb = match self.prop_block {
            Some(b) => b,
            None => {
                let b = self.b.create_block();
                self.b.set_cold_block(b);
                self.prop_block = Some(b);
                b
            }
        };
        let e = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx_ptr, OFF_ERR);
        let cont = self.b.create_block();
        self.b.ins().brif(e, pb, &[], cont, &[]);
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
                self.b.ins().store(MemFlagsData::trusted(), l, self.ctx_ptr, OFF_LINE);
                Ok(())
            }
            Stmt::Let { name, mutable, init, ty } => {
                let (v, t) = self.expr(init)?;
                if let Some(ty) = ty {
                    if !annotation_ok(ty, t) {
                        return bail(format!("'{}' does not match its annotation {}", name, ty));
                    }
                }
                self.declare_local(name, t, *mutable, v)
            }
            // Specializations are per argument type, so a matching
            // annotation costs nothing; otherwise the interpreter reports it.
            Stmt::CheckType { name, ty } => {
                let (_, t) = self.ident(name)?;
                if !annotation_ok(ty, t) {
                    return bail(format!("parameter '{}' does not match its annotation {}", name, ty));
                }
                Ok(())
            }
            Stmt::ReturnType(ty) => match self.ret {
                Some(r) if annotation_ok(ty, r) => Ok(()),
                _ => bail(format!("return type does not match the annotation {}", ty)),
            },
            Stmt::Expr(e) => {
                if self.list_push(e)? {
                    return Ok(());
                }
                self.expr(e)?;
                Ok(())
            }
            Stmt::Block(b) => self.block_stmts(b),
            Stmt::If { cond, then_branch, else_branch } => {
                let c = self.cond(cond)?;
                let then_b = self.b.create_block();
                let merge = self.b.create_block();
                let else_b = if else_branch.is_some() { self.b.create_block() } else { merge };
                self.b.ins().brif(c, then_b, &[], else_b, &[]);

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
                // depth -= 1
                let d = self.b.ins().load(types::I64, MemFlagsData::trusted(), self.ctx_ptr, OFF_DEPTH);
                let d1 = self.b.ins().iadd_imm_s(d, -1);
                self.b.ins().store(MemFlagsData::trusted(), d1, self.ctx_ptr, OFF_DEPTH);
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
        let c = self.cond(cond)?;
        self.b.ins().brif(c, body_b, &[], exit, &[]);

        self.b.switch_to_block(body_b);
        self.loops.push((header, exit));
        let r = self.stmt(body);
        self.loops.pop();
        r?;
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
        self.scopes.push(HashMap::new());
        let cur = self.b.use_var(idx);
        self.declare_local(var, Ty::Int, false, cur)?;
        self.loops.push((incr, exit));
        let r = self.stmt(body);
        self.loops.pop();
        self.scopes.pop();
        r?;
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
            // different types (bool vs number): never equal
            _ => self.b.ins().iconst(types::I8, 0),
        })
    }

    fn gen_match(&mut self, subject: &Expr, arms: &[MatchArm], want_value: bool) -> R<(ir::Value, Ty)> {
        let (sv, st) = self.expr(subject)?;
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
        self.b.ins().jump(eb, &[]);
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
                Ty::Unit | Ty::List => return bail("push of a value the JIT does not compile"),
            };
            let fr = self.helper(h);
            self.b.ins().call(fr, &[list, v]);
        }
        Ok(true)
    }

    /// Does `e` produce text (so `push` must build a string)? Literals,
    /// templates, `str(…)` and `+` with any of those.
    fn is_stringish(&self, e: &Expr) -> bool {
        match e {
            Expr::Str(_) | Expr::Template(_) => true,
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
            Expr::Template(parts) => {
                for p in parts {
                    if matches!(p, Expr::Str(_)) {
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
                    Ty::Unit | Ty::List => bail("value the JIT does not compile"),
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
            Ty::Unit | Ty::List => return bail("text of a value the JIT does not compile"),
        };
        let fr = self.helper(h);
        self.b.ins().call(fr, &[b, v]);
        Ok(())
    }

    fn ident(&mut self, name: &str) -> R<(ir::Value, Ty)> {
        if let Some(l) = self.lookup_local(name) {
            if l.ty == Ty::List {
                return bail(format!("list '{}' used as a value (only push is compiled)", name));
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
            Eq | Ne => unreachable!(),
        }
    }

    fn int_arith(&mut self, op: BinaryOp, a: ir::Value, b: ir::Value) -> ir::Value {
        use BinaryOp::*;
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
                if !via_module {
                    match (nf.name.as_str(), nf.arity, nf.variadic) {
                        ("print", 0, true) => return Ok(Callee::Print),
                        ("int", 1, false) => return Ok(Callee::ToInt),
                        ("float", 1, false) => return Ok(Callee::ToFloat),
                        _ => {}
                    }
                }
                bail(format!("native function '{}' not supported", nf.name))
            }
            _ => bail("value is not callable"),
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> R<(ir::Value, Ty)> {
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
                        Ty::List => bail("int(list)"),
                    }
                } else {
                    match t {
                        Ty::Float => Ok((v, Ty::Float)),
                        Ty::Int => Ok((self.to_f64(v), Ty::Float)),
                        _ => bail("float() of bool/nil not supported"),
                    }
                }
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
                let key = FnKey { ptr: Rc::as_ptr(&f.body) as usize, mask, n: args.len() as u8 };
                let (id, ret) = self.inner.specialize(&f, key, self.me)?;
                let fr = self.inner.module.declare_func_in_func(id, self.b.func);
                let inst = self.b.ins().call(fr, &vals);
                let r = self.b.inst_results(inst)[0];
                self.check_err();
                Ok((r, ret))
            }
        }
    }

    fn gen_math(&mut self, name: &str, args: &[Expr]) -> R<(ir::Value, Ty)> {
        let want = if name == "pow" { 2 } else { 1 };
        if args.len() != want {
            return bail(format!("math.{} expects {} argument(s)", name, want));
        }
        let (a, at) = self.expr(&args[0])?;
        match name {
            "sqrt" => {
                let f = match at {
                    Ty::Int => self.to_f64(a),
                    Ty::Float => a,
                    _ => return bail("sqrt requires a number"),
                };
                Ok((self.b.ins().sqrt(f), Ty::Float))
            }
            "abs" => match at {
                Ty::Int => {
                    let is_min = self.b.ins().icmp_imm_s(IntCC::Equal, a, i64::MIN);
                    self.trap_if(is_min, ops::CODE_OVERFLOW);
                    Ok((self.b.ins().iabs(a), Ty::Int))
                }
                Ty::Float => Ok((self.b.ins().fabs(a), Ty::Float)),
                _ => bail("abs requires a number"),
            },
            "floor" | "ceil" => match at {
                Ty::Int => Ok((a, Ty::Int)),
                Ty::Float => {
                    let r = if name == "floor" { self.b.ins().floor(a) } else { self.b.ins().ceil(a) };
                    Ok((self.b.ins().fcvt_to_sint_sat(types::I64, r), Ty::Int))
                }
                _ => bail("floor/ceil require a number"),
            },
            "pow" => {
                let (b, bt) = self.expr(&args[1])?;
                let x = match at {
                    Ty::Int => self.to_f64(a),
                    Ty::Float => a,
                    _ => return bail("pow requires numbers"),
                };
                let y = match bt {
                    Ty::Int => self.to_f64(b),
                    Ty::Float => b,
                    _ => return bail("pow requires numbers"),
                };
                Ok((self.float_arith(BinaryOp::Pow, x, y), Ty::Float))
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
                Ty::Unit => return bail("print of nil"),
                Ty::List => return bail("print of a list"),
            }
        }
        let fr = self.helper(Helper::PrintNl);
        self.b.ins().call(fr, &[]);
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
}

pub enum LoopOutcome {
    /// Not compilable (or JIT disabled): continue in the interpreter.
    Skip,
    /// The loop ran to completion in native code.
    Done,
    Err(String),
}

pub struct Jit {
    cfg: JitConfig,
    inner: Option<Inner>,
    init_failed: bool,
    counts: HashMap<usize, u32>,
    loop_names: HashMap<u32, Rc<Vec<String>>>,
}

impl Jit {
    pub fn new() -> Self {
        Jit {
            cfg: config(),
            inner: None,
            init_failed: false,
            counts: HashMap::new(),
            loop_names: HashMap::new(),
        }
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
            match Inner::new(self.cfg.debug) {
                Ok(i) => self.inner = Some(i),
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
        inner.ctx.depth = 0;
        let rem = sched::stack_remaining() as i64;
        inner.ctx.depth_limit = (rem / 1024).min(50_000_000);
    }

    /// Last line recorded by native code (for error messages).
    pub fn last_line(&self) -> u32 {
        self.inner.as_ref().map(|i| i.ctx.line as u32).unwrap_or(0)
    }

    /// Tries to run the function natively. `None` = use the interpreter.
    pub fn call_fn(&mut self, f: &Rc<FnValue>, args: &[Value]) -> Option<Result<Value, String>> {
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
        let key = FnKey { ptr: Rc::as_ptr(&f.body) as usize, mask, n: args.len() as u8 };

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
        let thunk: extern "C" fn(*mut JitCtx, *const u64, *mut u64) =
            unsafe { std::mem::transmute(addr) };
        thunk(&mut *inner.ctx, argbuf.as_ptr(), &mut out);

        if inner.ctx.err != 0 {
            return Some(Err(ops::code_message(inner.ctx.err).to_string()));
        }
        Some(Ok(match ret {
            Ty::Int => Value::Int(out as i64),
            Ty::Float => Value::Float(f64::from_bits(out)),
            Ty::Bool => Value::Bool(out & 0xff != 0),
            // functions never return lists (they are only loop slots)
            Ty::Unit | Ty::List => Value::Nil,
        }))
    }

    /// Tries to compile and run the loop from the current state of `env`.
    pub fn run_loop(
        &mut self,
        id: u32,
        kind: LoopKind,
        env: &Rc<RefCell<Environment>>,
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
                    LoopKind::For { body, .. } => collect_names(&mut set, body),
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
        for n in names.iter() {
            if let Some(b) = env.borrow().lookup(n) {
                // a list: its pointer, for `xs.push(n)` (the environment keeps it alive)
                if let Value::List(l) = &b.value {
                    slots.push((n.clone(), Ty::List, false));
                    init.push(Rc::as_ptr(l) as u64);
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
        if let LoopKind::For { lo, hi, .. } = &kind {
            init.push(*lo as u64);
            init.push(*hi as u64);
        }

        let key = LoopKey {
            id,
            is_for: matches!(kind, LoopKind::For { .. }),
            slots: slots.clone(),
        };

        let Some(inner) = self.inner() else {
            return LoopOutcome::Skip;
        };

        if !inner.loops.contains_key(&key) {
            let t0 = Instant::now();
            let entry = match inner.compile_loop(&kind, env, &slots) {
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
        let f: extern "C" fn(*mut JitCtx, *mut u64) -> i64 = unsafe { std::mem::transmute(addr) };
        f(&mut *inner.ctx, init.as_mut_ptr());

        // On error the modified variables are also written back (the native
        // code already wrote them to the slots before leaving).
        let failed = (inner.ctx.err != 0).then(|| ops::code_message(inner.ctx.err).to_string());
        for (i, (name, ty, _)) in slots.iter().enumerate() {
            if assigned[i] {
                let v = match ty {
                    Ty::Int => Value::Int(init[i] as i64),
                    Ty::Float => Value::Float(f64::from_bits(init[i])),
                    Ty::Bool => Value::Bool(init[i] & 0xff != 0),
                    // list slots are never assigned (they can't be reassigned in the loop)
                    Ty::Unit | Ty::List => Value::Nil,
                };
                if let Err(e) = env.borrow_mut().assign(name, v) {
                    return LoopOutcome::Err(e);
                }
            }
        }
        match failed {
            Some(e) => LoopOutcome::Err(e),
            None => LoopOutcome::Done,
        }
    }

    pub fn report(&self) -> String {
        let Some(i) = &self.inner else {
            return "[jit] no compilation was needed".into();
        };
        let s = &i.stats;
        let mut out = format!(
            "[jit] compiled functions: {} | compiled loops: {} | native calls: {} | native loops: {} | compile time: {:.2} ms",
            s.fns_compiled,
            s.loops_compiled,
            s.native_calls,
            s.native_loops,
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
