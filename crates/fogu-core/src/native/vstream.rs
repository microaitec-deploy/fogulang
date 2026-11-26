//! `fogu:stream` — streams of values: a sequence consumed a value at a time,
//! finite or infinite, composed with `map`, `filter`, `take` … See
//! `docs/architecture/value-streams.md`.
//!
//! ```fogu
//! import stream from "fogu:stream"
//!
//! const s = stream.create({ buffer: 8 })     // written by a task, read by another
//! fg fn() {
//!     for i in 0..100 { s.write(i)? }        // waits while 8 values are unread
//!     s.close()
//! }()
//! for v in s.filter((x) => x % 2 == 0).map((x) => x * 10).take(5) { print(v) }
//! ```
//!
//! Streams are **pulled**: an operator does its work when the consumer asks
//! for the next value, so a chain holds one value per stage (no list is
//! built), an infinite source is fine, and the consumer sets the pace —
//! that is the backpressure. The only buffer is the one of
//! `stream.create({ buffer })`, the bridge from tasks that push values.
//!
//! Ending: a source ends (`close()`, the end of a list or range, `nil` from a
//! generator), `fail(err)` or an error value returned by a function of the
//! chain ends it with that error — `for` stops and `s.error` has it; the
//! terminals (`toList`, `reduce` …) return it. `cancel()` stops a stream and
//! everything upstream: a writer of a `create` stream gets an error value
//! instead of waiting, a file or HTTP stream is closed. `take(n)` cancels
//! upstream once it has its `n` values.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use ahash::AHashMap as HashMap;

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::runtime::interpreter::{chan_recv, chan_send, Interpreter, RecvError};
use crate::runtime::sched;
use crate::runtime::value::{ChanValue, ErrorValue, ModuleValue, Value};

/// Where the values of a stream come from, or what it does to its upstream's.
enum Kind {
    /// `stream.create`: values written by tasks, at most `cap` unread.
    Queue { buf: VecDeque<Value>, cap: usize, closed: bool },
    /// `stream.from(list)`, `stream.of(...)`: a snapshot.
    Items(std::vec::IntoIter<Value>),
    Range { next: i64, end: Option<i64>, step: i64 },
    Chan(Rc<ChanValue>),
    /// A readable of `fogu:fs` / `fogu:http` (`for chunk in r`).
    Readable(NativeObjectRef),
    /// `stream.generate(fn)`: `fn(i)` for i = 0, 1, 2 … until it returns nil.
    Generate { f: Value, i: i64 },
    /// `stream.interval(d)`: 0, 1, 2 … one every `d`.
    Interval { every: std::time::Duration, i: i64 },
    Map(Value),
    Filter(Value),
    Take(i64),
    Skip(i64),
    TakeWhile(Value),
    Chunk(usize),
    Tap(Value),
    Enumerate(i64),
}

/// One stage of a stream: its source or operator, its upstream, how it ended.
pub struct Node {
    kind: RefCell<Kind>,
    upstream: Option<Rc<Node>>,
    done: Cell<bool>,
    cancelled: Cell<bool>,
    error: RefCell<Option<Rc<ErrorValue>>>,
}

impl Node {
    fn new(kind: Kind, upstream: Option<Rc<Node>>) -> Rc<Node> {
        Rc::new(Node { kind: RefCell::new(kind), upstream, done: Cell::new(false), cancelled: Cell::new(false), error: RefCell::new(None) })
    }

    fn fail(&self, e: Rc<ErrorValue>) {
        if self.error.borrow().is_none() {
            *self.error.borrow_mut() = Some(e);
        }
        self.done.set(true);
    }

    /// Stops this stage and every stage upstream (sources let go of what they hold).
    fn cancel(&self) {
        if self.cancelled.replace(true) {
            return;
        }
        self.done.set(true);
        match &mut *self.kind.borrow_mut() {
            Kind::Queue { buf, .. } => buf.clear(),
            Kind::Items(it) => *it = Vec::new().into_iter(),
            Kind::Readable(r) => {
                let _ = r.call("close", &[]);
            }
            _ => {}
        }
        if let Some(up) = &self.upstream {
            up.cancel();
        }
    }
}

fn err(msg: impl Into<String>) -> Rc<ErrorValue> {
    Rc::new(ErrorValue { message: msg.into(), cause: Value::Nil, trace: Vec::new() })
}

/// A stream value (`typeof` → "Stream").
pub struct StreamValue {
    pub node: Rc<Node>,
}

pub fn value(node: Rc<Node>) -> Value {
    NativeObjectRef::new(StreamValue { node }).to_value()
}

fn node_of(o: &NativeObjectRef) -> Option<Rc<Node>> {
    let rc = o.inner();
    let b = rc.borrow();
    b.as_any().downcast_ref::<StreamValue>().map(|s| Rc::clone(&s.node))
}

impl FoguObject for StreamValue {
    fn name(&self) -> &str {
        "Stream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let n = &self.node;
        Some(match field {
            "done" => Value::Bool(n.done.get()),
            "cancelled" => Value::Bool(n.cancelled.get()),
            "error" => n.error.borrow().clone().map(Value::Error).unwrap_or(Value::Nil),
            // values written and not read yet (a `create` stream)
            "buffered" => match &*n.kind.borrow() {
                Kind::Queue { buf, .. } => Value::Int(buf.len() as i64),
                _ => Value::Int(0),
            },
            _ => return None,
        })
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "next", "write", "close", "fail", "cancel", "map", "filter", "take", "skip", "takeWhile", "chunk", "tap", "enumerate",
            "toList", "reduce", "forEach", "count", "first", "find", "pipe",
        ]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------- pulling

/// What a function of the chain returned: its value, or the error value that
/// ends the stream (`return nil, error(...)`, an `error`).
fn call(it: &Interpreter, f: &Value, args: Vec<Value>) -> Result<Result<Value, Rc<ErrorValue>>, String> {
    let v = it.call_value(f.clone(), args, Rc::clone(it.globals_ref()))?;
    Ok(v.into_result())
}

/// The next value of `n`: `Some`, or `None` at its end (with `n.error` set
/// when it ended with an error). `Err` is a runtime error of the program
/// (it stops the program as anywhere else).
pub fn pull(it: &Interpreter, n: &Rc<Node>) -> Result<Option<Value>, String> {
    if n.done.get() {
        return Ok(None);
    }
    let up = n.upstream.clone();
    // the upstream's next value, or the end (passing its error on)
    let from_up = |it: &Interpreter| -> Result<Option<Value>, String> {
        let up = up.as_ref().expect("an operator has an upstream");
        match pull(it, up)? {
            Some(v) => Ok(Some(v)),
            None => {
                if let Some(e) = up.error.borrow().clone() {
                    n.fail(e);
                } else {
                    n.done.set(true);
                }
                Ok(None)
            }
        }
    };
    // the operators take what they need out of the cell before calling Fogu
    // code: a function of the chain may use the stream (`s.done`) itself
    enum Op {
        Map(Value),
        Filter(Value),
        TakeWhile(Value),
        Tap(Value),
        Chunk(usize),
        Other,
    }
    let op = match &*n.kind.borrow() {
        Kind::Map(f) => Op::Map(f.clone()),
        Kind::Filter(f) => Op::Filter(f.clone()),
        Kind::TakeWhile(f) => Op::TakeWhile(f.clone()),
        Kind::Tap(f) => Op::Tap(f.clone()),
        Kind::Chunk(k) => Op::Chunk(*k),
        _ => Op::Other,
    };
    match op {
        Op::Map(f) => {
            let Some(v) = from_up(it)? else { return Ok(None) };
            return match call(it, &f, vec![v])? {
                Ok(r) => Ok(Some(r)),
                Err(e) => {
                    n.fail(e);
                    n.cancel_upstream();
                    Ok(None)
                }
            };
        }
        Op::Filter(f) | Op::TakeWhile(f) => {
            let take_while = matches!(&*n.kind.borrow(), Kind::TakeWhile(_));
            loop {
                let Some(v) = from_up(it)? else { return Ok(None) };
                match call(it, &f, vec![v.clone()])? {
                    Ok(keep) if keep.is_truthy() => return Ok(Some(v)),
                    Ok(_) if take_while => {
                        n.done.set(true);
                        n.cancel_upstream();
                        return Ok(None);
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        n.fail(e);
                        n.cancel_upstream();
                        return Ok(None);
                    }
                }
            }
        }
        Op::Tap(f) => {
            let Some(v) = from_up(it)? else { return Ok(None) };
            if let Err(e) = call(it, &f, vec![v.clone()])? {
                n.fail(e);
                n.cancel_upstream();
                return Ok(None);
            }
            return Ok(Some(v));
        }
        Op::Chunk(k) => {
            let mut batch = Vec::with_capacity(k);
            while batch.len() < k {
                match from_up(it)? {
                    Some(v) => batch.push(v),
                    None => break,
                }
            }
            if batch.is_empty() {
                return Ok(None);
            }
            // the last, shorter batch is given; the end comes on the next pull
            n.done.set(false);
            return Ok(Some(Value::List(Rc::new(RefCell::new(batch)))));
        }
        Op::Other => {}
    }

    // the sources and the counting operators
    enum Next {
        Value(Value),
        End,
        Up,
        WaitQueue,
        Readable(NativeObjectRef),
        Chan(Rc<ChanValue>),
        Generate(Value, i64),
        Interval(std::time::Duration, i64),
        Skip(i64),
    }
    let next = {
        let mut k = n.kind.borrow_mut();
        match &mut *k {
            Kind::Queue { buf, closed, .. } => match buf.pop_front() {
                Some(v) => Next::Value(v),
                None if *closed => Next::End,
                None => Next::WaitQueue,
            },
            Kind::Items(items) => items.next().map(Next::Value).unwrap_or(Next::End),
            Kind::Range { next, end, step } => {
                let v = *next;
                let past = match end {
                    Some(e) => (*step > 0 && v >= *e) || (*step < 0 && v <= *e),
                    None => false,
                };
                if past {
                    Next::End
                } else {
                    *next = v.saturating_add(*step);
                    Next::Value(Value::Int(v))
                }
            }
            Kind::Readable(r) => Next::Readable(r.clone()),
            Kind::Chan(c) => Next::Chan(Rc::clone(c)),
            Kind::Generate { f, i } => {
                *i += 1;
                Next::Generate(f.clone(), *i - 1)
            }
            Kind::Interval { every, i } => {
                *i += 1;
                Next::Interval(*every, *i - 1)
            }
            Kind::Take(left) => {
                if *left <= 0 {
                    Next::End
                } else {
                    *left -= 1;
                    Next::Up
                }
            }
            Kind::Skip(left) => {
                let s = *left;
                *left = 0;
                Next::Skip(s)
            }
            Kind::Enumerate(i) => {
                *i += 1;
                Next::Skip(-(*i))
            }
            _ => unreachable!("handled above"),
        }
    };
    match next {
        Next::Value(v) => Ok(Some(v)),
        Next::End => {
            n.done.set(true);
            // `take(n)` has its values: what is upstream is not needed any more
            if matches!(&*n.kind.borrow(), Kind::Take(_)) {
                n.cancel_upstream();
            }
            Ok(None)
        }
        Next::Up => from_up(it),
        Next::Skip(k) if k >= 0 => {
            for _ in 0..k {
                if from_up(it)?.is_none() {
                    return Ok(None);
                }
            }
            from_up(it)
        }
        // enumerate: [index, value]
        Next::Skip(minus_i) => {
            let Some(v) = from_up(it)? else { return Ok(None) };
            Ok(Some(Value::tuple(vec![Value::Int(-minus_i - 1), v])))
        }
        Next::WaitQueue => {
            // a writer fills it, or closes it, or someone cancels: wait for that
            let node = Rc::clone(n);
            sched::wait_until(|| {
                node.cancelled.get()
                    || match &*node.kind.borrow() {
                        Kind::Queue { buf, closed, .. } => !buf.is_empty() || *closed,
                        _ => true,
                    }
            })?;
            pull(it, n)
        }
        Next::Readable(r) => match it.stream_next(&r)? {
            Some(v) => Ok(Some(v)),
            None => {
                match r.get("error") {
                    Some(Value::Error(e)) => n.fail(e),
                    // a WebSocket keeps the text of the error that closed it
                    Some(Value::Str(e)) => n.fail(err(e.to_string())),
                    _ => n.done.set(true),
                }
                Ok(None)
            }
        },
        Next::Chan(c) => match chan_recv(&c) {
            Ok(v) => Ok(Some(v)),
            Err(RecvError::Closed) => {
                n.done.set(true);
                Ok(None)
            }
            Err(RecvError::Other(e)) => Err(e),
        },
        Next::Generate(f, i) => match call(it, &f, vec![Value::Int(i)])? {
            Ok(Value::Nil) => {
                n.done.set(true);
                Ok(None)
            }
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                n.fail(e);
                Ok(None)
            }
        },
        Next::Interval(every, i) => {
            if i > 0 {
                crate::native::util::sleep_coop(every)?;
            }
            if n.cancelled.get() {
                return Ok(None);
            }
            Ok(Some(Value::Int(i)))
        }
    }
}

impl Node {
    fn cancel_upstream(&self) {
        if let Some(up) = &self.upstream {
            up.cancel();
        }
    }
}

/// The value a stream gives `next()`: the value, `nil` at the end, or the
/// error it ended with.
fn next_value(it: &Interpreter, n: &Rc<Node>) -> Result<Value, String> {
    match pull(it, n)? {
        Some(v) => Ok(v),
        None => Ok(n.error.borrow().clone().map(Value::Error).unwrap_or(Value::Nil)),
    }
}

/// For `for x in s`: the next value, or `None` at the end (an error ends the
/// loop and stays in `s.error`).
pub fn for_next(it: &Interpreter, o: &NativeObjectRef) -> Result<Option<Value>, String> {
    match node_of(o) {
        Some(n) => pull(it, &n),
        None => Ok(None),
    }
}

/// Is `o` a stream of this module?
pub fn is_value_stream(o: &NativeObjectRef) -> bool {
    o.with_name(|n| n == "Stream")
}

// ---------------------------------------------------------------- methods

fn func_arg(args: &[Value], i: usize, what: &str) -> Result<Value, String> {
    match args.get(i) {
        Some(v @ (Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) | Value::StructBoundMethod(_))) => Ok(v.clone()),
        Some(other) => Err(format!("{}: expects a function, got '{}'", what, other.type_name())),
        None => Err(format!("{}: expects a function", what)),
    }
}

fn count_arg(args: &[Value], what: &str) -> Result<i64, String> {
    match args.first() {
        Some(Value::Int(n)) if *n >= 0 => Ok(*n),
        Some(other) => Err(format!("{}: expects an int >= 0, got {}", what, other.to_display_string())),
        None => Err(format!("{}: expects an int >= 0", what)),
    }
}

/// The ending a terminal returns: the error the stream ended with, if any.
fn ended(n: &Node, v: Value) -> Value {
    match n.error.borrow().clone() {
        Some(e) => Value::tuple(vec![Value::Nil, Value::Error(e)]),
        None => v,
    }
}

/// A method of a stream (they call Fogu functions, so the interpreter runs them).
pub fn method(it: &Interpreter, o: &NativeObjectRef, name: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let n = node_of(o)?;
    let derive = |kind: Kind| value(Node::new(kind, Some(Rc::clone(&n))));
    Some((|| -> Result<Value, String> {
        Ok(match name {
            "next" => next_value(it, &n)?,
            // ---- a `create` stream: written by tasks
            "write" => {
                crate::native::util::max_args(args, 1, "write")?;
                let v = args.first().cloned().unwrap_or(Value::Nil);
                let room = || {
                    n.cancelled.get()
                        || match &*n.kind.borrow() {
                            Kind::Queue { buf, cap, closed } => *closed || buf.len() < *cap,
                            _ => true,
                        }
                };
                // backpressure: wait while the buffer is full
                sched::wait_until(room)?;
                if n.cancelled.get() {
                    return Ok(Value::Error(err("stream: cancelled by its consumer")));
                }
                match &mut *n.kind.borrow_mut() {
                    Kind::Queue { buf, closed, .. } => {
                        if *closed || n.error.borrow().is_some() {
                            return Ok(Value::Error(err("stream: write after close")));
                        }
                        buf.push_back(v);
                    }
                    _ => return Err("stream.write: only a stream.create() stream is written to".into()),
                }
                Value::Nil
            }
            "close" | "end" => {
                if let Kind::Queue { closed, .. } = &mut *n.kind.borrow_mut() {
                    *closed = true;
                } else {
                    n.cancel();
                }
                Value::Nil
            }
            // ends the stream with an error, after the values already written
            "fail" => {
                let e = match args.first() {
                    Some(Value::Error(e)) => Rc::clone(e),
                    Some(v) => err(v.to_display_string()),
                    None => err("stream: failed"),
                };
                if let Kind::Queue { closed, .. } = &mut *n.kind.borrow_mut() {
                    *closed = true;
                    *n.error.borrow_mut() = Some(e);
                } else {
                    n.fail(e);
                }
                Value::Nil
            }
            "cancel" => {
                n.cancel();
                Value::Nil
            }
            // ---- operators: a new stream, nothing done yet
            "map" => derive(Kind::Map(func_arg(args, 0, "map")?)),
            "filter" => derive(Kind::Filter(func_arg(args, 0, "filter")?)),
            "takeWhile" => derive(Kind::TakeWhile(func_arg(args, 0, "takeWhile")?)),
            "tap" => derive(Kind::Tap(func_arg(args, 0, "tap")?)),
            "take" => derive(Kind::Take(count_arg(args, "take")?)),
            "skip" => derive(Kind::Skip(count_arg(args, "skip")?)),
            "chunk" => match count_arg(args, "chunk")? {
                0 => return Err("chunk: the size must be > 0".into()),
                k => derive(Kind::Chunk(k as usize)),
            },
            "enumerate" => derive(Kind::Enumerate(0)),
            // ---- terminals: consume the stream
            "toList" => {
                let mut out = Vec::new();
                while let Some(v) = pull(it, &n)? {
                    out.push(v);
                }
                ended(&n, Value::List(Rc::new(RefCell::new(out))))
            }
            "count" => {
                let mut k = 0i64;
                while pull(it, &n)?.is_some() {
                    k += 1;
                }
                ended(&n, Value::Int(k))
            }
            // pipe(dest, { end = true }): every value into a channel, a WebSocket (`send`)
            // or a writable (`write`); then `dest` is closed, unless `end: false`.
            // Returns how many values went; an error of `dest` ends the stream with it.
            "pipe" => return pipe(it, &n, args),
            "forEach" => {
                let f = func_arg(args, 0, "forEach")?;
                while let Some(v) = pull(it, &n)? {
                    if let Err(e) = call(it, &f, vec![v])? {
                        n.fail(e);
                        n.cancel_upstream();
                        break;
                    }
                }
                ended(&n, Value::Nil)
            }
            "reduce" => {
                let f = func_arg(args, 0, "reduce")?;
                let mut acc = args.get(1).cloned().unwrap_or(Value::Nil);
                while let Some(v) = pull(it, &n)? {
                    match call(it, &f, vec![acc, v])? {
                        Ok(a) => acc = a,
                        Err(e) => {
                            n.fail(e);
                            n.cancel_upstream();
                            acc = Value::Nil;
                            break;
                        }
                    }
                }
                ended(&n, acc)
            }
            "first" => {
                let v = pull(it, &n)?;
                n.cancel();
                ended(&n, v.unwrap_or(Value::Nil))
            }
            "find" => {
                let f = func_arg(args, 0, "find")?;
                let mut found = Value::Nil;
                while let Some(v) = pull(it, &n)? {
                    match call(it, &f, vec![v.clone()])? {
                        Ok(k) if k.is_truthy() => {
                            found = v;
                            break;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            n.fail(e);
                            break;
                        }
                    }
                }
                n.cancel();
                ended(&n, found)
            }
            other => {
                return Err(format!(
                    "method '{}' does not exist on Stream (next write close fail cancel map filter take skip takeWhile chunk tap enumerate toList reduce forEach count first find pipe)",
                    other
                ))
            }
        })
    })())
}

/// `stream.pipe(dest, opts)` (a terminal, see `method`).
fn pipe(it: &Interpreter, n: &Rc<Node>, args: &[Value]) -> Result<Value, String> {
    crate::native::util::max_args(args, 2, "pipe")?;
    let end = match args.get(1) {
        None | Some(Value::Nil) => true,
        Some(Value::Map(m)) => m.borrow().get("end").is_none_or(|v| v.is_truthy()),
        Some(other) => return Err(format!("pipe: options must be a map, got '{}'", other.type_name())),
    };
    let mut sent = 0i64;
    match args.first() {
        Some(Value::Chan(c)) => {
            while let Some(v) = pull(it, n)? {
                if c.closed.get() {
                    n.fail(err("pipe: the channel is closed"));
                    n.cancel_upstream();
                    break;
                }
                chan_send(c, v)?;
                sent += 1;
            }
            if end {
                c.close();
            }
        }
        Some(Value::Object(o)) => {
            let write = if o.with_name(|name| name == "WebSocket") { "send" } else { "write" };
            while let Some(v) = pull(it, n)? {
                let failed = match it.call_native_method(o, write, &[v]) {
                    Ok(Value::Error(e)) => Some(e),
                    Ok(_) => None,
                    Err(e) if e.starts_with(crate::runtime::value::ERR_VALUE_PREFIX) => {
                        Some(err(e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(&e)))
                    }
                    Err(e) => return Err(e),
                };
                if let Some(e) = failed {
                    n.fail(e);
                    n.cancel_upstream();
                    break;
                }
                sent += 1;
            }
            if end {
                let _ = it.call_native_method(o, "close", &[]);
            }
        }
        Some(other) => return Err(format!("pipe: expects a chan, a WebSocket or a writable stream, got '{}'", other.type_name())),
        None => return Err("pipe: expects where the values go (a chan, a WebSocket or a writable stream)".into()),
    }
    Ok(ended(n, Value::Int(sent)))
}

// ---------------------------------------------------------------- module

fn from(args: &[Value]) -> Result<Value, String> {
    crate::native::util::max_args(args, 1, "stream.from")?;
    let kind = match args.first() {
        Some(Value::List(l)) => Kind::Items(l.borrow().clone().into_iter()),
        Some(Value::Tuple(t)) => Kind::Items(t.to_vec().into_iter()),
        Some(Value::Array(a)) => Kind::Items(a.borrow().to_values().into_iter()),
        Some(Value::Range(a, b)) => Kind::Range { next: *a, end: Some(*b), step: 1 },
        Some(Value::Chan(c)) => Kind::Chan(Rc::clone(c)),
        Some(Value::Str(s)) => Kind::Items(s.chars().map(|c| Value::Str(c.into())).collect::<Vec<_>>().into_iter()),
        Some(Value::Object(o)) if is_value_stream(o) => return Ok(Value::Object(o.clone())),
        Some(Value::Object(o)) if crate::runtime::interpreter::is_stream(o) => Kind::Readable(o.clone()),
        Some(other) => return Err(format!("stream.from: expects a list, a range, a chan, a text, a readable or a WebSocket, got '{}'", other.type_name())),
        None => Kind::Items(Vec::new().into_iter()),
    };
    Ok(value(Node::new(kind, None)))
}

pub fn build() -> ModuleValue {
    use crate::native::util::nfn_var;
    let mut e: HashMap<String, Value> = HashMap::new();
    // create({ buffer = 16 }): written by tasks (write / close / fail), read by a consumer
    e.insert(
        "create".into(),
        nfn_var("create", 0, |args| {
            crate::native::util::max_args(args, 1, "stream.create")?;
            let cap = match args.first() {
                None | Some(Value::Nil) => 16,
                Some(Value::Map(m)) => match m.borrow().get("buffer") {
                    None | Some(Value::Nil) => 16,
                    Some(Value::Int(n)) if *n >= 1 => *n as usize,
                    Some(other) => return Err(format!("stream.create: buffer must be an int >= 1, got {}", other.to_display_string())),
                },
                Some(other) => return Err(format!("stream.create: options must be a map, got '{}'", other.type_name())),
            };
            Ok(value(Node::new(Kind::Queue { buf: VecDeque::with_capacity(cap.min(1024)), cap, closed: false }, None)))
        }),
    );
    e.insert("from".into(), nfn_var("from", 0, from));
    e.insert("of".into(), nfn_var("of", 0, |args| Ok(value(Node::new(Kind::Items(args.to_vec().into_iter()), None)))));
    // range(start, end?, step = 1): end excluded; no end = infinite
    e.insert(
        "range".into(),
        nfn_var("range", 1, |args| {
            crate::native::util::max_args(args, 3, "stream.range")?;
            let int = |i: usize| match args.get(i) {
                None | Some(Value::Nil) => Ok(None),
                Some(Value::Int(n)) => Ok(Some(*n)),
                Some(other) => Err(format!("stream.range: expects ints, got {}", other.to_display_string())),
            };
            let start = int(0)?.unwrap_or(0);
            let step = int(2)?.unwrap_or(1);
            if step == 0 {
                return Err("stream.range: step must not be 0".into());
            }
            Ok(value(Node::new(Kind::Range { next: start, end: int(1)?, step }, None)))
        }),
    );
    // generate(fn(i)): fn(0), fn(1) … until it returns nil
    e.insert(
        "generate".into(),
        nfn_var("generate", 1, |args| Ok(value(Node::new(Kind::Generate { f: func_arg(args, 0, "stream.generate")?, i: 0 }, None)))),
    );
    // interval(d): 0, 1, 2 … one every d (infinite: take, takeWhile or cancel ends it)
    e.insert(
        "interval".into(),
        nfn_var("interval", 1, |args| {
            let every = crate::native::util::duration_arg(&args[0], "stream.interval")?;
            Ok(value(Node::new(Kind::Interval { every, i: 0 }, None)))
        }),
    );
    ModuleValue { name: "fogu:stream".into(), exports: RefCell::new(e) }
}
