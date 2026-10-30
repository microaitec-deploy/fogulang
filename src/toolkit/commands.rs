//! CLI subcommands: init, add, remove, install, update, list, pm, check, watch.

use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::ui;
use crate::pkg::install::{self, Options, Report};
use crate::pkg::source;
use crate::pkg::{self, Lock, Manifest, LOCKFILE, MANIFEST};

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn fail(msg: &str) -> i32 {
    ui::error(msg);
    1
}

// ---------------------------------------------------------------------
// project / scripts
// ---------------------------------------------------------------------

pub fn default_entry() -> Option<String> {
    let root = pkg::find_root(&cwd())?;
    let m = Manifest::load(&root.join(MANIFEST)).ok()?;
    let e = root.join(m.entry()?);
    Some(e.to_string_lossy().to_string())
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Runs `fogu <name>` as a script from fogu.json. `None` if it does not exist.
pub fn try_script(name: &str, extra: &[String]) -> Option<i32> {
    let root = pkg::find_root(&cwd())?;
    let m = Manifest::load(&root.join(MANIFEST)).ok()?;
    let (_, cmd) = m.scripts().into_iter().find(|(k, _)| k == name)?;
    let mut line = cmd.clone();
    for a in extra {
        line.push(' ');
        line.push_str(&sh_quote(a));
    }
    eprintln!("{} {}", ui::dim("$"), ui::dim(&line));
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", &line]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", &line]);
        c
    };
    // 'fogu' inside scripts is this very executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let mut paths = vec![dir.to_path_buf()];
            if let Some(p) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&p));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                c.env("PATH", joined);
            }
        }
    }
    c.env("FOGU_PROJECT", &root).current_dir(&root);
    Some(match c.status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => fail(&format!("could not run the script: {}", e)),
    })
}

// ---------------------------------------------------------------------
// init
// ---------------------------------------------------------------------

fn ask(prompt: &str, default: &str) -> String {
    print!("{} {} ", ui::bold(prompt), ui::dim(&format!("({})", default)));
    let _ = std::io::stdout().flush();
    let mut l = String::new();
    if std::io::stdin().read_line(&mut l).is_err() {
        return default.to_string();
    }
    let l = l.trim();
    if l.is_empty() { default.to_string() } else { l.to_string() }
}

pub fn init(args: &[String]) -> i32 {
    let yes = args.iter().any(|a| a == "-y" || a == "--yes") || !std::io::stdin().is_terminal();
    let dir = cwd();
    let path = dir.join(MANIFEST);
    if path.exists() {
        return fail(&format!("{} already exists in this directory", MANIFEST));
    }
    let dirname = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "my-app".into());
    let dirname: String = dirname
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '-' })
        .collect();
    let (name, entry) = if yes {
        (dirname, "main.fg".to_string())
    } else {
        (ask("package name:", &dirname), ask("entry file:", "main.fg"))
    };
    let mut m = Manifest::new(&path, &name);
    if let Some(o) = m.json.as_object_mut() {
        o.insert("entry".into(), entry.clone().into());
        let mut sc = serde_json::Map::new();
        sc.insert("start".into(), format!("fogu run {}", entry).into());
        sc.insert("dev".into(), format!("fogu --watch {}", entry).into());
        o.insert("scripts".into(), sc.into());
    }
    if let Err(e) = m.save() {
        return fail(&e);
    }
    let ep = dir.join(&entry);
    if !ep.exists() {
        if let Some(p) = ep.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&ep, "print(\"Hello, Fogu!\")\n");
        println!("{} {}", ui::green("created"), entry);
    }
    println!("{} {}", ui::green("created"), MANIFEST);
    let gi = dir.join(".gitignore");
    if !gi.exists() {
        match std::fs::write(&gi, GITIGNORE) {
            Ok(()) => println!("{} .gitignore", ui::green("created")),
            Err(e) => ui::warn(&format!("could not create .gitignore: {}", e)),
        }
    }
    println!("\nNext steps:\n  {}\n  {}", ui::cyan(&format!("fogu run {}", entry)), ui::cyan("fogu add user/repo"));
    0
}

/// `.gitignore` created by `fogu init` (an existing one is never overwritten).
const GITIGNORE: &str = "\
# Fogu
# Packages live in the global store (~/.fogu); fogu.lock pins them.
# Remove the next line if you vendor packages into the project.
fogu_modules/

# local data created by fogu:kv / fogu:sql
*.redb
*.db
*.db-journal

# build output and environment
dist/
.env

# editors / OS
.vscode/
.idea/
.DS_Store
Thumbs.db
";

// ---------------------------------------------------------------------
// packages
// ---------------------------------------------------------------------

struct Project {
    root: PathBuf,
    manifest: Manifest,
    global: bool,
}

fn open_project(global: bool, create_global: bool) -> Result<Project, String> {
    if global {
        let root = pkg::home();
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let p = root.join(MANIFEST);
        let manifest = if p.is_file() {
            Manifest::load(&p)?
        } else if create_global {
            Manifest::new(&p, "global")
        } else {
            return Err("there are no global packages yet (use 'fogu add -g ...')".into());
        };
        return Ok(Project { root, manifest, global: true });
    }
    let root = pkg::find_root(&cwd())
        .ok_or_else(|| format!("no {} found; use 'fogu init' or the -g (global) option", MANIFEST))?;
    let manifest = Manifest::load(&root.join(MANIFEST))?;
    Ok(Project { root, manifest, global: false })
}

fn split_flags(args: &[String], known: &[&str]) -> Result<(Vec<String>, HashSet<String>), String> {
    let mut pos = Vec::new();
    let mut fl = HashSet::new();
    for a in args {
        if a.starts_with('-') && a.len() > 1 {
            if known.contains(&a.as_str()) {
                fl.insert(a.trim_start_matches('-').to_string());
            } else {
                return Err(format!("unknown option: {}", a));
            }
        } else {
            pos.push(a.clone());
        }
    }
    Ok((pos, fl))
}

fn has(fl: &HashSet<String>, names: &[&str]) -> bool {
    names.iter().any(|n| fl.contains(*n))
}

fn print_report(r: &Report, started: Instant, only: Option<&HashSet<String>>) {
    for w in &r.warnings {
        ui::warn(w);
    }
    let mut fresh = 0;
    let mut cached = 0;
    for p in &r.packages {
        if p.fresh {
            fresh += 1;
        } else {
            cached += 1;
        }
        let show = match only {
            Some(s) => s.contains(&p.name),
            None => p.fresh,
        };
        if show {
            let v = if p.version.is_empty() { String::new() } else { format!("@{}", p.version) };
            println!(
                "{} {}{} {}",
                ui::green("+"),
                ui::bold(&p.name),
                v,
                ui::dim(&format!("({})", &p.commit[..p.commit.len().min(8)]))
            );
        }
    }
    println!(
        "\n{} package(s) [{} new, {} cached] {}",
        r.packages.len(),
        fresh,
        cached,
        ui::dim(&format!("[{}ms]", started.elapsed().as_millis()))
    );
}

fn log(s: &str) {
    eprintln!("{}", ui::dim(&format!("  {}", s)));
}

pub fn add(args: &[String]) -> i32 {
    let (pos, fl) = match split_flags(args, &["-g", "--global"]) {
        Ok(x) => x,
        Err(e) => return fail(&e),
    };
    if pos.is_empty() {
        return fail("usage: fogu add <package...>   e.g.: fogu add mcu, fogu add github:user/repo");
    }
    let mut p = match open_project(has(&fl, &["g", "global"]), true) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let t0 = Instant::now();
    let mut names = HashSet::new();
    for a in &pos {
        let (alias, spec) = match source::split_add_arg(a) {
            Ok(x) => x,
            Err(e) => return fail(&e),
        };
        // a bare `name[@version]` is a library of the Fogu server
        let parsed = match pkg::registry::parse_name(&spec) {
            Some((lib, version)) if !spec.contains('#') => pkg::registry::resolve(&lib, version.as_deref()).map(|(url, v)| {
                log(&format!("{} {} from {}", lib, v, url));
                (source::Source::Tarball { url }, lib)
            }),
            _ => source::parse(&spec),
        };
        let (src, def_name) = match parsed {
            Ok(x) => x,
            Err(e) => return fail(&e),
        };
        let name = alias.unwrap_or(def_name);
        if !pkg::valid_name(&name) {
            return fail(&format!("invalid package name: '{}' (use 'alias@source')", name));
        }
        // local folders are stored as absolute paths in the global manifest
        let canon = match &src {
            source::Source::Local { path } if p.global => {
                let abs = cwd().join(path);
                format!("file:{}", abs.canonicalize().unwrap_or(abs).display())
            }
            _ => source::canonical(&src),
        };
        p.manifest.set_dep(&name, &canon);
        names.insert(name);
    }
    let opts = Options { refresh: Some(names.clone()), frozen: false, log: &log };
    match install::install_all(&p.root, &p.manifest, &opts) {
        Ok(r) => {
            if let Err(e) = p.manifest.save() {
                return fail(&e);
            }
            print_report(&r, t0, Some(&names));
            0
        }
        Err(e) => fail(&e),
    }
}

pub fn remove(args: &[String]) -> i32 {
    let (pos, fl) = match split_flags(args, &["-g", "--global"]) {
        Ok(x) => x,
        Err(e) => return fail(&e),
    };
    if pos.is_empty() {
        return fail("usage: fogu remove <package...>");
    }
    let mut p = match open_project(has(&fl, &["g", "global"]), false) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let t0 = Instant::now();
    for n in &pos {
        if !p.manifest.remove_dep(n) {
            return fail(&format!("'{}' is not in the dependencies", n));
        }
    }
    let opts = Options { refresh: None, frozen: false, log: &log };
    match install::install_all(&p.root, &p.manifest, &opts) {
        Ok(r) => {
            if let Err(e) = p.manifest.save() {
                return fail(&e);
            }
            for n in &pos {
                println!("{} {}", ui::yellow("-"), n);
            }
            println!("\n{}", ui::dim(&format!("[{}ms] ({} package(s) left; the global store is not cleaned)", t0.elapsed().as_millis(), r.packages.len())));
            0
        }
        Err(e) => fail(&e),
    }
}

pub fn install(args: &[String]) -> i32 {
    let (_, fl) = match split_flags(args, &["-g", "--global", "--frozen", "--ci"]) {
        Ok(x) => x,
        Err(e) => return fail(&e),
    };
    let p = match open_project(has(&fl, &["g", "global"]), false) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let t0 = Instant::now();
    let opts = Options { refresh: None, frozen: has(&fl, &["frozen", "ci"]), log: &log };
    match install::install_all(&p.root, &p.manifest, &opts) {
        Ok(r) => {
            print_report(&r, t0, None);
            0
        }
        Err(e) => fail(&e),
    }
}

pub fn update(args: &[String]) -> i32 {
    let (pos, fl) = match split_flags(args, &["-g", "--global"]) {
        Ok(x) => x,
        Err(e) => return fail(&e),
    };
    let p = match open_project(has(&fl, &["g", "global"]), false) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    for n in &pos {
        if !p.manifest.deps().iter().any(|(d, _)| d == n) {
            return fail(&format!("'{}' is not in the dependencies", n));
        }
    }
    let t0 = Instant::now();
    let names: HashSet<String> = pos.iter().cloned().collect();
    let opts = Options { refresh: Some(names), frozen: false, log: &log };
    match install::install_all(&p.root, &p.manifest, &opts) {
        Ok(r) => {
            print_report(&r, t0, None);
            0
        }
        Err(e) => fail(&e),
    }
}

pub fn list(args: &[String]) -> i32 {
    let (_, fl) = match split_flags(args, &["-g", "--global", "--all"]) {
        Ok(x) => x,
        Err(e) => return fail(&e),
    };
    if has(&fl, &["all"]) {
        return pm(&["ls".to_string()]);
    }
    let p = match open_project(has(&fl, &["g", "global"]), false) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let lock = Lock::load(&p.root.join(LOCKFILE));
    let name = p.manifest.str_field("name").unwrap_or_default();
    let ver = p.manifest.str_field("version").unwrap_or_default();
    println!("{} {}", ui::bold(&name), ui::dim(&format!("v{} — {}", ver, p.root.display())));
    let deps = p.manifest.deps();
    if deps.is_empty() {
        println!("{}", ui::dim("(no dependencies)"));
        return 0;
    }
    for (n, spec) in deps {
        match lock.packages.get(&n) {
            Some(e) => {
                let v = if e.version.is_empty() { String::new() } else { format!("@{}", e.version) };
                println!(
                    "├─ {}{} {} {}",
                    ui::bold(&n),
                    v,
                    ui::dim(&format!("({})", &e.commit[..e.commit.len().min(8)])),
                    ui::dim(&spec)
                );
                for (dn, _) in &e.dependencies {
                    println!("│  └─ {}", dn);
                }
            }
            None => println!("├─ {} {} {}", ui::bold(&n), ui::yellow("[not installed]"), ui::dim(&spec)),
        }
    }
    0
}

pub fn pm(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        Some("path") | Some("bin") => {
            println!("{}", pkg::store().display());
            0
        }
        Some("ls") | Some("list") => {
            let mut rows: Vec<(String, u64)> = Vec::new();
            if let Ok(rd) = std::fs::read_dir(pkg::store()) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        rows.push((e.file_name().to_string_lossy().to_string(), install::dir_size(&e.path())));
                    }
                }
            }
            rows.sort();
            println!("{} {}", ui::bold("store:"), pkg::store().display());
            for (n, s) in &rows {
                println!("  {} {}", n, ui::dim(&ui::human_size(*s)));
            }
            println!("{}", ui::dim(&format!("{} package(s)", rows.len())));
            0
        }
        Some("cache") => {
            let st = pkg::store();
            if matches!(args.get(1).map(|s| s.as_str()), Some("clean") | Some("rm")) {
                let n = std::fs::read_dir(&st).map(|r| r.flatten().count()).unwrap_or(0);
                let _ = std::fs::remove_dir_all(&st);
                println!("{} {} package(s) removed from the store", ui::yellow("-"), n);
                println!("{}", ui::dim("(use 'fogu install' to fetch them again)"));
            } else {
                println!("{} {}", st.display(), ui::dim(&ui::human_size(install::dir_size(&st))));
            }
            0
        }
        Some("hash") => match args.get(1) {
            Some(d) => match install::tree_hash(Path::new(d)) {
                Ok(h) => {
                    println!("{}", h);
                    0
                }
                Err(e) => fail(&e),
            },
            None => fail("usage: fogu pm hash <folder>"),
        },
        _ => fail("usage: fogu pm <path|ls|cache [clean]|hash <folder>>"),
    }
}

// ---------------------------------------------------------------------
// check
// ---------------------------------------------------------------------

fn collect_fg(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if !matches!(name.as_str(), ".git" | "target" | "fogu_modules" | "node_modules") && !name.starts_with('.') {
                collect_fg(&p, out);
            }
        } else if name.ends_with(".fg") {
            out.push(p);
        }
    }
}

pub fn check(args: &[String]) -> i32 {
    let mut files: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if files.is_empty() {
        collect_fg(&cwd(), &mut files);
        files.sort();
    }
    if files.is_empty() {
        println!("{}", ui::dim("no .fg files found"));
        return 0;
    }
    let builtins: std::collections::HashSet<String> = crate::native::NativeContext::new()
        .globals()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let mut bad = 0;
    for f in &files {
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                bad += 1;
                eprintln!("{} {}: {}", ui::red_err("✗"), f.display(), e);
                continue;
            }
        };
        let r = crate::runtime::Lexer::new(&src)
            .tokenize()
            .and_then(|t| crate::runtime::Parser::new(t).parse_program())
            .and_then(|p| crate::runtime::resolver::check(&p, &builtins));
        match r {
            Ok(warnings) => {
                println!("{} {}", ui::green("✓"), f.display());
                for w in warnings {
                    println!("  {} {}: {}", ui::yellow("warning:"), f.display(), w);
                }
            }
            Err(e) => {
                bad += 1;
                let e = format!("{}: {}", f.display(), e);
                eprintln!("{} {}", ui::red_err("✗"), super::diag::render(&e));
            }
        }
    }
    println!("\n{} file(s), {} with errors", files.len(), bad);
    if bad > 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------
// --watch
// ---------------------------------------------------------------------

pub(super) fn snapshot(dir: &Path) -> u64 {
    fn walk(dir: &Path, h: &mut u64) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !matches!(name.as_str(), ".git" | "target" | "fogu_modules" | "node_modules") && !name.starts_with('.') {
                    walk(&p, h);
                }
            } else if name.ends_with(".fg") || name == MANIFEST || name == LOCKFILE {
                if let Ok(m) = e.metadata() {
                    let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0);
                    for v in [t, m.len()] {
                        *h = (*h ^ v).wrapping_mul(0x100000001b3);
                    }
                    for b in p.to_string_lossy().bytes() {
                        *h = (*h ^ b as u64).wrapping_mul(0x100000001b3);
                    }
                }
            }
        }
    }
    let mut h = 0xcbf29ce484222325u64;
    walk(dir, &mut h);
    h
}

pub fn watch(orig: &[String]) -> i32 {
    let child_args: Vec<String> = orig.iter().filter(|a| *a != "--watch" && *a != "-w").cloned().collect();
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return fail(&e.to_string()),
    };
    let dir = pkg::find_root(&cwd()).unwrap_or_else(cwd);
    eprintln!("{} {}", ui::cyan("[watch]"), ui::dim(&format!("watching {}", dir.display())));
    let mut last = snapshot(&dir);
    loop {
        let mut child = match Command::new(&exe).args(&child_args).spawn() {
            Ok(c) => c,
            Err(e) => return fail(&format!("could not start the program: {}", e)),
        };
        let mut exited = false;
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = snapshot(&dir);
            if now != last {
                last = now;
                if !exited {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                eprintln!("{} {}", ui::cyan("[watch]"), "change detected — restarting");
                break;
            }
            if !exited {
                if let Ok(Some(st)) = child.try_wait() {
                    exited = true;
                    eprintln!(
                        "{} {}",
                        ui::cyan("[watch]"),
                        ui::dim(&format!("exited ({}); waiting for changes", st.code().unwrap_or(-1)))
                    );
                }
            }
        }
    }
}
