use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};

static SCRIPT_ARGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The arguments passed to the script.
pub fn script_args() -> Vec<String> {
    SCRIPT_ARGS.lock().unwrap().clone()
}

/// Arguments passed to the script (`fogu script.fg a b c`).
pub fn set_script_args(args: Vec<String>) {
    *SCRIPT_ARGS.lock().unwrap() = args;
}

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    exports.insert(
        "args".into(),
        Value::List(Rc::new(RefCell::new(
            SCRIPT_ARGS.lock().unwrap().iter().map(|a| Value::Str(a.clone().into())).collect(),
        ))),
    );

    // `fogu:system` exposes a singleton instance of `SystemInfo`.
    exports.insert(
        "info".into(),
        NativeObjectRef::new(SystemInfo).to_value(),
    );

    exports.insert(
        "time".into(),
        nfn("time", 0, |_| {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs_f64();
            Ok(Value::Float(secs))
        }),
    );

    // readLine() → the next line of stdin (without the line ending), nil at the end;
    // other fg tasks keep running while it waits
    exports.insert(
        "readLine".into(),
        nfn("readLine", 0, |_| {
            let r = crate::native::rt::offload(|| {
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) => Ok(None),
                    Ok(_) => Ok(Some(line)),
                    Err(e) => Err(e.to_string()),
                }
            })?;
            match r {
                Ok(Some(line)) => Ok(Value::Str(line.trim_end_matches(['\n', '\r']).to_string().into())),
                Ok(None) => Ok(Value::Nil),
                Err(e) => Err(io_err!("system.readLine: {}", e)),
            }
        }),
    );

    exports.insert(
        "exit".into(),
        nfn("exit", 1, |args| {
            let code = match &args[0] {
                Value::Int(n) => *n,
                _ => 0,
            };
            crate::native::exit(code as i32);
        }),
    );

    ModuleValue {
        name: "fogu:system".into(),
        exports: RefCell::new(exports),
    }
}

// ---------- Native object ----------

pub struct SystemInfo;

impl FoguObject for SystemInfo {
    fn name(&self) -> &str {
        "SystemInfo"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "os" => Some(Value::Str(std::env::consts::OS.to_string().into())),
            "arch" => Some(Value::Str(std::env::consts::ARCH.to_string().into())),
            "family" => Some(Value::Str(std::env::consts::FAMILY.to_string().into())),
            "cores" => Some(Value::Int(
                std::thread::available_parallelism()
                    .map(|n| n.get() as i64)
                    .unwrap_or(1),
            )),
            "pid" => Some(Value::Int(crate::native::util::pid() as i64)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "env" => {
                if args.len() != 1 {
                    return Err("env(key) expects 1 argument".into());
                }
                match &args[0] {
                    Value::Str(k) => Ok(std::env::var(k)
                        .map(Value::str)
                        .unwrap_or(Value::Nil)),
                    other => Err(format!("env expects str, got '{}'", other.type_name())),
                }
            }
            "args" => {
                let v: Vec<Value> = std::env::args()
                    .map(Value::str)
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(v))))
            }
            "sleep_ms" => {
                if args.len() != 1 {
                    return Err("sleep_ms(ms) expects 1 argument".into());
                }
                match &args[0] {
                    Value::Int(n) if *n >= 0 => {
                        std::thread::sleep(std::time::Duration::from_millis(*n as u64));
                        Ok(Value::Nil)
                    }
                    other => Err(format!(
                        "sleep_ms expects int >= 0, got '{}'",
                        other.type_name()
                    )),
                }
            }
            other => Err(format!("method '{}' does not exist on SystemInfo", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["env", "args", "sleep_ms"]
    }
}