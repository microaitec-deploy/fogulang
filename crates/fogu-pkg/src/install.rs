//! Resolving, downloading and installing packages into the global store.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::semver;
use super::source::{self, Reference, Source};
use super::{Lock, LockEntry, Manifest, LOCKFILE, MANIFEST};

pub struct Installed {
    pub name: String,
    pub version: String,
    pub commit: String,
    pub fresh: bool,
}

#[derive(Default)]
pub struct Report {
    pub packages: Vec<Installed>,
    pub warnings: Vec<String>,
}

pub struct Options<'a> {
    /// Re-resolve these packages ignoring the lock (`None` = only missing ones;
    /// `Some(empty)` = all).
    pub refresh: Option<HashSet<String>>,
    /// Fail instead of changing the lock (CI).
    pub frozen: bool,
    pub log: &'a dyn Fn(&str),
}

// ---------------------------------------------------------------------
// git
// ---------------------------------------------------------------------

fn git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut c = Command::new("git");
    c.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "file:git:http:https:ssh")
        .env("GIT_SSH_COMMAND", std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".into()));
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let out = c.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "the 'git' command was not found; install git to use packages".to_string()
        } else {
            format!("failed to run git: {}", e)
        }
    })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
            .map(|l| l.trim_start_matches("fatal:").trim_start_matches("error:"))
            .or_else(|| err.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("unknown error");
        return Err(format!("git {}: {}", args.first().unwrap_or(&""), line.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn check_url(url: &str) -> Result<(), String> {
    if url.starts_with('-') || url.contains(['\n', '\0']) || url.to_lowercase().starts_with("ext::") {
        return Err(format!("invalid git URL: '{}'", url));
    }
    Ok(())
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Returns `(commit, semver_tag_name?)`.
fn resolve_git(url: &str, reference: &Reference) -> Result<(String, Option<String>), String> {
    check_url(url)?;
    if let Reference::Named(r) = reference {
        if r.len() == 40 && is_hex(r) {
            return Ok((r.to_lowercase(), None));
        }
    }
    let out = git(&["ls-remote", "--", url], None)?;
    let mut refs: Vec<(String, String)> = Vec::new();
    for l in out.lines() {
        if let Some((sha, name)) = l.split_once('\t') {
            refs.push((sha.to_string(), name.to_string()));
        }
    }
    let find = |name: &str| refs.iter().find(|(_, n)| n == name).map(|(s, _)| s.clone());
    match reference {
        Reference::Default => find("HEAD")
            .map(|s| (s, None))
            .ok_or_else(|| format!("'{}' has no HEAD (empty repository?)", url)),
        Reference::Named(r) => {
            if let Some(s) = find(&format!("refs/tags/{}^{{}}", r))
                .or_else(|| find(&format!("refs/tags/{}", r)))
                .or_else(|| find(&format!("refs/heads/{}", r)))
            {
                return Ok((s, None));
            }
            if r.len() >= 7 && is_hex(r) {
                if let Some((s, _)) = refs.iter().find(|(s, _)| s.starts_with(&r.to_lowercase())) {
                    return Ok((s.clone(), None));
                }
                return Err(format!("commit '{}' is not a ref tip; use the full SHA (40 characters)", r));
            }
            Err(format!("reference '{}' not found in {}", r, url))
        }
        Reference::Semver(range) => {
            let rg = semver::parse_range(range).ok_or_else(|| format!("invalid version range: '{}'", range))?;
            let mut tags: Vec<(String, semver::Version)> = Vec::new();
            for (_, n) in &refs {
                if let Some(t) = n.strip_prefix("refs/tags/") {
                    let t = t.trim_end_matches("^{}");
                    if let Some(v) = semver::parse_version(t) {
                        if !tags.iter().any(|(x, _)| x == t) {
                            tags.push((t.to_string(), v));
                        }
                    }
                }
            }
            let (tag, _) = semver::best(&rg, &tags)
                .ok_or_else(|| format!("no tag of {} satisfies '{}' (tags: {})", url, range, tags.len()))?
                .clone();
            let sha = find(&format!("refs/tags/{}^{{}}", tag))
                .or_else(|| find(&format!("refs/tags/{}", tag)))
                .ok_or("tag disappeared")?;
            Ok((sha, Some(tag)))
        }
    }
}

fn fetch_git(url: &str, commit: &str, dest: &Path) -> Result<(), String> {
    check_url(url)?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let d = dest.to_string_lossy().to_string();
    let shallow = (|| -> Result<(), String> {
        git(&["init", "-q", &d], None)?;
        git(&["-C", &d, "remote", "add", "origin", "--", url], None)?;
        git(&["-C", &d, "fetch", "-q", "--depth", "1", "origin", commit], None)?;
        git(&["-C", &d, "checkout", "-q", "FETCH_HEAD"], None)?;
        Ok(())
    })();
    if shallow.is_err() {
        let _ = std::fs::remove_dir_all(dest);
        git(&["clone", "-q", "--", url, &d], None)?;
        git(&["-C", &d, "checkout", "-q", commit], None)?;
    }
    let head = git(&["-C", &d, "rev-parse", "HEAD"], None)?;
    if head.trim() != commit {
        return Err(format!("fetched commit ({}) differs from the expected one ({})", head.trim(), commit));
    }
    std::fs::remove_dir_all(dest.join(".git")).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------
// tarball / local / hash
// ---------------------------------------------------------------------

#[cfg(target_family = "wasm")]
fn download(url: &str) -> Result<Vec<u8>, String> {
    Err(format!("cannot download {} on WebAssembly", url))
}

#[cfg(not(target_family = "wasm"))]
fn download(url: &str) -> Result<Vec<u8>, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("invalid URL: {}", url));
    }
    let resp = crate::net::blocking_client()
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("failed to download {}: {}", url, crate::net::error_chain(&e)))?;
    let mut buf = Vec::new();
    resp.take(256 * 1024 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| format!("failed to read {}: {}", url, e))?;
    Ok(buf)
}

fn sha_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn extract_tarball(bytes: &[u8], dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let gz = flate2::read::GzDecoder::new(bytes);
    tar::Archive::new(gz).unpack(dest).map_err(|e| format!("invalid archive: {}", e))?;
    // GitHub & co. wrap everything in a single folder
    let entries: Vec<_> = std::fs::read_dir(dest).map_err(|e| e.to_string())?.flatten().collect();
    if entries.len() == 1 && entries[0].path().is_dir() {
        let inner = entries[0].path();
        let tmp = dest.with_extension("inner");
        std::fs::rename(&inner, &tmp).map_err(|e| e.to_string())?;
        std::fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}

const SKIP: &[&str] = &[".git", "fogu_modules", "node_modules", "target"];

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for ent in std::fs::read_dir(from).map_err(|e| format!("{}: {}", from.display(), e))?.flatten() {
        let name = ent.file_name();
        if SKIP.contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        let ft = ent.file_type().map_err(|e| e.to_string())?;
        let dst = to.join(&name);
        if ft.is_dir() {
            copy_tree(&ent.path(), &dst)?;
        } else if ft.is_file() {
            std::fs::copy(ent.path(), &dst).map_err(|e| e.to_string())?;
        } // symlinks are ignored
    }
    Ok(())
}

/// Deterministic sha256 of the tree (sorted paths + content).
pub fn tree_hash(dir: &Path) -> Result<String, String> {
    fn walk(base: &Path, cur: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for e in std::fs::read_dir(cur)?.flatten() {
            let ft = e.file_type()?;
            if ft.is_dir() {
                walk(base, &e.path(), out)?;
            } else if ft.is_file() {
                out.push(e.path().strip_prefix(base).unwrap().to_path_buf());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files).map_err(|e| e.to_string())?;
    files.sort();
    let mut h = Sha256::new();
    for f in files {
        let data = std::fs::read(dir.join(&f)).map_err(|e| e.to_string())?;
        h.update(f.to_string_lossy().replace('\\', "/").as_bytes());
        h.update([0]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(&data);
    }
    Ok(format!("sha256-{}", h.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>()))
}

// ---------------------------------------------------------------------
// installation
// ---------------------------------------------------------------------

struct Ctx<'a> {
    root: PathBuf,
    old: Lock,
    new: Lock,
    visited: HashMap<String, String>,
    report: Report,
    opts: &'a Options<'a>,
}

fn tmp_dir() -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    super::home().join(".tmp").join(format!("{}-{}", std::process::id(), n))
}

impl Ctx<'_> {
    fn wants_refresh(&self, name: &str) -> bool {
        match &self.opts.refresh {
            None => false,
            Some(s) => s.is_empty() || s.contains(name),
        }
    }

    fn install(&mut self, name: &str, spec: &str, local_base: Option<&Path>) -> Result<(), String> {
        if !super::valid_name(name) {
            return Err(format!("invalid package name: '{}'", name));
        }
        if let Some(prev) = self.visited.get(name) {
            if prev != spec {
                self.report.warnings.push(format!(
                    "conflict in '{}': already installed as '{}', ignored '{}'",
                    name, prev, spec
                ));
            }
            return Ok(());
        }
        self.visited.insert(name.to_string(), spec.to_string());

        let (src, _) = source::parse(spec).map_err(|e| format!("{}: {}", name, e))?;
        let locked = self.old.packages.get(name).filter(|e| e.spec == spec && !self.wants_refresh(name)).cloned();

        let mut entry: LockEntry;
        let dir: PathBuf;
        let mut fresh = false;

        match &src {
            Source::Git { url, reference } => {
                let (commit, tag) = match &locked {
                    Some(e) => (e.commit.clone(), None),
                    None => {
                        if self.opts.frozen {
                            return Err(format!("'{}' is not in {} (--frozen mode)", name, LOCKFILE));
                        }
                        (self.opts.log)(&format!("resolving {} ({})", name, url));
                        resolve_git(url, reference)?
                    }
                };
                dir = super::store_dir(name, &commit);
                if !dir.is_dir() {
                    (self.opts.log)(&format!("fetching {}@{}", name, &commit[..commit.len().min(8)]));
                    let tmp = tmp_dir();
                    fetch_git(url, &commit, &tmp).inspect_err(|_| {
                        let _ = std::fs::remove_dir_all(&tmp);
                    })?;
                    self.place(&tmp, &dir)?;
                    fresh = true;
                }
                entry = LockEntry {
                    spec: spec.to_string(),
                    resolved: url.clone(),
                    commit,
                    version: tag.map(|t| t.trim_start_matches('v').to_string()).unwrap_or_default(),
                    ..Default::default()
                };
                if let Some(l) = &locked {
                    entry.version = l.version.clone();
                }
            }
            Source::Tarball { url } => {
                let want = locked.as_ref().map(|e| e.commit.clone());
                let existing = want.as_ref().map(|c| super::store_dir(name, c)).filter(|d| d.is_dir());
                let commit = if let (Some(c), Some(_)) = (&want, &existing) {
                    c.clone()
                } else {
                    if self.opts.frozen && locked.is_none() {
                        return Err(format!("'{}' is not in {} (--frozen mode)", name, LOCKFILE));
                    }
                    (self.opts.log)(&format!("downloading {}", url));
                    let bytes = download(url)?;
                    let c = sha_hex(&bytes);
                    if let Some(w) = &want {
                        if *w != c {
                            return Err(format!("the archive of '{}' changed (checksum differs from {})", name, LOCKFILE));
                        }
                    }
                    let d = super::store_dir(name, &c);
                    if !d.is_dir() {
                        let tmp = tmp_dir();
                        extract_tarball(&bytes, &tmp).inspect_err(|_| {
                            let _ = std::fs::remove_dir_all(&tmp);
                        })?;
                        self.place(&tmp, &d)?;
                        fresh = true;
                    }
                    c
                };
                dir = super::store_dir(name, &commit);
                entry = LockEntry { spec: spec.to_string(), resolved: url.clone(), commit, ..Default::default() };
            }
            Source::Local { path } => {
                let base = local_base.unwrap_or(&self.root);
                let p = base.join(path);
                let p = p.canonicalize().map_err(|e| format!("{}: local folder '{}': {}", name, p.display(), e))?;
                let tmp = tmp_dir();
                copy_tree(&p, &tmp).inspect_err(|_| {
                    let _ = std::fs::remove_dir_all(&tmp);
                })?;
                let h = tree_hash(&tmp)?;
                let commit = h.trim_start_matches("sha256-").to_string();
                dir = super::store_dir(name, &commit);
                if dir.is_dir() {
                    let _ = std::fs::remove_dir_all(&tmp);
                } else {
                    self.place(&tmp, &dir)?;
                    fresh = true;
                }
                entry = LockEntry { spec: spec.to_string(), resolved: p.display().to_string(), commit, ..Default::default() };
            }
        }

        // checksum
        let integrity = tree_hash(&dir)?;
        if let Some(l) = &locked {
            if !l.integrity.is_empty() && l.integrity != integrity {
                return Err(format!(
                    "integrity check of '{}' failed: the content in {} does not match {}\n  expected {}\n  got      {}",
                    name,
                    dir.display(),
                    LOCKFILE,
                    l.integrity,
                    integrity
                ));
            }
        }
        entry.integrity = integrity;

        // the package's own dependencies
        let mut sub: Vec<(String, String)> = Vec::new();
        if let Ok(m) = Manifest::load(&dir.join(MANIFEST)) {
            if entry.version.is_empty() {
                entry.version = m.str_field("version").unwrap_or_default();
            }
            sub = m.deps();
        }
        entry.dependencies = sub.iter().cloned().collect::<BTreeMap<_, _>>();
        self.report.packages.push(Installed {
            name: name.to_string(),
            version: entry.version.clone(),
            commit: entry.commit.clone(),
            fresh,
        });
        self.new.packages.insert(name.to_string(), entry);

        for (dn, ds) in sub {
            if matches!(source::parse(&ds), Ok((Source::Local { .. }, _))) {
                return Err(format!("'{}' depends on '{}' through a local path ({}), which is not portable", name, dn, ds));
            }
            self.install(&dn, &ds, None)?;
        }
        Ok(())
    }

    fn place(&self, tmp: &Path, dest: &Path) -> Result<(), String> {
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        match std::fs::rename(tmp, dest) {
            Ok(()) => Ok(()),
            Err(_) if dest.is_dir() => {
                let _ = std::fs::remove_dir_all(tmp);
                Ok(()) // another installation won the race
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(tmp);
                Err(format!("could not move to {}: {}", dest.display(), e))
            }
        }
    }
}

/// Installs all dependencies of the manifest in `root` and updates the lock.
pub fn install_all(root: &Path, manifest: &Manifest, opts: &Options) -> Result<Report, String> {
    let lock_path = root.join(LOCKFILE);
    let old = Lock::load(&lock_path);
    let mut cx = Ctx {
        root: root.to_path_buf(),
        old: old.clone(),
        new: Lock::default(),
        visited: HashMap::new(),
        report: Report::default(),
        opts,
    };
    for (name, spec) in manifest.deps() {
        cx.install(&name, &spec, None)?;
    }
    let _ = std::fs::remove_dir_all(super::home().join(".tmp"));
    if cx.new.packages != old.packages {
        if opts.frozen {
            return Err(format!("{} needs updating, but --frozen was requested", LOCKFILE));
        }
        cx.new.save(&lock_path)?;
    }
    Ok(cx.report)
}

/// Total size (bytes) of a directory.
pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            if let Ok(ft) = e.file_type() {
                if ft.is_dir() {
                    total += dir_size(&e.path());
                } else if let Ok(m) = e.metadata() {
                    total += m.len();
                }
            }
        }
    }
    total
}
