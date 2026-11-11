//! `fogu:ffi` module — calls functions of native libraries (C ABI).
//!
//! ```fogu
//! import ffi from "fogu:ffi"
//!
//! const libc = ffi.open("c")?                      // libc / msvcrt; or "user32", "./mylib.so"…
//! const strlen = libc.func("strlen", ["str"], "usize")?
//! print(strlen("hello"))                           // 5
//! const pow = libc.func("pow", ["f64", "f64"], "f64")?
//! print(pow(2.0, 10.0))                            // 1024.0
//! const buf = ffi.alloc(64)                        // out-parameters
//! const cmp = ffi.callback(fn(a, b) { return ffi.read(a, "i32") - ffi.read(b, "i32") }, ["ptr", "ptr"], "i32")
//! ```
//!
//! **Unsafe by nature**: a wrong signature can crash the program. Each
//! signature gets a native stub generated with Cranelift (any number of
//! arguments, integers, pointers, `f64` and `f32` mixed); on platforms without
//! a Cranelift backend (32-bit ARM) the precompiled signatures of
//! `ffi_calls.rs` are used (up to 4 arguments mixing integers and floats, or up
//! to 8 integers) and callbacks are not available. Types: `i8 i16 i32 i64 u8
//! u16 u32 u64 isize usize bool f32 f64 ptr str buffer` for arguments, the same
//! plus `void` for results. A missing library or symbol is an `error` value.
//!
//! Callbacks (`ffi.callback`) are C function pointers that run a Fogu function.
//! They run on the interpreter's thread while a C function called through
//! `fogu:ffi` is running (`qsort`, `EnumWindows`, event loops…); a call from
//! another thread, or when no ffi call is active, returns 0 to C and is
//! reported on stderr. An error inside a callback makes it return 0 for the
//! rest of the C call, which then fails with that error.

use std::cell::{Cell, RefCell};
use ahash::AHashMap as HashMap;
use std::ffi::{c_void, CStr, CString};
use std::rc::Rc;

use crate::runtime::environment::Environment;
use crate::runtime::ffi_stub::{self, Kind};
use crate::runtime::interpreter::Interpreter;
use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{list, max_args, nfn_var, str_arg, Num, TypedNative};

#[path = "ffi_calls.rs"]
mod calls;

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("open".into(), nfn_var("open", 1, open));
    e.insert("alloc".into(), nfn_var("alloc", 1, |a| {
        max_args(a, 1, "ffi.alloc")?;
        let n = match a.first() {
            Some(Value::Int(n)) if (0..=64 * 1024 * 1024).contains(n) => *n as usize,
            _ => return Err("ffi.alloc(size) expects a size in bytes (up to 64 MB)".into()),
        };
        Ok(new_buffer(vec![0u8; n]))
    }));
    e.insert("cstring".into(), nfn_var("cstring", 1, |a| {
        max_args(a, 1, "ffi.cstring")?;
        let s = str_arg(a, 0, "ffi.cstring")?;
        let c = CString::new(s).map_err(|_| "ffi.cstring: the text contains a NUL byte".to_string())?;
        Ok(new_buffer(c.into_bytes_with_nul()))
    }));
    e.insert("callback".into(), nfn_var("callback", 2, new_callback));
    e.insert("string".into(), nfn_var("string", 1, read_string));
    e.insert("bytes".into(), nfn_var("bytes", 2, read_bytes));
    e.insert("read".into(), nfn_var("read", 2, |a| {
        max_args(a, 3, "ffi.read")?;
        let p = address(a.first(), "ffi.read")?;
        let t = ty(&str_arg(a, 1, "ffi.read")?)?;
        let off = offset(a.get(2), "ffi.read")?;
        // SAFETY: the program gave the address; fogu:ffi is unsafe by nature.
        unsafe { read_at((p + off) as *const u8, t, "ffi.read") }
    }));
    e.insert("write".into(), nfn_var("write", 3, |a| {
        max_args(a, 4, "ffi.write")?;
        let p = address(a.first(), "ffi.write")?;
        let t = ty(&str_arg(a, 1, "ffi.write")?)?;
        let off = offset(a.get(3), "ffi.write")?;
        // SAFETY: as for ffi.read.
        unsafe { write_at((p + off) as *mut u8, t, &a[2], "ffi.write")? };
        Ok(Value::Nil)
    }));
    e.insert("sizeof".into(), nfn_var("sizeof", 1, |a| {
        max_args(a, 1, "ffi.sizeof")?;
        Ok(Value::Int(size_of(ty(&str_arg(a, 0, "ffi.sizeof")?)?) as i64))
    }));
    e.insert("lastError".into(), nfn_var("lastError", 0, |a| {
        max_args(a, 0, "ffi.lastError")?;
        Ok(Value::Int(LAST_ERRNO.with(Cell::get) as i64))
    }));
    e.insert("NULL".into(), Value::Int(0));
    e.insert("native".into(), Value::Bool(ffi_stub::caller(&[], Kind::Void).is_ok()));
    e.insert("types".into(), crate::native::util::str_list(TYPES.iter().map(|t| t.to_string())));
    e.insert("suffix".into(), Value::Str(std::env::consts::DLL_SUFFIX.into()));
    ModuleValue {
        name: "fogu:ffi".into(),
        exports: RefCell::new(e),
    }
}

const TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize", "bool", "f32", "f64", "ptr", "str", "buffer",
    "void",
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ty {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Bool,
    F32,
    F64,
    Ptr,
    Str,
    Buffer,
    Void,
}

fn ty(name: &str) -> Result<Ty, String> {
    Ok(match name {
        // the C names, the Rust names and the language's sized types (int8 … float64)
        "i8" | "char" | "int8" => Ty::I8,
        "i16" | "short" | "int16" => Ty::I16,
        "i32" | "int" | "int32" | "rune" => Ty::I32,
        "i64" | "long long" | "isize" | "int64" => Ty::I64,
        "u8" | "uchar" | "uint8" | "byte" => Ty::U8,
        "u16" | "ushort" | "uint16" => Ty::U16,
        "u32" | "uint" | "uint32" => Ty::U32,
        "u64" | "usize" | "size_t" | "uint64" | "uintptr" => Ty::U64,
        "bool" => Ty::Bool,
        "f32" | "float" | "float32" => Ty::F32,
        "f64" | "double" | "float64" => Ty::F64,
        "ptr" | "pointer" | "callback" => Ty::Ptr,
        "str" | "string" | "cstring" => Ty::Str,
        "buffer" => Ty::Buffer,
        "void" => Ty::Void,
        other => return Err(format!("ffi: unknown type '{}' ({})", other, TYPES.join(" "))),
    })
}

impl Ty {
    fn kind(self) -> Kind {
        match self {
            Ty::F64 => Kind::F64,
            Ty::F32 => Kind::F32,
            Ty::Void => Kind::Void,
            _ => Kind::Int,
        }
    }
}

fn size_of(t: Ty) -> usize {
    match t {
        Ty::I8 | Ty::U8 | Ty::Bool => 1,
        Ty::I16 | Ty::U16 => 2,
        Ty::I32 | Ty::U32 | Ty::F32 => 4,
        Ty::I64 | Ty::U64 | Ty::F64 => 8,
        Ty::Ptr | Ty::Str | Ty::Buffer => std::mem::size_of::<usize>(),
        Ty::Void => 0,
    }
}

/// The parameter types of `func`/`callback` (a list of type names).
fn param_types(v: Option<&Value>, what: &str) -> Result<Vec<Ty>, String> {
    match v {
        None | Some(Value::Nil) => Ok(Vec::new()),
        Some(Value::List(l)) => l.borrow().iter().map(|v| ty(&v.to_display_string())).collect(),
        Some(other) => Err(format!("{}: argument types must be a list, got '{}'", what, other.type_name())),
    }
}

/// An argument as the CPU sees it: an integer register or a float register.
#[derive(Clone, Copy, Debug)]
pub enum Arg {
    I(i64),
    F(f64),
}

// ---------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------

/// The library naming of a platform: `libx.so` (Linux), `libx.dylib` (macOS), `x.dll` (Windows).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Platform {
    Linux,
    MacOs,
    Windows,
}

impl Platform {
    fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
    fn prefix(self) -> &'static str {
        if self == Platform::Windows { "" } else { "lib" }
    }
    fn suffix(self) -> &'static str {
        match self {
            Platform::Linux => ".so",
            Platform::MacOs => ".dylib",
            Platform::Windows => ".dll",
        }
    }
}

fn candidates(name: &str) -> Vec<String> {
    candidates_for(name, Platform::current())
}

/// The files `ffi.open(name)` tries, in order, so one script works on every
/// platform: `"c"`/`"m"` are the C runtime and math library; a name without an
/// extension gets the platform's prefix and suffix (`"dir/fogu_x"` →
/// `dir/libfogu_x.so`, `dir/libfogu_x.dylib`, `dir/fogu_x.dll`); a name with
/// another platform's extension (`"x.dll"` on Linux) is also tried with the
/// native one (`libx.so`).
fn candidates_for(name: &str, os: Platform) -> Vec<String> {
    if name == "c" || name == "libc" {
        return match os {
            Platform::Windows => vec!["msvcrt.dll".into(), "ucrtbase.dll".into()],
            Platform::MacOs => vec!["libc.dylib".into(), "/usr/lib/libSystem.B.dylib".into()],
            Platform::Linux => vec!["libc.so.6".into(), "libc.so".into()],
        };
    }
    if name == "m" || name == "libm" {
        return match os {
            Platform::Windows => vec!["msvcrt.dll".into()],
            Platform::MacOs => vec!["libm.dylib".into(), "/usr/lib/libSystem.B.dylib".into()],
            Platform::Linux => vec!["libm.so.6".into(), "libm.so".into()],
        };
    }
    let split = name.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (dir, file) = name.split_at(split);
    // the file name without a library extension (also versioned: libx.so.6)
    let stem = [".dll", ".dylib", ".so"]
        .iter()
        .find_map(|ext| match file.find(ext) {
            Some(i) if file[i + ext.len()..].is_empty() || file[i + ext.len()..].starts_with('.') => Some(&file[..i]),
            _ => None,
        });
    let native = match stem {
        Some(_) => file.ends_with(os.suffix()) || (os == Platform::Linux && file.contains(".so.")),
        None => false,
    };
    let mut out = vec![name.to_string()];
    if !native {
        let stem = stem.unwrap_or(file);
        let bare = stem.strip_prefix("lib").filter(|s| !s.is_empty()).unwrap_or(stem);
        out.push(format!("{dir}{stem}{}", os.suffix()));
        out.push(format!("{dir}{}{bare}{}", os.prefix(), os.suffix()));
    }
    out.dedup();
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(c.clone()));
    out
}

fn open(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "ffi.open")?;
    let name = str_arg(args, 0, "ffi.open")?;
    let tried = candidates(&name);
    let mut last = String::new();
    for cand in &tried {
        // SAFETY: loading a library runs its initialization code; that is
        // what the program asked for.
        match unsafe { libloading::Library::new(cand) } {
            Ok(lib) => {
                return Ok(NativeObjectRef::new(LibValue { lib: Rc::new(lib), name: cand.clone() }).to_value());
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(io_err!("ffi.open: cannot load '{}' (tried {}): {}", name, tried.join(", "), last))
}

pub struct LibValue {
    lib: Rc<libloading::Library>,
    name: String,
}

impl LibValue {
    fn symbol(&self, symbol: &str, what: &str) -> Result<usize, String> {
        let cname = CString::new(symbol).map_err(|_| format!("{}: invalid symbol name", what))?;
        // SAFETY: we only read the address.
        unsafe {
            self.lib
                .get::<*const c_void>(cname.as_bytes_with_nul())
                .map(|s| *s as usize)
                .map_err(|e| io_err!("ffi: {}: no symbol '{}': {}", self.name, symbol, e))
        }
    }
}

impl FoguObject for LibValue {
    fn name(&self) -> &str {
        "Library"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "name").then(|| Value::Str(self.name.clone().into()))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // func(name, [argTypes], returnType = "void") → a callable function
            "func" | "fn" => {
                max_args(args, 3, "func")?;
                let symbol = str_arg(args, 0, "func")?;
                let params = param_types(args.get(1), "func")?;
                if params.contains(&Ty::Void) {
                    return Err("func: 'void' is only valid as the return type".into());
                }
                let ret = match args.get(2) {
                    None | Some(Value::Nil) => Ty::Void,
                    Some(v) => ty(&v.to_display_string())?,
                };
                if ret == Ty::Buffer {
                    return Err("func: 'buffer' is only valid as an argument (return a 'ptr')".into());
                }
                let kinds: Vec<Kind> = params.iter().map(|t| t.kind()).collect();
                let stub = ffi_stub::caller(&kinds, ret.kind()).ok();
                if stub.is_none() {
                    let floats = params.iter().filter(|t| **t == Ty::F64).count();
                    if params.contains(&Ty::F32) {
                        return Err("func: 'f32' arguments need native stubs, not available on this platform".into());
                    }
                    if params.len() > 8 || (params.len() > 4 && floats > 0) {
                        return Err("func: up to 4 arguments when floats are used, up to 8 integers/pointers otherwise".into());
                    }
                } else if params.len() > MAX_PARAMS {
                    return Err("func: up to 32 arguments".into());
                }
                let addr = self.symbol(&symbol, "func")?;
                let typed = if stub.is_some() { direct_entry(addr, &params, ret) } else { None };
                let f = FnInfo { lib: Rc::clone(&self.lib), addr, params, ret, name: symbol, stub };
                Ok(native_fn(f, typed))
            }
            "has" => {
                let symbol = str_arg(args, 0, "has")?;
                Ok(Value::Bool(self.symbol(&symbol, "has").is_ok()))
            }
            // addr(name) → the address of a symbol (a function or a global variable)
            "addr" | "symbol" => {
                max_args(args, 1, method)?;
                let symbol = str_arg(args, 0, method)?;
                Ok(Value::Int(self.symbol(&symbol, method)? as i64))
            }
            other => Err(format!("method '{}' does not exist on Library (func has addr)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["func", "has", "addr"]
    }
}

// ---------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------

/// The most parameters a C function called through `fogu:ffi` may have.
const MAX_PARAMS: usize = 32;

struct FnInfo {
    /// keeps the library loaded while the function exists
    #[allow(dead_code)]
    lib: Rc<libloading::Library>,
    addr: usize,
    params: Vec<Ty>,
    ret: Ty,
    name: String,
    /// the generated caller, or `None` for the precompiled signatures
    stub: Option<ffi_stub::Caller>,
}

thread_local! {
    /// Native functions are plain `fn` pointers: the declared signatures
    /// live here, found by the function's name (`ffi#<id>`).
    static FUNCS: RefCell<Vec<Rc<FnInfo>>> = const { RefCell::new(Vec::new()) };
    /// `errno` (`GetLastError` on Windows) right after the last ffi call.
    static LAST_ERRNO: Cell<i32> = const { Cell::new(0) };
}

/// The entry compiled code calls directly, for a signature of numbers only
/// (integers, pointers given as ints, `f64`, `bool`): no `Value` is built and
/// the interpreter is not involved. It does what [`call_ffi`] does around
/// the call — the result takes its declared width, `errno` is kept, an error
/// of a callback is raised — so both paths behave the same.
fn direct_entry(addr: usize, params: &[Ty], ret: Ty) -> Option<TypedNative> {
    use ffi_stub::{DirectParam, DirectRet};
    let mut dp = Vec::with_capacity(params.len());
    let mut nums = Vec::with_capacity(params.len());
    for p in params {
        let (d, n) = match p {
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::Ptr => (DirectParam::Int, Num::Int),
            Ty::F64 => (DirectParam::F64, Num::Float),
            Ty::Bool => (DirectParam::Bool, Num::Bool),
            // text and buffers need the interpreter; an int given as `f32`
            // would round differently here
            Ty::F32 | Ty::Str | Ty::Buffer | Ty::Void => return None,
        };
        dp.push(d);
        nums.push(n);
    }
    let (dr, rn) = match ret {
        Ty::I8 => (DirectRet::Signed(1), Num::Int),
        Ty::I16 => (DirectRet::Signed(2), Num::Int),
        Ty::I32 => (DirectRet::Signed(4), Num::Int),
        Ty::U8 => (DirectRet::Unsigned(1), Num::Int),
        Ty::U16 => (DirectRet::Unsigned(2), Num::Int),
        Ty::U32 => (DirectRet::Unsigned(4), Num::Int),
        Ty::I64 | Ty::U64 | Ty::Ptr => (DirectRet::Signed(8), Num::Int),
        Ty::Bool => (DirectRet::Bool, Num::Bool),
        Ty::F64 => (DirectRet::F64, Num::Float),
        Ty::F32 => (DirectRet::F32, Num::Float),
        // nil and text are not numbers
        Ty::Void | Ty::Str | Ty::Buffer => return None,
    };
    let entry = ffi_stub::direct(addr, &dp, dr, after_direct).ok()?;
    // lives as long as the function may be called: the program's life
    let params: &'static [Num] = Box::leak(nums.into_boxed_slice());
    Some(TypedNative { params, ret: rn, entry, fallible: true })
}

/// After a direct call (see [`direct_entry`]): what [`call_ffi`] does after its own.
extern "C" fn after_direct() {
    LAST_ERRNO.with(|e| e.set(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
    if let Some(err) = PENDING.with(|p| p.borrow_mut().take()) {
        crate::native::util::set_typed_error(err);
    }
}

fn native_fn(f: FnInfo, typed: Option<TypedNative>) -> Value {
    let (name, arity) = (f.name.clone(), f.params.len());
    let id = FUNCS.with(|fs| {
        let mut fs = fs.borrow_mut();
        fs.push(Rc::new(f));
        fs.len() - 1
    });
    // the id travels in the function's name; `call_ffi` reads it
    Value::NativeFn(Rc::new(NativeFnValue {
        name: format!("{}#ffi{}", name, id),
        arity,
        variadic: false,
        func: call_ffi_placeholder,
        intercept: Default::default(),
        typed,
    }))
}

fn call_ffi_placeholder(_: &[Value]) -> Result<Value, String> {
    Err("ffi: internal error: call through call_ffi".into())
}

/// Is this native function an ffi function? (name `symbol#ffiN`)
///
/// Runs on every ffi call: reads the digits from the end, no search.
#[inline]
pub fn ffi_id(name: &str) -> Option<usize> {
    let b = name.as_bytes();
    let mut i = b.len();
    let (mut id, mut mult) = (0usize, 1usize);
    while i > 0 && b[i - 1].is_ascii_digit() {
        id = id.checked_add((b[i - 1] - b'0') as usize * mult)?;
        mult = mult.checked_mul(10)?;
        i -= 1;
    }
    (i < b.len() && i >= 4 && &b[i - 4..i] == b"#ffi").then_some(id)
}

/// Values a call must keep alive until the C function returns.
#[derive(Default)]
struct KeepAlive {
    cstrings: Vec<CString>,
    buffers: Vec<Rc<RefCell<Vec<u8>>>>,
}

/// A Fogu value as a C argument word of type `t`.
fn to_word(t: Ty, v: &Value, keep: &mut KeepAlive, what: &dyn Fn() -> String) -> Result<u64, String> {
    let bad = || format!("{} must be {:?}, got '{}'", what(), t, v.type_name());
    Ok(match (t, v) {
        (Ty::F64, Value::Float(x)) => x.to_bits(),
        (Ty::F64, Value::Int(n)) => (*n as f64).to_bits(),
        (Ty::F32, Value::Float(x)) => u64::from((*x as f32).to_bits()),
        (Ty::F32, Value::Int(n)) => u64::from((*n as f32).to_bits()),
        (Ty::Bool, Value::Bool(b)) => u64::from(*b),
        (Ty::Str, Value::Str(s)) => {
            let c = CString::new(s.as_str()).map_err(|_| format!("{} contains a NUL byte", what()))?;
            let p = c.as_ptr() as u64;
            keep.cstrings.push(c);
            p
        }
        (Ty::Str | Ty::Ptr | Ty::Buffer, Value::Nil) => 0,
        (Ty::Buffer | Ty::Ptr, Value::Object(o)) => {
            let inner = o.inner();
            let b = inner.borrow();
            if let Some(buf) = b.as_any().downcast_ref::<Buffer>() {
                let p = buf.data.borrow_mut().as_mut_ptr() as u64;
                keep.buffers.push(Rc::clone(&buf.data));
                p
            } else if let Some(cb) = b.as_any().downcast_ref::<Callback>() {
                if t == Ty::Buffer {
                    return Err(bad());
                }
                cb.addr as u64
            } else {
                return Err(bad());
            }
        }
        (Ty::F64 | Ty::F32 | Ty::Str | Ty::Buffer | Ty::Bool | Ty::Void, _) => return Err(bad()),
        (_, Value::Int(n)) => *n as u64,
        (_, Value::Bool(b)) => u64::from(*b),
        _ => return Err(bad()),
    })
}

/// A C result word of type `t` as a Fogu value.
///
/// # Safety
/// For `str`, `r` must be NULL or point to a NUL-terminated string.
unsafe fn from_word(t: Ty, r: u64) -> Value {
    match t {
        Ty::Void => Value::Nil,
        Ty::F64 => Value::Float(f64::from_bits(r)),
        Ty::F32 => Value::Float(f32::from_bits(r as u32) as f64),
        Ty::I8 => Value::Int(r as i8 as i64),
        Ty::I16 => Value::Int(r as i16 as i64),
        Ty::I32 => Value::Int(r as i32 as i64),
        Ty::U8 => Value::Int(r as u8 as i64),
        Ty::U16 => Value::Int(r as u16 as i64),
        Ty::U32 => Value::Int(r as u32 as i64),
        Ty::Bool => Value::Bool(r as u8 != 0),
        Ty::Str if r == 0 => Value::Nil,
        // SAFETY: see above.
        Ty::Str => Value::Str(unsafe { CStr::from_ptr(r as *const std::ffi::c_char) }.to_string_lossy().into_owned().into()),
        _ => Value::Int(r as i64),
    }
}

/// Calls ffi function `id` (the interpreter routes `symbol#ffiN` here). Fogu
/// callbacks invoked by the C function during the call run on `it`.
pub fn call_ffi(id: usize, args: &[Value], it: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    // cloned out: a callback may create or call other ffi functions
    let f = FUNCS.with(|fs| fs.borrow().get(id).cloned()).ok_or("ffi: unknown function")?;
    if args.len() != f.params.len() {
        return Err(format!("{} expects {} arguments, got {}", f.name, f.params.len(), args.len()));
    }
    let mut keep = KeepAlive::default();
    // the argument words live on the stack (a signature has at most 32 parameters)
    let mut buf = [0u64; MAX_PARAMS];
    let words = &mut buf[..args.len()];
    for (i, (t, v)) in f.params.iter().zip(args).enumerate() {
        words[i] = to_word(*t, v, &mut keep, &|| format!("{}: argument {}", f.name, i + 1))?;
    }
    let _active = Active::enter(it, env);
    let mut out = 0u64;
    // SAFETY: the program declared the signature; a wrong one is undefined behaviour,
    // which is why fogu:ffi is documented as unsafe.
    unsafe {
        match f.stub {
            Some(stub) => stub(f.addr, words.as_ptr(), &mut out),
            None => {
                let raw: Vec<Arg> = f
                    .params
                    .iter()
                    .zip(words.iter())
                    .map(|(t, w)| if *t == Ty::F64 { Arg::F(f64::from_bits(*w)) } else { Arg::I(*w as i64) })
                    .collect();
                let addr = f.addr as *const c_void;
                out = match f.ret {
                    Ty::Void => {
                        calls::call::<()>(addr, &raw)?;
                        0
                    }
                    Ty::F64 => calls::call::<f64>(addr, &raw)?.to_bits(),
                    Ty::F32 => u64::from(calls::call::<f32>(addr, &raw)?.to_bits()),
                    _ => calls::call::<i64>(addr, &raw)? as u64,
                };
            }
        }
    }
    LAST_ERRNO.with(|e| e.set(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
    drop(keep);
    if let Some(err) = PENDING.with(|p| p.borrow_mut().take()) {
        return Err(err);
    }
    // SAFETY: a `str` result is a C string by the declared signature.
    Ok(unsafe { from_word(f.ret, out) })
}

// ---------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------

struct CbInfo {
    func: Value,
    params: Vec<Ty>,
    ret: Ty,
}

/// The interpreter running an ffi call (callbacks run on it).
struct Ctx {
    it: *const Interpreter,
    env: Rc<RefCell<Environment>>,
}

thread_local! {
    static CALLBACKS: RefCell<Vec<Option<Rc<CbInfo>>>> = const { RefCell::new(Vec::new()) };
    static ACTIVE: RefCell<Vec<Ctx>> = const { RefCell::new(Vec::new()) };
    /// The first error raised by a callback during the current ffi call.
    static PENDING: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Marks an ffi call as running (nested calls stack up).
struct Active;

impl Active {
    fn enter(it: &Interpreter, env: &Rc<RefCell<Environment>>) -> Active {
        ACTIVE.with(|a| a.borrow_mut().push(Ctx { it: it as *const _, env: Rc::clone(env) }));
        Active
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.borrow_mut().pop());
    }
}

/// ffi.callback(fn, [argTypes], returnType = "void") → a Callback (a C function pointer)
fn new_callback(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "ffi.callback")?;
    let func = match &a[0] {
        f @ (Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_)) => f.clone(),
        other => return Err(format!("ffi.callback(fn, types, ret) expects a function, got '{}'", other.type_name())),
    };
    let params = param_types(a.get(1), "ffi.callback")?;
    if params.iter().any(|t| matches!(t, Ty::Void | Ty::Buffer)) {
        return Err("ffi.callback: 'void' and 'buffer' are not valid parameter types (use 'ptr')".into());
    }
    let ret = match a.get(2) {
        None | Some(Value::Nil) => Ty::Void,
        Some(v) => ty(&v.to_display_string())?,
    };
    if matches!(ret, Ty::Str | Ty::Buffer) {
        return Err("ffi.callback: a callback cannot return 'str' or 'buffer' (return a 'ptr')".into());
    }
    let kinds: Vec<Kind> = params.iter().map(|t| t.kind()).collect();
    let info = Rc::new(CbInfo { func, params: params.clone(), ret });
    let id = CALLBACKS.with(|c| {
        let mut c = c.borrow_mut();
        c.push(Some(info));
        c.len() - 1
    });
    let addr = ffi_stub::callback(&kinds, ret.kind(), dispatch, id as u64).map_err(|e| {
        CALLBACKS.with(|c| c.borrow_mut()[id] = None);
        format!("ffi.callback: {}", e)
    })?;
    Ok(NativeObjectRef::new(Callback { id, addr }).to_value())
}

/// Called by every callback stub: runs the Fogu function of callback `id`.
extern "C" fn dispatch(id: u64, args: *const u64, out: *mut u64) {
    // a panic must not unwind into C
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_callback(id as usize, args)));
    let word = match r {
        Ok(Ok(w)) => w,
        Ok(Err(e)) => {
            PENDING.with(|p| p.borrow_mut().get_or_insert(e).clone());
            0
        }
        Err(_) => {
            PENDING.with(|p| p.borrow_mut().get_or_insert("ffi: a callback panicked".into()).clone());
            0
        }
    };
    // SAFETY: the stub passes a valid slot for the result.
    unsafe { *out = word };
}

fn run_callback(id: usize, args: *const u64) -> Result<u64, String> {
    if PENDING.with(|p| p.borrow().is_some()) {
        return Ok(0); // an earlier callback of this call failed
    }
    let info = CALLBACKS.with(|c| c.borrow().get(id).cloned().flatten());
    let Some(info) = info else {
        eprintln!("ffi: a closed callback was called (returning 0)");
        return Ok(0);
    };
    let ctx = ACTIVE.with(|a| a.borrow().last().map(|c| (c.it, Rc::clone(&c.env)))).or_else(|| {
        // called by a C function that compiled code called directly
        let it = crate::runtime::interpreter::native_interpreter()?;
        // SAFETY: that interpreter's native code is on the stack below us.
        Some((it, Rc::clone(unsafe { &*it }.globals_ref())))
    });
    let Some((it, env)) = ctx else {
        eprintln!("ffi: a callback was called outside an ffi call or from another thread (returning 0)");
        return Ok(0);
    };
    let mut vals = Vec::with_capacity(info.params.len());
    for (i, t) in info.params.iter().enumerate() {
        // SAFETY: the stub stored one word per declared parameter; `str` is a C string by the signature.
        vals.push(unsafe { from_word(*t, *args.add(i)) });
    }
    // SAFETY: the interpreter outlives the ffi call that registered `ctx`.
    let it = unsafe { &*it };
    let r = it.call_value(info.func.clone(), vals, env)?;
    let mut keep = KeepAlive::default();
    let w = match (info.ret, &r) {
        (Ty::Void, _) => 0,
        (_, Value::Nil) => 0,
        (t, v) => to_word(t, v, &mut keep, &|| "ffi.callback: the result".to_string())?,
    };
    Ok(w)
}

/// `ffi.callback(...)`: pass it where the C function expects a function pointer (`ptr`).
pub struct Callback {
    id: usize,
    addr: usize,
}

impl FoguObject for Callback {
    fn name(&self) -> &str {
        "Callback"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "address" | "ptr" => Some(Value::Int(self.addr as i64)),
            "closed" => Some(Value::Bool(CALLBACKS.with(|c| c.borrow()[self.id].is_none()))),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // close(): releases the Fogu function; later calls from C return 0
            "close" => {
                max_args(args, 0, "close")?;
                CALLBACKS.with(|c| c.borrow_mut()[self.id] = None);
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Callback (close)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------
// Raw memory
// ---------------------------------------------------------------------

fn address(v: Option<&Value>, what: &str) -> Result<usize, String> {
    match v {
        Some(Value::Int(p)) if *p != 0 => Ok(*p as usize),
        Some(Value::Int(_)) | Some(Value::Nil) => Err(format!("{}: NULL pointer", what)),
        Some(Value::Object(o)) => {
            let inner = o.inner();
            let b = inner.borrow();
            if let Some(buf) = b.as_any().downcast_ref::<Buffer>() {
                Ok(buf.data.borrow_mut().as_mut_ptr() as usize)
            } else {
                Err(format!("{}: expected an address (int) or a Buffer", what))
            }
        }
        _ => Err(format!("{}: expected an address (int) or a Buffer", what)),
    }
}

fn offset(v: Option<&Value>, what: &str) -> Result<usize, String> {
    match v {
        None | Some(Value::Nil) => Ok(0),
        Some(Value::Int(n)) if *n >= 0 => Ok(*n as usize),
        _ => Err(format!("{}: the offset must be an int >= 0", what)),
    }
}

/// Reads a value of type `t` at `p`.
///
/// # Safety
/// `p` must point to readable memory holding a `t`.
unsafe fn read_at(p: *const u8, t: Ty, what: &str) -> Result<Value, String> {
    // SAFETY: see above (unaligned reads are fine for any address).
    unsafe {
        Ok(match t {
            Ty::I8 => Value::Int(p.cast::<i8>().read_unaligned() as i64),
            Ty::U8 => Value::Int(p.read_unaligned() as i64),
            Ty::Bool => Value::Bool(p.read_unaligned() != 0),
            Ty::I16 => Value::Int(p.cast::<i16>().read_unaligned() as i64),
            Ty::U16 => Value::Int(p.cast::<u16>().read_unaligned() as i64),
            Ty::I32 => Value::Int(p.cast::<i32>().read_unaligned() as i64),
            Ty::U32 => Value::Int(p.cast::<u32>().read_unaligned() as i64),
            Ty::I64 | Ty::U64 => Value::Int(p.cast::<i64>().read_unaligned()),
            Ty::F32 => Value::Float(p.cast::<f32>().read_unaligned() as f64),
            Ty::F64 => Value::Float(p.cast::<f64>().read_unaligned()),
            Ty::Ptr => Value::Int(p.cast::<usize>().read_unaligned() as i64),
            Ty::Str => from_word(Ty::Str, p.cast::<usize>().read_unaligned() as u64),
            Ty::Buffer | Ty::Void => return Err(format!("{}: cannot read a '{:?}'", what, t)),
        })
    }
}

/// Writes `v` as a `t` at `p`.
///
/// # Safety
/// `p` must point to writable memory with room for a `t`.
unsafe fn write_at(p: *mut u8, t: Ty, v: &Value, what: &str) -> Result<(), String> {
    let n = match v {
        Value::Int(n) => *n,
        Value::Bool(b) => i64::from(*b),
        Value::Float(_) if matches!(t, Ty::F32 | Ty::F64) => 0,
        Value::Object(_) if t == Ty::Ptr => address(Some(v), what).or_else(|_| {
            let mut keep = KeepAlive::default();
            to_word(Ty::Ptr, v, &mut keep, &|| what.to_string()).map(|w| w as usize)
        })? as i64,
        other => return Err(format!("{}: cannot write a '{}' as {:?}", what, other.type_name(), t)),
    };
    let x = match v {
        Value::Float(x) => *x,
        _ => n as f64,
    };
    // SAFETY: see above.
    unsafe {
        match t {
            Ty::I8 | Ty::U8 | Ty::Bool => p.write_unaligned(n as u8),
            Ty::I16 | Ty::U16 => p.cast::<u16>().write_unaligned(n as u16),
            Ty::I32 | Ty::U32 => p.cast::<u32>().write_unaligned(n as u32),
            Ty::I64 | Ty::U64 => p.cast::<i64>().write_unaligned(n),
            Ty::Ptr => p.cast::<usize>().write_unaligned(n as usize),
            Ty::F32 => p.cast::<f32>().write_unaligned(x as f32),
            Ty::F64 => p.cast::<f64>().write_unaligned(x),
            Ty::Str | Ty::Buffer | Ty::Void => return Err(format!("{}: cannot write a '{:?}' (use a Buffer)", what, t)),
        }
    }
    Ok(())
}

/// ffi.string(ptr, len?): the C string at `ptr` (up to NUL, or `len` bytes).
fn read_string(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ffi.string")?;
    if matches!(a.first(), Some(Value::Int(0)) | Some(Value::Nil)) {
        return Ok(Value::Nil);
    }
    let p = address(a.first(), "ffi.string")?;
    // SAFETY: the program gave the address; fogu:ffi is unsafe by nature.
    let s = unsafe {
        match a.get(1) {
            None | Some(Value::Nil) => CStr::from_ptr(p as *const std::ffi::c_char).to_string_lossy().into_owned(),
            Some(Value::Int(n)) if *n >= 0 => String::from_utf8_lossy(std::slice::from_raw_parts(p as *const u8, *n as usize)).into_owned(),
            _ => return Err("ffi.string(ptr, len?): len must be an int >= 0".into()),
        }
    };
    Ok(Value::Str(s.into()))
}

/// ffi.bytes(ptr, len): a copy of `len` bytes at `ptr`, as a buffer.
fn read_bytes(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ffi.bytes")?;
    let p = address(a.first(), "ffi.bytes")?;
    let n = match a.get(1) {
        Some(Value::Int(n)) if (0..=1 << 30).contains(n) => *n as usize,
        _ => return Err("ffi.bytes(ptr, len): len must be an int between 0 and 1 GB".into()),
    };
    // SAFETY: as for ffi.string.
    Ok(Value::buffer(unsafe { std::slice::from_raw_parts(p as *const u8, n) }.to_vec()))
}

// ---------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------

fn new_buffer(data: Vec<u8>) -> Value {
    NativeObjectRef::new(Buffer { data: Rc::new(RefCell::new(data)) }).to_value()
}

/// Memory for out-parameters and structs: `ffi.alloc(n)`, `ffi.cstring(text)`.
pub struct Buffer {
    data: Rc<RefCell<Vec<u8>>>,
}

impl Buffer {
    /// The address of `size` bytes at `off`, checked against the buffer.
    fn at(&self, off: usize, size: usize, what: &str) -> Result<*mut u8, String> {
        let mut d = self.data.borrow_mut();
        if off.checked_add(size).is_none_or(|end| end > d.len()) {
            return Err(format!("{}: offset {} is outside the buffer ({} bytes)", what, off, d.len()));
        }
        // SAFETY: in bounds (checked above).
        Ok(unsafe { d.as_mut_ptr().add(off) })
    }
}

impl FoguObject for Buffer {
    fn name(&self) -> &str {
        "Buffer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "size" => Value::Int(self.data.borrow().len() as i64),
            "address" => Value::Int(self.data.borrow_mut().as_mut_ptr() as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let data = &self.data;
        match method {
            // bytes(start = 0, len = all) → list of ints
            "bytes" => {
                let d = data.borrow();
                let (s, n) = range_args(args, d.len())?;
                Ok(list(d[s..s + n].iter().map(|b| Value::Int(*b as i64)).collect()))
            }
            // text(): up to the first NUL byte, as UTF-8
            "text" => {
                let d = data.borrow();
                let end = d.iter().position(|b| *b == 0).unwrap_or(d.len());
                Ok(Value::Str(String::from_utf8_lossy(&d[..end]).into_owned().into()))
            }
            // write(offset, str | [bytes] | buffer)
            "write" => {
                max_args(args, 2, "write")?;
                let off = match args.first() {
                    Some(Value::Int(o)) if *o >= 0 => *o as usize,
                    _ => return Err("write(offset, data) expects an offset >= 0".into()),
                };
                let bytes: Vec<u8> = match args.get(1) {
                    Some(Value::Str(s)) => s.as_bytes().to_vec(),
                    Some(Value::List(l)) => l
                        .borrow()
                        .iter()
                        .map(|v| match v {
                            Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                            _ => Err("write: bytes must be ints 0..255".to_string()),
                        })
                        .collect::<Result<_, _>>()?,
                    Some(v @ Value::Buffer(_)) => crate::native::util::bytes_of(v, "write")?,
                    _ => return Err("write(offset, data) expects a str, a list of bytes or a buffer".into()),
                };
                let mut d = data.borrow_mut();
                if off + bytes.len() > d.len() {
                    return Err(format!("write: {} bytes at {} do not fit in {} bytes", bytes.len(), off, d.len()));
                }
                d[off..off + bytes.len()].copy_from_slice(&bytes);
                Ok(Value::Nil)
            }
            // get(type, offset = 0) · set(type, offset, value): typed little-endian access
            "get" => {
                max_args(args, 2, "get")?;
                let t = ty(&str_arg(args, 0, "get")?)?;
                let off = offset(args.get(1), "get")?;
                let p = self.at(off, size_of(t), "get")?;
                // SAFETY: in bounds.
                unsafe { read_at(p, t, "get") }
            }
            "set" => {
                max_args(args, 3, "set")?;
                let t = ty(&str_arg(args, 0, "set")?)?;
                let off = offset(args.get(1), "set")?;
                let v = args.get(2).ok_or("set(type, offset, value) expects a value")?;
                let p = self.at(off, size_of(t), "set")?;
                // SAFETY: in bounds.
                unsafe { write_at(p, t, v, "set")? };
                Ok(Value::Nil)
            }
            // ptr(offset = 0): the address of a byte of the buffer
            "ptr" => {
                max_args(args, 1, "ptr")?;
                let off = offset(args.first(), "ptr")?;
                Ok(Value::Int(self.at(off, 0, "ptr")? as i64))
            }
            // toBuffer(): a copy as a Fogu `buffer`
            "toBuffer" => Ok(Value::buffer(data.borrow().clone())),
            // i8(offset) … f64(offset): typed reads (the same as get(type, offset))
            "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" | "f32" | "f64" => {
                max_args(args, 1, method)?;
                let off = match args.first() {
                    Some(Value::Int(o)) if *o >= 0 => *o as usize,
                    _ => return Err(format!("{}(offset) expects an offset >= 0", method)),
                };
                let t = ty(method)?;
                let p = self.at(off, size_of(t), method).map_err(|_| format!("{}: offset {} is outside the buffer", method, off))?;
                // SAFETY: in bounds.
                unsafe { read_at(p, t, method) }
            }
            other => Err(format!(
                "method '{}' does not exist on Buffer (bytes text write get set ptr toBuffer i8 u8 i16 u16 i32 u32 i64 u64 f32 f64)",
                other
            )),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["bytes", "text", "write", "get", "set", "ptr", "toBuffer", "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "f32", "f64"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn range_args(args: &[Value], len: usize) -> Result<(usize, usize), String> {
    let s = match args.first() {
        None | Some(Value::Nil) => 0,
        Some(Value::Int(n)) if *n >= 0 && (*n as usize) <= len => *n as usize,
        _ => return Err("bytes(start, len): start is outside the buffer".into()),
    };
    let n = match args.get(1) {
        None | Some(Value::Nil) => len - s,
        Some(Value::Int(n)) if *n >= 0 && s + (*n as usize) <= len => *n as usize,
        _ => return Err("bytes(start, len): the range is outside the buffer".into()),
    };
    Ok((s, n))
}

#[cfg(test)]
mod tests {
    use super::{Platform, candidates_for};

    #[test]
    fn bare_name_gets_prefix_and_suffix() {
        assert_eq!(candidates_for("dir/fogu_x", Platform::Linux), ["dir/fogu_x", "dir/fogu_x.so", "dir/libfogu_x.so"]);
        assert_eq!(candidates_for("dir/fogu_x", Platform::MacOs), ["dir/fogu_x", "dir/fogu_x.dylib", "dir/libfogu_x.dylib"]);
        assert_eq!(candidates_for("dir\\fogu_x", Platform::Windows), ["dir\\fogu_x", "dir\\fogu_x.dll"]);
        assert_eq!(candidates_for("sqlite3", Platform::Linux), ["sqlite3", "sqlite3.so", "libsqlite3.so"]);
    }

    #[test]
    fn foreign_extension_maps_to_native() {
        assert_eq!(candidates_for("lib/x.dll", Platform::Linux), ["lib/x.dll", "lib/x.so", "lib/libx.so"]);
        assert_eq!(candidates_for("libx.so", Platform::Windows), ["libx.so", "libx.dll", "x.dll"]);
        assert_eq!(candidates_for("libx.dylib", Platform::Linux), ["libx.dylib", "libx.so"]);
        assert_eq!(candidates_for("x.so.6", Platform::MacOs), ["x.so.6", "x.dylib", "libx.dylib"]);
    }

    #[test]
    fn native_extension_is_used_as_is() {
        assert_eq!(candidates_for("/usr/lib/libz.so.1", Platform::Linux), ["/usr/lib/libz.so.1"]);
        assert_eq!(candidates_for("C:\\x\\y.dll", Platform::Windows), ["C:\\x\\y.dll"]);
        assert_eq!(candidates_for("libz.dylib", Platform::MacOs), ["libz.dylib"]);
    }

    #[test]
    fn c_runtime_per_platform() {
        assert_eq!(candidates_for("c", Platform::Windows)[0], "msvcrt.dll");
        assert_eq!(candidates_for("m", Platform::Linux)[0], "libm.so.6");
    }
}
