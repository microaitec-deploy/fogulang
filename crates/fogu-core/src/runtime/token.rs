use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    /// Template string `` `a ${expr} b` `` (spec §7.4).
    Template(Vec<TplPart>),
    /// Duration literal (`500ms`, `2s`, …) already converted to nanoseconds.
    Duration(i64),
    True,
    False,
    Nil,

    // Identifiers
    Ident(String),
    Underscore,

    // Keywords
    Let,
    Const,
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Import,
    Export,
    From,
    As,
    Struct,
    Interface,
    Embed,
    Match,
    Case,
    Default,
    Fg,
    Chan,
    Select,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,
    Eq,
    EqEq,
    Bang,
    BangEq,
    Lt,
    LtEq,
    LeftArrow, // <-
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Pipe,             // |
    PipeArrow,        // ->
    FatArrow,         // =>
    Question,         // ?
    QuestionQuestion, // ??
    QuestionDot,      // ?.

    // Compound assignment
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Dot,
    DotDot,
    Ellipsis, // ...

    Eof,
}

/// Part of a template string: literal text or the tokens of `${...}`
/// (terminated by `Eof`).
#[derive(Debug, Clone, PartialEq)]
pub enum TplPart {
    Lit(String),
    Code(Vec<Spanned>),
}

/// Token with its position in the source code (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub tok: Token,
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Int(n) => write!(f, "{}", n),
            Token::Float(n) => write!(f, "{}", n),
            Token::Str(s) => write!(f, "\"{}\"", s),
            Token::Ident(s) => write!(f, "{}", s),
            other => write!(f, "{:?}", other),
        }
    }
}
