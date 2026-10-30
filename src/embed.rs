//! Embedding the Fogu runtime in another program. [`Runtime`] owns a
//! thread (with the large stack the interpreter needs) that keeps one
//! interpreter alive: code evaluated by the host shares its globals, and
//! servers, `fg` tasks and `Fogu.signal` handlers keep running between the
//! host's calls. The C API over it lives in `ffi/` (`libfogu_rt`).
//!
//! ```no_run
//! let rt = fogu::embed::Runtime::new().unwrap();
//! rt.eval("let total = 40").unwrap();
//! assert_eq!(rt.eval("total + 2").unwrap(), "42");
//! rt.eval("fn double(x) { return x * 2 }").unwrap();
//! assert_eq!(rt.call("double", "[21]").unwrap(), "42");
//! ```
//!
//! Values cross as JSON text.

use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::native::host::HostCallback;
use crate::runtime::value::{Value, ERR_VALUE_PREFIX};
use crate::runtime::{sched, Interpreter, Lexer, Parser, MAIN_STACK};

type Reply = Sender<Result<String, String>>;

enum Cmd {
    Eval(String, Reply),
    Spawn(String, Reply),
    RunFile(String, Reply),
    Call(String, String, Reply),
    Get(String, Reply),
    Set(String, String, Reply),
    Emit(String, String, Reply),
    Host(Option<(HostCallback, usize)>, Reply),
    Stop,
}

/// A Fogu runtime on its own thread.
pub struct Runtime {
    tx: Sender<Cmd>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn plain(e: String) -> String {
    e.strip_prefix(ERR_VALUE_PREFIX).map(str::to_string).unwrap_or(e)
}

fn json(v: &Value) -> String {
    match v {
        // a returned error value is an error for the host
        Value::Nil => "null".into(),
        v => crate::native::json::stringify_value(v),
    }
}

fn parse(code: &str) -> Result<Rc<Vec<crate::runtime::ast::Stmt>>, String> {
    let tokens = Lexer::new(code).tokenize()?;
    Ok(Rc::new(Parser::new(tokens).parse_program()?))
}

fn result_of(r: Result<Value, String>) -> Result<String, String> {
    match r {
        Err(e) => Err(plain(e)),
        Ok(Value::Error(e)) => Err(e.message.clone()),
        Ok(v) => Ok(json(&v)),
    }
}

impl Runtime {
    /// Starts a runtime (its working directory is the process' one).
    pub fn new() -> Result<Runtime, String> {
        let (tx, rx) = channel::<Cmd>();
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let thread = std::thread::Builder::new()
            .name("fogu-embed".into())
            .stack_size(MAIN_STACK)
            .spawn(move || {
                sched::init_main_stack(MAIN_STACK);
                crate::register();
                let interp = Interpreter::new();
                let cwd = std::env::current_dir().unwrap_or_default();
                interp.set_base_dir(&cwd);
                interp.set_main_file("<embed>");
                crate::pkg::set_main_dir(&cwd);
                crate::native::host::set_embedded(true);
                let _ = ready_tx.send(Ok(()));
                serve(&interp, rx);
                crate::native::host::set_callback(None);
                sched::shutdown();
            })
            .map_err(|e| format!("cannot start the runtime thread: {}", e))?;
        ready_rx.recv().map_err(|_| "the runtime thread did not start".to_string())??;
        Ok(Runtime { tx, thread: Some(thread) })
    }

    fn ask(&self, make: impl FnOnce(Reply) -> Cmd) -> Result<String, String> {
        let (tx, rx) = channel();
        self.tx.send(make(tx)).map_err(|_| "the runtime stopped".to_string())?;
        rx.recv().map_err(|_| "the runtime stopped".to_string())?
    }

    /// Runs code in the global scope; the value of its last expression as JSON.
    pub fn eval(&self, code: &str) -> Result<String, String> {
        self.ask(|r| Cmd::Eval(code.to_string(), r))
    }

    /// Starts code as an `fg` task and returns at once (servers, loops…).
    pub fn spawn(&self, code: &str) -> Result<String, String> {
        self.ask(|r| Cmd::Spawn(code.to_string(), r))
    }

    /// Runs a file (its imports are relative to it) in the global scope.
    pub fn run_file(&self, path: &str) -> Result<String, String> {
        self.ask(|r| Cmd::RunFile(path.to_string(), r))
    }

    /// Calls a global function with a JSON array of arguments.
    pub fn call(&self, name: &str, args_json: &str) -> Result<String, String> {
        self.ask(|r| Cmd::Call(name.to_string(), args_json.to_string(), r))
    }

    pub fn get(&self, name: &str) -> Result<String, String> {
        self.ask(|r| Cmd::Get(name.to_string(), r))
    }

    pub fn set(&self, name: &str, value_json: &str) -> Result<String, String> {
        self.ask(|r| Cmd::Set(name.to_string(), value_json.to_string(), r))
    }

    /// Fires `Fogu.signal.on(name, fn(data))` handlers.
    pub fn emit(&self, name: &str, data_json: &str) -> Result<String, String> {
        self.ask(|r| Cmd::Emit(name.to_string(), data_json.to_string(), r))
    }

    /// The callback behind `Fogu.host.emit(name, data)` (`user` is passed back).
    pub fn on_host(&self, cb: Option<HostCallback>, user: usize) -> Result<String, String> {
        self.ask(|r| Cmd::Host(cb.map(|c| (c, user)), r))
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The runtime thread: commands, and between them a short cooperative
/// sleep (it serves HTTP requests and runs the other tasks).
fn serve(interp: &Interpreter, rx: Receiver<Cmd>) {
    let delay = crate::native::time::build().exports.borrow().get("delay").cloned();
    let globals = interp.globals_env();
    loop {
        let cmd = match rx.recv_timeout(Duration::from_millis(0)) {
            Ok(c) => c,
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                if sched::pending_tasks() == 0 && !http_serving() {
                    // nothing runs in the background: just wait for the host
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(c) => c,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                } else {
                    if let Some(d) = &delay {
                        let _ = interp.call_value(d.clone(), vec![Value::Int(5)], Rc::clone(&globals));
                    }
                    continue;
                }
            }
        };
        match cmd {
            Cmd::Stop => return,
            Cmd::Eval(code, r) => {
                let out = parse(&code).and_then(|p| interp.embed_eval(&p)).map(|v| v.unwrap_or(Value::Nil));
                let _ = r.send(result_of(out));
            }
            Cmd::Spawn(code, r) => {
                let out = parse(&code).and_then(|p| {
                    interp.spawn_task(Box::new(move |it| it.embed_eval(&p).map(|_| ())))?;
                    Ok(Value::Nil)
                });
                let _ = r.send(result_of(out));
            }
            Cmd::RunFile(path, r) => {
                let out = std::fs::read_to_string(&path).map_err(|e| format!("{}: {}", path, e)).and_then(|src| {
                    let dir = Path::new(&path).parent().map(|d| d.to_path_buf()).unwrap_or_default();
                    let dir = if dir.as_os_str().is_empty() { std::env::current_dir().unwrap_or_default() } else { dir };
                    interp.set_base_dir(&dir);
                    interp.set_main_file(&path);
                    parse(&src).and_then(|p| interp.embed_eval(&p)).map(|v| v.unwrap_or(Value::Nil))
                });
                let _ = r.send(result_of(out));
            }
            Cmd::Call(name, args, r) => {
                let out = (|| {
                    let f = interp.global(&name).ok_or_else(|| format!("no global function '{}'", name))?;
                    let args = match crate::native::json::parse_value(if args.trim().is_empty() { "[]" } else { &args })? {
                        Value::List(l) => l.borrow().clone(),
                        Value::Nil => Vec::new(),
                        other => vec![other],
                    };
                    interp.call_value(f, args, Rc::clone(&globals))
                })();
                let _ = r.send(result_of(out));
            }
            Cmd::Get(name, r) => {
                let _ = r.send(interp.global(&name).map(|v| json(&v)).ok_or_else(|| format!("no global '{}'", name)));
            }
            Cmd::Set(name, value, r) => {
                let out = crate::native::json::parse_value(&value).map(|v| {
                    interp.set_global(&name, v);
                    "null".to_string()
                });
                let _ = r.send(out.map_err(plain));
            }
            Cmd::Emit(name, data, r) => {
                let out = crate::native::json::parse_value(if data.trim().is_empty() { "null" } else { &data })
                    .and_then(|d| crate::native::signal::emit_from_host(name, d, interp, &globals))
                    .map(|_| "null".to_string());
                let _ = r.send(out.map_err(plain));
            }
            Cmd::Host(cb, r) => {
                crate::native::host::set_callback(cb);
                let _ = r.send(Ok("null".into()));
            }
        }
    }
}

/// A `fogu:web` / `fogu:http` server is running (its requests need the loop).
fn http_serving() -> bool {
    #[cfg(feature = "network")]
    return crate::native::http::serving();
    #[cfg(not(feature = "network"))]
    false
}
