//! Interactive REPL: history, multi-line input and echo of expression values.

use std::io::{BufRead, IsTerminal, Write};

use rustyline::error::ReadlineError;

use super::ui;
use crate::runtime::token::Token;
use crate::runtime::value::Value;
use crate::runtime::{Interpreter, Lexer, Parser};

/// Is the input still incomplete (open braces, unterminated string)?
fn incomplete(src: &str) -> bool {
    match Lexer::new(src).tokenize() {
        Err(e) => e.contains("unterminated"),
        Ok(toks) => {
            let mut depth = 0i32;
            for t in &toks {
                match t.tok {
                    Token::LBrace | Token::LParen | Token::LBracket => depth += 1,
                    Token::RBrace | Token::RParen | Token::RBracket => depth -= 1,
                    _ => {}
                }
            }
            depth > 0
        }
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Str(s) => ui::green(&format!("\"{}\"", s.escape_default())),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => ui::yellow(&v.to_display_string()),
        other => other.to_display_string(),
    }
}

fn eval_line(source: &str, interp: &Interpreter) -> Result<Option<Value>, String> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = Parser::new(tokens).parse_program()?;
    interp.repl_eval(&program)
}

fn handle(src: &str, interp: &mut Interpreter) -> bool {
    match src.trim() {
        ".exit" | ".quit" | "exit" | "quit" => return false,
        ".help" => {
            println!("  .help    this help\n  .clear   reset the state (variables and functions)\n  .exit    quit (or Ctrl+D)");
            return true;
        }
        ".clear" => {
            *interp = Interpreter::new();
            println!("{}", ui::dim("state cleared"));
            return true;
        }
        _ => {}
    }
    match eval_line(src, interp) {
        Ok(Some(v)) => println!("{}", show(&v)),
        Ok(None) => {}
        Err(e) => eprintln!("{} {}", ui::red_err("error:"), e),
    }
    true
}

pub fn start() -> i32 {
    let mut interp = Interpreter::new();
    let cwd = std::env::current_dir().unwrap_or_default();
    interp.set_base_dir(&cwd);
    interp.set_main_file("<repl>");
    crate::pkg::set_main_dir(&cwd);

    if !std::io::stdin().is_terminal() {
        // redirected input: read line by line, without prompts
        let stdin = std::io::stdin();
        let mut buf = String::new();
        for line in stdin.lock().lines().map_while(Result::ok) {
            buf.push_str(&line);
            buf.push('\n');
            if incomplete(&buf) {
                continue;
            }
            if !buf.trim().is_empty() && !handle(&buf, &mut interp) {
                break;
            }
            buf.clear();
        }
        let _ = std::io::stdout().flush();
        return 0;
    }

    println!(
        "{} {} — {}",
        ui::bold("Fogu"),
        super::VERSION,
        ui::dim(".help for help, Ctrl+D to quit")
    );
    let Ok(mut rl) = rustyline::DefaultEditor::new() else {
        eprintln!("could not start the line editor");
        return 1;
    };
    let hist = crate::pkg::home().join("repl_history");
    let _ = rl.load_history(&hist);

    let mut buf = String::new();
    loop {
        let prompt = if buf.is_empty() { "> " } else { "… " };
        match rl.readline(prompt) {
            Ok(line) => {
                buf.push_str(&line);
                buf.push('\n');
                if incomplete(&buf) {
                    continue;
                }
                let src = std::mem::take(&mut buf);
                if src.trim().is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(src.trim_end());
                if !handle(&src, &mut interp) {
                    break;
                }
            }
            Err(ReadlineError::Interrupted) => {
                buf.clear();
                println!("{}", ui::dim("(Ctrl+C — use Ctrl+D or .exit to quit)"));
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("read error: {}", e);
                break;
            }
        }
    }
    if let Some(p) = hist.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = rl.save_history(&hist);
    0
}
