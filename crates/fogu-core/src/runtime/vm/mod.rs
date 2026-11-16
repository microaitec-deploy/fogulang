//! The bytecode VM (Phase 4, `docs/bytecode.md`).
//!
//! A function body is compiled once to a [`Proto`]: a register machine whose
//! locals are slots resolved at compile time (no `Environment` per call or
//! per block, no scope walk per variable). Names that are not locals of the
//! function are read from the environment where it was defined, exactly as the
//! tree-walker does.
//!
//! The VM is selected per program (`--engine=vm`, `FOGU_ENGINE=vm`) and works
//! per function: a body that uses a construct the compiler does not know yet
//! is run by the tree-walker (`FOGU_VM_DEBUG=1` says which and why). The
//! tree-walking interpreter stays the semantic reference: every operation that
//! has a non-trivial meaning (calls, fields, indexing, arithmetic outside the
//! int/float fast paths, `?`, type checks) calls the interpreter's own code, so
//! results and error messages are the same.

mod compiler;
mod exec;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::super::ast::{BinaryOp, Block, Stmt, TypeExpr};
use super::super::symbol::Sym;
use super::super::value::{FnValue, Value};
use super::Interpreter;

pub use compiler::compile;

/// A register of a frame.
pub type Reg = u16;

/// One instruction (8 bytes). `A` is the destination unless said otherwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// The source line of what follows (for errors and `error` traces).
    Line(u32),
    /// `R[A] = R[B]`
    Move(Reg, Reg),
    /// `R[A] = consts[B]`
    LoadK(Reg, u16),
    LoadNil(Reg),
    LoadBool(Reg, bool),
    LoadInt(Reg, i32),
    /// `R[A] = globals[B]`, read in the function's defining environment.
    GetGlobal(Reg, u16),
    /// `globals[A] = R[B]`
    SetGlobal(u16, Reg),
    /// `globals[A] = R[B] op R[C]` where `R[B]` holds the global's value read
    /// before the right side ran (`g += x`, `g = g + x`); the result stays in `R[B]`.
    GlobalOp(u16, Reg, Reg, BinaryOp),

    /// `R[A] = R[B] op R[C]`: ints and floats inline, the rest as the interpreter.
    Add(Reg, Reg, Reg),
    Sub(Reg, Reg, Reg),
    Mul(Reg, Reg, Reg),
    Div(Reg, Reg, Reg),
    Rem(Reg, Reg, Reg),
    Lt(Reg, Reg, Reg),
    Le(Reg, Reg, Reg),
    Gt(Reg, Reg, Reg),
    Ge(Reg, Reg, Reg),
    Eq(Reg, Reg, Reg),
    Ne(Reg, Reg, Reg),
    /// Any other binary operator (`**`, bits).
    Bin(BinaryOp, Reg, Reg, Reg),
    Neg(Reg, Reg),
    Not(Reg, Reg),
    BitNot(Reg, Reg),

    Jmp(u32),
    /// A loop's back edge: also the `fogu:vm` deadline check.
    JmpBack(u32),
    /// Jump when `R[A]` is falsy (`false` or `nil`).
    JmpF(Reg, u32),
    JmpT(Reg, u32),
    /// Jump when `R[A]` is not `nil` (`??`).
    JmpNotNil(Reg, u32),

    /// `R[A] = [R[B] .. R[B+C]]` (the registers are emptied)
    NewList(Reg, Reg, u16),
    /// `R[A] = { keys[C][i]: R[B+i] }`
    NewMap(Reg, Reg, u16),
    /// `R[A]` = the text of `R[B] .. R[B+C]`
    Template(Reg, Reg, u16),
    /// `R[A] = R[B]..R[C]`
    Range(Reg, Reg, Reg),
    /// `R[A] = R[B][R[C]]`
    Index(Reg, Reg, Reg),
    /// `R[A][R[B]] = R[C]`
    SetIndex(Reg, Reg, Reg),
    /// `R[A] = R[B].names[C]`
    GetField(Reg, Reg, u16),
    /// `R[A].names[B] = R[C]`
    SetField(Reg, u16, Reg),

    /// `R[A] = R[B](R[B+1] .. R[B+C])`
    Call(Reg, Reg, u16),
    /// `R[A] = R[B].name(R[B+1] ..)`, name and argument count in `sites[C]`
    CallMethod(Reg, Reg, u16),
    /// `R[A] = new Name(R[B] ..)`, name and argument count in `sites[C]`
    New(Reg, Reg, u16),
    Ret(Reg),
    RetNil,
    /// `R[A] = R[A]?`: the value, or the function returns the error.
    Try(Reg),
    /// `const [a, b] = R[A]`: `C` registers from `R[B]`.
    Destructure(Reg, Reg, u16),
    /// A parameter's annotation (`types[B]`).
    CheckParam(Reg, u16),
    /// `let x: T = R[A]` (`types[B]`).
    CheckVar(Reg, u16),

    /// Starts `for x in R[B]` in the iterator slot `A`.
    IterInit(u8, Reg),
    /// Starts `for a, b in R[B]`; the variables' names (for the error) are `pairs[C]`.
    IterInit2(u8, Reg, u16),
    /// `R[B]` = the next item of the slot `A`, or jump to `C` when it is over.
    IterNext(u8, Reg, u32),
    /// As `IterNext`, for `R[B]` and `R[B+1]`.
    IterNext2(u8, Reg, u32),
    IterEnd(u8),
    /// `R[A] .. R[B]` = nil: a scope ended, its values are released.
    Clear(Reg, Reg),
}

/// What an instruction learned the last time it ran, so the next run skips
/// the lookup by name: valid while the layout (or the function body) is the same.
pub enum Cache {
    Empty,
    /// `GetField` / `SetField` on a struct: its layout and the field's slot.
    Field(u64, u32),
    /// `CallMethod` on a struct: its layout, the method and its proto (`None`:
    /// the tree-walker runs it).
    Method(u64, Rc<FnValue>, Option<Rc<Proto>>),
    /// `Call` of a compiled function: the address of its body and its proto.
    Func(usize, Rc<Proto>),
}

/// A compiled function body.
pub struct Proto {
    pub name: String,
    /// The body it was compiled from (kept alive: its address is the cache key).
    pub body: Block,
    /// Parameters without `this` and without `...rest`.
    pub nparams: u16,
    pub rest: bool,
    pub has_this: bool,
    pub nregs: u16,
    pub niters: u8,
    pub code: Vec<Op>,
    pub consts: Vec<Value>,
    pub globals: Vec<(Sym, Rc<str>)>,
    pub names: Vec<String>,
    /// Method / struct name and number of arguments of a call site.
    pub sites: Vec<(String, u16)>,
    pub keys: Vec<Vec<String>>,
    pub types: Vec<(String, TypeExpr)>,
    pub pairs: Vec<(String, String)>,
    pub ret_type: Option<TypeExpr>,
    /// May a `?` end this function (`Try`, or a `new` whose constructor has one)?
    pub propagates: bool,
    /// One inline cache per instruction (see [`Cache`]).
    pub cache: Vec<RefCell<Cache>>,
    /// How the VM calls this function from compiled code: see [`exec`].
    pub mode: std::cell::Cell<u8>,
    /// Calls seen while the JIT may still compile it.
    pub calls: std::cell::Cell<u32>,
}

impl Proto {
    /// The listing printed by `fogu disasm`.
    pub fn disasm(&self) -> String {
        let mut out = format!(
            "fn {} (params: {}{}{}, registers: {})\n",
            self.name,
            self.nparams,
            if self.rest { " + rest" } else { "" },
            if self.has_this { ", this" } else { "" },
            self.nregs
        );
        for (pc, op) in self.code.iter().enumerate() {
            let note = match *op {
                Op::LoadK(_, k) => format!("  ; {}", self.consts[k as usize].to_display_string()),
                Op::GetGlobal(_, g) | Op::SetGlobal(g, _) | Op::GlobalOp(g, ..) => format!("  ; {}", self.globals[g as usize].1),
                Op::GetField(_, _, n) | Op::SetField(_, n, _) => format!("  ; .{}", self.names[n as usize]),
                Op::CallMethod(_, _, s) | Op::New(_, _, s) => {
                    let (name, n) = &self.sites[s as usize];
                    format!("  ; {} ({} args)", name, n)
                }
                Op::NewMap(_, _, k) => format!("  ; {{{}}}", self.keys[k as usize].join(", ")),
                Op::CheckParam(_, t) | Op::CheckVar(_, t) => format!("  ; {}: {}", self.types[t as usize].0, self.types[t as usize].1),
                _ => String::new(),
            };
            out.push_str(&format!("  {:04}  {:?}{}\n", pc, op, note));
        }
        out
    }
}

// ---------- engine selection ----------

/// 0 = not decided (read `FOGU_ENGINE`), 1 = tree-walker, 2 = VM.
static ENGINE: AtomicU8 = AtomicU8::new(0);

/// Selects the engine for the interpreters created from now on (`--engine=`).
pub fn set_engine(name: &str) -> Result<(), String> {
    let v = match name {
        "tree" => 1,
        "vm" => 2,
        other => return Err(format!("unknown engine '{}' (use tree or vm)", other)),
    };
    ENGINE.store(v, Ordering::Relaxed);
    Ok(())
}

/// Does this process run function bodies on the VM?
pub fn enabled() -> bool {
    match ENGINE.load(Ordering::Relaxed) {
        0 => {
            let on = std::env::var("FOGU_ENGINE").is_ok_and(|v| v == "vm");
            ENGINE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        n => n == 2,
    }
}

fn debug() -> bool {
    static DEBUG: AtomicU8 = AtomicU8::new(0);
    match DEBUG.load(Ordering::Relaxed) {
        0 => {
            let on = std::env::var("FOGU_VM_DEBUG").is_ok_and(|v| v != "0");
            DEBUG.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        n => n == 2,
    }
}

// ---------- the cache of compiled bodies ----------

/// Per interpreter: body address (+ whether it is a method) → its proto, or
/// `None` when the compiler refused it. The body is kept so its address stays its own.
#[derive(Default)]
pub struct Protos {
    map: RefCell<rustc_hash::FxHashMap<(usize, bool), (Block, Option<Rc<Proto>>)>>,
}

impl Protos {
    /// The proto of `f` (compiled on the first call), or `None`: run it on the tree-walker.
    #[inline]
    pub fn get(&self, f: &FnValue, method: bool) -> Option<Rc<Proto>> {
        let key = (Rc::as_ptr(&f.body) as usize, method);
        if let Some((_, p)) = self.map.borrow().get(&key) {
            return p.clone();
        }
        let proto = match compile(&f.name, &f.params, &f.body, method) {
            Ok(p) => {
                if debug() {
                    eprintln!("[vm] function '{}' compiled ({} instructions, {} registers)", f.name, p.code.len(), p.nregs);
                }
                Some(Rc::new(p))
            }
            Err(why) => {
                if debug() {
                    eprintln!("[vm] function '{}' not compiled: {}", f.name, why);
                }
                None
            }
        };
        self.map.borrow_mut().insert(key, (Rc::clone(&f.body), proto.clone()));
        proto
    }
}

/// Runs the compiled body of `f` with its arguments (and `this` for a method).
/// The caller did what a call does before the body: arity, stack and deadline
/// checks, entering the function's module.
#[inline]
pub fn run(it: &Interpreter, p: &Rc<Proto>, f: &Rc<FnValue>, this: Option<Value>, args: Vec<Value>) -> Result<Value, String> {
    exec::run(it, p, f, this, args)
}

/// `fogu disasm`: the listing of every function and method declared at the
/// top level of a program (and why a body is not compiled).
pub fn disasm_program(program: &[Stmt]) -> String {
    use super::super::ast::ExportKind;
    let mut out = String::new();
    let mut one = |name: &str, params: &[String], body: &Block, method: bool| match compile(name, params, body, method) {
        Ok(p) => {
            out.push_str(&p.disasm());
            out.push('\n');
        }
        Err(why) => out.push_str(&format!("fn {}: not compiled ({})\n\n", name, why)),
    };
    let decl = |s: &Stmt, one: &mut dyn FnMut(&str, &[String], &Block, bool)| match s {
        Stmt::FnDecl { name, params, body } => one(name, params, body, false),
        Stmt::Struct(d) => {
            for (m, params, body) in &d.methods {
                one(&format!("{}.{}", d.name, m), params, body, true);
            }
        }
        _ => {}
    };
    for s in program {
        match s {
            Stmt::Export(k) => match &**k {
                ExportKind::Fn { name, params, body } => one(name, params, body, false),
                ExportKind::Struct(d) => {
                    for (m, params, body) in &d.methods {
                        one(&format!("{}.{}", d.name, m), params, body, true);
                    }
                }
                ExportKind::DefaultDecl(inner) => decl(inner, &mut one),
                _ => {}
            },
            other => decl(other, &mut one),
        }
    }
    out
}
