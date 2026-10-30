//! `fogu:ai` module — LLM client over HTTP: chat, streaming, tools (function
//! calling), agents, model listing and embeddings. Speaks three protocols:
//! OpenAI chat completions (OpenAI, Groq, OpenRouter, DeepSeek, Mistral, xAI,
//! Together, Fireworks, Perplexity, Ollama, LM Studio, any compatible server),
//! the Anthropic Messages API and the Google Gemini API.
//!
//! ```fogu
//! import ai from "fogu:ai"
//!
//! const claude = ai.client({ provider: "anthropic" })?     // $ANTHROPIC_API_KEY
//! print(claude.chat("Say hi in Portuguese")?.text)
//!
//! const s = claude.stream({ system: "Be brief", messages: [{ role: "user", content: "A haiku" }] })?
//! while true { const t = s.next()?; if t == nil { break }; print(t) }
//!
//! const weather = { name: "weather", description: "Current weather of a city",
//!     parameters: { type: "object", properties: { city: { type: "string" } }, required: ["city"] },
//!     run: (args) => ({ city: args.city, celsius: 21 }) }
//! const agent = ai.agent({ client: claude, instructions: "You are a travel assistant", tools: [weather] })
//! print(agent.run("Should I take a coat to Lisbon?")?.text)
//! ```
//!
//! Requests run on a background thread and wait cooperatively (other `fg`
//! tasks keep running). HTTP and provider failures are `error` values.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value as J};

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use super::json::{parse_value, stringify_value};
use super::object::{FoguObject, NativeObjectRef, Poller};
use super::util::{duration_arg, list, map, max_args, nfn_var, str_arg, thread_poller};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("client".into(), nfn_var("client", 0, client));
    e.insert("agent".into(), nfn_var("agent", 1, agent));
    e.insert("tool".into(), nfn_var("tool", 3, tool));
    e.insert(
        "providers".into(),
        super::util::str_list(PRESETS.iter().map(|p| p.0.to_string()).chain(["compatible".to_string()])),
    );
    ModuleValue {
        name: "fogu:ai".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Format {
    OpenAi,
    Anthropic,
    Gemini,
}

/// name, format, base url, key variable, default chat model, default embedding model
type Preset = (&'static str, Format, &'static str, &'static str, Option<&'static str>, Option<&'static str>);

const PRESETS: &[Preset] = &[
    ("openai", Format::OpenAi, "https://api.openai.com/v1", "OPENAI_API_KEY", Some("gpt-4o-mini"), Some("text-embedding-3-small")),
    ("anthropic", Format::Anthropic, "https://api.anthropic.com", "ANTHROPIC_API_KEY", Some("claude-sonnet-5-5"), None),
    ("gemini", Format::Gemini, "https://generativelanguage.googleapis.com", "GEMINI_API_KEY", Some("gemini-2.5-flash"), Some("gemini-embedding-001")),
    ("groq", Format::OpenAi, "https://api.groq.com/openai/v1", "GROQ_API_KEY", None, None),
    ("openrouter", Format::OpenAi, "https://openrouter.ai/api/v1", "OPENROUTER_API_KEY", None, None),
    ("deepseek", Format::OpenAi, "https://api.deepseek.com/v1", "DEEPSEEK_API_KEY", Some("deepseek-chat"), None),
    ("mistral", Format::OpenAi, "https://api.mistral.ai/v1", "MISTRAL_API_KEY", None, Some("mistral-embed")),
    ("xai", Format::OpenAi, "https://api.x.ai/v1", "XAI_API_KEY", None, None),
    ("together", Format::OpenAi, "https://api.together.xyz/v1", "TOGETHER_API_KEY", None, None),
    ("fireworks", Format::OpenAi, "https://api.fireworks.ai/inference/v1", "FIREWORKS_API_KEY", None, None),
    ("perplexity", Format::OpenAi, "https://api.perplexity.ai", "PERPLEXITY_API_KEY", None, None),
    ("ollama", Format::OpenAi, "http://127.0.0.1:11434/v1", "", None, None),
    ("lmstudio", Format::OpenAi, "http://127.0.0.1:1234/v1", "", None, None),
];

#[derive(Clone, Debug)]
struct Cfg {
    provider: String,
    format: Format,
    base: String,
    key: Option<String>,
    model: Option<String>,
    embed_model: Option<String>,
    timeout: Duration,
    headers: Vec<(String, String)>,
    max_tokens: Option<u64>,
}

fn get_s(m: &HashMap<String, Value>, k: &str) -> Option<String> {
    match m.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn env(k: &str) -> Option<String> {
    if k.is_empty() {
        return None;
    }
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// `ai.client(config?)`: `{ provider, model, apiKey, baseUrl, timeout, headers, maxTokens }`,
/// or a string `"provider"` / `"provider/model"`. Default provider: openai.
fn parse_cfg(arg: Option<&Value>) -> Result<Cfg, String> {
    let mut m: HashMap<String, Value> = match arg {
        None | Some(Value::Nil) => HashMap::new(),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(Value::Str(s)) => {
            let mut m = HashMap::new();
            let (p, model) = s.split_once('/').map_or((s.as_str(), None), |(a, b)| (a, Some(b)));
            m.insert("provider".to_string(), Value::Str(p.to_string()));
            if let Some(model) = model {
                m.insert("model".to_string(), Value::Str(model.to_string()));
            }
            m
        }
        Some(other) => return Err(format!("ai.client expects a map or a provider name, got '{}'", other.type_name())),
    };
    let provider = get_s(&m, "provider").unwrap_or_else(|| "openai".into()).to_lowercase();
    let preset = PRESETS.iter().find(|p| p.0 == provider);
    let (format, base, key_env, model, embed) = match (preset, provider.as_str()) {
        (Some(p), _) => (p.1, p.2.to_string(), p.3, p.4, p.5),
        (None, "compatible" | "openai-compatible") => (
            Format::OpenAi,
            get_s(&m, "baseUrl").ok_or("ai.client: provider 'compatible' needs 'baseUrl'")?,
            "",
            None,
            None,
        ),
        _ => {
            let names: Vec<&str> = PRESETS.iter().map(|p| p.0).collect();
            return Err(format!("ai.client: unknown provider '{}' ({}, compatible)", provider, names.join(", ")));
        }
    };
    let key = get_s(&m, "apiKey")
        .or_else(|| env(key_env))
        .or_else(|| if provider == "gemini" { env("GOOGLE_API_KEY") } else { None });
    if key.is_none() && !key_env.is_empty() {
        return Err(format!("ai.client: {} needs 'apiKey' (or ${})", provider, key_env));
    }
    let timeout = match m.remove("timeout") {
        None | Some(Value::Nil) => Duration::from_secs(120),
        Some(t) => duration_arg(&t, "ai.client: timeout")?,
    };
    let headers = match m.get("headers") {
        Some(Value::Map(h)) => {
            let mut v: Vec<(String, String)> =
                h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect();
            v.sort();
            v
        }
        _ => Vec::new(),
    };
    Ok(Cfg {
        format,
        base: get_s(&m, "baseUrl").unwrap_or(base).trim_end_matches('/').to_string(),
        key,
        model: get_s(&m, "model").or(model.map(String::from)),
        embed_model: get_s(&m, "embedModel").or(embed.map(String::from)),
        timeout,
        headers,
        max_tokens: match m.get("maxTokens") {
            Some(Value::Int(n)) if *n > 0 => Some(*n as u64),
            _ => None,
        },
        provider,
    })
}

// ---------------------------------------------------------------------
// Unified chat request / result (plain data: crosses threads)
// ---------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct ToolCall {
    id: String,
    name: String,
    arguments: J,
}

#[derive(Clone, Debug, Default)]
struct Msg {
    role: String,
    content: String,
    tool_calls: Vec<ToolCall>,
    tool_call_id: Option<String>,
    name: Option<String>,
}

#[derive(Clone, Debug)]
struct ToolDef {
    name: String,
    description: String,
    parameters: J,
}

#[derive(Clone, Debug, Default)]
struct ChatReq {
    model: String,
    system: Option<String>,
    messages: Vec<Msg>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    max_tokens: Option<u64>,
    stop: Vec<String>,
    tools: Vec<ToolDef>,
    /// "auto" | "none" | "required" | a tool name
    tool_choice: Option<String>,
    json: bool,
}

#[derive(Clone, Debug, Default)]
struct ChatRes {
    text: String,
    tool_calls: Vec<ToolCall>,
    finish: String,
    input_tokens: u64,
    output_tokens: u64,
    model: String,
}

fn to_json(v: &Value) -> J {
    serde_json::from_str(&stringify_value(v)).unwrap_or(J::Null)
}

fn from_json(j: &J) -> Value {
    parse_value(&j.to_string()).unwrap_or(Value::Nil)
}

fn content_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Nil) => String::new(),
        Some(Value::Str(s)) => s.clone(),
        Some(other) => stringify_value(other),
    }
}

fn parse_msg(v: &Value) -> Result<Msg, String> {
    let m = match v {
        Value::Str(s) => return Ok(Msg { role: "user".into(), content: s.clone(), ..Default::default() }),
        Value::Map(m) => m.borrow().clone(),
        other => return Err(format!("ai: a message must be a map or a str, got '{}'", other.type_name())),
    };
    let role = get_s(&m, "role").unwrap_or_else(|| "user".into());
    if !["system", "user", "assistant", "tool"].contains(&role.as_str()) {
        return Err(format!("ai: unknown message role '{}' (system, user, assistant, tool)", role));
    }
    let tool_calls = match m.get("toolCalls") {
        Some(Value::List(l)) => l
            .borrow()
            .iter()
            .map(|c| match c {
                Value::Map(c) => {
                    let c = c.borrow();
                    Ok(ToolCall {
                        id: get_s(&c, "id").unwrap_or_default(),
                        name: get_s(&c, "name").unwrap_or_default(),
                        arguments: c.get("arguments").map(to_json).unwrap_or(json!({})),
                    })
                }
                _ => Err("ai: toolCalls items must be maps".to_string()),
            })
            .collect::<Result<_, _>>()?,
        _ => Vec::new(),
    };
    Ok(Msg {
        role,
        content: content_text(m.get("content")),
        tool_calls,
        tool_call_id: get_s(&m, "toolCallId"),
        name: get_s(&m, "name"),
    })
}

fn msg_value(m: &Msg) -> Value {
    let mut pairs = vec![("role", Value::Str(m.role.clone())), ("content", Value::Str(m.content.clone()))];
    if !m.tool_calls.is_empty() {
        pairs.push(("toolCalls", list(m.tool_calls.iter().map(call_value).collect())));
    }
    if let Some(id) = &m.tool_call_id {
        pairs.push(("toolCallId", Value::Str(id.clone())));
    }
    if let Some(n) = &m.name {
        pairs.push(("name", Value::Str(n.clone())));
    }
    map(pairs)
}

fn call_value(c: &ToolCall) -> Value {
    map(vec![
        ("id", Value::Str(c.id.clone())),
        ("name", Value::Str(c.name.clone())),
        ("arguments", from_json(&c.arguments)),
    ])
}

fn parse_tool(v: &Value) -> Result<(ToolDef, Option<Value>), String> {
    let m = match v {
        Value::Map(m) => m.borrow().clone(),
        other => return Err(format!("ai: a tool must be a map, got '{}'", other.type_name())),
    };
    let name = get_s(&m, "name").ok_or("ai: a tool needs a 'name'")?;
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("ai: tool name '{}' may only use letters, digits, '_' and '-'", name));
    }
    let parameters = match m.get("parameters") {
        None | Some(Value::Nil) => json!({ "type": "object", "properties": {} }),
        Some(p @ Value::Map(_)) => to_json(p),
        Some(other) => return Err(format!("ai: tool '{}': parameters must be a JSON schema map, got '{}'", name, other.type_name())),
    };
    let run = m.get("run").filter(|v| !matches!(v, Value::Nil)).cloned();
    Ok((ToolDef { name, description: get_s(&m, "description").unwrap_or_default(), parameters }, run))
}

fn num(m: &HashMap<String, Value>, k: &str) -> Option<f64> {
    match m.get(k) {
        Some(Value::Int(n)) => Some(*n as f64),
        Some(Value::Float(f)) => Some(*f),
        _ => None,
    }
}

/// `chat("prompt")` or `chat({ messages, prompt, system, model, temperature,
/// maxTokens, topP, stop, tools, toolChoice, json })`.
fn parse_req(cfg: &Cfg, arg: Option<&Value>, what: &str) -> Result<ChatReq, String> {
    let m = match arg {
        Some(Value::Str(s)) => {
            let mut m = HashMap::new();
            m.insert("prompt".to_string(), Value::Str(s.clone()));
            m
        }
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(other) => return Err(format!("{} expects a prompt or an options map, got '{}'", what, other.type_name())),
        None => return Err(format!("{} expects a prompt or an options map", what)),
    };
    let mut messages: Vec<Msg> = match m.get("messages") {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::List(l)) => l.borrow().iter().map(parse_msg).collect::<Result<_, _>>()?,
        Some(other) => return Err(format!("{}: messages must be a list, got '{}'", what, other.type_name())),
    };
    if let Some(p) = get_s(&m, "prompt") {
        messages.push(Msg { role: "user".into(), content: p, ..Default::default() });
    }
    if messages.is_empty() {
        return Err(format!("{}: needs 'prompt' or 'messages'", what));
    }
    let tools = match m.get("tools") {
        Some(Value::List(l)) => l.borrow().iter().map(|t| parse_tool(t).map(|t| t.0)).collect::<Result<_, _>>()?,
        _ => Vec::new(),
    };
    let model = get_s(&m, "model")
        .or_else(|| cfg.model.clone())
        .ok_or_else(|| format!("{}: provider '{}' needs a 'model'", what, cfg.provider))?;
    Ok(ChatReq {
        model,
        system: get_s(&m, "system").or_else(|| get_s(&m, "instructions")),
        messages,
        temperature: num(&m, "temperature"),
        top_p: num(&m, "topP"),
        max_tokens: match m.get("maxTokens") {
            Some(Value::Int(n)) if *n > 0 => Some(*n as u64),
            _ => cfg.max_tokens,
        },
        stop: match m.get("stop") {
            Some(Value::Str(s)) => vec![s.clone()],
            Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
            _ => Vec::new(),
        },
        tools,
        tool_choice: get_s(&m, "toolChoice"),
        json: m.get("json").is_some_and(|v| v.is_truthy()),
    })
}

fn res_value(r: &ChatRes) -> Value {
    let message = Msg {
        role: "assistant".into(),
        content: r.text.clone(),
        tool_calls: r.tool_calls.clone(),
        ..Default::default()
    };
    map(vec![
        ("text", Value::Str(r.text.clone())),
        ("toolCalls", list(r.tool_calls.iter().map(call_value).collect())),
        ("finishReason", Value::Str(r.finish.clone())),
        ("model", Value::Str(r.model.clone())),
        (
            "usage",
            map(vec![
                ("inputTokens", Value::Int(r.input_tokens as i64)),
                ("outputTokens", Value::Int(r.output_tokens as i64)),
            ]),
        ),
        ("message", msg_value(&message)),
    ])
}

// ---------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------

/// System prompt: the `system` option plus any `system` messages.
fn system_text(req: &ChatReq) -> Option<String> {
    let mut parts: Vec<String> = req.system.iter().cloned().collect();
    parts.extend(req.messages.iter().filter(|m| m.role == "system").map(|m| m.content.clone()));
    if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
}

fn put(o: &mut serde_json::Map<String, J>, k: &str, v: J) {
    if !v.is_null() {
        o.insert(k.into(), v);
    }
}

fn openai_body(cfg: &Cfg, req: &ChatReq, stream: bool) -> J {
    let mut msgs = Vec::new();
    if let Some(s) = &req.system {
        msgs.push(json!({ "role": "system", "content": s }));
    }
    for m in &req.messages {
        msgs.push(match m.role.as_str() {
            "assistant" if !m.tool_calls.is_empty() => json!({
                "role": "assistant",
                "content": if m.content.is_empty() { J::Null } else { m.content.clone().into() },
                "tool_calls": m.tool_calls.iter().map(|c| json!({
                    "id": c.id, "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments.to_string() },
                })).collect::<Vec<_>>(),
            }),
            "tool" => json!({ "role": "tool", "tool_call_id": m.tool_call_id, "content": m.content }),
            role => json!({ "role": role, "content": m.content }),
        });
    }
    let mut b = serde_json::Map::new();
    b.insert("model".into(), req.model.clone().into());
    b.insert("messages".into(), msgs.into());
    put(&mut b, "temperature", req.temperature.map_or(J::Null, Into::into));
    put(&mut b, "top_p", req.top_p.map_or(J::Null, Into::into));
    let tokens_key = if cfg.provider == "openai" { "max_completion_tokens" } else { "max_tokens" };
    put(&mut b, tokens_key, req.max_tokens.map_or(J::Null, Into::into));
    if !req.stop.is_empty() {
        b.insert("stop".into(), req.stop.clone().into());
    }
    if !req.tools.is_empty() {
        b.insert(
            "tools".into(),
            req.tools
                .iter()
                .map(|t| json!({ "type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.parameters } }))
                .collect(),
        );
        if let Some(c) = &req.tool_choice {
            b.insert(
                "tool_choice".into(),
                match c.as_str() {
                    "auto" | "none" | "required" => c.clone().into(),
                    name => json!({ "type": "function", "function": { "name": name } }),
                },
            );
        }
    }
    if req.json {
        b.insert("response_format".into(), json!({ "type": "json_object" }));
    }
    if stream {
        b.insert("stream".into(), true.into());
        if cfg.provider == "openai" {
            b.insert("stream_options".into(), json!({ "include_usage": true }));
        }
    }
    J::Object(b)
}

/// Appends a block to the last message when it has the same role
/// (Anthropic and Gemini want alternating turns).
fn push_turn(out: &mut Vec<J>, role: &str, key: &str, block: J) {
    if let Some(last) = out.last_mut() {
        if last["role"] == role {
            last[key].as_array_mut().unwrap().push(block);
            return;
        }
    }
    out.push(json!({ "role": role, key: [block] }));
}

fn anthropic_body(req: &ChatReq, stream: bool) -> J {
    let mut msgs: Vec<J> = Vec::new();
    for m in &req.messages {
        match m.role.as_str() {
            "system" => {}
            "assistant" => {
                if !m.content.is_empty() {
                    push_turn(&mut msgs, "assistant", "content", json!({ "type": "text", "text": m.content }));
                }
                for c in &m.tool_calls {
                    push_turn(&mut msgs, "assistant", "content",
                        json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments }));
                }
            }
            "tool" => push_turn(&mut msgs, "user", "content", json!({
                "type": "tool_result", "tool_use_id": m.tool_call_id, "content": m.content })),
            _ => push_turn(&mut msgs, "user", "content", json!({ "type": "text", "text": m.content })),
        }
    }
    let mut system = system_text(req);
    if req.json {
        let note = "Answer with valid JSON only.".to_string();
        system = Some(system.map_or(note.clone(), |s| format!("{}\n\n{}", s, note)));
    }
    let mut b = serde_json::Map::new();
    b.insert("model".into(), req.model.clone().into());
    b.insert("max_tokens".into(), req.max_tokens.unwrap_or(4096).into());
    b.insert("messages".into(), msgs.into());
    put(&mut b, "system", system.map_or(J::Null, Into::into));
    put(&mut b, "temperature", req.temperature.map_or(J::Null, Into::into));
    put(&mut b, "top_p", req.top_p.map_or(J::Null, Into::into));
    if !req.stop.is_empty() {
        b.insert("stop_sequences".into(), req.stop.clone().into());
    }
    if !req.tools.is_empty() {
        b.insert(
            "tools".into(),
            req.tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
                .collect(),
        );
        if let Some(c) = &req.tool_choice {
            b.insert(
                "tool_choice".into(),
                match c.as_str() {
                    "auto" => json!({ "type": "auto" }),
                    "none" => json!({ "type": "none" }),
                    "required" => json!({ "type": "any" }),
                    name => json!({ "type": "tool", "name": name }),
                },
            );
        }
    }
    if stream {
        b.insert("stream".into(), true.into());
    }
    J::Object(b)
}

fn gemini_body(req: &ChatReq) -> J {
    // functionResponse needs the tool name: find it from the call id
    let names: HashMap<&str, &str> = req
        .messages
        .iter()
        .flat_map(|m| m.tool_calls.iter().map(|c| (c.id.as_str(), c.name.as_str())))
        .collect();
    let mut contents: Vec<J> = Vec::new();
    for m in &req.messages {
        match m.role.as_str() {
            "system" => {}
            "assistant" => {
                if !m.content.is_empty() {
                    push_turn(&mut contents, "model", "parts", json!({ "text": m.content }));
                }
                for c in &m.tool_calls {
                    push_turn(&mut contents, "model", "parts", json!({ "functionCall": { "name": c.name, "args": c.arguments } }));
                }
            }
            "tool" => {
                let name = m
                    .name
                    .clone()
                    .or_else(|| m.tool_call_id.as_deref().and_then(|id| names.get(id)).map(|s| s.to_string()))
                    .unwrap_or_default();
                let response = match serde_json::from_str::<J>(&m.content) {
                    Ok(o @ J::Object(_)) => o,
                    Ok(other) => json!({ "result": other }),
                    Err(_) => json!({ "result": m.content }),
                };
                push_turn(&mut contents, "user", "parts", json!({ "functionResponse": { "name": name, "response": response } }));
            }
            _ => push_turn(&mut contents, "user", "parts", json!({ "text": m.content })),
        }
    }
    let mut b = serde_json::Map::new();
    b.insert("contents".into(), contents.into());
    if let Some(s) = system_text(req) {
        b.insert("systemInstruction".into(), json!({ "parts": [{ "text": s }] }));
    }
    let mut g = serde_json::Map::new();
    put(&mut g, "temperature", req.temperature.map_or(J::Null, Into::into));
    put(&mut g, "topP", req.top_p.map_or(J::Null, Into::into));
    put(&mut g, "maxOutputTokens", req.max_tokens.map_or(J::Null, Into::into));
    if !req.stop.is_empty() {
        g.insert("stopSequences".into(), req.stop.clone().into());
    }
    if req.json {
        g.insert("responseMimeType".into(), "application/json".into());
    }
    if !g.is_empty() {
        b.insert("generationConfig".into(), J::Object(g));
    }
    if !req.tools.is_empty() {
        b.insert(
            "tools".into(),
            json!([{ "functionDeclarations": req.tools.iter().map(|t| json!({
                "name": t.name, "description": t.description, "parameters": t.parameters })).collect::<Vec<_>>() }]),
        );
        if let Some(c) = &req.tool_choice {
            let cfg = match c.as_str() {
                "auto" => json!({ "mode": "AUTO" }),
                "none" => json!({ "mode": "NONE" }),
                "required" => json!({ "mode": "ANY" }),
                name => json!({ "mode": "ANY", "allowedFunctionNames": [name] }),
            };
            b.insert("toolConfig".into(), json!({ "functionCallingConfig": cfg }));
        }
    }
    J::Object(b)
}

// ---------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------

fn args_of(v: &J) -> J {
    match v {
        J::String(s) if s.trim().is_empty() => json!({}),
        J::String(s) => serde_json::from_str(s).unwrap_or_else(|_| J::String(s.clone())),
        J::Null => json!({}),
        other => other.clone(),
    }
}

fn finish_of(raw: &str, has_tools: bool) -> String {
    if has_tools {
        return "tool_calls".into();
    }
    match raw {
        "stop" | "end_turn" | "STOP" | "stop_sequence" => "stop",
        "length" | "max_tokens" | "MAX_TOKENS" => "length",
        "tool_calls" | "tool_use" | "function_call" => "tool_calls",
        "content_filter" | "SAFETY" | "refusal" => "content_filter",
        "" => "stop",
        other => return other.to_lowercase(),
    }
    .into()
}

fn openai_parse(j: &J) -> ChatRes {
    let c = &j["choices"][0];
    let msg = &c["message"];
    let tool_calls: Vec<ToolCall> = msg["tool_calls"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| ToolCall {
                    id: t["id"].as_str().unwrap_or_default().into(),
                    name: t["function"]["name"].as_str().unwrap_or_default().into(),
                    arguments: args_of(&t["function"]["arguments"]),
                })
                .collect()
        })
        .unwrap_or_default();
    ChatRes {
        text: msg["content"].as_str().unwrap_or_default().into(),
        finish: finish_of(c["finish_reason"].as_str().unwrap_or_default(), !tool_calls.is_empty()),
        tool_calls,
        input_tokens: j["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: j["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        model: j["model"].as_str().unwrap_or_default().into(),
    }
}

fn anthropic_parse(j: &J) -> ChatRes {
    let mut r = ChatRes { model: j["model"].as_str().unwrap_or_default().into(), ..Default::default() };
    for b in j["content"].as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("text") => r.text.push_str(b["text"].as_str().unwrap_or_default()),
            Some("tool_use") => r.tool_calls.push(ToolCall {
                id: b["id"].as_str().unwrap_or_default().into(),
                name: b["name"].as_str().unwrap_or_default().into(),
                arguments: b["input"].clone(),
            }),
            _ => {}
        }
    }
    r.finish = finish_of(j["stop_reason"].as_str().unwrap_or_default(), !r.tool_calls.is_empty());
    r.input_tokens = j["usage"]["input_tokens"].as_u64().unwrap_or(0);
    r.output_tokens = j["usage"]["output_tokens"].as_u64().unwrap_or(0);
    r
}

/// Gemini (also each streamed chunk): text parts and function calls.
fn gemini_parse(j: &J, call_base: usize) -> ChatRes {
    let c = &j["candidates"][0];
    let mut r = ChatRes { model: j["modelVersion"].as_str().unwrap_or_default().into(), ..Default::default() };
    for p in c["content"]["parts"].as_array().into_iter().flatten() {
        if let Some(t) = p["text"].as_str() {
            if p["thought"].as_bool() != Some(true) {
                r.text.push_str(t);
            }
        }
        if p["functionCall"].is_object() {
            r.tool_calls.push(ToolCall {
                id: format!("call_{}", call_base + r.tool_calls.len() + 1),
                name: p["functionCall"]["name"].as_str().unwrap_or_default().into(),
                arguments: if p["functionCall"]["args"].is_null() { json!({}) } else { p["functionCall"]["args"].clone() },
            });
        }
    }
    r.finish = c["finishReason"].as_str().unwrap_or_default().into();
    r.input_tokens = j["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0);
    r.output_tokens = j["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0);
    r
}

// ---------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------

fn http(cfg: &Cfg, stream: bool) -> Result<reqwest::blocking::Client, String> {
    let mut b = reqwest::blocking::Client::builder()
        .user_agent(concat!("fogu-ai/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30));
    // a stream may last longer than the request timeout
    b = if stream { b.timeout(None) } else { b.timeout(cfg.timeout) };
    b.build().map_err(|e| io_err!("ai: {}", e))
}

fn authed(cfg: &Cfg, rb: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
    let mut rb = match (cfg.format, &cfg.key) {
        (Format::OpenAi, Some(k)) => rb.bearer_auth(k),
        (Format::Anthropic, Some(k)) => rb.header("x-api-key", k),
        (Format::Gemini, Some(k)) => rb.header("x-goog-api-key", k),
        (_, None) => rb,
    };
    if cfg.format == Format::Anthropic {
        rb = rb.header("anthropic-version", "2023-06-01");
    }
    for (k, v) in &cfg.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    rb
}

fn error_text(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<J>(body) {
        for c in [&v["error"]["message"], &v["error"], &v["message"], &v[0]["error"]["message"]] {
            if let Some(s) = c.as_str() {
                return s.to_string();
            }
        }
    }
    let t = body.trim();
    if t.chars().count() > 300 { format!("{}…", t.chars().take(300).collect::<String>()) } else { t.to_string() }
}

fn check(cfg: &Cfg, resp: reqwest::blocking::Response) -> Result<reqwest::blocking::Response, String> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().unwrap_or_default();
    Err(io_err!("ai: {} error ({}): {}", cfg.provider, status.as_u16(), error_text(&body)))
}

fn chat_url(cfg: &Cfg, req: &ChatReq, stream: bool) -> String {
    match cfg.format {
        Format::OpenAi => format!("{}/chat/completions", cfg.base),
        Format::Anthropic => format!("{}/v1/messages", cfg.base),
        Format::Gemini if stream => format!("{}/v1beta/models/{}:streamGenerateContent?alt=sse", cfg.base, req.model),
        Format::Gemini => format!("{}/v1beta/models/{}:generateContent", cfg.base, req.model),
    }
}

fn chat_body(cfg: &Cfg, req: &ChatReq, stream: bool) -> J {
    match cfg.format {
        Format::OpenAi => openai_body(cfg, req, stream),
        Format::Anthropic => anthropic_body(req, stream),
        Format::Gemini => gemini_body(req),
    }
}

fn do_chat(cfg: &Cfg, req: &ChatReq) -> Result<ChatRes, String> {
    let rb = http(cfg, false)?.post(chat_url(cfg, req, false)).json(&chat_body(cfg, req, false));
    let resp = check(cfg, authed(cfg, rb).send().map_err(|e| io_err!("ai: {}: {}", cfg.provider, e))?)?;
    let j: J = resp.json().map_err(|e| io_err!("ai: {}: invalid response: {}", cfg.provider, e))?;
    let mut r = match cfg.format {
        Format::OpenAi => openai_parse(&j),
        Format::Anthropic => anthropic_parse(&j),
        Format::Gemini => {
            let mut r = gemini_parse(&j, 0);
            r.finish = finish_of(&r.finish, !r.tool_calls.is_empty());
            r
        }
    };
    if r.model.is_empty() {
        r.model = req.model.clone();
    }
    Ok(r)
}

// ---------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------

enum Ev {
    Started,
    Text(String),
    Done(ChatRes),
    Fail(String),
}

/// Accumulates streamed events into the final result.
#[derive(Default)]
struct Acc {
    res: ChatRes,
    /// OpenAI: tool calls by index; Anthropic: by content block index.
    calls: Vec<(usize, ToolCall, String)>,
    raw_finish: String,
}

impl Acc {
    fn call(&mut self, index: usize) -> &mut (usize, ToolCall, String) {
        if let Some(i) = self.calls.iter().position(|c| c.0 == index) {
            return &mut self.calls[i];
        }
        self.calls.push((index, ToolCall::default(), String::new()));
        self.calls.last_mut().unwrap()
    }

    /// One SSE `data:` payload; returns the text delta, if any.
    fn feed(&mut self, format: Format, j: &J) -> Option<String> {
        match format {
            Format::OpenAi => {
                if let Some(m) = j["model"].as_str() {
                    self.res.model = m.into();
                }
                if j["usage"].is_object() {
                    self.res.input_tokens = j["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
                    self.res.output_tokens = j["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                }
                let c = &j["choices"][0];
                if let Some(f) = c["finish_reason"].as_str() {
                    self.raw_finish = f.into();
                }
                for t in c["delta"]["tool_calls"].as_array().into_iter().flatten() {
                    let idx = t["index"].as_u64().unwrap_or(0) as usize;
                    let entry = self.call(idx);
                    if let Some(id) = t["id"].as_str() {
                        entry.1.id = id.into();
                    }
                    if let Some(n) = t["function"]["name"].as_str() {
                        entry.1.name.push_str(n);
                    }
                    if let Some(a) = t["function"]["arguments"].as_str() {
                        entry.2.push_str(a);
                    }
                }
                c["delta"]["content"].as_str().filter(|s| !s.is_empty()).map(String::from)
            }
            Format::Anthropic => match j["type"].as_str().unwrap_or_default() {
                "message_start" => {
                    self.res.model = j["message"]["model"].as_str().unwrap_or_default().into();
                    self.res.input_tokens = j["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                    None
                }
                "content_block_start" if j["content_block"]["type"] == "tool_use" => {
                    let idx = j["index"].as_u64().unwrap_or(0) as usize;
                    let b = &j["content_block"];
                    let entry = self.call(idx);
                    entry.1.id = b["id"].as_str().unwrap_or_default().into();
                    entry.1.name = b["name"].as_str().unwrap_or_default().into();
                    None
                }
                "content_block_delta" => {
                    let d = &j["delta"];
                    match d["type"].as_str() {
                        Some("text_delta") => d["text"].as_str().map(String::from),
                        Some("input_json_delta") => {
                            let idx = j["index"].as_u64().unwrap_or(0) as usize;
                            let part = d["partial_json"].as_str().unwrap_or_default().to_string();
                            self.call(idx).2.push_str(&part);
                            None
                        }
                        _ => None,
                    }
                }
                "message_delta" => {
                    if let Some(s) = j["delta"]["stop_reason"].as_str() {
                        self.raw_finish = s.into();
                    }
                    if let Some(n) = j["usage"]["output_tokens"].as_u64() {
                        self.res.output_tokens = n;
                    }
                    None
                }
                _ => None,
            },
            Format::Gemini => {
                let r = gemini_parse(j, self.calls.len());
                for c in r.tool_calls {
                    let idx = self.calls.len();
                    self.calls.push((idx, c, String::new()));
                }
                if !r.finish.is_empty() {
                    self.raw_finish = r.finish;
                }
                if r.input_tokens > 0 {
                    self.res.input_tokens = r.input_tokens;
                }
                if r.output_tokens > 0 {
                    self.res.output_tokens = r.output_tokens;
                }
                if !r.model.is_empty() {
                    self.res.model = r.model;
                }
                Some(r.text).filter(|t| !t.is_empty())
            }
        }
    }

    fn finish(mut self) -> ChatRes {
        self.calls.sort_by_key(|c| c.0);
        self.res.tool_calls = self
            .calls
            .into_iter()
            .map(|(_, mut c, args)| {
                if !args.is_empty() {
                    c.arguments = args_of(&J::String(args));
                } else if c.arguments.is_null() {
                    c.arguments = json!({});
                }
                c
            })
            .collect();
        self.res.finish = finish_of(&self.raw_finish, !self.res.tool_calls.is_empty());
        self.res
    }
}

fn run_stream(cfg: Cfg, req: ChatReq, tx: std::sync::mpsc::Sender<Ev>) {
    let result = (|| -> Result<ChatRes, String> {
        let rb = http(&cfg, true)?.post(chat_url(&cfg, &req, true)).json(&chat_body(&cfg, &req, true));
        let resp = check(&cfg, authed(&cfg, rb).send().map_err(|e| io_err!("ai: {}: {}", cfg.provider, e))?)?;
        let _ = tx.send(Ev::Started);
        let mut acc = Acc::default();
        let mut data = String::new();
        for line in BufReader::new(resp).lines() {
            let line = line.map_err(|e| io_err!("ai: {}: stream interrupted: {}", cfg.provider, e))?;
            let line = line.trim_end_matches('\r');
            if let Some(d) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(d.trim_start());
                continue;
            }
            if !line.is_empty() || data.is_empty() {
                continue;
            }
            // blank line: end of an event
            let payload = std::mem::take(&mut data);
            if payload == "[DONE]" {
                break;
            }
            let j: J = match serde_json::from_str(&payload) {
                Ok(j) => j,
                Err(_) => continue,
            };
            if j["type"] == "error" || (j["error"].is_object() && j["choices"].is_null()) {
                return Err(io_err!("ai: {} stream error: {}", cfg.provider, error_text(&payload)));
            }
            if let Some(t) = acc.feed(cfg.format, &j) {
                if tx.send(Ev::Text(t)).is_err() {
                    return Err("stream closed".into());
                }
            }
        }
        if !data.is_empty() && data != "[DONE]" {
            if let Ok(j) = serde_json::from_str::<J>(&data) {
                if let Some(t) = acc.feed(cfg.format, &j) {
                    let _ = tx.send(Ev::Text(t));
                }
            }
        }
        let mut r = acc.finish();
        if r.model.is_empty() {
            r.model = req.model.clone();
        }
        Ok(r)
    })();
    let _ = tx.send(match result {
        Ok(r) => Ev::Done(r),
        Err(e) => Ev::Fail(e),
    });
}

struct StreamState {
    rx: Receiver<Ev>,
    text: String,
    result: Option<ChatRes>,
    error: Option<String>,
}

impl StreamState {
    /// The next event already received: `Some(Ok(Some(text)))`, end
    /// `Some(Ok(None))`, failure, or `None` when nothing arrived yet.
    fn poll(&mut self) -> Option<Result<Option<String>, String>> {
        if let Some(e) = &self.error {
            return Some(Err(e.clone()));
        }
        if self.result.is_some() {
            return Some(Ok(None));
        }
        loop {
            match self.rx.try_recv() {
                Ok(Ev::Started) => continue,
                Ok(Ev::Text(t)) => {
                    self.text.push_str(&t);
                    return Some(Ok(Some(t)));
                }
                Ok(Ev::Done(r)) => {
                    self.result = Some(r);
                    return Some(Ok(None));
                }
                Ok(Ev::Fail(e)) => {
                    self.error = Some(e.clone());
                    return Some(Err(e));
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    let e = io_err!("ai: the stream ended unexpectedly");
                    self.error = Some(e.clone());
                    return Some(Err(e));
                }
            }
        }
    }
}

fn poll_again() {
    sched::add_timer(Instant::now() + Duration::from_millis(2));
}

pub struct StreamValue {
    state: Rc<RefCell<StreamState>>,
}

impl FoguObject for StreamValue {
    fn name(&self) -> &str {
        "AiStream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let s = self.state.borrow();
        Some(match field {
            "text" => Value::Str(s.text.clone()),
            "done" => Value::Bool(s.result.is_some() || s.error.is_some()),
            "result" => s.result.as_ref().map_or(Value::Nil, res_value),
            _ => return None,
        })
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let state = Rc::clone(&self.state);
        match method {
            // next(timeout?) → the next piece of text, or nil at the end (or on timeout)
            "next" => {
                if let Err(e) = max_args(args, 1, "next") {
                    return Some(Err(e));
                }
                let deadline = match args.first() {
                    None | Some(Value::Nil) => None,
                    Some(v) => match duration_arg(v, "next") {
                        Ok(d) => Some(Instant::now() + d),
                        Err(e) => return Some(Err(e)),
                    },
                };
                Some(Ok(Box::new(move || match state.borrow_mut().poll() {
                    Some(Ok(Some(t))) => Some(Ok(Value::Str(t))),
                    Some(Ok(None)) => Some(Ok(Value::Nil)),
                    Some(Err(e)) => Some(Err(e)),
                    None if deadline.is_some_and(|d| Instant::now() >= d) => Some(Ok(Value::Nil)),
                    None => {
                        poll_again();
                        None
                    }
                })))
            }
            // collect() → waits for the end; the same result as chat
            "collect" => {
                if let Err(e) = max_args(args, 0, "collect") {
                    return Some(Err(e));
                }
                Some(Ok(Box::new(move || loop {
                    let mut s = state.borrow_mut();
                    match s.poll() {
                        Some(Ok(Some(_))) => continue,
                        Some(Ok(None)) => return Some(Ok(res_value(s.result.as_ref().unwrap()))),
                        Some(Err(e)) => return Some(Err(e)),
                        None => {
                            poll_again();
                            return None;
                        }
                    }
                })))
            }
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "collect"]
    }
}

// ---------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------

fn client(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "ai.client")?;
    let cfg = parse_cfg(args.first())?;
    Ok(NativeObjectRef::new(ClientValue { cfg }).to_value())
}

pub struct ClientValue {
    cfg: Cfg,
}

impl FoguObject for ClientValue {
    fn name(&self) -> &str {
        "AiClient"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "provider" => Value::Str(self.cfg.provider.clone()),
            "model" => self.cfg.model.clone().map_or(Value::Nil, Value::Str),
            "baseUrl" => Value::Str(self.cfg.base.clone()),
            _ => return None,
        })
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let cfg = self.cfg.clone();
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                "chat" => {
                    max_args(args, 1, "chat")?;
                    let req = parse_req(&cfg, args.first(), "chat")?;
                    thread_poller(move || do_chat(&cfg, &req), |r| Ok(res_value(&r)))
                }
                "stream" => {
                    max_args(args, 1, "stream")?;
                    let req = parse_req(&cfg, args.first(), "stream")?;
                    let (tx, rx) = channel();
                    std::thread::spawn(move || run_stream(cfg, req, tx));
                    let state = Rc::new(RefCell::new(StreamState { rx, text: String::new(), result: None, error: None }));
                    // wait until the provider accepted the request
                    Box::new(move || {
                        let mut s = state.borrow_mut();
                        match s.rx.try_recv() {
                            Ok(Ev::Started) => {}
                            Ok(Ev::Fail(e)) => return Some(Err(e)),
                            Ok(Ev::Text(t)) => s.text.push_str(&t),
                            Ok(Ev::Done(r)) => s.result = Some(r),
                            Err(TryRecvError::Empty) => {
                                poll_again();
                                return None;
                            }
                            Err(TryRecvError::Disconnected) => return Some(Err(io_err!("ai: the stream ended unexpectedly"))),
                        }
                        drop(s);
                        Some(Ok(NativeObjectRef::new(StreamValue { state: Rc::clone(&state) }).to_value()))
                    })
                }
                "models" => {
                    max_args(args, 0, "models")?;
                    thread_poller(move || list_models(&cfg), |items| {
                        Ok(list(
                            items
                                .into_iter()
                                .map(|(id, name)| map(vec![("id", Value::Str(id)), ("name", Value::Str(name))]))
                                .collect(),
                        ))
                    })
                }
                // embed(text | [texts], { model }?) → vector | [vectors]
                "embed" => {
                    max_args(args, 2, "embed")?;
                    if cfg.format == Format::Anthropic {
                        return Err("embed: anthropic has no embeddings API".into());
                    }
                    let (inputs, single) = match args.first() {
                        Some(Value::Str(s)) => (vec![s.clone()], true),
                        Some(Value::List(l)) => (l.borrow().iter().map(|v| v.to_display_string()).collect(), false),
                        _ => return Err("embed expects a str or a list of str".into()),
                    };
                    let model = match args.get(1) {
                        Some(Value::Map(m)) => get_s(&m.borrow(), "model"),
                        _ => None,
                    }
                    .or_else(|| cfg.embed_model.clone())
                    .ok_or_else(|| format!("embed: provider '{}' needs a 'model'", cfg.provider))?;
                    thread_poller(move || embed(&cfg, &model, &inputs), move |vecs| {
                        let mut out: Vec<Value> = vecs
                            .into_iter()
                            .map(|v| list(v.into_iter().map(Value::Float).collect()))
                            .collect();
                        Ok(if single { out.pop().unwrap_or(Value::Nil) } else { list(out) })
                    })
                }
                _ => return Ok(None),
            }))
        })();
        match r {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on AiClient (chat stream models embed)", method))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["chat", "stream", "models", "embed"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn list_models(cfg: &Cfg) -> Result<Vec<(String, String)>, String> {
    let url = match cfg.format {
        Format::OpenAi => format!("{}/models", cfg.base),
        Format::Anthropic => format!("{}/v1/models?limit=1000", cfg.base),
        Format::Gemini => format!("{}/v1beta/models?pageSize=1000", cfg.base),
    };
    let resp = check(cfg, authed(cfg, http(cfg, false)?.get(url)).send().map_err(|e| io_err!("ai: {}: {}", cfg.provider, e))?)?;
    let j: J = resp.json().map_err(|e| io_err!("ai: {}: invalid response: {}", cfg.provider, e))?;
    let items = j["data"].as_array().or_else(|| j["models"].as_array()).cloned().unwrap_or_default();
    let mut out: Vec<(String, String)> = items
        .iter()
        .map(|m| {
            let id = m["id"].as_str().or_else(|| m["name"].as_str()).unwrap_or_default();
            let id = id.strip_prefix("models/").unwrap_or(id).to_string();
            let name = m["display_name"].as_str().or_else(|| m["displayName"].as_str()).unwrap_or(&id).to_string();
            (id, name)
        })
        .collect();
    out.sort();
    Ok(out)
}

fn embed(cfg: &Cfg, model: &str, inputs: &[String]) -> Result<Vec<Vec<f64>>, String> {
    let c = http(cfg, false)?;
    let (rb, gemini) = match cfg.format {
        Format::Gemini => (
            c.post(format!("{}/v1beta/models/{}:batchEmbedContents", cfg.base, model)).json(&json!({
                "requests": inputs.iter().map(|t| json!({
                    "model": format!("models/{}", model), "content": { "parts": [{ "text": t }] } })).collect::<Vec<_>>()
            })),
            true,
        ),
        _ => (c.post(format!("{}/embeddings", cfg.base)).json(&json!({ "model": model, "input": inputs })), false),
    };
    let resp = check(cfg, authed(cfg, rb).send().map_err(|e| io_err!("ai: {}: {}", cfg.provider, e))?)?;
    let j: J = resp.json().map_err(|e| io_err!("ai: {}: invalid response: {}", cfg.provider, e))?;
    let vec_of = |v: &J| v.as_array().map(|a| a.iter().filter_map(J::as_f64).collect::<Vec<f64>>()).unwrap_or_default();
    Ok(if gemini {
        j["embeddings"].as_array().into_iter().flatten().map(|e| vec_of(&e["values"])).collect()
    } else {
        let mut d: Vec<&J> = j["data"].as_array().into_iter().flatten().collect();
        d.sort_by_key(|e| e["index"].as_u64().unwrap_or(0));
        d.into_iter().map(|e| vec_of(&e["embedding"])).collect()
    })
}

// ---------------------------------------------------------------------
// Tools and agents
// ---------------------------------------------------------------------

/// `ai.tool(name, description, parameters, run?)` → a tool map.
fn tool(args: &[Value]) -> Result<Value, String> {
    max_args(args, 4, "ai.tool")?;
    let name = str_arg(args, 0, "ai.tool")?;
    let description = str_arg(args, 1, "ai.tool")?;
    let mut pairs = vec![
        ("name", Value::Str(name)),
        ("description", Value::Str(description)),
        ("parameters", args[2].clone()),
    ];
    if let Some(run) = args.get(3) {
        pairs.push(("run", run.clone()));
    }
    let v = map(pairs);
    parse_tool(&v)?;
    Ok(v)
}

/// `ai.agent({ client, model, instructions, tools, maxSteps = 10, temperature,
/// maxTokens, memory = false, onStep })`.
fn agent(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "ai.agent")?;
    let m = match &args[0] {
        Value::Map(m) => m.borrow().clone(),
        other => return Err(format!("ai.agent expects an options map, got '{}'", other.type_name())),
    };
    let cfg = match m.get("client") {
        Some(Value::Object(o)) if o.name() == "AiClient" => {
            let inner = o.inner();
            let b = inner.borrow();
            b.as_any().downcast_ref::<ClientValue>().map(|c| c.cfg.clone()).ok_or("ai.agent: invalid client")?
        }
        None | Some(Value::Nil) => parse_cfg(None)?,
        Some(other @ (Value::Map(_) | Value::Str(_))) => parse_cfg(Some(other))?,
        Some(other) => return Err(format!("ai.agent: 'client' must be an ai.client, got '{}'", other.type_name())),
    };
    let mut tools = Vec::new();
    if let Some(Value::List(l)) = m.get("tools") {
        for t in l.borrow().iter() {
            let (def, run) = parse_tool(t)?;
            let run = run.ok_or_else(|| format!("ai.agent: tool '{}' needs a 'run' function", def.name))?;
            tools.push((def, run));
        }
    }
    let model = get_s(&m, "model")
        .or_else(|| cfg.model.clone())
        .ok_or_else(|| format!("ai.agent: provider '{}' needs a 'model'", cfg.provider))?;
    Ok(NativeObjectRef::new(AgentValue {
        system: get_s(&m, "instructions").or_else(|| get_s(&m, "system")),
        model,
        tools,
        max_steps: match m.get("maxSteps") {
            Some(Value::Int(n)) if *n > 0 => *n as usize,
            _ => 10,
        },
        temperature: num(&m, "temperature"),
        max_tokens: match m.get("maxTokens") {
            Some(Value::Int(n)) if *n > 0 => Some(*n as u64),
            _ => cfg.max_tokens,
        },
        memory: m.get("memory").is_some_and(|v| v.is_truthy()),
        on_step: m.get("onStep").filter(|v| !matches!(v, Value::Nil)).cloned(),
        history: Vec::new(),
        cfg,
    })
    .to_value())
}

pub struct AgentValue {
    cfg: Cfg,
    model: String,
    system: Option<String>,
    tools: Vec<(ToolDef, Value)>,
    max_steps: usize,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    memory: bool,
    on_step: Option<Value>,
    history: Vec<Msg>,
}

impl FoguObject for AgentValue {
    fn name(&self) -> &str {
        "Agent"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "model" => Value::Str(self.model.clone()),
            "tools" => super::util::str_list(self.tools.iter().map(|t| t.0.name.clone())),
            "history" => list(self.history.iter().map(msg_value).collect()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "reset" => {
                max_args(args, 0, "reset")?;
                self.history.clear();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Agent (run reset)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["run", "reset"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Is this `Agent.run`? (handled by the interpreter: it calls Fogu tools)
pub fn is_agent_run(receiver: &NativeObjectRef, method: &str) -> bool {
    method == "run" && receiver.name() == "Agent"
}

/// Waits cooperatively for a chat request made on a background thread.
fn chat_blocking(cfg: &Cfg, req: ChatReq) -> Result<ChatRes, String> {
    let (tx, rx) = channel();
    let cfg = cfg.clone();
    std::thread::spawn(move || {
        let _ = tx.send(do_chat(&cfg, &req));
    });
    let mut out = None;
    sched::wait_until(|| {
        if out.is_none() {
            match rx.try_recv() {
                Ok(r) => out = Some(r),
                Err(TryRecvError::Empty) => poll_again(),
                Err(TryRecvError::Disconnected) => out = Some(Err(io_err!("ai: the request thread failed"))),
            }
        }
        out.is_some()
    })?;
    out.unwrap()
}

/// `agent.run(input)`: asks the model, runs the tools it calls (Fogu
/// functions), sends the results back, until it answers without tools.
pub fn agent_run(
    receiver: &NativeObjectRef,
    args: &[Value],
    interp: &Interpreter,
    env: Rc<RefCell<Environment>>,
) -> Result<Value, String> {
    max_args(args, 1, "run")?;
    // copy what is needed: the tools may use the agent while we wait
    let (cfg, model, system, tools, max_steps, temperature, max_tokens, memory, on_step, history) = {
        let inner = receiver.inner();
        let b = inner.borrow();
        let a = b.as_any().downcast_ref::<AgentValue>().ok_or("run: not an agent")?;
        (
            a.cfg.clone(),
            a.model.clone(),
            a.system.clone(),
            a.tools.clone(),
            a.max_steps,
            a.temperature,
            a.max_tokens,
            a.memory,
            a.on_step.clone(),
            if a.memory { a.history.clone() } else { Vec::new() },
        )
    };
    let mut messages = history;
    match args.first() {
        Some(Value::List(l)) => {
            for m in l.borrow().iter() {
                messages.push(parse_msg(m)?);
            }
        }
        Some(v @ (Value::Str(_) | Value::Map(_))) => messages.push(parse_msg(v)?),
        _ => return Err("run expects a prompt, a message or a list of messages".into()),
    }
    let defs: Vec<ToolDef> = tools.iter().map(|t| t.0.clone()).collect();
    let (mut input_tokens, mut output_tokens) = (0u64, 0u64);
    let mut steps: Vec<Value> = Vec::new();
    let mut last = None;
    for step in 1..=max_steps {
        let req = ChatReq {
            model: model.clone(),
            system: system.clone(),
            messages: messages.clone(),
            temperature,
            max_tokens,
            tools: defs.clone(),
            ..Default::default()
        };
        let res = chat_blocking(&cfg, req)?;
        input_tokens += res.input_tokens;
        output_tokens += res.output_tokens;
        messages.push(Msg {
            role: "assistant".into(),
            content: res.text.clone(),
            tool_calls: res.tool_calls.clone(),
            ..Default::default()
        });
        let mut calls = Vec::new();
        for c in &res.tool_calls {
            let run = tools.iter().find(|t| t.0.name == c.name).map(|t| t.1.clone());
            let out = match run {
                Some(f) => interp.call_value(f, vec![from_json(&c.arguments)], Rc::clone(&env))?,
                None => Value::Error(Rc::new(crate::runtime::value::ErrorValue {
                    message: format!("unknown tool '{}'", c.name),
                    cause: Value::Nil,
                    trace: Vec::new(),
                })),
            };
            let content = match &out {
                Value::Error(e) => format!("error: {}", e.message),
                Value::Str(s) => s.clone(),
                Value::Nil => "null".into(),
                other => stringify_value(other),
            };
            messages.push(Msg {
                role: "tool".into(),
                content,
                tool_call_id: Some(c.id.clone()),
                name: Some(c.name.clone()),
                ..Default::default()
            });
            calls.push(map(vec![
                ("id", Value::Str(c.id.clone())),
                ("name", Value::Str(c.name.clone())),
                ("arguments", from_json(&c.arguments)),
                ("result", out),
            ]));
        }
        let step_value = map(vec![
            ("step", Value::Int(step as i64)),
            ("text", Value::Str(res.text.clone())),
            ("toolCalls", list(calls)),
        ]);
        if let Some(f) = &on_step {
            interp.call_value(f.clone(), vec![step_value.clone()], Rc::clone(&env))?;
        }
        steps.push(step_value);
        let done = res.tool_calls.is_empty();
        last = Some(res);
        if done {
            break;
        }
    }
    let res = last.unwrap_or_default();
    if !res.tool_calls.is_empty() {
        return Err(io_err!("ai: the agent did not finish within {} steps", max_steps));
    }
    if memory {
        let inner = receiver.inner();
        let mut b = inner.borrow_mut();
        if let Some(a) = b.as_any_mut().downcast_mut::<AgentValue>() {
            a.history = messages.clone();
        }
    }
    Ok(map(vec![
        ("text", Value::Str(res.text.clone())),
        ("finishReason", Value::Str(res.finish.clone())),
        ("steps", list(steps)),
        ("messages", list(messages.iter().map(msg_value).collect())),
        (
            "usage",
            map(vec![
                ("inputTokens", Value::Int(input_tokens as i64)),
                ("outputTokens", Value::Int(output_tokens as i64)),
            ]),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with_tool_turn() -> ChatReq {
        ChatReq {
            model: "m".into(),
            system: Some("be nice".into()),
            messages: vec![
                Msg { role: "user".into(), content: "2+3?".into(), ..Default::default() },
                Msg {
                    role: "assistant".into(),
                    tool_calls: vec![ToolCall { id: "c1".into(), name: "add".into(), arguments: json!({"a":2,"b":3}) }],
                    ..Default::default()
                },
                Msg { role: "tool".into(), content: "5".into(), tool_call_id: Some("c1".into()), ..Default::default() },
            ],
            tools: vec![ToolDef { name: "add".into(), description: "adds".into(), parameters: json!({"type":"object"}) }],
            tool_choice: Some("auto".into()),
            ..Default::default()
        }
    }

    fn cfg(p: &str) -> Cfg {
        parse_cfg(Some(&Value::Str(format!("{}/m", p)))).unwrap_or_else(|_| Cfg {
            provider: p.into(),
            format: Format::OpenAi,
            base: String::new(),
            key: None,
            model: None,
            embed_model: None,
            timeout: Duration::from_secs(1),
            headers: vec![],
            max_tokens: None,
        })
    }

    #[test]
    fn openai_request() {
        let b = openai_body(&cfg("ollama"), &req_with_tool_turn(), false);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["arguments"], "{\"a\":2,\"b\":3}");
        assert_eq!(b["messages"][3]["tool_call_id"], "c1");
        assert_eq!(b["tools"][0]["function"]["name"], "add");
        assert_eq!(b["tool_choice"], "auto");
    }

    #[test]
    fn anthropic_request() {
        let b = anthropic_body(&req_with_tool_turn(), true);
        assert_eq!(b["system"], "be nice");
        assert_eq!(b["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(b["messages"][2]["role"], "user");
        assert_eq!(b["messages"][2]["content"][0]["tool_use_id"], "c1");
        assert_eq!(b["tool_choice"]["type"], "auto");
        assert_eq!(b["stream"], true);
    }

    #[test]
    fn gemini_request() {
        let b = gemini_body(&req_with_tool_turn());
        assert_eq!(b["systemInstruction"]["parts"][0]["text"], "be nice");
        assert_eq!(b["contents"][1]["role"], "model");
        assert_eq!(b["contents"][1]["parts"][0]["functionCall"]["name"], "add");
        assert_eq!(b["contents"][2]["parts"][0]["functionResponse"]["name"], "add");
        assert_eq!(b["contents"][2]["parts"][0]["functionResponse"]["response"]["result"], 5);
    }

    #[test]
    fn responses() {
        let o = openai_parse(&json!({"model":"g","choices":[{"message":{"content":null,"tool_calls":[
            {"id":"x","function":{"name":"add","arguments":"{\"a\":1}"}}]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":3,"completion_tokens":4}}));
        assert_eq!((o.finish.as_str(), o.tool_calls[0].arguments["a"].as_i64()), ("tool_calls", Some(1)));
        let a = anthropic_parse(&json!({"content":[{"type":"text","text":"hi"}],"stop_reason":"end_turn",
            "usage":{"input_tokens":1,"output_tokens":2}}));
        assert_eq!((a.text.as_str(), a.finish.as_str(), a.output_tokens), ("hi", "stop", 2));
        let mut acc = Acc::default();
        for e in [
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t","name":"add"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"7}"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}),
        ] {
            acc.feed(Format::Anthropic, &e);
        }
        let r = acc.finish();
        assert_eq!(r.tool_calls[0].arguments, json!({"a": 7}));
        assert_eq!(r.finish, "tool_calls");
    }
}
