//! Built-in methods of list / map / str and equivalent global functions
//! (`xs.push(1)` and `push(xs, 1)`), plus `error` and `panic`.

use std::cell::RefCell;
use std::cmp::Ordering;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::Interpreter;
use crate::runtime::value::{ErrorValue, Value};

use crate::native::context::NativeContext;

fn list(v: Vec<Value>) -> Value {
    Value::List(Rc::new(RefCell::new(v)))
}

fn idx(v: &Value, len: usize, what: &str) -> Result<usize, String> {
    match v {
        Value::Int(i) => {
            let i = if *i < 0 { *i + len as i64 } else { *i };
            if i < 0 || i as usize > len {
                Err(format!("{}: index {} out of bounds (length {})", what, i, len))
            } else {
                Ok(i as usize)
            }
        }
        o => Err(format!("{}: index must be int, got '{}'", what, o.type_name())),
    }
}

fn need(args: &[Value], n: usize, name: &str) -> Result<(), String> {
    if args.len() < n {
        Err(format!("'{}' expects at least {} argument(s)", name, n))
    } else {
        Ok(())
    }
}

fn eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => (*x as f64) == *y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::List(x), Value::List(y)) => {
            Rc::ptr_eq(x, y) || {
                let (x, y) = (x.borrow(), y.borrow());
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| eq(a, b))
            }
        }
        (Value::Map(x), Value::Map(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

fn natural(a: &Value, b: &Value) -> Result<Ordering, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            let f = |v: &Value| match v {
                Value::Int(i) => *i as f64,
                Value::Float(f) => *f,
                _ => 0.0,
            };
            Ok(f(a).partial_cmp(&f(b)).unwrap_or(Ordering::Equal))
        }
        _ => Err(format!(
            "sort: cannot compare '{}' with '{}'",
            a.type_name(),
            b.type_name()
        )),
    }
}

fn call_fn(it: Option<&Interpreter>, f: &Value, args: Vec<Value>, name: &str) -> Result<Value, String> {
    match it {
        Some(it) => it.call_value(f.clone(), args, it.globals_ref().clone()),
        None => Err(format!("'{}' with a function can only be used as a method", name)),
    }
}

/// Dispatches `recv.name(args)`. `None` if the method does not exist.
pub fn call_method(
    it: &Interpreter,
    recv: &Value,
    name: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    dispatch(Some(it), recv, name, args)
}

fn dispatch(
    it: Option<&Interpreter>,
    recv: &Value,
    name: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match recv {
        Value::List(l) => list_method(it, l, recv, name, args),
        Value::Map(m) => map_method(m, name, args),
        Value::Buffer(b) => crate::native::buffer::method(b, name, args),
        Value::Str(s) if name == "len" => Some(Ok(Value::Int(s.char_len() as i64))),
        Value::Str(s) => str_method(s, name, args),
        _ => None,
    }
}

fn list_method(
    it: Option<&Interpreter>,
    l: &Rc<RefCell<Vec<Value>>>,
    recv: &Value,
    name: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    let r = (|| -> Result<Value, String> {
        Ok(match name {
            "len" => Value::Int(l.borrow().len() as i64),
            "push" => {
                l.borrow_mut().extend(args.iter().cloned());
                recv.clone()
            }
            "pop" => l.borrow_mut().pop().unwrap_or(Value::Nil),
            "shift" => {
                let mut b = l.borrow_mut();
                if b.is_empty() { Value::Nil } else { b.remove(0) }
            }
            "unshift" => {
                let mut b = l.borrow_mut();
                for (i, v) in args.iter().enumerate() {
                    b.insert(i, v.clone());
                }
                drop(b);
                recv.clone()
            }
            "insert" => {
                need(args, 2, name)?;
                let n = l.borrow().len();
                let i = idx(&args[0], n, "insert")?;
                l.borrow_mut().insert(i, args[1].clone());
                recv.clone()
            }
            "remove" => {
                need(args, 1, name)?;
                let n = l.borrow().len();
                let i = idx(&args[0], n, "remove")?;
                if i >= n {
                    return Err(format!("remove: index {} out of bounds (length {})", i, n));
                }
                l.borrow_mut().remove(i)
            }
            "clear" => {
                l.borrow_mut().clear();
                Value::Nil
            }
            "first" => l.borrow().first().cloned().unwrap_or(Value::Nil),
            "last" => l.borrow().last().cloned().unwrap_or(Value::Nil),
            "contains" => {
                need(args, 1, name)?;
                Value::Bool(l.borrow().iter().any(|v| eq(v, &args[0])))
            }
            "index_of" => {
                need(args, 1, name)?;
                Value::Int(
                    l.borrow().iter().position(|v| eq(v, &args[0])).map(|i| i as i64).unwrap_or(-1),
                )
            }
            "join" => {
                let sep = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    None => String::new().into(),
                    Some(o) => return Err(format!("join: separator must be str, got '{}'", o.type_name())),
                };
                // one buffer; strings are copied as they are
                let items = l.borrow();
                let mut out = String::with_capacity(items.iter().map(|v| if let Value::Str(s) = v { s.len() } else { 8 }).sum::<usize>() + sep.len() * items.len());
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(&sep);
                    }
                    match v {
                        Value::Str(s) => out.push_str(s),
                        other => out.push_str(&other.to_display_string()),
                    }
                }
                Value::Str(out.into())
            }
            "reverse" => {
                let mut v = l.borrow().clone();
                v.reverse();
                list(v)
            }
            "slice" => {
                let n = l.borrow().len();
                let a = match args.first() { Some(v) => idx(v, n, "slice")?, None => 0 };
                let b = match args.get(1) { Some(v) => idx(v, n, "slice")?, None => n };
                list(if a >= b { vec![] } else { l.borrow()[a..b].to_vec() })
            }
            "concat" => {
                let mut v = l.borrow().clone();
                for a in args {
                    match a {
                        Value::List(o) => v.extend(o.borrow().iter().cloned()),
                        o => v.push(o.clone()),
                    }
                }
                list(v)
            }
            "sort" => {
                let mut v = l.borrow().clone();
                let cmp = args.first().cloned();
                // the natural order of plain numbers or strings: the standard sorts
                if cmp.is_none() {
                    if v.iter().all(|x| matches!(x, Value::Int(_))) {
                        let mut ints: Vec<i64> = v.iter().map(|x| if let Value::Int(i) = x { *i } else { 0 }).collect();
                        ints.sort_unstable();
                        return Ok(list(ints.into_iter().map(Value::Int).collect()));
                    }
                    if v.iter().all(|x| matches!(x, Value::Str(_))) {
                        v.sort_by(|a, b| match (a, b) {
                            (Value::Str(x), Value::Str(y)) => x.cmp(y),
                            _ => Ordering::Equal,
                        });
                        return Ok(list(v));
                    }
                    let num = |x: &Value| match x {
                        Value::Int(i) => Some(*i as f64),
                        Value::Float(f) if !f.is_nan() => Some(*f),
                        _ => None,
                    };
                    if v.iter().all(|x| num(x).is_some()) {
                        // no NaN: a total order, so the stable std sort is safe
                        v.sort_by(|a, b| num(a).partial_cmp(&num(b)).unwrap_or(Ordering::Equal));
                        return Ok(list(v));
                    }
                }
                // `less(a, b)`: the comparator's answer (a bool, or a number < 0), or the natural order
                let mut less = |a: &Value, b: &Value| -> Result<bool, String> {
                    match &cmp {
                        None => natural(a, b).map(|o| o == Ordering::Less),
                        Some(f) => call_fn(it, f, vec![a.clone(), b.clone()], name).map(|r| match r {
                            Value::Bool(b) => b,
                            Value::Int(i) => i < 0,
                            Value::Float(f) => f < 0.0,
                            _ => false,
                        }),
                    }
                };
                list(merge_sort(v, &mut less)?)
            }
            "map" | "filter" | "each" | "find" | "any" | "all" | "reduce" => {
                need(args, 1, name)?;
                let items = l.borrow().clone();
                let f = &args[0];
                match name {
                    "map" => {
                        let mut o = Vec::with_capacity(items.len());
                        for x in items { o.push(call_fn(it, f, vec![x], name)?); }
                        list(o)
                    }
                    "filter" => {
                        let mut o = Vec::new();
                        for x in items { if call_fn(it, f, vec![x.clone()], name)?.is_truthy() { o.push(x); } }
                        list(o)
                    }
                    "each" => {
                        for (i, x) in items.into_iter().enumerate() { call_fn(it, f, vec![x, Value::Int(i as i64)], name)?; }
                        Value::Nil
                    }
                    "find" => {
                        for x in items { if call_fn(it, f, vec![x.clone()], name)?.is_truthy() { return Ok(x); } }
                        Value::Nil
                    }
                    "any" => {
                        for x in items { if call_fn(it, f, vec![x], name)?.is_truthy() { return Ok(Value::Bool(true)); } }
                        Value::Bool(false)
                    }
                    "all" => {
                        for x in items { if !call_fn(it, f, vec![x], name)?.is_truthy() { return Ok(Value::Bool(false)); } }
                        Value::Bool(true)
                    }
                    _ => {
                        let mut iter = items.into_iter();
                        let mut acc = match args.get(1) {
                            Some(v) => v.clone(),
                            None => iter.next().ok_or("reduce of an empty list without an initial value")?,
                        };
                        for x in iter { acc = call_fn(it, f, vec![acc, x], name)?; }
                        acc
                    }
                }
            }
            _ => return Err(String::from("\u{0}nomethod")),
        })
    })();
    match r {
        Err(e) if e == "\u{0}nomethod" => None,
        other => Some(other),
    }
}

fn map_method(
    m: &Rc<RefCell<HashMap<String, Value>>>,
    name: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    let key = |args: &[Value]| -> Result<String, String> {
        match args.first() {
            Some(Value::Str(s)) => Ok(s.to_string()),
            Some(o) => Err(format!("map key must be str, got '{}'", o.type_name())),
            None => Err(format!("'{}' expects a key", name)),
        }
    };
    let sorted = || {
        let b = m.borrow();
        let mut ks: Vec<String> = b.keys().cloned().collect();
        ks.sort();
        ks
    };
    Some((|| -> Result<Value, String> {
        Ok(match name {
            "len" => Value::Int(m.borrow().len() as i64),
            "keys" => list(sorted().into_iter().map(Value::str).collect()),
            "values" => {
                let b = m.borrow();
                list(sorted().iter().map(|k| b[k].clone()).collect())
            }
            "entries" => {
                let b = m.borrow();
                list(sorted().iter().map(|k| list(vec![Value::Str(k.clone().into()), b[k].clone()])).collect())
            }
            "has" => Value::Bool(m.borrow().contains_key(&key(args)?)),
            "get" => m.borrow().get(&key(args)?).cloned().unwrap_or_else(|| args.get(1).cloned().unwrap_or(Value::Nil)),
            "set" => {
                need(args, 2, name)?;
                m.borrow_mut().insert(key(args)?, args[1].clone());
                Value::Map(Rc::clone(m))
            }
            "remove" => m.borrow_mut().remove(&key(args)?).unwrap_or(Value::Nil),
            "clear" => { m.borrow_mut().clear(); Value::Nil }
            "merge" => {
                let mut out = m.borrow().clone();
                for a in args {
                    if let Value::Map(o) = a {
                        for (k, v) in o.borrow().iter() { out.insert(k.clone(), v.clone()); }
                    } else {
                        return Err("merge expects maps".into());
                    }
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
            _ => return Err(String::from("\u{0}nomethod")),
        })
    })())
    .and_then(|r| match r { Err(e) if e == "\u{0}nomethod" => None, o => Some(o) })
}

fn str_arg<'a>(args: &'a [Value], i: usize, name: &str) -> Result<&'a str, String> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s),
        Some(o) => Err(format!("'{}': argument {} must be str, got '{}'", name, i + 1, o.type_name())),
        None => Err(format!("'{}' expects more arguments", name)),
    }
}

fn str_method(s: &str, name: &str, args: &[Value]) -> Option<Result<Value, String>> {
    Some((|| -> Result<Value, String> {
        Ok(match name {
            "len" => Value::Int(s.chars().count() as i64),
            "upper" => Value::Str(s.to_uppercase().into()),
            "lower" => Value::Str(s.to_lowercase().into()),
            "trim" => Value::Str(s.trim().to_string().into()),
            "split" => {
                let sep = str_arg(args, 0, name)?;
                if sep.is_empty() {
                    list(s.chars().map(|c| Value::Str(c.to_string().into())).collect())
                } else {
                    list(s.split(sep).map(|p| Value::Str(p.to_string().into())).collect())
                }
            }
            "lines" => list(s.lines().map(|p| Value::Str(p.to_string().into())).collect()),
            "chars" => list(s.chars().map(|c| Value::Str(c.to_string().into())).collect()),
            "contains" => Value::Bool(s.contains(str_arg(args, 0, name)?)),
            "starts_with" => Value::Bool(s.starts_with(str_arg(args, 0, name)?)),
            "ends_with" => Value::Bool(s.ends_with(str_arg(args, 0, name)?)),
            "index_of" => {
                let n = str_arg(args, 0, name)?;
                Value::Int(s.find(n).map(|b| s[..b].chars().count() as i64).unwrap_or(-1))
            }
            "replace" => Value::Str(s.replace(str_arg(args, 0, name)?, str_arg(args, 1, name)?).into()),
            "repeat" => match args.first() {
                Some(Value::Int(n)) if *n >= 0 => Value::Str(s.repeat(*n as usize).into()),
                _ => return Err("repeat expects an int >= 0".into()),
            },
            "slice" => {
                let cs: Vec<char> = s.chars().collect();
                let a = match args.first() { Some(v) => idx(v, cs.len(), "slice")?, None => 0 };
                let b = match args.get(1) { Some(v) => idx(v, cs.len(), "slice")?, None => cs.len() };
                Value::Str(if a >= b { String::new().into() } else { cs[a..b].iter().collect::<String>().into() })
            }
            _ => return Err(String::from("\u{0}nomethod")),
        })
    })())
    .and_then(|r| match r { Err(e) if e == "\u{0}nomethod" => None, o => Some(o) })
}

macro_rules! global_method {
    ($ctx:expr, $($name:literal),+ $(,)?) => {
        $( $ctx.define_global_variadic($name, 1, |args| {
            match dispatch(None, &args[0], $name, &args[1..]) {
                Some(r) => r,
                None => Err(format!("'{}' is not applicable to '{}'", $name, args[0].type_name())),
            }
        }); )+
    };
}

pub fn install(ctx: &mut NativeContext) {
    global_method!(
        ctx, "push", "pop", "shift", "unshift", "insert", "remove", "clear", "first", "last",
        "contains", "index_of", "join", "reverse", "slice", "concat", "keys", "values",
        "entries", "has", "get", "merge", "upper", "lower", "trim", "split", "lines", "chars",
        "starts_with", "ends_with", "replace", "repeat"
    );

    // error(msg, cause?) — spec §29.
    ctx.define_global_variadic("error", 1, |args| {
        let message = args[0].to_display_string();
        let cause = args.get(1).cloned().unwrap_or(Value::Nil);
        Ok(Value::Error(Rc::new(ErrorValue { message, cause, trace: Vec::new() })))
    });

    // panic(msg) — unrecoverable failure (spec §31).
    ctx.define_global_variadic("panic", 0, |args| {
        let msg: Vec<String> = args.iter().map(|v| v.to_display_string()).collect();
        Err(format!("panic: {}", msg.join(" ")))
    });
}

/// A stable merge sort (O(n log n)) with a comparator that may fail; unlike the
/// standard library's sort it never panics when the comparator is inconsistent.
fn merge_sort(v: Vec<Value>, less: &mut dyn FnMut(&Value, &Value) -> Result<bool, String>) -> Result<Vec<Value>, String> {
    let n = v.len();
    if n < 2 {
        return Ok(v);
    }
    let mut src = v;
    // short runs first: a binary insertion sort of each block of 16
    const RUN: usize = 16;
    for start in (0..n).step_by(RUN) {
        let end = (start + RUN).min(n);
        for i in start + 1..end {
            let (mut lo, mut hi) = (start, i);
            while lo < hi {
                let mid = (lo + hi) / 2;
                if less(&src[i], &src[mid])? { hi = mid } else { lo = mid + 1 }
            }
            src[lo..=i].rotate_right(1);
        }
    }
    let mut buf: Vec<Value> = Vec::with_capacity(n);
    let mut width = RUN;
    while width < n {
        buf.clear();
        let mut i = 0;
        while i < n {
            let mid = (i + width).min(n);
            let end = (i + 2 * width).min(n);
            let (mut a, mut b) = (i, mid);
            while a < mid && b < end {
                // stable: the right side goes first only when strictly smaller
                if less(&src[b], &src[a])? {
                    buf.push(src[b].clone());
                    b += 1;
                } else {
                    buf.push(src[a].clone());
                    a += 1;
                }
            }
            buf.extend_from_slice(&src[a..mid]);
            buf.extend_from_slice(&src[b..end]);
            i = end;
        }
        std::mem::swap(&mut src, &mut buf);
        width *= 2;
    }
    Ok(src)
}
