//! `fogu:crypto` module — hashes, HMAC, authenticated encryption, password
//! hashing, secure random values and encodings.
//!
//! ```fogu
//! import crypto from "fogu:crypto"
//!
//! print(crypto.hash("sha256", "hello"))            // hex digest (md5 sha1 sha256 sha384 sha512)
//! print(crypto.hmac("sha256", "key", "message"))
//! const box = crypto.encrypt("secret data", "my password")      // AES-256-GCM, base64 text
//! print(crypto.decrypt(box, "my password")?)        // error value with a wrong key
//! const h = crypto.hashPassword("hunter2")           // argon2id (PHC string)
//! print(crypto.verifyPassword("hunter2", h))         // true
//! print(crypto.uuid(), crypto.randomHex(16), crypto.randomInt(1, 6))
//! print(crypto.uuidv7(), crypto.uuidv5("dns", "example.com"))  // time-ordered · name-based
//! print(crypto.base64("olá"), crypto.fromBase64("b2zDoQ=="))
//! ```

use std::cell::RefCell;
use ahash::AHashMap as HashMap;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Digest;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::util::{list, map, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    let mut f = |name: &str, min: usize, func: fn(&[Value]) -> Result<Value, String>| {
        e.insert(name.to_string(), nfn_var(name, min, func));
    };
    f("hash", 2, hash);
    f("hmac", 3, hmac_fn);
    f("encrypt", 2, encrypt);
    f("decrypt", 2, decrypt);
    f("hashPassword", 1, hash_password);
    f("verifyPassword", 2, verify_password);
    f("randomBytes", 1, |a| {
        max_args(a, 1, "randomBytes")?;
        Ok(list(random(count(a, "randomBytes")?)?.into_iter().map(|b| Value::Int(b as i64)).collect()))
    });
    f("randomHex", 1, |a| {
        max_args(a, 1, "randomHex")?;
        Ok(Value::Str(hex(&random(count(a, "randomHex")?)?).into()))
    });
    f("randomInt", 2, random_int);
    // uuid(version = 4, namespace?, name?) · uuidv4() · uuidv5(namespace, name) · uuidv7()
    f("uuid", 0, |a| {
        max_args(a, 3, "uuid")?;
        match a.first() {
            None | Some(Value::Nil) | Some(Value::Int(4)) => {
                max_args(a, 1, "uuid")?;
                Ok(uuid_value(uuid::Uuid::new_v4()))
            }
            Some(Value::Int(5)) => uuid_v5(&a[1..], "uuid"),
            Some(Value::Int(7)) => {
                max_args(a, 1, "uuid")?;
                Ok(uuid_value(uuid::Uuid::now_v7()))
            }
            Some(other) => Err(format!(
                "uuid: version must be 4, 5 or 7, got {}",
                other.to_display_string()
            )),
        }
    });
    f("uuidv4", 0, |a| {
        max_args(a, 0, "uuidv4")?;
        Ok(uuid_value(uuid::Uuid::new_v4()))
    });
    f("uuidv5", 2, |a| uuid_v5(a, "uuidv5"));
    f("uuidv7", 0, |a| {
        max_args(a, 0, "uuidv7")?;
        Ok(uuid_value(uuid::Uuid::now_v7()))
    });
    f("uuidInfo", 1, uuid_info);
    f("base64", 1, |a| {
        max_args(a, 2, "base64")?;
        let url = matches!(a.get(1), Some(Value::Map(m)) if m.borrow().get("url").is_some_and(|v| v.is_truthy()));
        let data = bytes_arg(&a[0], "base64")?;
        Ok(Value::Str(if url {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data).into()
        } else {
            base64::engine::general_purpose::STANDARD.encode(data).into()
        }))
    });
    f("fromBase64", 1, |a| {
        max_args(a, 2, "fromBase64")?;
        let s = str_arg(a, 0, "fromBase64")?;
        let clean: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = if clean.contains(['-', '_']) {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(clean.trim_end_matches('='))
        } else {
            base64::engine::general_purpose::STANDARD.decode(&clean)
        }
        .map_err(|e| io_err!("crypto.fromBase64: {}", e))?;
        as_text_or_bytes(bytes, a.get(1))
    });
    f("hex", 1, |a| {
        max_args(a, 1, "hex")?;
        Ok(Value::Str(hex(&bytes_arg(&a[0], "hex")?).into()))
    });
    f("fromHex", 1, |a| {
        max_args(a, 2, "fromHex")?;
        let bytes = unhex(&str_arg(a, 0, "fromHex")?).ok_or_else(|| io_err!("crypto.fromHex: invalid hex"))?;
        as_text_or_bytes(bytes, a.get(1))
    });
    f("equal", 2, |a| {
        max_args(a, 2, "equal")?;
        let (x, y) = (bytes_arg(&a[0], "equal")?, bytes_arg(&a[1], "equal")?);
        // constant time for equal lengths
        Ok(Value::Bool(x.len() == y.len() && x.iter().zip(&y).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0))
    });
    ModuleValue {
        name: "fogu:crypto".into(),
        exports: RefCell::new(e),
    }
}

fn uuid_value(u: uuid::Uuid) -> Value {
    Value::Str(u.hyphenated().to_string().into())
}

/// Version 5: SHA-1 of a namespace and a name — the same inputs always give the
/// same UUID. The namespace is `"dns"`, `"url"`, `"oid"`, `"x500"` or a UUID.
fn uuid_v5(a: &[Value], what: &str) -> Result<Value, String> {
    if a.len() != 2 {
        return Err(format!("{what}: expects a namespace and a name"));
    }
    let ns_text = str_arg(a, 0, what)?;
    let ns = match ns_text.to_ascii_lowercase().as_str() {
        "dns" => uuid::Uuid::NAMESPACE_DNS,
        "url" => uuid::Uuid::NAMESPACE_URL,
        "oid" => uuid::Uuid::NAMESPACE_OID,
        "x500" => uuid::Uuid::NAMESPACE_X500,
        other => uuid::Uuid::parse_str(other).map_err(|_| {
            format!("{what}: namespace must be \"dns\", \"url\", \"oid\", \"x500\" or a UUID, got '{ns_text}'")
        })?,
    };
    let name = bytes_arg(&a[1], what)?;
    Ok(uuid_value(uuid::Uuid::new_v5(&ns, &name)))
}

/// `uuidInfo(text)` → `{ version, variant, time }`; `time` is the creation time in
/// milliseconds since the epoch for time-based UUIDs (v1, v6, v7), else nil.
/// Text that is not a UUID is an error value (it usually comes from outside).
fn uuid_info(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "uuidInfo")?;
    let text = str_arg(a, 0, "uuidInfo")?;
    let u = match uuid::Uuid::parse_str(text.trim()) {
        Ok(u) => u,
        Err(e) => return Err(io_err!("uuidInfo: '{}' is not a UUID: {}", text, e)),
    };
    let variant = match u.get_variant() {
        uuid::Variant::NCS => "ncs",
        uuid::Variant::RFC4122 => "rfc4122",
        uuid::Variant::Microsoft => "microsoft",
        _ => "future",
    };
    let time = match u.get_timestamp() {
        Some(ts) => {
            let (secs, nanos) = ts.to_unix();
            Value::Int(secs as i64 * 1000 + (nanos / 1_000_000) as i64)
        }
        None => Value::Nil,
    };
    Ok(map(vec![
        ("version", Value::Int(u.get_version_num() as i64)),
        ("variant", Value::Str(variant.into())),
        ("time", time),
    ]))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// A str (its UTF-8 bytes) or a list of byte ints.
/// Data as bytes: a str (UTF-8), a buffer or a list of byte ints.
fn bytes_arg(v: &Value, what: &str) -> Result<Vec<u8>, String> {
    crate::native::util::bytes_of(v, what)
}

/// Decoded data as text, or as bytes with `{ bytes: true }` (or when not UTF-8).
fn as_text_or_bytes(bytes: Vec<u8>, opts: Option<&Value>) -> Result<Value, String> {
    let want_bytes = matches!(opts, Some(Value::Map(m)) if m.borrow().get("bytes").is_some_and(|v| v.is_truthy()));
    if !want_bytes {
        if let Ok(s) = String::from_utf8(bytes.clone()) {
            return Ok(Value::Str(s.into()));
        }
    }
    Ok(list(bytes.into_iter().map(|b| Value::Int(b as i64)).collect()))
}

fn count(a: &[Value], what: &str) -> Result<usize, String> {
    match a.first() {
        Some(Value::Int(n)) if (1..=1_048_576).contains(n) => Ok(*n as usize),
        _ => Err(format!("{}: expected a size between 1 and 1048576", what)),
    }
}

fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).map_err(|e| io_err!("crypto: no random source: {}", e))?;
    Ok(buf)
}

fn random_int(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "randomInt")?;
    let (lo, hi) = match (&a[0], &a[1]) {
        (Value::Int(lo), Value::Int(hi)) if lo <= hi => (*lo, *hi),
        _ => return Err("randomInt(min, max) expects ints with min <= max (inclusive)".into()),
    };
    let span = (hi as i128 - lo as i128 + 1) as u128;
    // rejection sampling: no modulo bias
    let zone = u128::from(u64::MAX) + 1 - ((u128::from(u64::MAX) + 1) % span);
    loop {
        let r = u64::from_le_bytes(random(8)?.try_into().unwrap()) as u128;
        if r < zone {
            return Ok(Value::Int((lo as i128 + (r % span) as i128) as i64));
        }
    }
}

fn digest(algo: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(match algo.to_lowercase().replace('-', "").as_str() {
        "md5" => md5::Md5::digest(data).to_vec(),
        "sha1" => sha1::Sha1::digest(data).to_vec(),
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha384" => sha2::Sha384::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        other => return Err(format!("crypto: unknown hash '{}' (md5 sha1 sha256 sha384 sha512)", other)),
    })
}

/// `hash(algo, data, { base64 })` → hex (or base64)
fn hash(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "hash")?;
    let d = digest(&str_arg(a, 0, "hash")?, &bytes_arg(&a[1], "hash")?)?;
    Ok(Value::Str(encode_out(&d, a.get(2)).into()))
}

fn encode_out(d: &[u8], opts: Option<&Value>) -> String {
    if matches!(opts, Some(Value::Map(m)) if m.borrow().get("base64").is_some_and(|v| v.is_truthy())) {
        base64::engine::general_purpose::STANDARD.encode(d)
    } else {
        hex(d)
    }
}

/// `hmac(algo, key, data, { base64 })`
fn hmac_fn(a: &[Value]) -> Result<Value, String> {
    max_args(a, 4, "hmac")?;
    let algo = str_arg(a, 0, "hmac")?.to_lowercase().replace('-', "");
    let key = bytes_arg(&a[1], "hmac")?;
    let data = bytes_arg(&a[2], "hmac")?;
    macro_rules! run {
        ($h:ty) => {{
            let mut m = <Hmac<$h> as Mac>::new_from_slice(&key).expect("HMAC accepts any key length");
            m.update(&data);
            m.finalize().into_bytes().to_vec()
        }};
    }
    let out = match algo.as_str() {
        "sha1" => run!(sha1::Sha1),
        "sha256" => run!(sha2::Sha256),
        "sha384" => run!(sha2::Sha384),
        "sha512" => run!(sha2::Sha512),
        "md5" => run!(md5::Md5),
        other => return Err(format!("crypto.hmac: unknown hash '{}' (sha1 sha256 sha384 sha512 md5)", other)),
    };
    Ok(Value::Str(encode_out(&out, a.get(3)).into()))
}

const VERSION: u8 = 1;

/// A 32-byte key: 64 hex characters are used as is; anything else is a
/// password stretched with argon2id (default parameters) and the salt.
fn key_for(secret: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    if secret.len() == 64 {
        if let Some(k) = unhex(secret) {
            return Ok(k.try_into().unwrap());
        }
    }
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(secret.as_bytes(), salt, &mut key)
        .map_err(|e| io_err!("crypto: key derivation: {}", e))?;
    Ok(key)
}

/// `encrypt(data, key)` → base64 of `version | salt(16) | nonce(12) | ciphertext+tag`.
fn encrypt(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "encrypt")?;
    let data = bytes_arg(&a[0], "encrypt")?;
    let secret = str_arg(a, 1, "encrypt")?;
    if secret.is_empty() {
        return Err("encrypt: the key cannot be empty".into());
    }
    let salt = random(16)?;
    let nonce = random(12)?;
    let cipher = Aes256Gcm::new_from_slice(&key_for(&secret, &salt)?).unwrap();
    let ct = cipher.encrypt(&Nonce::try_from(&nonce[..]).expect("12-byte nonce"), data.as_slice()).map_err(|_| io_err!("crypto.encrypt failed"))?;
    let mut out = vec![VERSION];
    out.extend(salt);
    out.extend(nonce);
    out.extend(ct);
    Ok(Value::Str(base64::engine::general_purpose::STANDARD.encode(out).into()))
}

/// `decrypt(box, key, { bytes })` → the data; a wrong key or altered data is an error value.
fn decrypt(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "decrypt")?;
    let boxed = str_arg(a, 0, "decrypt")?;
    let secret = str_arg(a, 1, "decrypt")?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(boxed.trim())
        .map_err(|_| io_err!("crypto.decrypt: the data is not valid base64"))?;
    if raw.len() < 1 + 16 + 12 + 16 || raw[0] != VERSION {
        return Err(io_err!("crypto.decrypt: the data was not made by crypto.encrypt"));
    }
    let (salt, rest) = raw[1..].split_at(16);
    let (nonce, ct) = rest.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&key_for(&secret, salt)?).unwrap();
    let plain = cipher
        .decrypt(&Nonce::try_from(nonce).expect("12-byte nonce"), ct)
        .map_err(|_| io_err!("crypto.decrypt: wrong key or corrupted data"))?;
    as_text_or_bytes(plain, a.get(2))
}

fn hash_password(a: &[Value]) -> Result<Value, String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    max_args(a, 1, "hashPassword")?;
    let pw = str_arg(a, 0, "hashPassword")?;
    let salt = SaltString::encode_b64(&random(16)?).map_err(|e| io_err!("crypto.hashPassword: {}", e))?;
    let h = argon2::Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map_err(|e| io_err!("crypto.hashPassword: {}", e))?;
    Ok(Value::Str(h.to_string().into()))
}

fn verify_password(a: &[Value]) -> Result<Value, String> {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    max_args(a, 2, "verifyPassword")?;
    let pw = str_arg(a, 0, "verifyPassword")?;
    let stored = str_arg(a, 1, "verifyPassword")?;
    let parsed = PasswordHash::new(&stored).map_err(|_| io_err!("crypto.verifyPassword: not a password hash"))?;
    Ok(Value::Bool(argon2::Argon2::default().verify_password(pw.as_bytes(), &parsed).is_ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests() {
        assert_eq!(hex(&digest("md5", b"abc").unwrap()), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(&digest("sha1", b"abc").unwrap()), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(&digest("SHA-256", b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(unhex("00ff10"), Some(vec![0, 255, 16]));
        assert_eq!(unhex("0g"), None);
    }

    #[test]
    fn round_trip() {
        let boxed = encrypt(&[Value::Str("segredo".into()), Value::Str("pw".into())]).unwrap();
        let back = decrypt(&[boxed.clone(), Value::Str("pw".into())]).unwrap();
        assert!(matches!(back, Value::Str(s) if s == "segredo"));
        assert!(decrypt(&[boxed, Value::Str("wrong".into())]).is_err());
    }
}

#[cfg(test)]
mod uuid_tests {
    use super::*;

    fn text(v: Value) -> String {
        match v {
            Value::Str(s) => s.to_string(),
            other => panic!("not a string: {}", other.to_display_string()),
        }
    }

    #[test]
    fn v5_is_deterministic_and_matches_rfc() {
        let a = [Value::Str("dns".into()), Value::Str("example.com".into())];
        assert_eq!(text(uuid_v5(&a, "uuidv5").unwrap()), "cfbff0d1-9375-5685-968c-48ce8b15ae17");
        let custom = [Value::Str("6ba7b811-9dad-11d1-80b4-00c04fd430c8".into()), Value::Str("https://fogu.dev".into())];
        let url = [Value::Str("url".into()), Value::Str("https://fogu.dev".into())];
        assert_eq!(uuid_v5(&custom, "uuidv5").unwrap().to_display_string(), uuid_v5(&url, "uuidv5").unwrap().to_display_string());
        assert!(uuid_v5(&[Value::Str("nope".into()), Value::Str("x".into())], "uuidv5").is_err());
    }

    #[test]
    fn v7_is_time_ordered() {
        let ids: Vec<String> = (0..1000).map(|_| text(uuid_value(uuid::Uuid::now_v7()))).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        assert_eq!(&ids[0][14..15], "7");
    }

    #[test]
    fn info_reads_version_and_time() {
        let id = uuid_value(uuid::Uuid::now_v7());
        let info = uuid_info(&[id]).unwrap().to_display_string();
        assert!(info.contains("version: 7") && info.contains("rfc4122") && !info.contains("time: nil"), "{info}");
        let v4 = uuid_info(&[uuid_value(uuid::Uuid::new_v4())]).unwrap().to_display_string();
        assert!(v4.contains("version: 4") && v4.contains("time: nil"), "{v4}");
        assert!(uuid_info(&[Value::Str("zzz".into())]).is_err());
    }
}
