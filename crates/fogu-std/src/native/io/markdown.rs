//! `fogu:markdown` — Markdown (CommonMark + the GitHub extensions: tables,
//! strikethrough, task lists, footnotes, `> [!NOTE]` alerts) with
//! pulldown-cmark: HTML, plain text, a tree of nodes, headings with slugs, a
//! table of contents and YAML/TOML front matter.
//!
//! ```fogu
//! import markdown from "fogu:markdown"
//!
//! const src = "---\ntitle: Hello\n---\n# Intro\n\nSome *text* and a [link](https://fogu.dev).\n"
//! const [meta, body] = markdown.frontmatter(src)?
//! print(meta.title)                                    // Hello
//! print(markdown.toHtml(body, { headingIds: true }))   // <h1 id="intro">Intro</h1>…
//! print(markdown.headings(src))                        // [{id: intro, level: 1, text: Intro}]
//! ```
//!
//! Front matter (a `---` YAML or `+++` TOML block on the first line) is
//! skipped by every function; `frontmatter` returns it. Invalid front matter
//! is an `error` value.

use std::cell::RefCell;
use ahash::{AHashMap as HashMap, AHashSet as HashSet};

use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::runtime::value::{ModuleValue, Value, ERR_VALUE_PREFIX};

use crate::native::util::{list, map, max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    // toHtml(text, { gfm = true, sanitize = false, headingIds = false, smart = false })
    e.insert("toHtml".into(), nfn_var("toHtml", 1, |a| {
        max_args(a, 2, "markdown.toHtml")?;
        let text = str_arg(a, 0, "markdown.toHtml")?;
        let o = opts(a, 1, "markdown.toHtml")?;
        let flag = |k: &str, d: bool| o.get(k).map_or(d, |v| v.is_truthy());
        let html = to_html(
            body_of(&text),
            HtmlOpts {
                gfm: flag("gfm", true),
                sanitize: flag("sanitize", false),
                heading_ids: flag("headingIds", false),
                smart: flag("smart", false),
            },
        );
        Ok(Value::str(html))
    }));
    e.insert("toText".into(), nfn_var("toText", 1, |a| {
        max_args(a, 1, "markdown.toText")?;
        let text = str_arg(a, 0, "markdown.toText")?;
        Ok(Value::str(blocks_text(&tree(body_of(&text))).trim().to_string()))
    }));
    e.insert("parse".into(), nfn_var("parse", 1, |a| {
        max_args(a, 1, "markdown.parse")?;
        let text = str_arg(a, 0, "markdown.parse")?;
        Ok(list(tree(body_of(&text)).iter().map(node_value).collect()))
    }));
    e.insert("headings".into(), nfn_var("headings", 1, |a| {
        max_args(a, 1, "markdown.headings")?;
        let text = str_arg(a, 0, "markdown.headings")?;
        let items = headings(body_of(&text))
            .into_iter()
            .map(|h| map(vec![("level", Value::Int(h.level as i64)), ("text", Value::str(h.text)), ("id", Value::str(h.id))]))
            .collect();
        Ok(list(items))
    }));
    // toc(text, { minLevel = 1, maxLevel = 6 }) → a nested Markdown list of links
    e.insert("toc".into(), nfn_var("toc", 1, |a| {
        max_args(a, 2, "markdown.toc")?;
        let text = str_arg(a, 0, "markdown.toc")?;
        let o = opts(a, 1, "markdown.toc")?;
        let level = |k: &str, d: i64| match o.get(k) {
            None | Some(Value::Nil) => Ok(d),
            Some(Value::Int(n)) => Ok(*n),
            Some(other) => Err(format!("markdown.toc: '{}' must be int, got '{}'", k, other.type_name())),
        };
        let (min, max) = (level("minLevel", 1)?, level("maxLevel", 6)?);
        let hs: Vec<Heading> =
            headings(body_of(&text)).into_iter().filter(|h| (min..=max).contains(&(h.level as i64))).collect();
        let base = hs.iter().map(|h| h.level).min().unwrap_or(1);
        let lines: Vec<String> = hs
            .iter()
            .map(|h| {
                let label = h.text.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
                format!("{}- [{}](#{})", "  ".repeat((h.level - base) as usize), label, h.id)
            })
            .collect();
        Ok(Value::str(lines.join("\n")))
    }));
    e.insert("slug".into(), nfn_var("slug", 1, |a| {
        max_args(a, 1, "markdown.slug")?;
        Ok(Value::str(slug(&str_arg(a, 0, "markdown.slug")?)))
    }));
    // frontmatter(text) → [meta, body]; meta is {} when there is no front matter
    e.insert("frontmatter".into(), nfn_var("frontmatter", 1, |a| {
        max_args(a, 1, "markdown.frontmatter")?;
        let text = str_arg(a, 0, "markdown.frontmatter")?;
        let Some(fm) = split_frontmatter(&text) else {
            return Ok(list(vec![map(vec![]), Value::str(text)]));
        };
        let parsed = if fm.toml {
            crate::native::formats::toml_parse(fm.meta)
        } else {
            crate::native::formats::yaml_parse(fm.meta)
        };
        let meta = parsed.map_err(|e| io_err!("markdown.frontmatter: {}", e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e)))?;
        let meta = if matches!(meta, Value::Nil) { map(vec![]) } else { meta };
        Ok(list(vec![meta, Value::str(fm.body)]))
    }));
    ModuleValue {
        name: "fogu:markdown".into(),
        exports: RefCell::new(e),
    }
}

/// The options map at argument `i` (missing or nil → empty).
fn opts(a: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match opt_arg(a, i) {
        None => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn options(gfm: bool, smart: bool) -> Options {
    let mut o = Options::empty();
    if gfm {
        o |= Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_GFM;
    }
    if smart {
        o |= Options::ENABLE_SMART_PUNCTUATION;
    }
    o
}

// =====================================================================
// Front matter
// =====================================================================

struct FrontMatter<'a> {
    toml: bool,
    meta: &'a str,
    body: &'a str,
}

/// A `---` (YAML, closed by `---` or `...`) or `+++` (TOML) block that starts
/// on the first line.
fn split_frontmatter(text: &str) -> Option<FrontMatter<'_>> {
    let t = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let first_end = t.find('\n')?;
    let fence = t[..first_end].trim_end();
    if fence != "---" && fence != "+++" {
        return None;
    }
    let mut pos = first_end + 1;
    while pos <= t.len() {
        let end = t[pos..].find('\n').map_or(t.len(), |i| pos + i);
        let line = t[pos..end].trim_end();
        if line == fence || (fence == "---" && line == "...") {
            let body = if end < t.len() { &t[end + 1..] } else { "" };
            return Some(FrontMatter { toml: fence == "+++", meta: &t[first_end + 1..pos], body });
        }
        if end == t.len() {
            break;
        }
        pos = end + 1;
    }
    None
}

/// The text without its front matter.
fn body_of(text: &str) -> &str {
    split_frontmatter(text).map_or(text, |f| f.body)
}

// =====================================================================
// Slugs
// =====================================================================

/// GitHub-style anchor: lowercase, spaces → `-`, punctuation dropped.
pub fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// Unique slugs within one document: `intro`, `intro-1`, `intro-2`…
#[derive(Default)]
struct Slugger {
    used: HashSet<String>,
}

impl Slugger {
    fn make(&mut self, text: &str) -> String {
        let mut base = slug(text);
        if base.is_empty() {
            base = "section".into();
        }
        let mut candidate = base.clone();
        let mut n = 0;
        while self.used.contains(&candidate) {
            n += 1;
            candidate = format!("{}-{}", base, n);
        }
        self.used.insert(candidate.clone());
        candidate
    }
}

// =====================================================================
// HTML
// =====================================================================

#[derive(Clone, Copy)]
pub struct HtmlOpts {
    pub gfm: bool,
    pub sanitize: bool,
    pub heading_ids: bool,
    pub smart: bool,
}

/// URLs a sanitized document may not link to (scripts, local files, non-image data).
fn unsafe_url(url: &str) -> bool {
    let u: String = url.chars().filter(|c| !c.is_ascii_whitespace() && !c.is_ascii_control()).collect::<String>().to_lowercase();
    if let Some(data) = u.strip_prefix("data:") {
        return !["image/png", "image/gif", "image/jpeg", "image/webp"].iter().any(|t| data.starts_with(t));
    }
    u.starts_with("javascript:") || u.starts_with("vbscript:") || u.starts_with("file:")
}

/// Markdown → HTML. `sanitize` escapes raw HTML and neutralizes unsafe link
/// targets; `heading_ids` gives every heading an `id` (its unique slug).
pub fn to_html(md: &str, o: HtmlOpts) -> String {
    let events: Vec<Event> = Parser::new_ext(md, options(o.gfm, o.smart)).collect();
    let mut slugs = Slugger::default();
    let mut out = Vec::with_capacity(events.len());
    for (i, ev) in events.iter().enumerate() {
        let ev = match ev {
            Event::Start(Tag::Heading { level, id: None, classes, attrs }) if o.heading_ids => {
                let id = slugs.make(&heading_text(&events[i + 1..]));
                Event::Start(Tag::Heading { level: *level, id: Some(id.into()), classes: classes.clone(), attrs: attrs.clone() })
            }
            Event::Html(s) | Event::InlineHtml(s) if o.sanitize => Event::Text(s.clone()),
            Event::Start(Tag::Link { link_type, dest_url, title, id }) if o.sanitize && unsafe_url(dest_url) => {
                Event::Start(Tag::Link { link_type: *link_type, dest_url: "#".into(), title: title.clone(), id: id.clone() })
            }
            Event::Start(Tag::Image { link_type, dest_url, title, id }) if o.sanitize && unsafe_url(dest_url) => {
                Event::Start(Tag::Image { link_type: *link_type, dest_url: "#".into(), title: title.clone(), id: id.clone() })
            }
            other => other.clone(),
        };
        out.push(ev);
    }
    let mut html = String::with_capacity(md.len() * 3 / 2);
    pulldown_cmark::html::push_html(&mut html, out.into_iter());
    html
}

/// The plain text of a heading, from the events after its start (same rules as `plain`).
fn heading_text(events: &[Event]) -> String {
    let mut s = String::new();
    for ev in events {
        match ev {
            Event::End(TagEnd::Heading(_)) => break,
            Event::Text(t) | Event::Code(t) => s.push_str(t),
            Event::SoftBreak | Event::HardBreak => s.push('\n'),
            _ => {}
        }
    }
    s
}

// =====================================================================
// The tree of nodes (parse, toText, headings)
// =====================================================================

#[derive(Default)]
struct Node {
    ty: &'static str,
    /// text / code / html / codeBlock content, image alt, footnote label
    text: String,
    level: u8,
    id: String,
    lang: String,
    url: String,
    title: String,
    kind: &'static str,
    ordered: bool,
    start: u64,
    checked: Option<bool>,
    header: bool,
    align: Vec<&'static str>,
    children: Vec<Node>,
}

fn node(ty: &'static str) -> Node {
    Node { ty, ..Default::default() }
}

fn open(tag: Tag) -> Node {
    match tag {
        Tag::Paragraph => node("paragraph"),
        Tag::Heading { level, .. } => Node { level: level as u8, ..node("heading") },
        Tag::BlockQuote(kind) => Node {
            kind: match kind {
                None => "",
                Some(BlockQuoteKind::Note) => "note",
                Some(BlockQuoteKind::Tip) => "tip",
                Some(BlockQuoteKind::Important) => "important",
                Some(BlockQuoteKind::Warning) => "warning",
                Some(BlockQuoteKind::Caution) => "caution",
            },
            ..node("blockquote")
        },
        Tag::CodeBlock(CodeBlockKind::Fenced(info)) => {
            Node { lang: info.split_whitespace().next().unwrap_or("").to_string(), ..node("codeBlock") }
        }
        Tag::CodeBlock(CodeBlockKind::Indented) => node("codeBlock"),
        Tag::HtmlBlock => node("html"),
        Tag::List(start) => Node { ordered: start.is_some(), start: start.unwrap_or(1), ..node("list") },
        Tag::Item => node("item"),
        Tag::FootnoteDefinition(label) => Node { text: label.to_string(), ..node("footnote") },
        Tag::DefinitionList => node("definitionList"),
        Tag::DefinitionListTitle => node("term"),
        Tag::DefinitionListDefinition => node("definition"),
        Tag::Table(align) => Node {
            align: align
                .iter()
                .map(|a| match a {
                    Alignment::None => "none",
                    Alignment::Left => "left",
                    Alignment::Center => "center",
                    Alignment::Right => "right",
                })
                .collect(),
            ..node("table")
        },
        Tag::TableHead => Node { header: true, ..node("row") },
        Tag::TableRow => node("row"),
        Tag::TableCell => node("cell"),
        Tag::Emphasis => node("emphasis"),
        Tag::Strong => node("strong"),
        Tag::Strikethrough => node("strikethrough"),
        Tag::Superscript => node("superscript"),
        Tag::Subscript => node("subscript"),
        Tag::Link { dest_url, title, .. } => Node { url: dest_url.to_string(), title: title.to_string(), ..node("link") },
        Tag::Image { dest_url, title, .. } => Node { url: dest_url.to_string(), title: title.to_string(), ..node("image") },
        Tag::MetadataBlock(_) => node("metadata"),
    }
}

/// Appends text to `parent`: into its content (code and HTML blocks) or as a
/// text node merged with the previous one.
fn push_text(parent: &mut Node, s: &str) {
    if matches!(parent.ty, "codeBlock" | "html") {
        parent.text.push_str(s);
        return;
    }
    if let Some(last) = parent.children.last_mut().filter(|n| n.ty == "text") {
        last.text.push_str(s);
        return;
    }
    parent.children.push(Node { text: s.to_string(), ..node("text") });
}

/// The block nodes of a document (front matter already removed).
fn tree(md: &str) -> Vec<Node> {
    let mut stack = vec![node("root")];
    let mut slugs = Slugger::default();
    for ev in Parser::new_ext(md, options(true, false)) {
        match ev {
            Event::Start(tag) => stack.push(open(tag)),
            Event::End(_) => {
                let mut n = stack.pop().expect("markdown: unbalanced events");
                match n.ty {
                    "heading" => n.id = slugs.make(&plain(&n.children)),
                    "image" => {
                        n.text = plain(&n.children);
                        n.children.clear();
                    }
                    "codeBlock" | "html" => {
                        if n.text.ends_with('\n') {
                            n.text.pop();
                        }
                    }
                    _ => {}
                }
                if n.ty != "metadata" {
                    stack.last_mut().expect("markdown: no parent").children.push(n);
                }
            }
            Event::Text(s) => push_text(stack.last_mut().unwrap(), &s),
            Event::Html(s) => push_text(stack.last_mut().unwrap(), &s),
            Event::InlineHtml(s) => stack.last_mut().unwrap().children.push(Node { text: s.to_string(), ..node("html") }),
            Event::Code(s) | Event::InlineMath(s) | Event::DisplayMath(s) => {
                stack.last_mut().unwrap().children.push(Node { text: s.to_string(), ..node("code") })
            }
            Event::FootnoteReference(label) => {
                stack.last_mut().unwrap().children.push(Node { text: label.to_string(), ..node("footnoteRef") })
            }
            Event::SoftBreak => push_text(stack.last_mut().unwrap(), "\n"),
            Event::HardBreak => stack.last_mut().unwrap().children.push(node("break")),
            Event::Rule => stack.last_mut().unwrap().children.push(node("rule")),
            Event::TaskListMarker(done) => {
                if let Some(item) = stack.iter_mut().rev().find(|n| n.ty == "item") {
                    item.checked = Some(done);
                }
            }
        }
    }
    stack.truncate(1);
    stack.pop().map(|r| r.children).unwrap_or_default()
}

/// The text of inline nodes (text, code, image alt, line breaks).
fn plain(nodes: &[Node]) -> String {
    let mut s = String::new();
    for n in nodes {
        match n.ty {
            "text" | "code" | "image" => s.push_str(&n.text),
            "break" => s.push('\n'),
            "html" | "footnoteRef" => {}
            _ => s.push_str(&plain(&n.children)),
        }
    }
    s
}

fn node_value(n: &Node) -> Value {
    let children = || list(n.children.iter().map(node_value).collect());
    let mut f: Vec<(&str, Value)> = vec![("type", Value::str(n.ty))];
    match n.ty {
        "text" | "code" | "html" => f.push(("text", Value::str(n.text.as_str()))),
        "codeBlock" => {
            f.push(("lang", Value::str(n.lang.as_str())));
            f.push(("text", Value::str(n.text.as_str())));
        }
        "heading" => {
            f.push(("level", Value::Int(n.level as i64)));
            f.push(("id", Value::str(n.id.as_str())));
            f.push(("children", children()));
        }
        "blockquote" => {
            if !n.kind.is_empty() {
                f.push(("kind", Value::str(n.kind)));
            }
            f.push(("children", children()));
        }
        "list" => {
            f.push(("ordered", Value::Bool(n.ordered)));
            f.push(("start", Value::Int(n.start as i64)));
            f.push(("children", children()));
        }
        "item" => {
            if let Some(c) = n.checked {
                f.push(("checked", Value::Bool(c)));
            }
            f.push(("children", children()));
        }
        "link" => {
            f.push(("url", Value::str(n.url.as_str())));
            f.push(("title", Value::str(n.title.as_str())));
            f.push(("children", children()));
        }
        "image" => {
            f.push(("url", Value::str(n.url.as_str())));
            f.push(("title", Value::str(n.title.as_str())));
            f.push(("alt", Value::str(n.text.as_str())));
        }
        "footnote" => {
            f.push(("label", Value::str(n.text.as_str())));
            f.push(("children", children()));
        }
        "footnoteRef" => f.push(("label", Value::str(n.text.as_str()))),
        "table" => {
            f.push(("align", list(n.align.iter().map(|a| Value::str(*a)).collect())));
            f.push(("children", children()));
        }
        "row" => {
            f.push(("header", Value::Bool(n.header)));
            f.push(("children", children()));
        }
        "rule" | "break" => {}
        _ => f.push(("children", children())),
    }
    map(f)
}

// ---------- toText ----------

fn is_block(ty: &str) -> bool {
    matches!(
        ty,
        "paragraph" | "heading" | "codeBlock" | "blockquote" | "list" | "table" | "rule" | "footnote" | "definitionList"
    )
}

/// Inline nodes as text: links keep their text, images their alt text.
fn inline_text(nodes: &[Node]) -> String {
    let mut s = String::new();
    for n in nodes {
        match n.ty {
            "text" | "code" | "image" => s.push_str(&n.text),
            "break" => s.push('\n'),
            "html" => {}
            "footnoteRef" => {
                s.push('[');
                s.push_str(&n.text);
                s.push(']');
            }
            _ => s.push_str(&inline_text(&n.children)),
        }
    }
    s
}

/// Block nodes as text, separated by blank lines.
fn blocks_text(nodes: &[Node]) -> String {
    let parts: Vec<String> = nodes.iter().map(block_text).filter(|p| !p.trim().is_empty()).collect();
    parts.join("\n\n")
}

/// The parts of a list item or definition: runs of inline nodes and blocks, one per line.
fn mixed_text(nodes: &[Node]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut run: Vec<&Node> = Vec::new();
    let flush = |run: &mut Vec<&Node>, parts: &mut Vec<String>| {
        if !run.is_empty() {
            let owned: String = run.iter().map(|n| inline_text(std::slice::from_ref(*n))).collect();
            if !owned.trim().is_empty() {
                parts.push(owned.trim().to_string());
            }
            run.clear();
        }
    };
    for n in nodes {
        if is_block(n.ty) {
            flush(&mut run, &mut parts);
            let t = block_text(n);
            if !t.trim().is_empty() {
                parts.push(t);
            }
        } else {
            run.push(n);
        }
    }
    flush(&mut run, &mut parts);
    parts.join("\n")
}

fn block_text(n: &Node) -> String {
    match n.ty {
        "paragraph" | "heading" | "cell" | "term" => inline_text(&n.children).trim().to_string(),
        "codeBlock" => n.text.clone(),
        "html" | "rule" => String::new(),
        "blockquote" | "definition" => blocks_text(&n.children),
        "definitionList" => n.children.iter().map(block_text).collect::<Vec<_>>().join("\n"),
        "list" => {
            let mut lines = Vec::new();
            for (i, item) in n.children.iter().enumerate() {
                let marker = if n.ordered { format!("{}. ", n.start + i as u64) } else { "- ".to_string() };
                let check = match item.checked {
                    Some(true) => "[x] ",
                    Some(false) => "[ ] ",
                    None => "",
                };
                let pad = " ".repeat(marker.chars().count());
                let body = mixed_text(&item.children);
                let mut first = true;
                for line in body.split('\n') {
                    if first {
                        lines.push(format!("{}{}{}", marker, check, line));
                        first = false;
                    } else if line.is_empty() {
                        lines.push(String::new());
                    } else {
                        lines.push(format!("{}{}", pad, line));
                    }
                }
                if first {
                    lines.push(format!("{}{}", marker, check).trim_end().to_string());
                }
            }
            lines.join("\n")
        }
        "table" => n
            .children
            .iter()
            .map(|row| row.children.iter().map(block_text).collect::<Vec<_>>().join(" | "))
            .collect::<Vec<_>>()
            .join("\n"),
        "footnote" => format!("[{}]: {}", n.text, mixed_text(&n.children)),
        _ => mixed_text(std::slice::from_ref(n)),
    }
}

// ---------- headings ----------

struct Heading {
    level: u8,
    text: String,
    id: String,
}

fn headings(md: &str) -> Vec<Heading> {
    fn walk(nodes: &[Node], out: &mut Vec<Heading>) {
        for n in nodes {
            if n.ty == "heading" {
                let text = plain(&n.children).replace('\n', " ").trim().to_string();
                out.push(Heading { level: n.level, text, id: n.id.clone() });
            } else {
                walk(&n.children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree(md), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_frontmatter() {
        assert_eq!(slug(" Hello, World! "), "hello-world");
        let mut s = Slugger::default();
        assert_eq!((s.make("A"), s.make("A"), s.make("!!")), ("a".into(), "a-1".into(), "section".into()));
        let fm = split_frontmatter("---\na: 1\n...\nbody\n").unwrap();
        assert_eq!((fm.toml, fm.meta, fm.body), (false, "a: 1\n", "body\n"));
        assert!(split_frontmatter("text\n---\n").is_none());
        assert_eq!(split_frontmatter("+++\nx = 1\n+++").unwrap().body, "");
    }

    #[test]
    fn html_ids_and_sanitize() {
        let o = HtmlOpts { gfm: true, sanitize: true, heading_ids: true, smart: false };
        assert_eq!(
            to_html("# Hi\n\n<b>x</b> [a](javascript:alert(1))\n", o),
            "<h1 id=\"hi\">Hi</h1>\n<p>&lt;b&gt;x&lt;/b&gt; <a href=\"#\">a</a></p>\n"
        );
    }
}
