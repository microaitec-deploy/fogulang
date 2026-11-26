//! `fogu:path` module — path manipulation (purely lexical, except `absolute`, `real`,
//! `isInside` and `safeJoin`, which look at the disk).
//!
//! ```fogu
//! import path from "fogu:path"
//! print(path.join("src", "lib", "a.fg"))    // src/lib/a.fg
//! print(path.extname("a/b.test.fg"))        // .fg
//! ```

use ahash::AHashMap as HashMap;
use std::path::{Component, Path, PathBuf};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::util::{nfn_var, path_str, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();

    e.insert("sep".into(), Value::Str("/".into()));
    e.insert("join".into(), nfn_var("join", 1, |a| {
        let mut p = PathBuf::new();
        for i in 0..a.len() {
            p.push(str_arg(a, i, "path.join")?);
        }
        Ok(Value::Str(path_str(&normalize(&p)).into()))
    }));
    e.insert("dirname".into(), nfn("dirname", 1, |a| {
        let s = str_arg(a, 0, "path.dirname")?;
        let p = Path::new(&s);
        let d = match p.parent() {
            Some(d) => path_str(d),
            // the folder of a root is the root itself (`/`, `C:/`), not `.`
            None if p.has_root() => path_str(p),
            None => String::new(),
        };
        Ok(Value::Str(if d.is_empty() { ".".into() } else { d.into() }))
    }));
    e.insert("basename".into(), nfn("basename", 1, |a| {
        let s = str_arg(a, 0, "path.basename")?;
        Ok(Value::Str(
            Path::new(&s).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into(),
        ))
    }));
    e.insert("stem".into(), nfn("stem", 1, |a| {
        let s = str_arg(a, 0, "path.stem")?;
        Ok(Value::Str(
            Path::new(&s).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into(),
        ))
    }));
    e.insert("extname".into(), nfn("extname", 1, |a| {
        let s = str_arg(a, 0, "path.extname")?;
        Ok(Value::Str(
            (Path::new(&s)
                .extension()
                .map(|x| format!(".{}", x.to_string_lossy()))
                .unwrap_or_default()).into(),
        ))
    }));
    e.insert("normalize".into(), nfn("normalize", 1, |a| {
        let s = str_arg(a, 0, "path.normalize")?;
        Ok(Value::Str(path_str(&normalize(Path::new(&s))).into()))
    }));
    e.insert("isAbsolute".into(), nfn("isAbsolute", 1, |a| {
        // (`/etc` is absolute on Windows too: it names one place on the current drive)
        let s = str_arg(a, 0, "path.isAbsolute")?;
        Ok(Value::Bool(Path::new(&s).is_absolute() || s.starts_with('/') || (cfg!(windows) && s.starts_with('\\'))))
    }));
    e.insert("absolute".into(), nfn("absolute", 1, |a| {
        let s = str_arg(a, 0, "path.absolute")?;
        std::path::absolute(&s)
            .map(|p| Value::Str(path_str(&normalize(&p)).into()))
            .map_err(|err| io_err!("path.absolute: {}: {}", s, err))
    }));

    // isInside(base, p): is `p` the folder `base` or under it, once `..` and links are resolved
    e.insert("isInside".into(), nfn("isInside", 2, |a| {
        let base = str_arg(a, 0, "path.isInside")?;
        let p = str_arg(a, 1, "path.isInside")?;
        Ok(Value::Bool(crate::native::util::is_inside(&base, &p)))
    }));
    // safeJoin(base, name): the path of an untrusted `name` under `base`, or an error
    e.insert("safeJoin".into(), nfn("safeJoin", 2, |a| {
        let base = str_arg(a, 0, "path.safeJoin")?;
        let name = str_arg(a, 1, "path.safeJoin")?;
        crate::native::util::contained(&base, &name)
            .map(|p| Value::Str(path_str(&p).into()))
            .map_err(|err| io_err!("path.safeJoin: {}", err))
    }));
    // real(p): the absolute path with every symbolic link resolved (the file must exist)
    e.insert("real".into(), nfn("real", 1, real));

    ModuleValue {
        name: "fogu:path".into(),
        exports: std::cell::RefCell::new(e),
    }
}

/// `path.real` / `fs.realpath`: the canonical path of something that exists.
pub fn real(a: &[Value]) -> Result<Value, String> {
    let s = str_arg(a, 0, "path.real")?;
    match Path::new(&s).canonicalize() {
        // (Windows answers `\\?\C:\…`: the plain form is what programs expect)
        Ok(p) => {
            let shown = path_str(&p);
            Ok(Value::Str(shown.strip_prefix("//?/").unwrap_or(&shown).to_string().into()))
        }
        Err(err) => Err(io_err!("path.real: {}: {}", s, err)),
    }
}

/// Removes `.` and resolves `..` lexically (`a/./b/../c` → `a/c`).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            // `..` removes a name, stays at the root (`/..` is `/`) and piles up on a leading
            // `..` (`../../a` keeps both: popping one `..` with the next was wrong)
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}
