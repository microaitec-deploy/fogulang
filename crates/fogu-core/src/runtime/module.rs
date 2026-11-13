use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::ast::ImportKind;
use super::environment::Environment;
use super::interpreter::Interpreter;
use super::lexer::Lexer;
use super::parser::Parser;
use super::value::{ModuleValue, Value, ERR_VALUE_PREFIX};

use crate::native::context::NativeContext;

/// The export key of `export default …` (`default` is a keyword, so no
/// declared name can collide with it).
pub const DEFAULT_EXPORT: &str = "default";

/// What `import … from "spec"` loads: a Fogu module or the value of a data file.
#[derive(Clone)]
pub enum Imported {
    Module(Rc<ModuleValue>),
    /// `.json`, `.yaml`, `.txt`, `.png`… (see [`data_kind`]).
    Data { name: String, value: Value },
}

impl Imported {
    /// `import x from`: the default export of a module that has one, the
    /// module object otherwise; the whole value of a data file.
    pub fn default_value(&self) -> Value {
        match self {
            Imported::Module(m) => match m.exports.borrow().get(DEFAULT_EXPORT) {
                Some(v) => v.clone(),
                None => Value::Module(Rc::clone(m)),
            },
            Imported::Data { value, .. } => value.clone(),
        }
    }

    /// `import * as ns from` / `import(path)`: the module object, or the
    /// value of a data file.
    pub fn namespace(&self) -> Value {
        match self {
            Imported::Module(m) => Value::Module(Rc::clone(m)),
            Imported::Data { value, .. } => value.clone(),
        }
    }

    /// `import { name } from` / `export { name } from`: an export of a
    /// module, or a key of the map a data file holds.
    pub fn member(&self, name: &str) -> Result<Value, String> {
        match self {
            Imported::Module(module) => {
                if name.starts_with('_') {
                    return Err(format!(
                        "'{}' is private to module '{}' (names starting with '_')",
                        name, module.name
                    ));
                }
                module
                    .exports
                    .borrow()
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("'{}' is not exported by '{}'", name, module.name))
            }
            Imported::Data { name: file, value } => match value {
                Value::Map(m) => match m.borrow().get(name) {
                    Some(v) => Ok(v.clone()),
                    None if name == DEFAULT_EXPORT => Ok(value.clone()),
                    None => Err(format!("'{}' is not a key of '{}'", name, file)),
                },
                _ if name == DEFAULT_EXPORT => Ok(value.clone()),
                other => Err(format!(
                    "cannot import '{}' from '{}': its value is a {}, not a map",
                    name,
                    file,
                    other.type_name()
                )),
            },
        }
    }

    /// `export * from`: every public export of a module (not `default`), or
    /// every key of the map a data file holds.
    pub fn all(&self) -> Result<Vec<(String, Value)>, String> {
        match self {
            Imported::Module(m) => Ok(m
                .exports
                .borrow()
                .iter()
                .filter(|(k, _)| k.as_str() != DEFAULT_EXPORT && !k.starts_with('_'))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()),
            Imported::Data { name, value } => match value {
                Value::Map(m) => Ok(m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
                other => Err(format!(
                    "cannot re-export the keys of '{}': its value is a {}, not a map",
                    name,
                    other.type_name()
                )),
            },
        }
    }
}

/// How a data file is imported, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    /// `.json`, parsed by the core.
    Json,
    /// `.json5 .yaml .yml .toml .xml .csv`, parsed by the standard library
    /// ([`crate::host::Host::load_data`]).
    Parsed,
    /// `.txt .md .html …` → a `str`.
    Text,
    /// images, fonts, `.bin`, `.wasm` … → a `buffer`.
    Binary,
}

const PARSED_EXTS: &[&str] = &["json5", "yaml", "yml", "toml", "xml", "csv"];
const TEXT_EXTS: &[&str] = &["txt", "md", "html", "htm", "css", "svg", "sql", "js"];
const BINARY_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "avif", "bin", "wasm", "woff", "woff2", "ttf", "otf", "pdf",
];

/// The kind of a data file by its extension; `None` for Fogu sources (`.fg`,
/// no extension) and unknown extensions (see [`unknown_type`]).
pub fn data_kind(path: &Path) -> Option<DataKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let ext = ext.as_str();
    if ext == "json" {
        Some(DataKind::Json)
    } else if PARSED_EXTS.contains(&ext) {
        Some(DataKind::Parsed)
    } else if TEXT_EXTS.contains(&ext) {
        Some(DataKind::Text)
    } else if BINARY_EXTS.contains(&ext) {
        Some(DataKind::Binary)
    } else {
        None
    }
}

/// The error for a file whose extension is neither `.fg` nor a data type;
/// `None` when the path can be loaded.
pub fn unknown_type(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "fg" || data_kind(path).is_some() {
        return None;
    }
    let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let list = |exts: &[&str]| exts.iter().map(|e| format!(".{}", e)).collect::<Vec<_>>().join(" ");
    Some(format!(
        "cannot import '{}': unknown file type (.fg modules; data: .json {}; text: {}; binary: {})",
        file,
        list(PARSED_EXTS),
        list(TEXT_EXTS),
        list(BINARY_EXTS)
    ))
}

pub struct ModuleCache {
    modules: RefCell<HashMap<PathBuf, Rc<ModuleValue>>>,
    data: RefCell<HashMap<PathBuf, Value>>,
    in_progress: RefCell<Vec<PathBuf>>,
    natives: NativeContext,
}

impl ModuleCache {
    pub fn new() -> Self {
        Self {
            modules: RefCell::new(HashMap::new()),
            data: RefCell::new(HashMap::new()),
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
    ) -> Result<Imported, String> {
        if let Some(name) = spec.strip_prefix("fogu:") {
            return self
                .natives
                .get_module(name)
                .map(Imported::Module)
                .ok_or_else(|| format!("unknown built-in module: 'fogu:{}'", name));
        }

        let bare = !(spec.starts_with('.') || spec.starts_with('/') || spec.contains(":\\"));
        let pkg_path = if bare { crate::pkg::resolve_import(spec, from_dir)? } else { None };
        let path = match pkg_path {
            Some(p) => p,
            None => resolve_path(spec, from_dir)?,
        };
        if let Some(e) = unknown_type(&path) {
            return Err(e);
        }
        // sources of a WebAssembly bundle live in memory (runtime/vfs.rs)
        let canonical = if super::vfs::contains(&path) {
            super::vfs::normalize(&path)
        } else {
            path.canonicalize().map_err(|e| format!("cannot resolve '{}': {}", path.display(), e))?
        };

        if let Some(kind) = data_kind(&canonical) {
            let name = display_path(&canonical);
            if let Some(v) = self.data.borrow().get(&canonical) {
                return Ok(Imported::Data { name, value: v.clone() });
            }
            let value = load_data(&canonical, kind)?;
            self.data.borrow_mut().insert(canonical, value.clone());
            return Ok(Imported::Data { name, value });
        }

        if let Some(m) = self.modules.borrow().get(&canonical) {
            return Ok(Imported::Module(Rc::clone(m)));
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

        Ok(Imported::Module(exports))
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
        super::fnsrc::register(canonical, parser.take_fn_spans());
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
        imported: &Imported,
        env: &Rc<RefCell<Environment>>,
    ) -> Result<(), String> {
        match kind {
            ImportKind::Named { alias } => {
                env.borrow_mut().define(alias.clone(), imported.default_value(), false);
                Ok(())
            }
            ImportKind::Namespace { alias } => {
                env.borrow_mut().define(alias.clone(), imported.namespace(), false);
                Ok(())
            }
            ImportKind::Members { members } => {
                for (orig, alias) in members {
                    let v = imported.member(orig)?;
                    env.borrow_mut().define(alias.clone(), v, false);
                }
                Ok(())
            }
            ImportKind::Combined { alias, rest } => {
                env.borrow_mut().define(alias.clone(), imported.default_value(), false);
                self.bind_import(rest, imported, env)
            }
        }
    }
}

/// Reads and parses a data file. Errors name the file (and the line, when
/// the parser gives one).
fn load_data(path: &Path, kind: DataKind) -> Result<Value, String> {
    let name = display_path(path);
    let bytes = match super::vfs::read(path) {
        Some(s) => s.into_bytes(),
        None => std::fs::read(path).map_err(|e| format!("error reading '{}': {}", name, e))?,
    };
    let text = |bytes: Vec<u8>| {
        let s = String::from_utf8(bytes).map_err(|_| format!("cannot import '{}': it is not valid UTF-8 text", name))?;
        // a byte order mark is not part of the text
        Ok::<String, String>(match s.strip_prefix('\u{feff}') {
            Some(rest) => rest.to_string(),
            None => s,
        })
    };
    match kind {
        DataKind::Binary => Ok(Value::buffer(bytes)),
        DataKind::Text => Ok(Value::str(text(bytes)?)),
        DataKind::Json => {
            let s = text(bytes)?;
            crate::native::json::parse_value(&s).map_err(|e| {
                let e = e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
                format!("{}: {}", name, with_line(&s, &e))
            })
        }
        DataKind::Parsed => {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
            let result = match crate::host::get() {
                Some(h) => h.load_data(path, &ext, &bytes),
                None => None,
            };
            match result {
                Some(r) => r.map_err(|e| {
                    let e = e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
                    format!("{}: {}", name, e)
                }),
                None => Err(format!("cannot import '{}': .{} files need the standard library", name, ext)),
            }
        }
    }
}

/// `… at position N` (a character offset, from the JSON parser) → `line L:C: …`.
fn with_line(text: &str, msg: &str) -> String {
    let pos = msg
        .rfind("position ")
        .and_then(|i| msg[i + 9..].split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse::<usize>().ok());
    match pos {
        Some(pos) => {
            let (mut line, mut col) = (1, 1);
            for c in text.chars().take(pos) {
                if c == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
            }
            format!("line {}:{}: {}", line, col, msg)
        }
        None => msg.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_kinds_by_extension() {
        assert_eq!(data_kind(Path::new("a/config.json")), Some(DataKind::Json));
        assert_eq!(data_kind(Path::new("x.YAML")), Some(DataKind::Parsed));
        assert_eq!(data_kind(Path::new("notes.md")), Some(DataKind::Text));
        assert_eq!(data_kind(Path::new("logo.png")), Some(DataKind::Binary));
        assert_eq!(data_kind(Path::new("mod.fg")), None);
        assert_eq!(data_kind(Path::new("mod")), None);
        assert!(unknown_type(Path::new("mod.fg")).is_none());
        assert!(unknown_type(Path::new("x.png")).is_none());
        assert!(unknown_type(Path::new("x.docx")).unwrap().starts_with("cannot import 'x.docx': unknown file type"));
    }

    #[test]
    fn json_positions_become_lines() {
        assert_eq!(with_line("{\n  \"a\": ?\n}", "unexpected '?' at position 9"), "line 2:8: unexpected '?' at position 9");
        assert_eq!(with_line("x", "no position"), "no position");
    }
}
