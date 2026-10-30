//! `Fogu.watch(paths, options?)` — file system events (on `notify`).
//!
//! ```fogu
//! const w = Fogu.watch("src")?                     // a file, a folder or a list of them
//! while true {
//!     const ev = w.next()?                         // { kind, path, paths } · nil on timeout/close
//!     print(ev.kind, ev.path)                      // create modify remove rename
//! }
//! ```
//!
//! Events of the same file that arrive within `debounce` (default 50ms) are
//! merged. `next` waits cooperatively: other `fg` tasks keep running.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::runtime::sched;
use crate::runtime::value::Value;

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, list, map, max_args, path_str};

#[derive(Clone, Debug)]
struct Ev {
    kind: &'static str,
    paths: Vec<PathBuf>,
    at: Instant,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Ev>,
    error: Option<String>,
}

fn kind_of(k: &EventKind) -> Option<&'static str> {
    use notify::event::{ModifyKind, RenameMode};
    Some(match k {
        EventKind::Create(_) => "create",
        EventKind::Remove(_) => "remove",
        EventKind::Modify(ModifyKind::Name(RenameMode::Any | RenameMode::Both | RenameMode::From | RenameMode::To)) => "rename",
        EventKind::Modify(ModifyKind::Metadata(_)) => return None,
        EventKind::Modify(_) => "modify",
        EventKind::Any => "modify",
        _ => return None,
    })
}

/// `Fogu.watch(path | [paths], { recursive = true, debounce = 50ms })`
pub fn watch(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "Fogu.watch")?;
    let paths: Vec<String> = match args.first() {
        Some(Value::Str(p)) => vec![p.to_string()],
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        _ => return Err("Fogu.watch expects a path or a list of paths".into()),
    };
    let (mut recursive, mut debounce) = (true, Duration::from_millis(50));
    if let Some(Value::Map(o)) = args.get(1) {
        let o = o.borrow();
        if let Some(v) = o.get("recursive") {
            recursive = v.is_truthy();
        }
        if let Some(v) = o.get("debounce").filter(|v| !matches!(v, Value::Nil)) {
            debounce = duration_arg(v, "Fogu.watch: debounce")?;
        }
    }
    let state = Arc::new(Mutex::new(State::default()));
    let st = Arc::clone(&state);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let mut s = st.lock().unwrap();
        match res {
            Ok(ev) => {
                if let Some(kind) = kind_of(&ev.kind) {
                    s.queue.push_back(Ev { kind, paths: ev.paths, at: Instant::now() });
                }
            }
            Err(e) => s.error = Some(e.to_string()),
        }
    })
    .map_err(|e| io_err!("Fogu.watch: {}", e))?;
    let mode = if recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
    let mut watched = Vec::new();
    for p in &paths {
        let pb = PathBuf::from(p);
        watcher.watch(&pb, mode).map_err(|e| io_err!("Fogu.watch: {}: {}", p, e))?;
        watched.push(path_str(&pb.canonicalize().unwrap_or(pb)));
    }
    Ok(NativeObjectRef::new(WatchValue { _watcher: Some(watcher), state, debounce, watched }).to_value())
}

pub struct WatchValue {
    _watcher: Option<notify::RecommendedWatcher>,
    state: Arc<Mutex<State>>,
    debounce: Duration,
    watched: Vec<String>,
}

/// Paths as shown to the program (without the `\\?\` prefix).
fn shown(p: &std::path::Path) -> String {
    let s = path_str(p);
    s.strip_prefix("//?/").map(String::from).unwrap_or(s)
}

fn event_value(e: Ev) -> Value {
    let paths: Vec<Value> = e.paths.iter().map(|p| Value::Str(shown(p).into())).collect();
    map(vec![
        ("kind", Value::Str(e.kind.into())),
        ("path", paths.first().cloned().unwrap_or(Value::Nil)),
        ("paths", list(paths)),
    ])
}

/// Takes the next event once it is older than the debounce window, merging
/// the later events of the same paths into it.
fn take(state: &Mutex<State>, debounce: Duration) -> Option<Ev> {
    let mut s = state.lock().unwrap();
    let first = s.queue.front()?.clone();
    if first.at.elapsed() < debounce {
        return None;
    }
    s.queue.pop_front();
    let mut ev = first;
    // a create followed by modifies is still a create; a remove wins
    s.queue.retain(|later| {
        if later.paths == ev.paths && later.at.duration_since(ev.at) < debounce {
            if later.kind == "remove" || later.kind == "rename" {
                ev.kind = later.kind;
            }
            false
        } else {
            true
        }
    });
    Some(ev)
}

impl FoguObject for WatchValue {
    fn name(&self) -> &str {
        "Watcher"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "paths" => crate::native::util::str_list(self.watched.clone()),
            "active" => Value::Bool(self._watcher.is_some()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "tryNext" => {
                max_args(args, 0, "tryNext")?;
                Ok(take(&self.state, self.debounce).map_or(Value::Nil, event_value))
            }
            "close" => {
                self._watcher = None;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Watcher (next tryNext close)", other)),
        }
    }

    /// `next(timeout?)` → event, nil on timeout or after `close()`.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        let deadline = match (max_args(args, 1, "next"), args.first()) {
            (Err(e), _) => return Some(Err(e)),
            (_, None | Some(Value::Nil)) => None,
            (_, Some(v)) => match duration_arg(v, "next") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
        };
        if self._watcher.is_none() {
            return Some(Ok(Box::new(|| Some(Ok(Value::Nil)))));
        }
        let (state, debounce) = (Arc::clone(&self.state), self.debounce);
        Some(Ok(Box::new(move || {
            if let Some(e) = take(&state, debounce) {
                return Some(Ok(event_value(e)));
            }
            if let Some(err) = state.lock().unwrap().error.take() {
                return Some(Err(io_err!("Fogu.watch: {}", err)));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            sched::add_timer(Instant::now() + Duration::from_millis(10));
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "close"]
    }
}
