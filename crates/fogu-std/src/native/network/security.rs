//! `fogu:security` module — security and network auditing tools: TLS and
//! certificate inspection, port scanning, DNS, HTTP security headers,
//! password strength, TOTP (2FA), secret scanning, Ed25519 signatures, IP
//! ranges and rate limiting. Built on rustls, x509-parser, rcgen,
//! zxcvbn, ed25519-dalek, ipnet and subtle; DNS is our own client (`dns.rs`).
//!
//! ```fogu
//! import security from "fogu:security"
//!
//! const t = security.tls("example.com")?            // { valid, protocol, cipher, daysLeft, chain: [...] }
//! print(t.valid, t.protocol, t.chain[0].subject, t.daysLeft)
//! print(security.scan("127.0.0.1", "22,80,443,8000-8100")?)   // [{ port, service, banner }]
//! print(security.dns("example.com", "MX")?)
//! print(security.headers("https://example.com")?.grade)       // A..F + what is missing
//! print(security.passwordStrength("hunter2").score)           // 0..4
//! const secret = security.totpSecret()
//! print(security.verifyTotp(secret, security.totp(secret)))   // true
//! print(security.scanSecrets("aws = AKIAIOSFODNN7EXAMPLE"))   // [{ rule, line, column, match }]
//! const k = security.keypair()                                // Ed25519
//! print(security.verify(k.publicKey, "msg", security.sign(k.privateKey, "msg")))
//! const { cert, key } = security.selfSigned(["localhost", "127.0.0.1"], { days: 30 })
//! print(security.inCidr("10.1.2.3", "10.0.0.0/8"), security.isPrivate("8.8.8.8"))
//! const rl = security.rateLimiter(5, 10)                      // 5/s, bursts of 10
//! if !rl.allow(clientIp) { /* 429 */ }
//! ```
//!
//! Scan only hosts you are allowed to test. Network failures are error
//! values; wrong arguments stop the program.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use sha2::Digest;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{duration_arg, list, map, max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    let mut f = |name: &str, min: usize, func: fn(&[Value]) -> Result<Value, String>| {
        e.insert(name.to_string(), nfn_var(name, min, func));
    };
    f("parseCert", 1, parse_cert);
    f("selfSigned", 1, self_signed);
    f("passwordStrength", 1, password_strength);
    f("totpSecret", 0, totp_secret);
    f("totp", 1, totp);
    f("verifyTotp", 2, verify_totp);
    f("otpauthUrl", 2, otpauth_url);
    f("scanSecrets", 1, scan_secrets);
    f("scanFiles", 1, scan_files);
    f("keypair", 0, keypair);
    f("sign", 2, sign);
    f("verify", 3, verify);
    f("constantTimeEq", 2, constant_time_eq);
    f("cidr", 1, cidr);
    f("inCidr", 2, in_cidr);
    f("isPrivate", 1, is_private);
    f("hosts", 1, hosts);
    f("rateLimiter", 2, rate_limiter);
    f("services", 0, |_| Ok(list(SERVICES.iter().map(|(p, s)| map(vec![("port", Value::Int(*p as i64)), ("service", Value::Str(s.to_string().into()))])).collect())));
    // network calls wait cooperatively (rt::offload / rt::block_on)
    f("tls", 1, tls);
    f("scan", 1, scan);
    f("dns", 1, |a| dns(a, false));
    f("reverseDns", 1, |a| dns(a, true));
    f("headers", 1, headers);
    ModuleValue {
        name: "fogu:security".into(),
        exports: RefCell::new(e),
    }
}

fn opts(v: Option<&Value>, what: &str) -> Result<HashMap<String, Value>, String> {
    match v {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn int_opt(o: &HashMap<String, Value>, k: &str, d: i64) -> i64 {
    match o.get(k) {
        Some(Value::Int(i)) => *i,
        Some(Value::Float(f)) => *f as i64,
        _ => d,
    }
}

fn dur_opt(o: &HashMap<String, Value>, k: &str, d: Duration, what: &str) -> Result<Duration, String> {
    match o.get(k) {
        None | Some(Value::Nil) => Ok(d),
        Some(v) => duration_arg(v, what),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

// ---------------------------------------------------------------------
// network functions (async: they return a poller)
// ---------------------------------------------------------------------

// ---------- TLS ----------

/// Accepts every certificate (to inspect it) but records what the real
/// verifier thinks of it.
#[derive(Debug)]
struct Recorder {
    inner: Arc<rustls::client::WebPkiServerVerifier>,
    verdict: Arc<Mutex<Option<Result<(), String>>>>,
}

impl rustls::client::danger::ServerCertVerifier for Recorder {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let r = self.inner.verify_server_cert(end_entity, intermediates, server_name, ocsp, now);
        *self.verdict.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.map(|_| ()).map_err(|e| e.to_string()));
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

struct TlsReport {
    protocol: String,
    cipher: String,
    alpn: Option<String>,
    verdict: Result<(), String>,
    chain: Vec<Vec<u8>>,
    millis: u128,
}

/// `tls(host, { port = 443, timeout = 5s, sni, ca, alpn })`
fn tls(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "security.tls")?;
    let target = str_arg(args, 0, "security.tls")?;
    let o = opts(args.get(1), "security.tls")?;
    let (host, port) = split_host_port(&target, int_opt(&o, "port", 443) as u16);
    let sni = o.get("sni").map(|v| v.to_display_string()).unwrap_or_else(|| host.clone());
    let timeout = dur_opt(&o, "timeout", Duration::from_secs(5), "security.tls timeout")?;
    let ca = o.get("ca").map(|v| v.to_display_string());
    let alpn: Vec<Vec<u8>> = match o.get("alpn") {
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string().into_bytes()).collect(),
        _ => Vec::new(),
    };
    let r = crate::native::rt::offload(move || tls_inspect(&host, port, &sni, timeout, ca.as_deref(), alpn))??;
    {
        {
            let chain: Vec<Value> = r.chain.iter().map(|der| cert_value(der)).collect::<Result<_, _>>()?;
            let days = match chain.first() {
                Some(Value::Map(m)) => m.borrow().get("daysLeft").cloned().unwrap_or(Value::Nil),
                _ => Value::Nil,
            };
            Ok(map(vec![
                ("valid", Value::Bool(r.verdict.is_ok())),
                ("error", r.verdict.err().map(Value::str).unwrap_or(Value::Nil)),
                ("protocol", Value::Str(r.protocol.into())),
                ("cipher", Value::Str(r.cipher.into())),
                ("alpn", r.alpn.map(Value::str).unwrap_or(Value::Nil)),
                ("daysLeft", days),
                ("handshakeMs", Value::Int(r.millis as i64)),
                ("chain", list(chain)),
            ]))
        }
    }
}

fn split_host_port(s: &str, default: u16) -> (String, u16) {
    let s = s.trim().trim_start_matches("https://").trim_start_matches("http://");
    let s = s.split('/').next().unwrap_or(s);
    if let Some(r) = s.strip_prefix('[') {
        if let Some((h, tail)) = r.split_once(']') {
            return (h.to_string(), tail.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(default));
        }
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h.to_string(), p.parse().unwrap_or(default)),
        _ => (s.to_string(), default),
    }
}

fn roots(ca: Option<&str>) -> Result<rustls::RootCertStore, String> {
    let mut store = rustls::RootCertStore::empty();
    match ca {
        Some(path) => {
            let data = std::fs::read(path).map_err(|e| io_err!("security.tls: ca {}: {}", path, e))?;
            for c in <rustls::pki_types::CertificateDer as rustls::pki_types::pem::PemObject>::pem_slice_iter(&data) {
                let c = c.map_err(|e| io_err!("security.tls: ca {}: {}", path, e))?;
                store.add(c).map_err(|e| io_err!("security.tls: ca {}: {}", path, e))?;
            }
        }
        None => {
            for c in rustls_native_certs::load_native_certs().certs {
                let _ = store.add(c);
            }
        }
    }
    Ok(store)
}

fn tls_inspect(host: &str, port: u16, sni: &str, timeout: Duration, ca: Option<&str>, alpn: Vec<Vec<u8>>) -> Result<TlsReport, String> {
    use std::io::Write;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let inner = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots(ca)?), provider.clone())
        .build()
        .map_err(|e| io_err!("security.tls: {}", e))?;
    let verdict = Arc::new(Mutex::new(None));
    let mut cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| io_err!("security.tls: {}", e))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Recorder { inner, verdict: verdict.clone() }))
        .with_no_client_auth();
    cfg.alpn_protocols = alpn;
    let name = rustls::pki_types::ServerName::try_from(sni.to_string()).map_err(|e| format!("security.tls: invalid name '{}': {}", sni, e))?;
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| io_err!("security.tls: {}: {}", host, e))?
        .next()
        .ok_or_else(|| io_err!("security.tls: {}: no address", host))?;
    let start = Instant::now();
    let mut tcp = std::net::TcpStream::connect_timeout(&addr, timeout).map_err(|e| io_err!("security.tls: {}:{}: {}", host, port, e))?;
    tcp.set_read_timeout(Some(timeout)).ok();
    tcp.set_write_timeout(Some(timeout)).ok();
    let mut conn = rustls::ClientConnection::new(Arc::new(cfg), name).map_err(|e| io_err!("security.tls: {}", e))?;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp).map_err(|e| io_err!("security.tls: {}:{}: handshake failed: {}", host, port, e))?;
    }
    let _ = conn.writer().flush();
    let millis = start.elapsed().as_millis();
    let chain = conn.peer_certificates().map(|c| c.iter().map(|d| d.as_ref().to_vec()).collect()).unwrap_or_default();
    let protocol = match conn.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3".to_string(),
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2".to_string(),
        Some(v) => format!("{:?}", v),
        None => "unknown".into(),
    };
    let cipher = conn.negotiated_cipher_suite().map(|c| format!("{:?}", c.suite())).unwrap_or_default();
    let alpn = conn.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned());
    conn.send_close_notify();
    let _ = conn.complete_io(&mut tcp);
    let verdict = verdict.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or(Ok(()));
    Ok(TlsReport { protocol, cipher, alpn, verdict, chain, millis })
}

// ---------- certificates ----------

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn iso(ts: i64) -> String {
    // civil date from unix seconds (UTC)
    let (y, m, d) = civil_from_days(ts.div_euclid(86_400));
    let s = ts.rem_euclid(86_400);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, s / 3600, s % 3600 / 60, s % 60)
}

/// Days since 1970-01-01 → (year, month, day) (H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// A DER certificate → its description.
fn cert_value(der: &[u8]) -> Result<Value, String> {
    let (_, c) = x509_parser::parse_x509_certificate(der).map_err(|e| io_err!("security: invalid certificate: {}", e))?;
    let reg = x509_parser::objects::oid_registry();
    let oid_name = |oid: &x509_parser::der_parser::oid::Oid| {
        x509_parser::objects::oid2sn(oid, reg).map(str::to_string).unwrap_or_else(|_| oid.to_id_string())
    };
    let mut names = Vec::new();
    if let Ok(Some(san)) = c.subject_alternative_name() {
        for n in &san.value.general_names {
            match n {
                x509_parser::extensions::GeneralName::DNSName(d) => names.push(Value::Str(d.to_string().into())),
                x509_parser::extensions::GeneralName::IPAddress(b) => {
                    let ip = match b.len() {
                        4 => IpAddr::from(<[u8; 4]>::try_from(*b).expect("4 bytes")).to_string(),
                        16 => IpAddr::from(<[u8; 16]>::try_from(*b).expect("16 bytes")).to_string(),
                        _ => hex(b),
                    };
                    names.push(Value::Str(ip.into()));
                }
                x509_parser::extensions::GeneralName::RFC822Name(m) => names.push(Value::Str(format!("email:{}", m).into())),
                x509_parser::extensions::GeneralName::URI(u) => names.push(Value::Str(format!("uri:{}", u).into())),
                _ => {}
            }
        }
    }
    let nb = c.validity().not_before.timestamp();
    let na = c.validity().not_after.timestamp();
    let now = now_secs();
    let key_bits = c.public_key().parsed().map(|k| k.key_size() as i64).unwrap_or(0);
    let cn = c.subject().iter_common_name().next().and_then(|a| a.as_str().ok()).map(str::to_string);
    Ok(map(vec![
        ("subject", Value::Str(c.subject().to_string().into())),
        ("commonName", cn.map(Value::str).unwrap_or(Value::Nil)),
        ("issuer", Value::Str(c.issuer().to_string().into())),
        ("names", list(names)),
        ("serial", Value::Str(c.raw_serial_as_string().into())),
        ("notBefore", Value::Str(iso(nb).into())),
        ("notAfter", Value::Str(iso(na).into())),
        ("daysLeft", Value::Int((na - now).div_euclid(86_400))),
        ("expired", Value::Bool(now > na)),
        ("selfSigned", Value::Bool(c.subject() == c.issuer())),
        ("ca", Value::Bool(c.is_ca())),
        ("signature", Value::Str(oid_name(&c.signature_algorithm.algorithm).into())),
        ("keyType", Value::Str(oid_name(&c.public_key().algorithm.algorithm).into())),
        ("keyBits", Value::Int(key_bits)),
        ("sha256", Value::Str(hex(&sha2::Sha256::digest(der)).into())),
    ]))
}

/// parseCert(pem | der bytes) → the first certificate's description
fn parse_cert(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "security.parseCert")?;
    let data = crate::native::util::bytes_of(&a[0], "security.parseCert")?;
    if data.starts_with(b"-----BEGIN") {
        let (_, pem) = x509_parser::pem::parse_x509_pem(&data).map_err(|e| io_err!("security.parseCert: invalid PEM: {}", e))?;
        return cert_value(&pem.contents);
    }
    cert_value(&data)
}

/// selfSigned(names, { days = 365 }) → { cert, key } (PEM)
fn self_signed(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.selfSigned")?;
    let names: Vec<String> = match &a[0] {
        Value::Str(s) => vec![s.to_string()],
        Value::List(l) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        other => return Err(format!("security.selfSigned: expects a name or a list of names, got '{}'", other.type_name())),
    };
    let o = opts(a.get(1), "security.selfSigned")?;
    let days = int_opt(&o, "days", 365).clamp(1, 36_500);
    let mut params = rcgen::CertificateParams::new(names.clone()).map_err(|e| format!("security.selfSigned: {}", e))?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, names.first().cloned().unwrap_or_else(|| "localhost".into()));
    params.distinguished_name = dn;
    let today = now_secs().div_euclid(86_400);
    let date = |days: i64| {
        let (y, m, d) = civil_from_days(days);
        rcgen::date_time_ymd(y as i32, m as u8, d as u8)
    };
    params.not_before = date(today - 1);
    params.not_after = date(today + days);
    let key = rcgen::KeyPair::generate().map_err(|e| io_err!("security.selfSigned: {}", e))?;
    let cert = params.self_signed(&key).map_err(|e| io_err!("security.selfSigned: {}", e))?;
    Ok(map(vec![("cert", Value::Str(cert.pem().into())), ("key", Value::Str(key.serialize_pem().into()))]))
}

// ---------- port scanning ----------

/// Well-known services (for `scan` results).
const SERVICES: &[(u16, &str)] = &[
    (21, "ftp"), (22, "ssh"), (23, "telnet"), (25, "smtp"), (53, "dns"), (80, "http"), (110, "pop3"), (111, "rpcbind"),
    (135, "msrpc"), (139, "netbios"), (143, "imap"), (389, "ldap"), (443, "https"), (445, "smb"), (465, "smtps"),
    (587, "submission"), (636, "ldaps"), (873, "rsync"), (993, "imaps"), (995, "pop3s"), (1433, "mssql"), (1521, "oracle"),
    (1883, "mqtt"), (2049, "nfs"), (2375, "docker"), (2376, "docker-tls"), (2379, "etcd"), (3000, "http-dev"),
    (3306, "mysql"), (3389, "rdp"), (4222, "nats"), (5000, "http-dev"), (5432, "postgres"), (5672, "amqp"),
    (5900, "vnc"), (6379, "redis"), (6443, "kubernetes"), (8000, "http-alt"), (8080, "http-proxy"), (8443, "https-alt"),
    (8883, "mqtts"), (9000, "http-alt"), (9042, "cassandra"), (9092, "kafka"), (9200, "elasticsearch"),
    (11211, "memcached"), (15672, "rabbitmq-ui"), (27017, "mongodb"), (50051, "grpc"),
];

fn service_of(port: u16) -> Option<&'static str> {
    SERVICES.iter().find(|(p, _)| *p == port).map(|(_, s)| *s)
}

/// 22 · [22, 80] · 1..1024 · "22,80,8000-8100" · "common"
fn ports_arg(v: &Value) -> Result<Vec<u16>, String> {
    let mut out = Vec::new();
    let port = |n: i64| u16::try_from(n).ok().filter(|p| *p > 0).ok_or_else(|| format!("security.scan: invalid port {}", n));
    match v {
        Value::Int(n) => out.push(port(*n)?),
        Value::Range(a, b) => {
            for n in *a..*b {
                out.push(port(n)?);
            }
        }
        Value::List(l) => {
            for x in l.borrow().iter() {
                match x {
                    Value::Int(n) => out.push(port(*n)?),
                    other => return Err(format!("security.scan: ports must be ints, got '{}'", other.type_name())),
                }
            }
        }
        Value::Str(s) if s.trim() == "common" => out.extend(SERVICES.iter().map(|(p, _)| *p)),
        Value::Str(s) => {
            for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match part.split_once('-') {
                    Some((a, b)) => {
                        let (a, b) = (a.trim().parse::<i64>(), b.trim().parse::<i64>());
                        let (Ok(a), Ok(b)) = (a, b) else { return Err(format!("security.scan: invalid range '{}'", part)) };
                        for n in a..=b {
                            out.push(port(n)?);
                        }
                    }
                    None => out.push(port(part.parse::<i64>().map_err(|_| format!("security.scan: invalid port '{}'", part))?)?),
                }
            }
        }
        other => return Err(format!("security.scan: ports must be an int, a list, a range or a string, got '{}'", other.type_name())),
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// scan(host, ports = "common", { timeout = 500ms, concurrency = 200, banner = false })
fn scan(args: &[Value]) -> Result<Value, String> {
    max_args(args, 3, "security.scan")?;
    let host = str_arg(args, 0, "security.scan")?;
    let ports = ports_arg(args.get(1).unwrap_or(&Value::Str("common".into())))?;
    let o = opts(args.get(2), "security.scan")?;
    let timeout = dur_opt(&o, "timeout", Duration::from_millis(500), "security.scan timeout")?;
    let conc = int_opt(&o, "concurrency", 200).clamp(1, 2000) as usize;
    let banner = o.get("banner").is_some_and(Value::is_truthy);
    let r = crate::native::rt::block_on(async move {
        let ip = tokio::net::lookup_host((host.as_str(), 0))
            .await
            .map_err(|e| io_err!("security.scan: {}: {}", host, e))?
            .next()
            .ok_or_else(|| io_err!("security.scan: {}: no address", host))?
            .ip();
        let sem = Arc::new(tokio::sync::Semaphore::new(conc));
        let mut tasks = Vec::with_capacity(ports.len());
        for p in ports {
            let sem = Arc::clone(&sem);
            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok()?;
                let conn = tokio::time::timeout(timeout, tokio::net::TcpStream::connect((ip, p))).await;
                let Ok(Ok(mut s)) = conn else { return None };
                let mut text = None;
                if banner {
                    use tokio::io::AsyncReadExt;
                    let mut buf = [0u8; 256];
                    if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(400), s.read(&mut buf)).await {
                        if n > 0 {
                            text = Some(String::from_utf8_lossy(&buf[..n]).trim().to_string());
                        }
                    }
                }
                Some((p, text))
            }));
        }
        let mut open = Vec::new();
        for t in tasks {
            if let Ok(Some(r)) = t.await {
                open.push(r);
            }
        }
        Ok::<_, String>((ip.to_string(), open))
    })?;
    r.map(|(_, open)| {
            list(
                open.into_iter()
                    .map(|(p, b)| {
                        map(vec![
                            ("port", Value::Int(p as i64)),
                            ("service", service_of(p).map(|s| Value::Str(s.into())).unwrap_or(Value::Nil)),
                            ("banner", b.map(Value::str).unwrap_or(Value::Nil)),
                        ])
                    })
                    .collect(),
            )
        })
}

// ---------- DNS ----------

/// dns(name, type = "A") → [{ type, value, ttl }] · reverseDns(ip) → [names]
fn dns(args: &[Value], reverse: bool) -> Result<Value, String> {
    let what = if reverse { "security.reverseDns" } else { "security.dns" };
    max_args(args, 3, what)?;
    let name = str_arg(args, 0, what)?;
    // dns(name, type?, opts?) · reverseDns(ip, opts?)
    let (rtype, o) = if reverse {
        ("PTR".to_string(), opts(args.get(1), what)?)
    } else {
        match args.get(1) {
            Some(Value::Map(_)) => ("A".to_string(), opts(args.get(1), what)?),
            v => (v.map(|v| v.to_display_string()).unwrap_or_else(|| "A".into()).to_ascii_uppercase(), opts(args.get(2), what)?),
        }
    };
    if crate::native::dns::type_code(&rtype).is_none() {
        return Err(format!("{}: unknown record type '{}'", what, rtype));
    }
    let qname = if reverse {
        let ip: IpAddr = name.parse().map_err(|_| format!("{}: '{}' is not an IP address", what, name))?;
        crate::native::dns::reverse_name(ip)
    } else {
        name.clone()
    };
    let server = o.get("server").map(|v| v.to_display_string());
    let timeout = dur_opt(&o, "timeout", Duration::from_secs(3), "dns timeout")?;
    let records = crate::native::rt::offload(move || crate::native::dns::resolve(&qname, &rtype, server.as_deref(), timeout))?.map_err(|e| io_err!("dns: {}", e))?;
    if reverse {
        return Ok(list(records.into_iter().map(|r| Value::Str(r.value.trim_end_matches('.').to_string().into())).collect()));
    }
    Ok(list(
        records
            .into_iter()
            .map(|r| map(vec![("type", Value::Str(r.rtype.into())), ("value", Value::Str(r.value.into())), ("ttl", Value::Int(r.ttl as i64))]))
            .collect(),
    ))
}

// ---------- HTTP security headers ----------

struct HeaderReport {
    url: String,
    status: u16,
    headers: Vec<(String, String)>,
    cookies: Vec<String>,
}

/// headers(url, { timeout = 10s }) → { grade, score, present, missing, warnings }
fn headers(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "security.headers")?;
    let url = str_arg(args, 0, "security.headers")?;
    let url = if url.contains("://") { url } else { format!("https://{}", url) };
    let o = opts(args.get(1), "security.headers")?;
    let timeout = dur_opt(&o, "timeout", Duration::from_secs(10), "security.headers timeout")?;
    let r = crate::native::rt::offload(
        move || -> Result<HeaderReport, String> {
            let client = reqwest::blocking::Client::builder()
                .timeout(timeout)
                .user_agent("fogu-security/1")
                .build()
                .map_err(|e| io_err!("security.headers: {}", e))?;
            let resp = client.get(&url).send().map_err(|e| io_err!("security.headers: {}: {}", url, e))?;
            let headers = resp
                .headers()
                .iter()
                .filter(|(k, _)| k.as_str() != "set-cookie")
                .map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                .collect();
            let cookies = resp.headers().get_all("set-cookie").iter().map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned()).collect();
            Ok(HeaderReport { url: resp.url().to_string(), status: resp.status().as_u16(), headers, cookies })
        },
    )??;
    Ok(audit_headers(&r))
}

fn audit_headers(r: &HeaderReport) -> Value {
    let get = |k: &str| r.headers.iter().find(|(h, _)| h == k).map(|(_, v)| v.clone());
    let https = r.url.starts_with("https://");
    let csp = get("content-security-policy");
    // (header, why it matters, present?)
    let checks: Vec<(&str, &str, bool)> = vec![
        ("strict-transport-security", "forces HTTPS on later visits", !https || get("strict-transport-security").is_some_and(|v| v.contains("max-age") && !v.contains("max-age=0"))),
        ("content-security-policy", "limits where scripts and content may come from", csp.is_some()),
        ("x-content-type-options", "stops MIME sniffing (nosniff)", get("x-content-type-options").is_some_and(|v| v.eq_ignore_ascii_case("nosniff"))),
        ("x-frame-options", "prevents clickjacking (or CSP frame-ancestors)", get("x-frame-options").is_some() || csp.as_deref().is_some_and(|c| c.contains("frame-ancestors"))),
        ("referrer-policy", "controls what the Referer header leaks", get("referrer-policy").is_some()),
        ("permissions-policy", "limits browser features (camera, geolocation…)", get("permissions-policy").is_some()),
    ];
    let score = checks.iter().filter(|c| c.2).count();
    let mut warnings = Vec::new();
    if !https {
        warnings.push(Value::Str("the site is served over plain HTTP".into()));
    }
    if let Some(s) = get("server").filter(|s| s.chars().any(|c| c.is_ascii_digit())) {
        warnings.push(Value::Str(format!("'server' reveals the software version: {}", s).into()));
    }
    if let Some(p) = get("x-powered-by") {
        warnings.push(Value::Str(format!("'x-powered-by' reveals the stack: {}", p).into()));
    }
    if csp.as_deref().is_some_and(|c| c.contains("'unsafe-inline'") || c.contains("'unsafe-eval'")) {
        warnings.push(Value::Str("the content security policy allows 'unsafe-inline' or 'unsafe-eval'".into()));
    }
    if get("access-control-allow-origin").as_deref() == Some("*") {
        warnings.push(Value::Str("CORS allows any origin (access-control-allow-origin: *)".into()));
    }
    for c in &r.cookies {
        let name = c.split('=').next().unwrap_or("").trim();
        let lc = c.to_ascii_lowercase();
        let mut miss = Vec::new();
        if https && !lc.contains("secure") {
            miss.push("Secure");
        }
        if !lc.contains("httponly") {
            miss.push("HttpOnly");
        }
        if !lc.contains("samesite") {
            miss.push("SameSite");
        }
        if !miss.is_empty() {
            warnings.push(Value::Str(format!("cookie '{}' without {}", name, miss.join(", ")).into()));
        }
    }
    let grade = match (score, warnings.len()) {
        (6, 0) => "A+",
        (6, _) | (5, 0) => "A",
        (5, _) | (4, _) => "B",
        (3, _) => "C",
        (2, _) => "D",
        _ => "F",
    };
    let present: HashMap<String, Value> = checks.iter().filter(|c| c.2).filter_map(|c| get(c.0).map(|v| (c.0.to_string(), Value::Str(v.into())))).collect();
    Value::Map(std::rc::Rc::new(RefCell::new(
        [
            ("url".to_string(), Value::Str(r.url.clone().into())),
            ("status".to_string(), Value::Int(r.status as i64)),
            ("grade".to_string(), Value::Str(grade.into())),
            ("score".to_string(), Value::Int(score as i64)),
            ("present".to_string(), Value::Map(std::rc::Rc::new(RefCell::new(present.into_iter().collect())))),
            (
                "missing".to_string(),
                list(checks.iter().filter(|c| !c.2).map(|c| map(vec![("header", Value::Str(c.0.into())), ("why", Value::Str(c.1.into()))])).collect()),
            ),
            ("warnings".to_string(), list(warnings)),
        ]
        .into_iter()
        .collect(),
    )))
}

// ---------------------------------------------------------------------
// passwords and 2FA
// ---------------------------------------------------------------------

/// passwordStrength(password, [user inputs]) → { score 0..4, guesses, crackTime, warning, suggestions }
fn password_strength(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.passwordStrength")?;
    let pw = str_arg(a, 0, "security.passwordStrength")?;
    let inputs: Vec<String> = match a.get(1) {
        Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        _ => Vec::new(),
    };
    let refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let e = zxcvbn::zxcvbn(&pw, &refs);
    let fb = e.feedback();
    Ok(map(vec![
        ("score", Value::Int(u8::from(e.score()) as i64)),
        ("guesses", Value::Int(e.guesses().min(i64::MAX as u64) as i64)),
        ("crackTime", Value::Str(e.crack_times().offline_slow_hashing_1e4_per_second().to_string().into())),
        ("crackTimeOnline", Value::Str(e.crack_times().online_throttling_100_per_hour().to_string().into())),
        ("warning", fb.and_then(|f| f.warning()).map(|w| Value::Str(w.to_string().into())).unwrap_or(Value::Nil)),
        ("suggestions", list(fb.map(|f| f.suggestions().iter().map(|s| Value::Str(s.to_string().into())).collect()).unwrap_or_default())),
    ]))
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(B32[((buf >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.chars().filter(|c| !c.is_whitespace() && *c != '=' && *c != '-') {
        let v = B32.iter().position(|b| *b as char == c.to_ascii_uppercase())? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            out.push((buf >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

/// RFC 4226 HOTP with HMAC-SHA1.
fn hotp(key: &[u8], counter: u64, digits: u32) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(key).expect("any key size");
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = (h[19] & 0x0f) as usize;
    let code = ((h[off] as u32 & 0x7f) << 24) | ((h[off + 1] as u32) << 16) | ((h[off + 2] as u32) << 8) | h[off + 3] as u32;
    format!("{:0width$}", code % 10u32.pow(digits), width = digits as usize)
}

fn totp_key(a: &[Value], what: &str) -> Result<Vec<u8>, String> {
    let s = str_arg(a, 0, what)?;
    base32_decode(&s).filter(|k| !k.is_empty()).ok_or_else(|| format!("{}: the secret must be base32 (from security.totpSecret())", what))
}

struct TotpOpts {
    step: u64,
    digits: u32,
    time: u64,
    window: i64,
}

fn totp_opts(v: Option<&Value>, what: &str) -> Result<TotpOpts, String> {
    let o = opts(v, what)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let time = match o.get("time") {
        Some(Value::Int(t)) => *t as u64,
        Some(Value::Float(t)) => *t as u64,
        _ => now,
    };
    Ok(TotpOpts {
        step: int_opt(&o, "step", 30).max(1) as u64,
        digits: int_opt(&o, "digits", 6).clamp(6, 8) as u32,
        time,
        window: int_opt(&o, "window", 1).clamp(0, 10),
    })
}

/// totpSecret(bytes = 20) → base32 secret
fn totp_secret(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "security.totpSecret")?;
    let n = match a.first() {
        Some(Value::Int(n)) if (10..=64).contains(n) => *n as usize,
        None => 20,
        Some(other) => return Err(format!("security.totpSecret: size must be an int 10..64, got {}", other.to_display_string())),
    };
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).map_err(|e| io_err!("security.totpSecret: {}", e))?;
    Ok(Value::Str(base32_encode(&b).into()))
}

/// totp(secret, { time, step = 30, digits = 6 }) → the current code
fn totp(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.totp")?;
    let key = totp_key(a, "security.totp")?;
    let o = totp_opts(a.get(1), "security.totp")?;
    Ok(Value::Str(hotp(&key, o.time / o.step, o.digits).into()))
}

/// verifyTotp(secret, code, { window = 1, time, step, digits }) → bool (constant time)
fn verify_totp(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "security.verifyTotp")?;
    let key = totp_key(a, "security.verifyTotp")?;
    let code = a[1].to_display_string();
    let o = totp_opts(a.get(2), "security.verifyTotp")?;
    let counter = (o.time / o.step) as i64;
    let mut ok = subtle::Choice::from(0);
    for d in -o.window..=o.window {
        let c = counter + d;
        if c >= 0 {
            let want = hotp(&key, c as u64, o.digits);
            if want.len() == code.len() {
                ok |= subtle::ConstantTimeEq::ct_eq(want.as_bytes(), code.as_bytes());
            }
        }
    }
    Ok(Value::Bool(bool::from(ok)))
}

/// otpauthUrl(secret, account, { issuer }) → the URL authenticator apps read (as a QR code)
fn otpauth_url(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "security.otpauthUrl")?;
    let secret = str_arg(a, 0, "security.otpauthUrl")?;
    let account = str_arg(a, 1, "security.otpauthUrl")?;
    let o = opts(a.get(2), "security.otpauthUrl")?;
    let enc = |s: &str| -> String {
        s.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) })
            .collect()
    };
    let mut url = String::from("otpauth://totp/");
    match o.get("issuer") {
        Some(i) => {
            let i = i.to_display_string();
            url.push_str(&format!("{}:{}?secret={}&issuer={}", enc(&i), enc(&account), secret, enc(&i)));
        }
        None => url.push_str(&format!("{}?secret={}", enc(&account), secret)),
    }
    Ok(Value::Str(url.into()))
}

// ---------------------------------------------------------------------
// secret scanning
// ---------------------------------------------------------------------

const SECRET_RULES: &[(&str, &str)] = &[
    ("aws-access-key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
    ("github-token", r"\bgh[pousr]_[A-Za-z0-9]{36,255}\b"),
    ("github-pat", r"\bgithub_pat_[A-Za-z0-9_]{60,255}\b"),
    ("gitlab-token", r"\bglpat-[A-Za-z0-9_-]{20,}\b"),
    ("slack-token", r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b"),
    ("slack-webhook", r"https://hooks\.slack\.com/services/[A-Za-z0-9/]{20,}"),
    ("google-api-key", r"\bAIza[0-9A-Za-z_-]{35}\b"),
    ("stripe-key", r"\b(?:sk|rk)_live_[0-9a-zA-Z]{24,}\b"),
    ("openai-key", r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}\b"),
    ("anthropic-key", r"\bsk-ant-[A-Za-z0-9_-]{32,}\b"),
    ("private-key", r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY(?: BLOCK)?-----"),
    ("jwt", r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b"),
    ("url-password", r"\b[a-z][a-z0-9+.-]*://[^/\s:@]+:[^/\s:@]{3,}@[^\s/]+"),
    ("generic-secret", r#"(?i)\b(?:api[_-]?key|secret|token|passwd|password|pwd)\b["']?\s*[:=]\s*["'][^"'\s]{8,}["']"#),
];

fn secret_rules() -> &'static Vec<(&'static str, regex::Regex)> {
    static RULES: std::sync::OnceLock<Vec<(&'static str, regex::Regex)>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| SECRET_RULES.iter().map(|(n, r)| (*n, regex::Regex::new(r).expect("valid rule"))).collect())
}

/// Keeps the first 4 and last 2 characters.
fn redact(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= 8 {
        return "*".repeat(c.len());
    }
    format!("{}…{}", c[..4].iter().collect::<String>(), c[c.len() - 2..].iter().collect::<String>())
}

fn find_secrets(text: &str, file: Option<&str>, out: &mut Vec<Value>) {
    for (lineno, line) in text.lines().enumerate() {
        for (rule, re) in secret_rules() {
            for m in re.find_iter(line) {
                let mut pairs = vec![
                    ("rule", Value::Str(rule.to_string().into())),
                    ("line", Value::Int(lineno as i64 + 1)),
                    ("column", Value::Int(line[..m.start()].chars().count() as i64 + 1)),
                    ("match", Value::Str(redact(m.as_str()).into())),
                ];
                if let Some(f) = file {
                    pairs.insert(0, ("file", Value::Str(f.to_string().into())));
                }
                out.push(map(pairs));
            }
        }
    }
}

/// scanSecrets(text) → [{ rule, line, column, match (redacted) }]
fn scan_secrets(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "security.scanSecrets")?;
    let text = str_arg(a, 0, "security.scanSecrets")?;
    let mut out = Vec::new();
    find_secrets(&text, None, &mut out);
    Ok(list(out))
}

/// scanFiles(path) → the same, with `file`, for a file or a folder (skips .git, node_modules, target, binaries)
fn scan_files(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "security.scanFiles")?;
    let root = std::path::PathBuf::from(str_arg(a, 0, "security.scanFiles")?);
    if !root.exists() {
        return Err(io_err!("security.scanFiles: {} not found", root.display()));
    }
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if matches!(name.as_str(), ".git" | "node_modules" | "target" | "dist" | ".venv" | "__pycache__") {
                continue;
            }
            if let Ok(rd) = std::fs::read_dir(&p) {
                let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                entries.sort();
                entries.reverse();
                stack.extend(entries);
            }
            continue;
        }
        let Ok(meta) = p.metadata() else { continue };
        if meta.len() > 2 * 1024 * 1024 {
            continue;
        }
        let Ok(data) = std::fs::read(&p) else { continue };
        if data.iter().take(8000).any(|b| *b == 0) {
            continue; // binary
        }
        find_secrets(&String::from_utf8_lossy(&data), Some(&p.to_string_lossy()), &mut out);
    }
    Ok(list(out))
}

// ---------------------------------------------------------------------
// signatures and comparisons
// ---------------------------------------------------------------------

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn key_bytes<const N: usize>(v: &Value, what: &str) -> Result<[u8; N], String> {
    let s = v.to_display_string();
    let raw = b64().decode(s.trim()).map_err(|_| format!("{}: the key must be base64", what))?;
    <[u8; N]>::try_from(raw.as_slice()).map_err(|_| format!("{}: the key must be {} bytes, got {}", what, N, raw.len()))
}

/// keypair() → { publicKey, privateKey } (Ed25519, base64)
fn keypair(a: &[Value]) -> Result<Value, String> {
    max_args(a, 0, "security.keypair")?;
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| io_err!("security.keypair: {}", e))?;
    let sk = ed25519_dalek::SigningKey::from_bytes(&seed);
    Ok(map(vec![
        ("publicKey", Value::Str(b64().encode(sk.verifying_key().to_bytes()).into())),
        ("privateKey", Value::Str(b64().encode(sk.to_bytes()).into())),
    ]))
}

/// sign(privateKey, message) → signature (base64)
fn sign(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.sign")?;
    use ed25519_dalek::Signer;
    let sk = ed25519_dalek::SigningKey::from_bytes(&key_bytes::<32>(&a[0], "security.sign")?);
    let msg = crate::native::util::bytes_of(&a[1], "security.sign")?;
    Ok(Value::Str(b64().encode(sk.sign(&msg).to_bytes()).into()))
}

/// verify(publicKey, message, signature) → bool
fn verify(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "security.verify")?;
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes::<32>(&a[0], "security.verify")?).map_err(|e| format!("security.verify: {}", e))?;
    let msg = crate::native::util::bytes_of(&a[1], "security.verify")?;
    let Ok(sig) = key_bytes::<64>(&a[2], "security.verify") else { return Ok(Value::Bool(false)) };
    let sig = ed25519_dalek::Signature::from_bytes(&sig);
    Ok(Value::Bool(vk.verify_strict(&msg, &sig).is_ok()))
}

/// constantTimeEq(a, b) → bool, without leaking where they differ
fn constant_time_eq(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.constantTimeEq")?;
    let x = crate::native::util::bytes_of(&a[0], "security.constantTimeEq")?;
    let y = crate::native::util::bytes_of(&a[1], "security.constantTimeEq")?;
    Ok(Value::Bool(x.len() == y.len() && bool::from(subtle::ConstantTimeEq::ct_eq(x.as_slice(), y.as_slice()))))
}

// ---------------------------------------------------------------------
// IP ranges
// ---------------------------------------------------------------------

fn net_arg(v: &Value, what: &str) -> Result<ipnet::IpNet, String> {
    let s = v.to_display_string();
    s.trim()
        .parse::<ipnet::IpNet>()
        .or_else(|_| s.trim().parse::<IpAddr>().map(ipnet::IpNet::from))
        .map_err(|_| format!("{}: invalid CIDR '{}'", what, s))
}

fn ip_arg(v: &Value, what: &str) -> Result<IpAddr, String> {
    let s = v.to_display_string();
    s.trim().parse().map_err(|_| format!("{}: invalid IP address '{}'", what, s))
}

/// cidr("10.0.0.0/24") → { network, broadcast, netmask, prefix, size, first, last, version }
fn cidr(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "security.cidr")?;
    let n = net_arg(&a[0], "security.cidr")?.trunc();
    let size: Value = match n {
        ipnet::IpNet::V4(v) => Value::Int(1i64 << (32 - v.prefix_len())),
        ipnet::IpNet::V6(v) if v.prefix_len() >= 66 => Value::Int(1i64 << (128 - v.prefix_len())),
        ipnet::IpNet::V6(v) => Value::Str(format!("2^{}", 128 - v.prefix_len()).into()),
    };
    let mut hosts = n.hosts();
    let first = hosts.next().map(|h| h.to_string());
    let last = match n {
        ipnet::IpNet::V4(v) if v.prefix_len() < 31 => Some(std::net::Ipv4Addr::from(u32::from(v.broadcast()) - 1).to_string()),
        _ => Some(n.broadcast().to_string()),
    };
    Ok(map(vec![
        ("network", Value::Str(n.network().to_string().into())),
        ("broadcast", Value::Str(n.broadcast().to_string().into())),
        ("netmask", Value::Str(n.netmask().to_string().into())),
        ("prefix", Value::Int(n.prefix_len() as i64)),
        ("size", size),
        ("first", first.map(Value::str).unwrap_or(Value::Nil)),
        ("last", last.map(Value::str).unwrap_or(Value::Nil)),
        ("version", Value::Int(if n.network().is_ipv4() { 4 } else { 6 })),
        ("cidr", Value::Str(n.to_string().into())),
    ]))
}

/// inCidr(ip, cidr | [cidrs]) → bool
fn in_cidr(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.inCidr")?;
    let ip = ip_arg(&a[0], "security.inCidr")?;
    let nets = match &a[1] {
        Value::List(l) => l.borrow().iter().map(|v| net_arg(v, "security.inCidr")).collect::<Result<Vec<_>, _>>()?,
        v => vec![net_arg(v, "security.inCidr")?],
    };
    Ok(Value::Bool(nets.iter().any(|n| n.contains(&ip))))
}

/// isPrivate(ip) → true for private, loopback, link-local, CGNAT and unique-local addresses
fn is_private(a: &[Value]) -> Result<Value, String> {
    max_args(a, 1, "security.isPrivate")?;
    Ok(Value::Bool(match ip_arg(&a[0], "security.isPrivate")? {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || (v.octets()[0] == 100 && (64..128).contains(&v.octets()[1])),
        IpAddr::V6(v) => v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80,
    }))
}

/// hosts(cidr, { limit = 65536 }) → the usable addresses
fn hosts(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.hosts")?;
    let n = net_arg(&a[0], "security.hosts")?.trunc();
    let o = opts(a.get(1), "security.hosts")?;
    let limit = int_opt(&o, "limit", 65_536).max(0) as usize;
    Ok(list(n.hosts().take(limit).map(|h| Value::Str(h.to_string().into())).collect()))
}

// ---------------------------------------------------------------------
// rate limiting
// ---------------------------------------------------------------------

/// A token bucket per key: `rate` tokens per second, up to `burst`.
pub struct RateLimiter {
    rate: f64,
    burst: f64,
    buckets: HashMap<String, (f64, Instant)>,
}

impl RateLimiter {
    fn refill(&mut self, key: &str) -> &mut (f64, Instant) {
        let now = Instant::now();
        let (rate, burst) = (self.rate, self.burst);
        let b = self.buckets.entry(key.to_string()).or_insert((burst, now));
        b.0 = (b.0 + now.duration_since(b.1).as_secs_f64() * rate).min(burst);
        b.1 = now;
        b
    }
}

/// rateLimiter(ratePerSecond, burst) → RateLimiter
fn rate_limiter(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "security.rateLimiter")?;
    let num = |v: &Value| match v {
        Value::Int(i) if *i > 0 => Ok(*i as f64),
        Value::Float(f) if *f > 0.0 => Ok(*f),
        other => Err(format!("security.rateLimiter(rate, burst): positive numbers, got {}", other.to_display_string())),
    };
    Ok(NativeObjectRef::new(RateLimiter { rate: num(&a[0])?, burst: num(&a[1])?, buckets: HashMap::new() }).to_value())
}

impl FoguObject for RateLimiter {
    fn name(&self) -> &str {
        "RateLimiter"
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let key = opt_arg(args, 0).map(|v| v.to_display_string()).unwrap_or_default();
        match method {
            // allow(key = "", cost = 1) → true if there were tokens (they are taken)
            "allow" => {
                max_args(args, 2, "allow")?;
                let cost = match args.get(1) {
                    Some(Value::Int(c)) => *c as f64,
                    Some(Value::Float(c)) => *c,
                    _ => 1.0,
                };
                let b = self.refill(&key);
                if b.0 >= cost {
                    b.0 -= cost;
                    Ok(Value::Bool(true))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            "remaining" => {
                max_args(args, 1, "remaining")?;
                Ok(Value::Int(self.refill(&key).0.floor() as i64))
            }
            "reset" => {
                max_args(args, 1, "reset")?;
                if args.is_empty() {
                    self.buckets.clear();
                } else {
                    self.buckets.remove(&key);
                }
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on RateLimiter (allow remaining reset)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["allow", "remaining", "reset"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_vectors() {
        // RFC 6238 appendix B (SHA-1, 8 digits)
        let key = b"12345678901234567890";
        assert_eq!(hotp(key, 59 / 30, 8), "94287082");
        assert_eq!(hotp(key, 1111111109 / 30, 8), "07081804");
        assert_eq!(base32_decode(&base32_encode(key)).unwrap(), key);
    }

    #[test]
    fn dates_and_ports() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(ports_arg(&Value::Str("22, 80,8000-8002".into())).unwrap(), vec![22, 80, 8000, 8001, 8002]);
        assert_eq!(split_host_port("https://example.com:8443/x", 443), ("example.com".into(), 8443));
        assert_eq!(split_host_port("[::1]:9", 443), ("::1".into(), 9));
    }

    #[test]
    fn redacts() {
        assert_eq!(redact("AKIAIOSFODNN7EXAMPLE"), "AKIA…LE");
    }
}
