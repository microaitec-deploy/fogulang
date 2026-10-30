//! `fogu:path` module — path manipulation (purely lexical, except `absolute`).
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
        let d = Path::new(&s).parent().map(path_str).unwrap_or_default();
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
        Ok(Value::Bool(Path::new(&str_arg(a, 0, "path.isAbsolute")?).is_absolute()))
    }));
    e.insert("absolute".into(), nfn("absolute", 1, |a| {
        let s = str_arg(a, 0, "path.absolute")?;
        std::path::absolute(&s)
            .map(|p| Value::Str(path_str(&normalize(&p)).into()))
            .map_err(|err| io_err!("path.absolute: {}: {}", s, err))
    }));

    ModuleValue {
        name: "fogu:path".into(),
        exports: std::cell::RefCell::new(e),
    }
}

/// Removes `.` and resolves `..` lexically (`a/./b/../c` → `a/c`).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}
