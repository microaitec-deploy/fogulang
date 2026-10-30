//! The HTTP client of `fogu:http`: `get/post/put/patch/delete/head`,
//! `request({...})`, file uploads (multipart) and downloads.
//!
//! ```fogu
//! import http from "fogu:http"
//!
//! const r = http.get("https://api.example.com/users", { query: { page: 2 }, headers: { authorization: "Bearer " + token } })?
//! print(r.status, r.ok, r.json()?)
//! http.post(url, { name: "Ana" })?                        // a map is sent as JSON
//! http.post(url, nil, { form: { user: "ana", pass: "x" } })?   // application/x-www-form-urlencoded
//! http.post(url, nil, { multipart: {                         // multipart/form-data
//!     title: "report",
//!     file: http.file("report.pdf"),                         // streamed from disk
//!     avatar: { data: bytes, filename: "a.png", type: "image/png" },
//!     tags: ["a", "b"],                                      // a repeated field
//! } })?
//! http.download("https://example.com/big.iso", "big.iso")?   // straight to disk
//! ```
//!
//! Options: `headers`, `query`, `json`, `form`, `multipart`, `body`,
//! `timeout`, `redirects` (default true), `basicAuth: [user, pass]`,
//! `bearer`. Error statuses (4xx/5xx) are responses (`r.ok` is false);
//! network failures are error values.

use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::time::Duration;

use crate::runtime::value::Value;

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{bytes_of, duration_arg, list, max_args, str_arg};

/// A file to upload, read when the request is sent (`http.file(path, { filename, type })`).
pub struct FileValue {
    path: String,
    filename: String,
    mime: String,
}

impl FoguObject for FileValue {
    fn name(&self) -> &str {
        "HttpFile"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "path" => Some(Value::Str(self.path.clone().into())),
            "filename" => Some(Value::Str(self.filename.clone().into())),
            "type" => Some(Value::Str(self.mime.clone().into())),
            _ => None,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "txt" | "log" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "wav" => "audio/wav",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    }
}

/// `http.file(path, { filename, type })`
pub fn file(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "http.file")?;
    let path = str_arg(a, 0, "http.file")?;
    if !std::path::Path::new(&path).is_file() {
        return Err(io_err!("http.file: {} not found", path));
    }
    let mut filename = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let mut mime = mime_of(&path).to_string();
    if let Some(Value::Map(m)) = a.get(1) {
        let m = m.borrow();
        if let Some(n) = m.get("filename") {
            filename = n.to_display_string();
        }
        if let Some(t) = m.get("type") {
            mime = t.to_display_string();
        }
    }
    Ok(NativeObjectRef::new(FileValue { path, filename, mime }).to_value())
}

// ---------------------------------------------------------------------
// the request, as plain (Send) data
// ---------------------------------------------------------------------

enum Part {
    Text(String, String),
    File { field: String, path: String, filename: String, mime: String },
    Bytes { field: String, data: Vec<u8>, filename: String, mime: String },
}

enum Body {
    None,
    Raw { data: Vec<u8>, content_type: &'static str },
    Form(Vec<(String, String)>),
    Multipart(Vec<Part>),
}

struct Plan {
    method: reqwest::Method,
    url: String,
    headers: Vec<(String, String)>,
    query: Vec<(String, String)>,
    body: Body,
    timeout: Option<Duration>,
    redirects: bool,
    basic: Option<(String, Option<String>)>,
    bearer: Option<String>,
    /// download to this path instead of reading the body
    save_to: Option<String>,
}

fn pairs(v: &Value, what: &str) -> Result<Vec<(String, String)>, String> {
    match v {
        Value::Nil => Ok(Vec::new()),
        Value::Map(m) => {
            let mut out = Vec::new();
            // alphabetical, like json.stringify: the same request every time
            let m = m.borrow();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for (k, v) in keys.into_iter().map(|k| (k, &m[k])) {
                match v {
                    Value::Nil => {}
                    Value::List(l) => out.extend(l.borrow().iter().map(|x| (k.clone(), x.to_display_string()))),
                    v => out.push((k.clone(), v.to_display_string())),
                }
            }
            Ok(out)
        }
        other => Err(format!("http: '{}' must be a map, got '{}'", what, other.type_name())),
    }
}

fn multipart_parts(v: &Value) -> Result<Vec<Part>, String> {
    let Value::Map(m) = v else { return Err(format!("http: 'multipart' must be a map, got '{}'", v.type_name())) };
    let mut out = Vec::new();
    fn one(field: &str, v: &Value, out: &mut Vec<Part>) -> Result<(), String> {
        match v {
            Value::Nil => {}
            Value::Object(o) if o.name() == "HttpFile" => {
                let inner = o.inner();
                let b = inner.borrow();
                let f = b.as_any().downcast_ref::<FileValue>().expect("HttpFile");
                out.push(Part::File { field: field.into(), path: f.path.clone(), filename: f.filename.clone(), mime: f.mime.clone() });
            }
            // { data, filename, type }
            Value::Map(m) if m.borrow().contains_key("data") => {
                let m = m.borrow();
                let data = bytes_of(m.get("data").unwrap_or(&Value::Nil), "multipart data")?;
                let filename = m.get("filename").map(|v| v.to_display_string()).unwrap_or_else(|| field.to_string());
                let mime = m.get("type").map(|v| v.to_display_string()).unwrap_or_else(|| mime_of(&filename).into());
                out.push(Part::Bytes { field: field.into(), data, filename, mime });
            }
            Value::List(l) => {
                for x in l.borrow().iter() {
                    one(field, x, out)?;
                }
            }
            Value::Map(_) => out.push(Part::Text(field.into(), crate::native::json::stringify_value(v))),
            v => out.push(Part::Text(field.into(), v.to_display_string())),
        }
        Ok(())
    }
    let m = m.borrow();
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    for k in keys {
        one(k, &m[k], &mut out)?;
    }
    Ok(out)
}

/// `(method, url, body?, options?)` → the plan.
fn plan(method: &str, url: &Value, body: Option<&Value>, o: Option<&Value>) -> Result<Plan, String> {
    let what = format!("http.{}", method.to_ascii_lowercase());
    let url = match url {
        Value::Str(s) => s.clone(),
        other => return Err(format!("{}: url must be str, got '{}'", what, other.type_name())),
    };
    let method = reqwest::Method::from_bytes(method.to_ascii_uppercase().as_bytes()).map_err(|e| format!("http: bad method {}: {}", method, e))?;
    let mut p = Plan {
        method,
        url: url.into_string(),
        headers: Vec::new(),
        query: Vec::new(),
        body: Body::None,
        timeout: None,
        redirects: true,
        basic: None,
        bearer: None,
        save_to: None,
    };
    let raw_body = |v: &Value| -> Result<Body, String> {
        Ok(match v {
            Value::Nil => Body::None,
            Value::Str(s) => Body::Raw { data: s.as_bytes().to_vec(), content_type: "text/plain; charset=utf-8" },
            Value::Map(_) | Value::List(_) => Body::Raw { data: crate::native::json::stringify_value(v).into_bytes(), content_type: "application/json" },
            Value::Int(_) | Value::Float(_) | Value::Bool(_) => Body::Raw { data: v.to_display_string().into_bytes(), content_type: "text/plain; charset=utf-8" },
            other => Body::Raw { data: bytes_of(other, &what)?, content_type: "application/octet-stream" },
        })
    };
    if let Some(b) = body {
        p.body = raw_body(b)?;
    }
    match o {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            let m = m.borrow();
            const KNOWN: &[&str] =
                &["headers", "query", "json", "form", "multipart", "body", "timeout", "redirects", "basicAuth", "bearer", "method", "url"];
            for k in m.keys() {
                if !KNOWN.contains(&k.as_str()) {
                    return Err(format!("{}: unknown option '{}' ({})", what, k, KNOWN.join(" ")));
                }
            }
            if let Some(h) = m.get("headers") {
                p.headers = pairs(h, "headers")?;
            }
            if let Some(q) = m.get("query") {
                p.query = pairs(q, "query")?;
            }
            if let Some(b) = m.get("body") {
                p.body = raw_body(b)?;
            }
            if let Some(j) = m.get("json") {
                p.body = Body::Raw { data: crate::native::json::stringify_value(j).into_bytes(), content_type: "application/json" };
            }
            if let Some(f) = m.get("form") {
                p.body = Body::Form(pairs(f, "form")?);
            }
            if let Some(mp) = m.get("multipart") {
                p.body = Body::Multipart(multipart_parts(mp)?);
            }
            if let Some(t) = m.get("timeout") {
                p.timeout = Some(duration_arg(t, &format!("{}: timeout", what))?);
            }
            if let Some(r) = m.get("redirects") {
                p.redirects = r.is_truthy();
            }
            if let Some(Value::List(l)) = m.get("basicAuth") {
                let l = l.borrow();
                p.basic = Some((l.first().map(|v| v.to_display_string()).unwrap_or_default(), l.get(1).map(|v| v.to_display_string())));
            }
            if let Some(t) = m.get("bearer") {
                p.bearer = Some(t.to_display_string());
            }
        }
        Some(other) => return Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
    Ok(p)
}

/// The response, as plain data.
struct Got {
    status: u16,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    saved: Option<u64>,
}

fn no_redirect_client() -> &'static reqwest::blocking::Client {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(None)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("cannot build the HTTP client")
    })
}

fn send(p: Plan) -> Result<Got, String> {
    let what = format!("http.{}", p.method.as_str().to_ascii_lowercase());
    let client = if p.redirects { crate::native::http::blocking_client() } else { no_redirect_client() };
    let mut req = client.request(p.method, &p.url);
    if !p.query.is_empty() {
        req = req.query(&p.query);
    }
    if let Some(t) = p.timeout {
        req = req.timeout(t);
    }
    if let Some((u, pw)) = p.basic {
        req = req.basic_auth(u, pw);
    }
    if let Some(t) = p.bearer {
        req = req.bearer_auth(t);
    }
    let has_ct = p.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    req = match p.body {
        Body::None => req,
        Body::Raw { data, content_type } => {
            let r = if has_ct { req } else { req.header("content-type", content_type) };
            r.body(data)
        }
        Body::Form(f) => req.form(&f),
        Body::Multipart(parts) => {
            let mut form = reqwest::blocking::multipart::Form::new();
            for part in parts {
                form = match part {
                    Part::Text(k, v) => form.text(k, v),
                    Part::File { field, path, filename, mime } => {
                        let part = reqwest::blocking::multipart::Part::file(&path)
                            .map_err(|e| io_err!("{}: {}: {}", what, path, e))?
                            .file_name(filename)
                            .mime_str(&mime)
                            .map_err(|e| format!("{}: invalid type '{}': {}", what, mime, e))?;
                        form.part(field, part)
                    }
                    Part::Bytes { field, data, filename, mime } => {
                        let part = reqwest::blocking::multipart::Part::bytes(data)
                            .file_name(filename)
                            .mime_str(&mime)
                            .map_err(|e| format!("{}: invalid type '{}': {}", what, mime, e))?;
                        form.part(field, part)
                    }
                };
            }
            req.multipart(form)
        }
    };
    for (k, v) in p.headers {
        req = req.header(k, v);
    }
    let mut resp = req.send().map_err(|e| io_err!("{}: {}", what, crate::native::http::error_chain(&e)))?;
    let status = resp.status().as_u16();
    let url = resp.url().to_string();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    if let Some(path) = p.save_to {
        if !(200..300).contains(&status) {
            return Err(io_err!("http.download: {}: status {}", url, status));
        }
        let mut f = std::fs::File::create(&path).map_err(|e| io_err!("http.download: {}: {}", path, e))?;
        let n = resp.copy_to(&mut f).map_err(|e| io_err!("http.download: {}", crate::native::http::error_chain(&e)))?;
        return Ok(Got { status, url, headers, body: Vec::new(), saved: Some(n) });
    }
    let body = resp.bytes().map_err(|e| io_err!("{}: {}", what, crate::native::http::error_chain(&e)))?.to_vec();
    Ok(Got { status, url, headers, body, saved: None })
}

fn run(p: Plan) -> Result<Value, String> {
    // off the interpreter thread: the other `fg` tasks keep going
    let g = crate::native::rt::offload(move || send(p))??;
    if let Some(n) = g.saved {
        return Ok(Value::Int(n as i64));
    }
    Ok(NativeObjectRef::new(ClientResponse { status: g.status as i64, url: g.url, headers: g.headers, body: Rc::new(g.body) }).to_value())
}

/// get/delete/head(url, options?)
pub fn no_body(method: &'static str, a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, &format!("http.{}", method.to_ascii_lowercase()))?;
    run(plan(method, &a[0], None, a.get(1))?)
}

/// post/put/patch(url, body?, options?)
pub fn with_body(method: &'static str, a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, &format!("http.{}", method.to_ascii_lowercase()))?;
    run(plan(method, &a[0], a.get(1), a.get(2))?)
}

/// request({ method = "GET", url, ...options })
pub fn request(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "http.request")?;
    let Some(Value::Map(m)) = a.first() else { return Err("http.request expects a map { method, url, ... }".into()) };
    let (method, url) = {
        let m = m.borrow();
        (m.get("method").map(|v| v.to_display_string()).unwrap_or_else(|| "GET".into()), m.get("url").cloned().unwrap_or(Value::Nil))
    };
    run(plan(&method, &url, None, a.first())?)
}

/// download(url, path, options?) → bytes written
pub fn download(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "http.download")?;
    let path = str_arg(a, 1, "http.download")?;
    let mut p = plan("GET", &a[0], None, a.get(2))?;
    p.save_to = Some(path);
    run(p)
}

/// A response of the client.
pub struct ClientResponse {
    status: i64,
    url: String,
    headers: Vec<(String, String)>,
    body: Rc<Vec<u8>>,
}

impl FoguObject for ClientResponse {
    fn name(&self) -> &str {
        "Response"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "status" => Some(Value::Int(self.status)),
            "ok" => Some(Value::Bool((200..300).contains(&self.status))),
            "url" => Some(Value::Str(self.url.clone().into())),
            "body" | "text" => Some(Value::Str(String::from_utf8_lossy(&self.body).into_owned().into())),
            // the first value of each header (lower case); `header(name)` joins repeated ones
            "headers" => {
                let mut m: HashMap<String, Value> = HashMap::new();
                for (k, v) in &self.headers {
                    m.entry(k.clone()).or_insert_with(|| Value::Str(v.clone().into()));
                }
                Some(Value::Map(Rc::new(std::cell::RefCell::new(m.into_iter().collect()))))
            }
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "json" => crate::native::json::parse_value(&String::from_utf8_lossy(&self.body)),
            "bytes" => Ok(crate::native::util::bytes_value(self.body.as_ref().clone())),
            // header(name) → all its values joined by ", " (nil when missing)
            "header" => {
                max_args(args, 1, "header")?;
                let name = str_arg(args, 0, "header")?.to_ascii_lowercase();
                let vals: Vec<&str> = self.headers.iter().filter(|(k, _)| *k == name).map(|(_, v)| v.as_str()).collect();
                Ok(if vals.is_empty() { Value::Nil } else { Value::Str(vals.join(", ").into()) })
            }
            "cookies" => Ok(list(
                self.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| Value::Str(v.clone().into())).collect(),
            )),
            other => Err(format!("method '{}' does not exist on Response (json bytes header cookies)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["json", "bytes", "header", "cookies"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
