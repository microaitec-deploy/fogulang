//! Fogu package management.
//!
//! * `fogu.json` — project manifest (dependencies, scripts, entry point).
//! * `fogu.lock` — pins the exact commit + checksum (sha256) of each package.
//! * Packages come from git repositories (GitHub, GitLab, any URL),
//!   tarballs, local folders or the Fogu server (`fogu add <name>`, see
//!   `registry`) and live in a **global store**:
//!   `$FOGU_HOME/fogu_modules/<name>@<commit>/` (default `~/.fogu`).
//!   Several versions coexist; each project picks its own via `fogu.lock`.
//! * Install scripts are never run (spec §41).

pub mod install;
pub mod net;
pub mod registry;
pub mod semver;
pub mod source;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value as Json};

pub const MANIFEST: &str = "fogu.json";
pub const LOCKFILE: &str = "fogu.lock";

/// Fogu base directory (`FOGU_HOME` or `~/.fogu`).
pub fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("FOGU_HOME") {
        return PathBuf::from(h);
    }
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".fogu")
}

/// Global package store (`fogu_modules`).
pub fn store() -> PathBuf {
    home().join("fogu_modules")
}

pub fn safe_name(name: &str) -> String {
    name.replace('/', "__")
}

pub fn store_dir(name: &str, commit: &str) -> PathBuf {
    let short: String = commit.chars().take(12).collect();
    store().join(format!("{}@{}", safe_name(name), short))
}

/// Safe package name (prevents escaping the store).
pub fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 100 || name.contains("..") || name.starts_with('.') {
        return false;
    }
    let (scope, rest) = match name.strip_prefix('@') {
        Some(r) => match r.split_once('/') {
            Some((s, n)) => (Some(s), n),
            None => return false,
        },
        None => (None, name),
    };
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    ok(rest) && scope.map_or(true, ok)
}

/// Nearest project directory (with `fogu.json`) starting at `from`.
pub fn find_root(from: &Path) -> Option<PathBuf> {
    let from = if from.as_os_str().is_empty() { Path::new(".") } else { from };
    let start = from.canonicalize().unwrap_or_else(|_| from.to_path_buf());
    let mut cur = Some(start.as_path());
    while let Some(d) = cur {
        if d.join(MANIFEST).is_file() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

// ---------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------

pub struct Manifest {
    pub path: PathBuf,
    pub json: Json,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read '{}': {}", path.display(), e))?;
        let json: Json = serde_json::from_str(&text)
            .map_err(|e| format!("invalid '{}': {}", path.display(), e))?;
        if !json.is_object() {
            return Err(format!("'{}' must contain a JSON object", path.display()));
        }
        Ok(Self { path: path.to_path_buf(), json })
    }

    pub fn new(path: &Path, name: &str) -> Self {
        let mut m = Map::new();
        m.insert("name".into(), Json::String(name.into()));
        m.insert("version".into(), Json::String("0.1.0".into()));
        m.insert("entry".into(), Json::String("main.fg".into()));
        m.insert("scripts".into(), Json::Object(Map::new()));
        m.insert("dependencies".into(), Json::Object(Map::new()));
        Self { path: path.to_path_buf(), json: Json::Object(m) }
    }

    pub fn save(&self) -> Result<(), String> {
        let mut text = serde_json::to_string_pretty(&self.json).map_err(|e| e.to_string())?;
        text.push('\n');
        std::fs::write(&self.path, text)
            .map_err(|e| format!("could not write '{}': {}", self.path.display(), e))
    }

    pub fn str_field(&self, k: &str) -> Option<String> {
        self.json.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
    }

    pub fn entry(&self) -> Option<String> {
        self.str_field("entry").or_else(|| self.str_field("main"))
    }

    pub fn deps(&self) -> Vec<(String, String)> {
        self.json
            .get("dependencies")
            .and_then(|d| d.as_object())
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn scripts(&self) -> Vec<(String, String)> {
        self.json
            .get("scripts")
            .and_then(|d| d.as_object())
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn deps_mut(&mut self) -> &mut Map<String, Json> {
        let obj = self.json.as_object_mut().expect("manifest is an object");
        let e = obj.entry("dependencies").or_insert_with(|| Json::Object(Map::new()));
        if !e.is_object() {
            *e = Json::Object(Map::new());
        }
        e.as_object_mut().unwrap()
    }

    pub fn set_dep(&mut self, name: &str, spec: &str) {
        let d = self.deps_mut();
        d.insert(name.to_string(), Json::String(spec.to_string()));
        // stable alphabetical order
        let mut items: Vec<(String, Json)> = std::mem::take(d).into_iter().collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        d.extend(items);
    }

    pub fn remove_dep(&mut self, name: &str) -> bool {
        self.deps_mut().shift_remove(name).is_some()
    }
}

// ---------------------------------------------------------------------
// Lockfile
// ---------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LockEntry {
    pub spec: String,
    pub resolved: String,
    pub commit: String,
    pub version: String,
    pub integrity: String,
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct Lock {
    pub packages: BTreeMap<String, LockEntry>,
}

impl Lock {
    pub fn load(path: &Path) -> Lock {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Lock::default();
        };
        let Ok(j) = serde_json::from_str::<Json>(&text) else {
            return Lock::default();
        };
        let mut lock = Lock::default();
        if let Some(pk) = j.get("packages").and_then(|p| p.as_object()) {
            for (name, e) in pk {
                let s = |k: &str| e.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let deps = e
                    .get("dependencies")
                    .and_then(|d| d.as_object())
                    .map(|o| {
                        o.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                lock.packages.insert(
                    name.clone(),
                    LockEntry {
                        spec: s("spec"),
                        resolved: s("resolved"),
                        commit: s("commit"),
                        version: s("version"),
                        integrity: s("integrity"),
                        dependencies: deps,
                    },
                );
            }
        }
        lock
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut pk = Map::new();
        for (name, e) in &self.packages {
            let mut o = Map::new();
            o.insert("spec".into(), e.spec.clone().into());
            o.insert("resolved".into(), e.resolved.clone().into());
            o.insert("commit".into(), e.commit.clone().into());
            if !e.version.is_empty() {
                o.insert("version".into(), e.version.clone().into());
            }
            o.insert("integrity".into(), e.integrity.clone().into());
            if !e.dependencies.is_empty() {
                let d: Map<String, Json> =
                    e.dependencies.iter().map(|(k, v)| (k.clone(), v.clone().into())).collect();
                o.insert("dependencies".into(), Json::Object(d));
            }
            pk.insert(name.clone(), Json::Object(o));
        }
        let mut top = Map::new();
        top.insert("lockfileVersion".into(), 1.into());
        top.insert("packages".into(), Json::Object(pk));
        let mut text = serde_json::to_string_pretty(&Json::Object(top)).map_err(|e| e.to_string())?;
        text.push('\n');
        std::fs::write(path, text).map_err(|e| format!("could not write '{}': {}", path.display(), e))
    }
}

// ---------------------------------------------------------------------
// Resolving `import x from "package"`
// ---------------------------------------------------------------------

static MAIN_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
static MAIN_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Project of the main file; its packages also apply to files that live
/// inside other packages (flat dependencies).
pub fn set_main_dir(dir: &Path) {
    *MAIN_ROOT.lock().unwrap() = find_root(dir);
    *MAIN_DIR.lock().unwrap() = Some(dir.to_path_buf());
}

/// `Fogu.asset(path)`: a file of the program, relative to the project root
/// (the folder with fogu.json) or to the main file's folder. In a compiled
/// executable that is the folder where the bundle (and its embedded files)
/// was extracted.
pub fn asset_path(rel: &str) -> PathBuf {
    let base = MAIN_ROOT
        .lock()
        .unwrap()
        .clone()
        .or_else(|| MAIN_DIR.lock().unwrap().clone())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(rel)
}

fn split_spec(spec: &str) -> (String, String) {
    let mut it = spec.splitn(if spec.starts_with('@') { 3 } else { 2 }, '/');
    if spec.starts_with('@') {
        let a = it.next().unwrap_or("");
        let b = it.next().unwrap_or("");
        (format!("{}/{}", a, b), it.next().unwrap_or("").to_string())
    } else {
        (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
    }
}

fn locate_dir(name: &str, from_dir: &Path) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(r) = find_root(from_dir) {
        roots.push(r);
    }
    if let Some(r) = MAIN_ROOT.lock().unwrap().clone() {
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    for r in &roots {
        // optional vendoring: <project>/fogu_modules/<name>
        let v = r.join("fogu_modules").join(name);
        if v.is_dir() {
            return Some(v);
        }
        let lock = Lock::load(&r.join(LOCKFILE));
        if let Some(e) = lock.packages.get(name) {
            let d = store_dir(name, &e.commit);
            if d.is_dir() {
                return Some(d);
            }
        }
    }
    // external libraries written in Fogu (the libs/ folder of the Fogu project)
    let mut starts = roots.clone();
    starts.push(from_dir.to_path_buf());
    if let Some(d) = lib_dir(name, &starts, std::env::var_os("FOGU_LIBS"), &home()) {
        return Some(d);
    }
    let g = Lock::load(&home().join(LOCKFILE));
    if let Some(e) = g.packages.get(name) {
        let d = store_dir(name, &e.commit);
        if d.is_dir() {
            return Some(d);
        }
    }
    // last resort: the most recent one in the store
    let prefix = format!("{}@", safe_name(name));
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for ent in std::fs::read_dir(store()).ok()?.flatten() {
        if ent.file_name().to_string_lossy().starts_with(&prefix) {
            let t = ent.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map_or(true, |(bt, _)| t > *bt) {
                best = Some((t, ent.path()));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// A library of the `libs/` folder (`import git from "git"`, `import serial from
/// "hardware/serial"`): in the folders of `$FOGU_LIBS` (a path list), then in the
/// nearest `libs/<name>` going up from the project (the Fogu repository keeps its
/// libraries next to the language), then in `$FOGU_HOME/libs/<name>` (installed
/// with Fogu). A library is a folder with a `fogu.json`.
fn lib_dir(name: &str, starts: &[PathBuf], fogu_libs: Option<std::ffi::OsString>, home: &Path) -> Option<PathBuf> {
    let is_lib = |d: &Path| d.join(MANIFEST).is_file();
    if let Some(paths) = fogu_libs {
        for base in std::env::split_paths(&paths) {
            let d = base.join(name);
            if is_lib(&d) {
                return Some(d);
            }
        }
    }
    for start in starts {
        let mut dir = Some(start.as_path());
        while let Some(d) = dir {
            let cand = d.join("libs").join(name);
            if is_lib(&cand) {
                return Some(cand);
            }
            dir = d.parent();
        }
    }
    let d = home.join("libs").join(name);
    is_lib(&d).then_some(d)
}

fn with_ext(p: &Path) -> Option<PathBuf> {
    if p.is_file() {
        return Some(p.to_path_buf());
    }
    let f = p.with_extension("fg");
    if p.extension().is_none() && f.is_file() {
        return Some(f);
    }
    let idx = p.join("index.fg");
    if idx.is_file() {
        return Some(idx);
    }
    None
}

/// Name and folder of the package a bare specifier refers to (used by
/// `fogu build` to embed the packages a program imports).
pub fn package_dir(spec: &str, from_dir: &Path) -> Option<(String, PathBuf)> {
    let (name, _) = split_spec(spec);
    if !valid_name(&name) {
        return None;
    }
    locate_dir(&name, from_dir).map(|d| (name, d))
}

/// Resolves a "bare" specifier (`http-x`, `user-repo/sub/file`).
/// `Ok(None)` = no package with that name (the caller tries a path).
pub fn resolve_import(spec: &str, from_dir: &Path) -> Result<Option<PathBuf>, String> {
    let (name, sub) = split_spec(spec);
    if !valid_name(&name) {
        return Ok(None);
    }
    let Some(dir) = locate_dir(&name, from_dir) else {
        return Ok(None);
    };
    if !sub.is_empty() {
        if sub.split('/').any(|c| c == "..") {
            return Err(format!("invalid path in '{}'", spec));
        }
        return with_ext(&dir.join(&sub))
            .map(Some)
            .ok_or_else(|| format!("'{}' does not exist in package '{}'", sub, name));
    }
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(m) = Manifest::load(&dir.join(MANIFEST)) {
        if let Some(e) = m.entry() {
            candidates.push(e);
        }
    }
    candidates.extend(["main.fg".into(), "index.fg".into(), format!("{}.fg", name.rsplit('/').next().unwrap_or(&name))]);
    for c in candidates {
        if c.contains("..") || Path::new(&c).is_absolute() {
            continue;
        }
        let p = dir.join(&c);
        if p.is_file() {
            return Ok(Some(p));
        }
    }
    Err(format!("package '{}' has no entry file (set \"entry\" in fogu.json)", name))
}

#[cfg(test)]
mod lib_dir_tests {
    use super::lib_dir;
    use std::fs;
    use std::path::PathBuf;

    fn lib(at: &PathBuf) {
        fs::create_dir_all(at).unwrap();
        fs::write(at.join("fogu.json"), "{\"name\": \"x\"}").unwrap();
    }

    #[test]
    fn finds_libs_in_order() {
        let tmp = std::env::temp_dir().join(format!("fogu-libdir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let project = tmp.join("repo/apps/app");
        fs::create_dir_all(&project).unwrap();
        let home = tmp.join("home");
        let env_libs = tmp.join("env-libs");

        // nothing yet
        assert_eq!(lib_dir("git", &[project.clone()], None, &home), None);
        // installed with Fogu
        lib(&home.join("libs/git"));
        assert_eq!(lib_dir("git", &[project.clone()], None, &home), Some(home.join("libs/git")));
        // the nearest libs/ going up from the project wins over the installed one
        lib(&tmp.join("repo/libs/git"));
        assert_eq!(lib_dir("git", &[project.clone()], None, &home), Some(tmp.join("repo/libs/git")));
        // $FOGU_LIBS wins over both
        lib(&env_libs.join("git"));
        let paths = std::env::join_paths([tmp.join("missing"), env_libs.clone()]).unwrap();
        assert_eq!(lib_dir("git", &[project.clone()], Some(paths), &home), Some(env_libs.join("git")));
        // a folder without fogu.json is not a library
        fs::create_dir_all(tmp.join("repo/libs/empty")).unwrap();
        assert_eq!(lib_dir("empty", &[project], None, &home), None);
        let _ = fs::remove_dir_all(&tmp);
    }
}
