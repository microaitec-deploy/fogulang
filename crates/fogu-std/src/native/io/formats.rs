//! Data formats: `fogu:csv`, `fogu:excel`, `fogu:yaml`, `fogu:toml` and
//! `fogu:xml` — each converts text (or files) to Fogu values and back.
//!
//! ```fogu
//! import csv from "fogu:csv"
//! import excel from "fogu:excel"
//! import yaml from "fogu:yaml"
//! import toml from "fogu:toml"
//! import xml from "fogu:xml"
//!
//! const people = csv.read("people.csv")?               // [{ name: "Ana", age: 30 }, …]
//! excel.write("people.xlsx", { People: people })?        // one sheet per key
//! print(excel.read("people.xlsx")?[0].name)              // first sheet → list of maps
//! yaml.write("people.yaml", people)?
//! const cfg = toml.parse("[server]\nport = 8080")?       // { server: { port: 8080 } }
//! print(xml.parse("<a x=\"1\"><b>hi</b></a>")?.children[0].text)
//! ```
//!
//! Malformed input and I/O failures are `error` values.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use serde_json::Value as J;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::util::{json_to_value, list, map, max_args, nfn_var, str_arg};

fn module(name: &str, fns: Vec<(&str, usize, fn(&[Value]) -> Result<Value, String>)>) -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    for (n, min, f) in fns {
        e.insert(n.to_string(), nfn_var(n, min, f));
    }
    ModuleValue {
        name: format!("fogu:{}", name),
        exports: RefCell::new(e),
    }
}

fn to_json(v: &Value) -> J {
    serde_json::from_str(&stringify_value(v)).unwrap_or(J::Null)
}

fn opts(a: &[Value], i: usize) -> HashMap<String, Value> {
    match a.get(i) {
        Some(Value::Map(m)) => m.borrow().clone(),
        _ => HashMap::new(),
    }
}

fn flag(o: &HashMap<String, Value>, k: &str, default: bool) -> bool {
    o.get(k).map_or(default, |v| v.is_truthy())
}

fn read_text(path: &str, what: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| io_err!("{}: {}: {}", what, path, e))
}

fn write_text(path: &str, text: &str, what: &str) -> Result<Value, String> {
    if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| io_err!("{}: {}: {}", what, path, e))?;
    }
    std::fs::write(path, text).map_err(|e| io_err!("{}: {}: {}", what, path, e))?;
    Ok(Value::Nil)
}

/// Text cell → int, float, bool, nil (empty) or str.
fn infer(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        return Value::Nil;
    }
    // keep codes like "007" or "+55…" as text
    let leading_zero = t.len() > 1 && t.starts_with('0') && !t.starts_with("0.");
    if !leading_zero && !t.starts_with('+') {
        if let Ok(n) = t.parse::<i64>() {
            return Value::Int(n);
        }
        // digits with a dot and/or an exponent (not "inf", "nan"…)
        let numeric = t.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c)) && t.chars().any(|c| c.is_ascii_digit());
        if numeric {
            if let Ok(f) = t.parse::<f64>() {
                return Value::Float(f);
            }
        }
    }
    match t.to_lowercase().as_str() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::Str(s.to_string().into()),
    }
}

/// A cell as text for CSV/Excel output.
fn cell_text(v: &Value) -> String {
    match v {
        Value::Nil => String::new(),
        Value::Str(s) => s.to_string(),
        Value::Map(_) | Value::List(_) => stringify_value(v),
        other => other.to_display_string(),
    }
}

/// Column names: the `columns` option, or every key in first-seen order.
fn columns(rows: &[Value], o: &HashMap<String, Value>) -> Vec<String> {
    if let Some(Value::List(c)) = o.get("columns") {
        return c.borrow().iter().map(|v| v.to_display_string()).collect();
    }
    let mut cols: Vec<String> = Vec::new();
    for r in rows {
        if let Value::Map(m) = r {
            let b = m.borrow();
            let mut keys: Vec<&String> = b.keys().collect();
            keys.sort();
            for k in keys {
                if !cols.contains(k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    cols
}

pub(crate) fn rows_arg(v: &Value, what: &str) -> Result<Vec<Value>, String> {
    match v {
        Value::List(l) => Ok(l.borrow().clone()),
        other => Err(format!("{}: expected a list of rows, got '{}'", what, other.type_name())),
    }
}

// =====================================================================
// CSV
// =====================================================================

pub fn csv_build() -> ModuleValue {
    module(
        "csv",
        vec![
            ("parse", 1, |a| {
                max_args(a, 2, "csv.parse")?;
                csv_parse(&str_arg(a, 0, "csv.parse")?, &opts(a, 1))
            }),
            ("stringify", 1, |a| {
                max_args(a, 2, "csv.stringify")?;
                csv_stringify(&rows_arg(&a[0], "csv.stringify")?, &opts(a, 1)).map(Value::str)
            }),
            ("read", 1, |a| {
                max_args(a, 2, "csv.read")?;
                csv_parse(&read_text(&str_arg(a, 0, "csv.read")?, "csv.read")?, &opts(a, 1))
            }),
            ("write", 2, |a| {
                max_args(a, 3, "csv.write")?;
                let text = csv_stringify(&rows_arg(&a[1], "csv.write")?, &opts(a, 2))?;
                write_text(&str_arg(a, 0, "csv.write")?, &text, "csv.write")
            }),
        ],
    )
}

fn delimiter(o: &HashMap<String, Value>, text: Option<&str>) -> Result<u8, String> {
    match o.get("delimiter") {
        Some(Value::Str(d)) if d.len() == 1 => Ok(d.as_bytes()[0]),
        Some(Value::Str(d)) if d == "\\t" || d == "tab" => Ok(b'\t'),
        Some(other) => Err(format!("csv: delimiter must be one character, got '{}'", other.to_display_string())),
        // guess from the first line: , ; tab |
        None => Ok(text
            .and_then(|t| t.lines().next())
            .and_then(|first| [b',', b';', b'\t', b'|'].into_iter().max_by_key(|d| first.bytes().filter(|b| b == d).count()))
            .filter(|d| text.unwrap().lines().next().unwrap().as_bytes().contains(d))
            .unwrap_or(b',')),
    }
}

/// `{ header = true, delimiter (guessed), types = true, trim = false }`
pub(crate) fn csv_parse(text: &str, o: &HashMap<String, Value>) -> Result<Value, String> {
    let header = flag(o, "header", true);
    let types = flag(o, "types", true);
    let trim = flag(o, "trim", false);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut r = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter(o, Some(text))?)
        .trim(if trim { csv::Trim::All } else { csv::Trim::None })
        .from_reader(text.as_bytes());
    let mut records = r.records();
    let cell = |s: &str| if types { infer(s) } else { Value::Str(s.to_string().into()) };
    let mut out = Vec::new();
    let names: Option<Vec<String>> = if header {
        match records.next() {
            Some(rec) => Some(rec.map_err(|e| io_err!("csv: {}", e))?.iter().map(|h| h.trim().to_string()).collect()),
            None => return Ok(list(Vec::new())),
        }
    } else {
        None
    };
    for rec in records {
        let rec = rec.map_err(|e| io_err!("csv: {}", e))?;
        out.push(match &names {
            Some(names) => {
                let mut m: HashMap<String, Value> = HashMap::new();
                for (i, name) in names.iter().enumerate() {
                    m.insert(name.clone(), rec.get(i).map_or(Value::Nil, cell));
                }
                Value::Map(Rc::new(RefCell::new(m)))
            }
            None => list(rec.iter().map(cell).collect()),
        });
    }
    Ok(list(out))
}

/// rows: maps (a header line is written) or lists. `{ header = true, delimiter = ",", columns }`
pub(crate) fn csv_stringify(rows: &[Value], o: &HashMap<String, Value>) -> Result<String, String> {
    let mut w = csv::WriterBuilder::new().delimiter(delimiter(o, None)?).from_writer(Vec::new());
    let maps = rows.iter().any(|r| matches!(r, Value::Map(_)));
    let cols = if maps { columns(rows, o) } else { Vec::new() };
    if maps && flag(o, "header", true) {
        w.write_record(&cols).map_err(|e| io_err!("csv: {}", e))?;
    }
    for r in rows {
        let cells: Vec<String> = match r {
            Value::Map(m) => {
                let m = m.borrow();
                cols.iter().map(|c| m.get(c).map_or(String::new(), cell_text)).collect()
            }
            Value::List(l) => l.borrow().iter().map(cell_text).collect(),
            other => vec![cell_text(other)],
        };
        w.write_record(&cells).map_err(|e| io_err!("csv: {}", e))?;
    }
    let bytes = w.into_inner().map_err(|e| io_err!("csv: {}", e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

// =====================================================================
// YAML
// =====================================================================

pub fn yaml_build() -> ModuleValue {
    module(
        "yaml",
        vec![
            ("parse", 1, |a| {
                max_args(a, 1, "yaml.parse")?;
                yaml_parse(&str_arg(a, 0, "yaml.parse")?)
            }),
            // parseAll(text) → every document of a multi-document stream (---)
            ("parseAll", 1, |a| {
                max_args(a, 1, "yaml.parseAll")?;
                let text = str_arg(a, 0, "yaml.parseAll")?;
                let mut docs = Vec::new();
                for d in serde_yaml::Deserializer::from_str(&text) {
                    let v: J = <J as serde::Deserialize>::deserialize(d).map_err(|e| io_err!("yaml: {}", e))?;
                    docs.push(json_to_value(&v));
                }
                Ok(list(docs))
            }),
            ("stringify", 1, |a| {
                max_args(a, 1, "yaml.stringify")?;
                serde_yaml::to_string(&to_json(&a[0])).map(Value::str).map_err(|e| io_err!("yaml: {}", e))
            }),
            ("read", 1, |a| {
                max_args(a, 1, "yaml.read")?;
                yaml_parse(&read_text(&str_arg(a, 0, "yaml.read")?, "yaml.read")?)
            }),
            ("write", 2, |a| {
                max_args(a, 2, "yaml.write")?;
                let text = serde_yaml::to_string(&to_json(&a[1])).map_err(|e| io_err!("yaml: {}", e))?;
                write_text(&str_arg(a, 0, "yaml.write")?, &text, "yaml.write")
            }),
        ],
    )
}

fn yaml_parse(text: &str) -> Result<Value, String> {
    let v: J = serde_yaml::from_str(text).map_err(|e| io_err!("yaml: {}", e))?;
    Ok(json_to_value(&v))
}

// =====================================================================
// TOML
// =====================================================================

pub fn toml_build() -> ModuleValue {
    module(
        "toml",
        vec![
            ("parse", 1, |a| {
                max_args(a, 1, "toml.parse")?;
                toml_parse(&str_arg(a, 0, "toml.parse")?)
            }),
            ("stringify", 1, |a| {
                max_args(a, 1, "toml.stringify")?;
                toml_stringify(&a[0]).map(Value::str)
            }),
            ("read", 1, |a| {
                max_args(a, 1, "toml.read")?;
                toml_parse(&read_text(&str_arg(a, 0, "toml.read")?, "toml.read")?)
            }),
            ("write", 2, |a| {
                max_args(a, 2, "toml.write")?;
                let text = toml_stringify(&a[1])?;
                write_text(&str_arg(a, 0, "toml.write")?, &text, "toml.write")
            }),
        ],
    )
}

fn toml_to_json(v: toml::Value) -> J {
    match v {
        toml::Value::String(s) => J::String(s),
        toml::Value::Integer(n) => J::from(n),
        toml::Value::Float(f) => serde_json::Number::from_f64(f).map_or(J::Null, J::Number),
        toml::Value::Boolean(b) => J::Bool(b),
        toml::Value::Datetime(d) => J::String(d.to_string()),
        toml::Value::Array(a) => J::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => J::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect()),
    }
}

fn toml_parse(text: &str) -> Result<Value, String> {
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| io_err!("toml: {}", e.to_string().trim()))?;
    Ok(json_to_value(&toml_to_json(toml::Value::Table(t))))
}

/// TOML has no null: nil values are left out.
fn json_to_toml(v: &J) -> Option<toml::Value> {
    Some(match v {
        J::Null => return None,
        J::Bool(b) => toml::Value::Boolean(*b),
        J::Number(n) => match n.as_i64() {
            Some(i) => toml::Value::Integer(i),
            None => toml::Value::Float(n.as_f64().unwrap_or(0.0)),
        },
        J::String(s) => toml::Value::String(s.clone()),
        J::Array(a) => toml::Value::Array(a.iter().filter_map(json_to_toml).collect()),
        J::Object(o) => toml::Value::Table(o.iter().filter_map(|(k, v)| json_to_toml(v).map(|t| (k.clone(), t))).collect()),
    })
}

fn toml_stringify(v: &Value) -> Result<String, String> {
    match json_to_toml(&to_json(v)) {
        Some(toml::Value::Table(t)) => toml::to_string_pretty(&t).map_err(|e| io_err!("toml: {}", e)),
        _ => Err("toml.stringify: the top level must be a map".into()),
    }
}

// =====================================================================
// XML
// =====================================================================

pub fn xml_build() -> ModuleValue {
    module(
        "xml",
        vec![
            ("parse", 1, |a| {
                max_args(a, 2, "xml.parse")?;
                xml_parse(&str_arg(a, 0, "xml.parse")?, &opts(a, 1))
            }),
            ("stringify", 1, |a| {
                max_args(a, 2, "xml.stringify")?;
                xml_stringify(&a[0], &opts(a, 1)).map(Value::str)
            }),
            ("read", 1, |a| {
                max_args(a, 2, "xml.read")?;
                xml_parse(&read_text(&str_arg(a, 0, "xml.read")?, "xml.read")?, &opts(a, 1))
            }),
            ("write", 2, |a| {
                max_args(a, 3, "xml.write")?;
                let text = xml_stringify(&a[1], &opts(a, 2))?;
                write_text(&str_arg(a, 0, "xml.write")?, &text, "xml.write")
            }),
        ],
    )
}

/// The root element as `{ name, attrs, children, text }`; children are
/// elements (maps) or text (str). `text` joins the direct text of the element.
fn xml_parse(text: &str, o: &HashMap<String, Value>) -> Result<Value, String> {
    use quick_xml::events::Event;
    let keep_ws = flag(o, "whitespace", false);
    // whitespace is trimmed here, not by the reader: quick-xml splits text around
    // entity references (`a &amp; b` → "a ", "&", " b"), so pieces are joined first
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(false);
    // stack of (name, attrs, children, text)
    let mut stack: Vec<(String, Vec<(String, Value)>, Vec<Value>, String)> = Vec::new();
    let mut root: Option<Value> = None;
    let mut pending = String::new();
    let err = |e: &dyn std::fmt::Display, pos: u64| io_err!("xml: {} (at byte {})", e, pos);
    let element = |name: String, attrs: Vec<(String, Value)>, children: Vec<Value>, text: String| {
        map(vec![
            ("name", Value::Str(name.into())),
            ("attrs", map(attrs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect())),
            ("children", list(children)),
            ("text", Value::Str(text.into())),
        ])
    };
    let attrs_of = |e: &quick_xml::events::BytesStart| -> Result<Vec<(String, Value)>, String> {
        let mut out = Vec::new();
        for a in e.attributes() {
            let a = a.map_err(|x| io_err!("xml: {}", x))?;
            let v = quick_xml::escape::unescape(&a.value).map_err(|x| io_err!("xml: {}", x))?.into_owned();
            out.push((a.key.as_ref().to_string(), Value::Str(v.into())));
        }
        Ok(out)
    };
    // the text gathered since the last tag becomes one child of the open element
    let flush = |pending: &mut String, stack: &mut Vec<(String, Vec<(String, Value)>, Vec<Value>, String)>| {
        let s = if keep_ws { pending.as_str() } else { pending.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) };
        if let Some(top) = stack.last_mut() {
            if !s.is_empty() {
                top.3.push_str(s);
                top.2.push(Value::Str(s.to_string().into()));
            }
        }
        pending.clear();
    };
    loop {
        let pos = reader.buffer_position();
        let ev = reader.read_event().map_err(|e| err(&e, pos))?;
        if !matches!(ev, Event::Text(_) | Event::GeneralRef(_)) {
            flush(&mut pending, &mut stack);
        }
        match ev {
            Event::Start(e) => {
                let name = e.name().as_ref().to_string();
                stack.push((name, attrs_of(&e)?, Vec::new(), String::new()));
            }
            Event::Empty(e) => {
                let name = e.name().as_ref().to_string();
                let el = element(name, attrs_of(&e)?, Vec::new(), String::new());
                match stack.last_mut() {
                    Some(parent) => parent.2.push(el),
                    None => root = Some(el),
                }
            }
            Event::End(_) => {
                let (name, attrs, children, text) = stack.pop().ok_or_else(|| io_err!("xml: unexpected closing tag"))?;
                let el = element(name, attrs, children, text);
                match stack.last_mut() {
                    Some(parent) => parent.2.push(el),
                    None => root = Some(el),
                }
            }
            Event::Text(t) => pending.push_str(&t.xml10_content()),
            Event::GeneralRef(r) => match r.resolve_char_ref().map_err(|e| err(&e, pos))? {
                Some(c) => pending.push(c),
                None => match quick_xml::escape::resolve_predefined_entity(&r) {
                    Some(s) => pending.push_str(s),
                    None => return Err(io_err!("xml: unknown entity '&{};' (at byte {})", &*r, pos)),
                },
            },
            Event::CData(t) => {
                let s = t.to_string();
                if let Some(top) = stack.last_mut() {
                    top.3.push_str(&s);
                    top.2.push(Value::Str(s.into()));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(io_err!("xml: unclosed element <{}>", stack.last().unwrap().0));
    }
    root.ok_or_else(|| io_err!("xml: no root element"))
}

fn xml_escape(s: &str, attr: bool) -> String {
    let mut out = s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    if attr {
        out = out.replace('"', "&quot;");
    }
    out
}

fn xml_node(v: &Value, pretty: bool, depth: usize, out: &mut String) -> Result<(), String> {
    let pad = if pretty { "  ".repeat(depth) } else { String::new() };
    match v {
        Value::Str(s) => {
            out.push_str(&pad);
            out.push_str(&xml_escape(s, false));
            if pretty {
                out.push('\n');
            }
        }
        Value::Map(m) => {
            let m = m.borrow();
            let name = match m.get("name") {
                Some(Value::Str(n)) if !n.is_empty() => n.clone(),
                _ => return Err("xml.stringify: an element needs a 'name'".into()),
            };
            out.push_str(&format!("{}<{}", pad, name));
            if let Some(Value::Map(attrs)) = m.get("attrs") {
                let a = attrs.borrow();
                let mut keys: Vec<&String> = a.keys().collect();
                keys.sort();
                for k in keys {
                    out.push_str(&format!(" {}=\"{}\"", k, xml_escape(&a[k].to_display_string(), true)));
                }
            }
            let children: Vec<Value> = match m.get("children") {
                Some(Value::List(c)) => c.borrow().clone(),
                _ => match m.get("text") {
                    Some(Value::Str(t)) if !t.is_empty() => vec![Value::Str(t.clone())],
                    _ => Vec::new(),
                },
            };
            if children.is_empty() {
                out.push_str("/>");
            } else if children.len() == 1 && matches!(children[0], Value::Str(_)) {
                out.push('>');
                out.push_str(&xml_escape(&children[0].to_display_string(), false));
                out.push_str(&format!("</{}>", name));
            } else {
                out.push('>');
                if pretty {
                    out.push('\n');
                }
                for c in &children {
                    xml_node(c, pretty, depth + 1, out)?;
                }
                out.push_str(&format!("{}</{}>", pad, name));
            }
            if pretty {
                out.push('\n');
            }
        }
        other => return Err(format!("xml.stringify: expected an element map or text, got '{}'", other.type_name())),
    }
    Ok(())
}

fn xml_stringify(v: &Value, o: &HashMap<String, Value>) -> Result<String, String> {
    let pretty = flag(o, "pretty", true);
    let mut out = String::new();
    if flag(o, "declaration", true) {
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        if pretty {
            out.push('\n');
        }
    }
    xml_node(v, pretty, 0, &mut out)?;
    Ok(out)
}

// =====================================================================
// Excel
// =====================================================================

pub fn excel_build() -> ModuleValue {
    module(
        "excel",
        vec![
            ("read", 1, |a| {
                max_args(a, 2, "excel.read")?;
                excel_read(&str_arg(a, 0, "excel.read")?, &opts(a, 1))
            }),
            ("sheets", 1, |a| {
                max_args(a, 1, "excel.sheets")?;
                use calamine::Reader;
                let path = str_arg(a, 0, "excel.sheets")?;
                let wb = calamine::open_workbook_auto(&path).map_err(|e| io_err!("excel: {}: {}", path, e))?;
                Ok(crate::native::util::str_list(wb.sheet_names()))
            }),
            ("write", 2, |a| {
                max_args(a, 3, "excel.write")?;
                excel_write(&str_arg(a, 0, "excel.write")?, &a[1], &opts(a, 2))
            }),
        ],
    )
}

/// Excel serial date → ISO text (`2024-03-01` or `2024-03-01T10:30:00`).
fn excel_date(serial: f64) -> String {
    let secs = ((serial - 25_569.0) * 86_400.0).round() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    if rem == 0 {
        format!("{:04}-{:02}-{:02}", y, m, d)
    } else {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
    }
}

fn excel_cell(c: &calamine::Data) -> Value {
    use calamine::Data;
    match c {
        Data::Int(n) => Value::Int(*n),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 9e15 => Value::Int(*f as i64),
        Data::Float(f) => Value::Float(*f),
        Data::String(s) => Value::Str(s.clone().into()),
        Data::Bool(b) => Value::Bool(*b),
        // durations ([hh]:mm:ss) stay numbers; dates and times become ISO text
        Data::DateTime(d) if !format!("{:?}", d).contains("TimeDelta") => Value::Str(excel_date(d.as_f64()).into()),
        Data::DateTime(d) => Value::Float(d.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Value::Str(s.clone().into()),
        Data::Error(e) => Value::Str(format!("#{:?}", e).into()),
        Data::Empty => Value::Nil,
    }
}

/// `{ sheet (name or index; "*" for all), header = true }` → rows (or `{ sheet: rows }`)
fn excel_read(path: &str, o: &HashMap<String, Value>) -> Result<Value, String> {
    use calamine::Reader;
    let mut wb = calamine::open_workbook_auto(path).map_err(|e| io_err!("excel: {}: {}", path, e))?;
    let names = wb.sheet_names();
    let header = flag(o, "header", true);
    let read_sheet = |wb: &mut calamine::Sheets<_>, name: &str| -> Result<Value, String> {
        let range = wb.worksheet_range(name).map_err(|e| io_err!("excel: {}: {}", name, e))?;
        let mut rows = range.rows();
        let mut out = Vec::new();
        let cols: Option<Vec<String>> = if header {
            match rows.next() {
                Some(r) => Some(r.iter().enumerate().map(|(i, c)| match excel_cell(c) {
                    Value::Nil => format!("col{}", i + 1),
                    v => v.to_display_string(),
                }).collect()),
                None => return Ok(list(Vec::new())),
            }
        } else {
            None
        };
        for r in rows {
            if r.iter().all(|c| matches!(c, calamine::Data::Empty)) {
                continue;
            }
            out.push(match &cols {
                Some(cols) => {
                    let m: HashMap<String, Value> = cols.iter().enumerate().map(|(i, c)| (c.clone(), r.get(i).map_or(Value::Nil, excel_cell))).collect();
                    Value::Map(Rc::new(RefCell::new(m)))
                }
                None => list(r.iter().map(excel_cell).collect()),
            });
        }
        Ok(list(out))
    };
    match o.get("sheet") {
        Some(Value::Str(s)) if s == "*" => {
            let mut all = Vec::new();
            for n in &names {
                all.push((n.clone(), read_sheet(&mut wb, n)?));
            }
            Ok(map(all.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()))
        }
        Some(Value::Str(s)) => {
            if !names.contains(s) {
                return Err(io_err!("excel: {}: no sheet '{}' ({})", path, s, names.join(", ")));
            }
            read_sheet(&mut wb, s)
        }
        Some(Value::Int(i)) => {
            let n = names.get(*i as usize).ok_or_else(|| io_err!("excel: {}: no sheet #{}", path, i))?.clone();
            read_sheet(&mut wb, &n)
        }
        _ => {
            let n = names.first().ok_or_else(|| io_err!("excel: {}: the workbook has no sheets", path))?.clone();
            read_sheet(&mut wb, &n)
        }
    }
}

/// `write(path, rows | { Sheet: rows, … }, { header = true, columns })`
fn excel_write(path: &str, data: &Value, o: &HashMap<String, Value>) -> Result<Value, String> {
    use rust_xlsxwriter::{Format, Workbook};
    let sheets: Vec<(String, Vec<Value>)> = match data {
        Value::List(l) => vec![("Sheet1".to_string(), l.borrow().clone())],
        Value::Map(m) => {
            let m = m.borrow();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            keys.into_iter().map(|k| Ok((k.clone(), rows_arg(&m[k], "excel.write")?))).collect::<Result<_, String>>()?
        }
        other => return Err(format!("excel.write: expected rows or {{ sheet: rows }}, got '{}'", other.type_name())),
    };
    let xerr = |e: rust_xlsxwriter::XlsxError| io_err!("excel: {}", e);
    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    for (name, rows) in &sheets {
        let ws = wb.add_worksheet();
        ws.set_name(name).map_err(xerr)?;
        let maps = rows.iter().any(|r| matches!(r, Value::Map(_)));
        let cols = if maps { columns(rows, o) } else { Vec::new() };
        let mut row = 0u32;
        if maps && flag(o, "header", true) {
            for (c, h) in cols.iter().enumerate() {
                ws.write_string_with_format(0, c as u16, h, &bold).map_err(xerr)?;
            }
            ws.set_freeze_panes(1, 0).map_err(xerr)?;
            row = 1;
        }
        for r in rows {
            let cells: Vec<Value> = match r {
                Value::Map(m) => {
                    let m = m.borrow();
                    cols.iter().map(|c| m.get(c).cloned().unwrap_or(Value::Nil)).collect()
                }
                Value::List(l) => l.borrow().clone(),
                other => vec![other.clone()],
            };
            for (c, v) in cells.iter().enumerate() {
                let c = c as u16;
                match v {
                    Value::Nil => {}
                    Value::Int(n) => {
                        ws.write_number(row, c, *n as f64).map_err(xerr)?;
                    }
                    Value::Float(f) => {
                        ws.write_number(row, c, *f).map_err(xerr)?;
                    }
                    Value::Bool(b) => {
                        ws.write_boolean(row, c, *b).map_err(xerr)?;
                    }
                    other => {
                        ws.write_string(row, c, cell_text(other)).map_err(xerr)?;
                    }
                }
            }
            row += 1;
        }
        ws.autofit();
    }
    if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| io_err!("excel: {}: {}", path, e))?;
    }
    wb.save(path).map_err(|e| io_err!("excel: {}: {}", path, e))?;
    Ok(Value::Nil)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inference_and_dates() {
        assert!(matches!(infer("42"), Value::Int(42)));
        assert!(matches!(infer("3.5"), Value::Float(f) if f == 3.5));
        assert!(matches!(infer("007"), Value::Str(_)));
        assert!(matches!(infer("+5511"), Value::Str(_)));
        assert!(matches!(infer("TRUE"), Value::Bool(true)));
        assert!(matches!(infer(""), Value::Nil));
        assert!(matches!(infer("1e3"), Value::Float(_)));
        assert!(matches!(infer("abc"), Value::Str(_)));
        assert_eq!(excel_date(45352.0), "2024-03-01");
        assert_eq!(excel_date(45352.4375), "2024-03-01T10:30:00");
    }
}
