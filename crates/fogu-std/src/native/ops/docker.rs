//! `fogu:docker` module — control Docker or Podman (containers, images,
//! networks, volumes, compose projects) and write/edit Dockerfiles and
//! compose files.
//!
//! ```fogu
//! import docker from "fogu:docker"
//!
//! print(docker.engine)                                  // "docker" or "podman"
//! const id = docker.run("nginx:alpine", { name: "web", ports: { "8080": "80" } })?
//! print(docker.ps()?)
//! print(docker.logs("web", { tail: 20 })?)
//! docker.rm("web", { force: true })?
//!
//! const df = docker.dockerfile("node:20-alpine")
//! df.workdir("/app")
//! df.copy("package*.json", ".")
//! df.run("npm ci")
//! df.cmd(["node", "index.js"])
//! df.save("Dockerfile")?
//!
//! const app = docker.compose("compose.yaml")
//! app.up()?
//! ```
//!
//! Commands go through the `docker`/`podman` CLI (found on the PATH; force
//! one with `FOGU_CONTAINER_ENGINE` or `docker.use("podman")`). A failing
//! command is an `error` value carrying its stderr.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::process::{Command, Stdio};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{json_to_value, list, map, nfn_var, opt_arg, str_arg};

thread_local! {
    static ENGINE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// set while a `docker.remote(ssh)` call runs: commands go to that server
    static TARGET: RefCell<Option<crate::native::ssh::Target>> = const { RefCell::new(None) };
    /// the engine of the remote call in progress
    static REMOTE_ENGINE: RefCell<Option<String>> = const { RefCell::new(None) };
    static EXPORTS: RefCell<Option<HashMap<String, Value>>> = const { RefCell::new(None) };
}

fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-:=@+,%".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Runs `f` with commands sent to `target` (and its engine).
fn on_target<R>(target: Option<crate::native::ssh::Target>, engine: Option<String>, f: impl FnOnce() -> R) -> R {
    let pt = TARGET.with(|t| std::mem::replace(&mut *t.borrow_mut(), target));
    let pe = REMOTE_ENGINE.with(|e| std::mem::replace(&mut *e.borrow_mut(), engine));
    let r = f();
    TARGET.with(|t| *t.borrow_mut() = pt);
    REMOTE_ENGINE.with(|e| *e.borrow_mut() = pe);
    r
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| d.join(name).is_file() || d.join(format!("{}.exe", name)).is_file())
    })
}

/// "docker" or "podman": the chosen one, `FOGU_CONTAINER_ENGINE`, or the first on the PATH.
fn engine() -> String {
    ENGINE.with(|e| {
        e.borrow_mut()
            .get_or_insert_with(|| {
                std::env::var("FOGU_CONTAINER_ENGINE")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| if !on_path("docker") && on_path("podman") { "podman".into() } else { "docker".into() })
            })
            .clone()
    })
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs the engine CLI on a worker thread (other `fg` tasks keep running).
fn exec(args: Vec<String>, cwd: Option<String>) -> Result<Out, String> {
    if let Some(t) = TARGET.with(|t| t.borrow().clone()) {
        let eng = REMOTE_ENGINE.with(|e| e.borrow().clone()).unwrap_or_else(|| "docker".into());
        let mut cmd = String::new();
        if let Some(d) = &cwd {
            cmd.push_str(&format!("cd {} && ", sh_quote(d)));
        }
        cmd.push_str(&eng);
        for a in &args {
            cmd.push(' ');
            cmd.push_str(&sh_quote(a));
        }
        return crate::native::rt::offload(move || -> Result<Out, String> {
            let (code, out, err) = t.run_bytes(&cmd, None).map_err(|e| io_err!("docker: {}", e))?;
            let stderr = String::from_utf8_lossy(&err).into_owned();
            if code == 255 {
                return Err(io_err!("docker: ssh {}: {}", t.display(), stderr.trim()));
            }
            Ok(Out { code: code as i32, stdout: String::from_utf8_lossy(&out).into_owned(), stderr })
        })?;
    }
    let eng = engine();
    crate::native::rt::offload(move || -> Result<Out, String> {
        let mut cmd = Command::new(&eng);
        cmd.args(&args).stdin(Stdio::null());
        if let Some(d) = cwd {
            cmd.current_dir(d);
        }
        let o = cmd.output().map_err(|e| io_err!("docker: cannot run '{}': {}", eng, e))?;
        Ok(Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        })
    })?
}

/// Runs and fails (error value) on a non-zero exit; returns stdout.
fn run_ok(args: Vec<String>, cwd: Option<String>) -> Result<String, String> {
    let what = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    let o = exec(args, cwd)?;
    if o.code != 0 {
        let msg = o.stderr.trim();
        return Err(io_err!("docker {}: {}", what, if msg.is_empty() { o.stdout.trim() } else { msg }));
    }
    Ok(o.stdout)
}

/// One JSON object per line (`--format '{{json .}}'`) → list of maps.
fn json_lines(s: &str) -> Value {
    list(
        s.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .flat_map(|j| match j {
                serde_json::Value::Array(a) => a.into_iter().map(|x| json_to_value(&x)).collect::<Vec<_>>(),
                other => vec![json_to_value(&other)],
            })
            .collect(),
    )
}

fn opts(v: Option<&Value>, what: &str) -> Result<HashMap<String, Value>, String> {
    match v {
        None => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn sorted(m: &HashMap<String, Value>) -> Vec<(String, Value)> {
    let mut v: Vec<_> = m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn strs(v: &Value) -> Vec<String> {
    match v {
        Value::List(l) => l.borrow().iter().map(|x| x.to_display_string()).collect(),
        Value::Nil => Vec::new(),
        other => vec![other.to_display_string()],
    }
}

/// `{ "8080": "80" }` / `["8080:80"]` / `{ k: v }` → repeated flags.
fn pairs(flag: &str, v: Option<&Value>, sep: &str, args: &mut Vec<String>) {
    match v {
        Some(Value::Map(m)) => {
            for (k, val) in sorted(&m.borrow()) {
                args.push(flag.into());
                args.push(format!("{}{}{}", k, sep, val.to_display_string()));
            }
        }
        Some(Value::List(l)) => {
            for x in l.borrow().iter() {
                args.push(flag.into());
                args.push(x.to_display_string());
            }
        }
        Some(Value::Nil) | None => {}
        Some(other) => {
            args.push(flag.into());
            args.push(other.to_display_string());
        }
    }
}

type F = fn(&[Value]) -> Result<Value, String>;

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    let mut f = |name: &str, min: usize, func: F| {
        e.insert(name.into(), nfn_var(name, min, func));
    };
    f("use", 1, |a| {
        let name = str_arg(a, 0, "docker.use")?;
        if name != "docker" && name != "podman" {
            return Err("docker.use: expects \"docker\" or \"podman\"".into());
        }
        ENGINE.with(|e| *e.borrow_mut() = Some(name));
        Ok(Value::Nil)
    });
    f("available", 0, |_| Ok(Value::Bool(exec(vec!["version".into()], None).is_ok_and(|o| o.code == 0))));
    f("version", 0, |_| {
        let out = run_ok(vec!["version".into(), "--format".into(), "{{json .}}".into()], None)?;
        Ok(serde_json::from_str::<serde_json::Value>(out.trim()).map(|j| json_to_value(&j)).unwrap_or(Value::Str(out.trim().into())))
    });
    // raw(args, { cwd }) → { code, stdout, stderr } (never fails on exit codes)
    f("raw", 1, |a| {
        let args = strs(&a[0]);
        let o = opts(opt_arg(a, 1), "docker.raw")?;
        let out = exec(args, o.get("cwd").map(|v| v.to_display_string()))?;
        Ok(map(vec![("code", Value::Int(out.code as i64)), ("stdout", Value::Str(out.stdout.into())), ("stderr", Value::Str(out.stderr.into()))]))
    });
    // ---------- containers ----------
    f("ps", 0, |a| {
        let o = opts(opt_arg(a, 0), "docker.ps")?;
        let mut args = vec!["ps".into(), "--no-trunc".into(), "--format".into(), "{{json .}}".into()];
        if o.get("all").is_some_and(Value::is_truthy) {
            args.push("--all".into());
        }
        pairs("--filter", o.get("filter"), "=", &mut args);
        Ok(json_lines(&run_ok(args, None)?))
    });
    // run(image, { name, detach = true, rm, ports, env, volumes, network, restart, labels, workdir, user, entrypoint, command, args }) → id or output
    f("run", 1, |a| {
        let image = str_arg(a, 0, "docker.run")?;
        let o = opts(opt_arg(a, 1), "docker.run")?;
        let detach = o.get("detach").is_none_or(Value::is_truthy);
        let mut args: Vec<String> = vec!["run".into()];
        if detach {
            args.push("-d".into());
        }
        if o.get("rm").is_some_and(Value::is_truthy) {
            args.push("--rm".into());
        }
        for (k, flag) in [("name", "--name"), ("network", "--network"), ("restart", "--restart"), ("workdir", "-w"), ("user", "-u"), ("entrypoint", "--entrypoint"), ("hostname", "--hostname"), ("memory", "--memory"), ("cpus", "--cpus")] {
            if let Some(v) = o.get(k).filter(|v| !matches!(v, Value::Nil)) {
                args.push(flag.into());
                args.push(v.to_display_string());
            }
        }
        pairs("-p", o.get("ports"), ":", &mut args);
        pairs("-e", o.get("env"), "=", &mut args);
        pairs("-v", o.get("volumes"), ":", &mut args);
        pairs("-l", o.get("labels"), "=", &mut args);
        if let Some(extra) = o.get("args") {
            args.extend(strs(extra));
        }
        args.push(image);
        if let Some(c) = o.get("command") {
            args.extend(match c {
                Value::Str(s) => vec!["sh".into(), "-c".into(), s.to_string()],
                other => strs(other),
            });
        }
        let out = run_ok(args, None)?;
        Ok(Value::Str(if detach { out.trim().to_string().into() } else { out.into() }))
    });
    for name in ["start", "stop", "restart", "kill", "pause", "unpause"] {
        f(name, 1, match name {
            "start" => |a| simple("start", a),
            "stop" => |a| simple("stop", a),
            "restart" => |a| simple("restart", a),
            "kill" => |a| simple("kill", a),
            "pause" => |a| simple("pause", a),
            _ => |a| simple("unpause", a),
        });
    }
    // rm(name, { force, volumes })
    f("rm", 1, |a| {
        let o = opts(opt_arg(a, 1), "docker.rm")?;
        let mut args = vec!["rm".to_string()];
        if o.get("force").is_some_and(Value::is_truthy) {
            args.push("-f".into());
        }
        if o.get("volumes").is_some_and(Value::is_truthy) {
            args.push("-v".into());
        }
        args.extend(strs(&a[0]));
        run_ok(args, None).map(|_| Value::Nil)
    });
    // logs(name, { tail, since, timestamps }) → str (stdout + stderr)
    f("logs", 1, |a| {
        let o = opts(opt_arg(a, 1), "docker.logs")?;
        let mut args = vec!["logs".to_string()];
        if let Some(t) = o.get("tail") {
            args.push("--tail".into());
            args.push(t.to_display_string());
        }
        if let Some(s) = o.get("since") {
            args.push("--since".into());
            args.push(s.to_display_string());
        }
        if o.get("timestamps").is_some_and(Value::is_truthy) {
            args.push("-t".into());
        }
        args.push(str_arg(a, 0, "docker.logs")?);
        let out = exec(args, None)?;
        if out.code != 0 {
            return Err(io_err!("docker logs: {}", out.stderr.trim()));
        }
        Ok(Value::Str((out.stdout + &out.stderr).into()))
    });
    // exec(name, command, { user, workdir, env }) → { code, stdout, stderr }
    f("exec", 2, |a| {
        let o = opts(opt_arg(a, 2), "docker.exec")?;
        let mut args = vec!["exec".to_string()];
        if let Some(u) = o.get("user") {
            args.push("-u".into());
            args.push(u.to_display_string());
        }
        if let Some(w) = o.get("workdir") {
            args.push("-w".into());
            args.push(w.to_display_string());
        }
        pairs("-e", o.get("env"), "=", &mut args);
        args.push(str_arg(a, 0, "docker.exec")?);
        args.extend(match &a[1] {
            Value::Str(s) => vec!["sh".into(), "-c".into(), s.to_string()],
            other => strs(other),
        });
        let out = exec(args, None)?;
        Ok(map(vec![("code", Value::Int(out.code as i64)), ("stdout", Value::Str(out.stdout.into())), ("stderr", Value::Str(out.stderr.into()))]))
    });
    // inspect(name | id) → map
    f("inspect", 1, |a| {
        let out = run_ok(vec!["inspect".into(), str_arg(a, 0, "docker.inspect")?], None)?;
        match json_lines(&out.replace('\n', "")) {
            Value::List(l) if !l.borrow().is_empty() => Ok(l.borrow()[0].clone()),
            _ => Ok(Value::Nil),
        }
    });
    f("stats", 0, |a| {
        let mut args = vec!["stats".to_string(), "--no-stream".into(), "--format".into(), "{{json .}}".into()];
        if let Some(n) = opt_arg(a, 0) {
            args.extend(strs(n));
        }
        Ok(json_lines(&run_ok(args, None)?))
    });
    // status(container) → { running, state, health, exitCode, startedAt, restarts, image }
    f("status", 1, |a| {
        let name = str_arg(a, 0, "docker.status")?;
        let out = run_ok(vec!["inspect".into(), "--type".into(), "container".into(), name], None)?;
        let Value::List(l) = json_lines(&out.replace('\n', "")) else { return Ok(Value::Nil) };
        let Some(Value::Map(c)) = l.borrow().first().cloned() else { return Ok(Value::Nil) };
        let c = c.borrow();
        let st = match c.get("State") { Some(Value::Map(m)) => m.borrow().clone(), _ => HashMap::new() };
        let g = |k: &str| st.get(k).cloned().unwrap_or(Value::Nil);
        let health = match st.get("Health") { Some(Value::Map(h)) => h.borrow().get("Status").cloned().unwrap_or(Value::Nil), _ => Value::Nil };
        let image = match c.get("Config") { Some(Value::Map(m)) => m.borrow().get("Image").cloned().unwrap_or(Value::Nil), _ => Value::Nil };
        Ok(map(vec![
            ("name", c.get("Name").map_or(Value::Nil, |v| Value::Str(v.to_display_string().trim_start_matches('/').to_string().into()))),
            ("running", g("Running")),
            ("state", g("Status")),
            ("health", health),
            ("exitCode", g("ExitCode")),
            ("startedAt", g("StartedAt")),
            ("restarts", c.get("RestartCount").cloned().unwrap_or(Value::Nil)),
            ("image", image),
        ]))
    });
    // Swarm services: services() → [{ ID, Name, Mode, Replicas, Image, Ports }]
    f("services", 0, |a| {
        let o = opts(opt_arg(a, 0), "docker.services")?;
        let mut args = vec!["service".to_string(), "ls".into(), "--format".into(), "{{json .}}".into()];
        pairs("--filter", o.get("filter"), "=", &mut args);
        Ok(json_lines(&run_ok(args, None)?))
    });
    // service(name) → SwarmService { status, tasks, restart, stop, start, scale, update, logs, remove }
    f("service", 1, |a| {
        let name = str_arg(a, 0, "docker.service")?;
        Ok(NativeObjectRef::new(SwarmService {
            name,
            host: TARGET.with(|t| t.borrow().clone()).map(|t| (t, REMOTE_ENGINE.with(|e| e.borrow().clone()).unwrap_or_else(|| "docker".into()))),
        })
        .to_value())
    });
    f("cp", 2, |a| run_ok(vec!["cp".into(), str_arg(a, 0, "docker.cp")?, str_arg(a, 1, "docker.cp")?], None).map(|_| Value::Nil));
    // ---------- images ----------
    f("images", 0, |_| Ok(json_lines(&run_ok(vec!["images".into(), "--format".into(), "{{json .}}".into()], None)?)));
    f("pull", 1, |a| run_ok(vec!["pull".into(), str_arg(a, 0, "docker.pull")?], None).map(|_| Value::Nil));
    f("push", 1, |a| run_ok(vec!["push".into(), str_arg(a, 0, "docker.push")?], None).map(|_| Value::Nil));
    f("tag", 2, |a| run_ok(vec!["tag".into(), str_arg(a, 0, "docker.tag")?, str_arg(a, 1, "docker.tag")?], None).map(|_| Value::Nil));
    f("rmi", 1, |a| {
        let o = opts(opt_arg(a, 1), "docker.rmi")?;
        let mut args = vec!["rmi".to_string()];
        if o.get("force").is_some_and(Value::is_truthy) {
            args.push("-f".into());
        }
        args.extend(strs(&a[0]));
        run_ok(args, None).map(|_| Value::Nil)
    });
    // build(context = ".", { tag, file, args, target, noCache, platform }) → output
    f("build", 0, |a| {
        let ctx = match opt_arg(a, 0) {
            Some(Value::Str(s)) => s.clone(),
            _ => ".".into(),
        };
        let o = opts(a.iter().find(|v| matches!(v, Value::Map(_))), "docker.build")?;
        let mut args = vec!["build".to_string()];
        for (k, flag) in [("tag", "-t"), ("file", "-f"), ("target", "--target"), ("platform", "--platform")] {
            if let Some(v) = o.get(k) {
                args.push(flag.into());
                args.push(v.to_display_string());
            }
        }
        pairs("--build-arg", o.get("args"), "=", &mut args);
        if o.get("noCache").is_some_and(Value::is_truthy) {
            args.push("--no-cache".into());
        }
        args.push(ctx.into_string());
        let out = exec(args, None)?;
        if out.code != 0 {
            return Err(io_err!("docker build: {}", last_lines(&out.stderr, 15)));
        }
        Ok(Value::Str((out.stdout + &out.stderr).into()))
    });
    // ---------- networks and volumes ----------
    f("networks", 0, |_| Ok(json_lines(&run_ok(vec!["network".into(), "ls".into(), "--format".into(), "{{json .}}".into()], None)?)));
    f("volumes", 0, |_| Ok(json_lines(&run_ok(vec!["volume".into(), "ls".into(), "--format".into(), "{{json .}}".into()], None)?)));
    f("createNetwork", 1, |a| run_ok(vec!["network".into(), "create".into(), str_arg(a, 0, "docker.createNetwork")?], None).map(|s| Value::Str(s.trim().into())));
    f("removeNetwork", 1, |a| run_ok(vec!["network".into(), "rm".into(), str_arg(a, 0, "docker.removeNetwork")?], None).map(|_| Value::Nil));
    f("createVolume", 1, |a| run_ok(vec!["volume".into(), "create".into(), str_arg(a, 0, "docker.createVolume")?], None).map(|s| Value::Str(s.trim().into())));
    f("removeVolume", 1, |a| run_ok(vec!["volume".into(), "rm".into(), str_arg(a, 0, "docker.removeVolume")?], None).map(|_| Value::Nil));
    f("prune", 0, |a| {
        let what = match opt_arg(a, 0) {
            Some(v) => v.to_display_string(),
            None => "system".into(),
        };
        if !["system", "container", "image", "volume", "network", "builder"].contains(&what.as_str()) {
            return Err("docker.prune: expects system, container, image, volume, network or builder".into());
        }
        run_ok(vec![what, "prune".into(), "-f".into()], None).map(Value::str)
    });
    // ---------- compose ----------
    // compose(file = "compose.yaml", { project, cwd, env }) → ComposeProject
    f("compose", 0, |a| {
        let file = match opt_arg(a, 0) {
            Some(Value::Str(s)) => Some(s.to_string()),
            _ => None,
        };
        let o = opts(a.iter().find(|v| matches!(v, Value::Map(_))), "docker.compose")?;
        Ok(NativeObjectRef::new(Compose {
            host: TARGET.with(|t| t.borrow().clone()).map(|t| (t, REMOTE_ENGINE.with(|e| e.borrow().clone()).unwrap_or_else(|| "docker".into()))),
            file,
            project: o.get("project").map(|v| v.to_display_string()),
            cwd: o.get("cwd").map(|v| v.to_display_string()),
            env_files: o.get("envFile").map(strs).unwrap_or_default(),
        })
        .to_value())
    });
    f("readCompose", 1, |a| {
        let path = str_arg(a, 0, "docker.readCompose")?;
        let text = std::fs::read_to_string(&path).map_err(|e| io_err!("docker.readCompose: {}: {}", path, e))?;
        let j: serde_json::Value = serde_yaml::from_str(&text).map_err(|e| io_err!("docker.readCompose: {}: {}", path, e))?;
        Ok(json_to_value(&j))
    });
    f("composeString", 1, |a| compose_yaml(&a[0]).map(Value::str));
    f("writeCompose", 2, |a| {
        let path = str_arg(a, 0, "docker.writeCompose")?;
        let text = compose_yaml(&a[1])?;
        std::fs::write(&path, text).map_err(|e| io_err!("docker.writeCompose: {}: {}", path, e))?;
        Ok(Value::Nil)
    });
    // ---------- Dockerfiles ----------
    // dockerfile(from?, { as }) → Dockerfile builder
    f("dockerfile", 0, |a| {
        let mut df = Dockerfile { lines: Vec::new() };
        if let Some(Value::Str(from)) = opt_arg(a, 0) {
            df.add_from(from, opt_arg(a, 1))?;
        }
        Ok(NativeObjectRef::new(df).to_value())
    });
    f("parseDockerfile", 1, |a| Ok(NativeObjectRef::new(Dockerfile::parse(&str_arg(a, 0, "docker.parseDockerfile")?)).to_value()));
    f("readDockerfile", 1, |a| {
        let path = str_arg(a, 0, "docker.readDockerfile")?;
        let text = std::fs::read_to_string(&path).map_err(|e| io_err!("docker.readDockerfile: {}: {}", path, e))?;
        Ok(NativeObjectRef::new(Dockerfile::parse(&text)).to_value())
    });
    // remote(ssh | "user@host", { engine = "docker" }) → the same API, run on that server
    e.insert("remote".into(), nfn_var("remote", 1, |a| {
        let target = crate::native::ssh::target_of(&a[0]).map_err(|e| format!("docker.remote: {}", e))?;
        let o = opts(opt_arg(a, 1), "docker.remote")?;
        let engine = o.get("engine").map(|v| v.to_display_string()).unwrap_or_else(|| "docker".into());
        if engine != "docker" && engine != "podman" {
            return Err("docker.remote: 'engine' must be \"docker\" or \"podman\"".into());
        }
        Ok(NativeObjectRef::new(Remote { target, engine }).to_value())
    }));
    e.insert("engine".into(), Value::Str(engine().into()));
    ModuleValue {
        name: "fogu:docker".into(),
        exports: RefCell::new(e),
    }
}

fn simple(cmd: &str, a: &[Value]) -> Result<Value, String> {
    let mut args = vec![cmd.to_string()];
    args.extend(strs(&a[0]));
    run_ok(args, None).map(|_| Value::Nil)
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.trim().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Compose YAML with the usual key order (services first, then networks, volumes…).
fn compose_yaml(v: &Value) -> Result<String, String> {
    let j: serde_json::Value = serde_json::from_str(&stringify_value(v)).map_err(|e| io_err!("docker: {}", e))?;
    let order = ["name", "version", "services", "networks", "volumes", "configs", "secrets"];
    let svc_order = ["image", "build", "container_name", "command", "entrypoint", "restart", "ports", "environment", "env_file", "volumes", "depends_on", "networks"];
    fn reorder(j: serde_json::Value, keys: &[&str]) -> serde_json::Value {
        match j {
            serde_json::Value::Object(m) => {
                let mut out = serde_json::Map::new();
                for k in keys {
                    if let Some(v) = m.get(*k) {
                        out.insert(k.to_string(), v.clone());
                    }
                }
                for (k, v) in m {
                    if !out.contains_key(&k) {
                        out.insert(k, v);
                    }
                }
                serde_json::Value::Object(out)
            }
            other => other,
        }
    }
    let mut top = reorder(j, &order);
    if let Some(serde_json::Value::Object(svcs)) = top.get_mut("services") {
        for (_, s) in svcs.iter_mut() {
            *s = reorder(s.take(), &svc_order);
        }
    }
    serde_yaml::to_string(&top).map_err(|e| io_err!("docker: {}", e))
}

// ================= COMPOSE PROJECT =================

pub struct Compose {
    /// a compose project on a remote server (and its engine)
    host: Option<(crate::native::ssh::Target, String)>,
    file: Option<String>,
    project: Option<String>,
    cwd: Option<String>,
    env_files: Vec<String>,
}

impl Compose {
    fn base(&self) -> Vec<String> {
        let mut args = vec!["compose".to_string()];
        if let Some(f) = &self.file {
            args.push("-f".into());
            args.push(f.clone());
        }
        if let Some(p) = &self.project {
            args.push("-p".into());
            args.push(p.clone());
        }
        for e in &self.env_files {
            args.push("--env-file".into());
            args.push(e.clone());
        }
        args
    }
}

impl FoguObject for Compose {
    fn name(&self) -> &str {
        "ComposeProject"
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let Some((t, eng)) = self.host.clone() else { return self.call_here(method, a) };
        // a local compose file is copied to the server first
        let mut remote_dir = None;
        if let Some(f) = self.file.clone().filter(|f| std::path::Path::new(f).is_file()) {
            let base = std::path::Path::new(&f).file_name().map(|b| b.to_string_lossy().to_string()).unwrap_or("compose.yaml".into());
            let dir = format!("/tmp/fogu-compose-{}", self.project.clone().unwrap_or_else(|| "default".into()));
            let (t2, f2, d2) = (t.clone(), f.clone(), dir.clone());
            crate::native::rt::offload(move || -> Result<(), String> {
                let (code, _, err) = t2.run_bytes(&format!("mkdir -p {}", sh_quote(&d2)), None).map_err(|e| io_err!("docker: {}", e))?;
                if code != 0 {
                    return Err(io_err!("docker compose: {}", String::from_utf8_lossy(&err).trim()));
                }
                crate::native::ssh::upload_file(&t2, &f2, &format!("{}/{}", d2, base))
            })??;
            remote_dir = Some((dir, std::path::Path::new(&f).file_name().map(|b| b.to_string_lossy().to_string()).unwrap_or_default()));
        }
        let saved = (self.file.clone(), self.cwd.clone());
        if let Some((dir, base)) = remote_dir {
            self.file = Some(base);
            if self.cwd.is_none() {
                self.cwd = Some(dir);
            }
        }
        let r = on_target(Some(t), Some(eng), || self.call_here(method, a));
        (self.file, self.cwd) = saved;
        r
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["up", "down", "ps", "services", "status", "logs", "config", "pull", "build", "restart", "stop", "start", "rm", "exec"]
    }
}

impl Compose {
    fn call_here(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        // services() → [{ name, state, status, health, containers, ports }] (also the ones not running)
        // status(service) → that one entry
        if method == "services" || method == "status" {
            let mut args = self.base();
            args.extend(["config".into(), "--services".into()]);
            let out = exec(args, self.cwd.clone())?;
            if out.code != 0 {
                return Err(io_err!("docker compose services: {}", last_lines(&out.stderr, 10)));
            }
            let mut args = self.base();
            args.extend(["ps".into(), "--all".into(), "--format".into(), "json".into()]);
            let ps = exec(args, self.cwd.clone())?;
            let rows = match json_lines(&ps.stdout) {
                Value::List(l) => l.borrow().clone(),
                _ => Vec::new(),
            };
            let field = |r: &Value, k: &str| match r {
                Value::Map(m) => m.borrow().get(k).cloned().unwrap_or(Value::Nil),
                _ => Value::Nil,
            };
            let want = if method == "status" { Some(str_arg(a, 0, "status")?) } else { None };
            let mut list_out = Vec::new();
            for name in out.stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if want.as_deref().is_some_and(|w| w != name) {
                    continue;
                }
                let mine: Vec<&Value> = rows.iter().filter(|r| field(r, "Service").to_display_string() == name).collect();
                let first = mine.first().copied();
                let state = first.map_or(Value::Str("not created".into()), |r| field(r, "State"));
                list_out.push(map(vec![
                    ("name", Value::Str(name.to_string().into())),
                    ("state", state.clone()),
                    ("running", Value::Bool(state.to_display_string() == "running")),
                    ("status", first.map_or(Value::Nil, |r| field(r, "Status"))),
                    ("health", first.map_or(Value::Nil, |r| field(r, "Health"))),
                    ("containers", Value::Int(mine.len() as i64)),
                    ("ports", first.map_or(Value::Nil, |r| field(r, "Publishers"))),
                ]));
            }
            if let Some(w) = want {
                return list_out.into_iter().next().ok_or_else(|| io_err!("docker compose: no service '{}'", w));
            }
            return Ok(list(list_out));
        }
        let o = opts(a.iter().find(|v| matches!(v, Value::Map(_))), method)?;
        let services: Vec<String> = a.iter().filter(|v| matches!(v, Value::Str(_) | Value::List(_))).flat_map(strs).collect();
        let flag = |k: &str| o.get(k).is_some_and(Value::is_truthy);
        let mut args = self.base();
        let json = match method {
            // up(services?, { detach = true, build, pull, removeOrphans, wait })
            "up" => {
                args.push("up".into());
                if o.get("detach").is_none_or(Value::is_truthy) {
                    args.push("-d".into());
                }
                if flag("build") {
                    args.push("--build".into());
                }
                if flag("removeOrphans") {
                    args.push("--remove-orphans".into());
                }
                if flag("wait") {
                    args.push("--wait".into());
                }
                if let Some(p) = o.get("pull") {
                    args.push("--pull".into());
                    args.push(p.to_display_string());
                }
                false
            }
            // down({ volumes, removeOrphans, images })
            "down" => {
                args.push("down".into());
                if flag("volumes") {
                    args.push("-v".into());
                }
                if flag("removeOrphans") {
                    args.push("--remove-orphans".into());
                }
                if let Some(i) = o.get("images") {
                    args.push("--rmi".into());
                    args.push(i.to_display_string());
                }
                false
            }
            "ps" => {
                args.extend(["ps".into(), "--all".into(), "--format".into(), "json".into()]);
                true
            }
            "logs" => {
                args.extend(["logs".into(), "--no-color".into()]);
                if let Some(t) = o.get("tail") {
                    args.push("--tail".into());
                    args.push(t.to_display_string());
                }
                false
            }
            "config" => {
                args.extend(["config".into(), "--format".into(), "json".into()]);
                true
            }
            "pull" | "build" | "restart" | "stop" | "start" | "rm" => {
                args.push(method.into());
                if method == "rm" {
                    args.extend(["-f".into(), "-s".into()]);
                }
                if method == "build" && flag("noCache") {
                    args.push("--no-cache".into());
                }
                false
            }
            // exec(service, command) → { code, stdout, stderr }
            "exec" => {
                let svc = str_arg(a, 0, "exec")?;
                args.extend(["exec".into(), "-T".into(), svc]);
                args.extend(match a.get(1) {
                    Some(Value::Str(s)) => vec!["sh".into(), "-c".into(), s.to_string()],
                    Some(other) => strs(other),
                    None => return Err("exec(service, command)".into()),
                });
                let out = exec(args, self.cwd.clone())?;
                return Ok(map(vec![("code", Value::Int(out.code as i64)), ("stdout", Value::Str(out.stdout.into())), ("stderr", Value::Str(out.stderr.into()))]));
            }
            other => return Err(format!("method '{}' does not exist on ComposeProject ({})", other, FoguObject::methods(self).join(" "))),
        };
        if method != "exec" {
            args.extend(services);
        }
        let out = exec(args, self.cwd.clone())?;
        if out.code != 0 {
            return Err(io_err!("docker compose {}: {}", method, last_lines(&out.stderr, 15)));
        }
        Ok(if json {
            json_lines(&out.stdout)
        } else if method == "logs" {
            Value::Str((out.stdout + &out.stderr).into())
        } else {
            Value::Nil
        })
    }
}

// ================= SWARM SERVICE =================

pub struct SwarmService {
    name: String,
    host: Option<(crate::native::ssh::Target, String)>,
}

impl SwarmService {
    fn call_here(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let n = self.name.clone();
        let svc = |more: &[&str]| -> Vec<String> {
            let mut v = vec!["service".to_string()];
            v.extend(more.iter().map(|s| s.to_string()));
            v
        };
        match method {
            // status() → { name, mode, replicas, running, desired, image, updatedAt }
            "status" => {
                let out = run_ok(svc(&["inspect", &n]), None)?;
                let Value::List(l) = json_lines(&out.replace('\n', "")) else { return Ok(Value::Nil) };
                let Some(info) = l.borrow().first().cloned() else { return Ok(Value::Nil) };
                let tasks = json_lines(&run_ok(svc(&["ps", &n, "--format", "{{json .}}", "--filter", "desired-state=running"]), None)?);
                let running = match &tasks {
                    Value::List(t) => t.borrow().iter().filter(|x| matches!(x, Value::Map(m) if m.borrow().get("CurrentState").is_some_and(|s| s.to_display_string().starts_with("Running")))).count(),
                    _ => 0,
                };
                let get = |v: &Value, path: &[&str]| -> Value {
                    let mut cur = v.clone();
                    for k in path {
                        cur = match &cur {
                            Value::Map(m) => m.borrow().get(*k).cloned().unwrap_or(Value::Nil),
                            _ => Value::Nil,
                        };
                    }
                    cur
                };
                let replicated = get(&info, &["Spec", "Mode", "Replicated", "Replicas"]);
                Ok(map(vec![
                    ("name", Value::Str(n.into())),
                    ("mode", Value::Str(if matches!(replicated, Value::Nil) { "global".into() } else { "replicated".into() })),
                    ("desired", replicated),
                    ("running", Value::Int(running as i64)),
                    ("image", get(&info, &["Spec", "TaskTemplate", "ContainerSpec", "Image"])),
                    ("updatedAt", get(&info, &["UpdatedAt"])),
                ]))
            }
            // tasks() → the service's containers (docker service ps)
            "tasks" => Ok(json_lines(&run_ok(svc(&["ps", &n, "--no-trunc", "--format", "{{json .}}"]), None)?)),
            // restart(): rolling restart of every task
            "restart" => run_ok(svc(&["update", "--force", "--detach", &n]), None).map(|_| Value::Nil),
            // stop() → scale to 0 · start(replicas = 1)
            "stop" => run_ok(svc(&["scale", "--detach", &format!("{}=0", n)]), None).map(|_| Value::Nil),
            "start" | "scale" => {
                let r = match a.first() {
                    Some(Value::Int(x)) if *x >= 0 => *x,
                    None if method == "start" => 1,
                    _ => return Err(format!("{}: expects a number of replicas", method)),
                };
                run_ok(svc(&["scale", "--detach", &format!("{}={}", n, r)]), None).map(|_| Value::Nil)
            }
            // update({ image, env, replicas, args })
            "update" => {
                let o = opts(a.first(), "update")?;
                let mut args = svc(&["update", "--detach"]);
                if let Some(i) = o.get("image") {
                    args.push("--image".into());
                    args.push(i.to_display_string());
                }
                if let Some(r) = o.get("replicas") {
                    args.push("--replicas".into());
                    args.push(r.to_display_string());
                }
                pairs("--env-add", o.get("env"), "=", &mut args);
                if let Some(x) = o.get("args") {
                    args.extend(strs(x));
                }
                args.push(n);
                run_ok(args, None).map(|_| Value::Nil)
            }
            "logs" => {
                let o = opts(a.first(), "logs")?;
                let mut args = svc(&["logs", "--no-task-ids"]);
                if let Some(t) = o.get("tail") {
                    args.push("--tail".into());
                    args.push(t.to_display_string());
                }
                args.push(n);
                let out = exec(args, None)?;
                if out.code != 0 {
                    return Err(io_err!("docker service logs: {}", out.stderr.trim()));
                }
                Ok(Value::Str((out.stdout + &out.stderr).into()))
            }
            "remove" => run_ok(svc(&["rm", &n]), None).map(|_| Value::Nil),
            other => Err(format!("method '{}' does not exist on SwarmService ({})", other, FoguObject::methods(self).join(" "))),
        }
    }
}

impl FoguObject for SwarmService {
    fn name(&self) -> &str {
        "SwarmService"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "name" => Some(Value::Str(self.name.clone().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        match self.host.clone() {
            Some((t, e)) => on_target(Some(t), Some(e), || self.call_here(method, a)),
            None => self.call_here(method, a),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["status", "tasks", "restart", "stop", "start", "scale", "update", "logs", "remove"]
    }
}

// ================= REMOTE =================

/// `docker.remote(ssh)`: every module function, run on the server.
pub struct Remote {
    target: crate::native::ssh::Target,
    engine: String,
}

const LOCAL_ONLY: &[&str] = &["use", "remote", "dockerfile", "parseDockerfile", "readDockerfile", "readCompose", "writeCompose", "composeString"];

impl FoguObject for Remote {
    fn name(&self) -> &str {
        "DockerRemote"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "engine" => Some(Value::Str(self.engine.clone().into())),
            "host" => Some(Value::Str(self.target.display().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let f = EXPORTS.with(|e| {
            e.borrow_mut()
                .get_or_insert_with(|| build().exports.into_inner())
                .get(method)
                .cloned()
        });
        match f {
            Some(Value::NativeFn(nf)) if !LOCAL_ONLY.contains(&method) => {
                on_target(Some(self.target.clone()), Some(self.engine.clone()), || (nf.func)(a))
            }
            _ => Err(format!("method '{}' does not exist on DockerRemote (the fogu:docker functions except {})", method, LOCAL_ONLY.join(" "))),
        }
    }
}

// ================= DOCKERFILE =================

pub struct Dockerfile {
    /// (instruction, value); comments and blank lines keep instruction "" / "#"
    lines: Vec<(String, String)>,
}

/// A JSON array for CMD/ENTRYPOINT/RUN given a list; a str stays shell form.
fn exec_form(v: &Value) -> String {
    match v {
        Value::List(_) => serde_json::to_string(&strs(v)).unwrap_or_default(),
        other => other.to_display_string(),
    }
}

fn quote_env(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '"' || c == '$' || c == '\\') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

impl Dockerfile {
    fn parse(text: &str) -> Dockerfile {
        let mut lines = Vec::new();
        let mut pending = String::new();
        for raw in text.lines() {
            let line = raw.trim_end();
            if pending.is_empty() {
                let t = line.trim_start();
                if t.is_empty() {
                    lines.push((String::new(), String::new()));
                    continue;
                }
                if let Some(c) = t.strip_prefix('#') {
                    lines.push(("#".into(), c.trim().to_string()));
                    continue;
                }
            }
            // continuation lines (backslash at the end)
            if let Some(cont) = line.strip_suffix('\\') {
                pending.push_str(cont);
                pending.push_str("\\\n");
                continue;
            }
            pending.push_str(line);
            let full = std::mem::take(&mut pending);
            let t = full.trim_start();
            let (instr, rest) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
            lines.push((instr.to_ascii_uppercase(), rest.trim().to_string()));
        }
        if !pending.is_empty() {
            let t = pending.trim_start().to_string();
            let (instr, rest) = t.split_once(char::is_whitespace).unwrap_or((&t, ""));
            lines.push((instr.to_ascii_uppercase(), rest.trim().to_string()));
        }
        Dockerfile { lines }
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (i, v) in &self.lines {
            match i.as_str() {
                "" => out.push('\n'),
                "#" => out.push_str(&format!("# {}\n", v)),
                _ => out.push_str(&format!("{} {}\n", i, v)),
            }
        }
        out
    }

    fn add_from(&mut self, image: &str, o: Option<&Value>) -> Result<(), String> {
        let o = opts(o, "from")?;
        let mut v = String::new();
        if let Some(p) = o.get("platform") {
            v.push_str(&format!("--platform={} ", p.to_display_string()));
        }
        v.push_str(image);
        if let Some(n) = o.get("as") {
            v.push_str(&format!(" AS {}", n.to_display_string()));
        }
        if !self.lines.is_empty() {
            self.lines.push((String::new(), String::new()));
        }
        self.lines.push(("FROM".into(), v));
        Ok(())
    }
}

impl FoguObject for Dockerfile {
    fn name(&self) -> &str {
        "Dockerfile"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            // [{ instruction, value }] without blank lines and comments
            "instructions" => Some(list(
                self.lines
                    .iter()
                    .filter(|(i, _)| !i.is_empty() && i != "#")
                    .map(|(i, v)| map(vec![("instruction", Value::Str(i.clone().into())), ("value", Value::Str(v.clone().into()))]))
                    .collect(),
            )),
            "text" => Some(Value::Str(self.render().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let s = |i: usize| str_arg(a, i, method);
        let mut push = |instr: &str, v: String| self.lines.push((instr.into(), v));
        match method {
            "from" => {
                let img = s(0)?;
                self.add_from(&img, opt_arg(a, 1))?;
            }
            // run(cmd | [cmds]) — a list of str becomes one RUN joined with &&
            "run" => match &a.first() {
                Some(Value::List(l)) if l.borrow().iter().all(|x| matches!(x, Value::Str(_))) && l.borrow().len() > 1 => {
                    let cmds = strs(&a[0]);
                    push("RUN", cmds.join(" \\\n    && "));
                }
                Some(v) => push("RUN", exec_form(v)),
                None => return Err("run(command)".into()),
            },
            "cmd" | "entrypoint" => push(&method.to_ascii_uppercase(), exec_form(a.first().ok_or(format!("{}(command)", method))?)),
            // copy(src, dest, { from, chown, chmod }) · add(...)
            "copy" | "add" => {
                let o = opts(opt_arg(a, 2), method)?;
                let mut v = String::new();
                for k in ["from", "chown", "chmod"] {
                    if let Some(x) = o.get(k) {
                        v.push_str(&format!("--{}={} ", k, x.to_display_string()));
                    }
                }
                let srcs = strs(a.first().unwrap_or(&Value::Nil));
                v.push_str(&format!("{} {}", srcs.join(" "), s(1)?));
                push(&method.to_ascii_uppercase(), v);
            }
            "workdir" | "user" | "shell" | "stopsignal" => {
                let v = if method == "shell" { exec_form(&a[0]) } else { s(0)? };
                push(&method.to_ascii_uppercase(), v)
            }
            // env(key, value) · env({ k: v })
            "env" | "label" | "arg" => {
                let instr = method.to_ascii_uppercase();
                match (a.first(), a.get(1)) {
                    (Some(Value::Map(m)), _) => {
                        for (k, v) in sorted(&m.borrow()) {
                            push(&instr, format!("{}={}", k, quote_env(&v.to_display_string())));
                        }
                    }
                    (Some(Value::Str(k)), Some(v)) => push(&instr, format!("{}={}", k, quote_env(&v.to_display_string()))),
                    (Some(Value::Str(k)), None) if method == "arg" => push(&instr, k.to_string()),
                    _ => return Err(format!("{}(key, value) or {}({{ key: value }})", method, method)),
                }
            }
            "expose" => {
                let ports: Vec<String> = a.iter().flat_map(strs).collect();
                push("EXPOSE", ports.join(" "));
            }
            "volume" => push("VOLUME", serde_json::to_string(&a.iter().flat_map(strs).collect::<Vec<_>>()).unwrap_or_default()),
            // healthcheck(cmd, { interval, timeout, retries, startPeriod })
            "healthcheck" => {
                let o = opts(opt_arg(a, 1), "healthcheck")?;
                let mut v = String::new();
                for (k, flag) in [("interval", "interval"), ("timeout", "timeout"), ("retries", "retries"), ("startPeriod", "start-period")] {
                    if let Some(x) = o.get(k) {
                        v.push_str(&format!("--{}={} ", flag, x.to_display_string()));
                    }
                }
                v.push_str(&format!("CMD {}", exec_form(a.first().unwrap_or(&Value::Nil))));
                push("HEALTHCHECK", v);
            }
            "comment" => push("#", s(0)?),
            "blank" => push("", String::new()),
            // raw(instruction, value)
            "raw" => push(&s(0)?.to_ascii_uppercase(), s(1)?),
            // editing: find(instr) → indexes · replace(instr, value) (first) · remove(index) · insert(index, instr, value)
            "find" => {
                let instr = s(0)?.to_ascii_uppercase();
                let idx: Vec<Value> = self.instr_indexes().into_iter().enumerate().filter(|(_, li)| self.lines[*li].0 == instr).map(|(n, _)| Value::Int(n as i64)).collect();
                return Ok(list(idx));
            }
            "replace" => {
                let instr = s(0)?.to_ascii_uppercase();
                let v = exec_form(a.get(1).unwrap_or(&Value::Nil));
                match self.lines.iter_mut().find(|(i, _)| *i == instr) {
                    Some(l) => l.1 = v,
                    None => return Ok(Value::Bool(false)),
                }
                return Ok(Value::Bool(true));
            }
            "remove" | "insert" => {
                let n = match a.first() { Some(Value::Int(n)) if *n >= 0 => *n as usize, _ => return Err(format!("{}: expects an instruction index", method)) };
                let idx = self.instr_indexes();
                if method == "remove" {
                    let li = *idx.get(n).ok_or_else(|| format!("remove: no instruction {}", n))?;
                    self.lines.remove(li);
                } else {
                    let li = idx.get(n).copied().unwrap_or(self.lines.len());
                    self.lines.insert(li, (s(1)?.to_ascii_uppercase(), exec_form(a.get(2).unwrap_or(&Value::Nil))));
                }
            }
            "toString" => return Ok(Value::Str(self.render().into())),
            "save" => {
                let path = match opt_arg(a, 0) { Some(_) => s(0)?, None => "Dockerfile".into() };
                std::fs::write(&path, self.render()).map_err(|e| io_err!("Dockerfile.save: {}: {}", path, e))?;
            }
            other => return Err(format!("method '{}' does not exist on Dockerfile ({})", other, self.methods().join(" "))),
        }
        Ok(Value::Nil)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "from", "run", "cmd", "entrypoint", "copy", "add", "workdir", "user", "shell", "stopsignal", "env", "label", "arg",
            "expose", "volume", "healthcheck", "comment", "blank", "raw", "find", "replace", "remove", "insert", "toString", "save",
        ]
    }
}

impl Dockerfile {
    /// Line indexes of the real instructions.
    fn instr_indexes(&self) -> Vec<usize> {
        self.lines.iter().enumerate().filter(|(_, (i, _))| !i.is_empty() && i != "#").map(|(n, _)| n).collect()
    }
}
