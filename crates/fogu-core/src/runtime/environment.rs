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

pub struct Environment {
    /// Keyed by interned names (`Sym`): a lookup hashes an integer, never a string.
    values: FxHashMap<Sym, (Value, bool)>,
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
            values: FxHashMap::default(),
            parent: None,
            stable: true,
            builtins: FxHashSet::default(),
            module: None,
        }))
    }

    /// Module environment: child of the globals, but also stable.
    pub fn new_module(parent: Rc<RefCell<Environment>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            values: FxHashMap::default(),
            parent: Some(parent),
            stable: true,
            builtins: FxHashSet::default(),
            module: None,
        }))
    }

    pub fn with_parent(parent: Rc<RefCell<Environment>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            values: FxHashMap::default(),
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

    /// Names defined in this environment (excluding the parents').
    pub fn names(&self) -> Vec<String> {
        self.values.keys().map(|s| symbol::name(*s).to_string()).collect()
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
        self.values.get(&symbol::existing(name)?).cloned()
    }

    /// A mutable binding the program may still declare (`fogu:vm` sandbox keys).
    pub fn define_shadowable(&mut self, name: String, value: Value) {
        let s = symbol::intern(&name);
        self.builtins.insert(s);
        self.define_sym(s, value, true);
    }

    /// Defines a global native function (may be redeclared by the program).
    pub fn define_builtin(&mut self, name: String, value: Value) {
        let s = symbol::intern(&name);
        self.builtins.insert(s);
        self.define_sym(s, value, false);
    }

    /// Program declaration (`let`, `const`, `fn`, `struct`): it is an error if
    /// the name already exists in this same scope (spec §13.1).
    pub fn declare(&mut self, name: String, value: Value, mutable: bool) -> Result<(), String> {
        self.declare_sym(symbol::intern(&name), value, mutable)
    }

    pub fn declare_sym(&mut self, s: Sym, value: Value, mutable: bool) -> Result<(), String> {
        if self.values.contains_key(&s) && !self.builtins.remove(&s) {
            return Err(format!("'{}' is already declared in this scope", symbol::name(s)));
        }
        self.define_sym(s, value, mutable);
        Ok(())
    }

    /// Defines or replaces a binding (parameters, loop variables, imports).
    pub fn define(&mut self, name: String, value: Value, mutable: bool) {
        self.define_sym(symbol::intern(&name), value, mutable);
    }

    pub fn define_sym(&mut self, s: Sym, value: Value, mutable: bool) {
        if self.stable && self.values.contains_key(&s) {
            REDEFINE_EPOCH.fetch_add(1, Ordering::Relaxed);
        }
        self.values.insert(s, (value, mutable));
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.get_sym(symbol::existing(name)?)
    }

    pub fn get_sym(&self, s: Sym) -> Option<Value> {
        if let Some((v, _)) = self.values.get(&s) {
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
        if let Some((v, m)) = self.values.get(&s) {
            return Some(Binding {
                value: v.clone(),
                mutable: *m,
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
        match self.values.get_mut(&s) {
            Some(slot) => slot.0 = value,
            None => self.define_sym(s, value, false),
        }
    }

    /// A loop scope nobody captured, ready for the next iteration: its
    /// bindings are dropped but the table keeps its memory.
    pub fn reset(&mut self) {
        self.values.clear();
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
        if let Some((existing, mutable)) = self.values.get_mut(&s) {
            if !*mutable {
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
        if let Some((existing, mutable)) = self.values.get_mut(&s) {
            if !*mutable {
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
