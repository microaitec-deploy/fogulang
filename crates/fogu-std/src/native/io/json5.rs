//! `fogu:json5` — JSON5 (<https://json5.org>), JSON for humans: comments,
//! trailing commas, unquoted keys, single-quoted and multi-line strings, hex
//! numbers, `.5` / `5.` / `+1`, `Infinity` and `NaN`.
//!
//! ```fogu
//! import json5 from "fogu:json5"
//!
//! const cfg = json5.parse("{ name: 'app', port: 0x1F90, tags: ['a', 'b',], /* ok */ }")?
//! print(cfg.port)                     // 8080
//! print(json5.stringify(cfg, 2))      // keys unquoted when they are identifiers
//! ```
//!
//! Malformed input is an `error` value whose message carries the
//! `line:column` of the problem. Map keys are written in sorted order (as
//! `json.stringify` does): the output is deterministic.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::util::{max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("parse".into(), nfn_var("parse", 1, |a| {
        max_args(a, 1, "json5.parse")?;
        parse(&str_arg(a, 0, "json5.parse")?).map_err(|e| io_err!("json5.parse: {}", e))
    }));
    e.insert("stringify".into(), nfn_var("stringify", 1, |a| {
        max_args(a, 2, "json5.stringify")?;
        let indent = indent_arg(opt_arg(a, 1), "json5.stringify")?;
        Ok(Value::str(stringify(&a[0], &indent)))
    }));
    e.insert("read".into(), nfn_var("read", 1, |a| {
        max_args(a, 1, "json5.read")?;
        let path = str_arg(a, 0, "json5.read")?;
        let text = std::fs::read_to_string(&path).map_err(|e| io_err!("json5.read: {}: {}", path, e))?;
        parse(&text).map_err(|e| io_err!("json5.read: {}: {}", path, e))
    }));
    // write(path, value, indent = 2): the file ends with a newline
    e.insert("write".into(), nfn_var("write", 2, |a| {
        max_args(a, 3, "json5.write")?;
        let path = str_arg(a, 0, "json5.write")?;
        let indent = match opt_arg(a, 2) {
            None => "  ".to_string(),
            some => indent_arg(some, "json5.write")?,
        };
        let mut text = stringify(&a[1], &indent);
        text.push('\n');
        if let Some(dir) = std::path::Path::new(&path).parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| io_err!("json5.write: {}: {}", path, e))?;
        }
        std::fs::write(&path, text).map_err(|e| io_err!("json5.write: {}: {}", path, e))?;
        Ok(Value::Nil)
    }));
    ModuleValue {
        name: "fogu:json5".into(),
        exports: RefCell::new(e),
    }
}

/// The indentation argument: a number of spaces (0–10) or a string (its
/// first 10 characters), like `JSON.stringify`.
fn indent_arg(v: Option<&Value>, what: &str) -> Result<String, String> {
    match v {
        None => Ok(String::new()),
        Some(Value::Int(n)) => Ok(" ".repeat((*n).clamp(0, 10) as usize)),
        Some(Value::Str(s)) => Ok(s.chars().take(10).collect()),
        Some(other) => Err(format!("{}: indent must be int or str, got '{}'", what, other.type_name())),
    }
}

// =====================================================================
// stringify
// =====================================================================

/// Serializes a `Value` as JSON5 (`indent` empty → compact).
pub fn stringify(v: &Value, indent: &str) -> String {
    let mut out = String::with_capacity(64);
    write_value(v, indent, 0, &mut out);
    out
}

fn newline(indent: &str, depth: usize, out: &mut String) {
    if !indent.is_empty() {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(indent);
        }
    }
}

fn write_value(v: &Value, indent: &str, depth: usize, out: &mut String) {
    use std::fmt::Write;
    match v {
        Value::Nil => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => {
            let _ = write!(out, "{}", n);
        }
        Value::Float(n) if n.is_nan() => out.push_str("NaN"),
        Value::Float(n) if n.is_infinite() => out.push_str(if *n > 0.0 { "Infinity" } else { "-Infinity" }),
        // 5.0 stays a float when read back
        Value::Float(n) if n.fract() == 0.0 => {
            let _ = write!(out, "{:.1}", n);
        }
        Value::Float(n) => {
            let _ = write!(out, "{}", n);
        }
        Value::Str(s) => write_str(s, out),
        Value::List(l) => {
            let items = l.borrow();
            write_seq(items.len(), '[', ']', indent, depth, out, |i, out| write_value(&items[i], indent, depth + 1, out));
        }
        Value::Buffer(b) => {
            let bytes = b.borrow();
            write_seq(bytes.len(), '[', ']', indent, depth, out, |i, out| {
                let _ = write!(out, "{}", bytes[i]);
            });
        }
        Value::Map(m) => {
            let b = m.borrow();
            let mut entries: Vec<(&String, &Value)> = b.iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
            write_seq(entries.len(), '{', '}', indent, depth, out, |i, out| {
                let (k, v) = entries[i];
                if is_identifier(k) {
                    out.push_str(k);
                } else {
                    write_str(k, out);
                }
                out.push(':');
                if !indent.is_empty() {
                    out.push(' ');
                }
                write_value(v, indent, depth + 1, out);
            });
        }
        other => write_str(&format!("<{}>", other.type_name()), out),
    }
}

fn write_seq(
    len: usize,
    open: char,
    close: char,
    indent: &str,
    depth: usize,
    out: &mut String,
    mut item: impl FnMut(usize, &mut String),
) {
    out.push(open);
    if len == 0 {
        out.push(close);
        return;
    }
    for i in 0..len {
        if i > 0 {
            out.push(',');
        }
        newline(indent, depth + 1, out);
        item(i, out);
    }
    newline(indent, depth, out);
    out.push(close);
}

/// An ECMAScript identifier name (written without quotes as a key).
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_ident_start(c) => chars.all(is_ident_part),
        _ => false,
    }
}

fn is_ident_start(c: char) -> bool {
    c == '$' || c == '_' || c.is_alphabetic()
}

fn is_ident_part(c: char) -> bool {
    is_ident_start(c) || c.is_alphanumeric() || c == '\u{200C}' || c == '\u{200D}'
}

/// A string literal: double quotes, or single quotes when that needs fewer escapes.
fn write_str(s: &str, out: &mut String) {
    use std::fmt::Write;
    let doubles = s.matches('"').count();
    let singles = s.matches('\'').count();
    let quote = if doubles > singles { '\'' } else { '"' };
    out.push(quote);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\u{0B}' => out.push_str("\\v"),
            '\0' if !matches!(chars.peek(), Some(d) if d.is_ascii_digit()) => out.push_str("\\0"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
}

// =====================================================================
// parse
// =====================================================================

/// Parses JSON5 text for other native code (e.g. importing `.json5` files).
/// The error is a plain message starting with `line L:C:` (not an error value).
pub fn parse_value(text: &str) -> Result<Value, String> {
    parse(text)
}

/// Parses JSON5 text. The error message starts with `line L:C:`.
fn parse(s: &str) -> Result<Value, String> {
    let mut p = Parser { s, pos: 0 };
    p.skip()?;
    let v = p.value(0)?;
    p.skip()?;
    match p.peek() {
        None => Ok(v),
        Some(c) => Err(p.err(format!("unexpected character {:?} after the value", c))),
    }
}

const MAX_DEPTH: usize = 512;

struct Parser<'a> {
    s: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s[self.pos..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        let mut it = self.s[self.pos..].chars();
        it.next();
        it.next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// An error at the current position.
    fn err(&self, msg: impl std::fmt::Display) -> String {
        self.err_at(self.pos, msg)
    }

    fn err_at(&self, pos: usize, msg: impl std::fmt::Display) -> String {
        let before = &self.s[..pos];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!("line {}:{}: {}", line, col, msg)
    }

    fn unexpected(&self) -> String {
        match self.peek() {
            None => self.err("unexpected end of input"),
            Some(c) => self.err(format!("unexpected character {:?}", c)),
        }
    }

    /// Skips whitespace and comments.
    fn skip(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() || c == '\u{FEFF}' => {
                    self.bump();
                }
                Some('/') if self.peek2() == Some('/') => {
                    while let Some(c) = self.bump() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                Some('/') if self.peek2() == Some('*') => {
                    let start = self.pos;
                    match self.s[self.pos + 2..].find("*/") {
                        Some(i) => self.pos += 2 + i + 2,
                        None => return Err(self.err_at(start, "unterminated comment")),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        match self.peek() {
            Some('{') => self.object(depth),
            Some('[') => self.array(depth),
            Some(q @ ('"' | '\'')) => self.string(q).map(Value::str),
            Some(c) if c.is_ascii_digit() || matches!(c, '.' | '+' | '-') => self.number(),
            Some(c) if is_ident_start(c) => {
                let start = self.pos;
                match self.word().as_str() {
                    "null" => Ok(Value::Nil),
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    "Infinity" => Ok(Value::Float(f64::INFINITY)),
                    "NaN" => Ok(Value::Float(f64::NAN)),
                    w => Err(self.err_at(start, format!("unexpected identifier '{}'", w))),
                }
            }
            _ => Err(self.unexpected()),
        }
    }

    fn word(&mut self) -> String {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if is_ident_part(c)) {
            self.bump();
        }
        self.s[start..self.pos].to_string()
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        let neg = match self.peek() {
            Some('-') => {
                self.bump();
                true
            }
            Some('+') => {
                self.bump();
                false
            }
            _ => false,
        };
        let body = self.pos;
        let v = match self.peek() {
            Some(c) if is_ident_start(c) => match self.word().as_str() {
                "Infinity" => Value::Float(if neg { f64::NEG_INFINITY } else { f64::INFINITY }),
                "NaN" => Value::Float(f64::NAN),
                _ => return Err(self.err_at(start, "invalid number")),
            },
            Some('0') if matches!(self.peek2(), Some('x' | 'X')) => {
                self.pos += 2;
                let digits = self.pos;
                while matches!(self.peek(), Some(c) if c.is_ascii_hexdigit()) {
                    self.bump();
                }
                let hex = &self.s[digits..self.pos];
                if hex.is_empty() {
                    return Err(self.err_at(start, "invalid hexadecimal number"));
                }
                let n = i64::from_str_radix(hex, 16).map_err(|_| self.err_at(start, "hexadecimal number too large"))?;
                Value::Int(if neg { -n } else { n })
            }
            _ => {
                let digits = |p: &mut Self| {
                    let from = p.pos;
                    while matches!(p.peek(), Some(c) if c.is_ascii_digit()) {
                        p.bump();
                    }
                    p.pos - from
                };
                let int_digits = digits(self);
                if int_digits > 1 && self.s[body..].starts_with('0') {
                    return Err(self.err_at(start, "leading zeros are not allowed"));
                }
                let mut is_float = false;
                let mut frac_digits = 0;
                if self.peek() == Some('.') {
                    is_float = true;
                    self.bump();
                    frac_digits = digits(self);
                }
                if int_digits + frac_digits == 0 {
                    return Err(self.err_at(start, "invalid number"));
                }
                if matches!(self.peek(), Some('e' | 'E')) {
                    is_float = true;
                    self.bump();
                    if matches!(self.peek(), Some('+' | '-')) {
                        self.bump();
                    }
                    if digits(self) == 0 {
                        return Err(self.err_at(start, "invalid number"));
                    }
                }
                let text = &self.s[body..self.pos];
                let int = if is_float { None } else { text.parse::<i64>().ok() };
                match int {
                    Some(n) => Value::Int(if neg { -n } else { n }),
                    None => {
                        let f: f64 = text.parse().map_err(|_| self.err_at(start, "invalid number"))?;
                        Value::Float(if neg { -f } else { f })
                    }
                }
            }
        };
        if matches!(self.peek(), Some(c) if is_ident_part(c)) {
            return Err(self.unexpected());
        }
        Ok(v)
    }

    fn hex(&mut self, n: usize, what: &str) -> Result<u32, String> {
        let start = self.pos;
        let mut code = 0u32;
        for _ in 0..n {
            match self.peek().and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    code = code * 16 + d;
                    self.bump();
                }
                None => return Err(self.err_at(start, format!("invalid {} escape", what))),
            }
        }
        Ok(code)
    }

    fn string(&mut self, quote: char) -> Result<String, String> {
        let start = self.pos;
        self.bump();
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(self.err_at(start, "unterminated string"));
            };
            match c {
                c if c == quote => return Ok(out),
                '\n' | '\r' => {
                    return Err(self.err_at(self.pos - 1, "line break in a string (escape it, or end the line with \\)"));
                }
                '\\' => {
                    let esc_pos = self.pos - 1;
                    let Some(e) = self.bump() else {
                        return Err(self.err_at(start, "unterminated string"));
                    };
                    match e {
                        'b' => out.push('\u{08}'),
                        'f' => out.push('\u{0C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'v' => out.push('\u{0B}'),
                        '0' if !matches!(self.peek(), Some(d) if d.is_ascii_digit()) => out.push('\0'),
                        '0'..='9' => return Err(self.err_at(esc_pos, format!("invalid escape '\\{}'", e))),
                        'x' => {
                            let code = self.hex(2, "\\x")?;
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        'u' => {
                            let mut code = self.hex(4, "\\u")?;
                            // a surrogate pair: \uD83D\uDE00
                            if (0xD800..0xDC00).contains(&code) && self.s[self.pos..].starts_with("\\u") {
                                let save = self.pos;
                                self.pos += 2;
                                match self.hex(4, "\\u") {
                                    Ok(low) if (0xDC00..0xE000).contains(&low) => {
                                        code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                    }
                                    _ => self.pos = save,
                                }
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        // line continuation: the backslash and the line break vanish
                        '\n' | '\u{2028}' | '\u{2029}' => {}
                        '\r' => {
                            if self.peek() == Some('\n') {
                                self.bump();
                            }
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        self.bump();
        let mut items = Vec::new();
        loop {
            self.skip()?;
            if self.peek() == Some(']') {
                self.bump();
                break;
            }
            items.push(self.value(depth + 1)?);
            self.skip()?;
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    break;
                }
                None => return Err(self.unexpected()),
                Some(c) => return Err(self.err(format!("expected ',' or ']', found {:?}", c))),
            }
        }
        Ok(Value::List(Rc::new(RefCell::new(items))))
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        self.bump();
        let mut map = HashMap::new();
        loop {
            self.skip()?;
            let key = match self.peek() {
                Some('}') => {
                    self.bump();
                    break;
                }
                Some(q @ ('"' | '\'')) => self.string(q)?,
                Some(c) if is_ident_start(c) => self.word(),
                None => return Err(self.unexpected()),
                Some(c) => return Err(self.err(format!("expected a key, found {:?}", c))),
            };
            self.skip()?;
            match self.peek() {
                Some(':') => {
                    self.bump();
                }
                None => return Err(self.unexpected()),
                Some(c) => return Err(self.err(format!("expected ':' after the key, found {:?}", c))),
            }
            self.skip()?;
            let value = self.value(depth + 1)?;
            map.insert(key, value);
            self.skip()?;
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {
                    self.bump();
                    break;
                }
                None => return Err(self.unexpected()),
                Some(c) => return Err(self.err(format!("expected ',' or '}}', found {:?}", c))),
            }
        }
        Ok(Value::Map(Rc::new(RefCell::new(map))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json5_features() {
        let v = parse(
            "// head\n{ a: 1, 'b c': [0x1F, -0xa, .5, 5., +3, Infinity, -Infinity,], \"d\": 'it\\'s \\\n ok', $e_1: null, /* x */ }",
        )
        .unwrap();
        assert_eq!(
            stringify(&v, ""),
            "{$e_1:null,a:1,\"b c\":[31,-10,0.5,5.0,3,Infinity,-Infinity],d:\"it's  ok\"}"
        );
        assert!(matches!(parse("NaN").unwrap(), Value::Float(f) if f.is_nan()));
        assert_eq!(stringify(&parse("{a: [1, {}], b: []}").unwrap(), "  "), "{\n  a: [\n    1,\n    {}\n  ],\n  b: []\n}");
        assert_eq!(stringify(&Value::str("say \"hi\""), ""), "'say \"hi\"'");
    }

    #[test]
    fn json5_errors() {
        assert_eq!(parse("{\n  a: 1,\n  b: x\n}").unwrap_err(), "line 3:6: unexpected identifier 'x'");
        assert_eq!(parse("[1, 2").unwrap_err(), "line 1:6: unexpected end of input");
        assert_eq!(parse("'abc").unwrap_err(), "line 1:1: unterminated string");
        assert_eq!(parse("/* x").unwrap_err(), "line 1:1: unterminated comment");
        assert_eq!(parse("[1,,2]").unwrap_err(), "line 1:4: unexpected character ','");
        assert_eq!(parse("012").unwrap_err(), "line 1:1: leading zeros are not allowed");
        assert_eq!(parse("{a 1}").unwrap_err(), "line 1:4: expected ':' after the key, found '1'");
        assert_eq!(parse("1 2").unwrap_err(), "line 1:3: unexpected character '2' after the value");
    }
}
