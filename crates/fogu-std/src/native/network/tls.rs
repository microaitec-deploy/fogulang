//! `fogu:tls` module — TLS (rustls) clients and servers over TCP.
//!
//! ```fogu
//! import tls from "fogu:tls"
//!
//! const pair = tls.selfSigned(["localhost", "127.0.0.1"])?    // { cert, key } (PEM)
//! const server = tls.listen("127.0.0.1:8443", { cert: pair.cert, key: pair.key })?
//! fg fn() {
//!     for conn in server {                                  // accept() in a loop
//!         conn.writeLine("hello " + conn.readLine()?)?
//!         conn.close()
//!     }
//! }()
//!
//! const c = tls.connect("127.0.0.1:8443", { ca: pair.cert })?
//! c.writeLine("tls")?
//! print(c.readLine()?, c.protocol)                          // hello tls TLSv1.3
//! ```
//!
//! A `TlsStream` has every method of `fogu:net`'s `TcpStream` (`read`,
//! `readExact`, `readLine`, `readText`, `readAll`, `write`, `writeLine`,
//! `close`, `setTimeout`) plus the fields `protocol`, `cipher`, `alpn`,
//! `serverName` and `peerCert` (the description of `security.parseCert`).
//! A `TlsListener` works like a `TcpListener`; handshakes run in the
//! background, so a slow or failed client never blocks `accept`.
//! `upgrade` / `upgradeServer` start TLS on a plain `TcpStream` (STARTTLS).
//! Certificates and keys are PEM text, a buffer, or the path of a PEM file.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ahash::AHashMap as HashMap;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::net::{closed, op, opt_timeout, TcpStreamValue};
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;
use crate::native::util::{max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    e.insert("listen".into(), nfn_var("listen", 2, listen));
    e.insert("upgrade".into(), nfn_var("upgrade", 1, upgrade));
    e.insert("upgradeServer".into(), nfn_var("upgradeServer", 2, upgrade_server));
    e.insert("selfSigned".into(), nfn_var("selfSigned", 1, crate::native::security::self_signed));
    e.insert("parseCert".into(), nfn_var("parseCert", 1, crate::native::security::parse_cert));
    ModuleValue {
        name: "fogu:tls".into(),
        exports: RefCell::new(e),
    }
}

// ================= CONFIGURATION =================

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    static P: OnceLock<Arc<rustls::crypto::CryptoProvider>> = OnceLock::new();
    P.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider())).clone()
}

/// The system's trusted roots (loaded once).
fn native_roots() -> Arc<rustls::RootCertStore> {
    static R: OnceLock<Arc<rustls::RootCertStore>> = OnceLock::new();
    R.get_or_init(|| {
        let mut store = rustls::RootCertStore::empty();
        for c in rustls_native_certs::load_native_certs().certs {
            let _ = store.add(c);
        }
        Arc::new(store)
    })
    .clone()
}

/// PEM data: the text itself (it contains `-----BEGIN`), a buffer, or the path of a file.
fn pem_arg(v: &Value, what: &str) -> Result<Vec<u8>, String> {
    match v {
        Value::Str(s) if s.contains("-----BEGIN") => Ok(s.as_bytes().to_vec()),
        Value::Str(s) => std::fs::read(s.as_str()).map_err(|e| io_err!("{}: {}: {}", what, s.as_str(), e)),
        Value::Buffer(_) => crate::native::util::bytes_of(v, what),
        other => Err(format!("{}: expected PEM text, a buffer or a file path, got '{}'", what, other.type_name())),
    }
}

fn certs_of(pem: &[u8], what: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let certs = CertificateDer::pem_slice_iter(pem).collect::<Result<Vec<_>, _>>().map_err(|e| io_err!("{}: invalid PEM: {:?}", what, e))?;
    if certs.is_empty() {
        return Err(io_err!("{}: no certificate in the PEM data", what));
    }
    Ok(certs)
}

fn key_of(pem: &[u8], what: &str) -> Result<PrivateKeyDer<'static>, String> {
    PrivateKeyDer::from_pem_slice(pem).map_err(|e| io_err!("{}: no usable private key in the PEM data: {:?}", what, e))
}

fn roots_of(pem: &[u8], what: &str) -> Result<rustls::RootCertStore, String> {
    let mut store = rustls::RootCertStore::empty();
    for c in certs_of(pem, what)? {
        store.add(c).map_err(|e| io_err!("{}: {}", what, e))?;
    }
    Ok(store)
}

fn alpn_of(v: &Value, what: &str) -> Result<Vec<Vec<u8>>, String> {
    match v {
        Value::Str(s) => Ok(vec![s.as_bytes().to_vec()]),
        Value::List(l) => l
            .borrow()
            .iter()
            .map(|p| match p {
                Value::Str(s) => Ok(s.as_bytes().to_vec()),
                other => Err(format!("{}: alpn must list strings, got '{}'", what, other.type_name())),
            })
            .collect(),
        other => Err(format!("{}: alpn must be a str or a list of str, got '{}'", what, other.type_name())),
    }
}

/// The options map (copied), checking for unknown keys.
fn options(v: Option<&Value>, what: &str, known: &[&str]) -> Result<HashMap<String, Value>, String> {
    let o: HashMap<String, Value> = match v {
        None | Some(Value::Nil) => HashMap::new(),
        Some(Value::Map(m)) => m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(other) => return Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    };
    let mut keys: Vec<&String> = o.keys().collect();
    keys.sort();
    for k in keys {
        if !known.contains(&k.as_str()) {
            return Err(format!("{}: unknown option '{}' ({})", what, k, known.join(" ")));
        }
    }
    Ok(o)
}

const CLIENT_OPTS: &[&str] = &["verify", "ca", "serverName", "sni", "cert", "key", "alpn", "timeout"];
const SERVER_OPTS: &[&str] = &["cert", "key", "ca", "clientAuth", "alpn", "timeout"];

/// Accepts any server certificate (`verify: false`), still checking the
/// handshake signatures.
#[derive(Debug)]
struct NoVerify(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// A client configuration and the options that are not part of it.
struct Client {
    config: Arc<rustls::ClientConfig>,
    server_name: Option<String>,
    timeout: Duration,
}

/// `{ verify = true, ca, serverName (sni), cert, key, alpn, timeout = 30s }`
fn client_config(v: Option<&Value>, what: &str) -> Result<Client, String> {
    let o = options(v, what, CLIENT_OPTS)?;
    let builder = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| io_err!("{}: {}", what, e))?;
    let verify = o.get("verify").is_none_or(|v| v.is_truthy());
    let builder = if verify {
        let roots = match o.get("ca") {
            Some(ca) if !matches!(ca, Value::Nil) => Arc::new(roots_of(&pem_arg(ca, &format!("{} ca", what))?, &format!("{} ca", what))?),
            _ => native_roots(),
        };
        builder.with_root_certificates(roots)
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(NoVerify(provider())))
    };
    let mut config = match (o.get("cert"), o.get("key")) {
        (Some(c), Some(k)) => {
            let certs = certs_of(&pem_arg(c, &format!("{} cert", what))?, &format!("{} cert", what))?;
            let key = key_of(&pem_arg(k, &format!("{} key", what))?, &format!("{} key", what))?;
            builder.with_client_auth_cert(certs, key).map_err(|e| io_err!("{}: client certificate: {}", what, e))?
        }
        (None, None) => builder.with_no_client_auth(),
        _ => return Err(format!("{}: a client certificate needs both 'cert' and 'key'", what)),
    };
    if let Some(a) = o.get("alpn") {
        config.alpn_protocols = alpn_of(a, what)?;
    }
    let server_name = match o.get("serverName").or_else(|| o.get("sni")) {
        None | Some(Value::Nil) => None,
        Some(Value::Str(s)) => Some(s.to_string()),
        Some(other) => return Err(format!("{}: serverName must be a str, got '{}'", what, other.type_name())),
    };
    let timeout = opt_timeout(o.get("timeout"), what)?.unwrap_or(Duration::from_secs(30));
    Ok(Client { config: Arc::new(config), server_name, timeout })
}

/// A server configuration from `{ cert, key, ca, clientAuth = "require", alpn }`
/// (also the `tls` option of `http.serve` and `app.listen`, with `alpn`
/// defaulting to `default_alpn`).
pub(crate) fn server_config(v: &Value, what: &str, default_alpn: &[&str]) -> Result<Arc<rustls::ServerConfig>, String> {
    let (config, _) = server_config_timeout(Some(v), what, default_alpn)?;
    Ok(config)
}

fn server_config_timeout(v: Option<&Value>, what: &str, default_alpn: &[&str]) -> Result<(Arc<rustls::ServerConfig>, Duration), String> {
    let o = options(v, what, SERVER_OPTS)?;
    let (Some(c), Some(k)) = (o.get("cert"), o.get("key")) else {
        return Err(format!("{}: TLS needs {{ cert, key }} (PEM text or file paths; tls.selfSigned makes a pair)", what));
    };
    let certs = certs_of(&pem_arg(c, &format!("{} cert", what))?, &format!("{} cert", what))?;
    let key = key_of(&pem_arg(k, &format!("{} key", what))?, &format!("{} key", what))?;
    let builder = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| io_err!("{}: {}", what, e))?;
    let builder = match o.get("ca") {
        None | Some(Value::Nil) => builder.with_no_client_auth(),
        Some(ca) => {
            let roots = roots_of(&pem_arg(ca, &format!("{} ca", what))?, &format!("{} ca", what))?;
            let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider());
            let verifier = match o.get("clientAuth") {
                None | Some(Value::Nil) => verifier,
                Some(Value::Str(s)) if s.as_str() == "require" => verifier,
                Some(Value::Str(s)) if s.as_str() == "optional" => verifier.allow_unauthenticated(),
                Some(other) => return Err(format!("{}: clientAuth must be \"require\" or \"optional\", got {}", what, other.to_display_string())),
            };
            builder.with_client_cert_verifier(verifier.build().map_err(|e| io_err!("{}: ca: {}", what, e))?)
        }
    };
    let mut config = builder.with_single_cert(certs, key).map_err(|e| io_err!("{}: certificate/key: {}", what, e))?;
    config.alpn_protocols = match o.get("alpn") {
        Some(a) => alpn_of(a, what)?,
        None => default_alpn.iter().map(|p| p.as_bytes().to_vec()).collect(),
    };
    let timeout = opt_timeout(o.get("timeout"), what)?.unwrap_or(Duration::from_secs(10));
    Ok((Arc::new(config), timeout))
}

// ================= STREAMS =================

/// What a handshake negotiated (plain data: built on the runtime's threads).
struct Info {
    protocol: String,
    cipher: String,
    alpn: Option<String>,
    peer_cert: Option<Vec<u8>>,
    server_name: Option<String>,
    peer: String,
    local: String,
}

fn info(c: &rustls::CommonState, server_name: Option<String>, peer: String, local: String) -> Info {
    let protocol = match c.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3".to_string(),
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2".to_string(),
        Some(v) => format!("{:?}", v),
        None => "unknown".into(),
    };
    Info {
        protocol,
        cipher: c.negotiated_cipher_suite().map(|s| format!("{:?}", s.suite())).unwrap_or_default(),
        alpn: c.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned()),
        peer_cert: c.peer_certificates().and_then(|l| l.first()).map(|d| d.as_ref().to_vec()),
        server_name,
        peer,
        local,
    }
}

fn addrs(s: &TcpStream) -> (String, String) {
    (
        s.peer_addr().map(|a| a.to_string()).unwrap_or_default(),
        s.local_addr().map(|a| a.to_string()).unwrap_or_default(),
    )
}

/// A finished TLS stream as a `TlsStream` object (on the interpreter thread).
fn wrap<S>(s: S, i: Info) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let opt = |s: Option<String>| s.map(Value::str).unwrap_or(Value::Nil);
    let cert = i.peer_cert.as_deref().and_then(|der| crate::native::security::cert_value(der).ok()).unwrap_or(Value::Nil);
    let extra = vec![
        ("protocol", Value::str(i.protocol)),
        ("cipher", Value::str(i.cipher)),
        ("alpn", opt(i.alpn)),
        ("serverName", opt(i.server_name)),
        ("peerCert", cert),
    ];
    TcpStreamValue::wrap_io(s, "TlsStream", i.peer, i.local, extra)
}

type ClientStream = tokio_rustls::client::TlsStream<TcpStream>;
type ServerStream = tokio_rustls::server::TlsStream<TcpStream>;

/// Client handshake over a connected socket.
async fn client_handshake(tcp: TcpStream, config: Arc<rustls::ClientConfig>, name: ServerName<'static>) -> Result<(ClientStream, Info), String> {
    tcp.set_nodelay(true).ok();
    let (peer, local) = addrs(&tcp);
    let sni = match &name {
        ServerName::DnsName(d) => Some(d.as_ref().to_string()),
        _ => None,
    };
    let s = TlsConnector::from(config).connect(name, tcp).await.map_err(|e| format!("handshake failed: {}", e))?;
    let i = info(s.get_ref().1, sni, peer, local);
    Ok((s, i))
}

/// Server handshake over an accepted socket.
async fn server_handshake(tcp: TcpStream, acceptor: TlsAcceptor) -> Result<(ServerStream, Info), String> {
    tcp.set_nodelay(true).ok();
    let (peer, local) = addrs(&tcp);
    let s = acceptor.accept(tcp).await.map_err(|e| format!("handshake failed: {}", e))?;
    let c = s.get_ref().1;
    let i = info(c, c.server_name().map(str::to_string), peer, local);
    Ok((s, i))
}

fn server_name(name: &str, what: &str) -> Result<ServerName<'static>, String> {
    let name = name.trim_start_matches('[').trim_end_matches(']');
    ServerName::try_from(name.to_string()).map_err(|e| format!("{}: invalid server name '{}': {}", what, name, e))
}

/// The host of `"host:port"` / `"[::1]:port"`.
fn host_of(addr: &str) -> &str {
    if let Some(r) = addr.strip_prefix('[') {
        return r.split(']').next().unwrap_or(r);
    }
    addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr)
}

// ================= FUNCTIONS =================

/// `connect("host:port", options?)` → TlsStream
fn connect(args: &[Value]) -> Result<Value, String> {
    const W: &str = "tls.connect";
    max_args(args, 2, W)?;
    let addr = str_arg(args, 0, "tls.connect(addr, options?)")?;
    let c = client_config(args.get(1), W)?;
    let name = server_name(c.server_name.as_deref().unwrap_or(host_of(&addr)), W)?;
    let a = addr.clone();
    let r = rt::block_on(async move {
        tokio::time::timeout(c.timeout, async move {
            let tcp = TcpStream::connect(a).await.map_err(|e| e.to_string())?;
            client_handshake(tcp, c.config, name).await
        })
        .await
    })?;
    let (s, i) = r.map_err(|_| io_err!("{}: timed out connecting to {}", W, addr))?.map_err(|e| io_err!("{}: {}: {}", W, addr, e))?;
    Ok(wrap(s, i))
}

/// The plain socket of a `TcpStream` argument (taken out of it).
fn take_tcp(v: &Value, what: &str) -> Result<TcpStream, String> {
    let Value::Object(o) = v else {
        return Err(format!("{}: expects a TcpStream, got '{}'", what, v.type_name()));
    };
    let kind = o.name();
    if kind != "TcpStream" && kind != "TlsStream" {
        return Err(format!("{}: expects a TcpStream, got '{}'", what, kind));
    }
    let rc = o.inner();
    let mut b = rc.borrow_mut();
    match b.as_any_mut().downcast_mut::<TcpStreamValue>() {
        Some(s) => s.take_tcp(what),
        None => Err(format!("{}: expects a TcpStream, got '{}'", what, kind)),
    }
}

/// `upgrade(conn, options?)` → TlsStream: the client side of STARTTLS.
fn upgrade(args: &[Value]) -> Result<Value, String> {
    const W: &str = "tls.upgrade";
    max_args(args, 2, W)?;
    let c = client_config(args.get(1), W)?;
    let tcp = take_tcp(&args[0], W)?;
    let host = match &c.server_name {
        Some(n) => n.clone(),
        None => tcp.peer_addr().map(|a| a.ip().to_string()).map_err(|e| io_err!("{}: {}", W, e))?,
    };
    let name = server_name(&host, W)?;
    let r = rt::block_on(async move { tokio::time::timeout(c.timeout, client_handshake(tcp, c.config, name)).await })?;
    let (s, i) = r.map_err(|_| io_err!("{}: handshake timed out", W))?.map_err(|e| io_err!("{}: {}", W, e))?;
    Ok(wrap(s, i))
}

/// `upgradeServer(conn, { cert, key, ... })` → TlsStream: the server side of STARTTLS.
fn upgrade_server(args: &[Value]) -> Result<Value, String> {
    const W: &str = "tls.upgradeServer";
    max_args(args, 2, W)?;
    let (config, timeout) = server_config_timeout(args.get(1), W, &[])?;
    let tcp = take_tcp(&args[0], W)?;
    let acceptor = TlsAcceptor::from(config);
    let r = rt::block_on(async move { tokio::time::timeout(timeout, server_handshake(tcp, acceptor)).await })?;
    let (s, i) = r.map_err(|_| io_err!("{}: handshake timed out", W))?.map_err(|e| io_err!("{}: {}", W, e))?;
    Ok(wrap(s, i))
}

/// `listen("ip:port", { cert, key, ca, clientAuth, alpn, timeout })` → TlsListener
fn listen(args: &[Value]) -> Result<Value, String> {
    const W: &str = "tls.listen";
    max_args(args, 2, W)?;
    let addr = str_arg(args, 0, "tls.listen(addr, options)")?;
    let (config, hs_timeout) = server_config_timeout(args.get(1), W, &[])?;
    let a = addr.clone();
    let listener = rt::block_on(async move { tokio::net::TcpListener::bind(a).await })?.map_err(|e| io_err!("{}: {} at {}", W, e, addr))?;
    let local = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| "?".into());
    let (tx, rx) = tokio::sync::mpsc::channel::<(ServerStream, Info)>(128);
    let failures = Arc::new(Mutex::new((0i64, None::<String>)));
    let fails = Arc::clone(&failures);
    let acceptor = TlsAcceptor::from(config);
    // accepts and runs every handshake in its own task: `accept` takes finished ones
    let task = rt::tokio().spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { continue };
            let (tx, acceptor, fails) = (tx.clone(), acceptor.clone(), Arc::clone(&fails));
            tokio::spawn(async move {
                let r = tokio::time::timeout(hs_timeout, server_handshake(tcp, acceptor)).await;
                match r.unwrap_or_else(|_| Err("handshake timed out".into())) {
                    Ok(done) => {
                        let _ = tx.send(done).await;
                    }
                    Err(e) => {
                        let mut f = fails.lock().unwrap_or_else(|e| e.into_inner());
                        f.0 += 1;
                        f.1 = Some(e);
                    }
                }
            });
        }
    });
    Ok(NativeObjectRef::new(TlsListenerValue {
        rx: Some(Arc::new(AsyncMutex::new(rx))),
        task: Some(task.abort_handle()),
        local,
        timeout: None,
        failures,
    })
    .to_value())
}

// ================= TLS LISTENER =================

pub struct TlsListenerValue {
    rx: Option<Arc<AsyncMutex<tokio::sync::mpsc::Receiver<(ServerStream, Info)>>>>,
    task: Option<tokio::task::AbortHandle>,
    local: String,
    timeout: Option<Duration>,
    /// failed handshakes: how many, and the last error
    failures: Arc<Mutex<(i64, Option<String>)>>,
}

impl TlsListenerValue {
    fn stop(&mut self) {
        self.rx = None;
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

impl Drop for TlsListenerValue {
    fn drop(&mut self) {
        self.stop();
    }
}

impl FoguObject for TlsListenerValue {
    fn name(&self) -> &str {
        "TlsListener"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "addr" | "localAddr" => Some(Value::Str(self.local.as_str().into())),
            "port" => Some(Value::Int(self.local.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0))),
            "failedHandshakes" => Some(Value::Int(self.failures.lock().unwrap_or_else(|e| e.into_inner()).0)),
            "lastError" => Some(self.failures.lock().unwrap_or_else(|e| e.into_inner()).1.clone().map(Value::str).unwrap_or(Value::Nil)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "close" => {
                self.stop();
                Ok(Value::Nil)
            }
            "setTimeout" => {
                max_args(args, 1, "setTimeout")?;
                self.timeout = opt_timeout(args.first(), "setTimeout")?;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on TlsListener (accept close setTimeout)", other)),
        }
    }

    /// `accept(timeout?)` → TlsStream (after a successful handshake)
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "accept" {
            return None;
        }
        let Some(rx) = self.rx.clone() else { return closed("accept") };
        let timeout = match (max_args(args, 1, "accept"), opt_timeout(args.first(), "accept")) {
            (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
            (_, Ok(t)) => t.or(self.timeout),
        };
        Some(Ok(op(
            "accept",
            timeout,
            async move { rx.lock().await.recv().await.ok_or_else(|| std::io::Error::other("the listener is closed")) },
            |(s, i)| wrap(s, i),
        )))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["accept", "close", "setTimeout"]
    }
}
