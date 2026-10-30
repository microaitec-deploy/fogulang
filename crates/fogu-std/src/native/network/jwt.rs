//! `fogu:jwt` module — JSON Web Tokens signed with HMAC (HS256/HS384/HS512).
//!
//! ```fogu
//! import jwt from "fogu:jwt"
//!
//! const token = jwt.sign({ sub: "42", role: "admin" }, secret, { expiresIn: 1h })
//! const claims = jwt.verify(token, secret)?      // error value when invalid/expired
//! print(jwt.decode(token)?.header.alg)            // no verification
//! app.use("/api", jwt.middleware(secret))         // fogu:web: ctx.user = claims, or 401
//! ```
//!
//! A token that fails verification (bad signature, expired, wrong issuer…)
//! is an operational error (`error` value); bad arguments stop the program.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;

use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha384, Sha512};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{map, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("sign".into(), nfn_var("sign", 2, sign));
    e.insert("verify".into(), nfn_var("verify", 2, verify));
    e.insert("decode".into(), nfn_var("decode", 1, decode));
    e.insert("middleware".into(), nfn_var("middleware", 1, middleware));
    ModuleValue {
        name: "fogu:jwt".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// base64url (no padding)
// ---------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..=chunk.len() {
            out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------
// HMAC
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Alg {
    Hs256,
    Hs384,
    Hs512,
}

impl Alg {
    fn parse(s: &str) -> Option<Alg> {
        match s.to_uppercase().as_str() {
            "HS256" => Some(Alg::Hs256),
            "HS384" => Some(Alg::Hs384),
            "HS512" => Some(Alg::Hs512),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Alg::Hs256 => "HS256",
            Alg::Hs384 => "HS384",
            Alg::Hs512 => "HS512",
        }
    }
}

fn mac(alg: Alg, key: &[u8], data: &[u8]) -> Vec<u8> {
    macro_rules! run {
        ($h:ty) => {{
            let mut m = <Hmac<$h>>::new_from_slice(key).expect("HMAC accepts any key length");
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }};
    }
    match alg {
        Alg::Hs256 => run!(Sha256),
        Alg::Hs384 => run!(Sha384),
        Alg::Hs512 => run!(Sha512),
    }
}

/// Constant-time check of a signature.
fn mac_ok(alg: Alg, key: &[u8], data: &[u8], sig: &[u8]) -> bool {
    macro_rules! run {
        ($h:ty) => {{
            let mut m = <Hmac<$h>>::new_from_slice(key).expect("HMAC accepts any key length");
            m.update(data);
            m.verify_slice(sig).is_ok()
        }};
    }
    match alg {
        Alg::Hs256 => run!(Sha256),
        Alg::Hs384 => run!(Sha384),
        Alg::Hs512 => run!(Sha512),
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn opts_arg(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

/// Seconds from an int (seconds) or a duration (`1h`, `15m`).
fn seconds(v: Option<&Value>, what: &str) -> Result<Option<i64>, String> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Int(n)) => Ok(Some(*n)),
        Some(Value::Duration(ns)) => Ok(Some(ns / 1_000_000_000)),
        Some(other) => Err(format!("{} must be seconds (int) or a duration, got '{}'", what, other.type_name())),
    }
}

fn secret_arg(args: &[Value], i: usize, what: &str) -> Result<Vec<u8>, String> {
    let s = str_arg(args, i, what)?;
    if s.is_empty() {
        return Err(format!("{}: the secret cannot be empty", what));
    }
    Ok(s.into_bytes())
}

fn str_opt(o: &HashMap<String, Value>, k: &str) -> Option<String> {
    match o.get(k) {
        Some(Value::Str(s)) => Some(s.to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------
// sign / verify / decode
// ---------------------------------------------------------------------

/// `sign(payload, secret, { algorithm, expiresIn, notBefore, issuer, audience, subject, jwtid, header, noTimestamp })`
fn sign(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "jwt.sign")?;
    let mut claims = match &args[0] {
        Value::Map(m) => m.borrow().clone(),
        other => return Err(format!("jwt.sign: the payload must be a map, got '{}'", other.type_name())),
    };
    let key = secret_arg(args, 1, "jwt.sign")?;
    let o = opts_arg(args, 2, "jwt.sign")?;
    let alg = match str_opt(&o, "algorithm") {
        None => Alg::Hs256,
        Some(a) => Alg::parse(&a).ok_or_else(|| format!("jwt.sign: unsupported algorithm '{}' (HS256, HS384, HS512)", a))?,
    };
    let now = now_secs();
    if !o.get("noTimestamp").is_some_and(|v| v.is_truthy()) {
        claims.entry("iat".into()).or_insert(Value::Int(now));
    }
    if let Some(s) = seconds(o.get("expiresIn"), "jwt.sign: expiresIn")? {
        claims.insert("exp".into(), Value::Int(now + s));
    }
    if let Some(s) = seconds(o.get("notBefore"), "jwt.sign: notBefore")? {
        claims.insert("nbf".into(), Value::Int(now + s));
    }
    for (opt, claim) in [("issuer", "iss"), ("subject", "sub"), ("audience", "aud"), ("jwtid", "jti")] {
        if let Some(v) = o.get(opt).filter(|v| !matches!(v, Value::Nil)) {
            claims.insert(claim.into(), v.clone());
        }
    }
    let mut header = match o.get("header") {
        Some(Value::Map(h)) => h.borrow().clone(),
        _ => HashMap::new(),
    };
    header.insert("alg".into(), Value::Str(alg.name().into()));
    header.insert("typ".into(), Value::Str("JWT".into()));

    let enc = |m: HashMap<String, Value>| b64_encode(stringify_value(&Value::Map(std::rc::Rc::new(RefCell::new(m)))).as_bytes());
    let signing_input = format!("{}.{}", enc(header), enc(claims));
    let sig = b64_encode(&mac(alg, &key, signing_input.as_bytes()));
    Ok(Value::Str(format!("{}.{}", signing_input, sig).into()))
}

struct Parts {
    header: Value,
    payload: Value,
    signing_input: String,
    signature: Vec<u8>,
}

fn split(token: &str) -> Result<Parts, String> {
    let bad = || io_err!("jwt: malformed token");
    let mut it = token.trim().split('.');
    let (h, p, s) = match (it.next(), it.next(), it.next(), it.next()) {
        (Some(h), Some(p), Some(s), None) => (h, p, s),
        _ => return Err(bad()),
    };
    let json = |part: &str| -> Result<Value, String> {
        let bytes = b64_decode(part).ok_or_else(bad)?;
        let text = String::from_utf8(bytes).map_err(|_| bad())?;
        match parse_value(&text) {
            Ok(v @ Value::Map(_)) => Ok(v),
            _ => Err(bad()),
        }
    };
    Ok(Parts {
        header: json(h)?,
        payload: json(p)?,
        signing_input: format!("{}.{}", h, p),
        signature: b64_decode(s).ok_or_else(bad)?,
    })
}

fn claim(v: &Value, k: &str) -> Value {
    match v {
        Value::Map(m) => m.borrow().get(k).cloned().unwrap_or(Value::Nil),
        _ => Value::Nil,
    }
}

/// Checked claims of a token, or an operational error.
fn verify_token(token: &str, key: &[u8], o: &HashMap<String, Value>) -> Result<Value, String> {
    let parts = split(token)?;
    let alg_name = claim(&parts.header, "alg").to_display_string();
    let alg = Alg::parse(&alg_name).ok_or_else(|| io_err!("jwt: unsupported algorithm '{}'", alg_name))?;
    if let Some(Value::List(allowed)) = o.get("algorithms") {
        if !allowed.borrow().iter().any(|a| a.to_display_string().eq_ignore_ascii_case(alg.name())) {
            return Err(io_err!("jwt: algorithm {} not allowed", alg.name()));
        }
    }
    if !mac_ok(alg, key, parts.signing_input.as_bytes(), &parts.signature) {
        return Err(io_err!("jwt: invalid signature"));
    }
    let p = &parts.payload;
    let now = now_secs();
    let leeway = seconds(o.get("leeway"), "jwt.verify: leeway")?.unwrap_or(0);
    let ignore_exp = o.get("ignoreExpiration").is_some_and(|v| v.is_truthy());
    if let Value::Int(exp) = claim(p, "exp") {
        if !ignore_exp && now >= exp + leeway {
            return Err(io_err!("jwt: token expired"));
        }
    }
    if let Value::Int(nbf) = claim(p, "nbf") {
        if now + leeway < nbf {
            return Err(io_err!("jwt: token not active yet"));
        }
    }
    if let Some(max) = seconds(o.get("maxAge"), "jwt.verify: maxAge")? {
        match claim(p, "iat") {
            Value::Int(iat) if now - iat <= max + leeway => {}
            _ => return Err(io_err!("jwt: token too old (maxAge)")),
        }
    }
    if let Some(iss) = str_opt(o, "issuer") {
        if claim(p, "iss").to_display_string() != iss {
            return Err(io_err!("jwt: invalid issuer"));
        }
    }
    if let Some(sub) = str_opt(o, "subject") {
        if claim(p, "sub").to_display_string() != sub {
            return Err(io_err!("jwt: invalid subject"));
        }
    }
    if let Some(aud) = str_opt(o, "audience") {
        let ok = match claim(p, "aud") {
            Value::List(l) => l.borrow().iter().any(|a| a.to_display_string() == aud),
            v => v.to_display_string() == aud,
        };
        if !ok {
            return Err(io_err!("jwt: invalid audience"));
        }
    }
    Ok(parts.payload)
}

/// `verify(token, secret, { algorithms, issuer, audience, subject, leeway, maxAge, ignoreExpiration })`
fn verify(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "jwt.verify")?;
    let token = str_arg(args, 0, "jwt.verify")?;
    let key = secret_arg(args, 1, "jwt.verify")?;
    let o = opts_arg(args, 2, "jwt.verify")?;
    verify_token(&token, &key, &o)
}

/// `decode(token)` → `{ header, payload }`, without checking the signature.
fn decode(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "jwt.decode")?;
    let parts = split(&str_arg(args, 0, "jwt.decode")?)?;
    Ok(map(vec![("header", parts.header), ("payload", parts.payload)]))
}

// ---------------------------------------------------------------------
// fogu:web middleware
// ---------------------------------------------------------------------

/// `middleware(secret, { property = "user", optional = false, …verify options })`:
/// reads `Authorization: Bearer <token>`; valid → `ctx[property] = claims`,
/// otherwise answers 401.
struct JwtMiddleware {
    key: Vec<u8>,
    opts: HashMap<String, Value>,
    property: String,
    optional: bool,
}

fn middleware(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "jwt.middleware")?;
    let key = secret_arg(args, 0, "jwt.middleware")?;
    let opts = opts_arg(args, 1, "jwt.middleware")?;
    Ok(NativeObjectRef::new(JwtMiddleware {
        key,
        property: str_opt(&opts, "property").unwrap_or_else(|| "user".into()),
        optional: opts.get("optional").is_some_and(|v| v.is_truthy()),
        opts,
    })
    .to_value())
}

fn unauthorized(ctx: &Value, msg: &str) -> Value {
    #[cfg(not(target_family = "wasm"))]
    {
        crate::native::web::set_header(ctx, "www-authenticate", "Bearer");
        crate::native::web::json_response(401, &map(vec![("error", Value::Str(msg.to_string().into()))]))
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = ctx;
        map(vec![("status", Value::Int(401)), ("error", Value::Str(msg.into()))])
    }
}

impl FoguObject for JwtMiddleware {
    fn name(&self) -> &str {
        "Middleware"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        if method != "handle" {
            return Err(format!("method '{}' does not exist on jwt middleware", method));
        }
        let ctx = args.first().cloned().unwrap_or(Value::Nil);
        let auth = claim(&claim(&ctx, "headers"), "authorization").to_display_string();
        let token = auth
            .strip_prefix("Bearer ")
            .or_else(|| auth.strip_prefix("bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let Some(token) = token else {
            return Ok(if self.optional { Value::Nil } else { unauthorized(&ctx, "missing bearer token") });
        };
        match verify_token(token, &self.key, &self.opts) {
            Ok(claims) => {
                if let Value::Map(c) = &ctx {
                    c.borrow_mut().insert(self.property.clone(), claims);
                }
                Ok(Value::Nil)
            }
            Err(e) => {
                let msg = e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
                Ok(unauthorized(&ctx, msg.strip_prefix("jwt: ").unwrap_or(&msg)))
            }
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["handle"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_round_trip() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar", "\u{ff}\u{fe}?>"] {
            assert_eq!(b64_decode(&b64_encode(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn rfc7515_hs256_example() {
        // RFC 7515, appendix A.1
        let key = b64_decode(
            "AyM1SysPpbyDfgZld3umj1qzKObwVMkoqQ-EstJQLr_T-1qS0gZH75aKtMN3Yj0iPS4hcgUuTwjAzZr1Z9CAow",
        )
        .unwrap();
        let input = "eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
        assert_eq!(
            b64_encode(&mac(Alg::Hs256, &key, input.as_bytes())),
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        );
    }
}
