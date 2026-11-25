//! Menus (`desktop.menu(...)` in Fogu, `fogu.menu.set(...)` in the page): the
//! app menu (the macOS menu bar; on Windows and Linux the menu bar of every
//! window that has none of its own) and a window's own menu bar.
//!
//! `{ window, menu: [items] }`: no `window` = the app menu; `menu: null`
//! removes it. The items are the tray's (tray.rs): `{ text, id, enabled,
//! checked, accelerator, items, action, role }`, "-" a separator; on macOS the
//! top-level items must be submenus (`{ text: "File", items: [...] }`).
//!
//! A click goes to the program as `desktop:menu-event` and to the pages as
//! `desktop:menu`, both `{ menu, item, checked }` (`menu`: the window's label,
//! or "app").

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::bridge::Bridge;

pub fn set(app: &AppHandle, o: &Value) -> Result<(), String> {
    let e = |e: tauri::Error| e.to_string();
    let window = o.get("window").and_then(Value::as_str).map(String::from);
    let owner = format!("menu:{}", window.as_deref().unwrap_or("app"));
    let items = match o.get("menu") {
        Some(Value::Array(items)) => Some(items.clone()),
        Some(Value::Null) | None => None,
        Some(_) => return Err("desktop.menu: `menu` must be a list of items or nil".into()),
    };
    match (window, items) {
        (Some(label), Some(items)) => {
            let w = app.get_webview_window(&label).ok_or_else(|| format!("desktop.menu: no window '{}'", label))?;
            let m = crate::tray::menu(app, &owner, &items)?;
            w.set_menu(m).map_err(e)?;
        }
        (Some(label), None) => {
            let w = app.get_webview_window(&label).ok_or_else(|| format!("desktop.menu: no window '{}'", label))?;
            w.remove_menu().map_err(e)?;
        }
        (None, Some(items)) => {
            let m = crate::tray::menu(app, &owner, &items)?;
            app.set_menu(m).map_err(e)?;
        }
        (None, None) => {
            app.remove_menu().map_err(e)?;
        }
    }
    // the pages match the accelerators themselves where the webview keeps the keys (Windows)
    if let Some(bridge) = app.try_state::<Arc<Bridge>>() {
        bridge.send_event(app, "desktop:accelerators".to_string(), crate::tray::accelerators(), None);
    }
    Ok(())
}

/// A menu item was clicked (tray.rs routes the `menu:` ids here).
pub fn on_item(app: &AppHandle, menu: &str, item: &str, checked: Option<bool>) {
    let Some(bridge) = app.try_state::<Arc<Bridge>>().map(|b| Arc::clone(b.inner())) else { return };
    let data = json!({ "menu": menu, "item": item, "checked": checked, "type": "menu" });
    bridge.notify("desktop:menu-event", data.clone());
    bridge.send_event(app, "desktop:menu".to_string(), data, None);
}
