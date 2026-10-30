//! `fogu:cli` module — terminal interaction: prompts (`dialoguer`), colors
//! and terminal control (`console`), spinners and progress bars
//! (`indicatif`), tables (`comfy-table`) and argument parsing (`clap`).
//!
//! ```fogu
//! import cli from "fogu:cli"
//!
//! const opts = cli.parse({
//!     name: "deploy", version: "1.0.0", about: "Deploys the app",
//!     args: [
//!         { name: "env", positional: true, required: true, choices: ["dev", "prod"] },
//!         { name: "force", short: "f", type: "bool", help: "skip the checks" },
//!     ],
//! })
//! const name = cli.input("Your name", { default: "Ana" })?
//! if cli.confirm(`Deploy to ${opts.env}?`, { default: true })? {
//!     const sp = cli.spinner("Uploading…")
//!     time.delay(1s)
//!     sp.succeed("Uploaded")
//! }
//! print(cli.green("done"), cli.style("!", "bold", "yellow"))
//! print(cli.table([["ana", 3], ["rui", 5]], { headers: ["name", "jobs"] }))
//! ```
//!
//! Without a terminal on stdin (pipes, CI) the prompts read plain lines, so
//! scripts stay testable: `echo "Ana" | fogu app.fg`. Spinners and bars draw
//! on stderr and only on a terminal.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::io::{BufRead, IsTerminal, Write};
use std::time::Duration;

use console::{Color, Style, Term};
use dialoguer::theme::ColorfulTheme;
use indicatif::{ProgressBar, ProgressStyle};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{list, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    let mut f = |name: &str, min: usize, func: fn(&[Value]) -> Result<Value, String>| {
        e.insert(name.into(), nfn_var(name, min, func));
    };
    // prompts
    f("input", 1, input);
    f("password", 1, password);
    f("confirm", 1, confirm);
    f("select", 2, select);
    f("multiSelect", 2, multi_select);
    // styles
    f("style", 1, style_fn);
    // shortcuts: cli.red(text), cli.bold(text)…
    f("bold", 1, |a| named_style("bold", a));
    f("dim", 1, |a| named_style("dim", a));
    f("italic", 1, |a| named_style("italic", a));
    f("underline", 1, |a| named_style("underline", a));
    f("strike", 1, |a| named_style("strike", a));
    f("inverse", 1, |a| named_style("inverse", a));
    f("black", 1, |a| named_style("black", a));
    f("red", 1, |a| named_style("red", a));
    f("green", 1, |a| named_style("green", a));
    f("yellow", 1, |a| named_style("yellow", a));
    f("blue", 1, |a| named_style("blue", a));
    f("magenta", 1, |a| named_style("magenta", a));
    f("cyan", 1, |a| named_style("cyan", a));
    f("white", 1, |a| named_style("white", a));
    f("gray", 1, |a| named_style("gray", a));
    f("colors", 0, colors);
    f("stripAnsi", 1, |a| Ok(Value::Str(console::strip_ansi_codes(&str_arg(a, 0, "stripAnsi")?).into_owned().into())));
    f("textWidth", 1, |a| Ok(Value::Int(console::measure_text_width(&str_arg(a, 0, "textWidth")?) as i64)));
    f("link", 2, link);
    // terminal
    f("isTTY", 0, is_tty);
    f("size", 0, size);
    f("clear", 0, |_| term_op(|t| t.clear_screen()));
    f("clearLine", 0, |_| term_op(|t| t.clear_line()));
    f("hideCursor", 0, |_| term_op(|t| t.hide_cursor()));
    f("showCursor", 0, |_| term_op(|t| t.show_cursor()));
    f("readKey", 0, read_key);
    // spinners, bars, tables, arguments
    f("spinner", 1, spinner);
    f("progress", 1, progress);
    f("table", 1, table);
    f("parse", 1, parse);
    ModuleValue {
        name: "fogu:cli".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------

fn opts_of(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn prompt_err(e: dialoguer::Error) -> String {
    io_err!("cli: {}", e)
}

/// Plain-line prompt (no terminal): the prompt goes to stderr.
fn read_line(prompt: &str) -> Result<Option<String>, String> {
    eprint!("{} ", prompt);
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let n = std::io::stdin().lock().read_line(&mut line).map_err(|e| io_err!("cli: {}", e))?;
    if n == 0 {
        eprintln!();
        return Ok(None); // end of input
    }
    eprintln!();
    Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
}

fn eof() -> String {
    io_err!("cli: no more input (stdin was closed)")
}

/// `input(prompt, { default, allowEmpty, type: "str" | "int" | "float" })`
fn input(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "cli.input")?;
    let prompt = str_arg(args, 0, "cli.input")?;
    let o = opts_of(args, 1, "cli.input")?;
    let default = o.get("default").filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
    let allow_empty = o.get("allowEmpty").is_some_and(|v| v.is_truthy());
    let kind = match o.get("type") {
        Some(Value::Str(s)) => s.clone(),
        _ => "str".into(),
    };
    if !["str", "int", "float"].contains(&kind.as_str()) {
        return Err(format!("cli.input: type must be str, int or float, got '{}'", kind));
    }
    let convert = |s: &str| -> Result<Value, String> {
        match kind.as_str() {
            "int" => s.trim().parse::<i64>().map(Value::Int).map_err(|_| format!("'{}' is not an integer", s)),
            "float" => s.trim().parse::<f64>().map(Value::Float).map_err(|_| format!("'{}' is not a number", s)),
            _ => Ok(Value::Str(s.to_string().into())),
        }
    };
    if interactive() {
        let theme = ColorfulTheme::default();
        let mut p = dialoguer::Input::<String>::with_theme(&theme)
            .with_prompt(&prompt)
            .allow_empty(allow_empty)
            .validate_with(|s: &String| -> Result<(), String> { convert(s).map(|_| ()) });
        if let Some(d) = &default {
            p = p.default(d.clone());
        }
        let s = p.interact_text().map_err(prompt_err)?;
        return convert(&s);
    }
    let shown = match &default {
        Some(d) => format!("{} ({})", prompt, d),
        None => prompt.clone(),
    };
    let line = read_line(&format!("{}:", shown))?.ok_or_else(eof)?;
    let s = if line.is_empty() { default.unwrap_or_default() } else { line };
    if s.is_empty() && !allow_empty {
        return Err(io_err!("cli: an answer is required"));
    }
    convert(&s).map_err(|e| io_err!("cli: {}", e))
}

/// `password(prompt, { confirm: "Repeat password", allowEmpty })`
fn password(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "cli.password")?;
    let prompt = str_arg(args, 0, "cli.password")?;
    let o = opts_of(args, 1, "cli.password")?;
    let allow_empty = o.get("allowEmpty").is_some_and(|v| v.is_truthy());
    if interactive() {
        let theme = ColorfulTheme::default();
        let mut p = dialoguer::Password::with_theme(&theme).with_prompt(&prompt).allow_empty_password(allow_empty);
        if let Some(c) = o.get("confirm").filter(|v| v.is_truthy()) {
            let again = match c {
                Value::Str(s) => s.clone(),
                _ => "Repeat password".into(),
            };
            p = p.with_confirmation(again, "The passwords do not match");
        }
        return p.interact().map(Value::str).map_err(prompt_err);
    }
    let s = read_line(&format!("{}:", prompt))?.ok_or_else(eof)?;
    if s.is_empty() && !allow_empty {
        return Err(io_err!("cli: an answer is required"));
    }
    Ok(Value::Str(s.into()))
}

/// `confirm(prompt, { default })` → bool
fn confirm(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "cli.confirm")?;
    let prompt = str_arg(args, 0, "cli.confirm")?;
    let o = opts_of(args, 1, "cli.confirm")?;
    let default = o.get("default").map(|v| v.is_truthy());
    if interactive() {
        let theme = ColorfulTheme::default();
        let mut p = dialoguer::Confirm::with_theme(&theme).with_prompt(&prompt);
        if let Some(d) = default {
            p = p.default(d);
        }
        return p.interact().map(Value::Bool).map_err(prompt_err);
    }
    let hint = match default {
        Some(true) => "[Y/n]",
        Some(false) => "[y/N]",
        None => "[y/n]",
    };
    loop {
        let line = read_line(&format!("{} {}", prompt, hint))?.ok_or_else(eof)?;
        match line.trim().to_lowercase().as_str() {
            "y" | "yes" | "true" => return Ok(Value::Bool(true)),
            "n" | "no" | "false" => return Ok(Value::Bool(false)),
            "" if default.is_some() => return Ok(Value::Bool(default.unwrap_or(false))),
            _ => eprintln!("please answer y or n"),
        }
    }
}

/// Items of select/multiSelect: strings, or maps `{ label, value }`.
fn items_of(v: Option<&Value>, what: &str) -> Result<Vec<(String, Value)>, String> {
    let l = match v {
        Some(Value::List(l)) => l.borrow().clone(),
        _ => return Err(format!("{} expects a list of items", what)),
    };
    if l.is_empty() {
        return Err(format!("{}: the list of items is empty", what));
    }
    Ok(l.into_iter()
        .map(|it| match &it {
            Value::Map(m) => {
                let m = m.borrow();
                let value = m.get("value").cloned().unwrap_or(Value::Nil);
                let label = m.get("label").map(|v| v.to_display_string()).unwrap_or_else(|| value.to_display_string());
                (label, value)
            }
            other => (other.to_display_string(), other.clone()),
        })
        .collect())
}

/// An item chosen in a plain line: its label or its number (from 1).
fn pick(items: &[(String, Value)], s: &str) -> Option<usize> {
    let s = s.trim();
    items.iter().position(|(l, _)| l == s).or_else(|| s.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= items.len()).map(|n| n - 1))
}

fn default_index(items: &[(String, Value)], d: Option<&Value>) -> Option<usize> {
    match d? {
        Value::Int(n) if *n >= 0 && (*n as usize) < items.len() => Some(*n as usize),
        v => {
            let want = v.to_display_string();
            items.iter().position(|(l, val)| l == &want || val.to_display_string() == want)
        }
    }
}

/// `select(prompt, items, { default, fuzzy, index })` → the chosen value
/// (its index with `index: true`); nil if cancelled with Esc.
fn select(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "cli.select")?;
    let prompt = str_arg(args, 0, "cli.select")?;
    let items = items_of(args.get(1), "cli.select")?;
    let o = opts_of(args, 2, "cli.select")?;
    let default = default_index(&items, o.get("default"));
    let want_index = o.get("index").is_some_and(|v| v.is_truthy());
    let labels: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    let chosen = if interactive() {
        let theme = ColorfulTheme::default();
        if o.get("fuzzy").is_some_and(|v| v.is_truthy()) {
            let mut p = dialoguer::FuzzySelect::with_theme(&theme).with_prompt(&prompt).items(&labels);
            if let Some(d) = default {
                p = p.default(d);
            }
            p.interact_opt().map_err(prompt_err)?
        } else {
            let mut p = dialoguer::Select::with_theme(&theme).with_prompt(&prompt).items(&labels);
            p = p.default(default.unwrap_or(0));
            p.interact_opt().map_err(prompt_err)?
        }
    } else {
        eprintln!("{}", prompt);
        for (i, l) in labels.iter().enumerate() {
            eprintln!("  {}) {}", i + 1, l);
        }
        loop {
            let line = read_line(">")?.ok_or_else(eof)?;
            if line.trim().is_empty() {
                if let Some(d) = default {
                    break Some(d);
                }
            }
            match pick(&items, &line) {
                Some(i) => break Some(i),
                None => eprintln!("choose one of the items (name or number)"),
            }
        }
    };
    Ok(match chosen {
        None => Value::Nil,
        Some(i) if want_index => Value::Int(i as i64),
        Some(i) => items[i].1.clone(),
    })
}

/// `multiSelect(prompt, items, { defaults: [...], index })` → list
fn multi_select(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "cli.multiSelect")?;
    let prompt = str_arg(args, 0, "cli.multiSelect")?;
    let items = items_of(args.get(1), "cli.multiSelect")?;
    let o = opts_of(args, 2, "cli.multiSelect")?;
    let mut checked = vec![false; items.len()];
    if let Some(Value::List(d)) = o.get("defaults") {
        for v in d.borrow().iter() {
            if let Some(i) = default_index(&items, Some(v)) {
                checked[i] = true;
            }
        }
    }
    let want_index = o.get("index").is_some_and(|v| v.is_truthy());
    let labels: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    let chosen: Option<Vec<usize>> = if interactive() {
        let theme = ColorfulTheme::default();
        dialoguer::MultiSelect::with_theme(&theme)
            .with_prompt(&prompt)
            .items(&labels)
            .defaults(&checked)
            .interact_opt()
            .map_err(prompt_err)?
    } else {
        eprintln!("{} (comma separated)", prompt);
        for (i, l) in labels.iter().enumerate() {
            eprintln!("  {}) {}", i + 1, l);
        }
        loop {
            let line = read_line(">")?.ok_or_else(eof)?;
            if line.trim().is_empty() {
                break Some((0..items.len()).filter(|i| checked[*i]).collect());
            }
            let picked: Option<Vec<usize>> = line.split(',').map(|p| pick(&items, p)).collect();
            match picked {
                Some(v) => break Some(v),
                None => eprintln!("choose items by name or number, separated by commas"),
            }
        }
    };
    Ok(match chosen {
        None => Value::Nil,
        Some(ix) => list(ix.into_iter().map(|i| if want_index { Value::Int(i as i64) } else { items[i].1.clone() }).collect()),
    })
}

// ---------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------

fn color_of(name: &str) -> Option<Color> {
    Some(match name {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        _ => return None,
    })
}

/// Applies one style name: `bold dim italic underline strike inverse blink`,
/// a color (`red`, `brightRed`, `gray`), a background (`bgRed`) or a 256-color
/// number (`"208"`, `"bg208"`).
fn apply(st: Style, name: &str) -> Result<Style, String> {
    let lower = name.to_lowercase();
    Ok(match lower.as_str() {
        "bold" => st.bold(),
        "dim" => st.dim(),
        "italic" => st.italic(),
        "underline" | "underlined" => st.underlined(),
        "strike" | "strikethrough" => st.strikethrough(),
        "inverse" | "reverse" => st.reverse(),
        "blink" => st.blink(),
        "hidden" => st.hidden(),
        "gray" | "grey" => st.black().bright(),
        s => {
            if let Some(c) = color_of(s) {
                st.fg(c)
            } else if let Some(c) = s.strip_prefix("bright").and_then(color_of) {
                st.fg(c).bright()
            } else if let Some(c) = s.strip_prefix("bg").and_then(color_of) {
                st.bg(c)
            } else if let Ok(n) = s.parse::<u8>() {
                st.color256(n)
            } else if let Some(n) = s.strip_prefix("bg").and_then(|n| n.parse::<u8>().ok()) {
                st.on_color256(n)
            } else {
                return Err(format!("cli.style: unknown style '{}'", name));
            }
        }
    })
}

/// `style(text, ...styles)` — styles may also come as one list.
fn style_fn(args: &[Value]) -> Result<Value, String> {
    let text = args.first().map(|v| v.to_display_string()).unwrap_or_default();
    let mut st = Style::new();
    for a in &args[1..] {
        match a {
            Value::List(l) => {
                for s in l.borrow().iter() {
                    st = apply(st, &s.to_display_string())?;
                }
            }
            v => st = apply(st, &v.to_display_string())?,
        }
    }
    Ok(Value::Str(st.apply_to(text).to_string().into()))
}

fn colors(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "cli.colors")?;
    if let Some(v) = args.first() {
        console::set_colors_enabled(v.is_truthy());
        console::set_colors_enabled_stderr(v.is_truthy());
    }
    Ok(Value::Bool(console::colors_enabled()))
}

/// `link(text, url)` — a clickable link (OSC 8) on terminals that support it.
fn link(args: &[Value]) -> Result<Value, String> {
    let text = str_arg(args, 0, "cli.link")?;
    let url = str_arg(args, 1, "cli.link")?;
    if !console::colors_enabled() {
        return Ok(Value::Str(format!("{} ({})", text, url).into()));
    }
    Ok(Value::Str(format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, text).into()))
}

// ---------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------

/// `isTTY(stream = "stdout")` — `"stdin"`, `"stdout"` or `"stderr"`.
fn is_tty(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "cli.isTTY")?;
    let which = if args.is_empty() { "stdout".to_string() } else { str_arg(args, 0, "cli.isTTY")? };
    Ok(Value::Bool(match which.as_str() {
        "stdin" => std::io::stdin().is_terminal(),
        "stdout" => std::io::stdout().is_terminal(),
        "stderr" => std::io::stderr().is_terminal(),
        other => return Err(format!("cli.isTTY: stream must be stdin, stdout or stderr, got '{}'", other)),
    }))
}

/// `size()` → `{ columns, rows }`, nil without a terminal.
fn size(_args: &[Value]) -> Result<Value, String> {
    Ok(match Term::stdout().size_checked() {
        Some((rows, cols)) => crate::native::util::map(vec![("columns", Value::Int(cols as i64)), ("rows", Value::Int(rows as i64))]),
        None => Value::Nil,
    })
}

fn term_op(f: impl FnOnce(&Term) -> std::io::Result<()>) -> Result<Value, String> {
    let t = Term::stdout();
    if t.is_term() {
        f(&t).map_err(|e| io_err!("cli: {}", e))?;
    }
    Ok(Value::Nil)
}

/// `readKey()` → a character or a key name (`Enter Escape Backspace Tab
/// ArrowUp ArrowDown ArrowLeft ArrowRight Home End PageUp PageDown Delete`).
fn read_key(_args: &[Value]) -> Result<Value, String> {
    use console::Key;
    let t = Term::stdout();
    if !t.is_term() {
        return Err(io_err!("cli.readKey: not a terminal"));
    }
    let k = t.read_key().map_err(|e| io_err!("cli: {}", e))?;
    Ok(Value::Str(match k {
        Key::Char(c) => c.to_string().into(),
        Key::Enter => "Enter".into(),
        Key::Escape => "Escape".into(),
        Key::Backspace => "Backspace".into(),
        Key::Tab => "Tab".into(),
        Key::BackTab => "BackTab".into(),
        Key::ArrowUp => "ArrowUp".into(),
        Key::ArrowDown => "ArrowDown".into(),
        Key::ArrowLeft => "ArrowLeft".into(),
        Key::ArrowRight => "ArrowRight".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::Del => "Delete".into(),
        Key::Insert => "Insert".into(),
        other => format!("{:?}", other).into(),
    }))
}

// ---------------------------------------------------------------------
// Spinners and progress bars
// ---------------------------------------------------------------------

/// A spinner or a progress bar (both draw on stderr, only on a terminal).
pub struct BarValue {
    bar: ProgressBar,
    spinner: bool,
    done: bool,
}

/// Prints the final line of a spinner: `✔ text`.
fn finish_line(b: &mut BarValue, symbol: &str, color: Color, text: Option<String>) {
    let text = text.unwrap_or_else(|| b.bar.message());
    b.bar.finish_and_clear();
    b.done = true;
    let sym = Style::new().fg(color).for_stderr().apply_to(symbol);
    eprintln!("{} {}", sym, text);
}

/// `spinner(text, { frames })`
fn spinner(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "cli.spinner")?;
    let text = str_arg(args, 0, "cli.spinner")?;
    let o = opts_of(args, 1, "cli.spinner")?;
    let bar = ProgressBar::new_spinner();
    let mut style = ProgressStyle::with_template("{spinner:.cyan} {msg}").map_err(|e| e.to_string())?;
    let frames: Vec<String> = match o.get("frames") {
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        _ => ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", " "].iter().map(|s| s.to_string()).collect(),
    };
    if frames.len() >= 2 {
        let refs: Vec<&str> = frames.iter().map(String::as_str).collect();
        style = style.tick_strings(&refs);
    }
    bar.set_style(style);
    bar.set_message(text);
    bar.enable_steady_tick(Duration::from_millis(80));
    Ok(NativeObjectRef::new(BarValue { bar, spinner: true, done: false }).to_value())
}

/// `progress(total, { message, template, chars, prefix })`
fn progress(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "cli.progress")?;
    let total = match args.first() {
        Some(Value::Int(n)) if *n >= 0 => *n as u64,
        _ => return Err("cli.progress expects the total as a non-negative int".into()),
    };
    let o = opts_of(args, 1, "cli.progress")?;
    let template = match o.get("template") {
        Some(Value::Str(t)) => t.clone(),
        _ => "{prefix}[{bar:30.cyan/blue}] {pos}/{len} {percent}% {msg} ({eta})".into(),
    };
    let mut style = ProgressStyle::with_template(&template).map_err(|e| format!("cli.progress: template: {}", e))?;
    let chars = match o.get("chars") {
        Some(Value::Str(c)) => c.clone(),
        _ => "█▉▊▋▌▍▎▏ ".into(),
    };
    if chars.chars().count() >= 2 {
        style = style.progress_chars(&chars);
    }
    let bar = ProgressBar::new(total);
    bar.set_style(style);
    if let Some(Value::Str(m)) = o.get("message") {
        bar.set_message(m.to_string());
    }
    if let Some(Value::Str(p)) = o.get("prefix") {
        bar.set_prefix(format!("{} ", p));
    }
    Ok(NativeObjectRef::new(BarValue { bar, spinner: false, done: false }).to_value())
}

impl Drop for BarValue {
    fn drop(&mut self) {
        if !self.done {
            self.bar.finish_and_clear();
        }
    }
}

impl FoguObject for BarValue {
    fn name(&self) -> &str {
        if self.spinner {
            "Spinner"
        } else {
            "ProgressBar"
        }
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "position" => Value::Int(self.bar.position() as i64),
            "length" => self.bar.length().map_or(Value::Nil, |n| Value::Int(n as i64)),
            "message" | "text" => Value::Str(self.bar.message().into()),
            "elapsed" => Value::Duration(self.bar.elapsed().as_nanos() as i64),
            "finished" => Value::Bool(self.done || self.bar.is_finished()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let text = |i: usize| args.get(i).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
        match method {
            "text" | "message" => {
                self.bar.set_message(text(0).unwrap_or_default());
            }
            "inc" => {
                let n = match args.first() {
                    Some(Value::Int(n)) if *n >= 0 => *n as u64,
                    None => 1,
                    _ => return Err("inc expects a non-negative int".into()),
                };
                self.bar.inc(n);
            }
            "set" => match args.first() {
                Some(Value::Int(n)) if *n >= 0 => self.bar.set_position(*n as u64),
                _ => return Err("set expects a non-negative int".into()),
            },
            "length" => match args.first() {
                Some(Value::Int(n)) if *n >= 0 => self.bar.set_length(*n as u64),
                _ => return Err("length expects a non-negative int".into()),
            },
            // a line above the bar, without breaking it
            "println" => {
                let line = text(0).unwrap_or_default();
                if self.bar.is_hidden() {
                    println!("{}", line);
                } else {
                    self.bar.println(line);
                }
            }
            "succeed" => finish_line(self, "✔", Color::Green, text(0)),
            "fail" => finish_line(self, "✖", Color::Red, text(0)),
            "warn" => finish_line(self, "⚠", Color::Yellow, text(0)),
            "info" => finish_line(self, "ℹ", Color::Blue, text(0)),
            "finish" => {
                self.done = true;
                match text(0) {
                    Some(m) if !self.spinner => self.bar.finish_with_message(m),
                    Some(m) => finish_line(self, "✔", Color::Green, Some(m)),
                    None => self.bar.finish(),
                }
            }
            "stop" | "clear" => {
                self.done = true;
                self.bar.finish_and_clear();
            }
            other => {
                return Err(format!(
                    "method '{}' does not exist on {} (text inc set length println succeed fail warn info finish stop)",
                    other,
                    self.name()
                ))
            }
        }
        Ok(Value::Nil)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["text", "message", "inc", "set", "length", "println", "succeed", "fail", "warn", "info", "finish", "stop", "clear"]
    }
}

// ---------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------

/// `table(rows, { headers, style: "rounded" | "utf8" | "ascii" | "markdown" | "none", align })`
/// → string. Rows are lists, or maps (columns = the headers, or their sorted keys).
fn table(args: &[Value]) -> Result<Value, String> {
    use comfy_table::{presets, CellAlignment, ContentArrangement, Table};
    max_args(args, 2, "cli.table")?;
    let rows = match args.first() {
        Some(Value::List(l)) => l.borrow().clone(),
        _ => return Err("cli.table expects a list of rows".into()),
    };
    let o = opts_of(args, 1, "cli.table")?;
    let mut headers: Option<Vec<String>> = match o.get("headers") {
        Some(Value::List(h)) => Some(h.borrow().iter().map(|v| v.to_display_string()).collect()),
        _ => None,
    };
    if headers.is_none() {
        if let Some(Value::Map(m)) = rows.first() {
            let mut keys: Vec<String> = m.borrow().keys().cloned().collect();
            keys.sort();
            headers = Some(keys);
        }
    }
    let mut t = Table::new();
    let preset = match o.get("style").map(|v| v.to_display_string()).as_deref() {
        None | Some("rounded") => presets::UTF8_FULL_CONDENSED,
        Some("utf8") => presets::UTF8_FULL,
        Some("ascii") => presets::ASCII_FULL_CONDENSED,
        Some("markdown") => presets::ASCII_MARKDOWN,
        Some("none") => presets::NOTHING,
        Some(other) => return Err(format!("cli.table: style must be rounded, utf8, ascii, markdown or none, got '{}'", other)),
    };
    t.load_preset(preset);
    if matches!(o.get("style"), None) {
        t.apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
    }
    t.set_content_arrangement(ContentArrangement::Dynamic);
    if let Some(Value::Int(w)) = o.get("width") {
        t.set_width((*w).clamp(10, u16::MAX as i64) as u16);
    }
    if let Some(h) = &headers {
        t.set_header(h.clone());
    }
    let cell = |v: &Value| match v {
        Value::Nil => String::new(),
        v => v.to_display_string(),
    };
    for r in &rows {
        let cells: Vec<String> = match r {
            Value::List(l) => l.borrow().iter().map(cell).collect(),
            Value::Map(m) => {
                let m = m.borrow();
                headers.as_ref().map(|h| h.iter().map(|k| m.get(k).map(cell).unwrap_or_default()).collect()).unwrap_or_default()
            }
            other => vec![cell(other)],
        };
        t.add_row(cells);
    }
    // numbers right aligned (unless `align` says otherwise per column)
    let aligns: Vec<String> = match o.get("align") {
        Some(Value::List(a)) => a.borrow().iter().map(|v| v.to_display_string()).collect(),
        _ => Vec::new(),
    };
    let ncols = t.column_count();
    for i in 0..ncols {
        let numeric = !rows.is_empty()
            && rows.iter().all(|r| {
                let v = match r {
                    Value::List(l) => l.borrow().get(i).cloned(),
                    Value::Map(m) => headers.as_ref().and_then(|h| h.get(i)).and_then(|k| m.borrow().get(k).cloned()),
                    _ => None,
                };
                matches!(v, Some(Value::Int(_) | Value::Float(_)) | None)
            });
        let a = match aligns.get(i).map(String::as_str) {
            Some("right") => CellAlignment::Right,
            Some("center") => CellAlignment::Center,
            Some("left") => CellAlignment::Left,
            _ if numeric => CellAlignment::Right,
            _ => CellAlignment::Left,
        };
        if let Some(c) = t.column_mut(i) {
            c.set_cell_alignment(a);
        }
    }
    Ok(Value::Str(t.to_string().into()))
}

// ---------------------------------------------------------------------
// Arguments (clap)
// ---------------------------------------------------------------------

#[derive(Clone)]
struct ArgSpec {
    name: String,
    kind: String,
    multiple: bool,
}

fn s_of(m: &HashMap<String, Value>, k: &str) -> Option<String> {
    match m.get(k) {
        Some(Value::Str(s)) => Some(s.to_string()),
        _ => None,
    }
}

/// Builds a clap command from `{ name, version, about, args: [...], commands: [...] }`.
fn command_of(spec: &HashMap<String, Value>, fallback: &str) -> Result<(clap::Command, Vec<ArgSpec>), String> {
    use clap::{Arg, ArgAction, Command};
    let name = s_of(spec, "name").unwrap_or_else(|| fallback.to_string());
    let mut cmd = Command::new(name);
    if let Some(v) = s_of(spec, "version") {
        cmd = cmd.version(v);
    }
    if let Some(a) = s_of(spec, "about") {
        cmd = cmd.about(a);
    }
    let mut specs = Vec::new();
    let args = match spec.get("args") {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::List(l)) => l.borrow().clone(),
        Some(other) => return Err(format!("cli.parse: args must be a list of maps, got '{}'", other.type_name())),
    };
    for (i, a) in args.iter().enumerate() {
        let a = match a {
            Value::Map(m) => m.borrow().clone(),
            _ => return Err("cli.parse: each arg is a map { name, ... }".into()),
        };
        let name = s_of(&a, "name").ok_or("cli.parse: an arg needs a 'name'")?;
        let kind = s_of(&a, "type").unwrap_or_else(|| "str".into());
        let multiple = a.get("multiple").is_some_and(|v| v.is_truthy());
        let positional = a.get("positional").is_some_and(|v| v.is_truthy());
        let mut arg = Arg::new(name.clone());
        if positional {
            arg = arg.index(i + 1);
        } else {
            arg = arg.long(s_of(&a, "long").unwrap_or_else(|| name.clone()));
            if let Some(s) = s_of(&a, "short") {
                arg = arg.short(s.chars().next().ok_or("cli.parse: empty 'short'")?);
            }
        }
        if let Some(h) = s_of(&a, "help") {
            arg = arg.help(h);
        }
        if let Some(e) = s_of(&a, "env") {
            arg = arg.env(e);
        }
        if a.get("required").is_some_and(|v| v.is_truthy()) {
            arg = arg.required(true);
        }
        arg = match kind.as_str() {
            "bool" => arg.action(ArgAction::SetTrue),
            "count" => arg.action(ArgAction::Count),
            "int" => arg.value_parser(clap::value_parser!(i64)),
            "float" => arg.value_parser(clap::value_parser!(f64)),
            "str" => arg,
            other => return Err(format!("cli.parse: type of '{}' must be str, int, float, bool or count, got '{}'", name, other)),
        };
        if multiple && !matches!(kind.as_str(), "bool" | "count") {
            arg = arg.action(ArgAction::Append).num_args(if positional { clap::builder::ValueRange::new(1..) } else { clap::builder::ValueRange::new(1) });
        }
        if let Some(Value::List(c)) = a.get("choices") {
            let c: Vec<String> = c.borrow().iter().map(|v| v.to_display_string()).collect();
            arg = arg.value_parser(clap::builder::PossibleValuesParser::new(c));
        }
        match a.get("default") {
            None | Some(Value::Nil) => {}
            Some(Value::List(d)) => {
                let d: Vec<String> = d.borrow().iter().map(|v| v.to_display_string()).collect();
                arg = arg.default_values(d);
            }
            Some(d) => arg = arg.default_value(d.to_display_string()),
        }
        cmd = cmd.arg(arg);
        specs.push(ArgSpec { name, kind, multiple });
    }
    match spec.get("commands") {
        None | Some(Value::Nil) => {}
        Some(Value::List(l)) => {
            for c in l.borrow().iter() {
                let c = match c {
                    Value::Map(m) => m.borrow().clone(),
                    _ => return Err("cli.parse: each command is a map { name, about, args }".into()),
                };
                s_of(&c, "name").ok_or("cli.parse: a command needs a 'name'")?;
                let (sub, _) = command_of(&c, "")?;
                cmd = cmd.subcommand(sub);
            }
            if spec.get("commandRequired").is_some_and(|v| v.is_truthy()) {
                cmd = cmd.subcommand_required(true).arg_required_else_help(true);
            }
        }
        Some(other) => return Err(format!("cli.parse: commands must be a list, got '{}'", other.type_name())),
    }
    Ok((cmd, specs))
}

fn matches_value(spec: &HashMap<String, Value>, m: &clap::ArgMatches, fallback: &str) -> Result<Value, String> {
    let (_, specs) = command_of(spec, fallback)?;
    let mut out: Vec<(String, Value)> = Vec::new();
    for a in &specs {
        let id = a.name.as_str();
        let v = match a.kind.as_str() {
            "bool" => Value::Bool(m.get_flag(id)),
            "count" => Value::Int(m.get_count(id) as i64),
            kind if a.multiple => {
                let items: Vec<Value> = match kind {
                    "int" => m.get_many::<i64>(id).map(|v| v.map(|n| Value::Int(*n)).collect()).unwrap_or_default(),
                    "float" => m.get_many::<f64>(id).map(|v| v.map(|n| Value::Float(*n)).collect()).unwrap_or_default(),
                    _ => m.get_many::<String>(id).map(|v| v.map(|s| Value::Str(s.clone().into())).collect()).unwrap_or_default(),
                };
                list(items)
            }
            "int" => m.get_one::<i64>(id).map_or(Value::Nil, |n| Value::Int(*n)),
            "float" => m.get_one::<f64>(id).map_or(Value::Nil, |n| Value::Float(*n)),
            _ => m.get_one::<String>(id).map_or(Value::Nil, |s| Value::Str(s.clone().into())),
        };
        out.push((a.name.clone(), v));
    }
    if let Some((name, sub)) = m.subcommand() {
        let sub_spec = match spec.get("commands") {
            Some(Value::List(l)) => l.borrow().iter().find_map(|c| match c {
                Value::Map(cm) if s_of(&cm.borrow(), "name").as_deref() == Some(name) => Some(cm.borrow().clone()),
                _ => None,
            }),
            _ => None,
        }
        .unwrap_or_default();
        out.push(("command".into(), Value::Str(name.to_string().into())));
        out.push(("args".into(), matches_value(&sub_spec, sub, name)?));
    } else if spec.contains_key("commands") {
        out.push(("command".into(), Value::Nil));
    }
    let mut map = HashMap::new();
    for (k, v) in out {
        map.insert(k, v);
    }
    Ok(Value::Map(std::rc::Rc::new(RefCell::new(map))))
}

/// `parse(spec, argv = Fogu.args, { exit = true })` → map of the values.
/// `--help`/`--version` print and exit; a usage error prints and exits
/// with 2, or comes back as an `error` value with `{ exit: false }`.
fn parse(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "cli.parse")?;
    let spec = opts_of(args, 0, "cli.parse")?;
    let argv: Vec<String> = match args.get(1) {
        None | Some(Value::Nil) => crate::native::system::script_args(),
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        Some(other) => return Err(format!("cli.parse: argv must be a list, got '{}'", other.type_name())),
    };
    let exit = !matches!(args.get(2), Some(Value::Map(m)) if m.borrow().get("exit").is_some_and(|v| !v.is_truthy()));
    let (cmd, _) = command_of(&spec, "app")?;
    let name = cmd.get_name().to_string();
    let full = std::iter::once(name.clone()).chain(argv);
    match cmd.try_get_matches_from(full) {
        Ok(m) => matches_value(&spec, &m, &name),
        Err(e) => {
            use clap::error::ErrorKind;
            let _ = std::io::stdout().flush();
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                let _ = e.print();
                std::process::exit(0);
            }
            if exit {
                let _ = e.print();
                std::process::exit(2);
            }
            Err(io_err!("{}", console::strip_ansi_codes(&e.render().to_string()).trim_end()))
        }
    }
}

/// `cli.red(text)`, `cli.bold(text)`…
fn named_style(name: &str, args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, name)?;
    let text = args.first().map(|v| v.to_display_string()).unwrap_or_default();
    Ok(Value::Str(apply(Style::new(), name)?.apply_to(text).to_string().into()))
}
