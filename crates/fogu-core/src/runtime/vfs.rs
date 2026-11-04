//! A read-only in-memory file system for the sources of a program: the
//! sources of a WebAssembly build (`fogu build --target wasm`, under
//! [`ROOT`]: the program runs without access to the host's disk), or the
//! sources of an embedded program laid over a real folder ([`install_at`]:
//! the desktop host keeps an app's `.fg` files in memory while its assets
//! are on disk). A virtual file wins over a file on disk with the same path.

use ahash::AHashMap as HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

pub const ROOT: &str = "/fogu-bundle";

/// The root of the virtual tree and its files (absolute, normalized paths).
static FILES: Mutex<Option<(PathBuf, HashMap<PathBuf, String>)>> = Mutex::new(None);

/// Installs the files (relative path → content) under [`ROOT`].
pub fn install(files: impl IntoIterator<Item = (String, String)>) {
    install_at(Path::new(ROOT), files);
}

/// Installs the files (relative path → content) under `root`, replacing any
/// earlier tree. `root` may be a real folder: its other files stay on disk.
pub fn install_at(root: &Path, files: impl IntoIterator<Item = (String, String)>) {
    let root = normalize(root);
    let map = files.into_iter().map(|(rel, text)| (normalize(&root.join(rel)), text)).collect();
    *FILES.lock().unwrap_or_else(|e| e.into_inner()) = Some((root, map));
}

/// Removes the virtual tree.
pub fn clear() {
    *FILES.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn active() -> bool {
    FILES.lock().map(|f| f.is_some()).unwrap_or(false)
}

fn root() -> Option<PathBuf> {
    FILES.lock().ok()?.as_ref().map(|(r, _)| r.clone())
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
    f.as_ref()?.1.get(&normalize(p)).cloned()
}

pub fn exists(p: &Path) -> bool {
    active() && read(p).is_some()
}

/// Is this a path inside the virtual tree? Under [`ROOT`] every path is
/// virtual; under a real folder only the installed files are.
pub fn contains(p: &Path) -> bool {
    match root() {
        Some(r) if r == Path::new(ROOT) => normalize(p).starts_with(ROOT),
        Some(_) => exists(p),
        None => false,
    }
}

/// A path for messages: virtual paths without the root (`main.fg`).
pub fn display(p: &Path) -> String {
    let s = p.display().to_string();
    let Some(r) = root() else { return s };
    match normalize(p).strip_prefix(&r) {
        Ok(rest) if exists(p) || r == Path::new(ROOT) => rest.display().to_string().replace('\\', "/"),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // one test: the tree is process-wide
    #[test]
    fn trees_under_root_and_over_a_real_folder() {
        install([("main.fg".to_string(), "print(1)".to_string())]);
        assert_eq!(read(Path::new("/fogu-bundle/./main.fg")).as_deref(), Some("print(1)"));
        assert!(contains(Path::new("/fogu-bundle/missing.fg")));
        assert_eq!(display(Path::new("/fogu-bundle/main.fg")), "main.fg");

        let dir = std::env::temp_dir().join("fogu-vfs-test");
        install_at(&dir, [("main.fg".to_string(), "a".to_string()), ("lib/x.fg".to_string(), "b".to_string())]);
        assert_eq!(read(&dir.join("lib").join("..").join("main.fg")).as_deref(), Some("a"));
        assert!(contains(&dir.join("lib/x.fg")));
        // the folder's other files are on disk, not virtual
        assert!(!contains(&dir.join("icon.png")));
        assert!(!contains(Path::new("/fogu-bundle/main.fg")));
        assert_eq!(display(&dir.join("lib").join("x.fg")), "lib/x.fg");
        clear();
        assert!(read(&dir.join("main.fg")).is_none());
        assert!(!contains(&dir.join("main.fg")));
    }
}
