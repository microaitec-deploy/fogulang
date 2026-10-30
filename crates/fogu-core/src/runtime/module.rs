use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::ast::ImportKind;
use super::environment::Environment;
use super::interpreter::Interpreter;
use super::lexer::Lexer;
use super::parser::Parser;
use super::value::{ModuleValue, Value};

use crate::native::context::NativeContext;

pub struct ModuleCache {
    modules: RefCell<HashMap<PathBuf, Rc<ModuleValue>>>,
    in_progress: RefCell<Vec<PathBuf>>,
    natives: NativeContext,
}

impl ModuleCache {
    pub fn new() -> Self {
        Self {
            modules: RefCell::new(HashMap::new()),
            in_progress: RefCell::new(Vec::new()),
            natives: NativeContext::new(),
        }
    }

    pub fn natives(&self) -> &NativeContext {
        &self.natives
    }

    pub fn resolve(
        &self,
        spec: &str,
        from_dir: &Path,
        interp: &Interpreter,
    ) -> Result<Rc<ModuleValue>, String> {
        if let Some(name) = spec.strip_prefix("fogu:") {
            return self
                .natives
                .get_module(name)
                .ok_or_else(|| format!("unknown built-in module: 'fogu:{}'", name));
        }

        let bare = !(spec.starts_with('.') || spec.starts_with('/') || spec.contains(":\\"));
        let pkg_path = if bare { crate::pkg::resolve_import(spec, from_dir)? } else { None };
        let path = match pkg_path {
            Some(p) => p,
            None => resolve_path(spec, from_dir)?,
        };
        // sources of a WebAssembly bundle live in memory (runtime/vfs.rs)
        let canonical = if super::vfs::contains(&path) {
            super::vfs::normalize(&path)
        } else {
            path.canonicalize().map_err(|e| format!("cannot resolve '{}': {}", path.display(), e))?
        };

        if let Some(m) = self.modules.borrow().get(&canonical) {
            return Ok(Rc::clone(m));
        }

        if self.in_progress.borrow().contains(&canonical) {
            return Err(format!(
                "cyclic dependency detected while loading '{}'",
                canonical.display()
            ));
        }

        self.in_progress.borrow_mut().push(canonical.clone());
        let result = self.load_module(&canonical, interp);
        self.in_progress.borrow_mut().pop();
        let exports = result?;

        self.modules
            .borrow_mut()
            .insert(canonical.clone(), Rc::clone(&exports));

        Ok(exports)
    }

    fn load_module(
        &self,
        canonical: &Path,
        interp: &Interpreter,
    ) -> Result<Rc<ModuleValue>, String> {
        let source = match super::vfs::read(canonical) {
            Some(s) => s,
            None => std::fs::read_to_string(canonical).map_err(|e| format!("error reading '{}': {}", canonical.display(), e))?,
        };

        let mut lexer = Lexer::new(&source);
        let tokens = lexer
            .tokenize()
            .map_err(|e| format!("{}: {}", display_path(canonical), e))?;

        let mut parser = Parser::new(tokens);
        let program = parser
            .parse_program()
            .map_err(|e| format!("{}: {}", display_path(canonical), e))?;
        super::resolver::check(&program, &interp.builtin_names())
            .map_err(|e| format!("{}: {}", display_path(canonical), e))?;

        let module_env = Environment::new_module(Rc::clone(interp.globals_ref()));
        module_env
            .borrow_mut()
            .set_module(&display_path(canonical));

        let exports = Rc::new(ModuleValue {
            name: display_path(canonical),
            exports: RefCell::new(HashMap::new()),
        });

        interp.eval_module(&program, module_env, canonical, Rc::clone(&exports))?;
        Ok(exports)
    }

    pub fn bind_import(
        &self,
        kind: &ImportKind,
        module: Rc<ModuleValue>,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<(), String> {
        match kind {
            ImportKind::Named { alias } => {
                let v = Value::Module(module);
                env.borrow_mut().define(alias.clone(), v, false);
                Ok(())
            }
            ImportKind::Namespace { alias } => {
                let v = Value::Module(module);
                env.borrow_mut().define(alias.clone(), v, false);
                Ok(())
            }
            ImportKind::Members { members } => {
                for (orig, alias) in members {
                    if orig.starts_with('_') {
                        return Err(format!(
                            "'{}' is private to module '{}' (names starting with '_')",
                            orig, module.name
                        ));
                    }
                    let v = module
                        .exports
                        .borrow()
                        .get(orig)
                        .cloned()
                        .ok_or_else(|| {
                            format!("'{}' is not exported by '{}'", orig, module.name)
                        })?;
                    env.borrow_mut().define(alias.clone(), v, false);
                }
                Ok(())
            }
        }
    }
}

/// Path for messages: relative to the current directory when possible and
/// without the `\\?\` prefix of Windows canonical paths.
pub fn display_path(p: &Path) -> String {
    if super::vfs::contains(p) {
        return super::vfs::display(p);
    }
    if let Ok(cwd) = std::env::current_dir().and_then(|c| c.canonicalize()) {
        if let Ok(rel) = p.strip_prefix(&cwd) {
            return rel.display().to_string();
        }
    }
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

/// Resolves a relative/absolute import path (adds `.fg` when missing).
pub fn resolve_path(spec: &str, from_dir: &Path) -> Result<PathBuf, String> {
    let p = Path::new(spec);
    let base = if p.is_absolute() {
        p.to_path_buf()
    } else {
        from_dir.join(p)
    };

    if base.extension().is_none() {
        let with_ext = base.with_extension("fg");
        if with_ext.exists() || super::vfs::exists(&with_ext) {
            return Ok(with_ext);
        }
    }

    Ok(base)
}