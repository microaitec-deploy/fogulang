//! The standard library's side of `fogu_core::host`: its modules, and the
//! calls it serves itself because they need the interpreter.

use std::cell::RefCell;
use std::rc::Rc;

use fogu_core::host::{Handled, Host};
use fogu_core::native::{NativeContext, NativeObjectRef};
use fogu_core::runtime::environment::Environment;
use fogu_core::runtime::interpreter::Interpreter;
use fogu_core::runtime::value::Value;

pub struct Std;

impl Host for Std {
    fn install(&self, ctx: &mut NativeContext) {
        install(ctx);
    }

    fn intercept(&self, callee: &Value, args: &[Value], it: &Interpreter, env: &Rc<RefCell<Environment>>) -> Option<Handled> {
        let _ = env;
        // `fogu:vm` runs Fogu code with this interpreter
        if let Some(r) = crate::native::vm::intercept(callee, args, it) {
            return Some(Handled::Native(r));
        }
        // `socketio.server/connect` start the task that runs their handlers
        #[cfg(all(feature = "network", not(target_family = "wasm")))]
        if let Some(r) = crate::native::socketio::intercept(callee, args, it, env) {
            return Some(Handled::Native(r));
        }
        // `nats.fogu()` objects run handlers and wait cooperatively
        #[cfg(all(feature = "messaging", not(target_family = "wasm")))]
        if let Some(r) = crate::native::nats::fogu::intercept(callee, args, it, env) {
            return Some(Handled::Native(r));
        }
        // `ai.mcpServer(...)` (an MCP server written in Fogu) and its `safeCall`
        #[cfg(all(feature = "ai", not(target_family = "wasm")))]
        if let Some(r) = crate::native::ai_mcp::intercept(callee, args, it) {
            return Some(Handled::Native(r));
        }
        // `broker.admin(...)` runs the broker's management app (Fogu code)
        #[cfg(all(feature = "messaging", not(target_family = "wasm")))]
        if let Some(r) = crate::native::broker_admin::intercept(callee, args, it) {
            return Some(Handled::Native(r));
        }
        // `shard.render(ctx, props)` of fogu:topcoat calls the shard's function
        #[cfg(all(feature = "network", not(target_family = "wasm")))]
        if let Some(r) = crate::native::topcoat::intercept(callee, args, it, env) {
            return Some(Handled::Native(r));
        }
        // `Fogu.signal.on/wait/emit` run handlers and wait cooperatively
        #[cfg(not(target_family = "wasm"))]
        if let Some(r) = crate::native::signal::intercept(callee, args, it, env) {
            return Some(Handled::Native(r));
        }
        if let Value::NativeFn(f) = callee {
            // a C function of fogu:ffi (its signature lives in the ffi module)
            #[cfg(not(target_family = "wasm"))]
            if let Some(id) = crate::native::ffi::ffi_id(&f.name) {
                return Some(Handled::Native(crate::native::ffi::call_ffi(id, args)));
            }
            #[cfg(all(feature = "network", not(target_family = "wasm")))]
            if crate::native::web::is_dispatch(&f.name) {
                let r = match args.first() {
                    Some(first) => crate::native::web::dispatch(&f.name, first, it, env.clone()),
                    None => Err("an app handler requires 1 argument (the request)".into()),
                };
                return Some(Handled::Direct(r));
            }
            // `cron.tick()` runs the jobs that are due
            if f.name == "tick" && args.is_empty() {
                return Some(Handled::Direct(cron_tick(it)));
            }
        }
        None
    }

    fn may_intercept_native(&self, f: &fogu_core::runtime::value::NativeFnValue) -> bool {
        // the names `intercept` looks at for native functions (keep in sync)
        let name = f.name.as_str();
        if name.starts_with("vm.") || name == "tick" {
            return true;
        }
        #[cfg(all(feature = "network", not(target_family = "wasm")))]
        if matches!(name, "socketio.server" | "socketio.connect") || crate::native::web::is_dispatch(name) {
            return true;
        }
        #[cfg(all(feature = "ai", not(target_family = "wasm")))]
        if name == crate::native::ai_mcp::FACTORY || name == "ai.__safeCall" {
            return true;
        }
        #[cfg(not(target_family = "wasm"))]
        if crate::native::ffi::ffi_id(name).is_some() {
            return true;
        }
        false
    }

    fn intercept_method(
        &self,
        receiver: &NativeObjectRef,
        method: &str,
        args: &[Value],
        it: &Interpreter,
        env: &Rc<RefCell<Environment>>,
    ) -> Option<Result<Value, String>> {
        let _ = (receiver, method, args, it, env);
        // `Agent.run` calls the Fogu tool functions the model asks for
        #[cfg(all(feature = "ai", not(target_family = "wasm")))]
        if crate::native::ai::is_agent_run(receiver, method) {
            return Some(crate::native::ai::agent_run(receiver, args, it, env.clone()));
        }
        // `GrpcServer.serve` runs the Fogu handlers of incoming requests
        #[cfg(all(feature = "rpc", not(target_family = "wasm")))]
        if crate::native::grpc::is_serve(receiver, method) {
            return Some(crate::native::grpc::serve(receiver, args, it, env.clone()));
        }
        None
    }

    fn before_call(&self, it: &Interpreter, env: &Rc<RefCell<Environment>>) {
        let _ = (it, env);
        // serve the requests a `fogu:web` / `fogu:http` server queued
        #[cfg(all(feature = "network", not(target_family = "wasm")))]
        if crate::native::http::requests_waiting() {
            crate::native::http::serve_pending(it, env);
        }
    }
}

fn cron_tick(it: &Interpreter) -> Result<Value, String> {
    let mut n = 0;
    for func in crate::native::cron::take_ready_jobs() {
        it.call_value(func, Vec::new(), it.globals_env())?;
        n += 1;
    }
    Ok(Value::Int(n))
}

/// The standard library modules and the global `Fogu` object.
fn install(ctx: &mut NativeContext) {
    ctx.define_lazy("system", crate::native::system::build);
    ctx.define_lazy("time", crate::native::time::build);
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("net", crate::native::net::build);
    ctx.define_lazy("cron", crate::native::cron::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("etcd", crate::native::etcd::build);
    #[cfg(feature = "rpc")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("grpc", crate::native::grpc::build);
    #[cfg(feature = "network")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("security", crate::native::security::build);
    #[cfg(feature = "messaging")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("broker", crate::native::broker::build);
    #[cfg(feature = "messaging")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("kafka", crate::native::kafka::build);
    #[cfg(feature = "messaging")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("mq", crate::native::broker::build);
    #[cfg(feature = "network")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("socketio", crate::native::socketio::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("kv", crate::native::kv::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("surrealkv", crate::native::surrealkv::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("surrealdb", crate::native::surrealdb::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("sql", crate::native::sql::build);
    #[cfg(feature = "network")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("http", crate::native::http::build);
    #[cfg(feature = "network")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("web", crate::native::web::build);
    #[cfg(feature = "network")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("topcoat", crate::native::topcoat::build);
    ctx.define_lazy("fs", crate::native::fs::build);
    ctx.define_lazy("path", crate::native::path::build);
    ctx.define_lazy("process", crate::native::process::build);
    ctx.define_lazy("sync", crate::native::sync::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("orm", crate::native::orm::build);
    #[cfg(feature = "network")]
    ctx.define_lazy("jwt", crate::native::jwt::build);
    #[cfg(feature = "messaging")]
    ctx.define_lazy("fogumq", crate::native::fogumq::build);
    #[cfg(feature = "messaging")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("foguMQ-client", crate::native::fogumq_client::build);
    #[cfg(feature = "network")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("mail", crate::native::mail::build);
    #[cfg(feature = "ai")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("ai", crate::native::ai::build);
    ctx.define_lazy("shell", crate::native::shell::build);
    #[cfg(feature = "ops")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("s3", crate::native::s3::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("redis", crate::native::redis::build);
    #[cfg(feature = "messaging")]
    ctx.define_lazy("nats", crate::native::nats::build);
    #[cfg(feature = "messaging")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("rabbitmq", crate::native::rabbitmq::build);
    #[cfg(feature = "messaging")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("mqtt", crate::native::mqtt::build);
    #[cfg(feature = "network")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("websocket", crate::native::websocket::build);
    ctx.define_lazy("regex", crate::native::regex::build);
    #[cfg(feature = "network")]
    ctx.define_lazy("html", crate::native::html::build);
    #[cfg(feature = "media")]
    ctx.define_lazy("image", crate::native::image::build);
    #[cfg(feature = "media")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("webview", crate::native::webview::build);
    #[cfg(feature = "ops")]
    ctx.define_lazy("docker", crate::native::docker::build);
    #[cfg(feature = "ops")]
    ctx.define_lazy("service", crate::native::service::build);
    #[cfg(feature = "ai")]
    ctx.define_lazy("plot", crate::native::plot::build);
    ctx.define_lazy("zip", crate::native::zip::build);
    #[cfg(feature = "rpc")]
    ctx.define_lazy("protobuf", crate::native::protobuf::build);
    ctx.define_lazy("crypto", crate::native::crypto::build);
    ctx.define_lazy("log", crate::native::log::build);
    ctx.define_lazy("csv", crate::native::formats::csv_build);
    ctx.define_lazy("excel", crate::native::formats::excel_build);
    ctx.define_lazy("yaml", crate::native::formats::yaml_build);
    ctx.define_lazy("toml", crate::native::formats::toml_build);
    ctx.define_lazy("xml", crate::native::formats::xml_build);
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("ffi", crate::native::ffi::build);
    #[cfg(feature = "ops")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("playbook", crate::native::playbook::build);
    #[cfg(feature = "messaging")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("bullmq", crate::native::bullmq::build);
    ctx.define_lazy("vm", crate::native::vm::build);
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("cli", crate::native::cli::build);
    #[cfg(feature = "database")]
    #[cfg(not(target_family = "wasm"))]
    ctx.define_lazy("scylla", crate::native::scylla::build);
    #[cfg(feature = "ops")]
    ctx.define_lazy("ssh", crate::native::ssh::build);
    // the global `Fogu` object (spec §45)
    ctx.define_value(
        "Fogu",
        Value::Module(Rc::new(crate::native::fogu_global::build())),
    );
}
