//! `fogu:topcoat` — server-rendered reactive pages in the model of Topcoat
//! (https://github.com/tokio-rs/topcoat), for `fogu:web` apps.
//!
//! ```fogu
//! import web from "fogu:web"
//! import tc from "fogu:topcoat"
//! const h = tc.h
//! const ui = tc.ui
//!
//! const results = tc.shard("results", fn(ctx, props) {
//!     return h("ul", nil, find(props.query).map(fn(p) { return h("li", nil, p) }))
//! })
//! const app = web.app()
//! app.get("/", fn(ctx) {
//!     const query = tc.signal(ctx, "query", "")
//!     return tc.page({ title: "Search", body: [
//!         ui.input({ placeholder: "Search", "@input": "query.set(e.target.value)" }),
//!         results.render(ctx, { query: tc.expr("query.get()", "") }),
//!     ]})
//! })
//! app.listen(3000)
//! ```
//!
//! Pages are HTML built on the server (`h`, `raw`, `page`, the `ui` components).
//! Signals hold browser state; `@event` handlers, `:attr` bindings and live text
//! update in the browser without a round-trip. Shards re-render on the server
//! when their inputs change; procedures are server functions called from the
//! browser. The browser runtime (`topcoat/runtime.js`) and the UI stylesheet
//! (`topcoat/ui.css`) are served by every `fogu:web` app under `/_tc/`.

use std::cell::{Cell, RefCell};
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{map, max_args, nfn_var, str_arg};

mod kit;
mod shadcn;

pub const RUNTIME_JS: &str = include_str!("topcoat/runtime.js");
pub const UI_CSS: &str = include_str!("topcoat/ui.css");
/// shadcn-rs's stylesheet, for `tc.shadcn` (`page({ ui: "shadcn" })`).
pub const SHADCN_CSS: &str = include_str!("topcoat/shadcn.css");

/// Tailwind v4 (browser build) for `page({ tailwind: true })`.
const TAILWIND_CDN: &str = "https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4.1.11";

/// Topcoat's theme tokens as Tailwind utilities (`bg-primary`, `text-muted-foreground`,
/// `border-border`…) and its `dark` class variant.
pub const TAILWIND_THEME: &str = "@custom-variant dark (&:is(.dark *));
@theme inline {
  --font-sans: var(--font-sans);
  --font-mono: var(--font-mono);
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-muted-foreground: var(--muted-foreground);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-destructive: var(--destructive);
  --color-destructive-foreground: var(--destructive-foreground);
  --color-success: var(--success);
  --color-warning: var(--warning);
  --color-border: var(--border);
  --color-ring: var(--ring);
  --color-sidebar: var(--sidebar);
  --color-sidebar-foreground: var(--sidebar-foreground);
  --color-sidebar-accent: var(--sidebar-accent);
  --color-sidebar-border: var(--sidebar-border);
  --shadow-xs: var(--shadow-xs);
  --shadow-sm: var(--shadow-sm);
}
";

/// The `<head>` markup that loads Tailwind: `true` (the browser build, for
/// development) or the URL of a stylesheet built with the Tailwind CLI.
fn tailwind_head(v: &Value) -> String {
    match v {
        Value::Str(href) => format!("<link rel=\"stylesheet\" href=\"{}\">", escape(href)),
        v if v.is_truthy() => format!(
            "<script src=\"{}\"></script><style type=\"text/tailwindcss\">{}</style>",
            TAILWIND_CDN, TAILWIND_THEME
        ),
        _ => String::new(),
    }
}

pub fn build() -> ModuleValue {
    let mut e = HashMap::new();
    let fns: [(&str, usize, fn(&[Value]) -> Result<Value, String>); 11] = [
        ("h", 1, h),
        ("raw", 1, |a| Ok(html_value(a[0].to_display_string()))),
        ("fragment", 0, |a| Ok(html_value(children(a)))),
        ("escape", 1, |a| Ok(Value::Str(escape(&a[0].to_display_string()).into()))),
        ("page", 0, page),
        ("signal", 3, signal),
        ("expr", 1, expr),
        ("shard", 2, shard),
        ("script", 0, |_| Ok(html_value("<script src=\"/_tc/runtime.js\" defer></script>".into()))),
        ("styles", 0, |_| Ok(html_value("<link rel=\"stylesheet\" href=\"/_tc/ui.css\">".into()))),
        ("tailwind", 0, |a| Ok(html_value(tailwind_head(a.first().unwrap_or(&Value::Bool(true)))))),
    ];
    for (n, min, f) in fns {
        e.insert(n.to_string(), nfn_var(n, min, f));
    }
    e.insert("ui".to_string(), ui_module());
    e.insert("shadcn".to_string(), shadcn::module());
    e.insert("tailwindTheme".to_string(), Value::Str(TAILWIND_THEME.to_string().into()));
    ModuleValue {
        name: "fogu:topcoat".into(),
        exports: RefCell::new(e),
    }
}

// ================= HTML =================

/// Markup that is inserted as it is (not escaped).
pub struct Html(pub String);

impl FoguObject for Html {
    fn name(&self) -> &str {
        "Html"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "html" => Some(Value::Str(self.0.clone().into())),
            "len" => Some(Value::Int(self.0.len() as i64)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "str" | "toString" => Ok(Value::Str(self.0.clone().into())),
            other => Err(format!("method '{}' does not exist on Html (str)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["str"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn html_value(s: String) -> Value {
    NativeObjectRef::new(Html(s)).to_value()
}

/// The markup of an `Html` value.
pub fn as_html(v: &Value) -> Option<String> {
    let Value::Object(o) = v else { return None };
    let inner = o.inner();
    let b = inner.borrow();
    if b.name() != "Html" {
        return None;
    }
    b.as_any().downcast_ref::<Html>().map(|h| h.0.clone())
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// JSON that can sit inside a <script> element.
fn script_json(v: &Value) -> String {
    stringify_value(v).replace("</", "<\\/")
}

const VOID: [&str; 14] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

fn render_child(v: &Value, out: &mut String) {
    match v {
        Value::Nil | Value::Bool(_) => {}
        Value::Str(s) => out.push_str(&escape(s)),
        Value::List(l) => {
            for c in l.borrow().iter() {
                render_child(c, out);
            }
        }
        Value::Object(o) => {
            if let Some(h) = as_html(v) {
                out.push_str(&h);
            } else if let Some((code, initial)) = reactive(v) {
                out.push_str(&format!("<span data-tc-text=\"{}\">", escape(&code)));
                render_child(&initial, out);
                out.push_str("</span>");
            } else {
                out.push_str(&escape(&o.inner().borrow().name().to_string()));
            }
        }
        other => out.push_str(&escape(&other.to_display_string())),
    }
}

fn children(vs: &[Value]) -> String {
    let mut out = String::new();
    for v in vs {
        render_child(v, &mut out);
    }
    out
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '>' | '/' | '=' | '<' | '`'))
}

/// `@click` → `tc-on:click`, `:hidden` → `tc-bind:hidden`.
fn attr_name(k: &str) -> String {
    if let Some(ev) = k.strip_prefix('@') {
        format!("tc-on:{}", ev)
    } else if let Some(a) = k.strip_prefix(':') {
        format!("tc-bind:{}", a)
    } else {
        k.to_string()
    }
}

fn class_value(v: &Value) -> String {
    match v {
        Value::List(l) => l.borrow().iter().map(class_value).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "),
        Value::Map(m) => sorted(&m.borrow()).into_iter().filter(|(_, on)| on.is_truthy()).map(|(k, _)| k).collect::<Vec<_>>().join(" "),
        Value::Nil | Value::Bool(false) => String::new(),
        other => other.to_display_string(),
    }
}

fn render_attrs(attrs: &[(String, Value)], out: &mut String) -> Result<(), String> {
    let mut plain: Vec<(String, Value)> = Vec::new();
    for (k, v) in attrs {
        let name = attr_name(k);
        if !valid_name(&name) {
            return Err(format!("h: invalid attribute name '{}'", k));
        }
        if let Some(bound) = name.strip_prefix("tc-bind:") {
            // a signal or expression: the browser keeps it in sync, the server renders its value
            let (code, initial) = reactive(v).unwrap_or_else(|| (v.to_display_string(), Value::Nil));
            out.push_str(&format!(" {}=\"{}\"", name, escape(&code)));
            if !matches!(initial, Value::Nil) && !attrs.iter().any(|(k2, _)| k2 == bound) {
                plain.push((bound.to_string(), initial));
            }
            continue;
        }
        plain.push((name, v.clone()));
    }
    for (name, v) in plain {
        let v = reactive(&v).map(|(_, init)| init).unwrap_or(v);
        match &v {
            Value::Nil | Value::Bool(false) => {}
            Value::Bool(true) => out.push_str(&format!(" {}", name)),
            _ => {
                let s = match name.as_str() {
                    "class" => class_value(&v),
                    "style" => match &v {
                        Value::Map(m) => sorted(&m.borrow()).iter().map(|(k, v)| format!("{}: {}", k, v.to_display_string())).collect::<Vec<_>>().join("; "),
                        other => other.to_display_string(),
                    },
                    _ => match &v {
                        Value::Map(_) | Value::List(_) => stringify_value(&v),
                        other => other.to_display_string(),
                    },
                };
                out.push_str(&format!(" {}=\"{}\"", name, escape(&s)));
            }
        }
    }
    Ok(())
}

fn attrs_of(v: &Value) -> Vec<(String, Value)> {
    match v {
        Value::Map(m) => sorted(&m.borrow()),
        _ => Vec::new(),
    }
}

/// A map's entries in key order (maps are hash maps; the HTML must not vary).
fn sorted(m: &HashMap<String, Value>) -> Vec<(String, Value)> {
    let mut v: Vec<(String, Value)> = m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn element(tag: &str, attrs: &[(String, Value)], kids: &str) -> Result<String, String> {
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("h: invalid tag name '{}'", tag));
    }
    let mut out = format!("<{}", tag);
    render_attrs(attrs, &mut out)?;
    out.push('>');
    if VOID.contains(&tag.to_ascii_lowercase().as_str()) {
        return Ok(out);
    }
    out.push_str(kids);
    out.push_str(&format!("</{}>", tag));
    Ok(out)
}

/// `h(tag, attrs?, ...children)`
fn h(args: &[Value]) -> Result<Value, String> {
    let tag = str_arg(args, 0, "h")?;
    let (attrs, rest) = match args.get(1) {
        Some(Value::Map(_)) => (attrs_of(&args[1]), &args[2..]),
        Some(Value::Nil) => (Vec::new(), &args[2..]),
        _ => (Vec::new(), &args[1..]),
    };
    Ok(html_value(element(&tag, &attrs, &children(rest))?))
}

// ================= PAGES =================

/// `page({ title, head, body, lang = "en", theme = "light" | "dark" | "system", dark,
/// ui = true | "shadcn" | false, tailwind, icon, bodyClass })`
fn page(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "tc.page")?;
    let o = match args.first() {
        Some(Value::Map(m)) => m.borrow().clone(),
        None | Some(Value::Nil) => HashMap::new(),
        Some(other) => return Err(format!("tc.page: options must be a map, got '{}'", other.type_name())),
    };
    let s = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
    let lang = s("lang").unwrap_or_else(|| "en".into());
    let theme = match (s("theme"), o.get("dark").is_some_and(Value::is_truthy)) {
        (Some(t), _) => t,
        (None, true) => "dark".into(),
        (None, false) => "light".into(),
    };
    if !matches!(theme.as_str(), "light" | "dark" | "system") {
        return Err(format!("tc.page: theme must be light, dark or system, got '{}'", theme));
    }
    let mut out = format!(
        "<!DOCTYPE html><html lang=\"{}\" data-theme=\"{}\"{}><head><meta charset=\"utf-8\">",
        escape(&lang),
        theme,
        if theme == "dark" { " class=\"dark\"" } else { "" }
    );
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    // the choice made with ui.themeToggle() (or the system's), applied before the first paint
    out.push_str("<script>(function(){try{var d=document.documentElement,t=localStorage.getItem('tc-theme');if(!t&&d.dataset.theme==='system')t=matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light';if(t)d.classList.toggle('dark',t==='dark')}catch(e){}})()</script>");
    match o.get("icon") {
        Some(Value::Str(href)) => out.push_str(&format!("<link rel=\"icon\" href=\"{}\">", escape(href))),
        Some(v) if !v.is_truthy() => {}
        _ => out.push_str(&format!("<link rel=\"icon\" href=\"{}\">", kit::favicon_href())),
    }
    if let Some(t) = s("title") {
        out.push_str(&format!("<title>{}</title>", escape(&t)));
    }
    match o.get("ui") {
        Some(Value::Str(k)) if k == "shadcn" => out.push_str("<link rel=\"stylesheet\" href=\"/_tc/shadcn.css\">"),
        Some(Value::Str(k)) if k != "topcoat" => return Err(format!("tc.page: unknown ui '{}' (topcoat shadcn, or false)", k)),
        Some(v) if !v.is_truthy() => {}
        _ => out.push_str("<link rel=\"stylesheet\" href=\"/_tc/ui.css\">"),
    }
    out.push_str("<script src=\"/_tc/runtime.js\" defer></script>");
    if let Some(tw) = o.get("tailwind") {
        out.push_str(&tailwind_head(tw));
    }
    if let Some(head) = o.get("head") {
        render_child(head, &mut out);
    }
    out.push_str("</head><body");
    if let Some(c) = o.get("bodyClass") {
        out.push_str(&format!(" class=\"{}\"", escape(&class_value(c))));
    }
    out.push('>');
    if let Some(body) = o.get("body") {
        render_child(body, &mut out);
    }
    out.push_str("</body></html>");
    Ok(html_value(out))
}

// ================= SIGNALS AND EXPRESSIONS =================

/// The per-request state kept in `ctx.tc`: `{ signals, input, rerun, app }`.
pub fn ctx_state(ctx: &Value) -> Option<Rc<RefCell<HashMap<String, Value>>>> {
    let Value::Map(c) = ctx else { return None };
    if let Some(Value::Map(t)) = c.borrow().get("tc") {
        return Some(Rc::clone(t));
    }
    let t = Rc::new(RefCell::new(HashMap::from([
        ("signals".to_string(), map(vec![])),
        ("input".to_string(), map(vec![])),
        ("rerun".to_string(), Value::Bool(false)),
    ])));
    c.borrow_mut().insert("tc".into(), Value::Map(Rc::clone(&t)));
    Some(t)
}

fn sub_map(t: &Rc<RefCell<HashMap<String, Value>>>, k: &str) -> Rc<RefCell<HashMap<String, Value>>> {
    if let Some(Value::Map(m)) = t.borrow().get(k) {
        return Rc::clone(m);
    }
    let m = Rc::new(RefCell::new(HashMap::new()));
    t.borrow_mut().insert(k.to_string(), Value::Map(Rc::clone(&m)));
    m
}

const RESERVED: [&str; 10] = ["call", "navigate", "toast", "ask", "copy", "toggleTheme", "tc", "e", "event", "window"];

/// `signal(ctx, name, initial)`: the value the browser sent back (a rerun, a
/// shard request) or `initial`.
fn signal(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "tc.signal")?;
    let ctx = &args[0];
    let name = str_arg(args, 1, "tc.signal")?;
    let ident = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if !ident || RESERVED.contains(&name.as_str()) {
        return Err(format!("tc.signal: '{}' is not a valid signal name (a JavaScript identifier)", name));
    }
    let t = ctx_state(ctx).ok_or("tc.signal: the first argument must be the request ctx")?;
    let signals = sub_map(&t, "signals");
    if let Some(Value::Map(entry)) = signals.borrow().get(&name) {
        return Ok(NativeObjectRef::new(Signal { name, entry: Rc::clone(entry) }).to_value());
    }
    let input = sub_map(&t, "input");
    let v = input.borrow().get(&name).cloned().unwrap_or_else(|| args[2].clone());
    let entry = Rc::new(RefCell::new(HashMap::from([("v".to_string(), v), ("t".to_string(), Value::Bool(false))])));
    signals.borrow_mut().insert(name.clone(), Value::Map(Rc::clone(&entry)));
    Ok(NativeObjectRef::new(Signal { name, entry }).to_value())
}

struct Signal {
    name: String,
    entry: Rc<RefCell<HashMap<String, Value>>>,
}

impl Signal {
    fn peek(&self) -> Value {
        self.entry.borrow().get("v").cloned().unwrap_or(Value::Nil)
    }

    /// A read on the server: a change in the browser renders again.
    fn track(&self) -> Value {
        self.entry.borrow_mut().insert("t".into(), Value::Bool(true));
        self.peek()
    }
}

impl FoguObject for Signal {
    fn name(&self) -> &str {
        "Signal"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "name" => Some(Value::Str(self.name.clone().into())),
            "value" => Some(self.track()),
            "js" => Some(Value::Str(format!("{}.get()", self.name).into())),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "get" => Ok(self.track()),
            "peek" => Ok(self.peek()),
            other => Err(format!("method '{}' does not exist on Signal (get peek)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["get", "peek"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// `expr(js, initial?)` — a JavaScript expression evaluated in the browser.
struct Expr {
    js: String,
    initial: Value,
}

fn expr(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "tc.expr")?;
    let js = str_arg(args, 0, "tc.expr")?;
    Ok(NativeObjectRef::new(Expr { js, initial: args.get(1).cloned().unwrap_or(Value::Nil) }).to_value())
}

impl FoguObject for Expr {
    fn name(&self) -> &str {
        "Expr"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "js" => Some(Value::Str(self.js.clone().into())),
            "initial" => Some(self.initial.clone()),
            _ => None,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The name of a `tc.signal`.
fn signal_name(v: &Value) -> Option<String> {
    let Value::Object(o) = v else { return None };
    let inner = o.inner();
    let b = inner.borrow();
    b.as_any_checked()?.downcast_ref::<Signal>().map(|s| s.name.clone())
}

/// A signal or an expression → (browser code, value rendered by the server).
fn reactive(v: &Value) -> Option<(String, Value)> {
    let Value::Object(o) = v else { return None };
    let inner = o.inner();
    let b = inner.borrow();
    let any = b.as_any_checked()?;
    if let Some(s) = any.downcast_ref::<Signal>() {
        return Some((format!("{}.get()", s.name), s.peek()));
    }
    any.downcast_ref::<Expr>().map(|e| (e.js.clone(), e.initial.clone()))
}

/// `as_any` only for the object types that implement it.
trait AnyChecked {
    fn as_any_checked(&self) -> Option<&dyn std::any::Any>;
}

impl AnyChecked for dyn FoguObject {
    fn as_any_checked(&self) -> Option<&dyn std::any::Any> {
        matches!(self.name(), "Signal" | "Expr" | "Html").then(|| self.as_any())
    }
}

/// `<script data-tc-signals>` with the signals declared while rendering.
fn signals_script(signals: &Rc<RefCell<HashMap<String, Value>>>) -> String {
    if signals.borrow().is_empty() {
        return String::new();
    }
    format!("<script type=\"application/json\" data-tc-signals>{}</script>", script_json(&Value::Map(Rc::clone(signals))))
}

/// The page's signals, placed before `</body>` (or at the end of a fragment).
pub fn with_signals(html: String, ctx: &Value) -> String {
    let Some(t) = ctx_state(ctx) else { return html };
    let script = signals_script(&sub_map(&t, "signals"));
    if script.is_empty() {
        return html;
    }
    match html.rfind("</body>") {
        Some(i) => format!("{}{}{}", &html[..i], script, &html[i..]),
        None => html + &script,
    }
}

/// One frame of Topcoat's response protocol (a JSON line).
pub fn frame(pairs: Vec<(&str, Value)>) -> String {
    stringify_value(&map(pairs)) + "\n"
}

// ================= SHARDS =================

/// `shard(name, fn(ctx, props))` → Shard; `shard.render(ctx, props)` renders it
/// in a page (and registers it with the request's app).
fn shard(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "tc.shard")?;
    let name = str_arg(args, 0, "tc.shard")?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        return Err(format!("tc.shard: invalid name '{}' (letters, digits, _ - .)", name));
    }
    if !matches!(args[1], Value::Fn(_) | Value::NativeFn(_)) {
        return Err(format!("tc.shard: expected a function, got '{}'", args[1].type_name()));
    }
    Ok(NativeObjectRef::new(Shard { name, func: args[1].clone() }).to_value())
}

pub struct Shard {
    pub name: String,
    pub func: Value,
}

impl FoguObject for Shard {
    fn name(&self) -> &str {
        "TopcoatShard"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "name" => Some(Value::Str(self.name.clone().into())),
            "handler" => Some(self.func.clone()),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("method '{}' does not exist on Shard (render)", method))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["render"]
    }
}

thread_local! {
    static SHARD_IDS: Cell<u64> = const { Cell::new(1) };
}

/// `shard.render(ctx, props)` runs Fogu code, so the interpreter routes it here.
pub fn intercept(callee: &Value, args: &[Value], interp: &Interpreter, env: &Rc<RefCell<Environment>>) -> Option<Result<Value, String>> {
    let Value::BoundMethod(bm) = callee else { return None };
    let (receiver, method) = (&bm.receiver, &bm.method);
    if method != "render" || receiver.name() != "TopcoatShard" {
        return None;
    }
    let (name, func) = (receiver.get("name")?.to_display_string(), receiver.get("handler")?);
    Some(render_in_page(interp, env, &name, &func, args))
}

fn render_in_page(interp: &Interpreter, env: &Rc<RefCell<Environment>>, name: &str, func: &Value, args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "shard.render")?;
    let ctx = args.first().cloned().unwrap_or(Value::Nil);
    let t = ctx_state(&ctx).ok_or("shard.render: the first argument must be the request ctx")?;
    if let Some(Value::Int(app)) = t.borrow().get("app") {
        crate::native::web::register_shard(*app as u64, name, func.clone());
    }
    // props: fixed values, or signals/expressions the browser watches
    let mut spec = Vec::new();
    let mut values = Vec::new();
    if let Some(Value::Map(m)) = args.get(1) {
        for (k, v) in sorted(&m.borrow()).iter() {
            match reactive(v) {
                Some((code, initial)) => {
                    spec.push((k.clone(), map(vec![("e", Value::Str(code.into()))])));
                    values.push((k.clone(), initial));
                }
                None => {
                    spec.push((k.clone(), map(vec![("v", v.clone())])));
                    values.push((k.clone(), v.clone()));
                }
            }
        }
    }
    let props = Value::Map(Rc::new(RefCell::new(values.into_iter().collect())));
    let spec = Value::Map(Rc::new(RefCell::new(spec.into_iter().collect())));
    let inner = match render_shard(interp, env, func, &ctx, props)? {
        Ok(html) => html,
        Err(e) => return Ok(e),
    };
    let id = SHARD_IDS.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    Ok(html_value(format!(
        "<tc-shard data-name=\"{}\" data-id=\"s{}\" data-props=\"{}\">{}</tc-shard>",
        escape(name),
        id,
        escape(&stringify_value(&spec)),
        inner
    )))
}

/// Calls a shard's function with its own signal scope: its markup, preceded by
/// the signals it declared. `Ok(Err(error value))` when it returned an error.
pub fn render_shard(interp: &Interpreter, env: &Rc<RefCell<Environment>>, func: &Value, ctx: &Value, props: Value) -> Result<Result<String, Value>, String> {
    let t = ctx_state(ctx).ok_or("shard: the request ctx is missing")?;
    let saved = t.borrow().get("signals").cloned();
    let own = Rc::new(RefCell::new(HashMap::new()));
    t.borrow_mut().insert("signals".into(), Value::Map(Rc::clone(&own)));
    let r = interp.call_value(func.clone(), vec![ctx.clone(), props], Rc::clone(env));
    match saved {
        Some(s) => t.borrow_mut().insert("signals".into(), s),
        None => t.borrow_mut().remove("signals"),
    };
    let v = r?;
    if matches!(v, Value::Error(_)) {
        return Ok(Err(v));
    }
    let mut body = String::new();
    render_child(&v, &mut body);
    Ok(Ok(signals_script(&own) + &body))
}

// ================= UI COMPONENTS =================

fn ui_module() -> Value {
    let mut m = HashMap::new();
    let fns: [(&str, usize, fn(&[Value]) -> Result<Value, String>); 22] = [
        ("button", 1, ui_button),
        ("badge", 1, |a| simple("span", "tc-badge", a, true)),
        ("card", 0, ui_card),
        ("stat", 2, kit::stat),
        ("input", 0, |a| control("input", "tc-input", a)),
        ("textarea", 0, ui_textarea),
        ("select", 1, ui_select),
        ("checkbox", 0, |a| checkable("tc-checkbox", a)),
        ("switch", 0, |a| checkable("tc-switch", a)),
        ("label", 1, |a| simple("label", "tc-label", a, false)),
        ("field", 2, ui_field),
        ("table", 2, ui_table),
        ("alert", 1, ui_alert),
        ("separator", 0, |_| Ok(html_value("<hr class=\"tc-separator\">".into()))),
        ("kbd", 1, |a| simple("kbd", "tc-kbd", a, false)),
        ("spinner", 0, |_| Ok(html_value("<span class=\"tc-spinner\" role=\"status\" aria-label=\"Loading\"></span>".into()))),
        ("progress", 1, ui_progress),
        ("skeleton", 0, ui_skeleton),
        ("tabs", 1, ui_tabs),
        ("shell", 1, ui_shell),
        ("link", 2, ui_link),
        ("stack", 0, |a| simple("div", "tc-stack", a, false)),
    ];
    for (n, min, f) in fns {
        m.insert(n.to_string(), nfn_var(n, min, f));
    }
    let more: [(&str, usize, fn(&[Value]) -> Result<Value, String>); 6] = [
        ("icon", 1, kit::icon),
        ("logo", 0, kit::logo),
        ("sparkline", 1, kit::sparkline),
        ("empty", 1, kit::empty),
        ("live", 0, kit::live),
        ("themeToggle", 0, kit::theme_toggle),
    ];
    for (n, min, f) in more {
        m.insert(n.to_string(), nfn_var(n, min, f));
    }
    m.insert("grid".to_string(), nfn_var("grid", 0, ui_grid));
    m.insert("row".to_string(), nfn_var("row", 0, |a| simple("div", "tc-row", a, false)));
    m.insert("container".to_string(), nfn_var("container", 0, |a| simple("div", "tc-container", a, false)));
    Value::Map(Rc::new(RefCell::new(m)))
}

/// Splits `opts` into the component's own keys and the extra attributes; the
/// component's class comes first, a `class` from the caller is appended.
fn split_opts(v: Option<&Value>, own: &[&str], base_class: &str) -> Result<(HashMap<String, Value>, Vec<(String, Value)>), String> {
    let mut mine = HashMap::new();
    let mut attrs = Vec::new();
    let mut class = base_class.to_string();
    match v {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            for (k, v) in sorted(&m.borrow()) {
                let (k, v) = (&k, &v);
                if own.contains(&k.as_str()) {
                    mine.insert(k.clone(), v.clone());
                } else if k == "class" {
                    let extra = class_value(v);
                    if !extra.is_empty() {
                        class = format!("{} {}", class, extra).trim().to_string();
                    }
                } else {
                    attrs.push((k.clone(), v.clone()));
                }
            }
        }
        Some(other) => return Err(format!("ui: options must be a map, got '{}'", other.type_name())),
    }
    if !class.is_empty() {
        attrs.insert(0, ("class".into(), Value::Str(class.into())));
    }
    Ok((mine, attrs))
}

/// Replaces the component's base class (the first attribute) with `class`,
/// keeping the classes the caller added.
fn set_base_class(attrs: &mut [(String, Value)], base: &str, class: String) {
    let extra = attrs[0].1.to_display_string();
    let extra = extra.strip_prefix(base).unwrap_or("").to_string();
    attrs[0].1 = Value::Str((class + &extra).into());
}

fn opt_str(o: &HashMap<String, Value>, k: &str) -> Option<String> {
    o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string())
}

fn variant_class(base: &str, o: &HashMap<String, Value>, allowed: &[&str]) -> Result<String, String> {
    let mut class = base.to_string();
    for key in ["variant", "size"] {
        if let Some(v) = opt_str(o, key) {
            if v == "default" || v == "md" || v == "primary" {
                continue;
            }
            if !allowed.contains(&v.as_str()) {
                return Err(format!("ui: unknown {} '{}' ({})", key, v, allowed.join(" ")));
            }
            class = format!("{} {}-{}", class, base, v);
        }
    }
    Ok(class)
}

/// `simple(tag)(children, opts)` — badges, labels, layout helpers.
fn simple(tag: &str, class: &str, a: &[Value], variants: bool) -> Result<Value, String> {
    let (o, mut attrs) = split_opts(a.get(1), &["variant"], class)?;
    if variants {
        let c = variant_class(class, &o, &["secondary", "outline", "destructive", "success", "warning"])?;
        set_base_class(&mut attrs, class, c);
    }
    Ok(html_value(element(tag, &attrs, &children(&a[..1.min(a.len())]))?))
}

/// `grid(children, { min = "220px" })` — columns at least `min` wide.
fn ui_grid(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.grid")?;
    let (o, mut attrs) = split_opts(a.get(1), &["min"], "tc-grid")?;
    if let Some(min) = opt_str(&o, "min") {
        if !min.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '%') {
            return Err(format!("ui.grid: invalid min '{}'", min));
        }
        attrs.push(("style".into(), Value::Str(format!("grid-template-columns: repeat(auto-fill, minmax({}, 1fr))", min).into())));
    }
    Ok(html_value(element("div", &attrs, &children(&a[..a.len().min(1)]))?))
}

/// `button(children, { variant, size, href, type = "button", ...attrs })`
fn ui_button(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.button")?;
    let (o, mut attrs) = split_opts(a.get(1), &["variant", "size", "href"], "tc-btn")?;
    let c = variant_class("tc-btn", &o, &["secondary", "outline", "ghost", "destructive", "link", "sm", "lg", "icon"])?;
    set_base_class(&mut attrs, "tc-btn", c);
    let kids = children(&a[..1]);
    match opt_str(&o, "href") {
        Some(href) => {
            attrs.push(("href".into(), Value::Str(href.into())));
            Ok(html_value(element("a", &attrs, &kids)?))
        }
        None => {
            if !attrs.iter().any(|(k, _)| k == "type") {
                attrs.push(("type".into(), Value::Str("button".into())));
            }
            Ok(html_value(element("button", &attrs, &kids)?))
        }
    }
}

/// `card(content, { title, description, footer, action, ...attrs })`
fn ui_card(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.card")?;
    let (o, attrs) = split_opts(a.get(1), &["title", "description", "footer", "action"], "tc-card")?;
    let mut inner = String::new();
    if o.contains_key("title") || o.contains_key("description") || o.contains_key("action") {
        let mut head = String::new();
        if let Some(t) = o.get("title") {
            head.push_str(&element("div", &[("class".into(), Value::Str("tc-card-title".into()))], &children(std::slice::from_ref(t)))?);
        }
        if let Some(d) = o.get("description") {
            head.push_str(&element("div", &[("class".into(), Value::Str("tc-card-description".into()))], &children(std::slice::from_ref(d)))?);
        }
        let head = element("div", &[("class".into(), Value::Str("tc-card-header".into()))], &head)?;
        inner.push_str(&match o.get("action") {
            Some(act) => element(
                "div",
                &[("class".into(), Value::Str("tc-row".into())), ("style".into(), Value::Str("padding-right:24px;align-items:flex-start".into()))],
                &(head.replace("class=\"tc-card-header\"", "class=\"tc-card-header\" style=\"flex:1\"") + &children(std::slice::from_ref(act))),
            )?,
            None => head,
        });
    }
    if let Some(c) = a.first().filter(|v| !matches!(v, Value::Nil)) {
        inner.push_str(&element("div", &[("class".into(), Value::Str("tc-card-content".into()))], &children(std::slice::from_ref(c)))?);
    }
    if let Some(f) = o.get("footer") {
        inner.push_str(&element("div", &[("class".into(), Value::Str("tc-card-footer".into()))], &children(std::slice::from_ref(f)))?);
    }
    Ok(html_value(element("div", &attrs, &inner)?))
}

fn control(tag: &str, class: &str, a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, tag)?;
    let (_, attrs) = split_opts(a.first(), &[], class)?;
    Ok(html_value(element(tag, &attrs, "")?))
}

/// `textarea({ value, ...attrs })`
fn ui_textarea(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "ui.textarea")?;
    let (o, attrs) = split_opts(a.first(), &["value"], "tc-textarea")?;
    let v = o.get("value").cloned().unwrap_or(Value::Nil);
    Ok(html_value(element("textarea", &attrs, &children(&[v]))?))
}

fn checkable(class: &str, a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "ui.checkbox")?;
    let (_, mut attrs) = split_opts(a.first(), &[], class)?;
    attrs.push(("type".into(), Value::Str("checkbox".into())));
    if class == "tc-switch" {
        attrs.push(("role".into(), Value::Str("switch".into())));
    }
    Ok(html_value(element("input", &attrs, "")?))
}

/// `select(options, { value, ...attrs })` — options: ["a", …], [[value, label], …] or [{ value, label }, …].
fn ui_select(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.select")?;
    let (o, attrs) = split_opts(a.get(1), &["value"], "tc-select")?;
    let selected = opt_str(&o, "value");
    let Some(Value::List(items)) = a.first() else {
        return Err("ui.select: options must be a list".into());
    };
    let mut inner = String::new();
    for it in items.borrow().iter() {
        let (v, l) = match it {
            Value::List(p) => {
                let p = p.borrow();
                (p.first().map(|v| v.to_display_string()).unwrap_or_default(), p.get(1).cloned().unwrap_or(Value::Nil))
            }
            Value::Map(m) => {
                let m = m.borrow();
                (m.get("value").map(|v| v.to_display_string()).unwrap_or_default(), m.get("label").cloned().unwrap_or(Value::Nil))
            }
            other => (other.to_display_string(), other.clone()),
        };
        let label = if matches!(l, Value::Nil) { Value::Str(v.clone().into()) } else { l };
        let mut oa = vec![("value".to_string(), Value::Str(v.clone().into()))];
        if selected.as_deref() == Some(v.as_str()) {
            oa.push(("selected".into(), Value::Bool(true)));
        }
        inner.push_str(&element("option", &oa, &children(&[label]))?);
    }
    Ok(html_value(element("select", &attrs, &inner)?))
}

/// `field(label, control, { description, error, for })`
fn ui_field(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "ui.field")?;
    let (o, attrs) = split_opts(a.get(2), &["description", "error", "for"], "tc-field")?;
    let mut la = vec![("class".to_string(), Value::Str("tc-label".into()))];
    if let Some(f) = opt_str(&o, "for") {
        la.push(("for".into(), Value::Str(f.into())));
    }
    let mut inner = element("label", &la, &children(&a[..1]))?;
    inner.push_str(&children(&a[1..2]));
    if let Some(d) = o.get("description") {
        inner.push_str(&element("div", &[("class".into(), Value::Str("tc-field-description".into()))], &children(std::slice::from_ref(d)))?);
    }
    if let Some(e) = o.get("error").filter(|v| v.is_truthy()) {
        inner.push_str(&element("div", &[("class".into(), Value::Str("tc-field-error".into())), ("role".into(), Value::Str("alert".into()))], &children(std::slice::from_ref(e)))?);
    }
    Ok(html_value(element("div", &attrs, &inner)?))
}

/// `table(columns, rows, { empty, caption, ...attrs })` — columns: ["name", …] or
/// [{ key, label, align }, …]; rows: maps (by key) or lists (by position).
fn ui_table(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "ui.table")?;
    let (o, attrs) = split_opts(a.get(2), &["empty", "caption", "header"], "tc-table")?;
    let Some(Value::List(cols)) = a.first() else {
        return Err("ui.table: columns must be a list".into());
    };
    let cols: Vec<(String, Value, Option<String>)> = cols
        .borrow()
        .iter()
        .map(|c| match c {
            Value::Map(m) => {
                let m = m.borrow();
                let key = m.get("key").map(|v| v.to_display_string()).unwrap_or_default();
                let label = m.get("label").cloned().unwrap_or(Value::Str(key.clone().into()));
                (key, label, m.get("align").map(|v| v.to_display_string()))
            }
            other => (other.to_display_string(), other.clone(), None),
        })
        .collect();
    let rows = match a.get(1) {
        Some(Value::List(r)) => r.borrow().clone(),
        None | Some(Value::Nil) => Vec::new(),
        Some(other) => return Err(format!("ui.table: rows must be a list, got '{}'", other.type_name())),
    };
    let align = |al: &Option<String>| -> Vec<(String, Value)> {
        al.as_ref().map(|a| vec![("style".to_string(), Value::Str(format!("text-align:{}", a).into()))]).unwrap_or_default()
    };
    let mut head = String::new();
    for (_, label, al) in &cols {
        head.push_str(&element("th", &align(al), &children(std::slice::from_ref(label)))?);
    }
    let mut body = String::new();
    if rows.is_empty() {
        let empty = o.get("empty").cloned().unwrap_or(Value::Str("No rows.".into()));
        let td = element("td", &[("class".into(), Value::Str("tc-table-empty".into())), ("colspan".into(), Value::Int(cols.len().max(1) as i64))], &children(&[empty]))?;
        body.push_str(&format!("<tr>{}</tr>", td));
    }
    for row in &rows {
        let mut tr = String::new();
        for (i, (key, _, al)) in cols.iter().enumerate() {
            let cell = match row {
                Value::Map(m) => m.borrow().get(key).cloned().unwrap_or(Value::Nil),
                Value::List(l) => l.borrow().get(i).cloned().unwrap_or(Value::Nil),
                other => if i == 0 { other.clone() } else { Value::Nil },
            };
            tr.push_str(&element("td", &align(al), &children(&[cell]))?);
        }
        body.push_str(&format!("<tr>{}</tr>", tr));
    }
    let show_head = o.get("header").is_none_or(Value::is_truthy);
    let mut inner = if show_head { format!("<thead><tr>{}</tr></thead><tbody>{}</tbody>", head, body) } else { format!("<tbody>{}</tbody>", body) };
    if let Some(c) = o.get("caption") {
        inner.push_str(&element("caption", &[], &children(std::slice::from_ref(c)))?);
    }
    let table = element("table", &attrs, &inner)?;
    Ok(html_value(format!("<div class=\"tc-table-wrap\">{}</div>", table)))
}

/// `alert(title, description?, { variant })`
fn ui_alert(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "ui.alert")?;
    let (o, mut attrs) = split_opts(a.get(2), &["variant"], "tc-alert")?;
    let c = variant_class("tc-alert", &o, &["destructive"])?;
    set_base_class(&mut attrs, "tc-alert", c);
    attrs.push(("role".into(), Value::Str("alert".into())));
    let mut inner = element("div", &[("class".into(), Value::Str("tc-alert-title".into()))], &children(&a[..1]))?;
    if let Some(d) = a.get(1).filter(|v| !matches!(v, Value::Nil)) {
        inner.push_str(&element("div", &[("class".into(), Value::Str("tc-alert-description".into()))], &children(std::slice::from_ref(d)))?);
    }
    Ok(html_value(element("div", &attrs, &inner)?))
}

/// `progress(value 0..100)` — a number, a signal or an expression.
fn ui_progress(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.progress")?;
    let (_, mut attrs) = split_opts(a.get(1), &[], "tc-progress")?;
    attrs.push(("role".into(), Value::Str("progressbar".into())));
    let bar = match reactive(&a[0]) {
        Some((code, init)) => format!(
            "<div style=\"width:{}%\" tc-bind:style=\"{}\"></div>",
            escape(&init.to_display_string()),
            escape(&format!("({{ width: Math.max(0, Math.min(100, {})) + '%' }})", code))
        ),
        None => {
            let v = match &a[0] {
                Value::Int(n) => *n as f64,
                Value::Float(f) => *f,
                other => return Err(format!("ui.progress: expected a number, got '{}'", other.type_name())),
            };
            attrs.push(("aria-valuenow".into(), Value::Float(v)));
            format!("<div style=\"width:{}%\"></div>", v.clamp(0.0, 100.0))
        }
    };
    Ok(html_value(element("div", &attrs, &bar)?))
}

/// `skeleton({ width, height })`
fn ui_skeleton(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "ui.skeleton")?;
    let (o, mut attrs) = split_opts(a.first(), &["width", "height"], "tc-skeleton")?;
    let w = opt_str(&o, "width").unwrap_or_else(|| "100%".into());
    let h = opt_str(&o, "height").unwrap_or_else(|| "16px".into());
    attrs.push(("style".into(), Value::Str(format!("width:{};height:{}", w, h).into())));
    Ok(html_value(element("div", &attrs, "")?))
}

/// `tabs([{ label, href, selected }])` — links that navigate without a reload.
fn ui_tabs(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.tabs")?;
    let (_, mut attrs) = split_opts(a.get(1), &[], "tc-tabs")?;
    attrs.push(("role".into(), Value::Str("tablist".into())));
    let Some(Value::List(items)) = a.first() else {
        return Err("ui.tabs: expected a list of { label, href, selected }".into());
    };
    let mut inner = String::new();
    for it in items.borrow().iter() {
        let Value::Map(m) = it else { return Err("ui.tabs: items must be maps".into()) };
        let m = m.borrow();
        let label = m.get("label").cloned().unwrap_or(Value::Nil);
        let selected = m.get("selected").is_some_and(Value::is_truthy);
        let mut ta = vec![("class".to_string(), Value::Str("tc-tab".into())), ("role".into(), Value::Str("tab".into())), ("aria-selected".into(), Value::Str(selected.to_string().into()))];
        match m.get("href") {
            Some(href) => {
                ta.push(("href".into(), href.clone()));
                ta.push(("tc-link".into(), Value::Bool(true)));
                inner.push_str(&element("a", &ta, &children(&[label]))?);
            }
            None => {
                for (k, v) in sorted(&m).iter().filter(|(k, _)| k.starts_with('@') || k.starts_with(':')) {
                    ta.push((k.clone(), v.clone()));
                }
                ta.push(("type".into(), Value::Str("button".into())));
                inner.push_str(&element("button", &ta, &children(&[label]))?);
            }
        }
    }
    Ok(html_value(element("div", &attrs, &inner)?))
}

/// `link(children, href, attrs?)` — navigates without a full reload.
fn ui_link(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "ui.link")?;
    let (_, mut attrs) = split_opts(a.get(2), &[], "")?;
    attrs.push(("href".into(), a[1].clone()));
    attrs.push(("tc-link".into(), Value::Bool(true)));
    Ok(html_value(element("a", &attrs, &children(&a[..1]))?))
}

/// `shell({ brand, nav: [{ label, href, current } | { group }], title, actions }, content)`
fn ui_shell(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.shell")?;
    let (o, attrs) = split_opts(a.first(), &["brand", "nav", "title", "actions", "footer"], "tc-shell")?;
    let mut side = String::new();
    if let Some(b) = o.get("brand") {
        side.push_str(&element("div", &[("class".into(), Value::Str("tc-sidebar-title".into()))], &children(std::slice::from_ref(b)))?);
    }
    if let Some(Value::List(nav)) = o.get("nav") {
        for it in nav.borrow().iter() {
            let Value::Map(m) = it else { continue };
            let m = m.borrow();
            if let Some(g) = m.get("group") {
                side.push_str(&element("div", &[("class".into(), Value::Str("tc-sidebar-group".into()))], &children(std::slice::from_ref(g)))?);
                continue;
            }
            let mut la = vec![
                ("class".to_string(), Value::Str("tc-sidebar-item".into())),
                ("href".into(), m.get("href").cloned().unwrap_or(Value::Str("#".into()))),
                ("tc-link".into(), Value::Bool(true)),
            ];
            if m.get("current").is_some_and(Value::is_truthy) {
                la.push(("aria-current".into(), Value::Str("page".into())));
            }
            let label = kit::nav_icon(&m)? + &children(&[m.get("label").cloned().unwrap_or(Value::Nil)]);
            side.push_str(&element("a", &la, &label)?);
        }
    }
    if let Some(f) = o.get("footer") {
        side.push_str(&element("div", &[("class".into(), Value::Str("tc-sidebar-footer".into()))], &children(std::slice::from_ref(f)))?);
    }
    let sidebar = element("nav", &[("class".into(), Value::Str("tc-sidebar".into()))], &side)?;
    let mut top = String::new();
    if let Some(t) = o.get("title") {
        top.push_str(&element("h2", &[], &children(std::slice::from_ref(t)))?);
    }
    top.push_str("<div class=\"tc-spacer\"></div>");
    if let Some(act) = o.get("actions") {
        top.push_str(&children(std::slice::from_ref(act)));
    }
    let main = format!(
        "<main class=\"tc-main\"><header class=\"tc-topbar\">{}</header><div class=\"tc-container tc-stack\">{}</div></main>",
        top,
        children(&a[1..])
    );
    Ok(html_value(element("div", &attrs, &(sidebar + &main))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_attributes() {
        assert_eq!(escape("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
        let attrs = vec![
            ("@click".to_string(), Value::Str("n.set(1)".into())),
            ("disabled".to_string(), Value::Bool(true)),
            ("hidden".to_string(), Value::Bool(false)),
            ("title".to_string(), Value::Str("a\"b".into())),
        ];
        assert_eq!(
            element("button", &attrs, "x").unwrap(),
            "<button tc-on:click=\"n.set(1)\" disabled title=\"a&quot;b\">x</button>"
        );
        assert_eq!(element("br", &[], "ignored").unwrap(), "<br>");
        assert!(element("x y", &[], "").is_err());
        assert!(render_attrs(&[("a b".into(), Value::Nil)], &mut String::new()).is_err());
    }

    #[test]
    fn signals_go_before_body_end() {
        let ctx = map(vec![]);
        let t = ctx_state(&ctx).unwrap();
        sub_map(&t, "signals").borrow_mut().insert("n".into(), map(vec![("v", Value::Int(1)), ("t", Value::Bool(false))]));
        let out = with_signals("<html><body><p>x</p></body></html>".into(), &ctx);
        assert_eq!(out, "<html><body><p>x</p><script type=\"application/json\" data-tc-signals>{\"n\":{\"t\":false,\"v\":1}}</script></body></html>");
        assert!(script_json(&Value::Str("</script>".into())).contains("<\\/script>"));
    }
}
