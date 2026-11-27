//! The bridge between the webview and the Fogu program.
//!
//! The Fogu runtime (`fogu::embed::Runtime`) lives on its own thread (Fogu
//! values are `Rc`, never shared between threads). Everything crosses as JSON:
//!
//! * UI → Fogu: `Runtime::emit(name, json)` fires `Fogu.signal.on(name, fn)`
//!   handlers that the `desktop` library installed (`desktop:call` for
//!   `fogu.call`, `desktop:ui:<name>` for `fogu.emit`, `desktop:reply` for the
//!   answers to the program's own requests such as dialogs).
//! * Fogu → host: `Fogu.host.emit(name, data)` calls [`on_host`] on the runtime
//!   thread; it only forwards the message to the dispatcher thread
//!   ([`Bridge::dispatch`]), which talks to Tauri. It must never call back into
//!   the runtime from there (the runtime thread is the caller: that would deadlock).
//!
//! Several runtimes can be alive: the program's (replaced by a new one when
//! the Fogu code is hot-reloaded, [`Bridge::reload`]) and the workers'
//! (`desktop.worker(file)`: modules on their own threads for CPU-bound work).
//! Each passes its own id to [`on_host`], so a request is answered in the
//! runtime that made it.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, RwLock};

use fogu::embed::Runtime;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

/// Messages from `Fogu.host.emit` (runtime id, name, JSON data).
static HOST_TX: OnceLock<mpsc::Sender<(usize, String, String)>> = OnceLock::new();
/// Ids of the runtimes (passed back by `on_host` as its `user` value).
static NEXT_RT: AtomicUsize = AtomicUsize::new(1);

/// `Fogu.host.emit` → here, on the runtime thread. The host owns both strings.
extern "C" fn on_host(name: *const c_char, data: *const c_char, user: *mut c_void) {
    let take = |p: *const c_char| {
        if p.is_null() {
            String::new()
        } else {
            // SAFETY: Fogu.host.emit hands over CString::into_raw pointers
            unsafe { CString::from_raw(p as *mut c_char) }.to_string_lossy().into_owned()
        }
    };
    let (name, data) = (take(name), take(data));
    if let Some(tx) = HOST_TX.get() {
        let _ = tx.send((user as usize, name, data));
    }
}

/// A new runtime whose `Fogu.host.emit` reaches the bridge; returns its id.
fn new_runtime() -> Result<(usize, Arc<Runtime>), String> {
    let rt = Runtime::new()?;
    let id = NEXT_RT.fetch_add(1, Ordering::Relaxed);
    rt.on_host(Some(on_host), id)?;
    Ok((id, Arc::new(rt)))
}

/// A worker: one or more runtimes that loaded the same module.
struct Worker {
    file: String,
    /// The runtime that started it (a hot reload stops the old program's workers).
    owner: usize,
    rts: Vec<(usize, Arc<Runtime>)>,
    next: AtomicUsize,
}

type Pending = oneshot::Sender<Result<Value, String>>;

pub struct Bridge {
    /// The program's runtime and its id (a hot reload replaces both).
    rt: RwLock<(usize, Arc<Runtime>)>,
    /// Every live runtime by id (the program's and the workers'), to answer requests.
    runtimes: Mutex<HashMap<usize, Arc<Runtime>>>,
    workers: Mutex<HashMap<u64, Worker>>,
    next_worker: AtomicU64,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    /// The program failed to start (shown in the window, returned by every call).
    fatal: Mutex<Option<String>>,
    /// Per window: its page called `fogu_ready`, so events go straight to it;
    /// before that they are queued for it.
    ready: Mutex<HashMap<String, bool>>,
    queue: Mutex<HashMap<String, Vec<Value>>>,
    log_file: Option<std::path::PathBuf>,
    /// Host → program notifications (tray and window events), emitted in
    /// order by their own thread: the main thread never waits for Fogu.
    notify_tx: Mutex<mpsc::Sender<(String, String)>>,
    /// The entry file (for reloads).
    entry: Mutex<String>,
    /// The processes below the host when the program started: a hot reload
    /// ends the ones the old program started since (crate::children).
    before: Mutex<std::collections::HashSet<u32>>,
}

impl Bridge {
    /// Starts the runtime (in the process' working directory, which the caller
    /// set to the extracted program) and the dispatcher thread.
    pub fn start(app: AppHandle, log_file: Option<std::path::PathBuf>) -> Result<Arc<Bridge>, String> {
        let (tx, rx) = mpsc::channel();
        HOST_TX.set(tx).map_err(|_| "the Fogu bridge was started twice".to_string())?;
        let (id, rt) = new_runtime()?;
        let (ntx, nrx) = mpsc::channel::<(String, String)>();
        let bridge = Arc::new(Bridge {
            rt: RwLock::new((id, Arc::clone(&rt))),
            runtimes: Mutex::new(HashMap::from([(id, rt)])),
            workers: Mutex::new(HashMap::new()),
            next_worker: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            fatal: Mutex::new(None),
            ready: Mutex::new(HashMap::new()),
            queue: Mutex::new(HashMap::new()),
            log_file,
            notify_tx: Mutex::new(ntx),
            entry: Mutex::new(String::new()),
            before: Mutex::new(std::collections::HashSet::new()),
        });
        let n = Arc::clone(&bridge);
        std::thread::Builder::new()
            .name("fogu-notify".into())
            .spawn(move || {
                while let Ok((name, data)) = nrx.recv() {
                    if let Err(e) = n.main().emit(&name, &data) {
                        n.log(&format!("cannot deliver {}: {}", name, e));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let b = Arc::clone(&bridge);
        std::thread::Builder::new()
            .name("fogu-dispatch".into())
            .spawn(move || {
                while let Ok((from, name, data)) = rx.recv() {
                    let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
                    b.dispatch(&app, from, &name, data);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(bridge)
    }

    /// The program's runtime.
    fn main(&self) -> Arc<Runtime> {
        Arc::clone(&self.rt.read().unwrap().1)
    }

    /// Runs the program's entry file (its top level registers the commands).
    pub fn run_main(self: &Arc<Self>, app: &AppHandle, path: &str) {
        *self.entry.lock().unwrap() = path.to_string();
        if reloads() {
            *self.before.lock().unwrap() = crate::children::snapshot();
        }
        if let Err(e) = self.main().run_file(path) {
            self.fail(app, format!("{}: {}", path, e));
        }
    }

    /// Hot reload: a new runtime runs the entry file again; the windows stay.
    /// Calls in flight are rejected; the pages get the event `desktop:reloaded`.
    /// State the program saved with `desktop.keep` is there for `desktop.kept`.
    pub fn reload(self: &Arc<Self>, app: &AppHandle) {
        let entry = self.entry.lock().unwrap().clone();
        if entry.is_empty() {
            return;
        }
        let (id, rt) = match new_runtime() {
            Ok(r) => r,
            Err(e) => {
                self.log(&format!("reload: {}", e));
                return;
            }
        };
        let (old_id, old) = std::mem::replace(&mut *self.rt.write().unwrap(), (id, Arc::clone(&rt)));
        {
            let mut all = self.runtimes.lock().unwrap();
            all.remove(&old_id);
            all.insert(id, rt);
        }
        for (_, p) in self.pending.lock().unwrap().drain() {
            let _ = p.send(Err("the Fogu program was reloaded".to_string()));
        }
        *self.fatal.lock().unwrap() = None;
        crate::window::reset_program();
        // the old program's workers go with it
        let old_workers: Vec<Worker> = {
            let mut ws = self.workers.lock().unwrap();
            let ids: Vec<u64> = ws.iter().filter(|(_, w)| w.owner == old_id).map(|(k, _)| *k).collect();
            let gone: Vec<Worker> = ids.iter().filter_map(|k| ws.remove(k)).collect();
            let mut all = self.runtimes.lock().unwrap();
            for w in &gone {
                for (rid, _) in &w.rts {
                    all.remove(rid);
                }
            }
            gone
        };
        // the old program stops before the new one starts: what it holds (a
        // tunnel's local port, a server's port) must be free again
        self.stop_old(old, old_workers);
        *self.before.lock().unwrap() = crate::children::snapshot();
        self.log(&format!("reloaded {}", entry));
        if let Err(e) = self.main().run_file(&entry) {
            self.fail(app, format!("{}: {}", entry, e));
            return;
        }
        self.send_event(app, "desktop:reloaded".to_string(), json!({ "entry": entry }), None);
    }

    /// Stops the runtime of the program a hot reload replaced (and its
    /// workers), then ends the processes it started that are still running:
    /// a runtime that stops does not free what a suspended task or a
    /// reference cycle holds, and a child process (`ssh -N` of a tunnel)
    /// would outlive it.
    fn stop_old(&self, old: Arc<Runtime>, workers: Vec<Worker>) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // a call in flight may still hold the runtime for a moment
            let mut old = old;
            for _ in 0..100 {
                match Arc::try_unwrap(old) {
                    Ok(rt) => {
                        drop(rt);
                        break;
                    }
                    Err(rt) => {
                        old = rt;
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
            }
            drop(workers);
            let _ = tx.send(());
        });
        if rx.recv_timeout(std::time::Duration::from_secs(5)).is_err() {
            self.log("reload: the old program did not stop within 5 s; ending its processes anyway");
        }
        let before = self.before.lock().unwrap().clone();
        let stopped = crate::children::stop_new(&before);
        if !stopped.is_empty() {
            self.log(&format!("reload: ended {} process(es) the old program started: {}", stopped.len(), stopped.join(", ")));
        }
    }

    // ---------- workers ----------

    /// `desktop.worker(file, { threads })`: `threads` runtimes load `file`.
    fn worker_start(&self, owner: usize, a: &Value) -> Result<Value, String> {
        let file = a.get("file").and_then(Value::as_str).ok_or("desktop.worker: a file is required")?.to_string();
        // a relative path is in the program's folder (the host's working directory)
        let file = if std::path::Path::new(&file).is_relative() {
            std::env::current_dir().map_err(|e| e.to_string())?.join(&file).to_string_lossy().into_owned()
        } else {
            file
        };
        let threads = a.get("threads").and_then(Value::as_u64).unwrap_or(1).clamp(1, 64) as usize;
        let mut rts = Vec::new();
        for _ in 0..threads {
            let (id, rt) = new_runtime()?;
            rt.run_file(&file).map_err(|e| format!("desktop.worker {}: {}", file, e))?;
            self.runtimes.lock().unwrap().insert(id, Arc::clone(&rt));
            rts.push((id, rt));
        }
        let wid = self.next_worker.fetch_add(1, Ordering::Relaxed);
        self.workers.lock().unwrap().insert(wid, Worker { file, owner, rts, next: AtomicUsize::new(0) });
        Ok(json!(wid))
    }

    /// `worker.call(fnName, ...args)`: on the next runtime of the worker (round robin).
    fn worker_call(&self, a: &Value) -> Result<Value, String> {
        let wid = a.get("worker").and_then(Value::as_u64).ok_or("desktop.worker: no worker id")?;
        let name = a.get("fn").and_then(Value::as_str).ok_or("desktop.worker: no function name")?;
        let args = a.get("args").cloned().unwrap_or(json!([]));
        let rt = {
            let ws = self.workers.lock().unwrap();
            let w = ws.get(&wid).ok_or("desktop.worker: the worker was stopped")?;
            let i = w.next.fetch_add(1, Ordering::Relaxed) % w.rts.len();
            Arc::clone(&w.rts[i].1)
        };
        let out = rt.call(name, &args.to_string())?;
        Ok(serde_json::from_str(&out).unwrap_or(Value::Null))
    }

    fn worker_stop(&self, a: &Value) -> Result<Value, String> {
        let wid = a.get("worker").and_then(Value::as_u64).ok_or("desktop.worker: no worker id")?;
        let Some(w) = self.workers.lock().unwrap().remove(&wid) else { return Ok(Value::Bool(false)) };
        {
            let mut all = self.runtimes.lock().unwrap();
            for (id, _) in &w.rts {
                all.remove(id);
            }
        }
        let file = w.file.clone();
        std::thread::spawn(move || drop(w));
        self.log(&format!("worker {} ({}) stopped", wid, file));
        Ok(Value::Bool(true))
    }

    pub fn log(&self, msg: &str) {
        eprintln!("fogu-desktop: {}", msg);
        if let Some(p) = &self.log_file {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{}", msg);
            }
        }
    }

    fn fail(&self, app: &AppHandle, msg: String) {
        let msg = match area_hint(&msg) {
            Some(hint) => format!("{}\n{}", msg, hint),
            None => msg,
        };
        self.log(&msg);
        *self.fatal.lock().unwrap() = Some(msg.clone());
        for (_, p) in self.pending.lock().unwrap().drain() {
            let _ = p.send(Err(msg.clone()));
        }
        let js = format!("window.fogu && window.fogu.__fatal({})", json!(msg));
        let labels: Vec<String> = self.ready.lock().unwrap().iter().filter(|(_, r)| **r).map(|(l, _)| l.clone()).collect();
        for l in labels {
            eval_in(app, &l, &js);
        }
    }

    fn fatal(&self) -> Option<String> {
        self.fatal.lock().unwrap().clone()
    }

    /// `fogu.call(name, payload)` → the value the Fogu command returned.
    pub async fn call(self: Arc<Self>, name: String, payload: Value) -> Result<Value, String> {
        if let Some(f) = self.fatal() {
            return Err(f);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let msg = json!({ "id": id, "name": name, "payload": payload }).to_string();
        let rt = self.main();
        let sent = tauri::async_runtime::spawn_blocking(move || rt.emit("desktop:call", &msg)).await;
        let sent = sent.map_err(|e| e.to_string()).and_then(|r| r);
        if let Err(e) = sent {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        rx.await.map_err(|_| "the Fogu program stopped".to_string())?
    }

    /// `fogu.emit(name, data)` → `desktop.on(name, fn)` handlers.
    pub async fn emit_to_program(self: Arc<Self>, name: String, data: Value) -> Result<(), String> {
        if let Some(f) = self.fatal() {
            return Err(f);
        }
        let rt = self.main();
        let data = data.to_string();
        tauri::async_runtime::spawn_blocking(move || rt.emit(&format!("desktop:ui:{}", name), &data))
            .await
            .map_err(|e| e.to_string())?
            .map(|_| ())
    }

    /// The page of a window loaded: hand it the queued events (and the startup error, if any).
    pub fn page_ready(&self, label: &str) -> Value {
        let mut ready = self.ready.lock().unwrap();
        ready.insert(label.to_string(), true);
        let events: Vec<Value> = self.queue.lock().unwrap().remove(label).unwrap_or_default();
        json!({ "events": events, "fatal": self.fatal(), "window": label })
    }

    /// A navigation started in a window: queue its events until the new page is ready.
    pub fn page_loading(&self, label: &str) {
        self.ready.lock().unwrap().insert(label.to_string(), false);
    }

    /// A window was destroyed.
    pub fn forget_window(&self, label: &str) {
        self.ready.lock().unwrap().remove(label);
        self.queue.lock().unwrap().remove(label);
    }

    /// Fires `Fogu.signal.on(name, fn)` in the program without waiting (tray
    /// and window events; safe on the main thread).
    pub fn notify(&self, name: &str, data: Value) {
        if self.fatal().is_some() {
            return;
        }
        let _ = self.notify_tx.lock().unwrap().send((name.to_string(), data.to_string()));
    }

    /// A message of `Fogu.host.emit` from the runtime `from`, on the dispatcher thread.
    fn dispatch(self: &Arc<Self>, app: &AppHandle, from: usize, name: &str, data: Value) {
        match name {
            // the answer to a fogu.call
            "desktop:result" => {
                let Some(id) = data.get("id").and_then(Value::as_u64) else { return };
                let Some(p) = self.pending.lock().unwrap().remove(&id) else { return };
                let r = if data.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    Ok(data.get("value").cloned().unwrap_or(Value::Null))
                } else {
                    Err(data.get("error").and_then(Value::as_str).unwrap_or("the command failed").to_string())
                };
                let _ = p.send(r);
            }
            "desktop:event" => {
                let ev = data.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let payload = data.get("data").cloned().unwrap_or(Value::Null);
                let target = data.get("window").and_then(Value::as_str);
                self.send_event(app, ev, payload, target);
            }
            "desktop:window" => {
                if let Err(e) = crate::window::apply(app, &data) {
                    self.log(&e);
                }
            }
            "desktop:watch" => crate::window::watch_program(&data),
            // desktop.shortcut(accel, fn) / removeShortcut: { op: "register" | "unregister", shortcut }
            "desktop:shortcut" => {
                #[cfg(desktop)]
                {
                    let sc = data.get("shortcut").and_then(Value::as_str);
                    let r = match data.get("op").and_then(Value::as_str) {
                        Some("unregister") => crate::shortcut::unregister(app, sc),
                        _ => match sc {
                            Some(sc) => crate::shortcut::register(app, sc),
                            None => Err("desktop.shortcut: no shortcut".into()),
                        },
                    };
                    if let Err(e) = r {
                        self.log(&e);
                        self.send_event(app, "desktop:error".to_string(), json!({ "message": e }), None);
                    }
                }
                #[cfg(not(desktop))]
                self.log("desktop.shortcut: global shortcuts are not supported on mobile");
            }
            // desktop.notify(...): shown by a thread of its own (it may ask for permission)
            "desktop:notify" => {
                let b = Arc::clone(self);
                let app = app.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::window::notify(&app, &data) {
                        b.log(&e);
                        b.send_event(&app, "desktop:error".to_string(), json!({ "message": e }), None);
                    }
                });
            }
            // desktop.menu(...): { window, menu }
            "desktop:menu" => {
                #[cfg(desktop)]
                if let Err(e) = crate::menu::set(app, &data) {
                    self.log(&e);
                    self.send_event(app, "desktop:error".to_string(), json!({ "message": e }), None);
                }
                #[cfg(not(desktop))]
                self.log("desktop.menu: menus are not supported on mobile");
            }
            // desktop.tray(...): { op: "set" | "remove", id, … }
            "desktop:tray" => {
                #[cfg(desktop)]
                {
                    let r = match data.get("op").and_then(Value::as_str) {
                        Some("remove") => {
                            crate::tray::remove(app, data.get("id").and_then(Value::as_str).unwrap_or("main"));
                            Ok(())
                        }
                        _ => crate::tray::set(app, &data),
                    };
                    if let Err(e) = r {
                        self.log(&e);
                        self.send_event(app, "desktop:error".to_string(), json!({ "message": e }), None);
                    }
                }
                #[cfg(not(desktop))]
                self.log("desktop.tray: tray icons are not supported on mobile");
            }
            "desktop:eval" => {
                if let Some(js) = data.get("js").and_then(Value::as_str) {
                    match data.get("window").and_then(Value::as_str) {
                        Some("*") => {
                            for l in app.webview_windows().keys() {
                                eval_in(app, l, js);
                            }
                        }
                        Some(l) => {
                            eval_in(app, l, js);
                        }
                        None => {
                            eval_in(app, "main", js);
                        }
                    }
                }
            }
            "desktop:log" => self.log(data.as_str().unwrap_or(&data.to_string())),
            "desktop:quit" => app.exit(data.get("code").and_then(Value::as_i64).unwrap_or(0) as i32),
            // a request of a runtime that needs an answer (dialogs, workers…):
            // answered in that runtime
            "desktop:request" => {
                let b = Arc::clone(self);
                let app = app.clone();
                std::thread::spawn(move || {
                    let id = data.get("id").cloned().unwrap_or(Value::Null);
                    let op = data.get("op").and_then(Value::as_str).unwrap_or("").to_string();
                    let args = data.get("args").cloned().unwrap_or(Value::Null);
                    let r = match op.as_str() {
                        "worker.start" => b.worker_start(from, &args),
                        "worker.call" => b.worker_call(&args),
                        "worker.stop" => b.worker_stop(&args),
                        _ => crate::window::request(&app, &op, &args),
                    };
                    let reply = match r {
                        Ok(v) => json!({ "id": id, "ok": true, "value": v }),
                        Err(e) => json!({ "id": id, "ok": false, "error": e }),
                    };
                    let Some(rt) = b.runtimes.lock().unwrap().get(&from).cloned() else { return };
                    if let Err(e) = rt.emit("desktop:reply", &reply.to_string()) {
                        b.log(&format!("cannot answer {}: {}", op, e));
                    }
                });
            }
            // any other Fogu.host.emit(name, data) is an event for the page
            other => self.send_event(app, other.to_string(), data, None),
        }
    }

    /// An event for the pages: every window, or only `target`. A window whose
    /// page is not ready yet gets it when it is.
    pub fn send_event(&self, app: &AppHandle, name: String, data: Value, target: Option<&str>) {
        // also a Tauri event, for pages that use window.__TAURI__.event.listen
        match target {
            Some(l) => {
                let _ = app.emit_to(l, &name, data.clone());
            }
            None => {
                let _ = app.emit(&name, data.clone());
            }
        }
        let labels: Vec<String> = match target {
            Some(l) => vec![l.to_string()],
            None => app.webview_windows().keys().cloned().collect(),
        };
        let js = format!("window.fogu && window.fogu.__dispatch({}, {})", json!(name), data);
        let ev = json!({ "name": name, "data": data });
        let mut ready = self.ready.lock().unwrap();
        for l in labels {
            if ready.get(&l).copied().unwrap_or(false) && eval_in(app, &l, &js) {
                continue;
            }
            ready.insert(l.clone(), false);
            let mut queues = self.queue.lock().unwrap();
            let q = queues.entry(l).or_default();
            // a page that never loads must not grow the queue forever
            if q.len() >= 10_000 {
                q.remove(0);
            }
            q.push(ev.clone());
        }
    }
}

/// Whether the program is hot-reloaded (`fogu desktop dev`).
fn reloads() -> bool {
    std::env::var("FOGU_DESKTOP_RELOAD").map(|v| !v.trim().is_empty()).unwrap_or(false)
}

/// The modules of the standard library areas a host may be built without (as
/// MODULE_AREAS in tools/build.fg): (area, modules).
const MODULE_AREAS: &[(&str, &[&str])] = &[
    ("database", &["etcd", "kv", "surrealkv", "surrealdb", "sql", "orm", "redis", "scylla"]),
    ("rpc", &["grpc", "protobuf"]),
    ("network", &["security", "tls", "socketio", "http", "web", "topcoat", "jwt", "mail", "websocket", "webrtc", "html"]),
    ("messaging", &["broker", "kafka", "mq", "fogumq", "foguMQ-client", "nats", "rabbitmq", "mqtt", "bullmq"]),
    ("ai", &["ai", "plot"]),
    ("ops", &["s3", "docker", "service", "playbook", "ssh"]),
    ("media", &["image", "webview"]),
];

/// For "unknown built-in module: 'fogu:ssh'" (the module is in an area this
/// host was built without): which area, and how to add it.
pub fn area_hint(msg: &str) -> Option<String> {
    const KEY: &str = "unknown built-in module: 'fogu:";
    let at = msg.find(KEY)?;
    let module = msg[at + KEY.len()..].split('\'').next()?;
    let (area, _) = MODULE_AREAS.iter().find(|(_, ms)| ms.contains(&module))?;
    Some(format!(
        "fogu:{} is in the '{}' area, which this desktop host was built without: add \"{}\" to \"desktop.features\" in fogu.json (e.g. \"desktop\": {{ \"features\": [\"network\", \"{}\"] }}) and start the app again",
        module, area, area, area
    ))
}

/// Runs JavaScript in a window; false when there is none.
pub fn eval_in(app: &AppHandle, label: &str, js: &str) -> bool {
    match app.get_webview_window(label) {
        Some(w) => w.eval(js).is_ok(),
        None => false,
    }
}
