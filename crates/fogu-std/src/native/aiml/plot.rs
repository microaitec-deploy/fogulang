//! `fogu:plot` module — charts in the style of matplotlib: line, scatter,
//! bar, area, histogram and pie charts, drawn as SVG and saved as SVG, PNG,
//! JPEG or WebP (rendered by resvg).
//!
//! ```fogu
//! import plot from "fogu:plot"
//!
//! const fig = plot.figure({ title: "Sales", xlabel: "month", ylabel: "€", width: 800, height: 450 })
//! fig.line([1, 2, 3, 4], [10, 14, 9, 17], { label: "2025" })
//! fig.line([1, 2, 3, 4], [8, 11, 13, 15], { label: "2026", dash: true, marker: true })
//! fig.save("sales.png")?
//!
//! const bars = plot.figure({ title: "Languages" })
//! bars.bar(["Fogu", "Rust", "Go"], [42, 30, 25])
//! print(bars.svg())
//! ```
//!
//! A figure holds any mix of line/scatter/area/bar/hist series on shared
//! axes (bars use categorical x positions); `pie` draws on its own.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::fmt::Write;

use crate::runtime::value::{ModuleValue, Value, display_float};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{max_args, nfn_var, opt_arg, str_arg};

const PALETTE: &[&str] = &["#f97316", "#2563eb", "#16a34a", "#dc2626", "#9333ea", "#0891b2", "#ca8a04", "#db2777", "#4b5563", "#65a30d"];

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("figure".into(), nfn_var("figure", 0, |a| {
        max_args(a, 1, "plot.figure")?;
        Ok(NativeObjectRef::new(Figure::new(opts(opt_arg(a, 0), "plot.figure")?)?).to_value())
    }));
    // shortcuts: plot.line(x, y, opts) → a figure with that one series (opts also takes the figure options)
    for kind in ["line", "scatter", "bar", "area", "hist", "pie"] {
        e.insert(kind.into(), nfn_var(kind, 1, match kind {
            "line" => |a| quick("line", a),
            "scatter" => |a| quick("scatter", a),
            "bar" => |a| quick("bar", a),
            "area" => |a| quick("area", a),
            "hist" => |a| quick("hist", a),
            _ => |a| quick("pie", a),
        }));
    }
    e.insert("palette".into(), crate::native::util::str_list(PALETTE.iter().map(|s| s.to_string())));
    ModuleValue {
        name: "fogu:plot".into(),
        exports: RefCell::new(e),
    }
}

fn quick(kind: &str, a: &[Value]) -> Result<Value, String> {
    let o = a.iter().rev().find_map(|v| if let Value::Map(m) = v { Some(m.borrow().clone()) } else { None }).unwrap_or_default();
    let mut fig = Figure::new(o)?;
    fig.call(kind, a)?;
    Ok(NativeObjectRef::new(fig).to_value())
}

fn opts(v: Option<&Value>, what: &str) -> Result<HashMap<String, Value>, String> {
    match v {
        None => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn num(v: &Value, what: &str) -> Result<f64, String> {
    match v {
        Value::Int(n) => Ok(*n as f64),
        Value::Float(f) => Ok(*f),
        Value::Bool(b) => Ok(*b as i32 as f64),
        other => Err(format!("{}: expected numbers, got '{}'", what, other.to_display_string())),
    }
}

fn nums(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    match v {
        Value::List(l) => l.borrow().iter().map(|x| num(x, what)).collect(),
        Value::Range(a, b) => Ok((*a..*b).map(|x| x as f64).collect()),
        other => Err(format!("{}: expected a list of numbers, got '{}'", what, other.type_name())),
    }
}

fn esc(s: &str) -> String {
    crate::native::html::escape(s)
}

#[derive(Clone)]
enum Kind {
    Line,
    Scatter,
    Area,
    Bar,
}

#[derive(Clone)]
struct Series {
    kind: Kind,
    x: Vec<f64>,
    y: Vec<f64>,
    label: Option<String>,
    color: String,
    width: f64,
    dash: bool,
    marker: bool,
    size: f64,
    opacity: f64,
}

struct Pie {
    labels: Vec<String>,
    values: Vec<f64>,
    donut: bool,
}

pub struct Figure {
    width: f64,
    height: f64,
    title: Option<String>,
    xlabel: Option<String>,
    ylabel: Option<String>,
    dark: bool,
    grid: bool,
    legend: bool,
    ymin: Option<f64>,
    ymax: Option<f64>,
    xmin: Option<f64>,
    xmax: Option<f64>,
    categories: Vec<String>,
    series: Vec<Series>,
    pie: Option<Pie>,
    hlines: Vec<(f64, String)>,
    vlines: Vec<(f64, String)>,
    notes: Vec<(f64, f64, String)>,
    background: Option<String>,
}

/// "Nice" tick values covering [lo, hi] (1-2-5 steps).
fn ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    if !(hi > lo) {
        return vec![lo];
    }
    let raw = (hi - lo) / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let start = (lo / step).ceil() * step;
    let mut out = Vec::new();
    let mut v = start;
    while v <= hi + step * 1e-9 {
        out.push(if v.abs() < step * 1e-9 { 0.0 } else { v });
        v += step;
    }
    out
}

fn fmt_tick(v: f64) -> String {
    let a = v.abs();
    if a >= 1e6 {
        format!("{}M", display_float(((v / 1e6) * 100.0).round() / 100.0).trim_end_matches(".0"))
    } else if a >= 1e4 {
        format!("{}k", display_float(((v / 1e3) * 100.0).round() / 100.0).trim_end_matches(".0"))
    } else {
        display_float((v * 1e6).round() / 1e6).trim_end_matches(".0").to_string()
    }
}

impl Figure {
    fn new(o: HashMap<String, Value>) -> Result<Figure, String> {
        let s = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
        let n = |k: &str| o.get(k).and_then(|v| num(v, "plot").ok());
        Ok(Figure {
            width: n("width").unwrap_or(800.0).max(100.0),
            height: n("height").unwrap_or(450.0).max(80.0),
            title: s("title"),
            xlabel: s("xlabel"),
            ylabel: s("ylabel"),
            dark: s("theme").as_deref() == Some("dark"),
            grid: o.get("grid").is_none_or(Value::is_truthy),
            legend: o.get("legend").is_none_or(Value::is_truthy),
            ymin: n("ymin"),
            ymax: n("ymax"),
            xmin: n("xmin"),
            xmax: n("xmax"),
            categories: Vec::new(),
            series: Vec::new(),
            pie: None,
            hlines: Vec::new(),
            vlines: Vec::new(),
            notes: Vec::new(),
            background: s("background"),
        })
    }

    fn next_color(&self) -> String {
        PALETTE[self.series.len() % PALETTE.len()].to_string()
    }

    fn add(&mut self, kind: Kind, x: Vec<f64>, y: Vec<f64>, o: &HashMap<String, Value>) -> Result<(), String> {
        if x.len() != y.len() {
            return Err(format!("plot: x has {} values but y has {}", x.len(), y.len()));
        }
        let color = o.get("color").map(|v| v.to_display_string()).unwrap_or_else(|| self.next_color());
        let n = |k: &str, d: f64| o.get(k).and_then(|v| num(v, "plot").ok()).unwrap_or(d);
        self.series.push(Series {
            kind,
            x,
            y,
            label: o.get("label").map(|v| v.to_display_string()),
            color,
            width: n("width", 2.0),
            dash: o.get("dash").is_some_and(Value::is_truthy),
            marker: o.get("marker").is_some_and(Value::is_truthy),
            size: n("size", 4.0),
            opacity: n("opacity", 1.0),
        });
        Ok(())
    }

    fn colors(&self) -> (&str, &str, &str, &str) {
        // background, text, grid, axis
        if self.dark { ("#0b0d12", "#e5e7eb", "#1f2937", "#6b7280") } else { ("#ffffff", "#1f2937", "#e5e7eb", "#9ca3af") }
    }

    fn render(&self) -> String {
        let (w, h) = (self.width, self.height);
        let (bg, fg, grid_c, axis_c) = self.colors();
        let bg = self.background.as_deref().unwrap_or(bg);
        let mut s = String::new();
        let _ = write!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"Inter, Segoe UI, Helvetica, Arial, DejaVu Sans, sans-serif\" font-size=\"12\">\
             <rect width=\"{w}\" height=\"{h}\" fill=\"{bg}\"/>"
        );
        let top = if self.title.is_some() { 44.0 } else { 18.0 };
        if let Some(t) = &self.title {
            let _ = write!(s, "<text x=\"{}\" y=\"28\" text-anchor=\"middle\" font-size=\"16\" font-weight=\"600\" fill=\"{fg}\">{}</text>", w / 2.0, esc(t));
        }
        if let Some(p) = &self.pie {
            self.render_pie(&mut s, p, top, fg);
            s.push_str("</svg>");
            return s;
        }
        let legend_items: Vec<&Series> = self.series.iter().filter(|x| x.label.is_some()).collect();
        let show_legend = self.legend && !legend_items.is_empty();
        let left = 62.0 + if self.ylabel.is_some() { 16.0 } else { 0.0 };
        let right = if show_legend { 150.0 } else { 20.0 };
        let bottom = 40.0 + if self.xlabel.is_some() { 18.0 } else { 0.0 };
        let (pw, ph) = ((w - left - right).max(10.0), (h - top - bottom).max(10.0));

        // data ranges
        let cat = !self.categories.is_empty();
        let bars: Vec<&Series> = self.series.iter().filter(|x| matches!(x.kind, Kind::Bar)).collect();
        let all_y = self.series.iter().flat_map(|x| x.y.iter().copied()).chain(self.hlines.iter().map(|l| l.0)).filter(|v| v.is_finite());
        let (mut y0, mut y1) = all_y.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        if !y0.is_finite() {
            (y0, y1) = (0.0, 1.0);
        }
        if !bars.is_empty() || self.series.iter().any(|x| matches!(x.kind, Kind::Area)) {
            y0 = y0.min(0.0);
            y1 = y1.max(0.0);
        }
        if y0 == y1 {
            y0 -= 1.0;
            y1 += 1.0;
        }
        let pad = (y1 - y0) * 0.05;
        let (mut y0, mut y1) = (self.ymin.unwrap_or(if y0 == 0.0 { 0.0 } else { y0 - pad }), self.ymax.unwrap_or(y1 + pad));
        let yt = ticks(y0, y1, 6);
        if self.ymin.is_none() {
            y0 = y0.min(*yt.first().unwrap_or(&y0));
        }
        if self.ymax.is_none() {
            y1 = y1.max(*yt.last().unwrap_or(&y1));
        }
        let (x0, x1) = if cat {
            (-0.5, self.categories.len() as f64 - 0.5)
        } else {
            let all_x = self.series.iter().flat_map(|x| x.x.iter().copied()).chain(self.vlines.iter().map(|l| l.0)).filter(|v| v.is_finite());
            let (a, b) = all_x.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
            let (a, b) = if a.is_finite() { (a, b) } else { (0.0, 1.0) };
            let (a, b) = if a == b { (a - 1.0, b + 1.0) } else { (a, b) };
            (self.xmin.unwrap_or(a), self.xmax.unwrap_or(b))
        };
        let sx = |x: f64| left + (x - x0) / (x1 - x0) * pw;
        let sy = |y: f64| top + ph - (y - y0) / (y1 - y0) * ph;

        // grid + y ticks
        for t in &yt {
            if *t < y0 - 1e-9 || *t > y1 + 1e-9 {
                continue;
            }
            let y = sy(*t);
            if self.grid {
                let _ = write!(s, "<line x1=\"{left}\" x2=\"{}\" y1=\"{y:.2}\" y2=\"{y:.2}\" stroke=\"{grid_c}\"/>", left + pw);
            }
            let _ = write!(s, "<text x=\"{}\" y=\"{:.2}\" text-anchor=\"end\" fill=\"{fg}\" opacity=\"0.75\">{}</text>", left - 8.0, y + 4.0, fmt_tick(*t));
        }
        // x ticks
        if cat {
            let n = self.categories.len();
            let every = (n as f64 / (pw / 60.0)).ceil().max(1.0) as usize;
            for (i, c) in self.categories.iter().enumerate() {
                if i % every != 0 {
                    continue;
                }
                let _ = write!(s, "<text x=\"{:.2}\" y=\"{}\" text-anchor=\"middle\" fill=\"{fg}\" opacity=\"0.75\">{}</text>", sx(i as f64), top + ph + 18.0, esc(c));
            }
        } else {
            for t in ticks(x0, x1, (pw / 90.0).max(2.0) as usize) {
                let x = sx(t);
                if self.grid {
                    let _ = write!(s, "<line x1=\"{x:.2}\" x2=\"{x:.2}\" y1=\"{top}\" y2=\"{}\" stroke=\"{grid_c}\" opacity=\"0.6\"/>", top + ph);
                }
                let _ = write!(s, "<text x=\"{x:.2}\" y=\"{}\" text-anchor=\"middle\" fill=\"{fg}\" opacity=\"0.75\">{}</text>", top + ph + 18.0, fmt_tick(t));
            }
        }
        // axes
        let _ = write!(s, "<line x1=\"{left}\" x2=\"{}\" y1=\"{}\" y2=\"{}\" stroke=\"{axis_c}\"/>", left + pw, top + ph, top + ph);
        let _ = write!(s, "<line x1=\"{left}\" x2=\"{left}\" y1=\"{top}\" y2=\"{}\" stroke=\"{axis_c}\"/>", top + ph);
        if let Some(l) = &self.xlabel {
            let _ = write!(s, "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" fill=\"{fg}\">{}</text>", left + pw / 2.0, h - 10.0, esc(l));
        }
        if let Some(l) = &self.ylabel {
            let (lx, ly) = (16.0, top + ph / 2.0);
            let _ = write!(s, "<text x=\"{lx}\" y=\"{ly}\" text-anchor=\"middle\" fill=\"{fg}\" transform=\"rotate(-90 {lx} {ly})\">{}</text>", esc(l));
        }

        let _ = write!(s, "<defs><clipPath id=\"plot\"><rect x=\"{left}\" y=\"{top}\" width=\"{pw}\" height=\"{ph}\"/></clipPath></defs><g clip-path=\"url(#plot)\">");
        // bars (grouped)
        let nb = bars.len().max(1) as f64;
        let group = 0.8;
        let bw = pw / (x1 - x0) * group / nb;
        for (bi, b) in bars.iter().enumerate() {
            for (x, y) in b.x.iter().zip(&b.y) {
                let cx = sx(*x) - bw * nb / 2.0 + bw * bi as f64;
                let (ya, yb) = (sy(y.max(0.0)), sy(y.min(0.0)));
                let _ = write!(
                    s,
                    "<rect x=\"{cx:.2}\" y=\"{ya:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\" opacity=\"{}\" rx=\"2\"/>",
                    (bw - 2.0).max(1.0),
                    (yb - ya).max(0.0),
                    b.color,
                    b.opacity
                );
            }
        }
        for sr in self.series.iter().filter(|x| !matches!(x.kind, Kind::Bar)) {
            let pts: Vec<(f64, f64)> = sr.x.iter().zip(&sr.y).filter(|(x, y)| x.is_finite() && y.is_finite()).map(|(x, y)| (sx(*x), sy(*y))).collect();
            let path = |pts: &[(f64, f64)]| pts.iter().enumerate().map(|(i, (x, y))| format!("{}{:.2},{:.2}", if i == 0 { "M" } else { "L" }, x, y)).collect::<String>();
            match sr.kind {
                Kind::Area if !pts.is_empty() => {
                    let base = sy(0f64.clamp(y0, y1));
                    let d = format!("{}L{:.2},{base:.2}L{:.2},{base:.2}Z", path(&pts), pts.last().unwrap().0, pts[0].0);
                    let _ = write!(s, "<path d=\"{d}\" fill=\"{}\" opacity=\"{}\"/>", sr.color, 0.25 * sr.opacity);
                    let _ = write!(s, "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>", path(&pts), sr.color, sr.width);
                }
                Kind::Line => {
                    let dash = if sr.dash { " stroke-dasharray=\"6 4\"" } else { "" };
                    let _ = write!(
                        s,
                        "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\" opacity=\"{}\"{dash}/>",
                        path(&pts),
                        sr.color,
                        sr.width,
                        sr.opacity
                    );
                    if sr.marker {
                        for (x, y) in &pts {
                            let _ = write!(s, "<circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"{}\" fill=\"{}\" stroke=\"{bg}\" stroke-width=\"1.5\"/>", sr.size, sr.color);
                        }
                    }
                }
                Kind::Scatter => {
                    for (x, y) in &pts {
                        let _ = write!(s, "<circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"{}\" fill=\"{}\" opacity=\"{}\"/>", sr.size, sr.color, 0.8 * sr.opacity);
                    }
                }
                _ => {}
            }
        }
        for (y, c) in &self.hlines {
            let _ = write!(s, "<line x1=\"{left}\" x2=\"{}\" y1=\"{:.2}\" y2=\"{:.2}\" stroke=\"{c}\" stroke-dasharray=\"5 4\"/>", left + pw, sy(*y), sy(*y));
        }
        for (x, c) in &self.vlines {
            let _ = write!(s, "<line x1=\"{:.2}\" x2=\"{:.2}\" y1=\"{top}\" y2=\"{}\" stroke=\"{c}\" stroke-dasharray=\"5 4\"/>", sx(*x), sx(*x), top + ph);
        }
        s.push_str("</g>");
        for (x, y, t) in &self.notes {
            let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"{fg}\">{}</text>", sx(*x), sy(*y), esc(t));
        }
        if show_legend {
            let lx = left + pw + 16.0;
            for (i, sr) in legend_items.iter().enumerate() {
                let ly = top + 8.0 + i as f64 * 20.0;
                match sr.kind {
                    Kind::Line => {
                        let dash = if sr.dash { " stroke-dasharray=\"5 3\"" } else { "" };
                        let _ = write!(s, "<line x1=\"{lx}\" x2=\"{}\" y1=\"{ly}\" y2=\"{ly}\" stroke=\"{}\" stroke-width=\"2.5\"{dash}/>", lx + 18.0, sr.color);
                    }
                    Kind::Scatter => {
                        let _ = write!(s, "<circle cx=\"{}\" cy=\"{ly}\" r=\"4.5\" fill=\"{}\"/>", lx + 9.0, sr.color);
                    }
                    _ => {
                        let _ = write!(s, "<rect x=\"{lx}\" y=\"{}\" width=\"18\" height=\"10\" rx=\"2\" fill=\"{}\"/>", ly - 5.0, sr.color);
                    }
                }
                let _ = write!(s, "<text x=\"{}\" y=\"{}\" fill=\"{fg}\">{}</text>", lx + 24.0, ly + 4.0, esc(sr.label.as_deref().unwrap_or("")));
            }
        }
        s.push_str("</svg>");
        s
    }

    fn render_pie(&self, s: &mut String, p: &Pie, top: f64, fg: &str) {
        let total: f64 = p.values.iter().filter(|v| **v > 0.0).sum();
        let legend_w = if self.legend { 170.0 } else { 0.0 };
        let r = ((self.width - legend_w).min(self.height - top) / 2.0 - 16.0).max(10.0);
        let (cx, cy) = ((self.width - legend_w) / 2.0, top + (self.height - top) / 2.0);
        let mut ang = -std::f64::consts::FRAC_PI_2;
        let (bg, ..) = self.colors();
        for (i, v) in p.values.iter().enumerate() {
            if *v <= 0.0 || total <= 0.0 {
                continue;
            }
            let frac = v / total;
            let a2 = ang + frac * std::f64::consts::TAU;
            let color = PALETTE[i % PALETTE.len()];
            if frac >= 0.9999 {
                let _ = write!(s, "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{color}\"/>");
            } else {
                let (x1, y1, x2, y2) = (cx + r * ang.cos(), cy + r * ang.sin(), cx + r * a2.cos(), cy + r * a2.sin());
                let large = if frac > 0.5 { 1 } else { 0 };
                let _ = write!(
                    s,
                    "<path d=\"M{cx:.2},{cy:.2}L{x1:.2},{y1:.2}A{r:.2},{r:.2} 0 {large} 1 {x2:.2},{y2:.2}Z\" fill=\"{color}\" stroke=\"{bg}\" stroke-width=\"2\"/>"
                );
            }
            if frac > 0.04 {
                let mid = (ang + a2) / 2.0;
                let lr = if p.donut { r * 0.8 } else { r * 0.62 };
                let _ = write!(
                    s,
                    "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"middle\" fill=\"#ffffff\" font-weight=\"600\">{}%</text>",
                    cx + lr * mid.cos(),
                    cy + lr * mid.sin() + 4.0,
                    (frac * 1000.0).round() / 10.0
                );
            }
            ang = a2;
        }
        if p.donut {
            let _ = write!(s, "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{:.2}\" fill=\"{bg}\"/>", r * 0.58);
        }
        if self.legend {
            let lx = self.width - legend_w + 10.0;
            for (i, l) in p.labels.iter().enumerate() {
                let ly = top + 10.0 + i as f64 * 20.0;
                let _ = write!(s, "<rect x=\"{lx}\" y=\"{}\" width=\"12\" height=\"12\" rx=\"2\" fill=\"{}\"/>", ly - 6.0, PALETTE[i % PALETTE.len()]);
                let _ = write!(s, "<text x=\"{}\" y=\"{}\" fill=\"{fg}\">{}</text>", lx + 18.0, ly + 4.0, esc(l));
            }
        }
    }

    /// x positions for categorical bars (registering new categories).
    fn category_positions(&mut self, cats: &[String]) -> Vec<f64> {
        cats.iter()
            .map(|c| match self.categories.iter().position(|x| x == c) {
                Some(i) => i as f64,
                None => {
                    self.categories.push(c.clone());
                    (self.categories.len() - 1) as f64
                }
            })
            .collect()
    }
}

impl FoguObject for Figure {
    fn name(&self) -> &str {
        "Figure"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "__display__" => Some(Value::Str(format!("<Figure {}x{} series={}>", self.width, self.height, self.series.len()).into())),
            "series" => Some(Value::Int(self.series.len() as i64)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let o = a.iter().skip(1).find_map(|v| if let Value::Map(m) = v { Some(m.borrow().clone()) } else { None }).unwrap_or_default();
        let positional: Vec<&Value> = a.iter().filter(|v| !matches!(v, Value::Map(_))).collect();
        match method {
            // line(y) · line(x, y, opts) — also scatter and area
            "line" | "scatter" | "area" => {
                let (x, y) = match positional.as_slice() {
                    [y] => {
                        let y = nums(y, method)?;
                        ((0..y.len()).map(|i| i as f64).collect(), y)
                    }
                    [x, y, ..] => (nums(x, method)?, nums(y, method)?),
                    _ => return Err(format!("{}(x, y, options) or {}(y)", method, method)),
                };
                let kind = match method { "line" => Kind::Line, "scatter" => Kind::Scatter, _ => Kind::Area };
                self.add(kind, x, y, &o)?;
            }
            // bar(categories, values, opts) · bar(values)
            "bar" => {
                let (cats, vals): (Vec<String>, Vec<f64>) = match positional.as_slice() {
                    [v] => {
                        let v = nums(v, "bar")?;
                        ((0..v.len()).map(|i| i.to_string()).collect(), v)
                    }
                    [c, v, ..] => {
                        let cats = match c {
                            Value::List(l) => l.borrow().iter().map(|x| x.to_display_string()).collect(),
                            other => return Err(format!("bar: categories must be a list, got '{}'", other.type_name())),
                        };
                        (cats, nums(v, "bar")?)
                    }
                    _ => return Err("bar(categories, values, options)".into()),
                };
                let x = self.category_positions(&cats);
                self.add(Kind::Bar, x, vals, &o)?;
            }
            // hist(values, { bins = 10 }) — bars over bin centers
            "hist" => {
                let v = nums(positional.first().ok_or("hist(values, options)")?, "hist")?;
                if v.is_empty() {
                    return Err("hist: no values".into());
                }
                let bins = o.get("bins").and_then(|b| num(b, "hist").ok()).unwrap_or(10.0).max(1.0) as usize;
                let (lo, hi) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
                let span = if hi > lo { hi - lo } else { 1.0 };
                let mut counts = vec![0.0; bins];
                for x in &v {
                    let i = (((x - lo) / span) * bins as f64).floor() as usize;
                    counts[i.min(bins - 1)] += 1.0;
                }
                let cats: Vec<String> = (0..bins).map(|i| fmt_tick(((lo + span * (i as f64 + 0.5) / bins as f64) * 100.0).round() / 100.0)).collect();
                let x = self.category_positions(&cats);
                self.add(Kind::Bar, x, counts, &o)?;
            }
            // pie(labels, values, { donut })
            "pie" => {
                let (labels, values) = match positional.as_slice() {
                    [l, v, ..] => (
                        match l {
                            Value::List(l) => l.borrow().iter().map(|x| x.to_display_string()).collect::<Vec<String>>(),
                            _ => return Err("pie: labels must be a list".into()),
                        },
                        nums(v, "pie")?,
                    ),
                    _ => return Err("pie(labels, values, options)".into()),
                };
                if labels.len() != values.len() {
                    return Err("pie: labels and values have different lengths".into());
                }
                self.pie = Some(Pie { labels, values, donut: o.get("donut").is_some_and(Value::is_truthy) });
            }
            "hline" | "vline" => {
                let v = num(positional.first().ok_or(format!("{}(value)", method))?, method)?;
                let c = o.get("color").map(|v| v.to_display_string()).unwrap_or_else(|| "#ef4444".into());
                if method == "hline" { self.hlines.push((v, c)) } else { self.vlines.push((v, c)) }
            }
            "text" => {
                let x = num(positional.first().ok_or("text(x, y, str)")?, "text")?;
                let y = num(positional.get(1).ok_or("text(x, y, str)")?, "text")?;
                self.notes.push((x, y, positional.get(2).map(|v| v.to_display_string()).unwrap_or_default()));
            }
            "title" | "xlabel" | "ylabel" => {
                let t = Some(str_arg(a, 0, method)?);
                match method {
                    "title" => self.title = t,
                    "xlabel" => self.xlabel = t,
                    _ => self.ylabel = t,
                }
            }
            "svg" => return Ok(Value::Str(self.render().into())),
            // image({ scale }) → Image
            "image" => {
                let mut ro = HashMap::new();
                if let Some(Value::Map(m)) = a.first() {
                    if let Some(sc) = m.borrow().get("scale") {
                        ro.insert("scale".to_string(), sc.clone());
                    }
                }
                return Ok(crate::native::image::wrap(crate::native::image::render_svg(&self.render(), &ro)?));
            }
            // save(path, { scale = 2, quality }) — .svg as text, .png/.jpg/.webp rendered
            "save" => {
                let path = str_arg(a, 0, "save")?;
                let svg = self.render();
                if path.to_ascii_lowercase().ends_with(".svg") {
                    std::fs::write(&path, svg).map_err(|e| io_err!("plot.save: {}: {}", path, e))?;
                    return Ok(Value::Nil);
                }
                let mut ro = HashMap::new();
                ro.insert("scale".to_string(), o.get("scale").cloned().unwrap_or(Value::Int(2)));
                let img = crate::native::image::render_svg(&svg, &ro)?;
                let data = crate::native::image::encode(&img, crate::native::image::format_of(&path)?, &o)?;
                std::fs::write(&path, data).map_err(|e| io_err!("plot.save: {}: {}", path, e))?;
                return Ok(Value::Nil);
            }
            other => return Err(format!("method '{}' does not exist on Figure ({})", other, self.methods().join(" "))),
        }
        Ok(Value::Nil)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["line", "scatter", "area", "bar", "hist", "pie", "hline", "vline", "text", "title", "xlabel", "ylabel", "svg", "image", "save"]
    }
}
