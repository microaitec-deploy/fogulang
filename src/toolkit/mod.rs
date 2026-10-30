//! Fogu toolkit — CLI (Bun-style), REPL and package manager.

pub mod build;
pub mod cli;
pub mod commands;
pub mod diag;
pub mod fmt;
pub mod frameworks;
#[cfg(not(target_family = "wasm"))]
pub mod repl;
pub mod service;
pub mod testing;
pub mod ui;
pub mod wasm_embed;

use std::env;
use std::path::Path;
use std::process;

use crate::runtime::jit::{self, JitConfig};
use crate::runtime::{sched, MAIN_STACK};

/// Stack of the WebAssembly build (`-z stack-size` in .cargo/config.toml).
#[cfg(target_family = "wasm")]
const WASM_STACK: usize = 64 * 1024 * 1024;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn help() -> String {
    format!(
        "{title} {ver} — scripting language with a JIT compiler (Cranelift)

{usage}
  fogu <file.fg> [args...]         run a file (shortcut for 'run')
  fogu <script> [args...]          run a script from fogu.json

{cmds}
  run <file|script>       run a file or a script from fogu.json
  repl                    open the interactive REPL (history, multi-line)
  init [-y]               create fogu.json, main.fg and .gitignore here
  add <package...>        add dependencies and install them
  remove <package...>     remove dependencies              (alias: rm)
  install                 install what fogu.json declares   (alias: i)
  update [package...]     update to the latest commits      (alias: up)
  list                    list the installed dependencies   (alias: ls)
  pm <path|ls|cache|hash> utilities for the global package store
  check [files...]        check syntax and names without running
  build --compile [file]  build a standalone executable in dist/ (--target linux-arm64,windows-x64,…|all; --targets)
  test [paths] [-t text]  run the tests of *.test.fg files
  fmt [paths] [--check]   format .fg files (- for stdin)
  play <playbook.yml>     run a playbook (-i inventory, --check, -l, -t, -e)
  desktop <create|dev|build|update>  desktop apps (Tauri + Vite); see fogu desktop help
  mcu <boards|build|flash|run|monitor|…>  microcontroller firmware; see fogu mcu help
  help | version

{opts}
  -e, --eval <code>       run code passed on the command line
  -p, --print <expr>      evaluate the expression and print the result
  -w, --watch             restart the program when a file changes
      --no-jit            disable the JIT compiler
      --jit-stats         show JIT statistics at the end
      --jit-debug         log to stderr what the JIT compiles/rejects
      --jit-dump          like --jit-debug, and print the IR (CLIF)
      --strict            an error in an 'fg' task terminates the process
      --cores <n|cpus>    threads the runtime uses (4), or the CPUs to run on (0,2-3)
  -v, --version           show the version
  -h, --help              show this help

{pk}
  fogu add github:user/repo                  (or user/repo)
  fogu add user/repo@^1.2.0                  semver version (tags v1.2.3)
  fogu add gitlab:group/repo#main            branch, tag or commit
  fogu add https://server/repo.git           any git URL
  fogu add alias@github:user/repo            use 'alias' as the import name
  fogu add ./folder  |  https://x/pkg.tar.gz  local folder or archive
  Stored in {home}/fogu_modules (global, shared between projects);
  fogu.lock pins commit + checksum. Usage: import x from \"name\"

Environment: FOGU_HOME (default ~/.fogu), NO_COLOR, FOGU_JIT=0, FOGU_JIT_CALLS, FOGU_JIT_LOOPS, FOGU_CORES",
        title = ui::bold("Fogu"),
        ver = VERSION,
        usage = ui::bold("Usage:"),
        cmds = ui::bold("Commands:"),
        opts = ui::bold("Options:"),
        pk = ui::bold("Packages (git):"),
        home = crate::pkg::home().display(),
    )
}

#[derive(Default)]
pub struct RunFlags {
    pub watch: bool,
    pub eval: Option<String>,
    pub print: Option<String>,
    pub help: bool,
    pub version: bool,
}

/// Consumes Fogu's options at the start of `args`. Stops at the first
/// argument that is not an option (file/command).
fn take_flags(args: &mut Vec<String>, cfg: &mut JitConfig, f: &mut RunFlags) -> Result<(), String> {
    while let Some(a) = args.first().cloned() {
        if !a.starts_with('-') || a == "-" {
            break;
        }
        args.remove(0);
        match a.as_str() {
            "--no-jit" => cfg.enabled = false,
            "--jit-stats" => jit::request_stats(),
            "--jit-debug" => cfg.debug = cfg.debug.max(1),
            "--jit-dump" => cfg.debug = 2,
            "--strict" => sched::set_strict(true),
            // --cores 4 (threads) · --cores 0,2-3 (pinned to those CPUs)
            "--cores" => {
                if args.is_empty() {
                    return Err("--cores expects a number or a list of CPUs (0,2-3)".into());
                }
                crate::native::rt::configure_cores(&args.remove(0))?;
            }
            "-w" | "--watch" => f.watch = true,
            "-h" | "--help" => f.help = true,
            "-v" | "-V" | "--version" => f.version = true,
            "-e" | "--eval" => {
                if args.is_empty() {
                    return Err(format!("{} expects an argument", a));
                }
                f.eval = Some(args.remove(0));
            }
            "-p" | "--print" => {
                if args.is_empty() {
                    return Err(format!("{} expects an argument", a));
                }
                f.print = Some(args.remove(0));
            }
            s => return Err(format!("unknown option: {}\n  see 'fogu --help'", s)),
        }
    }
    Ok(())
}

/// Loads `./.env` into the process environment (seen by `Fogu.env` and
/// `process.env`), like Bun. Variables already set win; `FOGU_DOTENV=0`
/// turns it off. Runs before any thread is started.
fn load_dotenv() {
    if env::var("FOGU_DOTENV").is_ok_and(|v| v == "0") {
        return;
    }
    let items = match dotenvy::from_path_iter(".env") {
        Ok(items) => items,
        Err(e) if e.not_found() => return,
        Err(e) => return ui::warn(&format!(".env: {}", e)),
    };
    // a bad line is reported and skipped; the others still load
    for item in items {
        match item {
            // SAFETY: called at startup, before any thread exists.
            Ok((k, v)) if env::var_os(&k).is_none() => unsafe { env::set_var(k, v) },
            Ok(_) => {}
            Err(e) => ui::warn(&format!(".env: {}", e)),
        }
    }
}

pub fn run() {
    crate::register();
    // OpenSSH may run `fogu` as its SSH_ASKPASS program (fogu:ssh passwords)
    #[cfg(feature = "ops")]
    if let Some(code) = crate::native::ssh::askpass_reply() {
        process::exit(code);
    }
    load_dotenv();
    // FOGU_CORES=4 (threads) or FOGU_CORES=0,2-3 (pinned to those CPUs); --cores wins
    if let Ok(spec) = env::var("FOGU_CORES") {
        if !spec.trim().is_empty() {
            if let Err(e) = crate::native::rt::configure_cores(&spec) {
                eprintln!("FOGU_CORES: {}", e);
                process::exit(2);
            }
        }
    }
    // A standalone executable (`fogu build --compile`) runs its embedded
    // program and passes every argument to it.
    if let Some(embedded) = build::embedded() {
        let code = match embedded {
            // `fogu build --service`: install / start / stop… or run as a service
            Ok(build::Embedded { entry, service: Some(spec) }) => service::main(&spec, entry, env::args().skip(1).collect()),
            Ok(e) => run_embedded(e.entry, env::args().skip(1).collect()),
            Err(e) => {
                ui::error(&e);
                1
            }
        };
        crate::native::cleanup();
        process::exit(code);
    }

    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut cfg = JitConfig::from_env();
    let mut flags = RunFlags::default();

    if let Err(e) = take_flags(&mut args, &mut cfg, &mut flags) {
        ui::error(&e);
        process::exit(2);
    }
    if flags.help {
        println!("{}", help());
        return;
    }
    if flags.version {
        println!("{}", VERSION);
        return;
    }

    // `run` also accepts options after the command.
    if args.first().map(|s| s.as_str()) == Some("run") {
        args.remove(0);
        if let Err(e) = take_flags(&mut args, &mut cfg, &mut flags) {
            ui::error(&e);
            process::exit(2);
        }
        if args.is_empty() && flags.eval.is_none() && flags.print.is_none() {
            // no target: use "entry" from fogu.json
            match commands::default_entry() {
                Some(e) => args.push(e),
                None => {
                    ui::error("usage: fogu run <file|script>   (no 'entry' in fogu.json)");
                    process::exit(1);
                }
            }
        }
        args.insert(0, "\u{1}run".into()); // marker: already a run target
    }

    // Watch mode: relaunches itself as a child process.
    if flags.watch {
        let code = commands::watch(&env::args().skip(1).collect::<Vec<_>>());
        process::exit(code);
    }

    jit::configure(cfg);

    // WebAssembly has no threads: the interpreter runs right here (the stack
    // size comes from the linker, see .cargo/config.toml)
    #[cfg(target_family = "wasm")]
    {
        sched::init_main_stack(WASM_STACK);
        let code = real_main(args, flags);
        crate::native::cleanup();
        process::exit(code);
    }

    // The interpreter is recursive: it runs on a thread with a large stack.
    #[allow(unreachable_code)]
    let handle = std::thread::Builder::new()
        .name("fogu-main".into())
        .stack_size(MAIN_STACK)
        .spawn(move || {
            sched::init_main_stack(MAIN_STACK);
            real_main(args, flags)
        })
        .expect("could not create the main thread");

    let code = handle.join().unwrap_or(101);
    crate::native::cleanup();
    process::exit(code);
}

/// Runs the program embedded in a standalone executable.
fn run_embedded(entry: std::path::PathBuf, args: Vec<String>) -> i32 {
    jit::configure(JitConfig::from_env());
    #[cfg(target_family = "wasm")]
    {
        sched::init_main_stack(WASM_STACK);
        crate::native::system::set_script_args(args);
        return finish(cli::run_file(&entry.to_string_lossy()));
    }
    #[allow(unreachable_code)]
    let handle = std::thread::Builder::new()
        .name("fogu-main".into())
        .stack_size(MAIN_STACK)
        .spawn(move || {
            sched::init_main_stack(MAIN_STACK);
            crate::native::system::set_script_args(args);
            finish(cli::run_file(&entry.to_string_lossy()))
        })
        .expect("could not create the main thread");
    handle.join().unwrap_or(101)
}

fn real_main(mut args: Vec<String>, flags: RunFlags) -> i32 {
    if let Some(code) = flags.eval {
        crate::native::system::set_script_args(args);
        return finish(crate::runtime::run_source(&code, "<eval>"));
    }
    if let Some(expr) = flags.print {
        crate::native::system::set_script_args(args);
        return finish(crate::runtime::run_source(&format!("print({})", expr), "<print>"));
    }

    if args.is_empty() {
        return start_repl();
    }

    let forced_run = args[0] == "\u{1}run";
    if forced_run {
        args.remove(0);
    }
    let cmd = args[0].clone();
    let rest = &args[1..];

    if !forced_run {
        match cmd.as_str() {
            "repl" => return start_repl(),
            "help" => {
                println!("{}", help());
                return 0;
            }
            "version" => {
                println!("{}", VERSION);
                return 0;
            }
            "init" => return commands::init(rest),
            "add" | "a" => return commands::add(rest),
            "remove" | "rm" | "uninstall" => return commands::remove(rest),
            "install" | "i" => return commands::install(rest),
            "update" | "up" | "upgrade" => return commands::update(rest),
            "list" | "ls" => return commands::list(rest),
            "pm" => return commands::pm(rest),
            "check" => return commands::check(rest),
            "build" => return build::build(rest),
            "test" => return testing::test(rest),
            "fmt" => return fmt::fmt(rest),
            "play" => return play(rest),
            "desktop" => return frameworks::desktop(rest),
            "mcu" => return frameworks::mcu(rest),
            _ => {}
        }
    }

    // A file? A script from fogu.json?
    if !Path::new(&cmd).is_file() {
        if let Some(code) = commands::try_script(&cmd, rest) {
            return code;
        }
        // a file of an installed package: `fogu desktop/build.fg <app>`
        if cmd.contains('/') && !cmd.starts_with(['.', '/']) {
            let cwd = std::env::current_dir().unwrap_or_default();
            if let Ok(Some(file)) = crate::pkg::resolve_import(&cmd, &cwd) {
                crate::native::system::set_script_args(rest.to_vec());
                return finish(cli::run_file(&file.to_string_lossy()));
            }
        }
        if cmd.ends_with(".fg") || Path::new(&cmd).extension().is_some() || cmd.contains('/') {
            ui::error(&format!("file not found: {}", cmd));
        } else {
            ui::error(&format!("unknown command or script: '{}'\n  see 'fogu --help'", cmd));
        }
        return 1;
    }

    crate::native::system::set_script_args(rest.to_vec());
    finish(cli::run_file(&cmd))
}

fn finish(r: Result<(), String>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", diag::render(&e));
            1
        }
    }
}

fn start_repl() -> i32 {
    #[cfg(not(target_family = "wasm"))]
    return repl::start();
    #[cfg(target_family = "wasm")]
    {
        ui::error("the REPL is not available on WebAssembly");
        1
    }
}

fn play(args: &[String]) -> i32 {
    #[cfg(all(feature = "ops", not(target_family = "wasm")))]
    return crate::native::playbook::play_cli(args);
    #[cfg(any(not(feature = "ops"), target_family = "wasm"))]
    {
        let _ = args;
        ui::error("fogu play is not available on WebAssembly");
        1
    }
}
