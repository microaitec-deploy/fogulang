//! Checked integer arithmetic, shared by the interpreter and the JIT (so
//! that both produce exactly the same results and error messages).
//!

pub const ERR_OVERFLOW: &str = "integer overflow";
pub const ERR_DIV_ZERO: &str = "division by zero";
pub const ERR_NEG_POW: &str = "negative integer exponent in '**' (use float)";
pub const ERR_STACK: &str = "stack overflow (recursion too deep)";
pub const ERR_MATCH: &str = "non-exhaustive match: no arm matched";

/// Error codes written by native code into `JitCtx::err`.
pub const CODE_OVERFLOW: i64 = 1;
pub const CODE_DIV_ZERO: i64 = 2;
pub const CODE_NEG_POW: i64 = 3;
pub const CODE_STACK: i64 = 4;
pub const CODE_MATCH: i64 = 5;

pub fn code_message(code: i64) -> &'static str {
    match code {
        CODE_OVERFLOW => ERR_OVERFLOW,
        CODE_DIV_ZERO => ERR_DIV_ZERO,
        CODE_NEG_POW => ERR_NEG_POW,
        CODE_STACK => ERR_STACK,
        CODE_MATCH => ERR_MATCH,
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
