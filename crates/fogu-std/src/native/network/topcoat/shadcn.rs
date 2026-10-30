//! `tc.shadcn` — the components of shadcn-rs (https://github.com/hughdbrown/shadcn-rs)
//! rendered on the server: the same markup and class names as its Yew
//! components, styled by its stylesheet (`/_tc/shadcn.css`, loaded by
//! `tc.page({ ui: "shadcn" })`). The interactive ones (tabs, dialog, switch,
//! accordion, dropdown) are driven by signals or by small browser handlers.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::value::Value;

use super::{children, element, escape, html_value, opt_str, reactive, signal_name, split_opts};
use crate::native::util::{max_args, nfn_var};

type Attrs = Vec<(String, Value)>;

fn s(v: &str) -> Value {
    Value::Str(v.to_string().into())
}

fn cls(c: &str) -> (String, Value) {
    ("class".to_string(), s(c))
}

fn el(tag: &str, attrs: &[(String, Value)], kids: &str) -> Result<String, String> {
    element(tag, attrs, kids)
}

fn kids(v: Option<&Value>) -> String {
    v.map(|v| children(std::slice::from_ref(v))).unwrap_or_default()
}

/// `variant-x size-y` from the options, checked against the allowed values.
fn modifiers(o: &HashMap<String, Value>, variants: &[&str], sizes: &[&str], default_size: Option<&str>) -> Result<String, String> {
    let mut out = String::new();
    let variant = opt_str(o, "variant").unwrap_or_else(|| "default".into());
    if !variants.is_empty() {
        if !variants.contains(&variant.as_str()) {
            return Err(format!("shadcn: unknown variant '{}' ({})", variant, variants.join(" ")));
        }
        out.push_str(&format!(" variant-{}", variant));
    }
    if let Some(size) = opt_str(o, "size").or(default_size.map(String::from)) {
        if !sizes.contains(&size.as_str()) {
            return Err(format!("shadcn: unknown size '{}' ({})", size, sizes.join(" ")));
        }
        out.push_str(&format!(" size-{}", size));
    }
    Ok(out)
}

/// The caller's attributes with the component's class in front.
fn with_class(base: String, attrs: Attrs) -> Attrs {
    let mut out = vec![];
    let mut class = base;
    for (k, v) in attrs {
        if k == "class" {
            let extra = v.to_display_string();
            if !extra.is_empty() {
                class = format!("{} {}", class, extra);
            }
        } else {
            out.push((k, v));
        }
    }
    out.insert(0, ("class".into(), Value::Str(class.into())));
    out
}

const SIZES: [&str; 5] = ["xs", "sm", "md", "lg", "xl"];

pub fn module() -> Value {
    let mut m = HashMap::new();
    let fns: [(&str, usize, fn(&[Value]) -> Result<Value, String>); 27] = [
        ("button", 1, button),
        ("badge", 1, badge),
        ("card", 0, card),
        ("stat", 2, stat),
        ("alert", 1, alert),
        ("input", 0, input),
        ("textarea", 0, textarea),
        ("select", 1, select),
        ("checkbox", 0, checkbox),
        ("switch", 1, switch),
        ("label", 1, label),
        ("field", 2, field),
        ("table", 2, table),
        ("tabs", 2, tabs),
        ("accordion", 1, accordion),
        ("dialog", 2, dialog),
        ("dropdown", 2, dropdown),
        ("tooltip", 2, tooltip),
        ("progress", 1, progress),
        ("skeleton", 0, skeleton),
        ("separator", 0, separator),
        ("avatar", 0, avatar),
        ("kbd", 1, kbd),
        ("breadcrumb", 1, breadcrumb),
        ("pagination", 2, pagination),
        ("shell", 1, shell),
        ("toaster", 0, |_| Ok(html_value("<ol class=\"sonner sonner-bottom-right\" data-tc-toaster></ol>".into()))),
    ];
    for (n, min, f) in fns {
        m.insert(n.to_string(), nfn_var(n, min, f));
    }
    // layout helpers shared with tc.ui
    let layout: [(&str, fn(&[Value]) -> Result<Value, String>); 4] = [
        ("row", |a| super::simple("div", "tc-row", a, false)),
        ("stack", |a| super::simple("div", "tc-stack", a, false)),
        ("grid", |a| super::simple("div", "tc-grid", a, false)),
        ("container", |a| super::simple("div", "tc-container", a, false)),
    ];
    for (n, f) in layout {
        m.insert(n.to_string(), nfn_var(n, 0, f));
    }
    Value::Map(Rc::new(RefCell::new(m)))
}

/// `button(children, { variant, size, href, loading, fullWidth, ...attrs })`
fn button(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.button")?;
    let (o, attrs) = split_opts(a.get(1), &["variant", "size", "href", "loading", "fullWidth"], "")?;
    let mut class = format!("btn{}", modifiers(&o, &["default", "primary", "secondary", "destructive", "outline", "ghost", "link"], &SIZES, Some("md"))?);
    let loading = o.get("loading").is_some_and(Value::is_truthy);
    if loading {
        class.push_str(" btn-loading");
    }
    if o.get("fullWidth").is_some_and(Value::is_truthy) {
        class.push_str(" btn-full-width");
    }
    let mut attrs = with_class(class, attrs);
    let mut inner = String::new();
    if loading {
        inner.push_str("<span class=\"btn-spinner\" aria-hidden=\"true\"></span>");
    }
    inner.push_str(&el("span", &[cls(if loading { "btn-content btn-content-loading" } else { "btn-content" })], &kids(a.first()))?);
    Ok(html_value(match opt_str(&o, "href") {
        Some(href) => {
            attrs.push(("href".into(), Value::Str(href.into())));
            el("a", &attrs, &inner)?
        }
        None => {
            if !attrs.iter().any(|(k, _)| k == "type") {
                attrs.push(("type".into(), s("button")));
            }
            if loading {
                attrs.push(("disabled".into(), Value::Bool(true)));
                attrs.push(("aria-busy".into(), s("true")));
            }
            el("button", &attrs, &inner)?
        }
    }))
}

fn badge(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.badge")?;
    let (o, attrs) = split_opts(a.get(1), &["variant"], "")?;
    let class = format!("badge{}", modifiers(&o, &["default", "secondary", "destructive", "outline"], &[], None)?);
    Ok(html_value(el("span", &with_class(class, attrs), &kids(a.first()))?))
}

/// `card(content, { title, description, footer, ...attrs })`
fn card(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.card")?;
    let (o, attrs) = split_opts(a.get(1), &["title", "description", "footer"], "")?;
    let mut inner = String::new();
    if o.contains_key("title") || o.contains_key("description") {
        let mut head = String::new();
        if let Some(t) = o.get("title") {
            head.push_str(&el("h3", &[cls("card-title")], &kids(Some(t)))?);
        }
        if let Some(d) = o.get("description") {
            head.push_str(&el("p", &[cls("card-description")], &kids(Some(d)))?);
        }
        inner.push_str(&el("div", &[cls("card-header")], &head)?);
    }
    if let Some(c) = a.first().filter(|v| !matches!(v, Value::Nil)) {
        inner.push_str(&el("div", &[cls("card-content")], &kids(Some(c)))?);
    }
    if let Some(f) = o.get("footer") {
        inner.push_str(&el("div", &[cls("card-footer")], &kids(Some(f)))?);
    }
    Ok(html_value(el("div", &with_class("card".into(), attrs), &inner)?))
}

/// `stat(label, value, { hint })` — a metric card.
fn stat(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.stat")?;
    let (o, attrs) = split_opts(a.get(2), &["hint"], "")?;
    let mut inner = el("p", &[cls("card-description")], &kids(a.first()))?;
    inner.push_str(&el("div", &[cls("tc-stat-value")], &kids(a.get(1)))?);
    if let Some(h) = o.get("hint") {
        inner.push_str(&el("p", &[cls("card-description")], &kids(Some(h)))?);
    }
    let content = el("div", &[cls("card-content"), ("style".into(), s("padding-top:1.25rem;display:flex;flex-direction:column;gap:.25rem"))], &inner)?;
    Ok(html_value(el("div", &with_class("card".into(), attrs), &content)?))
}

/// `alert(title, description?, { variant })`
fn alert(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.alert")?;
    let (o, attrs) = split_opts(a.get(2), &["variant"], "")?;
    let class = format!("alert{}", modifiers(&o, &["default", "destructive"], &[], None)?);
    let mut attrs = with_class(class, attrs);
    attrs.push(("role".into(), s("alert")));
    let mut inner = el("h5", &[cls("alert-title")], &kids(a.first()))?;
    if let Some(d) = a.get(1).filter(|v| !matches!(v, Value::Nil)) {
        inner.push_str(&el("div", &[cls("alert-description")], &kids(Some(d)))?);
    }
    Ok(html_value(el("div", &attrs, &inner)?))
}

fn state_classes(base: &str, o: &HashMap<String, Value>, attrs: &mut Attrs) -> String {
    let mut class = base.to_string();
    if o.get("error").is_some_and(Value::is_truthy) {
        class.push_str(&format!(" {}-error", base));
        attrs.push(("aria-invalid".into(), s("true")));
    }
    for (flag, suffix) in [("disabled", "disabled"), ("readonly", "readonly")] {
        if o.get(flag).is_some_and(Value::is_truthy) {
            class.push_str(&format!(" {}-{}", base, suffix));
            attrs.push((flag.into(), Value::Bool(true)));
        }
    }
    class
}

/// `input({ error, disabled, readonly, ...attrs })`
fn input(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "shadcn.input")?;
    let (o, mut attrs) = split_opts(a.first(), &["error", "disabled", "readonly"], "")?;
    let class = state_classes("input", &o, &mut attrs);
    Ok(html_value(el("input", &with_class(class, attrs), "")?))
}

/// `textarea({ value, resize, error, disabled, readonly, ...attrs })`
fn textarea(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "shadcn.textarea")?;
    let (o, mut attrs) = split_opts(a.first(), &["value", "resize", "error", "disabled", "readonly"], "")?;
    let mut class = state_classes("textarea", &o, &mut attrs);
    if let Some(r) = opt_str(&o, "resize") {
        class.push_str(&format!(" textarea-resize-{}", r));
    }
    Ok(html_value(el("textarea", &with_class(class, attrs), &kids(o.get("value")))?))
}

/// `select(options, { value, size, invalid, ...attrs })` — a native <select>.
fn select(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.select")?;
    let (o, attrs) = split_opts(a.get(1), &["value", "size", "invalid"], "")?;
    let mut class = format!("native-select{}", modifiers(&o, &[], &SIZES, Some("md"))?);
    if o.get("invalid").is_some_and(Value::is_truthy) {
        class.push_str(" native-select-invalid");
    }
    let selected = opt_str(&o, "value");
    let Some(Value::List(items)) = a.first() else {
        return Err("shadcn.select: options must be a list".into());
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
            other => (other.to_display_string(), Value::Nil),
        };
        let label = if matches!(l, Value::Nil) { Value::Str(v.clone().into()) } else { l };
        let mut oa = vec![("value".to_string(), Value::Str(v.clone().into()))];
        if selected.as_deref() == Some(v.as_str()) {
            oa.push(("selected".into(), Value::Bool(true)));
        }
        inner.push_str(&el("option", &oa, &kids(Some(&label)))?);
    }
    let select = el("select", &with_class(class, attrs), &inner)?;
    Ok(html_value(el("div", &[cls("native-select-wrapper")], &select)?))
}

fn checkbox(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "shadcn.checkbox")?;
    let (o, mut attrs) = split_opts(a.first(), &["error", "disabled"], "")?;
    let class = state_classes("checkbox", &o, &mut attrs);
    let mut attrs = with_class(class, attrs);
    attrs.push(("type".into(), s("checkbox")));
    Ok(html_value(el("input", &attrs, "")?))
}

/// `switch(signal, { size, disabled, name })` — a toggle bound to a bool signal.
fn switch(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.switch")?;
    let name = signal_name(&a[0]).ok_or("shadcn.switch: the first argument must be a signal (tc.signal)")?;
    let (o, attrs) = split_opts(a.get(1), &["size", "disabled"], "")?;
    let on = reactive(&a[0]).map(|(_, v)| v.is_truthy()).unwrap_or(false);
    let mut class = format!("switch{}", modifiers(&o, &[], &["sm", "md", "lg"], Some("md"))?);
    if on {
        class.push_str(" switch-checked");
    }
    let mut attrs = with_class(class, attrs);
    attrs.extend([
        ("type".to_string(), s("button")),
        ("role".into(), s("switch")),
        ("aria-checked".into(), s(if on { "true" } else { "false" })),
        (":aria-checked".into(), Value::Str(format!("String({}.get())", name).into())),
        (":class".into(), Value::Str(format!("({{ 'switch-checked': !!{}.get() }})", name).into())),
        ("@click".into(), Value::Str(format!("{}.toggle()", name).into())),
    ]);
    if o.get("disabled").is_some_and(Value::is_truthy) {
        attrs.push(("disabled".into(), Value::Bool(true)));
    }
    Ok(html_value(el("button", &attrs, "<span class=\"switch-thumb\"></span>")?))
}

/// `label(children, { required, for })`
fn label(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.label")?;
    let (o, attrs) = split_opts(a.get(1), &["required"], "")?;
    let mut inner = kids(a.first());
    if o.get("required").is_some_and(Value::is_truthy) {
        inner.push_str("<span class=\"label-required-indicator\" aria-hidden=\"true\">*</span>");
    }
    Ok(html_value(el("label", &with_class("label".into(), attrs), &inner)?))
}

/// `field(label, control, { description, error, required, for })`
fn field(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.field")?;
    let (o, attrs) = split_opts(a.get(2), &["description", "error", "required", "for"], "")?;
    let mut la = vec![cls("field-label")];
    if let Some(f) = opt_str(&o, "for") {
        la.push(("for".into(), Value::Str(f.into())));
    }
    let mut label = kids(a.first());
    if o.get("required").is_some_and(Value::is_truthy) {
        label.push_str("<span class=\"field-required\" aria-hidden=\"true\"> *</span>");
    }
    let mut inner = el("label", &la, &label)?;
    inner.push_str(&el("div", &[cls("field-input")], &kids(a.get(1)))?);
    if let Some(d) = o.get("description") {
        inner.push_str(&el("p", &[cls("field-help")], &kids(Some(d)))?);
    }
    let error = o.get("error").filter(|v| v.is_truthy());
    if let Some(e) = error {
        inner.push_str(&el("p", &[cls("field-error-message"), ("role".into(), s("alert"))], &kids(Some(e)))?);
    }
    let class = if error.is_some() { "field field-error" } else { "field" };
    Ok(html_value(el("div", &with_class(class.into(), attrs), &inner)?))
}

/// `table(columns, rows, { empty, caption })` — like `ui.table`.
fn table(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.table")?;
    let (o, attrs) = split_opts(a.get(2), &["empty", "caption"], "")?;
    let Some(Value::List(cols)) = a.first() else {
        return Err("shadcn.table: columns must be a list".into());
    };
    let cols: Vec<(String, Value)> = cols
        .borrow()
        .iter()
        .map(|c| match c {
            Value::Map(m) => {
                let m = m.borrow();
                let key = m.get("key").map(|v| v.to_display_string()).unwrap_or_default();
                (key.clone(), m.get("label").cloned().unwrap_or(Value::Str(key.into())))
            }
            other => (other.to_display_string(), other.clone()),
        })
        .collect();
    let rows = match a.get(1) {
        Some(Value::List(r)) => r.borrow().clone(),
        None | Some(Value::Nil) => Vec::new(),
        Some(other) => return Err(format!("shadcn.table: rows must be a list, got '{}'", other.type_name())),
    };
    let mut head = String::new();
    for (_, l) in &cols {
        head.push_str(&el("th", &[cls("table-head")], &kids(Some(l)))?);
    }
    let mut body = String::new();
    if rows.is_empty() {
        let empty = o.get("empty").cloned().unwrap_or(s("No rows."));
        let td = el("td", &[cls("table-cell"), ("colspan".into(), Value::Int(cols.len().max(1) as i64)), ("style".into(), s("text-align:center;opacity:.7"))], &kids(Some(&empty)))?;
        body.push_str(&el("tr", &[cls("table-row")], &td)?);
    }
    for row in &rows {
        let mut tr = String::new();
        for (i, (key, _)) in cols.iter().enumerate() {
            let cell = match row {
                Value::Map(m) => m.borrow().get(key).cloned().unwrap_or(Value::Nil),
                Value::List(l) => l.borrow().get(i).cloned().unwrap_or(Value::Nil),
                other if i == 0 => other.clone(),
                _ => Value::Nil,
            };
            tr.push_str(&el("td", &[cls("table-cell")], &kids(Some(&cell)))?);
        }
        body.push_str(&el("tr", &[cls("table-row")], &tr)?);
    }
    let mut inner = el("thead", &[cls("table-header")], &el("tr", &[cls("table-row")], &head)?)?;
    inner.push_str(&el("tbody", &[cls("table-body")], &body)?);
    if let Some(c) = o.get("caption") {
        inner.push_str(&el("caption", &[cls("table-caption")], &kids(Some(c)))?);
    }
    let t = el("table", &with_class("table".into(), attrs), &inner)?;
    Ok(html_value(el("div", &[cls("table-wrapper")], &t)?))
}

/// `tabs(signal, [{ value, label, content, disabled }], { vertical })` — the
/// signal holds the selected value.
fn tabs(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.tabs")?;
    let name = signal_name(&a[0]).ok_or("shadcn.tabs: the first argument must be a signal (tc.signal)")?;
    let current = reactive(&a[0]).map(|(_, v)| v.to_display_string()).unwrap_or_default();
    let (o, attrs) = split_opts(a.get(2), &["vertical"], "")?;
    let Some(Value::List(items)) = a.get(1) else {
        return Err("shadcn.tabs: expected a list of { value, label, content }".into());
    };
    let mut list = String::new();
    let mut panels = String::new();
    for it in items.borrow().iter() {
        let Value::Map(m) = it else { return Err("shadcn.tabs: items must be maps".into()) };
        let m = m.borrow();
        let v = m.get("value").map(|v| v.to_display_string()).ok_or("shadcn.tabs: an item has no value")?;
        let js = serde_json::to_string(&v).unwrap_or_default();
        let active = v == current;
        let state = if active { "active" } else { "inactive" };
        let cond = format!("{}.get() === {} ? 'active' : 'inactive'", name, js);
        let disabled = m.get("disabled").is_some_and(Value::is_truthy);
        let mut ta = vec![
            cls(if disabled { "tabs-trigger tabs-trigger-disabled" } else { "tabs-trigger" }),
            ("type".into(), s("button")),
            ("role".into(), s("tab")),
            ("id".into(), Value::Str(format!("tab-{}", v).into())),
            ("aria-controls".into(), Value::Str(format!("tabpanel-{}", v).into())),
            ("aria-selected".into(), s(if active { "true" } else { "false" })),
            (":aria-selected".into(), Value::Str(format!("String({}.get() === {})", name, js).into())),
            ("data-state".into(), s(state)),
            (":data-state".into(), Value::Str(cond.clone().into())),
        ];
        if disabled {
            ta.push(("disabled".into(), Value::Bool(true)));
        } else {
            ta.push(("@click".into(), Value::Str(format!("{}.set({})", name, js).into())));
        }
        list.push_str(&el("button", &ta, &kids(m.get("label").or(m.get("value"))))?);
        panels.push_str(&el(
            "div",
            &[
                cls("tabs-content"),
                ("role".into(), s("tabpanel")),
                ("id".into(), Value::Str(format!("tabpanel-{}", v).into())),
                ("aria-labelledby".into(), Value::Str(format!("tab-{}", v).into())),
                ("data-state".into(), s(state)),
                (":data-state".into(), Value::Str(cond.into())),
            ],
            &kids(m.get("content")),
        )?);
    }
    let orientation = if o.get("vertical").is_some_and(Value::is_truthy) { "tabs tabs-vertical" } else { "tabs tabs-horizontal" };
    let inner = el("div", &[cls("tabs-list"), ("role".into(), s("tablist"))], &list)? + &panels;
    Ok(html_value(el("div", &with_class(orientation.into(), attrs), &inner)?))
}

/// `accordion([{ title, content, open, disabled }], { multiple })` — opens and
/// closes in the browser.
fn accordion(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.accordion")?;
    let (o, attrs) = split_opts(a.get(1), &["multiple"], "")?;
    let multiple = o.get("multiple").is_some_and(Value::is_truthy);
    let Some(Value::List(items)) = a.first() else {
        return Err("shadcn.accordion: expected a list of { title, content }".into());
    };
    // toggles this item; without `multiple`, closes the others
    let toggle = format!(
        "const t = e.currentTarget, item = t.closest('.accordion-item'), open = t.dataset.state !== 'open';{}\
         t.dataset.state = open ? 'open' : 'closed'; t.setAttribute('aria-expanded', String(open));\
         item.classList.toggle('accordion-item-open', open); item.querySelector('.accordion-content').dataset.state = open ? 'open' : 'closed';",
        if multiple {
            String::new()
        } else {
            " for (const o of item.parentElement.querySelectorAll(':scope > .accordion-item-open')) if (o !== item) { o.classList.remove('accordion-item-open'); o.querySelector('.accordion-trigger').dataset.state = 'closed'; o.querySelector('.accordion-content').dataset.state = 'closed'; }".into()
        }
    );
    let mut inner = String::new();
    for it in items.borrow().iter() {
        let Value::Map(m) = it else { return Err("shadcn.accordion: items must be maps".into()) };
        let m = m.borrow();
        let open = m.get("open").is_some_and(Value::is_truthy);
        let disabled = m.get("disabled").is_some_and(Value::is_truthy);
        let state = if open { "open" } else { "closed" };
        let mut ta = vec![
            cls(if open { "accordion-trigger accordion-trigger-open" } else { "accordion-trigger" }),
            ("type".into(), s("button")),
            ("data-state".into(), s(state)),
            ("aria-expanded".into(), s(if open { "true" } else { "false" })),
        ];
        if disabled {
            ta.push(("disabled".into(), Value::Bool(true)));
        } else {
            ta.push(("@click".into(), Value::Str(toggle.clone().into())));
        }
        let trigger = el("button", &ta, &(kids(m.get("title")) + "<span class=\"accordion-trigger-icon accordion-chevron\" aria-hidden=\"true\">&#9662;</span>"))?;
        let content = el(
            "div",
            &[cls("accordion-content"), ("data-state".into(), s(state)), ("role".into(), s("region"))],
            &el("div", &[cls("accordion-content-text")], &kids(m.get("content")))?,
        )?;
        let mut ic = String::from("accordion-item");
        if open {
            ic.push_str(" accordion-item-open");
        }
        if disabled {
            ic.push_str(" accordion-item-disabled");
        }
        inner.push_str(&el("div", &[cls(&ic)], &(trigger + &content))?);
    }
    Ok(html_value(el("div", &with_class("accordion".into(), attrs), &inner)?))
}

/// `dialog(signal, { title, description, footer, trigger }, content)` — open
/// while the bool signal is true.
fn dialog(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.dialog")?;
    let name = signal_name(&a[0]).ok_or("shadcn.dialog: the first argument must be a signal (tc.signal)")?;
    let open = reactive(&a[0]).map(|(_, v)| v.is_truthy()).unwrap_or(false);
    let (o, attrs) = split_opts(a.get(1), &["title", "description", "footer", "trigger"], "")?;
    let close = format!("{}.set(false)", name);
    let mut out = String::new();
    if let Some(t) = o.get("trigger") {
        out.push_str(&el("span", &[cls("dialog-trigger"), ("@click".into(), Value::Str(format!("{}.set(true)", name).into()))], &kids(Some(t)))?);
    }
    let mut head = String::new();
    if let Some(t) = o.get("title") {
        head.push_str(&el("h2", &[cls("dialog-title")], &kids(Some(t)))?);
    }
    if let Some(d) = o.get("description") {
        head.push_str(&el("p", &[cls("dialog-description")], &kids(Some(d)))?);
    }
    let mut content = String::new();
    if !head.is_empty() {
        content.push_str(&el("div", &[cls("dialog-header")], &head)?);
    }
    content.push_str(&kids(a.get(2)));
    if let Some(f) = o.get("footer") {
        content.push_str(&el("div", &[cls("dialog-footer")], &kids(Some(f)))?);
    }
    content.push_str(&el("button", &[cls("dialog-close"), ("type".into(), s("button")), ("aria-label".into(), s("Close")), ("@click".into(), Value::Str(close.clone().into()))], "&times;")?);
    let mut ca = with_class("dialog-content".into(), attrs);
    ca.extend([("role".to_string(), s("dialog")), ("aria-modal".into(), s("true")), ("@keydown.escape".into(), Value::Str(close.clone().into()))]);
    let overlay = el("div", &[cls("dialog-overlay"), ("@click".into(), Value::Str(close.into()))], "")?;
    out.push_str(&el(
        "div",
        &[cls("dialog-root"), ("hidden".into(), Value::Bool(!open)), (":hidden".into(), Value::Str(format!("!{}.get()", name).into()))],
        &(overlay + &el("div", &ca, &content)?),
    )?);
    Ok(html_value(out))
}

/// `dropdown(trigger, [{ label, href, "@click" } | { separator: true } | { group }], { align })`
fn dropdown(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.dropdown")?;
    let (o, attrs) = split_opts(a.get(2), &["align"], "")?;
    let Some(Value::List(items)) = a.get(1) else {
        return Err("shadcn.dropdown: expected a list of items".into());
    };
    let mut menu = String::new();
    for it in items.borrow().iter() {
        let Value::Map(m) = it else { return Err("shadcn.dropdown: items must be maps".into()) };
        let m = m.borrow();
        if m.get("separator").is_some_and(Value::is_truthy) {
            menu.push_str("<div class=\"dropdown-menu-separator\" role=\"separator\"></div>");
            continue;
        }
        if let Some(g) = m.get("group") {
            menu.push_str(&el("div", &[cls("dropdown-menu-label")], &kids(Some(g)))?);
            continue;
        }
        let mut ia = vec![cls("dropdown-menu-item"), ("role".into(), s("menuitem"))];
        for (k, v) in super::sorted(&m).iter().filter(|(k, _)| k.starts_with('@') || k.starts_with(':')) {
            ia.push((k.clone(), v.clone()));
        }
        let label = kids(m.get("label"));
        menu.push_str(&match m.get("href") {
            Some(h) => {
                ia.push(("href".into(), h.clone()));
                ia.push(("tc-link".into(), Value::Bool(true)));
                el("a", &ia, &label)?
            }
            None => {
                ia.push(("type".into(), s("button")));
                el("button", &ia, &label)?
            }
        });
    }
    let toggle = "const m = e.currentTarget.nextElementSibling; m.hidden = !m.hidden; e.currentTarget.setAttribute('aria-expanded', String(!m.hidden)); \
                  if (!m.hidden) setTimeout(() => document.addEventListener('click', function off(ev) { if (!m.contains(ev.target) || ev.target.closest('.dropdown-menu-item')) { m.hidden = true; document.removeEventListener('click', off) } }), 0)";
    let trigger = el(
        "button",
        &[cls("btn variant-outline size-md dropdown-menu-trigger"), ("type".into(), s("button")), ("aria-haspopup".into(), s("menu")), ("aria-expanded".into(), s("false")), ("@click".into(), s(toggle))],
        &kids(a.first()),
    )?;
    let content = el("div", &[cls("dropdown-menu-content"), ("role".into(), s("menu")), ("hidden".into(), Value::Bool(true))], &menu)?;
    let root = if opt_str(&o, "align").as_deref() == Some("end") { "dropdown-menu-root align-end" } else { "dropdown-menu-root" };
    Ok(html_value(el("div", &with_class(root.into(), attrs), &(trigger + &content))?))
}

/// `tooltip(trigger, text, { position = "top" })`
fn tooltip(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.tooltip")?;
    let (o, attrs) = split_opts(a.get(2), &["position"], "")?;
    let pos = opt_str(&o, "position").unwrap_or_else(|| "top".into());
    if !["top", "bottom", "left", "right"].contains(&pos.as_str()) {
        return Err(format!("shadcn.tooltip: unknown position '{}' (top bottom left right)", pos));
    }
    let tip = el("span", &[cls(&format!("tooltip-content position-{}", pos)), ("role".into(), s("tooltip"))], &kids(a.get(1)))?;
    Ok(html_value(el("span", &with_class("tooltip tooltip-root".into(), attrs), &(el("span", &[cls("tooltip-trigger"), ("tabindex".into(), s("0"))], &kids(a.first()))? + &tip))?))
}

/// `progress(value 0..100 | signal | expr, { variant, size, label })`
fn progress(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.progress")?;
    let (o, attrs) = split_opts(a.get(1), &["variant", "size", "label"], "")?;
    let class = format!("progress{}", modifiers(&o, &["default", "primary", "secondary", "destructive"], &["sm", "md", "lg"], Some("md"))?);
    let mut attrs = with_class(class, attrs);
    attrs.push(("role".into(), s("progressbar")));
    let bar = match reactive(&a[0]) {
        Some((code, init)) => el(
            "div",
            &[
                cls("progress-bar"),
                ("style".into(), Value::Str(format!("width:{}%", init.to_display_string()).into())),
                (":style".into(), Value::Str(format!("({{ width: Math.max(0, Math.min(100, {})) + '%' }})", code).into())),
            ],
            "",
        )?,
        None => {
            let v = match &a[0] {
                Value::Int(n) => *n as f64,
                Value::Float(f) => *f,
                other => return Err(format!("shadcn.progress: expected a number, got '{}'", other.type_name())),
            };
            attrs.push(("aria-valuenow".into(), Value::Float(v)));
            el("div", &[cls("progress-bar"), ("style".into(), Value::Str(format!("width:{}%", v.clamp(0.0, 100.0)).into()))], "")?
        }
    };
    let mut out = el("div", &attrs, &bar)?;
    if let Some(l) = o.get("label") {
        out.push_str(&el("div", &[cls("progress-label")], &kids(Some(l)))?);
    }
    Ok(html_value(out))
}

/// `skeleton({ shape = "rectangle" | "circle" | "text", width, height })`
fn skeleton(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "shadcn.skeleton")?;
    let (o, attrs) = split_opts(a.first(), &["shape", "width", "height"], "")?;
    let shape = opt_str(&o, "shape").unwrap_or_else(|| "rectangle".into());
    let mut attrs = with_class(format!("skeleton skeleton-animate skeleton-{}", shape), attrs);
    let w = opt_str(&o, "width").unwrap_or_else(|| "100%".into());
    let h = opt_str(&o, "height").unwrap_or_else(|| if shape == "text" { "1em".into() } else { "1rem".into() });
    attrs.push(("style".into(), Value::Str(format!("width:{};height:{}", w, h).into())));
    Ok(html_value(el("div", &attrs, "")?))
}

fn separator(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "shadcn.separator")?;
    let (o, attrs) = split_opts(a.first(), &["vertical"], "")?;
    let dir = if o.get("vertical").is_some_and(Value::is_truthy) { "vertical" } else { "horizontal" };
    let mut attrs = with_class(format!("separator separator-{}", dir), attrs);
    attrs.extend([("role".to_string(), s("separator")), ("aria-orientation".into(), s(dir))]);
    Ok(html_value(el("div", &attrs, "")?))
}

/// `avatar({ src, alt, fallback, size, square })`
fn avatar(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "shadcn.avatar")?;
    let (o, attrs) = split_opts(a.first(), &["src", "alt", "fallback", "size", "square"], "")?;
    let shape = if o.get("square").is_some_and(Value::is_truthy) { "avatar-square" } else { "avatar-circle" };
    let class = format!("avatar {}{}", shape, modifiers(&o, &[], &SIZES, Some("md"))?);
    let alt = opt_str(&o, "alt").unwrap_or_default();
    let inner = match opt_str(&o, "src") {
        Some(src) => el("img", &[cls("avatar-image"), ("src".into(), Value::Str(src.into())), ("alt".into(), Value::Str(alt.into()))], "")?,
        None => {
            let fb = opt_str(&o, "fallback").unwrap_or_else(|| alt.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase());
            el("span", &[cls("avatar-fallback avatar-initials")], &escape(&fb))?
        }
    };
    Ok(html_value(el("span", &with_class(class, attrs), &inner)?))
}

fn kbd(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.kbd")?;
    let (o, attrs) = split_opts(a.get(1), &["size"], "")?;
    let class = format!("kbd{}", modifiers(&o, &[], &["xs", "sm", "md", "lg", "xl", "2xl"], Some("sm"))?);
    Ok(html_value(el("kbd", &with_class(class, attrs), &kids(a.first()))?))
}

/// `breadcrumb([{ label, href }])` — the last item is the current page.
fn breadcrumb(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.breadcrumb")?;
    let (_, attrs) = split_opts(a.get(1), &[], "")?;
    let Some(Value::List(items)) = a.first() else {
        return Err("shadcn.breadcrumb: expected a list of { label, href }".into());
    };
    let items = items.borrow();
    let mut list = String::new();
    for (i, it) in items.iter().enumerate() {
        let Value::Map(m) = it else { return Err("shadcn.breadcrumb: items must be maps".into()) };
        let m = m.borrow();
        let last = i + 1 == items.len();
        let label = kids(m.get("label"));
        let item = match (m.get("href"), last) {
            (Some(h), false) => el("a", &[cls("breadcrumb-link"), ("href".into(), h.clone()), ("tc-link".into(), Value::Bool(true))], &label)?,
            _ => el("span", &[cls("breadcrumb-page"), ("aria-current".into(), s("page"))], &label)?,
        };
        list.push_str(&el("li", &[cls("breadcrumb-item")], &item)?);
        if !last {
            list.push_str("<li class=\"breadcrumb-separator\" role=\"presentation\" aria-hidden=\"true\">/</li>");
        }
    }
    let mut attrs = with_class("breadcrumb".into(), attrs);
    attrs.push(("aria-label".into(), s("breadcrumb")));
    Ok(html_value(el("nav", &attrs, &el("ol", &[cls("breadcrumb-list")], &list)?)?))
}

/// `pagination(page, pages, { href })` — `page` is an int (links built from
/// `href`, with `{page}` replaced) or a signal (buttons that set it).
fn pagination(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "shadcn.pagination")?;
    let (o, attrs) = split_opts(a.get(2), &["href"], "")?;
    let pages = match &a[1] {
        Value::Int(n) if *n >= 1 => *n,
        other => return Err(format!("shadcn.pagination: pages must be an int >= 1, got {}", other.to_display_string())),
    };
    let sig = signal_name(&a[0]);
    let page = match (&a[0], reactive(&a[0])) {
        (Value::Int(n), _) => *n,
        (_, Some((_, Value::Int(n)))) => n,
        _ => 1,
    }
    .clamp(1, pages);
    let href = opt_str(&o, "href");
    if sig.is_none() && href.is_none() {
        return Err("shadcn.pagination: give a signal, or { href: \"/list?page={page}\" }".into());
    }
    // with a signal: prev/next are relative and the active page follows it in the browser
    let link = |target: i64, label: &str, class: &str, disabled: bool| -> Result<String, String> {
        let mut la = vec![cls(class)];
        if let (Some(name), true) = (&sig, label.chars().all(|c| c.is_ascii_digit())) {
            la.push((":class".into(), Value::Str(format!("({{ 'pagination-link-active': {n}.get() === {t}, active: {n}.get() === {t} }})", n = name, t = target).into())));
        }
        if disabled {
            la.push(("aria-disabled".into(), s("true")));
        }
        let inner = escape(label);
        match &sig {
            Some(name) => {
                la.push(("type".into(), s("button")));
                let step = if class.contains("pagination-previous") { Some(-1) } else if class.contains("pagination-next") { Some(1) } else { None };
                match step {
                    Some(d) => {
                        let bound = if d < 0 { "1".to_string() } else { pages.to_string() };
                        la.push(("@click".into(), Value::Str(format!("{n}.set(Math.{f}({b}, {n}.get() + ({d})))", n = name, f = if d < 0 { "max" } else { "min" }, b = bound, d = d).into())));
                        la.push((":disabled".into(), Value::Str(format!("{}.get() === {}", name, bound).into())));
                        if disabled {
                            la.push(("disabled".into(), Value::Bool(true)));
                        }
                    }
                    None => la.push(("@click".into(), Value::Str(format!("{}.set({})", name, target).into()))),
                }
                el("button", &la, &inner)
            }
            None => {
                if !disabled {
                    la.push(("href".into(), Value::Str(href.clone().unwrap_or_default().replace("{page}", &target.to_string()).into())));
                    la.push(("tc-link".into(), Value::Bool(true)));
                }
                el("a", &la, &inner)
            }
        }
    };
    let mut items = String::new();
    let first = page <= 1;
    items.push_str(&el("li", &[cls("pagination-item")], &link(page - 1, "‹ Previous", if first { "pagination-link pagination-previous pagination-previous-disabled" } else { "pagination-link pagination-previous" }, first)?)?);
    // 1 … p-1 p p+1 … n
    let mut last_shown = 0;
    for n in 1..=pages {
        if n == 1 || n == pages || (n - page).abs() <= 1 {
            if n - last_shown > 1 {
                items.push_str("<li class=\"pagination-item\"><span class=\"pagination-ellipsis\" aria-hidden=\"true\">…</span></li>");
            }
            let active = n == page;
            let mut l = link(n, &n.to_string(), if active { "pagination-link pagination-link-active active" } else { "pagination-link" }, false)?;
            if active {
                l = l.replacen('>', " aria-current=\"page\">", 1);
            }
            items.push_str(&el("li", &[cls("pagination-item")], &l)?);
            last_shown = n;
        }
    }
    let last = page >= pages;
    items.push_str(&el("li", &[cls("pagination-item")], &link(page + 1, "Next ›", if last { "pagination-link pagination-next pagination-next-disabled" } else { "pagination-link pagination-next" }, last)?)?);
    let mut attrs = with_class("pagination".into(), attrs);
    attrs.extend([("role".to_string(), s("navigation")), ("aria-label".into(), s("pagination"))]);
    Ok(html_value(el("nav", &attrs, &el("ul", &[cls("pagination-content")], &items)?)?))
}

/// `shell({ brand, nav: [{ label, href, current } | { group }], title, actions }, content)`
fn shell(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "shadcn.shell")?;
    let (o, attrs) = split_opts(a.first(), &["brand", "nav", "title", "actions"], "")?;
    let mut side = String::new();
    if let Some(b) = o.get("brand") {
        side.push_str(&el("div", &[cls("sidebar-header")], &kids(Some(b)))?);
    }
    let mut groups: Vec<(Option<Value>, String)> = vec![(None, String::new())];
    if let Some(Value::List(nav)) = o.get("nav") {
        for it in nav.borrow().iter() {
            let Value::Map(m) = it else { continue };
            let m = m.borrow();
            if let Some(g) = m.get("group") {
                groups.push((Some(g.clone()), String::new()));
                continue;
            }
            let current = m.get("current").is_some_and(Value::is_truthy);
            let mut la = vec![
                cls(if current { "sidebar-menu-button sidebar-menu-button-active" } else { "sidebar-menu-button" }),
                ("href".into(), m.get("href").cloned().unwrap_or(s("#"))),
                ("tc-link".into(), Value::Bool(true)),
            ];
            if current {
                la.push(("aria-current".into(), s("page")));
            }
            let item = el("li", &[cls("sidebar-menu-item")], &el("a", &la, &kids(m.get("label")))?)?;
            groups.last_mut().expect("one group").1.push_str(&item);
        }
    }
    let mut content = String::new();
    for (label, items) in groups {
        if items.is_empty() && label.is_none() {
            continue;
        }
        let mut g = String::new();
        if let Some(l) = label {
            g.push_str(&el("div", &[cls("sidebar-group-label")], &kids(Some(&l)))?);
        }
        g.push_str(&el("div", &[cls("sidebar-group-content")], &el("ul", &[cls("sidebar-menu")], &items)?)?);
        content.push_str(&el("div", &[cls("sidebar-group")], &g)?);
    }
    side.push_str(&el("div", &[cls("sidebar-content")], &content)?);
    let sidebar = el("aside", &[cls("sidebar")], &side)?;
    let mut top = String::new();
    if let Some(t) = o.get("title") {
        top.push_str(&el("h2", &[("style".into(), s("font-size:1.125rem;font-weight:600"))], &kids(Some(t)))?);
    }
    top.push_str("<div class=\"tc-spacer\"></div>");
    top.push_str(&kids(o.get("actions")));
    let main = format!("<main class=\"tc-main\"><header class=\"tc-topbar\">{}</header><div class=\"tc-container tc-stack\">{}</div></main>", top, kids(a.get(1)));
    Ok(html_value(el("div", &with_class("tc-shell".into(), attrs), &(sidebar + &main))?))
}
