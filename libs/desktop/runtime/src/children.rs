//! The processes the Fogu program started (`ssh -N` tunnels of `fogu:ssh`,
//! a command a task of the program still waits for…), so that a hot reload (`fogu desktop dev`) can close the
//! old program's ones: its runtime is dropped, but values kept alive by a
//! suspended task or a reference cycle are never freed, and a child process
//! does not end with the runtime that started it (the local port of a tunnel
//! would stay taken and the new program could not open it again).
//!
//! [`snapshot`] records the process ids that are already there when a program
//! starts; [`stop_new`] ends every process started since then by this process
//! (and what those started), except the web view's own processes
//! (WebView2's `msedgewebview2.exe`, WebKitGTK's `WebKit*Process`).

use std::collections::{HashMap, HashSet};

/// Image names (lowercase, a part of the name) of the web view's processes.
const WEBVIEW: &[&str] = &["msedgewebview2", "webkit"];

/// One process: (pid, parent pid, image name).
type Proc = (u32, u32, String);

/// The process ids below this process now.
pub fn snapshot() -> HashSet<u32> {
    descendants(&processes()).into_iter().map(|(pid, _)| pid).collect()
}

/// Ends the processes below this one that are not in `before` (and are not
/// the web view's); returns their names.
pub fn stop_new(before: &HashSet<u32>) -> Vec<String> {
    let procs = processes();
    let mut victims: Vec<(u32, String)> = descendants(&procs).into_iter().filter(|(pid, _)| !before.contains(pid)).collect();
    if victims.is_empty() {
        return Vec::new();
    }
    // the deepest first: a parent does not start a replacement
    victims.reverse();
    for (pid, _) in &victims {
        terminate(*pid);
    }
    #[cfg(unix)]
    {
        // SIGTERM first; what is still there after a moment gets SIGKILL
        std::thread::sleep(std::time::Duration::from_millis(300));
        let alive: HashSet<u32> = processes().into_iter().map(|p| p.0).collect();
        for (pid, _) in &victims {
            if alive.contains(pid) {
                // SAFETY: kill(2) on a process id
                unsafe { kill(*pid as i32, 9) };
            }
        }
    }
    victims.into_iter().map(|(_, name)| name).collect()
}

/// The processes below this one (children, their children…), outside the
/// web view's trees: (pid, name), parents before children.
fn descendants(procs: &[Proc]) -> Vec<(u32, String)> {
    let me = std::process::id();
    let mut kids: HashMap<u32, Vec<&Proc>> = HashMap::new();
    for p in procs {
        if p.0 != p.1 {
            kids.entry(p.1).or_default().push(p);
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = vec![me];
    while let Some(parent) = queue.pop() {
        for p in kids.get(&parent).map(|v| v.as_slice()).unwrap_or(&[]) {
            let lower = p.2.to_lowercase();
            if p.0 == me || WEBVIEW.iter().any(|w| lower.contains(w)) || !seen.insert(p.0) {
                continue;
            }
            out.push((p.0, p.2.clone()));
            queue.push(p.0);
        }
    }
    out
}

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
fn terminate(pid: u32) {
    // SAFETY: kill(2) with SIGTERM on a process id
    unsafe { kill(pid as i32, 15) };
}

/// Every process: /proc on Linux.
#[cfg(target_os = "linux")]
fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return out };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
        // "<pid> (<comm>) <state> <ppid> …": comm may hold spaces and parentheses
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else { continue };
        let name = stat[open + 1..close].to_string();
        let mut rest = stat[close + 1..].split_whitespace();
        let ppid = rest.nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        out.push((pid, ppid, name));
    }
    out
}

/// Every process: `ps` on the other Unix systems (macOS).
#[cfg(all(unix, not(target_os = "linux")))]
fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(child) = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,comm="])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return out;
    };
    let ps = child.id();
    let Ok(o) = child.wait_with_output() else { return out };
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        let mut f = line.split_whitespace();
        let (Some(pid), Some(ppid)) = (f.next().and_then(|s| s.parse::<u32>().ok()), f.next().and_then(|s| s.parse::<u32>().ok())) else {
            continue;
        };
        if pid == ps {
            continue;
        }
        let name = f.collect::<Vec<_>>().join(" ");
        let name = name.rsplit('/').next().unwrap_or("").to_string();
        out.push((pid, ppid, name));
    }
    out
}

/// Every process: a Toolhelp snapshot on Windows.
#[cfg(windows)]
fn processes() -> Vec<Proc> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    let mut out = Vec::new();
    // SAFETY: a process snapshot walked with a correctly sized entry, then closed
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            out.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(windows)]
fn terminate(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    // SAFETY: a handle opened for this pid, used once and closed
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn processes() -> Vec<Proc> {
    Vec::new()
}

#[cfg(not(any(unix, windows)))]
fn terminate(_pid: u32) {}
