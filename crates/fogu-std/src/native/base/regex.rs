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
        Ok(NativeObjectRef::new(RegexValue { re }).to_value())
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
    let re = compile(&str_arg(a, 1, &what)?, None)?;
    let mut rest = vec![Value::Str(text.into())];
    rest.extend(a[2..].iter().cloned());
    RegexValue { re }.call(op, &rest)
}

fn compile(pattern: &str, opts: Option<&Value>) -> Result<Regex, String> {
    let mut b = RegexBuilder::new(pattern);
    if let Some(Value::Map(o)) = opts {
        let o = o.borrow();
        let flag = |k: &str| o.get(k).is_some_and(|v| v.is_truthy());
        b.case_insensitive(flag("ignoreCase"))
            .multi_line(flag("multiline"))
            .dot_matches_new_line(flag("dotAll"))
            .ignore_whitespace(flag("verbose"))
            .unicode(!o.get("unicode").is_some_and(|v| !v.is_truthy()));
    }
    b.build().map_err(|e| io_err!("regex: invalid pattern: {}", e.to_string().lines().last().unwrap_or("")))
}

pub struct RegexValue {
    re: Regex,
}

/// byte offset → character index
fn char_index(s: &str, byte: usize) -> i64 {
    s[..byte].chars().count() as i64
}

fn match_value(text: &str, caps: &::regex::Captures, re: &Regex) -> Value {
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
        ("start", Value::Int(char_index(text, whole.start()))),
        ("end", Value::Int(char_index(text, whole.end()))),
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
                Ok(Value::Bool(self.re.find(&t).is_some_and(|m| m.start() == 0 && m.end() == t.len())))
            }
            "find" => {
                max_args(args, 1, "find")?;
                let t = text()?;
                Ok(self.re.captures(&t).map_or(Value::Nil, |c| match_value(&t, &c, &self.re)))
            }
            "findAll" => {
                max_args(args, 1, "findAll")?;
                let t = text()?;
                Ok(list(self.re.captures_iter(&t).map(|c| match_value(&t, &c, &self.re)).collect()))
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
