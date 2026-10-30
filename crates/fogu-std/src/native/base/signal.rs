//! `Fogu.signal` — operating-system signals and the program's own events.
//!
//! ```fogu
//! const app = web.app() ... app.listen(8080)
//! Fogu.signal.on("SIGHUP", fn() { reloadConfig() })
//! Fogu.signal.on("job.done", fn(data) { print("done:", data) })   // an event of the program
//! Fogu.signal.emit("job.done", { id: 7 })
//! const sig = Fogu.signal.wait()          // keeps the program alive until Ctrl+C / SIGTERM
//! print("stopping on", sig)               // "SIGINT"
//! ```
//!
//! `wait(names = ["SIGINT", "SIGTERM"], { timeout })` blocks the program
//! (servers and `fg` tasks keep running) until one of the signals or events
//! happens and returns its name (nil after `timeout`). Handlers given to
//! `on` run as `fg` tasks, with the event's data. Signals: SIGINT SIGTERM
//! SIGHUP SIGQUIT SIGUSR1 SIGUSR2 (Unix); SIGINT SIGBREAK SIGTERM (Windows).
//! With no handler and nobody waiting, SIGINT and SIGTERM still end the
//! program (exit code 130 / 143). `emit` also simulates a signal (tests).
//! `service-stop` is raised (with `SIGTERM`) when the Windows service manager
//! stops a standalone executable running as a service (toolkit/service.rs).

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched::{self, IoGuard};
use crate::runtime::value::Value;

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{duration_arg, list, map, max_args, str_arg};

#[cfg(unix)]
const OS_SIGNALS: &[&str] = &["SIGINT", "SIGTERM", "SIGHUP", "SIGQUIT", "SIGUSR1", "SIGUSR2"];
#[cfg(windows)]
const OS_SIGNALS: &[&str] = &["SIGINT", "SIGBREAK", "SIGTERM"];
#[cfg(not(any(unix, windows)))]
const OS_SIGNALS: &[&str] = &[];

/// Signals delivered by the OS (filled from the I/O runtime).
static ARRIVED: Mutex<VecDeque<&'static str>> = Mutex::new(VecDeque::new());
/// Signals with a listener installed.
static LISTENING: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

struct Event {
    seq: u64,
    name: String,
    data: Value,
    /// a `wait` returned it (so it was handled)
    claimed: bool,
}

#[derive(Default)]
struct State {
    handlers: HashMap<String, Vec<Value>>,
    /// recent events (waiters and the dispatcher read them by sequence)
    log: VecDeque<Event>,
    seq: u64,
    /// the dispatcher's position in the log
    dispatched: u64,
    pump: bool,
    waiters: Cell<u32>,
    /// while OS signals are listened to, waiting is not a deadlock
    guard: Option<IoGuard>,
    last: Option<(String, Value)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// The `Fogu.signal` object (its calls are served by `intercept`).
pub fn value() -> Value {
    NativeObjectRef::new(SignalObj).to_value()
}

pub struct SignalObj;

impl FoguObject for SignalObj {
    fn name(&self) -> &str {
        "FoguSignal"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "names" => Some(list(OS_SIGNALS.iter().map(|s| Value::Str(s.to_string().into())).collect())),
            "last" => Some(STATE.with(|s| match &s.borrow().last {
                Some((n, d)) => map(vec![("name", Value::Str(n.clone().into())), ("data", d.clone())]),
                None => Value::Nil,
            })),
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["on", "off", "emit", "wait"]
    }
}

fn os_name(name: &str) -> Option<&'static str> {
    OS_SIGNALS.iter().copied().find(|s| *s == name)
}

/// Called when something must be released if the program is stopped (an sshfs
/// mount, a SurrealKV store, a broker's db, browser processes): from then on
/// Ctrl+C / SIGTERM run `native::cleanup` before the program ends (exit code
/// 130 / 143) — unless the program listens to that signal with `Fogu.signal`,
/// whose own handling (above) also cleans up.
pub fn arm_cleanup() {
    static ARMED: std::sync::Once = std::sync::Once::new();
    ARMED.call_once(|| {
        let finish = |sig: &'static str, code: i32| {
            if LISTENING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|l| l.contains(sig)) {
                return false;
            }
            // off the runtime's threads: cleanup blocks on it
            std::thread::spawn(move || crate::native::exit(code));
            true
        };
        let rt = crate::native::rt::tokio();
        #[cfg(unix)]
        for (sig, code) in [("SIGINT", 130), ("SIGTERM", 143)] {
            use tokio::signal::unix::{signal, SignalKind};
            let kind = if sig == "SIGINT" { SignalKind::interrupt() } else { SignalKind::terminate() };
            let _g = rt.enter();
            if let Ok(mut s) = signal(kind) {
                rt.spawn(async move {
                    while s.recv().await.is_some() {
                        if finish(sig, code) {
                            break;
                        }
                    }
                });
            }
        }
        #[cfg(windows)]
        {
            let _g = rt.enter();
            if let Ok(mut s) = tokio::signal::windows::ctrl_c() {
                rt.spawn(async move {
                    while s.recv().await.is_some() {
                        if finish("SIGINT", 130) {
                            break;
                        }
                    }
                });
            }
            if let Ok(mut s) = tokio::signal::windows::ctrl_close() {
                rt.spawn(async move {
                    while s.recv().await.is_some() {
                        if finish("SIGTERM", 143) {
                            break;
                        }
                    }
                });
            }
        }
    });
}

/// The event raised when the Windows service manager stops the program
/// (a standalone executable running as a service; see `raise_stop`).
pub const SERVICE_STOP: &str = "service-stop";

/// A stop request from outside the program (the Windows service manager):
/// raises `service-stop` and `SIGTERM` for the program — the ones it listens
/// to. Returns false when it listens to neither (the caller ends the program).
pub fn raise_stop() -> bool {
    let names: Vec<&'static str> = {
        let l = LISTENING.lock().unwrap_or_else(|e| e.into_inner());
        [SERVICE_STOP, "SIGTERM"].into_iter().filter(|n| l.as_ref().is_some_and(|s| s.contains(n))).collect()
    };
    if names.is_empty() {
        return false;
    }
    ARRIVED.lock().unwrap_or_else(|e| e.into_inner()).extend(names);
    sched::wake();
    true
}

/// Starts listening to an OS signal (once).
fn listen(sig: &'static str) -> Result<(), String> {
    {
        let mut l = LISTENING.lock().unwrap_or_else(|e| e.into_inner());
        if !l.get_or_insert_with(HashSet::new).insert(sig) {
            return Ok(());
        }
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.guard.is_none() {
            s.guard = Some(IoGuard::new());
        }
    });
    // raised by `raise_stop`, not by the OS
    if sig == SERVICE_STOP {
        return Ok(());
    }
    let deliver = move || {
        ARRIVED.lock().unwrap_or_else(|e| e.into_inner()).push_back(sig);
        sched::wake();
    };
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    crate::native::rt::tokio().spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let kind = match sig {
                "SIGINT" => SignalKind::interrupt(),
                "SIGTERM" => SignalKind::terminate(),
                "SIGHUP" => SignalKind::hangup(),
                "SIGQUIT" => SignalKind::quit(),
                "SIGUSR1" => SignalKind::user_defined1(),
                _ => SignalKind::user_defined2(),
            };
            match signal(kind) {
                Ok(mut s) => {
                    let _ = tx.send(Ok(()));
                    while s.recv().await.is_some() {
                        deliver();
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        }
        #[cfg(windows)]
        {
            use tokio::signal::windows;
            macro_rules! pump {
                ($f:expr) => {
                    match $f {
                        Ok(mut s) => {
                            let _ = tx.send(Ok(()));
                            while s.recv().await.is_some() {
                                deliver();
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                        }
                    }
                };
            }
            match sig {
                "SIGINT" => pump!(windows::ctrl_c()),
                "SIGBREAK" => pump!(windows::ctrl_break()),
                _ => pump!(windows::ctrl_close()),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = deliver;
            let _ = tx.send(Err("signals are not supported here".into()));
        }
    });
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| io_err!("Fogu.signal: cannot listen to {}", sig))?
        .map_err(|e| io_err!("Fogu.signal: cannot listen to {}: {}", sig, e))
}

/// Moves the OS signals that arrived into the event log.
fn collect() {
    let arrived: Vec<&'static str> = ARRIVED.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect();
    for sig in arrived {
        push(sig.to_string(), Value::Nil);
    }
}

fn push(name: String, data: Value) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.seq += 1;
        let seq = s.seq;
        s.last = Some((name.clone(), data.clone()));
        s.log.push_back(Event { seq, name, data, claimed: false });
        while s.log.len() > 512 {
            s.log.pop_front();
        }
    });
    sched::note_progress();
}

/// The task that runs the handlers, in the order events happened.
fn start_pump(interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<(), String> {
    if STATE.with(|s| std::mem::replace(&mut s.borrow_mut().pump, true)) {
        return Ok(());
    }
    let env = Rc::clone(env);
    interp.spawn_task(Box::new(move |it| loop {
        let mut next: Option<(String, Value, bool)> = None;
        sched::wait_until(|| {
            collect();
            next = STATE.with(|s| {
                let mut s = s.borrow_mut();
                let pos = s.dispatched;
                let e = s.log.iter().find(|e| e.seq > pos).map(|e| (e.seq, e.name.clone(), e.data.clone(), e.claimed));
                e.map(|(seq, n, d, c)| {
                    s.dispatched = seq;
                    (n, d, c)
                })
            });
            next.is_some()
        })?;
        let Some((name, data, claimed)) = next else { continue };
        let (hs, waiting) = STATE.with(|s| {
            let s = s.borrow();
            (s.handlers.get(&name).cloned().unwrap_or_default(), s.waiters.get())
        });
        // nobody handles a termination signal: the usual behaviour
        if hs.is_empty() && waiting == 0 && !claimed {
            match name.as_str() {
                "SIGINT" => crate::native::exit(130),
                "SIGTERM" => crate::native::exit(143),
                _ => {}
            }
        }
        for h in hs {
            let env = Rc::clone(&env);
            // `fn() {...}` handlers do not take the data
            let args = match &h {
                Value::Fn(f) if f.params.is_empty() => vec![],
                _ => vec![data.clone()],
            };
            it.spawn_task(Box::new(move |it| it.call_value(h, args, env).map(|_| ())))?;
        }
    }))
}

/// `Fogu.signal.*` calls (they need the interpreter).
pub fn intercept(callee: &Value, args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Option<Result<Value, String>> {
    let Value::BoundMethod(bm) = callee else { return None };
    let (receiver, method) = (&bm.receiver, &bm.method);
    if receiver.name() != "FoguSignal" {
        return None;
    }
    Some(call(method, args, interp, env))
}

/// `Fogu.signal.emit` from an embedding host (`fogu_emit`).
pub fn emit_from_host(name: String, data: Value, interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<(), String> {
    push(name, data);
    start_pump(interp, env)
}

fn names_arg(v: Option<&Value>) -> Result<Vec<String>, String> {
    Ok(match v {
        None | Some(Value::Nil) => vec!["SIGINT".into(), "SIGTERM".into()],
        Some(Value::Str(s)) => vec![s.to_string()],
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        Some(Value::Map(_)) => vec!["SIGINT".into(), "SIGTERM".into()],
        Some(other) => return Err(format!("Fogu.signal.wait: expects a name or a list of names, got '{}'", other.type_name())),
    })
}

fn call(method: &str, args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    match method {
        // on(name, fn(data))
        "on" => {
            max_args(args, 2, "Fogu.signal.on")?;
            let name = str_arg(args, 0, "Fogu.signal.on")?;
            let f = args.get(1).cloned().ok_or("Fogu.signal.on(name, fn)")?;
            if let Some(sig) = os_name(&name) {
                listen(sig)?;
            } else if name == SERVICE_STOP {
                listen(SERVICE_STOP)?;
            } else if name.starts_with("SIG") {
                return Err(format!("Fogu.signal.on: '{}' is not a signal here ({})", name, OS_SIGNALS.join(" ")));
            }
            STATE.with(|s| s.borrow_mut().handlers.entry(name).or_default().push(f));
            start_pump(interp, env)?;
            Ok(Value::Nil)
        }
        // off(name): removes its handlers
        "off" => {
            max_args(args, 1, "Fogu.signal.off")?;
            let name = str_arg(args, 0, "Fogu.signal.off")?;
            STATE.with(|s| s.borrow_mut().handlers.remove(&name));
            Ok(Value::Nil)
        }
        // emit(name, data?): an event of the program (or a simulated signal)
        "emit" => {
            max_args(args, 2, "Fogu.signal.emit")?;
            let name = str_arg(args, 0, "Fogu.signal.emit")?;
            push(name, args.get(1).cloned().unwrap_or(Value::Nil));
            start_pump(interp, env)?;
            Ok(Value::Nil)
        }
        // wait(names = ["SIGINT", "SIGTERM"], { timeout }) → the name, or nil after the timeout
        "wait" => {
            max_args(args, 2, "Fogu.signal.wait")?;
            let names = names_arg(args.first())?;
            let opts = match args.last() {
                Some(Value::Map(m)) => Some(m.borrow().clone()),
                _ => None,
            };
            let deadline = match opts.as_ref().and_then(|o| o.get("timeout").cloned()) {
                Some(t) => Some(Instant::now() + duration_arg(&t, "Fogu.signal.wait timeout")?),
                None => None,
            };
            for n in &names {
                if let Some(sig) = os_name(n) {
                    listen(sig)?;
                } else if n == SERVICE_STOP {
                    listen(SERVICE_STOP)?;
                }
            }
            start_pump(interp, env)?;
            let from = STATE.with(|s| {
                let s = s.borrow();
                s.waiters.set(s.waiters.get() + 1);
                s.seq
            });
            let mut got: Option<String> = None;
            let r = loop {
                #[allow(unused_mut)]
                let mut http = false;
                let r = sched::wait_until(|| {
                    collect();
                    got = STATE.with(|s| {
                        let mut s = s.borrow_mut();
                        let e = s.log.iter_mut().find(|e| e.seq > from && names.contains(&e.name))?;
                        e.claimed = true;
                        Some(e.name.clone())
                    });
                    if got.is_some() {
                        return true;
                    }
                    // requests of a fogu:web / fogu:http server are answered while waiting
                    #[cfg(feature = "network")]
                    if crate::native::http::has_pending_request() {
                        http = true;
                        return true;
                    }
                    match deadline {
                        Some(d) if Instant::now() >= d => true,
                        Some(d) => {
                            sched::add_timer(d);
                            false
                        }
                        None => false,
                    }
                });
                if r.is_err() || !http || got.is_some() {
                    break r;
                }
                #[cfg(feature = "network")]
                crate::native::http::serve_pending(interp, env);
            };
            STATE.with(|s| {
                let s = s.borrow();
                s.waiters.set(s.waiters.get().saturating_sub(1));
            });
            r?;
            Ok(got.map(Value::str).unwrap_or(Value::Nil))
        }
        other => Err(format!("method '{}' does not exist on Fogu.signal (on off emit wait)", other)),
    }
}
