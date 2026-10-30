//! `ai.mcpServer(options)` — MCP (Model Context Protocol) servers written in
//! Fogu: tools, resources and prompts served over stdio or Streamable HTTP. The
//! protocol lives in `ai_mcp.fg` (embedded, run once per interpreter); this file
//! starts it and provides `safeCall`, which turns a handler's runtime error into
//! an error value so one failing tool does not stop the server.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::interpreter::Interpreter;
use crate::runtime::lexer::Lexer;
use crate::runtime::parser::Parser;
use crate::runtime::value::{ErrorValue, NativeFnValue, Value, ERR_VALUE_PREFIX};

const SOURCE: &str = include_str!("ai_mcp.fg");
pub const FACTORY: &str = "ai.mcpServer";
const SAFE_CALL: &str = "ai.__safeCall";

thread_local! {
    /// The `createServer` function, once the embedded source has run.
    static FACTORY_FN: RefCell<Option<Value>> = const { RefCell::new(None) };
}

fn native(name: &str) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.into(),
        arity: 0,
        variadic: true,
        func: |_| Err("this function runs through the interpreter".into()),
        intercept: Default::default(),
    }))
}

/// The value exported as `ai.mcpServer`.
pub fn factory_value() -> Value {
    native(FACTORY)
}

pub fn intercept(callee: &Value, args: &[Value], interp: &Interpreter) -> Option<Result<Value, String>> {
    let Value::NativeFn(f) = callee else { return None };
    match f.name.as_str() {
        FACTORY => Some(create(args, interp)),
        SAFE_CALL => Some(safe_call(args, interp)),
        _ => None,
    }
}

fn create(args: &[Value], interp: &Interpreter) -> Result<Value, String> {
    let factory = match FACTORY_FN.with(|f| f.borrow().clone()) {
        Some(f) => f,
        None => {
            let file = "fogu:ai/mcp.fg";
            let tokens = Lexer::new(SOURCE).tokenize().map_err(|e| format!("{}: {}", file, e))?;
            let program = Parser::new(tokens).parse_program().map_err(|e| format!("{}: {}", file, e))?;
            let env = interp.vm_env(false, file);
            env.borrow_mut().define("safeCall".into(), native(SAFE_CALL), false);
            let f = interp.vm_run(&program, env, file, None)?;
            FACTORY_FN.with(|c| *c.borrow_mut() = Some(f.clone()));
            f
        }
    };
    interp.call_value(factory, args.to_vec(), interp.globals_ref().clone())
}

/// `safeCall(f, ...args)`: the result of `f`, or its runtime error as an error value.
fn safe_call(args: &[Value], interp: &Interpreter) -> Result<Value, String> {
    let Some(f) = args.first().cloned() else {
        return Err("safeCall(f, ...args) expects a function".into());
    };
    match interp.call_value(f, args[1..].to_vec(), interp.globals_ref().clone()) {
        Ok(v) => Ok(v),
        Err(e) => {
            let message = e.strip_prefix(ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
            Ok(Value::Error(Rc::new(ErrorValue { message, cause: Value::Nil, trace: Vec::new() })))
        }
    }
}
