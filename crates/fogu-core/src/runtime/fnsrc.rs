//! Where the source text of each function is — for `sync.Worker(fn)`, which
//! sends a function's *source* to another thread (a function value cannot
//! cross threads: it lives in its thread's heap, with the environment it
//! captured).
//!
//! The parser notes where every function starts and ends ([`Span`]); the
//! loaders of files (the main program, imported modules) register those spans
//! with the file's path, keyed by the function's body (`Block`, an `Rc` shared
//! by every closure made from that definition). Nothing is kept for code that
//! is not in a file (`-e`, `fogu:vm`, the REPL): its functions answer `None`.
//! The text itself is read back from the file only when it is asked for, so
//! this costs a few words per function definition and nothing per call.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::ast::Block;

/// From the first token of a function to the first token after it (line and
/// column as the lexer counts them: from 1, in chars). `end == None`: the
/// function runs to the end of the text.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub start: (u32, u32),
    pub end: Option<(u32, u32)>,
}

thread_local! {
    /// Function body (its address) → the file and the span of its definition.
    static TABLE: RefCell<FxHashMap<usize, (Rc<Path>, Span)>> = RefCell::new(FxHashMap::default());
}

fn key(body: &Block) -> usize {
    Rc::as_ptr(body) as usize
}

/// Registers the functions a parser found in the file `path`.
pub fn register(path: &Path, spans: Vec<(Block, Span)>) {
    if spans.is_empty() {
        return;
    }
    // a path that stays valid if the program changes its working folder
    // (files of an executable's bundle are read from the bundle as they are)
    let path: Rc<Path> = if super::vfs::contains(path) {
        Rc::from(path)
    } else {
        Rc::from(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).as_path())
    };
    TABLE.with(|t| {
        let mut t = t.borrow_mut();
        for (body, span) in spans {
            // the entry keeps no reference to the body: the address is only a key
            t.insert(key(&body), (Rc::clone(&path), span));
        }
    });
}

/// The source text of the function whose body is `body`, if it was defined in
/// a file that can still be read.
pub fn source(body: &Block) -> Option<String> {
    let (path, span) = TABLE.with(|t| t.borrow().get(&key(body)).cloned())?;
    let text = match super::vfs::read(&path) {
        Some(t) => t,
        None => std::fs::read_to_string(&*path).ok()?,
    };
    Some(slice(&text, span))
}

/// The file a function was defined in (for messages).
pub fn file(body: &Block) -> Option<PathBuf> {
    TABLE.with(|t| t.borrow().get(&key(body)).map(|(p, _)| p.to_path_buf()))
}

/// The text of `span` in `text`, without the whitespace around it.
fn slice(text: &str, span: Span) -> String {
    let offset = |(line, col): (u32, u32)| -> usize {
        let mut l = 1u32;
        let mut c = 1u32;
        for (i, ch) in text.char_indices() {
            if l == line && c == col {
                return i;
            }
            if ch == '\n' {
                l += 1;
                c = 1;
            } else {
                c += 1;
            }
        }
        text.len()
    };
    let start = offset(span.start);
    let end = span.end.map(offset).unwrap_or(text.len()).max(start);
    let s = text[start..end].trim_end();
    // a statement separator that belongs to what follows
    s.strip_suffix(';').unwrap_or(s).trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_by_line_and_column() {
        let text = "const a = 1\nfn add(x, y) {\n    return x + y\n}\nprint(1)\n";
        let s = slice(text, Span { start: (2, 1), end: Some((5, 1)) });
        assert_eq!(s, "fn add(x, y) {\n    return x + y\n}");
        let s = slice("let f = fn(x) { return x }; print(f(1))", Span { start: (1, 9), end: Some((1, 29)) });
        assert_eq!(s, "fn(x) { return x }");
        let s = slice("x => 1", Span { start: (1, 1), end: None });
        assert_eq!(s, "x => 1");
        // columns count chars, not bytes
        let s = slice("const é = 1; fn f() { return \"á\" }", Span { start: (1, 14), end: None });
        assert_eq!(s, "fn f() { return \"á\" }");
    }
}
