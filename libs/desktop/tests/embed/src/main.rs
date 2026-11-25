//! The bridge protocol of fogu-desktop, end to end, without a window: the
//! example app (examples/hello) runs in `fogu::embed::Runtime` with the same
//! setup as runtime/src/main.rs + bridge.rs (working directory, environment,
//! `Fogu.host.emit` callback), and this program plays the page.
//!
//!   cargo run --release          (from libs/desktop/tests/embed)

use std::ffi::{c_char, c_void, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use fogu::embed::Runtime;
use serde_json::{json, Value};

static TX: OnceLock<Sender<(String, Value)>> = OnceLock::new();

extern "C" fn on_host(name: *const c_char, data: *const c_char, _user: *mut c_void) {
    let take = |p: *const c_char| unsafe { CString::from_raw(p as *mut c_char) }.to_string_lossy().into_owned();
    let (name, data) = (take(name), take(data));
    let data = serde_json::from_str(&data).unwrap_or(Value::Null);
    let _ = TX.get().unwrap().send((name, data));
}

thread_local! {
    /// Host messages received but not consumed yet (events may arrive before a result).
    static BACKLOG: std::cell::RefCell<Vec<(String, Value)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The next host message matching `name` (and, for events, the event name `ev`).
fn take(rx: &Receiver<(String, Value)>, name: &str, ev: Option<&str>) -> Value {
    let matches = |n: &str, d: &Value| n == name && ev.map_or(true, |e| d["name"] == e);
    if let Some(i) = BACKLOG.with(|b| b.borrow().iter().position(|(n, d)| matches(n, d))) {
        return BACKLOG.with(|b| b.borrow_mut().remove(i)).1;
    }
    loop {
        let (n, d) = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("timed out waiting for {} {:?}", name, ev));
        if matches(&n, &d) {
            return d;
        }
        BACKLOG.with(|b| b.borrow_mut().push((n, d)));
    }
}

fn next(rx: &Receiver<(String, Value)>, name: &str) -> Value {
    take(rx, name, None)
}

fn event(rx: &Receiver<(String, Value)>, ev: &str) -> Value {
    take(rx, "desktop:event", Some(ev))
}

fn call(rt: &Runtime, rx: &Receiver<(String, Value)>, id: u64, name: &str, payload: Value) -> Value {
    rt.emit("desktop:call", &json!({ "id": id, "name": name, "payload": payload }).to_string()).unwrap();
    let r = next(rx, "desktop:result");
    assert_eq!(r["id"], id, "results come back with their id");
    r
}

fn main() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app = here.join("../../examples/hello").canonicalize().unwrap();
    let data = std::env::temp_dir().join(format!("fogu-desktop-embed-test-{}", std::process::id()));
    std::fs::create_dir_all(&data).unwrap();

    // as runtime/src/main.rs: cwd = the program, then the environment, then the runtime
    std::env::set_current_dir(&app).unwrap();
    unsafe {
        std::env::set_var("FOGU_DESKTOP", "1");
        std::env::set_var("FOGU_DESKTOP_DATA_DIR", &data);
    }
    let (tx, rx) = channel();
    TX.set(tx).unwrap();
    let rt = Runtime::new().unwrap();
    rt.on_host(Some(on_host), 0).unwrap();
    rt.run_file(app.join("main.fg").to_str().unwrap()).expect("main.fg runs");
    println!("main.fg ran");

    let r = call(&rt, &rx, 1, "list", Value::Null);
    println!("list   → {}", r);
    assert_eq!(r["ok"], true);
    assert_eq!(r["value"]["items"], json!([]));

    let r = call(&rt, &rx, 2, "add", json!({ "text": "  write the README " }));
    println!("add    → {}", r);
    assert_eq!(r["value"]["text"], "write the README");
    let ev = event(&rx, "todos");
    println!("event  → {}", ev);
    assert_eq!(ev["data"]["stats"]["total"], 1);

    let r = call(&rt, &rx, 3, "add", json!({ "text": "   " }));
    println!("add '' → {}", r);
    assert_eq!(r, json!({ "id": 3, "ok": false, "error": "a todo needs some text" }));

    let r = call(&rt, &rx, 4, "toggle", json!({ "id": 1 }));
    assert_eq!(r["ok"], true);
    let _ = event(&rx, "todos");

    let r = call(&rt, &rx, 5, "nope", Value::Null);
    println!("nope   → {}", r);
    assert_eq!(r["ok"], false);

    // a page event: fogu.emit("clear-done") → desktop.on("clear-done")
    rt.emit("desktop:ui:clear-done", "null").unwrap();
    let _ = event(&rx, "todos");
    let ev = event(&rx, "toast");
    println!("event  → {}", ev);
    assert_eq!(ev, json!({ "name": "toast", "data": "1 finished todo(s) removed" }));

    // the background task keeps running between calls
    let up = event(&rx, "uptime");
    println!("uptime → {}", up["data"]);

    // a request with an answer: "export" opens a save dialog → desktop:request → desktop:reply
    let target = data.join("out.md");
    rt.emit("desktop:call", &json!({ "id": 6, "name": "export", "payload": null }).to_string()).unwrap();
    let req = next(&rx, "desktop:request");
    println!("request→ {}", req);
    assert_eq!(req["op"], "dialog.save");
    rt.emit("desktop:reply", &json!({ "id": req["id"], "ok": true, "value": target.to_str().unwrap() }).to_string()).unwrap();
    let r = next(&rx, "desktop:result");
    println!("export → {}", r);
    assert_eq!(r["value"], target.to_str().unwrap());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "# Todo\n\n\n");

    // the data file the app saved
    let saved = std::fs::read_to_string(data.join("todos.json")).unwrap();
    println!("saved  → {}", saved);
    drop(rt);
    let _ = std::fs::remove_dir_all(&data);
    println!("embed protocol: all checks passed");
}
