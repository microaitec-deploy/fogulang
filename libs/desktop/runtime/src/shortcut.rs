//! Global shortcuts (`desktop.shortcut(accel, fn)` in Fogu): system-wide key
//! combinations, also while the app is in the background. An accelerator is
//! "CmdOrCtrl+Shift+K", "Alt+Space", "F12"…
//!
//! A press goes to the program as `desktop:shortcut-event` and to the pages as
//! `desktop:shortcut`, both `{ shortcut, state: "pressed" | "released" }`.

use std::sync::Arc;

use serde_json::json;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::bridge::Bridge;

pub fn register(app: &AppHandle, accel: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    if gs.is_registered(accel) {
        return Ok(());
    }
    let name = accel.to_string();
    gs.on_shortcut(accel, move |app, _sc, ev| {
        let state = match ev.state {
            ShortcutState::Pressed => "pressed",
            ShortcutState::Released => "released",
        };
        let Some(bridge) = app.try_state::<Arc<Bridge>>().map(|b| Arc::clone(b.inner())) else { return };
        let data = json!({ "shortcut": name, "state": state });
        bridge.notify("desktop:shortcut-event", data.clone());
        bridge.send_event(app, "desktop:shortcut".to_string(), data, None);
    })
    .map_err(|e| format!("desktop.shortcut '{}': {}", accel, e))
}

pub fn unregister(app: &AppHandle, accel: Option<&str>) -> Result<(), String> {
    let gs = app.global_shortcut();
    match accel {
        Some(a) => gs.unregister(a).map_err(|e| format!("desktop.shortcut '{}': {}", a, e)),
        None => gs.unregister_all().map_err(|e| e.to_string()),
    }
}
