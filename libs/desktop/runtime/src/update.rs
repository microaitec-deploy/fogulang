//! Self-update, the host's part. The program (`desktop.update` in main.fg)
//! fetches the update manifest, downloads the new executable and checks its
//! SHA-256 and Ed25519 signature; then it asks the host to `update.apply`:
//! the running executable is renamed to `<exe>.old` (allowed on Windows while
//! it runs), the new one takes its place, and the app restarts. The `.old`
//! file is removed at the next start.
//!
//! A single-file app (`fogu desktop build`) carries everything in its
//! executable, so replacing it updates the program and the UI too. An app
//! installed in a folder the user cannot write (Program Files) cannot update
//! itself this way: the error says so.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::AppHandle;

fn old_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".old");
    exe.with_file_name(name)
}

/// At startup: removes the executable an update replaced. The old process
/// may still be exiting (its file is in use on Windows), so this retries for
/// a few seconds on a thread of its own.
pub fn cleanup() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = old_path(&exe);
    if !old.exists() {
        return;
    }
    let _ = std::thread::Builder::new().name("fogu-update-cleanup".into()).spawn(move || {
        for _ in 0..40 {
            if std::fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
}

/// `{ path, restart }`: puts the downloaded executable `path` in place of this one.
pub fn apply(app: &AppHandle, a: &Value) -> Result<Value, String> {
    let new = a.get("path").and_then(Value::as_str).map(PathBuf::from).ok_or("desktop.update: no file")?;
    if !new.is_file() {
        return Err(format!("desktop.update: {} does not exist", new.display()));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| {
        format!("desktop.update: cannot replace {} ({}); an app installed where the user cannot write needs its installer", exe.display(), e)
    })?;
    if let Err(e) = std::fs::copy(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("desktop.update: cannot write {}: {}", exe.display(), e));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(&new);
    if a.get("restart").and_then(Value::as_bool).unwrap_or(true) {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::process::Command::new(&exe)
            .args(&args)
            .spawn()
            .map_err(|e| format!("desktop.update: installed, but the restart failed: {}", e))?;
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            app.exit(0);
        });
    }
    Ok(Value::Bool(true))
}
