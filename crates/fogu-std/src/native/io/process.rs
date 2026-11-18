//! `fogu:process` module — external processes and the current process (spec §52).
//!
//! ```fogu
//! import process from "fogu:process"
//!
//! const r = process.run("git", ["status", "--short"])?
//! if r.ok { print(r.stdout) } else { print("exit code", r.code, r.stderr) }
//! print(process.shell("echo hello").stdout)
//! ```
//!
//! A non-zero exit code is *not* an error: check `r.ok` / `r.code`. Failing to
//! start the program (not found, no permission) is an operational error.

use ahash::AHashMap as HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::runtime::value::Str;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::util::{map, max_args, nfn_var, opt_arg, str_arg, str_list};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();

    e.insert("run".into(), nfn_var("run", 1, run));
    e.insert("shell".into(), nfn_var("shell", 1, |a| {
        max_args(a, 2, "process.shell")?;
        let line = str_arg(a, 0, "process.shell")?;
        let (prog, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
        let args = vec![
            Value::Str(prog.into()),
            crate::native::util::str_list([flag.to_string(), line]),
            a.get(1).cloned().unwrap_or(Value::Nil),
        ];
        run(&args)
    }));
    e.insert("env".into(), nfn_var("env", 0, |a| {
        max_args(a, 1, "process.env")?;
        match opt_arg(a, 0) {
            None => Ok(env_map()),
            Some(_) => {
                let k = str_arg(a, 0, "process.env")?;
                Ok(std::env::var(&k).map(Value::str).unwrap_or(Value::Nil))
            }
        }
    }));
    e.insert("setEnv".into(), nfn("setEnv", 2, |a| {
        let k = str_arg(a, 0, "process.setEnv")?;
        let v = str_arg(a, 1, "process.setEnv")?;
        // SAFETY: Fogu programs run on a single interpreter thread.
        unsafe { std::env::set_var(k, v) };
        Ok(Value::Nil)
    }));
    e.insert("cwd".into(), nfn("cwd", 0, |_| {
        std::env::current_dir()
            .map(|p| Value::Str(crate::native::util::path_str(&p).into()))
            .map_err(|err| io_err!("process.cwd: {}", err))
    }));
    e.insert("chdir".into(), nfn("chdir", 1, |a| {
        let p = str_arg(a, 0, "process.chdir")?;
        std::env::set_current_dir(&p)
            .map(|_| Value::Nil)
            .map_err(|err| io_err!("process.chdir: {}: {}", p, err))
    }));
    e.insert("exit".into(), nfn_var("exit", 0, |a| {
        let code = match opt_arg(a, 0) {
            None => 0,
            Some(Value::Int(n)) => *n as i32,
            Some(other) => return Err(format!("process.exit expects int, got '{}'", other.type_name())),
        };
        crate::native::exit_program(code)
    }));
    e.insert("pid".into(), Value::Int(crate::native::util::pid() as i64));
    e.insert("os".into(), Value::Str(std::env::consts::OS.into()));
    e.insert("arch".into(), Value::Str(std::env::consts::ARCH.into()));
    e.insert("args".into(), str_list(crate::native::system::script_args()));

    ModuleValue {
        name: "fogu:process".into(),
        exports: std::cell::RefCell::new(e),
    }
}

/// Environment variables as a map.
pub fn env_map() -> Value {
    let mut pairs: Vec<(String, Value)> = std::env::vars().map(|(k, v)| (k, Value::Str(v.into()))).collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let m: HashMap<Str, Value> = pairs.into_iter().map(|(k, v)| (Str::from(k), v)).collect();
    Value::Map(std::rc::Rc::new(std::cell::RefCell::new(m)))
}

/// `run(cmd, args = [], options = {cwd, env, input})` →
/// `{ ok, code, stdout, stderr }`.
pub fn run(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "process.run")?;
    let cmd = str_arg(a, 0, "process.run")?;
    let mut c = Command::new(&cmd);
    match opt_arg(a, 1) {
        None => {}
        Some(Value::List(l)) => {
            for v in l.borrow().iter() {
                c.arg(match v {
                    Value::Str(s) => s.clone(),
                    other => other.to_display_string().into(),
                });
            }
        }
        Some(other) => {
            return Err(format!("process.run: args must be a list, got '{}'", other.type_name()))
        }
    }
    let mut input: Option<Vec<u8>> = None;
    match opt_arg(a, 2) {
        None => {}
        Some(Value::Map(m)) => {
            let m = m.borrow();
            // an unknown option (`timeout`, a typo) stops the program instead of being ignored
            let mut keys: Vec<&str> = m.keys().map(|k| &**k).collect();
            keys.sort_unstable();
            if let Some(k) = keys.into_iter().find(|k| !matches!(*k, "cwd" | "env" | "input")) {
                return Err(format!("process.run: unknown option '{}' (expected cwd, env, input)", k));
            }
            match m.get("cwd") {
                None | Some(Value::Nil) => {}
                Some(Value::Str(dir)) => {
                    // checked here: the spawn error would name the program, not the folder
                    if !std::path::Path::new(&**dir).is_dir() {
                        return Err(io_err!("process.run: cwd {}: not a directory", dir));
                    }
                    c.current_dir(&**dir);
                }
                Some(other) => return Err(format!("process.run: 'cwd' must be a str, got '{}'", other.type_name())),
            }
            match m.get("env") {
                None | Some(Value::Nil) => {}
                Some(Value::Map(env)) => {
                    for (k, v) in env.borrow().iter() {
                        c.env(&**k, v.to_display_string());
                    }
                }
                Some(other) => return Err(format!("process.run: 'env' must be a map, got '{}'", other.type_name())),
            }
            match m.get("input") {
                None | Some(Value::Nil) => {}
                // a str as UTF-8, a buffer as its bytes
                Some(v) => input = Some(crate::native::util::bytes_of(v, "process.run: input")?),
            }
        }
        Some(other) => {
            return Err(format!(
                "process.run: options must be a map {{cwd, env, input}}, got '{}'",
                other.type_name()
            ))
        }
    }
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // the child runs off the interpreter thread: other `fg` tasks keep going
    let out = crate::native::rt::offload(move || {
        let mut child = c.spawn().map_err(|err| io_err!("process.run: {}: {}", cmd, err))?;
        // stdin is fed from its own thread, so a child that writes a lot
        // before reading everything cannot deadlock us
        let feeder = match (input, child.stdin.take()) {
            (Some(data), Some(mut stdin)) => Some(std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            })),
            _ => None,
        };
        let out = child.wait_with_output().map_err(|err| io_err!("process.run: {}: {}", cmd, err));
        if let Some(f) = feeder {
            let _ = f.join();
        }
        out
    })??;
    let code = out.status.code().unwrap_or(-1);
    Ok(map(vec![
        ("ok", Value::Bool(out.status.success())),
        ("code", Value::Int(code as i64)),
        ("stdout", Value::Str(String::from_utf8_lossy(&out.stdout).to_string().into())),
        ("stderr", Value::Str(String::from_utf8_lossy(&out.stderr).to_string().into())),
    ]))
}
