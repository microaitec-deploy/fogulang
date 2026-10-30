//! A read-only in-memory file system for the sources of a program embedded
//! in a WebAssembly build (`fogu build --target wasm`): the program runs
//! without access to the host's disk. Paths live under [`ROOT`].

use ahash::AHashMap as HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

pub const ROOT: &str = "/fogu-bundle";

static FILES: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// Installs the files (relative path → content) under [`ROOT`].
pub fn install(files: impl IntoIterator<Item = (String, String)>) {
    let map = files.into_iter().map(|(rel, text)| (normalize(&Path::new(ROOT).join(rel)), text)).collect();
    *FILES.lock().unwrap_or_else(|e| e.into_inner()) = Some(map);
}

fn active() -> bool {
    FILES.lock().map(|f| f.is_some()).unwrap_or(false)
}

/// `a/./b/../c` → `a/c` (lexically, without touching the disk).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The content of a virtual file.
pub fn read(p: &Path) -> Option<String> {
    if !active() {
        return None;
    }
    let f = FILES.lock().ok()?;
    f.as_ref()?.get(&normalize(p)).cloned()
}

pub fn exists(p: &Path) -> bool {
    active() && read(p).is_some()
}

/// Is this a path inside the virtual tree?
pub fn contains(p: &Path) -> bool {
    active() && normalize(p).starts_with(ROOT)
}

/// A path for messages: virtual paths without the root (`main.fg`).
pub fn display(p: &Path) -> String {
    let s = p.display().to_string();
    match s.strip_prefix(ROOT) {
        Some(rest) if active() => rest.trim_start_matches('/').to_string(),
        _ => s,
    }
}
