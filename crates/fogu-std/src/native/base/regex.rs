//! `fogu:regex` module — regular expressions (Rust `regex` syntax: no
//! backtracking, linear time).
//!
//! ```fogu
//! import regex from "fogu:regex"
//!
//! const re = regex.compile("(?P<user>\\w+)@(?P<host>[\\w.]+)", { ignoreCase: true })?
//! print(re.test("ana@fogu.dev"))                   // true
//! const m = re.find("mail: ana@fogu.dev")          // { text, start, end, groups, named }
//! print(m.named.host)                              // fogu.dev
//! print(re.replaceAll("a@x.io b@y.io", "${user} at ${host}"))
//! print(regex.split("a, b;c", "[,;]\\s*"))          // [a, b, c]
//! ```
//!
//! Positions are character indexes (like `str.index_of`). An invalid
//! pattern is an `error` value (patterns often come from user input).

use std::cell::RefCell;
use ahash::AHashMap as HashMap;

use ::regex::{Regex, RegexBuilder};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{list, map, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("compile".into(), nfn_var("compile", 1, |a| {
        max_args(a, 2, "regex.compile")?;
        let re = compile(&str_arg(a, 0, "regex.compile")?, a.get(1))?;
        Ok(NativeObjectRef::new(RegexValue::new(re)).to_value())
    }));
    e.insert("escape".into(), nfn_var("escape", 1, |a| {
        max_args(a, 1, "regex.escape")?;
        Ok(Value::Str(::regex::escape(&str_arg(a, 0, "regex.escape")?).into()))
    }));
    // shortcuts: regex.test(text, pattern), find, findAll, replace, replaceAll, split
    for name in ["test", "find", "findAll", "replace", "replaceAll", "split", "matches"] {
        e.insert(name.into(), nfn_var(name, 2, shortcut(name)));
    }
    ModuleValue {
        name: "fogu:regex".into(),
        exports: RefCell::new(e),
    }
}

fn shortcut(name: &str) -> fn(&[Value]) -> Result<Value, String> {
    match name {
        "test" => |a| run_short("test", a),
        "find" => |a| run_short("find", a),
        "findAll" => |a| run_short("findAll", a),
        "replace" => |a| run_short("replace", a),
        "replaceAll" => |a| run_short("replaceAll", a),
        "split" => |a| run_short("split", a),
        _ => |a| run_short("matches", a),
    }
}

/// `regex.op(text, pattern, ...rest)` → `compile(pattern).op(text, ...rest)`
fn run_short(op: &str, a: &[Value]) -> Result<Value, String> {
    let what = format!("regex.{}", op);
    let text = str_arg(a, 0, &what)?;
    // a trailing map is the options of the pattern, as in `regex.compile(pattern, opts)`
    let (extra, opts) = match a[2..].split_last() {
        Some((o @ Value::Map(_), rest)) => (rest, Some(o)),
        _ => (&a[2..], None),
    };
    let re = compile(&str_arg(a, 1, &what)?, opts)?;
    let mut rest = vec![Value::Str(text.into())];
    rest.extend(extra.iter().cloned());
    let (max, usage) = match op {
        "replace" | "replaceAll" => (1, "(text, pattern, replacement, opts?)"),
        "split" => (1, "(text, pattern, limit?, opts?)"),
        _ => (0, "(text, pattern, opts?)"),
    };
    if extra.len() > max {
        return Err(format!("{}{} expects at most {} arguments, got {}", what, usage, max + 3, a.len()));
    }
    RegexValue::new(re).call(op, &rest)
}

/// The options of `regex.compile(pattern, opts)`.
#[derive(Clone, Copy)]
struct Flags {
    ignore_case: bool,
    multiline: bool,
    dot_all: bool,
    verbose: bool,
    unicode: bool,
}

fn compile(pattern: &str, opts: Option<&Value>) -> Result<(Regex, Flags), String> {
    let mut f = Flags { ignore_case: false, multiline: false, dot_all: false, verbose: false, unicode: true };
    if let Some(Value::Map(o)) = opts {
        let o = o.borrow();
        let flag = |k: &str| o.get(k).is_some_and(|v| v.is_truthy());
        f = Flags {
            ignore_case: flag("ignoreCase"),
            multiline: flag("multiline"),
            dot_all: flag("dotAll"),
            verbose: flag("verbose"),
            unicode: !o.get("unicode").is_some_and(|v| !v.is_truthy()),
        };
    }
    Ok((build_with(pattern, f)?, f))
}

fn build_with(pattern: &str, f: Flags) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(f.ignore_case)
        .multi_line(f.multiline)
        .dot_matches_new_line(f.dot_all)
        .ignore_whitespace(f.verbose)
        .unicode(f.unicode)
        .build()
        .map_err(|e| io_err!("regex: invalid pattern: {}", e.to_string().lines().last().unwrap_or("")))
}

pub struct RegexValue {
    re: Regex,
    flags: Flags,
    /// `\A(?:pattern)\z` with the same options, built on the first `matches`
    full: Option<Regex>,
}

impl RegexValue {
    fn new((re, flags): (Regex, Flags)) -> Self {
        RegexValue { re, flags, full: None }
    }

    /// Does the whole text match? Not `find` covering the text: leftmost-first stops at the
    /// first alternative (`a|abc` finds `a` in "abc").
    fn full_match(&mut self, t: &str) -> Result<bool, String> {
        if self.full.is_none() {
            // `\A`/`\z`: whole text even with `multiline` (where ^ and $ match at lines);
            // with `verbose` the pattern may end in a comment, hence the newline
            let nl = if self.flags.verbose { "\n" } else { "" };
            let anchored = format!(r"\A(?:{}{})\z", self.re.as_str(), nl);
            self.full = Some(build_with(&anchored, self.flags)?);
        }
        Ok(self.full.as_ref().is_some_and(|f| f.is_match(t)))
    }
}

/// byte offset → character index, for offsets visited in increasing order (all the matches of
/// a text): counting from the start for each one made `findAll` quadratic
struct CharIndex<'a> {
    text: &'a str,
    ascii: bool,
    byte: usize,
    chars: i64,
}

impl<'a> CharIndex<'a> {
    fn new(text: &'a str) -> Self {
        CharIndex { text, ascii: text.is_ascii(), byte: 0, chars: 0 }
    }

    fn at(&mut self, byte: usize) -> i64 {
        if self.ascii {
            return byte as i64;
        }
        if byte < self.byte {
            self.byte = 0;
            self.chars = 0;
        }
        self.chars += self.text[self.byte..byte].chars().count() as i64;
        self.byte = byte;
        self.chars
    }
}

fn match_value(pos: &mut CharIndex, caps: &::regex::Captures, re: &Regex) -> Value {
    let whole = caps.get(0).unwrap();
    let groups: Vec<Value> = (1..caps.len())
        .map(|i| caps.get(i).map_or(Value::Nil, |m| Value::Str(m.as_str().to_string().into())))
        .collect();
    let named: Vec<(&str, Value)> = re
        .capture_names()
        .flatten()
        .map(|n| (n, caps.name(n).map_or(Value::Nil, |m| Value::Str(m.as_str().to_string().into()))))
        .collect();
    map(vec![
        ("text", Value::Str(whole.as_str().to_string().into())),
        ("start", Value::Int(pos.at(whole.start()))),
        ("end", Value::Int(pos.at(whole.end()))),
        ("groups", list(groups)),
        ("named", map(named)),
    ])
}

impl FoguObject for RegexValue {
    fn name(&self) -> &str {
        "Regex"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "source" => Value::Str(self.re.as_str().to_string().into()),
            "groups" => Value::Int(self.re.captures_len() as i64 - 1),
            "names" => crate::native::util::str_list(self.re.capture_names().flatten().map(String::from)),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let text = || str_arg(args, 0, method);
        match method {
            "test" => {
                max_args(args, 1, "test")?;
                Ok(Value::Bool(self.re.is_match(&text()?)))
            }
            // matches(text): does the WHOLE text match?
            "matches" => {
                max_args(args, 1, "matches")?;
                let t = text()?;
                Ok(Value::Bool(self.full_match(&t)?))
            }
            "find" => {
                max_args(args, 1, "find")?;
                let t = text()?;
                Ok(self.re.captures(&t).map_or(Value::Nil, |c| match_value(&mut CharIndex::new(&t), &c, &self.re)))
            }
            "findAll" => {
                max_args(args, 1, "findAll")?;
                let t = text()?;
                let mut pos = CharIndex::new(&t);
                Ok(list(self.re.captures_iter(&t).map(|c| match_value(&mut pos, &c, &self.re)).collect()))
            }
            // replace(text, replacement) — `$1`, `${name}`, `$$` for a literal $
            "replace" | "replaceAll" => {
                max_args(args, 2, method)?;
                let t = text()?;
                let rep = str_arg(args, 1, method)?;
                let out = if method == "replace" { self.re.replace(&t, rep.as_str()) } else { self.re.replace_all(&t, rep.as_str()) };
                Ok(Value::Str(out.into_owned().into()))
            }
            // split(text, limit?)
            "split" => {
                max_args(args, 2, "split")?;
                let t = text()?;
                let parts: Vec<String> = match args.get(1) {
                    Some(Value::Int(n)) if *n > 0 => self.re.splitn(&t, *n as usize).map(String::from).collect(),
                    _ => self.re.split(&t).map(String::from).collect(),
                };
                Ok(crate::native::util::str_list(parts))
            }
            other => Err(format!("method '{}' does not exist on Regex (test matches find findAll replace replaceAll split)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["test", "matches", "find", "findAll", "replace", "replaceAll", "split"]
    }
}
