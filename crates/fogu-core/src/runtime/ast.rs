use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Shared function body (cloning is O(1); the address is stable and serves
/// as the function's identity for the JIT).
pub type Block = Rc<Vec<Stmt>>;

static LOOP_IDS: AtomicU32 = AtomicU32::new(1);

/// Unique (per process) identifier of a loop. Used by the JIT as a cache
/// key: clones of the same AST share the id.
pub fn next_loop_id() -> u32 {
    LOOP_IDS.fetch_add(1, Ordering::Relaxed)
}

/// Prefix of the last parameter of a variadic function (`...args`).
/// The name stored in `params` includes the prefix.
pub const REST_PREFIX: &str = "...";

/// Name of the variadic parameter, if the parameter list ends in `...name`.
pub fn rest_param(params: &[String]) -> Option<&str> {
    params.last().and_then(|p| p.strip_prefix(REST_PREFIX))
}

/// A type annotation (spec §9, §27). Generic parameters are erased to `Any`.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// `any` or a generic type parameter.
    Any,
    /// `int`, `str`, `error`, a struct or an interface name…
    Named(String),
    /// `list<int>`, `map<str, int>`, `chan<T>` (only the outer kind is checked).
    Generic(String, Vec<TypeExpr>),
    /// `A | B`.
    Union(Vec<TypeExpr>),
    /// `T?` (same as `T | nil`).
    Optional(Box<TypeExpr>),
    /// `(int, error)` — multiple return values.
    Tuple(Vec<TypeExpr>),
}

impl TypeExpr {
    /// Does the type mention `error` (so `?` may be used in the function)?
    pub fn has_error(&self) -> bool {
        match self {
            TypeExpr::Any => true,
            TypeExpr::Named(n) => n == "error",
            TypeExpr::Generic(..) => false,
            TypeExpr::Union(v) | TypeExpr::Tuple(v) => v.iter().any(TypeExpr::has_error),
            TypeExpr::Optional(t) => t.has_error(),
        }
    }
}

impl std::fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn list(v: &[TypeExpr], sep: &str) -> String {
            v.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(sep)
        }
        match self {
            TypeExpr::Any => write!(f, "any"),
            TypeExpr::Named(n) => write!(f, "{}", n),
            TypeExpr::Generic(n, a) => write!(f, "{}<{}>", n, list(a, ", ")),
            TypeExpr::Union(v) => write!(f, "{}", list(v, " | ")),
            TypeExpr::Optional(t) => write!(f, "{}?", t),
            TypeExpr::Tuple(v) => write!(f, "({})", list(v, ", ")),
        }
    }
}

/// `interface Name { method(a, b) ... }` — structural (spec §25).
#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    pub name: String,
    /// Required methods: (name, number of parameters).
    pub methods: Vec<(String, usize)>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(crate::runtime::fstr::Str),
    Bool(bool),
    Nil,
    Ident(Name),

    List(Vec<Expr>),
    Map(Vec<(String, Expr)>),

    Unary {
        op: UnaryOp,
        right: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Logical {
        left: Box<Expr>,
        op: LogicalOp,
        right: Box<Expr>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    Access {
        target: Box<Expr>,
        field: String,
    },
    /// `target?.field` — if the target is `nil`, the whole chain is `nil` (spec §15).
    /// Only appears inside an [`Expr::OptChain`].
    OptAccess {
        target: Box<Expr>,
        field: String,
    },
    /// Chain of accesses/calls containing `?.`: a `?.` on `nil` stops the
    /// whole chain and the result is `nil`.
    OptChain(Box<Expr>),
    /// Template string: the parts are concatenated as text (spec §7.4).
    Template(Vec<Expr>),
    /// Duration literal, in nanoseconds (spec §7.7).
    Duration(i64),
    /// `import(spec)`: loads a module at run time → the module (as `import x
    /// from spec` binds it), or an `error` value when it cannot be loaded.
    DynImport(Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `target = value` or `target op= value`.
    /// `target` is always `Ident`, `Access` or `Index`.
    Assign {
        target: Box<Expr>,
        op: Option<BinaryOp>,
        value: Box<Expr>,
    },
    New {
        name: String,
        args: Vec<Expr>,
    },
    Fn {
        params: Vec<String>,
        body: Block,
    },
    Fg {
        call: Box<Expr>,
    },
    Chan {
        buffer: Option<Box<Expr>>,
    },
    Recv {
        chan: Box<Expr>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    /// `expr?` — error propagation (spec §30).
    Try {
        expr: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
    /// `a ?? b` — returns `b` when `a` is `nil`.
    Coalesce,
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    Named { alias: String },
    Members { members: Vec<(String, String)> },
    Namespace { alias: String },
}

#[derive(Debug, Clone)]
pub enum ExportKind {
    Fn {
        name: String,
        params: Vec<String>,
        body: Block,
    },
    Let {
        name: String,
        mutable: bool,
        init: Expr,
        ty: Option<TypeExpr>,
    },
    Struct(StructDecl),
    Interface(InterfaceDecl),
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub embeds: Vec<String>,
    pub fields: Vec<StructField>,
    pub methods: Vec<(String, Vec<String>, Block)>,
    pub constructor: Option<(Vec<String>, Block)>,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Literal(Expr),
    Ident(String),
    IdentBind {
        name: String,
        guard: Option<Expr>,
    },
    Or(Vec<Pattern>),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub enum SelectCase {
    Recv {
        var: String,
        chan: Expr,
        body: Vec<Stmt>,
    },
    Send {
        chan: Expr,
        value: Expr,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// Source line marker (for error messages).
    Line(u32),
    /// `const [a, b] = expr` — destructuring of lists/tuples.
    LetList {
        names: Vec<String>,
        mutable: bool,
        init: Expr,
    },
    Let {
        name: String,
        mutable: bool,
        init: Expr,
        /// `let x: int = ...` — checked after evaluating `init`.
        ty: Option<TypeExpr>,
    },
    /// Checks that a parameter matches its annotation (function prologue).
    CheckType {
        name: String,
        ty: TypeExpr,
    },
    /// Declared return type; always the first statement of a function body.
    ReturnType(TypeExpr),
    Interface(InterfaceDecl),
    Expr(Expr),
    Block(Vec<Stmt>),
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        id: u32,
        cond: Expr,
        body: Box<Stmt>,
    },
    For {
        id: u32,
        var: String,
        /// Second variable of `for k, v in map` / `for i, x in list`.
        var2: Option<String>,
        iter: Expr,
        body: Box<Stmt>,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    FnDecl {
        name: String,
        params: Vec<String>,
        body: Block,
    },
    Import {
        kind: ImportKind,
        path: String,
    },
    Export(Box<ExportKind>),
    Struct(Box<StructDecl>),
    Select {
        cases: Vec<SelectCase>,
        default: Option<Vec<Stmt>>,
    },
    MatchStmt {
        subject: Expr,
        arms: Vec<MatchArm>,
    },
    SendStmt {
        chan: Expr,
        value: Expr,
    },
}

/// A variable name with its interned symbol, computed once by the parser: the
/// interpreter looks variables up by `sym` (an integer), never by text.
/// It reads as the name's `String` (`Deref`).
#[derive(Debug, Clone)]
pub struct Name {
    pub text: String,
    pub sym: crate::runtime::symbol::Sym,
}

impl Name {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let sym = crate::runtime::symbol::intern(&text);
        Name { text, sym }
    }
}

impl std::ops::Deref for Name {
    type Target = String;
    fn deref(&self) -> &String {
        &self.text
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Self) -> bool {
        self.sym == other.sym
    }
}

impl PartialEq<String> for Name {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
