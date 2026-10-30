//! Readable diagnostics (spec §58): error code, position, source excerpt
//! with a marker and a suggestion when one is known.
//!
//! ```text
//! error[E0201]: division by zero
//!   --> main.fg:3:12
//!    |
//!  3 |     return a / b
//!    |     ^^^^^^^^^^^^
//!    = help: check the divisor before dividing
//! ```

use super::ui;

/// An error split into its parts.
#[derive(Debug, PartialEq)]
pub struct Diag {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub msg: String,
    /// `syntax`, `lexical`, `name`, `type` or `runtime`.
    pub kind: Option<&'static str>,
    pub help: Vec<String>,
}

/// Splits the messages produced by the lexer, parser, resolver and
/// interpreter:
///
/// * `file: line L:C: syntax error: msg`
/// * `file: line L: name error: msg\n  help: ...`
/// * `file:L: runtime error: msg`
pub fn parse(err: &str) -> Diag {
    let mut lines = err.lines();
    let first = lines.next().unwrap_or("");
    let mut help: Vec<String> = Vec::new();
    for l in lines {
        if let Some(h) = l.trim().strip_prefix("help:") {
            help.push(h.trim().to_string());
        }
    }

    let mut d = Diag {
        file: None,
        line: None,
        col: None,
        msg: first.to_string(),
        kind: None,
        help,
    };

    // runtime errors first: their message may itself contain ": line "
    if let Some(idx) = first.find(": runtime error: ") {
        let loc = &first[..idx];
        d.msg = first[idx + ": runtime error: ".len()..].to_string();
        d.kind = Some("runtime");
        match loc.rsplit_once(':') {
            Some((file, l)) if l.parse::<u32>().is_ok() => {
                d.file = real_file(file);
                d.line = l.parse().ok().filter(|&n| n > 0);
            }
            _ => d.file = real_file(loc),
        }
    } else if let Some(idx) = first.find(": line ") {
        // file: line L[:C]: <kind>: msg
        let file = &first[..idx];
        let rest = &first[idx + ": line ".len()..];
        let (pos, after) = rest.split_once(": ").unwrap_or((rest, ""));
        let mut nums = pos.split(':').map(|n| n.parse::<u32>().ok());
        d.line = nums.next().flatten();
        d.col = nums.next().flatten();
        (d.kind, d.msg) = split_kind(after);
        d.file = real_file(file);
    } else if let Some(rest) = first.strip_prefix("line ") {
        // without a file (e.g. an error inside a module that already has a prefix)
        let (pos, after) = rest.split_once(": ").unwrap_or((rest, ""));
        let mut nums = pos.split(':').map(|n| n.parse::<u32>().ok());
        d.line = nums.next().flatten();
        d.col = nums.next().flatten();
        (d.kind, d.msg) = split_kind(after);
    }
    d
}

/// `syntax error: msg` → (`syntax`, `msg`).
fn split_kind(s: &str) -> (Option<&'static str>, String) {
    for (prefix, kind) in [
        ("lexical error: ", "lexical"),
        ("syntax error: ", "syntax"),
        ("name error: ", "name"),
        ("type error: ", "type"),
    ] {
        if let Some(m) = s.strip_prefix(prefix) {
            return (Some(kind), m.to_string());
        }
    }
    (None, s.to_string())
}

/// `<eval>`, `<print>` and `<repl>` are not files.
fn real_file(f: &str) -> Option<String> {
    let f = f.trim();
    if f.is_empty() || f.starts_with('<') {
        None
    } else {
        Some(f.to_string())
    }
}

/// Code and suggestion for the known messages.
fn classify(msg: &str, kind: Option<&str>) -> (Option<&'static str>, Option<&'static str>) {
    const TABLE: &[(&str, &str, Option<&str>)] = &[
        ("used before its declaration", "E0101", None),
        ("is not defined", "E0100", None),
        ("is already declared in this scope", "E0102", Some("use another name, or a { } block to create a new scope")),
        ("const binding", "E0103", Some("declare it with 'let' if the value has to change")),
        ("is private", "E0104", Some("names starting with '_' are only accessible in the module where they are declared")),
        ("integer overflow", "E0200", Some("ints are 64-bit; use float for larger values")),
        ("division by zero", "E0201", Some("check the divisor before dividing")),
        ("out of bounds", "E0202", Some("use len(...) to check the length, or negative indices from the end")),
        ("non-exhaustive match", "E0203", Some("add a '_ => ...' arm for the remaining values")),
        ("is not callable", "E0204", None),
        ("stack overflow", "E0205", Some("the recursion is too deep; use a loop")),
        ("outside a function", "E0206", Some("'?' returns the error to the caller; at the top level handle it with 'const [v, err] = ...'")),
        ("type mismatch", "E0207", Some("the value does not match the type annotation")),
        ("unknown type", "E0208", None),
        ("has no error", "E0206", None),
        ("deadlock", "E0300", Some("some channel never receives or sends; use close(), a buffer, or select with default/timeout")),
    ];
    for (needle, code, help) in TABLE {
        if msg.contains(needle) {
            return (Some(code), *help);
        }
    }
    match kind {
        Some("lexical") => (Some("E0001"), None),
        Some("syntax") => (Some("E0002"), None),
        _ => (None, None),
    }
}

/// Position (start, length) to underline in a line of code.
fn underline(src_line: &str, col: Option<u32>, msg: &str) -> (usize, usize) {
    let chars: Vec<char> = src_line.chars().collect();
    if let Some(c) = col {
        let start = (c as usize).saturating_sub(1).min(chars.len());
        let len = chars[start..]
            .iter()
            .take_while(|c| c.is_alphanumeric() || **c == '_')
            .count()
            .max(1);
        return (start, len);
    }
    // quoted name in the message (e.g. variable 'x') present in the line
    if let Some(name) = msg.split('\'').nth(1).filter(|n| !n.is_empty()) {
        let name: Vec<char> = name.chars().collect();
        let is_word = |c: &char| c.is_alphanumeric() || *c == '_';
        // only as a whole word: `n` must not match inside `print`
        for start in 0..(chars.len() + 1).saturating_sub(name.len()) {
            let end = start + name.len();
            if chars[start..end] == name[..]
                && (start == 0 || !is_word(&chars[start - 1]))
                && (end == chars.len() || !is_word(&chars[end]))
            {
                return (start, name.len());
            }
        }
    }
    let start = chars.iter().take_while(|c| c.is_whitespace()).count();
    let end = chars.len() - chars.iter().rev().take_while(|c| c.is_whitespace()).count();
    (start, end.saturating_sub(start).max(1))
}

/// Full message, ready to write to stderr.
pub fn render(err: &str) -> String {
    let d = parse(err);
    let (code, known_help) = classify(&d.msg, d.kind);
    let head = match code {
        Some(c) => format!("error[{}]", c),
        None => "error".to_string(),
    };
    let mut out = format!("{} {}", ui::red_err(&format!("{}:", head)), ui::bold_err(&d.msg));

    if let Some(file) = &d.file {
        let pos = match (d.line, d.col) {
            (Some(l), Some(c)) => format!("{}:{}:{}", file, l, c),
            (Some(l), None) => format!("{}:{}", file, l),
            _ => file.clone(),
        };
        out.push_str(&format!("\n  {} {}", ui::cyan_err("-->"), pos));

        let src_line = d.line.and_then(|l| {
            std::fs::read_to_string(file)
                .ok()?
                .lines()
                .nth(l as usize - 1)
                .map(|s| s.replace('\t', "    "))
        });
        if let (Some(l), Some(src)) = (d.line, src_line) {
            let num = l.to_string();
            let pad = " ".repeat(num.len());
            let (start, len) = underline(&src, d.col, &d.msg);
            out.push_str(&format!("\n {} {}", pad, ui::cyan_err("|")));
            out.push_str(&format!("\n {} {} {}", ui::cyan_err(&num), ui::cyan_err("|"), src));
            out.push_str(&format!(
                "\n {} {} {}{}",
                pad,
                ui::cyan_err("|"),
                " ".repeat(start),
                ui::red_err(&"^".repeat(len))
            ));
        }
    }

    let mut helps = d.help.clone();
    if helps.is_empty() {
        helps.extend(known_help.map(str::to_string));
    }
    for h in helps {
        out.push_str(&format!("\n   {} {}", ui::cyan_err("= help:"), h));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_syntax_error() {
        let d = parse("main.fg: line 3:7: syntax error: expected ')'");
        assert_eq!(d.file.as_deref(), Some("main.fg"));
        assert_eq!((d.line, d.col), (Some(3), Some(7)));
        assert_eq!(d.msg, "expected ')'");
        assert_eq!(d.kind, Some("syntax"));
    }

    #[test]
    fn splits_a_runtime_error_with_help() {
        let d = parse("a\\b.fg:12: runtime error: division by zero");
        assert_eq!(d.file.as_deref(), Some("a\\b.fg"));
        assert_eq!(d.line, Some(12));
        assert_eq!(d.msg, "division by zero");
        let d = parse("x.fg: line 2: name error: variable 'y' is not defined\n  help: did you mean 'z'?");
        assert_eq!(d.help, vec!["did you mean 'z'?".to_string()]);
    }

    #[test]
    fn underlines_the_quoted_name() {
        assert_eq!(underline("print(usr.name)", None, "variable 'usr' is not defined"), (6, 3));
        assert_eq!(underline("  return a / b", None, "division by zero"), (2, 12));
        assert_eq!(underline("let x = (1 +", Some(9), ""), (8, 1));
        assert_eq!(underline("print(n.x)", None, "parameter 'n' expects"), (6, 1));
    }
}
