//! `fogu:shell` module — a small POSIX-style shell, in the spirit of Bun
//! Shell: pipes, `&&`/`||`/`;`, redirections, `$VAR`, `$(…)`, quotes, globs
//! and `~`, with the usual Linux commands built in (so scripts behave the
//! same on Windows). Other commands run as system programs.
//!
//! ```fogu
//! import shell from "fogu:shell"
//!
//! const r = shell.run("ls src | grep .rs | wc -l")?   // { ok, code, stdout, stderr }
//! print(shell.text("cat notes.txt | sort -u | head -n 3")?)
//! for f in shell.lines("ls *.fg")? { print(f) }
//! shell.exec("cargo build && echo done")?             // output goes to the terminal
//! shell.run("grep \"$NAME\" users.txt", { env: { NAME: name } })   // no injection
//! ```
//!
//! Built-in commands: `echo cat ls pwd cd mkdir rm cp mv touch grep head tail
//! wc sort uniq cut tr tee basename dirname which env export true false exit
//! sleep seq`. A non-zero exit code is not an error for `run`/`exec` (check
//! `ok`); `text`, `lines` and `json` turn it into an `error` value. A command
//! line that cannot be parsed stops the program.

use std::cell::RefCell;
use std::collections::BTreeMap;
use ahash::AHashMap as HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::util::{map, max_args, nfn_var, path_str, str_arg};

thread_local! {
    /// Working directory of `fogu:shell` (`shell.cd`), separate from the process'.
    static CWD: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

fn shell_cwd() -> PathBuf {
    CWD.with(|c| c.borrow().clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("run".into(), nfn_var("run", 1, |a| run_value(a, "run", Mode::Capture)));
    e.insert("exec".into(), nfn_var("exec", 1, |a| run_value(a, "exec", Mode::Inherit)));
    e.insert("text".into(), nfn_var("text", 1, |a| {
        let out = checked(a, "text")?;
        Ok(Value::Str(out.trim_end_matches(['\n', '\r']).to_string().into()))
    }));
    e.insert("lines".into(), nfn_var("lines", 1, |a| {
        let out = checked(a, "lines")?;
        Ok(crate::native::util::str_list(out.lines().filter(|l| !l.trim().is_empty()).map(String::from)))
    }));
    e.insert("json".into(), nfn_var("json", 1, |a| {
        let out = checked(a, "json")?;
        crate::native::json::parse_value(&out)
    }));
    e.insert("quote".into(), nfn_var("quote", 1, |a| {
        max_args(a, 1, "quote")?;
        Ok(Value::Str(quote(&a[0].to_display_string()).into()))
    }));
    e.insert("cd".into(), nfn_var("cd", 1, |a| {
        max_args(a, 1, "cd")?;
        let dir = resolve(&shell_cwd(), &str_arg(a, 0, "cd")?);
        if !dir.is_dir() {
            return Err(io_err!("shell.cd: {}: no such directory", path_str(&dir)));
        }
        let dir = dir.canonicalize().map(clean_unc).unwrap_or(dir);
        CWD.with(|c| *c.borrow_mut() = Some(dir));
        Ok(Value::Nil)
    }));
    e.insert("cwd".into(), nfn_var("cwd", 0, |a| {
        max_args(a, 0, "cwd")?;
        Ok(Value::Str(path_str(&shell_cwd()).into()))
    }));
    e.insert("which".into(), nfn_var("which", 1, |a| {
        max_args(a, 1, "which")?;
        Ok(find_program(&str_arg(a, 0, "which")?).map_or(Value::Nil, |p| Value::Str(path_str(&p).into())))
    }));
    e.insert("builtins".into(), crate::native::util::str_list(BUILTINS.iter().map(|s| s.to_string())));
    ModuleValue {
        name: "fogu:shell".into(),
        exports: RefCell::new(e),
    }
}

const BUILTINS: &[&str] = &[
    "echo", "cat", "ls", "pwd", "cd", "mkdir", "rm", "cp", "mv", "touch", "grep", "head", "tail", "wc", "sort",
    "uniq", "cut", "tr", "tee", "basename", "dirname", "which", "env", "export", "true", "false", "exit", "sleep",
    "seq",
];

/// `'…'` quoting for safe interpolation.
fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn clean_unc(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

// ---------------------------------------------------------------------
// Lexer / parser
// ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Part {
    /// Literal text; `true` when quoted (no glob, no tilde).
    Lit(String, bool),
    /// `$NAME` / `${NAME}`
    Var(String),
    /// `$(…)`
    Sub(String),
    /// `*` `?` from unquoted text
    Glob(String),
}

type Word = Vec<Part>;

#[derive(Clone, Debug, PartialEq)]
enum Redir {
    Out(Word, bool),  // > / >>
    Err(Word, bool),  // 2> / 2>>
    All(Word),        // &>
    In(Word),         // <
    ErrToOut,         // 2>&1
}

#[derive(Clone, Debug, Default)]
struct Cmd {
    assigns: Vec<(String, Word)>,
    words: Vec<Word>,
    redirs: Vec<Redir>,
}

#[derive(Clone, Debug, PartialEq)]
enum Op {
    And,
    Or,
    Seq,
}

#[derive(Debug)]
struct Script {
    /// pipelines joined by `&&`, `||`, `;` (the op *before* each pipeline)
    items: Vec<(Op, Vec<Cmd>)>,
}

#[derive(Debug, PartialEq)]
enum Tok {
    Word(Word),
    Pipe,
    And,
    Or,
    Semi,
    Redir(String),
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut toks = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch == ' ' || ch == '\t' || ch == '\r' {
            i += 1;
            continue;
        }
        if ch == '\n' || ch == ';' {
            toks.push(Tok::Semi);
            i += 1;
            continue;
        }
        if ch == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        let four: String = c[i..(i + 4).min(c.len())].iter().collect();
        if two == "&&" {
            toks.push(Tok::And);
            i += 2;
            continue;
        }
        if two == "||" {
            toks.push(Tok::Or);
            i += 2;
            continue;
        }
        if ch == '|' {
            toks.push(Tok::Pipe);
            i += 1;
            continue;
        }
        if four == "2>&1" {
            toks.push(Tok::Redir("2>&1".into()));
            i += 4;
            continue;
        }
        if let Some(r) = ["2>>", "2>", "&>", ">>", ">", "<"]
            .iter()
            .find(|r| c[i..(i + r.len()).min(c.len())].iter().collect::<String>() == **r)
        {
            toks.push(Tok::Redir(r.to_string()));
            i += r.len();
            continue;
        }
        if ch == '&' {
            return Err("shell: background jobs ('&') are not supported".into());
        }
        // a word
        let mut word: Word = Vec::new();
        let mut lit = String::new();
        let flush = |lit: &mut String, word: &mut Word, quoted: bool| {
            if !lit.is_empty() {
                word.push(Part::Lit(std::mem::take(lit), quoted));
            }
        };
        while i < c.len() && !" \t\r\n;|&<>".contains(c[i]) {
            match c[i] {
                '\'' => {
                    flush(&mut lit, &mut word, false);
                    let start = i + 1;
                    i = start;
                    while i < c.len() && c[i] != '\'' {
                        i += 1;
                    }
                    if i >= c.len() {
                        return Err("shell: unterminated single quote".into());
                    }
                    word.push(Part::Lit(c[start..i].iter().collect(), true));
                    i += 1;
                }
                '"' => {
                    flush(&mut lit, &mut word, false);
                    i += 1;
                    let mut q = String::new();
                    // an empty "" still makes a word
                    word.push(Part::Lit(String::new(), true));
                    loop {
                        if i >= c.len() {
                            return Err("shell: unterminated double quote".into());
                        }
                        match c[i] {
                            '"' => {
                                i += 1;
                                break;
                            }
                            '\\' if i + 1 < c.len() && "\"\\$`".contains(c[i + 1]) => {
                                q.push(c[i + 1]);
                                i += 2;
                            }
                            '$' => {
                                if !q.is_empty() {
                                    word.push(Part::Lit(std::mem::take(&mut q), true));
                                }
                                let (p, n) = dollar(&c, i)?;
                                word.push(p);
                                i = n;
                            }
                            other => {
                                q.push(other);
                                i += 1;
                            }
                        }
                    }
                    if !q.is_empty() {
                        word.push(Part::Lit(q, true));
                    }
                }
                '\\' if i + 1 < c.len() => {
                    lit.push(c[i + 1]);
                    i += 2;
                }
                '$' => {
                    flush(&mut lit, &mut word, false);
                    let (p, n) = dollar(&c, i)?;
                    word.push(p);
                    i = n;
                }
                '*' | '?' => {
                    flush(&mut lit, &mut word, false);
                    word.push(Part::Glob(c[i].to_string()));
                    i += 1;
                }
                other => {
                    lit.push(other);
                    i += 1;
                }
            }
        }
        flush(&mut lit, &mut word, false);
        toks.push(Tok::Word(word));
    }
    Ok(toks)
}

/// `$NAME`, `${NAME}`, `$?`, `$(…)` at `c[i] == '$'`.
fn dollar(c: &[char], i: usize) -> Result<(Part, usize), String> {
    let mut j = i + 1;
    match c.get(j) {
        Some('(') => {
            let mut depth = 1;
            j += 1;
            let start = j;
            let mut quote: Option<char> = None;
            while j < c.len() {
                match (quote, c[j]) {
                    (None, '\'' | '"') => quote = Some(c[j]),
                    (Some(q), ch) if ch == q => quote = None,
                    (None, '(') => depth += 1,
                    (None, ')') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if j >= c.len() {
                return Err("shell: unterminated $(".into());
            }
            Ok((Part::Sub(c[start..j].iter().collect()), j + 1))
        }
        Some('{') => {
            let start = j + 1;
            while j < c.len() && c[j] != '}' {
                j += 1;
            }
            if j >= c.len() {
                return Err("shell: unterminated ${".into());
            }
            Ok((Part::Var(c[start..j].iter().collect()), j + 1))
        }
        Some('?') => Ok((Part::Var("?".into()), j + 1)),
        Some(ch) if ch.is_ascii_alphabetic() || *ch == '_' => {
            let start = j;
            while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            Ok((Part::Var(c[start..j].iter().collect()), j))
        }
        _ => Ok((Part::Lit("$".into(), true), j)),
    }
}

fn parse(src: &str) -> Result<Script, String> {
    let toks = lex(src)?;
    let mut items: Vec<(Op, Vec<Cmd>)> = Vec::new();
    let mut pipeline: Vec<Cmd> = Vec::new();
    let mut cmd = Cmd::default();
    let mut op = Op::Seq;
    let mut it = toks.into_iter().peekable();
    let end_cmd = |cmd: &mut Cmd, pipeline: &mut Vec<Cmd>, what: &str| -> Result<(), String> {
        if cmd.words.is_empty() && cmd.assigns.is_empty() {
            if cmd.redirs.is_empty() {
                return Err(format!("shell: syntax error near '{}'", what));
            }
        }
        pipeline.push(std::mem::take(cmd));
        Ok(())
    };
    while let Some(t) = it.next() {
        match t {
            Tok::Word(w) => {
                // NAME=value before the command
                if cmd.words.is_empty() {
                    if let Some(Part::Lit(s, false)) = w.first() {
                        if let Some((name, rest)) = s.split_once('=') {
                            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                                let mut value: Word = Vec::new();
                                if !rest.is_empty() {
                                    value.push(Part::Lit(rest.to_string(), true));
                                }
                                value.extend(w[1..].iter().cloned());
                                cmd.assigns.push((name.to_string(), value));
                                continue;
                            }
                        }
                    }
                }
                cmd.words.push(w);
            }
            Tok::Redir(r) => {
                let target = if r == "2>&1" {
                    None
                } else {
                    match it.next() {
                        Some(Tok::Word(w)) => Some(w),
                        _ => return Err(format!("shell: '{}' needs a file name", r)),
                    }
                };
                cmd.redirs.push(match (r.as_str(), target) {
                    ("2>&1", _) => Redir::ErrToOut,
                    (">", Some(w)) => Redir::Out(w, false),
                    (">>", Some(w)) => Redir::Out(w, true),
                    ("2>", Some(w)) => Redir::Err(w, false),
                    ("2>>", Some(w)) => Redir::Err(w, true),
                    ("&>", Some(w)) => Redir::All(w),
                    ("<", Some(w)) => Redir::In(w),
                    _ => unreachable!(),
                });
            }
            Tok::Pipe => end_cmd(&mut cmd, &mut pipeline, "|")?,
            Tok::And | Tok::Or | Tok::Semi => {
                let sym = match t {
                    Tok::And => "&&",
                    Tok::Or => "||",
                    _ => ";",
                };
                if cmd.words.is_empty() && cmd.assigns.is_empty() && cmd.redirs.is_empty() && pipeline.is_empty() {
                    if t == Tok::Semi {
                        continue;
                    }
                    return Err(format!("shell: syntax error near '{}'", sym));
                }
                end_cmd(&mut cmd, &mut pipeline, sym)?;
                items.push((op, std::mem::take(&mut pipeline)));
                op = match t {
                    Tok::And => Op::And,
                    Tok::Or => Op::Or,
                    _ => Op::Seq,
                };
                if op != Op::Seq && it.peek().is_none() {
                    return Err(format!("shell: '{}' at the end of the command", sym));
                }
            }
        }
    }
    if !cmd.words.is_empty() || !cmd.assigns.is_empty() || !cmd.redirs.is_empty() {
        pipeline.push(cmd);
    } else if !pipeline.is_empty() {
        return Err("shell: '|' at the end of the command".into());
    }
    if !pipeline.is_empty() {
        items.push((op, pipeline));
    }
    Ok(Script { items })
}

// ---------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// stdout/stderr are captured
    Capture,
    /// the last command of each pipeline writes to the terminal
    Inherit,
}

struct Ctx {
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    last: i32,
    exited: bool,
    native: bool,
}

#[derive(Default)]
struct Out {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn resolve(cwd: &Path, p: &str) -> PathBuf {
    let p = expand_home(p);
    let pb = PathBuf::from(&p);
    if pb.is_absolute() { pb } else { cwd.join(pb) }
}

fn home() -> Option<String> {
    std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok())
}

fn expand_home(p: &str) -> String {
    match (p.strip_prefix('~'), home()) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => {
            format!("{}{}", h, rest)
        }
        _ => p.to_string(),
    }
}

/// A word → zero or more arguments (globs may expand to several).
fn expand(w: &Word, ctx: &mut Ctx) -> Result<Vec<String>, String> {
    let mut text = String::new();
    let mut pattern = String::new();
    let mut has_glob = false;
    for (i, p) in w.iter().enumerate() {
        match p {
            Part::Lit(s, quoted) => {
                let s = if i == 0 && !quoted { expand_home(s) } else { s.clone() };
                text.push_str(&s);
                // quoted glob characters must match literally
                pattern.push_str(&if *quoted { s.replace('*', "[*]").replace('?', "[?]") } else { s });
            }
            Part::Var(name) => {
                let v = if name == "?" { ctx.last.to_string() } else { ctx.env.get(name).cloned().unwrap_or_default() };
                text.push_str(&v);
                pattern.push_str(&v);
            }
            Part::Sub(src) => {
                let script = parse(src)?;
                let out = run_script(&script, ctx, Mode::Capture, Vec::new())?;
                let v = String::from_utf8_lossy(&out.stdout).trim_end_matches(['\n', '\r']).to_string();
                text.push_str(&v);
                pattern.push_str(&v);
            }
            Part::Glob(g) => {
                has_glob = true;
                text.push_str(g);
                pattern.push_str(g);
            }
        }
    }
    if has_glob && !pattern.contains("[*]") && !pattern.contains("[?]") {
        let base = path_str(&ctx.cwd);
        let abs = Path::new(&pattern).is_absolute();
        let full = if abs { pattern.clone() } else { format!("{}/{}", base.trim_end_matches('/'), pattern) };
        let found = crate::native::fs::glob(&full);
        if !found.is_empty() {
            let prefix = format!("{}/", base.trim_end_matches('/'));
            return Ok(found.into_iter().map(|f| if abs { f } else { f.strip_prefix(&prefix).map(String::from).unwrap_or(f) }).collect());
        }
    }
    Ok(vec![text])
}

fn run_script(script: &Script, ctx: &mut Ctx, mode: Mode, stdin: Vec<u8>) -> Result<Out, String> {
    let mut all = Out::default();
    for (op, pipeline) in &script.items {
        if ctx.exited {
            break;
        }
        let skip = match op {
            Op::And => ctx.last != 0,
            Op::Or => ctx.last == 0,
            Op::Seq => false,
        };
        if skip {
            continue;
        }
        let out = run_pipeline(pipeline, ctx, mode, stdin.clone())?;
        ctx.last = out.code;
        all.stdout.extend(out.stdout);
        all.stderr.extend(out.stderr);
        all.code = out.code;
    }
    Ok(all)
}

fn run_pipeline(cmds: &[Cmd], ctx: &mut Ctx, mode: Mode, stdin: Vec<u8>) -> Result<Out, String> {
    let mut input = stdin;
    let mut errs = Vec::new();
    let mut last = Out::default();
    for (i, cmd) in cmds.iter().enumerate() {
        let is_last = i + 1 == cmds.len();
        let out = run_cmd(cmd, ctx, if is_last { mode } else { Mode::Capture }, std::mem::take(&mut input))?;
        errs.extend(out.stderr);
        if is_last {
            last.stdout = out.stdout;
            last.code = out.code;
        } else {
            input = out.stdout;
        }
        if ctx.exited {
            break;
        }
    }
    last.stderr = errs;
    Ok(last)
}

fn write_file(ctx: &Ctx, w: &Word, data: &[u8], append: bool) -> Result<(), String> {
    let mut c = Ctx { cwd: ctx.cwd.clone(), env: ctx.env.clone(), last: ctx.last, exited: false, native: ctx.native };
    let name = expand(w, &mut c)?.join(" ");
    let path = resolve(&ctx.cwd, &name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&path)
        .map_err(|e| format!("{}: {}", name, e))?;
    f.write_all(data).map_err(|e| format!("{}: {}", name, e))
}

fn run_cmd(cmd: &Cmd, ctx: &mut Ctx, mode: Mode, stdin: Vec<u8>) -> Result<Out, String> {
    let mut args = Vec::new();
    for w in &cmd.words {
        args.extend(expand(w, ctx)?);
    }
    let mut assigned = Vec::new();
    for (k, w) in &cmd.assigns {
        assigned.push((k.clone(), expand(w, ctx)?.join(" ")));
    }
    // `NAME=value` alone sets a shell variable
    if args.is_empty() {
        for (k, v) in assigned {
            ctx.env.insert(k, v);
        }
        return Ok(Out::default());
    }
    let mut stdin = stdin;
    let mut err_to_out = false;
    let mut redirect_out = false;
    for r in &cmd.redirs {
        match r {
            Redir::In(w) => {
                let name = expand(w, ctx)?.join(" ");
                match std::fs::read(resolve(&ctx.cwd, &name)) {
                    Ok(d) => stdin = d,
                    Err(e) => {
                        return Ok(Out { code: 1, stdout: Vec::new(), stderr: format!("{}: {}\n", name, short(&e)).into_bytes() })
                    }
                }
            }
            Redir::ErrToOut => err_to_out = true,
            Redir::Out(..) | Redir::All(..) => redirect_out = true,
            Redir::Err(..) => {}
        }
    }
    let saved: Vec<(String, Option<String>)> =
        assigned.iter().map(|(k, v)| (k.clone(), ctx.env.insert(k.clone(), v.clone()))).collect();
    let inherit = mode == Mode::Inherit && !redirect_out && !err_to_out;
    let mut out = match exec_one(&args, ctx, stdin, inherit) {
        Ok(o) => o,
        Err(msg) => Out { code: 1, stdout: Vec::new(), stderr: format!("{}\n", msg).into_bytes() },
    };
    for (k, old) in saved {
        match old {
            Some(v) => ctx.env.insert(k, v),
            None => ctx.env.remove(&k),
        };
    }
    if err_to_out {
        let e = std::mem::take(&mut out.stderr);
        out.stdout.extend(e);
    }
    for r in &cmd.redirs {
        let res = match r {
            Redir::Out(w, append) => write_file(ctx, w, &std::mem::take(&mut out.stdout), *append),
            Redir::Err(w, append) => write_file(ctx, w, &std::mem::take(&mut out.stderr), *append),
            Redir::All(w) => {
                let mut data = std::mem::take(&mut out.stdout);
                data.extend(std::mem::take(&mut out.stderr));
                write_file(ctx, w, &data, false)
            }
            _ => Ok(()),
        };
        if let Err(e) = res {
            out.stderr.extend(format!("{}\n", e).into_bytes());
            out.code = 1;
        }
    }
    // in Inherit mode, the output of built-ins is printed directly
    if mode == Mode::Inherit {
        if !out.stdout.is_empty() {
            let _ = std::io::stdout().write_all(&std::mem::take(&mut out.stdout));
        }
        if !out.stderr.is_empty() {
            let _ = std::io::stderr().write_all(&std::mem::take(&mut out.stderr));
        }
    }
    Ok(out)
}

fn exec_one(args: &[String], ctx: &mut Ctx, stdin: Vec<u8>, inherit: bool) -> Result<Out, String> {
    let name = args[0].as_str();
    if ctx.native && BUILTINS.contains(&name) {
        return builtin(name, &args[1..], ctx, stdin);
    }
    let Some(program) = find_program_in(name, &ctx.env) else {
        return Ok(Out { code: 127, stdout: Vec::new(), stderr: format!("{}: command not found\n", name).into_bytes() });
    };
    let ext = program.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut c = if cfg!(windows) && (ext == "cmd" || ext == "bat") {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&program);
        c
    } else {
        Command::new(&program)
    };
    c.args(&args[1..]).current_dir(&ctx.cwd).env_clear().envs(&ctx.env);
    c.stdin(Stdio::piped());
    if inherit {
        c.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    // the child runs off the interpreter thread: other `fg` tasks keep going
    let label = name.to_string();
    let stdin = stdin.to_vec();
    let out = crate::native::rt::offload(move || {
        let mut child = c.spawn().map_err(|e| format!("{}: {}", label, e))?;
        let mut sin = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || {
            let _ = sin.write_all(&stdin);
        });
        let out = child.wait_with_output().map_err(|e| format!("{}: {}", label, e));
        let _ = writer.join();
        out
    })??;
    Ok(Out { code: out.status.code().unwrap_or(1), stdout: out.stdout, stderr: out.stderr })
}

fn find_program(name: &str) -> Option<PathBuf> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    find_program_in(name, &env)
}

fn find_program_in(name: &str, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = env
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("PATHEXT"))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        std::iter::once(String::new()).chain(pathext.split(';').map(|e| e.to_lowercase())).collect()
    } else {
        vec![String::new()]
    };
    let try_path = |p: PathBuf| -> Option<PathBuf> {
        for e in &exts {
            let candidate = if e.is_empty() { p.clone() } else { PathBuf::from(format!("{}{}", p.display(), e)) };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    };
    if name.contains('/') || name.contains('\\') {
        return try_path(PathBuf::from(name));
    }
    let path = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("PATH")).map(|(_, v)| v.clone()).unwrap_or_default();
    std::env::split_paths(&path).find_map(|dir| try_path(dir.join(name)))
}

// ---------------------------------------------------------------------
// Built-in commands
// ---------------------------------------------------------------------

/// Splits `-abc` style flags (and `-n 5` / `-n5` values) from operands.
struct Flags {
    set: Vec<char>,
    values: HashMap<char, String>,
    rest: Vec<String>,
}

fn flags(args: &[String], with_value: &str) -> Flags {
    let mut f = Flags { set: Vec::new(), values: HashMap::new(), rest: Vec::new() };
    let mut i = 0;
    let mut operands = false;
    while i < args.len() {
        let a = &args[i];
        if operands || !a.starts_with('-') || a == "-" || a.len() == 1 {
            f.rest.push(a.clone());
        } else if a == "--" {
            operands = true;
        } else if a[1..].chars().all(|c| c.is_ascii_digit()) {
            // head -5
            f.values.insert('n', a[1..].to_string());
        } else {
            let chars: Vec<char> = a[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let c = chars[j];
                if with_value.contains(c) {
                    let v: String = chars[j + 1..].iter().collect();
                    if v.is_empty() {
                        i += 1;
                        f.values.insert(c, args.get(i).cloned().unwrap_or_default());
                    } else {
                        f.values.insert(c, v);
                    }
                    break;
                }
                f.set.push(c);
                j += 1;
            }
        }
        i += 1;
    }
    f
}

fn ok(stdout: impl Into<Vec<u8>>) -> Result<Out, String> {
    Ok(Out { code: 0, stdout: stdout.into(), stderr: Vec::new() })
}

fn fail(code: i32, msg: String) -> Result<Out, String> {
    Ok(Out { code, stdout: Vec::new(), stderr: format!("{}\n", msg).into_bytes() })
}

/// Concatenated contents of the operands (`-` or none: stdin).
fn inputs(files: &[String], ctx: &Ctx, stdin: &[u8], cmd: &str) -> Result<Vec<u8>, String> {
    if files.is_empty() {
        return Ok(stdin.to_vec());
    }
    let mut data = Vec::new();
    for f in files {
        if f == "-" {
            data.extend_from_slice(stdin);
        } else {
            data.extend(std::fs::read(resolve(&ctx.cwd, f)).map_err(|e| format!("{}: {}: {}", cmd, f, short(&e)))?);
        }
    }
    Ok(data)
}

fn short(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".into(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".into(),
        _ => e.to_string(),
    }
}

fn text_lines(data: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(data).lines().map(String::from).collect()
}

fn join_lines(lines: &[String]) -> Vec<u8> {
    let mut s = lines.join("\n");
    if !lines.is_empty() {
        s.push('\n');
    }
    s.into_bytes()
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

fn builtin(name: &str, args: &[String], ctx: &mut Ctx, stdin: Vec<u8>) -> Result<Out, String> {
    match name {
        "true" => ok(""),
        "false" => Ok(Out { code: 1, ..Default::default() }),
        "echo" => {
            let (newline, rest) = match args.first().map(String::as_str) {
                Some("-n") => (false, &args[1..]),
                _ => (true, args),
            };
            let mut s = rest.join(" ");
            if newline {
                s.push('\n');
            }
            ok(s)
        }
        "pwd" => ok(format!("{}\n", path_str(&ctx.cwd))),
        "cd" => {
            let target = args.first().cloned().or_else(home).unwrap_or_else(|| ".".into());
            let dir = resolve(&ctx.cwd, &target);
            if !dir.is_dir() {
                return fail(1, format!("cd: {}: No such file or directory", target));
            }
            ctx.cwd = dir.canonicalize().map(clean_unc).unwrap_or(dir);
            ok("")
        }
        "export" => {
            for a in args {
                if let Some((k, v)) = a.split_once('=') {
                    ctx.env.insert(k.to_string(), v.to_string());
                }
            }
            ok("")
        }
        "env" => ok(ctx.env.iter().map(|(k, v)| format!("{}={}\n", k, v)).collect::<String>()),
        "exit" => {
            ctx.exited = true;
            let code = args.first().and_then(|a| a.parse().ok()).unwrap_or(ctx.last);
            Ok(Out { code, ..Default::default() })
        }
        "sleep" => {
            let secs: f64 = args.first().and_then(|a| a.trim_end_matches('s').parse().ok()).unwrap_or(0.0);
            // cooperative: the other `fg` tasks keep running
            crate::native::util::sleep_coop(std::time::Duration::from_secs_f64(secs.max(0.0)))?;
            ok("")
        }
        "seq" => {
            let nums: Vec<i64> = args.iter().filter_map(|a| a.parse().ok()).collect();
            let (a, step, b) = match nums.as_slice() {
                [b] => (1, 1, *b),
                [a, b] => (*a, 1, *b),
                [a, s, b] => (*a, *s, *b),
                _ => return fail(1, "seq: usage: seq [first [step]] last".into()),
            };
            if step == 0 {
                return fail(1, "seq: step cannot be 0".into());
            }
            let mut out = String::new();
            let mut n = a;
            while (step > 0 && n <= b) || (step < 0 && n >= b) {
                out.push_str(&format!("{}\n", n));
                n += step;
            }
            ok(out)
        }
        "cat" => match inputs(args, ctx, &stdin, "cat") {
            Ok(d) => ok(d),
            Err(e) => fail(1, e),
        },
        "ls" => {
            let f = flags(args, "");
            let all = f.set.contains(&'a') || f.set.contains(&'A');
            let long = f.set.contains(&'l');
            let targets = if f.rest.is_empty() { vec![".".to_string()] } else { f.rest.clone() };
            let mut out = String::new();
            let mut code = 0;
            let mut errs = String::new();
            for (n, t) in targets.iter().enumerate() {
                let p = resolve(&ctx.cwd, t);
                let line = |name: &str, path: &Path| -> String {
                    if long {
                        let md = std::fs::metadata(path).ok();
                        let kind = if path.is_dir() { "d" } else { "-" };
                        format!("{} {:>10} {}\n", kind, md.map(|m| m.len()).unwrap_or(0), name)
                    } else {
                        format!("{}\n", name)
                    }
                };
                if p.is_dir() {
                    if targets.len() > 1 {
                        out.push_str(&format!("{}{}:\n", if n > 0 { "\n" } else { "" }, t));
                    }
                    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&p)
                        .map(|rd| rd.flatten().map(|e| (e.file_name().to_string_lossy().to_string(), e.path())).collect())
                        .unwrap_or_default();
                    names.sort();
                    for (name, path) in names {
                        if all || !name.starts_with('.') {
                            out.push_str(&line(&name, &path));
                        }
                    }
                } else if p.exists() {
                    out.push_str(&line(t, &p));
                } else {
                    code = 2;
                    errs.push_str(&format!("ls: cannot access '{}': No such file or directory\n", t));
                }
            }
            Ok(Out { code, stdout: out.into_bytes(), stderr: errs.into_bytes() })
        }
        "mkdir" => {
            let f = flags(args, "m");
            for d in &f.rest {
                let p = resolve(&ctx.cwd, d);
                let r = if f.set.contains(&'p') { std::fs::create_dir_all(&p) } else { std::fs::create_dir(&p) };
                if let Err(e) = r {
                    if !(f.set.contains(&'p') && p.is_dir()) {
                        return fail(1, format!("mkdir: cannot create directory '{}': {}", d, short(&e)));
                    }
                }
            }
            ok("")
        }
        "touch" => {
            for t in args {
                let p = resolve(&ctx.cwd, t);
                let r = std::fs::OpenOptions::new().create(true).append(true).open(&p);
                if let Err(e) = r {
                    return fail(1, format!("touch: {}: {}", t, short(&e)));
                }
            }
            ok("")
        }
        "rm" => {
            let f = flags(args, "");
            let recursive = f.set.contains(&'r') || f.set.contains(&'R');
            let force = f.set.contains(&'f');
            for t in &f.rest {
                let p = resolve(&ctx.cwd, t);
                let r = if p.is_dir() {
                    if !recursive {
                        return fail(1, format!("rm: cannot remove '{}': Is a directory", t));
                    }
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_file(&p)
                };
                if let Err(e) = r {
                    if !(force && e.kind() == std::io::ErrorKind::NotFound) {
                        return fail(1, format!("rm: cannot remove '{}': {}", t, short(&e)));
                    }
                }
            }
            ok("")
        }
        "cp" | "mv" => {
            let f = flags(args, "");
            if f.rest.len() < 2 {
                return fail(1, format!("{}: missing destination", name));
            }
            let (srcs, dst) = f.rest.split_at(f.rest.len() - 1);
            let dst_path = resolve(&ctx.cwd, &dst[0]);
            if srcs.len() > 1 && !dst_path.is_dir() {
                return fail(1, format!("{}: target '{}' is not a directory", name, dst[0]));
            }
            for s in srcs {
                let sp = resolve(&ctx.cwd, s);
                let target = if dst_path.is_dir() {
                    dst_path.join(sp.file_name().unwrap_or_default())
                } else {
                    dst_path.clone()
                };
                let r = if name == "mv" {
                    std::fs::rename(&sp, &target)
                } else if sp.is_dir() {
                    if !(f.set.contains(&'r') || f.set.contains(&'R')) {
                        return fail(1, format!("cp: -r not specified; omitting directory '{}'", s));
                    }
                    copy_dir(&sp, &target)
                } else {
                    std::fs::copy(&sp, &target).map(|_| ())
                };
                if let Err(e) = r {
                    return fail(1, format!("{}: '{}': {}", name, s, short(&e)));
                }
            }
            ok("")
        }
        "grep" => {
            let f = flags(args, "eA");
            let mut rest = f.rest.clone();
            let pattern = match f.values.get(&'e') {
                Some(p) => p.clone(),
                None if !rest.is_empty() => rest.remove(0),
                None => return fail(2, "grep: usage: grep [-ivnclFE] pattern [file...]".into()),
            };
            let pat = if f.set.contains(&'F') { regex::escape(&pattern) } else { pattern.clone() };
            let pat = if f.set.contains(&'w') { format!(r"\b(?:{})\b", pat) } else { pat };
            let re = match regex::RegexBuilder::new(&pat).case_insensitive(f.set.contains(&'i')).build() {
                Ok(r) => r,
                Err(e) => return fail(2, format!("grep: invalid pattern: {}", e)),
            };
            let invert = f.set.contains(&'v');
            let many = rest.len() > 1;
            let mut out = String::new();
            let mut total = 0;
            let sources: Vec<(String, Result<Vec<u8>, String>)> = if rest.is_empty() {
                vec![(String::new(), Ok(stdin.clone()))]
            } else {
                rest.iter().map(|r| (r.clone(), inputs(std::slice::from_ref(r), ctx, &stdin, "grep"))).collect()
            };
            let mut errs = String::new();
            for (fname, data) in sources {
                let data = match data {
                    Ok(d) => d,
                    Err(e) => {
                        errs.push_str(&format!("{}\n", e));
                        continue;
                    }
                };
                let mut count = 0;
                for (n, line) in text_lines(&data).iter().enumerate() {
                    if re.is_match(line) != invert {
                        count += 1;
                        if !f.set.contains(&'c') && !f.set.contains(&'l') && !f.set.contains(&'q') {
                            let prefix = if many { format!("{}:", fname) } else { String::new() };
                            let num = if f.set.contains(&'n') { format!("{}:", n + 1) } else { String::new() };
                            out.push_str(&format!("{}{}{}\n", prefix, num, line));
                        }
                    }
                }
                if f.set.contains(&'c') {
                    out.push_str(&if many { format!("{}:{}\n", fname, count) } else { format!("{}\n", count) });
                } else if f.set.contains(&'l') && count > 0 {
                    out.push_str(&format!("{}\n", if fname.is_empty() { "(standard input)" } else { &fname }));
                }
                total += count;
            }
            let code = if !errs.is_empty() { 2 } else if total > 0 { 0 } else { 1 };
            Ok(Out { code, stdout: out.into_bytes(), stderr: errs.into_bytes() })
        }
        "head" | "tail" => {
            let f = flags(args, "nc");
            let n: usize = f.values.get(&'n').and_then(|v| v.trim_start_matches('+').parse().ok()).unwrap_or(10);
            let data = match inputs(&f.rest, ctx, &stdin, name) {
                Ok(d) => d,
                Err(e) => return fail(1, e),
            };
            if let Some(c) = f.values.get(&'c').and_then(|v| v.parse::<usize>().ok()) {
                let d = if name == "head" { data[..c.min(data.len())].to_vec() } else { data[data.len().saturating_sub(c)..].to_vec() };
                return ok(d);
            }
            let lines = text_lines(&data);
            let pick: Vec<String> = if name == "head" {
                lines.into_iter().take(n).collect()
            } else if f.values.get(&'n').is_some_and(|v| v.starts_with('+')) {
                lines.into_iter().skip(n.saturating_sub(1)).collect()
            } else {
                let skip = lines.len().saturating_sub(n);
                lines.into_iter().skip(skip).collect()
            };
            ok(join_lines(&pick))
        }
        "wc" => {
            let f = flags(args, "");
            let data = match inputs(&f.rest, ctx, &stdin, "wc") {
                Ok(d) => d,
                Err(e) => return fail(1, e),
            };
            let s = String::from_utf8_lossy(&data);
            let (l, w, c) = (s.matches('\n').count(), s.split_whitespace().count(), data.len());
            let only: Vec<usize> = [('l', l), ('w', w), ('c', c), ('m', s.chars().count())]
                .iter()
                .filter(|(k, _)| f.set.contains(k))
                .map(|(_, v)| *v)
                .collect();
            let nums = if only.is_empty() { vec![l, w, c] } else { only };
            ok(format!("{}\n", nums.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ")))
        }
        "sort" => {
            let f = flags(args, "tk");
            let data = match inputs(&f.rest, ctx, &stdin, "sort") {
                Ok(d) => d,
                Err(e) => return fail(1, e),
            };
            let mut lines = text_lines(&data);
            let key = |l: &String| -> String {
                match f.values.get(&'k').and_then(|k| k.split(',').next()?.parse::<usize>().ok()) {
                    Some(k) => {
                        let fields: Vec<&str> = match f.values.get(&'t') {
                            Some(t) => l.split(t.as_str()).collect(),
                            None => l.split_whitespace().collect(),
                        };
                        fields.get(k.saturating_sub(1)).map(|s| s.to_string()).unwrap_or_default()
                    }
                    None => l.clone(),
                }
            };
            if f.set.contains(&'n') {
                let num = |l: &String| key(l).trim().parse::<f64>().unwrap_or(0.0);
                lines.sort_by(|a, b| num(a).partial_cmp(&num(b)).unwrap_or(std::cmp::Ordering::Equal));
            } else if f.set.contains(&'f') {
                lines.sort_by_key(|l| key(l).to_lowercase());
            } else {
                lines.sort_by_key(|l| key(l));
            }
            if f.set.contains(&'r') {
                lines.reverse();
            }
            if f.set.contains(&'u') {
                lines.dedup();
            }
            ok(join_lines(&lines))
        }
        "uniq" => {
            let f = flags(args, "");
            let data = match inputs(&f.rest, ctx, &stdin, "uniq") {
                Ok(d) => d,
                Err(e) => return fail(1, e),
            };
            let mut groups: Vec<(String, usize)> = Vec::new();
            for l in text_lines(&data) {
                match groups.last_mut() {
                    Some((last, n)) if *last == l => *n += 1,
                    _ => groups.push((l, 1)),
                }
            }
            let out: Vec<String> = groups
                .into_iter()
                .filter(|(_, n)| !(f.set.contains(&'d') && *n < 2) && !(f.set.contains(&'u') && *n > 1))
                .map(|(l, n)| if f.set.contains(&'c') { format!("{:>7} {}", n, l) } else { l })
                .collect();
            ok(join_lines(&out))
        }
        "cut" => {
            let f = flags(args, "dfc");
            let data = match inputs(&f.rest, ctx, &stdin, "cut") {
                Ok(d) => d,
                Err(e) => return fail(1, e),
            };
            let delim = f.values.get(&'d').cloned().unwrap_or_else(|| "\t".into());
            let spec = match f.values.get(&'f').or_else(|| f.values.get(&'c')) {
                Some(s) => s.clone(),
                None => return fail(1, "cut: you must specify a list of fields (-f) or characters (-c)".into()),
            };
            let wanted = |i: usize| -> bool {
                spec.split(',').any(|r| match r.split_once('-') {
                    Some((a, b)) => i >= a.parse().unwrap_or(1) && (b.is_empty() || i <= b.parse().unwrap_or(usize::MAX)),
                    None => r.parse() == Ok(i),
                })
            };
            let by_char = f.values.contains_key(&'c');
            let out: Vec<String> = text_lines(&data)
                .iter()
                .map(|l| {
                    if by_char {
                        l.chars().enumerate().filter(|(i, _)| wanted(i + 1)).map(|(_, c)| c).collect()
                    } else if !l.contains(delim.as_str()) {
                        l.clone()
                    } else {
                        l.split(delim.as_str())
                            .enumerate()
                            .filter(|(i, _)| wanted(i + 1))
                            .map(|(_, s)| s)
                            .collect::<Vec<_>>()
                            .join(&delim)
                    }
                })
                .collect();
            ok(join_lines(&out))
        }
        "tr" => {
            let f = flags(args, "");
            let s = String::from_utf8_lossy(&stdin).to_string();
            let set = |s: &str| -> Vec<char> {
                match s {
                    "[:lower:]" => ('a'..='z').collect(),
                    "[:upper:]" => ('A'..='Z').collect(),
                    "[:digit:]" => ('0'..='9').collect(),
                    "[:space:]" => vec![' ', '\t', '\n', '\r'],
                    _ => {
                        let c: Vec<char> = s.chars().collect();
                        let mut out = Vec::new();
                        let mut i = 0;
                        while i < c.len() {
                            if i + 2 < c.len() && c[i + 1] == '-' {
                                out.extend(c[i]..=c[i + 2]);
                                i += 3;
                            } else {
                                out.push(match (c[i], c.get(i + 1)) {
                                    ('\\', Some('n')) => {
                                        i += 1;
                                        '\n'
                                    }
                                    ('\\', Some('t')) => {
                                        i += 1;
                                        '\t'
                                    }
                                    (ch, _) => ch,
                                });
                                i += 1;
                            }
                        }
                        out
                    }
                }
            };
            let a = set(f.rest.first().map(String::as_str).unwrap_or(""));
            if f.set.contains(&'d') {
                return ok(s.chars().filter(|c| !a.contains(c)).collect::<String>());
            }
            let b = set(f.rest.get(1).map(String::as_str).unwrap_or(""));
            if b.is_empty() {
                return fail(1, "tr: missing second set".into());
            }
            ok(s.chars()
                .map(|c| a.iter().position(|x| *x == c).map(|i| b[i.min(b.len() - 1)]).unwrap_or(c))
                .collect::<String>())
        }
        "tee" => {
            let f = flags(args, "");
            for t in &f.rest {
                let p = resolve(&ctx.cwd, t);
                let r = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(f.set.contains(&'a'))
                    .truncate(!f.set.contains(&'a'))
                    .open(&p)
                    .and_then(|mut file| file.write_all(&stdin));
                if let Err(e) = r {
                    return fail(1, format!("tee: {}: {}", t, short(&e)));
                }
            }
            ok(stdin)
        }
        "basename" => {
            let Some(p) = args.first() else { return fail(1, "basename: missing operand".into()) };
            let base = p.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").to_string();
            let base = match args.get(1) {
                Some(suffix) if base.ends_with(suffix.as_str()) && base != *suffix => base[..base.len() - suffix.len()].to_string(),
                _ => base,
            };
            ok(format!("{}\n", base))
        }
        "dirname" => {
            let Some(p) = args.first() else { return fail(1, "dirname: missing operand".into()) };
            let t = p.trim_end_matches(['/', '\\']);
            let d = match t.rfind(['/', '\\']) {
                Some(0) => "/".to_string(),
                Some(i) => t[..i].to_string(),
                None => ".".to_string(),
            };
            ok(format!("{}\n", d))
        }
        "which" => {
            let mut out = String::new();
            let mut code = 0;
            for a in args {
                if BUILTINS.contains(&a.as_str()) && ctx.native {
                    out.push_str(&format!("{}: shell built-in command\n", a));
                } else if let Some(p) = find_program_in(a, &ctx.env) {
                    out.push_str(&format!("{}\n", path_str(&p)));
                } else {
                    code = 1;
                }
            }
            Ok(Out { code, stdout: out.into_bytes(), stderr: Vec::new() })
        }
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------
// Fogu API
// ---------------------------------------------------------------------

/// `(command, { cwd, env, input, native })`
fn run_args(a: &[Value], what: &str) -> Result<(Script, Ctx, Vec<u8>), String> {
    max_args(a, 2, what)?;
    let src = str_arg(a, 0, what)?;
    let script = parse(&src)?;
    let mut env: BTreeMap<String, String> = std::env::vars().collect();
    let mut cwd = shell_cwd();
    let mut input = Vec::new();
    let mut native = true;
    match a.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            let m = m.borrow();
            if let Some(Value::Map(e)) = m.get("env") {
                for (k, v) in e.borrow().iter() {
                    env.insert(k.clone(), v.to_display_string());
                }
            }
            if let Some(Value::Str(d)) = m.get("cwd") {
                cwd = resolve(&cwd, d);
            }
            match m.get("input") {
                Some(Value::Str(s)) => input = s.as_bytes().to_vec(),
                Some(Value::Nil) | None => {}
                Some(other) => input = other.to_display_string().into_bytes(),
            }
            if let Some(Value::Bool(b)) = m.get("native") {
                native = *b;
            }
        }
        Some(other) => return Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
    Ok((script, Ctx { cwd, env, last: 0, exited: false, native }, input))
}

fn execute(a: &[Value], what: &str, mode: Mode) -> Result<Out, String> {
    let (script, mut ctx, input) = run_args(a, what)?;
    run_script(&script, &mut ctx, mode, input)
}

fn run_value(a: &[Value], what: &str, mode: Mode) -> Result<Value, String> {
    let out = execute(a, what, mode)?;
    Ok(map(vec![
        ("ok", Value::Bool(out.code == 0)),
        ("code", Value::Int(out.code as i64)),
        ("stdout", Value::Str(String::from_utf8_lossy(&out.stdout).into_owned().into())),
        ("stderr", Value::Str(String::from_utf8_lossy(&out.stderr).into_owned().into())),
    ]))
}

/// Runs a command line for other modules (fogu:playbook's local
/// connection): `(exit code, stdout, stderr)`.
#[cfg(feature = "ops")]
pub(crate) fn run_line(src: &str, cwd: Option<&Path>, env: &[(String, String)], stdin: Vec<u8>) -> Result<(i32, Vec<u8>, Vec<u8>), String> {
    let script = parse(src)?;
    let mut vars: BTreeMap<String, String> = std::env::vars().collect();
    vars.extend(env.iter().cloned());
    let mut ctx = Ctx { cwd: cwd.map(Path::to_path_buf).unwrap_or_else(shell_cwd), env: vars, last: 0, exited: false, native: true };
    let out = run_script(&script, &mut ctx, Mode::Capture, stdin)?;
    Ok((out.code, out.stdout, out.stderr))
}

/// stdout of a command that must succeed.
fn checked(a: &[Value], what: &str) -> Result<String, String> {
    let out = execute(a, what, Mode::Capture)?;
    if out.code != 0 {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(io_err!(
            "shell.{}: exit code {}{}",
            what,
            out.code,
            if err.is_empty() { String::new() } else { format!(": {}", err) }
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(src: &str, input: &str) -> (i32, String, String) {
        let script = parse(src).unwrap();
        let mut ctx = Ctx {
            cwd: std::env::current_dir().unwrap(),
            env: [("NAME".to_string(), "fogu world".to_string())].into_iter().collect(),
            last: 0,
            exited: false,
            native: true,
        };
        let o = run_script(&script, &mut ctx, Mode::Capture, input.as_bytes().to_vec()).unwrap();
        (o.code, String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into())
    }

    #[test]
    fn pipes_and_operators() {
        assert_eq!(sh("echo b a c | tr ' ' '\\n' | sort", "").1, "a\nb\nc\n");
        assert_eq!(sh("false && echo no || echo yes", "").1, "yes\n");
        assert_eq!(sh("echo 1; echo 2", "").1, "1\n2\n");
        assert_eq!(sh("printf_does_not_exist", "").0, 127);
        assert_eq!(sh("seq 3 | grep -c .", "").1, "3\n");
    }

    #[test]
    fn quoting_and_vars() {
        assert_eq!(sh("echo \"$NAME\" '$NAME' ${NAME}!", "").1, "fogu world $NAME fogu world!\n");
        assert_eq!(sh("X=1; echo $X; echo $(echo inner | tr a-z A-Z)", "").1, "1\nINNER\n");
        assert_eq!(sh("echo \"a  b\" c", "").1, "a  b c\n");
        assert_eq!(sh("false; echo $?", "").1, "1\n");
    }

    #[test]
    fn text_tools() {
        assert_eq!(sh("sort -u | uniq -c", "b\na\nb\n").1, "      1 a\n      1 b\n");
        assert_eq!(sh("head -n 2", "1\n2\n3\n").1, "1\n2\n");
        assert_eq!(sh("tail -1", "1\n2\n3\n").1, "3\n");
        assert_eq!(sh("cut -d , -f 2", "a,b,c\nd,e,f\n").1, "b\ne\n");
        assert_eq!(sh("grep -in HELLO", "x\nhello there\n").1, "2:hello there\n");
        assert_eq!(sh("wc -l", "1\n2\n").1, "2\n");
        assert_eq!(sh("sort -n -r", "10\n9\n100\n").1, "100\n10\n9\n");
    }

    #[test]
    fn syntax_errors() {
        assert!(parse("echo 'open").is_err());
        assert!(parse("echo a |").is_err());
        assert!(parse("&& echo").is_err());
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("plain-word"), "plain-word");
    }
}
