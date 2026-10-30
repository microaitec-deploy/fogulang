//! `b.admin(port, { user, password, host, title, dark })` — the management UI
//! of `fogu:broker`, a fogu:web + fogu:topcoat app written in Fogu
//! (`broker_admin.fg`, embedded in the runtime). It runs on the program's
//! interpreter, so the program must stay alive (`Fogu.signal.wait()`, a loop…).

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::lexer::Lexer;
use crate::runtime::parser::Parser;
use crate::runtime::value::Value;

const SOURCE: &str = include_str!("broker_admin.fg");

/// `broker.admin(...)` runs Fogu code, so the interpreter routes it here.
pub fn intercept(callee: &Value, args: &[Value], interp: &Interpreter) -> Option<Result<Value, String>> {
    let Value::BoundMethod(bm) = callee else { return None };
    let (receiver, method) = (&bm.receiver, &bm.method);
    if method != "admin" || receiver.name() != "Broker" {
        return None;
    }
    Some(start(Value::Object(receiver.clone()), args, interp))
}

fn start(broker: Value, args: &[Value], interp: &Interpreter) -> Result<Value, String> {
    crate::native::util::max_args(args, 2, "broker.admin")?;
    let mut opts: HashMap<String, Value> = match args.get(1) {
        None | Some(Value::Nil) => HashMap::new(),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(other) => return Err(format!("broker.admin: options must be a map, got '{}'", other.type_name())),
    };
    let port = match args.first() {
        Some(v @ (Value::Int(_) | Value::Str(_))) => v.clone(),
        None | Some(Value::Nil) => Value::Int(0),
        Some(other) => return Err(format!("broker.admin: expected a port or \"host:port\", got '{}'", other.type_name())),
    };
    opts.insert("port".into(), port);
    let file = "fogu:broker/admin.fg";
    let tokens = Lexer::new(SOURCE).tokenize().map_err(|e| format!("{}: {}", file, e))?;
    let program = Parser::new(tokens).parse_program().map_err(|e| format!("{}: {}", file, e))?;
    let env: Rc<RefCell<Environment>> = interp.vm_env(false, file);
    env.borrow_mut().define("broker".into(), broker, false);
    env.borrow_mut().define("options".into(), Value::Map(Rc::new(RefCell::new(opts))), false);
    // the route handlers keep `env` (and their bodies) alive after the run
    interp.vm_run(&program, env, file, None)
}
