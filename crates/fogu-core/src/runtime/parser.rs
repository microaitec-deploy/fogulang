use std::rc::Rc;

use super::ast::*;
use super::token::{Spanned, Token, TplPart};

/// Prepends the default-value prologue to the body.
fn with_prologue(prologue: Vec<Stmt>, body: Block) -> Block {
    if prologue.is_empty() {
        return body;
    }
    let mut stmts = prologue;
    stmts.extend(body.iter().cloned());
    Rc::new(stmts)
}

pub struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
    /// Generic type parameters in scope (`fn f<T>`, `struct S<T>`); they are
    /// erased to `any` in annotations.
    type_params: Vec<Vec<String>>,
}

impl Parser {
    pub fn new(tokens: Vec<Spanned>) -> Self {
        Self {
            tokens,
            pos: 0,
            type_params: Vec::new(),
        }
    }

    // ---------- Primitives ----------

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|s| &s.tok)
            .unwrap_or(&Token::Eof)
    }

    fn peek_n(&self, n: usize) -> &Token {
        self.tokens
            .get(self.pos + n)
            .map(|s| &s.tok)
            .unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self
            .tokens
            .get(self.pos)
            .map(|s| s.tok.clone())
            .unwrap_or(Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn check(&self, t: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(t)
    }

    fn matches(&mut self, t: &Token) -> bool {
        if self.check(t) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Is the current token on a later line than the previous token?
    fn nl_before(&self) -> bool {
        if self.pos == 0 {
            return false;
        }
        match (self.tokens.get(self.pos - 1), self.tokens.get(self.pos)) {
            (Some(a), Some(b)) => b.line > a.line,
            _ => false,
        }
    }

    fn here(&self) -> (u32, u32) {
        self.tokens
            .get(self.pos)
            .or_else(|| self.tokens.last())
            .map(|s| (s.line, s.col))
            .unwrap_or((1, 1))
    }

    fn fail<T>(&self, msg: impl AsRef<str>) -> Result<T, String> {
        let (l, c) = self.here();
        Err(format!(
            "line {}:{}: syntax error: {}",
            l,
            c,
            msg.as_ref()
        ))
    }

    fn expect(&mut self, t: Token, msg: &str) -> Result<(), String> {
        if self.check(&t) {
            self.advance();
            Ok(())
        } else {
            self.fail(format!("{} — found {:?}", msg, self.peek()))
        }
    }

    fn expect_ident(&mut self, what: &str) -> Result<String, String> {
        if let Token::Ident(s) = self.peek() {
            let s = s.clone();
            self.advance();
            Ok(s)
        } else {
            self.fail(format!("expected {}, found {:?}", what, self.peek()))
        }
    }

    fn line(&self) -> u32 {
        self.tokens.get(self.pos).map(|s| s.line as u32).unwrap_or(0)
    }

    fn consume_semicolon(&mut self) {
        let _ = self.matches(&Token::Semicolon);
    }

    fn expect_block_body(&mut self) -> Result<Block, String> {
        match self.parse_block()? {
            Stmt::Block(b) => Ok(Rc::new(b)),
            _ => unreachable!("parse_block always returns Stmt::Block"),
        }
    }

    // ---------- Program and statements ----------

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while !self.check(&Token::Eof) {
            stmts.push(Stmt::Line(self.line()));
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek().clone() {
            Token::Let => self.parse_let(true),
            Token::Const => self.parse_let(false),
            // `fn name(...)` is a declaration; `fn (...)` is an anonymous expression.
            Token::Fn if matches!(self.peek_n(1), Token::Ident(_)) => self.parse_fn_decl(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            // `loop { … }` repeats until `break` (or `return`), like `while true { … }`.
            // `loop` is contextual: anywhere else it is an ordinary name.
            Token::Ident(ref name) if name == "loop" && matches!(self.peek_n(1), Token::LBrace) => self.parse_loop(),
            Token::For => self.parse_for(),
            Token::Break => {
                self.advance();
                self.consume_semicolon();
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                self.consume_semicolon();
                Ok(Stmt::Continue)
            }
            Token::Return => self.parse_return(),
            Token::LBrace => self.parse_block(),
            // `import("./x.fg")` is an expression, not an import declaration
            Token::Import if !matches!(self.peek_n(1), Token::LParen) => self.parse_import(),
            Token::Export => self.parse_export(),
            Token::Struct => {
                let decl = self.parse_struct_decl()?;
                Ok(Stmt::Struct(Box::new(decl)))
            }
            Token::Interface => Ok(Stmt::Interface(self.parse_interface()?)),
            Token::Select => self.parse_select(),
            Token::Match => self.parse_match_stmt(),
            _ => {
                let expr = self.parse_expr()?;
                // `<expr> <- <value>` — send to a channel. A `<-` at the start
                // of a new line begins a new statement (a receive), instead.
                if self.check(&Token::LeftArrow) && !self.nl_before() {
                    self.advance();
                    let value = self.parse_expr()?;
                    self.consume_semicolon();
                    return Ok(Stmt::SendStmt { chan: expr, value });
                }
                self.consume_semicolon();
                Ok(Stmt::Expr(expr))
            }
        }
    }

    // ---------- Types (annotations) ----------

    /// A type annotation: `int`, `list<int>`, `(int, error)`, `map | nil`,
    /// `int?`, `fn(int): int`, a struct/interface name or a generic parameter.
    fn parse_type(&mut self) -> Result<TypeExpr, String> {
        let mut alts = vec![self.parse_type_postfix()?];
        while self.matches(&Token::Pipe) || self.matches(&Token::OrOr) {
            alts.push(self.parse_type_postfix()?);
        }
        Ok(if alts.len() == 1 { alts.pop().unwrap() } else { TypeExpr::Union(alts) })
    }

    fn parse_type_postfix(&mut self) -> Result<TypeExpr, String> {
        let mut t = self.parse_type_atom()?;
        while self.matches(&Token::Question) {
            t = TypeExpr::Optional(Box::new(t));
        }
        Ok(t)
    }

    fn parse_type_atom(&mut self) -> Result<TypeExpr, String> {
        match self.peek().clone() {
            Token::LParen => {
                self.advance();
                let mut items = vec![self.parse_type()?];
                while self.matches(&Token::Comma) {
                    items.push(self.parse_type()?);
                }
                self.expect(Token::RParen, "expected ')' in type")?;
                Ok(if items.len() == 1 { items.pop().unwrap() } else { TypeExpr::Tuple(items) })
            }
            Token::Nil => {
                self.advance();
                Ok(TypeExpr::Named("nil".into()))
            }
            Token::Fn => {
                // fn(int, str): bool — only "is a function" is checked
                self.advance();
                if self.matches(&Token::LParen) {
                    while !self.check(&Token::RParen) && !self.check(&Token::Eof) {
                        self.parse_type()?;
                        if !self.matches(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RParen, "expected ')' in function type")?;
                    if self.matches(&Token::Colon) {
                        self.parse_type()?;
                    }
                }
                Ok(TypeExpr::Named("fn".into()))
            }
            Token::Chan => {
                self.advance();
                let args = self.parse_type_args()?;
                Ok(if args.is_empty() {
                    TypeExpr::Named("chan".into())
                } else {
                    TypeExpr::Generic("chan".into(), args)
                })
            }
            Token::Ident(name) => {
                self.advance();
                let args = self.parse_type_args()?;
                if name == "any" || self.type_params.iter().any(|ps| ps.contains(&name)) {
                    return Ok(TypeExpr::Any);
                }
                Ok(if args.is_empty() {
                    TypeExpr::Named(name)
                } else {
                    TypeExpr::Generic(name, args)
                })
            }
            other => self.fail(format!("expected a type, found {:?}", other)),
        }
    }

    /// `<int, str>` after a generic type name (empty if there is none).
    fn parse_type_args(&mut self) -> Result<Vec<TypeExpr>, String> {
        let mut args = Vec::new();
        if self.matches(&Token::Lt) {
            loop {
                args.push(self.parse_type()?);
                if !self.matches(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Gt, "expected '>' after the type arguments")?;
        }
        Ok(args)
    }

    /// `<T, U: Comparable>` after a function or struct name (spec §27).
    /// The bounds are parsed but not enforced (type parameters are erased).
    fn parse_type_params(&mut self) -> Result<Vec<String>, String> {
        let mut params = Vec::new();
        if self.matches(&Token::Lt) {
            loop {
                params.push(self.expect_ident("a type parameter")?);
                if self.matches(&Token::Colon) {
                    self.parse_type()?;
                }
                if !self.matches(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Gt, "expected '>' after the type parameters")?;
        }
        Ok(params)
    }

    /// `interface Name { method(a, b): T ... }` (spec §25).
    fn parse_interface(&mut self) -> Result<InterfaceDecl, String> {
        self.advance(); // interface
        let name = self.expect_ident("an interface name")?;
        let tps = self.parse_type_params()?;
        self.type_params.push(tps);
        let r = (|| -> Result<InterfaceDecl, String> {
            self.expect(Token::LBrace, "expected '{' in interface")?;
            let mut methods = Vec::new();
            while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
                self.matches(&Token::Fn);
                let m = self.expect_ident("a method name")?;
                self.expect(Token::LParen, "expected '('")?;
                let mut n = 0;
                while !self.check(&Token::RParen) && !self.check(&Token::Eof) {
                    self.matches(&Token::Ellipsis);
                    self.expect_ident("a parameter")?;
                    if self.matches(&Token::Colon) {
                        self.parse_type()?;
                    }
                    n += 1;
                    if !self.matches(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen, "expected ')'")?;
                if self.matches(&Token::Colon) {
                    self.parse_type()?;
                }
                if !self.matches(&Token::Semicolon) {
                    let _ = self.matches(&Token::Comma);
                }
                methods.push((m, n));
            }
            self.expect(Token::RBrace, "expected '}' in interface")?;
            Ok(InterfaceDecl { name, methods })
        })();
        self.type_params.pop();
        r
    }

    /// `(a, b: int, c = 1, ...rest)`. Returns the names (the variadic one with
    /// the `...` prefix) and the prologue that applies the default values.
    fn parse_param_list(&mut self) -> Result<(Vec<String>, Vec<Stmt>), String> {
        self.expect(Token::LParen, "expected '('")?;
        let mut params: Vec<String> = Vec::new();
        let mut prologue = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                if rest_param(&params).is_some() {
                    return self.fail("the variadic parameter '...' must be the last one");
                }
                let rest = self.matches(&Token::Ellipsis);
                let name = self.expect_ident("a parameter")?;
                if params.iter().any(|p| p.trim_start_matches(REST_PREFIX) == name) {
                    return self.fail(format!("duplicate parameter '{}'", name));
                }
                let ty = if self.matches(&Token::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                if self.check(&Token::Eq) {
                    if rest {
                        return self.fail("the variadic parameter cannot have a default value");
                    }
                    self.advance();
                    let line = self.line();
                    let default = self.parse_expr()?;
                    // `p = p ?? <default>`: applies when the argument is missing.
                    prologue.push(Stmt::Line(line));
                    prologue.push(Stmt::Expr(Expr::Assign {
                        target: Box::new(Expr::Ident(Name::new(name.clone()))),
                        op: None,
                        value: Box::new(Expr::Logical {
                            left: Box::new(Expr::Ident(Name::new(name.clone()))),
                            op: LogicalOp::Coalesce,
                            right: Box::new(default),
                        }),
                    }));
                }
                if let Some(ty) = ty.filter(|t| *t != TypeExpr::Any) {
                    // the variadic parameter is a list of the annotated type
                    let ty = if rest { TypeExpr::Named("list".into()) } else { ty };
                    prologue.push(Stmt::CheckType {
                        name: name.clone(),
                        ty,
                    });
                }
                params.push(if rest { format!("{}{}", REST_PREFIX, name) } else { name });
                if !self.matches(&Token::Comma) {
                    break;
                }
                if self.check(&Token::RParen) {
                    break;
                }
            }
        }
        self.expect(Token::RParen, "expected ')'")?;
        Ok((params, prologue))
    }

    /// Optional type parameters, parameters, optional return type and body
    /// of a function.
    fn parse_fn_tail(&mut self) -> Result<(Vec<String>, Block), String> {
        let tps = self.parse_type_params()?;
        self.type_params.push(tps);
        let r = (|| -> Result<(Vec<String>, Block), String> {
            let (params, mut prologue) = self.parse_param_list()?;
            if let Some(ret) = self.parse_optional_return_type()? {
                // must be the first statement of the body (see Interpreter::run_body)
                prologue.insert(0, Stmt::ReturnType(ret));
            }
            let body = self.expect_block_body()?;
            Ok((params, with_prologue(prologue, body)))
        })();
        self.type_params.pop();
        r
    }

    /// `(params) => expr`, `(params) => { ... }` or `(params): T => ...` (spec §21).
    fn parse_arrow_fn(&mut self) -> Result<Expr, String> {
        let (params, mut prologue) = self.parse_param_list()?;
        if let Some(ret) = self.parse_optional_return_type()? {
            prologue.insert(0, Stmt::ReturnType(ret));
        }
        self.expect(Token::FatArrow, "expected '=>'")?;
        let body = if self.check(&Token::LBrace) {
            self.expect_block_body()?
        } else {
            let line = self.line();
            let e = self.parse_expr()?;
            Rc::new(vec![Stmt::Line(line), Stmt::Return(Some(e))])
        };
        Ok(Expr::Fn {
            params,
            body: with_prologue(prologue, body),
        })
    }

    /// Does the current `(` open the parameter list of an arrow function?
    fn arrow_ahead(&self) -> bool {
        if !self.check(&Token::LParen)
            || !matches!(self.peek_n(1), Token::RParen | Token::Ident(_) | Token::Ellipsis)
        {
            return false;
        }
        let mut depth = 0usize;
        let mut i = 0;
        loop {
            match self.peek_n(i) {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return match self.peek_n(i + 1) {
                            Token::FatArrow => true,
                            // `(params): Type =>` — only type tokens up to `=>`
                            Token::Colon => {
                                let mut j = i + 2;
                                loop {
                                    match self.peek_n(j) {
                                        Token::FatArrow => return true,
                                        Token::Ident(_) | Token::Nil | Token::Fn | Token::Chan
                                        | Token::Lt | Token::Gt | Token::Pipe | Token::Question
                                        | Token::LParen | Token::RParen | Token::Comma => j += 1,
                                        _ => return false,
                                    }
                                }
                            }
                            _ => false,
                        };
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            i += 1;
        }
    }

    fn parse_optional_return_type(&mut self) -> Result<Option<TypeExpr>, String> {
        if self.matches(&Token::Colon) {
            let t = self.parse_type()?;
            return Ok(Some(t).filter(|t| *t != TypeExpr::Any));
        }
        Ok(None)
    }

    // ---------- Blocks and declarations ----------

    fn parse_let(&mut self, mutable: bool) -> Result<Stmt, String> {
        self.advance();
        if self.matches(&Token::LBracket) {
            let mut names = Vec::new();
            loop {
                if self.matches(&Token::Underscore) {
                    names.push("_".to_string());
                } else {
                    names.push(self.expect_ident("a variable name")?);
                }
                if !self.matches(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RBracket, "expected ']'")?;
            self.expect(Token::Eq, "expected '=' after the list of names")?;
            let init = self.parse_expr()?;
            self.consume_semicolon();
            return Ok(Stmt::LetList { names, mutable, init });
        }
        let name = self.expect_ident("a variable name")?;
        let ty = if self.matches(&Token::Colon) {
            Some(self.parse_type()?).filter(|t| *t != TypeExpr::Any)
        } else {
            None
        };
        self.expect(Token::Eq, "expected '=' after the variable name")?;
        let init = self.parse_expr()?;
        self.consume_semicolon();
        Ok(Stmt::Let {
            name,
            mutable,
            init,
            ty,
        })
    }

    fn parse_fn_decl(&mut self) -> Result<Stmt, String> {
        self.advance();
        let name = self.expect_ident("a function name")?;
        let (params, body) = self.parse_fn_tail()?;
        Ok(Stmt::FnDecl { name, params, body })
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.advance();
        let cond = self.parse_expr()?;
        let then_branch = Box::new(self.parse_block()?);
        let else_branch = if self.matches(&Token::Else) {
            if self.check(&Token::If) {
                Some(Box::new(self.parse_if()?))
            } else {
                Some(Box::new(self.parse_block()?))
            }
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.advance();
        let cond = self.parse_expr()?;
        let body = Box::new(self.parse_block()?);
        Ok(Stmt::While {
            id: next_loop_id(),
            cond,
            body,
        })
    }

    fn parse_loop(&mut self) -> Result<Stmt, String> {
        self.advance();
        let body = Box::new(self.parse_block()?);
        Ok(Stmt::While {
            id: next_loop_id(),
            cond: Expr::Bool(true),
            body,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.advance();
        let var = self.expect_ident("an identifier")?;
        let var2 = if self.matches(&Token::Comma) {
            Some(self.expect_ident("an identifier")?)
        } else {
            None
        };
        self.expect(Token::In, "expected 'in'")?;
        let iter = self.parse_expr()?;
        let body = Box::new(self.parse_block()?);
        Ok(Stmt::For {
            id: next_loop_id(),
            var,
            var2,
            iter,
            body,
        })
    }

    fn parse_return(&mut self) -> Result<Stmt, String> {
        self.advance();
        // bare `return`: end of line, `;`, `}` or end of file.
        if self.check(&Token::Semicolon)
            || self.check(&Token::RBrace)
            || self.check(&Token::Eof)
            || self.nl_before()
        {
            self.consume_semicolon();
            return Ok(Stmt::Return(None));
        }
        let mut expr = self.parse_expr()?;
        // `return a, b` returns a tuple (list of 2+ elements).
        if self.check(&Token::Comma) {
            let mut items = vec![expr];
            while self.matches(&Token::Comma) {
                items.push(self.parse_expr()?);
            }
            expr = Expr::List(items);
        }
        self.consume_semicolon();
        Ok(Stmt::Return(Some(expr)))
    }

    fn parse_block(&mut self) -> Result<Stmt, String> {
        self.expect(Token::LBrace, "expected '{'")?;
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
            stmts.push(Stmt::Line(self.line()));
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace, "expected '}'")?;
        Ok(Stmt::Block(stmts))
    }

    // ---------- struct ----------

    fn parse_struct_decl(&mut self) -> Result<StructDecl, String> {
        self.advance(); // struct
        let name = self.expect_ident("a struct name")?;
        let tps = self.parse_type_params()?;
        self.type_params.push(tps);
        let r = self.parse_struct_body(name);
        self.type_params.pop();
        r
    }

    fn parse_struct_body(&mut self, name: String) -> Result<StructDecl, String> {
        self.expect(Token::LBrace, "expected '{' in struct")?;

        let mut embeds = Vec::new();
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut constructor = None;

        while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
            match self.peek().clone() {
                Token::Embed => {
                    self.advance();
                    embeds.push(self.expect_ident("a struct name after embed")?);
                }
                Token::Fn => {
                    self.advance();
                    let mname = self.expect_ident("a method name")?;
                    let (params, body) = self.parse_fn_tail()?;
                    if mname == "new" {
                        constructor = Some((params, body));
                    } else {
                        methods.push((mname, params, body));
                    }
                }
                Token::Ident(fname) => {
                    self.advance();
                    // `name(...) { }` — method without `fn` (includes `new`).
                    if self.check(&Token::LParen) {
                        let (params, body) = self.parse_fn_tail()?;
                        if fname == "new" {
                            constructor = Some((params, body));
                        } else {
                            methods.push((fname, params, body));
                        }
                        continue;
                    }
                    // Otherwise it is a field (its type is documentation only).
                    if self.matches(&Token::Colon) {
                        self.parse_type()?;
                    }
                    let default = if self.matches(&Token::Eq) {
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    self.consume_semicolon();
                    fields.push(StructField {
                        name: fname,
                        default,
                    });
                }
                other => {
                    return self.fail(format!(
                        "unexpected content inside struct: {:?}",
                        other
                    ));
                }
            }
        }
        self.expect(Token::RBrace, "expected '}' in struct")?;

        Ok(StructDecl {
            name,
            embeds,
            fields,
            methods,
            constructor,
        })
    }

    // ---------- select ----------

    fn parse_select(&mut self) -> Result<Stmt, String> {
        self.advance();
        self.expect(Token::LBrace, "expected '{' after select")?;

        let mut cases = Vec::new();
        let mut default = None;

        while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
            if self.matches(&Token::Default) {
                self.expect(Token::Colon, "expected ':' after default")?;
                default = Some(self.parse_stmt_list_until_case()?);
                continue;
            }

            self.expect(Token::Case, "expected 'case' or 'default' in select")?;

            // case <-c:
            if self.matches(&Token::LeftArrow) {
                let chan = self.parse_unary()?;
                self.expect(Token::Colon, "expected ':' after case")?;
                let body = self.parse_stmt_list_until_case()?;
                cases.push(SelectCase::Recv {
                    var: "_".into(),
                    chan,
                    body,
                });
                continue;
            }

            // case v = <-c:
            if let Token::Ident(name) = self.peek().clone() {
                if self.peek_n(1) == &Token::Eq {
                    self.advance();
                    self.advance();
                    self.expect(Token::LeftArrow, "expected '<-'")?;
                    let chan = self.parse_unary()?;
                    self.expect(Token::Colon, "expected ':' after case")?;
                    let body = self.parse_stmt_list_until_case()?;
                    cases.push(SelectCase::Recv {
                        var: name,
                        chan,
                        body,
                    });
                    continue;
                }
            }

            // case c <- v:
            let chan = self.parse_expr()?;
            self.expect(Token::LeftArrow, "expected '<-'")?;
            let value = self.parse_expr()?;
            self.expect(Token::Colon, "expected ':' after case")?;
            let body = self.parse_stmt_list_until_case()?;
            cases.push(SelectCase::Send { chan, value, body });
        }

        self.expect(Token::RBrace, "expected '}' in select")?;
        Ok(Stmt::Select { cases, default })
    }

    fn parse_stmt_list_until_case(&mut self) -> Result<Vec<Stmt>, String> {
        let mut body = Vec::new();
        while !self.check(&Token::RBrace)
            && !self.check(&Token::Case)
            && !self.check(&Token::Default)
            && !self.check(&Token::Eof)
        {
            body.push(Stmt::Line(self.line()));
            body.push(self.parse_stmt()?);
        }
        Ok(body)
    }

    // ---------- match ----------

    fn parse_match_stmt(&mut self) -> Result<Stmt, String> {
        self.advance();
        let subject = self.parse_expr()?;
        let arms = self.parse_match_body()?;
        Ok(Stmt::MatchStmt { subject, arms })
    }

    fn parse_match_expr(&mut self) -> Result<Expr, String> {
        self.advance();
        let subject = self.parse_expr()?;
        let arms = self.parse_match_body()?;
        Ok(Expr::Match {
            subject: Box::new(subject),
            arms,
        })
    }

    fn parse_match_body(&mut self) -> Result<Vec<MatchArm>, String> {
        self.expect(Token::LBrace, "expected '{' in match")?;
        let mut arms = Vec::new();
        while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
            let pattern = self.parse_pattern()?;
            if !self.matches(&Token::FatArrow) && !self.matches(&Token::PipeArrow) {
                return self.fail(format!(
                    "expected '=>' after a match pattern, found {:?}",
                    self.peek()
                ));
            }
            let body = self.parse_expr()?;
            arms.push(MatchArm { pattern, body });
            if !self.matches(&Token::Semicolon) {
                let _ = self.matches(&Token::Comma);
            }
        }
        self.expect(Token::RBrace, "expected '}' in match")?;
        Ok(arms)
    }

    fn parse_pattern(&mut self) -> Result<Pattern, String> {
        let first = self.parse_pattern_atom()?;
        if self.check(&Token::Pipe) {
            let mut pats = vec![first];
            while self.matches(&Token::Pipe) {
                pats.push(self.parse_pattern_atom()?);
            }
            return Ok(Pattern::Or(pats));
        }
        Ok(first)
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern, String> {
        if self.matches(&Token::Underscore) {
            return Ok(Pattern::Wildcard);
        }

        if let Token::Ident(name) = self.peek().clone() {
            match self.peek_n(1) {
                Token::If => {
                    self.advance();
                    self.advance();
                    let guard = self.parse_expr()?;
                    return Ok(Pattern::IdentBind {
                        name,
                        guard: Some(guard),
                    });
                }
                Token::FatArrow | Token::PipeArrow | Token::Pipe => {
                    self.advance();
                    return Ok(Pattern::Ident(name));
                }
                _ => {}
            }
        }

        // Literal patterns: use the `parse_or` level so as not to consume `|`
        // or `=>`, and without assignment.
        Ok(Pattern::Literal(self.parse_equality()?))
    }

    // ---------- import / export ----------

    fn parse_import(&mut self) -> Result<Stmt, String> {
        self.advance();
        let kind = if self.matches(&Token::LBrace) {
            let mut members = Vec::new();
            while !self.check(&Token::RBrace) {
                let orig = self.expect_ident("a name")?;
                let alias = if self.matches(&Token::As) {
                    self.expect_ident("alias")?
                } else {
                    orig.clone()
                };
                members.push((orig, alias));
                if !self.matches(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RBrace, "expected '}'")?;
            ImportKind::Members { members }
        } else if self.matches(&Token::Star) {
            self.expect(Token::As, "expected 'as'")?;
            ImportKind::Namespace {
                alias: self.expect_ident("alias")?,
            }
        } else {
            ImportKind::Named {
                alias: self.expect_ident("a name")?,
            }
        };
        self.expect(Token::From, "expected 'from'")?;
        let path = match self.peek().clone() {
            Token::Str(s) => {
                self.advance();
                s
            }
            other => return self.fail(format!("expected a path, found {:?}", other)),
        };
        self.consume_semicolon();
        Ok(Stmt::Import { kind, path })
    }

    fn parse_export(&mut self) -> Result<Stmt, String> {
        self.advance();
        match self.peek().clone() {
            Token::Fn => match self.parse_fn_decl()? {
                Stmt::FnDecl { name, params, body } => Ok(Stmt::Export(Box::new(
                    ExportKind::Fn { name, params, body },
                ))),
                _ => unreachable!(),
            },
            Token::Let | Token::Const => {
                let mutable = matches!(self.peek(), Token::Let);
                match self.parse_let(mutable)? {
                    Stmt::Let {
                        name,
                        mutable,
                        init,
                        ty,
                    } => Ok(Stmt::Export(Box::new(ExportKind::Let {
                        name,
                        mutable,
                        init,
                        ty,
                    }))),
                    _ => unreachable!(),
                }
            }
            Token::Struct => {
                let decl = self.parse_struct_decl()?;
                Ok(Stmt::Export(Box::new(ExportKind::Struct(decl))))
            }
            Token::Interface => {
                let decl = self.parse_interface()?;
                Ok(Stmt::Export(Box::new(ExportKind::Interface(decl))))
            }
            other => self.fail(format!(
                "expected 'fn', 'let', 'const' or 'struct' after export, found {:?}",
                other
            )),
        }
    }

    // ---------- Expressions ----------

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr, String> {
        let expr = self.parse_range()?;
        let op: Option<Option<BinaryOp>> = match self.peek() {
            Token::Eq => Some(None),
            Token::PlusEq => Some(Some(BinaryOp::Add)),
            Token::MinusEq => Some(Some(BinaryOp::Sub)),
            Token::StarEq => Some(Some(BinaryOp::Mul)),
            Token::SlashEq => Some(Some(BinaryOp::Div)),
            Token::PercentEq => Some(Some(BinaryOp::Rem)),
            _ => None,
        };
        if let Some(op) = op {
            if !matches!(
                expr,
                Expr::Ident(_) | Expr::Access { .. } | Expr::Index { .. }
            ) {
                return self.fail("invalid assignment target (use a variable, field or index)");
            }
            self.advance();
            let value = self.parse_assignment()?;
            return Ok(Expr::Assign {
                target: Box::new(expr),
                op,
                value: Box::new(value),
            });
        }
        Ok(expr)
    }

    fn parse_range(&mut self) -> Result<Expr, String> {
        let start = self.parse_or()?;
        if self.matches(&Token::DotDot) {
            let end = self.parse_or()?;
            return Ok(Expr::Range {
                start: Box::new(start),
                end: Box::new(end),
            });
        }
        Ok(start)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        loop {
            let op = if self.matches(&Token::OrOr) {
                LogicalOp::Or
            } else if self.matches(&Token::QuestionQuestion) {
                LogicalOp::Coalesce
            } else {
                break;
            };
            let right = self.parse_and()?;
            left = Expr::Logical {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_equality()?;
        while self.matches(&Token::AndAnd) {
            let right = self.parse_equality()?;
            left = Expr::Logical {
                left: Box::new(left),
                op: LogicalOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = if self.matches(&Token::EqEq) {
                BinaryOp::Eq
            } else if self.matches(&Token::BangEq) {
                BinaryOp::Ne
            } else {
                break;
            };
            let right = self.parse_comparison()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_term()?;
        loop {
            let op = if self.matches(&Token::Lt) {
                BinaryOp::Lt
            } else if self.matches(&Token::LtEq) {
                BinaryOp::Le
            } else if self.matches(&Token::Gt) {
                BinaryOp::Gt
            } else if self.matches(&Token::GtEq) {
                BinaryOp::Ge
            } else {
                break;
            };
            let right = self.parse_term()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_factor()?;
        loop {
            // A `-` at the start of a new line begins a new statement
            // (e.g. a match arm `-1 => ...`), it is not a subtraction.
            let op = if self.matches(&Token::Plus) {
                BinaryOp::Add
            } else if self.check(&Token::Minus) && !self.nl_before() {
                self.advance();
                BinaryOp::Sub
            } else {
                break;
            };
            let right = self.parse_factor()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_power()?;
        loop {
            let op = if self.matches(&Token::Star) {
                BinaryOp::Mul
            } else if self.matches(&Token::Slash) {
                BinaryOp::Div
            } else if self.matches(&Token::Percent) {
                BinaryOp::Rem
            } else {
                break;
            };
            let right = self.parse_power()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Spec §14: `!` `-` `<-` (level 2) bind tighter than `**` (level 3),
    // so `-2 ** 2` == `(-2) ** 2`. `**` is right-associative.
    fn parse_power(&mut self) -> Result<Expr, String> {
        let left = self.parse_unary()?;
        if self.matches(&Token::StarStar) {
            let right = self.parse_power()?;
            return Ok(Expr::Binary {
                left: Box::new(left),
                op: BinaryOp::Pow,
                right: Box::new(right),
            });
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if self.matches(&Token::Minus) {
            let right = self.parse_unary()?;
            return Ok(match right {
                Expr::Int(n) => match n.checked_neg() {
                    Some(m) => Expr::Int(m),
                    None => Expr::Unary {
                        op: UnaryOp::Neg,
                        right: Box::new(Expr::Int(n)),
                    },
                },
                Expr::Float(f) => Expr::Float(-f),
                other => Expr::Unary {
                    op: UnaryOp::Neg,
                    right: Box::new(other),
                },
            });
        }
        if self.matches(&Token::Bang) {
            let right = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                right: Box::new(right),
            });
        }
        if self.matches(&Token::LeftArrow) {
            let chan = self.parse_unary()?;
            return Ok(Expr::Recv {
                chan: Box::new(chan),
            });
        }
        self.parse_postfix()
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if !self.matches(&Token::Comma) {
                    break;
                }
                if self.check(&Token::RParen) {
                    break;
                }
            }
        }
        self.expect(Token::RParen, "expected ')'")?;
        Ok(args)
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        let mut optional = false;

        loop {
            // `(` and `[` at the start of a new line begin a new statement.
            if self.check(&Token::LParen) && !self.nl_before() {
                self.advance();
                let args = self.parse_args()?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.check(&Token::LBracket) && !self.nl_before() {
                self.advance();
                let index = self.parse_expr()?;
                self.expect(Token::RBracket, "expected ']'")?;
                expr = Expr::Index {
                    target: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.matches(&Token::Dot) {
                let field = self.parse_field_name()?;
                expr = Expr::Access {
                    target: Box::new(expr),
                    field,
                };
            } else if self.matches(&Token::QuestionDot) {
                optional = true;
                let field = self.parse_field_name()?;
                expr = Expr::OptAccess {
                    target: Box::new(expr),
                    field,
                };
            } else if self.check(&Token::Question) {
                self.advance();
                if optional {
                    expr = Expr::OptChain(Box::new(expr));
                    optional = false;
                }
                expr = Expr::Try {
                    expr: Box::new(expr),
                };
            } else {
                break;
            }
        }

        if optional {
            expr = Expr::OptChain(Box::new(expr));
        }
        Ok(expr)
    }

    /// Reads a field name after `.`. Accepts identifiers and keywords that
    /// are not ambiguous in that position.
    fn parse_field_name(&mut self) -> Result<String, String> {
        let name = match self.peek() {
            Token::Ident(s) => s.clone(),
            Token::From => "from".into(),
            Token::Match => "match".into(),
            Token::Case => "case".into(),
            Token::Default => "default".into(),
            Token::In => "in".into(),
            Token::Import => "import".into(),
            Token::Export => "export".into(),
            Token::Struct => "struct".into(),
            Token::Interface => "interface".into(),
            Token::Embed => "embed".into(),
            Token::As => "as".into(),
            Token::Select => "select".into(),
            Token::Chan => "chan".into(),
            Token::Fg => "fg".into(),
            other => {
                return self.fail(format!("expected a field name, found {:?}", other));
            }
        };
        self.advance();
        Ok(name)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        // fg <call>
        if self.matches(&Token::Fg) {
            let call = self.parse_postfix()?;
            if !matches!(call, Expr::Call { .. }) {
                return self.fail("'fg' must be followed by a function call");
            }
            return Ok(Expr::Fg {
                call: Box::new(call),
            });
        }

        if self.check(&Token::Match) {
            return self.parse_match_expr();
        }

        // import("./mod.fg"): a module loaded at run time
        if self.check(&Token::Import) && matches!(self.peek_n(1), Token::LParen) {
            self.advance();
            self.advance();
            let spec = self.parse_expr()?;
            self.expect(Token::RParen, "expected ')' after the module path of import(...)")?;
            return Ok(Expr::DynImport(Box::new(spec)));
        }

        // anonymous fn
        if self.check(&Token::Fn) {
            self.advance();
            if let Token::Ident(_) = self.peek() {
                self.advance();
            }
            let (params, body) = self.parse_fn_tail()?;
            return Ok(Expr::Fn { params, body });
        }

        if self.arrow_ahead() {
            return self.parse_arrow_fn();
        }

        // chan<T>(buffer: N) or chan(N)
        if self.matches(&Token::Chan) {
            if self.matches(&Token::Lt) {
                let mut depth = 1;
                while depth > 0 {
                    match self.advance() {
                        Token::Lt => depth += 1,
                        Token::Gt => depth -= 1,
                        Token::Eof => return self.fail("expected '>'"),
                        _ => {}
                    }
                }
            }
            let buffer = if self.matches(&Token::LParen) {
                if matches!(self.peek(), Token::Ident(_)) && self.peek_n(1) == &Token::Colon {
                    self.advance();
                    self.advance();
                }
                if self.matches(&Token::RParen) {
                    None
                } else {
                    let e = self.parse_expr()?;
                    self.expect(Token::RParen, "expected ')'")?;
                    Some(Box::new(e))
                }
            } else {
                None
            };
            return Ok(Expr::Chan { buffer });
        }

        let (line, col) = self.here();
        match self.advance() {
            Token::Int(n) => Ok(Expr::Int(n)),
            Token::Float(n) => Ok(Expr::Float(n)),
            Token::Str(s) => Ok(Expr::Str(s.into())),
            Token::Duration(ns) => Ok(Expr::Duration(ns)),
            Token::Template(parts) => {
                let mut out = Vec::with_capacity(parts.len());
                for part in parts {
                    match part {
                        TplPart::Lit(s) => out.push(Expr::Str(s.into())),
                        TplPart::Code(tokens) => {
                            let mut sub = Parser::new(tokens);
                            let e = sub.parse_expr()?;
                            if !sub.check(&Token::Eof) {
                                return sub.fail(format!(
                                    "expected '}}' in template string, found {:?}",
                                    sub.peek()
                                ));
                            }
                            out.push(e);
                        }
                    }
                }
                Ok(Expr::Template(out))
            }
            Token::True => Ok(Expr::Bool(true)),
            Token::False => Ok(Expr::Bool(false)),
            Token::Nil => Ok(Expr::Nil),

            Token::Ident(s) => {
                if s == "new" && matches!(self.peek(), Token::Ident(_)) {
                    let name = self.expect_ident("a struct name")?;
                    // `new Stack<int>()`: type arguments are erased
                    if self.check(&Token::Lt) {
                        self.parse_type_args()?;
                    }
                    let args = if self.check(&Token::LParen) && !self.nl_before() {
                        self.advance();
                        self.parse_args()?
                    } else {
                        Vec::new()
                    };
                    return Ok(Expr::New { name, args });
                }
                Ok(Expr::Ident(Name::new(s)))
            }

            Token::LParen => {
                let expr = self.parse_expr()?;
                self.expect(Token::RParen, "expected ')'")?;
                Ok(expr)
            }

            Token::LBracket => {
                let mut elems = Vec::new();
                while !self.check(&Token::RBracket) {
                    elems.push(self.parse_expr()?);
                    if !self.matches(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBracket, "expected ']'")?;
                Ok(Expr::List(elems))
            }

            Token::LBrace => {
                let mut entries: Vec<(String, Expr)> = Vec::new();
                while !self.check(&Token::RBrace) {
                    let key = match self.peek().clone() {
                        Token::Ident(s) | Token::Str(s) => {
                            self.advance();
                            s
                        }
                        // a key is always followed by ':', so any keyword works
                        other if keyword_text(&other).is_some() => {
                            self.advance();
                            keyword_text(&other).unwrap().to_string()
                        }
                        other => {
                            return self.fail(format!("expected a key, found {:?}", other));
                        }
                    };
                    self.expect(Token::Colon, "expected ':'")?;
                    let value = self.parse_expr()?;
                    entries.push((key, value));
                    if !self.matches(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBrace, "expected '}'")?;
                Ok(Expr::Map(entries))
            }

            other => Err(format!(
                "line {}:{}: syntax error: unexpected expression: {:?}",
                line, col, other
            )),
        }
    }
}

/// The source text of a keyword token (`None` for other tokens).
fn keyword_text(t: &Token) -> Option<&'static str> {
    Some(match t {
        Token::Let => "let",
        Token::Const => "const",
        Token::Fn => "fn",
        Token::Return => "return",
        Token::If => "if",
        Token::Else => "else",
        Token::While => "while",
        Token::For => "for",
        Token::In => "in",
        Token::Break => "break",
        Token::Continue => "continue",
        Token::True => "true",
        Token::False => "false",
        Token::Nil => "nil",
        Token::Import => "import",
        Token::Export => "export",
        Token::From => "from",
        Token::As => "as",
        Token::Struct => "struct",
        Token::Interface => "interface",
        Token::Embed => "embed",
        Token::Match => "match",
        Token::Case => "case",
        Token::Default => "default",
        Token::Fg => "fg",
        Token::Chan => "chan",
        Token::Select => "select",
        _ => return None,
    })
}
