//! Lists stored with the size their annotation gives (`let xs: list<u8> = []`):
//! see `docs/architecture/sized-storage.md`. A `TypedArray` holds the numbers
//! themselves — 1 byte each for `u8`, 4 for `i32`/`f32`, 8 for `i64`/`f64` —
//! instead of a 24-byte `Value` per element, and behaves as a list: an element
//! reads as an `int` (a `float` for the float types), and storing one checks it
//! as the annotation does. `list<char>` holds the characters themselves (4
//! bytes each); an element reads as a one-character `str`. The plain types
//! pack too: `list<int>` and `list<float>` take 8 bytes per element,
//! `list<bool>` one.

use super::ops::Sized;
use super::value::Value;

/// The elements of a sized list.
#[derive(Clone, Debug, PartialEq)]
pub enum TypedArray {
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    Char(Vec<char>),
    /// `list<int>`: any int.
    Int(Vec<i64>),
    /// `list<float>`: a float (an int is converted).
    Float(Vec<f64>),
    Bool(Vec<bool>),
}

/// The element type of a sized list: a sized number, a character, or one of
/// the plain types `int`, `float`, `bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elem {
    Num(Sized),
    Char,
    Int,
    Float,
    Bool,
}

impl Elem {
    /// The element type an annotation names (`u8`, `float32`, `char` …).
    pub fn from_name(name: &str) -> Option<Elem> {
        match name {
            "char" => Some(Elem::Char),
            "int" => Some(Elem::Int),
            "float" => Some(Elem::Float),
            "bool" => Some(Elem::Bool),
            _ => Sized::from_name(name).map(Elem::Num),
        }
    }
}

/// Runs `$body` with `$v` bound to the vector, whatever its element type.
macro_rules! each {
    ($a:expr, $v:ident => $body:expr) => {
        match $a {
            TypedArray::I8($v) => $body,
            TypedArray::I16($v) => $body,
            TypedArray::I32($v) => $body,
            TypedArray::I64($v) => $body,
            TypedArray::U8($v) => $body,
            TypedArray::U16($v) => $body,
            TypedArray::U32($v) => $body,
            TypedArray::U64($v) => $body,
            TypedArray::F32($v) => $body,
            TypedArray::F64($v) => $body,
            TypedArray::Char($v) => $body,
            TypedArray::Int($v) => $body,
            TypedArray::Float($v) => $body,
            TypedArray::Bool($v) => $body,
        }
    };
}

impl TypedArray {
    /// An empty array of `kind` with room for `cap` elements.
    pub fn with_capacity(kind: Elem, cap: usize) -> TypedArray {
        let kind = match kind {
            Elem::Char => return TypedArray::Char(Vec::with_capacity(cap)),
            Elem::Int => return TypedArray::Int(Vec::with_capacity(cap)),
            Elem::Float => return TypedArray::Float(Vec::with_capacity(cap)),
            Elem::Bool => return TypedArray::Bool(Vec::with_capacity(cap)),
            Elem::Num(k) => k,
        };
        match kind {
            Sized::I8 => TypedArray::I8(Vec::with_capacity(cap)),
            Sized::I16 => TypedArray::I16(Vec::with_capacity(cap)),
            Sized::I32 => TypedArray::I32(Vec::with_capacity(cap)),
            Sized::I64 => TypedArray::I64(Vec::with_capacity(cap)),
            Sized::U8 => TypedArray::U8(Vec::with_capacity(cap)),
            Sized::U16 => TypedArray::U16(Vec::with_capacity(cap)),
            Sized::U32 => TypedArray::U32(Vec::with_capacity(cap)),
            Sized::U64 => TypedArray::U64(Vec::with_capacity(cap)),
            Sized::F32 => TypedArray::F32(Vec::with_capacity(cap)),
            Sized::F64 => TypedArray::F64(Vec::with_capacity(cap)),
        }
    }

    pub fn kind(&self) -> Elem {
        Elem::Num(match self {
            TypedArray::Char(_) => return Elem::Char,
            TypedArray::Int(_) => return Elem::Int,
            TypedArray::Float(_) => return Elem::Float,
            TypedArray::Bool(_) => return Elem::Bool,
            TypedArray::I8(_) => Sized::I8,
            TypedArray::I16(_) => Sized::I16,
            TypedArray::I32(_) => Sized::I32,
            TypedArray::I64(_) => Sized::I64,
            TypedArray::U8(_) => Sized::U8,
            TypedArray::U16(_) => Sized::U16,
            TypedArray::U32(_) => Sized::U32,
            TypedArray::U64(_) => Sized::U64,
            TypedArray::F32(_) => Sized::F32,
            TypedArray::F64(_) => Sized::F64,
        })
    }

    /// The element type's name, as annotations write it.
    pub fn kind_name(&self) -> &'static str {
        match self.kind() {
            Elem::Num(k) => kind_name(k),
            Elem::Char => "char",
            Elem::Int => "int",
            Elem::Float => "float",
            Elem::Bool => "bool",
        }
    }

    pub fn len(&self) -> usize {
        each!(self, v => v.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes the elements take (for measurements and `fogu:system` reports).
    pub fn byte_size(&self) -> usize {
        each!(self, v => std::mem::size_of_val(v.as_slice()))
    }

    /// Element `i` (already in range) as a value.
    pub fn get(&self, i: usize) -> Value {
        match self {
            TypedArray::I8(v) => Value::Int(v[i] as i64),
            TypedArray::I16(v) => Value::Int(v[i] as i64),
            TypedArray::I32(v) => Value::Int(v[i] as i64),
            TypedArray::I64(v) => Value::Int(v[i]),
            TypedArray::U8(v) => Value::Int(v[i] as i64),
            TypedArray::U16(v) => Value::Int(v[i] as i64),
            TypedArray::U32(v) => Value::Int(v[i] as i64),
            TypedArray::U64(v) => Value::Int(v[i] as i64),
            TypedArray::F32(v) => Value::Float(v[i] as f64),
            TypedArray::F64(v) => Value::Float(v[i]),
            TypedArray::Char(v) => Value::Str(v[i].into()),
            TypedArray::Int(v) => Value::Int(v[i]),
            TypedArray::Float(v) => Value::Float(v[i]),
            TypedArray::Bool(v) => Value::Bool(v[i]),
        }
    }

    /// Element `i` (already in range) of an array of integers, as an int;
    /// `None` for the other kinds. For compiled code: no `Value` is built.
    #[inline]
    pub fn int_at(&self, i: usize) -> Option<i64> {
        Some(match self {
            TypedArray::Int(v) | TypedArray::I64(v) => v[i],
            TypedArray::U8(v) => v[i] as i64,
            TypedArray::I32(v) => v[i] as i64,
            TypedArray::U16(v) => v[i] as i64,
            TypedArray::I16(v) => v[i] as i64,
            TypedArray::U32(v) => v[i] as i64,
            TypedArray::I8(v) => v[i] as i64,
            TypedArray::U64(v) => v[i] as i64,
            _ => return None,
        })
    }

    /// Element `i` (already in range) of an array of floats; `None` for the others.
    #[inline]
    pub fn float_at(&self, i: usize) -> Option<f64> {
        Some(match self {
            TypedArray::Float(v) | TypedArray::F64(v) => v[i],
            TypedArray::F32(v) => v[i] as f64,
            _ => return None,
        })
    }

    /// Every element as a value (an ordinary list's contents).
    pub fn to_values(&self) -> Vec<Value> {
        (0..self.len()).map(|i| self.get(i)).collect()
    }

    /// The number to store for `v`, or the reason it cannot be stored. The
    /// rules of the annotation: an int must be in range; an int into a float
    /// array is converted; a float into an integer array is refused.
    fn number(&self, v: &Value) -> Result<Num, String> {
        let kind = match self.kind() {
            Elem::Num(k) => k,
            Elem::Char => {
                return match v {
                    Value::Str(s) => {
                        let mut cs = s.chars();
                        match (cs.next(), cs.next()) {
                            (Some(c), None) => Ok(Num::C(c)),
                            _ => Err(format!("str of {} characters (a char is one character)", s.char_len())),
                        }
                    }
                    other => Err(other.type_name().to_string()),
                }
            }
            Elem::Int => {
                return match v {
                    Value::Int(n) => Ok(Num::I(*n)),
                    other => Err(other.type_name().to_string()),
                }
            }
            Elem::Float => {
                return match v {
                    Value::Int(n) => Ok(Num::F(*n as f64)),
                    Value::Float(f) => Ok(Num::F(*f)),
                    other => Err(other.type_name().to_string()),
                }
            }
            Elem::Bool => {
                return match v {
                    Value::Bool(b) => Ok(Num::B(*b)),
                    other => Err(other.type_name().to_string()),
                }
            }
        };
        match (v, kind.is_float()) {
            (Value::Int(n), false) => {
                if kind.fits(*n) {
                    Ok(Num::I(*n))
                } else {
                    Err(super::ops::out_of_range(kind_name(kind), kind))
                }
            }
            (Value::Int(n), true) => Ok(Num::F(*n as f64)),
            (Value::Float(f), true) => Ok(Num::F(*f)),
            (other, _) => Err(other.type_name().to_string()),
        }
    }

    /// Stores `v` at `i` (already in range).
    pub fn set(&mut self, i: usize, v: &Value) -> Result<(), String> {
        let n = self.number(v)?;
        self.put(i, n);
        Ok(())
    }

    /// Appends `v`.
    pub fn push(&mut self, v: &Value) -> Result<(), String> {
        let n = self.number(v)?;
        let at = self.len();
        each!(self, vec => vec.push(Default::default()));
        self.put(at, n);
        Ok(())
    }

    /// Inserts `v` before `i` (`i <= len`).
    pub fn insert(&mut self, i: usize, v: &Value) -> Result<(), String> {
        let n = self.number(v)?;
        each!(self, vec => vec.insert(i, Default::default()));
        self.put(i, n);
        Ok(())
    }

    fn put(&mut self, i: usize, n: Num) {
        let (int, float) = match n {
            Num::I(x) => (x, x as f64),
            Num::F(f) => (f as i64, f),
            Num::C(c) => {
                if let TypedArray::Char(v) = self {
                    v[i] = c;
                }
                return;
            }
            Num::B(b) => {
                if let TypedArray::Bool(v) = self {
                    v[i] = b;
                }
                return;
            }
        };
        match self {
            TypedArray::I8(v) => v[i] = int as i8,
            TypedArray::I16(v) => v[i] = int as i16,
            TypedArray::I32(v) => v[i] = int as i32,
            TypedArray::I64(v) => v[i] = int,
            TypedArray::U8(v) => v[i] = int as u8,
            TypedArray::U16(v) => v[i] = int as u16,
            TypedArray::U32(v) => v[i] = int as u32,
            TypedArray::U64(v) => v[i] = int as u64,
            TypedArray::F32(v) => v[i] = float as f32,
            TypedArray::F64(v) => v[i] = float,
            TypedArray::Char(_) | TypedArray::Bool(_) => {}
            TypedArray::Int(v) => v[i] = int,
            TypedArray::Float(v) => v[i] = float,
        }
    }

    /// Removes and returns the last element.
    pub fn pop(&mut self) -> Option<Value> {
        let n = self.len();
        if n == 0 {
            return None;
        }
        let v = self.get(n - 1);
        each!(self, vec => { vec.pop(); });
        Some(v)
    }

    /// Removes and returns element `i` (already in range).
    pub fn remove(&mut self, i: usize) -> Value {
        let v = self.get(i);
        each!(self, vec => { vec.remove(i); });
        v
    }

    pub fn clear(&mut self) {
        each!(self, v => v.clear());
    }

    pub fn reverse(&mut self) {
        each!(self, v => v.reverse());
    }

    /// Sorts in the natural order of numbers (floats with NaN last).
    pub fn sort(&mut self) {
        match self {
            TypedArray::I8(v) => v.sort_unstable(),
            TypedArray::I16(v) => v.sort_unstable(),
            TypedArray::I32(v) => v.sort_unstable(),
            TypedArray::I64(v) => v.sort_unstable(),
            TypedArray::U8(v) => v.sort_unstable(),
            TypedArray::U16(v) => v.sort_unstable(),
            TypedArray::U32(v) => v.sort_unstable(),
            TypedArray::U64(v) => v.sort_unstable(),
            TypedArray::Char(v) => v.sort_unstable(),
            TypedArray::Int(v) => v.sort_unstable(),
            TypedArray::Bool(v) => v.sort_unstable(),
            TypedArray::Float(v) => v.sort_by(|a, b| a.total_cmp(b)),
            TypedArray::F32(v) => v.sort_by(|a, b| a.total_cmp(b)),
            TypedArray::F64(v) => v.sort_by(|a, b| a.total_cmp(b)),
        }
    }

    /// Elements `start..end` (already clamped) as a new array of the same kind.
    pub fn slice(&self, start: usize, end: usize) -> TypedArray {
        match self {
            TypedArray::I8(v) => TypedArray::I8(v[start..end].to_vec()),
            TypedArray::I16(v) => TypedArray::I16(v[start..end].to_vec()),
            TypedArray::I32(v) => TypedArray::I32(v[start..end].to_vec()),
            TypedArray::I64(v) => TypedArray::I64(v[start..end].to_vec()),
            TypedArray::U8(v) => TypedArray::U8(v[start..end].to_vec()),
            TypedArray::U16(v) => TypedArray::U16(v[start..end].to_vec()),
            TypedArray::U32(v) => TypedArray::U32(v[start..end].to_vec()),
            TypedArray::U64(v) => TypedArray::U64(v[start..end].to_vec()),
            TypedArray::F32(v) => TypedArray::F32(v[start..end].to_vec()),
            TypedArray::F64(v) => TypedArray::F64(v[start..end].to_vec()),
            TypedArray::Char(v) => TypedArray::Char(v[start..end].to_vec()),
            TypedArray::Int(v) => TypedArray::Int(v[start..end].to_vec()),
            TypedArray::Float(v) => TypedArray::Float(v[start..end].to_vec()),
            TypedArray::Bool(v) => TypedArray::Bool(v[start..end].to_vec()),
        }
    }

    /// How compiled code (the JIT) may store into this array without a check
    /// that can fail: [`STORE_INT`] any int, [`STORE_FLOAT`] any int or float,
    /// [`STORE_BOOL`] any bool, [`STORE_NONE`] nothing (a store may be out of
    /// range: the interpreter does it and reports).
    pub fn store_class(&self) -> u8 {
        match self {
            TypedArray::Int(_) | TypedArray::I64(_) => STORE_INT,
            TypedArray::Float(_) | TypedArray::F64(_) | TypedArray::F32(_) => STORE_FLOAT,
            TypedArray::Bool(_) => STORE_BOOL,
            _ => STORE_NONE,
        }
    }

    /// Appends an int to an array of class [`STORE_INT`] or [`STORE_FLOAT`].
    #[inline]
    pub fn push_i64(&mut self, n: i64) {
        match self {
            TypedArray::Int(v) | TypedArray::I64(v) => v.push(n),
            TypedArray::Float(v) | TypedArray::F64(v) => v.push(n as f64),
            TypedArray::F32(v) => v.push(n as f32),
            _ => {
                let _ = self.push(&Value::Int(n));
            }
        }
    }

    /// Appends a float to an array of class [`STORE_FLOAT`].
    #[inline]
    pub fn push_f64(&mut self, x: f64) {
        match self {
            TypedArray::Float(v) | TypedArray::F64(v) => v.push(x),
            TypedArray::F32(v) => v.push(x as f32),
            _ => {
                let _ = self.push(&Value::Float(x));
            }
        }
    }

    /// Appends a bool to an array of class [`STORE_BOOL`].
    #[inline]
    pub fn push_bool(&mut self, b: bool) {
        match self {
            TypedArray::Bool(v) => v.push(b),
            _ => {
                let _ = self.push(&Value::Bool(b));
            }
        }
    }

    /// An array of `kind` holding `items`, or the index of the first element
    /// that cannot be stored and why.
    pub fn from_values(kind: Elem, items: &[Value]) -> Result<TypedArray, (usize, String)> {
        let mut a = TypedArray::with_capacity(kind, items.len());
        for (i, v) in items.iter().enumerate() {
            a.push(v).map_err(|e| (i, e))?;
        }
        Ok(a)
    }
}

/// See [`TypedArray::store_class`].
pub const STORE_NONE: u8 = 0;
pub const STORE_INT: u8 = 1;
pub const STORE_FLOAT: u8 = 2;
pub const STORE_BOOL: u8 = 3;

enum Num {
    I(i64),
    F(f64),
    C(char),
    B(bool),
}

/// The short name of a sized type (`u8`, `f32` …).
pub fn kind_name(kind: Sized) -> &'static str {
    match kind {
        Sized::I8 => "i8",
        Sized::I16 => "i16",
        Sized::I32 => "i32",
        Sized::I64 => "i64",
        Sized::U8 => "u8",
        Sized::U16 => "u16",
        Sized::U32 => "u32",
        Sized::U64 => "u64",
        Sized::F32 => "f32",
        Sized::F64 => "f64",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_with_its_size() {
        let mut a = TypedArray::with_capacity(Elem::Num(Sized::U8), 0);
        for i in 0..1000 {
            a.push(&Value::Int(i % 256)).unwrap();
        }
        assert_eq!(a.len(), 1000);
        assert_eq!(a.byte_size(), 1000);
        assert!(matches!(a.get(300), Value::Int(44)));
        assert!(a.push(&Value::Int(256)).is_err());
        assert!(a.push(&Value::Float(1.0)).is_err());
        let mut f = TypedArray::with_capacity(Elem::Num(Sized::F32), 0);
        f.push(&Value::Int(3)).unwrap();
        f.push(&Value::Float(0.1)).unwrap();
        assert_eq!(f.byte_size(), 8);
        assert!(matches!(f.get(0), Value::Float(x) if x == 3.0));
        assert!(matches!(f.get(1), Value::Float(x) if x == 0.1f32 as f64));
        let mut i = TypedArray::from_values(Elem::Num(Sized::I16), &[Value::Int(-5), Value::Int(9), Value::Int(1)]).unwrap();
        i.sort();
        assert_eq!(i.to_values().len(), 3);
        assert!(matches!(i.get(0), Value::Int(-5)));
        assert!(matches!(i.remove(1), Value::Int(1)));
        assert!(TypedArray::from_values(Elem::Num(Sized::I8), &[Value::Int(1), Value::Int(200)]).is_err());
        let mut c = TypedArray::from_values(Elem::Char, &[Value::Str("é".into()), Value::Str("a".into())]).unwrap();
        assert_eq!(c.byte_size(), 8);
        c.sort();
        assert!(matches!(c.get(0), Value::Str(ref s) if &**s == "a"));
        assert!(c.push(&Value::Str("ab".into())).is_err());
        assert!(c.push(&Value::Int(1)).is_err());
        // the plain types: 8 bytes per int or float, 1 per bool
        let mut n = TypedArray::from_values(Elem::Int, &[Value::Int(i64::MAX), Value::Int(-1)]).unwrap();
        assert_eq!(n.byte_size(), 16);
        assert!(n.push(&Value::Float(1.5)).is_err());
        let fl = TypedArray::from_values(Elem::Float, &[Value::Int(2), Value::Float(0.5)]).unwrap();
        assert!(matches!(fl.get(0), Value::Float(x) if x == 2.0));
        let mut b = TypedArray::from_values(Elem::Bool, &[Value::Bool(true), Value::Bool(false)]).unwrap();
        assert_eq!(b.byte_size(), 2);
        b.sort();
        assert!(matches!(b.get(0), Value::Bool(false)));
        assert!(b.push(&Value::Int(1)).is_err());
    }
}
