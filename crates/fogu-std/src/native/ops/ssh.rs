//! `fogu:ssh` module — remote commands, file copies (scp) and tunnels, built on
//! the system OpenSSH client (`ssh`/`scp`, present on Linux, macOS and Windows
//! 10+). It honours `~/.ssh/config`, keys, the agent and `known_hosts`.
//!
//! ```fogu
//! import ssh from "fogu:ssh"
//!
//! const s = ssh.connect("deploy@example.com")?             // or { host, user, port, key, password }
//! const r = s.run("uname -a")?                             // { ok, code, stdout, stderr }
//! s.upload("dist/app", "/opt/app")?                        // local → remote (scp -r)
//! s.download("/var/log/app.log", "app.log")?               // remote → local
//! const db = s.forward(5433, "localhost", 5432)?           // local:5433 → remote localhost:5432 (ssh -L)
//! const web = s.reverse(8080, "localhost", 3000)?          // remote:8080 → local localhost:3000 (ssh -R)
//! db.close()
//!
//! const sh = s.shell()?                                    // one persistent remote shell
//! sh.run("cd /var/www && export APP_ENV=prod")?            // state survives between commands
//! print(sh.run("pwd; echo $APP_ENV")?.stdout)              // /var/www\nprod
//! sh.close()
//! const t = s.shell({ pty: true })?                        // expect-style: send/expect/read
//! t.send("sudo -k; sudo whoami\n"); t.expect("password")?; t.send(pw + "\n")
//! s.interactive()                                          // hand the terminal to the user (ssh -t)
//! ```
//!
//! Passwords are supported without a terminal: `fogu` itself acts as the
//! `SSH_ASKPASS` program (see [`askpass_reply`]). Connection, authentication
//! and copy failures are operational errors (`error` values); a remote command
//! that exits with a non-zero code is not (check `r.ok` / `r.code`).

use ahash::AHashMap as HashMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{map, max_args, nfn_var, opt_arg, str_arg, thread_poller};

/// Environment variable carrying the password to the askpass helper.
pub const ASKPASS_SECRET: &str = "FOGU_ASKPASS_SECRET";

/// When OpenSSH runs `fogu` as its `SSH_ASKPASS` program, the password is
/// in the environment: print it and exit. Called first thing at startup.
pub fn askpass_reply() -> Option<i32> {
    let secret = std::env::var(ASKPASS_SECRET).ok()?;
    // OpenSSH passes the prompt as the only argument
    let prompt = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    if prompt.contains("yes/no") {
        println!("no"); // never accept an unknown host key through askpass
    } else {
        println!("{}", secret);
    }
    Some(0)
}

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    ModuleValue {
        name: "fogu:ssh".into(),
        exports: std::cell::RefCell::new(e),
    }
}

#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) host: String,
    pub(crate) user: Option<String>,
    pub(crate) port: Option<u16>,
    pub(crate) key: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) strict: String,
    pub(crate) known_hosts: Option<String>,
    pub(crate) timeout: u64,
    pub(crate) options: Vec<(String, String)>,
}

impl Target {
    /// A target for other modules (fogu:playbook).
    pub(crate) fn new(host: &str, user: Option<String>, port: Option<u16>, key: Option<String>, password: Option<String>) -> Target {
        Target {
            host: host.to_string(),
            user,
            port,
            key,
            password,
            strict: "accept-new".into(),
            known_hosts: None,
            timeout: 10,
            options: Vec::new(),
        }
    }

    /// Runs a remote command with binary stdin: `(exit code, stdout, stderr)`;
    /// code 255 means the connection or the authentication failed.
    pub(crate) fn run_bytes(&self, remote: &str, input: Option<&[u8]>) -> Result<(i64, Vec<u8>, Vec<u8>), String> {
        let mut c = self.command("ssh");
        c.arg(self.dest(false)).arg(remote);
        c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| format!("cannot start the ssh client: {}", e))?;
        if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
            use std::io::Write;
            let data = data.to_vec();
            std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            });
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        Ok((out.status.code().unwrap_or(-1) as i64, out.stdout, out.stderr))
    }

    pub(crate) fn display(&self) -> String {
        self.dest(false)
    }
}

impl Target {
    /// `ssh -W %h:%p …` through this host, with its key, port and host-key settings
    /// (OpenSSH's ProxyJump would not inherit them).
    pub(crate) fn proxy_command(&self) -> String {
        let q = |s: &str| if s.contains(' ') { format!("\"{}\"", s) } else { s.to_string() };
        let mut c = String::from("ssh -W %h:%p");
        if let Some(p) = self.port {
            c.push_str(&format!(" -p {}", p));
        }
        if let Some(k) = &self.key {
            let k = expand_home(k);
            let k = if std::path::Path::new(&k).is_relative() {
                std::env::current_dir().map(|d| d.join(&k).to_string_lossy().to_string()).unwrap_or(k)
            } else {
                k
            };
            c.push_str(&format!(" -i {}", q(&k)));
        }
        c.push_str(&format!(" -o StrictHostKeyChecking={} -o BatchMode=yes -o LogLevel=ERROR", self.strict));
        if let Some(kh) = &self.known_hosts {
            c.push_str(&format!(" -o UserKnownHostsFile={}", q(&expand_home(kh))));
        }
        for (k, v) in &self.options {
            if k != "ProxyCommand" {
                c.push_str(&format!(" -o {}={}", k, q(v)));
            }
        }
        c.push(' ');
        c.push_str(&self.dest(false));
        c
    }

    /// `user@host` (with brackets for IPv6 in scp paths).
    fn dest(&self, for_scp: bool) -> String {
        let host = if for_scp && self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match &self.user {
            Some(u) => format!("{}@{}", u, host),
            None => host,
        }
    }

    fn command(&self, program: &str) -> Command {
        // FOGU_SSH: the ssh client to use (another build, a wrapper, tests)
        let exe = match std::env::var("FOGU_SSH") {
            Ok(p) if program == "ssh" && !p.is_empty() => p,
            _ => program.to_string(),
        };
        let mut c = Command::new(exe);
        let scp = program == "scp";
        let opt = |c: &mut Command, k: &str, v: &str| {
            c.arg("-o").arg(format!("{}={}", k, v));
        };
        if let Some(p) = self.port {
            c.arg(if scp { "-P" } else { "-p" }).arg(p.to_string());
        }
        if let Some(k) = &self.key {
            c.arg("-i").arg(expand_home(k));
        }
        opt(&mut c, "StrictHostKeyChecking", &self.strict);
        opt(&mut c, "ConnectTimeout", &self.timeout.to_string());
        opt(&mut c, "LogLevel", "ERROR");
        if let Some(kh) = &self.known_hosts {
            opt(&mut c, "UserKnownHostsFile", &expand_home(kh));
        }
        match &self.password {
            None => opt(&mut c, "BatchMode", "yes"),
            Some(pw) => {
                opt(&mut c, "BatchMode", "no");
                opt(&mut c, "NumberOfPasswordPrompts", "1");
                opt(&mut c, "PreferredAuthentications", "publickey,password,keyboard-interactive");
                if let Ok(exe) = std::env::current_exe() {
                    c.env("SSH_ASKPASS", exe);
                }
                c.env("SSH_ASKPASS_REQUIRE", "force").env("DISPLAY", ":0").env(ASKPASS_SECRET, pw);
            }
        }
        for (k, v) in &self.options {
            opt(&mut c, k, v);
        }
        c
    }
}

pub(crate) fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return std::path::Path::new(&home).join(rest).to_string_lossy().to_string();
        }
    }
    p.to_string()
}

/// `"user@host:port"` → (user, host, port).
fn parse_target(s: &str) -> Result<(Option<String>, String, Option<u16>), String> {
    let (user, rest) = match s.rsplit_once('@') {
        Some((u, r)) => (Some(u.to_string()), r),
        None => (None, s),
    };
    // [ipv6]:port, host:port, host
    let (host, port) = if let Some(r) = rest.strip_prefix('[') {
        let (h, tail) = r.split_once(']').ok_or_else(|| format!("invalid ssh target '{}'", s))?;
        (h.to_string(), tail.strip_prefix(':').map(str::to_string))
    } else if rest.matches(':').count() == 1 {
        let (h, p) = rest.split_once(':').unwrap();
        (h.to_string(), Some(p.to_string()))
    } else {
        (rest.to_string(), None)
    };
    let port = match port {
        Some(p) => Some(p.parse::<u16>().map_err(|_| format!("invalid port in ssh target '{}'", s))?),
        None => None,
    };
    if host.is_empty() {
        return Err(format!("invalid ssh target '{}'", s));
    }
    Ok((user, host, port))
}

/// `connect("user@host:22")` or `connect({ host, user, port, key, password,
/// strictHostKey, knownHosts, timeout, options })`. Checks the connection.
fn connect(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ssh.connect")?;
    let mut t = Target {
        host: String::new(),
        user: None,
        port: None,
        key: None,
        password: None,
        strict: "accept-new".into(),
        known_hosts: None,
        timeout: 10,
        options: Vec::new(),
    };
    let opts_map;
    let opts: Option<std::cell::Ref<HashMap<String, Value>>>;
    match &a[0] {
        Value::Str(s) => {
            let (u, h, p) = parse_target(s)?;
            t.user = u;
            t.host = h;
            t.port = p;
            opts_map = match opt_arg(a, 1) {
                Some(Value::Map(m)) => Some(m.clone()),
                None => None,
                Some(other) => return Err(format!("ssh.connect: options must be a map, got '{}'", other.type_name())),
            };
        }
        Value::Map(m) => opts_map = Some(m.clone()),
        other => return Err(format!("ssh.connect expects \"user@host\" or a map, got '{}'", other.type_name())),
    }
    opts = opts_map.as_ref().map(|m| m.borrow());
    if let Some(m) = &opts {
        let s = |k: &str| match m.get(k) {
            Some(Value::Str(v)) => Some(v.to_string()),
            _ => None,
        };
        if let Some(h) = s("host") {
            let (u, h, p) = parse_target(&h)?;
            t.host = h;
            t.user = t.user.clone().or(u);
            t.port = t.port.or(p);
        }
        t.user = s("user").or(t.user.clone());
        t.key = s("key");
        t.password = s("password");
        t.known_hosts = s("knownHosts");
        if let Some(v) = m.get("strictHostKey") {
            t.strict = match v {
                Value::Bool(true) => "yes".into(),
                Value::Bool(false) => "no".into(),
                other => other.to_display_string(),
            };
        }
        if let Some(Value::Int(p)) = m.get("port") {
            t.port = Some(u16::try_from(*p).map_err(|_| format!("ssh.connect: invalid port {}", p))?);
        }
        match m.get("timeout") {
            Some(Value::Duration(ns)) => t.timeout = ((*ns as u64) / 1_000_000_000).max(1),
            Some(Value::Int(s)) => t.timeout = (*s as u64).max(1),
            _ => {}
        }
        // through a bastion: { jump: ssh connection | "user@host:port" }
        if let Some(j) = m.get("jump") {
            let jt = target_of(j).map_err(|e| format!("ssh.connect: 'jump': {}", e))?;
            t.options.push(("ProxyCommand".into(), jt.proxy_command()));
        }
        if let Some(Value::Map(o)) = m.get("options") {
            let mut pairs: Vec<(String, String)> =
                o.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect();
            pairs.sort();
            t.options.extend(pairs);
        }
    }
    drop(opts);
    if t.host.is_empty() {
        return Err("ssh.connect: no host given".into());
    }

    // check the connection and the authentication once
    let r = exec(&t, "exit 0", None)?;
    if r.code != 0 {
        return Err(io_err!("ssh.connect: {}: {}", t.dest(false), r.stderr.trim()));
    }
    Ok(NativeObjectRef::new(SshValue { target: t }).to_value())
}

struct Outcome {
    code: i64,
    stdout: String,
    stderr: String,
}

fn exec(t: &Target, remote: &str, input: Option<&str>) -> Result<Outcome, String> {
    let mut c = t.command("ssh");
    c.arg(t.dest(false)).arg(remote);
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| io_err!("ssh: cannot start the ssh client: {}", e))?;
    // stdin from its own thread: a remote command that writes a lot first
    // cannot deadlock us
    let feeder = match (input.map(str::to_string), child.stdin.take()) {
        (Some(text), Some(mut stdin)) => Some(std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        })),
        _ => None,
    };
    let out = child.wait_with_output().map_err(|e| io_err!("ssh: {}", e));
    if let Some(f) = feeder {
        let _ = f.join();
    }
    let out = out?;
    Ok(Outcome {
        code: out.status.code().unwrap_or(-1) as i64,
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

/// ssh reports connection/auth failures with exit code 255.
fn check_transport(r: &Outcome, what: &str, t: &Target) -> Result<(), String> {
    if r.code == 255 {
        return Err(io_err!("ssh.{}: {}: {}", what, t.dest(false), r.stderr.trim()));
    }
    Ok(())
}

pub struct SshValue {
    target: Target,
}

fn scp(t: &Target, what: &str, from: String, to: String) -> Result<Value, String> {
    {
        let mut c = t.command("scp");
        c.arg("-r").arg("-q").arg(&from).arg(&to);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = c.output().map_err(|e| io_err!("ssh.{}: cannot start scp: {}", what, e))?;
        if !out.status.success() {
            return Err(io_err!(
                "ssh.{}: {} → {}: {}",
                what,
                from,
                to,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(Value::Nil)
    }
}

impl SshValue {
    fn remote(&self, path: &str) -> String {
        format!("{}:{}", self.target.dest(true), path)
    }

    /// Starts `ssh -N -L/-R …` and waits until the tunnel is ready.
    fn tunnel(&self, kind: &'static str, args: &[Value]) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!("{}(port, host, hostPort) expects 3 arguments", kind));
        }
        let port = |v: &Value, what: &str| -> Result<u16, String> {
            match v {
                Value::Int(p) => u16::try_from(*p).map_err(|_| format!("{}: invalid port {}", what, p)),
                other => Err(format!("{}: port must be int, got '{}'", what, other.type_name())),
            }
        };
        let mut listen = port(&args[0], kind)?;
        let host = str_arg(args, 1, kind)?;
        let to = port(&args[2], kind)?;
        if kind == "forward" && listen == 0 {
            // pick a free local port
            listen = TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .map(|a| a.port())
                .map_err(|e| io_err!("ssh.forward: no free local port: {}", e))?;
        }
        let spec = format!("{}:{}:{}", listen, host, to);
        let mut c = self.target.command("ssh");
        c.arg("-N")
            .arg("-o")
            .arg("ExitOnForwardFailure=yes")
            .arg("-o")
            .arg("ServerAliveInterval=30")
            .arg(if kind == "forward" { "-L" } else { "-R" })
            .arg(&spec)
            .arg(self.target.dest(false));
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| io_err!("ssh.{}: cannot start ssh: {}", kind, e))?;

        let deadline = Instant::now() + Duration::from_secs(self.target.timeout + 5);
        let settle = Instant::now() + Duration::from_millis(1500);
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                return Err(io_err!("ssh.{}: {} via {}: {}", kind, spec, self.target.dest(false), err.trim()));
            }
            let ready = if kind == "forward" {
                TcpStream::connect_timeout(&([127, 0, 0, 1], listen).into(), Duration::from_millis(200)).is_ok()
            } else {
                // a failed remote forward makes ssh exit (ExitOnForwardFailure)
                Instant::now() >= settle
            };
            if ready {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Err(io_err!("ssh.{}: {}: timed out waiting for the tunnel", kind, spec));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(NativeObjectRef::new(TunnelValue {
            child: Some(child),
            kind,
            listen,
            host,
            to,
            via: self.target.dest(false),
        })
        .to_value())
    }
}

impl FoguObject for SshValue {
    fn name(&self) -> &str {
        "Ssh"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "host" => Some(Value::Str(self.target.host.clone().into())),
            "user" => Some(self.target.user.clone().map(Value::str).unwrap_or(Value::Nil)),
            "port" => Some(Value::Int(self.target.port.unwrap_or(22) as i64)),
            _ => None,
        }
    }

    /// `run`, `upload` and `download` run the ssh/scp client off the
    /// interpreter thread: other `fg` tasks keep going.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let t = self.target.clone();
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // run(cmd, { input }) → { ok, code, stdout, stderr }
                "run" => {
                    max_args(args, 2, "run")?;
                    let cmd = str_arg(args, 0, "run")?;
                    let input = match opt_arg(args, 1) {
                        Some(Value::Map(m)) => match m.borrow().get("input") {
                            Some(Value::Str(s)) => Some(s.to_string()),
                            _ => None,
                        },
                        _ => None,
                    };
                    thread_poller(
                        move || {
                            let r = exec(&t, &cmd, input.as_deref())?;
                            check_transport(&r, "run", &t)?;
                            Ok(r)
                        },
                        |r| {
                            Ok(map(vec![
                                ("ok", Value::Bool(r.code == 0)),
                                ("code", Value::Int(r.code)),
                                ("stdout", Value::Str(r.stdout.into())),
                                ("stderr", Value::Str(r.stderr.into())),
                            ]))
                        },
                    )
                }
                "upload" => {
                    let local = str_arg(args, 0, "upload")?;
                    let remote = self.remote(&str_arg(args, 1, "upload")?);
                    thread_poller(move || scp(&t, "upload", local, remote).map(|_| ()), |_| Ok(Value::Nil))
                }
                "download" => {
                    let remote = self.remote(&str_arg(args, 0, "download")?);
                    let local = str_arg(args, 1, "download")?;
                    thread_poller(move || scp(&t, "download", remote, local).map(|_| ()), |_| Ok(Value::Nil))
                }
                // interactive(cmd?) → exit code: the user's terminal talks to the remote shell (ssh -t)
                "interactive" => {
                    max_args(args, 1, "interactive")?;
                    let cmd = match opt_arg(args, 0) {
                        Some(v) => Some(v.to_display_string()),
                        None => None,
                    };
                    thread_poller(
                        move || {
                            let mut c = t.command("ssh");
                            c.arg("-t").arg(t.dest(false));
                            if let Some(cmd) = &cmd {
                                c.arg(cmd);
                            }
                            let st = c.status().map_err(|e| io_err!("ssh.interactive: cannot start ssh: {}", e))?;
                            let code = st.code().unwrap_or(-1) as i64;
                            if code == 255 {
                                return Err(io_err!("ssh.interactive: {}: the connection failed", t.dest(false)));
                            }
                            Ok(code)
                        },
                        |code| Ok(Value::Int(code)),
                    )
                }
                // mount(remote, local, { readonly, keep, options, jump, tunnel }) → Mount (sshfs)
                "mount" => {
                    max_args(args, 3, "mount")?;
                    let remote = str_arg(args, 0, "mount")?;
                    let local = str_arg(args, 1, "mount")?;
                    let mut t = t;
                    let mut o = crate::native::ssh_mount::MountOpts { readonly: false, keep: false, options: Vec::new() };
                    if let Some(Value::Map(m)) = opt_arg(args, 2) {
                        let m = m.borrow();
                        o.readonly = m.get("readonly").is_some_and(Value::is_truthy);
                        o.keep = m.get("keep").is_some_and(Value::is_truthy);
                        if let Some(Value::List(l)) = m.get("options") {
                            o.options = l.borrow().iter().map(|v| v.to_display_string()).collect();
                        }
                        // through a bastion: { jump: ssh connection | "user@host:port" }
                        if let Some(j) = m.get("jump") {
                            let jt = target_of(j).map_err(|e| format!("mount: 'jump': {}", e))?;
                            t.options.push(("ProxyCommand".into(), jt.proxy_command()));
                        }
                        // through an open tunnel: { tunnel: s.forward(2222, "inner-host", 22) }
                        if let Some(tv) = m.get("tunnel") {
                            let port = tunnel_port(tv)?;
                            t.options.push(("HostKeyAlias".into(), t.host.clone()));
                            t.host = "127.0.0.1".into();
                            t.port = Some(port);
                        }
                    }
                    let host = t.dest(false);
                    let r2 = remote.clone();
                    thread_poller(
                        move || crate::native::ssh_mount::mount(&t, &r2, &local, &o),
                        move |local| {
                            Ok(NativeObjectRef::new(crate::native::ssh_mount::MountValue { local, remote, host, mounted: true }).to_value())
                        },
                    )
                }
                _ => return Ok(None),
            }))
        })();
        match r {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // local → remote: connections to localhost:port go to host:hostPort on the server
            "forward" => self.tunnel("forward", args),
            // shell({ pty, shell, cwd, env }) → SshShell: one persistent remote shell
            "shell" => {
                max_args(args, 1, "shell")?;
                let o = match opt_arg(args, 0) {
                    None => HashMap::new(),
                    Some(Value::Map(m)) => m.borrow().clone(),
                    Some(other) => return Err(format!("shell: options must be a map, got '{}'", other.type_name())),
                };
                Ok(NativeObjectRef::new(SshShell::open(&self.target, &o)?).to_value())
            }
            // remote → local: connections to the server's port come to host:hostPort here
            "reverse" => self.tunnel("reverse", args),
            "close" => Ok(Value::Nil),
            other => Err(format!("method '{}' does not exist on Ssh", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["run", "shell", "interactive", "upload", "download", "mount", "forward", "reverse", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The target of an `Ssh` value or a `"user@host[:port]"` string (for
/// modules that act on remote hosts, like `fogu:service`).
pub(crate) fn target_of(v: &Value) -> Result<Target, String> {
    match v {
        Value::Str(s) => {
            let (u, h, p) = parse_target(s)?;
            Ok(Target::new(&h, u, p, None, None))
        }
        Value::Object(o) => {
            let inner = o.inner();
            let b = inner.borrow();
            b.as_any()
                .downcast_ref::<SshValue>()
                .map(|s| s.target.clone())
                .ok_or_else(|| format!("expected an ssh connection, got '{}'", o.name()))
        }
        other => Err(format!("expected an ssh connection or \"user@host\", got '{}'", other.type_name())),
    }
}

/// Copies a local file to the remote host (scp).
pub(crate) fn upload_file(t: &Target, local: &str, remote: &str) -> Result<(), String> {
    scp(t, "upload", local.to_string(), format!("{}:{}", t.dest(true), remote)).map(|_| ())
}

/// A running `ssh -L` / `ssh -R` process; closed on `close()` or when dropped.
pub struct TunnelValue {
    child: Option<Child>,
    kind: &'static str,
    listen: u16,
    host: String,
    to: u16,
    via: String,
}

/// The local port of a `forward` tunnel value.
fn tunnel_port(v: &Value) -> Result<u16, String> {
    if let Value::Object(o) = v {
        let inner = o.inner();
        let b = inner.borrow();
        if let Some(t) = b.as_any().downcast_ref::<TunnelValue>() {
            if t.kind != "forward" {
                return Err("mount: 'tunnel' must be a local tunnel (s.forward(...))".into());
            }
            return Ok(t.listen);
        }
    }
    Err("mount: 'tunnel' must be a tunnel from s.forward(...)".into())
}

impl TunnelValue {
    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl FoguObject for TunnelValue {
    fn name(&self) -> &str {
        "Tunnel"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "kind" => Some(Value::Str(if self.kind == "forward" { "local".into() } else { "remote".into() })),
            "port" | "localPort" if self.kind == "forward" => Some(Value::Int(self.listen as i64)),
            "port" | "remotePort" if self.kind == "reverse" => Some(Value::Int(self.listen as i64)),
            "target" => Some(Value::Str(format!("{}:{}", self.host, self.to).into())),
            "via" => Some(Value::Str(self.via.clone().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "alive" => Ok(Value::Bool(match &mut self.child {
                Some(c) => matches!(c.try_wait(), Ok(None)),
                None => false,
            })),
            "close" => {
                self.stop();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Tunnel", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["alive", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Drop for TunnelValue {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------
// persistent shell sessions
// ---------------------------------------------------------------------

/// Bytes read from one of the session's pipes.
#[derive(Default)]
struct Pipe {
    buf: Vec<u8>,
    eof: bool,
}

type SharedPipe = std::sync::Arc<std::sync::Mutex<Pipe>>;

fn reader(mut from: impl Read + Send + 'static, into: SharedPipe) {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            let n = from.read(&mut chunk).unwrap_or(0);
            let mut p = into.lock().unwrap_or_else(|e| e.into_inner());
            if n == 0 {
                p.eof = true;
                drop(p);
                crate::runtime::sched::wake();
                return;
            }
            p.buf.extend_from_slice(&chunk[..n]);
            drop(p);
            crate::runtime::sched::wake();
        }
    });
}

/// A remote shell kept open between commands (`s.shell()`): `run` keeps the
/// working directory and variables; with `pty`, `send`/`expect`/`read`
/// drive programs that want a terminal.
pub struct SshShell {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    out: SharedPipe,
    err: SharedPipe,
    pty: bool,
    seq: u64,
    host: String,
    /// A `run` is in progress (its markers are not in the output yet).
    busy: std::rc::Rc<std::cell::Cell<bool>>,
}

/// Waits for `ready` (cooperatively) until `deadline`; the pending poller
/// keeps a timer so it is not taken for a deadlock.
fn poll_until<T: 'static>(deadline: Option<Instant>, mut ready: impl FnMut(bool) -> Option<Result<T, String>> + 'static, finish: impl Fn(T) -> Result<Value, String> + 'static) -> Poller {
    Box::new(move || {
        let late = deadline.is_some_and(|d| Instant::now() >= d);
        match ready(late) {
            Some(r) => Some(r.and_then(&finish)),
            None => {
                let next = Instant::now() + Duration::from_millis(50);
                crate::runtime::sched::add_timer(deadline.map_or(next, |d| d.min(next)));
                None
            }
        }
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

impl SshShell {
    fn open(t: &Target, o: &HashMap<String, Value>) -> Result<SshShell, String> {
        for k in o.keys() {
            if !["pty", "shell", "cwd", "env"].contains(&k.as_str()) {
                return Err(format!("shell: unknown option '{}' (pty shell cwd env)", k));
            }
        }
        let pty = o.get("pty").is_some_and(Value::is_truthy);
        let mut c = t.command("ssh");
        c.arg(if pty { "-tt" } else { "-T" }).arg(t.dest(false));
        if !pty {
            // a plain shell reading commands from stdin
            c.arg(o.get("shell").map(|v| v.to_display_string()).unwrap_or_else(|| "sh".into()));
        } else if let Some(sh) = o.get("shell") {
            c.arg(sh.to_display_string());
        }
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| io_err!("ssh.shell: cannot start the ssh client: {}", e))?;
        let out = SharedPipe::default();
        let err = SharedPipe::default();
        reader(child.stdout.take().expect("piped"), out.clone());
        reader(child.stderr.take().expect("piped"), err.clone());
        let mut sh = SshShell {
            stdin: child.stdin.take(),
            child: Some(child),
            out,
            err,
            pty,
            seq: 0,
            host: t.dest(false),
            busy: Default::default(),
        };
        // the starting directory and variables
        let mut setup = String::new();
        if let Some(Value::Map(env)) = o.get("env") {
            for (k, v) in env.borrow().iter() {
                if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || k.is_empty() {
                    return Err(format!("shell: invalid variable name '{}'", k));
                }
                setup.push_str(&format!("export {}={}\n", k, sh_quote(&v.to_display_string())));
            }
        }
        if let Some(cwd) = o.get("cwd") {
            setup.push_str(&format!("cd {}\n", sh_quote(&cwd.to_display_string())));
        }
        if !setup.is_empty() {
            sh.write(&setup)?;
        }
        Ok(sh)
    }

    fn write(&mut self, text: &str) -> Result<(), String> {
        use std::io::Write;
        let ok = match &mut self.stdin {
            Some(w) => w.write_all(text.as_bytes()).and_then(|_| w.flush()).is_ok(),
            None => false,
        };
        if ok { Ok(()) } else { Err(io_err!("ssh.shell: the session is closed")) }
    }

    fn alive(&mut self) -> bool {
        match &mut self.child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn stop(&mut self) {
        self.stdin.take();
        if let Some(mut c) = self.child.take() {
            // `exit` first (stdin closed); then make sure it is gone
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline && matches!(c.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn timeout_opt(args: &[Value], i: usize, what: &str) -> Result<Option<Instant>, String> {
        let v = match opt_arg(args, i) {
            None | Some(Value::Nil) => return Ok(None),
            Some(Value::Map(m)) => m.borrow().get("timeout").cloned(),
            Some(v) => Some(v.clone()),
        };
        match v {
            None | Some(Value::Nil) => Ok(None),
            Some(v) => Ok(Some(Instant::now() + crate::native::util::duration_arg(&v, what)?)),
        }
    }
}

/// `'text'` for sh (single quotes, `'` escaped).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl FoguObject for SshShell {
    fn name(&self) -> &str {
        "SshShell"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "host" => Some(Value::Str(self.host.clone().into())),
            "pty" => Some(Value::Bool(self.pty)),
            _ => None,
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // run(cmd, { timeout }) → { ok, code, stdout, stderr }; the shell keeps cd/export
                "run" => {
                    max_args(args, 2, "run")?;
                    if self.pty {
                        return Err("run: not available on a pty session (use send/expect)".into());
                    }
                    if self.busy.get() {
                        return Err("run: another command is still running in this shell (use one shell per task)".into());
                    }
                    let cmd = str_arg(args, 0, "run")?;
                    let deadline = Self::timeout_opt(args, 1, "run")?;
                    self.seq += 1;
                    let mark = format!("__FOGU_{}_{}__", std::process::id(), self.seq);
                    // stdin of the command is /dev/null: it cannot eat the next lines
                    self.write(&format!(
                        "{{ {}\n}} </dev/null; __fogu_rc=$?; printf '\\n{m}%s\\n' \"$__fogu_rc\"; printf '\\n{m}\\n' >&2\n",
                        cmd,
                        m = mark
                    ))?;
                    self.busy.set(true);
                    let (out, err, busy) = (self.out.clone(), self.err.clone(), self.busy.clone());
                    let host = self.host.clone();
                    let mark_b = format!("\n{}", mark).into_bytes();
                    poll_until(
                        deadline,
                        move |late| {
                            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
                            let mut e = err.lock().unwrap_or_else(|e| e.into_inner());
                            let (oi, ei) = (find(&o.buf, &mark_b), find(&e.buf, &mark_b));
                            if let (Some(oi), Some(ei)) = (oi, ei) {
                                // "\n<mark><code>\n" on stdout, "\n<mark>\n" on stderr
                                let rest = &o.buf[oi + mark_b.len()..];
                                let Some(nl) = rest.iter().position(|b| *b == b'\n') else { return None };
                                let code: i64 = String::from_utf8_lossy(&rest[..nl]).trim().parse().unwrap_or(-1);
                                let stdout = String::from_utf8_lossy(&o.buf[..oi]).into_owned();
                                let stderr = String::from_utf8_lossy(&e.buf[..ei]).into_owned();
                                let o_end = oi + mark_b.len() + nl + 1;
                                let e_end = (ei + mark_b.len() + 1).min(e.buf.len());
                                o.buf.drain(..o_end);
                                e.buf.drain(..e_end);
                                busy.set(false);
                                return Some(Ok((code, stdout, stderr)));
                            }
                            if o.eof {
                                busy.set(false);
                                let msg = String::from_utf8_lossy(&e.buf).trim().to_string();
                                return Some(Err(io_err!("ssh.shell: {}: the session ended{}", host, if msg.is_empty() { String::new() } else { format!(": {}", msg) })));
                            }
                            if late {
                                // the command keeps running: this shell cannot be used for others
                                return Some(Err(io_err!("ssh.shell: timed out waiting for the command")));
                            }
                            None
                        },
                        |(code, stdout, stderr)| {
                            Ok(map(vec![
                                ("ok", Value::Bool(code == 0)),
                                ("code", Value::Int(code)),
                                ("stdout", Value::Str(stdout.into())),
                                ("stderr", Value::Str(stderr.into())),
                            ]))
                        },
                    )
                }
                // expect(text | regex, { timeout = 30s }) → the output up to the match (consumed)
                "expect" => {
                    max_args(args, 2, "expect")?;
                    let pat = str_arg(args, 0, "expect")?;
                    let re = regex::Regex::new(&pat).unwrap_or_else(|_| regex::Regex::new(&regex::escape(&pat)).expect("escaped"));
                    let deadline = Self::timeout_opt(args, 1, "expect")?.or_else(|| Some(Instant::now() + Duration::from_secs(30)));
                    let out = self.out.clone();
                    poll_until(
                        deadline,
                        move |late| {
                            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
                            let text = String::from_utf8_lossy(&o.buf).into_owned();
                            if let Some(m) = re.find(&text) {
                                let upto = text[..m.end()].to_string();
                                let n = upto.len().min(o.buf.len());
                                o.buf.drain(..n);
                                return Some(Ok(upto));
                            }
                            if o.eof {
                                return Some(Err(io_err!("ssh.shell: the session ended before '{}' (got: {})", re.as_str(), text.trim())));
                            }
                            if late {
                                return Some(Err(io_err!("ssh.shell: timed out waiting for '{}' (got: {})", re.as_str(), text.trim())));
                            }
                            None
                        },
                        |s: String| Ok(Value::Str(s.into())),
                    )
                }
                // read({ timeout = 200ms }) → what arrived (stdout and stderr) until it goes quiet
                "read" => {
                    max_args(args, 1, "read")?;
                    let quiet = match Self::timeout_opt(args, 0, "read")? {
                        Some(d) => d.saturating_duration_since(Instant::now()),
                        None => Duration::from_millis(200),
                    };
                    let (out, err) = (self.out.clone(), self.err.clone());
                    let mut last_len = usize::MAX;
                    let mut since = Instant::now();
                    Box::new(move || {
                        let len = out.lock().map(|o| o.buf.len()).unwrap_or(0) + err.lock().map(|e| e.buf.len()).unwrap_or(0);
                        if len != last_len {
                            last_len = len;
                            since = Instant::now();
                        }
                        let eof = out.lock().map(|o| o.eof).unwrap_or(true);
                        if eof || since.elapsed() >= quiet {
                            let mut text = String::new();
                            for p in [&out, &err] {
                                let mut p = p.lock().unwrap_or_else(|e| e.into_inner());
                                text.push_str(&String::from_utf8_lossy(&p.buf));
                                p.buf.clear();
                            }
                            return Some(Ok(Value::Str(text.into())));
                        }
                        crate::runtime::sched::add_timer(Instant::now() + Duration::from_millis(20));
                        None
                    })
                }
                _ => return Ok(None),
            }))
        })();
        match r {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // send(text): raw input (add "\n" to press enter)
            "send" => {
                max_args(args, 1, "send")?;
                let text = str_arg(args, 0, "send")?;
                self.write(&text)?;
                Ok(Value::Nil)
            }
            "alive" => Ok(Value::Bool(self.alive())),
            "close" => {
                self.stop();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on SshShell ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["run", "send", "expect", "read", "alive", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Drop for SshShell {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::parse_target;

    #[test]
    fn quotes() {
        assert_eq!(super::sh_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn targets() {
        assert_eq!(parse_target("h").unwrap(), (None, "h".into(), None));
        assert_eq!(parse_target("u@h:2222").unwrap(), (Some("u".into()), "h".into(), Some(2222)));
        assert_eq!(parse_target("u@[::1]:22").unwrap(), (Some("u".into()), "::1".into(), Some(22)));
        assert!(parse_target("u@h:abc").is_err());
    }
}
