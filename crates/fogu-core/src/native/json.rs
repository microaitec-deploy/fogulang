use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    exports.insert(
        "stringify".into(),
        nfn("stringify", 1, |args| Ok(Value::Str(stringify(&args[0]).into()))),
    );

    exports.insert(
        "parse".into(),
        nfn("parse", 1, |args| match &args[0] {
            Value::Str(s) => parse(s).map_err(|e| io_err!("json.parse: {}", e)),
            other => Err(format!("'parse' expects str, got '{}'", other.type_name())),
        }),
    );

    ModuleValue {
        name: "fogu:json".into(),
        exports: RefCell::new(exports),
    }
}

// ---------- Public API for other native modules ----------

/// Serializes a `Value` to JSON. Never fails — unsupported types fall back
/// to their string representation.
pub fn stringify_value(v: &Value) -> String {
    stringify(v)
}

/// Parses JSON. Accepts any `&str`.
pub fn parse_value(s: &str) -> Result<Value, String> {
    parse(s)
}

// ---------- Internals ----------

fn stringify(v: &Value) -> String {
    let mut out = String::with_capacity(64);
    write_value(v, &mut out);
    out
}

/// Writes `v` as JSON into one buffer (map keys sorted: deterministic output).
fn write_value(v: &Value, out: &mut String) {
    use std::fmt::Write;
    match v {
        Value::Nil => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => {
            let _ = write!(out, "{}", n);
        }
        // JSON has no NaN or infinity
        Value::Float(n) if !n.is_finite() => out.push_str("null"),
        Value::Float(n) => {
            let _ = write!(out, "{}", n);
        }
        Value::Str(s) => write_str(s, out),
        Value::List(l) => {
            out.push('[');
            for (i, item) in l.borrow().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Buffer(b) => {
            out.push('[');
            for (i, x) in b.borrow().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{}", x);
            }
            out.push(']');
        }
        Value::Map(m) => {
            let b = m.borrow();
            let mut entries: Vec<(&String, &Value)> = b.iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_value(v, out);
            }
            out.push('}');
        }
        other => {
            out.push_str("\"<");
            out.push_str(other.type_name());
            out.push_str(">\"");
        }
    }
}

/// A JSON string literal: `"`, `\` and control characters are escaped.
fn write_str(s: &str, out: &mut String) {
    use std::fmt::Write;
    out.push('"');
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let esc = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x08 => "\\b",
            0x0c => "\\f",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&s[start..i]);
        if esc.is_empty() {
            let _ = write!(out, "\\u{:04x}", b);
        } else {
            out.push_str(esc);
        }
        start = i + 1;
    }
    out.push_str(&s[start..]);
    out.push('"');
}

fn parse(s: &str) -> Result<Value, String> {
    let mut p = JsonParser { s, b: s.as_bytes(), pos: 0 };
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.b.len() {
        return Err(format!("extra content after JSON at position {}", p.char_pos()));
    }
    Ok(v)
}

/// A JSON parser over the bytes of the text (no copy of it).
struct JsonParser<'a> {
    s: &'a str,
    b: &'a [u8],
    pos: usize,
}

impl JsonParser<'_> {
    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.b.get(self.pos) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    /// The character at the current position (error messages).
    fn peek_char(&self) -> Option<char> {
        self.s.get(self.pos..).and_then(|r| r.chars().next())
    }

    /// The position counted in characters (error messages).
    fn char_pos(&self) -> usize {
        self.s.char_indices().take_while(|(i, _)| *i < self.pos).count()
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => self.expect_lit("null").map(|_| Value::Nil),
            Some(b't') => self.expect_lit("true").map(|_| Value::Bool(true)),
            Some(b'f') => self.expect_lit("false").map(|_| Value::Bool(false)),
            Some(b'"') => self.parse_string().map(Value::str),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            _ => Err(format!("invalid JSON at position {}: {:?}", self.char_pos(), self.peek_char())),
        }
    }

    fn expect_lit(&mut self, lit: &str) -> Result<(), String> {
        if self.b[self.pos..].starts_with(lit.as_bytes()) {
            self.pos += lit.len();
            return Ok(());
        }
        Err(format!("expected '{}' at position {}", lit, self.char_pos()))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let hex = self.s.get(self.pos..self.pos + 4).unwrap_or("");
        let code = u32::from_str_radix(hex, 16).map_err(|_| format!("invalid \\u: {}", hex))?;
        self.pos += 4;
        Ok(code)
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let start = self.pos;
        // fast path: no escapes
        while let Some(&c) = self.b.get(self.pos) {
            match c {
                b'"' => {
                    let out = self.s[start..self.pos].to_string();
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => break,
                _ => self.pos += 1,
            }
        }
        let mut out = String::from(&self.s[start..self.pos]);
        let mut run = self.pos;
        while let Some(&c) = self.b.get(self.pos) {
            match c {
                b'"' => {
                    out.push_str(&self.s[run..self.pos]);
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(&self.s[run..self.pos]);
                    self.pos += 1;
                    let esc = self.peek().ok_or("truncated escape")?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            // a surrogate pair: 😀
                            if (0xD800..0xDC00).contains(&code) && self.b[self.pos..].starts_with(b"\\u") {
                                let save = self.pos;
                                self.pos += 2;
                                let low = self.hex4()?;
                                if (0xDC00..0xE000).contains(&low) {
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                } else {
                                    self.pos = save;
                                }
                            }
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        }
                        other => return Err(format!("invalid escape: \\{}", other as char)),
                    }
                    run = self.pos;
                }
                _ => self.pos += 1,
            }
        }
        Err("unterminated JSON string".into())
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits = |p: &mut Self| {
            while matches!(p.peek(), Some(c) if c.is_ascii_digit()) {
                p.pos += 1;
            }
        };
        digits(self);
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            digits(self);
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            digits(self);
        }
        let text = &self.s[start..self.pos];
        if is_float {
            text.parse::<f64>().map(Value::Float).map_err(|e| format!("invalid number: {}", e))
        } else {
            text.parse::<i64>().map(Value::Int).map_err(|e| format!("invalid number: {}", e))
        }
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::List(Rc::new(RefCell::new(items))));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(format!("expected ',' or ']', found {:?}", self.peek_char())),
            }
        }
        Ok(Value::List(Rc::new(RefCell::new(items))))
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        self.pos += 1;
        let mut map = HashMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Map(Rc::new(RefCell::new(map))));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(format!("invalid JSON at position {}: {:?}", self.char_pos(), self.peek_char()));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(format!("expected ':' at position {}", self.char_pos()));
            }
            self.pos += 1;
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(format!("expected ',' or '}}', found {:?}", self.peek_char())),
            }
        }
        Ok(Value::Map(Rc::new(RefCell::new(map))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_escapes() {
        let v = parse(r#"{"b":[1,2.5,true,null],"a":"x\"y\\z\né😀","c":{}}"#).unwrap();
        assert_eq!(stringify(&v), "{\"a\":\"x\\\"y\\\\z\\n\u{e9}\u{1f600}\",\"b\":[1,2.5,true,null],\"c\":{}}");
        assert_eq!(stringify(&Value::Str("\u{1b}[0m\u{0}".into())), "\"\\u001b[0m\\u0000\"");
        assert_eq!(stringify(&Value::Float(f64::INFINITY)), "null");
        assert!(parse("[1,").is_err());
        assert_eq!(parse("\"abc").unwrap_err(), "unterminated JSON string");
        assert_eq!(parse("{\"é\": x}").unwrap_err(), "invalid JSON at position 6: Some('x')");
        assert_eq!(parse("  [1] x").unwrap_err(), "extra content after JSON at position 6");
    }
}
