//! `fogu fmt` — formats `.fg` files in a single style (no configuration).
//!
//! The formatter works on tokens and keeps the program's line breaks:
//! indentation is 4 spaces per open `{ [ (`, spacing around commas, colons,
//! binary operators and braces is normalized, runs of blank lines become
//! one, trailing whitespace goes away. Strings, templates and comments are
//! kept exactly.
//!
//! Safety net: the formatted text is tokenized again with the real lexer and
//! must give the same tokens (and the same tokens starting a line) and the
//! same comments; otherwise the file is left untouched.
//!
//! ```text
//! fogu fmt                 every .fg file under the current folder
//! fogu fmt src main.fg     the given files and folders
//! fogu fmt --check         lists the files that are not formatted (exit 1)
//! fogu fmt -               formats stdin to stdout
//! ```

use std::path::{Path, PathBuf};

use super::ui;
use crate::runtime::token::{Spanned, Token, TplPart};
use crate::runtime::Lexer;

const KEYWORDS: &[&str] = &[
    "let", "const", "fn", "return", "if", "else", "while", "for", "in", "break", "continue", "true", "false", "nil",
    "import", "export", "from", "as", "struct", "interface", "embed", "match", "case", "default", "fg", "chan",
    "select",
];

/// Operators, longest first.
const OPS: &[&str] = &[
    "...", "**=", "<<=", ">>=", "==", "!=", "<=", ">=", "&&", "||", "=>", "??", "?.", "..", "**", "+=", "-=", "*=",
    "/=", "%=", "<-", "->", "<<", ">>", "::", "+", "-", "*", "/", "%", "=", "<", ">", "!", "?", ".", ",", ":", ";",
    "(", ")", "[", "]", "{", "}", "&", "|", "^", "~", "@", "#", "$",
];

/// Binary operators that get a space on both sides.
const BINARY: &[&str] = &[
    "=", "==", "!=", "<=", ">=", "&&", "||", "=>", "??", "**", "+=", "-=", "*=", "/=", "%=", "**=", "*", "/", "%",
    "<<", ">>", "<<=", ">>=", "|", "&", "^", "->",
];

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Word,    // identifier or keyword
    Number,  // 1, 2.5, 0xff, 500ms
    Str,     // "…", '…', `…` (verbatim)
    Comment, // // … or /* … */ (verbatim)
    Op,
    Newline,
}

#[derive(Clone, Debug)]
struct Piece {
    kind: Kind,
    text: String,
    /// whitespace before it in the source (on the same line)
    space_before: bool,
    /// how much whitespace (to keep the alignment of trailing comments)
    gap: usize,
}

fn scan(src: &str) -> Vec<Piece> {
    let c: Vec<char> = src.chars().collect();
    let mut out: Vec<Piece> = Vec::new();
    let mut i = 0;
    let mut space = false;
    let mut gap = 0usize;
    let push = |out: &mut Vec<Piece>, kind: Kind, text: String, space: &mut bool, gap: &mut usize| {
        out.push(Piece { kind, text, space_before: *space, gap: *gap });
        *space = false;
        *gap = 0;
    };
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            push(&mut out, Kind::Newline, "\n".into(), &mut space, &mut gap);
            i += 1;
            continue;
        }
        if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\u{feff}' {
            space = true;
            if ch == ' ' || ch == '\t' {
                gap += 1;
            }
            i += 1;
            continue;
        }
        let start = i;
        if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            let text: String = c[start..i].iter().collect();
            push(&mut out, Kind::Comment, text.trim_end().to_string(), &mut space, &mut gap);
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(c.len());
            push(&mut out, Kind::Comment, c[start..i].iter().collect(), &mut space, &mut gap);
            continue;
        }
        if ch == '"' || ch == '\'' {
            i += 1;
            while i < c.len() && c[i] != ch {
                if c[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(c.len());
            push(&mut out, Kind::Str, c[start..i].iter().collect(), &mut space, &mut gap);
            continue;
        }
        if ch == '`' {
            // a template: `${…}` may hold strings, templates and braces
            i += 1;
            let mut depth = 0usize;
            let mut stack: Vec<char> = Vec::new();
            while i < c.len() {
                if let Some(&q) = stack.last() {
                    if c[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if c[i] == q {
                        stack.pop();
                    }
                    i += 1;
                    continue;
                }
                if depth == 0 {
                    if c[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if c[i] == '`' {
                        break;
                    }
                    if c[i] == '$' && c.get(i + 1) == Some(&'{') {
                        depth = 1;
                        i += 2;
                        continue;
                    }
                    i += 1;
                    continue;
                }
                match c[i] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    '"' | '\'' | '`' => stack.push(c[i]),
                    _ => {}
                }
                i += 1;
            }
            i = (i + 1).min(c.len());
            push(&mut out, Kind::Str, c[start..i].iter().collect(), &mut space, &mut gap);
            continue;
        }
        if ch.is_ascii_digit() {
            let hex = ch == '0' && matches!(c.get(i + 1), Some('x' | 'X' | 'b' | 'B'));
            while i < c.len()
                && (c[i].is_ascii_alphanumeric()
                    || c[i] == '_'
                    || (c[i] == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
                    // the sign of an exponent: 2.5e-1
                    || (!hex && matches!(c[i], '+' | '-') && matches!(c[i - 1], 'e' | 'E') && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())))
            {
                i += 1;
            }
            push(&mut out, Kind::Number, c[start..i].iter().collect(), &mut space, &mut gap);
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            push(&mut out, Kind::Word, c[start..i].iter().collect(), &mut space, &mut gap);
            continue;
        }
        let rest: String = c[i..(i + 3).min(c.len())].iter().collect();
        let op = OPS.iter().find(|o| rest.starts_with(**o)).map(|o| o.to_string()).unwrap_or_else(|| ch.to_string());
        i += op.chars().count();
        push(&mut out, Kind::Op, op, &mut space, &mut gap);
    }
    out
}

fn is_keyword(p: &Piece) -> bool {
    p.kind == Kind::Word && KEYWORDS.contains(&p.text.as_str())
}

/// Ends an operand: after it, `-` is binary, `(` is a call, `[` an index.
fn ends_value(p: &Piece) -> bool {
    match p.kind {
        Kind::Word => !is_keyword(p) || matches!(p.text.as_str(), "true" | "false" | "nil"),
        Kind::Number | Kind::Str => true,
        Kind::Op => matches!(p.text.as_str(), ")" | "]" | "}" | "?"),
        _ => false,
    }
}

/// Is there a space between `prev` and `cur` (both on the same line)?
fn spaced(prev: &Piece, cur: &Piece, prev2: Option<&Piece>) -> bool {
    let (p, t) = (prev.text.as_str(), cur.text.as_str());
    if cur.kind == Kind::Comment {
        return true;
    }
    // what never has a space before
    if cur.kind == Kind::Op && matches!(t, "," | ";" | ")" | "]" | "." | "?." | "..") {
        return false;
    }
    if cur.kind == Kind::Op && t == ":" {
        return false;
    }
    if cur.kind == Kind::Op && t == "?" && ends_value(prev) {
        return false; // postfix `?`
    }
    // what never has a space after
    if prev.kind == Kind::Op && matches!(p, "(" | "[" | "." | "?." | ".." | "...") {
        return false;
    }
    if prev.kind == Kind::Op && p == "!" {
        return false;
    }
    // unary minus/plus: after an operator, a keyword or the line start
    if prev.kind == Kind::Op && matches!(p, "-" | "+") && !prev2.is_some_and(ends_value) {
        return false;
    }
    // what always has a space after
    if prev.kind == Kind::Op && matches!(p, "," | ":" | ";") {
        return true;
    }
    // braces: `{ x }`, but `{}`
    if prev.kind == Kind::Op && p == "{" {
        return t != "}";
    }
    if cur.kind == Kind::Op && t == "}" {
        return true;
    }
    if cur.kind == Kind::Op && t == "{" {
        return !(prev.kind == Kind::Op && matches!(p, "(" | "[" | "$"));
    }
    // binary operators
    if cur.kind == Kind::Op && (BINARY.contains(&t) || (matches!(t, "-" | "+") && ends_value(prev))) {
        return true;
    }
    if prev.kind == Kind::Op && (BINARY.contains(&p) || matches!(p, "-" | "+")) {
        return true;
    }
    // calls and indexes stick to their operand; `fn(` too
    if cur.kind == Kind::Op && matches!(t, "(" | "[") {
        if ends_value(prev) || (prev.kind == Kind::Word && p == "fn") {
            return false;
        }
        if is_keyword(prev) {
            return true;
        }
    }
    // words, numbers and strings next to each other
    if matches!(prev.kind, Kind::Word | Kind::Number | Kind::Str) && matches!(cur.kind, Kind::Word | Kind::Number | Kind::Str) {
        return true;
    }
    // anything else (`<`, `>`, `<-`, `@`…): keep what the author wrote
    cur.space_before
}

/// Formats a source text.
pub fn format_source(src: &str) -> String {
    let pieces = scan(src);
    let mut lines: Vec<Vec<Piece>> = vec![Vec::new()];
    for p in pieces {
        if p.kind == Kind::Newline {
            lines.push(Vec::new());
        } else {
            lines.last_mut().unwrap().push(p);
        }
    }
    let mut out = String::new();
    // indentation of the line where each open bracket was opened
    let mut open: Vec<usize> = Vec::new();
    let mut blank_run = 0;
    let mut wrote_any = false;
    for line in &lines {
        if line.is_empty() {
            blank_run += 1;
            continue;
        }
        // one level inside the innermost open bracket; a line that starts by
        // closing goes back to the level of the line that opened it
        let leading_close = line
            .iter()
            .take_while(|p| p.kind == Kind::Op && matches!(p.text.as_str(), "}" | "]" | ")"))
            .count();
        let base = if leading_close > 0 {
            let at = open.len().saturating_sub(leading_close);
            open.get(at).copied().unwrap_or(0)
        } else {
            open.last().map_or(0, |level| level + 1)
        };
        let continuation = line[0].kind == Kind::Op && matches!(line[0].text.as_str(), "." | "?.");
        let indent = base + usize::from(continuation);
        if wrote_any && blank_run > 0 {
            out.push('\n');
        }
        blank_run = 0;
        // a multi-line comment or template is one piece: its inner lines are kept
        out.push_str(&"    ".repeat(indent));
        for (i, p) in line.iter().enumerate() {
            if i > 0 && p.kind == Kind::Comment {
                // a trailing comment keeps its alignment (at least one space)
                out.push_str(&" ".repeat(p.gap.max(1)));
            } else if i > 0 && spaced(&line[i - 1], p, if i > 1 { Some(&line[i - 2]) } else { None }) {
                out.push(' ');
            }
            out.push_str(&p.text);
            if p.kind == Kind::Op {
                match p.text.as_str() {
                    "{" | "[" | "(" => open.push(indent),
                    "}" | "]" | ")" => {
                        open.pop();
                    }
                    _ => {}
                }
            }
        }
        out.push('\n');
        wrote_any = true;
    }
    out
}

// ---------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------

fn strip_tok(t: &Token) -> Token {
    match t {
        Token::Template(parts) => Token::Template(
            parts
                .iter()
                .map(|p| match p {
                    TplPart::Code(toks) => TplPart::Code(toks.iter().map(|s| Spanned { tok: strip_tok(&s.tok), line: 0, col: 0 }).collect()),
                    other => other.clone(),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Tokens with "starts a line" flags — what the parser may depend on.
fn shape(src: &str) -> Result<Vec<(Token, bool)>, String> {
    let toks = Lexer::new(src).tokenize()?;
    let mut prev_line = 0;
    Ok(toks
        .iter()
        .filter(|s| s.tok != Token::Eof)
        .map(|s| {
            let new_line = s.line != prev_line;
            prev_line = s.line;
            (strip_tok(&s.tok), new_line)
        })
        .collect())
}

fn comments(src: &str) -> Vec<String> {
    scan(src).into_iter().filter(|p| p.kind == Kind::Comment).map(|p| p.text).collect()
}

/// The formatted text, or why the file was left alone.
pub fn format_checked(src: &str) -> Result<String, String> {
    let before = shape(src).map_err(|e| format!("syntax error: {}", e))?;
    let out = format_source(src);
    let after = shape(&out).map_err(|e| format!("internal error (please report): the formatted text does not parse: {}", e))?;
    if before != after || comments(src) != comments(&out) {
        let at = before.iter().zip(&after).position(|(a, b)| a != b).unwrap_or(before.len().min(after.len()));
        let show = |v: &[(Token, bool)]| v.get(at).map(|(t, nl)| format!("{:?}{}", t, if *nl { " (new line)" } else { "" })).unwrap_or_else(|| "end".into());
        return Err(format!(
            "internal error (please report): formatting would change the program at token #{} ({} -> {}); file left as is",
            at,
            show(&before),
            show(&after)
        ));
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(rd) = std::fs::read_dir(path) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if p.is_dir() && (name.starts_with('.') || matches!(name.as_str(), "target" | "dist" | "node_modules" | "fogu_modules")) {
                continue;
            }
            collect(&p, out);
        }
    } else if path.extension().is_some_and(|e| e == "fg") {
        out.push(path.to_path_buf());
    }
}

pub fn fmt(args: &[String]) -> i32 {
    let check = args.iter().any(|a| a == "--check");
    let targets: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if targets.iter().any(|t| t.as_str() == "-") {
        use std::io::Read;
        let mut src = String::new();
        if std::io::stdin().read_to_string(&mut src).is_err() {
            ui::error("cannot read stdin");
            return 1;
        }
        return match format_checked(&src) {
            Ok(out) => {
                print!("{}", out);
                0
            }
            Err(e) => {
                ui::error(&e);
                1
            }
        };
    }
    let mut files = Vec::new();
    if targets.is_empty() {
        collect(Path::new("."), &mut files);
    } else {
        for t in &targets {
            let p = Path::new(t.as_str());
            if !p.exists() {
                ui::error(&format!("{}: no such file or folder", t));
                return 1;
            }
            collect(p, &mut files);
        }
    }
    let (mut changed, mut failed) = (0, 0);
    for f in &files {
        let shown = f.to_string_lossy().replace('\\', "/");
        let shown = shown.strip_prefix("./").unwrap_or(&shown).to_string();
        let raw = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                ui::error(&format!("{}: {}", shown, e));
                failed += 1;
                continue;
            }
        };
        // keep the file's line endings
        let crlf = raw.contains("\r\n");
        let src = raw.replace("\r\n", "\n");
        match format_checked(&src) {
            Ok(out) => {
                if out == src {
                    continue;
                }
                changed += 1;
                if check {
                    println!("{}", shown);
                } else {
                    let data = if crlf { out.replace('\n', "\r\n") } else { out };
                    if let Err(e) = std::fs::write(f, data) {
                        ui::error(&format!("{}: {}", shown, e));
                        failed += 1;
                        continue;
                    }
                    println!("{} {}", ui::green("formatted"), shown);
                }
            }
            Err(e) => {
                ui::error(&format!("{}: {}", shown, e));
                failed += 1;
            }
        }
    }
    if check {
        if changed > 0 {
            println!("{}", ui::dim(&format!("{} of {} file(s) need formatting", changed, files.len())));
            return 1;
        }
        println!("{}", ui::dim(&format!("{} file(s) already formatted", files.len())));
    } else if changed == 0 && failed == 0 {
        println!("{}", ui::dim(&format!("{} file(s) already formatted", files.len())));
    }
    i32::from(failed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(s: &str) -> String {
        format_checked(s).unwrap()
    }

    #[test]
    fn spacing() {
        assert_eq!(f("let x=1+2*3\n"), "let x = 1 + 2 * 3\n");
        assert_eq!(f("print( a ,b )\n"), "print(a, b)\n");
        assert_eq!(f("const m={a:1,b:[1,2]}\n"), "const m = { a: 1, b: [1, 2] }\n");
        assert_eq!(f("const e = {}\nconst n = -1\nconst d = a - -b\n"), "const e = {}\nconst n = -1\nconst d = a - -b\n");
        assert_eq!(f("const r = f(x)?.y ?? 0\nfor i in 0..10 { g(i) }\n"), "const r = f(x)?.y ?? 0\nfor i in 0..10 { g(i) }\n");
        assert_eq!(f("const g = (a, b) => a+b\nconst h = fn(x) { return !x }\n"), "const g = (a, b) => a + b\nconst h = fn(x) { return !x }\n");
    }

    #[test]
    fn indentation_and_blank_lines() {
        let src = "fn f(x) {\nif x {\n  return [\n1,\n2,\n  ]\n}\n\n\n\n  return nil   \n}\n\n\n";
        assert_eq!(f(src), "fn f(x) {\n    if x {\n        return [\n            1,\n            2,\n        ]\n    }\n\n    return nil\n}\n");
        assert_eq!(f("app\n.get(\"/\")\n.listen(80)\n"), "app\n    .get(\"/\")\n    .listen(80)\n");
        // two brackets opened on one line indent once
        assert_eq!(
            f("app.get(\"/\", fn(ctx) {\nreturn web.json({\nok: true\n})\n})\n"),
            "app.get(\"/\", fn(ctx) {\n    return web.json({\n        ok: true\n    })\n})\n"
        );
    }

    #[test]
    fn strings_and_comments_are_kept() {
        let src = "const s = \"a  ,b\"   // keep  this\nconst t = `x ${ a+b } y`\n/* block\n   comment */\n";
        assert_eq!(f(src), "const s = \"a  ,b\"   // keep  this\nconst t = `x ${ a+b } y`\n/* block\n   comment */\n");
        assert_eq!(f(&f(src)), f(src));
    }
}
