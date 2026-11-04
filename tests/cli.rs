//! Integration tests of CLI commands that work on whole projects:
//! `fogu init`, `fogu test`, `fogu build --compile`, `fogu fmt`, `fogu check` and `.env` loading.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh, empty folder for one test (removed when dropped).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("fogu-cli-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn write(&self, rel: &str, text: &str) {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fogu(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fogu"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("FOGU_HOME", dir.join(".fogu-home"))
        .output()
        .expect("could not run fogu")
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
    .replace("\r\n", "\n")
}

#[test]
fn init_creates_the_project_files() {
    let d = TempDir::new("init");
    let o = fogu(d.path(), &["init", "-y"]);
    assert!(o.status.success(), "{}", text(&o));
    for f in ["fogu.json", "main.fg", ".gitignore"] {
        assert!(d.path().join(f).is_file(), "missing {}", f);
    }
    let gi = fs::read_to_string(d.path().join(".gitignore")).unwrap();
    assert!(gi.contains("fogu_modules/") && gi.contains("dist/"));
    let run = fogu(d.path(), &["run"]);
    assert_eq!(text(&run).trim(), "Hello, Fogu!");

    // an existing .gitignore is kept
    let d2 = TempDir::new("init-keep");
    d2.write(".gitignore", "custom\n");
    assert!(fogu(d2.path(), &["init", "-y"]).status.success());
    assert_eq!(fs::read_to_string(d2.path().join(".gitignore")).unwrap(), "custom\n");
}

#[test]
fn test_command_reports_passes_failures_and_skips() {
    let d = TempDir::new("test");
    d.write("util.fg", "export fn sum(a, b) { return a + b }\n");
    d.write(
        "tests/util.test.fg",
        r#"import { test, skip, expect } from "fogu:test"
import { sum } from "../util.fg"

test("sum adds", () => {
    expect(sum(1, 2)).toBe(3)
    expect([1, 2]).not.toContain(3)
})
test("fails on purpose", () => {
    expect(sum(2, 2)).toBe(5)
})
skip("later", () => {})
"#,
    );
    let o = fogu(d.path(), &["test"]);
    let out = text(&o);
    assert!(!o.status.success(), "a failing test must fail the command:\n{}", out);
    assert!(out.contains("✓ sum adds"), "{}", out);
    assert!(out.contains("✗ fails on purpose"), "{}", out);
    assert!(out.contains("expected 4 to be 5"), "{}", out);
    assert!(out.contains("○ later (skipped)"), "{}", out);
    assert!(out.contains("1 passed, 1 failed, 1 skipped"), "{}", out);

    // -t keeps only the matching tests
    let o = fogu(d.path(), &["test", "-t", "sum"]);
    let out = text(&o);
    assert!(o.status.success(), "{}", out);
    assert!(out.contains("1 passed") && !out.contains("fails on purpose"), "{}", out);
}

#[test]
fn build_compile_makes_a_standalone_executable() {
    let d = TempDir::new("build");
    assert!(fogu(d.path(), &["init", "-y"]).status.success());
    d.write("lib/math.fg", "export fn twice(x) { return x * 2 }\n");
    d.write(
        "main.fg",
        "import { twice } from \"./lib/math.fg\"\nimport system from \"fogu:system\"\nprint(twice(21), system.args)\n",
    );
    let o = fogu(d.path(), &["build", "--compile"]);
    assert!(o.status.success(), "{}", text(&o));

    let name = d.path().file_name().unwrap().to_string_lossy().to_string();
    let exe = d
        .path()
        .join("dist")
        .join(if cfg!(windows) { format!("{}.exe", name) } else { name });
    assert!(exe.is_file(), "missing {}", exe.display());

    // it runs from another folder, without the project next to it
    let elsewhere = TempDir::new("build-run");
    let run = Command::new(&exe)
        .args(["one", "two"])
        .current_dir(elsewhere.path())
        .output()
        .unwrap();
    assert_eq!(text(&run).trim(), "42 [one, two]");
}

#[test]
fn build_compile_bundles_imported_data_files() {
    let d = TempDir::new("build-data");
    assert!(fogu(d.path(), &["init", "-y"]).status.success());
    d.write("data.json", "{ \"name\": \"bundled\", \"port\": 8080 }\n");
    d.write("lib/notes.txt", "hello from a text file\n");
    fs::write(d.path().join("logo.png"), [0x89u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0xff]).unwrap();
    // a data file imported by a module, and a re-export
    d.write("lib/info.fg", "import notes from \"./notes.txt\"\nexport default notes.trim()\nexport { port } from \"../data.json\"\n");
    d.write(
        "main.fg",
        "import data from \"./data.json\"\nimport logo from \"./logo.png\"\nimport info, { port } from \"./lib/info.fg\"\n\
         print(data.name, port, typeof(logo), len(logo), logo[9], info)\n",
    );
    let o = fogu(d.path(), &["build", "--compile"]);
    assert!(o.status.success(), "{}", text(&o));

    let name = d.path().file_name().unwrap().to_string_lossy().to_string();
    let exe = d
        .path()
        .join("dist")
        .join(if cfg!(windows) { format!("{}.exe", name) } else { name });
    // the data files travel inside the executable
    let bundle = bundle_of(&exe);
    assert!(bundle["files"]["data.json"].is_string(), "{}", bundle["files"]);
    assert!(bundle["files"]["lib/notes.txt"].is_string(), "{}", bundle["files"]);
    assert!(bundle["assets"]["logo.png"].is_string(), "{}", bundle["assets"]);

    let elsewhere = TempDir::new("build-data-run");
    let run = Command::new(&exe).current_dir(elsewhere.path()).output().unwrap();
    assert_eq!(text(&run).trim(), "bundled 8080 buffer 10 255 hello from a text file");

    // an unknown file type stops the build
    d.write("main.fg", "import doc from \"./notes.docx\"\nprint(doc)\n");
    d.write("notes.docx", "x");
    let o = fogu(d.path(), &["build", "--compile"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("unknown file type"), "{}", text(&o));
}

#[test]
fn check_reports_missing_imported_files_without_parsing_data() {
    let d = TempDir::new("check-imports");
    d.write("config.json", "{ \"port\": 1 }\n");
    d.write(
        "main.fg",
        "import cfg from \"./config.json\"\nimport gone from \"./gone.json\"\nimport m from \"./missing.fg\"\nprint(cfg, gone, m)\n",
    );
    let o = fogu(d.path(), &["check", "main.fg"]);
    let out = text(&o);
    assert!(!o.status.success(), "{}", out);
    assert!(out.contains("cannot find './gone.json'"), "{}", out);
    assert!(out.contains("cannot find './missing.fg'"), "{}", out);
    assert!(!out.contains("config.json"), "{}", out);
}

#[test]
fn fmt_formats_files_and_check_reports_them() {
    let dir = TempDir::new("fmt");
    dir.write("ugly.fg", "fn add(a,b){\nreturn a+b\n}\n\n\n\nprint( add(1,2) )   // three\n");
    dir.write("pretty.fg", "print(1)\n");
    let check = fogu(dir.path(), &["fmt", "--check"]);
    assert_eq!(check.status.code(), Some(1), "{}", text(&check));
    assert!(text(&check).contains("ugly.fg"));
    assert!(!text(&check).contains("pretty.fg"));
    let run = fogu(dir.path(), &["fmt"]);
    assert!(run.status.success(), "{}", text(&run));
    assert_eq!(
        fs::read_to_string(dir.path().join("ugly.fg")).unwrap(),
        "fn add(a, b) {\n    return a + b\n}\n\nprint(add(1, 2))   // three\n"
    );
    let again = fogu(dir.path(), &["fmt", "--check"]);
    assert!(again.status.success(), "{}", text(&again));
}

#[test]
fn dotenv_is_loaded_into_the_environment() {
    let d = TempDir::new("dotenv");
    d.write(".env", "GREETING=\"hello env\"\nPORT=8080\nBAD LINE ===\nKEEP=from-file\n");
    d.write(
        "main.fg",
        "import process from \"fogu:process\"\nprint(Fogu.env.GREETING, process.env(\"PORT\"), Fogu.env.KEEP)\n",
    );
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fogu"));
    let o = cmd.arg("main.fg").current_dir(d.path()).env("NO_COLOR", "1").env("KEEP", "from-shell").output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    // the variables already set win; a bad line is reported and skipped
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hello env 8080 from-shell");
    assert!(text(&o).contains("warning: .env"), "{}", text(&o));
    let off = Command::new(env!("CARGO_BIN_EXE_fogu"))
        .arg("main.fg")
        .current_dir(d.path())
        .env("FOGU_DOTENV", "0")
        .env_remove("KEEP")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&off.stdout).trim(), "nil nil nil");
}
/// `ssh.shell()` against a stand-in ssh client (`FOGU_SSH`) that runs the
/// remote command with the local `sh`: no server needed.
#[cfg(unix)]
#[test]
fn ssh_shell_sessions_keep_state() {
    use std::os::unix::fs::PermissionsExt;
    let d = TempDir::new("ssh-shell");
    d.write(
        "fake-ssh",
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o|-p|-i) shift 2;; -*) shift;; *) break;; esac; done\nshift\nif [ $# -eq 0 ]; then exec sh; else exec sh -c \"$*\"; fi\n",
    );
    let fake = d.path().join("fake-ssh");
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(d.path().join("work/sub")).unwrap();
    d.write(
        "main.fg",
        r#"import ssh from "fogu:ssh"
const s = ssh.connect("me@example.test")?
const sh = s.shell({ cwd: "work", env: { GREETING: "it's ok" } })?
print(sh.run("cd sub && export X=42")?.ok)
const r = sh.run("basename $(pwd); echo $X $GREETING; echo oops >&2; printf no-newline")?
print(r.stdout)
print(r.stderr.trim(), r.code)
print(sh.run("exit_code() { return 3; }; exit_code")?.code)
const [_, late] = sh.run("sleep 2", { timeout: 100ms })
print(late)
sh.close()
print(sh.alive())
const t = s.shell()?
t.send("echo ready-for-input\n")
print(t.expect("ready-for-.....")?.trim())
t.send("echo one; echo two >&2\n")
print(t.read({ timeout: 300ms }).split("\n").len() >= 2)
t.close()
"#,
    );
    let out = Command::new(env!("CARGO_BIN_EXE_fogu"))
        .arg("main.fg")
        .current_dir(d.path())
        .env("NO_COLOR", "1")
        .env("FOGU_SSH", &fake)
        .output()
        .expect("could not run fogu");
    assert_eq!(
        text(&out),
        "true\nsub\n42 it's ok\nno-newline\noops 0\n3\nerror: ssh.shell: timed out waiting for the command\nfalse\nready-for-input\ntrue\n"
    );
}

/// The JSON bundle appended to a standalone executable.
fn bundle_of(exe: &Path) -> serde_json::Value {
    let bytes = fs::read(exe).unwrap();
    let n = bytes.len();
    assert_eq!(&bytes[n - 8..], b"FOGUBND1", "no bundle in {}", exe.display());
    let len = u64::from_le_bytes(bytes[n - 16..n - 8].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[n - 16 - len..n - 16]).unwrap()
}

#[test]
fn build_compile_service_records_the_settings() {
    let d = TempDir::new("service");
    assert!(fogu(d.path(), &["init", "-y"]).status.success());
    d.write("main.fg", "import system from \"fogu:system\"\nprint(\"args:\", system.args)\n");
    // fogu.json "service" (the flags below win over it)
    let manifest = d.path().join("fogu.json");
    let mut m: serde_json::Value = serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    m["service"] = serde_json::json!({ "startType": "delayed", "args": ["--port", "8080"], "stopTimeout": 7 });
    fs::write(&manifest, serde_json::to_string_pretty(&m).unwrap()).unwrap();

    // a stand-in Windows runtime: only its PE header is checked (x86-64)
    let mut pe = vec![0u8; 0x200];
    pe[..2].copy_from_slice(b"MZ");
    pe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    pe[0x40..0x44].copy_from_slice(b"PE\0\0");
    pe[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes());
    fs::write(d.path().join("fake-runtime.exe"), &pe).unwrap();
    let o = fogu(
        d.path(),
        &[
            "build", "--compile", "--target", "windows-x64", "--runtime", "fake-runtime.exe", "--service", "--service-name", "myapp",
            "--display-name", "My App", "--description", "Serves things",
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("service:"), "{}", text(&o));
    let name = d.path().file_name().unwrap().to_string_lossy().to_string();
    let exe = d.path().join("dist").join(format!("{}-windows-x64.exe", name));
    let b = bundle_of(&exe);
    assert_eq!(
        b["service"],
        serde_json::json!({
            "name": "myapp", "displayName": "My App", "description": "Serves things", "startType": "delayed",
            "args": ["--port", "8080"], "stopTimeout": 7
        })
    );
    // the runtime part is untouched
    assert_eq!(&fs::read(&exe).unwrap()[..pe.len()], &pe[..]);

    // bad settings are refused
    let o = fogu(d.path(), &["build", "--compile", "--target", "windows-x64", "--runtime", "fake-runtime.exe", "--service-name", "bad name"]);
    assert!(!o.status.success() && text(&o).contains("invalid service name"), "{}", text(&o));

    // Linux: the executable gets a systemd unit and the service commands
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        let o = fogu(d.path(), &["build", "--compile", "--target", "linux-x64", "--runtime", env!("CARGO_BIN_EXE_fogu"), "--service"]);
        assert!(o.status.success(), "{}", text(&o));
        let exe = d.path().join("dist").join(format!("{}-linux-x64", name));
        assert_eq!(bundle_of(&exe)["service"]["name"], serde_json::json!(name));
        let unit = fs::read_to_string(d.path().join("dist").join(format!("{}.service", name))).unwrap();
        assert!(unit.contains(&format!("ExecStart=/opt/{0}/{0}-linux-x64 --port 8080\n", name)), "{}", unit);
        assert!(unit.contains("TimeoutStopSec=7\n"), "{}", unit);
        let run = |args: &[&str]| Command::new(&exe).args(args).current_dir(d.path()).env("NO_COLOR", "1").output().unwrap();
        // a console run is the program itself; `--` passes command-like arguments
        assert_eq!(text(&run(&["x"])).trim(), "args: [x]");
        assert_eq!(text(&run(&["--", "install"])).trim(), "args: [install]");
        let help = run(&["service-help"]);
        assert!(help.status.success() && text(&help).contains("install [--start auto|manual|delayed]"), "{}", text(&help));
        let bad = run(&["stop", "now"]);
        assert!(!bad.status.success() && text(&bad).contains("takes no arguments"), "{}", text(&bad));
    }
}

// ---------- fogu check ----------

#[test]
fn check_walks_folders_and_skips_hidden_and_build_ones() {
    let d = TempDir::new("check-dir");
    d.write("src/a.fg", "print(1)\n");
    d.write("src/sub/b.fg", "print(2)\n");
    // never searched: these would fail
    for skipped in [".hidden/x.fg", "node_modules/x.fg", "target/x.fg", "dist/x.fg", ".fogu/x.fg"] {
        d.write(skipped, "print(undefined_name)\n");
    }
    let o = fogu(d.path(), &["check", "src"]);
    let out = text(&o);
    assert!(o.status.success(), "{}", out);
    assert!(out.contains("2 file(s), 0 with errors"), "{}", out);
    // without arguments: every .fg under the current folder (the same two)
    let o = fogu(d.path(), &["check"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("2 file(s)"), "{}", text(&o));
    // a missing path is an error
    let o = fogu(d.path(), &["check", "nope"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("nope: file or folder not found"), "{}", text(&o));
}

#[test]
fn check_help_is_not_a_file() {
    let d = TempDir::new("check-help");
    let o = fogu(d.path(), &["check", "--help"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("Usage: fogu check"), "{}", text(&o));
    let o = fogu(d.path(), &["check", "--bogus"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
}

#[test]
fn check_reports_every_error_as_json() {
    let d = TempDir::new("check-json");
    d.write(
        "main.fg",
        "const unused = 1\nprint(1 + \"a\")\nconst n: int = \"x\"\nprint(len(1, 2))\nprint(n)\n",
    );
    let o = fogu(d.path(), &["check", "--json", "main.fg"]);
    assert!(!o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).expect("JSON on stdout");
    assert_eq!(v["errors"], 3, "{}", v);
    assert_eq!(v["warnings"], 1, "{}", v);
    assert_eq!(v["ok"], false);
    let diags = v["diagnostics"].as_array().unwrap();
    let first = &diags[0];
    assert_eq!(first["file"], "main.fg");
    assert_eq!(first["line"], 2);
    assert_eq!(first["severity"], "error");
    assert_eq!(first["code"], "E0210");
    assert_eq!(first["message"], "operator not applicable between 'int' and 'str'");
    assert!(first["column"].as_u64().is_some(), "{}", first);
    assert_eq!(diags[1]["code"], "E0207");
    assert_eq!(diags[2]["code"], "E0209");
    let w = &diags[3];
    assert_eq!(
        (w["severity"].as_str(), w["code"].as_str(), w["line"].as_u64()),
        (Some("warning"), Some("W0001"), Some(1))
    );
    assert_eq!(w["column"], 7, "{}", w);
}

#[test]
fn check_strict_fails_on_warnings() {
    let d = TempDir::new("check-strict");
    d.write("main.fg", "import json from \"fogu:json\"\nfn f() {\n    return 1\n    print(2)\n}\nprint(f())\n");
    let o = fogu(d.path(), &["check", "main.fg"]);
    let out = text(&o);
    assert!(o.status.success(), "{}", out);
    assert!(out.contains("warning[W0002]: unused import 'json'"), "{}", out);
    assert!(out.contains("warning[W0003]: unreachable code after 'return'"), "{}", out);
    for flag in ["--strict", "--deny-warnings"] {
        let o = fogu(d.path(), &["check", flag, "main.fg"]);
        assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    }
    // --quiet: nothing for a clean file
    d.write("ok.fg", "print(1)\n");
    let o = fogu(d.path(), &["check", "-q", "ok.fg"]);
    assert!(o.status.success());
    assert_eq!(text(&o).trim(), "");
    // warnings are not printed when running
    let o = fogu(d.path(), &["main.fg"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(text(&o).trim(), "1");
}
