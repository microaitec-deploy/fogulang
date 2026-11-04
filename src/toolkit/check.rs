//! `fogu check`: the static checks of the resolver on .fg files, without
//! running them — every error and warning of each file, as text or JSON.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value as Json};

use super::{diag, ui};

const HELP: &str = "Usage: fogu check [options] [files or folders...]

Checks .fg files without running them: syntax, undefined names, use before
declaration, unknown types, '?' misuse, non-exhaustive match, and type errors
the code would hit when it runs (operators on incompatible types, values that
cannot match an annotation, wrong number of arguments). Folders are searched
recursively (skipping hidden folders, node_modules, target, dist and
fogu_modules); without paths, every .fg file under the current folder.

Warnings: unused variables and imports, unreachable code, shadowed variables
(prefix a name with '_' to silence the warnings about it).

Options:
      --json             machine-readable output (file, line, column,
                         severity, code, message, help of each diagnostic)
      --strict           warnings also make the exit code 1
      --deny-warnings    same as --strict
  -q, --quiet            only print the problems
  -w, --watch            check again whenever a .fg file changes
  -h, --help             show this help

Exit code: 0 when there are no errors (and no warnings with --strict), 1 otherwise.";

#[derive(Default)]
struct Opts {
    json: bool,
    strict: bool,
    quiet: bool,
    watch: bool,
    paths: Vec<String>,
}

/// Folders never searched: hidden ones (`.git`, `.fogu`…) and build/dependency output.
fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "dist" | "fogu_modules")
}

fn collect_fg(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut found = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if !skip_dir(&name) {
                collect_fg(&p, &mut found);
            }
        } else if name.ends_with(".fg") {
            found.push(p);
        }
    }
    found.sort();
    out.extend(found);
}

/// The files to check, or an error for each path that does not exist.
fn files_of(paths: &[String]) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut missing = Vec::new();
    if paths.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut found = Vec::new();
        collect_fg(&cwd, &mut found);
        // shown relative to the current folder
        files.extend(found.into_iter().map(|p| p.strip_prefix(&cwd).map(Path::to_path_buf).unwrap_or(p)));
    }
    for p in paths {
        let path = PathBuf::from(p);
        if path.is_dir() {
            collect_fg(&path, &mut files);
        } else if path.is_file() {
            files.push(path);
        } else {
            missing.push(p.clone());
        }
    }
    let mut seen = HashSet::new();
    files.retain(|f| seen.insert(f.clone()));
    (files, missing)
}

/// Every error and warning of one file (each prefixed with the file name).
fn check_file(f: &Path, builtins: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let name = f.display().to_string();
    let src = match std::fs::read_to_string(f) {
        Ok(s) => s,
        Err(e) => return (vec![format!("{}: {}", name, e)], Vec::new()),
    };
    let parsed = crate::runtime::Lexer::new(&src)
        .tokenize()
        .and_then(|t| crate::runtime::Parser::new(t).parse_program());
    let program = match parsed {
        Ok(p) => p,
        Err(e) => return (vec![format!("{}: {}", name, e)], Vec::new()),
    };
    let r = crate::runtime::resolver::analyze(&program, builtins);
    let mut warnings: Vec<(u32, String)> = r
        .warnings
        .into_iter()
        .map(|w| (diag::parse(&w).line.unwrap_or(0), format!("{}: {}", name, w)))
        .collect();
    warnings.sort_by_key(|(l, _)| *l);
    let errors = r.errors.into_iter().map(|e| format!("{}: {}", name, e)).collect();
    (errors, warnings.into_iter().map(|(_, w)| w).collect())
}

fn finding_json(text: &str, warning: bool) -> Json {
    let f = diag::finding(text, warning);
    json!({
        "file": f.file,
        "line": f.line,
        "column": f.col,
        "severity": f.severity,
        "code": f.code,
        "message": f.msg,
        "help": f.help,
    })
}

/// Checks once; returns the exit code.
fn run(o: &Opts) -> i32 {
    let (files, missing) = files_of(&o.paths);
    let builtins: HashSet<String> = crate::native::NativeContext::new()
        .globals()
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    let (mut n_err, mut n_warn, mut bad) = (0usize, 0usize, 0usize);
    let mut diags: Vec<Json> = Vec::new();
    for m in &missing {
        n_err += 1;
        bad += 1;
        if o.json {
            diags.push(json!({
                "file": m, "line": null, "column": null, "severity": "error", "code": null,
                "message": "file or folder not found", "help": [],
            }));
        } else {
            eprintln!("{} {}: file or folder not found", ui::red_err("✗"), m);
        }
    }
    for f in &files {
        let (errors, warnings) = check_file(f, &builtins);
        n_err += errors.len();
        n_warn += warnings.len();
        if !errors.is_empty() {
            bad += 1;
        }
        if o.json {
            diags.extend(errors.iter().map(|e| finding_json(e, false)));
            diags.extend(warnings.iter().map(|w| finding_json(w, true)));
            continue;
        }
        if errors.is_empty() && (!o.quiet || !warnings.is_empty()) {
            println!("{} {}", ui::green("✓"), f.display());
        }
        for e in &errors {
            eprintln!("{} {}", ui::red_err("✗"), diag::render(e));
        }
        for w in &warnings {
            println!("  {}", diag::render_warning(w).replace('\n', "\n  "));
        }
    }

    let failed = n_err > 0 || (o.strict && n_warn > 0);
    if o.json {
        let out = json!({
            "files": files.len(),
            "errors": n_err,
            "warnings": n_warn,
            "ok": !failed,
            "diagnostics": diags,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else if files.is_empty() && missing.is_empty() {
        println!("{}", ui::dim("no .fg files found"));
    } else if !o.quiet || n_err > 0 || n_warn > 0 {
        println!(
            "\n{} file(s), {} with errors, {} error(s), {} warning(s)",
            files.len() + missing.len(),
            bad,
            n_err,
            n_warn
        );
    }
    i32::from(failed)
}

/// A hash of the .fg files under the checked paths (for `--watch`).
fn snapshot(paths: &[String]) -> u64 {
    let (files, _) = files_of(paths);
    let mut h = 0xcbf29ce484222325u64;
    for f in files {
        let meta = std::fs::metadata(&f).ok();
        let t = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        for v in [t, meta.map(|m| m.len()).unwrap_or(0)] {
            h = (h ^ v).wrapping_mul(0x100000001b3);
        }
        for b in f.to_string_lossy().bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

pub fn check(args: &[String]) -> i32 {
    let mut o = Opts::default();
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{}", HELP);
                return 0;
            }
            "--json" => o.json = true,
            "--strict" | "--deny-warnings" => o.strict = true,
            "-q" | "--quiet" => o.quiet = true,
            "-w" | "--watch" => o.watch = true,
            s if s.starts_with('-') && s != "-" => {
                ui::error(&format!("unknown option for 'fogu check': {}\n  see 'fogu check --help'", s));
                return 2;
            }
            _ => o.paths.push(a.clone()),
        }
    }
    if !o.watch {
        return run(&o);
    }
    loop {
        run(&o);
        eprintln!("{}", ui::dim("[watch] waiting for changes (Ctrl+C to stop)"));
        let last = snapshot(&o.paths);
        while snapshot(&o.paths) == last {
            std::thread::sleep(Duration::from_millis(300));
        }
        eprintln!("\n{}", ui::cyan("[watch] change detected — checking again"));
    }
}
