//! `fogu:log` module — structured logging with levels, fields, child
//! loggers, text or JSON lines, to stderr, stdout or a file.
//!
//! ```fogu
//! import log from "fogu:log"
//!
//! log.info("server started", { port: 3000 })
//! // 2026-10-10T12:00:00.000Z INFO  server started port=3000
//! log.setLevel("debug")                // or FOGU_LOG=debug
//! log.setFormat("json")                // {"time":"…","level":"info","msg":"…","port":3000}
//! log.setOutput("logs/app.log")        // "stderr" (default), "stdout" or a file (appended)
//! const db = log.child({ module: "db" })
//! db.warn("slow query", { ms: 812 })
//! ```

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::io::{IsTerminal, Write};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{max_args, nfn_var, str_arg};

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Off,
}

impl Level {
    fn parse(s: &str) -> Option<Level> {
        Some(match s.to_lowercase().as_str() {
            "trace" => Level::Trace,
            "debug" => Level::Debug,
            "info" => Level::Info,
            "warn" | "warning" => Level::Warn,
            "error" => Level::Error,
            "off" | "none" | "silent" => Level::Off,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
            Level::Off => "off",
        }
    }
    fn color(self) -> &'static str {
        match self {
            Level::Trace => "\x1b[90m",
            Level::Debug => "\x1b[36m",
            Level::Info => "\x1b[32m",
            Level::Warn => "\x1b[33m",
            Level::Error => "\x1b[31m",
            Level::Off => "",
        }
    }
}

enum Output {
    Stderr,
    Stdout,
    File(std::fs::File),
}

struct Config {
    level: Level,
    json: bool,
    output: Output,
    time: bool,
}

thread_local! {
    static CONFIG: RefCell<Config> = RefCell::new(Config {
        level: std::env::var("FOGU_LOG").ok().and_then(|l| Level::parse(&l)).unwrap_or(Level::Info),
        json: std::env::var("FOGU_LOG_FORMAT").is_ok_and(|f| f == "json"),
        output: Output::Stderr,
        time: true,
    });
}

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    for lvl in ["trace", "debug", "info", "warn", "error"] {
        e.insert(lvl.into(), nfn_var(lvl, 1, level_fn(lvl)));
    }
    e.insert("setLevel".into(), nfn_var("setLevel", 1, |a| {
        max_args(a, 1, "setLevel")?;
        let l = str_arg(a, 0, "setLevel")?;
        let level = Level::parse(&l).ok_or_else(|| format!("log.setLevel: unknown level '{}' (trace debug info warn error off)", l))?;
        CONFIG.with(|c| c.borrow_mut().level = level);
        Ok(Value::Nil)
    }));
    e.insert("level".into(), nfn_var("level", 0, |_| Ok(Value::Str(CONFIG.with(|c| c.borrow().level.name().to_string()).into()))));
    e.insert("setFormat".into(), nfn_var("setFormat", 1, |a| {
        max_args(a, 1, "setFormat")?;
        let json = match str_arg(a, 0, "setFormat")?.as_str() {
            "json" => true,
            "text" => false,
            other => return Err(format!("log.setFormat: '{}' (text or json)", other)),
        };
        CONFIG.with(|c| c.borrow_mut().json = json);
        Ok(Value::Nil)
    }));
    e.insert("setOutput".into(), nfn_var("setOutput", 1, |a| {
        max_args(a, 1, "setOutput")?;
        let out = match str_arg(a, 0, "setOutput")?.as_str() {
            "stderr" => Output::Stderr,
            "stdout" => Output::Stdout,
            path => {
                if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| io_err!("log.setOutput: {}: {}", path, e))?;
                }
                let f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| io_err!("log.setOutput: {}: {}", path, e))?;
                Output::File(f)
            }
        };
        CONFIG.with(|c| c.borrow_mut().output = out);
        Ok(Value::Nil)
    }));
    e.insert("setTime".into(), nfn_var("setTime", 1, |a| {
        max_args(a, 1, "setTime")?;
        let on = a[0].is_truthy();
        CONFIG.with(|c| c.borrow_mut().time = on);
        Ok(Value::Nil)
    }));
    e.insert("child".into(), nfn_var("child", 1, |a| {
        max_args(a, 1, "child")?;
        Ok(NativeObjectRef::new(Logger { fields: fields_of(a.first(), "child")? }).to_value())
    }));
    ModuleValue {
        name: "fogu:log".into(),
        exports: RefCell::new(e),
    }
}

fn level_fn(name: &str) -> fn(&[Value]) -> Result<Value, String> {
    match name {
        "trace" => |a| emit(Level::Trace, &[], a),
        "debug" => |a| emit(Level::Debug, &[], a),
        "info" => |a| emit(Level::Info, &[], a),
        "warn" => |a| emit(Level::Warn, &[], a),
        _ => |a| emit(Level::Error, &[], a),
    }
}

fn fields_of(v: Option<&Value>, what: &str) -> Result<Vec<(String, Value)>, String> {
    match v {
        None | Some(Value::Nil) => Ok(Vec::new()),
        Some(Value::Map(m)) => {
            let mut f: Vec<(String, Value)> = m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            f.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(f)
        }
        // an error value logs its message
        Some(Value::Error(e)) => Ok(vec![("error".into(), Value::Str(e.message.clone().into()))]),
        Some(other) => Err(format!("log.{}: fields must be a map, got '{}'", what, other.type_name())),
    }
}

fn now_iso() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", y, m, d, rem / 3600, rem % 3600 / 60, rem % 60, ms.rem_euclid(1000))
}

/// `level(message, fields?)`; extra fields come from child loggers.
fn emit(level: Level, base: &[(String, Value)], a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, level.name())?;
    let enabled = CONFIG.with(|c| level >= c.borrow().level && c.borrow().level != Level::Off);
    if !enabled {
        return Ok(Value::Nil);
    }
    let msg = match a.first() {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Error(e)) => e.message.clone().into(),
        Some(v) => v.to_display_string().into(),
        None => String::new().into(),
    };
    let mut fields: Vec<(String, Value)> = base.to_vec();
    for (k, v) in fields_of(a.get(1), level.name())? {
        fields.retain(|(x, _)| *x != k);
        fields.push((k, v));
    }
    CONFIG.with(|c| {
        let mut c = c.borrow_mut();
        let line = if c.json {
            let mut obj = serde_json::Map::new();
            if c.time {
                obj.insert("time".into(), now_iso().into());
            }
            obj.insert("level".into(), level.name().into());
            obj.insert("msg".into(), msg.into_string().into());
            for (k, v) in &fields {
                obj.insert(k.clone(), serde_json::from_str(&crate::native::json::stringify_value(v)).unwrap_or(serde_json::Value::Null));
            }
            serde_json::Value::Object(obj).to_string()
        } else {
            let color = match &c.output {
                Output::Stderr => std::io::stderr().is_terminal(),
                Output::Stdout => std::io::stdout().is_terminal(),
                Output::File(_) => false,
            } && std::env::var_os("NO_COLOR").is_none();
            let lvl = format!("{:<5}", level.name().to_uppercase());
            let lvl = if color { format!("{}{}\x1b[0m", level.color(), lvl) } else { lvl };
            let mut s = if c.time { format!("{} {} {}", now_iso(), lvl, msg) } else { format!("{} {}", lvl, msg) };
            for (k, v) in &fields {
                let text = match v {
                    Value::Str(t) if t.is_empty() || t.contains([' ', '=', '"']) => format!("{:?}", t),
                    Value::Str(t) => t.to_string(),
                    Value::Map(_) | Value::List(_) => crate::native::json::stringify_value(v),
                    other => other.to_display_string(),
                };
                s.push_str(&format!(" {}={}", k, text));
            }
            s
        };
        let r = match &mut c.output {
            Output::Stderr => writeln!(std::io::stderr(), "{}", line),
            Output::Stdout => writeln!(std::io::stdout(), "{}", line),
            Output::File(f) => writeln!(f, "{}", line),
        };
        r.map_err(|e| io_err!("log: cannot write: {}", e))
    })?;
    Ok(Value::Nil)
}

/// `log.child(fields)`: the same functions, with fields added to every line.
pub struct Logger {
    fields: Vec<(String, Value)>,
}

impl FoguObject for Logger {
    fn name(&self) -> &str {
        "Logger"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "fields").then(|| crate::native::util::map(self.fields.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "child" => {
                let mut fields = self.fields.clone();
                for (k, v) in fields_of(args.first(), "child")? {
                    fields.retain(|(x, _)| *x != k);
                    fields.push((k, v));
                }
                Ok(NativeObjectRef::new(Logger { fields }).to_value())
            }
            other => match Level::parse(other).filter(|l| *l != Level::Off) {
                Some(level) => emit(level, &self.fields, args),
                None => Err(format!("method '{}' does not exist on Logger (trace debug info warn error child)", other)),
            },
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["trace", "debug", "info", "warn", "error", "child"]
    }
}
