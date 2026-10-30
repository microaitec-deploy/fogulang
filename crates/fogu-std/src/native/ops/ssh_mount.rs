//! `ssh.mount(...)` — mounts a remote folder as a local directory (or a
//! drive letter on Windows) like `sshfs`: Linux and macOS use the `sshfs`
//! program (FUSE / macFUSE), Windows uses SSHFS-Win (`net use`).
//!
//! ```fogu
//! const s = ssh.connect("deploy@example.com")?
//! const m = s.mount("/var/www", "./www")?          // Windows: s.mount("/var/www", "X:")
//! print(fs.list("./www")?)
//! m.unmount()?
//! ```
//!
//! Mounts are unmounted when the program ends, unless `{ keep: true }`.

use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::runtime::value::Value;

use crate::native::object::{FoguObject, Poller};
use crate::native::ssh::Target;

/// Active mounts: (local path or drive, windows?) — unmounted at exit.
static MOUNTS: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());

fn on_path(name: &str) -> Option<String> {
    if let Ok(p) = std::env::var("FOGU_SSHFS") {
        if !p.is_empty() && name == "sshfs" {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|d| [d.join(name), d.join(format!("{}.exe", name))])
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

pub(crate) struct MountOpts {
    pub readonly: bool,
    pub keep: bool,
    pub options: Vec<String>,
}

/// Mounts `remote` at `local`; blocking (run it on a worker thread).
pub(crate) fn mount(t: &Target, remote: &str, local: &str, o: &MountOpts) -> Result<String, String> {
    let windows = cfg!(windows);
    let local = &if windows { local.to_string() } else { absolute(local) };
    if windows {
        mount_windows(t, remote, local, o)?;
    } else {
        mount_unix(t, remote, local, o)?;
    }
    if !o.keep {
        if let Ok(mut m) = MOUNTS.lock() {
            m.push((local.to_string(), windows));
        }
        // Ctrl+C / SIGTERM unmount it too
        #[cfg(not(target_family = "wasm"))]
        crate::native::signal::arm_cleanup();
    }
    Ok(local.to_string())
}

/// sshfs changes to / when it goes to the background: relative paths must become absolute.
fn absolute(p: &str) -> String {
    let p = crate::native::ssh::expand_home(p);
    let path = std::path::Path::new(&p);
    if path.is_absolute() {
        return p;
    }
    std::env::current_dir().map(|d| d.join(path).to_string_lossy().to_string()).unwrap_or(p)
}

fn mount_unix(t: &Target, remote: &str, local: &str, o: &MountOpts) -> Result<(), String> {
    let sshfs = on_path("sshfs").ok_or_else(|| {
        io_err!("ssh.mount: the 'sshfs' program was not found (Linux: apt install sshfs; macOS: macFUSE + sshfs) — or set FOGU_SSHFS")
    })?;
    std::fs::create_dir_all(local).map_err(|e| io_err!("ssh.mount: cannot create {}: {}", local, e))?;
    let mut c = Command::new(&sshfs);
    // an IPv6 address needs brackets in `host:path`
    let host = if t.host.contains(':') { format!("[{}]", t.host) } else { t.host.clone() };
    let dest = match &t.user {
        Some(u) => format!("{}@{}:{}", u, host, remote),
        None => format!("{}:{}", host, remote),
    };
    c.arg(&dest).arg(local);
    if let Some(p) = t.port {
        c.arg("-p").arg(p.to_string());
    }
    let mut opts = vec![
        "reconnect".to_string(),
        "ServerAliveInterval=15".into(),
        "ServerAliveCountMax=3".into(),
        format!("ConnectTimeout={}", t.timeout),
        format!("StrictHostKeyChecking={}", t.strict),
    ];
    if let Some(k) = &t.key {
        opts.push(format!("IdentityFile={}", absolute(k)));
    }
    if let Some(kh) = &t.known_hosts {
        opts.push(format!("UserKnownHostsFile={}", absolute(kh)));
    }
    if t.password.is_some() {
        opts.push("password_stdin".into());
    } else {
        opts.push("BatchMode=yes".into());
    }
    if o.readonly {
        opts.push("ro".into());
    }
    for (k, v) in &t.options {
        opts.push(format!("{}={}", k, v));
    }
    opts.extend(o.options.iter().cloned());
    for x in opts {
        c.arg("-o").arg(x);
    }
    c.stdin(if t.password.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| io_err!("ssh.mount: cannot start sshfs: {}", e))?;
    if let (Some(pw), Some(mut si)) = (&t.password, child.stdin.take()) {
        use std::io::Write;
        let _ = writeln!(si, "{}", pw);
    }
    // sshfs goes to the background once the mount is ready
    let out = child.wait_with_output().map_err(|e| io_err!("ssh.mount: {}", e))?;
    if !out.status.success() {
        return Err(io_err!("ssh.mount: {}: {}", dest, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

fn mount_windows(t: &Target, remote: &str, local: &str, o: &MountOpts) -> Result<(), String> {
    // `net use` mounts on a drive letter
    let drive = local.trim_end_matches(['\\', '/']);
    if !(drive.len() == 2 && drive.as_bytes()[0].is_ascii_alphabetic() && drive.as_bytes()[1] == b':') {
        return Err(format!("ssh.mount: on Windows the local side is a drive letter like \"X:\", got '{}'", local));
    }
    // SSHFS-Win reads only %USERPROFILE%\.ssh\id_rsa: another key would be ignored
    if let Some(k) = &t.key {
        let default = std::path::Path::new(&std::env::var("USERPROFILE").unwrap_or_default()).join(".ssh").join("id_rsa");
        let given = std::path::Path::new(&crate::native::ssh::expand_home(k)).to_path_buf();
        if given.canonicalize().ok() != default.canonicalize().ok() {
            return Err(format!(
                "ssh.mount: SSHFS-Win only uses {} (key '{}' cannot be used); copy the key there or use a password",
                default.display(),
                k
            ));
        }
    }
    let local = drive;
    // SSHFS-Win UNC paths: \\sshfs[.r|.k|.kr]\user@host!port\path (r: from /, k: key auth)
    let absolute = remote.starts_with('/');
    let prefix = match (t.password.is_some() || t.key.is_none(), absolute) {
        (true, false) => "sshfs",
        (true, true) => "sshfs.r",
        (false, false) => "sshfs.k",
        (false, true) => "sshfs.kr",
    };
    let user = t.user.clone().unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_default());
    let port = t.port.map(|p| format!("!{}", p)).unwrap_or_default();
    let path = remote.trim_start_matches('/').replace('/', "\\");
    let unc = format!("\\\\{}\\{}@{}{}\\{}", prefix, user, t.host, port, path);
    let mut c = Command::new("net");
    c.arg("use").arg(local).arg(unc.trim_end_matches('\\'));
    if let Some(pw) = &t.password {
        c.arg(pw);
    }
    c.arg("/persistent:no");
    // SSHFS-Win has no read-only switch on `net use`: refuse rather than mount read-write
    if o.readonly {
        return Err("ssh.mount: { readonly: true } is not supported on Windows (SSHFS-Win)".into());
    }
    let out = c.stdin(Stdio::null()).output().map_err(|e| io_err!("ssh.mount: cannot run 'net use': {}", e))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(io_err!("ssh.mount: {} (is SSHFS-Win installed?): {}", unc, msg.trim()));
    }
    Ok(())
}

fn unmount_path(local: &str, windows: bool) -> Result<(), String> {
    let run = |prog: &str, args: &[&str]| Command::new(prog).args(args).stdin(Stdio::null()).output();
    if windows {
        let o = run("net", &["use", local, "/delete", "/y"]).map_err(|e| io_err!("unmount: {}", e))?;
        if !o.status.success() {
            return Err(io_err!("unmount: {}: {}", local, String::from_utf8_lossy(&o.stderr).trim()));
        }
        return Ok(());
    }
    let attempts: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("umount", &[]), ("diskutil", &["unmount"])]
    } else {
        &[("fusermount3", &["-u"]), ("fusermount", &["-u"]), ("umount", &[])]
    };
    let mut last = String::new();
    for (prog, pre) in attempts {
        let mut args: Vec<&str> = pre.to_vec();
        args.push(local);
        match run(prog, &args) {
            Ok(o) if o.status.success() => return Ok(()),
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
            Err(_) => continue,
        }
    }
    Err(io_err!("unmount: {}: {}", local, last))
}

fn forget(local: &str) {
    if let Ok(mut m) = MOUNTS.lock() {
        m.retain(|(l, _)| l != local);
    }
}

/// Unmounts what is still mounted (called when the program ends).
pub fn unmount_all() {
    let all: Vec<(String, bool)> = MOUNTS.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
    for (l, w) in all {
        let _ = unmount_path(&l, w);
    }
}

pub struct MountValue {
    pub(crate) local: String,
    pub(crate) remote: String,
    pub(crate) host: String,
    pub(crate) mounted: bool,
}

impl FoguObject for MountValue {
    fn name(&self) -> &str {
        "Mount"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "path" | "local" => Value::Str(self.local.clone().into()),
            "remote" => Value::Str(self.remote.clone().into()),
            "host" => Value::Str(self.host.clone().into()),
            "mounted" => Value::Bool(self.mounted),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on Mount (unmount)", method))
    }

    /// `unmount()` runs `fusermount -u` / `net use /delete` without holding the object.
    fn start_async(&mut self, method: &str, _args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "unmount" {
            return None;
        }
        if !self.mounted {
            let mut done = Some(Ok(Value::Nil));
            return Some(Ok(Box::new(move || done.take())));
        }
        // marked at once: a second unmount() meanwhile is a no-op
        self.mounted = false;
        let (l, w) = (self.local.clone(), cfg!(windows));
        Some(Ok(crate::native::util::thread_poller(
            move || {
                let r = unmount_path(&l, w);
                if r.is_ok() {
                    forget(&l);
                }
                r
            },
            |_| Ok(Value::Nil),
        )))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["unmount"]
    }
}
