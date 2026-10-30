//! Colored terminal output (off when not a TTY or with NO_COLOR).

use std::io::IsTerminal;
use std::sync::OnceLock;

fn on(is_err: bool) -> bool {
    static OUT: OnceLock<bool> = OnceLock::new();
    static ERR: OnceLock<bool> = OnceLock::new();
    let cell = if is_err { &ERR } else { &OUT };
    *cell.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").map_or(true, |t| t != "dumb")
            && if is_err { std::io::stderr().is_terminal() } else { std::io::stdout().is_terminal() }
    })
}

fn paint(code: &str, s: &str, is_err: bool) -> String {
    if on(is_err) {
        format!("\x1b[{}m{}\x1b[0m", code, s)
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String { paint("1", s, false) }
pub fn dim(s: &str) -> String { paint("2", s, false) }
pub fn green(s: &str) -> String { paint("32", s, false) }
pub fn yellow(s: &str) -> String { paint("33", s, false) }
pub fn red(s: &str) -> String { paint("31", s, false) }
pub fn cyan(s: &str) -> String { paint("36", s, false) }
pub fn red_err(s: &str) -> String { paint("31;1", s, true) }
pub fn yellow_err(s: &str) -> String { paint("33", s, true) }
pub fn bold_err(s: &str) -> String { paint("1", s, true) }
pub fn cyan_err(s: &str) -> String { paint("36", s, true) }

pub fn error(msg: &str) {
    eprintln!("{} {}", red_err("error:"), msg);
}

pub fn warn(msg: &str) {
    eprintln!("{} {}", yellow_err("warning:"), msg);
}

pub fn human_size(n: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{} B", n) } else { format!("{:.1} {}", v, U[i]) }
}
