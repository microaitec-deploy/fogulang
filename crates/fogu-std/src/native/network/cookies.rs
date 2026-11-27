//! HTTP cookies: the `Cookie` header a server reads, the `Set-Cookie` header
//! it writes (with signed values), and the jar a client session keeps.

use std::time::{Duration, SystemTime};

use ahash::AHashMap as HashMap;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::runtime::value::{Str, Value};

/// Percent-encodes what a cookie value may not hold (as `encodeURIComponent`).
pub fn encode(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~!$&'()*+/:<>?@[]^`{|}".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub fn decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && v.is_char_boundary(i + 1) && v.is_char_boundary(i + 3) {
            if let Ok(n) = u8::from_str_radix(&v[i + 1..i + 3], 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=1; b=two` → [(a, 1), (b, two)] (values decoded; later duplicates lose).
pub fn parse_header(h: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for part in h.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        let k = k.trim();
        if k.is_empty() || out.iter().any(|(n, _)| n == k) {
            continue;
        }
        let v = v.trim().trim_matches('"');
        out.push((k.to_string(), decode(v)));
    }
    out
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&'*+-.^_`|~".contains(&b))
}

fn mac(secret: &str, name: &str, value: &str) -> String {
    let mut m = <Hmac<Sha256>>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    m.update(name.as_bytes());
    m.update(b"=");
    m.update(value.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(m.finalize().into_bytes())
}

/// `value.signature`: the name is signed too, so a value cannot move to another cookie.
pub fn sign(secret: &str, name: &str, value: &str) -> String {
    format!("{}.{}", value, mac(secret, name, value))
}

/// The value of a signed cookie, or `None` when its signature is wrong.
pub fn unsign(secret: &str, name: &str, signed: &str) -> Option<String> {
    let (value, sig) = signed.rsplit_once('.')?;
    let want = mac(secret, name, value);
    let ok = want.len() == sig.len() && want.bytes().zip(sig.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    ok.then(|| value.to_string())
}

/// The cookies of a request as a map; with `secret`, only signed cookies whose
/// signature holds, with their values.
pub fn cookies_value(header: &str, secret: Option<&str>) -> Value {
    let mut m: HashMap<Str, Value> = HashMap::new();
    for (k, v) in parse_header(header) {
        let v = match secret {
            Some(s) => match unsign(s, &k, &v) {
                Some(v) => v,
                None => continue,
            },
            None => v,
        };
        m.insert(k.into(), Value::Str(v.into()));
    }
    Value::Map(std::rc::Rc::new(std::cell::RefCell::new(m)))
}

/// `Set-Cookie` text: `name=value; Path=/; HttpOnly; SameSite=Lax` by default —
/// `{ path, domain, maxAge, expires, secure, httpOnly, sameSite, partitioned, secret }`.
pub fn set_cookie(name: &str, value: &str, o: Option<&Value>, what: &str) -> Result<String, String> {
    if !valid_name(name) {
        return Err(format!("{}: '{}' is not a valid cookie name", what, name));
    }
    let empty = HashMap::new();
    let opts = match o {
        None | Some(Value::Nil) => None,
        Some(Value::Map(m)) => Some(m.borrow().clone()),
        Some(other) => return Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    };
    let o = opts.as_ref().unwrap_or(&empty);
    let text = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
    let value = match text("secret") {
        Some(s) => sign(&s, name, value),
        None => value.to_string(),
    };
    let mut out = format!("{}={}", name, encode(&value));
    let path = text("path").unwrap_or_else(|| "/".into());
    if path.bytes().any(|b| b == b';' || b < 0x20) {
        return Err(format!("{}: invalid path '{}'", what, path));
    }
    out.push_str(&format!("; Path={}", path));
    if let Some(d) = text("domain") {
        if d.bytes().any(|b| b == b';' || b <= 0x20) {
            return Err(format!("{}: invalid domain '{}'", what, d));
        }
        out.push_str(&format!("; Domain={}", d));
    }
    if let Some(v) = o.get("maxAge").filter(|v| !matches!(v, Value::Nil)) {
        let secs = match v {
            Value::Int(n) => *n,
            Value::Duration(ns) => ns / 1_000_000_000,
            other => return Err(format!("{}: maxAge must be seconds or a duration, got '{}'", what, other.type_name())),
        };
        out.push_str(&format!("; Max-Age={}", secs));
        if secs <= 0 {
            out.push_str("; Expires=Thu, 01 Jan 1970 00:00:00 GMT");
        }
    }
    if let Some(e) = text("expires") {
        out.push_str(&format!("; Expires={}", e));
    }
    let same = text("sameSite").unwrap_or_else(|| "Lax".into());
    let same = match same.to_ascii_lowercase().as_str() {
        "lax" => "Lax",
        "strict" => "Strict",
        "none" => "None",
        _ => return Err(format!("{}: sameSite is Lax, Strict or None, got '{}'", what, same)),
    };
    // SameSite=None is only kept by browsers on a Secure cookie
    let secure = o.get("secure").map(Value::is_truthy).unwrap_or(same == "None");
    if secure {
        out.push_str("; Secure");
    }
    if o.get("httpOnly").is_none_or(Value::is_truthy) {
        out.push_str("; HttpOnly");
    }
    out.push_str(&format!("; SameSite={}", same));
    if o.get("partitioned").is_some_and(Value::is_truthy) {
        out.push_str("; Partitioned");
    }
    Ok(out)
}

/// Adds a `Set-Cookie` line to a map of headers (several cookies: one per line).
pub fn add_to(headers: &mut HashMap<Str, Value>, line: String) {
    match headers.get_mut("set-cookie") {
        Some(Value::Str(prev)) if !prev.is_empty() => {
            *prev = format!("{}\n{}", prev, line).into();
        }
        _ => {
            headers.insert("set-cookie".into(), Value::Str(line.into()));
        }
    }
}

/// The header lines a (name, value) pair stands for: `set-cookie` may hold several.
pub fn lines<'a>(name: &'a str, value: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let many = name.eq_ignore_ascii_case("set-cookie");
    value.split(move |c| many && c == '\n').filter(|l| !l.is_empty())
}

// ---------------------------------------------------------------- the client's jar

#[derive(Clone, Debug)]
struct Stored {
    name: String,
    value: String,
    domain: String,
    host_only: bool,
    path: String,
    secure: bool,
    expires: Option<SystemTime>,
}

/// The cookies of a client session (RFC 6265, without the public-suffix list:
/// a `Domain` must be the host or one of its parents).
#[derive(Default, Debug)]
pub struct Jar {
    cookies: Vec<Stored>,
}

fn default_path(url: &reqwest::Url) -> String {
    let p = url.path();
    match p.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => p[..i].to_string(),
    }
}

fn path_matches(req: &str, cookie: &str) -> bool {
    req == cookie || (req.starts_with(cookie) && (cookie.ends_with('/') || req.as_bytes().get(cookie.len()) == Some(&b'/')))
}

impl Jar {
    /// A `Set-Cookie` header received from `url`.
    pub fn store(&mut self, url: &reqwest::Url, header: &str) {
        let Some(host) = url.host_str().map(|h| h.to_ascii_lowercase()) else { return };
        let mut parts = header.split(';');
        let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else { return };
        let (name, value) = (name.trim().to_string(), value.trim().trim_matches('"').to_string());
        if name.is_empty() {
            return;
        }
        let mut c = Stored { name, value, domain: host.clone(), host_only: true, path: default_path(url), secure: false, expires: None };
        let mut max_age = None;
        for attr in parts {
            let (k, v) = attr.split_once('=').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((attr.trim(), ""));
            match k.to_ascii_lowercase().as_str() {
                "domain" => {
                    let d = v.trim_start_matches('.').to_ascii_lowercase();
                    // only the host itself or a parent of it
                    if d.is_empty() || !(host == d || host.ends_with(&format!(".{}", d))) {
                        return;
                    }
                    c.domain = d;
                    c.host_only = false;
                }
                "path" if v.starts_with('/') => c.path = v.to_string(),
                "secure" => c.secure = true,
                "max-age" => max_age = v.parse::<i64>().ok(),
                "expires" if max_age.is_none() => c.expires = parse_http_date(v),
                _ => {}
            }
        }
        if let Some(s) = max_age {
            c.expires = Some(if s <= 0 { SystemTime::UNIX_EPOCH } else { SystemTime::now() + Duration::from_secs(s as u64) });
        }
        self.cookies.retain(|o| !(o.name == c.name && o.domain == c.domain && o.path == c.path));
        if c.expires.is_none_or(|e| e > SystemTime::now()) {
            self.cookies.push(c);
        }
    }

    /// The `Cookie` header for a request to `url` (longer paths first).
    pub fn header_for(&mut self, url: &reqwest::Url) -> Option<String> {
        let now = SystemTime::now();
        self.cookies.retain(|c| c.expires.is_none_or(|e| e > now));
        let host = url.host_str()?.to_ascii_lowercase();
        let https = url.scheme() == "https";
        let mut hits: Vec<&Stored> = self
            .cookies
            .iter()
            .filter(|c| if c.host_only { host == c.domain } else { host == c.domain || host.ends_with(&format!(".{}", c.domain)) })
            .filter(|c| path_matches(url.path(), &c.path) && (https || !c.secure))
            .collect();
        if hits.is_empty() {
            return None;
        }
        hits.sort_by(|a, b| b.path.len().cmp(&a.path.len()));
        Some(hits.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; "))
    }

    /// name → value, for `client.cookies`.
    pub fn values(&self) -> Vec<(String, String)> {
        let now = SystemTime::now();
        self.cookies.iter().filter(|c| c.expires.is_none_or(|e| e > now)).map(|c| (c.name.clone(), decode(&c.value))).collect()
    }

    /// A cookie set by the program (for `url`'s host, path `/`).
    pub fn set(&mut self, url: &reqwest::Url, name: &str, value: &str) {
        self.store(url, &format!("{}={}; Path=/", name, encode(value)));
    }

    pub fn clear(&mut self) {
        self.cookies.clear();
    }
}

/// `Wdy, DD Mon YYYY HH:MM:SS GMT` (RFC 7231 IMF-fixdate; also `-` separators).
fn parse_http_date(s: &str) -> Option<SystemTime> {
    let s = s.trim();
    let rest = s.split_once(',').map(|(_, r)| r.trim()).unwrap_or(s);
    let f: Vec<&str> = rest.split(|c| c == ' ' || c == '-').filter(|x| !x.is_empty()).collect();
    if f.len() < 4 {
        return None;
    }
    let day: i64 = f[0].parse().ok()?;
    let month = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
        .iter()
        .position(|m| f[1].to_ascii_lowercase().starts_with(m))? as i64
        + 1;
    let mut year: i64 = f[2].parse().ok()?;
    if year < 100 {
        year += if year < 70 { 2000 } else { 1900 };
    }
    let hms: Vec<i64> = f[3].split(':').filter_map(|x| x.parse().ok()).collect();
    if hms.len() != 3 {
        return None;
    }
    // days from the civil date (Howard Hinnant's algorithm)
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hms[0] * 3600 + hms[1] * 60 + hms[2];
    if secs < 0 {
        return Some(SystemTime::UNIX_EPOCH);
    }
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jar_rules() {
        let u = reqwest::Url::parse("http://app.example.com/a/b").unwrap();
        let mut j = Jar::default();
        j.store(&u, "sid=1; Path=/; HttpOnly");
        j.store(&u, "deep=2");
        j.store(&u, "wide=3; Domain=example.com");
        j.store(&u, "evil=4; Domain=other.com");
        j.store(&u, "sec=5; Secure");
        j.store(&u, "gone=6; Max-Age=0");
        assert_eq!(j.header_for(&reqwest::Url::parse("http://app.example.com/a/b/c").unwrap()).unwrap(), "deep=2; wide=3; sid=1");
        assert_eq!(j.header_for(&reqwest::Url::parse("http://www.example.com/a/x").unwrap()).unwrap(), "wide=3");
        assert!(j.header_for(&reqwest::Url::parse("https://app.example.com/a/x").unwrap()).unwrap().contains("sec=5"));
        j.store(&u, "sid=; Max-Age=0; Path=/");
        assert!(!j.header_for(&u).unwrap().contains("sid="));
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:10 GMT"), Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10)));
    }

    #[test]
    fn signed_values() {
        let s = sign("k", "sid", "42");
        assert_eq!(unsign("k", "sid", &s).as_deref(), Some("42"));
        assert_eq!(unsign("k", "other", &s), None);
        assert_eq!(unsign("x", "sid", &s), None);
        assert_eq!(decode(&encode("a b;ç")), "a b;ç");
    }
}
