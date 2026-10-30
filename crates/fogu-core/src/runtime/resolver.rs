//! Static checks done before running and by `fogu check` (spec §13.1, §19,
//! §30 and §62):
//!
//! * using a variable that does not exist in any scope → error (with a suggestion);
//! * using a name at the same code level *before* its declaration → error;
//! * unknown type names in annotations → error;
//! * `?` inside a function whose declared return type has no `error` → error;
//! * a `match` without a catch-all arm (`_` or a plain name) → error;
//! * local variables that are never used → warning (reported by `fogu check`).
//!
//! Inside a function body, names of the outer scopes may be declared later
//! (the function only runs afterwards, e.g. mutual recursion).
//! Scopes follow exactly those of the interpreter.

use std::collections::HashSet;

use super::ast::*;

/// Type names that are always available.
const BUILTIN_TYPES: &[&str] = &[
    "int", "float", "str", "string", "bool", "nil", "void", "duration", "list", "map", "tuple",
    "range", "chan", "error", "fn", "struct", "object", "any", "buffer",
];

struct Scope {
    /// Declared up to the current point.
    declared: HashSet<String>,
    /// Everything declared in this scope (including what comes later).
    all: HashSet<String>,
    /// Parameter scope of a function: what is outside it may be used even
    /// if it is declared later.
    fn_boundary: bool,
    /// Local variables (`let`/`const`) declared here, with their line.
    locals: Vec<(String, u32)>,
    /// Names of this scope that were used.
    used: HashSet<String>,
}

pub struct Resolver<'a> {
    scopes: Vec<Scope>,
    builtins: &'a HashSet<String>,
    line: u32,
    /// Declared return type of each enclosing function (`None` = not annotated).
    returns: Vec<(String, Option<TypeExpr>)>,
    warnings: Vec<String>,
}

/// Checks a program (or module). `builtins` are the global native names.
/// Returns the warnings (e.g. unused variables) when there are no errors.
pub fn check(program: &[Stmt], builtins: &HashSet<String>) -> Result<Vec<String>, String> {
    let mut r = Resolver {
        scopes: Vec::new(),
        builtins,
        line: 0,
        returns: Vec::new(),
        warnings: Vec::new(),
    };
    r.push(collect(program), HashSet::new(), false);
    r.stmts(program)?;
    r.pop();
    Ok(r.warnings)
}

/// Names declared directly in a list of statements.
fn collect(stmts: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in stmts {
        match s {
            Stmt::Let { name, .. } | Stmt::FnDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::LetList { names, .. } => {
                out.extend(names.iter().filter(|n| *n != "_").cloned());
            }
            Stmt::Struct(d) => {
                out.insert(d.name.clone());
            }
            Stmt::Interface(d) => {
                out.insert(d.name.clone());
            }
            Stmt::Import { kind, .. } => out.extend(import_names(kind)),
            Stmt::Export(e) => {
                out.insert(match &**e {
                    ExportKind::Fn { name, .. } | ExportKind::Let { name, .. } => name.clone(),
                    ExportKind::Struct(d) => d.name.clone(),
                    ExportKind::Interface(d) => d.name.clone(),
                });
            }
            _ => {}
        }
    }
    out
}

fn import_names(kind: &ImportKind) -> Vec<String> {
    match kind {
        ImportKind::Named { alias } | ImportKind::Namespace { alias } => vec![alias.clone()],
        ImportKind::Members { members } => members.iter().map(|(_, a)| a.clone()).collect(),
    }
}

fn param_names(params: &[String]) -> HashSet<String> {
    params
        .iter()
        .map(|p| p.trim_start_matches(REST_PREFIX).to_string())
        .collect()
}

/// Edit distance (to suggest similar names).
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Does the arm match every value (`_`, or a name without a guard)?
fn catch_all(p: &Pattern) -> bool {
    match p {
        Pattern::Wildcard | Pattern::Ident(_) => true,
        Pattern::IdentBind { guard: None, .. } => true,
        Pattern::Or(ps) => ps.iter().any(catch_all),
        _ => false,
    }
}

/// Literal booleans covered by a pattern (`true | false` is exhaustive).
fn bools(p: &Pattern, out: &mut HashSet<bool>) {
    match p {
        Pattern::Literal(Expr::Bool(b)) => {
            out.insert(*b);
        }
        Pattern::Or(ps) => ps.iter().for_each(|p| bools(p, out)),
        _ => {}
    }
}

impl Resolver<'_> {
    fn push(&mut self, all: HashSet<String>, declared: HashSet<String>, fn_boundary: bool) {
        let mut all = all;
        all.extend(declared.iter().cloned());
        self.scopes.push(Scope {
            declared,
            all,
            fn_boundary,
            locals: Vec::new(),
            used: HashSet::new(),
        });
    }

    /// Leaves a scope, reporting its unused local variables.
    fn pop(&mut self) {
        if let Some(s) = self.scopes.pop() {
            for (name, line) in s.locals {
                if !s.used.contains(&name) && !name.starts_with('_') {
                    self.warnings.push(format!(
                        "line {}: unused variable '{}' (prefix it with '_' to silence this)",
                        line, name
                    ));
                }
            }
        }
    }

    fn declare(&mut self, name: &str) {
        if let Some(s) = self.scopes.last_mut() {
            s.declared.insert(name.to_string());
            s.all.insert(name.to_string());
        }
    }

    /// `let`/`const`: like `declare`, but tracked for the unused warning.
    fn declare_local(&mut self, name: &str) {
        let line = self.line;
        self.declare(name);
        if let Some(s) = self.scopes.last_mut() {
            s.locals.push((name.to_string(), line));
        }
    }

    fn fail<T>(&self, msg: String) -> Result<T, String> {
        Err(format!("line {}: name error: {}", self.line, msg))
    }

    fn type_fail<T>(&self, msg: String) -> Result<T, String> {
        Err(format!("line {}: type error: {}", self.line, msg))
    }

    fn use_name(&mut self, name: &str) -> Result<(), String> {
        let mut crossed_fn = false;
        // Declared later in a scope where it does not exist yet: as in the
        // interpreter, an outer scope may provide the name.
        let mut later = false;
        for s in self.scopes.iter_mut().rev() {
            if s.declared.contains(name) || (crossed_fn && s.all.contains(name)) {
                s.used.insert(name.to_string());
                return Ok(());
            }
            later |= s.all.contains(name);
            if s.fn_boundary {
                crossed_fn = true;
            }
        }
        if self.builtins.contains(name) {
            return Ok(());
        }
        if later {
            return self.fail(format!(
                "variable '{}' used before its declaration\n  help: declare '{}' before this line (there is no hoisting)",
                name, name
            ));
        }
        let mut msg = format!("variable '{}' is not defined", name);
        if let Some(s) = self.suggest(name) {
            msg.push_str(&format!("\n  help: did you mean '{}'?", s));
        }
        self.fail(msg)
    }

    /// Type names used in an annotation must be built-in types or a
    /// struct/interface visible from here.
    fn use_type(&mut self, ty: &TypeExpr) -> Result<(), String> {
        match ty {
            TypeExpr::Any => Ok(()),
            TypeExpr::Named(n) | TypeExpr::Generic(n, _) => {
                if let TypeExpr::Generic(_, args) = ty {
                    for a in args {
                        self.use_type(a)?;
                    }
                }
                if BUILTIN_TYPES.contains(&n.as_str()) {
                    return Ok(());
                }
                let known = self.scopes.iter().any(|s| s.all.contains(n));
                if !known {
                    let mut msg = format!("unknown type '{}'", n);
                    // a small typo, or a longer/shorter spelling (`integer` → `int`)
                    let close = BUILTIN_TYPES
                        .iter()
                        .filter(|t| t.len() >= 3)
                        .map(|t| {
                            let d = distance(n, t);
                            let prefix = n.starts_with(*t) || t.starts_with(n.as_str());
                            (if prefix { d.min(1) } else { d }, *t)
                        })
                        .filter(|(d, _)| *d <= 2)
                        .min();
                    if let Some((_, t)) = close {
                        msg.push_str(&format!("\n  help: did you mean '{}'?", t));
                    }
                    return self.type_fail(msg);
                }
                self.use_name(n)
            }
            TypeExpr::Optional(t) => self.use_type(t),
            TypeExpr::Union(ts) | TypeExpr::Tuple(ts) => ts.iter().try_for_each(|t| self.use_type(t)),
        }
    }

    fn suggest(&self, name: &str) -> Option<String> {
        let max = (name.chars().count() / 3).clamp(1, 2);
        self.scopes
            .iter()
            .flat_map(|s| s.all.iter())
            .chain(self.builtins.iter())
            .map(|c| (distance(name, c), c))
            .filter(|(d, _)| *d <= max)
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)))
            .map(|(_, c)| c.clone())
    }

    fn function(&mut self, name: &str, params: &[String], body: &[Stmt], this: bool) -> Result<(), String> {
        let mut declared = param_names(params);
        if this {
            declared.insert("this".into());
        }
        let ret = match body.first() {
            Some(Stmt::ReturnType(t)) => Some(t.clone()),
            _ => None,
        };
        // The body runs in the same environment as the parameters.
        self.push(collect(body), declared, true);
        self.returns.push((name.to_string(), ret));
        let saved = self.line;
        let r = self.stmts(body);
        self.line = saved;
        self.returns.pop();
        self.pop();
        r
    }

    fn struct_decl(&mut self, d: &StructDecl) -> Result<(), String> {
        for e in &d.embeds {
            self.use_name(e)?;
        }
        // Default field values are evaluated on instantiation.
        self.push(HashSet::new(), HashSet::new(), true);
        for f in &d.fields {
            if let Some(e) = &f.default {
                self.expr(e)?;
            }
        }
        self.pop();
        for (mname, params, body) in &d.methods {
            self.function(&format!("{}.{}", d.name, mname), params, body, true)?;
        }
        if let Some((params, body)) = &d.constructor {
            self.function(&format!("{}.new", d.name), params, body, true)?;
        }
        Ok(())
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for s in stmts {
            self.stmt(s)?;
        }
        Ok(())
    }

    /// Statements in a new scope (block).
    fn block(&mut self, stmts: &[Stmt], declared: HashSet<String>) -> Result<(), String> {
        self.push(collect(stmts), declared, false);
        let r = self.stmts(stmts);
        self.pop();
        r
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Line(n) => self.line = *n,
            Stmt::Let { name, init, ty, .. } => {
                if let Some(t) = ty {
                    self.use_type(t)?;
                }
                self.expr(init)?;
                self.declare_local(name);
            }
            Stmt::LetList { names, init, .. } => {
                self.expr(init)?;
                for n in names.iter().filter(|n| *n != "_") {
                    self.declare_local(n);
                }
            }
            Stmt::CheckType { ty, .. } => self.use_type(ty)?,
            Stmt::ReturnType(ty) => self.use_type(ty)?,
            Stmt::Interface(d) => self.declare(&d.name),
            Stmt::Expr(e) => self.expr(e)?,
            Stmt::Block(b) => self.block(b, HashSet::new())?,
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(cond)?;
                self.stmt(then_branch)?;
                if let Some(e) = else_branch {
                    self.stmt(e)?;
                }
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond)?;
                self.stmt(body)?;
            }
            Stmt::For {
                var,
                var2,
                iter,
                body,
                ..
            } => {
                self.expr(iter)?;
                let mut vars: HashSet<String> = HashSet::from([var.clone()]);
                vars.extend(var2.iter().cloned());
                self.push(HashSet::new(), vars, false);
                let r = self.stmt(body);
                self.pop();
                r?;
            }
            Stmt::Break | Stmt::Continue | Stmt::Return(None) => {}
            Stmt::Return(Some(e)) => self.expr(e)?,
            Stmt::FnDecl { name, params, body } => {
                self.declare(name);
                self.function(name, params, body, false)?;
            }
            Stmt::Import { kind, .. } => {
                for n in import_names(kind) {
                    self.declare(&n);
                }
            }
            Stmt::Export(e) => match &**e {
                ExportKind::Fn { name, params, body } => {
                    self.declare(name);
                    self.function(name, params, body, false)?;
                }
                ExportKind::Let { name, init, ty, .. } => {
                    if let Some(t) = ty {
                        self.use_type(t)?;
                    }
                    self.expr(init)?;
                    self.declare(name);
                }
                ExportKind::Struct(d) => {
                    self.declare(&d.name);
                    self.struct_decl(d)?;
                }
                ExportKind::Interface(d) => self.declare(&d.name),
            },
            Stmt::Struct(d) => {
                self.declare(&d.name);
                self.struct_decl(d)?;
            }
            Stmt::Select { cases, default } => {
                for c in cases {
                    match c {
                        SelectCase::Recv { var, chan, body } => {
                            self.expr(chan)?;
                            // the receive case has its own environment
                            self.push(collect(body), HashSet::from([var.clone()]), false);
                            let r = self.stmts(body);
                            self.pop();
                            r?;
                        }
                        // the other bodies run in the select's environment
                        SelectCase::Send { chan, value, body } => {
                            self.expr(chan)?;
                            self.expr(value)?;
                            self.stmts(body)?;
                        }
                    }
                }
                if let Some(d) = default {
                    self.stmts(d)?;
                }
            }
            Stmt::MatchStmt { subject, arms } => {
                self.expr(subject)?;
                self.arms(arms)?;
            }
            Stmt::SendStmt { chan, value } => {
                self.expr(chan)?;
                self.expr(value)?;
            }
        }
        Ok(())
    }

    fn arms(&mut self, arms: &[MatchArm]) -> Result<(), String> {
        // spec §19: a match must be exhaustive
        let mut seen_bools = HashSet::new();
        arms.iter().for_each(|a| bools(&a.pattern, &mut seen_bools));
        if !arms.iter().any(|a| catch_all(&a.pattern)) && seen_bools.len() < 2 {
            return Err(format!(
                "line {}: type error: non-exhaustive match\n  help: add a '_ => ...' arm for the remaining values",
                self.line
            ));
        }
        for arm in arms {
            let mut binds = HashSet::new();
            self.pattern(&arm.pattern, &mut binds)?;
            self.push(HashSet::new(), binds, false);
            let r = self.pattern_guards(&arm.pattern).and_then(|_| self.expr(&arm.body));
            self.pop();
            r?;
        }
        Ok(())
    }

    /// Resolves the literals of a pattern and collects the names it binds.
    fn pattern(&mut self, p: &Pattern, binds: &mut HashSet<String>) -> Result<(), String> {
        match p {
            Pattern::Wildcard => {}
            Pattern::Literal(e) => self.expr(e)?,
            Pattern::Ident(n) | Pattern::IdentBind { name: n, .. } => {
                binds.insert(n.clone());
            }
            Pattern::Or(ps) => {
                for p in ps {
                    self.pattern(p, binds)?;
                }
            }
        }
        Ok(())
    }

    fn pattern_guards(&mut self, p: &Pattern) -> Result<(), String> {
        match p {
            Pattern::IdentBind { guard: Some(g), .. } => self.expr(g),
            Pattern::Or(ps) => ps.iter().try_for_each(|p| self.pattern_guards(p)),
            _ => Ok(()),
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Str(_)
            | Expr::Bool(_)
            | Expr::Nil
            | Expr::Duration(_) => Ok(()),
            Expr::Ident(n) => self.use_name(n),
            Expr::DynImport(spec) => self.expr(spec),
            Expr::List(v) | Expr::Template(v) => v.iter().try_for_each(|x| self.expr(x)),
            Expr::Map(v) => v.iter().try_for_each(|(_, x)| self.expr(x)),
            Expr::Unary { right, .. } => self.expr(right),
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
                self.expr(left)?;
                self.expr(right)
            }
            Expr::Range { start, end } => {
                self.expr(start)?;
                self.expr(end)
            }
            Expr::Index { target, index } => {
                self.expr(target)?;
                self.expr(index)
            }
            Expr::Access { target, .. } | Expr::OptAccess { target, .. } => self.expr(target),
            Expr::OptChain(inner) => self.expr(inner),
            Expr::Call { callee, args } => {
                self.expr(callee)?;
                args.iter().try_for_each(|x| self.expr(x))
            }
            Expr::Assign { target, value, .. } => {
                self.expr(value)?;
                self.expr(target)
            }
            Expr::New { name, args } => {
                self.use_name(name)?;
                args.iter().try_for_each(|x| self.expr(x))
            }
            Expr::Fn { params, body } => self.function("<anonymous>", params, body, false),
            Expr::Fg { call } => self.expr(call),
            Expr::Chan { buffer } => buffer.iter().try_for_each(|b| self.expr(b)),
            Expr::Recv { chan } => self.expr(chan),
            Expr::Match { subject, arms } => {
                self.expr(subject)?;
                self.arms(arms)
            }
            Expr::Try { expr } => {
                // spec §30: `?` only in functions whose return type has `error`
                if let Some((name, Some(ret))) = self.returns.last() {
                    if !ret.has_error() {
                        return self.type_fail(format!(
                            "'?' cannot be used in '{}': its return type {} has no error\n  help: declare the return type as (T, error), or handle the error here",
                            name, ret
                        ));
                    }
                }
                self.expr(expr)
            }
        }
    }
}
