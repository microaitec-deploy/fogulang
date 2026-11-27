//! Tray icons (Tauri's `tray-icon` feature): `desktop.tray(...)` in Fogu.
//!
//! A tray is `{ id, icon, tooltip, title, visible, menu: [items],
//! showMenuOnLeftClick, clickAction, doubleClickAction }`; a menu item is
//! `"-"` (a separator) or `{ id, text, enabled, checked, accelerator,
//! action, items }` (`checked` makes it a check item, `items` a submenu,
//! `action` = quit | show | hide | toggle is done by the host itself).
//!
//! Events go to the program as `desktop:tray-event` and to the pages as
//! `desktop:tray`, both `{ tray, type, … }`: type `click` / `rightClick` /
//! `middleClick` / `doubleClick` (Windows and macOS; Linux delivers no icon
//! clicks, only the menu) and `menu` `{ item, checked }`.
//!
//! Linux (appindicator) delivers no icon clicks at all: a tray with a click
//! handler or `clickAction` gets a first menu item instead (`linuxClickItem`,
//! default "Open"), which fires the same `click` event and action.
//!
//! The item builders are shared with the window / app menus (menu.rs): an
//! item may also be `{ role }`, a predefined item (copy, paste, quit…).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use serde_json::{json, Value};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::bridge::Bridge;

/// Separates the tray id and the item id in a menu id: menu events of every
/// menu arrive in one handler.
const SEP: char = '\u{1f}';

#[derive(Default)]
struct TrayState {
    /// tray id → the check items of its menu (their state for events)
    checks: HashMap<String, HashMap<String, CheckMenuItem<Wry>>>,
    /// tray id → item id → host action (quit, show, hide, toggle)
    actions: HashMap<String, HashMap<String, String>>,
    /// tray id → (click action, double-click action)
    clicks: HashMap<String, (Option<String>, Option<String>)>,
    /// tray id → the text of the Linux click item (when the tray wants clicks)
    click_items: HashMap<String, String>,
    /// tray id → its menu items as given (to rebuild the menu with the click item)
    items: HashMap<String, Vec<Value>>,
    /// menu owner ("menu:app", "menu:<label>") → (accelerator, item id)
    accels: HashMap<String, Vec<(String, String)>>,
}

/// The id of the Linux click fallback item.
const CLICK_ITEM: &str = "__click";

static STATE: LazyLock<Mutex<TrayState>> = LazyLock::new(|| Mutex::new(TrayState::default()));

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(String::from)
}

fn flag(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(Value::as_bool)
}

/// Whether the app has a tray icon (close/minimize to tray only hide the
/// window when there is one to bring it back).
pub fn any(app: &AppHandle) -> bool {
    let ids: Vec<String> = STATE.lock().unwrap().clicks.keys().cloned().collect();
    ids.iter().any(|id| app.tray_by_id(id.as_str()).is_some())
}

fn icon(app: &AppHandle, o: &Value) -> Result<Option<Image<'static>>, String> {
    match text(o, "icon") {
        // relative paths: the program's folder (the host's working directory)
        Some(p) => Image::from_path(&p).map(Some).map_err(|e| format!("desktop.tray: icon {}: {}", p, e)),
        // the app's icon, else the one compiled into the host
        None => Ok(crate::app_icon().or_else(|| app.default_window_icon().map(|i| i.clone().to_owned()))),
    }
}

/// Builds the menu of a tray (or of a window / the app: `owner` "menu:<…>");
/// remembers its check items and actions.
pub(crate) fn menu(app: &AppHandle, tray: &str, items: &[Value]) -> Result<Menu<Wry>, String> {
    let m = Menu::new(app).map_err(|e| e.to_string())?;
    let mut checks = HashMap::new();
    let mut actions = HashMap::new();
    let mut accels = Vec::new();
    let click = STATE.lock().unwrap().click_items.get(tray).cloned();
    if let Some(text) = click.filter(|_| cfg!(target_os = "linux")) {
        let e = |e: tauri::Error| e.to_string();
        m.append(&MenuItem::with_id(app, format!("{}{}{}", tray, SEP, CLICK_ITEM), &text, true, None::<&str>).map_err(e)?).map_err(e)?;
        if !items.is_empty() {
            m.append(&PredefinedMenuItem::separator(app).map_err(e)?).map_err(e)?;
        }
    }
    for it in items {
        let k = item(app, tray, it, &mut checks, &mut actions, &mut accels)?;
        m.append(&k).map_err(|e| e.to_string())?;
    }
    let mut st = STATE.lock().unwrap();
    st.checks.insert(tray.to_string(), checks);
    st.actions.insert(tray.to_string(), actions);
    st.accels.insert(tray.to_string(), accels);
    Ok(m)
}

fn item(
    app: &AppHandle,
    tray: &str,
    it: &Value,
    checks: &mut HashMap<String, CheckMenuItem<Wry>>,
    actions: &mut HashMap<String, String>,
    accels: &mut Vec<(String, String)>,
) -> Result<MenuItemKind<Wry>, String> {
    let e = |e: tauri::Error| e.to_string();
    if it.as_str() == Some("-") || flag(it, "separator") == Some(true) {
        return PredefinedMenuItem::separator(app).map(MenuItemKind::Predefined).map_err(e);
    }
    if let Some(role) = text(it, "role") {
        return predefined(app, &role, text(it, "text")).map(MenuItemKind::Predefined);
    }
    let label = text(it, "text").or_else(|| text(it, "label")).unwrap_or_default();
    let id = text(it, "id").unwrap_or_else(|| label.clone());
    let full = format!("{}{}{}", tray, SEP, id);
    let enabled = flag(it, "enabled").unwrap_or(true);
    let accel = text(it, "accelerator");
    if let Some(Value::Array(sub)) = it.get("items") {
        let s = Submenu::with_id(app, full, &label, enabled).map_err(e)?;
        for c in sub {
            let k = item(app, tray, c, checks, actions, accels)?;
            s.append(&k).map_err(e)?;
        }
        return Ok(MenuItemKind::Submenu(s));
    }
    if let Some(a) = text(it, "action") {
        actions.insert(id.clone(), a);
    }
    if let Some(a) = &accel {
        accels.push((a.clone(), id.clone()));
    }
    if let Some(checked) = flag(it, "checked") {
        let c = CheckMenuItem::with_id(app, full, &label, enabled, checked, accel.as_deref()).map_err(e)?;
        checks.insert(id, c.clone());
        return Ok(MenuItemKind::Check(c));
    }
    MenuItem::with_id(app, full, &label, enabled, accel.as_deref()).map(MenuItemKind::MenuItem).map_err(e)
}

/// A predefined item: `{ role: "copy" }`, with an optional text.
fn predefined(app: &AppHandle, role: &str, text: Option<String>) -> Result<PredefinedMenuItem<Wry>, String> {
    let t = text.as_deref();
    let r = match role {
        "separator" => PredefinedMenuItem::separator(app),
        "copy" => PredefinedMenuItem::copy(app, t),
        "cut" => PredefinedMenuItem::cut(app, t),
        "paste" => PredefinedMenuItem::paste(app, t),
        "selectAll" => PredefinedMenuItem::select_all(app, t),
        "undo" => PredefinedMenuItem::undo(app, t),
        "redo" => PredefinedMenuItem::redo(app, t),
        "minimize" => PredefinedMenuItem::minimize(app, t),
        "maximize" => PredefinedMenuItem::maximize(app, t),
        "fullscreen" => PredefinedMenuItem::fullscreen(app, t),
        "hide" => PredefinedMenuItem::hide(app, t),
        "hideOthers" => PredefinedMenuItem::hide_others(app, t),
        "showAll" => PredefinedMenuItem::show_all(app, t),
        "closeWindow" => PredefinedMenuItem::close_window(app, t),
        "quit" => PredefinedMenuItem::quit(app, t),
        "services" => PredefinedMenuItem::services(app, t),
        "about" => PredefinedMenuItem::about(app, t, None),
        other => return Err(format!("desktop: unknown menu role '{}' (copy cut paste selectAll undo redo minimize maximize fullscreen hide hideOthers showAll closeWindow quit services about separator)", other)),
    };
    r.map_err(|e| e.to_string())
}

/// Creates the tray `id` (default "main") or changes it: only the keys given
/// change (`tooltip: null` removes the tooltip, `menu: null` the menu).
pub fn set(app: &AppHandle, o: &Value) -> Result<(), String> {
    let id = text(o, "id").unwrap_or_else(|| "main".into());
    let e = |e: tauri::Error| e.to_string();
    {
        let mut st = STATE.lock().unwrap();
        let has_action = {
            let c = st.clicks.entry(id.clone()).or_default();
            if o.get("clickAction").is_some() {
                c.0 = text(o, "clickAction");
            }
            if o.get("doubleClickAction").is_some() {
                c.1 = text(o, "doubleClickAction");
            }
            c.0.is_some()
        };
        // Linux: clicks come through a menu item
        let wants = flag(o, "hasClick").unwrap_or(false) || has_action;
        let label = text(o, "linuxClickItem").unwrap_or_else(|| "Open".into());
        let before = st.click_items.get(&id).cloned();
        if wants && o.get("linuxClickItem") != Some(&Value::Bool(false)) {
            st.click_items.insert(id.clone(), label);
        } else if o.get("hasClick").is_some() || o.get("clickAction").is_some() || o.get("linuxClickItem").is_some() {
            st.click_items.remove(&id);
        }
        let changed = before != st.click_items.get(&id).cloned();
        if let Some(Value::Array(items)) = o.get("menu") {
            st.items.insert(id.clone(), items.clone());
        } else if o.get("menu") == Some(&Value::Null) {
            st.items.remove(&id);
        }
        // rebuild the menu below when only the click item changed
        if changed && o.get("menu").is_none() && cfg!(target_os = "linux") {
            drop(st);
            return set(app, &merge_menu(o, &id));
        }
    }
    let new_menu = match o.get("menu") {
        Some(Value::Array(items)) => Some(Some(menu(app, &id, items)?)),
        Some(Value::Null) if cfg!(target_os = "linux") && STATE.lock().unwrap().click_items.contains_key(&id) => Some(Some(menu(app, &id, &[])?)),
        Some(Value::Null) => Some(None),
        // Linux: a tray that wants clicks always has a menu (the click item)
        None if cfg!(target_os = "linux") && app.tray_by_id(id.as_str()).is_none() && STATE.lock().unwrap().click_items.contains_key(&id) => {
            Some(Some(menu(app, &id, &[])?))
        }
        _ => None,
    };
    if let Some(t) = app.tray_by_id(id.as_str()) {
        if o.get("icon").is_some() {
            t.set_icon(icon(app, o)?).map_err(e)?;
        }
        if o.get("tooltip").is_some() {
            t.set_tooltip(text(o, "tooltip")).map_err(e)?;
        }
        if o.get("title").is_some() {
            t.set_title(text(o, "title")).map_err(e)?;
        }
        if let Some(m) = new_menu {
            t.set_menu(m).map_err(e)?;
        }
        if let Some(b) = flag(o, "showMenuOnLeftClick") {
            t.set_show_menu_on_left_click(b).map_err(e)?;
        }
        if let Some(b) = flag(o, "visible") {
            t.set_visible(b).map_err(e)?;
        }
        return Ok(());
    }
    let mut b = TrayIconBuilder::with_id(id.clone()).show_menu_on_left_click(flag(o, "showMenuOnLeftClick").unwrap_or(false));
    match icon(app, o)? {
        Some(i) => b = b.icon(i),
        None => return Err("desktop.tray: no icon (give one: { icon: \"icons/tray.png\" })".into()),
    }
    if let Some(t) = text(o, "tooltip") {
        b = b.tooltip(t);
    }
    if let Some(t) = text(o, "title") {
        b = b.title(t);
    }
    if let Some(Some(m)) = &new_menu {
        b = b.menu(m);
    }
    let t = b.build(app).map_err(e)?;
    if flag(o, "visible") == Some(false) {
        t.set_visible(false).map_err(e)?;
    }
    Ok(())
}

/// `o` with the tray's current menu items (to rebuild its menu).
fn merge_menu(o: &Value, id: &str) -> Value {
    let mut o = o.clone();
    let items = STATE.lock().unwrap().items.get(id).cloned().unwrap_or_default();
    if let Some(m) = o.as_object_mut() {
        m.insert("menu".into(), Value::Array(items));
        m.remove("hasClick");
        m.remove("clickAction");
        m.remove("linuxClickItem");
    }
    o
}

/// Removes a tray icon; false when there was none.
pub fn remove(app: &AppHandle, id: &str) -> bool {
    {
        let mut st = STATE.lock().unwrap();
        st.checks.remove(id);
        st.actions.remove(id);
        st.clicks.remove(id);
        st.click_items.remove(id);
        st.items.remove(id);
    }
    app.remove_tray_by_id(id).is_some()
}

/// `fogu.window` op "tray" from the page: `{ op: "set" | "remove", … }`.
pub fn page_op(app: &AppHandle, a: &Value) -> Result<Value, String> {
    match text(a, "op").as_deref() {
        Some("remove") => Ok(Value::Bool(remove(app, &text(a, "id").unwrap_or_else(|| "main".into())))),
        _ => set(app, a).map(|_| Value::Null),
    }
}

fn host_action(app: &AppHandle, a: &str) {
    match a {
        "quit" => app.exit(0),
        "show" => crate::window::show_main(app),
        "hide" => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
        }
        "toggle" => crate::window::toggle_main(app),
        _ => {}
    }
}

fn deliver(app: &AppHandle, data: Value) {
    let Some(bridge) = app.try_state::<Arc<Bridge>>().map(|b| Arc::clone(b.inner())) else { return };
    bridge.notify("desktop:tray-event", data.clone());
    bridge.send_event(app, "desktop:tray".to_string(), data, None);
}

/// The accelerators of the menu bars: `[{ accel, menu, item }]` (the page
/// matches them itself where the webview keeps the keys from the menu).
pub fn accelerators() -> Value {
    let st = STATE.lock().unwrap();
    let mut out = Vec::new();
    for (owner, list) in &st.accels {
        if let Some(menu) = owner.strip_prefix("menu:") {
            for (accel, item) in list {
                out.push(json!({ "accel": accel, "menu": menu, "item": item }));
            }
        }
    }
    Value::Array(out)
}

/// Fires a menu item as a click would (a check item toggles first).
pub fn fire(app: &AppHandle, owner: &str, id: &str) {
    let check = STATE.lock().unwrap().checks.get(owner).and_then(|m| m.get(id)).cloned();
    if let Some(c) = check {
        let _ = c.set_checked(!c.is_checked().unwrap_or(false));
    }
    handle(app, owner, id);
}

/// Every menu event (main thread). Only the tray menus' items have SEP in their id.
pub fn on_menu_event(app: &AppHandle, ev: MenuEvent) {
    let full = ev.id().0.clone();
    let Some((tray, id)) = full.split_once(SEP) else { return };
    handle(app, tray, id);
}

fn handle(app: &AppHandle, tray: &str, id: &str) {
    // window / app menus (menu.rs)
    if tray.starts_with("menu:") {
        let (checked, action) = {
            let st = STATE.lock().unwrap();
            let checked = st.checks.get(tray).and_then(|m| m.get(id)).and_then(|c| c.is_checked().ok());
            let action = st.actions.get(tray).and_then(|m| m.get(id)).cloned();
            (checked, action)
        };
        crate::menu::on_item(app, &tray["menu:".len()..], id, checked);
        if let Some(a) = action {
            run_action(app, &a);
        }
        return;
    }
    // Linux: the click fallback item
    if id == CLICK_ITEM {
        click(app, tray, "click");
        return;
    }
    let (checked, action) = {
        let st = STATE.lock().unwrap();
        let checked = st.checks.get(tray).and_then(|m| m.get(id)).and_then(|c| c.is_checked().ok());
        let action = st.actions.get(tray).and_then(|m| m.get(id)).cloned();
        (checked, action)
    };
    let data = json!({ "tray": tray, "type": "menu", "item": id, "checked": checked });
    // the program hears about the item before a quit
    deliver(app, data);
    if let Some(a) = action {
        run_action(app, &a);
    }
}

/// An item's host action; a quit waits a moment so the program hears about the item first.
pub(crate) fn run_action(app: &AppHandle, a: &str) {
    if a == "quit" {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            app.exit(0);
        });
    } else {
        host_action(app, a);
    }
}

/// A click on a tray (or its Linux click item): its action, then the event.
fn click(app: &AppHandle, tray: &str, kind: &str) {
    let action = {
        let st = STATE.lock().unwrap();
        st.clicks.get(tray).and_then(|c| if kind == "click" { c.0.clone() } else if kind == "doubleClick" { c.1.clone() } else { None })
    };
    if let Some(a) = action {
        host_action(app, &a);
    }
    deliver(app, json!({ "tray": tray, "type": kind }));
}

/// Every tray icon event (main thread).
pub fn on_tray_event(app: &AppHandle, ev: TrayIconEvent) {
    let (tray, kind) = match &ev {
        TrayIconEvent::Click { id, button, button_state: MouseButtonState::Up, .. } => (
            id.0.clone(),
            match button {
                MouseButton::Left => "click",
                MouseButton::Right => "rightClick",
                MouseButton::Middle => "middleClick",
            },
        ),
        TrayIconEvent::DoubleClick { id, .. } => (id.0.clone(), "doubleClick"),
        _ => return,
    };
    click(app, &tray, kind);
}
