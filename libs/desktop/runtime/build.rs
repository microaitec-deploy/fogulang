//! Stages the Fogu app that this binary embeds, then runs tauri-build.
//!
//! The app folder comes from `FOGU_DESKTOP_APP` (set by `build.fg`; default:
//! `../examples/hello`). It is copied to `.bundle/`:
//!
//! * `.bundle/ui/`      — the frontend: `FOGU_DESKTOP_UI` (an absolute folder, set
//!   by build.fg: a Vite build's outDir) or the app's `desktop.ui` folder (default
//!   `ui`); Tauri embeds it (`build.frontendDist` in tauri.conf.json). With
//!   `FOGU_DESKTOP_DEV_URL` (dev.fg) the window loads that URL instead and the
//!   folder only holds a placeholder.
//! * `.bundle/program/` — every other file of the app (main.fg, modules, assets,
//!   fogu.json) plus this library as `fogu_modules/desktop`, so that
//!   `import desktop from "desktop"` resolves inside the executable. It is
//!   embedded with `include_dir!` and extracted on first run. The folders listed
//!   in `FOGU_DESKTOP_SKIP` (one absolute path per line: the Vite sources) are left out.
//!
//! `FOGU_DESKTOP_HASH` (a digest of everything staged) names the extraction folder.
//!
//! With `FOGU_DESKTOP_GENERIC=1` nothing is staged: the executable is the
//! generic host (see main.rs), reused by every app.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Never copied into the program bundle.
const SKIP: &[&str] = &[".git", "dist", "target", "node_modules", ".bundle", ".desktop-data"];

fn main() {
    println!("cargo:rerun-if-env-changed=FOGU_DESKTOP_APP");
    println!("cargo:rerun-if-env-changed=FOGU_DESKTOP_UI");
    println!("cargo:rerun-if-env-changed=FOGU_DESKTOP_SKIP");
    println!("cargo:rerun-if-env-changed=FOGU_DESKTOP_DEV_URL");
    println!("cargo:rerun-if-env-changed=FOGU_DESKTOP_STAMP");
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    println!("cargo:rerun-if-changed=../main.fg");
    println!("cargo:rerun-if-changed=../update.fg");
    println!("cargo:rerun-if-changed=../fogu.json");

    println!("cargo:rerun-if-env-changed=FOGU_DESKTOP_GENERIC");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    webview2_delay_load(&manifest);
    if env_nonempty("FOGU_DESKTOP_GENERIC").is_some() {
        // the generic host: no app inside; it runs the bundle appended to it
        // (`fogu desktop build`) or the folder FOGU_DESKTOP_PROGRAM_DIR names
        // (`fogu desktop dev`). A placeholder keeps include_dir! and Tauri happy.
        let bundle = manifest.join(".bundle");
        let _ = fs::remove_dir_all(&bundle);
        fs::create_dir_all(bundle.join("ui")).unwrap();
        fs::write(
            bundle.join("ui/index.html"),
            "<!doctype html><meta charset=utf-8><title>Fogu Desktop</title><p>This is the generic Fogu desktop host: run an app with <code>fogu desktop dev</code> or build one with <code>fogu desktop build</code>.</p>",
        )
        .unwrap();
        fs::create_dir_all(bundle.join("program")).unwrap();
        fs::write(bundle.join("program/.fogu-desktop-entry"), "").unwrap();
        println!("cargo:rustc-env=FOGU_DESKTOP_HASH=generic");
        println!("cargo:rustc-env=FOGU_DESKTOP_PROGRAM={}", bundle.join("program").display());
        tauri_build();
        return;
    }
    let app = match std::env::var("FOGU_DESKTOP_APP") {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => manifest.join("../examples/hello"),
    };
    let app = app.canonicalize().unwrap_or_else(|e| panic!("FOGU_DESKTOP_APP {}: {}", app.display(), e));
    let manifest_json = read_manifest(&app.join("fogu.json"));
    let ui = match env_nonempty("FOGU_DESKTOP_UI") {
        Some(p) => PathBuf::from(p),
        None => app.join(manifest_json.desktop_ui.clone().unwrap_or_else(|| "ui".to_string())),
    };
    let dev_url = env_nonempty("FOGU_DESKTOP_DEV_URL");
    if dev_url.is_none() && !ui.join("index.html").is_file() {
        panic!("fogu-desktop: {} has no index.html (set \"desktop\": {{ \"ui\": \"…\" }} in fogu.json)", ui.display());
    }
    let entry = manifest_json.entry.clone().unwrap_or_else(|| "main.fg".to_string());
    if !app.join(&entry).is_file() {
        panic!("fogu-desktop: the entry {} does not exist", app.join(&entry).display());
    }

    let bundle = manifest.join(".bundle");
    let _ = fs::remove_dir_all(&bundle);
    let mut hasher = Sha256::new();

    // the frontend (in dev mode the window loads the dev server: a placeholder is enough)
    let mut skip: Vec<PathBuf> = Vec::new();
    if let Some(url) = &dev_url {
        fs::create_dir_all(bundle.join("ui")).unwrap();
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>dev</title><p>This debug build loads the dev server at <a href=\"{0}\">{0}</a>.</p>",
            url
        );
        fs::write(bundle.join("ui/index.html"), page).unwrap();
        hasher.update(url.as_bytes());
    } else {
        copy_tree(&ui, &bundle.join("ui"), &[], &mut hasher);
    }
    // the program (without the UI folder when it sits inside the app, nor the skipped folders)
    if let Ok(c) = ui.canonicalize() {
        skip.push(c);
    }
    for p in env_nonempty("FOGU_DESKTOP_SKIP").unwrap_or_default().lines() {
        if let Ok(c) = PathBuf::from(p.trim()).canonicalize() {
            skip.push(c);
        }
    }
    copy_tree(&app, &bundle.join("program"), &skip, &mut hasher);
    // the desktop library itself, vendored for `import desktop from "desktop"`
    let lib = manifest.join("..");
    let vend = bundle.join("program/fogu_modules/desktop");
    fs::create_dir_all(&vend).unwrap();
    for f in ["fogu.json", "main.fg", "update.fg"] {
        let data = fs::read(lib.join(f)).unwrap();
        hasher.update(f.as_bytes());
        hasher.update(&data);
        fs::write(vend.join(f), data).unwrap();
    }
    // the entry file name, read by the host at run time
    fs::write(bundle.join("program/.fogu-desktop-entry"), &entry).unwrap();
    hasher.update(entry.as_bytes());

    let hash = hasher.finalize();
    let hex: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
    println!("cargo:rustc-env=FOGU_DESKTOP_HASH={}", hex);
    println!("cargo:rustc-env=FOGU_DESKTOP_PROGRAM={}", bundle.join("program").display());

    tauri_build();
}

/// Runs tauri-build with an ACL manifest for the bridge's commands. Tauri lets
/// a remote page (the Vite dev server of `fogu desktop dev`) call app commands
/// only when the app declares permissions for them; capabilities/ grants
/// `allow-fogu-call`, `allow-fogu-emit`, `allow-fogu-ready` and `allow-fogu-window` to the local UI
/// (fogu://, tauri://) and to localhost dev servers.
fn tauri_build() {
    let manifest = tauri_build::AppManifest::new().commands(&["fogu_call", "fogu_emit", "fogu_ready", "fogu_window"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest)).expect("tauri-build");
}

/// Windows with the GNU toolchain: WebView2Loader.dll is delay-loaded and
/// embedded, so the executable starts without the DLL next to it (see
/// webview2/README.md). Sets FOGU_WEBVIEW2_LOADER, which main.rs embeds.
fn webview2_delay_load(manifest: &Path) {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "gnu" {
        return;
    }
    let (arch, machine) = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default().as_str() {
        "x86_64" => ("x64", "i386:x86-64"),
        other => panic!("fogu-desktop: no WebView2Loader.dll for {} with the GNU toolchain", other),
    };
    let dll = manifest.join("webview2").join(arch).join("WebView2Loader.dll");
    let def = manifest.join("webview2/WebView2Loader.def");
    println!("cargo:rerun-if-changed={}", dll.display());
    println!("cargo:rerun-if-changed={}", def.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("webview2-delay");
    fs::create_dir_all(&out).unwrap();
    // rustc links `-l WebView2Loader.dll`: ld looks for libWebView2Loader.dll.a
    // here before webview2-com-sys's plain import library
    let lib = out.join("libWebView2Loader.dll.a");
    let candidates = ["x86_64-w64-mingw32-dlltool", "dlltool"];
    let ok = candidates.iter().any(|tool| {
        std::process::Command::new(tool)
            .args(["--machine", machine, "--input-def"])
            .arg(&def)
            .args(["--dllname", "WebView2Loader.dll", "--output-delaylib"])
            .arg(&lib)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    });
    if !ok {
        panic!("fogu-desktop: dlltool (x86_64-w64-mingw32-dlltool) is needed to build the Windows host");
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-env=FOGU_WEBVIEW2_LOADER={}", dll.display());
}

fn copy_tree(from: &Path, to: &Path, skip: &[PathBuf], hasher: &mut Sha256) {
    fs::create_dir_all(to).unwrap();
    let mut entries: Vec<_> = fs::read_dir(from).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        let name_s = name.to_string_lossy().to_string();
        let path = e.path();
        if SKIP.contains(&name_s.as_str()) {
            continue;
        }
        if let Ok(c) = path.canonicalize() {
            if skip.contains(&c) {
                continue;
            }
        }
        let ty = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if ty.is_dir() {
            println!("cargo:rerun-if-changed={}", path.display());
            copy_tree(&path, &to.join(&name), skip, hasher);
        } else if ty.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
            let data = fs::read(&path).unwrap();
            hasher.update(name_s.as_bytes());
            hasher.update(&data);
            fs::write(to.join(&name), data).unwrap();
        }
    }
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// The fields of fogu.json the build needs.
#[derive(Default)]
struct Manifest {
    entry: Option<String>,
    desktop_ui: Option<String>,
}

fn read_manifest(path: &Path) -> Manifest {
    let Ok(text) = fs::read_to_string(path) else { return Default::default() };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => panic!("fogu-desktop: {}: {}", path.display(), e),
    };
    Manifest {
        entry: v.get("entry").and_then(|e| e.as_str()).map(String::from),
        desktop_ui: v.pointer("/desktop/ui").and_then(|e| e.as_str()).map(String::from),
    }
}
