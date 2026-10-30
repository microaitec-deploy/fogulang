//! `fogu test` — runs the tests of `*.test.fg` files (spec §63).
//!
//! ```text
//! fogu test                    every *.test.fg under the current folder
//! fogu test tests/ math.test.fg  given folders and files
//! fogu test -t parse           only tests whose name contains "parse"
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{diag, ui};
use crate::runtime::{run_test_file, TestStatus};

const SKIP_DIRS: &[&str] = &[".git", "target", "dist", "fogu_modules", "node_modules"];

pub fn test(args: &[String]) -> i32 {
    let mut filter: Option<String> = None;
    let mut targets: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-t" | "--filter" => match it.next() {
                Some(f) => filter = Some(f.clone()),
                None => {
                    ui::error("-t expects a text to filter the test names");
                    return 2;
                }
            },
            s if s.starts_with('-') => {
                ui::error(&format!("unknown option: {}", s));
                return 2;
            }
            s => targets.push(PathBuf::from(s)),
        }
    }
    if targets.is_empty() {
        targets.push(PathBuf::from("."));
    }

    let mut files = Vec::new();
    for t in &targets {
        if t.is_dir() {
            collect(t, &mut files);
        } else if t.is_file() {
            files.push(t.clone());
        } else {
            ui::error(&format!("not found: {}", t.display()));
            return 1;
        }
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        println!("{}", ui::dim("no *.test.fg files found"));
        return 0;
    }

    let started = Instant::now();
    let (mut passed, mut failed, mut skipped, mut broken) = (0, 0, 0, 0);
    for f in &files {
        println!("{}", ui::bold(&f.display().to_string()));
        match run_test_file(f, filter.as_deref()) {
            Err(e) => {
                broken += 1;
                println!("  {} {}", ui::red_err("✗"), ui::dim("the file could not be loaded"));
                eprintln!("{}", indent(&diag::render(&e)));
            }
            Ok(results) => {
                for r in results {
                    match r.status {
                        TestStatus::Passed => {
                            passed += 1;
                            println!("  {} {} {}", ui::green("✓"), r.name, ui::dim(&format!("({}ms)", r.millis)));
                        }
                        TestStatus::Skipped => {
                            skipped += 1;
                            println!("  {} {} {}", ui::yellow("○"), r.name, ui::dim("(skipped)"));
                        }
                        TestStatus::Failed(e) => {
                            failed += 1;
                            println!("  {} {}", ui::red_err("✗"), r.name);
                            eprintln!("{}", indent(&diag::render(&e)));
                        }
                    }
                }
            }
        }
    }

    let mut summary = vec![ui::green(&format!("{} passed", passed))];
    if failed > 0 {
        summary.push(ui::red_err(&format!("{} failed", failed)));
    }
    if skipped > 0 {
        summary.push(ui::yellow(&format!("{} skipped", skipped)));
    }
    if broken > 0 {
        summary.push(ui::red_err(&format!("{} file(s) failed to load", broken)));
    }
    println!(
        "\n{} {}",
        summary.join(", "),
        ui::dim(&format!("— {} file(s) [{}ms]", files.len(), started.elapsed().as_millis()))
    );
    if failed + broken > 0 { 1 } else { 0 }
}

fn indent(text: &str) -> String {
    text.lines().map(|l| format!("    {}", l)).collect::<Vec<_>>().join("\n")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                collect(&p, out);
            }
        } else if name.ends_with(".test.fg") {
            // `./tests/a.test.fg` → `tests/a.test.fg`
            out.push(p.components().filter(|c| *c != std::path::Component::CurDir).collect());
        }
    }
}
