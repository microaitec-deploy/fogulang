//! fogu-desktop — a desktop window (Tauri: WebView2 / WKWebView / WebKitGTK)
//! whose backend is a Fogu program.
//!
//! Where the app comes from, in this order (see ../README.md):
//!
//! 1. a bundle appended to this executable by `fogu desktop build` (the
//!    `fogu build --compile` format, read through `fogu::toolkit::build`):
//!    its assets and UI extracted once to a temp folder (the UI is served from
//!    there over the `fogu://` protocol); the program's own `.fg` sources stay
//!    in memory (`fogu::runtime::vfs`), never on disk;
//! 2. the folder `FOGU_DESKTOP_PROGRAM_DIR` (`fogu desktop dev`): run in place;
//!    the window loads `FOGU_DESKTOP_DEV_URL` (the Vite dev server) or the
//!    app's UI folder; with `FOGU_DESKTOP_RELOAD=1` a change of a `.fg` file
//!    reloads the program in place (a new runtime, same windows);
//! 3. an app staged by build.rs and embedded (`--installer` builds): its UI is
//!    served by Tauri (`tauri://localhost`) and its program's other files
//!    extracted on first run to the app's local data folder (the `.fg` sources
//!    stay in memory).
//!
//! The same executable, built with `FOGU_DESKTOP_GENERIC=1`, is the *generic
//! host* that 1 and 2 reuse for every app: no Rust build per app.

mod bridge;
mod children;
#[cfg(desktop)]
mod menu;
#[cfg(desktop)]
mod shortcut;
#[cfg(desktop)]
mod tray;
mod update;
mod window;

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use bridge::Bridge;
use include_dir::{include_dir, Dir};
use serde_json::Value;
use tauri::http::{Request, Response};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow};

/// The app's Fogu program (main.fg, modules, assets, fogu.json, fogu_modules/desktop)
/// when build.rs staged one; empty in the generic host.
static PROGRAM: Dir<'static> = include_dir!("$FOGU_DESKTOP_PROGRAM");
/// A digest of everything embedded: names the extraction folder.
const HASH: &str = env!("FOGU_DESKTOP_HASH");
/// The frontend client, injected into every page before its scripts.
pub(crate) const CLIENT_JS: &str = include_str!("../../client/fogu.js");
/// The UI folder the `fogu://` protocol serves (bundle and dev modes).
static UI_DIR: OnceLock<PathBuf> = OnceLock::new();
/// The app's own icon (`icons/icon.png` or `icons/icon.ico` of the program),
/// given to every window and as the default tray icon.
static APP_ICON: OnceLock<tauri::image::Image<'static>> = OnceLock::new();

/// The app's icon, when the program has one.
pub(crate) fn app_icon() -> Option<tauri::image::Image<'static>> {
    APP_ICON.get().cloned()
}

/// The icon file of the program in `dir`: icons/icon.png, else icons/icon.ico.
fn icon_file(dir: &Path) -> Option<PathBuf> {
    ["icon.png", "icon.ico"].iter().map(|f| dir.join("icons").join(f)).find(|p| p.is_file())
}

/// Windows groups task bar buttons (and names them in the Task Manager's
/// "Apps") by AppUserModelID: the app's identifier, not the host's.
/// Set before the first window opens.
#[cfg(windows)]
fn set_app_user_model_id(id: &str) {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
    }
    // at most 128 characters, no spaces
    let id: String = id.chars().filter(|c| !c.is_whitespace()).take(128).collect();
    if id.is_empty() {
        return;
    }
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated UTF-16 string, copied by the call
    let hr = unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
    if hr < 0 {
        eprintln!("fogu-desktop: SetCurrentProcessExplicitAppUserModelID failed ({:#x})", hr);
    }
}

/// macOS: the Dock shows the icon of the .app bundle; a host run outside one
/// (`fogu desktop dev`, `dist/<name>`) would show the host's. On the main thread.
#[cfg(target_os = "macos")]
fn set_dock_icon(file: &Path) {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    type Id = *mut c_void;
    let Ok(bytes) = std::fs::read(file) else { return };
    // SAFETY: objc_msgSend called through the exact signature of each message;
    // the NSData copies the bytes; the NSImage is kept by NSApp (one reference leaks, once)
    unsafe {
        let send0: unsafe extern "C" fn(Id, Id) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send1: unsafe extern "C" fn(Id, Id, Id) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_bytes: unsafe extern "C" fn(Id, Id, *const c_void, usize) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let class = |n: &CStr| objc_getClass(n.as_ptr());
        let sel = |n: &CStr| sel_registerName(n.as_ptr());
        let data = send_bytes(class(c"NSData"), sel(c"dataWithBytes:length:"), bytes.as_ptr() as *const c_void, bytes.len());
        if data.is_null() {
            return;
        }
        let image = send1(send0(class(c"NSImage"), sel(c"alloc")), sel(c"initWithData:"), data);
        if image.is_null() {
            return;
        }
        let app = send0(class(c"NSApplication"), sel(c"sharedApplication"));
        if !app.is_null() {
            send1(app, sel(c"setApplicationIconImage:"), image);
        }
    }
}

#[tauri::command]
async fn fogu_call(bridge: State<'_, Arc<Bridge>>, name: String, payload: Option<Value>) -> Result<Value, String> {
    Arc::clone(&bridge).call(name, payload.unwrap_or(Value::Null)).await
}

#[tauri::command]
async fn fogu_emit(bridge: State<'_, Arc<Bridge>>, name: String, data: Option<Value>) -> Result<(), String> {
    Arc::clone(&bridge).emit_to_program(name, data.unwrap_or(Value::Null)).await
}

#[tauri::command]
fn fogu_ready(bridge: State<'_, Arc<Bridge>>, window: WebviewWindow) -> Value {
    bridge.page_ready(window.label())
}

/// `fogu.window.*` in the page: `op` on the window `target` (default: the
/// calling window). See window::page_op.
#[tauri::command]
async fn fogu_window(app: AppHandle, window: WebviewWindow, op: String, target: Option<String>, args: Option<Value>) -> Result<Value, String> {
    let label = target.unwrap_or_else(|| window.label().to_string());
    window::page_op(&app, &label, &op, args.unwrap_or(Value::Null))
}

/// Where the app comes from.
enum Source {
    /// A bundle appended to this executable (`fogu desktop build`): the
    /// extracted root and its entry file.
    Bundle { root: PathBuf, entry: PathBuf },
    /// A folder on disk (`fogu desktop dev`).
    Dir(PathBuf),
    /// Staged by build.rs and embedded in the executable.
    Embedded,
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn source() -> Result<Source, String> {
    // the program's .fg files are kept in memory (not extracted)
    if let Some(r) = fogu::toolkit::build::embedded_in_memory() {
        let e = r?;
        return Ok(Source::Bundle { root: e.root, entry: e.entry });
    }
    if let Some(dir) = env_nonempty("FOGU_DESKTOP_PROGRAM_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(format!("FOGU_DESKTOP_PROGRAM_DIR={} is not a folder", dir.display()));
        }
        return Ok(Source::Dir(dir));
    }
    let staged = PROGRAM.get_file(".fogu-desktop-entry").and_then(|f| f.contents_utf8()).unwrap_or("");
    if staged.trim().is_empty() {
        return Err("this is the generic Fogu desktop host: run an app with `fogu desktop dev`, or build one with `fogu desktop build`".into());
    }
    Ok(Source::Embedded)
}

/// fogu.json (the app's manifest), from the embedded program or a folder.
fn manifest_embedded() -> Value {
    PROGRAM
        .get_file("fogu.json")
        .and_then(|f| f.contents_utf8())
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or(Value::Null)
}

fn manifest_in(dir: &Path) -> Value {
    std::fs::read_to_string(dir.join("fogu.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// `.fogu-desktop.json`, written by build.fg next to the program it bundles:
/// `{ "ui": "<folder of the built UI>", "identifier": "…" }`.
fn bundle_info(root: &Path) -> Value {
    std::fs::read_to_string(root.join(".fogu-desktop.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// The app's identifier (names its data folders): "desktop.identifier" in
/// fogu.json, else dev.fogu.app.<name>.
fn identifier(man: &Value, info: &Value) -> String {
    if let Some(id) = info.get("identifier").and_then(Value::as_str) {
        return id.to_string();
    }
    if let Some(id) = man.pointer("/desktop/identifier").and_then(Value::as_str) {
        return id.to_string();
    }
    let name = man.get("name").and_then(Value::as_str).unwrap_or("app");
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    format!("dev.fogu.app.{}", slug)
}

/// Whether an embedded file stays in memory: the program's own sources.
fn in_memory(rel: &str) -> bool {
    rel.ends_with(".fg") && !rel.starts_with("fogu_modules/")
}

/// Every file of an embedded folder (relative path with '/', contents).
fn embedded_files<'a>(d: &'a Dir<'a>, out: &mut Vec<&'a include_dir::File<'a>>) {
    for f in d.files() {
        out.push(f);
    }
    for sub in d.dirs() {
        embedded_files(sub, out);
    }
}

/// Extracts the embedded program once per build (a new build gets a new
/// folder; old ones are removed): every file but its own `.fg` sources,
/// which are installed in memory over that folder, at every start.
fn extract(base: &Path) -> Result<PathBuf, String> {
    let dir = base.join(format!("program-{}", HASH));
    let mut files = Vec::new();
    embedded_files(&PROGRAM, &mut files);
    let rel = |f: &include_dir::File| f.path().to_string_lossy().replace('\\', "/");
    fogu::runtime::vfs::install_at(
        &dir,
        files
            .iter()
            .filter(|f| in_memory(&rel(f)))
            .map(|f| (rel(f), f.contents_utf8().unwrap_or_default().to_string())),
    );
    if dir.join(".fogu-desktop-entry").is_file() {
        return Ok(dir);
    }
    std::fs::create_dir_all(base).map_err(|e| format!("{}: {}", base.display(), e))?;
    let tmp = base.join(format!(".program-{}-{}", HASH, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {}", tmp.display(), e))?;
    for f in &files {
        let r = rel(f);
        if in_memory(&r) {
            continue;
        }
        let p = tmp.join(&r);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
        }
        std::fs::write(&p, f.contents()).map_err(|e| format!("cannot extract the program to {}: {}", p.display(), e))?;
    }
    if std::fs::rename(&tmp, &dir).is_err() {
        // another instance extracted it first
        let _ = std::fs::remove_dir_all(&tmp);
        if !dir.join(".fogu-desktop-entry").is_file() {
            return Err(format!("cannot extract the program to {}", dir.display()));
        }
    }
    if let Ok(rd) = std::fs::read_dir(base) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("program-") && e.path() != dir {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(dir)
}

// ---------- the fogu:// protocol: the UI from a folder ----------

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// `fogu://localhost/<path>` → `<UI_DIR>/<path>`; a path that is not a file
/// (a client-side route) gets index.html, as a static web server would.
fn serve_ui(request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let reply = |status: u16, ty: &str, body: Vec<u8>| {
        Response::builder()
            .status(status)
            .header("Content-Type", ty)
            .header("Cache-Control", "no-cache")
            .body(Cow::Owned(body))
            .unwrap()
    };
    let Some(root) = UI_DIR.get() else {
        return reply(500, "text/plain", b"no UI folder".to_vec());
    };
    let raw = request.uri().path();
    let decoded = percent_decode(raw.trim_start_matches('/'));
    let rel = if decoded.is_empty() { "index.html".to_string() } else { decoded };
    if rel.split('/').any(|c| c == "..") {
        return reply(403, "text/plain", b"forbidden".to_vec());
    }
    let mut file = root.join(&rel);
    if file.is_dir() {
        file = file.join("index.html");
    }
    if !file.is_file() && !rel.contains('.') {
        file = root.join("index.html");
    }
    match std::fs::read(&file) {
        Ok(bytes) => reply(200, mime(&file.to_string_lossy()), bytes),
        Err(_) => reply(404, "text/plain", format!("{} not found", rel).into_bytes()),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const UI_URL: &str = "fogu://localhost/index.html";

/// A startup failure also goes to <local data dir>/fogu-desktop.log (release
/// builds on Windows have no console).
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let r = start(app);
    if let Err(e) = &r {
        eprintln!("fogu-desktop: startup failed: {}", e);
        if let Ok(dir) = app.path().app_local_data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("fogu-desktop.log"), format!("startup failed: {}\n", e));
        }
    }
    r
}

fn start(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let paths = app.path();

    // the program folder, its manifest, the entry file, the data folders and the page
    let src = source()?;
    let source_kind = match &src {
        Source::Dir(_) => Kind::Dir,
        _ => Kind::Other,
    };
    let (man, program, entry, local, data, url, id) = match src {
        Source::Bundle { root, entry } => {
            let man = manifest_in(&root);
            let info = bundle_info(&root);
            let id = identifier(&man, &info);
            let ui = info.get("ui").and_then(Value::as_str).unwrap_or("ui");
            let ui_dir = root.join(ui);
            if !ui_dir.join("index.html").is_file() {
                return Err(format!("the bundle has no UI at {} (no index.html)", ui_dir.display()).into());
            }
            let _ = UI_DIR.set(ui_dir);
            let local = paths.local_data_dir()?.join(&id);
            let data = paths.data_dir()?.join(&id);
            window::set_base(window::Base::Fogu);
            (man, root, entry, local, data, WebviewUrl::CustomProtocol(UI_URL.parse()?), id)
        }
        Source::Dir(dir) => {
            let man = manifest_in(&dir);
            let id = identifier(&man, &Value::Null);
            let entry = man.get("entry").and_then(Value::as_str).unwrap_or("main.fg").to_string();
            let url = if let Some(dev) = env_nonempty("FOGU_DESKTOP_DEV_URL") {
                window::set_base(window::Base::Dev(dev.clone()));
                WebviewUrl::External(dev.parse()?)
            } else {
                let ui = env_nonempty("FOGU_DESKTOP_UI_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.join(man.pointer("/desktop/ui").and_then(Value::as_str).unwrap_or("ui")));
                if !ui.join("index.html").is_file() {
                    return Err(format!("{} has no index.html (set \"desktop\": {{ \"ui\": \"…\" }} in fogu.json, or FOGU_DESKTOP_UI_DIR)", ui.display()).into());
                }
                let _ = UI_DIR.set(ui);
                window::set_base(window::Base::Fogu);
                WebviewUrl::CustomProtocol(UI_URL.parse()?)
            };
            let local = paths.local_data_dir()?.join(&id);
            let data = paths.data_dir()?.join(&id);
            let entry = dir.join(entry);
            (man, dir, entry, local, data, url, id)
        }
        Source::Embedded => {
            let man = manifest_embedded();
            let local = paths.app_local_data_dir()?;
            let data = paths.app_data_dir()?;
            let program = extract(&local)?;
            let entry = std::fs::read_to_string(program.join(".fogu-desktop-entry")).unwrap_or_else(|_| "main.fg".into());
            let entry = program.join(entry.trim());
            window::set_base(window::Base::App);
            let id = handle.config().identifier.clone();
            (man, program, entry, local, data, WebviewUrl::App("index.html".into()), id)
        }
    };
    let cfg = man.get("desktop").cloned().unwrap_or(Value::Null);
    let _ = std::fs::create_dir_all(&local);
    let _ = std::fs::create_dir_all(&data);
    // desktop.keep / desktop.kept: state carried across hot reloads, not across runs
    let kept = local.join("kept.json");
    let _ = std::fs::remove_file(&kept);
    let reload = matches!(source_kind, Kind::Dir) && env_nonempty("FOGU_DESKTOP_RELOAD").is_some();

    // the runtime starts in the program's folder: imports, fogu.json and Fogu.asset resolve there
    std::env::set_current_dir(&program)?;
    // SAFETY: set before the Fogu runtime thread starts; nothing else reads them concurrently
    unsafe {
        std::env::set_var("FOGU_DESKTOP", "1");
        std::env::set_var("FOGU_DESKTOP_DATA_DIR", &data);
        std::env::set_var("FOGU_DESKTOP_PROGRAM_DIR", &program);
        std::env::set_var("FOGU_DESKTOP_KEPT", &kept);
    }
    let bridge = Bridge::start(handle.clone(), Some(local.join("fogu-desktop.log")))?;
    app.manage(Arc::clone(&bridge));
    // every window shares the webview's data (cookies, storage, cache), per app, not per host
    window::set_webview_data(local.join("webview"));
    // closeToTray / minimizeToTray from fogu.json
    window::configure_app(&cfg);
    // the app's identity for the system, before the first window: the task
    // bar's grouping (Windows), the windows' and the Dock's icon
    #[cfg(windows)]
    set_app_user_model_id(&id);
    #[cfg(not(windows))]
    let _ = &id;
    if let Some(file) = icon_file(&program) {
        match tauri::image::Image::from_path(&file) {
            Ok(img) => {
                let _ = APP_ICON.set(img.to_owned());
                #[cfg(target_os = "macos")]
                set_dock_icon(&file);
            }
            Err(e) => bridge.log(&format!("the app icon {}: {}", file.display(), e)),
        }
    }

    // the main window (settings from fogu.json "desktop"; desktop.window(...) changes them later)
    let title = cfg
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| man.get("name").and_then(Value::as_str))
        .unwrap_or("Fogu")
        .to_string();
    let mut opts = match &cfg {
        Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    opts.insert("title".into(), Value::String(title));
    window::build(&handle, "main", url, &Value::Object(opts))?;

    // the program's top level (it registers the commands) runs off the main thread;
    // calls made meanwhile wait for it in the runtime's queue
    let main = entry.to_string_lossy().to_string();
    if reload {
        // a plain UI folder (no dev server): its changes reload the pages
        let ui = if env_nonempty("FOGU_DESKTOP_DEV_URL").is_none() { UI_DIR.get().cloned() } else { None };
        watch_sources(Arc::clone(&bridge), handle.clone(), program.clone(), ui);
    }
    std::thread::Builder::new().name("fogu-main".into()).spawn(move || bridge.run_main(&handle, &main))?;
    Ok(())
}

enum Kind {
    Dir,
    Other,
}

/// The files under `dir` that `keep` accepts, with their modification times.
fn snapshot(dir: &Path, keep: &dyn Fn(&str) -> bool, out: &mut Vec<(PathBuf, std::time::SystemTime, u64)>, depth: usize) {
    const SKIP: &[&str] = &["node_modules", "target", "dist", ".git", ".desktop-data", ".fogu"];
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            if depth < 12 && !SKIP.contains(&name.as_str()) && !name.starts_with('.') {
                snapshot(&p, keep, out, depth + 1);
            }
        } else if keep(&name) {
            out.push((p, meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len()));
        }
    }
}

/// `fogu desktop dev`: reloads the program when one of its sources changes,
/// and the pages when a file of a plain UI folder changes.
fn watch_sources(bridge: Arc<Bridge>, app: AppHandle, dir: PathBuf, ui: Option<PathBuf>) {
    let ui_abs = ui.as_ref().and_then(|u| u.canonicalize().ok());
    let sources = move || {
        let mut v = Vec::new();
        snapshot(&dir, &|n: &str| n.ends_with(".fg") || n == "fogu.json", &mut v, 0);
        v.sort();
        v
    };
    let pages = move || {
        let mut v = Vec::new();
        if let Some(u) = &ui_abs {
            snapshot(u, &|_: &str| true, &mut v, 0);
        }
        v.sort();
        v
    };
    let _ = std::thread::Builder::new().name("fogu-watch".into()).spawn(move || {
        let (mut last, mut last_ui) = (sources(), pages());
        loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let (now, now_ui) = (sources(), pages());
            if now != last || now_ui != last_ui {
                // let an editor finish writing
                std::thread::sleep(std::time::Duration::from_millis(120));
                let program_changed = now != last;
                last = sources();
                last_ui = pages();
                if program_changed {
                    bridge.reload(&app);
                } else {
                    bridge.log("UI changed: reloading the pages");
                    for w in app.webview_windows().values() {
                        let _ = w.eval("location.reload()");
                    }
                }
            }
        }
    });
}

/// Windows, GNU toolchain: WebView2Loader.dll is delay-loaded (build.rs) and
/// embedded here, so the executable needs nothing next to it. Before the first
/// WebView2 call, load the copy beside the executable or, when there is none,
/// the embedded one extracted to %LOCALAPPDATA%\fogu\desktop\webview2\<hash>.
/// The delay-load helper then finds the module already loaded.
#[cfg(all(windows, target_env = "gnu"))]
mod webview2_loader {
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    static DLL: &[u8] = include_bytes!(env!("FOGU_WEBVIEW2_LOADER"));

    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
    }

    fn load(path: &std::path::Path) -> bool {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated path; the module stays loaded for the process
        !unsafe { LoadLibraryW(wide.as_ptr()) }.is_null()
    }

    pub fn ensure() {
        let exe = std::env::current_exe().ok();
        if let Some(dir) = exe.as_ref().and_then(|e| e.parent()) {
            let beside = dir.join("WebView2Loader.dll");
            if beside.is_file() && load(&beside) {
                return;
            }
        }
        // FNV-1a of the DLL names the folder: a new loader never overwrites one in use
        let hash = DLL.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join("fogu").join("desktop").join("webview2").join(format!("{:016x}", hash));
        let path = dir.join("WebView2Loader.dll");
        let current = std::fs::metadata(&path).map(|m| m.len() == DLL.len() as u64).unwrap_or(false);
        if !current {
            let _ = std::fs::create_dir_all(&dir);
            let tmp = dir.join(format!("WebView2Loader.{}.tmp", std::process::id()));
            if std::fs::write(&tmp, DLL).is_ok() && std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        if !load(&path) {
            eprintln!("fogu-desktop: could not load {}", path.display());
        }
    }
}


/// The application: the desktop executable's main (main.rs) and the mobile
/// entry point (`fogu desktop build --android/--ios` links this crate as a library).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ssh (fogu:ssh with a password) runs this executable as its SSH_ASKPASS
    // program: answer and exit before anything else, or every prompt would
    // start the app again (which would connect again…)
    #[cfg(feature = "ops")]
    if let Some(code) = fogu::native::ssh::askpass_reply() {
        std::process::exit(code);
    }
    #[cfg(all(windows, target_env = "gnu"))]
    webview2_loader::ensure();
    update::cleanup();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .register_uri_scheme_protocol("fogu", |_ctx, request| serve_ui(&request))
        .invoke_handler(tauri::generate_handler![fogu_call, fogu_emit, fogu_ready, fogu_window])
        .on_window_event(window::on_event)
        .setup(setup);
    // tray icons and their menus
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .on_menu_event(tray::on_menu_event)
        .on_tray_icon_event(tray::on_tray_event);
    if let Err(e) = builder.run(tauri::generate_context!()) {
        eprintln!("fogu-desktop: {}", e);
        std::process::exit(1);
    }
}
