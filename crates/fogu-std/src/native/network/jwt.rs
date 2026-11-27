//! `fogu:jwt` module — JSON Web Tokens signed with HMAC (HS256/HS384/HS512)
//! or with a public key (RS256/384/512, PS256/384/512, ES256/384, EdDSA).
//!
//! ```fogu
//! import jwt from "fogu:jwt"
//!
//! const token = jwt.sign({ sub: "42", role: "admin" }, secret, { expiresIn: 1h })
//! const claims = jwt.verify(token, secret)?      // error value when invalid/expired
//! print(jwt.decode(token)?.header.alg)            // no verification
//! app.use("/api", jwt.middleware(secret))         // fogu:web: ctx.user = claims, or 401
//!
//! // public keys: PEM, a JWK, a JWK set, or one fetched (and refreshed) from a URL
//! const t2 = jwt.sign({ sub: "42" }, keys.privateKey, { algorithm: "ES256", keyid: "k1" })
//! jwt.verify(t2, keys.publicKey)?
//! const idp = jwt.oidc("https://login.example.com/realms/main")?   // discovery → JWKS, checks iss
//! app.use("/api", jwt.middleware(idp, { audience: "chat" }))
//! ```
//!
//! A token that fails verification (bad signature, expired, wrong issuer…)
//! is an operational error (`error` value); bad arguments stop the program.
//! The algorithm comes from the key, never from the token alone: a secret
//! accepts only HS*, a public key only the algorithms of its type (and a
//! JWK's own `alg`), `alg: none` never.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;

use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha384, Sha512};

use crate::runtime::value::Str;
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
    #[cfg(not(target_family = "wasm"))]
    {
        // jwks(url, { ttl, minRefresh, timeout, issuer }) · oidc(issuer, opts): keys fetched from the network
        e.insert("jwks".into(), nfn_var("jwks", 1, asym::jwks));
        e.insert("oidc".into(), nfn_var("oidc", 1, asym::oidc));
    }
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

/// A part of a compact token: base64url without padding (RFC 7515), in its
/// one canonical spelling — no `=`, no `+`/`/`, no unused bits set — so a
/// token has exactly one accepted form.
fn b64url_part(s: &str) -> Option<Vec<u8>> {
    if s.len() % 4 == 1 || s.bytes().any(|c| !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_')) {
        return None;
    }
    let out = b64_decode(s)?;
    // the bits of the last character beyond the last byte must be zero
    let unused = (s.len() * 6) % 8;
    if unused > 0 {
        let last = *s.as_bytes().last()?;
        let v = match last {
            b'A'..=b'Z' => last - b'A',
            b'a'..=b'z' => last - b'a' + 26,
            b'0'..=b'9' => last - b'0' + 52,
            b'-' => 62,
            _ => 63,
        };
        if v & ((1u8 << unused) - 1) != 0 {
            return None;
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

fn opts_arg(args: &[Value], i: usize, what: &str) -> Result<HashMap<Str, Value>, String> {
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

fn str_opt(o: &HashMap<Str, Value>, k: &str) -> Option<String> {
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
    let alg = sign_alg(&key, str_opt(&o, "algorithm"))?;
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
    if let Some(kid) = o.get("keyid").filter(|v| !matches!(v, Value::Nil)) {
        header.insert("kid".into(), Value::Str(kid.to_display_string().into()));
    }

    let enc = |m: HashMap<Str, Value>| b64_encode(stringify_value(&Value::Map(std::rc::Rc::new(RefCell::new(m)))).as_bytes());
    let signing_input = format!("{}.{}", enc(header), enc(claims));
    let sig = b64_encode(&alg.sign(&key, signing_input.as_bytes())?);
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
    // no trimming: a token with spaces around it is not a token
    let mut it = token.split('.');
    let (h, p, s) = match (it.next(), it.next(), it.next(), it.next()) {
        (Some(h), Some(p), Some(s), None) => (h, p, s),
        _ => return Err(bad()),
    };
    let json = |part: &str| -> Result<Value, String> {
        let bytes = b64url_part(part).ok_or_else(bad)?;
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
        signature: b64url_part(s).ok_or_else(bad)?,
    })
}

fn claim(v: &Value, k: &str) -> Value {
    match v {
        Value::Map(m) => m.borrow().get(k).cloned().unwrap_or(Value::Nil),
        _ => Value::Nil,
    }
}

/// Whether the `algorithms` option (if any) lists `name`.
fn allowed(o: &HashMap<Str, Value>, name: &str) -> Result<(), String> {
    if let Some(Value::List(list)) = o.get("algorithms") {
        if !list.borrow().iter().any(|a| a.to_display_string().eq_ignore_ascii_case(name)) {
            return Err(io_err!("jwt: algorithm {} not allowed", name));
        }
    }
    Ok(())
}

/// Checked claims of a token, or an operational error.
fn verify_token(token: &str, key: &Key, o: &HashMap<Str, Value>) -> Result<Value, String> {
    let parts = split(token)?;
    let alg_name = claim(&parts.header, "alg").to_display_string();
    if alg_name.eq_ignore_ascii_case("none") {
        return Err(io_err!("jwt: unsigned tokens (alg none) are not accepted"));
    }
    match key {
        Key::Secret(secret) => {
            let alg = match Alg::parse(&alg_name) {
                Some(a) => a,
                #[cfg(not(target_family = "wasm"))]
                None if crate::native::network::keys::SigAlg::parse(&alg_name).is_some() => {
                    return Err(io_err!("jwt: {} needs a public key, not a secret", alg_name))
                }
                None => return Err(io_err!("jwt: unsupported algorithm '{}'", alg_name)),
            };
            allowed(o, alg.name())?;
            if !mac_ok(alg, secret, parts.signing_input.as_bytes(), &parts.signature) {
                return Err(io_err!("jwt: invalid signature"));
            }
        }
        #[cfg(not(target_family = "wasm"))]
        Key::Public(keys) => asym::check(&parts, &alg_name, keys, o)?,
        #[cfg(not(target_family = "wasm"))]
        Key::Jwks(j) => {
            asym::check(&parts, &alg_name, &j.keys(), o)?;
            // a provider's keys sign only its own tokens
            if let (Some(iss), None) = (&j.issuer, o.get("issuer")) {
                if claim(&parts.payload, "iss").to_display_string() != *iss {
                    return Err(io_err!("jwt: invalid issuer"));
                }
            }
        }
    }
    let p = &parts.payload;
    // NumericDate (RFC 7519) is any JSON number, fractions included; compared as f64 so that
    // neither a float claim slips past the check nor a huge int overflows the sums
    let date = |k: &str| -> Result<Option<f64>, String> {
        match claim(p, k) {
            Value::Nil => Ok(None),
            Value::Int(n) => Ok(Some(n as f64)),
            Value::Float(f) if f.is_finite() => Ok(Some(f)),
            _ => Err(io_err!("jwt: invalid '{}' claim (not a number)", k)),
        }
    };
    let now = now_secs() as f64;
    let leeway = seconds(o.get("leeway"), "jwt.verify: leeway")?.unwrap_or(0) as f64;
    let ignore_exp = o.get("ignoreExpiration").is_some_and(|v| v.is_truthy());
    if let Some(exp) = date("exp")? {
        if !ignore_exp && now >= exp + leeway {
            return Err(io_err!("jwt: token expired"));
        }
    }
    if let Some(nbf) = date("nbf")? {
        if now + leeway < nbf {
            return Err(io_err!("jwt: token not active yet"));
        }
    }
    let iat = date("iat")?;
    if let Some(max) = seconds(o.get("maxAge"), "jwt.verify: maxAge")? {
        match iat {
            Some(iat) if now - iat <= max as f64 + leeway => {}
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

/// `verify(token, key, { algorithms, issuer, audience, subject, leeway, maxAge, ignoreExpiration })`:
/// `key` is a secret, a PEM key, a JWK, a JWK set (`{ keys }` or a list) or `jwt.jwks(url)`.
fn verify(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "jwt.verify")?;
    let token = str_arg(args, 0, "jwt.verify")?;
    let key = key_arg(&args[1], "jwt.verify")?;
    let o = opts_arg(args, 2, "jwt.verify")?;
    // keys from a URL: fetched (waiting cooperatively) when missing, stale or rotated
    #[cfg(not(target_family = "wasm"))]
    if let Key::Jwks(j) = &key {
        if j.needs_fetch(asym::kid_of(&token).as_deref()) {
            let job = j.fetch_job();
            let r = crate::native::rt::offload(job)?;
            j.apply(r);
        }
    }
    verify_token(&token, &key, &o)
}

/// What a token is verified with.
#[derive(Clone)]
enum Key {
    Secret(Vec<u8>),
    #[cfg(not(target_family = "wasm"))]
    Public(Vec<crate::native::network::keys::Jwk>),
    #[cfg(not(target_family = "wasm"))]
    Jwks(asym::JwksHandle),
}

fn key_arg(v: &Value, what: &str) -> Result<Key, String> {
    match v {
        #[cfg(not(target_family = "wasm"))]
        Value::Str(s) if crate::native::network::keys::is_pem(s) => {
            let key = crate::native::network::keys::public_from_pem(s).map_err(|e| io_err!("{}: {}", what, e))?;
            Ok(Key::Public(vec![crate::native::network::keys::Jwk { kid: None, alg: None, use_: None, key }]))
        }
        Value::Str(s) if s.is_empty() => Err(format!("{}: the secret cannot be empty", what)),
        Value::Str(s) => Ok(Key::Secret(s.as_bytes().to_vec())),
        #[cfg(not(target_family = "wasm"))]
        Value::Map(_) | Value::List(_) => asym::keys_value(v, what).map(Key::Public),
        #[cfg(not(target_family = "wasm"))]
        Value::Object(o) if o.name() == "Jwks" => {
            let b = o.inner();
            let b = b.borrow();
            let j = b.as_any().downcast_ref::<asym::Jwks>().expect("Jwks");
            Ok(Key::Jwks(j.0.clone()))
        }
        other => Err(format!("{}: the key must be a secret, a PEM key, a JWK or a JWK set, got '{}'", what, other.type_name())),
    }
}

/// The algorithm `sign` uses: the one asked for, else HS256 for a secret
/// and the key's own for a private key.
fn sign_alg(key: &[u8], asked: Option<String>) -> Result<SignAlg, String> {
    let text = std::str::from_utf8(key).unwrap_or("");
    #[cfg(not(target_family = "wasm"))]
    {
        use crate::native::network::keys;
        if keys::is_pem(text) {
            let alg = match asked {
                None => keys::private_family(text).map_err(|e| io_err!("jwt.sign: {}", e))?.default_alg(),
                Some(a) => keys::SigAlg::parse(&a).ok_or_else(|| {
                    if Alg::parse(&a).is_some() {
                        format!("jwt.sign: {} needs a secret, not a PEM key", a)
                    } else {
                        format!("jwt.sign: unsupported algorithm '{}' (HS256 HS384 HS512 {})", a, keys::SigAlg::NAMES)
                    }
                })?,
            };
            return Ok(SignAlg::Key(alg, text.to_string()));
        }
        if let Some(a) = asked.as_deref().filter(|a| keys::SigAlg::parse(a).is_some()) {
            return Err(format!("jwt.sign: {} needs a private key in PEM (PKCS#8), not a secret", a));
        }
    }
    let _ = text;
    match asked {
        None => Ok(SignAlg::Mac(Alg::Hs256, key.to_vec())),
        Some(a) => Alg::parse(&a).map(|x| SignAlg::Mac(x, key.to_vec())).ok_or_else(|| format!("jwt.sign: unsupported algorithm '{}' (HS256, HS384, HS512)", a)),
    }
}

enum SignAlg {
    Mac(Alg, Vec<u8>),
    #[cfg(not(target_family = "wasm"))]
    Key(crate::native::network::keys::SigAlg, String),
}

impl SignAlg {
    fn name(&self) -> &'static str {
        match self {
            SignAlg::Mac(a, _) => a.name(),
            #[cfg(not(target_family = "wasm"))]
            SignAlg::Key(a, _) => a.name(),
        }
    }

    fn sign(&self, _key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
        match self {
            SignAlg::Mac(a, k) => Ok(mac(*a, k, data)),
            #[cfg(not(target_family = "wasm"))]
            SignAlg::Key(a, pem) => crate::native::network::keys::sign(*a, pem, data).map_err(|e| io_err!("jwt.sign: {}", e)),
        }
    }
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
    key: Key,
    opts: HashMap<Str, Value>,
    property: String,
    optional: bool,
}

fn middleware(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "jwt.middleware")?;
    let key = key_arg(&args[0], "jwt.middleware")?;
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

fn bearer(ctx: &Value) -> Option<String> {
    let auth = claim(&claim(ctx, "headers"), "authorization").to_display_string();
    auth.strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// What the middleware does with a request: `nil` to go on, or the 401 answer.
fn authenticate(key: &Key, opts: &HashMap<Str, Value>, property: &str, optional: bool, ctx: &Value) -> Value {
    let Some(token) = bearer(ctx) else {
        return if optional { Value::Nil } else { unauthorized(ctx, "missing bearer token") };
    };
    match verify_token(&token, key, opts) {
        Ok(claims) => {
            if let Value::Map(c) = ctx {
                c.borrow_mut().insert(property.into(), claims);
            }
            Value::Nil
        }
        Err(e) => {
            let msg = e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(&e).to_string();
            unauthorized(ctx, msg.strip_prefix("jwt: ").unwrap_or(&msg))
        }
    }
}

impl FoguObject for JwtMiddleware {
    fn name(&self) -> &str {
        "JwtMiddleware"
    }

    // keys from a URL that must be fetched first: the request waits for them
    // cooperatively (the server goes on with the others)
    #[cfg(not(target_family = "wasm"))]
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<crate::native::object::Poller, String>> {
        let Key::Jwks(j) = &self.key else { return None };
        if method != "handle" {
            return None;
        }
        let ctx = args.first().cloned().unwrap_or(Value::Nil);
        let token = bearer(&ctx)?;
        if !j.needs_fetch(asym::kid_of(&token).as_deref()) {
            return None;
        }
        let pending = crate::native::rt::spawn_blocking(j.fetch_job());
        let (key, opts, property, optional) = (self.key.clone(), self.opts.clone(), self.property.clone(), self.optional);
        Some(Ok(Box::new(move || {
            let r = pending.try_take()?;
            if let Key::Jwks(j) = &key {
                j.apply(r);
            }
            Some(Ok(authenticate(&key, &opts, &property, optional, &ctx)))
        })))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        if method != "handle" {
            return Err(format!("method '{}' does not exist on jwt middleware", method));
        }
        let ctx = args.first().cloned().unwrap_or(Value::Nil);
        Ok(authenticate(&self.key, &self.opts, &self.property, self.optional, &ctx))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["handle"]
    }
}

// ---------------------------------------------------------------------
// public keys and key sets
// ---------------------------------------------------------------------

#[cfg(not(target_family = "wasm"))]
mod asym {
    use super::*;
    use crate::native::network::keys::{self, Jwk, SigAlg};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// The `kid` of a token's header, without verifying anything.
    pub(super) fn kid_of(token: &str) -> Option<String> {
        let h = token.split('.').next()?;
        let v: serde_json::Value = serde_json::from_slice(&b64url_part(h)?).ok()?;
        v.get("kid")?.as_str().map(str::to_string)
    }

    fn json_of(v: &Value, what: &str) -> Result<serde_json::Value, String> {
        serde_json::from_str(&stringify_value(v)).map_err(|e| format!("{}: {}", what, e))
    }

    /// A JWK, a JWK set (`{ keys: [...] }`) or a list of JWKs. In a set, keys
    /// that cannot verify signatures (`oct`, `use: enc`, unknown types) are skipped.
    pub(super) fn keys_value(v: &Value, what: &str) -> Result<Vec<Jwk>, String> {
        let j = json_of(v, what)?;
        let set = match (&j, j.get("keys")) {
            (_, Some(serde_json::Value::Array(a))) => a.clone(),
            (serde_json::Value::Array(a), _) => a.clone(),
            _ => return keys::jwk_from_json(&j).map(|k| vec![k]).map_err(|e| io_err!("{}: {}", what, e)),
        };
        let keys = usable(&set);
        if keys.is_empty() {
            return Err(io_err!("{}: the key set has no key that verifies signatures", what));
        }
        Ok(keys)
    }

    fn usable(set: &[serde_json::Value]) -> Vec<Jwk> {
        set.iter().filter_map(|k| keys::jwk_from_json(k).ok()).filter(|k| k.use_.as_deref() != Some("enc")).collect()
    }

    /// The signature of `parts` against `keys`, choosing them by `kid` and type.
    pub(super) fn check(parts: &Parts, alg_name: &str, keys: &[Jwk], o: &HashMap<Str, Value>) -> Result<(), String> {
        let alg = SigAlg::parse(alg_name).ok_or_else(|| {
            if Alg::parse(alg_name).is_some() {
                io_err!("jwt: an {} token cannot be verified with a public key", alg_name)
            } else {
                io_err!("jwt: unsupported algorithm '{}'", alg_name)
            }
        })?;
        allowed(o, alg.name())?;
        let kid = match claim(&parts.header, "kid") {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        };
        let mut candidates: Vec<&Jwk> = keys
            .iter()
            .filter(|k| k.key.family() == alg.family() && k.alg.as_deref().is_none_or(|a| a == alg.name()))
            .collect();
        // a set whose keys have ids: the token's id picks the key
        if let Some(kid) = &kid {
            if keys.iter().any(|k| k.kid.is_some()) {
                candidates.retain(|k| k.kid.as_deref() == Some(kid.as_str()));
                if candidates.is_empty() {
                    return Err(io_err!("jwt: unknown key id '{}'", kid));
                }
            }
        }
        if candidates.is_empty() {
            return Err(io_err!("jwt: no key for {}", alg.name()));
        }
        let ok = candidates
            .iter()
            .any(|k| keys::verify(alg, &k.key, parts.signing_input.as_bytes(), &parts.signature).unwrap_or(false));
        if ok { Ok(()) } else { Err(io_err!("jwt: invalid signature")) }
    }

    // ----------------------------------------------------- JWKS from a URL

    #[derive(Default)]
    struct State {
        keys: Vec<Jwk>,
        fetched: Option<Instant>,
        tried: Option<Instant>,
        error: Option<String>,
    }

    /// Keys fetched from a URL; shared by the `Jwks` value, `verify` and middleware.
    #[derive(Clone)]
    pub(super) struct JwksHandle {
        url: String,
        state: Arc<Mutex<State>>,
        ttl: Duration,
        min_refresh: Duration,
        timeout: Duration,
        pub(super) issuer: Option<String>,
    }

    pub(super) struct Jwks(pub(super) JwksHandle);

    impl JwksHandle {
        fn lock(&self) -> std::sync::MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(|e| e.into_inner())
        }

        pub(super) fn keys(&self) -> Vec<Jwk> {
            self.lock().keys.clone()
        }

        /// No keys yet, stale ones, or a key id they lack (a rotation) — at most
        /// one fetch per `minRefresh`, so random ids cannot flood the provider.
        pub(super) fn needs_fetch(&self, kid: Option<&str>) -> bool {
            let st = self.lock();
            if st.tried.is_some_and(|t| t.elapsed() < self.min_refresh) {
                return false;
            }
            st.keys.is_empty()
                || st.fetched.is_none_or(|t| t.elapsed() >= self.ttl)
                || kid.is_some_and(|kid| !st.keys.iter().any(|k| k.kid.as_deref() == Some(kid)))
        }

        pub(super) fn fetch_job(&self) -> impl FnOnce() -> Result<Vec<Jwk>, String> + Send + 'static {
            self.lock().tried = Some(Instant::now());
            let (url, timeout) = (self.url.clone(), self.timeout);
            move || fetch(&url, timeout)
        }

        /// A fetch's result: new keys, or the error (the old keys stay in use).
        pub(super) fn apply(&self, r: Result<Vec<Jwk>, String>) {
            let mut st = self.lock();
            match r {
                Ok(keys) => {
                    st.keys = keys;
                    st.fetched = Some(Instant::now());
                    st.error = None;
                }
                Err(e) => st.error = Some(e.trim_start_matches(crate::runtime::value::ERR_VALUE_PREFIX).to_string()),
            }
        }
    }

    fn get_json(url: &str, timeout: Duration, what: &str) -> Result<serde_json::Value, String> {
        let r = crate::native::util::blocking_client()
            .get(url)
            .timeout(timeout)
            .header("accept", "application/json")
            .send()
            .map_err(|e| io_err!("{}: {}: {}", what, url, crate::native::util::error_chain(&e)))?;
        if !r.status().is_success() {
            return Err(io_err!("{}: {} answered {}", what, url, r.status().as_u16()));
        }
        let text = r.text().map_err(|e| io_err!("{}: {}: {}", what, url, e))?;
        serde_json::from_str(&text).map_err(|e| io_err!("{}: {}: not JSON: {}", what, url, e))
    }

    fn fetch(url: &str, timeout: Duration) -> Result<Vec<Jwk>, String> {
        let j = get_json(url, timeout, "jwt.jwks")?;
        let set = j.get("keys").and_then(|k| k.as_array()).ok_or_else(|| io_err!("jwt.jwks: {} is not a JWK set (no 'keys')", url))?;
        let keys = usable(set);
        if keys.is_empty() {
            return Err(io_err!("jwt.jwks: {} has no key that verifies signatures", url));
        }
        Ok(keys)
    }

    fn duration_opt(o: &HashMap<Str, Value>, k: &str, default: Duration) -> Result<Duration, String> {
        Ok(seconds(o.get(k), &format!("jwt.jwks: {}", k))?.map_or(default, |s| Duration::from_secs(s.max(0) as u64)))
    }

    fn handle(url: String, o: &HashMap<Str, Value>, issuer: Option<String>) -> Result<JwksHandle, String> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!("jwt.jwks: expects an http(s) URL, got '{}'", url));
        }
        Ok(JwksHandle {
            url,
            state: Arc::new(Mutex::new(State::default())),
            ttl: duration_opt(o, "ttl", Duration::from_secs(600))?,
            min_refresh: duration_opt(o, "minRefresh", Duration::from_secs(30))?,
            timeout: duration_opt(o, "timeout", Duration::from_secs(5))?,
            issuer: issuer.or_else(|| str_opt(o, "issuer")),
        })
    }

    /// `jwt.jwks(url, { ttl = 10m, minRefresh = 30s, timeout = 5s, issuer })`: the keys are
    /// fetched on first use, again after `ttl`, and when a token names a key id they lack.
    pub(super) fn jwks(args: &[Value]) -> Result<Value, String> {
        max_args(args, 2, "jwt.jwks")?;
        let url = str_arg(args, 0, "jwt.jwks")?;
        let o = opts_arg(args, 1, "jwt.jwks")?;
        Ok(NativeObjectRef::new(Jwks(handle(url, &o, None)?)).to_value())
    }

    /// `jwt.oidc(issuer, opts)`: OpenID Connect discovery (`/.well-known/openid-configuration`)
    /// → the provider's JWKS; tokens must also carry `iss: issuer`.
    pub(super) fn oidc(args: &[Value]) -> Result<Value, String> {
        max_args(args, 2, "jwt.oidc")?;
        let issuer = str_arg(args, 0, "jwt.oidc")?;
        let o = opts_arg(args, 1, "jwt.oidc")?;
        let timeout = duration_opt(&o, "timeout", Duration::from_secs(5))?;
        let url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
        let iss = issuer.clone();
        let (jwks_uri, keys) = crate::native::rt::offload(move || -> Result<(String, Result<Vec<Jwk>, String>), String> {
            let doc = get_json(&url, timeout, "jwt.oidc")?;
            if doc.get("issuer").and_then(|v| v.as_str()) != Some(iss.as_str()) {
                return Err(io_err!("jwt.oidc: {} names another issuer", url));
            }
            let uri = doc.get("jwks_uri").and_then(|v| v.as_str()).ok_or_else(|| io_err!("jwt.oidc: {} has no jwks_uri", url))?.to_string();
            let keys = fetch(&uri, timeout);
            Ok((uri, keys))
        })??;
        let h = handle(jwks_uri, &o, Some(issuer))?;
        h.lock().tried = Some(Instant::now());
        h.apply(keys);
        Ok(NativeObjectRef::new(Jwks(h)).to_value())
    }

    impl FoguObject for Jwks {
        fn name(&self) -> &str {
            "Jwks"
        }

        fn get(&self, field: &str) -> Option<Value> {
            let st = self.0.lock();
            match field {
                "url" => Some(Value::Str(self.0.url.clone().into())),
                "issuer" => Some(self.0.issuer.clone().map_or(Value::Nil, |s| Value::Str(s.into()))),
                "kids" => Some(crate::native::util::list(st.keys.iter().filter_map(|k| k.kid.clone()).map(|k| Value::Str(k.into())).collect())),
                "size" => Some(Value::Int(st.keys.len() as i64)),
                "error" => Some(st.error.clone().map_or(Value::Nil, |e| Value::Str(e.into()))),
                _ => None,
            }
        }

        fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
            Err(format!("method '{}' does not exist on Jwks (fields: url issuer kids size error)", method))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
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
