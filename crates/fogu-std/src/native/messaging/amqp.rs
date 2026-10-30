//! An AMQP 0-9-1 client over the runtime's sockets (TCP or TLS through
//! native-tls), used by `fogu:rabbitmq`. One I/O thread owns the socket: it
//! writes the program's frames, sends heartbeats and dispatches what the
//! broker sends to the channel that waits for it (method replies, publisher
//! confirms, consumer deliveries).

use std::collections::{BTreeMap, VecDeque};
use ahash::AHashMap as HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------
// field values and tables
// ---------------------------------------------------------------------

/// A field value of an AMQP table.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Timestamp(u64),
    Array(Vec<Field>),
    Table(Table),
    Void,
}

pub type Table = Vec<(String, Field)>;

#[derive(Default)]
struct Enc(Vec<u8>);

impl Enc {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn short(&mut self, s: &str) -> &mut Self {
        let b = &s.as_bytes()[..s.len().min(255)];
        self.0.push(b.len() as u8);
        self.0.extend_from_slice(b);
        self
    }
    fn long(&mut self, b: &[u8]) -> &mut Self {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
        self
    }
    fn bits(&mut self, bits: &[bool]) -> &mut Self {
        let mut v = 0u8;
        for (i, b) in bits.iter().enumerate() {
            if *b {
                v |= 1 << i;
            }
        }
        self.u8(v)
    }
    fn field(&mut self, f: &Field) {
        match f {
            Field::Bool(b) => {
                self.u8(b't').u8(u8::from(*b));
            }
            Field::Int(n) => {
                self.u8(b'l').u64(*n as u64);
            }
            Field::Float(x) => {
                self.u8(b'd').u64(x.to_bits());
            }
            Field::Str(s) => {
                self.u8(b'S').long(s.as_bytes());
            }
            Field::Bytes(b) => {
                self.u8(b'x').long(b);
            }
            Field::Timestamp(t) => {
                self.u8(b'T').u64(*t);
            }
            Field::Array(a) => {
                let mut inner = Enc::default();
                for f in a {
                    inner.field(f);
                }
                self.u8(b'A').long(&inner.0);
            }
            Field::Table(t) => {
                self.u8(b'F').table(t);
            }
            Field::Void => {
                self.u8(b'V');
            }
        }
    }
    fn table(&mut self, t: &Table) -> &mut Self {
        let mut inner = Enc::default();
        for (k, v) in t {
            inner.short(k);
            inner.field(v);
        }
        self.long(&inner.0)
    }
}

struct Dec<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Dec<'a> {
    fn new(b: &'a [u8]) -> Self {
        Dec { b, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("amqp: truncated frame")?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, String> {
        let s = self.take(8)?;
        Ok(u64::from_be_bytes(s.try_into().expect("8 bytes")))
    }
    fn short(&mut self) -> Result<String, String> {
        let n = self.u8()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn long(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn field(&mut self) -> Result<Field, String> {
        Ok(match self.u8()? {
            b't' => Field::Bool(self.u8()? != 0),
            b'b' => Field::Int(self.u8()? as i8 as i64),
            b'B' => Field::Int(self.u8()? as i64),
            b's' => Field::Int(self.u16()? as i16 as i64),
            b'u' => Field::Int(self.u16()? as i64),
            b'I' => Field::Int(self.u32()? as i32 as i64),
            b'i' => Field::Int(self.u32()? as i64),
            b'l' | b'L' => Field::Int(self.u64()? as i64),
            b'f' => Field::Float(f32::from_bits(self.u32()?) as f64),
            b'd' => Field::Float(f64::from_bits(self.u64()?)),
            b'D' => {
                let scale = self.u8()?;
                let v = self.u32()? as i32 as f64;
                Field::Float(v / 10f64.powi(scale as i32))
            }
            b'S' => Field::Str(String::from_utf8_lossy(self.long()?).into_owned()),
            b'x' => Field::Bytes(self.long()?.to_vec()),
            b'T' => Field::Timestamp(self.u64()?),
            b'A' => {
                let raw = self.long()?;
                let mut d = Dec::new(raw);
                let mut a = Vec::new();
                while d.at < raw.len() {
                    a.push(d.field()?);
                }
                Field::Array(a)
            }
            b'F' => Field::Table(self.table()?),
            b'V' => Field::Void,
            other => return Err(format!("amqp: unknown field type '{}'", other as char)),
        })
    }
    fn table(&mut self) -> Result<Table, String> {
        let raw = self.long()?;
        let mut d = Dec::new(raw);
        let mut t = Vec::new();
        while d.at < raw.len() {
            let k = d.short()?;
            t.push((k, d.field()?));
        }
        Ok(t)
    }
}

// ---------------------------------------------------------------------
// frames
// ---------------------------------------------------------------------

const FRAME_METHOD: u8 = 1;
const FRAME_HEADER: u8 = 2;
const FRAME_BODY: u8 = 3;
const FRAME_HEARTBEAT: u8 = 8;
const FRAME_END: u8 = 0xce;

fn frame(kind: u8, ch: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 8);
    f.push(kind);
    f.extend_from_slice(&ch.to_be_bytes());
    f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    f.extend_from_slice(payload);
    f.push(FRAME_END);
    f
}

fn method(ch: u16, class: u16, m: u16, args: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(args.len() + 4);
    p.extend_from_slice(&class.to_be_bytes());
    p.extend_from_slice(&m.to_be_bytes());
    p.extend_from_slice(args);
    frame(FRAME_METHOD, ch, &p)
}

/// The properties of a message (`basic` class).
#[derive(Clone, Debug, Default)]
pub struct Props {
    pub content_type: Option<String>,
    pub content_encoding: Option<String>,
    pub headers: Option<Table>,
    pub delivery_mode: Option<u8>,
    pub priority: Option<u8>,
    pub correlation_id: Option<String>,
    pub reply_to: Option<String>,
    pub expiration: Option<String>,
    pub message_id: Option<String>,
    pub timestamp: Option<u64>,
    pub kind: Option<String>,
    pub user_id: Option<String>,
    pub app_id: Option<String>,
}

fn header_payload(size: u64, p: &Props) -> Vec<u8> {
    let mut flags = 0u16;
    let mut e = Enc::default();
    macro_rules! s {
        ($f:expr, $bit:expr) => {
            if let Some(v) = &$f {
                flags |= 1 << $bit;
                e.short(v);
            }
        };
    }
    s!(p.content_type, 15);
    s!(p.content_encoding, 14);
    if let Some(h) = &p.headers {
        flags |= 1 << 13;
        e.table(h);
    }
    if let Some(d) = p.delivery_mode {
        flags |= 1 << 12;
        e.u8(d);
    }
    if let Some(pr) = p.priority {
        flags |= 1 << 11;
        e.u8(pr);
    }
    s!(p.correlation_id, 10);
    s!(p.reply_to, 9);
    s!(p.expiration, 8);
    s!(p.message_id, 7);
    if let Some(t) = p.timestamp {
        flags |= 1 << 6;
        e.u64(t);
    }
    s!(p.kind, 5);
    s!(p.user_id, 4);
    s!(p.app_id, 3);
    let mut out = Enc::default();
    out.u16(60).u16(0).u64(size).u16(flags);
    out.0.extend(e.0);
    out.0
}

fn parse_header(b: &[u8]) -> Result<(u64, Props), String> {
    let mut d = Dec::new(b);
    d.u16()?;
    d.u16()?;
    let size = d.u64()?;
    let flags = d.u16()?;
    let has = |bit: u16| flags & (1 << bit) != 0;
    let mut p = Props::default();
    if has(15) {
        p.content_type = Some(d.short()?);
    }
    if has(14) {
        p.content_encoding = Some(d.short()?);
    }
    if has(13) {
        p.headers = Some(d.table()?);
    }
    if has(12) {
        p.delivery_mode = Some(d.u8()?);
    }
    if has(11) {
        p.priority = Some(d.u8()?);
    }
    if has(10) {
        p.correlation_id = Some(d.short()?);
    }
    if has(9) {
        p.reply_to = Some(d.short()?);
    }
    if has(8) {
        p.expiration = Some(d.short()?);
    }
    if has(7) {
        p.message_id = Some(d.short()?);
    }
    if has(6) {
        p.timestamp = Some(d.u64()?);
    }
    if has(5) {
        p.kind = Some(d.short()?);
    }
    if has(4) {
        p.user_id = Some(d.short()?);
    }
    if has(3) {
        p.app_id = Some(d.short()?);
    }
    Ok((size, p))
}

// ---------------------------------------------------------------------
// the connection
// ---------------------------------------------------------------------

/// A message delivered to a consumer or returned by `get`.
#[derive(Clone, Debug, Default)]
pub struct Delivery {
    pub channel: u16,
    pub delivery_tag: u64,
    pub redelivered: bool,
    pub exchange: String,
    pub routing_key: String,
    pub props: Props,
    pub body: Vec<u8>,
}

/// A method the broker answered with: class, method, arguments (and its content).
pub struct Reply {
    pub class: u16,
    pub method: u16,
    pub args: Vec<u8>,
    pub content: Option<Delivery>,
}

#[derive(Default)]
pub struct Inbox {
    pub deliveries: VecDeque<Delivery>,
    pub error: Option<String>,
}

enum Waiter {
    Method(Sender<Result<Reply, String>>),
}

enum Out {
    /// frames to write on a channel; the waiter gets the next synchronous reply
    Send { ch: u16, frames: Vec<u8>, waiter: Option<Waiter> },
    /// a publish on a confirm channel: answered by basic.ack / basic.nack
    Publish { ch: u16, frames: Vec<u8>, done: Sender<Result<(), String>> },
    /// deliveries of this consumer tag (on this channel) go to this inbox
    Consumer { tag: String, ch: u16, inbox: Arc<Mutex<Inbox>> },
    Close,
}

struct ChannelState {
    waiting: VecDeque<Waiter>,
    confirms: BTreeMap<u64, Sender<Result<(), String>>>,
    next_tag: u64,
    returned: Option<String>,
    /// a method with content, waiting for its header and body frames
    assembling: Option<(u16, u16, Vec<u8>, Delivery, u64)>,
}

impl ChannelState {
    fn new() -> Self {
        ChannelState { waiting: VecDeque::new(), confirms: BTreeMap::new(), next_tag: 1, returned: None, assembling: None }
    }

    fn fail(&mut self, e: &str) {
        for w in self.waiting.drain(..) {
            let Waiter::Method(tx) = w;
            let _ = tx.send(Err(e.to_string()));
        }
        for (_, tx) in std::mem::take(&mut self.confirms) {
            let _ = tx.send(Err(e.to_string()));
        }
    }
}

#[derive(Default)]
pub struct Status {
    pub connected: bool,
    pub error: Option<String>,
    /// channels the broker closed (they must be opened again)
    pub closed_channels: Vec<u16>,
}

/// A connection: send frames, wait for replies.
pub struct Conn {
    tx: Sender<Out>,
    pub status: Arc<Mutex<Status>>,
    pub frame_max: u32,
    next_channel: Mutex<u16>,
    channel_max: u16,
}

enum Wire {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Read for Wire {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(s) => s.read(b),
            Wire::Tls(s) => s.read(b),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(s) => s.write(b),
            Wire::Tls(s) => s.write(b),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Wire::Plain(s) => s.flush(),
            Wire::Tls(s) => s.flush(),
        }
    }
}

/// Reads one frame (blocking up to the socket's timeout): (type, channel, payload).
fn read_frame(w: &mut Wire, buf: &mut Vec<u8>) -> Result<Option<(u8, u16, Vec<u8>)>, String> {
    let mut chunk = [0u8; 16384];
    loop {
        if buf.len() >= 7 {
            let size = u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]) as usize;
            if buf.len() >= 8 + size {
                if buf[7 + size] != FRAME_END {
                    return Err("amqp: bad frame end".into());
                }
                let kind = buf[0];
                let ch = u16::from_be_bytes([buf[1], buf[2]]);
                let payload = buf[7..7 + size].to_vec();
                buf.drain(..8 + size);
                return Ok(Some((kind, ch, payload)));
            }
        }
        match w.read(&mut chunk) {
            Ok(0) => return Err("the broker closed the connection".into()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Waits for a method frame during the handshake.
fn expect(w: &mut Wire, buf: &mut Vec<u8>, class: u16, m: u16, deadline: Instant) -> Result<Vec<u8>, String> {
    loop {
        if Instant::now() > deadline {
            return Err("timed out during the handshake".into());
        }
        let Some((kind, _, p)) = read_frame(w, buf)? else { continue };
        if kind != FRAME_METHOD || p.len() < 4 {
            continue;
        }
        let (c, mm) = (u16::from_be_bytes([p[0], p[1]]), u16::from_be_bytes([p[2], p[3]]));
        if (c, mm) == (10, 50) {
            let mut d = Dec::new(&p[4..]);
            let code = d.u16()?;
            return Err(format!("{} {}", code, d.short()?));
        }
        if (c, mm) == (class, m) {
            return Ok(p[4..].to_vec());
        }
    }
}

pub struct Url {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub vhost: String,
    pub heartbeat: Option<u16>,
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `amqp[s]://user:pass@host:port/vhost?heartbeat=30`
pub fn parse_url(url: &str) -> Result<Url, String> {
    let (tls, rest) = match url.split_once("://") {
        Some(("amqp", r)) => (false, r),
        Some(("amqps", r)) => (true, r),
        _ => return Err(format!("expected an amqp:// or amqps:// url, got '{}'", url)),
    };
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (auth, hostpath) = match rest.rfind('@') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => ("guest:guest", rest),
    };
    let (user, pass) = auth.split_once(':').unwrap_or((auth, ""));
    let (hostport, vhost) = match hostpath.split_once('/') {
        Some((h, v)) => (h, if v.is_empty() { "/".to_string() } else { pct_decode(v) }),
        None => (hostpath, "/".to_string()),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h.trim_matches(['[', ']']).to_string(), p.parse().map_err(|_| format!("invalid port '{}'", p))?),
        _ => (hostport.trim_matches(['[', ']']).to_string(), if tls { 5671 } else { 5672 }),
    };
    let heartbeat = query.split('&').find_map(|kv| kv.strip_prefix("heartbeat=")).and_then(|v| v.parse().ok());
    Ok(Url { tls, host: if host.is_empty() { "localhost".into() } else { host }, port, user: pct_decode(user), pass: pct_decode(pass), vhost, heartbeat })
}

impl Conn {
    /// Connects, authenticates (PLAIN) and opens the virtual host.
    pub fn open(u: &Url, timeout: Duration) -> Result<Conn, String> {
        let addr = (u.host.as_str(), u.port).to_socket_addrs().map_err(|e| format!("{}: {}", u.host, e))?.next().ok_or_else(|| format!("{}: no address", u.host))?;
        let tcp = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("cannot connect to {}:{}: {}", u.host, u.port, e))?;
        tcp.set_nodelay(true).ok();
        tcp.set_read_timeout(Some(Duration::from_millis(50))).ok();
        let mut w = if u.tls {
            let c = native_tls::TlsConnector::new().map_err(|e| format!("TLS: {}", e))?;
            Wire::Tls(Box::new(c.connect(&u.host, tcp).map_err(|e| format!("TLS handshake: {}", e))?))
        } else {
            Wire::Plain(tcp)
        };
        let deadline = Instant::now() + timeout;
        let mut buf = Vec::new();
        w.write_all(b"AMQP\x00\x00\x09\x01").map_err(|e| e.to_string())?;
        expect(&mut w, &mut buf, 10, 10, deadline)?; // connection.start
        let caps: Table = ["publisher_confirms", "consumer_cancel_notify", "basic.nack", "exchange_exchange_bindings", "connection.blocked", "authentication_failure_close"]
            .iter()
            .map(|c| (c.to_string(), Field::Bool(true)))
            .collect();
        let props: Table = vec![
            ("product".into(), Field::Str("fogu".into())),
            ("version".into(), Field::Str(env!("CARGO_PKG_VERSION").into())),
            ("platform".into(), Field::Str("Rust".into())),
            ("connection_name".into(), Field::Str("fogu".into())),
            ("capabilities".into(), Field::Table(caps)),
        ];
        let mut e = Enc::default();
        e.table(&props).short("PLAIN").long(format!("\0{}\0{}", u.user, u.pass).as_bytes()).short("en_US");
        w.write_all(&method(0, 10, 11, &e.0)).map_err(|e| e.to_string())?;
        let tune = expect(&mut w, &mut buf, 10, 30, deadline).map_err(|e| if e.contains("403") || e.contains("ACCESS_REFUSED") { format!("access refused for user '{}' ({})", u.user, e) } else { e })?;
        let mut d = Dec::new(&tune);
        let (ch_max, fr_max, hb) = (d.u16()?, d.u32()?, d.u16()?);
        let channel_max = if ch_max == 0 { 2047 } else { ch_max.min(2047) };
        let frame_max = if fr_max == 0 { 131_072 } else { fr_max.min(131_072) };
        let heartbeat = u.heartbeat.unwrap_or(if hb == 0 { 60 } else { hb.min(60) });
        let mut e = Enc::default();
        e.u16(channel_max).u32(frame_max).u16(heartbeat);
        w.write_all(&method(0, 10, 31, &e.0)).map_err(|e| e.to_string())?;
        let mut e = Enc::default();
        e.short(&u.vhost).short("").u8(0);
        w.write_all(&method(0, 10, 40, &e.0)).map_err(|e| e.to_string())?;
        expect(&mut w, &mut buf, 10, 41, deadline).map_err(|e| format!("cannot open virtual host '{}': {}", u.vhost, e))?;
        let (tx, rx) = channel::<Out>();
        let status = Arc::new(Mutex::new(Status { connected: true, ..Default::default() }));
        let st = Arc::clone(&status);
        std::thread::Builder::new()
            .name("fogu-amqp".into())
            .spawn(move || io_loop(w, buf, rx, st, Duration::from_secs(heartbeat as u64)))
            .map_err(|e| e.to_string())?;
        Ok(Conn { tx, status, frame_max, next_channel: Mutex::new(0), channel_max })
    }

    /// Sends a method and waits for the broker's answer.
    pub fn call(&self, ch: u16, class: u16, m: u16, args: &[u8], timeout: Duration) -> Result<Reply, String> {
        let (tx, rx) = channel();
        self.tx.send(Out::Send { ch, frames: method(ch, class, m, args), waiter: Some(Waiter::Method(tx)) }).map_err(|_| "the connection is closed".to_string())?;
        rx.recv_timeout(timeout).map_err(|_| "no answer from the broker".to_string())?
    }

    /// Sends a method that has no answer (basic.ack…).
    pub fn cast(&self, ch: u16, class: u16, m: u16, args: &[u8]) -> Result<(), String> {
        self.tx.send(Out::Send { ch, frames: method(ch, class, m, args), waiter: None }).map_err(|_| "the connection is closed".to_string())
    }

    /// Opens a new channel (in confirm mode when asked).
    pub fn open_channel(&self, confirm: bool, timeout: Duration) -> Result<u16, String> {
        let ch = {
            let mut n = self.next_channel.lock().unwrap_or_else(|e| e.into_inner());
            *n = if *n >= self.channel_max { 1 } else { *n + 1 };
            *n
        };
        let mut e = Enc::default();
        e.short("");
        self.call(ch, 20, 10, &e.0, timeout)?;
        if confirm {
            self.call(ch, 85, 10, &[0], timeout)?;
        }
        Ok(ch)
    }

    pub fn close_channel(&self, ch: u16, timeout: Duration) {
        let mut e = Enc::default();
        e.u16(200).short("bye").u16(0).u16(0);
        let _ = self.call(ch, 20, 40, &e.0, timeout);
    }

    /// Publishes on a confirm channel and waits for the broker's ack.
    pub fn publish(&self, ch: u16, exchange: &str, key: &str, mandatory: bool, props: &Props, body: &[u8], timeout: Duration) -> Result<(), String> {
        let mut e = Enc::default();
        e.u16(0).short(exchange).short(key).bits(&[mandatory, false]);
        let mut frames = method(ch, 60, 40, &e.0);
        frames.extend(frame(FRAME_HEADER, ch, &header_payload(body.len() as u64, props)));
        let max = (self.frame_max as usize).saturating_sub(8).max(4096);
        for part in body.chunks(max) {
            frames.extend(frame(FRAME_BODY, ch, part));
        }
        let (tx, rx) = channel();
        self.tx.send(Out::Publish { ch, frames, done: tx }).map_err(|_| "the connection is closed".to_string())?;
        rx.recv_timeout(timeout).map_err(|_| "no confirmation from the broker".to_string())?
    }

    /// Routes the deliveries of a consumer tag to an inbox.
    pub fn register(&self, tag: &str, ch: u16, inbox: Arc<Mutex<Inbox>>) -> Result<(), String> {
        self.tx.send(Out::Consumer { tag: tag.to_string(), ch, inbox }).map_err(|_| "the connection is closed".to_string())
    }

    pub fn close(&self, timeout: Duration) -> Result<(), String> {
        let mut e = Enc::default();
        e.u16(200).short("bye").u16(0).u16(0);
        let r = self.call(0, 10, 50, &e.0, timeout).map(|_| ());
        let _ = self.tx.send(Out::Close);
        r
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        let _ = self.tx.send(Out::Close);
    }
}

pub fn enc() -> EncBuilder {
    EncBuilder(Enc::default())
}

/// A small public face of the encoder, for building method arguments.
pub struct EncBuilder(Enc);

impl EncBuilder {
    pub fn u8(mut self, v: u8) -> Self {
        self.0.u8(v);
        self
    }
    pub fn u16(mut self, v: u16) -> Self {
        self.0.u16(v);
        self
    }
    pub fn u32(mut self, v: u32) -> Self {
        self.0.u32(v);
        self
    }
    pub fn u64(mut self, v: u64) -> Self {
        self.0.u64(v);
        self
    }
    pub fn short(mut self, s: &str) -> Self {
        self.0.short(s);
        self
    }
    pub fn bits(mut self, b: &[bool]) -> Self {
        self.0.bits(b);
        self
    }
    pub fn table(mut self, t: &Table) -> Self {
        self.0.table(t);
        self
    }
    pub fn done(self) -> Vec<u8> {
        self.0 .0
    }
}

pub fn dec(b: &[u8]) -> DecReader<'_> {
    DecReader(Dec::new(b))
}

pub struct DecReader<'a>(Dec<'a>);

impl DecReader<'_> {
    pub fn u32(&mut self) -> Result<u32, String> {
        self.0.u32()
    }
    pub fn short(&mut self) -> Result<String, String> {
        self.0.short()
    }
}

fn io_loop(mut w: Wire, mut buf: Vec<u8>, rx: Receiver<Out>, status: Arc<Mutex<Status>>, heartbeat: Duration) {
    let mut channels: HashMap<u16, ChannelState> = HashMap::new();
    let mut consumers: HashMap<String, (u16, Arc<Mutex<Inbox>>)> = HashMap::new();
    let beat = (heartbeat / 2).max(Duration::from_secs(1));
    let mut last_sent = Instant::now();
    let mut last_seen = Instant::now();
    let fail_all = |channels: &mut HashMap<u16, ChannelState>, consumers: &HashMap<String, (u16, Arc<Mutex<Inbox>>)>, e: &str| {
        for c in channels.values_mut() {
            c.fail(e);
        }
        for (_, i) in consumers.values() {
            i.lock().unwrap_or_else(|p| p.into_inner()).error.get_or_insert_with(|| e.to_string());
        }
        crate::runtime::sched::wake();
    };
    let reason = 'conn: loop {
        // the program's frames
        loop {
            let out = match rx.try_recv() {
                Ok(o) => o,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'conn "closed".to_string(),
            };
            let (data, chn) = match out {
                Out::Close => break 'conn "closed".to_string(),
                Out::Consumer { tag, ch, inbox } => {
                    consumers.insert(tag, (ch, inbox));
                    continue;
                }
                Out::Send { ch, frames, waiter } => {
                    // a (re)opened channel starts afresh
                    if frames.len() > 11 && frames[7..11] == [0, 20, 0, 10] {
                        channels.insert(ch, ChannelState::new());
                    }
                    let c = channels.entry(ch).or_insert_with(ChannelState::new);
                    if let Some(wt) = waiter {
                        c.waiting.push_back(wt);
                    }
                    (frames, ch)
                }
                Out::Publish { ch, frames, done } => {
                    let c = channels.entry(ch).or_insert_with(ChannelState::new);
                    let tag = c.next_tag;
                    c.next_tag += 1;
                    c.confirms.insert(tag, done);
                    (frames, ch)
                }
            };
            let _ = chn;
            if let Err(e) = w.write_all(&data).and_then(|_| w.flush()) {
                break 'conn e.to_string();
            }
            last_sent = Instant::now();
        }
        if last_sent.elapsed() >= beat {
            if let Err(e) = w.write_all(&frame(FRAME_HEARTBEAT, 0, &[])) {
                break 'conn e.to_string();
            }
            last_sent = Instant::now();
        }
        if last_seen.elapsed() > heartbeat * 2 + Duration::from_secs(1) {
            break 'conn "missed heartbeats".to_string();
        }
        let (kind, ch, p) = match read_frame(&mut w, &mut buf) {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => break 'conn e,
        };
        last_seen = Instant::now();
        match kind {
            FRAME_HEARTBEAT => {}
            FRAME_METHOD if p.len() >= 4 => {
                let (class, m) = (u16::from_be_bytes([p[0], p[1]]), u16::from_be_bytes([p[2], p[3]]));
                let args = p[4..].to_vec();
                let c = channels.entry(ch).or_insert_with(ChannelState::new);
                match (class, m) {
                    // connection.close from the broker
                    (10, 50) => {
                        let mut d = Dec::new(&args);
                        let code = d.u16().unwrap_or(0);
                        let text = d.short().unwrap_or_default();
                        let _ = w.write_all(&method(0, 10, 51, &[]));
                        break 'conn format!("connection closed by the broker: {} {}", code, text);
                    }
                    // our connection.close was accepted
                    (10, 51) => {
                        if let Some(Waiter::Method(tx)) = c.waiting.pop_front() {
                            let _ = tx.send(Ok(Reply { class, method: m, args, content: None }));
                        }
                        break 'conn "closed".to_string();
                    }
                    // the broker closed this channel (an error on it)
                    (20, 40) => {
                        let mut d = Dec::new(&args);
                        let code = d.u16().unwrap_or(0);
                        let text = d.short().unwrap_or_default();
                        let _ = w.write_all(&method(ch, 20, 41, &[]));
                        let e = format!("{} {}", code, text);
                        c.fail(&e);
                        let mut s = status.lock().unwrap_or_else(|p| p.into_inner());
                        s.closed_channels.push(ch);
                        drop(s);
                        // its consumers end with the same error
                        for (cch, i) in consumers.values() {
                            if *cch == ch {
                                i.lock().unwrap_or_else(|p| p.into_inner()).error.get_or_insert_with(|| e.clone());
                            }
                        }
                        crate::runtime::sched::wake();
                    }
                    // publisher confirms: ack / nack (maybe multiple)
                    (60, 80) | (60, 120) => {
                        let mut d = Dec::new(&args);
                        let tag = d.u64().unwrap_or(0);
                        let multiple = d.u8().unwrap_or(0) & 1 == 1;
                        let tags: Vec<u64> = if multiple { c.confirms.range(..=tag).map(|(t, _)| *t).collect() } else { vec![tag] };
                        for t in tags {
                            if let Some(tx) = c.confirms.remove(&t) {
                                let r = if m == 120 {
                                    Err("the broker rejected the message".to_string())
                                } else if let Some(ret) = c.returned.take() {
                                    Err(format!("message returned: {}", ret))
                                } else {
                                    Ok(())
                                };
                                let _ = tx.send(r);
                            }
                        }
                    }
                    // a method with content: wait for its header and body
                    (60, 60) | (60, 71) | (60, 50) => {
                        c.assembling = Some((class, m, args, Delivery { channel: ch, ..Default::default() }, 0));
                    }
                    // the broker cancelled a consumer (queue deleted…)
                    (60, 30) => {
                        let tag = Dec::new(&args).short().unwrap_or_default();
                        if let Some((_, i)) = consumers.remove(&tag) {
                            i.lock().unwrap_or_else(|p| p.into_inner()).error = Some("consumer cancelled by the broker".into());
                            crate::runtime::sched::wake();
                        }
                    }
                    _ => {
                        if let Some(Waiter::Method(tx)) = c.waiting.pop_front() {
                            let _ = tx.send(Ok(Reply { class, method: m, args, content: None }));
                        }
                    }
                }
            }
            FRAME_HEADER => {
                let c = channels.entry(ch).or_insert_with(ChannelState::new);
                if let Some((_, _, _, d, size)) = c.assembling.as_mut() {
                    if let Ok((s, props)) = parse_header(&p) {
                        *size = s;
                        d.props = props;
                    }
                }
                complete(c, &consumers);
            }
            FRAME_BODY => {
                let c = channels.entry(ch).or_insert_with(ChannelState::new);
                if let Some((_, _, _, d, _)) = c.assembling.as_mut() {
                    d.body.extend_from_slice(&p);
                }
                complete(c, &consumers);
            }
            _ => {}
        }
    };
    let mut s = status.lock().unwrap_or_else(|p| p.into_inner());
    s.connected = false;
    if reason != "closed" {
        s.error = Some(reason.clone());
    }
    drop(s);
    fail_all(&mut channels, &consumers, if reason == "closed" { "the connection is closed" } else { &reason });
}

/// A content method whose body is complete goes where it belongs.
fn complete(c: &mut ChannelState, consumers: &HashMap<String, (u16, Arc<Mutex<Inbox>>)>) {
    let Some((_, _, _, d, size)) = &c.assembling else { return };
    if (d.body.len() as u64) < *size {
        return;
    }
    let (class, m, args, mut d, _) = c.assembling.take().expect("checked");
    let mut a = Dec::new(&args);
    match m {
        // basic.deliver: consumer-tag, delivery-tag, redelivered, exchange, routing-key
        60 => {
            let tag = a.short().unwrap_or_default();
            d.delivery_tag = a.u64().unwrap_or(0);
            d.redelivered = a.u8().unwrap_or(0) & 1 == 1;
            d.exchange = a.short().unwrap_or_default();
            d.routing_key = a.short().unwrap_or_default();
            if let Some((_, i)) = consumers.get(&tag) {
                i.lock().unwrap_or_else(|p| p.into_inner()).deliveries.push_back(d);
                crate::runtime::sched::wake();
            }
        }
        // basic.get-ok: delivery-tag, redelivered, exchange, routing-key, message-count
        71 => {
            d.delivery_tag = a.u64().unwrap_or(0);
            d.redelivered = a.u8().unwrap_or(0) & 1 == 1;
            d.exchange = a.short().unwrap_or_default();
            d.routing_key = a.short().unwrap_or_default();
            if let Some(Waiter::Method(tx)) = c.waiting.pop_front() {
                let _ = tx.send(Ok(Reply { class, method: m, args: Vec::new(), content: Some(d) }));
            }
        }
        // basic.return: reply-code, reply-text (before the publish's ack)
        50 => {
            let code = a.u16().unwrap_or(0);
            let text = a.short().unwrap_or_default();
            c.returned = Some(format!("{} {}", code, text));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_round_trip() {
        let t: Table = vec![
            ("a".into(), Field::Bool(true)),
            ("n".into(), Field::Int(-7)),
            ("f".into(), Field::Float(1.5)),
            ("s".into(), Field::Str("olá".into())),
            ("l".into(), Field::Array(vec![Field::Int(1), Field::Str("x".into())])),
            ("t".into(), Field::Table(vec![("k".into(), Field::Void)])),
        ];
        let mut e = Enc::default();
        e.table(&t);
        assert_eq!(Dec::new(&e.0).table().unwrap(), t);
    }

    #[test]
    fn urls_and_headers() {
        let u = parse_url("amqps://ana:p%40ss@mq.example.com/prod%2Fv1?heartbeat=10").unwrap();
        assert_eq!((u.tls, u.host.as_str(), u.port, u.user.as_str(), u.pass.as_str(), u.vhost.as_str(), u.heartbeat), (true, "mq.example.com", 5671, "ana", "p@ss", "prod/v1", Some(10)));
        assert_eq!(parse_url("amqp://h/%2f").unwrap().vhost, "/");
        let p = Props { content_type: Some("application/json".into()), delivery_mode: Some(2), message_id: Some("m1".into()), headers: Some(vec![("x".into(), Field::Int(1))]), ..Default::default() };
        let (size, q) = parse_header(&header_payload(42, &p)).unwrap();
        assert_eq!((size, q.content_type.as_deref(), q.delivery_mode, q.message_id.as_deref()), (42, Some("application/json"), Some(2), Some("m1")));
        assert_eq!(q.headers, p.headers);
    }
}
