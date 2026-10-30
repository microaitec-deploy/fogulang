//! `fs.remote(ssh)` — the `fogu:fs` operations on a server, through a
//! `fogu:ssh` connection (POSIX shell tools on the remote side).
//!
//! ```fogu
//! import ssh from "fogu:ssh"
//! import fs from "fogu:fs"
//!
//! const rfs = fs.remote(ssh.connect("deploy@example.com")?)     // { sudo: true } for root-owned paths
//! rfs.mkdir("/srv/app/config")?
//! rfs.writeText("/srv/app/config/app.env", "PORT=8080\n")?
//! print(rfs.readText("/srv/app/config/app.env")?)
//! print(rfs.list("/srv/app")?, rfs.stat("/srv/app/config/app.env")?)
//! rfs.upload("dist/app", "/srv/app/app")?
//! ```
//!
//! Every call runs on a worker thread; failures are `error` values.

use crate::runtime::value::Value;

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::ssh::Target;
use crate::native::util::{list, map, max_args, opt_arg, str_arg, thread_poller};

pub fn remote(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "fs.remote")?;
    let target = crate::native::ssh::target_of(&a[0]).map_err(|e| format!("fs.remote: {}", e))?;
    let sudo = matches!(opt_arg(a, 1), Some(Value::Map(m)) if m.borrow().get("sudo").is_some_and(Value::is_truthy));
    Ok(NativeObjectRef::new(RemoteFs { target, sudo }).to_value())
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub struct RemoteFs {
    target: Target,
    sudo: bool,
}

/// What a command's output becomes.
enum Want {
    Text,
    Lines,
    Nil,
    /// exit code 0 → true, 1 → false
    Test,
    List,
    Stat,
}

impl FoguObject for RemoteFs {
    fn name(&self) -> &str {
        "RemoteFs"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "host" => Some(Value::Str(self.target.display().into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on RemoteFs ({})", method, self.methods().join(" ")))
    }

    fn start_async(&mut self, method: &str, a: &[Value]) -> Option<Result<Poller, String>> {
        if !self.methods().contains(&method) {
            return None;
        }
        let t = self.target.clone();
        let sudo = if self.sudo { "sudo -n " } else { "" };
        let what = format!("fs.{}", method);
        let r = (|| -> Result<Poller, String> {
            let p = |i: usize| str_arg(a, i, &what);
            // upload/download use scp
            if method == "upload" || method == "download" {
                max_args(a, 2, method)?;
                let (x, y) = (p(0)?, p(1)?);
                let up = method == "upload";
                return Ok(thread_poller(
                    move || {
                        if up {
                            crate::native::ssh::upload_file(&t, &x, &y)
                        } else {
                            download(&t, &x, &y)
                        }
                    },
                    |_| Ok(Value::Nil),
                ));
            }
            let mut input: Option<Vec<u8>> = None;
            let (cmd, want) = match method {
                "readText" => (format!("{}cat -- {}", sudo, q(&p(0)?)), Want::Text),
                "readLines" => (format!("{}cat -- {}", sudo, q(&p(0)?)), Want::Lines),
                "writeText" | "append" => {
                    max_args(a, 2, method)?;
                    input = Some(p(1)?.into_bytes());
                    let op = if method == "append" { "-a " } else { "" };
                    (format!("{}tee {}-- {} > /dev/null", sudo, op, q(&p(0)?)), Want::Nil)
                }
                "exists" => (format!("test -e {}", q(&p(0)?)), Want::Test),
                "isFile" => (format!("test -f {}", q(&p(0)?)), Want::Test),
                "isDir" => (format!("test -d {}", q(&p(0)?)), Want::Test),
                "mkdir" => (format!("{}mkdir -p -- {}", sudo, q(&p(0)?)), Want::Nil),
                "remove" => {
                    let x = q(&p(0)?);
                    (format!("if [ -d {x} ]; then {s}rmdir -- {x}; else {s}rm -- {x}; fi", x = x, s = sudo), Want::Nil)
                }
                "removeAll" => (format!("{}rm -rf -- {}", sudo, q(&p(0)?)), Want::Nil),
                "copy" => (format!("{}cp -r -- {} {}", sudo, q(&p(0)?), q(&p(1)?)), Want::Nil),
                "move" => (format!("{}mv -- {} {}", sudo, q(&p(0)?), q(&p(1)?)), Want::Nil),
                "list" => (format!("{}ls -1A -- {}", sudo, q(&p(0)?)), Want::List),
                "stat" => {
                    let x = q(&p(0)?);
                    (format!("{}stat -c '%s|%F|%Y|%a' -- {x} && (test -w {x} && echo rw || echo ro)", sudo, x = x), Want::Stat)
                }
                // glob(pattern): expanded by the remote shell (* ? [..] only)
                "glob" => {
                    let pat = p(0)?;
                    if pat.chars().any(|c| ";|&$`<>(){}\\\"'\n".contains(c)) {
                        return Err(format!("fs.glob: the pattern '{}' has characters not allowed on a remote host", pat));
                    }
                    (format!("for f in {}; do [ -e \"$f\" ] && echo \"$f\"; done; true", pat.replace(' ', "\\ ")), Want::List)
                }
                "cwd" => ("pwd".into(), Want::Text),
                "tempDir" => ("echo \"${TMPDIR:-/tmp}\"".into(), Want::Text),
                _ => unreachable!(),
            };
            let label = format!("{}: {}", what, a.first().map(|v| v.to_display_string()).unwrap_or_default());
            Ok(thread_poller(
                move || {
                    let (code, out, err) = t.run_bytes(&cmd, input.as_deref()).map_err(|e| io_err!("{}: {}", label, e))?;
                    if code == 255 {
                        return Err(io_err!("{}: ssh {}: {}", label, t.display(), String::from_utf8_lossy(&err).trim()));
                    }
                    Ok((code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned(), label))
                },
                move |(code, out, err, label)| {
                    if let Want::Test = want {
                        return Ok(Value::Bool(code == 0));
                    }
                    if code != 0 {
                        // "cat: /x: No such file or directory" → "No such file or directory"
                        let msg = err.trim().lines().last().unwrap_or("").rsplit(": ").next().unwrap_or("").to_string();
                        return Err(io_err!("{}: {}", label, if msg.is_empty() { format!("exit code {}", code) } else { msg }));
                    }
                    Ok(match want {
                        Want::Text => Value::Str(if matches!(method_kind(&label), "cwd" | "tempDir") { out.trim().to_string().into() } else { out.into() }),
                        Want::Lines => list(out.lines().map(|l| Value::Str(l.to_string().into())).collect()),
                        Want::Nil | Want::Test => Value::Nil,
                        Want::List => {
                            let mut names: Vec<String> = out.lines().filter(|l| !l.is_empty()).map(String::from).collect();
                            names.sort();
                            list(names.into_iter().map(Value::str).collect())
                        }
                        Want::Stat => {
                            let mut lines = out.lines();
                            let f: Vec<&str> = lines.next().unwrap_or("").split('|').collect();
                            let ro = lines.next() == Some("ro");
                            let kind = f.get(1).copied().unwrap_or("");
                            map(vec![
                                ("size", Value::Int(f.first().and_then(|s| s.parse().ok()).unwrap_or(0))),
                                ("isFile", Value::Bool(kind.contains("regular"))),
                                ("isDir", Value::Bool(kind == "directory")),
                                ("readonly", Value::Bool(ro)),
                                ("modified", Value::Int(f.get(2).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) * 1000)),
                                ("mode", Value::Str(f.get(3).copied().unwrap_or("").to_string().into())),
                            ])
                        }
                    })
                },
            ))
        })();
        Some(r)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "readText", "readLines", "writeText", "append", "exists", "isFile", "isDir", "mkdir", "remove", "removeAll", "copy",
            "move", "list", "stat", "glob", "cwd", "tempDir", "upload", "download",
        ]
    }
}

fn method_kind(label: &str) -> &str {
    label.trim_start_matches("fs.").split(':').next().unwrap_or("")
}

fn download(t: &Target, remote: &str, local: &str) -> Result<(), String> {
    let (code, out, err) = t.run_bytes(&format!("cat -- {}", q(remote)), None).map_err(|e| io_err!("fs.download: {}", e))?;
    if code != 0 {
        return Err(io_err!("fs.download: {}: {}", remote, String::from_utf8_lossy(&err).trim().rsplit(": ").next().unwrap_or("")));
    }
    std::fs::write(local, out).map_err(|e| io_err!("fs.download: {}: {}", local, e))
}
