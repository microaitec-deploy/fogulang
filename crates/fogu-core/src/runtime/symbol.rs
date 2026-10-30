//! Interned names: every variable name becomes a small integer (`Sym`) once,
//! so scopes are keyed by integers — no string hashing or comparison on each
//! lookup and no `String` allocated per declaration.
//!
//! The interner is per thread, like the values and environments that use it
//! (they are `Rc`, never shared between threads).

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Sym(u32);

#[derive(Default)]
struct Interner {
    ids: FxHashMap<Rc<str>, Sym>,
    names: Vec<Rc<str>>,
}

thread_local! {
    static INTERNER: RefCell<Interner> = RefCell::new(Interner::default());
}

/// The symbol of `name`, creating it the first time.
pub fn intern(name: &str) -> Sym {
    INTERNER.with(|i| {
        let mut i = i.borrow_mut();
        if let Some(&s) = i.ids.get(name) {
            return s;
        }
        let rc: Rc<str> = Rc::from(name);
        let s = Sym(i.names.len() as u32);
        i.names.push(Rc::clone(&rc));
        i.ids.insert(rc, s);
        s
    })
}

/// The symbol of `name` if it was ever interned: a name that never was cannot
/// be bound anywhere.
pub fn existing(name: &str) -> Option<Sym> {
    INTERNER.with(|i| i.borrow().ids.get(name).copied())
}

/// The text of a symbol.
pub fn name(s: Sym) -> Rc<str> {
    INTERNER.with(|i| Rc::clone(&i.borrow().names[s.0 as usize]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_stable() {
        let a = intern("alpha");
        assert_eq!(intern("alpha"), a);
        assert_ne!(intern("beta"), a);
        assert_eq!(&*name(a), "alpha");
        assert_eq!(existing("alpha"), Some(a));
        assert_eq!(existing("never-interned-name"), None);
    }
}
