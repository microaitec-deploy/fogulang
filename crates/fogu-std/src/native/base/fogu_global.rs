//! The global `Fogu` object (spec §45): runtime primitives available without
//! an import.
//!
//! ```fogu
//! const cfg = Fogu.file("config.json").json()?
//! print(Fogu.env.HOME, Fogu.args, Fogu.hash("abc"))
//! const r = Fogu.run("git", ["--version"])?
//! for f in Fogu.glob("*.fg") { print(f) }
//! Fogu.sleep(100ms)
//! ```

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::path::Path;

use sha2::{Digest, Sha256, Sha512};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{duration_arg, max_args, nfn_var, opt_arg, path_str, str_arg, str_list};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();

    e.insert("version".into(), Value::Str(env!("CARGO_PKG_VERSION").into()));
    // cores() → how many the runtime uses · cores(n) → use n threads · cores([0, 2]) → only those CPUs
    e.insert("cores".into(), nfn_var("cores", 0, cores));
    e.insert("os".into(), Value::Str(std::env::consts::OS.into()));
    e.insert("env".into(), crate::native::process::env_map());
    e.insert("args".into(), str_list(crate::native::system::script_args()));

    e.insert("file".into(), nfn("file", 1, |a| {
        let p = str_arg(a, 0, "Fogu.file")?;
        Ok(NativeObjectRef::new(FileValue { path: p }).to_value())
    }));
    e.insert("sleep".into(), nfn("sleep", 1, |a| {
        crate::native::util::sleep_coop(duration_arg(&a[0], "Fogu.sleep")?)?;
        Ok(Value::Nil)
    }));
    e.insert("exit".into(), nfn_var("exit", 0, |a| {
        let code = match opt_arg(a, 0) {
            None => 0,
            Some(Value::Int(n)) => *n as i32,
            Some(other) => return Err(format!("Fogu.exit expects int, got '{}'", other.type_name())),
        };
        crate::native::exit(code)
    }));
    // hash(data, "sha256" | "sha512") → hex string
    e.insert("hash".into(), nfn_var("hash", 1, |a| {
        max_args(a, 2, "Fogu.hash")?;
        let data = match &a[0] {
            Value::Str(s) => s.clone(),
            other => other.to_display_string().into(),
        };
        let algo = match opt_arg(a, 1) {
            None => "sha256".to_string(),
            Some(_) => str_arg(a, 1, "Fogu.hash")?,
        };
        let bytes: Vec<u8> = match algo.as_str() {
            "sha256" => Sha256::digest(data.as_bytes()).to_vec(),
            "sha512" => Sha512::digest(data.as_bytes()).to_vec(),
            other => return Err(format!("Fogu.hash: unknown algorithm '{}' (use sha256 or sha512)", other)),
        };
        Ok(Value::Str(bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>().into()))
    }));

    // shared with the modules, so there is a single implementation
    let fs = crate::native::fs::build();
    e.insert("glob".into(), fs.exports.borrow()["glob"].clone());
    #[cfg(all(feature = "database", not(target_family = "wasm")))]
    e.insert("sql".into(), crate::native::sql::build().exports.borrow()["open"].clone());
    e.insert("run".into(), nfn_var("run", 1, crate::native::process::run));
    // Fogu.signal: OS signals and the program's events (served by signal::intercept)
    #[cfg(not(target_family = "wasm"))]
    e.insert("signal".into(), crate::native::signal::value());
    // Fogu.host: the program that embeds this runtime (ffi/)
    #[cfg(not(target_family = "wasm"))]
    e.insert("host".into(), crate::native::host::value());
    // asset(path): a project file, also inside `fogu build --compile` executables
    #[cfg(not(target_family = "wasm"))]
    e.insert("watch".into(), nfn_var("watch", 1, crate::native::watch::watch));
    e.insert("asset".into(), nfn("asset", 1, |a| {
        let rel = crate::native::util::str_arg(a, 0, "Fogu.asset")?;
        Ok(Value::Str(crate::native::util::path_str(&crate::pkg::asset_path(&rel)).into()))
    }));
    // write(...values): like print, without the space separator or the newline
    e.insert("write".into(), nfn_var("write", 0, |a| {
        use std::io::Write as _;
        let mut out = std::io::stdout().lock();
        for v in a {
            let _ = write!(out, "{}", v.to_display_string());
        }
        let _ = out.flush();
        Ok(Value::Nil)
    }));

    ModuleValue {
        name: "Fogu".into(),
        exports: RefCell::new(e),
    }
}

/// `Fogu.file(path)` — a handle to a file (nothing is read until asked).
pub struct FileValue {
    path: String,
}

impl FileValue {
    fn read(&self, what: &str) -> Result<String, String> {
        std::fs::read_to_string(&self.path).map_err(|e| io_err!("{}: {}: {}", what, self.path, e))
    }
}

impl FoguObject for FileValue {
    fn name(&self) -> &str {
        "File"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let p = Path::new(&self.path);
        match field {
            "path" => Some(Value::Str(self.path.clone().into())),
            "name" => Some(Value::Str(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into())),
            "ext" => Some(Value::Str(p.extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default().into())),
            "dir" => Some(Value::Str(p.parent().map(path_str).unwrap_or_default().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "text" => self.read("file.text").map(Value::str),
            "lines" => Ok(str_list(self.read("file.lines")?.lines().map(str::to_string))),
            "json" => {
                let text = self.read("file.json")?;
                crate::native::json::parse_value(&text).map_err(|e| io_err!("file.json: {}: {}", self.path, e))
            }
            "write" | "append" => {
                use std::io::Write;
                let text = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    Some(other) => other.to_display_string().into(),
                    None => return Err(format!("file.{}(text) expects 1 argument", method)),
                };
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(method == "append")
                    .truncate(method == "write")
                    .open(&self.path)
                    .and_then(|mut f| f.write_all(text.as_bytes()))
                    .map(|_| Value::Nil)
                    .map_err(|e| io_err!("file.{}: {}: {}", method, self.path, e))
            }
            "exists" => Ok(Value::Bool(Path::new(&self.path).is_file())),
            "size" => std::fs::metadata(&self.path)
                .map(|m| Value::Int(m.len() as i64))
                .map_err(|e| io_err!("file.size: {}: {}", self.path, e)),
            "remove" => std::fs::remove_file(&self.path)
                .map(|_| Value::Nil)
                .map_err(|e| io_err!("file.remove: {}: {}", self.path, e)),
            other => Err(format!("method '{}' does not exist on File", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["text", "lines", "json", "write", "append", "exists", "size", "remove"]
    }
}

/// `Fogu.cores()` · `Fogu.cores(n)` · `Fogu.cores([cpus])`.
fn cores(a: &[Value]) -> Result<Value, String> {
    crate::native::util::max_args(a, 1, "Fogu.cores")?;
    match a.first() {
        None | Some(Value::Nil) => Ok(Value::Int(crate::native::rt::cores() as i64)),
        Some(Value::Int(n)) if *n >= 1 => {
            crate::native::rt::set_cores(*n as usize).map_err(|e| io_err!("{}", e))?;
            Ok(Value::Int(*n))
        }
        Some(Value::List(l)) => {
            let mut cpus = Vec::new();
            for v in l.borrow().iter() {
                match v {
                    Value::Int(c) if *c >= 0 => cpus.push(*c as usize),
                    other => return Err(format!("Fogu.cores: CPUs are ints >= 0, got {}", other.to_display_string())),
                }
            }
            crate::native::rt::pin_cores(&cpus).map_err(|e| io_err!("{}", e))?;
            // the thread count follows, unless the runtime is already running
            let _ = crate::native::rt::set_cores(cpus.len());
            Ok(crate::native::util::list(cpus.into_iter().map(|c| Value::Int(c as i64)).collect()))
        }
        Some(other) => Err(format!("Fogu.cores: expected a number >= 1 or a list of CPUs, got {}", other.to_display_string())),
    }
}
