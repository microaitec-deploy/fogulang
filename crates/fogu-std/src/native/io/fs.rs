//! `fogu:fs` module — files and directories (spec §46).
//!
//! ```fogu
//! import fs from "fogu:fs"
//!
//! fs.writeText("notes.txt", "hello")?
//! print(fs.readText("notes.txt")?)
//! for f in fs.glob("src/**/*.fg") { print(f) }
//! ```
//!
//! Failures (missing file, permission denied…) are operational errors, so they
//! reach the program as `error` values.

use std::collections::BTreeSet;
use ahash::AHashMap as HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::util::{map, path_str, str_arg, str_list};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();

    e.insert("readText".into(), nfn("readText", 1, |a| {
        let p = str_arg(a, 0, "fs.readText")?;
        std::fs::read_to_string(&p)
            .map(Value::str)
            .map_err(|err| io_err!("fs.readText: {}: {}", p, err))
    }));
    e.insert("readLines".into(), nfn("readLines", 1, |a| {
        let p = str_arg(a, 0, "fs.readLines")?;
        let text = std::fs::read_to_string(&p).map_err(|err| io_err!("fs.readLines: {}: {}", p, err))?;
        Ok(str_list(text.lines().map(str::to_string)))
    }));
    e.insert("writeText".into(), nfn("writeText", 2, |a| {
        let p = str_arg(a, 0, "fs.writeText")?;
        let text = str_arg(a, 1, "fs.writeText")?;
        std::fs::write(&p, text)
            .map(|_| Value::Nil)
            .map_err(|err| io_err!("fs.writeText: {}: {}", p, err))
    }));
    e.insert("append".into(), nfn("append", 2, |a| {
        use std::io::Write;
        let p = str_arg(a, 0, "fs.append")?;
        let text = str_arg(a, 1, "fs.append")?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .map(|_| Value::Nil)
            .map_err(|err| io_err!("fs.append: {}: {}", p, err))
    }));
    e.insert("exists".into(), nfn("exists", 1, |a| {
        Ok(Value::Bool(Path::new(&str_arg(a, 0, "fs.exists")?).exists()))
    }));
    e.insert("isFile".into(), nfn("isFile", 1, |a| {
        Ok(Value::Bool(Path::new(&str_arg(a, 0, "fs.isFile")?).is_file()))
    }));
    e.insert("isDir".into(), nfn("isDir", 1, |a| {
        Ok(Value::Bool(Path::new(&str_arg(a, 0, "fs.isDir")?).is_dir()))
    }));
    e.insert("mkdir".into(), nfn("mkdir", 1, |a| {
        let p = str_arg(a, 0, "fs.mkdir")?;
        std::fs::create_dir_all(&p)
            .map(|_| Value::Nil)
            .map_err(|err| io_err!("fs.mkdir: {}: {}", p, err))
    }));
    // remove: a file or an *empty* directory; removeAll: recursive.
    e.insert("remove".into(), nfn("remove", 1, |a| {
        let p = str_arg(a, 0, "fs.remove")?;
        let r = if Path::new(&p).is_dir() {
            std::fs::remove_dir(&p)
        } else {
            std::fs::remove_file(&p)
        };
        r.map(|_| Value::Nil).map_err(|err| io_err!("fs.remove: {}: {}", p, err))
    }));
    e.insert("removeAll".into(), nfn("removeAll", 1, |a| {
        let p = str_arg(a, 0, "fs.removeAll")?;
        let path = Path::new(&p);
        if !path.exists() {
            return Ok(Value::Nil);
        }
        let r = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        r.map(|_| Value::Nil).map_err(|err| io_err!("fs.removeAll: {}: {}", p, err))
    }));
    e.insert("copy".into(), nfn("copy", 2, |a| {
        let from = str_arg(a, 0, "fs.copy")?;
        let to = str_arg(a, 1, "fs.copy")?;
        copy_any(Path::new(&from), Path::new(&to))
            .map(|_| Value::Nil)
            .map_err(|err| io_err!("fs.copy: {} → {}: {}", from, to, err))
    }));
    e.insert("move".into(), nfn("move", 2, |a| {
        let from = str_arg(a, 0, "fs.move")?;
        let to = str_arg(a, 1, "fs.move")?;
        std::fs::rename(&from, &to)
            .map(|_| Value::Nil)
            .map_err(|err| io_err!("fs.move: {} → {}: {}", from, to, err))
    }));
    e.insert("list".into(), nfn("list", 1, |a| {
        let p = str_arg(a, 0, "fs.list")?;
        let rd = std::fs::read_dir(&p).map_err(|err| io_err!("fs.list: {}: {}", p, err))?;
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        Ok(str_list(names))
    }));
    e.insert("stat".into(), nfn("stat", 1, |a| {
        let p = str_arg(a, 0, "fs.stat")?;
        let m = std::fs::metadata(&p).map_err(|err| io_err!("fs.stat: {}: {}", p, err))?;
        let modified = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(map(vec![
            ("size", Value::Int(m.len() as i64)),
            ("isFile", Value::Bool(m.is_file())),
            ("isDir", Value::Bool(m.is_dir())),
            ("readonly", Value::Bool(m.permissions().readonly())),
            ("modified", Value::Int(modified)),
        ]))
    }));
    e.insert("glob".into(), nfn("glob", 1, |a| {
        let pattern = str_arg(a, 0, "fs.glob")?;
        Ok(str_list(glob(&pattern)))
    }));
    e.insert("cwd".into(), nfn("cwd", 0, |_| {
        std::env::current_dir()
            .map(|p| Value::Str(path_str(&p).into()))
            .map_err(|err| io_err!("fs.cwd: {}", err))
    }));
    e.insert("tempDir".into(), nfn("tempDir", 0, |_| Ok(Value::Str(path_str(&std::env::temp_dir()).into()))));
    // fs.remote(ssh, { sudo }) → the same operations on a server (see fs_remote.rs)
    #[cfg(feature = "ops")]
    e.insert("remote".into(), crate::native::util::nfn_var("remote", 1, crate::native::fs_remote::remote));

    ModuleValue {
        name: "fogu:fs".into(),
        exports: std::cell::RefCell::new(e),
    }
}

/// Copies a file, or a directory recursively.
fn copy_any(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_any(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

// ---------- glob ----------

/// Files matching a pattern with `*`, `?` and `**` (any number of folders),
/// sorted. Names starting with `.` only match a pattern that starts with `.`.
pub fn glob(pattern: &str) -> Vec<String> {
    let pattern = pattern.replace('\\', "/");
    let parts: Vec<&str> = pattern.split('/').collect();
    let first_wild = parts.iter().position(|p| p.contains(['*', '?'])).unwrap_or(parts.len());
    let base = parts[..first_wild].join("/");
    let rest = &parts[first_wild..];
    let mut out = BTreeSet::new();
    if rest.is_empty() {
        if Path::new(&base).exists() {
            out.insert(base);
        }
    } else {
        // `/*` has an empty base but starts at the root
        let (dir, prefix) = match (base.is_empty(), pattern.starts_with('/')) {
            (true, true) => (PathBuf::from("/"), "/".to_string()),
            (true, false) => (PathBuf::from("."), String::new()),
            _ => (PathBuf::from(&base), base.clone()),
        };
        walk(&dir, &prefix, rest, &mut out);
    }
    out.into_iter().collect()
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else if prefix.ends_with('/') {
        format!("{}{}", prefix, name)
    } else {
        format!("{}/{}", prefix, name)
    }
}

fn walk(dir: &Path, prefix: &str, rest: &[&str], out: &mut BTreeSet<String>) {
    let Some((comp, tail)) = rest.split_first() else { return };
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<(String, PathBuf, bool)> = rd
        .flatten()
        .map(|e| {
            let p = e.path();
            (e.file_name().to_string_lossy().to_string(), p.clone(), p.is_dir())
        })
        .collect();
    entries.sort();
    if *comp == "**" {
        // zero folders
        if tail.is_empty() {
            for (name, path, is_dir) in &entries {
                if name.starts_with('.') {
                    continue;
                }
                let shown = join(prefix, name);
                if *is_dir {
                    walk(path, &shown, rest, out);
                } else {
                    out.insert(shown);
                }
            }
            return;
        }
        walk(dir, prefix, tail, out);
        // one or more folders
        for (name, path, is_dir) in &entries {
            if *is_dir && !name.starts_with('.') {
                walk(path, &join(prefix, name), rest, out);
            }
        }
        return;
    }
    for (name, path, is_dir) in &entries {
        if name.starts_with('.') && !comp.starts_with('.') {
            continue;
        }
        if !wildcard(comp, name) {
            continue;
        }
        let shown = join(prefix, name);
        if tail.is_empty() {
            out.insert(shown);
        } else if *is_dir {
            walk(path, &shown, tail, out);
        }
    }
}

/// `*` (any run of characters) and `?` (one character).
fn wildcard(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = name.chars().collect();
    let (mut i, mut j) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while j < s.len() {
        if i < p.len() && (p[i] == '?' || p[i] == s[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(st) = star {
            i = st + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

#[cfg(test)]
mod tests {
    use super::wildcard;

    #[test]
    fn wildcards() {
        assert!(wildcard("*.fg", "main.fg"));
        assert!(!wildcard("*.fg", "main.rs"));
        assert!(wildcard("a?c", "abc"));
        assert!(wildcard("*", "anything"));
        assert!(wildcard("m*n*.fg", "main.fg"));
        assert!(!wildcard("m*x", "main"));
    }
}
