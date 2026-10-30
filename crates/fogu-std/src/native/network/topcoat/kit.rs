//! More of `tc.ui`: icons, the Fogu logo, sparklines, stat cards with an icon
//! and a trend, empty states, a live indicator and a light/dark switch.

use ahash::AHashMap as HashMap;

use crate::runtime::value::Value;

use super::{children, element, escape, html_value, opt_str, split_opts};
use crate::native::util::max_args;

/// 24×24 stroke icons (drawn for Fogu in the style of Lucide).
const ICONS: &[(&str, &str)] = &[
    ("activity", "M3 12h4l3-8 4 16 3-8h4"),
    ("alert", "M12 3 2 20h20zM12 10v4M12 17h.01"),
    ("arrow-right", "M5 12h14M13 6l6 6-6 6"),
    ("check", "M5 12.5l4.5 4.5L19 7"),
    ("chevron-right", "M9 6l6 6-6 6"),
    ("clock", "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 7v5l3 2"),
    ("copy", "M9 9h11v11H9zM5 15H4V4h11v1"),
    ("database", "M4 5c0 1.7 3.6 3 8 3s8-1.3 8-3-3.6-3-8-3-8 1.3-8 3zM4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"),
    ("edit", "M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4"),
    ("eye", "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12zM12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6"),
    ("flame", "M12 22c4 0 7-2.7 7-6.6 0-3.2-2-5.7-4-7.4.3 2-1 3.4-2 3.4 0-3.4-1.6-6.5-4.5-8.4.3 3-1.5 5.3-3 7A8.3 8.3 0 0 0 5 15.4C5 19.3 8 22 12 22z"),
    ("hash", "M5 9h14M5 15h14M10 3 8 21M16 3l-2 18"),
    ("home", "M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"),
    ("key", "M2.5 15a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0-9 0M10.2 11.8 20 2M17 5l3 3M14.5 7.5 17 10"),
    ("layers", "M12 3 2 8l10 5 10-5zM2 13l10 5 10-5M2 17.5l10 5 10-5"),
    ("lock", "M5 11h14v10H5zM8 11V7a4 4 0 0 1 8 0v4"),
    ("logout", "M15 4h4a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-4M10 16l4-4-4-4M14 12H4"),
    ("message", "M4 5h16v11H8l-4 4z"),
    ("moon", "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"),
    ("network", "M12 5a2 2 0 1 0 0-4a2 2 0 1 0 0 4M5 23a2 2 0 1 0 0-4a2 2 0 1 0 0 4M19 23a2 2 0 1 0 0-4a2 2 0 1 0 0 4M12 5v6M12 11l-6.3 8.2M12 11l6.3 8.2"),
    ("plus", "M12 5v14M5 12h14"),
    ("radio", "M4.9 19.1a10 10 0 0 1 0-14.2M7.8 16.2a6 6 0 0 1 0-8.4M16.2 7.8a6 6 0 0 1 0 8.4M19.1 4.9a10 10 0 0 1 0 14.2M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0"),
    ("refresh", "M20 11a8 8 0 0 0-14.6-4.5L4 8M4 4v4h4M4 13a8 8 0 0 0 14.6 4.5L20 16M20 20v-4h-4"),
    ("search", "M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14M20 20l-4.2-4.2"),
    ("send", "M22 2 11 13M22 2l-7 20-4-9-9-4z"),
    ("server", "M4 3h16v7H4zM4 14h16v7H4zM8 6.5h.01M8 17.5h.01"),
    ("settings", "M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1M15 4v4M9 10v4M17 16v4"),
    ("shield", "M12 3 4 6v6c0 4.5 3.4 8 8 9 4.6-1 8-4.5 8-9V6z"),
    ("sun", "M12 8a4 4 0 1 0 0 8a4 4 0 1 0 0-8M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"),
    ("terminal", "M4 17l6-5-6-5M12 19h8"),
    ("trash", "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6"),
    ("users", "M16 20v-1.5A3.5 3.5 0 0 0 12.5 15h-5A3.5 3.5 0 0 0 4 18.5V20M10 11a3.5 3.5 0 1 0 0-7a3.5 3.5 0 1 0 0 7M20 20v-1.5a3.5 3.5 0 0 0-2.5-3.3M15.5 4.2a3.5 3.5 0 0 1 0 6.6"),
    ("x", "M6 6l12 12M18 6 6 18"),
    ("zap", "M13 2 4 14h7l-1 8 9-12h-7z"),
];

pub fn icon_svg(name: &str, size: u32, class: &str) -> Option<String> {
    let path = ICONS.iter().find(|(n, _)| *n == name)?.1;
    Some(format!(
        "<svg class=\"tc-icon{}{}\" width=\"{s}\" height=\"{s}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"{}\"/></svg>",
        if class.is_empty() { "" } else { " " },
        escape(class),
        path,
        s = size
    ))
}

fn icon_names() -> String {
    ICONS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(" ")
}

/// An icon by name, or an error listing the names.
pub fn icon_or_err(name: &str, size: u32, class: &str, what: &str) -> Result<String, String> {
    icon_svg(name, size, class).ok_or_else(|| format!("{}: unknown icon '{}' ({})", what, name, icon_names()))
}

/// `icon(name, { size = 16, class })`
pub fn icon(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.icon")?;
    let name = a[0].to_display_string();
    let (o, _) = split_opts(a.get(1), &["size", "class"], "")?;
    let size = match o.get("size") {
        Some(Value::Int(n)) if *n > 0 && *n <= 512 => *n as u32,
        _ => 16,
    };
    Ok(html_value(icon_or_err(&name, size, &opt_str(&o, "class").unwrap_or_default(), "ui.icon")?))
}

/// The Fogu mark: a flame on a dark tile.
pub fn logo_svg(size: u32) -> String {
    format!(
        "<svg class=\"tc-logo\" width=\"{s}\" height=\"{s}\" viewBox=\"0 0 32 32\" aria-hidden=\"true\">\
<defs><linearGradient id=\"tc-logo-g\" x1=\"0\" y1=\"1\" x2=\"0.4\" y2=\"0\"><stop offset=\"0\" stop-color=\"#ea580c\"/><stop offset=\".55\" stop-color=\"#f97316\"/><stop offset=\"1\" stop-color=\"#fbbf24\"/></linearGradient></defs>\
<rect width=\"32\" height=\"32\" rx=\"8\" fill=\"#18181b\"/>\
<path d=\"M16 27.5c4.9 0 8.2-3.2 8.2-7.6 0-3.8-2.3-6.6-4.7-8.6.3 2.4-1.1 4-2.4 4 0-4-1.9-7.7-5.3-9.8.4 3.5-1.7 6.2-3.5 8.2A9.8 9.8 0 0 0 7.8 19.9c0 4.4 3.3 7.6 8.2 7.6z\" fill=\"url(#tc-logo-g)\"/>\
<path d=\"M16 27.5c2.3 0 3.8-1.6 3.8-3.6 0-1.9-1.2-3.3-2.4-4.3.1 1.2-.5 2-1.2 2 0-1.7-.9-3.2-2.2-4.1.1 1.5-.7 2.7-1.5 3.6a4.6 4.6 0 0 0-1.2 2.9c0 2 1.6 3.5 4.7 3.5z\" fill=\"#fff7ed\" opacity=\".92\"/></svg>",
        s = size
    )
}

/// `logo({ size = 28, text })` — the mark, with an optional word beside it.
pub fn logo(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "ui.logo")?;
    let (o, _) = split_opts(a.first(), &["size", "text"], "")?;
    let size = match o.get("size") {
        Some(Value::Int(n)) if *n > 0 => *n as u32,
        _ => 28,
    };
    let mut out = logo_svg(size);
    if let Some(t) = o.get("text") {
        out = format!("<span class=\"tc-brand\">{}<span class=\"tc-brand-text\">{}</span></span>", out, children(std::slice::from_ref(t)));
    }
    Ok(html_value(out))
}

/// The favicon of the Fogu mark, as a data URL.
pub fn favicon_href() -> String {
    let svg = logo_svg(32);
    let mut out = String::from("data:image/svg+xml,");
    for c in svg.chars() {
        match c {
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '#' => out.push_str("%23"),
            '"' => out.push('\''),
            c => out.push(c),
        }
    }
    out
}

fn numbers(v: Option<&Value>) -> Vec<f64> {
    match v {
        Some(Value::List(l)) => l
            .borrow()
            .iter()
            .filter_map(|x| match x {
                Value::Int(n) => Some(*n as f64),
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// An area chart of `values` as inline SVG (in `currentColor`).
pub fn sparkline_svg(values: &[f64], w: f64, h: f64, area: bool) -> String {
    if values.is_empty() {
        return format!("<svg class=\"tc-sparkline\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" aria-hidden=\"true\"></svg>");
    }
    let (min, max) = values.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    // nothing changes: a quiet dashed baseline
    if (max - min).abs() < f64::EPSILON {
        let y = h - 3.0;
        return format!(
            "<svg class=\"tc-sparkline\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" preserveAspectRatio=\"none\" aria-hidden=\"true\"><line x1=\"0\" y1=\"{y}\" x2=\"{w}\" y2=\"{y}\" stroke=\"currentColor\" stroke-width=\"1.25\" stroke-dasharray=\"3 4\" opacity=\".35\" vector-effect=\"non-scaling-stroke\"/></svg>"
        );
    }
    let span = if (max - min).abs() < f64::EPSILON { 1.0 } else { max - min };
    let n = values.len().max(2) - 1;
    let pad = 2.0;
    let pts: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = if values.len() == 1 { w } else { i as f64 * w / n as f64 };
            let y = pad + (h - 2.0 * pad) * (1.0 - (v - min) / span);
            (x, y)
        })
        .collect();
    let line: Vec<String> = pts.iter().map(|(x, y)| format!("{:.1},{:.1}", x, y)).collect();
    let mut svg = format!("<svg class=\"tc-sparkline\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" preserveAspectRatio=\"none\" aria-hidden=\"true\">");
    if area {
        svg.push_str(&format!(
            "<polygon points=\"0,{h} {} {:.1},{h}\" fill=\"currentColor\" opacity=\".12\"/>",
            line.join(" "),
            pts.last().map_or(w, |p| p.0)
        ));
    }
    svg.push_str(&format!(
        "<polyline points=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.75\" stroke-linejoin=\"round\" stroke-linecap=\"round\" vector-effect=\"non-scaling-stroke\"/></svg>",
        line.join(" ")
    ));
    svg
}

/// `sparkline(values, { width = 120, height = 32, area = true })`
pub fn sparkline(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.sparkline")?;
    let (o, _) = split_opts(a.get(1), &["width", "height", "area"], "")?;
    let num = |k: &str, d: f64| match o.get(k) {
        Some(Value::Int(n)) => *n as f64,
        Some(Value::Float(f)) => *f,
        _ => d,
    };
    let area = o.get("area").is_none_or(Value::is_truthy);
    Ok(html_value(sparkline_svg(&numbers(a.first()), num("width", 120.0), num("height", 32.0), area)))
}

const TONES: [&str; 6] = ["default", "primary", "success", "warning", "destructive", "info"];

/// `stat(label, value, { hint, icon, trend: [numbers], tone })`
pub fn stat(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "ui.stat")?;
    let (o, attrs) = split_opts(a.get(2), &["hint", "icon", "trend", "tone"], "tc-card tc-stat")?;
    let tone = opt_str(&o, "tone").unwrap_or_else(|| "default".into());
    if !TONES.contains(&tone.as_str()) {
        return Err(format!("ui.stat: unknown tone '{}' ({})", tone, TONES.join(" ")));
    }
    let mut head = element("span", &[("class".into(), Value::Str("tc-stat-label".into()))], &children(&a[..1]))?;
    if let Some(i) = opt_str(&o, "icon") {
        head.push_str(&format!("<span class=\"tc-stat-icon\">{}</span>", icon_or_err(&i, 16, "", "ui.stat")?));
    }
    let mut inner = element("div", &[("class".into(), Value::Str("tc-stat-head".into()))], &head)?;
    inner.push_str(&element("div", &[("class".into(), Value::Str("tc-stat-value".into()))], &children(&a[1..2]))?);
    let trend = numbers(o.get("trend"));
    if !trend.is_empty() {
        inner.push_str(&format!("<div class=\"tc-stat-trend\">{}</div>", sparkline_svg(&trend, 240.0, 36.0, true)));
    }
    if let Some(h) = o.get("hint") {
        inner.push_str(&element("div", &[("class".into(), Value::Str("tc-stat-hint".into()))], &children(std::slice::from_ref(h)))?);
    }
    let mut attrs = attrs;
    if let Some((_, c)) = attrs.iter_mut().find(|(k, _)| k == "class") {
        *c = Value::Str(format!("{} tc-tone-{}", c.to_display_string(), tone).into());
    }
    Ok(html_value(element("div", &attrs, &inner)?))
}

/// `empty(title, { description, icon = "layers", action })`
pub fn empty(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "ui.empty")?;
    let (o, attrs) = split_opts(a.get(1), &["description", "icon", "action"], "tc-empty")?;
    let icon = icon_or_err(&opt_str(&o, "icon").unwrap_or_else(|| "layers".into()), 22, "", "ui.empty")?;
    let mut inner = format!("<div class=\"tc-empty-icon\">{}</div>", icon);
    inner.push_str(&element("div", &[("class".into(), Value::Str("tc-empty-title".into()))], &children(&a[..1]))?);
    if let Some(d) = o.get("description") {
        inner.push_str(&element("div", &[("class".into(), Value::Str("tc-empty-text".into()))], &children(std::slice::from_ref(d)))?);
    }
    if let Some(act) = o.get("action") {
        inner.push_str(&children(std::slice::from_ref(act)));
    }
    Ok(html_value(element("div", &attrs, &inner)?))
}

/// `live(text = "Live")` — a pulsing status dot.
pub fn live(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "ui.live")?;
    let text = a.first().cloned().unwrap_or(Value::Str("Live".into()));
    Ok(html_value(format!("<span class=\"tc-live\"><span class=\"tc-live-dot\"></span>{}</span>", children(&[text]))))
}

/// `themeToggle()` — switches light/dark and remembers the choice.
pub fn theme_toggle(a: &[Value]) -> Result<Value, String> {
    max_args(a, 0, "ui.themeToggle")?;
    Ok(html_value(format!(
        "<button class=\"tc-btn tc-btn-ghost tc-btn-icon tc-theme-toggle\" type=\"button\" aria-label=\"Switch light/dark theme\" title=\"Switch theme\" tc-on:click=\"tc.toggleTheme()\">{}{}</button>",
        icon_svg("sun", 16, "tc-only-dark").unwrap_or_default(),
        icon_svg("moon", 16, "tc-only-light").unwrap_or_default()
    )))
}

/// The icon of a nav item, if it names one.
pub fn nav_icon(m: &HashMap<String, Value>) -> Result<String, String> {
    match opt_str(m, "icon") {
        Some(i) => icon_or_err(&i, 16, "", "ui.shell"),
        None => Ok(String::new()),
    }
}
