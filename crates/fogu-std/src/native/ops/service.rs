//! `fogu:service` module — Linux services with systemd: generate unit files
//! and `service.sh` management scripts, install, control and inspect them.
//!
//! ```fogu
//! import service from "fogu:service"
//!
//! const kv = service.define({
//!     name: "microaitec-kv",
//!     exec: "/apps/microaitec/kv/one_kv",
//!     workdir: "/apps/microaitec/kv",
//!     env: { RUST_LOG: "info" },
//! })
//! print(kv.unit())                      // the .service file
//! kv.writeScript("service.sh")?         // ./service.sh {create|start|stop|restart|status|remove|logs}
//! kv.install()?                         // as root: writes the unit, daemon-reload, enable
//! kv.start()?
//! print(kv.status()?)                   // { active: "active", sub: "running", enabled: "enabled", pid, ... }
//! ```
//!
//! Control commands run `systemctl` (with `sudo` when `{ sudo: true }`) and
//! fail with an `error` value carrying its output.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::process::{Command, Stdio};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{list, map, max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("define".into(), nfn_var("define", 1, |a| {
        max_args(a, 1, "service.define")?;
        let Value::Map(m) = &a[0] else {
            return Err("service.define: expects a map of options".into());
        };
        Ok(NativeObjectRef::new(Spec::from(&m.borrow())?).to_value())
    }));
    // service.status(name) → { active, sub, enabled, pid, memory, since, description }
    e.insert("status".into(), nfn_var("status", 1, |a| {
        max_args(a, 2, "service.status")?;
        let host = host_opt(opt_arg(a, 1))?;
        on_host(host, || status(&str_arg(a, 0, "service.status")?))
    }));
    // service.list({ all }) → [{ name, load, active, sub, description }]
    e.insert("list".into(), nfn_var("list", 0, |a| {
        max_args(a, 1, "service.list")?;
        let all = matches!(opt_arg(a, 0), Some(Value::Map(m)) if m.borrow().get("all").is_some_and(Value::is_truthy));
        let mut args = vec!["list-units", "--type=service", "--no-legend", "--plain", "--no-pager"];
        if all {
            args.push("--all");
        }
        let out = on_host(host_opt(opt_arg(a, 0))?, || systemctl(&args, sudo_opt(opt_arg(a, 0))))?;
        Ok(list(
            out.lines()
                .filter_map(|l| {
                    let mut it = l.split_whitespace();
                    let name = it.next()?;
                    let (load, active, sub) = (it.next()?, it.next()?, it.next()?);
                    let desc: Vec<&str> = it.collect();
                    Some(map(vec![
                        ("name", Value::Str(name.trim_end_matches(".service").into())),
                        ("load", Value::Str(load.into())),
                        ("active", Value::Str(active.into())),
                        ("sub", Value::Str(sub.into())),
                        ("description", Value::Str(desc.join(" ").into())),
                    ]))
                })
                .collect(),
        ))
    }));
    // control by name: service.start("nginx", { sudo }) …
    for name in ["start", "stop", "restart", "reload", "enable", "disable"] {
        e.insert(name.into(), nfn_var(name, 1, match name {
            "start" => |a| control_named("start", a),
            "stop" => |a| control_named("stop", a),
            "restart" => |a| control_named("restart", a),
            "reload" => |a| control_named("reload", a),
            "enable" => |a| control_named("enable", a),
            _ => |a| control_named("disable", a),
        }));
    }
    e.insert("logs".into(), nfn_var("logs", 1, |a| {
        max_args(a, 2, "service.logs")?;
        on_host(host_opt(opt_arg(a, 1))?, || logs(&str_arg(a, 0, "service.logs")?, opt_arg(a, 1)))
    }));
    e.insert("daemonReload".into(), nfn_var("daemonReload", 0, |a| {
        let sudo = sudo_opt(opt_arg(a, 0));
        on_host(host_opt(opt_arg(a, 0))?, || systemctl(&["daemon-reload"], sudo)).map(|_| Value::Nil)
    }));
    // unit files as data: parseUnit(text) → { Unit: { … }, Service: { … }, Install: { … } }
    e.insert("parseUnit".into(), nfn_var("parseUnit", 1, |a| {
        max_args(a, 1, "service.parseUnit")?;
        Ok(parse_unit(&str_arg(a, 0, "service.parseUnit")?))
    }));
    e.insert("readUnit".into(), nfn_var("readUnit", 1, |a| {
        max_args(a, 2, "service.readUnit")?;
        let path = str_arg(a, 0, "service.readUnit")?;
        let path = if path.contains('/') { path } else { format!("/etc/systemd/system/{}.service", path.trim_end_matches(".service")) };
        let text = match host_opt(opt_arg(a, 1))? {
            Some(h) => on_host(Some(h), || run("cat", &[&path], false))?,
            None => std::fs::read_to_string(&path).map_err(|e| io_err!("service.readUnit: {}: {}", path, e))?,
        };
        Ok(parse_unit(&text))
    }));
    e.insert("unitString".into(), nfn_var("unitString", 1, |a| {
        max_args(a, 1, "service.unitString")?;
        unit_string(&a[0]).map(Value::str)
    }));
    ModuleValue {
        name: "fogu:service".into(),
        exports: RefCell::new(e),
    }
}

fn sudo_opt(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Map(m)) if m.borrow().get("sudo").is_some_and(Value::is_truthy))
}

/// Where commands run: this machine, or a server through `fogu:ssh`.
type Host = Option<crate::native::ssh::Target>;

thread_local! {
    /// the host of the call in progress (set by the module functions and the Service methods)
    static HOST: RefCell<Host> = const { RefCell::new(None) };
}

fn host_opt(v: Option<&Value>) -> Result<Host, String> {
    match v {
        Some(Value::Map(m)) => match m.borrow().get("ssh") {
            None | Some(Value::Nil) => Ok(None),
            Some(s) => crate::native::ssh::target_of(s).map(Some).map_err(|e| format!("service: 'ssh': {}", e)),
        },
        _ => Ok(None),
    }
}

/// Runs `f` with commands going to `host`.
fn on_host<R>(host: Host, f: impl FnOnce() -> R) -> R {
    let prev = HOST.with(|h| std::mem::replace(&mut *h.borrow_mut(), host));
    let r = f();
    HOST.with(|h| *h.borrow_mut() = prev);
    r
}

fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-:=@+,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Runs a program (locally or on the current ssh host) with optional stdin.
fn run_input(prog: &str, args: &[&str], sudo: bool, input: Option<String>) -> Result<String, String> {
    if let Some(t) = HOST.with(|h| h.borrow().clone()) {
        let mut cmd = String::new();
        if sudo {
            cmd.push_str("sudo -n ");
        }
        cmd.push_str(prog);
        for a in args {
            cmd.push(' ');
            cmd.push_str(&sh_quote(a));
        }
        let what = format!("{} on {}", cmd, t.display());
        return crate::native::rt::offload(move || -> Result<String, String> {
            let (code, out, err) = t.run_bytes(&cmd, input.as_deref().map(str::as_bytes)).map_err(|e| io_err!("service: {}", e))?;
            let out = String::from_utf8_lossy(&out).into_owned();
            if code == 255 {
                return Err(io_err!("service: ssh {}: {}", t.display(), String::from_utf8_lossy(&err).trim()));
            }
            if code != 0 {
                let err = String::from_utf8_lossy(&err);
                let msg = if err.trim().is_empty() { out.trim().to_string() } else { err.trim().to_string() };
                return Err(io_err!("service: {} failed: {}", what, msg));
            }
            Ok(out)
        })?;
    }
    if let Some(data) = input {
        // local stdin (used by `tee`)
        let mut c = Command::new(if sudo { "sudo" } else { prog });
        if sudo {
            c.arg(prog);
        }
        c.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| io_err!("service: cannot run {}: {}", prog, e))?;
        if let Some(mut si) = child.stdin.take() {
            use std::io::Write;
            let _ = si.write_all(data.as_bytes());
        }
        let o = child.wait_with_output().map_err(|e| io_err!("service: {}", e))?;
        if !o.status.success() {
            return Err(io_err!("service: {} failed: {}", prog, String::from_utf8_lossy(&o.stderr).trim()));
        }
        return Ok(String::from_utf8_lossy(&o.stdout).into_owned());
    }
    run_local(prog, args, sudo)
}

fn run(prog: &str, args: &[&str], sudo: bool) -> Result<String, String> {
    run_input(prog, args, sudo, None)
}

fn run_local(prog: &str, args: &[&str], sudo: bool) -> Result<String, String> {
    let (prog, args): (String, Vec<String>) = if sudo {
        ("sudo".into(), std::iter::once(prog.to_string()).chain(args.iter().map(|s| s.to_string())).collect())
    } else {
        (prog.into(), args.iter().map(|s| s.to_string()).collect())
    };
    let what = format!("{} {}", prog, args.join(" "));
    crate::native::rt::offload(move || -> Result<String, String> {
        let o = Command::new(&prog)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| io_err!("service: cannot run {}: {}", prog, e))?;
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        if !o.status.success() {
            let err = String::from_utf8_lossy(&o.stderr);
            let msg = if err.trim().is_empty() { out.trim() } else { err.trim() };
            return Err(io_err!("service: {} failed: {}", what, msg));
        }
        Ok(out)
    })?
}

fn systemctl(args: &[&str], sudo: bool) -> Result<String, String> {
    run("systemctl", args, sudo)
}

fn control_named(cmd: &str, a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, cmd)?;
    let name = str_arg(a, 0, cmd)?;
    on_host(host_opt(opt_arg(a, 1))?, || systemctl(&[cmd, &name], sudo_opt(opt_arg(a, 1)))).map(|_| Value::Nil)
}

fn status(name: &str) -> Result<Value, String> {
    let out = systemctl(
        &["show", name, "--no-pager", "-p", "ActiveState,SubState,UnitFileState,MainPID,MemoryCurrent,ActiveEnterTimestamp,Description,LoadState,ExecMainStatus"],
        false,
    )?;
    let kv: HashMap<&str, &str> = out.lines().filter_map(|l| l.split_once('=')).collect();
    let get = |k: &str| kv.get(k).copied().unwrap_or("");
    let num = |k: &str| get(k).parse::<i64>().map_or(Value::Nil, Value::Int);
    Ok(map(vec![
        ("name", Value::Str(name.into())),
        ("loaded", Value::Bool(get("LoadState") == "loaded")),
        ("active", Value::Str(get("ActiveState").into())),
        ("sub", Value::Str(get("SubState").into())),
        ("enabled", Value::Str(get("UnitFileState").into())),
        ("running", Value::Bool(get("SubState") == "running")),
        ("pid", num("MainPID")),
        ("memory", num("MemoryCurrent")),
        ("since", Value::Str(get("ActiveEnterTimestamp").into())),
        ("exitStatus", num("ExecMainStatus")),
        ("description", Value::Str(get("Description").into())),
    ]))
}

fn logs(name: &str, o: Option<&Value>) -> Result<Value, String> {
    let mut lines = "100".to_string();
    let mut since = None;
    let mut sudo = false;
    if let Some(Value::Map(m)) = o {
        let m = m.borrow();
        if let Some(v) = m.get("lines") {
            lines = v.to_display_string();
        }
        since = m.get("since").map(|v| v.to_display_string());
        sudo = m.get("sudo").is_some_and(Value::is_truthy);
    }
    let mut args = vec!["-u", name, "-n", &lines, "--no-pager", "-o", "short-iso"];
    if let Some(s) = &since {
        args.push("--since");
        args.push(s);
    }
    run("journalctl", &args, sudo).map(Value::str)
}

// ================= UNIT FILES =================

/// Sections in a stable order; inside each, keys keep their order.
fn parse_unit(text: &str) -> Value {
    let mut sections: Vec<(String, Vec<(String, Value)>)> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_string(), Vec::new()));
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        if sections.is_empty() {
            sections.push(("Unit".into(), Vec::new()));
        }
        let (k, v) = (k.trim().to_string(), Value::Str(v.trim().to_string().into()));
        let sec = &mut sections.last_mut().expect("a section").1;
        // repeated keys (Environment=, ExecStartPre=) become lists
        match sec.iter_mut().find(|(kk, _)| *kk == k) {
            Some((_, Value::List(l))) => l.borrow_mut().push(v),
            Some((_, old)) => *old = list(vec![old.clone(), v]),
            None => sec.push((k, v)),
        }
    }
    map(sections.iter().map(|(n, kv)| (n.as_str(), map(kv.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()))).collect())
}

fn unit_string(v: &Value) -> Result<String, String> {
    let Value::Map(m) = v else {
        return Err("service.unitString: expects { Unit: {…}, Service: {…}, Install: {…} }".into());
    };
    let m = m.borrow();
    let mut names: Vec<&String> = m.keys().collect();
    let rank = |s: &str| match s {
        "Unit" => 0,
        "Service" | "Socket" | "Timer" | "Mount" | "Path" => 1,
        "Install" => 3,
        _ => 2,
    };
    names.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.cmp(b)));
    let mut out = String::new();
    for n in names {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("[{}]\n", n));
        let Value::Map(sec) = &m[n] else {
            return Err(format!("service.unitString: section '{}' must be a map", n));
        };
        let sec = sec.borrow();
        let mut keys: Vec<&String> = sec.keys().collect();
        keys.sort_by_key(|k| key_rank(k));
        for k in keys {
            match &sec[k] {
                Value::List(l) => {
                    for x in l.borrow().iter() {
                        out.push_str(&format!("{}={}\n", k, x.to_display_string()));
                    }
                }
                Value::Bool(b) => out.push_str(&format!("{}={}\n", k, if *b { "yes" } else { "no" })),
                Value::Duration(ns) => out.push_str(&format!("{}={}\n", k, crate::runtime::value::display_duration(*ns))),
                other => out.push_str(&format!("{}={}\n", k, other.to_display_string())),
            }
        }
    }
    Ok(out)
}

/// The usual order of the common keys (maps have none).
fn key_rank(k: &str) -> (usize, String) {
    const ORDER: &[&str] = &[
        "Description", "Documentation", "After", "Wants", "Requires", "Before", "Type", "User", "Group", "WorkingDirectory",
        "Environment", "EnvironmentFile", "ExecStartPre", "ExecStart", "ExecStartPost", "ExecReload", "ExecStop", "Restart",
        "RestartSec", "TimeoutStopSec", "LimitNOFILE", "StandardOutput", "StandardError", "WantedBy", "RequiredBy", "Alias",
    ];
    (ORDER.iter().position(|o| *o == k).unwrap_or(ORDER.len()), k.to_string())
}

// ================= SERVICE SPEC =================

pub struct Spec {
    name: String,
    description: String,
    exec: String,
    user: String,
    group: Option<String>,
    workdir: Option<String>,
    after: String,
    kind: String,
    restart: String,
    restart_sec: String,
    limit_nofile: Option<String>,
    wanted_by: String,
    env: Vec<(String, String)>,
    env_file: Option<String>,
    /// extra keys per section: { Service: { MemoryMax: "1G" } }
    extra: Vec<(String, Vec<(String, String)>)>,
    sudo: bool,
    dir: String,
    host: Host,
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-:=@+,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

impl Spec {
    fn from(o: &HashMap<String, Value>) -> Result<Spec, String> {
        const KEYS: &[&str] = &[
            "name", "description", "exec", "args", "user", "group", "workdir", "after", "type", "restart", "restartSec",
            "limitNofile", "wantedBy", "env", "envFile", "extra", "sudo", "dir", "ssh",
        ];
        for k in o.keys() {
            if !KEYS.contains(&k.as_str()) {
                return Err(format!("service.define: unknown option '{}' ({})", k, KEYS.join(" ")));
            }
        }
        let s = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
        let name = s("name").ok_or("service.define: 'name' is required")?;
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.@".contains(c)) {
            return Err(format!("service.define: invalid service name '{}'", name));
        }
        let mut exec = s("exec").ok_or("service.define: 'exec' (the program to run) is required")?;
        if let Some(args) = o.get("args") {
            let parts: Vec<String> = match args {
                Value::List(l) => l.borrow().iter().map(|v| shell_quote(&v.to_display_string())).collect(),
                other => vec![other.to_display_string()],
            };
            exec = format!("{} {}", exec, parts.join(" "));
        }
        let env = match o.get("env") {
            Some(Value::Map(m)) => {
                let mut v: Vec<(String, String)> = m.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect();
                v.sort();
                v
            }
            None | Some(Value::Nil) => Vec::new(),
            Some(_) => return Err("service.define: 'env' must be a map".into()),
        };
        let mut extra = Vec::new();
        if let Some(Value::Map(m)) = o.get("extra") {
            let mut secs: Vec<(String, Value)> = m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            secs.sort_by(|a, b| a.0.cmp(&b.0));
            for (sec, kv) in secs {
                let Value::Map(kv) = kv else { return Err("service.define: 'extra' is { Section: { Key: value } }".into()) };
                let mut items: Vec<(String, String)> = kv.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect();
                items.sort();
                extra.push((sec, items));
            }
        }
        let num_or_str = |k: &str, d: &str| match o.get(k) {
            Some(Value::Duration(ns)) => (ns / 1_000_000_000).to_string(),
            Some(v) if !matches!(v, Value::Nil) => v.to_display_string(),
            _ => d.to_string(),
        };
        Ok(Spec {
            description: s("description").unwrap_or_else(|| name.clone()),
            exec,
            user: s("user").unwrap_or_else(|| "root".into()),
            group: s("group"),
            workdir: s("workdir"),
            after: s("after").unwrap_or_else(|| "network.target".into()),
            kind: s("type").unwrap_or_else(|| "simple".into()),
            restart: s("restart").unwrap_or_else(|| "always".into()),
            restart_sec: num_or_str("restartSec", "5"),
            limit_nofile: match o.get("limitNofile") {
                Some(Value::Nil) | Some(Value::Bool(false)) => None,
                Some(v) => Some(v.to_display_string()),
                None => Some("65535".into()),
            },
            wanted_by: s("wantedBy").unwrap_or_else(|| "multi-user.target".into()),
            env,
            env_file: s("envFile"),
            extra,
            sudo: o.get("sudo").is_some_and(Value::is_truthy),
            dir: s("dir").unwrap_or_else(|| "/etc/systemd/system".into()),
            host: match o.get("ssh") {
                None | Some(Value::Nil) => None,
                Some(v) => Some(crate::native::ssh::target_of(v).map_err(|e| format!("service.define: 'ssh': {}", e))?),
            },
            name,
        })
    }

    fn unit_path(&self) -> String {
        format!("{}/{}.service", self.dir.trim_end_matches('/'), self.name)
    }

    fn unit(&self) -> String {
        let mut u = String::new();
        u.push_str(&format!("[Unit]\nDescription={}\nAfter={}\n", self.description, self.after));
        self.push_extra(&mut u, "Unit");
        u.push_str(&format!("\n[Service]\nType={}\nUser={}\n", self.kind, self.user));
        if let Some(g) = &self.group {
            u.push_str(&format!("Group={}\n", g));
        }
        if let Some(w) = &self.workdir {
            u.push_str(&format!("WorkingDirectory={}\n", w));
        }
        for (k, v) in &self.env {
            u.push_str(&format!("Environment=\"{}={}\"\n", k, v.replace('"', "\\\"")));
        }
        if let Some(f) = &self.env_file {
            u.push_str(&format!("EnvironmentFile={}\n", f));
        }
        u.push_str(&format!("ExecStart={}\nRestart={}\nRestartSec={}\n", self.exec, self.restart, self.restart_sec));
        if let Some(l) = &self.limit_nofile {
            u.push_str(&format!("LimitNOFILE={}\n", l));
        }
        self.push_extra(&mut u, "Service");
        u.push_str(&format!("\n[Install]\nWantedBy={}\n", self.wanted_by));
        self.push_extra(&mut u, "Install");
        u
    }

    fn push_extra(&self, u: &mut String, section: &str) {
        for (sec, items) in &self.extra {
            if sec == section {
                for (k, v) in items {
                    u.push_str(&format!("{}={}\n", k, v));
                }
            }
        }
    }

    /// A self-contained bash script: create | start | stop | restart | status | remove | logs.
    fn script(&self) -> String {
        let bin = self.exec.split_whitespace().next().unwrap_or("").to_string();
        let mut s = String::new();
        s.push_str("#!/bin/bash\n");
        s.push_str("# Generated by fogu:service — manages a systemd service.\n\n");
        s.push_str("# =========================\n# CONFIGURATION\n# =========================\n");
        s.push_str(&format!("SERVICE_NAME={}\n", shell_quote(&self.name)));
        s.push_str(&format!("DESCRIPTION={}\n", shell_quote(&self.description)));
        s.push_str(&format!("BIN_PATH={}\n", shell_quote(&bin)));
        s.push_str(&format!("USER={}\n", shell_quote(&self.user)));
        s.push_str(&format!("WORKDIR={}\n", shell_quote(self.workdir.as_deref().unwrap_or("/"))));
        s.push_str(&format!("SERVICE_FILE=\"{}/${{SERVICE_NAME}}.service\"\n\n", self.dir.trim_end_matches('/')));
        // the unit with the variables substituted by bash
        let unit = self
            .unit()
            .replace(&format!("Description={}\n", self.description), "Description=$DESCRIPTION\n")
            .replace(&format!("User={}\n", self.user), "User=$USER\n")
            .replace(&format!("WorkingDirectory={}\n", self.workdir.clone().unwrap_or_default()), "WorkingDirectory=$WORKDIR\n")
            .replacen(&format!("ExecStart={}", bin), "ExecStart=$BIN_PATH", 1)
            .replace('`', "\\`");
        s.push_str("# =========================\n# FUNCTIONS\n# =========================\n");
        s.push_str("create_service() {\n  echo \"🛠️ Creating the systemd service...\"\n\n");
        s.push_str("  if [ ! -f \"$BIN_PATH\" ]; then\n    echo \"❌ Binary not found at $BIN_PATH\"\n    exit 1\n  fi\n\n");
        s.push_str("  cat <<EOF | tee \"$SERVICE_FILE\" > /dev/null\n");
        s.push_str(&unit);
        s.push_str("EOF\n\n");
        s.push_str("  systemctl daemon-reexec\n  systemctl daemon-reload\n  systemctl enable \"$SERVICE_NAME\"\n\n");
        s.push_str("  echo \"✅ Service created and registered\"\n}\n\n");
        s.push_str("start_service() {\n  systemctl start \"$SERVICE_NAME\"\n  systemctl status \"$SERVICE_NAME\" --no-pager\n}\n\n");
        s.push_str("stop_service() {\n  systemctl stop \"$SERVICE_NAME\"\n  echo \"🛑 Service stopped\"\n}\n\n");
        s.push_str("restart_service() {\n  systemctl restart \"$SERVICE_NAME\"\n  systemctl status \"$SERVICE_NAME\" --no-pager\n}\n\n");
        s.push_str("status_service() {\n  systemctl status \"$SERVICE_NAME\" --no-pager\n}\n\n");
        s.push_str("logs_service() {\n  journalctl -u \"$SERVICE_NAME\" -n \"${1:-100}\" --no-pager\n}\n\n");
        s.push_str("remove_service() {\n  echo \"🗑️ Removing the service...\"\n\n");
        s.push_str("  systemctl stop \"$SERVICE_NAME\"\n  systemctl disable \"$SERVICE_NAME\"\n  rm -f \"$SERVICE_FILE\"\n  systemctl daemon-reload\n\n");
        s.push_str("  echo \"✅ Service removed\"\n}\n\n");
        s.push_str("# =========================\n# CLI\n# =========================\n");
        s.push_str("case \"$1\" in\n");
        for (cmd, f) in [("create", "create_service"), ("start", "start_service"), ("stop", "stop_service"), ("restart", "restart_service"), ("status", "status_service"), ("remove", "remove_service")] {
            s.push_str(&format!("  {})\n    {}\n    ;;\n", cmd, f));
        }
        s.push_str("  logs)\n    logs_service \"$2\"\n    ;;\n");
        s.push_str("  *)\n    echo \"Usage: $0 {create|start|stop|restart|status|remove|logs [lines]}\"\n    exit 1\n    ;;\nesac\n");
        s
    }

    fn info(&self) -> Value {
        map(vec![
            ("name", Value::Str(self.name.clone().into())),
            ("description", Value::Str(self.description.clone().into())),
            ("exec", Value::Str(self.exec.clone().into())),
            ("user", Value::Str(self.user.clone().into())),
            ("workdir", self.workdir.clone().map_or(Value::Nil, Value::str)),
            ("unitPath", Value::Str(self.unit_path().into())),
            ("host", self.host.as_ref().map_or(Value::Nil, |t| Value::Str(t.display().into()))),
        ])
    }
}

fn write_file(path: &str, text: &str, mode: u32) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| io_err!("service: cannot write {}: {}", path, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

impl FoguObject for Spec {
    fn name(&self) -> &str {
        "Service"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match &self.info() {
            Value::Map(m) => m.borrow().get(field).cloned(),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let host = self.host.clone();
        on_host(host, || self.call_here(method, a))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "unit", "script", "writeScript", "writeUnit", "install", "uninstall", "deploy", "installScript", "start", "stop",
            "restart", "reload", "enable", "disable", "status", "isActive", "isEnabled", "logs",
        ]
    }
}

impl Spec {
    fn call_here(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let name = self.name.clone();
        let sudo = self.sudo;
        let remote = self.host.is_some();
        match method {
            "unit" => Ok(Value::Str(self.unit().into())),
            "script" => Ok(Value::Str(self.script().into())),
            // writeScript(path = "service.sh") — executable
            "writeScript" => {
                let path = match opt_arg(a, 0) { Some(_) => str_arg(a, 0, method)?, None => "service.sh".into() };
                write_file(&path, &self.script(), 0o755)?;
                Ok(Value::Nil)
            }
            // writeUnit(path = "<name>.service")
            "writeUnit" => {
                let path = match opt_arg(a, 0) { Some(_) => str_arg(a, 0, method)?, None => format!("{}.service", name) };
                write_file(&path, &self.unit(), 0o644)?;
                Ok(Value::Nil)
            }
            // install(): unit into /etc/systemd/system (or 'dir'), daemon-reload, enable
            "install" => {
                let path = self.unit_path();
                if remote {
                    // the unit goes through stdin: `tee` on the server
                    run_input("tee", &[&path], sudo, Some(self.unit()))?;
                } else if sudo {
                    let tmp = std::env::temp_dir().join(format!("{}.service", name));
                    let t = tmp.to_string_lossy().to_string();
                    write_file(&t, &self.unit(), 0o644)?;
                    run("install", &["-m", "644", &t, &path], true)?;
                    let _ = std::fs::remove_file(&tmp);
                } else {
                    write_file(&path, &self.unit(), 0o644)?;
                }
                systemctl(&["daemon-reload"], sudo)?;
                systemctl(&["enable", &name], sudo)?;
                Ok(Value::Nil)
            }
            // uninstall(): stop, disable, remove the unit, daemon-reload
            "uninstall" => {
                let _ = systemctl(&["stop", &name], sudo);
                let _ = systemctl(&["disable", &name], sudo);
                let path = self.unit_path();
                if sudo || remote {
                    run("rm", &["-f", &path], sudo)?;
                } else if std::path::Path::new(&path).exists() {
                    std::fs::remove_file(&path).map_err(|e| io_err!("service: cannot remove {}: {}", path, e))?;
                }
                systemctl(&["daemon-reload"], sudo)?;
                Ok(Value::Nil)
            }
            // deploy(localBinary): copies the program to its 'exec' path (scp on a remote host),
            // installs or updates the unit and restarts the service
            "deploy" => {
                max_args(a, 1, "deploy")?;
                let local = str_arg(a, 0, "deploy")?;
                let bin = self.exec.split_whitespace().next().unwrap_or("").to_string();
                let dir = std::path::Path::new(&bin).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                match HOST.with(|h| h.borrow().clone()) {
                    Some(t) => {
                        let tmp = format!("/tmp/fogu-deploy-{}", name);
                        let (l, t2, tmp2) = (local.clone(), t.clone(), tmp.clone());
                        crate::native::rt::offload(move || crate::native::ssh::upload_file(&t2, &l, &tmp2))??;
                        if !dir.is_empty() {
                            run("mkdir", &["-p", &dir], sudo)?;
                        }
                        run("install", &["-m", "755", &tmp, &bin], sudo)?;
                        let _ = run("rm", &["-f", &tmp], false);
                    }
                    None => {
                        if !dir.is_empty() {
                            run("mkdir", &["-p", &dir], sudo)?;
                        }
                        run("install", &["-m", "755", &local, &bin], sudo)?;
                    }
                }
                self.call_here("install", &[])?;
                systemctl(&["restart", &name], sudo)?;
                Ok(Value::Nil)
            }
            // installScript(path = "service.sh"): writes the script on the (remote) host, executable
            "installScript" => {
                let path = match opt_arg(a, 0) { Some(_) => str_arg(a, 0, method)?, None => "service.sh".into() };
                if remote {
                    run_input("tee", &[&path], sudo, Some(self.script()))?;
                    run("chmod", &["755", &path], sudo)?;
                } else {
                    write_file(&path, &self.script(), 0o755)?;
                }
                Ok(Value::Nil)
            }
            "start" | "stop" | "restart" | "reload" | "enable" | "disable" => systemctl(&[method, &name], sudo).map(|_| Value::Nil),
            "status" => status(&name),
            "isActive" => Ok(Value::Bool(systemctl(&["is-active", "--quiet", &name], false).is_ok())),
            "isEnabled" => Ok(Value::Bool(systemctl(&["is-enabled", "--quiet", &name], false).is_ok())),
            "logs" => logs(&name, opt_arg(a, 0)),
            other => Err(format!("method '{}' does not exist on Service ({})", other, self.methods().join(" "))),
        }
    }
}
