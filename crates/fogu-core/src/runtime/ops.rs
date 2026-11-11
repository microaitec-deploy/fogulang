//! Checked integer arithmetic, shared by the interpreter and the JIT (so
//! that both produce exactly the same results and error messages).
//!

pub const ERR_OVERFLOW: &str = "integer overflow";
pub const ERR_DIV_ZERO: &str = "division by zero";
pub const ERR_NEG_POW: &str = "negative integer exponent in '**' (use float)";
pub const ERR_STACK: &str = "stack overflow (recursion too deep)";
pub const ERR_MATCH: &str = "non-exhaustive match: no arm matched";
pub const ERR_SHIFT: &str = "negative shift count";
/// Native code was stopped by the deadline of its `fogu:vm` run (the
/// interpreter turns it into the timeout error of that run).
pub const ERR_CANCEL: &str = "script execution timed out";

/// Error codes written by native code into `JitCtx::err`.
pub const CODE_OVERFLOW: i64 = 1;
pub const CODE_DIV_ZERO: i64 = 2;
pub const CODE_NEG_POW: i64 = 3;
pub const CODE_STACK: i64 = 4;
pub const CODE_MATCH: i64 = 5;
pub const CODE_SHIFT: i64 = 6;

pub fn code_message(code: i64) -> &'static str {
    match code {
        CODE_OVERFLOW => ERR_OVERFLOW,
        CODE_DIV_ZERO => ERR_DIV_ZERO,
        CODE_NEG_POW => ERR_NEG_POW,
        CODE_STACK => ERR_STACK,
        CODE_MATCH => ERR_MATCH,
        CODE_SHIFT => ERR_SHIFT,
        c if c >= CODE_CUSTOM => CUSTOM.with(|m| m.borrow().get((c - CODE_CUSTOM) as usize).copied()).unwrap_or("unknown error in native code"),
        _ => "unknown error in native code",
    }
}

/// `a ** b` for integers. Returns the error code on failure.
pub fn int_pow(a: i64, b: i64) -> Result<i64, i64> {
    if b < 0 {
        return Err(CODE_NEG_POW);
    }
    match a {
        0 => Ok(if b == 0 { 1 } else { 0 }),
        1 => Ok(1),
        -1 => Ok(if b % 2 == 0 { 1 } else { -1 }),
        _ => {
            if b > 63 {
                return Err(CODE_OVERFLOW);
            }
            a.checked_pow(b as u32).ok_or(CODE_OVERFLOW)
        }
    }
}

/// `a << n` (as in Go: bits shifted out are lost, 64 or more gives 0).
pub fn int_shl(a: i64, n: i64) -> Result<i64, i64> {
    match n {
        0..=63 => Ok(((a as u64) << n) as i64),
        64.. => Ok(0),
        _ => Err(CODE_SHIFT),
    }
}

/// `a >> n`, keeping the sign (64 or more gives 0 or -1).
pub fn int_shr(a: i64, n: i64) -> Result<i64, i64> {
    match n {
        0.. => Ok(a >> n.min(63)),
        _ => Err(CODE_SHIFT),
    }
}

pub fn int_div(a: i64, b: i64) -> Result<i64, i64> {
    if b == 0 {
        return Err(CODE_DIV_ZERO);
    }
    a.checked_div(b).ok_or(CODE_OVERFLOW)
}

pub fn int_rem(a: i64, b: i64) -> Result<i64, i64> {
    if b == 0 {
        return Err(CODE_DIV_ZERO);
    }
    a.checked_rem(b).ok_or(CODE_OVERFLOW)
}

// ----------------------------------------------------------------------
// Sized numeric types (as in Go): int8 … uint64, float32, float64
// ----------------------------------------------------------------------

/// A sized numeric type. Fogu numbers stay `int` (64-bit signed) and `float`
/// (64-bit) at run time: a sized type is a *range* (annotations) and a
/// *conversion* (`int8(x)` wraps like Go's `int8(x)`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sized {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    /// `uint64`, `uint`, `uintptr`: 0..=i64::MAX (Fogu integers are 64-bit signed).
    U64,
    F32,
    F64,
}

/// Every sized type name (the conversion functions have the same names).
pub const SIZED_NAMES: &[&str] = &[
    "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "uint", "uintptr",
    "byte", "rune", "float32", "float64",
];

impl Sized {
    pub fn from_name(name: &str) -> Option<Sized> {
        Some(match name {
            "int8" => Sized::I8,
            "int16" => Sized::I16,
            "int32" | "rune" => Sized::I32,
            "int64" => Sized::I64,
            "uint8" | "byte" => Sized::U8,
            "uint16" => Sized::U16,
            "uint32" => Sized::U32,
            "uint64" | "uint" | "uintptr" => Sized::U64,
            "float32" => Sized::F32,
            "float64" => Sized::F64,
            _ => return None,
        })
    }

    pub fn is_float(self) -> bool {
        matches!(self, Sized::F32 | Sized::F64)
    }

    /// The inclusive range of an integer type (`None` for the float types).
    pub fn range(self) -> Option<(i64, i64)> {
        Some(match self {
            Sized::I8 => (i8::MIN as i64, i8::MAX as i64),
            Sized::I16 => (i16::MIN as i64, i16::MAX as i64),
            Sized::I32 => (i32::MIN as i64, i32::MAX as i64),
            Sized::I64 => (i64::MIN, i64::MAX),
            Sized::U8 => (0, u8::MAX as i64),
            Sized::U16 => (0, u16::MAX as i64),
            Sized::U32 => (0, u32::MAX as i64),
            Sized::U64 => (0, i64::MAX),
            Sized::F32 | Sized::F64 => return None,
        })
    }

    /// Does the integer fit the type (always false for the float types)?
    pub fn fits(self, n: i64) -> bool {
        self.range().is_some_and(|(lo, hi)| n >= lo && n <= hi)
    }

    /// The integer conversion of Go: keeps the low bits (two's complement).
    /// `None` only for a negative number into `uint64`, which a Fogu `int`
    /// cannot hold.
    pub fn wrap(self, n: i64) -> Option<i64> {
        Some(match self {
            Sized::I8 => n as i8 as i64,
            Sized::I16 => n as i16 as i64,
            Sized::I32 => n as i32 as i64,
            Sized::U8 => n as u8 as i64,
            Sized::U16 => n as u16 as i64,
            Sized::U32 => n as u32 as i64,
            Sized::U64 if n < 0 => return None,
            _ => n,
        })
    }

    /// A float as this float type (`float32` rounds to single precision).
    pub fn round(self, f: f64) -> f64 {
        match self {
            Sized::F32 => f as f32 as f64,
            _ => f,
        }
    }
}

/// `got …` of a type mismatch where an `int` does not fit a sized annotation
/// (shared by the interpreter and the JIT).
pub fn out_of_range(name: &str, kind: Sized) -> String {
    let (lo, hi) = kind.range().unwrap_or((0, 0));
    format!("int out of range ({} is {}..{})", name, lo, hi)
}

/// `uint64(-1)`.
pub fn err_negative(name: &str) -> String {
    format!("cannot convert a negative int to {} (it holds 0..{})", name, i64::MAX)
}

thread_local! {
    /// Messages of errors raised by native code that are specific to one
    /// program (a parameter's name and type): [`custom_code`] / [`code_message`].
    static CUSTOM: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

const CODE_CUSTOM: i64 = 1000;

/// An error code for `msg` (the same message always gets the same code).
pub fn custom_code(msg: &str) -> i64 {
    CUSTOM.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(i) = c.iter().position(|m| *m == msg) {
            return CODE_CUSTOM + i as i64;
        }
        c.push(Box::leak(msg.to_string().into_boxed_str()));
        CODE_CUSTOM + c.len() as i64 - 1
    })
}

#[cfg(test)]
mod sized_tests {
    use super::*;

    #[test]
    fn wraps_like_go() {
        assert_eq!(Sized::I8.wrap(127 + 1), Some(-128));
        assert_eq!(Sized::U8.wrap(-1), Some(255));
        assert_eq!(Sized::U8.wrap(256), Some(0));
        assert_eq!(Sized::I16.wrap(40000), Some(-25536));
        assert_eq!(Sized::I32.wrap(1 << 31), Some(i32::MIN as i64));
        assert_eq!(Sized::U32.wrap(-1), Some(4294967295));
        assert_eq!(Sized::U64.wrap(-1), None);
        assert_eq!(Sized::I64.wrap(i64::MIN), Some(i64::MIN));
        assert_eq!(Sized::F32.round(0.1), 0.1f32 as f64);
    }

    #[test]
    fn custom_codes_are_stable() {
        let a = custom_code("x");
        assert_eq!(a, custom_code("x"));
        assert_ne!(a, custom_code("y"));
        assert_eq!(code_message(a), "x");
    }
}
