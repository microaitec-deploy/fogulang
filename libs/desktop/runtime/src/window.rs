//! Windows: building them (fogu.json `desktop`, `desktop.createWindow`),
//! changing them (`desktop.window(...)`, window handles in Fogu,
//! `fogu.window.*` in the page), their events (resized, moved, focus,
//! closeRequested…), and the requests of the program that need an answer
//! (window state, dialogs).
//!
//! Threads: `apply` and `request` run on the dispatcher or a request thread
//! (Tauri forwards the calls to the main thread); `on_event` runs on the main
//! thread and never waits for the Fogu program (it only queues messages).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use serde_json::{json, Value};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::bridge::Bridge;

/// Where the pages of the app come from: new windows open a route under it.
pub enum Base {
    /// The UI folder over the host's `fogu://` protocol (bundle and dev without Vite).
    Fogu,
    /// The Vite dev server (`fogu desktop dev`).
    Dev(String),
    /// Tauri's embedded assets (`--installer` and mobile builds).
    App,
}

static BASE: OnceLock<Base> = OnceLock::new();
static WEBVIEW_DATA: OnceLock<PathBuf> = OnceLock::new();

pub fn set_base(b: Base) {
    let _ = BASE.set(b);
}

pub fn set_webview_data(dir: PathBuf) {
    let _ = WEBVIEW_DATA.set(dir);
}

/// Per window: its minimum and maximum inner size (logical), to clamp the
/// sizes set from code (Windows does not apply them to `set_size`).
#[derive(Clone, Copy, Default)]
struct Limits {
    min: Option<(f64, f64)>,
    max: Option<(f64, f64)>,
}

static LIMITS: LazyLock<Mutex<HashMap<String, Limits>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn limits(label: &str) -> Limits {
    LIMITS.lock().unwrap().get(label).copied().unwrap_or_default()
}

/// Records the limits a message sets (`minWidth`…, `minSize: null`…).
fn record_limits(label: &str, o: &Value) {
    let mut all = LIMITS.lock().unwrap();
    let l = all.entry(label.to_string()).or_default();
    if o.get("minSize") == Some(&Value::Null) {
        l.min = None;
    }
    if o.get("maxSize") == Some(&Value::Null) {
        l.max = None;
    }
    if num(o, "minWidth").is_some() || num(o, "minHeight").is_some() {
        l.min = Some((num(o, "minWidth").unwrap_or(0.0), num(o, "minHeight").unwrap_or(0.0)));
    }
    if num(o, "maxWidth").is_some() || num(o, "maxHeight").is_some() {
        l.max = Some((num(o, "maxWidth").unwrap_or(1.0e6), num(o, "maxHeight").unwrap_or(1.0e6)));
    }
}

/// A size inside the window's limits.
pub fn clamp_size(label: &str, width: f64, height: f64) -> (f64, f64) {
    let l = limits(label);
    let (mut w, mut h) = (width, height);
    if let Some((mw, mh)) = l.max {
        w = w.min(mw);
        h = h.min(mh);
    }
    if let Some((mw, mh)) = l.min {
        w = w.max(mw);
        h = h.max(mh);
    }
    (w, h)
}

/// The opacity of a whole window, 0.0–1.0 (native calls: Tauri 2 has none).
#[cfg(windows)]
pub fn set_opacity(w: &WebviewWindow, alpha: f64) -> Result<(), String> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED};
    let hwnd = w.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    // SAFETY: a live top-level window handle of this process
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & WS_EX_LAYERED as isize == 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
        }
        if SetLayeredWindowAttributes(hwnd, 0, a, LWA_ALPHA) == 0 {
            return Err("desktop: SetLayeredWindowAttributes failed".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_opacity(w: &WebviewWindow, alpha: f64) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let ns = w.ns_window().map_err(|e| e.to_string())? as *mut AnyObject;
    let a = alpha.clamp(0.0, 1.0);
    let (tx, rx) = std::sync::mpsc::channel();
    let ptr = ns as usize;
    // AppKit: on the main thread
    w.run_on_main_thread(move || {
        // SAFETY: the NSWindow of a live window; setAlphaValue: takes a CGFloat
        let _: () = unsafe { msg_send![ptr as *mut AnyObject, setAlphaValue: a] };
        let _ = tx.send(());
    })
    .map_err(|e| e.to_string())?;
    let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn set_opacity(w: &WebviewWindow, alpha: f64) -> Result<(), String> {
    use gtk::prelude::WidgetExt;
    let a = alpha.clamp(0.0, 1.0);
    let w2 = w.clone();
    w.run_on_main_thread(move || {
        if let Ok(g) = w2.gtk_window() {
            g.set_opacity(a);
        }
    })
    .map_err(|e| e.to_string())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn set_opacity(_w: &WebviewWindow, _alpha: f64) -> Result<(), String> {
    Err("desktop: window opacity is not supported on this platform".into())
}

/// Who wants which window events, and who decides when a window may close.
#[derive(Default)]
struct Watch {
    /// event types the program listens to (`"*"`: all)
    program: HashSet<String>,
    /// event types the pages listen to
    page: HashSet<String>,
    /// windows whose close the program decides (desktop … onCloseRequested)
    close_program: HashSet<String>,
    /// windows whose close their page decides (fogu.window.onCloseRequested)
    close_page: HashSet<String>,
}

static WATCH: LazyLock<Mutex<Watch>> = LazyLock::new(|| Mutex::new(Watch::default()));
/// Closing the main window hides it while a tray icon exists.
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);
/// Minimizing the main window hides it while a tray icon exists.
static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(false);

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn flag(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(Value::as_bool)
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(String::from)
}

/// fogu.json `desktop`: `closeToTray`, `minimizeToTray`.
pub fn configure_app(cfg: &Value) {
    if let Some(b) = flag(cfg, "closeToTray") {
        CLOSE_TO_TRAY.store(b, Ordering::Relaxed);
    }
    if let Some(b) = flag(cfg, "minimizeToTray") {
        MINIMIZE_TO_TRAY.store(b, Ordering::Relaxed);
    }
}

fn get(app: &AppHandle, label: &str) -> Result<WebviewWindow, String> {
    app.get_webview_window(label).ok_or_else(|| format!("desktop: no window '{}'", label))
}

/// The label of a message (`window` key), default "main".
fn label_of(o: &Value) -> String {
    text(o, "window").unwrap_or_else(|| "main".to_string())
}

// ---------- building ----------

/// A page of the app for a new window: a route of the UI ("settings.html",
/// "/settings", "#/about"), or an absolute http(s) URL.
pub fn url_for(route: &str) -> Result<WebviewUrl, String> {
    let route = route.trim();
    if route.starts_with("http://") || route.starts_with("https://") {
        return route.parse().map(WebviewUrl::External).map_err(|e| format!("{}: {}", route, e));
    }
    let rel = route.trim_start_matches('/');
    let rel = if rel.is_empty() || rel.starts_with('#') || rel.starts_with('?') {
        format!("index.html{}", rel)
    } else {
        rel.to_string()
    };
    match BASE.get().unwrap_or(&Base::Fogu) {
        Base::Fogu => format!("fogu://localhost/{}", rel).parse().map(WebviewUrl::CustomProtocol).map_err(|e| format!("{}: {}", rel, e)),
        Base::Dev(base) => {
            // the dev server serves index.html for client-side routes itself
            let rel = rel.strip_prefix("index.html").map(|r| r.to_string()).unwrap_or(rel);
            format!("{}/{}", base.trim_end_matches('/'), rel)
                .parse()
                .map(WebviewUrl::External)
                .map_err(|e| format!("{}: {}", rel, e))
        }
        Base::App => Ok(WebviewUrl::App(rel.into())),
    }
}

/// Builds a window with the page API injected. `opts`: the keys of fogu.json
/// `desktop` (title, width, height, minWidth, minHeight, maxWidth, maxHeight,
/// x, y, center, resizable, maximized, fullscreen, alwaysOnTop, decorations,
/// transparent, visible, focus, skipTaskbar, shadow, closable, minimizable,
/// maximizable, contentProtected).
pub fn build(app: &AppHandle, label: &str, url: WebviewUrl, opts: &Value) -> Result<WebviewWindow, String> {
    let bridge = app.try_state::<Arc<Bridge>>().map(|b| Arc::clone(b.inner()));
    let mut w = WebviewWindowBuilder::new(app, label, url)
        .initialization_script(crate::CLIENT_JS)
        .on_page_load(move |w, p| {
            if p.event() == PageLoadEvent::Started {
                if let Some(b) = &bridge {
                    b.page_loading(w.label());
                }
            }
        });
    if let Some(dir) = WEBVIEW_DATA.get() {
        w = w.data_directory(dir.clone());
    }
    #[cfg(desktop)]
    {
        w = configure(w, opts);
    }
    #[cfg(not(desktop))]
    let _ = opts;
    record_limits(label, opts);
    let win = w.build().map_err(|e| format!("cannot open the window '{}': {}", label, e))?;
    #[cfg(desktop)]
    if let Some(a) = num(opts, "opacity") {
        if let Err(e) = set_opacity(&win, a) {
            eprintln!("fogu-desktop: {}", e);
        }
    }
    Ok(win)
}

#[cfg(desktop)]
fn configure<'a>(
    mut w: WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
    o: &Value,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    if let Some(t) = text(o, "title") {
        w = w.title(t);
    }
    w = w.inner_size(num(o, "width").unwrap_or(960.0), num(o, "height").unwrap_or(680.0));
    if num(o, "minWidth").is_some() || num(o, "minHeight").is_some() {
        w = w.min_inner_size(num(o, "minWidth").unwrap_or(0.0), num(o, "minHeight").unwrap_or(0.0));
    }
    if num(o, "maxWidth").is_some() || num(o, "maxHeight").is_some() {
        w = w.max_inner_size(num(o, "maxWidth").unwrap_or(1.0e6), num(o, "maxHeight").unwrap_or(1.0e6));
    }
    match (num(o, "x"), num(o, "y")) {
        (Some(x), Some(y)) => w = w.position(x, y),
        _ => {
            if flag(o, "center").unwrap_or(true) {
                w = w.center();
            }
        }
    }
    w = w.resizable(flag(o, "resizable").unwrap_or(true));
    if flag(o, "maximized") == Some(true) {
        w = w.maximized(true);
    }
    if flag(o, "fullscreen") == Some(true) {
        w = w.fullscreen(true);
    }
    if flag(o, "alwaysOnTop") == Some(true) {
        w = w.always_on_top(true);
    }
    if flag(o, "decorations") == Some(false) {
        w = w.decorations(false);
    }
    // macOS: through Tauri's macos-private-api (the host's default feature)
    #[cfg(any(not(target_os = "macos"), feature = "macos-private-api"))]
    if flag(o, "transparent") == Some(true) {
        w = w.transparent(true);
    }
    if flag(o, "visible") == Some(false) {
        w = w.visible(false);
    }
    if let Some(b) = flag(o, "focus") {
        w = w.focused(b);
    }
    if let Some(b) = flag(o, "skipTaskbar") {
        w = w.skip_taskbar(b);
    }
    if let Some(b) = flag(o, "shadow") {
        w = w.shadow(b);
    }
    if let Some(b) = flag(o, "closable") {
        w = w.closable(b);
    }
    if let Some(b) = flag(o, "minimizable") {
        w = w.minimizable(b);
    }
    if let Some(b) = flag(o, "maximizable") {
        w = w.maximizable(b);
    }
    if let Some(b) = flag(o, "contentProtected") {
        w = w.content_protected(b);
    }
    w
}

/// `desktop.createWindow(label, { url, …options })` → its state.
fn create(app: &AppHandle, a: &Value) -> Result<Value, String> {
    let label = text(a, "label").ok_or("desktop.createWindow: a label is required")?;
    if label.is_empty() || !label.chars().all(|c| c.is_ascii_alphanumeric() || "-/:_".contains(c)) {
        return Err(format!("desktop.createWindow: invalid label '{}' (letters, digits, '-', '/', ':', '_')", label));
    }
    if let Some(w) = app.get_webview_window(&label) {
        // already open: bring it to the front
        #[cfg(desktop)]
        {
            let _ = w.show();
            let _ = w.set_focus();
        }
        return state(&w);
    }
    let url = url_for(&text(a, "url").unwrap_or_default())?;
    let mut opts = a.clone();
    if text(a, "title").is_none() {
        if let (Some(m), Some(main)) = (opts.as_object_mut(), app.get_webview_window("main")) {
            m.insert("title".into(), Value::String(main.title().unwrap_or_default()));
        }
    }
    let w = build(app, &label, url, &opts)?;
    state(&w)
}

// ---------- state ----------

/// `{ label, title, width, height, x, y, maximized, minimized, fullscreen,
/// visible, focused, decorated, resizable, alwaysOnTop, scaleFactor }`
/// (logical pixels).
pub fn state(w: &WebviewWindow) -> Result<Value, String> {
    let f = w.scale_factor().map_err(|e| e.to_string())?;
    let s = w.inner_size().map_err(|e| e.to_string())?.to_logical::<f64>(f);
    let p = w.outer_position().map(|p| p.to_logical::<f64>(f)).ok();
    #[allow(unused_mut)]
    let mut v = json!({
        "label": w.label(),
        "title": w.title().unwrap_or_default(),
        "width": s.width,
        "height": s.height,
        "x": p.map(|p| p.x),
        "y": p.map(|p| p.y),
        "maximized": w.is_maximized().unwrap_or(false),
        "minimized": w.is_minimized().unwrap_or(false),
        "fullscreen": w.is_fullscreen().unwrap_or(false),
        "visible": w.is_visible().unwrap_or(true),
        "focused": w.is_focused().unwrap_or(false),
        "decorated": w.is_decorated().unwrap_or(true),
        "resizable": w.is_resizable().unwrap_or(true),
        "scaleFactor": f,
    });
    #[cfg(desktop)]
    {
        v["alwaysOnTop"] = Value::Bool(w.is_always_on_top().unwrap_or(false));
    }
    Ok(v)
}

// ---------- changing ----------

/// `desktop.window({ window, …options, action })`: changes a window (default
/// "main"). Options: title width height x y minWidth minHeight maxWidth
/// maxHeight resizable fullscreen maximized minimized alwaysOnTop center
/// visible focus decorations shadow skipTaskbar closable minimizable
/// maximizable contentProtected; `minSize: null` / `maxSize: null` remove a
/// limit. `action`: minimize unminimize maximize unmaximize toggleMaximize
/// center show hide focus close destroy startDragging toggleFullscreen.
/// Global: closeToTray, minimizeToTray.
#[cfg(desktop)]
pub fn apply(app: &AppHandle, o: &Value) -> Result<(), String> {
    use tauri::{LogicalPosition, LogicalSize};
    if let Some(b) = flag(o, "closeToTray") {
        CLOSE_TO_TRAY.store(b, Ordering::Relaxed);
    }
    if let Some(b) = flag(o, "minimizeToTray") {
        MINIMIZE_TO_TRAY.store(b, Ordering::Relaxed);
    }
    let label = label_of(o);
    let w = get(app, &label)?;
    record_limits(&label, o);
    let e = |r: tauri::Result<()>| r.map_err(|e| e.to_string());
    if let Some(t) = text(o, "title") {
        e(w.set_title(&t))?;
    }
    if let Some(b) = flag(o, "decorations") {
        e(w.set_decorations(b))?;
    }
    if let Some(b) = flag(o, "shadow") {
        e(w.set_shadow(b))?;
    }
    if let Some(b) = flag(o, "resizable") {
        e(w.set_resizable(b))?;
    }
    if o.get("minSize") == Some(&Value::Null) {
        e(w.set_min_size(None::<LogicalSize<f64>>))?;
    }
    if o.get("maxSize") == Some(&Value::Null) {
        e(w.set_max_size(None::<LogicalSize<f64>>))?;
    }
    if num(o, "minWidth").is_some() || num(o, "minHeight").is_some() {
        e(w.set_min_size(Some(LogicalSize::new(num(o, "minWidth").unwrap_or(0.0), num(o, "minHeight").unwrap_or(0.0)))))?;
    }
    if num(o, "maxWidth").is_some() || num(o, "maxHeight").is_some() {
        e(w.set_max_size(Some(LogicalSize::new(num(o, "maxWidth").unwrap_or(1.0e6), num(o, "maxHeight").unwrap_or(1.0e6)))))?;
    }
    if num(o, "width").is_some() || num(o, "height").is_some() {
        let f = w.scale_factor().unwrap_or(1.0);
        let cur = w.inner_size().ok().map(|s| s.to_logical::<f64>(f));
        let width = num(o, "width").or(cur.map(|c| c.width)).unwrap_or(800.0);
        let height = num(o, "height").or(cur.map(|c| c.height)).unwrap_or(600.0);
        // inside minWidth…maxHeight on every OS (Windows only enforces them on the user's resizing)
        let (width, height) = clamp_size(&label, width, height);
        e(w.set_size(LogicalSize::new(width, height)))?;
    }
    if num(o, "x").is_some() || num(o, "y").is_some() {
        let f = w.scale_factor().unwrap_or(1.0);
        let cur = w.outer_position().ok().map(|p| p.to_logical::<f64>(f));
        let x = num(o, "x").or(cur.map(|c| c.x)).unwrap_or(0.0);
        let y = num(o, "y").or(cur.map(|c| c.y)).unwrap_or(0.0);
        e(w.set_position(LogicalPosition::new(x, y)))?;
    }
    if let Some(b) = flag(o, "alwaysOnTop") {
        e(w.set_always_on_top(b))?;
    }
    if let Some(b) = flag(o, "skipTaskbar") {
        e(w.set_skip_taskbar(b))?;
    }
    if let Some(b) = flag(o, "closable") {
        e(w.set_closable(b))?;
    }
    if let Some(b) = flag(o, "minimizable") {
        e(w.set_minimizable(b))?;
    }
    if let Some(b) = flag(o, "maximizable") {
        e(w.set_maximizable(b))?;
    }
    if let Some(b) = flag(o, "contentProtected") {
        e(w.set_content_protected(b))?;
    }
    if let Some(a) = num(o, "opacity") {
        set_opacity(&w, a)?;
    }
    if let Some(b) = flag(o, "fullscreen") {
        e(w.set_fullscreen(b))?;
    }
    match flag(o, "maximized") {
        Some(true) => e(w.maximize())?,
        Some(false) => e(w.unmaximize())?,
        None => {}
    }
    match flag(o, "minimized") {
        Some(true) => e(w.minimize())?,
        Some(false) => e(w.unminimize())?,
        None => {}
    }
    if flag(o, "center") == Some(true) {
        e(w.center())?;
    }
    match flag(o, "visible") {
        Some(true) => e(w.show())?,
        Some(false) => e(w.hide())?,
        None => {}
    }
    if flag(o, "focus") == Some(true) {
        e(w.set_focus())?;
    }
    if let Some(a) = text(o, "action") {
        action(&w, &a)?;
    }
    Ok(())
}

#[cfg(not(desktop))]
pub fn apply(_app: &AppHandle, _o: &Value) -> Result<(), String> {
    Err("desktop: changing windows is not supported on mobile".into())
}

#[cfg(desktop)]
pub fn action(w: &WebviewWindow, a: &str) -> Result<(), String> {
    let r = match a {
        "minimize" => w.minimize(),
        "unminimize" => w.unminimize(),
        "maximize" => w.maximize(),
        "unmaximize" => w.unmaximize(),
        "toggleMaximize" => {
            if w.is_maximized().unwrap_or(false) {
                w.unmaximize()
            } else {
                w.maximize()
            }
        }
        "toggleFullscreen" => w.set_fullscreen(!w.is_fullscreen().unwrap_or(false)),
        "center" => w.center(),
        "show" => w.show().and_then(|_| w.unminimize()).and_then(|_| w.set_focus()),
        "hide" => w.hide(),
        "focus" => w.set_focus(),
        // close: goes through closeRequested (and the handlers that may keep it open)
        "close" => w.close(),
        // destroy: closes without asking
        "destroy" => w.destroy(),
        "startDragging" => w.start_dragging(),
        "reload" => w.reload(),
        other => return Err(format!("desktop: unknown window action '{}'", other)),
    };
    r.map_err(|e| e.to_string())
}

/// Shows, unminimizes and focuses the main window (tray "show").
#[cfg(desktop)]
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Hides the main window, or shows it when it is hidden or minimized (tray "toggle").
#[cfg(desktop)]
pub fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

/// `desktop:watch` from the program: `{ listen: type }`, `{ unlisten: type }`,
/// `{ intercept: label }`, `{ release: label }`.
pub fn watch_program(o: &Value) {
    let mut w = WATCH.lock().unwrap();
    if let Some(t) = text(o, "listen") {
        w.program.insert(t);
    }
    if let Some(t) = text(o, "unlisten") {
        w.program.remove(&t);
    }
    if let Some(l) = text(o, "intercept") {
        w.close_program.insert(l);
    }
    if let Some(l) = text(o, "release") {
        w.close_program.remove(&l);
    }
}

/// A hot reload: the new program registers its own window listeners and close handlers.
pub fn reset_program() {
    let mut w = WATCH.lock().unwrap();
    w.program.clear();
    w.close_program.clear();
}

// ---------- events ----------

/// Every window event (main thread): forwarded as `desktop:window-event`
/// (program) and the page event `desktop:window`, both
/// `{ window, type, …data }`, to whoever listens to that type.
pub fn on_event(window: &tauri::Window, ev: &WindowEvent) {
    let label = window.label().to_string();
    let app = window.app_handle().clone();
    let Some(bridge) = app.try_state::<Arc<Bridge>>().map(|b| Arc::clone(b.inner())) else { return };
    let (kind, mut data) = match ev {
        WindowEvent::Resized(s) => {
            let f = window.scale_factor().unwrap_or(1.0);
            let l = s.to_logical::<f64>(f);
            #[cfg(desktop)]
            if label == "main" && MINIMIZE_TO_TRAY.load(Ordering::Relaxed) && crate::tray::any(&app) && window.is_minimized().unwrap_or(false) {
                let _ = window.hide();
            }
            ("resized", json!({ "width": l.width, "height": l.height }))
        }
        WindowEvent::Moved(p) => {
            let f = window.scale_factor().unwrap_or(1.0);
            let l = p.to_logical::<f64>(f);
            ("moved", json!({ "x": l.x, "y": l.y }))
        }
        WindowEvent::Focused(true) => ("focus", json!({})),
        WindowEvent::Focused(false) => ("blur", json!({})),
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => ("scaleChanged", json!({ "scaleFactor": scale_factor })),
        WindowEvent::ThemeChanged(t) => ("themeChanged", json!({ "theme": t.to_string() })),
        WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
            ("fileDrop", json!({ "paths": paths.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>() }))
        }
        WindowEvent::Destroyed => {
            bridge.forget_window(&label);
            ("destroyed", json!({}))
        }
        WindowEvent::CloseRequested { api, .. } => {
            let (program, page) = {
                let w = WATCH.lock().unwrap();
                (w.close_program.contains(&label), w.close_page.contains(&label))
            };
            let mut prevented = false;
            if program || page {
                // the handler decides; it closes the window with destroy()
                api.prevent_close();
                prevented = true;
            } else {
                #[cfg(desktop)]
                if label == "main" && CLOSE_TO_TRAY.load(Ordering::Relaxed) && crate::tray::any(&app) {
                    api.prevent_close();
                    let _ = window.hide();
                    prevented = true;
                }
            }
            let decide = if program {
                "program"
            } else if page {
                "page"
            } else {
                "none"
            };
            ("closeRequested", json!({ "prevented": prevented, "decides": decide }))
        }
        _ => return,
    };
    data["window"] = Value::String(label.clone());
    data["type"] = Value::String(kind.to_string());
    let (to_program, to_page) = {
        let w = WATCH.lock().unwrap();
        let wants = |set: &HashSet<String>| set.contains(kind) || set.contains("*");
        let decides = data["decides"].as_str().unwrap_or("");
        (wants(&w.program) || decides == "program", wants(&w.page) || decides == "page")
    };
    if to_program {
        bridge.notify("desktop:window-event", data.clone());
    }
    if to_page {
        bridge.send_event(&app, "desktop:window".to_string(), data, None);
    }
}

// ---------- the page's fogu.window ----------

/// `fogu.window.*` (lib.rs `fogu_window`): `op` on the window `label`.
pub fn page_op(app: &AppHandle, label: &str, op: &str, args: Value) -> Result<Value, String> {
    match op {
        "notify" => notify(app, &args),
        #[cfg(desktop)]
        "menu" => crate::menu::set(app, &args).map(|_| Value::Null),
        #[cfg(desktop)]
        "accelerators" => Ok(crate::tray::accelerators()),
        // an accelerator the page caught: { menu, item }
        #[cfg(desktop)]
        "menuItem" => {
            let menu = text(&args, "menu").unwrap_or_else(|| "app".into());
            let item = text(&args, "item").ok_or("fogu: menuItem needs an item")?;
            crate::tray::fire(app, &format!("menu:{}", menu), &item);
            Ok(Value::Null)
        }
        #[cfg(desktop)]
        "shortcut" => {
            let sc = text(&args, "shortcut");
            if flag(&args, "off") == Some(true) {
                crate::shortcut::unregister(app, sc.as_deref()).map(|_| Value::Null)
            } else {
                crate::shortcut::register(app, &sc.ok_or("fogu.shortcut: no shortcut")?).map(|_| Value::Null)
            }
        }
        "set" => {
            let mut o = if args.is_object() { args } else { json!({}) };
            o["window"] = Value::String(label.to_string());
            apply(app, &o)?;
            Ok(Value::Null)
        }
        "get" => state(&get(app, label)?),
        "list" => Ok(labels(app)),
        "create" => create(app, &args),
        "listen" | "unlisten" => {
            let mut w = WATCH.lock().unwrap();
            for t in args.get("events").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if op == "listen" {
                    w.page.insert(t.to_string());
                } else {
                    w.page.remove(t);
                }
            }
            Ok(Value::Null)
        }
        "interceptClose" => {
            let mut w = WATCH.lock().unwrap();
            if flag(&args, "on").unwrap_or(true) {
                w.close_page.insert(label.to_string());
            } else {
                w.close_page.remove(label);
            }
            Ok(Value::Null)
        }
        #[cfg(desktop)]
        "tray" => crate::tray::page_op(app, &args),
        other => Err(format!("desktop: unknown window operation '{}'", other)),
    }
}

fn labels(app: &AppHandle) -> Value {
    let mut l: Vec<String> = app.webview_windows().keys().cloned().collect();
    l.sort();
    json!(l)
}

// ---------- notifications ----------

/// `desktop.notify({ title, body, icon })`: a system notification.
pub fn notify(app: &AppHandle, a: &Value) -> Result<Value, String> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    let n = app.notification();
    if !matches!(n.permission_state(), Ok(PermissionState::Granted)) {
        match n.request_permission() {
            Ok(PermissionState::Granted) => {}
            Ok(_) => return Err("desktop.notify: notifications are not allowed for this app".into()),
            Err(e) => return Err(format!("desktop.notify: {}", e)),
        }
    }
    let mut b = n.builder();
    b = b.title(text(a, "title").unwrap_or_else(|| app.package_info().name.clone()));
    if let Some(t) = text(a, "body") {
        b = b.body(t);
    }
    if let Some(i) = text(a, "icon") {
        b = b.icon(i);
    }
    b.show().map_err(|e| format!("desktop.notify: {}", e))?;
    Ok(Value::Bool(true))
}

// ---------- requests of the program (answered on their own thread) ----------

/// Answers a `desktop:request` (on its own thread: the dialogs block it).
pub fn request(app: &AppHandle, op: &str, a: &Value) -> Result<Value, String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    match op {
        "window.get" => state(&get(app, &label_of(a))?),
        "window.list" => Ok(labels(app)),
        "window.create" => create(app, a),
        "window.set" => {
            apply(app, a)?;
            Ok(Value::Null)
        }
        #[cfg(desktop)]
        "tray.set" => crate::tray::set(app, a).map(|_| Value::Null),
        #[cfg(desktop)]
        "tray.remove" => Ok(Value::Bool(crate::tray::remove(app, &text(a, "id").unwrap_or_else(|| "main".into())))),
        #[cfg(not(desktop))]
        "tray.set" | "tray.remove" => Err("desktop: tray icons are not supported on mobile".into()),
        #[cfg(desktop)]
        "menu.set" => crate::menu::set(app, a).map(|_| Value::Null),
        #[cfg(desktop)]
        "shortcut.register" => crate::shortcut::register(app, &text(a, "shortcut").ok_or("desktop.shortcut: no shortcut")?).map(|_| Value::Null),
        #[cfg(desktop)]
        "shortcut.unregister" => crate::shortcut::unregister(app, text(a, "shortcut").as_deref()).map(|_| Value::Null),
        #[cfg(not(desktop))]
        "menu.set" | "shortcut.register" | "shortcut.unregister" => Err(format!("desktop: {} is not supported on mobile", op)),
        "notify" => notify(app, a),
        "update.apply" => crate::update::apply(app, a),
        "dialog.open" | "dialog.save" => {
            let mut d = app.dialog().file();
            #[cfg(desktop)]
            if let Some(w) = app.get_webview_window(&label_of(a)) {
                d = d.set_parent(&w);
            }
            if let Some(t) = text(a, "title") {
                d = d.set_title(t);
            }
            if let Some(dir) = text(a, "directory").or_else(|| text(a, "defaultPath")) {
                d = d.set_directory(dir);
            }
            if let Some(n) = text(a, "fileName") {
                d = d.set_file_name(n);
            }
            // filters: { "Images": ["png", "jpg"] } or [{ name, extensions }]
            match a.get("filters") {
                Some(Value::Object(m)) => {
                    for (name, exts) in m {
                        let exts: Vec<&str> = exts.as_array().map(|l| l.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                        d = d.add_filter(name, &exts);
                    }
                }
                Some(Value::Array(l)) => {
                    for f in l {
                        let name = text(f, "name").unwrap_or_default();
                        let exts: Vec<&str> = f
                            .get("extensions")
                            .and_then(Value::as_array)
                            .map(|l| l.iter().filter_map(Value::as_str).collect())
                            .unwrap_or_default();
                        d = d.add_filter(name, &exts);
                    }
                }
                _ => {}
            }
            let path = |p: tauri_plugin_dialog::FilePath| Value::String(p.to_string());
            if op == "dialog.save" {
                return Ok(d.blocking_save_file().map(path).unwrap_or(Value::Null));
            }
            let multiple = flag(a, "multiple").unwrap_or(false);
            let folder = flag(a, "folder").unwrap_or(false);
            #[cfg(desktop)]
            if folder {
                return Ok(if multiple {
                    d.blocking_pick_folders().map(|l| Value::Array(l.into_iter().map(path).collect())).unwrap_or(Value::Null)
                } else {
                    d.blocking_pick_folder().map(path).unwrap_or(Value::Null)
                });
            }
            #[cfg(not(desktop))]
            if folder {
                return Err("desktop: folder dialogs are not supported on mobile".into());
            }
            Ok(if multiple {
                d.blocking_pick_files().map(|l| Value::Array(l.into_iter().map(path).collect())).unwrap_or(Value::Null)
            } else {
                d.blocking_pick_file().map(path).unwrap_or(Value::Null)
            })
        }
        "dialog.message" | "dialog.confirm" => {
            let msg = text(a, "message").unwrap_or_default();
            let mut d = app.dialog().message(msg);
            #[cfg(desktop)]
            if let Some(w) = app.get_webview_window(&label_of(a)) {
                d = d.parent(&w);
            }
            if let Some(t) = text(a, "title") {
                d = d.title(t);
            }
            d = d.kind(match text(a, "kind").as_deref() {
                Some("warning") => MessageDialogKind::Warning,
                Some("error") => MessageDialogKind::Error,
                _ => MessageDialogKind::Info,
            });
            if op == "dialog.confirm" {
                let ok = text(a, "ok").unwrap_or_else(|| "OK".into());
                let cancel = text(a, "cancel").unwrap_or_else(|| "Cancel".into());
                d = d.buttons(MessageDialogButtons::OkCancelCustom(ok, cancel));
            }
            Ok(Value::Bool(d.blocking_show()))
        }
        other => Err(format!("desktop: unknown request '{}'", other)),
    }
}
