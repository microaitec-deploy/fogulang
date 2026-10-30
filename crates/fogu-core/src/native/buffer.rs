//! The `buffer` type: a mutable sequence of bytes (network and file data,
//! binary protocols). Built with the global `bytes(...)`:
//!
//! ```fogu
//! const b = bytes("héllo")                 // UTF-8 of a str
//! const z = bytes(4)                       // 4 zero bytes
//! const h = bytes("cafe", "hex")           // hex / base64 / base64url / latin1
//! print(b.len(), b[0], b.text(), b.hex())  // 6 104 héllo 68c3a96c6c6f
//! b[0] = 72                                // bytes are ints 0..255
//! z.writeU16(0, 513)                       // little-endian by default
//! print(z.readU16(0), z.readU16(0, true))  // 513 258 (big-endian)
//! for x in b.slice(0, 2) { print(x) }
//! ```
//!
//! Network modules (`net`, `websocket`, `http`), `fs`, `zip`, `image`… take
//! and return buffers; `bytes_of` (native/util.rs) also still accepts a str or
//! a list of byte ints.

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;

use crate::runtime::value::Value;

fn b64() -> &'static base64::engine::GeneralPurpose {
    &base64::engine::general_purpose::STANDARD
}

fn b64url() -> &'static base64::engine::GeneralPurpose {
    &base64::engine::general_purpose::URL_SAFE_NO_PAD
}

fn encoding(v: Option<&Value>, what: &str) -> Result<String, String> {
    match v {
        None | Some(Value::Nil) => Ok("utf8".into()),
        Some(Value::Str(s)) => Ok(s.to_ascii_lowercase().replace('-', "")),
        Some(other) => Err(format!("{}: the encoding must be a str, got '{}'", what, other.type_name())),
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.len() % 2 != 0 {
        return Err(format!("bytes: invalid hex (odd length {})", s.len()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| format!("bytes: invalid hex '{}'", &s[i..i + 2])))
        .collect()
}

pub fn encode_hex(b: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(b.len() * 2);
    for x in b {
        out.push(DIGITS[(x >> 4) as usize] as char);
        out.push(DIGITS[(x & 15) as usize] as char);
    }
    out
}

/// `bytes(x, encoding = "utf8")`: str (utf8 hex base64 base64url latin1),
/// int n (n zero bytes), list of byte ints, or another buffer (a copy).
pub fn bytes(args: &[Value]) -> Result<Value, String> {
    if args.len() > 2 {
        return Err(format!("bytes expects at most 2 arguments, got {}", args.len()));
    }
    let data = match args.first() {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::Str(s)) => match encoding(args.get(1), "bytes")?.as_str() {
            "utf8" => s.as_bytes().to_vec(),
            "hex" => decode_hex(s)?,
            "base64" => b64().decode(s.trim()).map_err(|e| format!("bytes: invalid base64: {}", e))?,
            "base64url" => b64url().decode(s.trim().trim_end_matches('=')).map_err(|e| format!("bytes: invalid base64url: {}", e))?,
            "latin1" | "binary" => s.chars().map(|c| if (c as u32) < 256 { Ok(c as u8) } else { Err(format!("bytes: '{}' is not latin1", c)) }).collect::<Result<_, _>>()?,
            other => return Err(format!("bytes: unknown encoding '{}' (utf8 hex base64 base64url latin1)", other)),
        },
        Some(Value::Int(n)) if *n >= 0 => vec![0u8; *n as usize],
        Some(Value::Buffer(b)) => b.borrow().clone(),
        Some(v @ Value::List(_)) => crate::native::util::bytes_of(v, "bytes")?,
        Some(other) => return Err(format!("bytes expects a str, an int size, a list of bytes or a buffer, got '{}'", other.type_name())),
    };
    Ok(Value::buffer(data))
}

/// The bytes of a str / buffer / list / single byte int (append, indexOf…).
fn operand(v: &Value, what: &str) -> Result<Vec<u8>, String> {
    match v {
        Value::Int(n) if (0..=255).contains(n) => Ok(vec![*n as u8]),
        Value::Int(n) => Err(format!("{}: a byte must be 0..255, got {}", what, n)),
        other => crate::native::util::bytes_of(other, what),
    }
}

fn int_arg(args: &[Value], i: usize, what: &str) -> Result<i64, String> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(format!("{}: expected an int, got '{}'", what, other.type_name())),
        None => Err(format!("{}: missing argument {}", what, i + 1)),
    }
}

/// A position in `0..=len` (negative counts from the end).
fn pos(v: Option<&Value>, len: usize, default: usize, what: &str) -> Result<usize, String> {
    match v {
        None | Some(Value::Nil) => Ok(default),
        Some(Value::Int(i)) => {
            let i = if *i < 0 { *i + len as i64 } else { *i };
            Ok(i.clamp(0, len as i64) as usize)
        }
        Some(other) => Err(format!("{}: index must be int, got '{}'", what, other.type_name())),
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(hay.len()));
    }
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// Numeric reads/writes: `readU16(offset, bigEndian = false)`.
fn width(name: &str) -> Option<(usize, bool, bool)> {
    // (bytes, signed, float)
    Some(match name {
        "U8" => (1, false, false),
        "I8" => (1, true, false),
        "U16" => (2, false, false),
        "I16" => (2, true, false),
        "U32" => (4, false, false),
        "I32" => (4, true, false),
        "I64" => (8, true, false),
        "F32" => (4, true, true),
        "F64" => (8, true, true),
        _ => return None,
    })
}

fn big_endian(v: Option<&Value>) -> bool {
    v.is_some_and(Value::is_truthy)
}

fn read_num(b: &[u8], name: &str, kind: &str, args: &[Value]) -> Result<Value, String> {
    let (n, signed, float) = width(kind).expect("checked");
    let off = int_arg(args, 0, name)?;
    if off < 0 || off as usize + n > b.len() {
        return Err(format!("{}: offset {} out of bounds (length {})", name, off, b.len()));
    }
    let mut raw = [0u8; 8];
    raw[..n].copy_from_slice(&b[off as usize..off as usize + n]);
    if big_endian(args.get(1)) {
        raw[..n].reverse();
    }
    let u = u64::from_le_bytes(raw);
    Ok(match (float, n) {
        (true, 4) => Value::Float(f32::from_bits(u as u32) as f64),
        (true, _) => Value::Float(f64::from_bits(u)),
        (false, 8) => Value::Int(u as i64),
        (false, _) if signed => {
            let shift = 64 - 8 * n as u32;
            Value::Int(((u << shift) as i64) >> shift)
        }
        (false, _) => Value::Int(u as i64),
    })
}

fn write_num(b: &mut [u8], name: &str, kind: &str, args: &[Value]) -> Result<Value, String> {
    let (n, signed, float) = width(kind).expect("checked");
    let off = int_arg(args, 0, name)?;
    if off < 0 || off as usize + n > b.len() {
        return Err(format!("{}: offset {} out of bounds (length {})", name, off, b.len()));
    }
    let raw: [u8; 8] = match (float, args.get(1)) {
        (true, Some(Value::Float(f))) if n == 4 => ((*f as f32).to_bits() as u64).to_le_bytes(),
        (true, Some(Value::Int(i))) if n == 4 => ((*i as f32).to_bits() as u64).to_le_bytes(),
        (true, Some(Value::Float(f))) => f.to_bits().to_le_bytes(),
        (true, Some(Value::Int(i))) => (*i as f64).to_bits().to_le_bytes(),
        (false, Some(Value::Int(i))) => {
            let bits = 8 * n as u32;
            let ok = if n == 8 {
                true
            } else if signed {
                let lim = 1i64 << (bits - 1);
                (-lim..lim).contains(i)
            } else {
                (0..(1i64 << bits)).contains(i)
            };
            if !ok {
                return Err(format!("{}: {} does not fit in {} bits", name, i, bits));
            }
            (*i as u64).to_le_bytes()
        }
        (_, other) => return Err(format!("{}: expected a number, got {}", name, other.map_or("nothing".into(), |v| v.type_name().to_string()))),
    };
    let mut bytes = raw;
    if big_endian(args.get(2)) {
        bytes[..n].reverse();
    }
    b[off as usize..off as usize + n].copy_from_slice(&bytes[..n]);
    Ok(Value::Nil)
}

/// Methods of a buffer (`b.text()`, `b.slice(1, 3)`…).
pub fn method(buf: &Rc<RefCell<Vec<u8>>>, name: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let r = (|| -> Result<Value, String> {
        Ok(match name {
            "len" => Value::Int(buf.borrow().len() as i64),
            "isEmpty" => Value::Bool(buf.borrow().is_empty()),
            // text(encoding = "utf8"): invalid UTF-8 becomes U+FFFD
            "text" | "toString" => {
                let b = buf.borrow();
                Value::Str(match encoding(args.first(), name)?.as_str() {
                    "utf8" => String::from_utf8_lossy(&b).into(),
                    "hex" => encode_hex(&b).into(),
                    "base64" => b64().encode(&*b).into(),
                    "base64url" => b64url().encode(&*b).into(),
                    "latin1" | "binary" => b.iter().map(|&x| x as char).collect(),
                    other => return Err(format!("{}: unknown encoding '{}' (utf8 hex base64 base64url latin1)", name, other)),
                })
            }
            "hex" => Value::Str(encode_hex(&buf.borrow()).into()),
            "base64" => Value::Str(b64().encode(&*buf.borrow()).into()),
            "toList" => Value::List(Rc::new(RefCell::new(buf.borrow().iter().map(|&x| Value::Int(x as i64)).collect()))),
            "copy" | "clone" => Value::buffer(buf.borrow().clone()),
            // slice(start = 0, end = len) → a new buffer
            "slice" => {
                let b = buf.borrow();
                let a = pos(args.first(), b.len(), 0, name)?;
                let e = pos(args.get(1), b.len(), b.len(), name)?;
                Value::buffer(if a < e { b[a..e].to_vec() } else { Vec::new() })
            }
            "get" => {
                let b = buf.borrow();
                let i = int_arg(args, 0, name)?;
                let i = if i < 0 { i + b.len() as i64 } else { i };
                if i < 0 || i as usize >= b.len() { Value::Nil } else { Value::Int(b[i as usize] as i64) }
            }
            // push(byte...) / append(str | buffer | list | byte) → the buffer itself
            "push" | "append" | "write" => {
                let mut add = Vec::new();
                for a in args {
                    add.extend(operand(a, name)?);
                }
                buf.borrow_mut().extend_from_slice(&add);
                Value::Buffer(Rc::clone(buf))
            }
            "concat" => {
                let mut out = buf.borrow().clone();
                for a in args {
                    out.extend(operand(a, name)?);
                }
                Value::buffer(out)
            }
            "indexOf" | "includes" => {
                let needle = operand(args.first().ok_or(format!("{}(value) expects 1 argument", name))?, name)?;
                let b = buf.borrow();
                let from = pos(args.get(1), b.len(), 0, name)?;
                let at = find(&b, &needle, from);
                if name == "includes" { Value::Bool(at.is_some()) } else { Value::Int(at.map_or(-1, |i| i as i64)) }
            }
            "startsWith" | "endsWith" => {
                let needle = operand(args.first().ok_or(format!("{}(value) expects 1 argument", name))?, name)?;
                let b = buf.borrow();
                Value::Bool(if name == "startsWith" { b.starts_with(&needle) } else { b.ends_with(&needle) })
            }
            // split(separator) → list of buffers
            "split" => {
                let sep = operand(args.first().ok_or("split(separator) expects 1 argument")?, name)?;
                if sep.is_empty() {
                    return Err("split: the separator must not be empty".into());
                }
                let b = buf.borrow();
                let mut out = Vec::new();
                let mut start = 0;
                while let Some(i) = find(&b, &sep, start) {
                    out.push(Value::buffer(b[start..i].to_vec()));
                    start = i + sep.len();
                }
                out.push(Value::buffer(b[start..].to_vec()));
                Value::List(Rc::new(RefCell::new(out)))
            }
            "equals" => Value::Bool(match args.first() {
                Some(v) => operand(v, name).is_ok_and(|o| *buf.borrow() == o),
                None => false,
            }),
            // fill(byte, start = 0, end = len)
            "fill" => {
                let byte = match args.first() {
                    Some(Value::Int(n)) if (0..=255).contains(n) => *n as u8,
                    _ => return Err("fill(byte) expects an int 0..255".into()),
                };
                let mut b = buf.borrow_mut();
                let len = b.len();
                let a = pos(args.get(1), len, 0, name)?;
                let e = pos(args.get(2), len, len, name)?;
                if a < e {
                    b[a..e].fill(byte);
                }
                Value::Buffer(Rc::clone(buf))
            }
            "clear" => {
                buf.borrow_mut().clear();
                Value::Nil
            }
            "reverse" => {
                buf.borrow_mut().reverse();
                Value::Buffer(Rc::clone(buf))
            }
            _ => {
                if let Some(kind) = name.strip_prefix("read").filter(|k| width(k).is_some()) {
                    return read_num(&buf.borrow(), name, kind, args);
                }
                if let Some(kind) = name.strip_prefix("write").filter(|k| width(k).is_some()) {
                    return write_num(&mut buf.borrow_mut(), name, kind, args);
                }
                return Err(format!(
                    "'buffer' has no method '{}' (len text hex base64 slice get push append concat indexOf includes startsWith endsWith split equals fill clear reverse copy toList readU8..readF64 writeU8..writeF64)",
                    name
                ));
            }
        })
    })();
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(v: &[u8]) -> Rc<RefCell<Vec<u8>>> {
        Rc::new(RefCell::new(v.to_vec()))
    }

    #[test]
    fn numbers_round_trip() {
        let buf = b(&[0; 8]);
        method(&buf, "writeI16", &[Value::Int(0), Value::Int(-2)]).unwrap().unwrap();
        let v = method(&buf, "readI16", &[Value::Int(0)]).unwrap().unwrap();
        assert!(matches!(v, Value::Int(-2)));
        method(&buf, "writeU32", &[Value::Int(0), Value::Int(0x01020304), Value::Bool(true)]).unwrap().unwrap();
        assert_eq!(&buf.borrow()[..4], &[1, 2, 3, 4]);
        method(&buf, "writeF64", &[Value::Int(0), Value::Float(1.5)]).unwrap().unwrap();
        assert!(matches!(method(&buf, "readF64", &[Value::Int(0)]).unwrap().unwrap(), Value::Float(f) if f == 1.5));
    }

    #[test]
    fn encodings() {
        assert_eq!(encode_hex(&[0xca, 0xfe]), "cafe");
        assert_eq!(decode_hex("ca fe").unwrap(), vec![0xca, 0xfe]);
        assert_eq!(find(b"hello", b"ll", 0), Some(2));
    }
}
