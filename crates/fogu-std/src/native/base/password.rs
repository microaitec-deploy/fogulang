//! `fogu:password` module — password hashing (argon2id/i/d, bcrypt, scrypt,
//! PBKDF2), verification with algorithm detection, rehash checks, random
//! passwords and passphrases, and a strength estimate.
//!
//! ```fogu
//! import password from "fogu:password"
//!
//! const h = password.hash("hunter2")                      // argon2id, OWASP parameters
//! print(password.verify("hunter2", h))                   // true
//! const b = password.hash("hunter2", { algorithm: "bcrypt", cost: 12 })
//! print(password.info(b).algorithm)                      // bcrypt
//! if password.needsRehash(b) { /* store password.hash(pw) on the next login */ }
//! print(password.generate(20), password.passphrase(5))
//! print(password.strength("hunter2").score)              // 0..4
//! ```
//!
//! Hashing is slow on purpose (tens to hundreds of milliseconds): `hash` and
//! `verify` run off the interpreter thread (`rt::offload`), so other `fg`
//! tasks keep running. The salt is always random (16 bytes). A `pepper`
//! option mixes a server-side secret in before hashing:
//! `base64(HMAC-SHA256(pepper, password))` is what gets hashed, so the same
//! pepper is needed to verify (and bcrypt's 72-byte limit no longer applies).
//! bcrypt refuses passwords longer than 72 bytes instead of silently
//! truncating them (hash → error value; verify → false).

use std::cell::RefCell;
use std::sync::OnceLock;

use ahash::AHashMap as HashMap;
use argon2::password_hash::{self, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine as _;
use hmac::{Hmac, Mac};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::util::{list, map, max_args, nfn_var, opt_arg, str_arg};

// OWASP Password Storage Cheat Sheet (2024+) minimums
const ARGON2_MEMORY: u32 = 19_456; // KiB (19 MiB)
const ARGON2_TIME: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;
const BCRYPT_COST: u32 = 12;
const SCRYPT_LOG_N: u8 = 17; // N = 131072 (128 MiB with r = 8)
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;
const PBKDF2_SHA256_ITERATIONS: u32 = 600_000;
const PBKDF2_SHA512_ITERATIONS: u32 = 210_000;
/// scrypt memory (128·N·r bytes) above this is refused, also when verifying a stored hash
const SCRYPT_MAX_MEMORY: u128 = 2 << 30;

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    let mut f = |name: &str, min: usize, func: fn(&[Value]) -> Result<Value, String>| {
        e.insert(name.to_string(), nfn_var(name, min, func));
    };
    f("hash", 1, hash_fn);
    f("verify", 2, verify_fn);
    f("needsRehash", 1, needs_rehash_fn);
    f("info", 1, info_fn);
    f("generate", 0, generate_fn);
    f("passphrase", 0, passphrase_fn);
    f("strength", 1, |a| strength_fn(a, "password.strength"));
    e.insert(
        "algorithms".into(),
        list(ALGORITHMS.iter().map(|s| Value::str(*s)).collect()),
    );
    ModuleValue {
        name: "fogu:password".into(),
        exports: RefCell::new(e),
    }
}

const ALGORITHMS: [&str; 7] = ["argon2id", "argon2i", "argon2d", "bcrypt", "scrypt", "pbkdf2-sha256", "pbkdf2-sha512"];

// ---------------------------------------------------------------------
// options
// ---------------------------------------------------------------------

const HASH_KEYS: [&str; 10] = ["algorithm", "memory", "time", "parallelism", "cost", "n", "r", "p", "iterations", "pepper"];

fn opts(v: Option<&Value>, keys: &[&str], what: &str) -> Result<HashMap<String, Value>, String> {
    let m: HashMap<String, Value> = match v {
        None | Some(Value::Nil) => return Ok(HashMap::new()),
        Some(Value::Map(m)) => m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(other) => return Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    };
    let mut names: Vec<&String> = m.keys().collect();
    names.sort();
    for k in names {
        if !keys.contains(&k.as_str()) {
            return Err(format!("{}: unknown option '{}' (expected {})", what, k, keys.join(", ")));
        }
    }
    Ok(m)
}

fn opt_u32(o: &HashMap<String, Value>, k: &str, lo: u32, hi: u32, what: &str) -> Result<Option<u32>, String> {
    match o.get(k) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Int(i)) if (lo as i64..=hi as i64).contains(i) => Ok(Some(*i as u32)),
        Some(Value::Int(i)) => Err(format!("{}: option '{}' must be between {} and {}, got {}", what, k, lo, hi, i)),
        Some(other) => Err(format!("{}: option '{}' must be an int, got '{}'", what, k, other.type_name())),
    }
}

fn opt_bool(o: &HashMap<String, Value>, k: &str, default: bool, what: &str) -> Result<bool, String> {
    match o.get(k) {
        None | Some(Value::Nil) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => Err(format!("{}: option '{}' must be a bool, got '{}'", what, k, other.type_name())),
    }
}

fn pepper_opt(o: &HashMap<String, Value>, what: &str) -> Result<Option<Vec<u8>>, String> {
    match o.get("pepper") {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Str(s)) if !s.is_empty() => Ok(Some(s.as_bytes().to_vec())),
        Some(Value::Buffer(_)) => Ok(Some(crate::native::util::bytes_of(o.get("pepper").unwrap(), what)?)),
        Some(other) => Err(format!("{}: option 'pepper' must be a non-empty str or buffer, got '{}'", what, other.type_name())),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Algo {
    Argon2(argon2::Algorithm),
    Bcrypt,
    Scrypt,
    Pbkdf2Sha256,
    Pbkdf2Sha512,
}

impl Algo {
    fn name(self) -> &'static str {
        match self {
            Algo::Argon2(argon2::Algorithm::Argon2id) => "argon2id",
            Algo::Argon2(argon2::Algorithm::Argon2i) => "argon2i",
            Algo::Argon2(argon2::Algorithm::Argon2d) => "argon2d",
            Algo::Bcrypt => "bcrypt",
            Algo::Scrypt => "scrypt",
            Algo::Pbkdf2Sha256 => "pbkdf2-sha256",
            Algo::Pbkdf2Sha512 => "pbkdf2-sha512",
        }
    }

    fn parse(s: &str) -> Option<Algo> {
        Some(match s.to_ascii_lowercase().as_str() {
            "argon2id" | "argon2" => Algo::Argon2(argon2::Algorithm::Argon2id),
            "argon2i" => Algo::Argon2(argon2::Algorithm::Argon2i),
            "argon2d" => Algo::Argon2(argon2::Algorithm::Argon2d),
            "bcrypt" => Algo::Bcrypt,
            "scrypt" => Algo::Scrypt,
            "pbkdf2-sha256" | "pbkdf2" => Algo::Pbkdf2Sha256,
            "pbkdf2-sha512" => Algo::Pbkdf2Sha512,
            _ => return None,
        })
    }
}

/// What `hash` produces (and `needsRehash` compares against).
#[derive(Clone, Debug)]
struct Spec {
    algo: Algo,
    memory: u32,
    time: u32,
    parallelism: u32,
    cost: u32,
    log_n: u8,
    r: u32,
    p: u32,
    iterations: u32,
    pepper: Option<Vec<u8>>,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            algo: Algo::Argon2(argon2::Algorithm::Argon2id),
            memory: ARGON2_MEMORY,
            time: ARGON2_TIME,
            parallelism: ARGON2_PARALLELISM,
            cost: BCRYPT_COST,
            log_n: SCRYPT_LOG_N,
            r: SCRYPT_R,
            p: SCRYPT_P,
            iterations: PBKDF2_SHA256_ITERATIONS,
            pepper: None,
        }
    }
}

fn spec(v: Option<&Value>, what: &str) -> Result<Spec, String> {
    let o = opts(v, &HASH_KEYS, what)?;
    let mut s = Spec::default();
    match o.get("algorithm") {
        None | Some(Value::Nil) => {}
        Some(Value::Str(a)) => {
            s.algo = Algo::parse(a).ok_or_else(|| format!("{}: unknown algorithm '{}' (expected {})", what, a, ALGORITHMS.join(", ")))?;
        }
        Some(other) => return Err(format!("{}: option 'algorithm' must be a str, got '{}'", what, other.type_name())),
    }
    if s.algo == Algo::Pbkdf2Sha512 {
        s.iterations = PBKDF2_SHA512_ITERATIONS;
    }
    if let Some(m) = opt_u32(&o, "memory", 8, u32::MAX, what)? {
        s.memory = m;
    }
    if let Some(t) = opt_u32(&o, "time", 1, u32::MAX, what)? {
        s.time = t;
    }
    if let Some(p) = opt_u32(&o, "parallelism", 1, 0xFF_FFFF, what)? {
        s.parallelism = p;
    }
    if s.memory < 8 * s.parallelism {
        return Err(format!("{}: argon2 'memory' must be at least 8 KiB per lane ({} KiB with parallelism {})", what, 8 * s.parallelism, s.parallelism));
    }
    if let Some(c) = opt_u32(&o, "cost", 4, 31, what)? {
        s.cost = c;
    }
    if let Some(n) = opt_u32(&o, "n", 2, 1 << 30, what)? {
        if !n.is_power_of_two() {
            return Err(format!("{}: scrypt 'n' must be a power of two, got {}", what, n));
        }
        s.log_n = n.trailing_zeros() as u8;
    }
    if let Some(r) = opt_u32(&o, "r", 1, 1 << 20, what)? {
        s.r = r;
    }
    if let Some(p) = opt_u32(&o, "p", 1, 1 << 20, what)? {
        s.p = p;
    }
    if s.algo == Algo::Scrypt && scrypt_memory(s.log_n, s.r) > SCRYPT_MAX_MEMORY {
        return Err(format!("{}: scrypt parameters need more than 2 GiB of memory (128 * n * r bytes)", what));
    }
    if let Some(i) = opt_u32(&o, "iterations", 1000, u32::MAX, what)? {
        s.iterations = i;
    }
    s.pepper = pepper_opt(&o, what)?;
    Ok(s)
}

fn scrypt_memory(log_n: u8, r: u32) -> u128 {
    128u128 * (1u128 << log_n) * r as u128
}

/// The bytes that get hashed: the password, or `base64(HMAC-SHA256(pepper, password))`.
fn secret(pw: &str, pepper: Option<&[u8]>) -> Vec<u8> {
    match pepper {
        None => pw.as_bytes().to_vec(),
        Some(key) => {
            let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
            mac.update(pw.as_bytes());
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()).into_bytes()
        }
    }
}

fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).map_err(|e| format!("no random source: {}", e))?;
    Ok(buf)
}

// ---------------------------------------------------------------------
// hash / verify
// ---------------------------------------------------------------------

/// Hashes with `s` (runs on the calling thread). `Err` is a plain message.
fn hash_with(pw: &str, s: &Spec) -> Result<String, String> {
    let input = secret(pw, s.pepper.as_deref());
    let salt_bytes = random(16)?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| e.to_string())?;
    match s.algo {
        Algo::Argon2(alg) => {
            let params = argon2::Params::new(s.memory, s.time, s.parallelism, None).map_err(|e| format!("argon2: {}", e))?;
            let h = argon2::Argon2::new(alg, argon2::Version::V0x13, params)
                .hash_password(&input, &salt)
                .map_err(|e| format!("argon2: {}", e))?;
            Ok(h.to_string())
        }
        Algo::Bcrypt => {
            if input.len() > 72 {
                return Err(format!(
                    "bcrypt: the password is {} bytes, more than bcrypt's 72 (use argon2id, or a pepper, which pre-hashes it)",
                    input.len()
                ));
            }
            let salt16: [u8; 16] = salt_bytes.as_slice().try_into().expect("16-byte salt");
            let parts = bcrypt::hash_with_salt(&input, s.cost, salt16).map_err(|e| format!("bcrypt: {}", e))?;
            Ok(parts.format_for_version(bcrypt::Version::TwoB))
        }
        Algo::Scrypt => {
            let params = scrypt::Params::new(s.log_n, s.r, s.p, 32).map_err(|e| format!("scrypt: {}", e))?;
            let h = scrypt::Scrypt
                .hash_password_customized(&input, None, None, params, &salt)
                .map_err(|e| format!("scrypt: {}", e))?;
            Ok(h.to_string())
        }
        Algo::Pbkdf2Sha256 | Algo::Pbkdf2Sha512 => {
            let alg = if s.algo == Algo::Pbkdf2Sha256 { pbkdf2::Algorithm::Pbkdf2Sha256 } else { pbkdf2::Algorithm::Pbkdf2Sha512 };
            let params = pbkdf2::Params { rounds: s.iterations, output_length: 32 };
            let h = pbkdf2::Pbkdf2
                .hash_password_customized(&input, Some(alg.ident()), None, params, &salt)
                .map_err(|e| format!("pbkdf2: {}", e))?;
            Ok(h.to_string())
        }
    }
}

/// A stored hash, without its secret parts.
#[derive(Clone, Debug)]
enum Parsed {
    Argon2 { algo: Algo, version: u32, memory: u32, time: u32, parallelism: u32, length: usize },
    Bcrypt { version: String, cost: u32 },
    Scrypt { log_n: u8, r: u32, p: u32, length: usize },
    Pbkdf2 { algo: Algo, iterations: u32, length: usize },
}

impl Parsed {
    fn algo(&self) -> Algo {
        match self {
            Parsed::Argon2 { algo, .. } | Parsed::Pbkdf2 { algo, .. } => *algo,
            Parsed::Bcrypt { .. } => Algo::Bcrypt,
            Parsed::Scrypt { .. } => Algo::Scrypt,
        }
    }
}

/// Recognizes `$argon2id$…`, `$2b$…` (`$2a$`, `$2y$`), `$scrypt$…`, `$pbkdf2-sha256$…` / `$pbkdf2-sha512$…`.
fn parse(stored: &str) -> Result<Parsed, String> {
    let bad = |why: &str| format!("not a valid password hash ({})", why);
    if let Some(rest) = stored.strip_prefix("$2") {
        // $2b$12$ + 22 salt chars + 31 hash chars
        let b = stored.as_bytes();
        if b.len() != 60 || !stored.is_ascii() || b[3] != b'$' || b[6] != b'$' {
            return Err(bad("malformed bcrypt hash"));
        }
        let version = format!("2{}", &rest[..1]);
        if !matches!(version.as_str(), "2a" | "2b" | "2y") {
            return Err(bad(&format!("unsupported bcrypt version ${}$", version)));
        }
        let cost: u32 = stored[4..6].parse().map_err(|_| bad("malformed bcrypt cost"))?;
        const B64: &str = "./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        if !(4..=31).contains(&cost) || !stored[7..].chars().all(|c| B64.contains(c)) {
            return Err(bad("malformed bcrypt hash"));
        }
        return Ok(Parsed::Bcrypt { version, cost });
    }
    let h = PasswordHash::new(stored).map_err(|e| bad(&e.to_string()))?;
    let length = h.hash.map(|o| o.len()).unwrap_or(0);
    if h.hash.is_none() || h.salt.is_none() {
        return Err(bad("no salt or hash"));
    }
    let alg = h.algorithm.as_str();
    match Algo::parse(alg) {
        Some(algo @ Algo::Argon2(_)) if alg.starts_with("argon2") && alg != "argon2" => {
            let p = argon2::Params::try_from(&h).map_err(|e| bad(&e.to_string()))?;
            let version = h.version.unwrap_or(0x13);
            argon2::Version::try_from(version).map_err(|e| bad(&e.to_string()))?;
            Ok(Parsed::Argon2 { algo, version, memory: p.m_cost(), time: p.t_cost(), parallelism: p.p_cost(), length })
        }
        Some(Algo::Scrypt) => {
            let p = scrypt::Params::try_from(&h).map_err(|e| bad(&e.to_string()))?;
            if scrypt_memory(p.log_n(), p.r()) > SCRYPT_MAX_MEMORY {
                return Err(bad("scrypt parameters need more than 2 GiB of memory"));
            }
            Ok(Parsed::Scrypt { log_n: p.log_n(), r: p.r(), p: p.p(), length })
        }
        Some(algo @ (Algo::Pbkdf2Sha256 | Algo::Pbkdf2Sha512)) if alg != "pbkdf2" => {
            let p = pbkdf2::Params::try_from(&h).map_err(|e| bad(&e.to_string()))?;
            Ok(Parsed::Pbkdf2 { algo, iterations: p.rounds, length })
        }
        _ => Err(bad(&format!("unsupported algorithm '{}'", alg))),
    }
}

/// Checks `pw` against `stored` (constant-time comparison in every algorithm).
/// `Err` is a plain message (a malformed or unsupported hash).
fn verify_with(pw: &str, stored: &str, pepper: Option<&[u8]>) -> Result<bool, String> {
    let parsed = parse(stored)?;
    let input = secret(pw, pepper);
    let outcome = match parsed {
        Parsed::Bcrypt { .. } => {
            // never truncate: a longer password cannot match a hash made by this module
            if input.len() > 72 {
                return Ok(false);
            }
            return bcrypt::verify(&input, stored).map_err(|e| format!("not a valid password hash ({})", e));
        }
        Parsed::Argon2 { .. } => argon2::Argon2::default().verify_password(&input, &PasswordHash::new(stored).expect("parsed")),
        Parsed::Scrypt { .. } => scrypt::Scrypt.verify_password(&input, &PasswordHash::new(stored).expect("parsed")),
        Parsed::Pbkdf2 { .. } => pbkdf2::Pbkdf2.verify_password(&input, &PasswordHash::new(stored).expect("parsed")),
    };
    match outcome {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(e) => Err(format!("not a valid password hash ({})", e)),
    }
}

fn needs_rehash(p: &Parsed, s: &Spec) -> bool {
    if p.algo() != s.algo {
        return true;
    }
    match p {
        Parsed::Argon2 { version, memory, time, parallelism, length, .. } => {
            *version < 0x13 || *memory < s.memory || *time < s.time || *parallelism < s.parallelism || *length < 32
        }
        Parsed::Bcrypt { version, cost } => version == "2a" || *cost < s.cost,
        Parsed::Scrypt { log_n, r, p, length } => *log_n < s.log_n || *r < s.r || *p < s.p || *length < 32,
        Parsed::Pbkdf2 { iterations, length, .. } => *iterations < s.iterations || *length < 32,
    }
}

/// `password.hash` with the default parameters (used by `crypto.hashPassword`).
pub fn hash_default(pw: String, what: &str) -> Result<Value, String> {
    let s = Spec::default();
    let r = crate::native::rt::offload(move || hash_with(&pw, &s))?;
    r.map(Value::str).map_err(|e| io_err!("{}: {}", what, e))
}

/// `password.verify` without options (used by `crypto.verifyPassword`).
pub fn verify_default(pw: String, stored: String, what: &str) -> Result<Value, String> {
    let r = crate::native::rt::offload(move || verify_with(&pw, &stored, None))?;
    r.map(Value::Bool).map_err(|e| io_err!("{}: {}", what, e))
}

/// hash(password, { algorithm, memory, time, parallelism, cost, n, r, p, iterations, pepper }) → str
fn hash_fn(a: &[Value]) -> Result<Value, String> {
    const W: &str = "password.hash";
    max_args(a, 2, W)?;
    let pw = str_arg(a, 0, W)?;
    let s = spec(opt_arg(a, 1), W)?;
    let r = crate::native::rt::offload(move || hash_with(&pw, &s))?;
    r.map(Value::str).map_err(|e| io_err!("{}: {}", W, e))
}

/// verify(password, hash, { pepper }) → bool · error (malformed hash)
fn verify_fn(a: &[Value]) -> Result<Value, String> {
    const W: &str = "password.verify";
    max_args(a, 3, W)?;
    let pw = str_arg(a, 0, W)?;
    let stored = str_arg(a, 1, W)?;
    // the same options as `hash` are accepted (one config map for both); only the pepper matters
    let pepper = spec(opt_arg(a, 2), W)?.pepper;
    let r = crate::native::rt::offload(move || verify_with(&pw, &stored, pepper.as_deref()))?;
    r.map(Value::Bool).map_err(|e| io_err!("{}: {}", W, e))
}

/// needsRehash(hash, options?) → bool · error
fn needs_rehash_fn(a: &[Value]) -> Result<Value, String> {
    const W: &str = "password.needsRehash";
    max_args(a, 2, W)?;
    let stored = str_arg(a, 0, W)?;
    let s = spec(opt_arg(a, 1), W)?;
    let p = parse(&stored).map_err(|e| io_err!("{}: {}", W, e))?;
    Ok(Value::Bool(needs_rehash(&p, &s)))
}

/// info(hash) → { algorithm, version, ...parameters } · error
fn info_fn(a: &[Value]) -> Result<Value, String> {
    const W: &str = "password.info";
    max_args(a, 1, W)?;
    let stored = str_arg(a, 0, W)?;
    let p = parse(&stored).map_err(|e| io_err!("{}: {}", W, e))?;
    let int = |n: u64| Value::Int(n as i64);
    Ok(match p {
        Parsed::Argon2 { algo, version, memory, time, parallelism, length } => map(vec![
            ("algorithm", Value::str(algo.name())),
            ("version", int(version as u64)),
            ("memory", int(memory as u64)),
            ("time", int(time as u64)),
            ("parallelism", int(parallelism as u64)),
            ("length", int(length as u64)),
        ]),
        Parsed::Bcrypt { version, cost } => map(vec![
            ("algorithm", Value::str("bcrypt")),
            ("version", Value::str(version)),
            ("cost", int(cost as u64)),
        ]),
        Parsed::Scrypt { log_n, r, p, length } => map(vec![
            ("algorithm", Value::str("scrypt")),
            ("version", Value::Nil),
            ("n", int(1u64 << log_n)),
            ("r", int(r as u64)),
            ("p", int(p as u64)),
            ("length", int(length as u64)),
        ]),
        Parsed::Pbkdf2 { algo, iterations, length } => map(vec![
            ("algorithm", Value::str(algo.name())),
            ("version", Value::Nil),
            ("iterations", int(iterations as u64)),
            ("length", int(length as u64)),
        ]),
    })
}

// ---------------------------------------------------------------------
// random passwords and passphrases
// ---------------------------------------------------------------------

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?/~";
const AMBIGUOUS: &str = "Il1O0o|`'\"";

/// A uniform index in `0..n` from the OS CSPRNG (rejection sampling, no modulo bias).
fn uniform(n: usize) -> Result<usize, String> {
    let n = n as u64;
    let zone = u64::MAX - (u64::MAX % n);
    loop {
        let r = u64::from_le_bytes(random(8)?.try_into().expect("8 bytes"));
        if r < zone {
            return Ok((r % n) as usize);
        }
    }
}

/// generate(length = 16, { upper, lower, digits, symbols, excludeAmbiguous }) → str
fn generate_fn(a: &[Value]) -> Result<Value, String> {
    const W: &str = "password.generate";
    max_args(a, 2, W)?;
    let length = match opt_arg(a, 0) {
        None => 16,
        Some(Value::Int(n)) if (1..=4096).contains(n) => *n as usize,
        Some(Value::Int(n)) => return Err(format!("{}: length must be between 1 and 4096, got {}", W, n)),
        Some(other) => return Err(format!("{}: length must be an int, got '{}'", W, other.type_name())),
    };
    let o = opts(opt_arg(a, 1), &["upper", "lower", "digits", "symbols", "excludeAmbiguous"], W)?;
    let ambiguous = opt_bool(&o, "excludeAmbiguous", false, W)?;
    let mut classes: Vec<Vec<char>> = Vec::new();
    for (k, set) in [("lower", LOWER), ("upper", UPPER), ("digits", DIGITS)] {
        if opt_bool(&o, k, true, W)? {
            classes.push(set.chars().collect());
        }
    }
    match o.get("symbols") {
        Some(Value::Str(s)) if !s.is_empty() => classes.push(s.chars().collect()),
        Some(Value::Str(_)) => return Err(format!("{}: option 'symbols' must not be empty", W)),
        _ => {
            if opt_bool(&o, "symbols", true, W)? {
                classes.push(SYMBOLS.chars().collect());
            }
        }
    }
    for c in classes.iter_mut() {
        if ambiguous {
            c.retain(|ch| !AMBIGUOUS.contains(*ch));
        }
        c.dedup();
    }
    classes.retain(|c| !c.is_empty());
    if classes.is_empty() {
        return Err(format!("{}: every character class is disabled", W));
    }
    if length < classes.len() {
        return Err(format!("{}: length {} is shorter than the {} character classes chosen", W, length, classes.len()));
    }
    let mut all: Vec<char> = classes.iter().flatten().copied().collect();
    all.sort_unstable();
    all.dedup();
    // one of each class, the rest from all of them, then a Fisher-Yates shuffle
    let mut out: Vec<char> = Vec::with_capacity(length);
    for c in &classes {
        out.push(c[uniform(c.len())?]);
    }
    while out.len() < length {
        out.push(all[uniform(all.len())?]);
    }
    for i in (1..out.len()).rev() {
        out.swap(i, uniform(i + 1)?);
    }
    Ok(Value::str(out.into_iter().collect::<String>()))
}

fn words() -> &'static [&'static str] {
    static W: OnceLock<Vec<&'static str>> = OnceLock::new();
    W.get_or_init(|| include_str!("password_words.txt").split_whitespace().collect())
}

/// passphrase(words = 5, sep = "-") → str (words from an embedded list of ~1150, ≈10 bits each)
fn passphrase_fn(a: &[Value]) -> Result<Value, String> {
    const W: &str = "password.passphrase";
    max_args(a, 2, W)?;
    let n = match opt_arg(a, 0) {
        None => 5,
        Some(Value::Int(n)) if (1..=64).contains(n) => *n as usize,
        Some(Value::Int(n)) => return Err(format!("{}: words must be between 1 and 64, got {}", W, n)),
        Some(other) => return Err(format!("{}: words must be an int, got '{}'", W, other.type_name())),
    };
    let sep = match opt_arg(a, 1) {
        None => "-".to_string(),
        Some(_) => str_arg(a, 1, W)?,
    };
    let list = words();
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(list[uniform(list.len())?]);
    }
    Ok(Value::str(out.join(&sep)))
}

// ---------------------------------------------------------------------
// strength
// ---------------------------------------------------------------------

/// The result of either estimator (zxcvbn, or the built-in entropy estimate).
struct Strength {
    score: u8,
    guesses: f64,
    warning: Option<String>,
    suggestions: Vec<String>,
    estimator: &'static str,
}

/// zxcvbn's thresholds, shared by both estimators.
#[cfg_attr(all(feature = "network", not(target_family = "wasm")), allow(dead_code))] // the fallback estimator (the tests compare it with zxcvbn)
fn score_of(guesses: f64) -> u8 {
    const DELTA: f64 = 5.0;
    if guesses < 1e3 + DELTA {
        0
    } else if guesses < 1e6 + DELTA {
        1
    } else if guesses < 1e8 + DELTA {
        2
    } else if guesses < 1e10 + DELTA {
        3
    } else {
        4
    }
}

/// zxcvbn's wording for a crack time.
fn display_time(seconds: f64) -> String {
    const MINUTE: f64 = 60.0;
    const HOUR: f64 = MINUTE * 60.0;
    const DAY: f64 = HOUR * 24.0;
    const MONTH: f64 = DAY * 31.0;
    const YEAR: f64 = MONTH * 12.0;
    const CENTURY: f64 = YEAR * 100.0;
    let unit = |base: f64, name: &str| {
        let b = base.floor() as u64;
        format!("{} {}{}", b, name, if b > 1 { "s" } else { "" })
    };
    if seconds < 1.0 {
        "less than a second".into()
    } else if seconds < MINUTE {
        unit(seconds, "second")
    } else if seconds < HOUR {
        unit(seconds / MINUTE, "minute")
    } else if seconds < DAY {
        unit(seconds / HOUR, "hour")
    } else if seconds < MONTH {
        unit(seconds / DAY, "day")
    } else if seconds < YEAR {
        unit(seconds / MONTH, "month")
    } else if seconds < CENTURY {
        unit(seconds / YEAR, "year")
    } else {
        "centuries".into()
    }
}

#[cfg(all(feature = "network", not(target_family = "wasm")))]
fn estimate(pw: &str, inputs: &[String]) -> Strength {
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let e = zxcvbn::zxcvbn(pw, &refs);
    let fb = e.feedback();
    Strength {
        score: u8::from(e.score()),
        guesses: e.guesses() as f64,
        warning: fb.and_then(|f| f.warning()).map(|w| w.to_string()),
        suggestions: fb.map(|f| f.suggestions().iter().map(|s| s.to_string()).collect()).unwrap_or_default(),
        estimator: "zxcvbn",
    }
}

#[cfg(not(all(feature = "network", not(target_family = "wasm"))))]
fn estimate(pw: &str, inputs: &[String]) -> Strength {
    estimate_entropy(pw, inputs)
}

/// The most common passwords (lowercase): a few of the top of the public breach lists.
#[cfg_attr(all(feature = "network", not(target_family = "wasm")), allow(dead_code))] // the fallback estimator (the tests compare it with zxcvbn)
const COMMON: &[&str] = &[
    "123456", "password", "12345678", "qwerty", "123456789", "12345", "1234", "111111", "1234567", "dragon",
    "123123", "baseball", "abc123", "football", "monkey", "letmein", "696969", "shadow", "master", "666666",
    "qwertyuiop", "123321", "mustang", "1234567890", "michael", "654321", "superman", "1qaz2wsx", "7777777", "121212",
    "000000", "qazwsx", "123qwe", "killer", "trustno1", "jordan", "jennifer", "zxcvbnm", "asdfgh", "hunter",
    "buster", "soccer", "harley", "batman", "andrew", "tigger", "sunshine", "iloveyou", "2000", "charlie",
    "robert", "thomas", "hockey", "ranger", "daniel", "starwars", "klaster", "112233", "george", "computer",
    "michelle", "jessica", "pepper", "1111", "zxcvbn", "555555", "11111111", "131313", "freedom", "777777",
    "pass", "maggie", "159753", "aaaaaa", "ginger", "princess", "joshua", "cheese", "amanda", "summer",
    "love", "ashley", "nicole", "chelsea", "biteme", "matthew", "access", "yankees", "987654321", "dallas",
    "austin", "thunder", "taylor", "matrix", "admin", "welcome", "login", "passw0rd", "hunter2", "changeme",
];

/// Built-in estimate (used when zxcvbn is not compiled in): character-pool entropy with
/// discounts for common passwords, dictionary words, user inputs, repeats and sequences.
/// It follows zxcvbn's scale (guesses → score 0..4), so both paths agree on clear cases.
#[cfg_attr(all(feature = "network", not(target_family = "wasm")), allow(dead_code))] // the fallback estimator (the tests compare it with zxcvbn)
fn estimate_entropy(pw: &str, inputs: &[String]) -> Strength {
    let lower = pw.to_lowercase();
    let mut warning: Option<String> = None;
    let mut suggestions: Vec<String> = Vec::new();
    // strip the leetspeak and the usual decorations around a word
    let unleet: String = lower
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' | '!' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            c => c,
        })
        .collect();
    let core = lower.trim_end_matches(|c: char| c.is_ascii_digit() || c.is_ascii_punctuation());
    let tail = lower.len() - core.len();
    if let Some(rank) = COMMON.iter().position(|c| *c == lower) {
        let guesses = (rank + 2) as f64;
        return finish(guesses, Some(if rank < 10 { "This is a top-10 common password." } else { "This is a top-100 common password." }.into()),
            vec!["Add another word or two. Uncommon words are better.".into()]);
    }
    if COMMON.contains(&unleet.as_str()) || (!core.is_empty() && COMMON.contains(&core)) {
        let guesses = 100.0 * 10f64.powi(tail as i32).max(1.0) * 4.0;
        return finish(guesses, Some("This is similar to a commonly used password.".into()),
            vec!["Add another word or two. Uncommon words are better.".into(), "Predictable substitutions like '@' instead of 'a' don't help very much.".into()]);
    }
    if !core.is_empty() && (words().contains(&core) || inputs.iter().any(|i| i.to_lowercase() == core)) {
        let guesses = words().len() as f64 * 10f64.powi(tail as i32).max(1.0) * 2.0;
        warning = Some("A word by itself is easy to guess.".into());
        suggestions.push("Add another word or two. Uncommon words are better.".into());
        return finish(guesses, warning, suggestions);
    }
    // character pool
    let chars: Vec<char> = pw.chars().collect();
    let mut pool = 0u32;
    if chars.iter().any(|c| c.is_ascii_lowercase()) {
        pool += 26;
    }
    if chars.iter().any(|c| c.is_ascii_uppercase()) {
        pool += 26;
    }
    if chars.iter().any(|c| c.is_ascii_digit()) {
        pool += 10;
    }
    if chars.iter().any(|c| c.is_ascii_punctuation() || *c == ' ') {
        pool += 33;
    }
    if chars.iter().any(|c| !c.is_ascii()) {
        pool += 100;
    }
    let per_char = (pool.max(1) as f64).log2();
    // repeats (aaa) and sequences (abc, 123, cba) add almost nothing
    let (mut bits, mut repeats, mut sequences) = (0.0, false, false);
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && chars[i - 1] == *c {
            repeats = true;
            bits += 1.0;
        } else if i > 0 && ((*c as i64) - (chars[i - 1] as i64)).abs() == 1 {
            sequences = true;
            bits += 1.0;
        } else {
            bits += per_char;
        }
    }
    // a user input inside the password counts as a single guess from a small list
    for inp in inputs.iter().map(|s| s.to_lowercase()).filter(|s| s.chars().count() >= 3) {
        if lower.contains(&inp) {
            bits -= (inp.chars().count() as f64 * per_char - 10.0).max(0.0);
        }
    }
    let guesses = 2f64.powf(bits.max(0.0)).max(1.0);
    if repeats && warning.is_none() {
        warning = Some("Repeats like \"aaa\" are easy to guess.".into());
        suggestions.push("Avoid repeated words and characters.".into());
    }
    if sequences && warning.is_none() {
        warning = Some("Sequences like abc or 6543 are easy to guess.".into());
        suggestions.push("Avoid sequences.".into());
    }
    if score_of(guesses) < 3 {
        suggestions.push("Add another word or two. Uncommon words are better.".into());
        if chars.len() < 12 {
            suggestions.push("Use a longer password: 12 characters or more.".into());
        }
    }
    finish(guesses, warning, suggestions)
}

#[cfg_attr(all(feature = "network", not(target_family = "wasm")), allow(dead_code))] // the fallback estimator (the tests compare it with zxcvbn)
fn finish(guesses: f64, warning: Option<String>, suggestions: Vec<String>) -> Strength {
    Strength { score: score_of(guesses), guesses, warning, suggestions, estimator: "entropy" }
}

fn strength_value(s: Strength) -> Value {
    map(vec![
        ("score", Value::Int(s.score as i64)),
        ("guesses", Value::Int(s.guesses.min(i64::MAX as f64) as i64)),
        ("bits", Value::Float((s.guesses.max(1.0).log2() * 10.0).round() / 10.0)),
        ("crackTime", Value::str(display_time(s.guesses / 1e4))),
        ("crackTimeOnline", Value::str(display_time(s.guesses / (100.0 / 3600.0)))),
        ("warning", s.warning.map(Value::str).unwrap_or(Value::Nil)),
        ("suggestions", list(s.suggestions.into_iter().map(Value::str).collect())),
        ("estimator", Value::str(s.estimator)),
    ])
}

/// strength(password, [user inputs]) → { score 0..4, guesses, bits, crackTime, crackTimeOnline,
/// warning, suggestions, estimator } (also `security.passwordStrength`)
pub fn strength_fn(a: &[Value], what: &str) -> Result<Value, String> {
    max_args(a, 2, what)?;
    let pw = str_arg(a, 0, what)?;
    let inputs: Vec<String> = match opt_arg(a, 1) {
        None => Vec::new(),
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        Some(other) => return Err(format!("{}: user inputs must be a list, got '{}'", what, other.type_name())),
    };
    Ok(strength_value(estimate(&pw, &inputs)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast(algo: &str) -> Spec {
        let mut s = Spec { algo: Algo::parse(algo).unwrap(), ..Spec::default() };
        s.memory = 64;
        s.time = 1;
        s.cost = 4;
        s.log_n = 4;
        s.iterations = 1000;
        s
    }

    #[test]
    fn every_algorithm_round_trips() {
        for alg in ALGORITHMS {
            let s = fast(alg);
            let h = hash_with("hunter2", &s).unwrap();
            assert_eq!(parse(&h).unwrap().algo().name(), alg, "{}", h);
            assert!(verify_with("hunter2", &h, None).unwrap(), "{}", h);
            assert!(!verify_with("hunter3", &h, None).unwrap(), "{}", h);
            assert!(!needs_rehash(&parse(&h).unwrap(), &s));
            assert!(needs_rehash(&parse(&h).unwrap(), &Spec::default()) || alg == "argon2id");
        }
    }

    #[test]
    fn pepper_and_bcrypt_limit() {
        let mut s = fast("bcrypt");
        let long = "x".repeat(100);
        assert!(hash_with(&long, &s).is_err());
        s.pepper = Some(b"secret".to_vec());
        let h = hash_with(&long, &s).unwrap();
        assert!(verify_with(&long, &h, Some(b"secret")).unwrap());
        assert!(!verify_with(&long, &h, None).unwrap());
        assert!(!verify_with(&long, &h, Some(b"other")).unwrap());
    }

    #[test]
    fn known_vectors() {
        // bcrypt, from the OpenBSD test suite
        assert!(verify_with("U*U", "$2a$05$CCCCCCCCCCCCCCCCCCCCC.E5YPO9kmyuRGyh0XouQYb4YMJKvyOeW", None).unwrap());
        assert!(parse("$2b$12$short").is_err());
        assert!(parse("plain text").is_err());
        assert!(parse("$pbkdf2$i=1000,l=32$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g").is_err());
    }

    #[test]
    fn the_two_estimators_agree_on_clear_cases() {
        for (pw, lo, hi) in [("password", 0, 0), ("123456", 0, 0), ("hunter", 0, 1), ("Tr0ub4dour&3xQ!vZ#82kLp", 4, 4), ("correct horse battery staple 9!", 4, 4)] {
            let e = estimate_entropy(pw, &[]);
            assert!((lo..=hi).contains(&e.score), "{} → {}", pw, e.score);
            let z = estimate(pw, &[]);
            assert!((lo..=hi).contains(&z.score), "{} → {} ({})", pw, z.score, z.estimator);
        }
    }

    #[test]
    fn generate_covers_every_class() {
        for _ in 0..50 {
            let Value::Str(s) = generate_fn(&[Value::Int(4)]).unwrap() else { panic!() };
            assert_eq!(s.chars().count(), 4);
            assert!(s.chars().any(|c| c.is_ascii_lowercase()));
            assert!(s.chars().any(|c| c.is_ascii_uppercase()));
            assert!(s.chars().any(|c| c.is_ascii_digit()));
            assert!(s.chars().any(|c| SYMBOLS.contains(c)));
        }
    }
}
