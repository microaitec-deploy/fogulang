//! Blocking WebSocket client (`ws://`, `wss://`) for code that drives a
//! socket from its own thread: the `fogu:socketio` client and
//! `fogu:surrealdb`. Programs use `fogu:websocket`, which runs connections as
//! tasks on the shared runtime.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use tungstenite::WebSocket;

// ---------------------------------------------------------------------


pub enum Stream {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Stream {
    /// The TCP socket under the (maybe TLS) stream. Socket options must be
    /// set on it, not on a `try_clone` (a separate handle on Windows).
    pub(crate) fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(s) => s,
            Stream::Tls(s) => s.get_ref(),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

/// An open blocking client connection (`fogu:socketio` client, `fogu:surrealdb`).
pub(crate) struct Dialed {
    pub ws: WebSocket<Stream>,
}

/// Opens a `ws://` / `wss://` connection (TLS through native-tls).
pub(crate) fn dial(url: &str, extra_headers: &[(String, String)], protocols: Option<&str>, timeout: Duration) -> Result<Dialed, String> {
    let (tls, rest) = match url.split_once("://") {
        Some(("ws", r)) => (false, r),
        Some(("wss", r)) => (true, r),
        _ => return Err(format!("websocket.connect: expected a ws:// or wss:// url, got '{}'", url)),
    };
    let hostport = rest.split(['/', '?']).next().unwrap_or("");
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => {
            (h.trim_matches(['[', ']']).to_string(), p.parse::<u16>().map_err(|_| format!("websocket.connect: invalid port '{}'", p))?)
        }
        _ => (hostport.trim_matches(['[', ']']).to_string(), if tls { 443 } else { 80 }),
    };
    use tungstenite::client::IntoClientRequest;
    let mut req = url.into_client_request().map_err(|e| format!("websocket.connect: {}", e))?;
    for (k, v) in extra_headers {
        let name = tungstenite::http::HeaderName::from_bytes(k.as_bytes()).map_err(|_| format!("websocket.connect: invalid header '{}'", k))?;
        let value = tungstenite::http::HeaderValue::from_str(v).map_err(|_| format!("websocket.connect: invalid value for '{}'", k))?;
        req.headers_mut().insert(name, value);
    }
    if let Some(p) = protocols {
        req.headers_mut().insert("Sec-WebSocket-Protocol", p.parse().map_err(|_| "websocket.connect: invalid protocols")?);
    }
    let addr = format!("{}:{}", host, port)
        .to_socket_addrs()
        .map_err(|e| io_err!("websocket: cannot resolve {}: {}", host, e))?
        .next()
        .ok_or_else(|| io_err!("websocket: cannot resolve {}", host))?;
    let tcp = TcpStream::connect_timeout(&addr, timeout).map_err(|e| io_err!("websocket: cannot connect to {}: {}", url, e))?;
    tcp.set_read_timeout(Some(timeout)).ok();
    let stream = if tls {
        let c = native_tls::TlsConnector::new().map_err(|e| io_err!("websocket: TLS: {}", e))?;
        Stream::Tls(Box::new(c.connect(&host, tcp).map_err(|e| io_err!("websocket: TLS handshake: {}", e))?))
    } else {
        Stream::Plain(tcp)
    };
    let (ws, _) = tungstenite::client(req, stream).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(tungstenite::Error::Http(r)) => {
            io_err!("websocket: handshake rejected ({})", r.status())
        }
        other => io_err!("websocket: handshake failed: {}", other),
    })?;
    Ok(Dialed { ws })
}
