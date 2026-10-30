//! `fogu:html` module — parse and query HTML (CSS selectors), Jinja templates
//! (minijinja), sanitizing, escaping and conversions (HTML → text/Markdown,
//! Markdown → HTML).
//!
//! ```fogu
//! import html from "fogu:html"
//!
//! const doc = html.parse("<ul><li><a href='/a'>A</a></li><li><a href='/b'>B</a></li></ul>")
//! for a in doc.select("li > a") { print(a.text, a.attr("href")) }
//! print(html.render("<p>Hi {{ name }}</p>", { name: "<Ana>" })?)   // escaped
//! print(html.toMarkdown("<h1>Title</h1><p>Some <b>bold</b></p>"))
//! ```
//!
//! An invalid CSS selector stops the program (it is written by the
//! programmer); template errors are `error` values (templates often live in
//! files).

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use ego_tree::{NodeId, NodeRef};
use minijinja::{AutoEscape, Environment};
use scraper::{ElementRef, Html, Node, Selector};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{list, map, max_args, nfn_var, opt_arg, str_arg, str_list};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("parse".into(), nfn_var("parse", 1, |a| {
        max_args(a, 1, "html.parse")?;
        Ok(Document::new(Html::parse_document(&str_arg(a, 0, "html.parse")?)))
    }));
    e.insert("fragment".into(), nfn_var("fragment", 1, |a| {
        max_args(a, 1, "html.fragment")?;
        Ok(Document::new(Html::parse_fragment(&str_arg(a, 0, "html.fragment")?)))
    }));
    // html.select(text, css) → list of elements (shortcut for parse + select)
    e.insert("select".into(), nfn_var("select", 2, |a| {
        max_args(a, 2, "html.select")?;
        let doc = Rc::new(Html::parse_document(&str_arg(a, 0, "html.select")?));
        let sel = selector(&str_arg(a, 1, "html.select")?)?;
        let root = doc.root_element().id();
        Ok(select_from(&doc, root, &sel, true))
    }));
    e.insert("escape".into(), nfn_var("escape", 1, |a| {
        max_args(a, 1, "html.escape")?;
        Ok(Value::Str(escape(&str_arg(a, 0, "html.escape")?).into()))
    }));
    e.insert("unescape".into(), nfn_var("unescape", 1, |a| {
        max_args(a, 1, "html.unescape")?;
        Ok(Value::Str(unescape(&str_arg(a, 0, "html.unescape")?).into()))
    }));
    e.insert("sanitize".into(), nfn_var("sanitize", 1, |a| {
        max_args(a, 2, "html.sanitize")?;
        sanitize(&str_arg(a, 0, "html.sanitize")?, opt_arg(a, 1))
    }));
    e.insert("text".into(), nfn_var("text", 1, |a| {
        max_args(a, 1, "html.text")?;
        let doc = Html::parse_document(&str_arg(a, 0, "html.text")?);
        Ok(Value::Str(to_text(doc.tree.root()).into()))
    }));
    e.insert("toMarkdown".into(), nfn_var("toMarkdown", 1, |a| {
        max_args(a, 1, "html.toMarkdown")?;
        let doc = Html::parse_document(&str_arg(a, 0, "html.toMarkdown")?);
        Ok(Value::Str(to_markdown(doc.tree.root()).into()))
    }));
    e.insert("fromMarkdown".into(), nfn_var("fromMarkdown", 1, |a| {
        max_args(a, 1, "html.fromMarkdown")?;
        Ok(Value::Str(markdown_to_html(&str_arg(a, 0, "html.fromMarkdown")?).into()))
    }));
    // templates
    e.insert("render".into(), nfn_var("render", 1, |a| {
        max_args(a, 2, "html.render")?;
        let src = str_arg(a, 0, "html.render")?;
        let env = new_env(true);
        let tmpl = env.template_from_str(&src).map_err(|e| tmpl_err("html.render", e))?;
        tmpl.render(context(a.get(1))).map(Value::str).map_err(|e| tmpl_err("html.render", e))
    }));
    e.insert("template".into(), nfn_var("template", 1, |a| {
        max_args(a, 2, "html.template")?;
        let src = str_arg(a, 0, "html.template")?;
        let mut env = new_env(autoescape_opt(a.get(1)).unwrap_or(true));
        env.add_template_owned("main", src).map_err(|e| tmpl_err("html.template", e))?;
        Ok(NativeObjectRef::new(Template { env }).to_value())
    }));
    e.insert("templates".into(), nfn_var("templates", 1, |a| {
        max_args(a, 2, "html.templates")?;
        let dir = str_arg(a, 0, "html.templates")?;
        if !std::path::Path::new(&dir).is_dir() {
            return Err(io_err!("html.templates: '{}' is not a directory", dir));
        }
        let mut env = new_env(true);
        if let Some(on) = autoescape_opt(a.get(1)) {
            env.set_auto_escape_callback(move |_| if on { AutoEscape::Html } else { AutoEscape::None });
        } else {
            // by extension: .html .htm .xml .j2.html …
            env.set_auto_escape_callback(minijinja::default_auto_escape_callback);
        }
        env.set_loader(minijinja::path_loader(dir));
        Ok(NativeObjectRef::new(Templates { env }).to_value())
    }));
    ModuleValue {
        name: "fogu:html".into(),
        exports: RefCell::new(e),
    }
}

// ================= DOCUMENT / ELEMENT =================

fn selector(css: &str) -> Result<Selector, String> {
    Selector::parse(css).map_err(|e| format!("html: invalid CSS selector '{}': {}", css, e))
}

/// Elements matching `sel` below `id` (`all = false` → only the first, or nil).
fn select_from(doc: &Rc<Html>, id: NodeId, sel: &Selector, all: bool) -> Value {
    let Some(el) = doc.tree.get(id).and_then(ElementRef::wrap) else {
        return if all { list(Vec::new()) } else { Value::Nil };
    };
    let mut found = el.select(sel).map(|m| element(doc, m.id()));
    if all { list(found.collect()) } else { found.next().unwrap_or(Value::Nil) }
}

fn element(doc: &Rc<Html>, id: NodeId) -> Value {
    NativeObjectRef::new(Element { doc: Rc::clone(doc), id }).to_value()
}

pub struct Document {
    doc: Rc<Html>,
}

impl Document {
    fn new(html: Html) -> Value {
        NativeObjectRef::new(Document { doc: Rc::new(html) }).to_value()
    }
}

impl FoguObject for Document {
    fn name(&self) -> &str {
        "HtmlDocument"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let root = self.doc.root_element();
        Some(match field {
            "root" => element(&self.doc, root.id()),
            "title" => {
                let sel = Selector::parse("title").unwrap();
                root.select(&sel).next().map_or(Value::Nil, |t| Value::Str(clean_text(t).into()))
            }
            "text" => Value::Str(clean_text(root).into()),
            "html" => Value::Str(root.html().into()),
            "errors" => str_list(self.doc.errors.iter().map(|e| e.to_string())),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let root = self.doc.root_element().id();
        match method {
            "select" | "find" => {
                max_args(args, 1, method)?;
                let sel = selector(&str_arg(args, 0, method)?)?;
                // the root element itself may match too (e.g. "html")
                Ok(select_from(&self.doc, root, &sel, method == "select"))
            }
            other => Err(format!("method '{}' does not exist on HtmlDocument (select find)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["select", "find"]
    }
}

pub struct Element {
    doc: Rc<Html>,
    id: NodeId,
}

impl Element {
    fn el(&self) -> ElementRef<'_> {
        ElementRef::wrap(self.doc.tree.get(self.id).expect("node of this document")).expect("an element node")
    }
}

impl FoguObject for Element {
    fn name(&self) -> &str {
        "HtmlElement"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let el = self.el();
        let v = el.value();
        Some(match field {
            "tag" => Value::Str(v.name().to_string().into()),
            "text" => Value::Str(clean_text(el).into()),
            "html" => Value::Str(el.inner_html().into()),
            "outerHtml" => Value::Str(el.html().into()),
            "id" => v.id().map_or(Value::Nil, |s| Value::Str(s.to_string().into())),
            "classes" => str_list(v.classes().map(String::from)),
            "attrs" => map(v.attrs().collect::<Vec<_>>().into_iter().map(|(k, val)| (k, Value::Str(val.to_string().into()))).collect()),
            "children" => list(el.children().filter_map(ElementRef::wrap).map(|c| element(&self.doc, c.id())).collect()),
            "parent" => el.parent().and_then(ElementRef::wrap).map_or(Value::Nil, |p| element(&self.doc, p.id())),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "select" | "find" => {
                max_args(args, 1, method)?;
                let sel = selector(&str_arg(args, 0, method)?)?;
                Ok(select_from(&self.doc, self.id, &sel, method == "select"))
            }
            "attr" => {
                max_args(args, 1, "attr")?;
                let name = str_arg(args, 0, "attr")?;
                Ok(self.el().value().attr(&name).map_or(Value::Nil, |s| Value::Str(s.to_string().into())))
            }
            "hasClass" => {
                max_args(args, 1, "hasClass")?;
                let name = str_arg(args, 0, "hasClass")?;
                Ok(Value::Bool(self.el().value().classes().any(|c| c == name)))
            }
            // closest(css): this element or the nearest ancestor that matches
            "closest" => {
                max_args(args, 1, "closest")?;
                let sel = selector(&str_arg(args, 0, "closest")?)?;
                let mut cur = Some(self.el());
                while let Some(e) = cur {
                    if sel.matches(&e) {
                        return Ok(element(&self.doc, e.id()));
                    }
                    cur = e.parent().and_then(ElementRef::wrap);
                }
                Ok(Value::Nil)
            }
            "matches" => {
                max_args(args, 1, "matches")?;
                Ok(Value::Bool(selector(&str_arg(args, 0, "matches")?)?.matches(&self.el())))
            }
            "toText" => Ok(Value::Str(to_text(*self.el()).into())),
            "toMarkdown" => Ok(Value::Str(to_markdown(*self.el()).into())),
            other => Err(format!(
                "method '{}' does not exist on HtmlElement (select find attr hasClass closest matches toText toMarkdown)",
                other
            )),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["select", "find", "attr", "hasClass", "closest", "matches", "toText", "toMarkdown"]
    }
}

/// The text of an element with whitespace runs collapsed and trimmed.
fn clean_text(el: ElementRef) -> String {
    let raw: String = el.text().collect();
    collapse_ws(&raw).trim().to_string()
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

// ================= ESCAPING / SANITIZING =================

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

const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"), ("lt", "<"), ("gt", ">"), ("quot", "\""), ("apos", "'"), ("nbsp", "\u{a0}"),
    ("copy", "©"), ("reg", "®"), ("trade", "™"), ("hellip", "…"), ("mdash", "—"), ("ndash", "–"),
    ("lsquo", "‘"), ("rsquo", "’"), ("ldquo", "“"), ("rdquo", "”"), ("laquo", "«"), ("raquo", "»"),
    ("bull", "•"), ("middot", "·"), ("deg", "°"), ("euro", "€"), ("pound", "£"), ("yen", "¥"),
    ("cent", "¢"), ("sect", "§"), ("para", "¶"), ("times", "×"), ("divide", "÷"), ("plusmn", "±"),
    ("frac12", "½"), ("frac14", "¼"), ("frac34", "¾"), ("larr", "←"), ("rarr", "→"), ("uarr", "↑"),
    ("darr", "↓"), ("iexcl", "¡"), ("iquest", "¿"), ("shy", "\u{ad}"), ("zwj", "\u{200d}"),
];

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(';').map(|j| j + 1).filter(|&j| j <= 32);
        let decoded = end.and_then(|j| {
            let name = &rest[1..j];
            if let Some(num) = name.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse().ok(),
                };
                code.and_then(char::from_u32).map(|c| (c.to_string(), j))
            } else {
                ENTITIES.iter().find(|(n, _)| *n == name).map(|(_, v)| (v.to_string(), j))
            }
        });
        match decoded {
            Some((text, j)) => {
                out.push_str(&text);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn strings(v: Option<&Value>, what: &str) -> Result<Vec<String>, String> {
    match v {
        None | Some(Value::Nil) => Ok(Vec::new()),
        Some(Value::List(l)) => l
            .borrow()
            .iter()
            .map(|x| match x {
                Value::Str(s) => Ok(s.to_string()),
                other => Err(format!("html.sanitize: '{}' must be a list of str, got '{}'", what, other.type_name())),
            })
            .collect(),
        Some(other) => Err(format!("html.sanitize: '{}' must be a list of str, got '{}'", what, other.type_name())),
    }
}

/// `sanitize(text, { tags, addTags, removeTags, attributes, stripComments })`
fn sanitize(text: &str, opts: Option<&Value>) -> Result<Value, String> {
    let o = match opts {
        Some(Value::Map(m)) => m.borrow().clone(),
        None => HashMap::new(),
        Some(other) => return Err(format!("html.sanitize: options must be a map, got '{}'", other.type_name())),
    };
    let tags = strings(o.get("tags"), "tags")?;
    let add = strings(o.get("addTags"), "addTags")?;
    let remove = strings(o.get("removeTags"), "removeTags")?;
    let attrs = strings(o.get("attributes"), "attributes")?;
    let mut b = ammonia::Builder::default();
    if o.contains_key("tags") {
        b.tags(tags.iter().map(String::as_str).collect());
    }
    b.add_tags(add.iter().map(String::as_str));
    b.rm_tags(remove.iter().map(String::as_str));
    // attributes allowed on every tag
    b.add_generic_attributes(attrs.iter().map(String::as_str));
    if let Some(v) = o.get("stripComments") {
        b.strip_comments(v.is_truthy());
    }
    Ok(Value::Str(b.clean(text).to_string().into()))
}

// ================= CONVERSIONS =================

const SKIP: &[&str] = &["script", "style", "head", "noscript", "template", "svg", "iframe", "object", "canvas"];
const BLOCK: &[&str] = &[
    "p", "div", "section", "article", "header", "footer", "nav", "main", "aside", "figure", "figcaption",
    "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "table", "tr", "blockquote", "pre", "dl", "dt",
    "dd", "form", "address", "hr", "fieldset", "details", "summary", "body", "html",
];

/// Collapses 3+ newlines into 2, trims the end of each line and the text.
fn tidy(s: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in s.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(line);
    }
    out
}

/// Appends inline text with whitespace collapsed (no space at a line start).
fn push_inline(out: &mut String, text: &str) {
    let t = collapse_ws(text);
    let at_start = out.is_empty() || out.ends_with('\n') || out.ends_with(' ');
    out.push_str(if at_start { t.trim_start() } else { &t });
}

/// Plain text with paragraph breaks, list bullets and tab-separated cells.
fn to_text(node: NodeRef<Node>) -> String {
    fn walk(node: NodeRef<Node>, out: &mut String, pre: bool) {
        match node.value() {
            Node::Text(t) => {
                if pre { out.push_str(&t[..]) } else { push_inline(out, &t[..]) }
            }
            Node::Element(e) => {
                let tag = e.name();
                if SKIP.contains(&tag) {
                    return;
                }
                match tag {
                    "br" => return out.push('\n'),
                    "hr" => return out.push_str("\n\n"),
                    "li" => out.push_str("\n- "),
                    "td" | "th" if node.prev_sibling().is_some_and(|s| s.value().is_element()) => out.push('\t'),
                    _ if BLOCK.contains(&tag) => out.push_str(if matches!(tag, "tr" | "dt" | "dd") { "\n" } else { "\n\n" }),
                    _ => {}
                }
                for c in node.children() {
                    walk(c, out, pre || tag == "pre");
                }
                if BLOCK.contains(&tag) && tag != "li" {
                    out.push('\n');
                }
            }
            Node::Document | Node::Fragment => node.children().for_each(|c| walk(c, out, pre)),
            _ => {}
        }
    }
    let mut out = String::new();
    walk(node, &mut out, false);
    tidy(&out)
}

/// Markdown (CommonMark + tables) from an HTML tree.
fn to_markdown(node: NodeRef<Node>) -> String {
    fn children(node: NodeRef<Node>, pre: bool) -> String {
        let mut out = String::new();
        for c in node.children() {
            walk(c, &mut out, pre);
        }
        out
    }
    fn inline(node: NodeRef<Node>) -> String {
        collapse_ws(&children(node, false)).trim().to_string()
    }
    fn walk(node: NodeRef<Node>, out: &mut String, pre: bool) {
        match node.value() {
            Node::Text(t) => {
                if pre { out.push_str(&t[..]) } else { push_inline(out, &t[..]) }
            }
            Node::Document | Node::Fragment => node.children().for_each(|c| walk(c, out, pre)),
            Node::Element(e) => {
                let tag = e.name();
                if SKIP.contains(&tag) {
                    return;
                }
                match tag {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        let level = tag[1..].parse::<usize>().unwrap_or(1);
                        out.push_str(&format!("\n\n{} {}\n\n", "#".repeat(level), inline(node)));
                    }
                    "strong" | "b" => wrap(out, "**", &inline(node)),
                    "em" | "i" => wrap(out, "*", &inline(node)),
                    "del" | "s" | "strike" => wrap(out, "~~", &inline(node)),
                    "code" if !pre => wrap(out, "`", &collapse_ws(&children(node, true))),
                    "pre" => {
                        let lang = node
                            .children()
                            .filter_map(ElementRef::wrap)
                            .find(|c| c.value().name() == "code")
                            .and_then(|c| c.value().classes().find_map(|k| k.strip_prefix("language-").map(String::from)))
                            .unwrap_or_default();
                        let code = children(node, true);
                        out.push_str(&format!("\n\n```{}\n{}\n```\n\n", lang, code.trim_end_matches('\n')));
                    }
                    "a" => {
                        let text = inline(node);
                        match e.attr("href") {
                            Some(href) if !href.is_empty() => {
                                push_inline(out, "");
                                out.push_str(&format!("[{}]({})", text, href));
                            }
                            _ => push_inline(out, &text),
                        }
                    }
                    "img" => {
                        let src = e.attr("src").unwrap_or("");
                        out.push_str(&format!("![{}]({})", e.attr("alt").unwrap_or(""), src));
                    }
                    "br" => out.push_str("  \n"),
                    "hr" => out.push_str("\n\n---\n\n"),
                    "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure"
                    | "figcaption" | "form" | "address" | "details" | "summary" | "dl" => {
                        out.push_str("\n\n");
                        out.push_str(&children(node, false));
                        out.push_str("\n\n");
                    }
                    "dt" | "dd" => {
                        out.push('\n');
                        out.push_str(&inline(node));
                        out.push('\n');
                    }
                    "blockquote" => {
                        let body = tidy(&children(node, false));
                        let quoted: Vec<String> = body.lines().map(|l| if l.is_empty() { ">".into() } else { format!("> {}", l) }).collect();
                        out.push_str(&format!("\n\n{}\n\n", quoted.join("\n")));
                    }
                    "ul" | "ol" => {
                        let mut n = 0;
                        out.push_str("\n\n");
                        for li in node.children().filter_map(ElementRef::wrap).filter(|c| c.value().name() == "li") {
                            n += 1;
                            let bullet = if tag == "ol" { format!("{}. ", n) } else { "- ".into() };
                            let body = tidy(&children(*li, false));
                            let pad = " ".repeat(bullet.len());
                            for (i, line) in body.lines().enumerate() {
                                if i == 0 {
                                    out.push_str(&format!("{}{}\n", bullet, line));
                                } else if line.is_empty() {
                                    out.push('\n');
                                } else {
                                    out.push_str(&format!("{}{}\n", pad, line));
                                }
                            }
                        }
                        out.push('\n');
                    }
                    "table" => {
                        let tr = Selector::parse("tr").unwrap();
                        let cell = Selector::parse("th, td").unwrap();
                        let Some(table) = ElementRef::wrap(node) else { return };
                        let rows: Vec<Vec<String>> = table
                            .select(&tr)
                            .map(|r| r.select(&cell).map(|c| inline(*c).replace('|', "\\|")).collect())
                            .filter(|r: &Vec<String>| !r.is_empty())
                            .collect();
                        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                        out.push_str("\n\n");
                        for (i, r) in rows.iter().enumerate() {
                            let mut cells = r.clone();
                            cells.resize(width, String::new());
                            out.push_str(&format!("| {} |\n", cells.join(" | ")));
                            if i == 0 {
                                out.push_str(&format!("|{}\n", " --- |".repeat(width)));
                            }
                        }
                        out.push('\n');
                    }
                    _ => {
                        let inner = children(node, pre);
                        out.push_str(&inner);
                    }
                }
            }
            _ => {}
        }
    }
    fn wrap(out: &mut String, mark: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        push_inline(out, "");
        out.push_str(mark);
        out.push_str(text);
        out.push_str(mark);
    }
    let mut out = String::new();
    walk(node, &mut out, false);
    tidy(&out)
}

pub fn markdown_to_html(md: &str) -> String {
    use pulldown_cmark::{Options, Parser};
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES;
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, Parser::new_ext(md, opts));
    out
}

// ================= TEMPLATES (Jinja) =================

fn new_env(autoescape: bool) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(move |_| if autoescape { AutoEscape::Html } else { AutoEscape::None });
    // {{ text | markdown }} → HTML (not escaped again)
    env.add_filter("markdown", |s: String| minijinja::Value::from_safe_string(markdown_to_html(&s)));
    env.add_filter("striptags", |s: String| to_text(Html::parse_fragment(&s).tree.root()));
    env
}

fn autoescape_opt(v: Option<&Value>) -> Option<bool> {
    match v {
        Some(Value::Map(m)) => m.borrow().get("autoescape").map(Value::is_truthy),
        _ => None,
    }
}

/// The template context: a map (or nil) as a minijinja value.
fn context(v: Option<&Value>) -> minijinja::Value {
    match v {
        None | Some(Value::Nil) => minijinja::Value::from(()),
        Some(v) => {
            let j: serde_json::Value = serde_json::from_str(&stringify_value(v)).unwrap_or(serde_json::Value::Null);
            minijinja::Value::from_serialize(&j)
        }
    }
}

fn tmpl_err(what: &str, e: minijinja::Error) -> String {
    let mut msg = format!("{}: {}", what, e);
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        msg.push_str(&format!(": {}", s));
        src = s.source();
    }
    io_err!("{}", msg)
}

pub struct Template {
    env: Environment<'static>,
}

impl FoguObject for Template {
    fn name(&self) -> &str {
        "Template"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "render" => {
                max_args(args, 1, "render")?;
                let t = self.env.get_template("main").map_err(|e| tmpl_err("render", e))?;
                t.render(context(args.first())).map(Value::str).map_err(|e| tmpl_err("render", e))
            }
            other => Err(format!("method '{}' does not exist on Template (render)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["render"]
    }
}

pub struct Templates {
    env: Environment<'static>,
}

impl FoguObject for Templates {
    fn name(&self) -> &str {
        "Templates"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // render(name, data?) — `{% extends %}`, `{% include %}` and `{% import %}` load from the folder
            "render" => {
                max_args(args, 2, "render")?;
                let name = str_arg(args, 0, "render")?;
                let t = self.env.get_template(&name).map_err(|e| tmpl_err("render", e))?;
                t.render(context(args.get(1))).map(Value::str).map_err(|e| tmpl_err("render", e))
            }
            // renderString(source, data?) — an inline template that may extend/include the folder's ones
            "renderString" => {
                max_args(args, 2, "renderString")?;
                let src = str_arg(args, 0, "renderString")?;
                self.env.render_str(&src, context(args.get(1))).map(Value::str).map_err(|e| tmpl_err("renderString", e))
            }
            "has" => {
                max_args(args, 1, "has")?;
                Ok(Value::Bool(self.env.get_template(&str_arg(args, 0, "has")?).is_ok()))
            }
            other => Err(format!("method '{}' does not exist on Templates (render renderString has)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["render", "renderString", "has"]
    }
}
