//! Standard library (`fogu:*`), grouped by area (the core built-ins come from
//! `fogu-core` and are re-exported here). Areas other than `base` and
//! `io` are cargo features (all on by default): `cargo build --release
//! --no-default-features --features network,database` builds a smaller binary.
//! Every module is re-exported here, so its path stays `crate::native::<name>`.

pub mod base;
pub use fogu_core::native::{buffer, collections, context, events, function, json, math, object, rt, test, util};
pub mod io;
#[cfg(feature = "network")]
pub mod network;
#[cfg(feature = "database")]
pub mod database;
#[cfg(feature = "messaging")]
pub mod messaging;
#[cfg(feature = "ops")]
pub mod ops;
#[cfg(feature = "ai")]
pub mod aiml;
#[cfg(feature = "media")]
pub mod media;
#[cfg(feature = "rpc")]
pub mod rpc;
pub use base::time;
pub use base::types;
pub use base::sync;
pub use base::cron;
pub use base::log;
pub use base::regex;
pub use base::crypto;
pub use base::password;
pub use base::vm;
pub use base::fogu_global;
#[cfg(not(target_family = "wasm"))]
pub use base::host;
#[cfg(not(target_family = "wasm"))]
pub use base::signal;
#[cfg(not(target_family = "wasm"))]
pub use base::cli;
#[cfg(not(target_family = "wasm"))]
pub use base::ffi;
pub use io::fs;
pub use io::stream;
pub use io::path;
pub use io::process;
pub use io::system;
pub use io::shell;
#[cfg(not(target_family = "wasm"))]
pub use io::watch;
pub use io::zip;
pub use io::formats;
pub use io::json5;
pub use io::markdown;
#[cfg(not(target_family = "wasm"))]
pub use io::net;
#[cfg(not(target_family = "wasm"))]
pub use io::ws_client;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::dns;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::http;
#[cfg(all(feature = "network", target_os = "linux"))]
pub use network::http_direct;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::http_client;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::websocket;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::socketio;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::webrtc;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::web;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::topcoat;
#[cfg(feature = "network")]
pub use network::jwt;
#[cfg(feature = "network")]
pub use network::html;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::mail;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::security;
#[cfg(feature = "network")]
#[cfg(not(target_family = "wasm"))]
pub use network::tls;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::sql;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::orm;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::kv;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::surrealkv;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::surrealdb;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::scylla;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::redis;
#[cfg(feature = "database")]
#[cfg(not(target_family = "wasm"))]
pub use database::etcd;
#[cfg(feature = "messaging")]
pub use messaging::nats;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::mqtt;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::kafka;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::rabbitmq;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::amqp;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::bullmq;
#[cfg(feature = "messaging")]
pub use messaging::fogumq;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::fogumq_client;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::broker;
#[cfg(feature = "messaging")]
#[cfg(not(target_family = "wasm"))]
pub use messaging::broker_admin;
#[cfg(feature = "ops")]
pub use ops::ssh;
#[cfg(feature = "ops")]
pub use ops::ssh_mount;
#[cfg(feature = "ops")]
pub use ops::fs_remote;
#[cfg(feature = "ops")]
pub use ops::docker;
#[cfg(feature = "ops")]
pub use ops::service;
#[cfg(feature = "ops")]
#[cfg(not(target_family = "wasm"))]
pub use ops::playbook;
#[cfg(feature = "ops")]
#[cfg(not(target_family = "wasm"))]
pub use ops::s3;
#[cfg(feature = "ai")]
#[cfg(not(target_family = "wasm"))]
pub use aiml::ai;
#[cfg(feature = "ai")]
#[cfg(not(target_family = "wasm"))]
pub use aiml::ai_mcp;
#[cfg(feature = "ai")]
#[cfg(not(target_family = "wasm"))]
pub use aiml::mcp_client;
#[cfg(feature = "ai")]
pub use aiml::plot;
#[cfg(feature = "media")]
pub use media::image;
#[cfg(feature = "media")]
#[cfg(not(target_family = "wasm"))]
pub use media::webview;
#[cfg(feature = "rpc")]
#[cfg(not(target_family = "wasm"))]
pub use rpc::grpc;
#[cfg(feature = "rpc")]
pub use rpc::protobuf;

/// Releases what native modules hold outside the process (browser
/// processes of `fogu:webview`); called before the program exits.
pub fn cleanup() {
    #[cfg(all(feature = "media", not(target_family = "wasm")))]
    webview::shutdown();
    #[cfg(all(feature = "database", not(target_family = "wasm")))]
    surrealkv::close_all();
    #[cfg(all(feature = "messaging", not(target_family = "wasm")))]
    broker::close_all();
    #[cfg(feature = "ops")]
    ssh_mount::unmount_all();
}

/// Called with the exit code right before the program ends the process
/// (`exit`, `process.exit`, `Fogu.exit`, an unhandled SIGINT / SIGTERM): the
/// Windows service runner reports SERVICE_STOPPED from here.
static EXIT_HOOK: std::sync::OnceLock<fn(i32)> = std::sync::OnceLock::new();

/// Set by `fogu test`: `exit` called by the program stops it with an error instead of ending
/// the process, so the test fails and the other tests and files still run (an `exit(0)` in a
/// test made the whole run succeed and skipped the rest).
static TEST_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_test_mode(on: bool) {
    TEST_MODE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// `exit`, `process.exit`, `Fogu.exit`: ends the process, or in `fogu test` is an error.
pub fn exit_program(code: i32) -> Result<fogu_core::runtime::value::Value, String> {
    if TEST_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(format!("exit({}) called during a test (a test cannot end the process)", code));
    }
    exit(code)
}

/// Installs the exit hook (once; later calls are ignored).
pub fn on_exit(hook: fn(i32)) {
    let _ = EXIT_HOOK.set(hook);
}

/// Ends the program: `cleanup`, the exit hook, then the process exits.
pub fn exit(code: i32) -> ! {
    // one exit at a time: a second one (another signal, `process.exit` in a
    // task) must not end the process while the first is still flushing a
    // store or unmounting a folder in `cleanup`
    static EXITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _only = EXITING.lock().unwrap_or_else(|e| e.into_inner());
    cleanup();
    if let Some(hook) = EXIT_HOOK.get() {
        hook(code);
    }
    end_process(code)
}

/// The last step of [`exit`]. `exit(3)` and Windows' `ExitProcess` run the
/// thread-local destructors of the calling thread — which here may be a task (a
/// coroutine on its own stack) while the scheduler, the servers and the other tasks
/// still use that state: `Fogu.exit()` in a `fg` task with the program in
/// `Fogu.signal.wait()` crashed on Windows (0xC0000005, N01) and, with an HTTP server
/// running, aborted on Linux (a destructor reached a thread-local already gone:
/// exit code 134). The process is ended without them, as the program's memory is
/// already left to the system: the output is flushed first, `cleanup` already ran.
#[cfg(windows)]
fn end_process(code: i32) -> ! {
    use std::io::Write;
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn TerminateProcess(process: *mut std::ffi::c_void, code: u32) -> i32;
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: both are plain kernel32 calls on the pseudo-handle of this process
    unsafe {
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    std::process::exit(code)
}

#[cfg(unix)]
fn end_process(code: i32) -> ! {
    use std::io::Write;
    unsafe extern "C" {
        fn _exit(code: i32) -> !;
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: ends the process at once; nothing runs after it
    unsafe { _exit(code) }
}

#[cfg(not(any(unix, windows)))]
fn end_process(code: i32) -> ! {
    std::process::exit(code)
}

#[allow(unused_imports)]
pub use fogu_core::native::NativeContext;
#[allow(unused_imports)]
pub use fogu_core::native::NativeFunction;
#[allow(unused_imports)]
pub use fogu_core::native::{FoguObject, NativeObjectRef};