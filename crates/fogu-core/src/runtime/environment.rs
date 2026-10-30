use std::cell::RefCell;
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::symbol::{self, Sym};
use super::value::Value;

/// Incremented whenever a binding of a *stable* environment (globals/modules)
/// is redefined. The JIT uses it to invalidate code that assumed the old
/// value.
static REDEFINE_EPOCH: AtomicU64 = AtomicU64::new(0);

pub fn redefine_epoch() -> u64 {
    REDEFINE_EPOCH.load(Ordering::Relaxed)
}

/// A scope with more names than this moves from the vector to the map.
const SMALL_MAX: usize = 16;

pub struct Environment {
    /// Block and call scopes hold a handful of names: a vector searched
    /// linearly beats hashing, costs one allocation (none when the scope is
    /// reused by a loop) and keeps the values in one cache line.
    small: Vec<(Sym, Value, bool)>,
    /// Stable environments (globals, modules) and the rare big scope: keyed by
    /// interned names (`Sym`), so a lookup hashes an integer, never a string.
    values: FxHashMap<Sym, (Value, bool)>,
    /// Which of the two holds the bindings.
    is_small: bool,
    parent: Option<Rc<RefCell<Environment>>>,
    /// Top-level environment (globals or module): lives for the whole program.
    stable: bool,
    /// Global native functions: the program may redeclare them.
    builtins: FxHashSet<Sym>,
    /// Name of the module (file) this top-level environment belongs to.
    module: Option<Rc<str>>,
}

/// Result of [`Environment::lookup`].
pub struct Binding {
    pub value: Value,
    pub mutable: bool,
    /// The binding lives in a stable environment (global/module).
    pub stable: bool,
}

impl Environment {
    /// Top-level environment (globals or a module).
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            small: Vec::new(),
            values: FxHashMap::default(),
            is_small: false,
            parent: None,
            stable: true,
            builtins: FxHashSet::default(),
            module: None,
        }))
    }

    /// Module environment: child of the globals, but also stable.
    pub fn new_module(parent: Rc<RefCell<Environment>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            small: Vec::new(),
            values: FxHashMap::default(),
            is_small: false,
            parent: Some(parent),
            stable: true,
            builtins: FxHashSet::default(),
            module: None,
        }))
    }

    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            small: Vec::new(),
            values: FxHashMap::default(),
            is_small: true,
            parent: Some(parent),
            stable: false,
            builtins: FxHashSet::default(),
            module: None,
        }))
    }

    pub fn set_module(&mut self, name: &str) {
        self.module = Some(Rc::from(name));
    }

    /// Module where the code of this environment runs (the one of the nearest
    /// top-level environment). It does not depend on global state, so it is
    /// exact even with interleaved `fg` tasks.
    pub fn module_name(&self) -> Option<Rc<str>> {
        match (&self.module, &self.parent) {
            (Some(m), _) => Some(Rc::clone(m)),
            (None, Some(p)) => p.borrow().module_name(),
            (None, None) => None,
        }
    }

    // ---------- the two storages ----------

    #[inline]
    fn find(&self, s: Sym) -> Option<(&Value, bool)> {
        if self.is_small {
            self.small.iter().find(|(k, _, _)| *k == s).map(|(_, v, m)| (v, *m))
        } else {
            self.values.get(&s).map(|(v, m)| (v, *m))
        }
    }

    #[inline]
    fn find_mut(&mut self, s: Sym) -> Option<(&mut Value, bool)> {
        if self.is_small {
            self.small.iter_mut().find(|(k, _, _)| *k == s).map(|(_, v, m)| (v, *m))
        } else {
            self.values.get_mut(&s).map(|(v, m)| (v, *m))
        }
    }

    #[inline]
    fn contains(&self, s: Sym) -> bool {
        if self.is_small {
            self.small.iter().any(|(k, _, _)| *k == s)
        } else {
            self.values.contains_key(&s)
        }
    }

    /// Adds a binding that is not there yet.
    fn insert_new(&mut self, s: Sym, value: Value, mutable: bool) {
        if self.is_small {
            if self.small.len() < SMALL_MAX {
                self.small.push((s, value, mutable));
                return;
            }
            // a big scope: move to the map
            self.is_small = false;
            for (k, v, m) in self.small.drain(..) {
                self.values.insert(k, (v, m));
            }
        }
        self.values.insert(s, (value, mutable));
    }

    /// Names defined in this environment (excluding the parents').
    pub fn names(&self) -> Vec<String> {
        if self.is_small {
            self.small.iter().map(|(s, _, _)| symbol::name(*s).to_string()).collect()
        } else {
            self.values.keys().map(|s| symbol::name(*s).to_string()).collect()
        }
    }

    /// Names visible from this environment (its own and the parents').
    pub fn all_names(&self) -> Vec<String> {
        let mut out = self.names();
        if let Some(p) = &self.parent {
            out.extend(p.borrow().all_names());
        }
        out
    }

    /// Own binding (value, mutable) of this environment.
    pub fn get_own(&self, name: &str) -> Option<(Value, bool)> {
        let (v, m) = self.find(symbol::existing(name)?)?;
        Some((v.clone(), m))
    }

    /// A mutable binding the program may still declare (`fogu:vm` sandbox keys).
    pub fn define_shadowable(&mut self, name: impl AsRef<str>, value: Value) {
        let s = symbol::intern(name.as_ref());
        self.builtins.insert(s);
        self.define_sym(s, value, true);
    }

    /// Defines a global native function (may be redeclared by the program).
    pub fn define_builtin(&mut self, name: impl AsRef<str>, value: Value) {
        let s = symbol::intern(name.as_ref());
        self.builtins.insert(s);
        self.define_sym(s, value, false);
    }

    /// Program declaration (`let`, `const`, `fn`, `struct`): it is an error if
    /// the name already exists in this same scope (spec §13.1).
    pub fn declare(&mut self, name: impl AsRef<str>, value: Value, mutable: bool) -> Result<(), String> {
        self.declare_sym(symbol::intern(name.as_ref()), value, mutable)
    }

    pub fn declare_sym(&mut self, s: Sym, value: Value, mutable: bool) -> Result<(), String> {
        if self.contains(s) && !self.builtins.remove(&s) {
            return Err(format!("'{}' is already declared in this scope", symbol::name(s)));
        }
        self.define_sym(s, value, mutable);
        Ok(())
    }

    /// Defines or replaces a binding (parameters, loop variables, imports).
    pub fn define(&mut self, name: impl AsRef<str>, value: Value, mutable: bool) {
        self.define_sym(symbol::intern(name.as_ref()), value, mutable);
    }

    pub fn define_sym(&mut self, s: Sym, value: Value, mutable: bool) {
        if let Some((slot, _)) = self.find_mut(s) {
            *slot = value;
            if self.stable {
                REDEFINE_EPOCH.fetch_add(1, Ordering::Relaxed);
            }
            // the mutability follows the new definition
            if self.is_small {
                if let Some(e) = self.small.iter_mut().find(|(k, _, _)| *k == s) {
                    e.2 = mutable;
                }
            } else if let Some(e) = self.values.get_mut(&s) {
                e.1 = mutable;
            }
            return;
        }
        self.insert_new(s, value, mutable);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.get_sym(symbol::existing(name)?)
    }

    pub fn get_sym(&self, s: Sym) -> Option<Value> {
        if let Some((v, _)) = self.find(s) {
            return Some(v.clone());
        }
        match &self.parent {
            Some(parent) => parent.borrow().get_sym(s),
            None => None,
        }
    }

    /// Looks up the nearest binding, with the metadata the JIT needs.
    pub fn lookup(&self, name: &str) -> Option<Binding> {
        self.lookup_sym(symbol::existing(name)?)
    }

    fn lookup_sym(&self, s: Sym) -> Option<Binding> {
        if let Some((v, m)) = self.find(s) {
            return Some(Binding {
                value: v.clone(),
                mutable: m,
                stable: self.stable,
            });
        }
        match &self.parent {
            Some(parent) => parent.borrow().lookup_sym(s),
            None => None,
        }
    }

    /// Sets a binding of this environment (not a parent's), reusing its slot
    /// when it exists: a loop variable costs no allocation per iteration.
    pub fn set_own(&mut self, name: &str, value: Value) {
        self.set_own_sym(symbol::intern(name), value);
    }

    pub fn set_own_sym(&mut self, s: Sym, value: Value) {
        match self.find_mut(s) {
            Some((slot, _)) => *slot = value,
            None => self.insert_new(s, value, false),
        }
    }

    /// A loop scope nobody captured, ready for the next iteration: its
    /// bindings are dropped but the table keeps its memory.
    pub fn reset(&mut self) {
        self.small.clear();
        self.values.clear();
        if !self.stable {
            self.is_small = true;
        }
    }

    /// Runs `f` on the slot of the nearest binding of `name` (which must be
    /// mutable, as for [`assign`](Self::assign)). Lets `s = s + x` append to
    /// the string in place.
    pub fn update<R>(&mut self, name: &str, f: impl FnOnce(&mut Value) -> R) -> Result<R, String> {
        match symbol::existing(name) {
            Some(s) => self.update_sym(s, f),
            None => Err(format!("variable '{}' is not defined", name)),
        }
    }

    pub fn update_sym<R>(&mut self, s: Sym, f: impl FnOnce(&mut Value) -> R) -> Result<R, String> {
        if let Some((existing, mutable)) = self.find_mut(s) {
            if !mutable {
                return Err(format!(
                    "cannot assign to '{}' because it is a const binding",
                    symbol::name(s)
                ));
            }
            return Ok(f(existing));
        }
        if let Some(parent) = &self.parent {
            return parent.borrow_mut().update_sym(s, f);
        }
        Err(format!("variable '{}' is not defined", symbol::name(s)))
    }

    pub fn assign(&mut self, name: &str, value: Value) -> Result<(), String> {
        match symbol::existing(name) {
            Some(s) => self.assign_sym(s, value),
            None => Err(format!("variable '{}' is not defined", name)),
        }
    }

    pub fn assign_sym(&mut self, s: Sym, value: Value) -> Result<(), String> {
        if let Some((existing, mutable)) = self.find_mut(s) {
            if !mutable {
                return Err(format!(
                    "cannot assign to '{}' because it is a const binding",
                    symbol::name(s)
                ));
            }
            *existing = value;
            return Ok(());
        }
        if let Some(parent) = &self.parent {
            return parent.borrow_mut().assign_sym(s, value);
        }
        Err(format!("variable '{}' is not defined", symbol::name(s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_scopes_grow_into_maps() {
        let g = Environment::new();
        let e = Environment::with_parent(g);
        let mut env = e.borrow_mut();
        for i in 0..40 {
            env.define(format!("v{}", i), Value::Int(i), true);
        }
        assert!(!env.is_small);
        assert!(matches!(env.get("v0"), Some(Value::Int(0))));
        assert!(matches!(env.get("v39"), Some(Value::Int(39))));
        assert!(env.assign("v5", Value::Int(-5)).is_ok());
        assert!(matches!(env.get("v5"), Some(Value::Int(-5))));
        assert!(env.declare("v5", Value::Nil, true).is_err());
        env.reset();
        assert!(env.is_small);
        assert!(env.get("v0").is_none());
    }

    #[test]
    fn declare_and_assign_in_a_small_scope() {
        let g = Environment::new();
        g.borrow_mut().declare("x", Value::Int(1), true).unwrap();
        let e = Environment::with_parent(g);
        let mut env = e.borrow_mut();
        env.declare("y", Value::Int(2), false).unwrap();
        assert!(env.declare("y", Value::Int(3), false).is_err());
        assert!(env.assign("y", Value::Int(3)).is_err()); // const
        assert!(env.assign("x", Value::Int(10)).is_ok()); // the parent's
        assert!(matches!(env.get("x"), Some(Value::Int(10))));
        assert_eq!(env.names(), vec!["y".to_string()]);
    }
}
