//! `fogu desktop …` and `fogu mcu …`: front-ends for the desktop and mcu
//! libraries (libs/desktop, libs/mcu). The library is looked up like an import
//! (the project's dependency, a libs/ checkout, the global list); when it is
//! missing it is installed globally from the Fogu server (`fogu add -g <name>`).

use std::path::{Path, PathBuf};

use super::{cli, commands, ui};
use crate::pkg;

const DESKTOP_HELP: &str = "fogu desktop — desktop apps: a Tauri window whose backend is Fogu

  fogu desktop create <dir> [--template vanilla|ts|react|vue|svelte] [--install]
  fogu desktop dev [dir] [--port N] [--target windows|native] [--no-watch] [--rebuild-host]
                                         Vite dev server + the window, live reload
  fogu desktop build [dir] [--target windows|<triple>] [--debug] [--installer] [--rebuild-host]
                                         the executable in <dir>/dist/
  fogu desktop update                    install the latest desktop library
  fogu desktop path                      where the library is

  [dir] defaults to the current folder.";

const MCU_HELP: &str = "fogu mcu — microcontroller firmware in Fogu
  (STM32, Raspberry Pi Pico, Arduino Uno/Nano, ESP32-C3, ESP32)

  fogu mcu boards                        board profiles and chips
  fogu mcu ports                         serial ports and boot drives
  fogu mcu build <app.fg> [--board B] [--console usb|uart] [-o out]
  fogu mcu flash <file|app.fg> [--board B] [--port COM5] [--drive E:]
  fogu mcu run <app.fg> [--board B] [--port COM3]   build + flash + monitor
  fogu mcu monitor [--port COM3] [--seconds N]
  fogu mcu detect [--board B] [--port COM5]
  fogu mcu rust <app.fg> · inspect <file>
  fogu mcu update                        install the latest mcu library
  fogu mcu path                          where the library and the tool are

  Every command of the fogu-mcu tool works; `fogu mcu help` shows all its options.";

/// The folder of the library `name`, installing it globally when missing.
fn library(name: &str, install: bool) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some((_, dir)) = pkg::package_dir(name, &cwd) {
        return Ok(dir);
    }
    if !install {
        return Err(format!("the {} library is not installed (fogu add -g {})", name, name));
    }
    let server = pkg::registry::server_url().unwrap_or_default();
    eprintln!("{}", ui::dim(&format!("installing the {} library from {}…", name, server)));
    if commands::add(&["-g".to_string(), name.to_string()]) != 0 {
        return Err(format!("could not install the {} library", name));
    }
    pkg::package_dir(name, &cwd).map(|(_, d)| d).ok_or_else(|| format!("the {} library was installed but is not found", name))
}

fn update(name: &str) -> i32 {
    commands::add(&["-g".to_string(), name.to_string()])
}

fn fail(msg: &str) -> i32 {
    ui::error(msg);
    1
}

pub fn desktop(args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str).unwrap_or("help");
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };
    let script = match sub {
        "create" | "new" | "init" => "create.fg",
        "dev" => "dev.fg",
        "build" => "build.fg",
        "update" | "upgrade" => return update("desktop"),
        "path" | "where" => {
            return match library("desktop", false) {
                Ok(d) => {
                    println!("{}", d.display());
                    0
                }
                Err(e) => fail(&e),
            };
        }
        "help" | "-h" | "--help" => {
            println!("{}", DESKTOP_HELP);
            return 0;
        }
        other => return fail(&format!("unknown desktop command '{}'\n\n{}", other, DESKTOP_HELP)),
    };
    let dir = match library("desktop", true) {
        Ok(d) => d,
        Err(e) => return fail(&e),
    };
    let file = dir.join(script);
    if !file.is_file() {
        return fail(&format!("{} has no {} (update it: fogu desktop update)", dir.display(), script));
    }
    // dev and build work on the current folder by default
    let mut argv = rest.to_vec();
    if script != "create.fg" && !argv.iter().any(|a| !a.starts_with('-') && !is_flag_value(&argv, a)) {
        argv.insert(0, ".".into());
    }
    crate::native::system::set_script_args(argv);
    // the scripts run `fogu build --compile` with this very executable
    if let Ok(exe) = std::env::current_exe() {
        // SAFETY: set before the program runs; nothing reads the environment concurrently yet
        unsafe { std::env::set_var("FOGU_BIN", exe) };
    }
    match cli::run_file(&file.to_string_lossy()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", super::diag::render(&e));
            1
        }
    }
}

/// Whether `a` is the value of a flag that takes one (`--target windows`).
fn is_flag_value(argv: &[String], a: &str) -> bool {
    const WITH_VALUE: &[&str] = &["--target", "--port", "--host", "--features", "--template", "--title", "--identifier"];
    argv.windows(2).any(|w| WITH_VALUE.contains(&w[0].as_str()) && w[1] == a)
}

/// The fogu-mcu executable: prebuilt in the package's bin/, or built in a checkout.
fn mcu_tool(dir: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "fogu-mcu.exe" } else { "fogu-mcu" };
    [dir.join("bin").join(exe), dir.join("tool").join("target").join("release").join(exe)]
        .into_iter()
        .find(|p| p.is_file())
}

pub fn mcu(args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str).unwrap_or("help");
    match sub {
        "update" | "upgrade" => return update("mcu"),
        "-h" | "--help" if args.len() == 1 => {
            println!("{}", MCU_HELP);
            return 0;
        }
        _ => {}
    }
    if args.is_empty() {
        println!("{}", MCU_HELP);
        return 0;
    }
    let dir = match library("mcu", true) {
        Ok(d) => d,
        Err(e) => return fail(&e),
    };
    let Some(tool) = mcu_tool(&dir) else {
        return fail(&format!(
            "the mcu library in {} has no fogu-mcu for this system\n  build it: cd {}/tool && cargo build --release",
            dir.display(),
            dir.display()
        ));
    };
    if matches!(sub, "path" | "where") {
        println!("library {}\ntool    {}", dir.display(), tool.display());
        return 0;
    }
    run_tool(&tool, args)
}

#[cfg(not(target_family = "wasm"))]
fn run_tool(tool: &Path, args: &[String]) -> i32 {
    match std::process::Command::new(tool).args(args).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => fail(&format!("cannot run {}: {}", tool.display(), e)),
    }
}

#[cfg(target_family = "wasm")]
fn run_tool(_tool: &Path, _args: &[String]) -> i32 {
    fail("fogu mcu is not available on WebAssembly")
}
