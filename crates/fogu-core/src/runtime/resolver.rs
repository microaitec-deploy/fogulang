//! Static checks done before running and by `fogu check` (spec §13.1, §19,
//! §30 and §62):
//!
//! * using a variable that does not exist in any scope → error (with a suggestion);
//! * using a name at the same code level *before* its declaration → error;
//! * unknown type names in annotations → error;
//! * `?` inside a function whose declared return type has no `error` → error;
//! * a `match` without a catch-all arm (`_` or a plain name) → error;
//! * a light, conservative type inference (literals, annotated or never
//!   reassigned variables, signatures of the functions declared here):
//!   operators on incompatible types (`1 + "a"`), an annotation that the value
//!   cannot match (`const n: int = "x"`), an argument that cannot match an
//!   annotated parameter, a `return` that cannot match the declared return type,
//!   too many arguments to a function, a wrong number of arguments to a core
//!   global (`len(1, 2)`) → error. These are exactly the errors the interpreter
//!   would raise when running that code; when a type is not known, nothing is
//!   reported;
//! * warnings (reported by `fogu check`): local variables that are never used,
//!   unused imports, code after `return`/`break`/`continue`, a variable that
//!   shadows another of the same function. Names starting with `_` are exempt.
//!
//! Inside a function body, names of the outer scopes may be declared later
//! (the function only runs afterwards, e.g. mutual recursion).
//! Scopes follow exactly those of the interpreter.
//!
//! Errors are collected per statement (`analyze` returns all of them);
//! `check` — what the interpreter runs before a program — returns the first.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::ast::*;

/// Type names that are always available.
const BUILTIN_TYPES: &[&str] = &[
    "int", "float", "str", "string", "bool", "nil", "void", "duration", "list", "map", "tuple",
    "range", "chan", "error", "fn", "struct", "object", "any", "buffer",
    // sized numbers, as in Go (`ops::SIZED_NAMES`)
    "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "uint", "uintptr",
    "byte", "rune", "float32", "float64",
];

/// Arity of the core globals (`native/context.rs`, `native/collections.rs`):
/// (name, arguments, variadic, result type). Checked by a unit test.
const CORE_GLOBALS: &[(&str, usize, bool, Ty)] = &[
    ("len", 1, false, Ty::Int),
    ("close", 1, false, Ty::Nil),
    ("timeout", 1, false, Ty::Chan),
    ("typeof", 1, false, Ty::Str),
    ("str", 1, false, Ty::Str),
    ("int", 1, false, Ty::Any),
    ("float", 1, false, Ty::Any),
    ("int8", 1, false, Ty::Int),
    ("int16", 1, false, Ty::Int),
    ("int32", 1, false, Ty::Int),
    ("int64", 1, false, Ty::Int),
    ("uint8", 1, false, Ty::Int),
    ("uint16", 1, false, Ty::Int),
    ("uint32", 1, false, Ty::Int),
    ("uint64", 1, false, Ty::Int),
    ("uint", 1, false, Ty::Int),
    ("uintptr", 1, false, Ty::Int),
    ("byte", 1, false, Ty::Int),
    ("rune", 1, false, Ty::Int),
    ("float32", 1, false, Ty::Float),
    ("float64", 1, false, Ty::Float),
    ("clock", 0, false, Ty::Any),
    ("error", 1, true, Ty::Any),
];

/// What the inference knows about a value. Every variant except `Any` and
/// `Num` is exactly the run-time type (`Value::type_name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    /// Not known: never reported.
    Any,
    Int,
    Float,
    /// `int` or `float` (a variable annotated `float` may hold an int).
    Num,
    Str,
    Bool,
    Nil,
    Duration,
    List,
    Map,
    Range,
    Chan,
    Fn,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Any => "any",
            Ty::Int => "int",
            Ty::Float => "float",
            Ty::Num => "number",
            Ty::Str => "str",
            Ty::Bool => "bool",
            Ty::Nil => "nil",
            Ty::Duration => "duration",
            Ty::List => "list",
            Ty::Map => "map",
            Ty::Range => "range",
            Ty::Chan => "chan",
            Ty::Fn => "fn",
        }
    }

    fn numeric(self) -> bool {
        matches!(self, Ty::Int | Ty::Float | Ty::Num)
    }

    /// The type a variable annotated with `t` has (only built-in names).
    fn of_annotation(t: &TypeExpr) -> Ty {
        match t {
            TypeExpr::Named(n) | TypeExpr::Generic(n, _) => match n.as_str() {
                "int" => Ty::Int,
                "float" | "float32" | "float64" => Ty::Num,
                n if super::ops::Sized::from_name(n).is_some() => Ty::Int,
                "str" | "string" => Ty::Str,
                "bool" => Ty::Bool,
                "duration" => Ty::Duration,
                "list" | "tuple" => Ty::List,
                "map" => Ty::Map,
                "range" => Ty::Range,
                "chan" => Ty::Chan,
                _ => Ty::Any,
            },
            _ => Ty::Any,
        }
    }

    /// Can a value of this type match the annotation? `None` = not known.
    /// Mirrors `Interpreter::type_matches` for the built-in type names.
    fn matches(self, t: &TypeExpr) -> Option<bool> {
        if self == Ty::Any {
            return None;
        }
        match t {
            TypeExpr::Any => Some(true),
            TypeExpr::Named(n) | TypeExpr::Generic(n, _) => {
                let ok = match n.as_str() {
                    "int" if self == Ty::Num => return None,
                    "int" => self == Ty::Int,
                    "float" | "float32" | "float64" => self.numeric(),
                    // a sized integer: whether an int fits is only known at run time
                    n if super::ops::Sized::from_name(n).is_some() => {
                        if matches!(self, Ty::Int | Ty::Num) {
                            return None;
                        }
                        false
                    }
                    "str" | "string" => self == Ty::Str,
                    "bool" => self == Ty::Bool,
                    "nil" | "void" => self == Ty::Nil,
                    "duration" => self == Ty::Duration,
                    "list" | "tuple" => self == Ty::List,
                    "map" => self == Ty::Map,
                    "range" => self == Ty::Range,
                    "chan" => self == Ty::Chan,
                    "error" => self == Ty::Nil,
                    "fn" => self == Ty::Fn,
                    // never one of the types this inference knows
                    "buffer" | "struct" | "object" => false,
                    // a struct or an interface (a map may satisfy one)
                    _ => return None,
                };
                Some(ok)
            }
            TypeExpr::Optional(t) => {
                if self == Ty::Nil {
                    Some(true)
                } else {
                    self.matches(t)
                }
            }
            TypeExpr::Union(ts) => {
                let mut unknown = false;
                for t in ts {
                    match self.matches(t) {
                        Some(true) => return Some(true),
                        None => unknown = true,
                        Some(false) => {}
                    }
                }
                if unknown { None } else { Some(false) }
            }
            // a tuple is a list of the right length (not known here)
            TypeExpr::Tuple(_) => {
                if self == Ty::List {
                    None
                } else {
                    Some(false)
                }
            }
        }
    }
}

/// Result type of `l op r`, or the run-time error message (the same text as
/// `interpreter::eval_binary` / `compare_values`).
fn binop(l: Ty, op: BinaryOp, r: Ty) -> Result<Ty, String> {
    use BinaryOp::*;
    use Ty::*;
    match op {
        Eq | Ne => return Ok(Bool),
        Lt | Le | Gt | Ge => {
            return match (l, r) {
                (Any, _) | (_, Any) | (Num, _) | (_, Num) => Ok(Bool),
                _ if l.numeric() && r.numeric() => Ok(Bool),
                (Str, Str) | (Duration, Duration) => Ok(Bool),
                _ => Err(format!("cannot compare '{}' with '{}'", l.name(), r.name())),
            };
        }
        _ => {}
    }
    if l == Any || r == Any {
        return Ok(Any);
    }
    let between = || Err(format!("operator not applicable between '{}' and '{}'", l.name(), r.name()));
    match (l, r) {
        (Int, Int) => Ok(Int),
        _ if l.numeric() && r.numeric() => Ok(if l == Float || r == Float { Float } else { Num }),
        (Str, Str) => match op {
            Add => Ok(Str),
            _ => Err("operator not applicable to strings".into()),
        },
        (Duration, Duration) => match op {
            Add | Sub | Rem => Ok(Duration),
            Div => Ok(Float),
            _ => between(),
        },
        (Duration, Int | Float) => match op {
            Mul | Div => Ok(Duration),
            _ => between(),
        },
        (Int | Float | Num, Duration) if op == Mul => Ok(Duration),
        (Duration, Num) if matches!(op, Mul | Div) => Ok(Duration),
        // the message would name 'int' or 'float': not known
        (Num, _) | (_, Num) => Ok(Any),
        (List, List) => match op {
            Add => Ok(List),
            _ => Err("operator not applicable to lists".into()),
        },
        _ => between(),
    }
}

/// Signature of a function declared with `fn` (never reassigned).
struct Sig {
    name: String,
    params: Vec<String>,
    rest: bool,
    /// Annotation of each (non-variadic) parameter.
    types: HashMap<String, TypeExpr>,
    /// Parameters with a default value.
    defaults: HashSet<String>,
    ret: Option<TypeExpr>,
}

impl Sig {
    fn new(name: &str, params: &[String], body: &[Stmt]) -> Sig {
        let mut types = HashMap::new();
        let mut defaults = HashSet::new();
        let mut ret = None;
        // the prologue written by the parser (`parse_param_list`, `parse_fn_tail`)
        for s in body {
            match s {
                Stmt::ReturnType(t) => ret = Some(t.clone()),
                Stmt::Line(_) => {}
                Stmt::CheckType { name, ty } => {
                    types.insert(name.clone(), ty.clone());
                }
                Stmt::Expr(Expr::Assign { target, op: None, value }) => match (&**target, &**value) {
                    (Expr::Ident(p), Expr::Logical { left, op: LogicalOp::Coalesce, .. })
                        if matches!(&**left, Expr::Ident(q) if q == p) =>
                    {
                        defaults.insert(p.text.clone());
                    }
                    _ => break,
                },
                _ => break,
            }
        }
        let rest = rest_param(params).is_some();
        let params: Vec<String> = params.iter().filter(|p| !p.starts_with(REST_PREFIX)).cloned().collect();
        Sig { name: name.to_string(), params, rest, types, defaults, ret }
    }
}

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
    /// Imported names, with their line.
    imports: Vec<(String, u32)>,
    /// Names of this scope that were used.
    used: HashSet<String>,
    /// Known types of the names of this scope.
    types: HashMap<String, Ty>,
    /// Signatures of the functions declared directly in this scope.
    fns: HashMap<String, Rc<Sig>>,
}

pub struct Resolver<'a> {
    scopes: Vec<Scope>,
    builtins: &'a HashSet<String>,
    line: u32,
    /// Declared return type of each enclosing function (`None` = not annotated).
    returns: Vec<(String, Option<TypeExpr>)>,
    /// Every name that is the target of an assignment somewhere: their type
    /// may change, so it is never inferred.
    assigned: HashSet<String>,
    errors: Vec<String>,
    warnings: Vec<String>,
}

/// Everything found in a program: errors and warnings, in source order,
/// each as `line N: <kind> error: msg` / `line N: msg`.
#[derive(Debug, Default)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Checks a program (or module). `builtins` are the global native names.
/// Returns the warnings (e.g. unused variables) when there are no errors,
/// or the first error.
pub fn check(program: &[Stmt], builtins: &HashSet<String>) -> Result<Vec<String>, String> {
    let mut r = analyze(program, builtins);
    if r.errors.is_empty() {
        Ok(r.warnings)
    } else {
        Err(r.errors.swap_remove(0))
    }
}

/// Checks a program and returns every error and warning (`fogu check`).
pub fn analyze(program: &[Stmt], builtins: &HashSet<String>) -> Report {
    let mut assigned = HashSet::new();
    walk_stmts(program, &mut assigned);
    let mut r = Resolver {
        scopes: Vec::new(),
        builtins,
        line: 0,
        returns: Vec::new(),
        assigned,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    r.push(program, HashSet::new(), false);
    r.stmts(program);
    r.pop();
    Report { errors: r.errors, warnings: r.warnings }
}

/// Names declared directly in a list of statements.
fn collect(stmts: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in stmts {
        out.extend(declared_by(s));
    }
    out
}

/// The names a statement declares in its scope.
fn declared_by(s: &Stmt) -> Vec<String> {
    match s {
        Stmt::Let { name, .. } | Stmt::FnDecl { name, .. } => vec![name.clone()],
        Stmt::LetList { names, .. } => names.iter().filter(|n| *n != "_").cloned().collect(),
        Stmt::Struct(d) => vec![d.name.clone()],
        Stmt::Interface(d) => vec![d.name.clone()],
        Stmt::Import { kind, .. } => import_names(kind),
        Stmt::Export(e) => match &**e {
            ExportKind::Fn { name, .. } | ExportKind::Let { name, .. } => vec![name.clone()],
            ExportKind::Struct(d) => vec![d.name.clone()],
            ExportKind::Interface(d) => vec![d.name.clone()],
            ExportKind::LetList { names, .. } => names.iter().filter(|n| *n != "_").cloned().collect(),
            ExportKind::DefaultDecl(s) => declared_by(s),
            // re-exports and `export default <expr>` bind no name here
            ExportKind::Names { .. } | ExportKind::All { .. } | ExportKind::Default(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Signatures of the functions declared once (and never reassigned) in a
/// list of statements.
fn signatures(stmts: &[Stmt], assigned: &HashSet<String>) -> HashMap<String, Rc<Sig>> {
    let mut count: HashMap<String, usize> = HashMap::new();
    for s in stmts {
        for n in declared_by(s) {
            *count.entry(n).or_default() += 1;
        }
    }
    let mut out = HashMap::new();
    for s in stmts {
        let (name, params, body) = match s {
            Stmt::FnDecl { name, params, body } => (name, params, body),
            Stmt::Export(e) => match &**e {
                ExportKind::Fn { name, params, body } => (name, params, body),
                _ => continue,
            },
            _ => continue,
        };
        if count.get(name) == Some(&1) && !assigned.contains(name) {
            out.insert(name.clone(), Rc::new(Sig::new(name, params, body)));
        }
    }
    out
}

fn import_names(kind: &ImportKind) -> Vec<String> {
    match kind {
        ImportKind::Named { alias } | ImportKind::Namespace { alias } => vec![alias.clone()],
        ImportKind::Members { members } => members.iter().map(|(_, a)| a.clone()).collect(),
        ImportKind::Combined { alias, rest } => {
            let mut out = vec![alias.clone()];
            out.extend(import_names(rest));
            out
        }
    }
}

fn param_names(params: &[String]) -> HashSet<String> {
    params
        .iter()
        .map(|p| p.trim_start_matches(REST_PREFIX).to_string())
        .collect()
}

// ---------- names that are assigned somewhere ----------

fn walk_stmts(stmts: &[Stmt], out: &mut HashSet<String>) {
    stmts.iter().for_each(|s| walk_stmt(s, out));
}

fn walk_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Line(_) | Stmt::CheckType { .. } | Stmt::ReturnType(_) | Stmt::Interface(_) => {}
        Stmt::Break | Stmt::Continue | Stmt::Import { .. } | Stmt::Return(None) => {}
        Stmt::LetList { init, .. } | Stmt::Let { init, .. } => walk_expr(init, out),
        Stmt::Expr(e) | Stmt::Return(Some(e)) => walk_expr(e, out),
        Stmt::Block(b) => walk_stmts(b, out),
        Stmt::If { cond, then_branch, else_branch } => {
            walk_expr(cond, out);
            walk_stmt(then_branch, out);
            if let Some(e) = else_branch {
                walk_stmt(e, out);
            }
        }
        Stmt::While { cond, body, .. } => {
            walk_expr(cond, out);
            walk_stmt(body, out);
        }
        Stmt::For { iter, body, .. } => {
            walk_expr(iter, out);
            walk_stmt(body, out);
        }
        Stmt::FnDecl { body, .. } => walk_stmts(body, out),
        Stmt::Export(e) => match &**e {
            ExportKind::Fn { body, .. } => walk_stmts(body, out),
            ExportKind::Let { init, .. } => walk_expr(init, out),
            ExportKind::Struct(d) => walk_struct(d, out),
            ExportKind::LetList { init, .. } | ExportKind::Default(init) => walk_expr(init, out),
            ExportKind::DefaultDecl(s) => walk_stmt(s, out),
            ExportKind::Interface(_) | ExportKind::Names { .. } | ExportKind::All { .. } => {}
        },
        Stmt::Struct(d) => walk_struct(d, out),
        Stmt::Select { cases, default } => {
            for c in cases {
                match c {
                    SelectCase::Recv { chan, body, .. } => {
                        walk_expr(chan, out);
                        walk_stmts(body, out);
                    }
                    SelectCase::Send { chan, value, body } => {
                        walk_expr(chan, out);
                        walk_expr(value, out);
                        walk_stmts(body, out);
                    }
                }
            }
            if let Some(d) = default {
                walk_stmts(d, out);
            }
        }
        Stmt::MatchStmt { subject, arms } => {
            walk_expr(subject, out);
            walk_arms(arms, out);
        }
        Stmt::SendStmt { chan, value } => {
            walk_expr(chan, out);
            walk_expr(value, out);
        }
    }
}

fn walk_struct(d: &StructDecl, out: &mut HashSet<String>) {
    for f in &d.fields {
        if let Some(e) = &f.default {
            walk_expr(e, out);
        }
    }
    for (_, _, body) in &d.methods {
        walk_stmts(body, out);
    }
    if let Some((_, body)) = &d.constructor {
        walk_stmts(body, out);
    }
}

fn walk_arms(arms: &[MatchArm], out: &mut HashSet<String>) {
    fn guards(p: &Pattern, out: &mut HashSet<String>) {
        match p {
            Pattern::IdentBind { guard: Some(g), .. } => walk_expr(g, out),
            Pattern::Or(ps) => ps.iter().for_each(|p| guards(p, out)),
            _ => {}
        }
    }
    for a in arms {
        guards(&a.pattern, out);
        walk_expr(&a.body, out);
    }
}

fn walk_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Nil | Expr::Duration(_) => {}
        Expr::Ident(_) => {}
        Expr::DynImport(x) | Expr::OptChain(x) => walk_expr(x, out),
        Expr::List(v) | Expr::Template(v) => v.iter().for_each(|x| walk_expr(x, out)),
        Expr::Map(v) => v.iter().for_each(|(_, x)| walk_expr(x, out)),
        Expr::Unary { right, .. } => walk_expr(right, out),
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            walk_expr(left, out);
            walk_expr(right, out);
        }
        Expr::Range { start, end } => {
            walk_expr(start, out);
            walk_expr(end, out);
        }
        Expr::Index { target, index } => {
            walk_expr(target, out);
            walk_expr(index, out);
        }
        Expr::Access { target, .. } | Expr::OptAccess { target, .. } => walk_expr(target, out),
        Expr::Call { callee, args } => {
            walk_expr(callee, out);
            args.iter().for_each(|x| walk_expr(x, out));
        }
        Expr::Assign { target, value, .. } => {
            if let Expr::Ident(n) = &**target {
                out.insert(n.text.clone());
            }
            walk_expr(target, out);
            walk_expr(value, out);
        }
        Expr::New { args, .. } => args.iter().for_each(|x| walk_expr(x, out)),
        Expr::Fn { body, .. } => walk_stmts(body, out),
        Expr::Fg { call } => walk_expr(call, out),
        Expr::Chan { buffer } => buffer.iter().for_each(|b| walk_expr(b, out)),
        Expr::Recv { chan } => walk_expr(chan, out),
        Expr::Match { subject, arms } => {
            walk_expr(subject, out);
            walk_arms(arms, out);
        }
        Expr::Try { expr } => walk_expr(expr, out),
    }
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
    /// Enters a scope whose statements are `stmts`.
    fn push(&mut self, stmts: &[Stmt], declared: HashSet<String>, fn_boundary: bool) {
        let mut all = collect(stmts);
        all.extend(declared.iter().cloned());
        let fns = signatures(stmts, &self.assigned);
        self.scopes.push(Scope {
            declared,
            all,
            fn_boundary,
            locals: Vec::new(),
            imports: Vec::new(),
            used: HashSet::new(),
            types: HashMap::new(),
            fns,
        });
    }

    /// Leaves a scope, reporting its unused local variables and imports.
    fn pop(&mut self) {
        if let Some(s) = self.scopes.pop() {
            let mut unused: Vec<(u32, String)> = Vec::new();
            for (name, line) in s.locals {
                if !s.used.contains(&name) && !name.starts_with('_') {
                    unused.push((
                        line,
                        format!(
                            "line {}: unused variable '{}' (prefix it with '_' to silence this)",
                            line, name
                        ),
                    ));
                }
            }
            for (name, line) in s.imports {
                if !s.used.contains(&name) && !name.starts_with('_') {
                    unused.push((line, format!("line {}: unused import '{}'", line, name)));
                }
            }
            unused.sort_by_key(|(l, _)| *l);
            self.warnings.extend(unused.into_iter().map(|(_, w)| w));
        }
    }

    fn declare(&mut self, name: &str) {
        if let Some(s) = self.scopes.last_mut() {
            s.declared.insert(name.to_string());
            s.all.insert(name.to_string());
        }
    }

    /// `let`/`const`: like `declare`, but tracked for the unused and the
    /// shadowing warnings.
    fn declare_local(&mut self, name: &str) {
        let line = self.line;
        let in_block = self.scopes.last().is_some_and(|s| !s.fn_boundary);
        if in_block && !name.starts_with('_') {
            // an enclosing block of the same function already has this name
            let n = self.scopes.len();
            for s in self.scopes[..n.saturating_sub(1)].iter().rev() {
                if let Some((_, l)) = s.locals.iter().find(|(x, _)| x == name) {
                    self.warnings.push(format!(
                        "line {}: variable '{}' shadows the one declared on line {}",
                        line, name, l
                    ));
                    break;
                }
                if s.fn_boundary {
                    break;
                }
            }
        }
        self.declare(name);
        if let Some(s) = self.scopes.last_mut() {
            s.locals.push((name.to_string(), line));
        }
    }

    /// Records the known type of a name of the current scope.
    fn set_type(&mut self, name: &str, ty: Ty) {
        if ty == Ty::Any || self.assigned.contains(name) {
            return;
        }
        if let Some(s) = self.scopes.last_mut() {
            s.types.insert(name.to_string(), ty);
        }
    }

    /// The scope a name refers to (as `use_name` resolves it).
    fn binding(&self, name: &str) -> Option<&Scope> {
        let mut crossed_fn = false;
        for s in self.scopes.iter().rev() {
            if s.declared.contains(name) || (crossed_fn && s.all.contains(name)) {
                return Some(s);
            }
            if s.fn_boundary {
                crossed_fn = true;
            }
        }
        None
    }

    fn type_of(&self, name: &str) -> Ty {
        match self.binding(name) {
            Some(s) if s.fns.contains_key(name) => Ty::Fn,
            Some(s) => s.types.get(name).copied().unwrap_or(Ty::Any),
            None => Ty::Any,
        }
    }

    fn fail<T>(&self, msg: String) -> Result<T, String> {
        Err(format!("line {}: name error: {}", self.line, msg))
    }

    fn type_fail<T>(&self, msg: String) -> Result<T, String> {
        Err(format!("line {}: type error: {}", self.line, msg))
    }

    /// A type error that does not stop the analysis.
    fn report(&mut self, msg: String) {
        self.errors.push(format!("line {}: type error: {}", self.line, msg));
    }

    /// `export { name }`: the name must exist at this point (the export takes
    /// its current value). Same message as the interpreter.
    fn export_name(&mut self, name: &str) -> Result<(), String> {
        let visible = self.scopes.last().is_some_and(|s| s.declared.contains(name))
            || self.binding(name).is_some()
            || self.builtins.contains(name);
        if visible {
            return self.use_name(name);
        }
        let later = self.scopes.last().is_some_and(|s| s.all.contains(name));
        let mut msg = format!("cannot export '{}': it is not defined", name);
        if later {
            msg.push_str(&format!("\n  help: declare '{}' before this line (the export takes its current value)", name));
        } else if let Some(s) = self.suggest(name) {
            msg.push_str(&format!("\n  help: did you mean '{}'?", s));
        }
        self.fail(msg)
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

    fn function(&mut self, name: &str, params: &[String], body: &[Stmt], this: bool) {
        let mut declared = param_names(params);
        if this {
            declared.insert("this".into());
        }
        let ret = match body.first() {
            Some(Stmt::ReturnType(t)) => Some(t.clone()),
            _ => None,
        };
        // The body runs in the same environment as the parameters.
        self.push(body, declared, true);
        self.returns.push((name.to_string(), ret));
        let saved = self.line;
        self.stmts(body);
        self.line = saved;
        self.returns.pop();
        self.pop();
    }

    fn struct_decl(&mut self, d: &StructDecl) -> Result<(), String> {
        for e in &d.embeds {
            self.use_name(e)?;
        }
        // Default field values are evaluated on instantiation.
        self.push(&[], HashSet::new(), true);
        let r = d.fields.iter().filter_map(|f| f.default.as_ref()).try_for_each(|e| self.expr(e));
        self.pop();
        r?;
        for (mname, params, body) in &d.methods {
            self.function(&format!("{}.{}", d.name, mname), params, body, true);
        }
        if let Some((params, body)) = &d.constructor {
            self.function(&format!("{}.new", d.name), params, body, true);
        }
        Ok(())
    }

    /// Runs the statements of a list; an error stops only its statement.
    fn stmts(&mut self, stmts: &[Stmt]) {
        let mut ended: Option<&str> = None;
        for s in stmts {
            if let Some(kw) = ended {
                if !matches!(s, Stmt::Line(_)) {
                    self.warnings.push(format!("line {}: unreachable code after '{}'", self.line, kw));
                    ended = None;
                    // reported once per list; keep checking the rest
                    self.stmts_after_end(s);
                    continue;
                }
            }
            if let Err(e) = self.stmt(s) {
                self.errors.push(e);
            }
            ended = match s {
                Stmt::Return(_) => Some("return"),
                Stmt::Break => Some("break"),
                Stmt::Continue => Some("continue"),
                _ => ended,
            };
        }
    }

    /// A statement after the end of its list: checked, but no further
    /// "unreachable" warning for the same list.
    fn stmts_after_end(&mut self, s: &Stmt) {
        if let Err(e) = self.stmt(s) {
            self.errors.push(e);
        }
    }

    /// Statements in a new scope (block).
    fn block(&mut self, stmts: &[Stmt], declared: HashSet<String>) {
        self.push(stmts, declared, false);
        self.stmts(stmts);
        self.pop();
    }

    /// `let x: T = init` / `const x = init`.
    fn let_decl(&mut self, name: &str, init: &Expr, ty: &Option<TypeExpr>, local: bool) -> Result<(), String> {
        let typed = match ty {
            Some(t) => self.use_type(t),
            None => Ok(()),
        };
        // declared even when its initializer has an error (no cascade)
        let init_ty = typed.and_then(|_| self.expr_ty(init));
        if local {
            self.declare_local(name);
        } else {
            self.declare(name);
        }
        let init_ty = init_ty?;
        let var_ty = match ty {
            Some(t) => {
                if init_ty.matches(t) == Some(false) {
                    self.report(format!(
                        "type mismatch: variable '{}' is declared as {}, got {}",
                        name,
                        t,
                        init_ty.name()
                    ));
                }
                // `let x: uint8 = 300`: a literal that can never fit
                if let (TypeExpr::Named(n), Some(v)) = (t, int_literal(init)) {
                    if let Some(k) = super::ops::Sized::from_name(n).filter(|k| !k.is_float() && !k.fits(v)) {
                        self.report(format!(
                            "type mismatch: variable '{}' is declared as {}, got {}",
                            name,
                            t,
                            super::ops::out_of_range(n, k)
                        ));
                    }
                }
                Ty::of_annotation(t)
            }
            None => init_ty,
        };
        self.set_type(name, var_ty);
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Line(n) => self.line = *n,
            Stmt::Let { name, init, ty, .. } => self.let_decl(name, init, ty, true)?,
            Stmt::LetList { names, init, .. } => {
                let r = self.expr(init);
                for n in names.iter().filter(|n| *n != "_") {
                    self.declare_local(n);
                }
                r?;
            }
            Stmt::CheckType { name, ty } => {
                self.use_type(ty)?;
                // after the check, the parameter has that type
                let t = Ty::of_annotation(ty);
                self.set_type(name, t);
            }
            Stmt::ReturnType(ty) => self.use_type(ty)?,
            Stmt::Interface(d) => self.declare(&d.name),
            Stmt::Expr(e) => self.expr(e)?,
            Stmt::Block(b) => self.block(b, HashSet::new()),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(cond)?;
                self.branch(then_branch);
                if let Some(e) = else_branch {
                    self.branch(e);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond)?;
                self.branch(body);
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
                self.push(&[], vars, false);
                self.branch(body);
                self.pop();
            }
            Stmt::Break | Stmt::Continue | Stmt::Return(None) => {}
            Stmt::Return(Some(e)) => {
                let t = self.expr_ty(e)?;
                if let Some((name, Some(ret))) = self.returns.last() {
                    if t.matches(ret) == Some(false) {
                        let msg = format!("type mismatch: function '{}' must return {}, got {}", name, ret, t.name());
                        self.report(msg);
                    }
                }
            }
            Stmt::FnDecl { name, params, body } => {
                self.declare(name);
                self.function(name, params, body, false);
            }
            Stmt::Import { kind, .. } => {
                let line = self.line;
                for n in import_names(kind) {
                    self.declare(&n);
                    if let Some(s) = self.scopes.last_mut() {
                        s.imports.push((n, line));
                    }
                }
            }
            Stmt::Export(e) => match &**e {
                ExportKind::Fn { name, params, body } => {
                    self.declare(name);
                    self.function(name, params, body, false);
                }
                ExportKind::Let { name, init, ty, .. } => self.let_decl(name, init, ty, false)?,
                ExportKind::Struct(d) => {
                    self.declare(&d.name);
                    self.struct_decl(d)?;
                }
                ExportKind::Interface(d) => self.declare(&d.name),
                ExportKind::LetList { names, init, .. } => {
                    let r = self.expr(init);
                    for n in names.iter().filter(|n| *n != "_") {
                        self.declare(n);
                    }
                    r?;
                }
                ExportKind::Names { names, from: None } => {
                    for (orig, _) in names {
                        self.export_name(orig)?;
                    }
                }
                ExportKind::Names { from: Some(_), .. } | ExportKind::All { .. } => {}
                ExportKind::Default(e) => self.expr(e)?,
                ExportKind::DefaultDecl(s) => self.stmt(s)?,
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
                            self.push(body, HashSet::from([var.clone()]), false);
                            self.stmts(body);
                            self.pop();
                        }
                        // the other bodies run in the select's environment
                        SelectCase::Send { chan, value, body } => {
                            self.expr(chan)?;
                            self.expr(value)?;
                            self.stmts(body);
                        }
                    }
                }
                if let Some(d) = default {
                    self.stmts(d);
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

    /// The body of an `if`/`while`/`for`: its errors stay inside it.
    fn branch(&mut self, s: &Stmt) {
        if let Err(e) = self.stmt(s) {
            self.errors.push(e);
        }
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
            self.push(&[], binds, false);
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
        self.expr_ty(e).map(|_| ())
    }

    /// Resolves an expression and returns what is known about its type.
    fn expr_ty(&mut self, e: &Expr) -> Result<Ty, String> {
        Ok(match e {
            Expr::Int(_) => Ty::Int,
            Expr::Float(_) => Ty::Float,
            Expr::Str(_) => Ty::Str,
            Expr::Bool(_) => Ty::Bool,
            Expr::Nil => Ty::Nil,
            Expr::Duration(_) => Ty::Duration,
            Expr::Ident(n) => {
                self.use_name(n)?;
                self.type_of(n)
            }
            Expr::DynImport(spec) => {
                self.expr(spec)?;
                Ty::Any
            }
            Expr::List(v) => {
                v.iter().try_for_each(|x| self.expr(x))?;
                Ty::List
            }
            Expr::Template(v) => {
                v.iter().try_for_each(|x| self.expr(x))?;
                Ty::Str
            }
            Expr::Map(v) => {
                v.iter().try_for_each(|(_, x)| self.expr(x))?;
                Ty::Map
            }
            Expr::Unary { op, right } => {
                let t = self.expr_ty(right)?;
                match op {
                    UnaryOp::Not => Ty::Bool,
                    UnaryOp::Neg => match t {
                        Ty::Any | Ty::Int | Ty::Float | Ty::Num | Ty::Duration => t,
                        other => {
                            self.report(format!("operator '-' not applicable to '{}'", other.name()));
                            Ty::Any
                        }
                    },
                }
            }
            Expr::Binary { left, op, right } => {
                let l = self.expr_ty(left)?;
                let r = self.expr_ty(right)?;
                match binop(l, *op, r) {
                    Ok(t) => t,
                    Err(msg) => {
                        self.report(msg);
                        Ty::Any
                    }
                }
            }
            Expr::Logical { left, op, right } => {
                let l = self.expr_ty(left)?;
                let r = self.expr_ty(right)?;
                match op {
                    LogicalOp::Coalesce if l == Ty::Nil => r,
                    LogicalOp::Coalesce if l != Ty::Any => l,
                    _ if l == r => l,
                    _ => Ty::Any,
                }
            }
            Expr::Range { start, end } => {
                self.expr(start)?;
                self.expr(end)?;
                Ty::Range
            }
            Expr::Index { target, index } => {
                self.expr(target)?;
                self.expr(index)?;
                Ty::Any
            }
            Expr::Access { target, .. } | Expr::OptAccess { target, .. } => {
                self.expr(target)?;
                Ty::Any
            }
            Expr::OptChain(inner) => {
                self.expr(inner)?;
                Ty::Any
            }
            Expr::Call { callee, args } => self.call(callee, args)?,
            Expr::Assign { target, value, .. } => {
                self.expr(value)?;
                self.expr(target)?;
                Ty::Any
            }
            Expr::New { name, args } => {
                self.use_name(name)?;
                args.iter().try_for_each(|x| self.expr(x))?;
                Ty::Any
            }
            Expr::Fn { params, body } => {
                self.function("<anonymous>", params, body, false);
                Ty::Fn
            }
            Expr::Fg { call } => {
                self.expr(call)?;
                Ty::Nil
            }
            Expr::Chan { buffer } => {
                buffer.iter().try_for_each(|b| self.expr(b))?;
                Ty::Chan
            }
            Expr::Recv { chan } => {
                self.expr(chan)?;
                Ty::Any
            }
            Expr::Match { subject, arms } => {
                self.expr(subject)?;
                self.arms(arms)?;
                Ty::Any
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
                self.expr(expr)?;
                Ty::Any
            }
        })
    }

    /// A call: the arguments of a known function are checked against its
    /// signature (the same errors the interpreter raises when calling it).
    fn call(&mut self, callee: &Expr, args: &[Expr]) -> Result<Ty, String> {
        self.expr(callee)?;
        let mut arg_tys = Vec::with_capacity(args.len());
        for a in args {
            arg_tys.push(self.expr_ty(a)?);
        }
        let Expr::Ident(name) = callee else {
            return Ok(Ty::Any);
        };
        let n = args.len();
        match self.binding(name) {
            Some(s) => {
                let Some(sig) = s.fns.get(name.as_str()).cloned() else {
                    return Ok(Ty::Any);
                };
                // `check_arity`: missing arguments are nil (or their default)
                if !sig.rest && n > sig.params.len() {
                    self.report(format!(
                        "function '{}' expects at most {} arguments, got {}",
                        sig.name,
                        sig.params.len(),
                        n
                    ));
                }
                for (p, t) in sig.params.iter().zip(&arg_tys) {
                    let Some(want) = sig.types.get(p) else { continue };
                    if *t == Ty::Nil && sig.defaults.contains(p) {
                        continue;
                    }
                    if t.matches(want) == Some(false) {
                        self.report(format!(
                            "type mismatch: parameter '{}' expects {}, got {}",
                            p,
                            want,
                            t.name()
                        ));
                    }
                }
                Ok(sig.ret.as_ref().map(Ty::of_annotation).unwrap_or(Ty::Any))
            }
            None if self.builtins.contains(name.as_str()) => {
                let Some(&(_, arity, variadic, ret)) = CORE_GLOBALS.iter().find(|g| g.0 == name.as_str()) else {
                    return Ok(Ty::Any);
                };
                if variadic && n < arity {
                    self.report(format!(
                        "native function '{}' expects at least {} arguments, got {}",
                        name, arity, n
                    ));
                } else if !variadic && n != arity {
                    self.report(format!("native function '{}' expects {} arguments, got {}", name, arity, n));
                }
                Ok(ret)
            }
            None => Ok(Ty::Any),
        }
    }
}

/// The value of an integer literal (`300`, `-1`).
fn int_literal(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => Some(*n),
        Expr::Unary { op: UnaryOp::Neg, right } => match &**right {
            Expr::Int(n) => n.checked_neg(),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::value::Value;

    #[test]
    fn core_globals_match_the_native_context() {
        let ctx = crate::native::NativeContext::new();
        let globals: HashMap<String, Value> = ctx.globals().into_iter().collect();
        for (name, arity, variadic, _) in CORE_GLOBALS {
            match globals.get(*name) {
                Some(Value::NativeFn(f)) => {
                    assert_eq!((f.arity, f.variadic), (*arity, *variadic), "global '{}'", name)
                }
                _ => panic!("'{}' is not a native global", name),
            }
        }
    }

    fn run(src: &str) -> Report {
        let tokens = crate::runtime::Lexer::new(src).tokenize().unwrap();
        let program = crate::runtime::Parser::new(tokens).parse_program().unwrap();
        let builtins: HashSet<String> = crate::native::NativeContext::new()
            .globals()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        analyze(&program, &builtins)
    }

    #[test]
    fn reports_operators_on_incompatible_literals() {
        let r = run("print(1 + \"a\")\nprint(\"a\" * 2)\nprint(-\"x\")\nprint(1 < \"b\")");
        assert_eq!(
            r.errors,
            vec![
                "line 1: type error: operator not applicable between 'int' and 'str'",
                "line 2: type error: operator not applicable between 'str' and 'int'",
                "line 3: type error: operator '-' not applicable to 'str'",
                "line 4: type error: cannot compare 'int' with 'str'",
            ]
        );
    }

    #[test]
    fn follows_variables_annotations_and_signatures() {
        let r = run(
            "const n: int = \"x\"\nconst s = \"a\"\nprint(s - 1)\n\
             fn f(a: int, b = 2) { return a + b }\nprint(f(1, nil), f(\"x\"), f(1, 2, 3))\n\
             fn g(): str { return 1 }\nprint(g() + 1, len(1, 2))",
        );
        assert_eq!(
            r.errors,
            vec![
                "line 1: type error: type mismatch: variable 'n' is declared as int, got str",
                "line 3: type error: operator not applicable between 'str' and 'int'",
                "line 5: type error: type mismatch: parameter 'a' expects int, got str",
                "line 5: type error: function 'f' expects at most 2 arguments, got 3",
                "line 6: type error: type mismatch: function 'g' must return str, got int",
                "line 7: type error: operator not applicable between 'str' and 'int'",
                "line 7: type error: native function 'len' expects 1 arguments, got 2",
            ]
        );
    }

    #[test]
    fn unknown_types_are_never_reported() {
        let r = run(
            "let x = 1\nx = \"a\"\nprint(x + \"b\")\n\
             fn h(v: float) { return v + 1 }\nconst y: int = h(1)\n\
             fn k(...r) { return r }\nprint(k(1, 2, 3), 1.5 * 2, 2 * 1s, \"a\" + `b`)\n\
             const z: float = 1\nprint(z + 1, len([1]) + 1)",
        );
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }

    #[test]
    fn warns_about_unreachable_code_shadowing_and_unused_imports() {
        let r = run(
            "import json from \"fogu:json\"\nfn f() {\n  const a = 1\n  if true {\n    const a = 2\n    print(a)\n  }\n  return a\n  print(\"never\")\n}\nf()",
        );
        assert_eq!(
            r.warnings,
            vec![
                "line 5: variable 'a' shadows the one declared on line 3",
                "line 9: unreachable code after 'return'",
                "line 1: unused import 'json'",
            ]
        );
    }

    #[test]
    fn collects_every_error() {
        let r = run("print(a)\nprint(b)\nconst c = d\nprint(c)");
        assert_eq!(r.errors.len(), 3, "{:?}", r.errors);
        assert!(r.errors[2].contains("'d' is not defined"));
    }
}
