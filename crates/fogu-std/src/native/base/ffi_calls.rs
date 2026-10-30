// Every C signature that fogu:ffi can call (generated: up to 4 arguments
// mixing i64 and f64, or up to 8 integer/pointer arguments).
// Do not edit by hand.

use std::ffi::c_void;

use super::Arg;

/// Calls `f` with the arguments, returning `R` (`i64`, `f64`, `f32` or `()`).
///
/// # Safety
/// `f` must be a C function with exactly this signature.
pub unsafe fn call<R>(f: *const c_void, args: &[Arg]) -> Result<R, String> {
    // SAFETY: see above.
    Ok(unsafe {
        match args {
        [] => std::mem::transmute::<*const c_void, extern "C" fn() -> R>(f)(),
        [Arg::I(a0)] => std::mem::transmute::<*const c_void, extern "C" fn(i64) -> R>(f)(*a0),
        [Arg::F(a0)] => std::mem::transmute::<*const c_void, extern "C" fn(f64) -> R>(f)(*a0),
        [Arg::I(a0), Arg::I(a1)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64) -> R>(f)(*a0, *a1),
        [Arg::I(a0), Arg::F(a1)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, f64) -> R>(f)(*a0, *a1),
        [Arg::F(a0), Arg::I(a1)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, i64) -> R>(f)(*a0, *a1),
        [Arg::F(a0), Arg::F(a1)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, f64) -> R>(f)(*a0, *a1),
        [Arg::I(a0), Arg::I(a1), Arg::I(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, i64) -> R>(f)(*a0, *a1, *a2),
        [Arg::I(a0), Arg::I(a1), Arg::F(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, f64) -> R>(f)(*a0, *a1, *a2),
        [Arg::I(a0), Arg::F(a1), Arg::I(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, f64, i64) -> R>(f)(*a0, *a1, *a2),
        [Arg::I(a0), Arg::F(a1), Arg::F(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, f64, f64) -> R>(f)(*a0, *a1, *a2),
        [Arg::F(a0), Arg::I(a1), Arg::I(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, i64, i64) -> R>(f)(*a0, *a1, *a2),
        [Arg::F(a0), Arg::I(a1), Arg::F(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, i64, f64) -> R>(f)(*a0, *a1, *a2),
        [Arg::F(a0), Arg::F(a1), Arg::I(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, f64, i64) -> R>(f)(*a0, *a1, *a2),
        [Arg::F(a0), Arg::F(a1), Arg::F(a2)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, f64, f64) -> R>(f)(*a0, *a1, *a2),
        [Arg::I(a0), Arg::I(a1), Arg::I(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::I(a1), Arg::I(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, i64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::I(a1), Arg::F(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, f64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::I(a1), Arg::F(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, f64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::F(a1), Arg::I(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, f64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::F(a1), Arg::I(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, f64, i64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::F(a1), Arg::F(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, f64, f64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::F(a1), Arg::F(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, f64, f64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::I(a1), Arg::I(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, i64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::I(a1), Arg::I(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, i64, i64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::I(a1), Arg::F(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, i64, f64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::I(a1), Arg::F(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, i64, f64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::F(a1), Arg::I(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, f64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::F(a1), Arg::I(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, f64, i64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::F(a1), Arg::F(a2), Arg::I(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, f64, f64, i64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::F(a0), Arg::F(a1), Arg::F(a2), Arg::F(a3)] => std::mem::transmute::<*const c_void, extern "C" fn(f64, f64, f64, f64) -> R>(f)(*a0, *a1, *a2, *a3),
        [Arg::I(a0), Arg::I(a1), Arg::I(a2), Arg::I(a3), Arg::I(a4)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, i64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3, *a4),
        [Arg::I(a0), Arg::I(a1), Arg::I(a2), Arg::I(a3), Arg::I(a4), Arg::I(a5)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, i64, i64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3, *a4, *a5),
        [Arg::I(a0), Arg::I(a1), Arg::I(a2), Arg::I(a3), Arg::I(a4), Arg::I(a5), Arg::I(a6)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3, *a4, *a5, *a6),
        [Arg::I(a0), Arg::I(a1), Arg::I(a2), Arg::I(a3), Arg::I(a4), Arg::I(a5), Arg::I(a6), Arg::I(a7)] => std::mem::transmute::<*const c_void, extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> R>(f)(*a0, *a1, *a2, *a3, *a4, *a5, *a6, *a7),
        _ => return Err("ffi: unsupported signature (up to 4 arguments with floats, or up to 8 integers/pointers)".into()),
        }
    })
}
