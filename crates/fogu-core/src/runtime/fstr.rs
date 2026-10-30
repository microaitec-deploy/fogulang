//! `Str`: the string type of Fogu values.
//!
//! A `Value::str` used to own a `String`, so reading a variable, iterating a
//! list or passing an argument copied the whole text. `Str` is a
//! reference-counted, copy-on-write string instead:
//!
//! * `clone` is O(1) (a reference count);
//! * mutating (`DerefMut`, `push_str`…) copies only if the text is shared,
//!   so `s = s + "x"` / `s += "x"` on an unshared string appends in place;
//! * the number of chars is cached, so `s.len()` is O(1) after the first call
//!   and indexing an ASCII string (`s[i]`) is O(1).
//!
//! It dereferences to `String` (and from there to `str`), so read-only code
//! uses it like a `String`.

use std::borrow::Borrow;
use std::cell::Cell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

const UNKNOWN: usize = usize::MAX;

#[derive(Clone)]
struct Inner {
    text: String,
    /// Cached `text.chars().count()` (`UNKNOWN` until computed).
    chars: Cell<usize>,
}

#[derive(Clone)]
pub struct Str(Rc<Inner>);

impl Str {
    pub fn new() -> Str {
        Str::from(String::new())
    }

    pub fn as_str(&self) -> &str {
        &self.0.text
    }

    /// The `String` inside: free when this is the only reference.
    pub fn into_string(self) -> String {
        match Rc::try_unwrap(self.0) {
            Ok(inner) => inner.text,
            Err(rc) => rc.text.clone(),
        }
    }

    /// Number of chars (cached).
    pub fn char_len(&self) -> usize {
        let n = self.0.chars.get();
        if n != UNKNOWN {
            return n;
        }
        let n = self.0.text.chars().count();
        self.0.chars.set(n);
        n
    }

    /// Only ASCII: char `i` is byte `i`.
    pub fn is_ascii_fast(&self) -> bool {
        self.char_len() == self.0.text.len()
    }

    /// The char at index `i` (already normalized and in range).
    pub fn char_at(&self, i: usize) -> Option<char> {
        if self.is_ascii_fast() {
            self.0.text.as_bytes().get(i).map(|&b| b as char)
        } else {
            self.0.text.chars().nth(i)
        }
    }

    /// Both point to the same text (no comparison needed).
    pub fn ptr_eq(a: &Str, b: &Str) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }
}

impl Default for Str {
    fn default() -> Str {
        Str::new()
    }
}

impl Deref for Str {
    type Target = String;
    fn deref(&self) -> &String {
        &self.0.text
    }
}

impl DerefMut for Str {
    /// Copy-on-write: copies the text only if it is shared.
    fn deref_mut(&mut self) -> &mut String {
        let inner = Rc::make_mut(&mut self.0);
        inner.chars.set(UNKNOWN);
        &mut inner.text
    }
}

impl From<String> for Str {
    fn from(text: String) -> Str {
        Str(Rc::new(Inner { text, chars: Cell::new(UNKNOWN) }))
    }
}

impl From<&str> for Str {
    fn from(s: &str) -> Str {
        Str::from(s.to_string())
    }
}

impl From<&String> for Str {
    fn from(s: &String) -> Str {
        Str::from(s.clone())
    }
}

impl From<&Str> for Str {
    fn from(s: &Str) -> Str {
        s.clone()
    }
}

impl From<char> for Str {
    fn from(c: char) -> Str {
        let mut text = String::with_capacity(c.len_utf8());
        text.push(c);
        Str(Rc::new(Inner { text, chars: Cell::new(1) }))
    }
}

impl From<Box<str>> for Str {
    fn from(s: Box<str>) -> Str {
        Str::from(String::from(s))
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
        &self.0.text
    }
}

impl AsRef<[u8]> for Str {
    fn as_ref(&self) -> &[u8] {
        self.0.text.as_bytes()
    }
}

impl AsRef<std::path::Path> for Str {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0.text)
    }
}

impl AsRef<std::ffi::OsStr> for Str {
    fn as_ref(&self) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(&self.0.text)
    }
}

impl Borrow<str> for Str {
    fn borrow(&self) -> &str {
        &self.0.text
    }
}

impl PartialEq for Str {
    fn eq(&self, other: &Str) -> bool {
        Str::ptr_eq(self, other) || self.0.text == other.0.text
    }
}

impl Eq for Str {}

impl PartialEq<str> for Str {
    fn eq(&self, other: &str) -> bool {
        self.0.text == other
    }
}

impl PartialEq<&str> for Str {
    fn eq(&self, other: &&str) -> bool {
        self.0.text == *other
    }
}

impl PartialEq<String> for Str {
    fn eq(&self, other: &String) -> bool {
        &self.0.text == other
    }
}

impl PartialEq<Str> for str {
    fn eq(&self, other: &Str) -> bool {
        self == other.0.text
    }
}

impl PartialEq<Str> for &str {
    fn eq(&self, other: &Str) -> bool {
        *self == other.0.text
    }
}

impl PartialEq<Str> for String {
    fn eq(&self, other: &Str) -> bool {
        *self == other.0.text
    }
}

impl PartialOrd for Str {
    fn partial_cmp(&self, other: &Str) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Str {
    fn cmp(&self, other: &Str) -> std::cmp::Ordering {
        self.0.text.cmp(&other.0.text)
    }
}

impl Hash for Str {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.text.hash(state)
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.text, f)
    }
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0.text, f)
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

    #[test]
    fn clone_is_shared_and_mutation_copies() {
        let a = Str::from("hello");
        let mut b = a.clone();
        assert!(Str::ptr_eq(&a, &b));
        b.push_str(" world");
        assert_eq!(a, "hello");
        assert_eq!(b, "hello world");
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
    }

    #[test]
    fn unique_append_does_not_copy() {
        let mut s = Str::from(String::with_capacity(64));
        s.push_str("x");
        let p = s.as_ptr();
        s = s + "y";
        assert_eq!(s.as_ptr(), p);
        assert_eq!(s.into_string(), "xy");
    }
}
