//! `ai.mcpClient(options)` — a client for MCP (Model Context Protocol) servers:
//! a local program spoken to over stdio (`{ command, args, env, cwd }`) or a
//! remote one over Streamable HTTP (`{ url, headers }`). `client.tools()` turns
//! the server's tools into `ai.tool` values that `ai.agent` can use.

use ahash::AHashMap as HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::runtime::sched;
use crate::runtime::value::Value;

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, json_to_value, list, map, max_args, str_arg};

const PROTOCOL: &str = "2025-06-18";

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn to_json(v: &Value) -> Json {
    serde_json::from_str(&stringify_value(v)).unwrap_or(Json::Null)
}

fn rpc_error(e: &Json) -> String {
    e.get("message").and_then(Json::as_str).map(String::from).unwrap_or_else(|| e.to_string())
}

type Slot = Arc<Mutex<Option<Result<Json, String>>>>;

// ---------------------------------------------------------------------
// transports
// ---------------------------------------------------------------------

/// A server process: JSON-RPC lines on its stdin/stdout.
struct Stdio_ {
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pending: Mutex<HashMap<u64, Slot>>,
    error: Mutex<Option<String>>,
}

impl Stdio_ {
    fn write(&self, msg: &Json) -> Result<(), String> {
        let mut g = lock(&self.stdin);
        let w = g.as_mut().ok_or("the server is closed")?;
        let mut line = msg.to_string();
        line.push('\n');
        w.write_all(line.as_bytes()).and_then(|_| w.flush()).map_err(|e| format!("cannot write to the server: {}", e))
    }

    fn fail_all(&self, why: &str) {
        *lock(&self.error) = Some(why.to_string());
        for (_, slot) in lock(&self.pending).drain() {
            *lock(&slot) = Some(Err(why.to_string()));
        }
        sched::wake();
    }
}

/// Reads the server's messages: answers ours, replies to its own requests.
fn read_loop(io: Arc<Stdio_>, stdout: std::process::ChildStdout) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Json>(line.trim()) else { continue };
        let id = msg.get("id").cloned();
        match (msg.get("method").and_then(Json::as_str), id) {
            // a request from the server: ping is answered, the rest is not supported
            (Some(method), Some(id)) => {
                let reply = if method == "ping" {
                    json!({ "jsonrpc": "2.0", "id": id, "result": {} })
                } else {
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("the client does not support {}", method) } })
                };
                let _ = io.write(&reply);
            }
            (None, Some(id)) => {
                if let Some(slot) = id.as_u64().and_then(|n| lock(&io.pending).remove(&n)) {
                    *lock(&slot) = Some(match msg.get("error") {
                        Some(e) if !e.is_null() => Err(rpc_error(e)),
                        _ => Ok(msg.get("result").cloned().unwrap_or(Json::Null)),
                    });
                    sched::wake();
                }
            }
            _ => {} // notifications (logging, progress…)
        }
    }
    io.fail_all("the MCP server exited");
}

/// A remote server: JSON-RPC POSTed to one URL (JSON or SSE answers).
struct Http {
    client: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
    session: Mutex<Option<String>>,
}

async fn http_post(h: Arc<Http>, msg: Json, timeout: Duration) -> Result<Json, String> {
    let mut req = h
        .client
        .post(&h.url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", PROTOCOL)
        .body(msg.to_string());
    for (k, v) in &h.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if let Some(s) = lock(&h.session).clone() {
        req = req.header("mcp-session-id", s);
    }
    let res = req.send().await.map_err(|e| format!("{} ({})", e, h.url))?;
    if let Some(s) = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
        *lock(&h.session) = Some(s.to_string());
    }
    let status = res.status();
    let sse = res.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.contains("text/event-stream"));
    let text = res.text().await.map_err(|e| e.to_string())?;
    if msg.get("id").is_none() {
        return Ok(Json::Null); // a notification: 202, no body
    }
    if !status.is_success() {
        let detail = serde_json::from_str::<Json>(&text).ok().and_then(|j| j.get("error").map(rpc_error)).unwrap_or(text);
        return Err(format!("HTTP {}: {}", status.as_u16(), detail.chars().take(200).collect::<String>()));
    }
    // an SSE answer: the `data:` event that carries our id
    let answer: Json = if sse {
        text.split("\n\n")
            .filter_map(|ev| {
                let data: String = ev.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).collect::<Vec<_>>().join("\n");
                serde_json::from_str::<Json>(&data).ok()
            })
            .find(|j| j.get("id") == msg.get("id"))
            .ok_or("no answer in the event stream")?
    } else {
        serde_json::from_str(&text).map_err(|_| format!("not JSON: {}", text.chars().take(200).collect::<String>()))?
    };
    match answer.get("error") {
        Some(e) if !e.is_null() => Err(rpc_error(e)),
        _ => Ok(answer.get("result").cloned().unwrap_or(Json::Null)),
    }
}

enum Transport {
    Stdio(Arc<Stdio_>),
    Http(Arc<Http>),
}

struct Conn {
    transport: Transport,
    next: AtomicU64,
    timeout: Duration,
    closed: AtomicBool,
}

impl Conn {
    /// Sends a request; the poller yields its result (or an error message).
    fn request(&self, method: &str, params: Json) -> Result<Box<dyn FnMut() -> Option<Result<Json, String>>>, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let timeout = self.timeout;
        match &self.transport {
            Transport::Stdio(io) => {
                if let Some(e) = lock(&io.error).clone() {
                    return Err(e);
                }
                let slot: Slot = Arc::default();
                lock(&io.pending).insert(id, Arc::clone(&slot));
                io.write(&msg)?;
                let until = Instant::now() + timeout;
                Ok(Box::new(move || match lock(&slot).take() {
                    Some(r) => Some(r),
                    None if Instant::now() >= until => Some(Err(format!("no answer after {:?}", timeout))),
                    None => {
                        sched::add_timer(Instant::now() + Duration::from_millis(10));
                        None
                    }
                }))
            }
            Transport::Http(h) => {
                let pending = crate::native::rt::spawn(http_post(Arc::clone(h), msg, timeout));
                Ok(Box::new(move || pending.try_take()))
            }
        }
    }

    fn notify(&self, method: &str) {
        let msg = json!({ "jsonrpc": "2.0", "method": method });
        match &self.transport {
            Transport::Stdio(io) => {
                let _ = io.write(&msg);
            }
            Transport::Http(h) => {
                let _ = crate::native::rt::spawn(http_post(Arc::clone(h), msg, self.timeout));
            }
        }
    }

    /// A request waited for on this thread (only while connecting).
    fn request_blocking(&self, method: &str, params: Json) -> Result<Json, String> {
        let mut p = self.request(method, params)?;
        loop {
            if let Some(r) = p() {
                return r;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Transport::Stdio(io) = &self.transport {
            lock(&io.stdin).take(); // EOF: a well-behaved server exits
            if let Some(mut child) = lock(&io.child).take() {
                let deadline = Instant::now() + Duration::from_millis(500);
                while Instant::now() < deadline {
                    if let Ok(Some(_)) = child.try_wait() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.close();
    }
}

// ---------------------------------------------------------------------
// ai.mcpClient
// ---------------------------------------------------------------------

/// `mcpClient({ command, args, env, cwd, stderr } | { url, headers }, { timeout = 30s })`
pub fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "ai.mcpClient")?;
    let o: HashMap<String, Value> = match args.first() {
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(Value::Str(u)) => HashMap::from([("url".to_string(), Value::Str(u.clone()))]),
        _ => return Err("ai.mcpClient expects { command, args } (stdio) or { url } (HTTP)".into()),
    };
    let s = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
    let timeout = match o.get("timeout") {
        None | Some(Value::Nil) => Duration::from_secs(30),
        Some(t) => duration_arg(t, "ai.mcpClient timeout")?,
    };
    let strs = |k: &str| -> Vec<String> {
        match o.get(k) {
            Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
            _ => Vec::new(),
        }
    };
    let pairs = |k: &str| -> Vec<(String, String)> {
        match o.get(k) {
            Some(Value::Map(m)) => m.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect(),
            _ => Vec::new(),
        }
    };
    let transport = match (s("command"), s("url")) {
        (Some(cmd), _) => {
            let mut c = Command::new(&cmd);
            c.args(strs("args")).envs(pairs("env")).stdin(Stdio::piped()).stdout(Stdio::piped());
            c.stderr(if s("stderr").as_deref() == Some("ignore") { Stdio::null() } else { Stdio::inherit() });
            if let Some(dir) = s("cwd") {
                c.current_dir(dir);
            }
            let mut child = c.spawn().map_err(|e| io_err!("ai.mcpClient: cannot start {}: {}", cmd, e))?;
            let stdin = child.stdin.take();
            let stdout = child.stdout.take().ok_or("ai.mcpClient: no stdout")?;
            let io = Arc::new(Stdio_ { stdin: Mutex::new(stdin), child: Mutex::new(Some(child)), pending: Mutex::default(), error: Mutex::default() });
            let r = Arc::clone(&io);
            std::thread::Builder::new().name("fogu-mcp".into()).spawn(move || read_loop(r, stdout)).map_err(|e| e.to_string())?;
            Transport::Stdio(io)
        }
        (None, Some(url)) => {
            let client = reqwest::Client::builder().build().map_err(|e| e.to_string())?;
            Transport::Http(Arc::new(Http { client, url, headers: pairs("headers"), session: Mutex::default() }))
        }
        (None, None) => return Err("ai.mcpClient expects { command, args } (stdio) or { url } (HTTP)".into()),
    };
    let conn = Arc::new(Conn { transport, next: AtomicU64::new(0), timeout, closed: AtomicBool::new(false) });
    let c2 = Arc::clone(&conn);
    let init = crate::native::rt::offload(move || {
        let r = c2.request_blocking(
            "initialize",
            json!({ "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": { "name": "fogu", "version": env!("CARGO_PKG_VERSION") } }),
        );
        if r.is_ok() {
            c2.notify("notifications/initialized");
        }
        r
    })?;
    let init = match init {
        Ok(j) => j,
        Err(e) => {
            conn.close();
            return Err(io_err!("ai.mcpClient: {}", e));
        }
    };
    Ok(NativeObjectRef::new(McpClient { conn, init }).to_value())
}

/// The text of MCP content blocks.
fn content_text(content: &Json) -> String {
    content
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|c| match c.get("type").and_then(Json::as_str) {
                    Some("text") => c.get("text").and_then(Json::as_str).map(String::from),
                    Some("resource") => c.pointer("/resource/text").and_then(Json::as_str).map(String::from),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// A tools/call result → its structured value, else its text; `isError` → an error value.
fn tool_value(r: &Json) -> Result<Value, String> {
    if r.get("isError").and_then(Json::as_bool) == Some(true) {
        let msg = content_text(&r["content"]);
        return Err(io_err!("{}", if msg.is_empty() { "the tool failed".to_string() } else { msg }));
    }
    if let Some(s) = r.get("structuredContent").filter(|s| !s.is_null()) {
        return Ok(json_to_value(s));
    }
    Ok(Value::Str(content_text(&r["content"]).into()))
}

fn finish(mut p: Box<dyn FnMut() -> Option<Result<Json, String>>>, what: String, f: fn(&Json) -> Result<Value, String>) -> Poller {
    Box::new(move || {
        let r = p()?;
        Some(match r {
            Ok(j) => f(&j),
            Err(e) => Err(io_err!("{}: {}", what, e)),
        })
    })
}

struct McpClient {
    conn: Arc<Conn>,
    init: Json,
}

impl FoguObject for McpClient {
    fn name(&self) -> &str {
        "McpClient"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "serverInfo" => json_to_value(&self.init["serverInfo"]),
            "protocolVersion" => json_to_value(&self.init["protocolVersion"]),
            "capabilities" => json_to_value(&self.init["capabilities"]),
            "instructions" => json_to_value(&self.init["instructions"]),
            "closed" => Value::Bool(self.conn.closed.load(Ordering::SeqCst)),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "close" => {
                self.conn.close();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on McpClient ({})", other, self.methods().join(" "))),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let what = format!("mcp.{}", method);
        let conn = Arc::clone(&self.conn);
        let r = (|| -> Result<Poller, String> {
            if conn.closed.load(Ordering::SeqCst) {
                return Err(io_err!("{}: the client is closed", what));
            }
            let req = |m: &str, p: Json| conn.request(m, p).map_err(|e| io_err!("{}: {}", what, e));
            Ok(match method {
                "listTools" => finish(req("tools/list", json!({}))?, what, |j| Ok(json_to_value(&j["tools"]))),
                // callTool(name, arguments) → the structured result or the text; isError → an error value
                "callTool" => {
                    max_args(args, 2, &what)?;
                    let name = str_arg(args, 0, &what)?;
                    let arguments = args.get(1).map(to_json).filter(|j| !j.is_null()).unwrap_or(json!({}));
                    finish(req("tools/call", json!({ "name": name, "arguments": arguments }))?, what, tool_value)
                }
                "listResources" => finish(req("resources/list", json!({}))?, what, |j| Ok(json_to_value(&j["resources"]))),
                "readResource" => {
                    max_args(args, 1, &what)?;
                    let uri = str_arg(args, 0, &what)?;
                    finish(req("resources/read", json!({ "uri": uri }))?, what, |j| Ok(Value::Str(
                        j["contents"].as_array().map(|c| c.iter().filter_map(|x| x.get("text").and_then(Json::as_str)).collect::<Vec<_>>().join("\n")).unwrap_or_default().into(),
                    )))
                }
                "listPrompts" => finish(req("prompts/list", json!({}))?, what, |j| Ok(json_to_value(&j["prompts"]))),
                // getPrompt(name, arguments) → { description, messages: [{ role, text }] }
                "getPrompt" => {
                    max_args(args, 2, &what)?;
                    let name = str_arg(args, 0, &what)?;
                    let arguments = args.get(1).map(to_json).filter(|j| !j.is_null()).unwrap_or(json!({}));
                    finish(req("prompts/get", json!({ "name": name, "arguments": arguments }))?, what, |j| {
                        let messages = j["messages"]
                            .as_array()
                            .map(|ms| {
                                ms.iter()
                                    .map(|m| map(vec![
                                        ("role", json_to_value(&m["role"])),
                                        ("text", Value::Str(content_text(&Json::Array(vec![m["content"].clone()])).into())),
                                    ]))
                                    .collect()
                            })
                            .unwrap_or_default();
                        Ok(map(vec![("description", json_to_value(&j["description"])), ("messages", list(messages))]))
                    })
                }
                "ping" => finish(req("ping", json!({}))?, what, |_| Ok(Value::Bool(true))),
                // tools() → ai.tool values for ai.agent: each one calls the server
                "tools" => {
                    let conn2 = Arc::clone(&conn);
                    let mut p = req("tools/list", json!({}))?;
                    Box::new(move || {
                        let r = p()?;
                        Some(r.map_err(|e| io_err!("mcp.tools: {}", e)).map(|j| {
                            let tools = j["tools"].as_array().cloned().unwrap_or_default();
                            list(
                                tools
                                    .iter()
                                    .map(|t| {
                                        let name = t["name"].as_str().unwrap_or_default().to_string();
                                        let runner = NativeObjectRef::new(McpTool { conn: Arc::clone(&conn2), name: name.clone() });
                                        map(vec![
                                            ("name", Value::Str(name.into())),
                                            ("description", json_to_value(&t["description"])),
                                            ("parameters", json_to_value(&t["inputSchema"])),
                                            ("run", Value::bound(runner, "run")),
                                        ])
                                    })
                                    .collect(),
                            )
                        }))
                    })
                }
                _ => return Err(String::new()),
            })
        })();
        match r {
            Err(e) if e.is_empty() => None,
            r => Some(r),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["listTools", "callTool", "listResources", "readResource", "listPrompts", "getPrompt", "ping", "tools", "close"]
    }
}

/// One server tool as a callable `run(args)` for `ai.agent`.
struct McpTool {
    conn: Arc<Conn>,
    name: String,
}

impl FoguObject for McpTool {
    fn name(&self) -> &str {
        "McpTool"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "name").then(|| Value::Str(self.name.clone().into()))
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "run" {
            return None;
        }
        let arguments = args.first().map(to_json).filter(|j| !j.is_null()).unwrap_or(json!({}));
        Some(
            self.conn
                .request("tools/call", json!({ "name": self.name, "arguments": arguments }))
                .map(|p| finish(p, format!("mcp tool {}", self.name), tool_value))
                .map_err(|e| io_err!("mcp tool {}: {}", self.name, e)),
        )
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["run"]
    }
}
