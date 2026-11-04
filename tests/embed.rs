//! The embedding API (`fogu::embed::Runtime`, the core of `libfogu_rt`).

use std::ffi::{c_char, c_void, CStr, CString};

use fogu::embed::Runtime;

/// A host function: answers the arguments' JSON reversed as a text, or an error for "fail".
extern "C" fn reverse(args: *const c_char, user: *mut c_void) -> *mut c_char {
    let calls = unsafe { &*(user as *const std::sync::atomic::AtomicUsize) };
    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let json = unsafe { CStr::from_ptr(args) }.to_str().unwrap().to_string();
    if json.contains("fail") {
        let mut b = vec![fogu::native::host::ERROR_MARK];
        b.extend_from_slice(b"host said no");
        return CString::new(b).unwrap().into_raw();
    }
    if json == "[]" {
        return std::ptr::null_mut();
    }
    let rev: String = json.chars().rev().collect();
    CString::new(serde_json::to_string(&rev).unwrap()).unwrap().into_raw()
}

#[test]
fn eval_call_and_globals() {
    let rt = Runtime::new().unwrap();
    rt.eval("fn double(x) { return x * 2 }").unwrap();
    assert_eq!(rt.call("double", "[21]").unwrap(), "42");
    rt.set("cfg", r#"{"n": 3}"#).unwrap();
    assert_eq!(rt.eval("cfg.n + 1").unwrap(), "4");
    assert!(rt.eval("1 / 0").unwrap_err().contains("division by zero"));
}

#[test]
fn registered_host_functions() {
    let rt = Runtime::new().unwrap();
    let calls = Box::leak(Box::new(std::sync::atomic::AtomicUsize::new(0)));
    rt.register("rev", reverse, calls as *const _ as usize).unwrap();
    assert_eq!(rt.eval(r#"rev("ab", 1)"#).unwrap(), r#""]1,\"ba\"[""#);
    assert_eq!(rt.eval("rev()").unwrap(), "null");
    assert_eq!(rt.eval(r#"const [_, err] = rev("fail")
err.message"#).unwrap(), r#""host said no""#);
    // usable from Fogu code like any function, also inside a loop and by name
    assert_eq!(rt.eval(r#"let n = 0
for i in 0..300 { if rev(i) != nil { n = n + 1 } }
n"#).unwrap(), "300");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 303);
    assert!(rt.register("1bad", reverse, 0).is_err());
}

/// Sources kept in memory over a real folder (the desktop host's bundles):
/// imports between them resolve, the folder's other files are read from disk.
#[test]
fn sources_in_memory_over_a_folder() {
    let dir = std::env::temp_dir().join(format!("fogu-embed-vfs-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data").join("n.txt"), "41").unwrap();
    fogu::runtime::vfs::install_at(
        &dir,
        [
            ("main.fg".to_string(), "import fs from \"fogu:fs\"\nimport lib from \"./lib/add.fg\"\nlet answer = lib.add(int(fs.readText(root + \"/data/n.txt\")?), 1)".to_string()),
            ("lib/add.fg".to_string(), "export fn add(a, b) { return a + b }".to_string()),
        ],
    );
    assert!(!dir.join("main.fg").exists());
    let rt = Runtime::new().unwrap();
    rt.set("root", &serde_json::to_string(&dir.to_string_lossy()).unwrap()).unwrap();
    rt.run_file(&dir.join("main.fg").to_string_lossy()).unwrap();
    assert_eq!(rt.eval("answer").unwrap(), "42");
    drop(rt);
    fogu::runtime::vfs::clear();
    let _ = std::fs::remove_dir_all(&dir);
}
