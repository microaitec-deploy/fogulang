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
//! ```
//!
//! **Unsafe by nature**: a wrong signature can crash the program. Supported
//! signatures: up to 4 arguments mixing integers and floats, or up to 8
//! integer/pointer arguments. Types: `i8 i16 i32 i64 u8 u16 u32 u64 isize
//! usize bool f64 ptr str buffer` for arguments, the same plus `f32` and
//! `void` for results. A missing library or symbol is an `error` value.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::ffi::{c_void, CStr, CString};
use std::rc::Rc;

use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{list, max_args, nfn_var, str_arg};

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
        Ok(NativeObjectRef::new(Buffer { data: Rc::new(RefCell::new(vec![0u8; n])) }).to_value())
    }));
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
        "i8" | "char" => Ty::I8,
        "i16" | "short" => Ty::I16,
        "i32" | "int" => Ty::I32,
        "i64" | "long long" | "isize" => Ty::I64,
        "u8" | "uchar" => Ty::U8,
        "u16" | "ushort" => Ty::U16,
        "u32" | "uint" => Ty::U32,
        "u64" | "usize" | "size_t" => Ty::U64,
        "bool" => Ty::Bool,
        "f32" | "float" => Ty::F32,
        "f64" | "double" => Ty::F64,
        "ptr" | "pointer" => Ty::Ptr,
        "str" | "string" | "cstring" => Ty::Str,
        "buffer" => Ty::Buffer,
        "void" => Ty::Void,
        other => return Err(format!("ffi: unknown type '{}' ({})", other, TYPES.join(" "))),
    })
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
                let params: Vec<Ty> = match args.get(1) {
                    None | Some(Value::Nil) => Vec::new(),
                    Some(Value::List(l)) => l.borrow().iter().map(|v| ty(&v.to_display_string())).collect::<Result<_, _>>()?,
                    Some(other) => return Err(format!("func: argument types must be a list, got '{}'", other.type_name())),
                };
                if params.iter().any(|t| matches!(t, Ty::Void | Ty::F32)) {
                    return Err("func: 'void' and 'f32' are only valid as the return type".into());
                }
                let ret = match args.get(2) {
                    None | Some(Value::Nil) => Ty::Void,
                    Some(v) => ty(&v.to_display_string())?,
                };
                if ret == Ty::Buffer {
                    return Err("func: 'buffer' is only valid as an argument (return a 'ptr')".into());
                }
                let floats = params.iter().filter(|t| **t == Ty::F64).count();
                if params.len() > 8 || (params.len() > 4 && floats > 0) {
                    return Err("func: up to 4 arguments when floats are used, up to 8 integers/pointers otherwise".into());
                }
                let cname = CString::new(symbol.clone()).map_err(|_| "func: invalid symbol name".to_string())?;
                // SAFETY: we only read the address; calling it is checked by the declared signature.
                let addr = unsafe {
                    self.lib
                        .get::<*const c_void>(cname.as_bytes_with_nul())
                        .map(|s| *s)
                        .map_err(|e| io_err!("ffi: {}: no symbol '{}': {}", self.name, symbol, e))?
                };
                let f = FnInfo { lib: Rc::clone(&self.lib), addr: addr as usize, params, ret, name: symbol };
                Ok(native_fn(f))
            }
            "has" => {
                let symbol = str_arg(args, 0, "has")?;
                let cname = CString::new(symbol).map_err(|_| "has: invalid symbol name".to_string())?;
                Ok(Value::Bool(unsafe { self.lib.get::<*const c_void>(cname.as_bytes_with_nul()).is_ok() }))
            }
            other => Err(format!("method '{}' does not exist on Library (func has)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["func", "has"]
    }
}

// ---------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------

struct FnInfo {
    /// keeps the library loaded while the function exists
    #[allow(dead_code)]
    lib: Rc<libloading::Library>,
    addr: usize,
    params: Vec<Ty>,
    ret: Ty,
    name: String,
}

thread_local! {
    /// Native functions are plain `fn` pointers: the declared signatures
    /// live here, found by the function's name (`ffi#<id>`).
    static FUNCS: RefCell<Vec<FnInfo>> = const { RefCell::new(Vec::new()) };
}

fn native_fn(f: FnInfo) -> Value {
    let (name, arity) = (f.name.clone(), f.params.len());
    let id = FUNCS.with(|fs| {
        let mut fs = fs.borrow_mut();
        fs.push(f);
        fs.len() - 1
    });
    // the id travels in the function's name; `call_ffi` reads it
    Value::NativeFn(Rc::new(NativeFnValue {
        name: format!("{}#ffi{}", name, id),
        arity,
        variadic: false,
        func: call_ffi_placeholder,
        intercept: Default::default(),
    }))
}

fn call_ffi_placeholder(_: &[Value]) -> Result<Value, String> {
    Err("ffi: internal error: call through call_ffi".into())
}

/// Is this native function an ffi function? (name `symbol#ffiN`)
pub fn ffi_id(name: &str) -> Option<usize> {
    name.rsplit_once("#ffi").and_then(|(_, id)| id.parse().ok())
}

/// Calls ffi function `id` (the interpreter routes `symbol#ffiN` here).
pub fn call_ffi(id: usize, args: &[Value]) -> Result<Value, String> {
    FUNCS.with(|fs| {
        let fs = fs.borrow();
        let f = fs.get(id).ok_or("ffi: unknown function")?;
        if args.len() != f.params.len() {
            return Err(format!("{} expects {} arguments, got {}", f.name, f.params.len(), args.len()));
        }
        // keep strings and buffers alive during the call
        let mut cstrings: Vec<CString> = Vec::new();
        let mut buffers: Vec<Rc<RefCell<Vec<u8>>>> = Vec::new();
        let mut raw: Vec<Arg> = Vec::with_capacity(args.len());
        for (i, (t, v)) in f.params.iter().zip(args).enumerate() {
            let bad = || format!("{}: argument {} must be {:?}, got '{}'", f.name, i + 1, t, v.type_name());
            raw.push(match (t, v) {
                (Ty::F64, Value::Float(x)) => Arg::F(*x),
                (Ty::F64, Value::Int(n)) => Arg::F(*n as f64),
                (Ty::Bool, Value::Bool(b)) => Arg::I(i64::from(*b)),
                (Ty::Str, Value::Str(s)) => {
                    let c = CString::new(s.as_str()).map_err(|_| format!("{}: argument {} contains a NUL byte", f.name, i + 1))?;
                    let p = c.as_ptr() as i64;
                    cstrings.push(c);
                    Arg::I(p)
                }
                (Ty::Str | Ty::Ptr, Value::Nil) => Arg::I(0),
                (Ty::Buffer | Ty::Ptr, Value::Object(o)) if o.name() == "Buffer" => {
                    let inner = o.inner();
                    let b = inner.borrow();
                    let buf = b.as_any().downcast_ref::<Buffer>().ok_or_else(bad)?;
                    let p = buf.data.borrow_mut().as_mut_ptr() as i64;
                    buffers.push(Rc::clone(&buf.data));
                    Arg::I(p)
                }
                (Ty::F64 | Ty::Str | Ty::Buffer | Ty::Bool, _) => return Err(bad()),
                (_, Value::Int(n)) => Arg::I(*n),
                (_, Value::Bool(b)) => Arg::I(i64::from(*b)),
                _ => return Err(bad()),
            });
        }
        let addr = f.addr as *const c_void;
        // SAFETY: the program declared the signature; a wrong one is undefined behaviour,
        // which is why fogu:ffi is documented as unsafe.
        let out = unsafe {
            match f.ret {
                Ty::Void => {
                    calls::call::<()>(addr, &raw)?;
                    Value::Nil
                }
                Ty::F64 => Value::Float(calls::call::<f64>(addr, &raw)?),
                Ty::F32 => Value::Float(calls::call::<f32>(addr, &raw)? as f64),
                t => {
                    let r = calls::call::<i64>(addr, &raw)?;
                    match t {
                        Ty::I8 => Value::Int(r as i8 as i64),
                        Ty::I16 => Value::Int(r as i16 as i64),
                        Ty::I32 => Value::Int(r as i32 as i64),
                        Ty::U8 => Value::Int(r as u8 as i64),
                        Ty::U16 => Value::Int(r as u16 as i64),
                        Ty::U32 => Value::Int(r as u32 as i64),
                        Ty::Bool => Value::Bool(r as u8 != 0),
                        Ty::Str => {
                            if r == 0 {
                                Value::Nil
                            } else {
                                Value::Str(CStr::from_ptr(r as *const std::ffi::c_char).to_string_lossy().into_owned().into())
                            }
                        }
                        _ => Value::Int(r),
                    }
                }
            }
        };
        drop(cstrings);
        drop(buffers);
        Ok(out)
    })
}

// ---------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------

/// Memory for out-parameters: `ffi.alloc(n)`.
pub struct Buffer {
    data: Rc<RefCell<Vec<u8>>>,
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
            // write(offset, str | [bytes])
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
                    _ => return Err("write(offset, data) expects a str or a list of bytes".into()),
                };
                let mut d = data.borrow_mut();
                if off + bytes.len() > d.len() {
                    return Err(format!("write: {} bytes at {} do not fit in {} bytes", bytes.len(), off, d.len()));
                }
                d[off..off + bytes.len()].copy_from_slice(&bytes);
                Ok(Value::Nil)
            }
            // i32(offset) · i64(offset) · f64(offset): little-endian reads
            "i32" | "u32" | "i64" | "f64" => {
                let off = match args.first() {
                    Some(Value::Int(o)) if *o >= 0 => *o as usize,
                    _ => return Err(format!("{}(offset) expects an offset >= 0", method)),
                };
                let size = if method == "i32" || method == "u32" { 4 } else { 8 };
                let d = data.borrow();
                if off + size > d.len() {
                    return Err(format!("{}: offset {} is outside the buffer", method, off));
                }
                let b = &d[off..off + size];
                Ok(match method {
                    "i32" => Value::Int(i32::from_le_bytes(b.try_into().unwrap()) as i64),
                    "u32" => Value::Int(u32::from_le_bytes(b.try_into().unwrap()) as i64),
                    "i64" => Value::Int(i64::from_le_bytes(b.try_into().unwrap())),
                    _ => Value::Float(f64::from_le_bytes(b.try_into().unwrap())),
                })
            }
            other => Err(format!("method '{}' does not exist on Buffer (bytes text write i32 u32 i64 f64)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["bytes", "text", "write", "i32", "u32", "i64", "f64"]
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
