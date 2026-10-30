//! Conformance suite (spec §70).
//!
//! Each `tests/spec/<name>.fg` runs twice — with the JIT and with
//! `--no-jit` — and both runs must produce:
//!
//! * `<name>.out` — the expected stdout (required);
//! * `<name>.err` — if present, the program must fail and stderr must
//!   contain every non-empty line of this file.
//!
//! Files starting with `_` are helper modules (not tests).
//! Data files with `.tmp.` in their name are deleted after each test.
//!
//! `FOGU_BLESS=1 cargo test` (re)writes the `.out` files from the output
//! with `--no-jit` (the interpreter is the semantic reference).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn spec_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("spec")
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn run(file: &Path, jit: bool) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fogu"));
    if !jit {
        cmd.arg("--no-jit");
    }
    let out = cmd
        .arg(file.file_name().unwrap())
        .current_dir(file.parent().unwrap())
        .env("NO_COLOR", "1")
        .env_remove("FOGU_JIT")
        // low thresholds: force the JIT to compile even in small tests
        .env("FOGU_JIT_CALLS", "1")
        .env("FOGU_JIT_LOOPS", "2")
        .output()
        .expect("could not run the fogu binary");
    Run {
        ok: out.status.success(),
        stdout: normalize(&String::from_utf8_lossy(&out.stdout)),
        stderr: normalize(&String::from_utf8_lossy(&out.stderr)),
    }
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Deletes the `*.tmp.*` files and folders created by the tests (kv, sql, fs…).
fn remove_tmp_files(dir: &Path) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().contains(".tmp.") {
            let p = e.path();
            let _ = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
        }
    }
}

fn check_one(file: &Path, bless: bool) -> Vec<String> {
    let mut errors = Vec::new();
    let name = file.file_name().unwrap().to_string_lossy().to_string();
    let out_path = file.with_extension("out");
    let err_path = file.with_extension("err");
    let expected_err = fs::read_to_string(&err_path).ok().map(|s| normalize(&s));

    let interp = run(file, false);
    remove_tmp_files(file.parent().unwrap());
    if bless {
        fs::write(&out_path, &interp.stdout).unwrap();
    }
    let expected_out = match fs::read_to_string(&out_path) {
        Ok(s) => normalize(&s),
        Err(_) => {
            errors.push(format!("{}: missing {}", name, out_path.display()));
            return errors;
        }
    };

    let jit = run(file, true);
    remove_tmp_files(file.parent().unwrap());
    for (label, r) in [("interpreter", &interp), ("jit", &jit)] {
        if r.stdout != expected_out {
            errors.push(format!(
                "{} [{}]: different stdout\n--- expected\n{}--- got\n{}--- stderr\n{}",
                name, label, expected_out, r.stdout, r.stderr
            ));
        }
        match &expected_err {
            None if !r.ok => errors.push(format!(
                "{} [{}]: unexpected error:\n{}",
                name, label, r.stderr
            )),
            Some(_) if r.ok => errors.push(format!(
                "{} [{}]: should have failed, but succeeded",
                name, label
            )),
            Some(exp) => {
                for line in exp.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    if !r.stderr.contains(line) {
                        errors.push(format!(
                            "{} [{}]: stderr does not contain '{}'\n--- stderr\n{}",
                            name, label, line, r.stderr
                        ));
                    }
                }
            }
            None => {}
        }
    }
    errors
}

#[test]
fn spec_tests() {
    let bless = std::env::var("FOGU_BLESS").is_ok_and(|v| v == "1");
    let mut files: Vec<PathBuf> = fs::read_dir(spec_dir())
        .expect("tests/spec folder is missing")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "fg"))
        // `_name.fg` are helper modules imported by the tests
        .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no tests in tests/spec");

    let mut failures = Vec::new();
    for f in &files {
        failures.extend(check_one(f, bless));
    }
    if !failures.is_empty() {
        panic!(
            "{} of {} tests failed:\n\n{}",
            failures.len(),
            files.len(),
            failures.join("\n\n")
        );
    }
}
