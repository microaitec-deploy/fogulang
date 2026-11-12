//! `Str`: the string type of Fogu values.
//!
//! A `Value::str` used to own a `String`, so reading a variable, iterating a
//! list or passing an argument copied the whole text. `Str` is instead:
//!
//! * **inline** for texts of up to 15 bytes (most keys, words, numbers and
//!   pieces of a `split`): the bytes live in the value itself, so creating,
//!   copying and dropping such a string touches no memory outside the value;
//! * a reference-counted, copy-on-write `String` for longer texts: `clone`
//!   is O(1), and mutating (`push_str`, [`Str::make_mut`]) copies only if the
//!   text is shared, so `s = s + "x"` / `s += "x"` on an unshared string
//!   appends in place;
//! * the number of chars of a long text is cached, so `s.len()` is O(1)
//!   after the first call and indexing an ASCII string (`s[i]`) is O(1).
//!
//! It dereferences to `str`, so read-only code uses it like a `&str`.

use std::borrow::Borrow;
use std::cell::Cell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::Rc;

const UNKNOWN: usize = usize::MAX;

/// Bytes an inline string holds.
const INLINE: usize = 15;

/// A length of 0..=15 as a type: the unused values of the byte let `Repr`
/// (and `Value`) tell the variants apart without growing.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
enum Len {
    L0, L1, L2, L3, L4, L5, L6, L7, L8, L9, L10, L11, L12, L13, L14, L15,
}

impl Len {
    #[inline]
    fn new(n: usize) -> Len {
        debug_assert!(n <= INLINE);
        // SAFETY: every value 0..=15 is a variant of this `repr(u8)` enum
        unsafe { std::mem::transmute::<u8, Len>(n as u8) }
    }
}

#[derive(Clone)]
struct Inner {
    text: String,
    /// Cached `text.chars().count()` (`UNKNOWN` until computed).
    chars: Cell<usize>,
}

#[derive(Clone)]
enum Repr {
    /// The first `len` bytes of `buf` are the text (valid UTF-8).
    Inline { buf: [u8; INLINE], len: Len },
    Heap(Rc<Inner>),
}

#[derive(Clone)]
pub struct Str(Repr);

// a string must fit the 16-byte payload of `Value` (24 bytes with its tag)
const _: () = assert!(std::mem::size_of::<Str>() == 16);

impl Str {
    #[inline]
    pub fn new() -> Str {
        Str(Repr::Inline { buf: [0; INLINE], len: Len::L0 })
    }

    /// `s` (at most `INLINE` bytes) as an inline string.
    #[inline]
    fn inline(s: &str) -> Str {
        let mut buf = [0u8; INLINE];
        buf[..s.len()].copy_from_slice(s.as_bytes());
        Str(Repr::Inline { buf, len: Len::new(s.len()) })
    }

    fn heap(text: String) -> Str {
        Str(Repr::Heap(Rc::new(Inner { text, chars: Cell::new(UNKNOWN) })))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            // SAFETY: the first `len` bytes were copied from a `str`
            Repr::Inline { buf, len } => unsafe { std::str::from_utf8_unchecked(buf.get_unchecked(..*len as usize)) },
            Repr::Heap(rc) => &rc.text,
        }
    }

    /// The text as a `String`: free when this is the only reference to a
    /// long text.
    pub fn into_string(self) -> String {
        match self.0 {
            Repr::Inline { .. } => self.as_str().to_string(),
            Repr::Heap(rc) => match Rc::try_unwrap(rc) {
                Ok(inner) => inner.text,
                Err(rc) => rc.text.clone(),
            },
        }
    }

    /// Number of chars (cached for long texts).
    #[inline]
    pub fn char_len(&self) -> usize {
        match &self.0 {
            Repr::Inline { buf, len } => {
                let n = *len as usize;
                // UTF-8 continuation bytes (10xxxxxx) start no char
                buf[..n].iter().filter(|&&b| (b as i8) >= -0x40).count()
            }
            Repr::Heap(rc) => {
                let n = rc.chars.get();
                if n != UNKNOWN {
                    return n;
                }
                let n = rc.text.chars().count();
                rc.chars.set(n);
                n
            }
        }
    }

    /// Only ASCII: char `i` is byte `i`.
    #[inline]
    pub fn is_ascii_fast(&self) -> bool {
        self.char_len() == self.as_str().len()
    }

    /// The char at index `i` (already normalized and in range).
    pub fn char_at(&self, i: usize) -> Option<char> {
        let s = self.as_str();
        if self.is_ascii_fast() {
            s.as_bytes().get(i).map(|&b| b as char)
        } else {
            s.chars().nth(i)
        }
    }

    /// Both are the same text for sure, without comparing long texts.
    pub fn ptr_eq(a: &Str, b: &Str) -> bool {
        match (&a.0, &b.0) {
            (Repr::Heap(x), Repr::Heap(y)) => Rc::ptr_eq(x, y),
            (Repr::Inline { .. }, Repr::Inline { .. }) => a.as_str() == b.as_str(),
            _ => false,
        }
    }

    /// Appends `s`: in place when the text is inline and still fits, or is
    /// not shared; otherwise on a copy (copy-on-write).
    pub fn push_str(&mut self, s: &str) {
        match &mut self.0 {
            Repr::Inline { buf, len } => {
                let n = *len as usize;
                let total = n + s.len();
                if total <= INLINE {
                    buf[n..total].copy_from_slice(s.as_bytes());
                    *len = Len::new(total);
                    return;
                }
                // room to keep growing: a text built piece by piece is the
                // usual reason to outgrow the inline form
                let mut text = String::with_capacity(total.max(24) * 2);
                // SAFETY: as in `as_str`
                text.push_str(unsafe { std::str::from_utf8_unchecked(&buf[..n]) });
                text.push_str(s);
                *self = Str::heap(text);
            }
            Repr::Heap(rc) => {
                let inner = Rc::make_mut(rc);
                inner.chars.set(UNKNOWN);
                inner.text.push_str(s);
            }
        }
    }

    /// Appends one char (see [`Str::push_str`]).
    pub fn push(&mut self, c: char) {
        self.push_str(c.encode_utf8(&mut [0u8; 4]));
    }

    /// The text as a `String` to change in place (copy-on-write: the text is
    /// copied first if it is shared).
    pub fn make_mut(&mut self) -> &mut String {
        if let Repr::Inline { .. } = self.0 {
            *self = Str::heap(self.as_str().to_string());
        }
        match &mut self.0 {
            Repr::Heap(rc) => {
                let inner = Rc::make_mut(rc);
                inner.chars.set(UNKNOWN);
                &mut inner.text
            }
            Repr::Inline { .. } => unreachable!(),
        }
    }

    /// Empties the text.
    pub fn clear(&mut self) {
        *self = Str::new();
    }
}

impl Default for Str {
    fn default() -> Str {
        Str::new()
    }
}

impl Deref for Str {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl From<String> for Str {
    #[inline]
    fn from(text: String) -> Str {
        if text.len() <= INLINE { Str::inline(&text) } else { Str::heap(text) }
    }
}

impl From<&str> for Str {
    #[inline]
    fn from(s: &str) -> Str {
        if s.len() <= INLINE { Str::inline(s) } else { Str::heap(s.to_string()) }
    }
}

impl From<&String> for Str {
    fn from(s: &String) -> Str {
        Str::from(s.as_str())
    }
}

impl From<&Str> for Str {
    fn from(s: &Str) -> Str {
        s.clone()
    }
}

impl From<char> for Str {
    fn from(c: char) -> Str {
        Str::inline(c.encode_utf8(&mut [0u8; 4]))
    }
}

impl From<Box<str>> for Str {
    fn from(s: Box<str>) -> Str {
        Str::from(String::from(s))
    }
}

impl PartialEq<Str> for std::borrow::Cow<'_, str> {
    fn eq(&self, other: &Str) -> bool {
        self.as_ref() == other.as_str()
    }
}

impl From<std::borrow::Cow<'_, str>> for Str {
    fn from(s: std::borrow::Cow<'_, str>) -> Str {
        Str::from(s.into_owned())
    }
}

impl From<Str> for String {
    fn from(s: Str) -> String {
        s.into_string()
    }
}

impl AsRef<str> for Str {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for Str {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl AsRef<std::path::Path> for Str {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(self.as_str())
    }
}

impl AsRef<std::ffi::OsStr> for Str {
    fn as_ref(&self) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(self.as_str())
    }
}

impl Borrow<str> for Str {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for Str {
    fn eq(&self, other: &Str) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Str {}

impl PartialEq<str> for Str {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Str {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for Str {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<Str> for str {
    fn eq(&self, other: &Str) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<Str> for &str {
    fn eq(&self, other: &Str) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<Str> for String {
    fn eq(&self, other: &Str) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialOrd for Str {
    fn partial_cmp(&self, other: &Str) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Str {
    fn cmp(&self, other: &Str) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for Str {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl std::ops::Add<&str> for Str {
    type Output = Str;
    /// Appends in place when the text is not shared.
    fn add(mut self, rhs: &str) -> Str {
        self.push_str(rhs);
        self
    }
}

impl std::ops::AddAssign<&str> for Str {
    fn add_assign(&mut self, rhs: &str) {
        self.push_str(rhs);
    }
}

impl<'a> FromIterator<&'a char> for Str {
    fn from_iter<I: IntoIterator<Item = &'a char>>(iter: I) -> Str {
        Str::from(iter.into_iter().collect::<String>())
    }
}

impl FromIterator<String> for Str {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Str {
        Str::from(iter.into_iter().collect::<String>())
    }
}

impl<'a> FromIterator<&'a str> for Str {
    fn from_iter<I: IntoIterator<Item = &'a str>>(iter: I) -> Str {
        Str::from(iter.into_iter().collect::<String>())
    }
}

impl From<Str> for std::borrow::Cow<'static, str> {
    fn from(s: Str) -> Self {
        std::borrow::Cow::Owned(s.into_string())
    }
}

impl FromIterator<char> for Str {
    fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> Str {
        Str::from(iter.into_iter().collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::Str;

    const LONG: &str = "a text that does not fit inline";

    #[test]
    fn clone_is_shared_and_mutation_copies() {
        for text in ["hello", LONG] {
            let a = Str::from(text);
            let mut b = a.clone();
            assert!(Str::ptr_eq(&a, &b));
            b.push_str(" world");
            assert_eq!(a, text);
            assert_eq!(b, format!("{} world", text));
        }
    }

    #[test]
    fn char_cache() {
        let mut s = Str::from("añb");
        assert_eq!(s.char_len(), 3);
        assert!(!s.is_ascii_fast());
        assert_eq!(s.char_at(1), Some('ñ'));
        s.push('c');
        assert_eq!(s.char_len(), 4);
        let t = Str::from("abc");
        assert_eq!(t.char_at(2), Some('c'));
        let mut long = Str::from(format!("{}ñ", LONG));
        assert_eq!(long.char_len(), LONG.len() + 1);
        assert!(!long.is_ascii_fast());
        long.push('!');
        assert_eq!(long.char_len(), LONG.len() + 2);
        assert_eq!(long.char_at(LONG.len()), Some('ñ'));
    }

    #[test]
    fn unique_append_does_not_copy() {
        let mut s = Str::from(String::with_capacity(64));
        s.push_str(LONG);
        s.push_str("x");
        let p = s.as_ptr();
        s = s + "y";
        assert_eq!(s.as_ptr(), p);
        assert_eq!(s.into_string(), format!("{}xy", LONG));
    }

    #[test]
    fn inline_boundaries() {
        let mut s = Str::new();
        let mut want = String::new();
        // across 15 bytes (inline) into the heap form, one byte and one
        // multi-byte char at a time
        for i in 0..40 {
            if i % 3 == 0 {
                s.push('é');
                want.push('é');
            } else {
                s.push_str("x");
                want.push('x');
            }
            assert_eq!(s.as_str(), want);
            assert_eq!(s.char_len(), want.chars().count());
            assert_eq!(Str::from(want.as_str()), s);
            assert_eq!(Str::from(want.clone()), s);
        }
        for n in [0usize, 1, 14, 15, 16, 17] {
            let text = "z".repeat(n);
            let a = Str::from(text.as_str());
            assert_eq!(a.len(), n);
            assert_eq!(a.clone().into_string(), text);
            let mut b = a.clone();
            b.make_mut().push('!');
            assert_eq!(a, text);
            assert_eq!(b, format!("{}!", text));
        }
    }

    #[test]
    fn hashes_like_str() {
        use std::collections::HashMap;
        let mut m: HashMap<Str, i32> = HashMap::new();
        m.insert(Str::from("short"), 1);
        m.insert(Str::from(LONG), 2);
        assert_eq!(m.get("short"), Some(&1));
        assert_eq!(m.get(LONG), Some(&2));
    }
}
