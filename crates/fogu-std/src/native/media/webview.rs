//! `fogu:webview` module — drives a real browser (Chrome, Edge or Chromium)
//! through the Chrome DevTools Protocol: render JavaScript-heavy pages,
//! click, type, run scripts, take screenshots and PDFs.
//!
//! ```fogu
//! import webview from "fogu:webview"
//!
//! const browser = webview.launch()?                 // headless
//! const page = browser.open("https://example.com")?
//! print(page.title()?)
//! print(page.eval("document.querySelectorAll('a').length")?)
//! page.screenshot()?.save("shot.png")?
//! browser.close()
//! ```
//!
//! The browser is found in `FOGU_CHROME`, the `path` option, the PATH
//! (`chrome chromium msedge`…) or the usual install folders. Every command
//! runs on a worker thread, so other `fg` tasks keep running meanwhile.

use std::cell::RefCell;
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value as J};
use tungstenite::{Message, WebSocket};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, json_to_value, list, max_args, nfn_var, opt_arg, str_arg, thread_poller};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    // webview.launch({ headless = true, path, args, width = 1280, height = 800, userAgent, timeout = 30s })
    e.insert("launch".into(), nfn_var("launch", 0, |a| {
        max_args(a, 1, "webview.launch")?;
        let cfg = LaunchCfg::from(opt_arg(a, 0))?;
        let b = crate::native::rt::offload(move || Browser::launch(cfg))??;
        Ok(NativeObjectRef::new(BrowserValue { b: Some(b) }).to_value())
    }));
    // webview.render(url, { wait, timeout }) → the HTML after JavaScript ran
    e.insert("render".into(), nfn_var("render", 1, |a| {
        max_args(a, 2, "webview.render")?;
        let url = str_arg(a, 0, "webview.render")?;
        let o = PageOpts::from(opt_arg(a, 1))?;
        let html = crate::native::rt::offload(move || -> Result<String, String> {
            let b = shared()?;
            let t = b.new_tab()?;
            let r = t.goto(&url, &o).and_then(|_| t.eval("document.documentElement.outerHTML"));
            let _ = t.close();
            Ok(r?.as_str().unwrap_or_default().to_string())
        })??;
        Ok(Value::Str(html.into()))
    }));
    e.insert("find".into(), nfn_var("find", 0, |a| {
        max_args(a, 0, "webview.find")?;
        Ok(find_browser(None).map_or(Value::Nil, |p| Value::Str(crate::native::util::path_str(&p).into())))
    }));
    ModuleValue {
        name: "fogu:webview".into(),
        exports: RefCell::new(e),
    }
}

// ================= LAUNCHING =================

fn find_browser(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit.map(String::from).or_else(|| std::env::var("FOGU_CHROME").ok()).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let names = [
        "google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome", "msedge",
        "microsoft-edge", "microsoft-edge-stable", "brave-browser", "chrome-headless-shell",
    ];
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in names {
                for cand in [dir.join(n), dir.join(format!("{}.exe", n))] {
                    if cand.is_file() {
                        return Some(cand);
                    }
                }
            }
        }
    }
    let mut fixed: Vec<PathBuf> = vec![
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
        "/Applications/Chromium.app/Contents/MacOS/Chromium".into(),
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into(),
        "/snap/bin/chromium".into(),
    ];
    for base in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
        if let Ok(b) = std::env::var(base) {
            fixed.push(PathBuf::from(&b).join("Google/Chrome/Application/chrome.exe"));
            fixed.push(PathBuf::from(&b).join("Microsoft/Edge/Application/msedge.exe"));
            fixed.push(PathBuf::from(&b).join("Chromium/Application/chrome.exe"));
        }
    }
    fixed.into_iter().find(|p| p.is_file())
}

#[derive(Clone)]
struct LaunchCfg {
    headless: bool,
    path: Option<String>,
    args: Vec<String>,
    width: u32,
    height: u32,
    user_agent: Option<String>,
    timeout: Duration,
    /// `--app=url`: a chromeless window (used by desktop-like scripts)
    app: Option<String>,
}

impl LaunchCfg {
    fn from(v: Option<&Value>) -> Result<Self, String> {
        let mut c = LaunchCfg {
            headless: true,
            path: None,
            args: Vec::new(),
            width: 1280,
            height: 800,
            user_agent: None,
            timeout: Duration::from_secs(30),
            app: None,
        };
        let Some(v) = v else { return Ok(c) };
        let Value::Map(m) = v else {
            return Err(format!("webview.launch: options must be a map, got '{}'", v.type_name()));
        };
        for (k, v) in m.borrow().iter() {
            match k.as_str() {
                "headless" => c.headless = v.is_truthy(),
                "path" => c.path = Some(v.to_display_string()),
                "userAgent" => c.user_agent = Some(v.to_display_string()),
                "app" => c.app = Some(v.to_display_string()),
                "width" | "height" => {
                    let Value::Int(n) = v else { return Err(format!("webview.launch: '{}' must be an int", k)) };
                    if k == "width" { c.width = (*n).max(1) as u32 } else { c.height = (*n).max(1) as u32 }
                }
                "timeout" => c.timeout = duration_arg(v, "webview.launch timeout")?,
                "args" => match v {
                    Value::List(l) => c.args = l.borrow().iter().map(|x| x.to_display_string()).collect(),
                    _ => return Err("webview.launch: 'args' must be a list of str".into()),
                },
                other => return Err(format!("webview.launch: unknown option '{}' (headless path args width height userAgent timeout app)", other)),
            }
        }
        Ok(c)
    }
}

/// One DevTools websocket to the browser; tabs are flattened sessions on it.
struct Conn {
    ws: WebSocket<TcpStream>,
    next_id: u64,
    /// events read while waiting for a response: (sessionId, method, params)
    events: VecDeque<(String, String, J)>,
}

impl Conn {
    fn read(&mut self, deadline: Instant) -> Result<J, String> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io_err!("webview: timed out waiting for the browser"));
            }
            let _ = self.ws.get_ref().set_read_timeout(Some(left.max(Duration::from_millis(1))));
            match self.ws.read() {
                Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).map_err(|e| io_err!("webview: bad message: {}", e)),
                Ok(Message::Close(_)) => return Err(io_err!("webview: the browser closed the connection")),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(e) => return Err(io_err!("webview: connection lost: {}", e)),
            }
        }
    }

    fn call(&mut self, session: Option<&str>, method: &str, params: J, timeout: Duration) -> Result<J, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = J::String(s.into());
        }
        self.ws.send(Message::text(msg.to_string())).map_err(|e| io_err!("webview: send failed: {}", e))?;
        let deadline = Instant::now() + timeout;
        loop {
            let m = self.read(deadline)?;
            if m.get("id").and_then(J::as_u64) == Some(id) {
                if let Some(err) = m.get("error") {
                    let text = err.get("message").and_then(J::as_str).unwrap_or("error");
                    return Err(io_err!("webview: {}: {}", method, text));
                }
                return Ok(m.get("result").cloned().unwrap_or(J::Null));
            }
            if let Some(ev) = m.get("method").and_then(J::as_str) {
                let sid = m.get("sessionId").and_then(J::as_str).unwrap_or("").to_string();
                self.events.push_back((sid, ev.to_string(), m.get("params").cloned().unwrap_or(J::Null)));
                if self.events.len() > 1000 {
                    self.events.pop_front();
                }
            }
        }
    }

    /// Waits for event `name` on `session` (already queued, or arriving before `deadline`).
    fn wait_event(&mut self, session: &str, name: &str, deadline: Instant) -> Result<J, String> {
        loop {
            if let Some(i) = self.events.iter().position(|(s, m, _)| s == session && m == name) {
                return Ok(self.events.remove(i).expect("present").2);
            }
            let m = self.read(deadline)?;
            if let Some(ev) = m.get("method").and_then(J::as_str) {
                let sid = m.get("sessionId").and_then(J::as_str).unwrap_or("").to_string();
                self.events.push_back((sid, ev.to_string(), m.get("params").cloned().unwrap_or(J::Null)));
            }
        }
    }
}

struct BrowserInner {
    conn: Mutex<Conn>,
    child: Mutex<Option<Child>>,
    profile: PathBuf,
    cfg: LaunchCfg,
}

impl Drop for BrowserInner {
    fn drop(&mut self) {
        if let Ok(mut c) = self.conn.lock() {
            let _ = c.call(None, "Browser.close", json!({}), Duration::from_secs(2));
        }
        if let Some(mut ch) = self.child.lock().ok().and_then(|mut c| c.take()) {
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = ch.try_wait() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = ch.kill();
            let _ = ch.wait();
        }
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

#[derive(Clone)]
struct Browser(Arc<BrowserInner>);

impl Browser {
    fn launch(cfg: LaunchCfg) -> Result<Browser, String> {
        let exe = find_browser(cfg.path.as_deref()).ok_or_else(|| {
            io_err!("webview: no Chrome, Edge or Chromium found; install one or set FOGU_CHROME / the 'path' option")
        })?;
        let profile = std::env::temp_dir().join(format!("fogu-webview-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple()));
        let mut cmd = Command::new(&exe);
        cmd.arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(["--no-first-run", "--no-default-browser-check", "--disable-extensions", "--disable-background-networking",
                   "--disable-sync", "--disable-dev-shm-usage", "--mute-audio", "--hide-scrollbars"])
            .arg(format!("--window-size={},{}", cfg.width, cfg.height));
        if cfg.headless {
            cmd.args(["--headless=new", "--disable-gpu"]);
        }
        // root in containers cannot use the sandbox
        #[cfg(unix)]
        if std::env::var("USER").is_ok_and(|u| u == "root") || std::path::Path::new("/.dockerenv").exists() {
            cmd.arg("--no-sandbox");
        }
        if let Some(ua) = &cfg.user_agent {
            cmd.arg(format!("--user-agent={}", ua));
        }
        cmd.args(&cfg.args);
        match &cfg.app {
            Some(url) => cmd.arg(format!("--app={}", url)),
            None => cmd.arg("about:blank"),
        };
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| io_err!("webview: cannot start {}: {}", exe.display(), e))?;
        // "DevTools listening on ws://127.0.0.1:PORT/devtools/browser/ID"
        let stderr = child.stderr.take().expect("piped");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !sent {
                    if let Some(i) = line.find("ws://") {
                        let _ = tx.send(Ok(line[i..].trim().to_string()));
                        sent = true;
                    }
                }
            }
            if !sent {
                let _ = tx.send(Err(()));
            }
        });
        let url = match rx.recv_timeout(cfg.timeout) {
            Ok(Ok(u)) => u,
            _ => {
                let _ = child.kill();
                return Err(io_err!("webview: {} did not start (no DevTools endpoint)", exe.display()));
            }
        };
        let addr = url.trim_start_matches("ws://").split('/').next().unwrap_or("").to_string();
        let stream = TcpStream::connect(&addr).map_err(|e| io_err!("webview: cannot connect to {}: {}", addr, e))?;
        let (ws, _) = tungstenite::client(url.as_str(), stream).map_err(|e| io_err!("webview: handshake failed: {}", e))?;
        let inner = Arc::new(BrowserInner {
            conn: Mutex::new(Conn { ws, next_id: 0, events: VecDeque::new() }),
            child: Mutex::new(Some(child)),
            profile,
            cfg,
        });
        crate::native::signal::arm_cleanup();
        if let Ok(mut alive) = ALIVE.lock() {
            alive.retain(|w| w.strong_count() > 0);
            alive.push(Arc::downgrade(&inner));
        }
        Ok(Browser(inner))
    }

    fn call(&self, session: Option<&str>, method: &str, params: J) -> Result<J, String> {
        let timeout = self.0.cfg.timeout;
        self.0.conn.lock().map_err(|_| "webview: connection poisoned".to_string())?.call(session, method, params, timeout)
    }

    fn new_tab(&self) -> Result<Tab, String> {
        let r = self.call(None, "Target.createTarget", json!({ "url": "about:blank" }))?;
        let target = r["targetId"].as_str().unwrap_or_default().to_string();
        let r = self.call(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }))?;
        let session = r["sessionId"].as_str().unwrap_or_default().to_string();
        let t = Tab { b: self.clone(), target, session };
        t.cmd("Page.enable", json!({}))?;
        t.cmd("Runtime.enable", json!({}))?;
        t.cmd("Page.setLifecycleEventsEnabled", json!({ "enabled": true }))?;
        let cfg = &self.0.cfg;
        t.cmd("Emulation.setDeviceMetricsOverride", json!({ "width": cfg.width, "height": cfg.height, "deviceScaleFactor": 1, "mobile": false }))?;
        Ok(t)
    }
}

/// Every launched browser, so the processes are closed when the program ends.
static ALIVE: Mutex<Vec<std::sync::Weak<BrowserInner>>> = Mutex::new(Vec::new());

static SHARED: Mutex<Option<Browser>> = Mutex::new(None);

/// A headless browser shared by `webview.render` and `image.fromHtml`
/// (started on first use, closed at exit).
fn shared() -> Result<Browser, String> {
    let mut s = SHARED.lock().map_err(|_| "webview: poisoned".to_string())?;
    if let Some(b) = s.as_ref() {
        // still alive?
        if b.call(None, "Browser.getVersion", json!({})).is_ok() {
            return Ok(b.clone());
        }
    }
    let b = Browser::launch(LaunchCfg::from(None)?)?;
    *s = Some(b.clone());
    Ok(b)
}

/// Closes the shared browser (called when the program ends).
pub fn shutdown() {
    if let Ok(mut s) = SHARED.lock() {
        s.take();
    }
    let alive: Vec<_> = ALIVE.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
    for b in alive.iter().filter_map(std::sync::Weak::upgrade) {
        if let Some(mut ch) = b.child.lock().ok().and_then(|mut c| c.take()) {
            let _ = ch.kill();
            let _ = ch.wait();
        }
        let _ = std::fs::remove_dir_all(&b.profile);
    }
}

// ================= TABS =================

#[derive(Clone)]
struct PageOpts {
    /// "load" (default), "domcontentloaded", "networkidle" or "none"
    wait: String,
    timeout: Option<Duration>,
}

impl PageOpts {
    fn from(v: Option<&Value>) -> Result<Self, String> {
        let mut o = PageOpts { wait: "load".into(), timeout: None };
        if let Some(Value::Map(m)) = v {
            let m = m.borrow();
            if let Some(w) = m.get("wait") {
                o.wait = w.to_display_string().to_ascii_lowercase();
                if !["load", "domcontentloaded", "networkidle", "none"].contains(&o.wait.as_str()) {
                    return Err(format!("webview: 'wait' must be load, domcontentloaded, networkidle or none, got '{}'", o.wait));
                }
            }
            if let Some(t) = m.get("timeout") {
                o.timeout = Some(duration_arg(t, "webview timeout")?);
            }
        }
        Ok(o)
    }
}

#[derive(Clone)]
struct Tab {
    b: Browser,
    target: String,
    session: String,
}

/// JavaScript string literal.
fn js_str(s: &str) -> String {
    J::String(s.to_string()).to_string()
}

impl Tab {
    fn cmd(&self, method: &str, params: J) -> Result<J, String> {
        self.b.call(Some(&self.session), method, params)
    }

    fn goto(&self, url: &str, o: &PageOpts) -> Result<(), String> {
        let deadline = Instant::now() + o.timeout.unwrap_or(self.b.0.cfg.timeout);
        {
            let mut c = self.b.0.conn.lock().map_err(|_| "webview: poisoned".to_string())?;
            c.events.retain(|(s, _, _)| s != &self.session);
        }
        let r = self.cmd("Page.navigate", json!({ "url": url }))?;
        if let Some(err) = r.get("errorText").and_then(J::as_str) {
            return Err(io_err!("webview: cannot open {}: {}", url, err));
        }
        self.wait_load(o, deadline)
    }

    fn wait_load(&self, o: &PageOpts, deadline: Instant) -> Result<(), String> {
        let event = match o.wait.as_str() {
            "none" => return Ok(()),
            "domcontentloaded" => "Page.domContentEventFired",
            _ => "Page.loadEventFired",
        };
        {
            let mut c = self.b.0.conn.lock().map_err(|_| "webview: poisoned".to_string())?;
            c.wait_event(&self.session, event, deadline)?;
            if o.wait == "networkidle" {
                // lifecycle event "networkIdle" (500ms without requests)
                loop {
                    let p = c.wait_event(&self.session, "Page.lifecycleEvent", deadline)?;
                    if p["name"] == "networkIdle" {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Evaluates an expression (promises are awaited) and returns its JSON value.
    fn eval(&self, expr: &str) -> Result<J, String> {
        let r = self.cmd("Runtime.evaluate", json!({
            "expression": expr, "awaitPromise": true, "returnByValue": true, "userGesture": true,
        }))?;
        if let Some(ex) = r.get("exceptionDetails") {
            let msg = ex["exception"]["description"].as_str().or_else(|| ex["text"].as_str()).unwrap_or("exception");
            return Err(io_err!("webview: script error: {}", msg.lines().next().unwrap_or(msg)));
        }
        Ok(r["result"].get("value").cloned().unwrap_or(J::Null))
    }

    /// Center of the first element matching `sel` (scrolled into view).
    fn center(&self, sel: &str) -> Result<(f64, f64), String> {
        let r = self.eval(&format!(
            "(() => {{ const e = document.querySelector({}); if (!e) return null; e.scrollIntoView({{block: 'center', inline: 'center'}}); \
             const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; }})()",
            js_str(sel)
        ))?;
        match r.as_array() {
            Some(a) if a.len() == 2 => Ok((a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0))),
            _ => Err(io_err!("webview: no element matches '{}'", sel)),
        }
    }

    fn wait_for(&self, sel: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let probe = format!("!!document.querySelector({})", js_str(sel));
        loop {
            if self.eval(&probe)?.as_bool() == Some(true) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io_err!("webview: timed out waiting for '{}'", sel));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn click(&self, sel: &str) -> Result<(), String> {
        let (x, y) = self.center(sel)?;
        for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
            self.cmd("Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }))?;
        }
        Ok(())
    }

    fn press(&self, key: &str) -> Result<(), String> {
        let (code, vk, text) = match key {
            "Enter" => ("Enter", 13, "\r"),
            "Tab" => ("Tab", 9, ""),
            "Escape" => ("Escape", 27, ""),
            "Backspace" => ("Backspace", 8, ""),
            "Delete" => ("Delete", 46, ""),
            "ArrowUp" => ("ArrowUp", 38, ""),
            "ArrowDown" => ("ArrowDown", 40, ""),
            "ArrowLeft" => ("ArrowLeft", 37, ""),
            "ArrowRight" => ("ArrowRight", 39, ""),
            "Space" | " " => ("Space", 32, " "),
            k if k.chars().count() == 1 => {
                return self.cmd("Input.insertText", json!({ "text": k })).map(|_| ());
            }
            other => return Err(format!("press: unknown key '{}' (Enter Tab Escape Backspace Delete Arrow* Space or one character)", other)),
        };
        let mut down = json!({ "type": "keyDown", "key": key, "code": code, "windowsVirtualKeyCode": vk });
        if !text.is_empty() {
            down["text"] = J::String(text.into());
        }
        self.cmd("Input.dispatchKeyEvent", down)?;
        self.cmd("Input.dispatchKeyEvent", json!({ "type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk }))?;
        Ok(())
    }

    fn set_content(&self, html: &str) -> Result<(), String> {
        let tree = self.cmd("Page.getFrameTree", json!({}))?;
        let frame = tree["frameTree"]["frame"]["id"].as_str().unwrap_or_default().to_string();
        self.cmd("Page.setDocumentContent", json!({ "frameId": frame, "html": html }))?;
        // images and web fonts
        self.eval(
            "new Promise(r => { const done = () => (document.fonts ? document.fonts.ready : Promise.resolve()).then(() => r(true)); \
             document.readyState === 'complete' ? done() : addEventListener('load', done); setTimeout(() => r(true), 10000); })",
        )?;
        Ok(())
    }

    /// PNG/JPEG/WebP bytes of the viewport, the full page or one element.
    fn screenshot(&self, full: bool, element: Option<&str>, format: &str, quality: Option<i64>) -> Result<Vec<u8>, String> {
        let mut p = json!({ "format": format, "captureBeyondViewport": full || element.is_some() });
        if let Some(q) = quality {
            p["quality"] = json!(q);
        }
        if let Some(sel) = element {
            let r = self.eval(&format!(
                "(() => {{ const e = document.querySelector({}); if (!e) return null; const r = e.getBoundingClientRect(); \
                 return [r.left + scrollX, r.top + scrollY, r.width, r.height]; }})()",
                js_str(sel)
            ))?;
            let a = r.as_array().ok_or_else(|| io_err!("webview: no element matches '{}'", sel))?;
            let n = |i: usize| a.get(i).and_then(J::as_f64).unwrap_or(0.0);
            p["clip"] = json!({ "x": n(0), "y": n(1), "width": n(2).max(1.0), "height": n(3).max(1.0), "scale": 1 });
        } else if full {
            let m = self.cmd("Page.getLayoutMetrics", json!({}))?;
            let size = &m["cssContentSize"];
            p["clip"] = json!({ "x": 0, "y": 0, "width": size["width"], "height": size["height"], "scale": 1 });
        }
        let r = self.cmd("Page.captureScreenshot", p)?;
        base64::engine::general_purpose::STANDARD
            .decode(r["data"].as_str().unwrap_or_default())
            .map_err(|_| io_err!("webview: invalid screenshot data"))
    }

    fn close(&self) -> Result<(), String> {
        self.b.call(None, "Target.closeTarget", json!({ "targetId": self.target })).map(|_| ())
    }
}

/// Renders HTML in the shared headless browser and returns PNG bytes
/// (`image.fromHtml`). Blocking: call it through `rt::offload`.
pub fn html_to_png(html: &str, width: u32, height: u32, full: bool) -> Result<Vec<u8>, String> {
    let b = shared()?;
    let t = b.new_tab()?;
    let r = (|| {
        t.cmd("Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": height, "deviceScaleFactor": 1, "mobile": false }))?;
        t.cmd("Emulation.setDefaultBackgroundColorOverride", json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 0 } }))?;
        t.set_content(html)?;
        t.screenshot(full, None, "png", None)
    })();
    let _ = t.close();
    r
}

// ================= FOGU OBJECTS =================

pub struct BrowserValue {
    b: Option<Browser>,
}

impl BrowserValue {
    fn browser(&self) -> Result<Browser, String> {
        self.b.clone().ok_or_else(|| "webview: the browser was closed".to_string())
    }
}

impl FoguObject for BrowserValue {
    fn name(&self) -> &str {
        "Browser"
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "close" => {
                // dropping the last reference closes the process (open pages keep it alive)
                self.b.take();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Browser (open close version)", other)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let b = match self.browser() {
            Ok(b) => b,
            Err(e) if matches!(method, "open" | "version") => return Some(Err(e)),
            Err(_) => return None,
        };
        Some(match method {
            // open(url?, { wait, timeout }) → Page
            "open" => (|| {
                max_args(args, 2, "open")?;
                let url = match opt_arg(args, 0) {
                    Some(v) => Some(str_arg(std::slice::from_ref(v), 0, "open")?),
                    None => None,
                };
                let o = PageOpts::from(opt_arg(args, 1))?;
                Ok(thread_poller(
                    move || {
                        let t = b.new_tab()?;
                        if let Some(u) = url {
                            t.goto(&u, &o)?;
                        }
                        Ok(t)
                    },
                    |t| Ok(NativeObjectRef::new(PageValue { t: Some(t) }).to_value()),
                ))
            })(),
            "version" => Ok(thread_poller(
                move || b.call(None, "Browser.getVersion", json!({}))?["product"].as_str().map(String::from).ok_or_else(|| "?".to_string()),
                |s| Ok(Value::Str(s.into())),
            )),
            _ => return None,
        })
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["open", "close", "version"]
    }
}

pub struct PageValue {
    t: Option<Tab>,
}

fn to_json(v: &Value) -> J {
    serde_json::from_str(&stringify_value(v)).unwrap_or(J::Null)
}

impl FoguObject for PageValue {
    fn name(&self) -> &str {
        "Page"
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on Page ({})", method, self.methods().join(" ")))
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if !self.methods().contains(&method) {
            return None;
        }
        let Some(t) = self.t.clone() else {
            return Some(Err("webview: the page was closed".into()));
        };
        if method == "close" {
            self.t = None;
        }
        let a = args.to_vec();
        let r = (|| -> Result<Poller, String> {
            let s = |i: usize| str_arg(&a, i, method);
            let done = |_: ()| Ok(Value::Nil);
            let to_val = |j: J| Ok(json_to_value(&j));
            Ok(match method {
                "goto" | "reload" => {
                    let url = if method == "goto" { Some(s(0)?) } else { None };
                    let o = PageOpts::from(opt_arg(&a, if method == "goto" { 1 } else { 0 }))?;
                    thread_poller(
                        move || match url {
                            Some(u) => t.goto(&u, &o),
                            None => {
                                t.cmd("Page.reload", json!({}))?;
                                let deadline = Instant::now() + o.timeout.unwrap_or(t.b.0.cfg.timeout);
                                t.wait_load(&o, deadline)
                            }
                        },
                        done,
                    )
                }
                "back" | "forward" => {
                    let js = if method == "back" { "history.back()" } else { "history.forward()" };
                    thread_poller(move || {
                        t.eval(js)?;
                        std::thread::sleep(Duration::from_millis(100));
                        t.eval("new Promise(r => document.readyState === 'complete' ? r() : addEventListener('load', () => r()))").map(|_| ())
                    }, done)
                }
                "url" => thread_poller(move || t.eval("location.href"), to_val),
                "title" => thread_poller(move || t.eval("document.title"), to_val),
                "content" => thread_poller(move || t.eval("document.documentElement.outerHTML"), to_val),
                "text" => {
                    let js = match opt_arg(&a, 0) {
                        Some(_) => format!("(() => {{ const e = document.querySelector({}); return e ? e.innerText : null }})()", js_str(&s(0)?)),
                        None => "document.body ? document.body.innerText : ''".into(),
                    };
                    thread_poller(move || t.eval(&js), to_val)
                }
                // eval(js, ...args): an expression, or a function called with JSON args
                "eval" => {
                    let code = s(0)?;
                    let js = if a.len() > 1 {
                        let args: Vec<String> = a[1..].iter().map(|v| to_json(v).to_string()).collect();
                        format!("({})({})", code, args.join(", "))
                    } else {
                        code
                    };
                    thread_poller(move || t.eval(&js), to_val)
                }
                "setContent" => {
                    let html = s(0)?;
                    thread_poller(move || t.set_content(&html), done)
                }
                "click" => {
                    let sel = s(0)?;
                    thread_poller(move || t.click(&sel), done)
                }
                // type(selector, text): focuses the element and types the text
                "type" => {
                    let (sel, text) = (s(0)?, s(1)?);
                    thread_poller(
                        move || {
                            t.click(&sel)?;
                            t.cmd("Input.insertText", json!({ "text": text })).map(|_| ())
                        },
                        done,
                    )
                }
                // fill(selector, value): sets the value directly (inputs, textareas, selects)
                "fill" => {
                    let (sel, val) = (s(0)?, a.get(1).map(|v| v.to_display_string()).unwrap_or_default());
                    let js = format!(
                        "(() => {{ const e = document.querySelector({}); if (!e) throw new Error('no element matches ' + {0}); \
                         e.focus(); e.value = {}; e.dispatchEvent(new Event('input', {{bubbles: true}})); \
                         e.dispatchEvent(new Event('change', {{bubbles: true}})); return true }})()",
                        js_str(&sel), js_str(&val)
                    );
                    thread_poller(move || t.eval(&js).map(|_| ()), done)
                }
                "press" => {
                    let key = s(0)?;
                    thread_poller(move || t.press(&key), done)
                }
                // waitFor(selector, timeout = 30s)
                "waitFor" => {
                    let sel = s(0)?;
                    let tm = match opt_arg(&a, 1) { Some(v) => duration_arg(v, "waitFor")?, None => t.b.0.cfg.timeout };
                    thread_poller(move || t.wait_for(&sel, tm), done)
                }
                "waitForNavigation" => {
                    let o = PageOpts::from(opt_arg(&a, 0))?;
                    let deadline = Instant::now() + o.timeout.unwrap_or(t.b.0.cfg.timeout);
                    thread_poller(move || t.wait_load(&o, deadline), done)
                }
                // select(css) → [{ tag, text, html, attrs }] (a snapshot of the live DOM)
                "select" => {
                    let js = format!(
                        "Array.from(document.querySelectorAll({})).map(e => ({{ tag: e.tagName.toLowerCase(), text: (e.innerText || e.textContent || '').trim(), \
                         html: e.innerHTML, attrs: Object.fromEntries(Array.from(e.attributes).map(a => [a.name, a.value])) }}))",
                        js_str(&s(0)?)
                    );
                    thread_poller(move || t.eval(&js), to_val)
                }
                // screenshot({ fullPage, element, format = "png", quality }) → Image
                "screenshot" => {
                    let o = match opt_arg(&a, 0) { Some(Value::Map(m)) => m.borrow().clone(), _ => HashMap::new() };
                    let full = o.get("fullPage").is_some_and(Value::is_truthy);
                    let element = o.get("element").map(|v| v.to_display_string());
                    thread_poller(move || t.screenshot(full, element.as_deref(), "png", None), |png| crate::native::image::from_encoded(&png))
                }
                // pdf(path?, { landscape, background = true, scale, format = "A4" }) → nil (saved) or bytes
                "pdf" => {
                    let path = match opt_arg(&a, 0) { Some(Value::Str(p)) => Some(p.to_string()), _ => None };
                    let o = match a.iter().find_map(|v| if let Value::Map(m) = v { Some(m.borrow().clone()) } else { None }) {
                        Some(m) => m,
                        None => HashMap::new(),
                    };
                    let (pw, ph) = match o.get("format").map(|v| v.to_display_string().to_ascii_lowercase()).as_deref() {
                        None | Some("a4") => (8.27, 11.69),
                        Some("letter") => (8.5, 11.0),
                        Some("legal") => (8.5, 14.0),
                        Some("a3") => (11.69, 16.54),
                        Some("a5") => (5.83, 8.27),
                        Some(other) => return Err(format!("pdf: unknown format '{}' (A3 A4 A5 Letter Legal)", other)),
                    };
                    let params = json!({
                        "landscape": o.get("landscape").is_some_and(Value::is_truthy),
                        "printBackground": o.get("background").is_none_or(Value::is_truthy),
                        "scale": match o.get("scale") { Some(Value::Float(f)) => *f, Some(Value::Int(n)) => *n as f64, _ => 1.0 },
                        "paperWidth": pw, "paperHeight": ph,
                    });
                    thread_poller(
                        move || {
                            let r = t.cmd("Page.printToPDF", params)?;
                            let data = base64::engine::general_purpose::STANDARD
                                .decode(r["data"].as_str().unwrap_or_default())
                                .map_err(|_| io_err!("webview: invalid PDF data"))?;
                            if let Some(p) = &path {
                                std::fs::write(p, &data).map_err(|e| io_err!("webview.pdf: {}: {}", p, e))?;
                                return Ok(None);
                            }
                            Ok(Some(data))
                        },
                        |d| Ok(d.map_or(Value::Nil, |d| list(d.into_iter().map(|b| Value::Int(b as i64)).collect()))),
                    )
                }
                "setViewport" => {
                    let w = match a.first() { Some(Value::Int(n)) => *n, _ => return Err("setViewport(width, height)".into()) };
                    let h = match a.get(1) { Some(Value::Int(n)) => *n, _ => return Err("setViewport(width, height)".into()) };
                    thread_poller(
                        move || t.cmd("Emulation.setDeviceMetricsOverride", json!({ "width": w, "height": h, "deviceScaleFactor": 1, "mobile": false })).map(|_| ()),
                        done,
                    )
                }
                "setHeaders" => {
                    let headers = to_json(a.first().unwrap_or(&Value::Nil));
                    thread_poller(
                        move || {
                            t.cmd("Network.enable", json!({}))?;
                            t.cmd("Network.setExtraHTTPHeaders", json!({ "headers": headers })).map(|_| ())
                        },
                        done,
                    )
                }
                "cookies" => thread_poller(
                    move || {
                        t.cmd("Network.enable", json!({}))?;
                        Ok(t.cmd("Network.getCookies", json!({}))?["cookies"].clone())
                    },
                    to_val,
                ),
                // setCookie({ name, value, url | domain, path, secure, httpOnly })
                "setCookie" => {
                    let c = to_json(a.first().unwrap_or(&Value::Nil));
                    thread_poller(
                        move || {
                            t.cmd("Network.enable", json!({}))?;
                            let r = t.cmd("Network.setCookie", c)?;
                            if r["success"] == J::Bool(false) {
                                return Err(io_err!("webview: the cookie was rejected (give 'url' or 'domain')"));
                            }
                            Ok(())
                        },
                        done,
                    )
                }
                "close" => thread_poller(move || t.close(), done),
                _ => unreachable!(),
            })
        })();
        Some(r)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "goto", "reload", "back", "forward", "url", "title", "content", "text", "eval", "setContent", "click", "type",
            "fill", "press", "waitFor", "waitForNavigation", "select", "screenshot", "pdf", "setViewport", "setHeaders",
            "cookies", "setCookie", "close",
        ]
    }
}

impl Drop for PageValue {
    fn drop(&mut self) {
        // close the tab of a page that goes out of scope (not the shared browser)
        if let Some(t) = self.t.take() {
            if Arc::strong_count(&t.b.0) > 1 {
                let _ = t.close();
            }
        }
    }
}
