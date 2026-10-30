//! `fogu:zip` module — ZIP archives (create, extract, list, read, add;
//! AES password protection), tar / tar.gz archives and gzip data.
//!
//! ```fogu
//! import zip from "fogu:zip"
//!
//! zip.zip("site.zip", ["public", "README.md"])?        // folders are recursive
//! print(zip.list("site.zip")?)                         // [{ name, size, compressedSize, isDir, modified }]
//! print(zip.read("site.zip", "README.md")?)
//! zip.unzip("site.zip", "out")?
//! zip.tar("backup.tar.gz", "data")?                    // gzip by the extension
//! zip.untar("backup.tar.gz", "restore")?
//! const packed = zip.gzip("hello hello hello")         // bytes
//! print(zip.gunzip(packed)?)
//! ```
//!
//! Extraction refuses entries that would escape the target folder. Bytes are
//! lists of ints; I/O and format failures are error values.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::util::{bytes_of, bytes_value};
use crate::native::util::{list, map, max_args, nfn_var, opt_arg, path_str, str_arg};

type F = fn(&[Value]) -> Result<Value, String>;

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    let mut f = |name: &str, min: usize, func: F| {
        e.insert(name.into(), nfn_var(name, min, func));
    };
    // zip(dest, source | [sources], { level = 6, password, store }) → number of files
    f("zip", 2, |a| {
        max_args(a, 3, "zip.zip")?;
        let dest = str_arg(a, 0, "zip.zip")?;
        let sources = paths(&a[1], "zip.zip")?;
        let o = opts(opt_arg(a, 2))?;
        let mut entries = Vec::new();
        for s in &sources {
            collect(Path::new(s), &mut entries).map_err(|e| io_err!("zip.zip: {}: {}", s, e))?;
        }
        let file = File::create(&dest).map_err(|e| io_err!("zip.zip: {}: {}", dest, e))?;
        let mut w = zip::ZipWriter::new(file);
        let pw = o.get("password").map(|v| v.to_display_string());
        let mut n = 0;
        for (path, name) in &entries {
            let fo = file_options(&o, pw.as_deref())?;
            if path.is_dir() {
                w.add_directory(format!("{}/", name), fo).map_err(|e| io_err!("zip.zip: {}", e))?;
            } else {
                w.start_file(name.clone(), fo).map_err(|e| io_err!("zip.zip: {}", e))?;
                let mut src = File::open(path).map_err(|e| io_err!("zip.zip: {}: {}", path_str(path), e))?;
                std::io::copy(&mut src, &mut w).map_err(|e| io_err!("zip.zip: {}", e))?;
                n += 1;
            }
        }
        w.finish().map_err(|e| io_err!("zip.zip: {}", e))?;
        Ok(Value::Int(n))
    });
    // write(dest, { "name": str | bytes, "dir/": nil }, opts) — an archive from memory
    f("write", 2, |a| {
        max_args(a, 3, "zip.write")?;
        let dest = str_arg(a, 0, "zip.write")?;
        let Value::Map(m) = &a[1] else { return Err("zip.write: expects { name: content }".into()) };
        let o = opts(opt_arg(a, 2))?;
        let file = File::create(&dest).map_err(|e| io_err!("zip.write: {}: {}", dest, e))?;
        let mut w = zip::ZipWriter::new(file);
        let mut items: Vec<(String, Value)> = m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        items.sort_by(|x, y| x.0.cmp(&y.0));
        for (name, content) in items {
            put(&mut w, &name, &content, &o)?;
        }
        w.finish().map_err(|e| io_err!("zip.write: {}", e))?;
        Ok(Value::Nil)
    });
    // add(zipPath, name, content, opts) — appends to an existing archive
    f("add", 3, |a| {
        max_args(a, 4, "zip.add")?;
        let path = str_arg(a, 0, "zip.add")?;
        let name = str_arg(a, 1, "zip.add")?;
        let o = opts(opt_arg(a, 3))?;
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).map_err(|e| io_err!("zip.add: {}: {}", path, e))?;
        let mut w = zip::ZipWriter::new_append(file).map_err(|e| io_err!("zip.add: {}: {}", path, e))?;
        put(&mut w, &name, &a[2], &o)?;
        w.finish().map_err(|e| io_err!("zip.add: {}", e))?;
        Ok(Value::Nil)
    });
    // unzip(src, destDir = ".", { password, only: [names] }) → extracted paths
    f("unzip", 1, |a| {
        max_args(a, 3, "zip.unzip")?;
        let src = str_arg(a, 0, "zip.unzip")?;
        let dest = match opt_arg(a, 1) { Some(Value::Str(d)) => d.clone(), _ => ".".into() };
        let o = opts(a.iter().skip(1).find(|v| matches!(v, Value::Map(_))))?;
        let only: Option<Vec<String>> = o.get("only").map(|v| paths(v, "zip.unzip")).transpose()?;
        let mut ar = open_zip(&src, "zip.unzip")?;
        std::fs::create_dir_all(&dest).map_err(|e| io_err!("zip.unzip: {}: {}", dest, e))?;
        let mut out = Vec::new();
        for i in 0..ar.len() {
            let mut entry = by_index(&mut ar, i, &o, "zip.unzip")?;
            let name = entry_name(&entry);
            if only.as_ref().is_some_and(|l| !l.iter().any(|x| *x == name || name.starts_with(&format!("{}/", x.trim_end_matches('/'))))) {
                continue;
            }
            // absolute names (`/etc/x`, `C:\x`) are refused too, not made relative
            let absolute = name.starts_with(['/', '\\']) || name.as_bytes().get(1) == Some(&b':');
            let Some(rel) = entry.enclosed_name().filter(|_| !absolute) else {
                return Err(io_err!("zip.unzip: unsafe entry name '{}' (outside the target folder)", name));
            };
            let target = Path::new(&dest).join(&rel);
            // a link must stay inside too (checked before anything is created for it)
            let link = if entry.is_symlink() {
                let mut link = String::new();
                entry.read_to_string(&mut link).map_err(|e| io_err!("zip.unzip: {}: {}", name, e))?;
                if !safe_link(Path::new(&dest), &rel, Path::new(&link)) {
                    return Err(io_err!("zip.unzip: unsafe symlink '{}' -> '{}' (outside the target folder)", name, link));
                }
                Some(link)
            } else {
                None
            };
            if entry.is_dir() {
                std::fs::create_dir_all(&target).map_err(|e| io_err!("zip.unzip: {}: {}", path_str(&target), e))?;
                inside_dest(Path::new(&dest), &target, &name, "zip.unzip")?;
                continue;
            }
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p).map_err(|e| io_err!("zip.unzip: {}: {}", path_str(p), e))?;
                inside_dest(Path::new(&dest), p, &name, "zip.unzip")?;
            }
            // never write through a link left at the target (by the archive or before)
            if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
                std::fs::remove_file(&target).map_err(|e| io_err!("zip.unzip: {}: {}", path_str(&target), e))?;
            }
            if let Some(link) = link {
                // a real link on Unix; elsewhere a file holding the link target (like Python's zipfile)
                #[cfg(unix)]
                std::os::unix::fs::symlink(&link, &target).map_err(|e| io_err!("zip.unzip: {}: {}", path_str(&target), e))?;
                #[cfg(not(unix))]
                std::fs::write(&target, link.as_bytes()).map_err(|e| io_err!("zip.unzip: {}: {}", path_str(&target), e))?;
                out.push(Value::Str(path_str(&target).into()));
                continue;
            }
            let mut w = File::create(&target).map_err(|e| io_err!("zip.unzip: {}: {}", path_str(&target), e))?;
            std::io::copy(&mut entry, &mut w).map_err(|e| io_err!("zip.unzip: {}: {}", name, e))?;
            // only the rwx bits: never setuid / setgid / sticky
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = w.set_permissions(std::fs::Permissions::from_mode(mode & 0o777));
            }
            out.push(Value::Str(path_str(&target).into()));
        }
        Ok(list(out))
    });
    // list(src) → [{ name, size, compressedSize, isDir, modified, encrypted }]
    f("list", 1, |a| {
        max_args(a, 1, "zip.list")?;
        let src = str_arg(a, 0, "zip.list")?;
        let mut ar = open_zip(&src, "zip.list")?;
        let mut out = Vec::new();
        for i in 0..ar.len() {
            let e = ar.by_index_raw(i).map_err(|e| io_err!("zip.list: {}", e))?;
            let modified = e.last_modified().map(|d| format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", d.year(), d.month(), d.day(), d.hour(), d.minute(), d.second()));
            out.push(map(vec![
                ("name", Value::Str(entry_name(&e).into())),
                ("size", Value::Int(e.size() as i64)),
                ("compressedSize", Value::Int(e.compressed_size() as i64)),
                ("isDir", Value::Bool(e.is_dir())),
                ("encrypted", Value::Bool(e.encrypted())),
                ("modified", modified.map_or(Value::Nil, Value::str)),
            ]));
        }
        Ok(list(out))
    });
    // read(src, name, { bytes, password }) → str (or bytes when not UTF-8 / asked)
    f("read", 2, |a| {
        max_args(a, 3, "zip.read")?;
        let src = str_arg(a, 0, "zip.read")?;
        let name = str_arg(a, 1, "zip.read")?;
        let o = opts(opt_arg(a, 2))?;
        let mut ar = open_zip(&src, "zip.read")?;
        let idx = ar.index_for_name(&name).ok_or_else(|| io_err!("zip.read: no entry '{}' in {}", name, src))?;
        let mut entry = by_index(&mut ar, idx, &o, "zip.read")?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(|e| io_err!("zip.read: {}: {}", name, e))?;
        Ok(text_or_bytes(data, &o))
    });
    // tar(dest, source | [sources]) — .tar.gz / .tgz are gzip-compressed
    f("tar", 2, |a| {
        max_args(a, 2, "zip.tar")?;
        let dest = str_arg(a, 0, "zip.tar")?;
        let sources = paths(&a[1], "zip.tar")?;
        let file = File::create(&dest).map_err(|e| io_err!("zip.tar: {}: {}", dest, e))?;
        let w: Box<dyn Write> = if is_gz(&dest) { Box::new(flate2::write::GzEncoder::new(file, flate2::Compression::default())) } else { Box::new(file) };
        let mut b = tar::Builder::new(w);
        let mut n = 0;
        for s in &sources {
            let mut entries = Vec::new();
            collect(Path::new(s), &mut entries).map_err(|e| io_err!("zip.tar: {}: {}", s, e))?;
            for (path, name) in &entries {
                // headers built by hand: tar's metadata helpers do not exist on every platform (WebAssembly)
                let meta = std::fs::metadata(path).map_err(|e| io_err!("zip.tar: {}: {}", path_str(path), e))?;
                let mut h = tar::Header::new_gnu();
                let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                h.set_mtime(mtime);
                if meta.is_dir() {
                    h.set_entry_type(tar::EntryType::Directory);
                    h.set_mode(0o755);
                    h.set_size(0);
                    h.set_cksum();
                    b.append_data(&mut h, format!("{}/", name), std::io::empty()).map_err(|e| io_err!("zip.tar: {}", e))?;
                } else {
                    #[cfg(unix)]
                    let mode = {
                        use std::os::unix::fs::PermissionsExt;
                        meta.permissions().mode() & 0o777
                    };
                    #[cfg(not(unix))]
                    let mode = 0o644;
                    h.set_entry_type(tar::EntryType::Regular);
                    h.set_mode(mode);
                    h.set_size(meta.len());
                    h.set_cksum();
                    let f = File::open(path).map_err(|e| io_err!("zip.tar: {}: {}", path_str(path), e))?;
                    b.append_data(&mut h, name, f).map_err(|e| io_err!("zip.tar: {}", e))?;
                }
            }
            n += 1;
        }
        let w = b.into_inner().map_err(|e| io_err!("zip.tar: {}", e))?;
        drop(w);
        Ok(Value::Int(n))
    });
    // untar(src, destDir = ".") → extracted paths
    f("untar", 1, |a| {
        max_args(a, 2, "zip.untar")?;
        let src = str_arg(a, 0, "zip.untar")?;
        let dest = match opt_arg(a, 1) { Some(Value::Str(d)) => d.clone(), _ => ".".into() };
        let mut ar = tar::Archive::new(tar_reader(&src, "zip.untar")?);
        std::fs::create_dir_all(&dest).map_err(|e| io_err!("zip.untar: {}: {}", dest, e))?;
        let mut out = Vec::new();
        for entry in ar.entries().map_err(|e| io_err!("zip.untar: {}: {}", src, e))? {
            let mut entry = entry.map_err(|e| io_err!("zip.untar: {}", e))?;
            let rel = entry.path().map_err(|e| io_err!("zip.untar: {}", e))?.into_owned();
            // tar refuses `..` names and writing through links that leave dest, but not
            // creating such a link: check symlink targets here (hard links tar checks)
            if entry.header().entry_type().is_symlink() {
                let link = entry.link_name().map_err(|e| io_err!("zip.untar: {}", e))?.unwrap_or_default().into_owned();
                if !safe_link(Path::new(&dest), &rel, &link) {
                    return Err(io_err!("zip.untar: unsafe symlink '{}' -> '{}' (outside the target folder)", path_str(&rel), path_str(&link)));
                }
            }
            // unpack_in refuses `..` names (a leading `/` is dropped: the entry stays inside)
            let ok = entry.unpack_in(&dest).map_err(|e| io_err!("zip.untar: {}: {}", path_str(&rel), e))?;
            if !ok {
                return Err(io_err!("zip.untar: unsafe entry name '{}' (outside the target folder)", path_str(&rel)));
            }
            if entry.header().entry_type().is_file() {
                out.push(path_str(&Path::new(&dest).join(&rel)));
            }
        }
        out.sort();
        Ok(crate::native::util::str_list(out))
    });
    // listTar(src) → [{ name, size, isDir }]
    f("listTar", 1, |a| {
        let src = str_arg(a, 0, "zip.listTar")?;
        let mut ar = tar::Archive::new(tar_reader(&src, "zip.listTar")?);
        let mut out = Vec::new();
        for entry in ar.entries().map_err(|e| io_err!("zip.listTar: {}: {}", src, e))? {
            let entry = entry.map_err(|e| io_err!("zip.listTar: {}", e))?;
            let name = entry.path().map(|p| path_str(&p)).unwrap_or_default();
            out.push(map(vec![
                ("name", Value::Str(name.into())),
                ("size", Value::Int(entry.header().size().unwrap_or(0) as i64)),
                ("isDir", Value::Bool(entry.header().entry_type().is_dir())),
            ]));
        }
        Ok(list(out))
    });
    // gzip(str | bytes, { level }) → bytes · gunzip(bytes, { bytes }) → str | bytes
    f("gzip", 1, |a| {
        let data = bytes_of(&a[0], "zip.gzip")?;
        let o = opts(opt_arg(a, 1))?;
        let level = match o.get("level") { Some(Value::Int(n)) if (0..=9).contains(n) => *n as u32, _ => 6 };
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
        enc.write_all(&data).map_err(|e| io_err!("zip.gzip: {}", e))?;
        Ok(bytes_value(enc.finish().map_err(|e| io_err!("zip.gzip: {}", e))?))
    });
    f("gunzip", 1, |a| {
        let data = bytes_of(&a[0], "zip.gunzip")?;
        let o = opts(opt_arg(a, 1))?;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut out).map_err(|e| io_err!("zip.gunzip: not gzip data ({})", e))?;
        Ok(text_or_bytes(out, &o))
    });
    // gzipFile(src, dest = src + ".gz") · gunzipFile(src, dest = src without ".gz")
    f("gzipFile", 1, |a| {
        let src = str_arg(a, 0, "zip.gzipFile")?;
        let dest = match opt_arg(a, 1) { Some(Value::Str(d)) => d.clone(), _ => format!("{}.gz", src).into() };
        let mut r = File::open(&src).map_err(|e| io_err!("zip.gzipFile: {}: {}", src, e))?;
        let w = File::create(&dest).map_err(|e| io_err!("zip.gzipFile: {}: {}", dest, e))?;
        let mut enc = flate2::write::GzEncoder::new(w, flate2::Compression::default());
        std::io::copy(&mut r, &mut enc).and_then(|_| enc.finish().map(|_| ())).map_err(|e| io_err!("zip.gzipFile: {}", e))?;
        Ok(Value::Str(dest))
    });
    f("gunzipFile", 1, |a| {
        let src = str_arg(a, 0, "zip.gunzipFile")?;
        let dest = match opt_arg(a, 1) { Some(Value::Str(d)) => d.clone(), _ => src.strip_suffix(".gz").map(String::from).unwrap_or_else(|| format!("{}.out", src)).into() };
        let r = File::open(&src).map_err(|e| io_err!("zip.gunzipFile: {}: {}", src, e))?;
        let mut w = File::create(&dest).map_err(|e| io_err!("zip.gunzipFile: {}: {}", dest, e))?;
        std::io::copy(&mut flate2::read::GzDecoder::new(r), &mut w).map_err(|e| io_err!("zip.gunzipFile: {}: {}", src, e))?;
        Ok(Value::Str(dest))
    });
    ModuleValue {
        name: "fogu:zip".into(),
        exports: RefCell::new(e),
    }
}

fn opts(v: Option<&Value>) -> Result<HashMap<String, Value>, String> {
    match v {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("zip: options must be a map, got '{}'", other.type_name())),
    }
}

fn paths(v: &Value, what: &str) -> Result<Vec<String>, String> {
    match v {
        Value::Str(s) => Ok(vec![s.to_string()]),
        Value::List(l) => l.borrow().iter().map(|x| match x {
            Value::Str(s) => Ok(s.to_string()),
            other => Err(format!("{}: paths must be str, got '{}'", what, other.type_name())),
        }).collect(),
        other => Err(format!("{}: expects a path or a list of paths, got '{}'", what, other.type_name())),
    }
}

/// Files and folders under `p`, named relative to `p`'s parent (like `zip -r`).
fn collect(p: &Path, out: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
    let base = p.parent().unwrap_or(Path::new(""));
    fn walk(p: &Path, base: &Path, out: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
        let name = path_str(p.strip_prefix(base).unwrap_or(p)).trim_start_matches("./").to_string();
        if p.is_dir() {
            out.push((p.to_path_buf(), name));
            let mut children: Vec<PathBuf> = std::fs::read_dir(p)?.flatten().map(|e| e.path()).collect();
            children.sort();
            for c in children {
                walk(&c, base, out)?;
            }
        } else if p.exists() {
            out.push((p.to_path_buf(), name));
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"));
        }
        Ok(())
    }
    walk(p, base, out)
}

fn file_options<'a>(o: &HashMap<String, Value>, pw: Option<&'a str>) -> Result<zip::write::FileOptions<'a, 'static, ()>, String> {
    let mut fo = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    if o.get("store").is_some_and(Value::is_truthy) {
        fo = fo.compression_method(zip::CompressionMethod::Stored);
    } else {
        let level = match o.get("level") {
            Some(Value::Int(n)) if (0..=9).contains(n) => *n,
            Some(other) => return Err(format!("zip: 'level' must be an int in 0..9, got {}", other.to_display_string())),
            None => 6,
        };
        fo = fo.compression_method(zip::CompressionMethod::Deflated).compression_level(Some(level));
    }
    Ok(match pw {
        Some(p) => fo.with_aes_encryption(zip::AesMode::Aes256, p),
        None => fo,
    })
}

fn put(w: &mut zip::ZipWriter<File>, name: &str, content: &Value, o: &HashMap<String, Value>) -> Result<(), String> {
    let pw = o.get("password").map(|v| v.to_display_string());
    let fo = file_options(o, pw.as_deref())?;
    if name.ends_with('/') || matches!(content, Value::Nil) {
        return w.add_directory(name.trim_end_matches('/').to_string() + "/", fo).map_err(|e| io_err!("zip: {}", e));
    }
    let data = bytes_of(content, "zip")?;
    w.start_file(name.to_string(), fo).map_err(|e| io_err!("zip: {}", e))?;
    w.write_all(&data).map_err(|e| io_err!("zip: {}", e))
}

fn open_zip(path: &str, what: &str) -> Result<zip::ZipArchive<File>, String> {
    let f = File::open(path).map_err(|e| io_err!("{}: {}: {}", what, path, e))?;
    zip::ZipArchive::new(f).map_err(|e| io_err!("{}: {}: {}", what, path, e))
}

fn by_index<'a>(ar: &'a mut zip::ZipArchive<File>, i: usize, o: &HashMap<String, Value>, what: &str) -> Result<zip::read::ZipFile<'a, File>, String> {
    match o.get("password") {
        Some(pw) => ar.by_index_decrypt(i, pw.to_display_string().as_bytes()).map_err(|e| io_err!("{}: {}", what, e)),
        None => ar.by_index(i).map_err(|e| match e {
            zip::result::ZipError::UnsupportedArchive(m) if m.contains("Password") || m.contains("encrypted") => io_err!("{}: the archive is encrypted (give {{ password }})", what),
            e => io_err!("{}: {}", what, e),
        }),
    }
}

fn entry_name(e: &zip::read::ZipFile<'_, File>) -> String {
    e.name().map(|n| n.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(e.name_raw()).into_owned())
}

/// Errors when `dir` (already created) resolves outside `dest` — i.e. through a link.
fn inside_dest(dest: &Path, dir: &Path, name: &str, what: &str) -> Result<(), String> {
    let (Ok(d), Ok(p)) = (dest.canonicalize(), dir.canonicalize()) else { return Ok(()) };
    if p.starts_with(&d) {
        Ok(())
    } else {
        Err(io_err!("{}: unsafe entry name '{}' (through a link outside the target folder)", what, name))
    }
}

/// A link stored at `rel` (relative to `dest`) pointing to `target` stays inside
/// `dest`: the target is relative, its `..` come first and do not climb above
/// `dest`, and they climb only through real folders (no link in `rel`'s parents).
fn safe_link(dest: &Path, rel: &Path, target: &Path) -> bool {
    use std::path::Component;
    let parents: Vec<&std::ffi::OsStr> = rel.parent().map_or(Vec::new(), |p| p.components().filter_map(|c| match c {
        Component::Normal(n) => Some(n),
        _ => None,
    }).collect());
    let mut ups = 0;
    let mut seen_name = false;
    for c in target.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => return false,
            Component::CurDir => {}
            Component::ParentDir if seen_name => return false,
            Component::ParentDir => ups += 1,
            Component::Normal(_) => seen_name = true,
        }
    }
    if target.as_os_str().is_empty() || ups > parents.len() {
        return false;
    }
    if ups > 0 {
        let mut p = dest.to_path_buf();
        for n in &parents {
            p.push(n);
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
                return false;
            }
        }
    }
    true
}

fn text_or_bytes(data: Vec<u8>, o: &HashMap<String, Value>) -> Value {
    if !o.get("bytes").is_some_and(Value::is_truthy) {
        if let Ok(s) = String::from_utf8(data.clone()) {
            return Value::Str(s.into());
        }
    }
    bytes_value(data)
}

fn is_gz(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    l.ends_with(".gz") || l.ends_with(".tgz")
}

fn tar_reader(path: &str, what: &str) -> Result<Box<dyn Read>, String> {
    let f = File::open(path).map_err(|e| io_err!("{}: {}: {}", what, path, e))?;
    Ok(if is_gz(path) { Box::new(flate2::read::GzDecoder::new(f)) } else { Box::new(f) })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        let m = build();
        let f = m.exports.borrow().get(name).cloned().unwrap();
        let Value::NativeFn(f) = f else { panic!("not a native fn") };
        (f.func)(args)
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fogu-zip-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn archive(path: &Path, build: impl FnOnce(&mut zip::ZipWriter<File>)) {
        let mut w = zip::ZipWriter::new(File::create(path).unwrap());
        build(&mut w);
        w.finish().unwrap();
    }

    #[test]
    fn setuid_bits_are_dropped() {
        let d = tmp("suid");
        let z = d.join("a.zip");
        archive(&z, |w| {
            w.start_file("run.sh", zip::write::SimpleFileOptions::default().unix_permissions(0o6755)).unwrap();
            w.write_all(b"#!/bin/sh\n").unwrap();
        });
        let out = d.join("out");
        call("unzip", &[Value::str(path_str(&z)), Value::str(path_str(&out))]).unwrap();
        let mode = std::fs::metadata(out.join("run.sh")).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o755);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn never_writes_through_a_link_outside() {
        let d = tmp("through");
        let outside = d.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        // a link left in the target folder (not by the archive) pointing outside
        std::os::unix::fs::symlink(&outside, out.join("evil")).unwrap();
        std::os::unix::fs::symlink(outside.join("f.txt"), out.join("f.txt")).unwrap();
        let z = d.join("a.zip");
        archive(&z, |w| {
            w.start_file("evil/x.txt", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"x").unwrap();
        });
        let err = call("unzip", &[Value::str(path_str(&z)), Value::str(path_str(&out))]).unwrap_err();
        assert!(err.contains("through a link"), "{}", err);
        assert!(!outside.join("x.txt").exists());
        // a file entry replaces a link at its own path instead of following it
        let z2 = d.join("b.zip");
        archive(&z2, |w| {
            w.start_file("f.txt", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"inside").unwrap();
        });
        call("unzip", &[Value::str(path_str(&z2)), Value::str(path_str(&out))]).unwrap();
        assert!(!outside.join("f.txt").exists());
        assert_eq!(std::fs::read_to_string(out.join("f.txt")).unwrap(), "inside");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn link_targets() {
        let d = Path::new("/nonexistent-dest");
        assert!(safe_link(d, Path::new("a"), Path::new("b")));
        assert!(safe_link(d, Path::new("bin/x"), Path::new("../lib/x.so")));
        assert!(!safe_link(d, Path::new("a"), Path::new("../b")));
        assert!(!safe_link(d, Path::new("a"), Path::new("/etc/passwd")));
        assert!(!safe_link(d, Path::new("a/b"), Path::new("c/../../..")));
        assert!(!safe_link(d, Path::new("a"), Path::new("")));
    }
}
