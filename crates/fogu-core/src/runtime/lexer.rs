use super::token::{Spanned, Token, TplPart};

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self::at(source.chars().collect(), 1, 1)
    }

    /// Lexer starting at a given position (code inside `${...}`).
    fn at(input: Vec<char>, line: u32, col: u32) -> Self {
        Self {
            input,
            pos: 0,
            line,
            col,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        if let Some(ch) = c {
            self.pos += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        c
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, String> {
        Err(format!(
            "line {}:{}: lexical error: {}",
            self.line,
            self.col,
            msg.into()
        ))
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.advance();
                }
                Some('/') if self.peek_next() == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                Some('/') if self.peek_next() == Some('*') => {
                    let (l, c0) = (self.line, self.col);
                    self.advance();
                    self.advance();
                    let mut closed = false;
                    while let Some(c) = self.advance() {
                        if c == '*' && self.peek() == Some('/') {
                            self.advance();
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        return Err(format!(
                            "line {}:{}: lexical error: unterminated block comment",
                            l, c0
                        ));
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    pub fn tokenize(&mut self) -> Result<Vec<Spanned>, String> {
        let mut tokens: Vec<Spanned> = Vec::new();

        loop {
            self.skip_whitespace_and_comments()?;
            let (line, col) = (self.line, self.col);
            let c = match self.peek() {
                Some(c) => c,
                None => {
                    tokens.push(Spanned { tok: Token::Eof, line, col });
                    break;
                }
            };

            let token = match c {
                '0'..='9' => self.read_number()?,
                '"' | '\'' => self.read_string(c)?,
                '`' => self.read_template()?,
                '_' => {
                    // `_` alone is the match wildcard; `_name` is an identifier.
                    if self
                        .peek_next()
                        .map_or(true, |n| !n.is_alphanumeric() && n != '_')
                    {
                        self.advance();
                        Token::Underscore
                    } else {
                        self.read_ident()
                    }
                }
                c if c.is_alphabetic() => self.read_ident(),

                '+' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::PlusEq
                    } else {
                        Token::Plus
                    }
                }
                '-' => {
                    self.advance();
                    match self.peek() {
                        Some('>') => {
                            self.advance();
                            Token::PipeArrow
                        }
                        Some('=') => {
                            self.advance();
                            Token::MinusEq
                        }
                        _ => Token::Minus,
                    }
                }
                '*' => {
                    self.advance();
                    match self.peek() {
                        Some('*') => {
                            self.advance();
                            Token::StarStar
                        }
                        Some('=') => {
                            self.advance();
                            Token::StarEq
                        }
                        _ => Token::Star,
                    }
                }
                '/' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::SlashEq
                    } else {
                        Token::Slash
                    }
                }
                '%' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::PercentEq
                    } else {
                        Token::Percent
                    }
                }
                '=' => {
                    self.advance();
                    match self.peek() {
                        Some('=') => {
                            self.advance();
                            Token::EqEq
                        }
                        Some('>') => {
                            self.advance();
                            Token::FatArrow
                        }
                        _ => Token::Eq,
                    }
                }
                '!' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::BangEq
                    } else {
                        Token::Bang
                    }
                }
                '<' => {
                    self.advance();
                    match self.peek() {
                        Some('=') => {
                            self.advance();
                            Token::LtEq
                        }
                        // `<-` is always a single token (channels).
                        // To compare with a negative number write `x < -1`.
                        Some('-') => {
                            self.advance();
                            Token::LeftArrow
                        }
                        _ => Token::Lt,
                    }
                }
                '>' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Token::GtEq
                    } else {
                        Token::Gt
                    }
                }
                '&' => {
                    self.advance();
                    if self.peek() == Some('&') {
                        self.advance();
                        Token::AndAnd
                    } else {
                        return self.err("expected '&' after '&'");
                    }
                }
                '|' => {
                    self.advance();
                    if self.peek() == Some('|') {
                        self.advance();
                        Token::OrOr
                    } else {
                        Token::Pipe
                    }
                }
                '(' => {
                    self.advance();
                    Token::LParen
                }
                ')' => {
                    self.advance();
                    Token::RParen
                }
                '{' => {
                    self.advance();
                    Token::LBrace
                }
                '}' => {
                    self.advance();
                    Token::RBrace
                }
                '[' => {
                    self.advance();
                    Token::LBracket
                }
                ']' => {
                    self.advance();
                    Token::RBracket
                }
                ',' => {
                    self.advance();
                    Token::Comma
                }
                ';' => {
                    self.advance();
                    Token::Semicolon
                }
                ':' => {
                    self.advance();
                    Token::Colon
                }
                '?' => {
                    self.advance();
                    if self.peek() == Some('?') {
                        self.advance();
                        Token::QuestionQuestion
                    } else if self.peek() == Some('.') && self.peek_next() != Some('.') {
                        self.advance();
                        Token::QuestionDot
                    } else {
                        Token::Question
                    }
                }
                '.' => {
                    self.advance();
                    if self.peek() == Some('.') {
                        self.advance();
                        if self.peek() == Some('.') {
                            self.advance();
                            Token::Ellipsis
                        } else {
                            Token::DotDot
                        }
                    } else {
                        Token::Dot
                    }
                }
                _ => return self.err(format!("unexpected character: '{}'", c)),
            };

            tokens.push(Spanned { tok: token, line, col });
        }

        Ok(tokens)
    }

    fn read_number(&mut self) -> Result<Token, String> {
        // 0x.. and 0b..
        if self.peek() == Some('0') {
            let radix = match self.peek_next() {
                Some('x') | Some('X') => Some(16),
                Some('b') | Some('B') => Some(2),
                _ => None,
            };
            if let Some(radix) = radix {
                self.advance();
                self.advance();
                let mut digits = String::new();
                while let Some(c) = self.peek() {
                    if c == '_' {
                        self.advance();
                    } else if c.is_digit(radix) {
                        digits.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                if digits.is_empty() {
                    return self.err("numeric literal without digits");
                }
                return match i64::from_str_radix(&digits, radix) {
                    Ok(n) => Ok(Token::Int(n)),
                    Err(_) => self.err("integer out of the 64-bit range"),
                };
            }
        }

        let mut s = String::new();
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                if c != '_' {
                    s.push(c);
                }
                self.advance();
            } else if c == '.'
                && !is_float
                && self.peek_next().map_or(false, |n| n.is_ascii_digit())
            {
                is_float = true;
                s.push('.');
                self.advance();
            } else {
                break;
            }
        }

        // exponent: 1e9, 2.5E-3
        if matches!(self.peek(), Some('e') | Some('E')) {
            let n1 = self.peek_next();
            let n2 = self.input.get(self.pos + 2).copied();
            let ok = match n1 {
                Some(d) if d.is_ascii_digit() => true,
                Some('+') | Some('-') => n2.map_or(false, |d| d.is_ascii_digit()),
                _ => false,
            };
            if ok {
                is_float = true;
                s.push('e');
                self.advance();
                if let Some(sign @ ('+' | '-')) = self.peek() {
                    s.push(sign);
                    self.advance();
                }
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() {
                        s.push(d);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        if let Some(mult) = self.duration_unit() {
            return self.make_duration(&s, is_float, mult);
        }

        if is_float {
            match s.parse::<f64>() {
                Ok(f) => Ok(Token::Float(f)),
                Err(e) => self.err(format!("invalid float: {}", e)),
            }
        } else {
            match s.parse::<i64>() {
                Ok(n) => Ok(Token::Int(n)),
                Err(_) => self.err(format!("integer '{}' out of the 64-bit range", s)),
            }
        }
    }

    fn read_string(&mut self, quote: char) -> Result<Token, String> {
        let (l, c0) = (self.line, self.col);
        self.advance();
        let mut s = String::new();

        while let Some(c) = self.advance() {
            if c == quote {
                return Ok(Token::Str(s));
            }
            if c == '\\' {
                match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('0') => s.push('\0'),
                    Some('\\') => s.push('\\'),
                    Some('\'') => s.push('\''),
                    Some('"') => s.push('"'),
                    Some(other) => return self.err(format!("invalid escape: \\{}", other)),
                    None => break,
                }
            } else {
                s.push(c);
            }
        }

        Err(format!(
            "line {}:{}: lexical error: unterminated string",
            l, c0
        ))
    }

    /// Duration suffix right after a number (`ns us ms s m h d`).
    /// If present, consumes it and returns the nanoseconds per unit.
    fn duration_unit(&mut self) -> Option<i64> {
        let mut word = String::new();
        let mut i = self.pos;
        while let Some(&c) = self.input.get(i) {
            if c.is_alphanumeric() || c == '_' {
                word.push(c);
                i += 1;
            } else {
                break;
            }
        }
        let mult: i64 = match word.as_str() {
            "ns" => 1,
            "us" | "µs" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60 * 1_000_000_000,
            "h" => 3_600 * 1_000_000_000,
            "d" => 86_400 * 1_000_000_000,
            _ => return None,
        };
        for _ in 0..word.chars().count() {
            self.advance();
        }
        Some(mult)
    }

    fn make_duration(&self, digits: &str, is_float: bool, mult: i64) -> Result<Token, String> {
        let ns = if is_float {
            let f: f64 = match digits.parse() {
                Ok(f) => f,
                Err(e) => return self.err(format!("invalid duration: {}", e)),
            };
            let v = (f * mult as f64).round();
            if !v.is_finite() || v.abs() >= i64::MAX as f64 {
                return self.err("duration out of the 64-bit range");
            }
            v as i64
        } else {
            match digits.parse::<i64>().ok().and_then(|n| n.checked_mul(mult)) {
                Some(n) => n,
                None => return self.err("duration out of the 64-bit range"),
            }
        };
        Ok(Token::Duration(ns))
    }

    /// Template string: `` `text ${expr} text` `` (spec §7.4). The code of
    /// each `${...}` is tokenized here and parsed later by the parser.
    fn read_template(&mut self) -> Result<Token, String> {
        let (l, c0) = (self.line, self.col);
        self.advance(); // `
        let mut parts = Vec::new();
        let mut lit = String::new();

        loop {
            let Some(c) = self.advance() else {
                return Err(format!(
                    "line {}:{}: lexical error: unterminated template string",
                    l, c0
                ));
            };
            match c {
                '`' => break,
                '\\' => match self.advance() {
                    Some('n') => lit.push('\n'),
                    Some('t') => lit.push('\t'),
                    Some('r') => lit.push('\r'),
                    Some('0') => lit.push('\0'),
                    Some(e @ ('\\' | '`' | '$' | '\'' | '"')) => lit.push(e),
                    Some(other) => return self.err(format!("invalid escape: \\{}", other)),
                    None => {}
                },
                '$' if self.peek() == Some('{') => {
                    self.advance();
                    let (cl, cc) = (self.line, self.col);
                    let code = self.read_interpolation(l, c0)?;
                    if code.iter().all(|c| c.is_whitespace()) {
                        return Err(format!(
                            "line {}:{}: lexical error: empty '${{}}' in template string",
                            cl, cc
                        ));
                    }
                    let tokens = Lexer::at(code, cl, cc).tokenize()?;
                    if !lit.is_empty() {
                        parts.push(TplPart::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(TplPart::Code(tokens));
                }
                other => lit.push(other),
            }
        }
        if !lit.is_empty() || parts.is_empty() {
            parts.push(TplPart::Lit(lit));
        }
        Ok(Token::Template(parts))
    }

    /// Reads the code of `${...}` (right after `${`) up to its closing `}`,
    /// respecting nested braces and strings.
    fn read_interpolation(&mut self, l: u32, c0: u32) -> Result<Vec<char>, String> {
        let mut code = Vec::new();
        let mut depth = 0usize;
        let mut quote: Option<char> = None;
        loop {
            let Some(c) = self.advance() else {
                return Err(format!(
                    "line {}:{}: lexical error: '${{' without '}}' in template string",
                    l, c0
                ));
            };
            if let Some(q) = quote {
                code.push(c);
                if c == '\\' {
                    if let Some(n) = self.advance() {
                        code.push(n);
                    }
                } else if c == q {
                    quote = None;
                }
                continue;
            }
            match c {
                '"' | '\'' | '`' => quote = Some(c),
                '{' => depth += 1,
                '}' if depth == 0 => return Ok(code),
                '}' => depth -= 1,
                _ => {}
            }
            code.push(c);
        }
    }

    fn read_ident(&mut self) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        match s.as_str() {
            "let" => Token::Let,
            "const" => Token::Const,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "true" => Token::True,
            "false" => Token::False,
            "nil" => Token::Nil,
            "import" => Token::Import,
            "export" => Token::Export,
            "from" => Token::From,
            "as" => Token::As,
            "struct" => Token::Struct,
            "interface" => Token::Interface,
            "embed" => Token::Embed,
            "match" => Token::Match,
            "case" => Token::Case,
            "default" => Token::Default,
            "fg" => Token::Fg,
            "chan" => Token::Chan,
            "select" => Token::Select,
            _ => Token::Ident(s),
        }
    }
}
