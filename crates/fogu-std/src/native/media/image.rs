//! `fogu:image` module — create, transform, draw on and encode images; SVG
//! rendering (resvg) and animated GIFs.
//!
//! ```fogu
//! import image from "fogu:image"
//!
//! const img = image.new(320, 200, "#1e293b")
//! img.rect(20, 20, 120, 80, { fill: "#f97316", radius: 12 })
//! img.circle(240, 100, 50, { stroke: "white", width: 4 })
//! img.text("Fogu", 30, 150, { size: 32, color: "white" })
//! img.save("card.png")?
//! image.open("photo.jpg")?.resize(800).grayscale().save("small.webp")?
//! image.writeGif("spin.gif", frames, { delay: 80ms })?
//! ```
//!
//! Transformations (`resize crop rotate blur grayscale …`) return a **new**
//! image; drawing methods (`rect circle line polygon text paste fill
//! setPixel drawSvg`) change the image in place and return nil. Colors are
//! `"#rgb"`, `"#rrggbb"`, `"#rrggbbaa"`, a CSS name (`"red"`), `"rgb(r,g,b)"`
//! or `[r, g, b, a?]`. Bytes are lists of ints (like `fs`/`crypto`).

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::io::Cursor;
use std::sync::Arc;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use base64::Engine;
use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::imageops::FilterType;
use image::{AnimationDecoder, Delay, DynamicImage, Frame, ImageFormat, Rgba, RgbaImage};
use resvg::tiny_skia::{self, Paint, PathBuilder, Pixmap, Stroke, Transform};
use resvg::usvg;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{duration_arg, list, max_args, nfn_var, opt_arg, path_str, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    // image.new(width, height, background = transparent)
    e.insert("new".into(), nfn_var("new", 2, |a| {
        max_args(a, 3, "image.new")?;
        let (w, h) = (dim(&a[0], "image.new")?, dim(&a[1], "image.new")?);
        let bg = match opt_arg(a, 2) {
            Some(c) => color(c)?,
            None => Rgba([0, 0, 0, 0]),
        };
        Ok(wrap(RgbaImage::from_pixel(w, h, bg)))
    }));
    e.insert("open".into(), nfn_var("open", 1, |a| {
        max_args(a, 1, "image.open")?;
        let path = str_arg(a, 0, "image.open")?;
        let img = image::open(&path).map_err(|e| io_err!("image.open: {}: {}", path, e))?;
        Ok(wrap(img.to_rgba8()))
    }));
    // image.decode(bytes | base64 | data URL)
    e.insert("decode".into(), nfn_var("decode", 1, |a| {
        max_args(a, 1, "image.decode")?;
        let bytes = input_bytes(&a[0], "image.decode")?;
        let img = image::load_from_memory(&bytes).map_err(|e| io_err!("image.decode: {}", e))?;
        Ok(wrap(img.to_rgba8()))
    }));
    // image.fromSvg(svg, { width, height, scale, background })
    e.insert("fromSvg".into(), nfn_var("fromSvg", 1, |a| {
        max_args(a, 2, "image.fromSvg")?;
        let svg = str_arg(a, 0, "image.fromSvg")?;
        let o = opts(opt_arg(a, 1), "image.fromSvg")?;
        Ok(wrap(render_svg(&svg, &o)?))
    }));
    // image.fromHtml(html, { width = 800, height = 600, fullPage = true }) — rendered by a
    // headless Chrome/Edge (see fogu:webview); transparent where the page has no background
    e.insert("fromHtml".into(), nfn_var("fromHtml", 1, |a| {
        max_args(a, 2, "image.fromHtml")?;
        let html = str_arg(a, 0, "image.fromHtml")?;
        let o = opts(opt_arg(a, 1), "image.fromHtml")?;
        let w = o.get("width").map(|v| dim(v, "image.fromHtml")).transpose()?.unwrap_or(800);
        let h = o.get("height").map(|v| dim(v, "image.fromHtml")).transpose()?.unwrap_or(600);
        let full = o.get("fullPage").is_none_or(Value::is_truthy);
        #[cfg(not(target_family = "wasm"))]
        {
            let png = crate::native::rt::offload(move || crate::native::webview::html_to_png(&html, w, h, full))??;
            from_encoded(&png)
        }
        #[cfg(target_family = "wasm")]
        {
            let _ = (html, w, h, full);
            Err(io_err!("image.fromHtml: needs a browser, not available on WebAssembly"))
        }
    }));
    // image.frames(path | bytes) → the frames of an animated GIF ({ image, delay })
    e.insert("frames".into(), nfn_var("frames", 1, |a| {
        max_args(a, 1, "image.frames")?;
        let bytes = match &a[0] {
            Value::Str(p) if !p.starts_with("data:") => std::fs::read(p).map_err(|e| io_err!("image.frames: {}: {}", p, e))?,
            v => input_bytes(v, "image.frames")?,
        };
        let dec = GifDecoder::new(Cursor::new(bytes)).map_err(|e| io_err!("image.frames: {}", e))?;
        let frames = dec.into_frames().collect_frames().map_err(|e| io_err!("image.frames: {}", e))?;
        Ok(list(
            frames
                .into_iter()
                .map(|f| {
                    let (n, d) = f.delay().numer_denom_ms();
                    let ms = if d == 0 { 0 } else { (n / d) as i64 };
                    crate::native::util::map(vec![("image", wrap(f.into_buffer())), ("delay", Value::Duration(ms * 1_000_000))])
                })
                .collect(),
        ))
    }));
    // image.gif(frames, { delay = 100ms, loop = 0 }) → bytes
    e.insert("gif".into(), nfn_var("gif", 1, |a| {
        max_args(a, 2, "image.gif")?;
        Ok(bytes_value(encode_gif(&a[0], opt_arg(a, 1))?))
    }));
    e.insert("writeGif".into(), nfn_var("writeGif", 2, |a| {
        max_args(a, 3, "image.writeGif")?;
        let path = str_arg(a, 0, "image.writeGif")?;
        let data = encode_gif(&a[1], opt_arg(a, 2))?;
        std::fs::write(&path, data).map_err(|e| io_err!("image.writeGif: {}: {}", path, e))?;
        Ok(Value::Nil)
    }));
    // image.info(path) → { width, height, format } without decoding the pixels
    e.insert("info".into(), nfn_var("info", 1, |a| {
        max_args(a, 1, "image.info")?;
        let path = str_arg(a, 0, "image.info")?;
        let r = image::ImageReader::open(&path)
            .and_then(|r| r.with_guessed_format())
            .map_err(|e| io_err!("image.info: {}: {}", path, e))?;
        let fmt = r.format().map(|f| f.extensions_str()[0].to_string());
        let (w, h) = r.into_dimensions().map_err(|e| io_err!("image.info: {}: {}", path, e))?;
        Ok(crate::native::util::map(vec![
            ("width", Value::Int(w as i64)),
            ("height", Value::Int(h as i64)),
            ("format", fmt.map_or(Value::Nil, Value::str)),
        ]))
    }));
    e.insert("color".into(), nfn_var("color", 1, |a| {
        max_args(a, 1, "image.color")?;
        let c = color(&a[0])?;
        Ok(list(c.0.iter().map(|&x| Value::Int(x as i64)).collect()))
    }));
    ModuleValue {
        name: "fogu:image".into(),
        exports: RefCell::new(e),
    }
}

/// An image as a Fogu value (used by `fogu:webview` screenshots too).
pub fn wrap(img: RgbaImage) -> Value {
    NativeObjectRef::new(Image { img }).to_value()
}

/// Decodes encoded bytes (PNG, JPEG…) into an image value.
pub fn from_encoded(bytes: &[u8]) -> Result<Value, String> {
    let img = image::load_from_memory(bytes).map_err(|e| io_err!("image: {}", e))?;
    Ok(wrap(img.to_rgba8()))
}

fn bytes_value(data: Vec<u8>) -> Value {
    Value::buffer(data)
}

fn dim(v: &Value, what: &str) -> Result<u32, String> {
    match v {
        Value::Int(n) if *n > 0 && *n <= 65535 => Ok(*n as u32),
        Value::Float(f) if *f >= 1.0 && *f <= 65535.0 => Ok(f.round() as u32),
        other => Err(format!("{}: sizes must be ints in 1..65535, got {}", what, other.to_display_string())),
    }
}

fn num(v: &Value, what: &str) -> Result<f32, String> {
    match v {
        Value::Int(n) => Ok(*n as f32),
        Value::Float(f) => Ok(*f as f32),
        other => Err(format!("{}: expected a number, got '{}'", what, other.type_name())),
    }
}

fn num_at(a: &[Value], i: usize, what: &str) -> Result<f32, String> {
    match a.get(i) {
        Some(v) => num(v, what),
        None => Err(format!("{}: missing argument {}", what, i + 1)),
    }
}

fn opts(v: Option<&Value>, what: &str) -> Result<HashMap<String, Value>, String> {
    match v {
        None => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

/// Bytes from a buffer, a list of ints, a base64 string or a `data:` URL.
fn input_bytes(v: &Value, what: &str) -> Result<Vec<u8>, String> {
    match v {
        Value::Buffer(b) => Ok(b.borrow().clone()),
        Value::List(l) => l
            .borrow()
            .iter()
            .map(|b| match b {
                Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                _ => Err(format!("{}: byte lists must hold ints 0..255", what)),
            })
            .collect(),
        Value::Str(s) => {
            let b64 = match s.strip_prefix("data:") {
                Some(rest) => rest.split_once(',').map(|(_, d)| d).unwrap_or(""),
                None => s.as_str(),
            };
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|_| io_err!("{}: the text is not valid base64", what))
        }
        other => Err(format!("{}: expected bytes (list of ints) or base64 str, got '{}'", what, other.type_name())),
    }
}

// ================= COLORS =================

const NAMED: &[(&str, [u8; 4])] = &[
    ("transparent", [0, 0, 0, 0]), ("black", [0, 0, 0, 255]), ("white", [255, 255, 255, 255]),
    ("red", [255, 0, 0, 255]), ("green", [0, 128, 0, 255]), ("lime", [0, 255, 0, 255]),
    ("blue", [0, 0, 255, 255]), ("yellow", [255, 255, 0, 255]), ("cyan", [0, 255, 255, 255]),
    ("magenta", [255, 0, 255, 255]), ("gray", [128, 128, 128, 255]), ("grey", [128, 128, 128, 255]),
    ("silver", [192, 192, 192, 255]), ("orange", [255, 165, 0, 255]), ("purple", [128, 0, 128, 255]),
    ("pink", [255, 192, 203, 255]), ("brown", [165, 42, 42, 255]), ("navy", [0, 0, 128, 255]),
    ("teal", [0, 128, 128, 255]), ("olive", [128, 128, 0, 255]), ("maroon", [128, 0, 0, 255]),
    ("gold", [255, 215, 0, 255]), ("indigo", [75, 0, 130, 255]), ("violet", [238, 130, 238, 255]),
];

pub fn color(v: &Value) -> Result<Rgba<u8>, String> {
    let bad = || format!("image: invalid color {}", v.to_display_string());
    match v {
        Value::Str(s) => {
            let s = s.trim().to_ascii_lowercase();
            if let Some(hex) = s.strip_prefix('#') {
                let h = |i: usize, n: usize| u8::from_str_radix(&hex[i..i + n], 16).map_err(|_| bad());
                let x = |i: usize| h(i, 1).map(|d| d * 17);
                return Ok(Rgba(match hex.len() {
                    3 => [x(0)?, x(1)?, x(2)?, 255],
                    4 => [x(0)?, x(1)?, x(2)?, x(3)?],
                    6 => [h(0, 2)?, h(2, 2)?, h(4, 2)?, 255],
                    8 => [h(0, 2)?, h(2, 2)?, h(4, 2)?, h(6, 2)?],
                    _ => return Err(bad()),
                }));
            }
            if let Some(inner) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
                let parts: Vec<f32> = inner.split(',').map(|p| p.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|_| bad())?;
                let c = |f: f32| f.clamp(0.0, 255.0).round() as u8;
                return match parts[..] {
                    [r, g, b] => Ok(Rgba([c(r), c(g), c(b), 255])),
                    // alpha in 0..1, like CSS
                    [r, g, b, a] => Ok(Rgba([c(r), c(g), c(b), c(a * 255.0)])),
                    _ => Err(bad()),
                };
            }
            NAMED.iter().find(|(n, _)| *n == s).map(|(_, c)| Rgba(*c)).ok_or_else(bad)
        }
        Value::List(l) => {
            let l = l.borrow();
            let c: Vec<u8> = l
                .iter()
                .map(|x| match x {
                    Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                    _ => Err(bad()),
                })
                .collect::<Result<_, _>>()?;
            match c[..] {
                [r, g, b] => Ok(Rgba([r, g, b, 255])),
                [r, g, b, a] => Ok(Rgba([r, g, b, a])),
                _ => Err(bad()),
            }
        }
        _ => Err(bad()),
    }
}

fn color_hex(c: Rgba<u8>) -> String {
    let [r, g, b, a] = c.0;
    if a == 255 { format!("#{:02x}{:02x}{:02x}", r, g, b) } else { format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a) }
}

// ================= PIXMAP BRIDGE (tiny-skia is premultiplied) =================

fn to_pixmap(img: &RgbaImage) -> Pixmap {
    let mut pm = Pixmap::new(img.width(), img.height()).expect("non-empty image");
    for (dst, src) in pm.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    pm
}

fn from_pixmap(pm: &Pixmap) -> RgbaImage {
    let mut img = RgbaImage::new(pm.width(), pm.height());
    for (dst, src) in img.pixels_mut().zip(pm.pixels()) {
        let c = src.demultiply();
        *dst = Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    img
}

fn paint(c: Rgba<u8>) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0[0], c.0[1], c.0[2], c.0[3]);
    p.anti_alias = true;
    p
}

// ================= FONTS / SVG =================

thread_local! {
    static FONTDB: RefCell<Option<Arc<usvg::fontdb::Database>>> = const { RefCell::new(None) };
    static FONTS: RefCell<HashMap<String, FontArc>> = RefCell::new(HashMap::new());
}

/// The system fonts, loaded once (used for SVG text and `img.text`).
fn fontdb() -> Arc<usvg::fontdb::Database> {
    FONTDB.with(|db| {
        db.borrow_mut()
            .get_or_insert_with(|| {
                let mut d = usvg::fontdb::Database::new();
                d.load_system_fonts();
                // the generic families default to Arial/Times/Courier: use what is installed
                let has = |d: &usvg::fontdb::Database, name: &str| d.faces().any(|f| f.families.iter().any(|(n, _)| n == name));
                let pick = |d: &usvg::fontdb::Database, names: &[&str]| names.iter().find(|n| has(d, n)).map(|n| n.to_string());
                if let Some(n) = pick(&d, &["Arial", "Helvetica", "Segoe UI", "DejaVu Sans", "Liberation Sans", "Noto Sans", "Ubuntu", "Cantarell"]) {
                    d.set_sans_serif_family(n);
                }
                if let Some(n) = pick(&d, &["Times New Roman", "Times", "DejaVu Serif", "Liberation Serif", "Noto Serif"]) {
                    d.set_serif_family(n);
                }
                if let Some(n) = pick(&d, &["Courier New", "Consolas", "Menlo", "DejaVu Sans Mono", "Liberation Mono", "Ubuntu Mono", "Noto Sans Mono"]) {
                    d.set_monospace_family(n);
                }
                Arc::new(d)
            })
            .clone()
    })
}

/// A font file (`.ttf`/`.otf`) or a system family (`"sans-serif"`, `"DejaVu Sans"`, `"monospace"`).
fn font(spec: &str, bold: bool) -> Result<FontArc, String> {
    let key = format!("{}|{}", spec, bold);
    if let Some(f) = FONTS.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(f);
    }
    let f = if std::path::Path::new(spec).is_file() {
        let data = std::fs::read(spec).map_err(|e| io_err!("image: font {}: {}", spec, e))?;
        FontArc::try_from_vec(data).map_err(|_| io_err!("image: '{}' is not a valid font file", spec))?
    } else {
        use usvg::fontdb::{Family, Query, Weight};
        let db = fontdb();
        let family = match spec {
            "serif" => Family::Serif,
            "monospace" | "mono" => Family::Monospace,
            "sans-serif" | "sans" | "" => Family::SansSerif,
            name => Family::Name(name),
        };
        let weight = if bold { Weight::BOLD } else { Weight::NORMAL };
        let id = db
            .query(&Query { families: &[family, Family::SansSerif], weight, ..Query::default() })
            .or_else(|| db.faces().next().map(|f| f.id))
            .ok_or_else(|| io_err!("image: no fonts found on this system; pass {{ font: \"path/to/font.ttf\" }}"))?;
        db.with_face_data(id, |data, index| ab_glyph::FontVec::try_from_vec_and_index(data.to_vec(), index).ok())
            .flatten()
            .map(FontArc::new)
            .ok_or_else(|| io_err!("image: could not load the font '{}'", spec))?
    };
    FONTS.with(|c| c.borrow_mut().insert(key, f.clone()));
    Ok(f)
}

fn svg_tree(svg: &str) -> Result<usvg::Tree, String> {
    let mut opt = usvg::Options::default();
    opt.fontdb = fontdb();
    usvg::Tree::from_str(svg, &opt).map_err(|e| io_err!("image: invalid SVG: {}", e))
}

/// Renders an SVG at `{ width, height, scale }` (keeping the aspect ratio
/// when only one side is given) over an optional `background`.
pub fn render_svg(svg: &str, o: &HashMap<String, Value>) -> Result<RgbaImage, String> {
    let tree = svg_tree(svg)?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    let scale = o.get("scale").map(|v| num(v, "image.fromSvg")).transpose()?.unwrap_or(1.0);
    let (w, h) = match (o.get("width"), o.get("height")) {
        (Some(w), Some(h)) => (dim(w, "image.fromSvg")? as f32, dim(h, "image.fromSvg")? as f32),
        (Some(w), None) => {
            let w = dim(w, "image.fromSvg")? as f32;
            (w, sh * w / sw)
        }
        (None, Some(h)) => {
            let h = dim(h, "image.fromSvg")? as f32;
            (sw * h / sh, h)
        }
        (None, None) => (sw * scale, sh * scale),
    };
    let (w, h) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
    let mut pm = Pixmap::new(w, h).ok_or_else(|| io_err!("image.fromSvg: invalid size {}x{}", w, h))?;
    if let Some(bg) = o.get("background") {
        let c = color(bg)?;
        pm.fill(tiny_skia::Color::from_rgba8(c.0[0], c.0[1], c.0[2], c.0[3]));
    }
    resvg::render(&tree, Transform::from_scale(w as f32 / sw, h as f32 / sh), &mut pm.as_mut());
    Ok(from_pixmap(&pm))
}

// ================= GIF =================

fn encode_gif(frames: &Value, o: Option<&Value>) -> Result<Vec<u8>, String> {
    let o = opts(o, "image.gif")?;
    let default_delay = match o.get("delay") {
        Some(d) => duration_arg(d, "image.gif delay")?.as_millis() as u32,
        None => 100,
    };
    let repeat = match o.get("loop") {
        None | Some(Value::Int(0)) | Some(Value::Bool(true)) => Repeat::Infinite,
        Some(Value::Bool(false)) => Repeat::Finite(0),
        Some(Value::Int(n)) if *n > 0 => Repeat::Finite((*n).min(65535) as u16),
        Some(other) => return Err(format!("image.gif: 'loop' must be an int >= 0 or bool, got {}", other.to_display_string())),
    };
    let speed = match o.get("speed") {
        Some(Value::Int(n)) if (1..=30).contains(n) => *n as i32,
        Some(other) => return Err(format!("image.gif: 'speed' must be an int in 1..30, got {}", other.to_display_string())),
        None => 10,
    };
    let Value::List(items) = frames else {
        return Err(format!("image.gif: frames must be a list of images, got '{}'", frames.type_name()));
    };
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new_with_speed(&mut out, speed);
        enc.set_repeat(repeat).map_err(|e| io_err!("image.gif: {}", e))?;
        let items = items.borrow();
        if items.is_empty() {
            return Err("image.gif: no frames".into());
        }
        let mut size = None;
        for it in items.iter() {
            // a frame is an image or { image, delay }
            let (img, delay) = match it {
                Value::Map(m) => {
                    let m = m.borrow();
                    let img = m.get("image").cloned().unwrap_or(Value::Nil);
                    let d = match m.get("delay") {
                        Some(d) => duration_arg(d, "image.gif delay")?.as_millis() as u32,
                        None => default_delay,
                    };
                    (img, d)
                }
                v => (v.clone(), default_delay),
            };
            let mut buf = with_image(&img, "image.gif", |i| i.clone())?;
            // every frame takes the size of the first one
            let (w, h) = *size.get_or_insert(buf.dimensions());
            if buf.dimensions() != (w, h) {
                buf = image::imageops::resize(&buf, w, h, FilterType::Triangle);
            }
            enc.encode_frame(Frame::from_parts(buf, 0, 0, Delay::from_numer_denom_ms(delay, 1)))
                .map_err(|e| io_err!("image.gif: {}", e))?;
        }
    }
    Ok(out)
}

/// Runs `f` with the pixels of an `Image` value.
pub fn with_image<R>(v: &Value, what: &str, f: impl FnOnce(&RgbaImage) -> R) -> Result<R, String> {
    if let Value::Object(o) = v {
        let inner = o.inner();
        // already borrowed: the image being changed (img.paste(img, …))
        let b = inner.try_borrow().map_err(|_| format!("{}: an image cannot be drawn onto itself (use clone())", what))?;
        if let Some(img) = b.as_any().downcast_ref::<Image>() {
            return Ok(f(&img.img));
        }
    }
    Err(format!("{}: expected an Image, got '{}'", what, v.type_name()))
}

// ================= ENCODING =================

pub fn format_of(name: &str) -> Result<ImageFormat, String> {
    let ext = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
    ImageFormat::from_extension(&ext).ok_or_else(|| io_err!("image: unknown image format '{}' (png jpg webp gif bmp ico tiff)", name))
}

pub fn encode(img: &RgbaImage, fmt: ImageFormat, o: &HashMap<String, Value>) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    match fmt {
        ImageFormat::Jpeg => {
            let q = match o.get("quality") {
                Some(Value::Int(q)) if (1..=100).contains(q) => *q as u8,
                Some(other) => return Err(format!("image: 'quality' must be an int in 1..100, got {}", other.to_display_string())),
                None => 85,
            };
            // JPEG has no alpha: flatten over `background` (white)
            let bg = o.get("background").map(color).transpose()?.unwrap_or(Rgba([255, 255, 255, 255]));
            let rgb = flatten(img, bg);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q)
                .encode_image(&rgb)
                .map_err(|e| io_err!("image: {}", e))?;
        }
        _ => DynamicImage::ImageRgba8(img.clone()).write_to(&mut out, fmt).map_err(|e| io_err!("image: {}", e))?,
    }
    Ok(out.into_inner())
}

fn flatten(img: &RgbaImage, bg: Rgba<u8>) -> image::RgbImage {
    image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let a = p[3] as u32;
        let mix = |c: u8, b: u8| ((c as u32 * a + b as u32 * (255 - a)) / 255) as u8;
        image::Rgb([mix(p[0], bg.0[0]), mix(p[1], bg.0[1]), mix(p[2], bg.0[2])])
    })
}

// ================= THE IMAGE OBJECT =================

pub struct Image {
    pub img: RgbaImage,
}

/// Stroke/fill options shared by the shape methods: `{ fill, stroke, width }`
/// (`color` is an alias of `fill`; with neither, the shape is filled black).
fn shape_paints(o: &HashMap<String, Value>) -> Result<(Option<Rgba<u8>>, Option<(Rgba<u8>, f32)>), String> {
    let fill = o.get("fill").or_else(|| o.get("color")).map(color).transpose()?;
    let stroke = match o.get("stroke") {
        Some(c) => Some((color(c)?, o.get("width").map(|w| num(w, "width")).transpose()?.unwrap_or(1.0))),
        None => None,
    };
    if fill.is_none() && stroke.is_none() {
        return Ok((Some(Rgba([0, 0, 0, 255])), None));
    }
    Ok((fill, stroke))
}

impl Image {
    fn draw_path(&mut self, path: tiny_skia::Path, o: &HashMap<String, Value>) -> Result<(), String> {
        let (fill, stroke) = shape_paints(o)?;
        let mut pm = to_pixmap(&self.img);
        if let Some(c) = fill {
            pm.fill_path(&path, &paint(c), tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
        if let Some((c, w)) = stroke {
            let s = Stroke { width: w, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
            pm.stroke_path(&path, &paint(c), &s, Transform::identity(), None);
        }
        self.img = from_pixmap(&pm);
        Ok(())
    }

    fn text(&mut self, args: &[Value]) -> Result<Value, String> {
        let text = str_arg(args, 0, "text")?;
        let (x, y) = (num_at(args, 1, "text")?, num_at(args, 2, "text")?);
        let o = opts(opt_arg(args, 3), "text")?;
        let size = o.get("size").map(|v| num(v, "text size")).transpose()?.unwrap_or(16.0);
        let c = o.get("color").map(color).transpose()?.unwrap_or(Rgba([0, 0, 0, 255]));
        let spec = match o.get("font") {
            Some(Value::Str(s)) => s.clone(),
            _ => "sans-serif".into(),
        };
        let f = font(&spec, o.get("bold").is_some_and(Value::is_truthy))?;
        let sf = f.as_scaled(PxScale::from(size));
        let line_h = sf.height() + sf.line_gap();
        let align = match o.get("align") {
            Some(Value::Str(s)) => s.clone(),
            _ => "left".into(),
        };
        // (x, y) is the top-left of the text box; lines are split on "\n"
        for (i, line) in text.split('\n').enumerate() {
            let width = line_width(&sf, line);
            let mut caret = match align.as_str() {
                "center" => x - width / 2.0,
                "right" => x - width,
                _ => x,
            };
            let base = y + sf.ascent() + i as f32 * line_h;
            let mut prev = None;
            for ch in line.chars() {
                let id = sf.glyph_id(ch);
                if let Some(p) = prev {
                    caret += sf.kern(p, id);
                }
                let g = id.with_scale_and_position(sf.scale(), ab_glyph::point(caret, base));
                caret += sf.h_advance(id);
                prev = Some(id);
                let Some(og) = f.outline_glyph(g) else { continue };
                let bb = og.px_bounds();
                let (w, h) = (self.img.width() as i32, self.img.height() as i32);
                og.draw(|gx, gy, cov| {
                    let (px, py) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && px < w && py < h {
                        blend(self.img.get_pixel_mut(px as u32, py as u32), c, cov);
                    }
                });
            }
        }
        Ok(Value::Nil)
    }
}

fn line_width<F: Font, S: ScaleFont<F>>(sf: &S, line: &str) -> f32 {
    let mut w = 0.0;
    let mut prev = None;
    for ch in line.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

/// Source-over blend of `c` with coverage `cov` onto `dst` (straight alpha).
fn blend(dst: &mut Rgba<u8>, c: Rgba<u8>, cov: f32) {
    let sa = (c.0[3] as f32 / 255.0) * cov.clamp(0.0, 1.0);
    let da = dst.0[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return;
    }
    for i in 0..3 {
        let v = (c.0[i] as f32 * sa + dst.0[i] as f32 * da * (1.0 - sa)) / oa;
        dst.0[i] = v.round() as u8;
    }
    dst.0[3] = (oa * 255.0).round() as u8;
}

fn adjust(img: &RgbaImage, f: impl Fn(f32) -> f32) -> RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        for i in 0..3 {
            p.0[i] = f(p.0[i] as f32).clamp(0.0, 255.0).round() as u8;
        }
    }
    out
}

impl FoguObject for Image {
    fn name(&self) -> &str {
        "Image"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "width" => Value::Int(self.img.width() as i64),
            "height" => Value::Int(self.img.height() as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        let (w, h) = self.img.dimensions();
        let new = |img: RgbaImage| Ok(wrap(img));
        match method {
            // ---------- transformations (return a new image) ----------
            "clone" => new(self.img.clone()),
            // resize(width, height?, { fit = "fill" | "contain" | "cover", filter })
            "resize" => {
                max_args(a, 3, "resize")?;
                let o = opts(opt_arg(a, 2), "resize")?;
                let filter = match o.get("filter").map(|v| v.to_display_string()).as_deref() {
                    None | Some("lanczos") => FilterType::Lanczos3,
                    Some("nearest") => FilterType::Nearest,
                    Some("linear") | Some("triangle") => FilterType::Triangle,
                    Some("cubic") => FilterType::CatmullRom,
                    Some("gaussian") => FilterType::Gaussian,
                    Some(other) => return Err(format!("resize: unknown filter '{}' (nearest linear cubic gaussian lanczos)", other)),
                };
                let tw = match opt_arg(a, 0) { Some(v) => Some(dim(v, "resize")?), None => None };
                let th = match opt_arg(a, 1) { Some(v) => Some(dim(v, "resize")?), None => None };
                let (tw, th) = match (tw, th) {
                    (Some(x), Some(y)) => (x, y),
                    (Some(x), None) => (x, ((h as f64 * x as f64 / w as f64).round() as u32).max(1)),
                    (None, Some(y)) => (((w as f64 * y as f64 / h as f64).round() as u32).max(1), y),
                    (None, None) => return Err("resize: give a width, a height or both".into()),
                };
                let fit = match o.get("fit") { Some(Value::Str(s)) => s.clone(), _ => "fill".into() };
                let dynimg = DynamicImage::ImageRgba8(self.img.clone());
                new(match fit.as_str() {
                    "fill" => image::imageops::resize(&self.img, tw, th, filter),
                    "contain" => dynimg.resize(tw, th, filter).to_rgba8(),
                    "cover" => dynimg.resize_to_fill(tw, th, filter).to_rgba8(),
                    other => return Err(format!("resize: unknown fit '{}' (fill contain cover)", other)),
                })
            }
            "thumbnail" => {
                max_args(a, 2, "thumbnail")?;
                let tw = dim(a.first().unwrap_or(&Value::Nil), "thumbnail")?;
                let th = match opt_arg(a, 1) { Some(v) => dim(v, "thumbnail")?, None => tw };
                new(DynamicImage::ImageRgba8(self.img.clone()).thumbnail(tw, th).to_rgba8())
            }
            "crop" => {
                max_args(a, 4, "crop")?;
                let (x, y) = (num_at(a, 0, "crop")?.max(0.0) as u32, num_at(a, 1, "crop")?.max(0.0) as u32);
                let (cw, ch) = (dim(a.get(2).unwrap_or(&Value::Nil), "crop")?, dim(a.get(3).unwrap_or(&Value::Nil), "crop")?);
                if x >= w || y >= h {
                    return Err(format!("crop: ({}, {}) is outside the {}x{} image", x, y, w, h));
                }
                new(image::imageops::crop_imm(&self.img, x, y, cw.min(w - x), ch.min(h - y)).to_image())
            }
            // rotate(degrees, { background }) — clockwise; any angle (the canvas grows to fit)
            "rotate" => {
                max_args(a, 2, "rotate")?;
                let deg = num_at(a, 0, "rotate")?.rem_euclid(360.0);
                let o = opts(opt_arg(a, 1), "rotate")?;
                match deg {
                    d if d == 0.0 => new(self.img.clone()),
                    d if d == 90.0 => new(image::imageops::rotate90(&self.img)),
                    d if d == 180.0 => new(image::imageops::rotate180(&self.img)),
                    d if d == 270.0 => new(image::imageops::rotate270(&self.img)),
                    d => {
                        let r = d.to_radians();
                        let (s, c) = (r.sin().abs(), r.cos().abs());
                        let nw = (w as f32 * c + h as f32 * s).ceil() as u32;
                        let nh = (w as f32 * s + h as f32 * c).ceil() as u32;
                        let mut pm = Pixmap::new(nw, nh).expect("non-empty");
                        if let Some(bg) = o.get("background") {
                            let b = color(bg)?;
                            pm.fill(tiny_skia::Color::from_rgba8(b.0[0], b.0[1], b.0[2], b.0[3]));
                        }
                        let src = to_pixmap(&self.img);
                        let t = Transform::from_translate(nw as f32 / 2.0, nh as f32 / 2.0)
                            .pre_rotate(d)
                            .pre_translate(-(w as f32) / 2.0, -(h as f32) / 2.0);
                        let pp = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
                        pm.draw_pixmap(0, 0, src.as_ref(), &pp, t, None);
                        new(from_pixmap(&pm))
                    }
                }
            }
            "flipH" => new(image::imageops::flip_horizontal(&self.img)),
            "flipV" => new(image::imageops::flip_vertical(&self.img)),
            "grayscale" => {
                let g = DynamicImage::ImageRgba8(self.img.clone()).grayscale().to_rgba8();
                new(g)
            }
            "invert" => {
                let mut img = self.img.clone();
                image::imageops::invert(&mut img);
                new(img)
            }
            "blur" => {
                max_args(a, 1, "blur")?;
                let sigma = match a.first() { Some(v) => num(v, "blur")?, None => 2.0 };
                new(image::imageops::fast_blur(&self.img, sigma.max(0.1)))
            }
            "sharpen" => {
                max_args(a, 2, "sharpen")?;
                let sigma = match a.first() { Some(v) => num(v, "sharpen")?, None => 1.0 };
                let th = match a.get(1) { Some(v) => num(v, "sharpen")? as i32, None => 2 };
                new(image::imageops::unsharpen(&self.img, sigma, th))
            }
            // brightness(-255..255), contrast(-100..100), hue(degrees), saturate(factor), opacity(0..1)
            "brightness" => {
                let d = num_at(a, 0, "brightness")?;
                new(adjust(&self.img, |v| v + d))
            }
            "contrast" => {
                let c = num_at(a, 0, "contrast")?;
                let f = ((100.0 + c) / 100.0).powi(2);
                new(adjust(&self.img, |v| ((v / 255.0 - 0.5) * f + 0.5) * 255.0))
            }
            "hue" => new(image::imageops::huerotate(&self.img, num_at(a, 0, "hue")? as i32)),
            "saturate" => {
                let f = num_at(a, 0, "saturate")?;
                let mut img = self.img.clone();
                for p in img.pixels_mut() {
                    let [r, g, b, _] = p.0;
                    let l = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
                    for i in 0..3 {
                        p.0[i] = (l + (p.0[i] as f32 - l) * f).clamp(0.0, 255.0).round() as u8;
                    }
                }
                new(img)
            }
            "opacity" => {
                let f = num_at(a, 0, "opacity")?.clamp(0.0, 1.0);
                let mut img = self.img.clone();
                img.pixels_mut().for_each(|p| p.0[3] = (p.0[3] as f32 * f).round() as u8);
                new(img)
            }
            // ---------- drawing (in place) ----------
            "fill" => {
                max_args(a, 1, "fill")?;
                let c = color(a.first().unwrap_or(&Value::Nil))?;
                self.img.pixels_mut().for_each(|p| *p = c);
                Ok(Value::Nil)
            }
            "getPixel" => {
                let (x, y) = (num_at(a, 0, "getPixel")? as i64, num_at(a, 1, "getPixel")? as i64);
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    return Err(format!("getPixel: ({}, {}) is outside the {}x{} image", x, y, w, h));
                }
                Ok(Value::Str(color_hex(*self.img.get_pixel(x as u32, y as u32)).into()))
            }
            "setPixel" => {
                let (x, y) = (num_at(a, 0, "setPixel")? as i64, num_at(a, 1, "setPixel")? as i64);
                let c = color(a.get(2).unwrap_or(&Value::Nil))?;
                if x >= 0 && y >= 0 && x < w as i64 && y < h as i64 {
                    self.img.put_pixel(x as u32, y as u32, c);
                }
                Ok(Value::Nil)
            }
            // rect(x, y, w, h, { fill, stroke, width, radius })
            "rect" => {
                max_args(a, 5, "rect")?;
                let (x, y, rw, rh) = (num_at(a, 0, "rect")?, num_at(a, 1, "rect")?, num_at(a, 2, "rect")?, num_at(a, 3, "rect")?);
                let o = opts(opt_arg(a, 4), "rect")?;
                let r = o.get("radius").map(|v| num(v, "radius")).transpose()?.unwrap_or(0.0).min(rw / 2.0).min(rh / 2.0);
                let mut pb = PathBuilder::new();
                if r > 0.0 {
                    pb.move_to(x + r, y);
                    pb.line_to(x + rw - r, y);
                    pb.quad_to(x + rw, y, x + rw, y + r);
                    pb.line_to(x + rw, y + rh - r);
                    pb.quad_to(x + rw, y + rh, x + rw - r, y + rh);
                    pb.line_to(x + r, y + rh);
                    pb.quad_to(x, y + rh, x, y + rh - r);
                    pb.line_to(x, y + r);
                    pb.quad_to(x, y, x + r, y);
                    pb.close();
                } else if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, rw, rh) {
                    pb.push_rect(rect);
                }
                if let Some(p) = pb.finish() {
                    self.draw_path(p, &o)?;
                }
                Ok(Value::Nil)
            }
            // circle(cx, cy, r, opts) · ellipse(cx, cy, rx, ry, opts)
            "circle" | "ellipse" => {
                let (cx, cy, rx) = (num_at(a, 0, method)?, num_at(a, 1, method)?, num_at(a, 2, method)?);
                let (ry, oi) = if method == "ellipse" { (num_at(a, 3, method)?, 4) } else { (rx, 3) };
                let o = opts(opt_arg(a, oi), method)?;
                let mut pb = PathBuilder::new();
                if let Some(r) = tiny_skia::Rect::from_xywh(cx - rx, cy - ry, rx * 2.0, ry * 2.0) {
                    pb.push_oval(r);
                }
                if let Some(p) = pb.finish() {
                    self.draw_path(p, &o)?;
                }
                Ok(Value::Nil)
            }
            // line(x1, y1, x2, y2, { color, width })
            "line" => {
                max_args(a, 5, "line")?;
                let mut o = opts(opt_arg(a, 4), "line")?;
                let c = o.remove("color").or_else(|| o.remove("stroke")).unwrap_or(Value::Str("black".into()));
                o.remove("fill");
                o.insert("stroke".into(), c);
                let mut pb = PathBuilder::new();
                pb.move_to(num_at(a, 0, "line")?, num_at(a, 1, "line")?);
                pb.line_to(num_at(a, 2, "line")?, num_at(a, 3, "line")?);
                if let Some(p) = pb.finish() {
                    self.draw_path(p, &o)?;
                }
                Ok(Value::Nil)
            }
            // polygon([[x, y], ...], { fill, stroke, width, closed = true })
            "polygon" => {
                max_args(a, 2, "polygon")?;
                let Some(Value::List(pts)) = a.first() else {
                    return Err("polygon: expects a list of [x, y] points".into());
                };
                let mut o = opts(opt_arg(a, 1), "polygon")?;
                let closed = o.remove("closed").is_none_or(|v| v.is_truthy());
                let mut pb = PathBuilder::new();
                for (i, p) in pts.borrow().iter().enumerate() {
                    let Value::List(xy) = p else { return Err("polygon: each point must be [x, y]".into()) };
                    let xy = xy.borrow();
                    let (x, y) = (num_at(&xy, 0, "polygon")?, num_at(&xy, 1, "polygon")?);
                    if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
                }
                if closed {
                    pb.close();
                }
                if let Some(p) = pb.finish() {
                    self.draw_path(p, &o)?;
                }
                Ok(Value::Nil)
            }
            // text(str, x, y, { size = 16, color, font, bold, align })
            "text" => {
                max_args(a, 4, "text")?;
                self.text(a)
            }
            // measureText(str, { size, font, bold }) → { width, height }
            "measureText" => {
                max_args(a, 2, "measureText")?;
                let text = str_arg(a, 0, "measureText")?;
                let o = opts(opt_arg(a, 1), "measureText")?;
                let size = o.get("size").map(|v| num(v, "size")).transpose()?.unwrap_or(16.0);
                let spec = match o.get("font") { Some(Value::Str(s)) => s.clone(), _ => "sans-serif".into() };
                let f = font(&spec, o.get("bold").is_some_and(Value::is_truthy))?;
                let sf = f.as_scaled(PxScale::from(size));
                let lines: Vec<&str> = text.split('\n').collect();
                let width = lines.iter().map(|l| line_width(&sf, l)).fold(0.0, f32::max);
                let height = sf.height() * lines.len() as f32 + sf.line_gap() * (lines.len() - 1) as f32;
                Ok(crate::native::util::map(vec![("width", Value::Int(width.ceil() as i64)), ("height", Value::Int(height.ceil() as i64))]))
            }
            // paste(other, x, y, { opacity }) — alpha-blended
            "paste" => {
                max_args(a, 4, "paste")?;
                let (x, y) = (num_at(a, 1, "paste")? as i64, num_at(a, 2, "paste")? as i64);
                let o = opts(opt_arg(a, 3), "paste")?;
                let mut top = with_image(a.first().unwrap_or(&Value::Nil), "paste", |i| i.clone())?;
                if let Some(op) = o.get("opacity") {
                    let f = num(op, "opacity")?.clamp(0.0, 1.0);
                    top.pixels_mut().for_each(|p| p.0[3] = (p.0[3] as f32 * f).round() as u8);
                }
                image::imageops::overlay(&mut self.img, &top, x, y);
                Ok(Value::Nil)
            }
            // drawSvg(svg, x, y, { width, height, scale })
            "drawSvg" => {
                max_args(a, 4, "drawSvg")?;
                let svg = str_arg(a, 0, "drawSvg")?;
                let (x, y) = (num_at(a, 1, "drawSvg")? as i64, num_at(a, 2, "drawSvg")? as i64);
                let top = render_svg(&svg, &opts(opt_arg(a, 3), "drawSvg")?)?;
                image::imageops::overlay(&mut self.img, &top, x, y);
                Ok(Value::Nil)
            }
            // ---------- output ----------
            // save(path, { quality, background }) — the format comes from the extension
            "save" => {
                max_args(a, 2, "save")?;
                let path = str_arg(a, 0, "save")?;
                let data = encode(&self.img, format_of(&path)?, &opts(opt_arg(a, 1), "save")?)?;
                if let Some(dir) = std::path::Path::new(&path).parent().filter(|d| !d.as_os_str().is_empty()) {
                    let _ = std::fs::create_dir_all(dir);
                }
                std::fs::write(&path, data).map_err(|e| io_err!("image.save: {}: {}", path_str(std::path::Path::new(&path)), e))?;
                Ok(Value::Nil)
            }
            // encode(format = "png", opts) → bytes · toBase64(format) · toDataUrl(format)
            "encode" | "toBase64" | "toDataUrl" => {
                max_args(a, 2, method)?;
                let fmt_name = match opt_arg(a, 0) { Some(v) => str_arg(std::slice::from_ref(v), 0, method)?, None => "png".into() };
                let fmt = format_of(&fmt_name)?;
                let data = encode(&self.img, fmt, &opts(opt_arg(a, 1), method)?)?;
                Ok(match method {
                    "encode" => bytes_value(data),
                    "toBase64" => Value::Str(base64::engine::general_purpose::STANDARD.encode(data).into()),
                    _ => Value::Str(format!("data:{};base64,{}", fmt.to_mime_type(), base64::engine::general_purpose::STANDARD.encode(data)).into()),
                })
            }
            other => Err(format!("method '{}' does not exist on Image ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "clone", "resize", "thumbnail", "crop", "rotate", "flipH", "flipV", "grayscale", "invert", "blur", "sharpen",
            "brightness", "contrast", "hue", "saturate", "opacity", "fill", "getPixel", "setPixel", "rect", "circle",
            "ellipse", "line", "polygon", "text", "measureText", "paste", "drawSvg", "save", "encode", "toBase64", "toDataUrl",
        ]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
