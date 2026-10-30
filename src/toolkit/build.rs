//! `fogu build --compile` — standalone executables (spec §64).
//!
//! The output is a copy of the `fogu` binary itself with a *bundle* appended:
//!
//! ```text
//! [ fogu executable ][ bundle (JSON) ][ bundle length: u64 LE ][ MAGIC ]
//! ```
//!
//! The bundle holds the program's entry file, every `.fg` file it imports
//! (followed transitively) and the packages it uses, laid out as a small
//! project (`fogu.json`, sources, `fogu_modules/<name>/…`). When an
//! executable starts and finds the trailer, it extracts the bundle once into
//! a cache folder (keyed by its hash) and runs the entry file from there,
//! passing every command-line argument to the program.
//!
//! Other files can be embedded too (`"embed": ["dist", "templates/*.html"]`
//! in fogu.json, or `--embed <glob>`); the program finds them with
//! `Fogu.asset("dist")`, which also works when running from the sources.
//!
//! Cross-platform: `--target linux-arm64` (or a list, or `all`) appends the
//! same bundle to the Fogu runtime of that platform — a `fogu-<target>`
//! binary found next to this executable, in `dist/`, in `$FOGU_HOME/runtimes`
//! or given with `--runtime`. The runtimes are built by `docker/Dockerfile`.
//! Smaller executables: the standard library areas a program imports decide
//! the runtime "flavor" — `core` when it only uses the always-built modules
//! (16 MB instead of 61 MB on linux-x64), else e.g. `network+database`. A
//! `fogu-<target>-<flavor>` runtime is used when found (or downloaded); with
//! `--minimal` it is compiled with `cargo` from the Fogu sources (`$FOGU_SRC`
//! or the checkout this `fogu` was built in). Otherwise the full runtime is used.
//!
//! When none is found, the runtime of this Fogu version is downloaded from the
//! Fogu server (`$FOGU_SERVER`, or the address saved in `$FOGU_HOME/server` by
//! the install scripts), checked against its SHA-256 and cached in
//! `$FOGU_HOME/runtimes/<version>/` — so any machine builds for any platform.
//!
//! Services: `--service` (or `"service"` in fogu.json) records service settings
//! in the bundle; the executable then accepts `install`, `uninstall`, `start`,
//! `stop` and `status`, and runs as a Windows service / systemd unit
//! (toolkit/service.rs). A Linux target also gets `<name>.service` next to it.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};

use super::ui;
use crate::pkg::{self, Manifest, MANIFEST};
use crate::runtime::ast::Stmt;
use crate::runtime::{Lexer, Parser};

const MAGIC: &[u8; 8] = b"FOGUBND1";

/// Platforms of `--target`: (name, Rust triple, aliases).
const TARGETS: &[(&str, &str, &[&str])] = &[
    ("linux-x64", "x86_64-unknown-linux-gnu", &["linux"]),
    ("linux-arm64", "aarch64-unknown-linux-gnu", &["raspberry", "rpi", "raspberry-arm64"]),
    ("linux-armv7", "armv7-unknown-linux-gnueabihf", &["raspberry-armv7", "rpi-armv7"]),
    ("windows-x64", "x86_64-pc-windows-gnu", &["windows", "win"]),
    ("windows-x86", "i686-pc-windows-gnu", &["win32", "windows-32", "windows-i686"]),
    ("macos-x64", "x86_64-apple-darwin", &["mac-x64", "darwin-x64"]),
    ("macos-arm64", "aarch64-apple-darwin", &["mac", "macos", "darwin-arm64"]),
    ("wasm", "wasm32-wasip1", &["wasi", "wasm32", "webassembly"]),
];
/// Folders that are never embedded when copying a package.
const SKIP_DIRS: &[&str] = &[".git", "target", "dist", "node_modules"];

/// The program to embed: virtual path → content.
struct Bundle {
    name: String,
    entry: String,
    files: BTreeMap<String, String>,
    /// `fogu:*` modules imported anywhere in the bundle (and `sql` for `Fogu.sql`)
    modules: std::collections::BTreeSet<String>,
    /// `fogu:vm` can import any module at run time: only the full runtime fits
    dynamic: bool,
    /// embedded files (any content), base64
    assets: BTreeMap<String, String>,
    /// `"service"` of fogu.json (null when absent)
    service: Json,
}

/// The `--service*` flags of `fogu build`.
#[derive(Default)]
struct ServiceFlags {
    on: bool,
    name: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    start_type: Option<String>,
}

/// The service settings of the bundle: fogu.json `"service"`, then the flags.
fn service_spec(bundle: &Bundle, f: &ServiceFlags) -> Result<Option<super::service::ServiceSpec>, String> {
    use super::service::{check_name, ServiceSpec, StartType};
    let mut spec = ServiceSpec::from_json(&bundle.service, &bundle.name).map_err(|e| format!("fogu.json: {}", e))?;
    if f.on && spec.is_none() {
        spec = Some(ServiceSpec::new(&bundle.name));
    }
    let Some(mut s) = spec else { return Ok(None) };
    if let Some(n) = &f.name {
        check_name(n)?;
        if f.display_name.is_none() && s.display_name == s.name {
            s.display_name = n.clone();
        }
        s.name = n.clone();
    }
    if let Some(d) = &f.display_name {
        s.display_name = d.clone();
    }
    if let Some(d) = &f.description {
        s.description = d.clone();
    }
    if let Some(t) = &f.start_type {
        s.start_type = StartType::parse(t)?;
    }
    Ok(Some(s))
}

// ---------------------------------------------------------------------
// fogu build
// ---------------------------------------------------------------------

pub fn build(args: &[String]) -> i32 {
    let mut compile = false;
    let mut targets: Vec<String> = Vec::new();
    let mut runtime: Option<PathBuf> = None;
    let mut out_dir = PathBuf::from("dist");
    let mut entry_arg: Option<String> = None;
    let mut embed: Vec<String> = Vec::new();
    let mut minimal = false;
    let mut full = false;
    let mut svc = ServiceFlags::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--compile" => compile = true,
            "--target" => match it.next() {
                Some(t) => targets.extend(t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty())),
                None => return fail("--target expects a platform (fogu build --targets lists them)"),
            },
            "--runtime" => match it.next() {
                Some(r) => runtime = Some(PathBuf::from(r)),
                None => return fail("--runtime expects the path of a fogu binary"),
            },
            "--targets" => {
                list_targets();
                return 0;
            }
            "--embed" => match it.next() {
                Some(g) => embed.push(g.clone()),
                None => return fail("--embed expects a file, folder or glob"),
            },
            "--minimal" => minimal = true,
            "--service" => svc.on = true,
            "--service-name" | "--display-name" | "--description" | "--start-type" => {
                let Some(v) = it.next() else { return fail(&format!("{} expects a value", a)) };
                svc.on = true;
                let slot = match a.as_str() {
                    "--service-name" => &mut svc.name,
                    "--display-name" => &mut svc.display_name,
                    "--description" => &mut svc.description,
                    _ => &mut svc.start_type,
                };
                *slot = Some(v.clone());
            }
            "--full" => full = true,
            "-o" | "--outdir" => match it.next() {
                Some(d) => out_dir = PathBuf::from(d),
                None => return fail("-o expects a folder"),
            },
            s if s.starts_with('-') => return fail(&format!("unknown option: {}", s)),
            s => entry_arg = Some(s.to_string()),
        }
    }
    if !compile {
        return fail("usage: fogu build --compile [entry.fg] [-o <folder>] [--embed <glob>]... [--target <platform>] [--minimal | --full] [--service [--service-name <n>] [--display-name <n>] [--description <t>] [--start-type auto|manual|delayed]]");
    }
    let started = std::time::Instant::now();
    let bundle = match collect(entry_arg.as_deref(), &embed) {
        Ok(b) => b,
        Err(e) => return fail(&e),
    };
    let service = match service_spec(&bundle, &svc) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    // the standard library areas the program needs (None = any: fogu:vm)
    let flavor = if full || bundle.dynamic { None } else { Some(flavor_of(&bundle.modules)) };
    // the platforms and, for each one, the runtime binary to start from
    let mut plan: Vec<(Option<&'static str>, PathBuf)> = Vec::new();
    if targets.is_empty() {
        let lean = match (&runtime, &flavor, host_target()) {
            (None, Some(f), Some(host)) => lean_runtime(host, f, minimal),
            _ => None,
        };
        match lean.map(Ok).or_else(|| runtime.clone().map(Ok)).unwrap_or_else(|| std::env::current_exe().map_err(|e| e.to_string())) {
            Ok(exe) => plan.push((None, exe)),
            Err(e) => return fail(&format!("cannot find the fogu executable: {}", e)),
        }
    } else {
        let mut names: Vec<&'static str> = Vec::new();
        for t in &targets {
            if t == "all" {
                names.extend(TARGETS.iter().map(|t| t.0));
                continue;
            }
            match target_name(t) {
                Some(n) => names.push(n),
                None => return fail(&format!("unknown target '{}' (fogu build --targets lists them)", t)),
            }
        }
        names.dedup();
        if runtime.is_some() && names.len() > 1 {
            return fail("--runtime works with a single --target");
        }
        let all = targets.iter().any(|t| t == "all");
        for n in names {
            let lean = match (&runtime, &flavor) {
                (None, Some(f)) => lean_runtime(n, f, minimal),
                _ => None,
            };
            match lean.map(Ok).or_else(|| runtime.clone().map(Ok)).unwrap_or_else(|| find_runtime(n, true)) {
                Ok(p) => match check_runtime(&p, n) {
                    Ok(()) => plan.push((Some(n), p)),
                    Err(e) => return fail(&e),
                },
                // with `all`, the platforms without a runtime are skipped
                Err(e) if all => ui::warn(&e),
                Err(e) => return fail(&e),
            }
        }
        if plan.is_empty() {
            return fail("no runtime found for any target (build them with docker/Dockerfile)");
        }
    }

    if let Some(f) = &flavor {
        println!("  {} {}", ui::dim("runtime:"), ui::dim(&format!("{} ({})", f, if f == "core" { "only the always-built modules" } else { "standard library areas used" })));
    }
    for f in bundle.files.keys() {
        println!("  {} {}", ui::dim("+"), f);
    }
    if !bundle.assets.is_empty() {
        let bytes: usize = bundle.assets.values().map(|b| b.len() * 3 / 4).sum();
        println!(
            "  {} {} embedded file(s) {}",
            ui::dim("+"),
            bundle.assets.len(),
            ui::dim(&format!("({})", ui::human_size(bytes as u64)))
        );
    }
    if let Some(s) = &service {
        println!(
            "  {} {}",
            ui::dim("service:"),
            ui::dim(&format!("{} (\"{}\", start: {}) — run '<exe> install' as administrator / root", s.name, s.display_name, s.start_type.name()))
        );
    }
    for (target, base_exe) in &plan {
        let platform = target.or(host_target()).unwrap_or("");
        if service.is_some() && !(platform.starts_with("windows") || platform.starts_with("linux")) {
            ui::warn(&format!("{}: services are supported on Windows and Linux; the settings are kept but unused", platform));
        }
        match write_executable(&bundle, service.as_ref(), &out_dir, base_exe, *target) {
            Ok(path) => {
                // Linux: a systemd unit next to the binary (`<exe> install` writes it too)
                if let (Some(s), true) = (&service, platform.starts_with("linux")) {
                    let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let dir = format!("/opt/{}", s.name);
                    let unit = format!(
                        "# Copy {file} to {dir}/ (or edit ExecStart), then this file to /etc/systemd/system/,\n# or simply run `sudo {dir}/{file} install` on the target machine.\n{}",
                        s.systemd_unit(&format!("{}/{}", dir, file), &dir)
                    );
                    let unit_path = path.with_file_name(format!("{}.service", s.name));
                    match std::fs::write(&unit_path, unit) {
                        Ok(()) => println!("{} {}", ui::green("wrote"), unit_path.display()),
                        Err(e) => return fail(&format!("{}: {}", unit_path.display(), e)),
                    }
                }
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                println!(
                    "{} {} {}",
                    ui::green("built"),
                    path.display(),
                    ui::dim(&format!(
                        "({}, {} file(s), {}ms)",
                        ui::human_size(size),
                        bundle.files.len(),
                        started.elapsed().as_millis()
                    ))
                );
            }
            Err(e) => return fail(&e),
        }
    }
    0
}

/// A platform name, alias or Rust triple → the platform name.
fn target_name(t: &str) -> Option<&'static str> {
    let t = t.to_lowercase();
    if t == "native" || t == "host" {
        return host_target();
    }
    TARGETS
        .iter()
        .find(|(n, triple, aliases)| *n == t || *triple == t || aliases.contains(&t.as_str()))
        .map(|t| t.0)
        .or_else(|| {
            // the MSVC build is the same platform for a Windows executable
            (t == "x86_64-pc-windows-msvc").then_some("windows-x64")
        })
}

/// The platform this fogu was built for.
fn host_target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "arm") => "linux-armv7",
        ("windows", "x86_64") => "windows-x64",
        ("windows", "x86") => "windows-x86",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        _ => return None,
    })
}

fn list_targets() {
    let host = host_target();
    for (name, triple, aliases) in TARGETS {
        let found = find_runtime(name, false).is_ok();
        println!(
            "  {:<12} {:<32} {}{}",
            name,
            ui::dim(triple),
            if found { ui::green("runtime found") } else { ui::dim("no runtime") },
            if Some(*name) == host { ui::dim(" (this machine)") } else { String::new() }
        );
        if !aliases.is_empty() {
            println!("  {:<12} {}", "", ui::dim(&format!("aliases: {}", aliases.join(", "))));
        }
    }
}

fn exe_name(base: &str, target: &str) -> String {
    if target == "wasm" {
        return format!("{}.wasm", base);
    }
    if target.starts_with("windows") {
        format!("{}.exe", base)
    } else {
        base.to_string()
    }
}

/// The `fogu-<target>` runtime: `$FOGU_RUNTIMES`, next to this executable
/// (also `runtimes/`), `dist/` of the current folder, `$FOGU_HOME/runtimes`
/// (and its `<version>/` cache); for this machine's platform, this very
/// executable; else, with `download`, the Fogu server's copy of this version.
fn find_runtime(target: &str, download: bool) -> Result<PathBuf, String> {
    let file = exe_name(&format!("fogu-{}", target), target);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("FOGU_RUNTIMES") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(dir.join("runtimes"));
        dirs.push(dir);
    }
    dirs.push(PathBuf::from("dist"));
    dirs.push(pkg::home().join("runtimes"));
    dirs.push(pkg::home().join("runtimes").join(env!("CARGO_PKG_VERSION")));
    for d in &dirs {
        let p = d.join(&file);
        if p.is_file() {
            return Ok(p);
        }
    }
    // a source checkout: target/release/fogu → target/wasm32-wasip1/release/fogu.wasm
    if target == "wasm" {
        if let Some(t) = std::env::current_exe().ok().and_then(|e| e.parent()?.parent().map(Path::to_path_buf)) {
            let p = t.join("wasm32-wasip1").join("release").join("fogu.wasm");
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    if host_target() == Some(target) {
        return std::env::current_exe().map_err(|e| e.to_string());
    }
    let server = server_url();
    if download && target != "wasm" {
        if let Some(server) = &server {
            return download_runtime(server, target, &file);
        }
    }
    Err(format!(
        "no fogu runtime for '{}': put {} in {} (build it with docker/Dockerfile), pass --runtime <path>, or set FOGU_SERVER to a Fogu server to download it{}",
        target,
        file,
        pkg::home().join("runtimes").display(),
        server.map(|s| format!(" (server: {})", s)).unwrap_or_default()
    ))
}

/// The standard library area (cargo feature of `fogu`) a `fogu:*` module needs;
/// `None` = always built. Kept in sync with `fogu-std/src/host.rs` by a test.
fn module_area(module: &str) -> Option<&'static str> {
    Some(match module {
        "etcd" | "kv" | "surrealkv" | "surrealdb" | "sql" | "orm" | "redis" | "scylla" => "database",
        "grpc" | "protobuf" => "rpc",
        "security" | "socketio" | "http" | "web" | "topcoat" | "jwt" | "mail" | "websocket" | "html" => "network",
        "broker" | "kafka" | "mq" | "fogumq" | "foguMQ-client" | "nats" | "rabbitmq" | "mqtt" | "bullmq" => "messaging",
        "ai" | "plot" => "ai",
        "s3" | "docker" | "service" | "playbook" | "ssh" => "ops",
        "image" | "webview" => "media",
        _ => return None,
    })
}

/// The runtime flavor for a set of modules: `core`, or the areas joined by `+`
/// (with the areas they enable: database → rpc, ai → media + network).
fn flavor_of(modules: &std::collections::BTreeSet<String>) -> String {
    let mut areas: std::collections::BTreeSet<&'static str> = modules.iter().filter_map(|m| module_area(m)).collect();
    if areas.contains("database") {
        areas.insert("rpc");
    }
    if areas.contains("ai") {
        areas.insert("media");
        areas.insert("network");
    }
    if areas.is_empty() {
        "core".to_string()
    } else {
        areas.into_iter().collect::<Vec<_>>().join("+")
    }
}

/// A runtime with only the areas of `flavor`: `fogu-<target>-<flavor>` found
/// like the full one (or downloaded); with `minimal`, compiled from the Fogu
/// sources when it is not found. `None` = use the full runtime.
fn lean_runtime(target: &str, flavor: &str, minimal: bool) -> Option<PathBuf> {
    if target == "wasm" {
        return None;
    }
    let file = exe_name(&format!("fogu-{}-{}", target, flavor), target);
    let version = env!("CARGO_PKG_VERSION");
    let cache = pkg::home().join("runtimes").join(version);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("FOGU_RUNTIMES") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(dir.join("runtimes"));
        dirs.push(dir);
    }
    dirs.push(PathBuf::from("dist"));
    dirs.push(cache.clone());
    if let Some(p) = dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file()) {
        return Some(p);
    }
    if let Some(server) = server_url() {
        if let Ok(p) = download_runtime_file(&server, target, Some(flavor), &file) {
            return Some(p);
        }
    }
    if minimal {
        match build_lean_runtime(target, flavor, &cache.join(&file)) {
            Ok(p) => return Some(p),
            Err(e) => ui::warn(&format!("--minimal: {}; using the full runtime", e)),
        }
    } else {
        eprintln!(
            "{}",
            ui::dim(&format!(
                "no {} runtime for {} found: using the full one (`fogu build --compile --minimal` compiles a smaller one)",
                flavor, target
            ))
        );
    }
    None
}

/// The Fogu sources: `$FOGU_SRC`, or the checkout this `fogu` was built in
/// (`<src>/target/<…>/fogu`).
fn fogu_sources() -> Option<PathBuf> {
    let is_src = |d: &Path| d.join("Cargo.toml").is_file() && d.join("crates").join("fogu-std").is_dir();
    if let Some(d) = std::env::var_os("FOGU_SRC").map(PathBuf::from) {
        return is_src(&d).then_some(d);
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).find(|d| is_src(d)).map(Path::to_path_buf)
}

/// `cargo build --release --no-default-features --features <areas>` of the
/// Fogu sources for `target`, copied to `dest`.
fn build_lean_runtime(target: &str, flavor: &str, dest: &Path) -> Result<PathBuf, String> {
    let src = fogu_sources().ok_or("the Fogu sources are not available (set FOGU_SRC to a checkout of lang/)")?;
    let triple = TARGETS.iter().find(|t| t.0 == target).map(|t| t.1).ok_or("unknown target")?;
    let target_dir = pkg::home().join("build");
    let mut cmd = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(&src)
        .args(["build", "--release", "-p", "fogu", "--no-default-features"])
        .arg("--target-dir")
        .arg(&target_dir);
    if flavor != "core" {
        cmd.arg("--features").arg(flavor.replace('+', ","));
    }
    let native = host_target() == Some(target);
    if !native {
        cmd.args(["--target", triple]);
    }
    eprintln!("{}", ui::dim(&format!("compiling a {} runtime for {} (cargo, a few minutes the first time)…", flavor, target)));
    let status = cmd.status().map_err(|e| format!("cannot run cargo: {}", e))?;
    if !status.success() {
        return Err(format!("cargo build failed for {}", target));
    }
    let mut built = target_dir.clone();
    if !native {
        built.push(triple);
    }
    built.push("release");
    built.push(exe_name("fogu", target));
    std::fs::create_dir_all(dest.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    std::fs::copy(&built, dest).map_err(|e| format!("{}: {}", built.display(), e))?;
    Ok(dest.to_path_buf())
}

/// The Fogu server: `$FOGU_SERVER`, else the address the install scripts saved
/// in `$FOGU_HOME/server`, else https://fogulang.microaitec.com.
fn server_url() -> Option<String> {
    pkg::registry::server_url()
}

/// Downloads the runtime of `target` for this Fogu version from the server
/// (`/download/<version>/<file>`, the names the install scripts use), checks
/// its SHA-256 (`<file>.sha256`) and caches it in `$FOGU_HOME/runtimes/<version>/`.
fn download_runtime(server: &str, target: &str, file: &str) -> Result<PathBuf, String> {
    download_runtime_file(server, target, None, file)
}

/// The file names on the server: the full runtimes are named as the install
/// scripts expect (`fogu`, `fogu.exe`, `fogu-<target>`); a flavor adds
/// `-<flavor>` before the extension (`fogu-core`, `fogu-core.exe`,
/// `fogu-linux-arm64-network`).
#[cfg(not(target_family = "wasm"))]
fn download_runtime_file(server: &str, target: &str, flavor: Option<&str>, file: &str) -> Result<PathBuf, String> {
    let version = env!("CARGO_PKG_VERSION");
    let stem = match target {
        "linux-x64" | "windows-x64" => "fogu".to_string(),
        t => format!("fogu-{}", t),
    };
    let stem = match flavor {
        Some(f) => format!("{}-{}", stem, f),
        None => stem,
    };
    let remote = if target.starts_with("windows") { format!("{}.exe", stem) } else { stem };
    let url = format!("{}/download/{}/{}", server, version, remote);
    eprintln!("{}", ui::dim(&format!("downloading the {} runtime: {}", target, url)));
    let client = pkg::net::blocking_client();
    let get = |u: &str| -> Result<Vec<u8>, String> {
        let res = client
            .get(u)
            .timeout(std::time::Duration::from_secs(600))
            .send()
            .map_err(|e| format!("{}: {}", u, pkg::net::error_chain(&e)))?;
        if !res.status().is_success() {
            return Err(format!("{}: HTTP {}", u, res.status()));
        }
        res.bytes().map(|b| b.to_vec()).map_err(|e| format!("{}: {}", u, pkg::net::error_chain(&e)))
    };
    let bytes = get(&url).map_err(|e| format!("no {} runtime for fogu {} on the server ({})", target, version, e))?;
    let expected = String::from_utf8_lossy(&get(&format!("{}.sha256", url))?)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if expected.is_empty() || expected != actual {
        return Err(format!("the {} runtime from {} does not match its SHA-256 (expected {}, got {})", target, url, expected, actual));
    }
    let dir = pkg::home().join("runtimes").join(version);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let dest = dir.join(file);
    let tmp = dir.join(format!("{}.part", file));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {}", tmp.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    eprintln!("{}", ui::dim(&format!("cached {}", dest.display())));
    Ok(dest)
}

#[cfg(target_family = "wasm")]
fn download_runtime_file(_server: &str, target: &str, _flavor: Option<&str>, _file: &str) -> Result<PathBuf, String> {
    Err(format!("no fogu runtime for '{}'", target))
}

#[cfg(test)]
mod flavor_tests {
    use super::*;

    #[test]
    fn module_areas_match_the_stdlib_registry() {
        // every `ctx.define_lazy("name", …)` under `#[cfg(feature = "area")]`
        let host = include_str!("../../crates/fogu-std/src/host.rs").replace("\r\n", "\n");
        let body = &host[host.find("fn install").expect("install")..];
        let mut feature: Option<String> = None;
        for line in body.lines().map(str::trim) {
            if let Some(f) = line.strip_prefix("#[cfg(feature = \"").and_then(|r| r.split('"').next()) {
                feature = Some(f.to_string());
                continue;
            }
            if line.starts_with("#[cfg(") {
                continue;
            }
            if let Some(name) = line.strip_prefix("ctx.define_lazy(\"").and_then(|r| r.split('"').next()) {
                assert_eq!(module_area(name), feature.as_deref(), "fogu:{} is in area {:?}", name, feature);
            }
            if !line.is_empty() {
                feature = None;
            }
        }
    }

    #[test]
    fn flavors() {
        let set = |m: &[&str]| m.iter().map(|s| s.to_string()).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(flavor_of(&set(&["fs", "shell", "json"])), "core");
        assert_eq!(flavor_of(&set(&["http", "fs"])), "network");
        assert_eq!(flavor_of(&set(&["sql", "web"])), "database+network+rpc");
        assert_eq!(flavor_of(&set(&["ai"])), "ai+media+network");
    }
}

/// Checks that a runtime binary is an executable of the expected platform.
fn check_runtime(path: &Path, target: &str) -> Result<(), String> {
    use std::io::Read;
    let mut head = [0u8; 1024];
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let n = f.read(&mut head).map_err(|e| format!("{}: {}", path.display(), e))?;
    let head = &head[..n];
    let u16le = |i: usize| head.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32le = |i: usize| head.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let found = if head.starts_with(b"\0asm") {
        "wasm"
    } else if head.starts_with(b"\x7fELF") {
        match u16le(18) {
            Some(0x3e) => "linux-x64",
            Some(0xb7) => "linux-arm64",
            Some(0x28) => "linux-armv7",
            _ => "linux (another architecture)",
        }
    } else if head.starts_with(b"MZ") {
        // the PE header (offset at 0x3c) names the machine
        match u32le(0x3c).and_then(|pe| u16le(pe as usize + 4)) {
            Some(0x014c) => "windows-x86",
            Some(0xaa64) => "windows (arm64)",
            _ => "windows-x64",
        }
    } else if u32le(0) == Some(0xfeed_facf) {
        match u32le(4) {
            Some(0x0100_0007) => "macos-x64",
            Some(0x0100_000c) => "macos-arm64",
            _ => "macos (another architecture)",
        }
    } else {
        "not an executable"
    };
    if found != target {
        return Err(format!("{} is not a {} runtime ({})", path.display(), target, found));
    }
    Ok(())
}

fn fail(msg: &str) -> i32 {
    ui::error(msg);
    1
}

/// Finds the entry file and every module it needs.
fn collect(entry_arg: Option<&str>, extra_embed: &[String]) -> Result<Bundle, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = pkg::find_root(&cwd);
    let manifest = root.as_ref().and_then(|r| Manifest::load(&r.join(MANIFEST)).ok());

    let entry_path = match entry_arg {
        Some(e) => cwd.join(e),
        None => {
            let e = manifest
                .as_ref()
                .and_then(|m| m.entry())
                .ok_or("no entry file: pass one (fogu build --compile main.fg) or set \"entry\" in fogu.json")?;
            root.clone().unwrap_or_else(|| cwd.clone()).join(e)
        }
    };
    let entry_path = entry_path
        .canonicalize()
        .map_err(|e| format!("entry file '{}': {}", entry_path.display(), e))?;
    // The bundle's root: the project folder (or the entry's folder).
    let base = match &root {
        Some(r) if entry_path.starts_with(r.canonicalize().unwrap_or(r.clone())) => {
            r.canonicalize().unwrap_or(r.clone())
        }
        _ => entry_path.parent().map(Path::to_path_buf).unwrap_or(cwd.clone()),
    };

    let name = manifest
        .as_ref()
        .and_then(|m| m.str_field("name"))
        .unwrap_or_else(|| {
            entry_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "app".into())
        });
    let name: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '-' })
        .collect();

    pkg::set_main_dir(&base);
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut packages: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut modules: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut queue: VecDeque<(PathBuf, Option<(String, PathBuf)>)> = VecDeque::new();
    queue.push_back((entry_path.clone(), None));

    while let Some((file, in_pkg)) = queue.pop_front() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let src = std::fs::read_to_string(&file)
            .map_err(|e| format!("error reading '{}': {}", file.display(), e))?;
        let tokens = Lexer::new(&src).tokenize().map_err(|e| format!("{}: {}", file.display(), e))?;
        let dynamic = dynamic_imports(&tokens);
        let program = Parser::new(tokens).parse_program().map_err(|e| format!("{}: {}", file.display(), e))?;

        // Files of the project itself are stored by their path relative to
        // the root; package files are copied whole further below.
        if in_pkg.is_none() {
            let rel = file.strip_prefix(&base).map_err(|_| {
                format!(
                    "'{}' is outside the project folder '{}'; move it inside to bundle it",
                    file.display(),
                    base.display()
                )
            })?;
            files.insert(virtual_path(rel), src.clone());
        }

        if src.contains("Fogu.sql") {
            modules.insert("sql".to_string());
        }
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        for spec in imports(&program).into_iter().chain(dynamic) {
            if let Some(m) = spec.strip_prefix("fogu:") {
                modules.insert(m.to_string());
                continue;
            }
            let bare = !(spec.starts_with('.') || spec.starts_with('/') || spec.contains(":\\"));
            if bare {
                if let Some((pname, pdir)) = pkg::package_dir(&spec, &dir) {
                    packages.entry(pname).or_insert(pdir);
                    if let Some(target) = pkg::resolve_import(&spec, &dir)? {
                        let target = target.canonicalize().map_err(|e| e.to_string())?;
                        let owner = packages
                            .iter()
                            .find(|(_, d)| target.starts_with(d.canonicalize().unwrap_or((*d).clone())))
                            .map(|(n, d)| (n.clone(), d.clone()));
                        queue.push_back((target, owner));
                    }
                    continue;
                }
            }
            match &in_pkg {
                // a relative import inside a package stays inside that package
                Some(owner) => {
                    let target = crate::runtime::module::resolve_path(&spec, &dir)?;
                    let target = target
                        .canonicalize()
                        .map_err(|e| format!("cannot resolve '{}': {}", spec, e))?;
                    queue.push_back((target, Some(owner.clone())));
                }
                None => {
                    let target = crate::runtime::module::resolve_path(&spec, &dir)?;
                    let target = target
                        .canonicalize()
                        .map_err(|e| format!("cannot resolve '{}' (from {}): {}", spec, file.display(), e))?;
                    queue.push_back((target, None));
                }
            }
        }
    }

    // packages: copied whole into fogu_modules/<name>/ (vendoring layout)
    for (pname, pdir) in &packages {
        copy_package(pdir, &format!("fogu_modules/{}", pname), &mut files)?;
    }

    // a manifest makes the extracted folder a project root (vendored packages)
    let mut m = json!({ "name": name, "version": "0.0.0", "entry": "", "dependencies": {} });
    if let Some(man) = &manifest {
        m = man.json.clone();
    }
    let entry_rel = virtual_path(entry_path.strip_prefix(&base).map_err(|e| e.to_string())?);
    m["entry"] = Json::String(entry_rel.clone());
    files.insert(MANIFEST.into(), serde_json::to_string_pretty(&m).unwrap_or_default());

    // embedded files: fogu.json "embed" + --embed
    let mut patterns: Vec<String> = manifest
        .as_ref()
        .and_then(|m| m.json["embed"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    patterns.extend(extra_embed.iter().cloned());
    let assets = collect_assets(&base, &patterns)?;

    let dynamic = modules.contains("vm");
    let service = manifest.as_ref().map(|m| m.json["service"].clone()).unwrap_or(Json::Null);
    Ok(Bundle {
        name,
        entry: entry_rel,
        files,
        modules,
        dynamic,
        assets,
        service,
    })
}

/// Files (any content) matching the embed patterns: a file, a folder (taken
/// whole) or a glob with `*`, `?` and `**`, relative to the project root.
fn collect_assets(base: &Path, patterns: &[String]) -> Result<BTreeMap<String, String>, String> {
    use base64::Engine as _;
    fn add(base: &Path, p: &Path, out: &mut BTreeMap<String, String>) -> Result<(), String> {
        if p.is_dir() {
            let entries = std::fs::read_dir(p).map_err(|e| format!("embed: {}: {}", p.display(), e))?;
            for ent in entries.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                if name == ".git" || name == "node_modules" {
                    continue;
                }
                add(base, &ent.path(), out)?;
            }
            return Ok(());
        }
        let rel = p.strip_prefix(base).map_err(|_| format!("embed: '{}' is outside the project folder", p.display()))?;
        let data = std::fs::read(p).map_err(|e| format!("embed: {}: {}", p.display(), e))?;
        out.insert(virtual_path(rel), base64::engine::general_purpose::STANDARD.encode(data));
        Ok(())
    }
    let mut out = BTreeMap::new();
    for pat in patterns {
        let full = base.join(pat);
        let matches: Vec<PathBuf> = if pat.contains(['*', '?']) {
            // the glob does not understand the \?\ prefix of canonical Windows paths
            let text = full.to_string_lossy().to_string();
            let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
            crate::native::fs::glob(&text).into_iter().map(PathBuf::from).collect()
        } else if full.exists() {
            vec![full.clone()]
        } else {
            return Err(format!("embed: '{}' does not exist", pat));
        };
        if matches.is_empty() {
            return Err(format!("embed: '{}' matches no file", pat));
        }
        for m in matches {
            let m = m.canonicalize().unwrap_or(m);
            add(base, &m, &mut out)?;
        }
    }
    Ok(out)
}

/// `import ... from "spec"` specifiers of a program (imports are top-level only).
fn imports(program: &[Stmt]) -> Vec<String> {
    program
        .iter()
        .filter_map(|s| match s {
            Stmt::Import { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect()
}

/// `import("spec")` with a literal path anywhere in the code: bundled like a
/// static import (a computed path cannot be known; list it in "embed").
fn dynamic_imports(tokens: &[crate::runtime::token::Spanned]) -> Vec<String> {
    use crate::runtime::token::Token;
    tokens
        .windows(4)
        .filter_map(|w| match (&w[0].tok, &w[1].tok, &w[2].tok, &w[3].tok) {
            (Token::Import, Token::LParen, Token::Str(s), Token::RParen) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// `a\b.fg` → `a/b.fg`.
fn virtual_path(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn copy_package(dir: &Path, prefix: &str, files: &mut BTreeMap<String, String>) -> Result<(), String> {
    fn walk(base: &Path, dir: &Path, prefix: &str, files: &mut BTreeMap<String, String>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        for ent in entries.flatten() {
            let p = ent.path();
            let name = ent.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    walk(base, &p, prefix, files)?;
                }
            } else if name.ends_with(".fg") || name == MANIFEST {
                let rel = p.strip_prefix(base).map_err(|e| e.to_string())?;
                let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
                files.insert(format!("{}/{}", prefix, virtual_path(rel)), src);
            }
        }
        Ok(())
    }
    walk(dir, dir, prefix, files)
}

/// Copies the runtime (without any previous bundle) and appends the bundle.
/// With a target the file is named `<name>-<target>`.
fn write_executable(bundle: &Bundle, service: Option<&super::service::ServiceSpec>, out_dir: &Path, exe: &Path, target: Option<&str>) -> Result<PathBuf, String> {
    let mut bytes = std::fs::read(exe).map_err(|e| format!("cannot read {}: {}", exe.display(), e))?;
    if let Some((start, _)) = find_bundle(&bytes) {
        bytes.truncate(start);
    }
    let mut payload = json!({
        "name": bundle.name,
        "entry": bundle.entry,
        "files": bundle.files,
        "assets": bundle.assets,
    });
    if let Some(s) = service {
        payload["service"] = s.to_json();
    }
    let payload = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    if target == Some("wasm") {
        // WebAssembly: the bundle goes into the module's memory (wasm_embed.rs)
        bytes = super::wasm_embed::embed(&bytes, &payload).map_err(|e| format!("{}: {}", exe.display(), e))?;
    } else {
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(MAGIC);
    }

    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {}", out_dir.display(), e))?;
    let file_name = match target {
        Some(t) => exe_name(&format!("{}-{}", bundle.name, t), t),
        None if cfg!(windows) => format!("{}.exe", bundle.name),
        None => bundle.name.clone(),
    };
    let out = out_dir.join(file_name);
    std::fs::write(&out, &bytes).map_err(|e| format!("cannot write {}: {}", out.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755));
    }
    Ok(out)
}

/// Position (start, length) of an appended bundle, if the bytes have one.
fn find_bundle(bytes: &[u8]) -> Option<(usize, usize)> {
    let n = bytes.len();
    if n < 16 || &bytes[n - 8..] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(bytes[n - 16..n - 8].try_into().ok()?) as usize;
    let start = n.checked_sub(16)?.checked_sub(len)?;
    Some((start, len))
}

// ---------------------------------------------------------------------
// Running an embedded program
// ---------------------------------------------------------------------

/// The bundle slot of the WebAssembly runtime: the marker, then the address
/// and length of the bundle, filled in by `fogu build --target wasm`.
#[cfg(target_family = "wasm")]
#[used]
#[unsafe(no_mangle)]
static FOGU_WASM_SLOT: [u8; 24] = *b"FOGU-WASM-BUNDLE\0\0\0\0\0\0\0\0";

/// WebAssembly: the bundle sits in memory; its sources are served by the
/// in-memory file system (runtime/vfs.rs), assets are written next to the
/// program when the host gives access to a folder.
#[cfg(target_family = "wasm")]
pub fn embedded_entry() -> Option<Result<PathBuf, String>> {
    let slot: [u8; 24] = unsafe { std::ptr::read_volatile(&FOGU_WASM_SLOT) };
    let ptr = u32::from_le_bytes([slot[16], slot[17], slot[18], slot[19]]) as usize;
    let len = u32::from_le_bytes([slot[20], slot[21], slot[22], slot[23]]) as usize;
    if len == 0 {
        return None;
    }
    let payload = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
    Some((|| {
        let j: Json = serde_json::from_slice(payload).map_err(|e| format!("corrupted bundle: {}", e))?;
        let entry = j["entry"].as_str().ok_or("corrupted bundle: no entry")?.to_string();
        let files = j["files"].as_object().ok_or("corrupted bundle: no files")?;
        crate::runtime::vfs::install(files.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())));
        Ok(Path::new(crate::runtime::vfs::ROOT).join(entry))
    })())
}

/// If this executable carries a bundle, extracts it (once) and returns the
/// path of the entry file to run.
#[cfg(not(target_family = "wasm"))]
pub fn embedded_entry() -> Option<Result<PathBuf, String>> {
    embedded().map(|r| r.map(|e| e.entry))
}

/// The program embedded in this executable.
pub struct Embedded {
    pub entry: PathBuf,
    /// service settings (`fogu build --service`)
    pub service: Option<super::service::ServiceSpec>,
}

/// Like `embedded_entry`, with the service settings of the bundle.
#[cfg(not(target_family = "wasm"))]
pub fn embedded() -> Option<Result<Embedded, String>> {
    let exe = std::env::current_exe().ok()?;
    let bytes = read_tail(&exe)?;
    Some(extract(&bytes))
}

#[cfg(target_family = "wasm")]
pub fn embedded() -> Option<Result<Embedded, String>> {
    embedded_entry().map(|r| r.map(|entry| Embedded { entry, service: None }))
}



/// Reads the executable only when it ends with the bundle trailer.
fn read_tail(exe: &Path) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(exe).ok()?;
    let size = f.metadata().ok()?.len();
    if size < 16 {
        return None;
    }
    let mut tail = [0u8; 16];
    f.seek(SeekFrom::End(-16)).ok()?;
    f.read_exact(&mut tail).ok()?;
    if &tail[8..] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(tail[..8].try_into().ok()?);
    f.seek(SeekFrom::End(-16 - len as i64)).ok()?;
    let mut payload = vec![0u8; len as usize];
    f.read_exact(&mut payload).ok()?;
    Some(payload)
}

fn extract(payload: &[u8]) -> Result<Embedded, String> {
    let j: Json = serde_json::from_slice(payload).map_err(|e| format!("corrupted bundle: {}", e))?;
    let entry = j["entry"].as_str().ok_or("corrupted bundle: no entry")?.to_string();
    let service = super::service::ServiceSpec::from_json(&j["service"], j["name"].as_str().unwrap_or("app"))
        .map_err(|e| format!("corrupted bundle: {}", e))?;
    let files = j["files"].as_object().ok_or("corrupted bundle: no files")?;

    let hash: String = Sha256::digest(payload).iter().take(8).map(|b| format!("{:02x}", b)).collect();
    let dir = std::env::temp_dir().join("fogu-bundles").join(hash);
    let done = dir.join(".complete");
    if !done.is_file() {
        for (rel, content) in files {
            if rel.split('/').any(|c| c == ".." || c.is_empty()) {
                return Err(format!("corrupted bundle: invalid path '{}'", rel));
            }
            let p = dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
            }
            std::fs::write(&p, content.as_str().unwrap_or_default())
                .map_err(|e| format!("{}: {}", p.display(), e))?;
        }
        if let Some(assets) = j["assets"].as_object() {
            use base64::Engine as _;
            for (rel, data) in assets {
                if rel.split('/').any(|c| c == ".." || c.is_empty()) {
                    return Err(format!("corrupted bundle: invalid path '{}'", rel));
                }
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data.as_str().unwrap_or_default())
                    .map_err(|_| format!("corrupted bundle: bad data for '{}'", rel))?;
                let p = dir.join(rel);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
                }
                std::fs::write(&p, bytes).map_err(|e| format!("{}: {}", p.display(), e))?;
            }
        }
        std::fs::write(&done, b"").map_err(|e| e.to_string())?;
    }
    Ok(Embedded { entry: dir.join(entry), service })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_an_appended_bundle() {
        let mut bytes = b"EXECUTABLE".to_vec();
        assert_eq!(find_bundle(&bytes), None);
        let payload = b"{}";
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(MAGIC);
        assert_eq!(find_bundle(&bytes), Some((10, 2)));
    }

    #[test]
    fn target_names_and_aliases() {
        assert_eq!(target_name("raspberry"), Some("linux-arm64"));
        assert_eq!(target_name("aarch64-apple-darwin"), Some("macos-arm64"));
        assert_eq!(target_name("x86_64-pc-windows-msvc"), Some("windows-x64"));
        assert_eq!(target_name("win32"), Some("windows-x86"));
        assert_eq!(target_name("i686-pc-windows-gnu"), Some("windows-x86"));
        assert_eq!(target_name("amiga"), None);
        assert_eq!(exe_name("app-windows-x64", "windows-x64"), "app-windows-x64.exe");
    }

    #[test]
    fn virtual_paths_use_slashes() {
        assert_eq!(virtual_path(Path::new("src").join("a.fg").as_path()), "src/a.fg");
    }
}
