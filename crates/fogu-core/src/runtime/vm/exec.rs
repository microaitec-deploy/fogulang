//! The run loop: one `match` per instruction over the registers of a frame.
//!
//! A call from compiled code to a compiled function pushes a frame in the same
//! loop: no Rust recursion, the callee's registers follow the caller's in one
//! vector. Everything else (natives, functions the tree-walker runs, functions
//! with native code from the JIT) is called through the interpreter.
//!
//! A run started from Rust (`run`: the first Fogu call of a program, a callback
//! of a native function, a task) owns its register and frame vectors, so `fg`
//! tasks — which only switch inside such Rust calls — never share them.

use std::cell::RefCell;
use std::rc::Rc;

use ahash::AHashMap as HashMap;

use super::super::super::ast::BinaryOp;
use super::super::super::environment::Environment;
use super::super::super::ops;
use super::super::super::value::{ChanValue, FnValue, Str, Value};
use super::super::{
    assign_index, chan_recv, check_arity, compare_values, describe_for, destructure, eval_binary, eval_index, norm_index, propagated_return,
    undefined, values_equal, Interpreter, RecvError, PROPAGATE,
};
use super::{Cache, Op, Proto, Reg};
use crate::native::NativeObjectRef;

// `Proto::mode` starts at 0: the JIT may still compile the function, so it is
// called through the interpreter.
/// The JIT did not take it: compiled code calls it in the same loop.
const INLINE: u8 = 1;
/// It has native code: always through the interpreter (which runs it).
const NATIVE: u8 = 2;

/// Frames of one run: beyond this, recursion is "too deep" (the tree-walker's
/// limit is its stack; a frame here costs ~100 bytes plus its registers).
const MAX_FRAMES: usize = 200_000;

/// What a `for` loop walks.
enum Iter {
    Done,
    Range(i64, i64),
    /// A snapshot of a list, the sorted keys of a map, the characters of a text.
    Items(std::vec::IntoIter<Value>),
    Bytes(std::vec::IntoIter<u8>),
    Pairs(std::vec::IntoIter<(Value, Value)>),
    Chan(Rc<ChanValue>),
    /// `for conn in listener`
    Accept(NativeObjectRef),
    /// `for msg in dataChannel`
    Recv(NativeObjectRef),
}

struct Frame {
    f: Rc<FnValue>,
    proto: Rc<Proto>,
    /// Where it continues after the call it made.
    pc: usize,
    /// Its first register / iterator slot.
    base: usize,
    iter_base: usize,
    /// The caller's register that receives the result.
    ret: Reg,
    /// The caller's file, when this function is from another module.
    saved_file: Option<Rc<str>>,
    saved_line: u32,
}

thread_local! {
    /// Register vectors of finished runs, reused by the next ones.
    static POOL: RefCell<Vec<Vec<Value>>> = const { RefCell::new(Vec::new()) };
}

const POOL_MAX: usize = 64;

fn take_regs(n: usize) -> Vec<Value> {
    let mut v = POOL.with(|p| p.borrow_mut().pop()).unwrap_or_default();
    v.resize(n, Value::Nil);
    v
}

fn give_regs(mut v: Vec<Value>) {
    // dropping the values may run code (closing a socket…): not while the pool is borrowed
    v.clear();
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() < POOL_MAX {
            p.push(v);
        }
    });
}

#[inline(always)]
fn take(v: &mut Value) -> Value {
    std::mem::replace(v, Value::Nil)
}

#[inline(always)]
fn env_of<'a>(it: &'a Interpreter, f: &'a FnValue) -> &'a Rc<RefCell<Environment>> {
    f.closure.as_ref().unwrap_or(&it.globals)
}

/// `_x` fields and methods of a struct are private to its module.
#[inline]
fn check_private(it: &Interpreter, target: &Value, field: &str, f: &FnValue) -> Result<(), String> {
    if !field.starts_with('_') {
        return Ok(());
    }
    let Value::Struct(s) = target else {
        return Ok(());
    };
    let s = s.borrow();
    if s.module.is_some() && s.module != env_of(it, f).borrow().module_name() {
        return Err(format!("'{}' is private to the module where '{}' was declared", field, s.type_name));
    }
    Ok(())
}

/// The proto to call `f` with in the same loop, or `None`: call it through the interpreter.
#[inline]
fn inline_proto(it: &Interpreter, f: &Rc<FnValue>, method: bool) -> Option<Rc<Proto>> {
    let p = it.vm_protos.get(f, method)?;
    // methods are never compiled by the JIT
    if method {
        return Some(p);
    }
    match p.mode.get() {
        INLINE => Some(p),
        NATIVE => None,
        _ => {
            if !it.jit_enabled {
                p.mode.set(INLINE);
                return Some(p);
            }
            // its first calls go through the interpreter, where the JIT counts
            // them and compiles the function if it can
            let calls = p.calls.get() + 1;
            p.calls.set(calls);
            let jit = it.jit.borrow();
            if calls > jit.call_threshold().saturating_add(2) {
                p.mode.set(if jit.compiled(Rc::as_ptr(&f.body) as usize) { NATIVE } else { INLINE });
            }
            None
        }
    }
}

pub fn run(it: &Interpreter, p: &Rc<Proto>, f: &Rc<FnValue>, this: Option<Value>, mut args: Vec<Value>) -> Result<Value, String> {
    let mut regs = take_regs(p.nregs as usize);
    let mut at = 0;
    if let Some(t) = this {
        regs[0] = t;
        at = 1;
    }
    let fixed = p.nparams as usize;
    let extra = if args.len() > fixed { args.split_off(fixed) } else { Vec::new() };
    for (i, v) in args.into_iter().enumerate() {
        regs[at + i] = v;
    }
    if p.rest {
        regs[at + fixed] = Value::List(Rc::new(RefCell::new(extra)));
    }
    let frame = Frame { f: Rc::clone(f), proto: Rc::clone(p), pc: 0, base: 0, iter_base: 0, ret: 0, saved_file: None, saved_line: it.cur_line.get() };
    let out = exec(it, frame, &mut regs);
    give_regs(regs);
    out
}

fn code_err(c: i64) -> String {
    ops::code_message(c).to_string()
}

/// The value a function returns when `?` meets an error (as the interpreter:
/// `interpreter::propagated_return`).
fn propagated(it: &Interpreter, p: &Proto) -> Value {
    let err = it.pending.borrow_mut().take().unwrap_or(Value::Nil);
    propagated_return(p.ret_type.as_ref(), err)
}

fn exec(it: &Interpreter, first: Frame, regs: &mut Vec<Value>) -> Result<Value, String> {
    let host = crate::host::get();
    let mut stack: Vec<Frame> = Vec::new();
    let mut iters: Vec<Iter> = Vec::new();
    if first.proto.niters > 0 {
        iters.resize_with(first.proto.niters as usize, || Iter::Done);
    }
    let mut cur = first;
    let mut pc = 0usize;
    let mut base = 0usize;

    // the compiler only emits registers below `nregs`, which the frame reserved
    macro_rules! r {
        ($i:expr) => {
            regs[base + $i as usize]
        };
    }
    macro_rules! env {
        () => {
            env_of(it, &cur.f)
        };
    }
    // `R[a] = R[b] op R[c]` with the int and float cases inline
    macro_rules! arith {
        ($a:expr, $b:expr, $c:expr, $op:expr, $int:expr, $float:expr) => {{
            let v = match (&r!($b), &r!($c)) {
                (Value::Int(x), Value::Int(y)) => Value::Int($int(*x, *y).map_err(code_err)?),
                (Value::Float(x), Value::Float(y)) => Value::Float($float(*x, *y)),
                _ => {
                    // `s = s + x`: the variable gives up its text, which then grows in place
                    let l = if $a == $b && $b != $c { take(&mut r!($b)) } else { r!($b).clone() };
                    eval_binary(l, $op, r!($c).clone())?
                }
            };
            r!($a) = v;
        }};
    }
    macro_rules! compare {
        ($a:expr, $b:expr, $c:expr, $op:expr, $cmp:tt) => {{
            let v = match (&r!($b), &r!($c)) {
                (Value::Int(x), Value::Int(y)) => Value::Bool(x $cmp y),
                (l, r) => compare_values(l.clone(), $op, r.clone())?,
            };
            r!($a) = v;
        }};
    }
    macro_rules! equal {
        ($a:expr, $b:expr, $c:expr, $want:expr) => {{
            let eq = match (&r!($b), &r!($c)) {
                (Value::Int(x), Value::Int(y)) => x == y,
                // a struct with `equals(other)` decides, also inside lists and maps
                (l @ (Value::Struct(_) | Value::List(_) | Value::Map(_)), r) => it.equal(l, r, env!())?,
                (l, r) => values_equal(l, r),
            };
            r!($a) = Value::Bool(eq == $want);
        }};
    }
    // the function of the current frame returns `v`
    macro_rules! do_return {
        ($v:expr) => {{
            let v: Value = $v;
            if let Some(ty) = &cur.proto.ret_type {
                if !it.type_matches(&v, ty, env!())? {
                    return Err(format!("type mismatch: function '{}' must return {}, got {}", cur.proto.name, ty, describe_for(&v, ty)));
                }
            }
            it.cur_line.set(cur.saved_line);
            match stack.pop() {
                None => return Ok(v),
                Some(caller) => {
                    if let Some(file) = cur.saved_file.take() {
                        *it.cur_file.borrow_mut() = file;
                    }
                    regs.truncate(cur.base);
                    iters.truncate(cur.iter_base);
                    let ret = cur.ret;
                    cur = caller;
                    pc = cur.pc;
                    base = cur.base;
                    r!(ret) = v;
                }
            }
        }};
    }
    // calls `f` (compiled as `proto`) in this loop: its arguments are the `n`
    // registers from `R[first]`, its result goes to `R[ret]`
    macro_rules! push_frame {
        ($f:expr, $proto:expr, $this:expr, $what:expr, $name:expr, $first:expr, $n:expr, $ret:expr) => {{
            let f: Rc<FnValue> = $f;
            let proto: Rc<Proto> = $proto;
            let n: usize = $n;
            let fixed = proto.nparams as usize;
            if n > fixed && !proto.rest {
                check_arity($what, $name, &f.params, n)?;
            }
            if stack.len() >= MAX_FRAMES {
                return Err(ops::ERR_STACK.into());
            }
            if it.deadline_count.get() != 0 {
                it.check_deadline()?;
            }
            let src = base + $first as usize;
            let nb = regs.len();
            regs.resize(nb + proto.nregs as usize, Value::Nil);
            let mut at = nb;
            if let Some(t) = $this {
                regs[at] = t;
                at += 1;
            }
            for i in 0..n.min(fixed) {
                let v = take(&mut regs[src + i]);
                regs[at + i] = v;
            }
            if proto.rest {
                let extra: Vec<Value> = (fixed.min(n)..n).map(|i| take(&mut regs[src + i])).collect();
                regs[at + fixed] = Value::List(Rc::new(RefCell::new(extra)));
            }
            let iter_base = iters.len();
            if proto.niters > 0 {
                iters.resize_with(iter_base + proto.niters as usize, || Iter::Done);
            }
            let saved_line = it.cur_line.get();
            // entering the callee's file, unless it is the caller's
            let saved_file = match (&f.module, &cur.f.module) {
                (Some(a), Some(b)) if Rc::ptr_eq(a, b) => None,
                (Some(a), _) => Some(it.cur_file.replace(Rc::clone(a))),
                (None, _) => None,
            };
            cur.pc = pc;
            let callee = Frame { f, proto, pc: 0, base: nb, iter_base, ret: $ret, saved_file, saved_line };
            stack.push(std::mem::replace(&mut cur, callee));
            pc = 0;
            base = nb;
        }};
    }
    macro_rules! take_args {
        ($first:expr, $n:expr) => {{
            let s = base + $first as usize;
            regs[s..s + $n as usize].iter_mut().map(take).collect::<Vec<Value>>()
        }};
    }

    loop {
        let op = cur.proto.code[pc];
        pc += 1;
        match op {
            Op::Line(n) => it.cur_line.set(n),
            Op::Move(a, b) => {
                let v = r!(b).clone();
                r!(a) = v;
            }
            Op::LoadK(a, k) => r!(a) = cur.proto.consts[k as usize].clone(),
            Op::LoadNil(a) => r!(a) = Value::Nil,
            Op::LoadBool(a, b) => r!(a) = Value::Bool(b),
            Op::LoadInt(a, n) => r!(a) = Value::Int(n as i64),
            Op::GetGlobal(a, g) => {
                let (sym, name) = &cur.proto.globals[g as usize];
                let v = env!().borrow().get_sym(*sym).ok_or_else(|| undefined(name))?;
                r!(a) = v;
            }
            Op::SetGlobal(g, b) => {
                let v = r!(b).clone();
                env!().borrow_mut().assign_sym(cur.proto.globals[g as usize].0, v)?;
            }
            Op::GlobalOp(g, b, c, bop) => {
                let (sym, name) = &cur.proto.globals[g as usize];
                let left = take(&mut r!(b));
                let v = match (bop, &left, &r!(c)) {
                    (BinaryOp::Add, Value::Str(_), Value::Str(rhs)) => {
                        let rhs = rhs.clone();
                        it.append_in_place(name, left, &rhs, env!())?
                    }
                    _ => {
                        let v = eval_binary(left, bop, r!(c).clone())?;
                        env!().borrow_mut().assign_sym(*sym, v.clone())?;
                        v
                    }
                };
                r!(b) = v;
            }

            Op::Add(a, b, c) => arith!(a, b, c, BinaryOp::Add, |x: i64, y: i64| x.checked_add(y).ok_or(ops::CODE_OVERFLOW), |x: f64, y: f64| x + y),
            Op::Sub(a, b, c) => arith!(a, b, c, BinaryOp::Sub, |x: i64, y: i64| x.checked_sub(y).ok_or(ops::CODE_OVERFLOW), |x: f64, y: f64| x - y),
            Op::Mul(a, b, c) => arith!(a, b, c, BinaryOp::Mul, |x: i64, y: i64| x.checked_mul(y).ok_or(ops::CODE_OVERFLOW), |x: f64, y: f64| x * y),
            Op::Div(a, b, c) => arith!(a, b, c, BinaryOp::Div, ops::int_div, |x: f64, y: f64| x / y),
            Op::Rem(a, b, c) => arith!(a, b, c, BinaryOp::Rem, ops::int_rem, |x: f64, y: f64| x % y),
            Op::Lt(a, b, c) => compare!(a, b, c, BinaryOp::Lt, <),
            Op::Le(a, b, c) => compare!(a, b, c, BinaryOp::Le, <=),
            Op::Gt(a, b, c) => compare!(a, b, c, BinaryOp::Gt, >),
            Op::Ge(a, b, c) => compare!(a, b, c, BinaryOp::Ge, >=),
            Op::Eq(a, b, c) => equal!(a, b, c, true),
            Op::Ne(a, b, c) => equal!(a, b, c, false),
            Op::Bin(bop, a, b, c) => {
                let v = eval_binary(r!(b).clone(), bop, r!(c).clone())?;
                r!(a) = v;
            }
            Op::Neg(a, b) => {
                let v = match &r!(b) {
                    Value::Int(n) => n.checked_neg().map(Value::Int).ok_or_else(|| ops::ERR_OVERFLOW.to_string())?,
                    Value::Float(n) => Value::Float(-n),
                    Value::Duration(n) => n.checked_neg().map(Value::Duration).ok_or_else(|| ops::ERR_OVERFLOW.to_string())?,
                    other => return Err(format!("operator '-' not applicable to '{}'", other.type_name())),
                };
                r!(a) = v;
            }
            Op::Not(a, b) => {
                let v = Value::Bool(!r!(b).is_truthy());
                r!(a) = v;
            }
            Op::BitNot(a, b) => {
                let v = match &r!(b) {
                    Value::Int(n) => Value::Int(!n),
                    other => return Err(format!("operator '~' not applicable to '{}'", other.type_name())),
                };
                r!(a) = v;
            }

            Op::Jmp(t) => pc = t as usize,
            Op::JmpBack(t) => {
                if it.deadline_count.get() != 0 {
                    it.check_deadline()?;
                }
                pc = t as usize;
            }
            Op::JmpF(a, t) => {
                if !r!(a).is_truthy() {
                    pc = t as usize;
                }
            }
            Op::JmpT(a, t) => {
                if r!(a).is_truthy() {
                    pc = t as usize;
                }
            }
            Op::JmpNotNil(a, t) => {
                if !matches!(r!(a), Value::Nil) {
                    pc = t as usize;
                }
            }

            Op::NewList(a, b, n) => {
                let items = take_args!(b, n);
                r!(a) = Value::List(Rc::new(RefCell::new(items)));
            }
            Op::NewTuple(a, b, n) => {
                let items = take_args!(b, n);
                r!(a) = Value::tuple(items);
            }
            Op::NewMap(a, b, k) => {
                let keys = &cur.proto.keys[k as usize];
                let mut map = HashMap::with_capacity(keys.len());
                for (i, key) in keys.iter().enumerate() {
                    map.insert(key.clone(), take(&mut r!(b as usize + i)));
                }
                r!(a) = Value::Map(Rc::new(RefCell::new(map)));
            }
            Op::Template(a, b, n) => {
                let mut out = String::new();
                for i in 0..n as usize {
                    match take(&mut r!(b as usize + i)) {
                        Value::Str(s) => out.push_str(&s),
                        v => out.push_str(&v.to_display_string()),
                    }
                }
                r!(a) = Value::Str(out.into());
            }
            Op::Range(a, b, c) => {
                let v = match (&r!(b), &r!(c)) {
                    (Value::Int(x), Value::Int(y)) => Value::Range(*x, *y),
                    _ => return Err("range '..' requires integers".into()),
                };
                r!(a) = v;
            }
            Op::Index(a, b, c) => {
                let v = match (&r!(b), &r!(c)) {
                    (Value::List(l), Value::Int(i)) => {
                        let l = l.borrow();
                        let at = norm_index(*i, l.len())?;
                        l[at].clone()
                    }
                    (t, i) => eval_index(t.clone(), i.clone())?,
                };
                r!(a) = v;
            }
            Op::SetIndex(a, b, c) => assign_index(&r!(a), r!(b).clone(), r!(c).clone())?,
            Op::GetField(a, b, n) => {
                let name = &cur.proto.names[n as usize];
                let v = match &r!(b) {
                    Value::Map(m) => m.borrow().get(name.as_str()).cloned().unwrap_or(Value::Nil),
                    Value::Struct(s) => {
                        let inst = s.borrow();
                        let slot = &cur.proto.cache[pc - 1];
                        let hit = match &*slot.borrow() {
                            Cache::Field(id, i) if *id == inst.layout.id => Some(*i),
                            _ => None,
                        };
                        match hit {
                            Some(i) => inst.values[i as usize].clone(),
                            None => {
                                check_private(it, &r!(b), name, &cur.f)?;
                                // a method wins over a field of the same name
                                if !inst.methods.contains_key(name.as_str()) {
                                    if let Some(i) = inst.layout.index.get(name.as_str()) {
                                        *slot.borrow_mut() = Cache::Field(inst.layout.id, *i);
                                    }
                                }
                                drop(inst);
                                it.eval_access(r!(b).clone(), name)?
                            }
                        }
                    }
                    t => it.eval_access(t.clone(), name)?,
                };
                r!(a) = v;
            }
            Op::SetField(a, n, c) => {
                let name = &cur.proto.names[n as usize];
                let mut done = false;
                if let Value::Struct(s) = &r!(a) {
                    let slot = &cur.proto.cache[pc - 1];
                    let mut inst = s.borrow_mut();
                    let hit = match &*slot.borrow() {
                        Cache::Field(id, i) if *id == inst.layout.id => Some(*i),
                        _ => None,
                    };
                    match hit {
                        Some(i) => {
                            inst.values[i as usize] = r!(c).clone();
                            done = true;
                        }
                        None => {
                            if let Some(i) = inst.layout.index.get(name.as_str()) {
                                // cached once the write below passed the privacy check
                                if !name.starts_with('_') || inst.module.is_none() || inst.module == env!().borrow().module_name() {
                                    *slot.borrow_mut() = Cache::Field(inst.layout.id, *i);
                                }
                            }
                        }
                    }
                }
                if !done {
                    check_private(it, &r!(a), name, &cur.f)?;
                    it.assign_field(&r!(a), name, r!(c).clone())?;
                }
            }

            Op::Call(a, b, n) => {
                let callee = take(&mut r!(b));
                if let Value::Fn(f) = &callee {
                    let slot = &cur.proto.cache[pc - 1];
                    let hit = match &*slot.borrow() {
                        Cache::Func(body, p) if *body == Rc::as_ptr(&f.body) as usize && p.mode.get() == INLINE => Some(Rc::clone(p)),
                        _ => None,
                    };
                    let proto = match hit {
                        Some(p) => Some(p),
                        None => {
                            let p = inline_proto(it, f, false);
                            if let Some(p) = &p {
                                *slot.borrow_mut() = Cache::Func(Rc::as_ptr(&f.body) as usize, Rc::clone(p));
                            }
                            p
                        }
                    };
                    if let Some(proto) = proto {
                        if let Some(h) = host {
                            h.before_call(it, env!());
                        }
                        let Value::Fn(f) = callee else { unreachable!() };
                        let name = Rc::clone(&f);
                        push_frame!(f, proto, None::<Value>, "function", &name.name, b + 1, n as usize, a);
                        continue;
                    }
                }
                let args = take_args!(b + 1, n);
                let v = it.call_value(callee, args, Rc::clone(env!()))?;
                r!(a) = v;
            }
            Op::CallMethod(a, b, s) => {
                let proto = Rc::clone(&cur.proto);
                let (name, n) = &proto.sites[s as usize];
                let recv = take(&mut r!(b));
                // a struct's compiled method: a frame in this loop
                if let Value::Struct(inst) = &recv {
                    let layout = inst.borrow().layout.id;
                    let slot = &cur.proto.cache[pc - 1];
                    let hit = match &*slot.borrow() {
                        Cache::Method(id, f, p) if *id == layout => Some((Rc::clone(f), p.clone())),
                        _ => None,
                    };
                    let found = match hit {
                        Some(found) => Some(found),
                        None => {
                            let method = inst.borrow().methods.get(name.as_str()).cloned();
                            match method {
                                Some(f) => {
                                    check_private(it, &recv, name, &cur.f)?;
                                    let p = inline_proto(it, &f, true);
                                    *slot.borrow_mut() = Cache::Method(layout, Rc::clone(&f), p.clone());
                                    Some((f, p))
                                }
                                None => None,
                            }
                        }
                    };
                    if let Some((f, callee_proto)) = found {
                        if let Some(h) = host {
                            h.before_call(it, env!());
                        }
                        match callee_proto {
                            Some(callee) => {
                                push_frame!(f, callee, Some(recv), "method", name, b + 1, *n as usize, a);
                                continue;
                            }
                            None => {
                                let args = take_args!(b + 1, *n);
                                let Value::Struct(inst) = recv else { unreachable!() };
                                let v = it.call_struct_fn(inst, f, name, args)?;
                                r!(a) = v;
                                continue;
                            }
                        }
                    }
                }
                // the commonest built-in methods, without an argument vector
                // (what `collections::call_method` does for them)
                match (&recv, name.as_str(), *n) {
                    (Value::List(l), "len", 0) => {
                        let v = Value::Int(l.borrow().len() as i64);
                        r!(a) = v;
                        continue;
                    }
                    (Value::List(l), "push", 1) => {
                        let v = take(&mut r!(b + 1));
                        l.borrow_mut().push(v);
                        r!(a) = recv;
                        continue;
                    }
                    (Value::Str(s), "len", 0) => {
                        let v = Value::Int(s.char_len() as i64);
                        r!(a) = v;
                        continue;
                    }
                    _ => {}
                }
                let args = take_args!(b + 1, *n);
                let builtin = match &recv {
                    Value::List(_) | Value::Str(_) | Value::Buffer(_) => true,
                    Value::Map(m) => !crate::runtime::interpreter::map_key_is_method(m, name.as_str()),
                    _ => false,
                };
                check_private(it, &recv, name, &cur.f)?;
                let v = if builtin {
                    match crate::native::collections::call_method(it, &recv, name, &args) {
                        Some(r) => r?,
                        // a key holding something else: calling it says what it is
                        None if matches!(&recv, Value::Map(m) if m.borrow().contains_key(name.as_str())) => {
                            let callee = it.eval_access(recv, name)?;
                            it.call_value(callee, args, Rc::clone(env!()))?
                        }
                        None => return Err(format!("'{}' has no method '{}'", recv.type_name(), name)),
                    }
                } else {
                    let callee = it.eval_access(recv, name)?;
                    it.call_value(callee, args, Rc::clone(env!()))?
                };
                r!(a) = v;
            }
            Op::New(a, b, s) => {
                let (name, n) = &cur.proto.sites[s as usize];
                let args = take_args!(b, *n);
                match it.new_struct(name, args, env!()) {
                    Ok(v) => r!(a) = v,
                    // a `?` in a field's default value or in the constructor
                    Err(e) if e == PROPAGATE => do_return!(propagated(it, &cur.proto)),
                    Err(e) => return Err(e),
                }
            }
            Op::Ret(a) => do_return!(take(&mut r!(a))),
            Op::RetNil => do_return!(Value::Nil),
            Op::Try(a) => {
                let v = take(&mut r!(a));
                match it.propagate(v) {
                    Ok(v) => r!(a) = v,
                    Err(e) if e == PROPAGATE => do_return!(propagated(it, &cur.proto)),
                    Err(e) => return Err(e),
                }
            }
            Op::Destructure(a, b, n, literal) => {
                let items = destructure(&r!(a), literal, n as usize);
                for (i, v) in items.into_iter().take(n as usize).enumerate() {
                    r!(b as usize + i) = v;
                }
            }
            Op::CheckParam(a, t) => {
                let (name, ty) = &cur.proto.types[t as usize];
                if !it.type_matches(&r!(a), ty, env!())? {
                    return Err(format!("type mismatch: parameter '{}' expects {}, got {}", name, ty, describe_for(&r!(a), ty)));
                }
            }
            Op::CheckVar(a, t) => {
                let (name, ty) = &cur.proto.types[t as usize];
                it.check_var(name, &r!(a), ty, env!())?;
            }

            Op::IterInit(slot, b) => {
                iters[cur.iter_base + slot as usize] = match r!(b).clone() {
                    Value::Range(lo, hi) => Iter::Range(lo, hi),
                    // a snapshot: the body may modify the list
                    Value::List(l) => Iter::Items(l.borrow().clone().into_iter()),
                    Value::Tuple(t) => Iter::Items(t.to_vec().into_iter()),
                    Value::Buffer(b) => Iter::Bytes(b.borrow().clone().into_iter()),
                    Value::Chan(ch) => Iter::Chan(ch),
                    Value::Str(s) => Iter::Items(s.chars().map(|c| Value::Str(c.into())).collect::<Vec<_>>().into_iter()),
                    Value::Map(m) => {
                        // deterministic (alphabetical) key order
                        let mut keys: Vec<Str> = m.borrow().keys().cloned().collect();
                        keys.sort();
                        Iter::Items(keys.into_iter().map(|k| Value::Str(k.into())).collect::<Vec<_>>().into_iter())
                    }
                    Value::Object(o) if matches!(o.name().as_str(), "TcpListener" | "TlsListener") => Iter::Accept(o),
                    Value::Object(o) if o.name() == "DataChannel" => Iter::Recv(o),
                    other => return Err(format!("value of type '{}' is not iterable", other.type_name())),
                };
            }
            Op::IterInit2(slot, b, names) => {
                let pairs: Vec<(Value, Value)> = match &r!(b) {
                    Value::Map(m) => {
                        let m = m.borrow();
                        let mut keys: Vec<&Str> = m.keys().collect();
                        keys.sort();
                        keys.into_iter().map(|k| (Value::Str(k.clone().into()), m[k].clone())).collect()
                    }
                    Value::List(l) => l.borrow().iter().enumerate().map(|(i, v)| (Value::Int(i as i64), v.clone())).collect(),
                    Value::Tuple(t) => t.iter().enumerate().map(|(i, v)| (Value::Int(i as i64), v.clone())).collect(),
                    Value::Buffer(b) => b.borrow().iter().enumerate().map(|(i, x)| (Value::Int(i as i64), Value::Int(*x as i64))).collect(),
                    Value::Str(s) => s.chars().enumerate().map(|(i, c)| (Value::Int(i as i64), Value::Str(c.to_string().into()))).collect(),
                    other => {
                        let (v1, v2) = &cur.proto.pairs[names as usize];
                        return Err(format!("'for {}, {} in' expects a map, list or str, got '{}'", v1, v2, other.type_name()));
                    }
                };
                iters[cur.iter_base + slot as usize] = Iter::Pairs(pairs.into_iter());
            }
            Op::IterNext(slot, b, t) => {
                let next = match &mut iters[cur.iter_base + slot as usize] {
                    Iter::Range(i, end) => {
                        if *i < *end {
                            *i += 1;
                            Some(Value::Int(*i - 1))
                        } else {
                            None
                        }
                    }
                    Iter::Items(items) => items.next(),
                    Iter::Bytes(bytes) => bytes.next().map(|x| Value::Int(x as i64)),
                    Iter::Chan(ch) => match it.keep_loc(|| chan_recv(ch)) {
                        Ok(v) => Some(v),
                        Err(RecvError::Closed) => None,
                        Err(RecvError::Other(e)) => return Err(e),
                    },
                    Iter::Accept(o) => Some(it.call_native_method(o, "accept", &[])?),
                    Iter::Recv(o) => Some(it.call_native_method(o, "recv", &[])?).filter(|m| !matches!(m, Value::Nil)),
                    Iter::Pairs(_) | Iter::Done => None,
                };
                match next {
                    Some(v) => {
                        if it.deadline_count.get() != 0 {
                            it.check_deadline()?;
                        }
                        r!(b) = v;
                    }
                    None => pc = t as usize,
                }
            }
            Op::IterNext2(slot, b, t) => {
                let next = match &mut iters[cur.iter_base + slot as usize] {
                    Iter::Pairs(pairs) => pairs.next(),
                    _ => None,
                };
                match next {
                    Some((k, v)) => {
                        if it.deadline_count.get() != 0 {
                            it.check_deadline()?;
                        }
                        r!(b) = k;
                        r!(b + 1) = v;
                    }
                    None => pc = t as usize,
                }
            }
            Op::IterEnd(slot) => iters[cur.iter_base + slot as usize] = Iter::Done,
            Op::Clear(a, b) => {
                for i in a..b {
                    r!(i) = Value::Nil;
                }
            }
        }
    }
}
