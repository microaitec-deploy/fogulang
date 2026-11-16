//! AST → bytecode, one function body at a time.
//!
//! Registers follow a stack discipline: parameters first, then each local where
//! it is declared, then the temporaries of the expression being compiled. A
//! block's registers are free again when it ends.
//!
//! `Err(reason)` means "not compiled": the tree-walker runs that body. That is
//! the answer for every construct this step does not cover (closures, `match`,
//! `fg`, channels, `?.`, declarations inside a function) and for the programs
//! whose error is the interpreter's to report (assigning to a constant,
//! declaring a name twice).

use std::rc::Rc;

use super::super::super::ast::*;
use super::super::super::symbol::{self, Sym};
use super::super::super::value::{Str, Value};
use super::{Op, Proto, Reg};

type CResult<T> = Result<T, String>;

struct Local {
    sym: Sym,
    reg: Reg,
    mutable: bool,
}

struct Loop {
    /// Jumps to patch with the loop's exit.
    breaks: Vec<usize>,
    /// Where `continue` goes.
    top: u32,
    /// Registers from here up belong to the iteration: released on `break` / `continue`.
    base: Reg,
}

struct Compiler {
    code: Vec<Op>,
    consts: Vec<Value>,
    globals: Vec<(Sym, Rc<str>)>,
    names: Vec<String>,
    sites: Vec<(String, u16)>,
    keys: Vec<Vec<Str>>,
    types: Vec<(String, TypeExpr)>,
    pairs: Vec<(String, String)>,
    locals: Vec<Local>,
    /// Index in `locals` where each open scope starts.
    scopes: Vec<usize>,
    next: Reg,
    max: Reg,
    loops: Vec<Loop>,
    iters: u8,
    max_iters: u8,
    /// A variable's register being assigned by the expression in compilation:
    /// its operands are still read from it, so nothing may be built in it.
    var_dst: Option<Reg>,
}

fn bail<T>(why: impl Into<String>) -> CResult<T> {
    Err(why.into())
}

fn idx(n: usize, what: &str) -> CResult<u16> {
    u16::try_from(n).map_err(|_| format!("too many {}", what))
}

/// Compiles the body of a function (`method`: it has `this`).
pub fn compile(name: &str, params: &[String], body: &Block, method: bool) -> CResult<Proto> {
    let mut c = Compiler {
        code: Vec::new(),
        consts: Vec::new(),
        globals: Vec::new(),
        names: Vec::new(),
        sites: Vec::new(),
        keys: Vec::new(),
        types: Vec::new(),
        pairs: Vec::new(),
        locals: Vec::new(),
        scopes: vec![0],
        next: 0,
        max: 0,
        loops: Vec::new(),
        iters: 0,
        max_iters: 0,
        var_dst: None,
    };
    if method {
        c.declare("this", false)?;
    }
    let rest = rest_param(params);
    let fixed = if rest.is_some() { params.len() - 1 } else { params.len() };
    for p in &params[..fixed] {
        c.declare(p, true)?;
    }
    if let Some(r) = rest {
        c.declare(r, true)?;
    }
    let mut ret_type = None;
    for (i, s) in body.iter().enumerate() {
        match s {
            Stmt::ReturnType(t) if i == 0 => ret_type = Some(t.clone()),
            other => c.stmt(other)?,
        }
    }
    c.emit(Op::RetNil);
    if c.code.len() > u32::MAX as usize / 2 {
        return bail("function too large");
    }
    let propagates = c.code.iter().any(|op| matches!(op, Op::Try(_) | Op::New(..)));
    let cache = c.code.iter().map(|_| std::cell::RefCell::new(super::Cache::Empty)).collect();
    Ok(Proto {
        name: name.to_string(),
        body: Rc::clone(body),
        nparams: fixed as u16,
        rest: rest.is_some(),
        has_this: method,
        nregs: c.max,
        niters: c.max_iters,
        code: c.code,
        consts: c.consts,
        globals: c.globals,
        names: c.names,
        sites: c.sites,
        keys: c.keys,
        types: c.types,
        pairs: c.pairs,
        ret_type,
        propagates,
        cache,
        mode: std::cell::Cell::new(0),
        calls: std::cell::Cell::new(0),
    })
}

/// May evaluating `e` assign a variable? Then operands evaluated before it
/// must be copied out of their variables' registers first.
fn has_assign(e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Nil | Expr::Ident(_) | Expr::Duration(_) => false,
        Expr::Assign { .. } => true,
        Expr::List(v) | Expr::Template(v) => v.iter().any(has_assign),
        Expr::Map(v) => v.iter().any(|(_, e)| has_assign(e)),
        Expr::Unary { right, .. } => has_assign(right),
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => has_assign(left) || has_assign(right),
        Expr::Range { start, end } => has_assign(start) || has_assign(end),
        Expr::Index { target, index } => has_assign(target) || has_assign(index),
        Expr::Access { target, .. } => has_assign(target),
        Expr::Call { callee, args } => has_assign(callee) || args.iter().any(has_assign),
        Expr::New { args, .. } => args.iter().any(has_assign),
        Expr::Try { expr } => has_assign(expr),
        // not compiled anyway
        _ => true,
    }
}

impl Compiler {
    fn emit(&mut self, op: Op) -> usize {
        self.code.push(op);
        self.code.len() - 1
    }

    fn here(&self) -> u32 {
        self.code.len() as u32
    }

    /// Points the jump at `at` to the next instruction.
    fn patch(&mut self, at: usize) {
        let to = self.here();
        match &mut self.code[at] {
            Op::Jmp(t) | Op::JmpBack(t) | Op::JmpF(_, t) | Op::JmpT(_, t) | Op::JmpNotNil(_, t) | Op::IterNext(_, _, t) | Op::IterNext2(_, _, t) => *t = to,
            other => unreachable!("not a jump: {:?}", other),
        }
    }

    fn alloc(&mut self) -> CResult<Reg> {
        let r = self.next;
        if r >= Reg::MAX - 2 {
            return bail("too many registers");
        }
        self.next += 1;
        self.max = self.max.max(self.next);
        Ok(r)
    }

    /// Declares a local in the innermost scope, in the next register.
    fn declare(&mut self, name: &str, mutable: bool) -> CResult<Reg> {
        let reg = self.alloc()?;
        self.bind(name, reg, mutable)?;
        Ok(reg)
    }

    /// Names `reg` (already allocated) as a local of the innermost scope.
    fn bind(&mut self, name: &str, reg: Reg, mutable: bool) -> CResult<()> {
        let sym = symbol::intern(name);
        let start = *self.scopes.last().unwrap();
        if self.locals[start..].iter().any(|l| l.sym == sym) {
            // the interpreter reports it
            return bail(format!("'{}' declared twice in a scope", name));
        }
        self.locals.push(Local { sym, reg, mutable });
        Ok(())
    }

    fn lookup(&self, sym: Sym) -> Option<&Local> {
        self.locals.iter().rev().find(|l| l.sym == sym)
    }

    fn open_scope(&mut self) {
        self.scopes.push(self.locals.len());
    }

    /// Ends the innermost scope: its registers are free (and emptied, so the
    /// values they held are released as the interpreter's scope would).
    fn close_scope(&mut self, base: Reg) {
        let start = self.scopes.pop().unwrap();
        if self.locals.len() > start {
            self.emit(Op::Clear(base, self.next));
        }
        self.locals.truncate(start);
        self.next = base;
    }

    fn konst(&mut self, v: Value) -> CResult<u16> {
        self.consts.push(v);
        idx(self.consts.len() - 1, "constants")
    }

    fn global(&mut self, name: &Name) -> CResult<u16> {
        if let Some(i) = self.globals.iter().position(|(s, _)| *s == name.sym) {
            return Ok(i as u16);
        }
        self.globals.push((name.sym, Rc::from(name.text.as_str())));
        idx(self.globals.len() - 1, "names")
    }

    fn name(&mut self, field: &str) -> CResult<u16> {
        if let Some(i) = self.names.iter().position(|n| n == field) {
            return Ok(i as u16);
        }
        self.names.push(field.to_string());
        idx(self.names.len() - 1, "names")
    }

    fn site(&mut self, name: &str, nargs: usize) -> CResult<u16> {
        let n = idx(nargs, "arguments")?;
        if let Some(i) = self.sites.iter().position(|(s, k)| s == name && *k == n) {
            return Ok(i as u16);
        }
        self.sites.push((name.to_string(), n));
        idx(self.sites.len() - 1, "call sites")
    }

    fn ty(&mut self, name: &str, t: &TypeExpr) -> CResult<u16> {
        self.types.push((name.to_string(), t.clone()));
        idx(self.types.len() - 1, "annotations")
    }

    // ---------- statements ----------

    fn stmt(&mut self, s: &Stmt) -> CResult<()> {
        match s {
            Stmt::Line(n) => {
                // two markers in a row: only the last one matters
                if let Some(Op::Line(l)) = self.code.last_mut() {
                    *l = *n;
                } else {
                    self.emit(Op::Line(*n));
                }
            }
            Stmt::Let { name, mutable, init, ty } => {
                // the initializer does not see the new name
                let r = self.alloc()?;
                self.into(init, r)?;
                if let Some(t) = ty {
                    let t = self.ty(name, t)?;
                    self.emit(Op::CheckVar(r, t));
                }
                self.bind(name, r, *mutable)?;
            }
            Stmt::LetList { names, mutable, init } => {
                let base = self.next;
                for _ in names {
                    self.alloc()?;
                }
                let mark = self.next;
                let v = self.expr(init, None)?;
                self.emit(Op::Destructure(v, base, names.len() as u16));
                self.next = mark;
                for (i, n) in names.iter().enumerate() {
                    if n != "_" {
                        self.bind(n, base + i as Reg, *mutable)?;
                    }
                }
            }
            Stmt::CheckType { name, ty } => {
                let Some(reg) = self.lookup(symbol::intern(name)).map(|l| l.reg) else {
                    return bail("annotation of a name that is not a local");
                };
                let t = self.ty(name, ty)?;
                self.emit(Op::CheckParam(reg, t));
            }
            // only the first statement of a body means something (handled by `compile`)
            Stmt::ReturnType(_) => {}
            Stmt::Expr(e) => {
                let mark = self.next;
                self.expr(e, None)?;
                self.next = mark;
            }
            Stmt::Block(stmts) => self.block(stmts)?,
            Stmt::If { cond, then_branch, else_branch } => {
                let mark = self.next;
                let c = self.expr(cond, None)?;
                let to_else = self.emit(Op::JmpF(c, 0));
                self.next = mark;
                self.stmt(then_branch)?;
                match else_branch {
                    Some(e) => {
                        let to_end = self.emit(Op::Jmp(0));
                        self.patch(to_else);
                        self.stmt(e)?;
                        self.patch(to_end);
                    }
                    None => self.patch(to_else),
                }
            }
            Stmt::While { cond, body, .. } => {
                let top = self.here();
                let mark = self.next;
                let c = self.expr(cond, None)?;
                let exit = self.emit(Op::JmpF(c, 0));
                self.next = mark;
                self.loops.push(Loop { breaks: vec![exit], top, base: mark });
                self.stmt(body)?;
                self.emit(Op::JmpBack(top));
                self.end_loop();
            }
            Stmt::For { var, var2, iter, body, .. } => {
                if self.iters == u8::MAX {
                    return bail("loops nested too deep");
                }
                let slot = self.iters;
                let base = self.next;
                let it = self.expr(iter, None)?;
                match var2 {
                    None => self.emit(Op::IterInit(slot, it)),
                    Some(v2) => {
                        self.pairs.push((var.clone(), v2.clone()));
                        let p = idx(self.pairs.len() - 1, "loops")?;
                        self.emit(Op::IterInit2(slot, it, p))
                    }
                };
                self.next = base;
                self.iters += 1;
                self.max_iters = self.max_iters.max(self.iters);

                // the loop variables: a scope of their own, constant bindings
                self.open_scope();
                let v = self.declare(var, false)?;
                if let Some(v2) = var2 {
                    self.declare(v2, false)?;
                }
                let top = self.here();
                let next = match var2 {
                    None => self.emit(Op::IterNext(slot, v, 0)),
                    Some(_) => self.emit(Op::IterNext2(slot, v, 0)),
                };
                self.loops.push(Loop { breaks: vec![next], top, base: self.next });
                self.stmt(body)?;
                self.emit(Op::Jmp(top));
                self.end_loop();
                self.emit(Op::IterEnd(slot));
                self.close_scope(base);
                self.iters -= 1;
            }
            Stmt::Break => {
                let Some(l) = self.loops.last() else {
                    return bail("'break' outside a loop");
                };
                let base = l.base;
                if self.next > base {
                    self.emit(Op::Clear(base, self.next));
                }
                let j = self.emit(Op::Jmp(0));
                self.loops.last_mut().unwrap().breaks.push(j);
            }
            Stmt::Continue => {
                let Some(l) = self.loops.last() else {
                    return bail("'continue' outside a loop");
                };
                let (base, top) = (l.base, l.top);
                if self.next > base {
                    self.emit(Op::Clear(base, self.next));
                }
                self.emit(Op::JmpBack(top));
            }
            Stmt::Return(e) => match e {
                Some(e) => {
                    let mark = self.next;
                    let r = self.expr(e, None)?;
                    self.emit(Op::Ret(r));
                    self.next = mark;
                }
                None => {
                    self.emit(Op::RetNil);
                }
            },
            Stmt::Interface(_) => return bail("interface declared in a function"),
            Stmt::FnDecl { .. } => return bail("function declared in a function"),
            Stmt::Struct(_) => return bail("struct declared in a function"),
            Stmt::Import { .. } | Stmt::Export(_) => return bail("import/export in a function"),
            Stmt::Select { .. } => return bail("select"),
            Stmt::MatchStmt { .. } => return bail("match"),
            Stmt::SendStmt { .. } => return bail("channel send"),
        }
        Ok(())
    }

    fn end_loop(&mut self) {
        let l = self.loops.pop().unwrap();
        for j in l.breaks {
            self.patch(j);
        }
    }

    fn block(&mut self, stmts: &[Stmt]) -> CResult<()> {
        let base = self.next;
        self.open_scope();
        for s in stmts {
            self.stmt(s)?;
        }
        self.close_scope(base);
        Ok(())
    }

    // ---------- expressions ----------

    /// Compiles `e` into the register `r`; the temporaries it used are free again.
    fn into(&mut self, e: &Expr, r: Reg) -> CResult<()> {
        let mark = self.next;
        self.expr(e, Some(r))?;
        self.next = mark;
        Ok(())
    }

    /// Compiles `e` into a new temporary.
    fn fresh(&mut self, e: &Expr) -> CResult<Reg> {
        let r = self.alloc()?;
        self.into(e, r)?;
        Ok(r)
    }

    /// An operand evaluated before `later`: a variable's own register, unless
    /// `later` may assign it — then a copy.
    fn operand(&mut self, e: &Expr, later: &[&Expr]) -> CResult<Reg> {
        if later.iter().any(|l| has_assign(l)) {
            self.fresh(e)
        } else {
            self.expr(e, None)
        }
    }

    fn dst(&mut self, dst: Option<Reg>) -> CResult<Reg> {
        match dst {
            Some(r) => Ok(r),
            None => self.alloc(),
        }
    }

    /// Compiles `e`; the result is in the returned register (`dst` when given).
    /// Temporaries above the result may be left allocated: the caller resets `next`.
    fn expr(&mut self, e: &Expr, dst: Option<Reg>) -> CResult<Reg> {
        match e {
            Expr::Int(n) => {
                let a = self.dst(dst)?;
                match i32::try_from(*n) {
                    Ok(small) => self.emit(Op::LoadInt(a, small)),
                    Err(_) => {
                        let k = self.konst(Value::Int(*n))?;
                        self.emit(Op::LoadK(a, k))
                    }
                };
                Ok(a)
            }
            Expr::Float(x) => self.load(Value::Float(*x), dst),
            Expr::Str(s) => self.load(Value::Str(s.clone()), dst),
            Expr::Duration(ns) => self.load(Value::Duration(*ns), dst),
            Expr::Bool(b) => {
                let a = self.dst(dst)?;
                self.emit(Op::LoadBool(a, *b));
                Ok(a)
            }
            Expr::Nil => {
                let a = self.dst(dst)?;
                self.emit(Op::LoadNil(a));
                Ok(a)
            }
            Expr::Ident(name) => match self.lookup(name.sym).map(|l| l.reg) {
                Some(reg) => match dst {
                    Some(a) if a != reg => {
                        self.emit(Op::Move(a, reg));
                        Ok(a)
                    }
                    _ => Ok(reg),
                },
                None => {
                    let g = self.global(name)?;
                    let a = self.dst(dst)?;
                    self.emit(Op::GetGlobal(a, g));
                    Ok(a)
                }
            },
            Expr::List(elems) => {
                let a = self.dst(dst)?;
                let start = self.next;
                for e in elems {
                    self.fresh(e)?;
                }
                self.emit(Op::NewList(a, start, idx(elems.len(), "list items")?));
                self.next = start;
                Ok(a)
            }
            Expr::Map(entries) => {
                let a = self.dst(dst)?;
                let start = self.next;
                for (_, e) in entries {
                    self.fresh(e)?;
                }
                self.keys.push(entries.iter().map(|(k, _)| k.clone()).collect());
                let k = idx(self.keys.len() - 1, "map literals")?;
                self.emit(Op::NewMap(a, start, k));
                self.next = start;
                Ok(a)
            }
            Expr::Template(parts) => {
                let a = self.dst(dst)?;
                let start = self.next;
                for e in parts {
                    self.fresh(e)?;
                }
                self.emit(Op::Template(a, start, idx(parts.len(), "template parts")?));
                self.next = start;
                Ok(a)
            }
            Expr::Unary { op, right } => {
                let a = self.dst(dst)?;
                let mark = self.next;
                let r = self.expr(right, None)?;
                self.emit(match op {
                    UnaryOp::Neg => Op::Neg(a, r),
                    UnaryOp::Not => Op::Not(a, r),
                    UnaryOp::BitNot => Op::BitNot(a, r),
                });
                self.next = mark;
                Ok(a)
            }
            Expr::Binary { left, op, right } => {
                let a = self.dst(dst)?;
                let mark = self.next;
                // a left side that is itself computed is built in the destination:
                // `a + b + c` then grows one text in place instead of copying it
                let computed = !matches!(&**left, Expr::Ident(n) if self.lookup(n.sym).is_some());
                let l = if computed && self.var_dst != Some(a) {
                    self.into(left, a)?;
                    a
                } else {
                    self.operand(left, &[right])?
                };
                let r = self.expr(right, None)?;
                self.emit(binary(*op, a, l, r));
                self.next = mark;
                Ok(a)
            }
            Expr::Logical { left, op, right } => {
                let a = self.dst(dst)?;
                self.into(left, a)?;
                let skip = self.emit(match op {
                    LogicalOp::And => Op::JmpF(a, 0),
                    LogicalOp::Or => Op::JmpT(a, 0),
                    LogicalOp::Coalesce => Op::JmpNotNil(a, 0),
                });
                self.into(right, a)?;
                self.patch(skip);
                Ok(a)
            }
            Expr::Range { start, end } => {
                let a = self.dst(dst)?;
                let mark = self.next;
                let l = self.operand(start, &[end])?;
                let r = self.expr(end, None)?;
                self.emit(Op::Range(a, l, r));
                self.next = mark;
                Ok(a)
            }
            Expr::Index { target, index } => {
                let a = self.dst(dst)?;
                let mark = self.next;
                let t = self.operand(target, &[index])?;
                let i = self.expr(index, None)?;
                self.emit(Op::Index(a, t, i));
                self.next = mark;
                Ok(a)
            }
            Expr::Access { target, field } => {
                let a = self.dst(dst)?;
                let mark = self.next;
                let t = self.expr(target, None)?;
                let n = self.name(field)?;
                self.emit(Op::GetField(a, t, n));
                self.next = mark;
                Ok(a)
            }
            Expr::Call { callee, args } => {
                // callee (or receiver) and arguments in consecutive registers
                let start = self.alloc()?;
                let op = match &**callee {
                    Expr::Access { target, field } => {
                        self.into(target, start)?;
                        for a in args {
                            self.fresh(a)?;
                        }
                        let s = self.site(field, args.len())?;
                        Op::CallMethod(dst.unwrap_or(start), start, s)
                    }
                    Expr::OptAccess { .. } => return bail("'?.' call"),
                    other => {
                        self.into(other, start)?;
                        for a in args {
                            self.fresh(a)?;
                        }
                        Op::Call(dst.unwrap_or(start), start, idx(args.len(), "arguments")?)
                    }
                };
                self.emit(op);
                self.next = start + 1;
                Ok(dst.unwrap_or(start))
            }
            Expr::New { name, args } => {
                let a = self.dst(dst)?;
                let start = self.next;
                for arg in args {
                    self.fresh(arg)?;
                }
                let s = self.site(name, args.len())?;
                self.emit(Op::New(a, start, s));
                self.next = start;
                Ok(a)
            }
            Expr::Try { expr } => {
                let a = self.dst(dst)?;
                self.into(expr, a)?;
                self.emit(Op::Try(a));
                Ok(a)
            }
            Expr::Assign { target, op, value } => {
                let r = self.assign(target, *op, value)?;
                match dst {
                    Some(a) if a != r => {
                        self.emit(Op::Move(a, r));
                        Ok(a)
                    }
                    _ => Ok(r),
                }
            }
            Expr::OptAccess { .. } | Expr::OptChain(_) => bail("'?.'"),
            Expr::DynImport(_) => bail("import()"),
            Expr::Fn { .. } => bail("closure"),
            Expr::Fg { .. } => bail("fg"),
            Expr::Chan { .. } | Expr::Recv { .. } => bail("channel"),
            Expr::Match { .. } => bail("match"),
        }
    }

    fn load(&mut self, v: Value, dst: Option<Reg>) -> CResult<Reg> {
        let a = self.dst(dst)?;
        let k = self.konst(v)?;
        self.emit(Op::LoadK(a, k));
        Ok(a)
    }

    /// `target = value` / `target op= value`; returns the register of the assigned value.
    fn assign(&mut self, target: &Expr, op: Option<BinaryOp>, value: &Expr) -> CResult<Reg> {
        match target {
            Expr::Ident(name) => {
                let local = self.lookup(name.sym).map(|l| (l.reg, l.mutable));
                match local {
                    Some((_, false)) => bail(format!("assignment to the constant '{}'", name)),
                    Some((x, true)) => {
                        match op {
                            Some(bop) => {
                                let mark = self.next;
                                // the variable is read before the right side runs
                                let cur = if has_assign(value) {
                                    let c = self.alloc()?;
                                    self.emit(Op::Move(c, x));
                                    c
                                } else {
                                    x
                                };
                                let r = self.expr(value, None)?;
                                self.emit(binary(bop, x, cur, r));
                                self.next = mark;
                            }
                            // these write their destination last, after reading every operand
                            None if matches!(
                                value,
                                Expr::Binary { .. } | Expr::Unary { .. } | Expr::Call { .. } | Expr::Index { .. } | Expr::Access { .. } | Expr::New { .. }
                            ) =>
                            {
                                let outer = self.var_dst.replace(x);
                                let r = self.into(value, x);
                                self.var_dst = outer;
                                r?;
                            }
                            None => {
                                let mark = self.next;
                                let r = self.expr(value, None)?;
                                if r != x {
                                    self.emit(Op::Move(x, r));
                                }
                                self.next = mark;
                            }
                        }
                        Ok(x)
                    }
                    None => {
                        let g = self.global(name)?;
                        // `g += x` and `g = g + x`: the global is read first
                        let compound = match (op, value) {
                            (Some(bop), _) => Some((bop, value)),
                            (None, Expr::Binary { left, op: BinaryOp::Add, right }) if matches!(&**left, Expr::Ident(l) if l == name) => Some((BinaryOp::Add, &**right)),
                            _ => None,
                        };
                        match compound {
                            Some((bop, rhs)) => {
                                let cur = self.alloc()?;
                                self.emit(Op::GetGlobal(cur, g));
                                let r = self.expr(rhs, None)?;
                                self.emit(Op::GlobalOp(g, cur, r, bop));
                                self.next = cur + 1;
                                Ok(cur)
                            }
                            None => {
                                let r = self.expr(value, None)?;
                                self.emit(Op::SetGlobal(g, r));
                                Ok(r)
                            }
                        }
                    }
                }
            }
            Expr::Access { target: obj, field } => {
                let o = self.operand(obj, &[value])?;
                let n = self.name(field)?;
                let v = match op {
                    None => self.expr(value, None)?,
                    Some(bop) => {
                        let cur = self.alloc()?;
                        self.emit(Op::GetField(cur, o, n));
                        let r = self.expr(value, None)?;
                        self.emit(binary(bop, cur, cur, r));
                        self.next = cur + 1;
                        cur
                    }
                };
                self.emit(Op::SetField(o, n, v));
                Ok(v)
            }
            Expr::Index { target: obj, index } => {
                let o = self.operand(obj, &[index, value])?;
                let i = self.operand(index, &[value])?;
                let v = match op {
                    None => self.expr(value, None)?,
                    Some(bop) => {
                        let cur = self.alloc()?;
                        self.emit(Op::Index(cur, o, i));
                        let r = self.expr(value, None)?;
                        self.emit(binary(bop, cur, cur, r));
                        self.next = cur + 1;
                        cur
                    }
                };
                self.emit(Op::SetIndex(o, i, v));
                Ok(v)
            }
            _ => bail("invalid assignment target"),
        }
    }
}

fn binary(op: BinaryOp, a: Reg, l: Reg, r: Reg) -> Op {
    match op {
        BinaryOp::Add => Op::Add(a, l, r),
        BinaryOp::Sub => Op::Sub(a, l, r),
        BinaryOp::Mul => Op::Mul(a, l, r),
        BinaryOp::Div => Op::Div(a, l, r),
        BinaryOp::Rem => Op::Rem(a, l, r),
        BinaryOp::Lt => Op::Lt(a, l, r),
        BinaryOp::Le => Op::Le(a, l, r),
        BinaryOp::Gt => Op::Gt(a, l, r),
        BinaryOp::Ge => Op::Ge(a, l, r),
        BinaryOp::Eq => Op::Eq(a, l, r),
        BinaryOp::Ne => Op::Ne(a, l, r),
        other => Op::Bin(other, a, l, r),
    }
}
