//! `fogu:math` — numbers: constants, trigonometry, logarithms, rounding,
//! integer helpers, statistics over lists and a seedable random generator.
//!
//! Functions that the JIT compiles inline are listed in `jit.rs` (`JIT_MATH`):
//! keep their results identical to the ones here (the interpreter is the
//! reference).

use std::cell::{Cell, RefCell};
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::ops::ERR_OVERFLOW;
use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

type R = Result<Value, String>;

pub fn build() -> ModuleValue {
    let mut e = HashMap::new();

    for (name, v) in [
        ("PI", std::f64::consts::PI),
        ("E", std::f64::consts::E),
        ("TAU", std::f64::consts::TAU),
        ("LN2", std::f64::consts::LN_2),
        ("LN10", std::f64::consts::LN_10),
        ("SQRT2", std::f64::consts::SQRT_2),
        ("EPSILON", f64::EPSILON),
        ("INF", f64::INFINITY),
        ("NAN", f64::NAN),
    ] {
        e.insert(name.into(), Value::Float(v));
    }
    e.insert("MAX_INT".into(), Value::Int(i64::MAX));
    e.insert("MIN_INT".into(), Value::Int(i64::MIN));

    let fixed: &[(&str, usize, fn(&[Value]) -> R)] = &[
        ("abs", 1, abs),
        ("sqrt", 1, sqrt),
        ("cbrt", 1, cbrt),
        ("pow", 2, pow),
        ("floor", 1, floor),
        ("ceil", 1, ceil),
        ("trunc", 1, trunc),
        ("fract", 1, fract),
        ("sign", 1, sign),
        ("sin", 1, sin),
        ("cos", 1, cos),
        ("tan", 1, tan),
        ("asin", 1, asin),
        ("acos", 1, acos),
        ("atan", 1, atan),
        ("atan2", 2, atan2),
        ("sinh", 1, sinh),
        ("cosh", 1, cosh),
        ("tanh", 1, tanh),
        ("asinh", 1, asinh),
        ("acosh", 1, acosh),
        ("atanh", 1, atanh),
        ("exp", 1, exp),
        ("exp2", 1, exp2),
        ("expm1", 1, expm1),
        ("log2", 1, log2),
        ("log10", 1, log10),
        ("log1p", 1, log1p),
        ("hypot", 2, hypot),
        ("degrees", 1, degrees),
        ("radians", 1, radians),
        ("clamp", 3, clamp),
        ("lerp", 3, lerp),
        ("isNaN", 1, is_nan),
        ("isInf", 1, is_inf),
        ("isFinite", 1, is_finite),
        ("gcd", 2, gcd),
        ("lcm", 2, lcm),
        ("factorial", 1, factorial),
        ("isPrime", 1, is_prime),
        ("sum", 1, sum),
        ("mean", 1, mean),
        ("median", 1, median),
        ("random", 0, random),
        ("randomInt", 2, random_int),
        ("uniform", 2, uniform),
        ("choice", 1, choice),
        ("shuffle", 1, shuffle),
        ("seed", 1, seed),
    ];
    for &(name, arity, f) in fixed {
        e.insert(name.into(), nfn(name, arity, f));
    }
    // optional or variable arguments
    e.insert("log".into(), var("log", 1, log));
    e.insert("round".into(), var("round", 1, round));
    e.insert("min".into(), var("min", 1, min));
    e.insert("max".into(), var("max", 1, max));

    ModuleValue {
        name: "fogu:math".into(),
        exports: RefCell::new(e),
    }
}

pub fn nfn(name: &str, arity: usize, f: fn(&[Value]) -> R) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.into(),
        arity,
        variadic: false,
        func: f,
        intercept: Default::default(),
    }))
}

fn var(name: &str, min: usize, f: fn(&[Value]) -> R) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.into(),
        arity: min,
        variadic: true,
        func: f,
        intercept: Default::default(),
    }))
}

fn num(v: &Value) -> Result<f64, String> {
    match v {
        Value::Int(n) => Ok(*n as f64),
        Value::Float(n) => Ok(*n),
        other => Err(format!("expected a number, got '{}'", other.type_name())),
    }
}

/// The one argument of `name`, as a float.
fn arg(a: &[Value], name: &str) -> Result<f64, String> {
    match &a[0] {
        Value::Int(n) => Ok(*n as f64),
        Value::Float(n) => Ok(*n),
        other => Err(format!("'{}' not applicable to '{}'", name, other.type_name())),
    }
}

fn int_arg(v: &Value, name: &str) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("'{}' expects ints, got '{}'", name, other.type_name())),
    }
}

// ---------------------------------------------------------------------
// Basics and rounding
// ---------------------------------------------------------------------

fn abs(a: &[Value]) -> R {
    match &a[0] {
        Value::Int(n) => n.checked_abs().map(Value::Int).ok_or_else(|| ERR_OVERFLOW.to_string()),
        Value::Float(n) => Ok(Value::Float(n.abs())),
        other => Err(format!("'abs' not applicable to '{}'", other.type_name())),
    }
}

fn sqrt(a: &[Value]) -> R {
    Ok(Value::Float(arg(a, "sqrt")?.sqrt()))
}

fn cbrt(a: &[Value]) -> R {
    Ok(Value::Float(arg(a, "cbrt")?.cbrt()))
}

fn pow(a: &[Value]) -> R {
    Ok(Value::Float(num(&a[0])?.powf(num(&a[1])?)))
}

/// Rounding that keeps ints as they are and turns floats into ints.
fn to_int(a: &[Value], name: &str, f: fn(f64) -> f64) -> R {
    match &a[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(n) => Ok(Value::Int(f(*n) as i64)),
        other => Err(format!("'{}' not applicable to '{}'", name, other.type_name())),
    }
}

fn floor(a: &[Value]) -> R {
    to_int(a, "floor", f64::floor)
}

fn ceil(a: &[Value]) -> R {
    to_int(a, "ceil", f64::ceil)
}

fn trunc(a: &[Value]) -> R {
    to_int(a, "trunc", f64::trunc)
}

/// round(x) → int (halves away from zero); round(x, digits) → float.
fn round(a: &[Value]) -> R {
    match a {
        [x] => to_int(a, "round", f64::round).map_err(|_| format!("'round' not applicable to '{}'", x.type_name())),
        [x, Value::Int(d)] => match x {
            Value::Int(n) if *d >= 0 => Ok(Value::Int(*n)),
            _ => {
                let v = num(x)?;
                let p = 10f64.powi((*d).clamp(-308, 308) as i32);
                Ok(Value::Float((v * p).round() / p))
            }
        },
        _ => Err("round(x, digits?) expects a number and an int".into()),
    }
}

fn fract(a: &[Value]) -> R {
    Ok(Value::Float(arg(a, "fract")?.fract()))
}

fn sign(a: &[Value]) -> R {
    match &a[0] {
        Value::Int(n) => Ok(Value::Int(n.signum())),
        Value::Float(n) if *n == 0.0 || n.is_nan() => Ok(Value::Float(*n)),
        Value::Float(n) => Ok(Value::Float(n.signum())),
        other => Err(format!("'sign' not applicable to '{}'", other.type_name())),
    }
}

// ---------------------------------------------------------------------
// Trigonometry, exponentials, logarithms
// ---------------------------------------------------------------------

macro_rules! unary {
    ($($name:ident),*) => {$(
        fn $name(a: &[Value]) -> R {
            Ok(Value::Float(arg(a, stringify!($name))?.$name()))
        }
    )*};
}

unary!(sin, cos, tan, asin, acos, atan, sinh, cosh, tanh, asinh, acosh, atanh, exp, exp2, log2, log10);

fn expm1(a: &[Value]) -> R {
    Ok(Value::Float(arg(a, "expm1")?.exp_m1()))
}

fn log1p(a: &[Value]) -> R {
    Ok(Value::Float(arg(a, "log1p")?.ln_1p()))
}

/// log(x) is the natural logarithm; log(x, base) any base.
fn log(a: &[Value]) -> R {
    match a {
        [_] => Ok(Value::Float(arg(a, "log")?.ln())),
        [x, b] => Ok(Value::Float(num(x)?.log(num(b)?))),
        _ => Err("log(x, base?) expects 1 or 2 arguments".into()),
    }
}

fn atan2(a: &[Value]) -> R {
    Ok(Value::Float(num(&a[0])?.atan2(num(&a[1])?)))
}

fn hypot(a: &[Value]) -> R {
    Ok(Value::Float(num(&a[0])?.hypot(num(&a[1])?)))
}

fn degrees(a: &[Value]) -> R {
    Ok(Value::Float(arg(a, "degrees")?.to_degrees()))
}

fn radians(a: &[Value]) -> R {
    Ok(Value::Float(arg(a, "radians")?.to_radians()))
}

// ---------------------------------------------------------------------
// Comparisons
// ---------------------------------------------------------------------

/// The numbers of min/max: the arguments, or the one list given.
fn numbers(a: &[Value], name: &str) -> Result<Vec<Value>, String> {
    let items = match a {
        [Value::List(l)] => l.borrow().clone(),
        _ => a.to_vec(),
    };
    if items.is_empty() {
        return Err(format!("{}: no numbers given", name));
    }
    for v in &items {
        if !matches!(v, Value::Int(_) | Value::Float(_)) {
            return Err(format!("'{}' not applicable to '{}'", name, v.type_name()));
        }
    }
    Ok(items)
}

/// The smallest (or largest) number, keeping its type; NaN is ignored
/// unless every number is NaN.
fn extreme(a: &[Value], name: &str, larger: bool) -> R {
    let items = numbers(a, name)?;
    let mut best = items[0].clone();
    for v in &items[1..] {
        let (x, b) = (num(v)?, num(&best)?);
        let better = match (v, &best) {
            (Value::Int(x), Value::Int(b)) => if larger { x > b } else { x < b },
            _ => b.is_nan() || if larger { x > b } else { x < b },
        };
        if better && !x.is_nan() {
            best = v.clone();
        }
    }
    Ok(best)
}

fn min(a: &[Value]) -> R {
    extreme(a, "min", false)
}

fn max(a: &[Value]) -> R {
    extreme(a, "max", true)
}

/// clamp(x, lo, hi): ints stay ints.
fn clamp(a: &[Value]) -> R {
    match (&a[0], &a[1], &a[2]) {
        (Value::Int(x), Value::Int(lo), Value::Int(hi)) if lo <= hi => Ok(Value::Int(*x.max(lo).min(hi))),
        (x, lo, hi) => {
            let (x, lo, hi) = (num(x)?, num(lo)?, num(hi)?);
            if lo > hi {
                return Err("clamp(x, lo, hi): lo is greater than hi".into());
            }
            Ok(Value::Float(x.max(lo).min(hi)))
        }
    }
}

fn lerp(a: &[Value]) -> R {
    let (x, y, t) = (num(&a[0])?, num(&a[1])?, num(&a[2])?);
    Ok(Value::Float(x + (y - x) * t))
}

fn is_nan(a: &[Value]) -> R {
    Ok(Value::Bool(arg(a, "isNaN")?.is_nan()))
}

fn is_inf(a: &[Value]) -> R {
    Ok(Value::Bool(arg(a, "isInf")?.is_infinite()))
}

fn is_finite(a: &[Value]) -> R {
    Ok(Value::Bool(arg(a, "isFinite")?.is_finite()))
}

// ---------------------------------------------------------------------
// Integers
// ---------------------------------------------------------------------

fn gcd_of(mut a: i64, mut b: i64) -> Result<i64, String> {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.checked_abs().ok_or_else(|| ERR_OVERFLOW.to_string())
}

fn gcd(a: &[Value]) -> R {
    Ok(Value::Int(gcd_of(int_arg(&a[0], "gcd")?, int_arg(&a[1], "gcd")?)?))
}

fn lcm(a: &[Value]) -> R {
    let (x, y) = (int_arg(&a[0], "lcm")?, int_arg(&a[1], "lcm")?);
    if x == 0 || y == 0 {
        return Ok(Value::Int(0));
    }
    let g = gcd_of(x, y)?;
    (x / g)
        .checked_mul(y)
        .and_then(i64::checked_abs)
        .map(Value::Int)
        .ok_or_else(|| ERR_OVERFLOW.to_string())
}

fn factorial(a: &[Value]) -> R {
    let n = int_arg(&a[0], "factorial")?;
    if n < 0 {
        return Err("factorial: the number must be >= 0".into());
    }
    (1..=n)
        .try_fold(1i64, |acc, k| acc.checked_mul(k))
        .map(Value::Int)
        .ok_or_else(|| ERR_OVERFLOW.to_string())
}

fn is_prime(a: &[Value]) -> R {
    let n = int_arg(&a[0], "isPrime")?;
    if n < 2 {
        return Ok(Value::Bool(false));
    }
    if n % 2 == 0 || n % 3 == 0 {
        return Ok(Value::Bool(n < 4));
    }
    let mut k = 5i64;
    while k.saturating_mul(k) <= n {
        if n % k == 0 || n % (k + 2) == 0 {
            return Ok(Value::Bool(false));
        }
        k += 6;
    }
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------------
// Statistics over a list
// ---------------------------------------------------------------------

fn list_arg(a: &[Value], name: &str) -> Result<Vec<Value>, String> {
    match &a[0] {
        Value::List(l) => Ok(l.borrow().clone()),
        other => Err(format!("{}(list) expects a list of numbers, got '{}'", name, other.type_name())),
    }
}

/// sum(list): int while every element is an int, float otherwise.
fn sum(a: &[Value]) -> R {
    let items = list_arg(a, "sum")?;
    let mut acc = Value::Int(0);
    for v in &items {
        acc = match (&acc, v) {
            (Value::Int(s), Value::Int(x)) => Value::Int(s.checked_add(*x).ok_or_else(|| ERR_OVERFLOW.to_string())?),
            (s, x) => Value::Float(num(s)? + num(x).map_err(|_| format!("'sum' not applicable to '{}'", x.type_name()))?),
        };
    }
    Ok(acc)
}

fn floats(a: &[Value], name: &str) -> Result<Vec<f64>, String> {
    let items = list_arg(a, name)?;
    if items.is_empty() {
        return Err(format!("{}: the list is empty", name));
    }
    items
        .iter()
        .map(|v| num(v).map_err(|_| format!("'{}' not applicable to '{}'", name, v.type_name())))
        .collect()
}

fn mean(a: &[Value]) -> R {
    let xs = floats(a, "mean")?;
    Ok(Value::Float(xs.iter().sum::<f64>() / xs.len() as f64))
}

fn median(a: &[Value]) -> R {
    let mut xs = floats(a, "median")?;
    xs.sort_by(f64::total_cmp);
    let m = xs.len() / 2;
    Ok(Value::Float(if xs.len() % 2 == 1 { xs[m] } else { (xs[m - 1] + xs[m]) / 2.0 }))
}

// ---------------------------------------------------------------------
// Random numbers (xoshiro256**; not for cryptography: use fogu:crypto)
// ---------------------------------------------------------------------

thread_local! {
    static RNG: Cell<[u64; 4]> = Cell::new(seed_state(entropy()));
}

fn entropy() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.finish()
}

fn seed_state(mut x: u64) -> [u64; 4] {
    // splitmix64 spreads one seed over the four words
    let mut next = || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    [next(), next(), next(), next()]
}

pub fn next_u64() -> u64 {
    RNG.with(|c| {
        let mut s = c.get();
        let out = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        c.set(s);
        out
    })
}

/// A float in [0, 1).
pub fn next_f64() -> f64 {
    (next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// An int in [0, n) without modulo bias.
fn below(n: u64) -> u64 {
    let zone = u64::MAX - (u64::MAX % n);
    loop {
        let r = next_u64();
        if r < zone {
            return r % n;
        }
    }
}

fn random(_: &[Value]) -> R {
    Ok(Value::Float(next_f64()))
}

/// randomInt(min, max): both ends included.
fn random_int(a: &[Value]) -> R {
    let (lo, hi) = match (&a[0], &a[1]) {
        (Value::Int(lo), Value::Int(hi)) if lo <= hi => (*lo, *hi),
        _ => return Err("randomInt(min, max) expects ints with min <= max (inclusive)".into()),
    };
    let span = hi.wrapping_sub(lo) as u64;
    let r = if span == u64::MAX { next_u64() } else { below(span + 1) };
    Ok(Value::Int(lo.wrapping_add(r as i64)))
}

/// uniform(lo, hi): a float in [lo, hi).
fn uniform(a: &[Value]) -> R {
    let (lo, hi) = (num(&a[0])?, num(&a[1])?);
    Ok(Value::Float(lo + (hi - lo) * next_f64()))
}

/// choice(list): a random element (nil for an empty list).
fn choice(a: &[Value]) -> R {
    match &a[0] {
        Value::List(l) => {
            let l = l.borrow();
            Ok(if l.is_empty() { Value::Nil } else { l[below(l.len() as u64) as usize].clone() })
        }
        other => Err(format!("choice(list) expects a list, got '{}'", other.type_name())),
    }
}

/// shuffle(list): shuffles the list in place (Fisher-Yates) and returns it.
fn shuffle(a: &[Value]) -> R {
    match &a[0] {
        Value::List(l) => {
            {
                let mut v = l.borrow_mut();
                for i in (1..v.len()).rev() {
                    v.swap(i, below(i as u64 + 1) as usize);
                }
            }
            Ok(a[0].clone())
        }
        other => Err(format!("shuffle(list) expects a list, got '{}'", other.type_name())),
    }
}

/// seed(n): makes the random sequence repeatable.
fn seed(a: &[Value]) -> R {
    let n = int_arg(&a[0], "seed")?;
    RNG.with(|c| c.set(seed_state(n as u64)));
    Ok(Value::Nil)
}
