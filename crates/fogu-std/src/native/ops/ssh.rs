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

use crate::runtime::value::Str;
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
        let q = proxy_arg;
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
        c.push_str(&format!(" -o {} -o BatchMode=yes -o LogLevel=ERROR", q(&ssh_option("StrictHostKeyChecking", &self.strict))));
        if let Some(kh) = &self.known_hosts {
            c.push_str(&format!(" -o {}", q(&ssh_option("UserKnownHostsFile", &expand_home(kh)))));
        }
        for (k, v) in &self.options {
            if k != "ProxyCommand" {
                c.push_str(&format!(" -o {}", q(&ssh_option(k, v))));
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
        self.command_with(program, "ERROR")
    }

    /// The ssh/scp client with this target's options; `log` is its `LogLevel`
    /// (ssh keeps the first value given, so it cannot be changed afterwards).
    fn command_with(&self, program: &str, log: &str) -> Command {
        // FOGU_SSH: the ssh client to use (another build, a wrapper, tests)
        let exe = match std::env::var("FOGU_SSH") {
            Ok(p) if program == "ssh" && !p.is_empty() => p,
            _ => program.to_string(),
        };
        let mut c = Command::new(exe);
        // a GUI program (the desktop host) on Windows: no console window per ssh call
        crate::native::process::hide_window(&mut c, false);
        let scp = program == "scp";
        let opt = |c: &mut Command, k: &str, v: &str| {
            c.arg("-o").arg(ssh_option(k, v));
        };
        if let Some(p) = self.port {
            c.arg(if scp { "-P" } else { "-p" }).arg(p.to_string());
        }
        if let Some(k) = &self.key {
            c.arg("-i").arg(expand_home(k));
        }
        opt(&mut c, "StrictHostKeyChecking", &self.strict);
        opt(&mut c, "ConnectTimeout", &self.timeout.to_string());
        opt(&mut c, "LogLevel", log);
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

/// `Key=value` for ssh's `-o`. ssh's config parser splits values at spaces
/// (`UserKnownHostsFile=/Users/me/Library/Application Support/kh` is two files,
/// the first `~/Library/Application`), so a value with spaces, quotes or `#` is
/// double-quoted, `"` escaped. A trailing `\` is doubled so it does not escape
/// the closing quote (a Windows folder). The commands (`ProxyCommand`…) take
/// the rest of the line as it is: never quoted.
fn ssh_option(k: &str, v: &str) -> String {
    const VERBATIM: &[&str] = &["proxycommand", "localcommand", "remotecommand", "knownhostscommand"];
    let special = |c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '#';
    if VERBATIM.contains(&k.to_ascii_lowercase().as_str()) || !v.contains(special) {
        return format!("{}={}", k, v);
    }
    let mut q = String::with_capacity(k.len() + v.len() + 4);
    q.push_str(k);
    q.push_str("=\"");
    for c in v.chars() {
        if c == '"' {
            q.push('\\');
        }
        q.push(c);
    }
    if v.ends_with('\\') {
        q.push('\\');
    }
    q.push('"');
    q
}

/// One argument of the `ProxyCommand` line, which ssh runs through the shell
/// (`sh -c` on Unix; on Windows it is split like a command line).
fn proxy_arg(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c) || (cfg!(windows) && c == '\\'));
    if plain {
        s.to_string()
    } else if cfg!(windows) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        sh_quote(s)
    }
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
    let opts: Option<std::cell::Ref<HashMap<Str, Value>>>;
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
                o.borrow().iter().map(|(k, v)| (k.to_string(), v.to_display_string())).collect();
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

    /// Starts `ssh -N -L/-R …`; the poller yields the `Tunnel` once it is ready
    /// (a forward's local port accepts connections, the server accepted a reverse),
    /// or an error value with ssh's own message when it exits first (a busy port,
    /// a refused forward). The interpreter keeps running other tasks meanwhile.
    fn tunnel(&self, kind: &'static str, args: &[Value]) -> Result<Poller, String> {
        if args.len() < 3 || args.len() > 4 {
            return Err(format!("{}(port, host, hostPort, {{ bind }}) expects 3 or 4 arguments", kind));
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
        let bind = tunnel_opts(kind, args.get(3))?;
        let forward = kind == "forward";
        let (check, connect) = local_addrs(bind.as_deref(), listen);
        if forward {
            if listen == 0 {
                // pick a free local port
                listen = TcpListener::bind(check)
                    .and_then(|l| l.local_addr())
                    .map(|a| a.port())
                    .map_err(|e| io_err!("ssh.forward: no free local port: {}", e))?;
            } else if let Err(e) = TcpListener::bind(check) {
                // another program has it: connecting to it would look like a working tunnel
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    return Err(io_err!("ssh.forward: local port {} ({}) is already in use", listen, check.ip()));
                }
            }
        }
        let connect = std::net::SocketAddr::new(connect.ip(), listen);
        let spec = forward_spec(bind.as_deref(), listen, &host, to);
        let via = self.target.dest(false);
        // a reverse forward is confirmed only in ssh's debug messages ("remote forward
        // success", "Allocated port N" for port 0); they are read from its stderr
        let mut c = self.target.command_with("ssh", if forward { "ERROR" } else { "DEBUG1" });
        c.arg("-N")
            .arg("-o")
            .arg("ExitOnForwardFailure=yes")
            .arg("-o")
            .arg("ServerAliveInterval=30")
            .arg(if forward { "-L" } else { "-R" })
            .arg(&spec)
            .arg(&via);
        // stdout never inherited: `fogu x.fg | head` must not wait for the tunnel
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        let (mut child, stdin) = spawn_owned(c).map_err(|e| io_err!("ssh.{}: cannot start ssh: {}", kind, e))?;
        let log = SharedPipe::default();
        if let Some(e) = child.stderr.take() {
            log_reader(e, log.clone());
        }
        let mut pending = Some(TunnelValue { child: Some(child), stdin, log, kind, listen, bind, host, to, via: via.clone() });
        let deadline = Instant::now() + Duration::from_secs(self.target.timeout + 5);
        let mut since: Option<Instant> = None;
        Ok(poll_until(
            Some(deadline),
            move |late| {
                let t = pending.as_mut()?;
                let (text, eof) = t.log_text();
                let exited = t.child.as_mut().is_none_or(|c| matches!(c.try_wait(), Ok(Some(_)) | Err(_)));
                // its last words arrive when the pipe closes
                if exited && (eof || late) {
                    let mut t = pending.take()?;
                    let code = t.child.as_mut().and_then(|c| c.try_wait().ok().flatten()).and_then(|s| s.code());
                    t.stop();
                    let msg = ssh_errors(&text);
                    let msg = if msg.is_empty() { format!("ssh exited (code {})", code.map_or("?".into(), |c| c.to_string())) } else { msg };
                    return Some(Err(io_err!("ssh.{}: {} via {}: {}", kind, spec, via, msg)));
                }
                let ready = if exited {
                    false
                } else if forward {
                    TcpStream::connect_timeout(&connect, Duration::from_millis(100)).is_ok()
                } else if let Some(p) = allocated_port(&text) {
                    t.listen = p;
                    true
                } else {
                    // a fixed port: the success line; port 0 without the "Allocated" line
                    // (an old server) or a client that does not log it: shortly after
                    // the session started
                    let started = text.contains("remote forward success") || text.contains("Entering interactive session");
                    if started && since.is_none() {
                        since = Some(Instant::now());
                    }
                    let wait = if t.listen == 0 || !text.contains("remote forward success") { 1500 } else { 0 };
                    since.is_some_and(|s| s.elapsed() >= Duration::from_millis(wait))
                };
                if ready {
                    return pending.take().map(Ok);
                }
                if late {
                    let mut t = pending.take()?;
                    t.stop();
                    let msg = ssh_errors(&text);
                    return Some(Err(io_err!(
                        "ssh.{}: {}: timed out waiting for the tunnel{}",
                        kind,
                        spec,
                        if msg.is_empty() { String::new() } else { format!(" ({})", msg) }
                    )));
                }
                None
            },
            |t: TunnelValue| Ok(NativeObjectRef::new(t).to_value()),
        ))
    }
}

/// `{ bind }` of `forward` / `reverse`: the address the tunnel listens on (local for
/// `forward`, on the server for `reverse`).
fn tunnel_opts(kind: &str, v: Option<&Value>) -> Result<Option<String>, String> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Map(m)) => {
            let m = m.borrow();
            let mut keys: Vec<&str> = m.keys().map(|k| &**k).collect();
            keys.sort_unstable();
            if let Some(k) = keys.into_iter().find(|k| *k != "bind") {
                return Err(format!("{}: unknown option '{}' (bind)", kind, k));
            }
            match m.get("bind") {
                None | Some(Value::Nil) => Ok(None),
                Some(Value::Str(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
                Some(other) => Err(format!("{}: 'bind' must be an address, got '{}'", kind, other.to_display_string())),
            }
        }
        Some(other) => Err(format!("{}: options must be a map, got '{}'", kind, other.type_name())),
    }
}

/// `[bind:]port:host:hostPort` for `-L` / `-R` (IPv6 addresses in brackets).
fn forward_spec(bind: Option<&str>, port: u16, host: &str, to: u16) -> String {
    let b = |h: &str| if h.contains(':') && !h.starts_with('[') { format!("[{}]", h) } else { h.to_string() };
    match bind {
        Some(a) => format!("{}:{}:{}:{}", b(a), port, b(host), to),
        None => format!("{}:{}:{}", port, b(host), to),
    }
}

/// Where a `forward` listens: the address to check it is free (ssh's default
/// `localhost` → 127.0.0.1, `*` → every address) and the one to connect to.
fn local_addrs(bind: Option<&str>, port: u16) -> (std::net::SocketAddr, std::net::SocketAddr) {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
    let check = match bind.map(|b| b.trim_start_matches('[').trim_end_matches(']')) {
        None | Some("localhost") => IpAddr::V4(Ipv4Addr::LOCALHOST),
        Some("*") | Some("") => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        Some(b) => match b.parse::<IpAddr>() {
            Ok(ip) => ip,
            Err(_) => (b, port).to_socket_addrs().ok().and_then(|mut a| a.next()).map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |a| a.ip()),
        },
    };
    let connect = match check {
        IpAddr::V4(a) if a.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(a) if a.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    ((check, port).into(), (connect, port).into())
}

/// The port of "Allocated port 43210 for remote forward to localhost:3000" (`-R 0:…`).
fn allocated_port(log: &str) -> Option<u16> {
    let i = log.find("Allocated port ")?;
    log[i + "Allocated port ".len()..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok().filter(|p| *p > 0)
}

/// ssh's messages for an error value: without its debug and progress lines.
fn ssh_errors(log: &str) -> String {
    log.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with("debug")
                && !l.starts_with("Authenticated to")
                && !l.starts_with("Allocated port")
                && !l.starts_with("Transferred:")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Reads a tunnel's stderr for its whole life (a full pipe would block ssh),
/// keeping the last 64 KiB.
fn log_reader(mut from: impl Read + Send + 'static, into: SharedPipe) {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            let n = from.read(&mut chunk).unwrap_or(0);
            let mut p = into.lock().unwrap_or_else(|e| e.into_inner());
            if n == 0 {
                p.eof = true;
                return;
            }
            p.buf.extend_from_slice(&chunk[..n]);
            if p.buf.len() > 64 * 1024 {
                let cut = p.buf.len() - 32 * 1024;
                p.buf.drain(..cut);
            }
        }
    });
}

/// macOS and the BSDs have no parent-death signal: ssh runs under this sh, whose
/// stdin is a pipe only `fogu` holds. When `fogu` ends — however it ends — the
/// pipe closes, `read` returns and ssh is stopped; a SIGTERM to the watcher stops
/// it too. Its exit code is ssh's.
#[cfg(all(unix, not(target_os = "linux")))]
const WATCHER: &str = "trap 'kill $p 2>/dev/null; exit 143' TERM INT HUP; exec 3<&0; \"$@\" </dev/null & p=$!; \
    { read _ <&3; kill $p 2>/dev/null; } & w=$!; wait $p; c=$?; kill $w 2>/dev/null; exit $c";

/// Starts a long-lived ssh (a tunnel) that ends with this program however the
/// program ends (`process.exit`, an uncaught error, a closed window: `Drop`
/// does not run then). Linux: `PR_SET_PDEATHSIG` (sent when the thread that
/// started it ends: the interpreter's); macOS/BSD: the [`WATCHER`] (its stdin
/// pipe is returned: dropping it stops ssh); Windows: a kill-on-close job.
fn spawn_owned(c: Command) -> std::io::Result<(Child, Option<std::process::ChildStdin>)> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe extern "C" {
            fn prctl(option: std::ffi::c_int, arg2: std::ffi::c_ulong, arg3: std::ffi::c_ulong, arg4: std::ffi::c_ulong, arg5: std::ffi::c_ulong) -> std::ffi::c_int;
            fn getppid() -> std::ffi::c_int;
        }
        const PR_SET_PDEATHSIG: std::ffi::c_int = 1;
        const SIGTERM: std::ffi::c_ulong = 15;
        let mut c = c;
        let parent = std::process::id() as std::ffi::c_int;
        // SAFETY: between fork and exec only async-signal-safe system calls, no allocation
        unsafe {
            c.pre_exec(move || {
                prctl(PR_SET_PDEATHSIG, SIGTERM, 0, 0, 0);
                // the parent already gone (before prctl): do not start at all
                if getppid() != parent {
                    return Err(std::io::Error::from_raw_os_error(3));
                }
                Ok(())
            });
        }
        c.spawn().map(|child| (child, None))
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let mut w = Command::new("/bin/sh");
        w.arg("-c").arg(WATCHER).arg("fogu-ssh").arg(c.get_program()).args(c.get_args());
        for (k, v) in c.get_envs() {
            match v {
                Some(v) => w.env(k, v),
                None => w.env_remove(k),
            };
        }
        if let Some(d) = c.get_current_dir() {
            w.current_dir(d);
        }
        w.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = w.spawn()?;
        let stdin = child.stdin.take();
        Ok((child, stdin))
    }
    #[cfg(windows)]
    {
        let mut c = c;
        let child = c.spawn()?;
        job::adopt(&child);
        Ok((child, None))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let mut c = c;
        c.spawn().map(|child| (child, None))
    }
}

/// One job object for the tunnels of this process, with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: Windows closes its handle when the
/// process ends, and every process in it is ended.
#[cfg(windows)]
mod job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct BasicLimits {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    /// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`
    #[repr(C)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: [u64; 6],
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(job: *mut c_void, class: i32, info: *mut c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    }

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;

    /// The job's handle (as an address: a raw pointer is not `Sync`); 0 when it could not be made.
    fn handle() -> usize {
        static JOB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        *JOB.get_or_init(|| {
            // SAFETY: plain kernel32 calls; the structure is zeroed and has the C layout
            unsafe {
                let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if job.is_null() {
                    return 0;
                }
                let mut info: ExtendedLimits = std::mem::zeroed();
                info.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    &mut info as *mut ExtendedLimits as *mut c_void,
                    std::mem::size_of::<ExtendedLimits>() as u32,
                );
                // the handle is never closed: it closes when the process ends
                if ok == 0 { 0 } else { job as usize }
            }
        })
    }

    /// Puts `child` in the job (best effort: without it the tunnel only ends on `close`/drop).
    pub(super) fn adopt(child: &std::process::Child) {
        let job = handle();
        if job != 0 {
            // SAFETY: the job handle lives for the whole process; the child's handle is valid
            unsafe {
                AssignProcessToJobObject(job as *mut c_void, child.as_raw_handle() as *mut c_void);
            }
        }
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
                // local → remote: connections to [bind:]port here go to host:hostPort on the server
                "forward" => self.tunnel("forward", args)?,
                // remote → local: connections to the server's [bind:]port come to host:hostPort here
                "reverse" => self.tunnel("reverse", args)?,
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
            "close" => Ok(Value::Nil),
            other => Err(format!("method '{}' does not exist on Ssh ({})", other, self.methods().join(" "))),
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

/// A running `ssh -L` / `ssh -R` process; closed on `close()`, when dropped and
/// when the program ends (see [`spawn_owned`]).
pub struct TunnelValue {
    child: Option<Child>,
    /// macOS/BSD: the watcher's stdin (see [`WATCHER`]); dropping it stops ssh.
    stdin: Option<std::process::ChildStdin>,
    /// ssh's stderr.
    log: SharedPipe,
    kind: &'static str,
    /// The listening port (for `reverse(0, …)`, the one the server allocated).
    listen: u16,
    bind: Option<String>,
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
        let watched = self.stdin.take().is_some();
        if let Some(mut c) = self.child.take() {
            if watched {
                // the watcher stops ssh when its stdin closes; killing it first would leave ssh running
                let deadline = Instant::now() + Duration::from_millis(1000);
                while Instant::now() < deadline && matches!(c.try_wait(), Ok(None)) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// What ssh wrote to stderr so far, and whether it closed it.
    fn log_text(&self) -> (String, bool) {
        let p = self.log.lock().unwrap_or_else(|e| e.into_inner());
        (String::from_utf8_lossy(&p.buf).into_owned(), p.eof)
    }

    fn pid(&self) -> Value {
        self.child.as_ref().map_or(Value::Nil, |c| Value::Int(c.id() as i64))
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
            "bind" => Some(self.bind.clone().map(Value::str).unwrap_or(Value::Nil)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "alive" => Ok(Value::Bool(match &mut self.child {
                Some(c) => matches!(c.try_wait(), Ok(None)),
                None => false,
            })),
            // the ssh process (macOS/BSD: its watcher; ending either ends the tunnel); nil once closed
            "pid" => Ok(self.pid()),
            "close" => {
                self.stop();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Tunnel ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["alive", "pid", "close"]
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
    fn open(t: &Target, o: &HashMap<Str, Value>) -> Result<SshShell, String> {
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
    fn option_quoting() {
        use super::ssh_option;
        assert_eq!(ssh_option("LogLevel", "ERROR"), "LogLevel=ERROR");
        assert_eq!(
            ssh_option("UserKnownHostsFile", "/Users/me/Library/Application Support/app/kh"),
            "UserKnownHostsFile=\"/Users/me/Library/Application Support/app/kh\""
        );
        assert_eq!(ssh_option("X", "a \"b\""), "X=\"a \\\"b\\\"\"");
        assert_eq!(ssh_option("X", "C:\\My Dir\\"), "X=\"C:\\My Dir\\\\\"");
        assert_eq!(ssh_option("ProxyCommand", "ssh -W %h:%p bastion"), "ProxyCommand=ssh -W %h:%p bastion");
        if !cfg!(windows) {
            assert_eq!(super::proxy_arg("UserKnownHostsFile=\"/a b\""), "'UserKnownHostsFile=\"/a b\"'");
            assert_eq!(super::proxy_arg("LogLevel=ERROR"), "LogLevel=ERROR");
        }
    }

    #[test]
    fn tunnel_specs() {
        use super::{allocated_port, forward_spec, local_addrs, ssh_errors};
        assert_eq!(forward_spec(None, 5433, "localhost", 5432), "5433:localhost:5432");
        assert_eq!(forward_spec(Some("127.0.0.1"), 0, "::1", 80), "127.0.0.1:0:[::1]:80");
        assert_eq!(forward_spec(Some("::"), 8080, "db", 5432), "[::]:8080:db:5432");
        assert_eq!(allocated_port("debug1: x\nAllocated port 43210 for remote forward to localhost:3000\n"), Some(43210));
        assert_eq!(allocated_port("Allocated port 0 for"), None);
        assert_eq!(allocated_port("remote forward success"), None);
        assert_eq!(
            ssh_errors("debug1: Connecting\nbind [127.0.0.1]:5433: Address already in use\r\nCould not request local forwarding.\n"),
            "bind [127.0.0.1]:5433: Address already in use; Could not request local forwarding."
        );
        let (check, connect) = local_addrs(Some("0.0.0.0"), 80);
        assert_eq!((check.to_string(), connect.to_string()), ("0.0.0.0:80".into(), "127.0.0.1:80".into()));
        assert_eq!(local_addrs(None, 1).0.to_string(), "127.0.0.1:1");
        assert_eq!(local_addrs(Some("[::1]"), 2).1.to_string(), "[::1]:2");
    }

    #[test]
    fn targets() {
        assert_eq!(parse_target("h").unwrap(), (None, "h".into(), None));
        assert_eq!(parse_target("u@h:2222").unwrap(), (Some("u".into()), "h".into(), Some(2222)));
        assert_eq!(parse_target("u@[::1]:22").unwrap(), (Some("u".into()), "::1".into(), Some(22)));
        assert!(parse_target("u@h:abc").is_err());
    }
}
