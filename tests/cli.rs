//! Integration tests of CLI commands that work on whole projects:
//! `fogu init`, `fogu test`, `fogu build --compile`, `fogu build --lib`, `fogu fmt`, `fogu check`
//! and `.env` loading.

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
fn test_command_runs_hooks_groups_and_each() {
    let d = TempDir::new("test-hooks");
    d.write(
        "life.test.fg",
        r#"import { test, describe, expect, beforeAll, afterAll, beforeEach, afterEach } from "fogu:test"
import fs from "fogu:fs"
const log = []
beforeAll(() => log.push("all:before"))
afterAll(() => {
    log.push("all:after")
    fs.writeText("order.txt", log.join(" "))?
})
beforeEach(() => log.push("each:before"))
afterEach(() => log.push("each:after"))
test("top", () => log.push("top"))
describe("db", () => {
    beforeAll(() => log.push("db:before"))
    afterAll(() => log.push("db:after"))
    beforeEach(() => log.push("db:each"))
    afterEach(() => log.push("db:each:after"))
    test("fails", () => {
        log.push("fails")
        expect(1).toBe(2)
    })
    describe("nested", () => { test("deep", () => log.push("deep")) })
})
test.each([[2, 3, 5], [1, 1, 2]])("add(%d, %d) = %d", (a, b, c) => expect(a + b).toBe(c))
test.each([{ s: "7", n: 7 }])("int($s)", (row) => expect(int(row.s)).toBe(row.n))
test.todo("later")
"#,
    );
    d.write(
        "setup.test.fg",
        r#"import { test, describe, expect, beforeAll, afterAll } from "fogu:test"
describe("broken", () => {
    beforeAll(() => { return nil, error("no database") })
    afterAll(() => print("cleaned up"))
    test("a", () => expect(1).toBe(1))
})
"#,
    );
    // `only` keeps only those tests of its file
    d.write(
        "only.test.fg",
        r#"import { test, expect } from "fogu:test"
test.only("chosen", () => expect(1).toBe(1))
test("not chosen", () => expect(1).toBe(2))
"#,
    );
    let o = fogu(d.path(), &["test"]);
    let out = text(&o);
    assert!(!o.status.success(), "{}", out);
    assert!(out.contains("✓ top") && out.contains("✗ db > fails") && out.contains("✓ db > nested > deep"), "{}", out);
    assert!(out.contains("✓ add(2, 3) = 5") && out.contains("✓ add(1, 1) = 2") && out.contains("✓ int(7)"), "{}", out);
    assert!(out.contains("○ later (todo)"), "{}", out);
    // outer beforeEach first, inner afterEach first; cleanup after a failure; afterAll last
    let order = fs::read_to_string(d.path().join("order.txt")).unwrap();
    assert_eq!(
        order,
        "all:before each:before top each:after db:before each:before db:each fails db:each:after each:after \
         each:before db:each deep db:each:after each:after db:after each:before each:after each:before each:after \
         each:before each:after all:after"
    );
    // a failed beforeAll fails the tests of its group and still runs afterAll; only: only that test
    assert!(out.contains("✗ broken > a") && out.contains("beforeAll failed: returned an error: no database"), "{}", out);
    assert!(out.contains("cleaned up") && out.contains("✓ chosen") && !out.contains("not chosen"), "{}", out);
}

#[test]
fn test_command_fails_tests_that_exit_or_fail_in_a_task() {
    let d = TempDir::new("test-exit");
    d.write("a.test.fg", "import { test, expect } from \"fogu:test\"\ntest(\"fails\", () => { expect(1).toBe(2) })\n");
    d.write("b.test.fg", "import { test, expect } from \"fogu:test\"\ntest(\"exits\", () => { Fogu.exit(0) })\n");
    d.write(
        "c.test.fg",
        "import { test, expect } from \"fogu:test\"\nimport time from \"fogu:time\"\ntest(\"never skipped\", () => { expect(1).toBe(1) })\ntest(\"task fails\", () => {\n    fg fn() { expect(1).toBe(2) }()\n    time.delay(20ms)\n})\n",
    );
    let o = fogu(d.path(), &["test"]);
    let out = text(&o);
    // Fogu.exit(0) in a test used to end the run with code 0 and skip the other files
    assert_eq!(o.status.code(), Some(1), "{}", out);
    assert!(out.contains("✗ exits"), "{}", out);
    assert!(out.contains("exit(0) called during a test"), "{}", out);
    assert!(out.contains("✓ never skipped"), "{}", out);
    // a failed expect inside an `fg` task fails the test that started it
    assert!(out.contains("✗ task fails"), "{}", out);
    assert!(out.contains("1 passed, 3 failed"), "{}", out);
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
fn check_reports_unknown_modules_packages_and_names() {
    let d = TempDir::new("check-names");
    d.write("lib.fg", "export fn f() { return 1 }\nexport const K = 2\nfn _hidden() {}\n");
    d.write(
        "main.fg",
        "import x from \"fogu:jsno\"\nimport y from \"no-such-package\"\nimport { f, K, g } from \"./lib.fg\"\nimport { parse, nope } from \"fogu:json\"\nimport { _hidden } from \"./lib.fg\"\nprint(x, y, f, K, g, parse, nope, _hidden)\n",
    );
    let o = fogu(d.path(), &["check", "main.fg"]);
    let out = text(&o);
    assert!(!o.status.success(), "{}", out);
    assert!(out.contains("unknown built-in module: 'fogu:jsno'"), "{}", out);
    assert!(out.contains("cannot find the package 'no-such-package'"), "{}", out);
    assert!(out.contains("'g' is not exported by './lib.fg'"), "{}", out);
    assert!(out.contains("'nope' is not exported by 'fogu:json'"), "{}", out);
    assert!(out.contains("'_hidden' is private"), "{}", out);
    assert!(!out.contains("'f' is not") && !out.contains("'K' is not") && !out.contains("'parse' is not"), "{}", out);
}

#[test]
fn check_reports_redeclarations_and_assignments_to_constants() {
    let d = TempDir::new("check-decl");
    d.write(
        "main.fg",
        "fn never() {\n    let a = 1\n    let a = 2\n    const c = 1\n    c = 2\n    return a\n}\nfor i in 0..2 { i = 5 }\nlet ok = 1\nok = 2\nfn p(x) { x = 3\n    return x }\nfn q() {\n    const z = 5\n    return z()\n}\nprint(\"loaded\", ok)\n",
    );
    let o = fogu(d.path(), &["check", "main.fg"]);
    let out = text(&o);
    assert!(!o.status.success(), "{}", out);
    assert!(out.contains("'a' is already declared in this scope"), "{}", out);
    assert!(out.contains("cannot assign to 'c' because it is a const binding"), "{}", out);
    assert!(out.contains("cannot assign to 'i' because it is a const binding"), "{}", out);
    assert!(out.contains("value of type 'int' is not callable"), "{}", out);
    assert!(!out.contains("'ok'") && !out.contains("'x'"), "{}", out);
}

#[test]
fn test_command_fails_a_test_whose_fg_task_failed() {
    let d = TempDir::new("test-task");
    d.write(
        "t.test.fg",
        r#"import { test, expect } from "fogu:test"
test("task fails", () => {
    const done = chan(1)
    fg fn() {
        const n = 0
        done <- 1 / n
    }()
    Fogu.sleep(5ms)
})
test("fine", () => { expect(1).toBe(1) })
"#,
    );
    let o = fogu(d.path(), &["test"]);
    let out = text(&o);
    assert!(!o.status.success(), "{}", out);
    assert!(out.contains("✗ task fails"), "{}", out);
    assert!(out.contains("division by zero"), "{}", out);
    assert!(out.contains("t.test.fg:6"), "{}", out);
    assert!(out.contains("✓ fine"), "{}", out);
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

/// `fogu build --lib`: the program appended to the embeddable runtime is a
/// shared library a host loads; `fogu_load` runs it and `fogu_call` calls its
/// functions. Needs `libfogu_rt` next to the fogu binary (`cargo build
/// --release -p fogu-ffi`, or `--workspace`); without it the test only checks
/// the error message.
#[cfg(unix)]
#[test]
fn build_lib_makes_a_loadable_shared_library() {
    use std::ffi::{c_char, c_void, CStr, CString};

    let d = TempDir::new("buildlib");
    d.write("fogu.json", "{ \"name\": \"mathlib\", \"version\": \"1.0.0\", \"entry\": \"main.fg\" }\n");
    d.write("util.fg", "export fn double(n) { return n * 2 }\n");
    d.write(
        "main.fg",
        "import { double } from \"./util.fg\"\nlet calls = 0\nfn add(a, b) {\n    calls += 1\n    return a + b\n}\nfn greet(name) { return `Hello, ${name}! ${double(21)}` }\nfn fail(msg) { return nil, error(msg) }\nfn _hidden() { return 0 }\n",
    );

    let bin_dir = Path::new(env!("CARGO_BIN_EXE_fogu")).parent().unwrap().to_path_buf();
    let runtime = bin_dir.join(if cfg!(target_os = "macos") { "libfogu_rt.dylib" } else { "libfogu_rt.so" });
    if !runtime.is_file() {
        // no runtime library: the command must say what is missing
        let o = Command::new(env!("CARGO_BIN_EXE_fogu"))
            .args(["build", "--lib"])
            .current_dir(d.path())
            .env("NO_COLOR", "1")
            .env("FOGU_HOME", d.path().join(".fogu-home"))
            .env_remove("FOGU_SERVER")
            .env_remove("FOGU_RUNTIMES")
            .output()
            .unwrap();
        let out = text(&o);
        assert!(!o.status.success() && out.contains("no embeddable runtime"), "{}", out);
        eprintln!("build_lib: {} not built, the library was not loaded", runtime.display());
        return;
    }

    let o = fogu(d.path(), &["build", "--lib", "--runtime", &runtime.to_string_lossy()]);
    let out = text(&o);
    assert!(o.status.success(), "{}", out);
    assert!(out.contains("functions: add, greet, fail"), "{}", out);
    let lib = d.path().join("dist").join(if cfg!(target_os = "macos") { "libmathlib.dylib" } else { "libmathlib.so" });
    assert!(lib.is_file(), "missing {}", lib.display());
    let header = fs::read_to_string(d.path().join("dist").join("mathlib.h")).unwrap();
    assert!(header.contains("add(a, b)") && header.contains("fogu_load") && !header.contains("_hidden"), "{}", header);

    // SEC02: with `export fn` in the program, only those are listed as its functions
    let e = TempDir::new("buildlib-exports");
    e.write("fogu.json", "{ \"name\": \"explib\", \"version\": \"1.0.0\", \"entry\": \"main.fg\" }\n");
    e.write("main.fg", "fn helper(x) { return x + 1 }\nexport fn api(x) { return helper(x) }\n");
    let o = fogu(e.path(), &["build", "--lib", "--runtime", &runtime.to_string_lossy()]);
    let out = text(&o);
    assert!(o.status.success() && out.contains("functions: api") && !out.contains("helper"), "{}", out);
    let header = fs::read_to_string(e.path().join("dist").join("explib.h")).unwrap();
    assert!(header.contains("api(x)") && !header.contains("helper"), "{}", header);

    // load it as a host would (from another folder: nothing of the project is needed)
    type Load = unsafe extern "C" fn(*mut *mut c_void) -> *mut c_char;
    type Call = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> *mut c_char;
    type FreeStr = unsafe extern "C" fn(*mut c_char);
    type Free = unsafe extern "C" fn(*mut c_void);
    let elsewhere = TempDir::new("buildlib-run");
    std::env::set_current_dir(elsewhere.path()).unwrap();
    unsafe {
        let path = CString::new(lib.to_string_lossy().as_bytes()).unwrap();
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
        assert!(!handle.is_null(), "dlopen failed");
        let sym = |name: &str| {
            let n = CString::new(name).unwrap();
            let p = libc::dlsym(handle, n.as_ptr());
            assert!(!p.is_null(), "missing symbol {}", name);
            p
        };
        let load: Load = std::mem::transmute(sym("fogu_load"));
        let call: Call = std::mem::transmute(sym("fogu_call"));
        let free_str: FreeStr = std::mem::transmute(sym("fogu_free_string"));
        let free: Free = std::mem::transmute(sym("fogu_free"));
        let take = |p: *mut c_char| {
            let s = CStr::from_ptr(p).to_string_lossy().to_string();
            free_str(p);
            s
        };

        let mut rt: *mut c_void = std::ptr::null_mut();
        assert_eq!(take(load(&mut rt)), "{\"ok\":true,\"value\":null}");
        assert!(!rt.is_null());
        let ask = |name: &str, args: &str| {
            let (n, a) = (CString::new(name).unwrap(), CString::new(args).unwrap());
            take(call(rt, n.as_ptr(), a.as_ptr()))
        };
        assert_eq!(ask("add", "[2, 40]"), "{\"ok\":true,\"value\":42}");
        assert_eq!(ask("greet", "[\"host\"]"), "{\"ok\":true,\"value\":\"Hello, host! 42\"}");
        // `return nil, error(msg)` is the error of the call
        assert_eq!(ask("fail", "[\"boom\"]"), "{\"ok\":false,\"error\":\"boom\"}");
        assert!(ask("nope", "[]").contains("\"ok\":false"));
        free(rt);
    }
}

// ---- 0.8.4: the findings of the third test round ----

#[test]
fn exit_in_a_task_ends_the_process_with_its_code() {
    // N01: `Fogu.exit()` in a task while the program waits for a signal crashed on
    // Windows (0xC0000005) after the output was written
    let dir = TempDir::new("exit-task");
    dir.write("a.fg", "fg fn() { Fogu.sleep(100ms); print(\"task exits\"); Fogu.exit(0) }()\nFogu.signal.wait()\n");
    let o = fogu(dir.path(), &["a.fg"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "task exits");
    dir.write("b.fg", "fg fn() { Fogu.sleep(50ms); Fogu.exit(7) }()\nFogu.signal.wait()\n");
    assert_eq!(fogu(dir.path(), &["b.fg"]).status.code(), Some(7));
    // with a server and a request still running: aborted on Linux (exit code 134,
    // "panic in a destructor during cleanup") before 0.8.4
    dir.write(
        "c.fg",
        "import web from \"fogu:web\"\nimport http from \"fogu:http\"\nconst app = web.app()\napp.get(\"/slow\", (ctx) => { Fogu.sleep(2s); return \"late\" })\napp.get(\"/a\", (ctx) => \"a\")\napp.listen(18561, { quiet: true })?\nfg fn() { http.get(\"http://127.0.0.1:18561/slow\") }()\nfg fn() {\n    print(http.get(\"http://127.0.0.1:18561/a\")?.body)\n    Fogu.sleep(200ms)\n    print(\"bye\")\n    Fogu.exit(3)\n}()\nFogu.signal.wait()\n",
    );
    let o = fogu(dir.path(), &["c.fg"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(!text(&o).contains("panicked"), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n"), "a\nbye\n");
}

#[test]
fn a_task_that_ends_with_an_error_reports_it() {
    // §23: `f()?` in a task ended it without a word
    let dir = TempDir::new("task-error");
    dir.write(
        "a.fg",
        "fn f() { return nil, error(\"oops\") }\nfg fn() {\n    print(\"before\")\n    f()?\n    print(\"after\")\n}()\nfg fn() { return 1, nil }()\nFogu.sleep(200ms)\nprint(\"main done\")\n",
    );
    let o = fogu(dir.path(), &["a.fg"]);
    let all = text(&o);
    assert_eq!(String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n"), "before\nmain done\n", "{}", all);
    assert!(all.contains("unhandled error: oops"), "{}", all);
    assert!(all.contains("in the fg task started at a.fg:2"), "{}", all);
    // only the task that failed is reported
    assert_eq!(all.matches("unhandled error").count(), 1, "{}", all);
}

#[test]
fn sql_errors_do_not_show_the_password() {
    // SEC03
    let dir = TempDir::new("sql-redact");
    dir.write(
        "a.fg",
        "import sql from \"fogu:sql\"\nconst [db, err] = sql.open(\"postgres://admin:SuperSecretPw123@127.0.0.1:1/appdb?connect_timeout=1&password=Other9Secret\")\nprint(err.message)\n",
    );
    let o = fogu(dir.path(), &["a.fg"]);
    let all = text(&o);
    assert!(all.contains("postgres://admin:***@127.0.0.1:1/appdb"), "{}", all);
    assert!(!all.contains("SuperSecretPw123") && !all.contains("Other9Secret"), "{}", all);
}

#[test]
fn check_warns_about_arms_after_a_catch_all_and_test_has_a_timeout() {
    let dir = TempDir::new("r3-tools");
    dir.write("m.fg", "fn classify(n: int): str {\n    return match n {\n        1 => \"one\"\n        _ => \"many\"\n        2 => \"two\"\n    }\n}\nprint(classify(2))\n");
    let o = fogu(dir.path(), &["check", "m.fg"]);
    assert!(text(&o).contains("W0006") && text(&o).contains("unreachable match arm"), "{}", text(&o));
    // T04: a test that never ends fails, the next one runs
    dir.write(
        "hang.test.fg",
        "import { test, expect } from \"fogu:test\"\nimport time from \"fogu:time\"\ntest(\"before\", () => { expect(1).toBe(1) })\ntest(\"hangs\", () => { loop { time.delay(20ms) } })\ntest(\"after\", () => { expect(1).toBe(1) })\n",
    );
    let o = fogu(dir.path(), &["test", "--timeout", "500ms", "hang.test.fg"]);
    let all = text(&o);
    assert_eq!(o.status.code(), Some(1), "{}", all);
    assert!(all.contains("timed out after 500ms"), "{}", all);
    assert!(all.contains("2 passed") && all.contains("1 failed"), "{}", all);
    // T05
    dir.write("top.fg", "let x = 1\nprint(x + 2)\n");
    assert!(text(&fogu(dir.path(), &["disasm", "top.fg"])).contains("no functions or methods to disassemble"));
    // T01: continuation lines keep their indentation
    dir.write("cont.fg", "const total = 1 +\n    2 +\n    3\nfn compute(a, b) {\n    let result = a\n        + b\n        * 2\n    return result\n}\nprint(total, compute(3, 4))\n");
    let before = fs::read_to_string(dir.path().join("cont.fg")).unwrap();
    fogu(dir.path(), &["fmt", "cont.fg"]);
    assert_eq!(fs::read_to_string(dir.path().join("cont.fg")).unwrap().replace("\r\n", "\n"), before);
}

#[test]
fn check_finds_the_obvious_static_errors_of_l11() {
    let dir = TempDir::new("l11");
    let cases = [
        ("a.fg", "struct S {}\nprint(S())\n", "is a struct: create one with `new S()`"),
        ("b.fg", "fn f() {}\nprint(new f())\n", "'f' is not a struct type (it is 'fn')"),
        ("c.fg", "print(\"s\".foo())\n", "'str' has no method 'foo'"),
        ("d.fg", "print([1].pushh(2))\n", "'list' has no method 'pushh'"),
        ("e.fg", "struct N { a = 1 }\nprint(new N(1, 2, 3))\n", "has no constructor, but 3 arguments were passed"),
        ("f.fg", "struct S {\n    fn m(a) { return a }\n}\nprint(new S().m(1, 2, 3))\n", "method 'm' expects at most 1 arguments, got 3"),
        ("g.fg", "interface Closable { close() }\nfn useC(c: Closable) { return c }\nprint(useC(5))\n", "parameter 'c' expects Closable, got int"),
        ("h.fg", "fn f(x: int = \"s\") { return x }\nprint(f())\n", "the default value of parameter 'x' is str, it expects int"),
        ("i.fg", "let x: char = \"AB\"\nprint(x)\n", "got str of 2 characters"),
        ("j.fg", "import u from \"./_u.fg\"\nprint(u._p())\n", "'_p' is private to module 'u'"),
        ("k.fg", "import fs from \"fogu:fs\"\nprint(fs.thisDoesNotExist(\"x\"))\n", "'fogu:fs' has no member 'thisDoesNotExist'"),
    ];
    dir.write("_u.fg", "export fn _p() { return 1 }\nexport fn q() { return 2 }\n");
    for (file, src, want) in cases {
        dir.write(file, src);
        let o = fogu(dir.path(), &["check", file]);
        assert!(text(&o).contains(want), "{}: {}", file, text(&o));
    }
    // and what is right stays right
    dir.write(
        "ok.fg",
        "import u from \"./_u.fg\"\nimport fs from \"fogu:fs\"\nfn shadow(fs) { return fs.whatever }\nprint(fs.exists(\"ok.fg\"), shadow({ whatever: 1 }))\nstruct P {\n    x = 0\n    new(x) { this.x = x }\n    fn get() { return this.x }\n}\ninterface Getter { get() }\nfn show(g: Getter) { return g.get() }\nfn f(x: int = 3) { return x }\nlet c: char = \"A\"\nprint(new P(1).get(), show(new P(2)), show({ get: () => 3 }), f(), c, \"s\".upper(), [1].push(2), u.q())\n",
    );
    let o = fogu(dir.path(), &["check", "ok.fg"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&fogu(dir.path(), &["ok.fg"]).stdout).trim(), "true 1\n1 2 3 3 A S [1, 2] 2");
}

#[test]
#[cfg(unix)]
fn process_run_kills_a_child_that_runs_out_of_time() {
    // B35
    let dir = TempDir::new("proc-timeout");
    dir.write(
        "a.fg",
        "import process from \"fogu:process\"\nconst t0 = clock()\nconst r = process.run(\"sleep\", [\"5\"], { timeout: 300ms })?\nprint(r.ok, r.timedOut, clock() - t0 < 3.0)\nconst q = process.run(\"true\", [], { timeout: 5s })?\nprint(q.ok, q.timedOut)\n",
    );
    let o = fogu(dir.path(), &["a.fg"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n"), "false true true\ntrue false\n", "{}", text(&o));
}

#[test]
#[cfg(target_os = "linux")]
fn vm_contexts_do_not_keep_their_functions() {
    // P07: every fogu:vm context that defined a function leaked ~1.2 KB (266 MB for 200k)
    let dir = TempDir::new("vm-leak");
    dir.write(
        "a.fg",
        "import vm from \"fogu:vm\"\nfor i in 0..60000 { vm.runInNewContext(\"fn f(x) { return x + 1 }\\nf(1)\") }\nconst st = Fogu.file(\"/proc/self/status\").text()\nfor line in st.split(\"\\n\") { if line.contains(\"VmRSS\") { print(int(line.split(\":\")[1].trim().split(\" \")[0])) } }\n",
    );
    let o = fogu(dir.path(), &["a.fg"]);
    let kb: u64 = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(u64::MAX);
    // (0.8.3: about 90 MB; now the same as a program that makes no context)
    assert!(kb < 40_000, "RSS after 60000 contexts: {} kB\n{}", kb, text(&o));
}

#[test]
fn jit_debug_names_a_variable_compiled_code_only_reads() {
    // B23: the message said "assignment to const 'x'" for a `let x`
    let dir = TempDir::new("jit-debug");
    dir.write("a.fg", "let x = []\nfor i in 0..1000 { x = [x] }\nprint(len(x))\n");
    let o = fogu(dir.path(), &["--jit-debug", "a.fg"]);
    let t = text(&o);
    assert!(t.contains("assignment to 'x', which compiled code only reads"), "{}", t);
    assert!(!t.contains("const"), "{}", t);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "1", "{}", t);
}

#[test]
#[cfg(unix)]
fn local_packages_from_an_archive_and_after_a_change() {
    let dir = TempDir::new("local-pkg");
    dir.write("lib/fogu.json", "{\"name\":\"mylib\",\"version\":\"1.0.0\",\"main\":\"index.fg\"}\n");
    dir.write("lib/index.fg", "export fn hi() { return \"hi\" }\n");
    dir.write("app/fogu.json", "{\"name\":\"app\",\"version\":\"0.1.0\",\"entry\":\"main.fg\"}\n");
    dir.write("app/t.fg", "import { hi } from \"mylib-1.0\"\nprint(hi())\n");
    let app = dir.path().join("app");
    let tar = Command::new("tar")
        .args(["czf", "mylib-1.0.tar.gz", "-C", "lib", "."])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(tar.success());

    // §10: a local .tar.gz is extracted (it failed with "the directory name is invalid")
    let o = fogu(&app, &["add", "../mylib-1.0.tar.gz"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = fogu(&app, &["t.fg"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hi", "{}", text(&o));

    // §17: a local folder that changed after fogu.lock is named as such, with the way out
    let o = fogu(&app, &["add", "../lib"]);
    assert!(o.status.success(), "{}", text(&o));
    fs::write(dir.path().join("lib/index.fg"), "export fn hi() { return \"HI\" }\n").unwrap();
    let o = fogu(&app, &["install"]);
    let t = text(&o);
    assert!(!o.status.success(), "{}", t);
    assert!(t.contains("the local package 'lib' changed since fogu.lock was written"), "{}", t);
    assert!(t.contains("run `fogu update lib`"), "{}", t);
}

#[test]
#[cfg(unix)]
fn nested_local_packages_and_link_mode() {
    let dir = TempDir::new("nested-local");
    // keycloak depends on oidc through a path relative to its own folder
    dir.write("libs/oidc/fogu.json", "{\"name\":\"oidc\",\"version\":\"1.0.0\",\"main\":\"index.fg\"}\n");
    dir.write("libs/oidc/index.fg", "export fn verify() { return \"verified\" }\n");
    dir.write(
        "libs/keycloak/fogu.json",
        "{\"name\":\"keycloak\",\"version\":\"1.0.0\",\"main\":\"index.fg\",\"dependencies\":{\"oidc\":\"file:../oidc\"}}\n",
    );
    dir.write("libs/keycloak/index.fg", "import { verify } from \"oidc\"\nexport fn login() { return \"login \" + verify() }\n");
    dir.write("app/fogu.json", "{\"name\":\"app\",\"version\":\"0.1.0\",\"entry\":\"main.fg\",\"dependencies\":{\"keycloak\":\"file:../libs/keycloak\"}}\n");
    dir.write("app/t.fg", "import { login } from \"keycloak\"\nprint(login())\n");
    let app = dir.path().join("app");
    let o = fogu(&app, &["install"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = fogu(&app, &["t.fg"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "login verified", "{}", text(&o));

    // link: the library's folder itself — an edit shows at the next run, no `fogu update`
    dir.write("libs/mylib/fogu.json", "{\"name\":\"mylib\",\"version\":\"1.0.0\",\"main\":\"index.fg\"}\n");
    dir.write("libs/mylib/index.fg", "export fn hi() { return \"hi\" }\n");
    dir.write("app2/fogu.json", "{\"name\":\"app2\",\"version\":\"0.1.0\",\"entry\":\"main.fg\",\"dependencies\":{\"mylib\":\"link:../libs/mylib\"}}\n");
    dir.write("app2/t.fg", "import { hi } from \"mylib\"\nprint(hi())\n");
    let app2 = dir.path().join("app2");
    let o = fogu(&app2, &["install"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = fogu(&app2, &["t.fg"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hi", "{}", text(&o));
    fs::write(dir.path().join("libs/mylib/index.fg"), "export fn hi() { return \"HI\" }\n").unwrap();
    let o = fogu(&app2, &["t.fg"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "HI", "{}", text(&o));
    let o = fogu(&app2, &["install"]);
    assert!(o.status.success(), "{}", text(&o));
}

#[test]
fn build_compile_leaves_out_package_tests_and_excludes() {
    let dir = TempDir::new("bundle-exclude");
    dir.write("lib/fogu.json", "{\"name\":\"lib\",\"version\":\"1.0.0\",\"main\":\"index.fg\",\"exclude\":[\"tools/\",\"build.fg\"]}\n");
    dir.write("lib/index.fg", "import { helper } from \"./tools/used.fg\"\nexport fn hi() { return \"hi \" + helper() }\n");
    dir.write("lib/tools/used.fg", "export fn helper() { return \"there\" }\n");
    dir.write("lib/tools/unused.fg", "print(1)\n");
    dir.write("lib/build.fg", "print(1)\n");
    dir.write("lib/tests/a.fg", "print(1)\n");
    dir.write("lib/a.test.fg", "print(1)\n");
    dir.write("lib/extra.fg", "print(1)\n");
    dir.write("app/fogu.json", "{\"name\":\"app\",\"version\":\"0.1.0\",\"entry\":\"main.fg\",\"dependencies\":{\"lib\":\"file:../lib\"}}\n");
    dir.write("app/main.fg", "import { hi } from \"lib\"\nprint(hi())\n");
    let app = dir.path().join("app");
    let o = fogu(&app, &["install"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = fogu(&app, &["build", "--compile", "-o", "out"]);
    assert!(o.status.success(), "{}", text(&o));
    // main.fg, fogu.json, lib/fogu.json, lib/index.fg, lib/tools/used.fg (imported), lib/extra.fg
    assert!(text(&o).contains("6 file(s)"), "{}", text(&o));
    let exe = app.join("out").join(if cfg!(windows) { "app.exe" } else { "app" });
    let o = Command::new(&exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hi there", "{}", text(&o));
}
