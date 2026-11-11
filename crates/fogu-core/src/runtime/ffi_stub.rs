//! Native stubs for `fogu:ffi`, generated with Cranelift for one exact C
//! signature each (cached by signature):
//!
//! - a **caller** `extern "C" fn(f, args: *const u64, out: *mut u64)` loads the
//!   arguments from `args`, calls `f` with the signature and stores the result
//!   in `out` — any number and mix of integer/pointer and float arguments;
//! - a **callback** with the signature itself: it stores its arguments in an
//!   array and calls `dispatch(id, args, out)` (Rust), then returns `out[0]`.
//!   Its address is a C function pointer that runs Fogu code.
//!
//! Integers and pointers travel as 64-bit words (a narrower C integer is read
//! from the low bits, as every supported ABI does); `f32` is a real `float`.
//! Where Cranelift has no backend (32-bit ARM) both return an error and
//! `fogu:ffi` falls back to its precompiled signatures.

use std::cell::RefCell;
use std::collections::HashMap;

use cranelift_codegen::ir::{self, types, AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

/// How a C value travels: in an integer register or in a float register.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Int,
    F64,
    F32,
    Void,
}

impl Kind {
    fn clif(self) -> ir::Type {
        match self {
            Kind::Int | Kind::Void => types::I64,
            Kind::F64 => types::F64,
            Kind::F32 => types::F32,
        }
    }
}

/// `dispatch(id, args, out)` of a callback.
pub type Dispatch = extern "C" fn(u64, *const u64, *mut u64);

/// A caller stub.
pub type Caller = unsafe extern "C" fn(usize, *const u64, *mut u64);

struct Stubs {
    module: JITModule,
    callers: HashMap<(Vec<Kind>, Kind), usize>,
    counter: u32,
}

thread_local! {
    static STUBS: RefCell<Option<Result<Stubs, String>>> = const { RefCell::new(None) };
}

fn with_stubs<T>(f: impl FnOnce(&mut Stubs) -> Result<T, String>) -> Result<T, String> {
    STUBS.with(|s| {
        let mut s = s.borrow_mut();
        let st = s.get_or_insert_with(new_stubs);
        match st {
            Ok(st) => f(st),
            Err(e) => Err(e.clone()),
        }
    })
}

fn new_stubs() -> Result<Stubs, String> {
    let mut flags = settings::builder();
    flags.set("use_colocated_libcalls", "false").map_err(|e| e.to_string())?;
    flags.set("is_pic", "false").map_err(|e| e.to_string())?;
    flags.set("opt_level", "speed").map_err(|e| e.to_string())?;
    let isa = cranelift_native::builder()
        .map_err(|e| format!("ffi: no native code generator for this platform: {}", e))?
        .finish(settings::Flags::new(flags))
        .map_err(|e| e.to_string())?;
    let module = JITModule::new(JITBuilder::with_isa(isa, cranelift_module::default_libcall_names()));
    Ok(Stubs { module, callers: HashMap::new(), counter: 0 })
}

/// Signature of a C function with these parameters and result.
fn c_sig(m: &JITModule, params: &[Kind], ret: Kind) -> ir::Signature {
    let mut sig = m.make_signature();
    for p in params {
        sig.params.push(AbiParam::new(p.clif()));
    }
    if ret != Kind::Void {
        sig.returns.push(AbiParam::new(ret.clif()));
    }
    sig
}

impl Stubs {
    /// Declares, builds and finalizes one function; returns its address.
    fn build(&mut self, sig: ir::Signature, body: impl FnOnce(&mut FunctionBuilder, &[ir::Value])) -> Result<usize, String> {
        self.counter += 1;
        let id = self
            .module
            .declare_function(&format!("ffi_stub_{}", self.counter), Linkage::Local, &sig)
            .map_err(|e| e.to_string())?;
        let mut ctx = Context::new();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(2, id.as_u32());
        let mut fbc = FunctionBuilderContext::new();
        let cfg = self.module.target_config();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbc);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            let ps = b.block_params(entry).to_vec();
            body(&mut b, &ps);
            b.seal_all_blocks();
            b.finalize(cfg);
        }
        self.module.define_function(id, &mut ctx).map_err(|e| format!("ffi stub: {:?}", e))?;
        self.module.clear_context(&mut ctx);
        self.module.finalize_definitions().map_err(|e| format!("ffi stub: {:?}", e))?;
        Ok(self.module.get_finalized_function(id) as usize)
    }
}

/// The caller stub of a signature (built once, then cached).
pub fn caller(params: &[Kind], ret: Kind) -> Result<Caller, String> {
    with_stubs(|st| {
        let key = (params.to_vec(), ret);
        if let Some(&a) = st.callers.get(&key) {
            // SAFETY: built below with exactly this signature.
            return Ok(unsafe { std::mem::transmute::<usize, Caller>(a) });
        }
        let mut outer = st.module.make_signature();
        for _ in 0..3 {
            outer.params.push(AbiParam::new(types::I64));
        }
        let inner = c_sig(&st.module, params, ret);
        let addr = st.build(outer, |b, ps| {
            let (f, args, out) = (ps[0], ps[1], ps[2]);
            let vals: Vec<ir::Value> = params
                .iter()
                .enumerate()
                .map(|(i, k)| b.ins().load(k.clif(), MemFlagsData::trusted(), args, (8 * i) as i32))
                .collect();
            let sr = b.import_signature(inner);
            let inst = b.ins().call_indirect(sr, f, &vals);
            if ret != Kind::Void {
                let r = b.inst_results(inst)[0];
                b.ins().store(MemFlagsData::trusted(), r, out, 0);
            }
            b.ins().return_(&[]);
        })?;
        st.callers.insert(key, addr);
        // SAFETY: the stub has the `Caller` signature.
        Ok(unsafe { std::mem::transmute::<usize, Caller>(addr) })
    })
}

/// A new C function with this signature that calls `dispatch(id, …)`.
/// Callbacks are never freed (their code is small); `fogu:ffi` reuses ids.
pub fn callback(params: &[Kind], ret: Kind, dispatch: Dispatch, id: u64) -> Result<usize, String> {
    with_stubs(|st| {
        let sig = c_sig(&st.module, params, ret);
        let mut dsig = st.module.make_signature();
        for _ in 0..3 {
            dsig.params.push(AbiParam::new(types::I64));
        }
        st.build(sig, |b, ps| {
            let n = params.len().max(1) as u32;
            let args = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8 * n, 3));
            let out = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
            let ap = b.ins().stack_addr(types::I64, args, 0);
            let op = b.ins().stack_addr(types::I64, out, 0);
            let zero = b.ins().iconst(types::I64, 0);
            b.ins().store(MemFlagsData::trusted(), zero, op, 0);
            for (i, v) in ps.iter().enumerate() {
                b.ins().store(MemFlagsData::trusted(), *v, ap, (8 * i) as i32);
            }
            let idv = b.ins().iconst(types::I64, id as i64);
            let fv = b.ins().iconst(types::I64, dispatch as usize as i64);
            let sr = b.import_signature(dsig);
            b.ins().call_indirect(sr, fv, &[idv, ap, op]);
            if ret == Kind::Void {
                b.ins().return_(&[]);
            } else {
                let r = b.ins().load(ret.clif(), MemFlagsData::trusted(), op, 0);
                b.ins().return_(&[r]);
            }
        })
    })
}

/// A parameter of a [`direct`] entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirectParam {
    /// Any C integer or pointer, passed as an `i64` (the callee reads the low bits).
    Int,
    F64,
    /// A C `bool`, passed as a `u8` (0 or 1).
    Bool,
}

/// What the C function returns, and how it becomes an `i64` / `f64` / `u8`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirectRet {
    /// A signed integer of this many bytes (sign-extended).
    Signed(u8),
    /// An unsigned integer of this many bytes (zero-extended).
    Unsigned(u8),
    /// A C `bool`: its low byte, as 0 or 1 (`u8`).
    Bool,
    F64,
    /// A `float`, widened to `f64`.
    F32,
}

/// A function with a plain numeric signature — `extern "C" fn(i64 | f64 |
/// u8 …) -> i64 | f64 | u8` — that calls the C function at `addr`, widens
/// its result as `ret` says and then calls `after()` (the caller's
/// bookkeeping: `errno`, a pending callback error). Compiled code calls it
/// directly. Each call builds a new entry (they are never freed).
pub fn direct(addr: usize, params: &[DirectParam], ret: DirectRet, after: extern "C" fn()) -> Result<usize, String> {
    with_stubs(|st| {
        let mut outer = st.module.make_signature();
        let mut inner = st.module.make_signature();
        for p in params {
            let (o, i) = match p {
                DirectParam::Int => (types::I64, types::I64),
                DirectParam::F64 => (types::F64, types::F64),
                DirectParam::Bool => (types::I8, types::I64),
            };
            outer.params.push(AbiParam::new(o));
            inner.params.push(AbiParam::new(i));
        }
        let (o, i) = match ret {
            DirectRet::Signed(_) | DirectRet::Unsigned(_) => (types::I64, types::I64),
            DirectRet::Bool => (types::I8, types::I64),
            DirectRet::F64 => (types::F64, types::F64),
            DirectRet::F32 => (types::F64, types::F32),
        };
        outer.returns.push(AbiParam::new(o));
        inner.returns.push(AbiParam::new(i));
        let after_sig = st.module.make_signature();
        let params = params.to_vec();
        st.build(outer, move |b, ps| {
            let vals: Vec<ir::Value> = params
                .iter()
                .zip(ps)
                .map(|(p, v)| match p {
                    DirectParam::Bool => b.ins().uextend(types::I64, *v),
                    _ => *v,
                })
                .collect();
            let f = b.ins().iconst(types::I64, addr as i64);
            let sr = b.import_signature(inner);
            let inst = b.ins().call_indirect(sr, f, &vals);
            let r = b.inst_results(inst)[0];
            // the upper bits of a narrow C result are not defined: keep its own bits
            let narrow = |b: &mut FunctionBuilder, bytes: u8, signed: bool| {
                let ty = match bytes {
                    1 => types::I8,
                    2 => types::I16,
                    4 => types::I32,
                    _ => return r,
                };
                let low = b.ins().ireduce(ty, r);
                if signed {
                    b.ins().sextend(types::I64, low)
                } else {
                    b.ins().uextend(types::I64, low)
                }
            };
            let out = match ret {
                DirectRet::Signed(n) => narrow(b, n, true),
                DirectRet::Unsigned(n) => narrow(b, n, false),
                DirectRet::Bool => {
                    let low = b.ins().ireduce(types::I8, r);
                    b.ins().icmp_imm_s(ir::condcodes::IntCC::NotEqual, low, 0)
                }
                DirectRet::F64 => r,
                DirectRet::F32 => b.ins().fpromote(types::F64, r),
            };
            let af = b.ins().iconst(types::I64, after as usize as i64);
            let asr = b.import_signature(after_sig);
            b.ins().call_indirect(asr, af, &[]);
            b.ins().return_(&[out]);
        })
    })
}
