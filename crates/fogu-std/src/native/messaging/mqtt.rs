//! `fogu:mqtt` module — MQTT 3.1.1 client (our own, over the runtime's sockets): publish,
//! subscribe with `+`/`#` wildcards, QoS 0/1/2, retained messages, last will,
//! automatic reconnection (subscriptions are restored).
//!
//! ```fogu
//! import mqtt from "fogu:mqtt"
//!
//! const m = mqtt.connect("mqtt://127.0.0.1:1883", { clientId: "sensor-7" })?   // mqtts:// for TLS
//! m.subscribe("home/+/temperature", { qos: 1 })?
//! m.publish("home/kitchen/temperature", "21.5", { qos: 1, retain: true })?
//! const msg = m.next(2s)?                          // nil after 2s
//! print(msg.topic, msg.data, msg.qos, msg.retain)
//! ```
//!
//! MQTT 5: `connect(url, { version: 5, sessionExpiry })`; `publish(topic, data,
//! { contentType, responseTopic, correlationData, messageExpiry, userProperties })`;
//! `subscribe(filter, { qos, noLocal, retainAsPublished, retainHandling })`; the
//! message has `contentType responseTopic correlationData messageExpiry
//! userProperties`, and refusals carry the broker's reason.
//!
//! Network failures are `error` values; while disconnected the client keeps
//! retrying in the background (`m.connected` tells the state).

use std::cell::RefCell;
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, max_args, nfn_var, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 0, connect));
    e.insert(
        "matches".into(),
        nfn_var("matches", 2, |a| {
            max_args(a, 2, "matches")?;
            Ok(Value::Bool(topic_matches(&str_arg(a, 0, "matches")?, &str_arg(a, 1, "matches")?)))
        }),
    );
    ModuleValue {
        name: "fogu:mqtt".into(),
        exports: RefCell::new(e),
    }
}

/// `+` matches one level, `#` the rest (also the parent level).
pub fn topic_matches(filter: &str, topic: &str) -> bool {
    let f: Vec<&str> = filter.split('/').collect();
    let t: Vec<&str> = topic.split('/').collect();
    for (i, part) in f.iter().enumerate() {
        match *part {
            "#" => return true,
            "+" => {
                if i >= t.len() {
                    return false;
                }
            }
            p => {
                if t.get(i) != Some(&p) {
                    return false;
                }
            }
        }
    }
    f.len() == t.len()
}

fn qos(v: Option<&Value>) -> Result<QoS, String> {
    match v {
        None | Some(Value::Nil) | Some(Value::Int(0)) => Ok(QoS::AtMostOnce),
        Some(Value::Int(1)) => Ok(QoS::AtLeastOnce),
        Some(Value::Int(2)) => Ok(QoS::ExactlyOnce),
        Some(other) => Err(format!("mqtt: qos must be 0, 1 or 2, got {}", other.to_display_string())),
    }
}

fn payload(v: Option<&Value>) -> Vec<u8> {
    match v {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::Str(s)) => s.as_bytes().to_vec(),
        Some(other) => stringify_value(other).into_bytes(),
    }
}

// ---------------------------------------------------------------------
// the protocol (MQTT 3.1.1, our own)
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QoS {
    AtMostOnce = 0,
    AtLeastOnce = 1,
    ExactlyOnce = 2,
}

/// MQTT 5 properties of a message (ignored by 3.1.1).
#[derive(Clone, Default, Debug)]
struct Props {
    payload_utf8: Option<bool>,
    message_expiry: Option<u32>,
    content_type: Option<String>,
    response_topic: Option<String>,
    correlation: Option<Vec<u8>>,
    user: Vec<(String, String)>,
}

fn put_varint(b: &mut Vec<u8>, mut n: usize) {
    loop {
        let mut byte = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            byte |= 0x80;
        }
        b.push(byte);
        if n == 0 {
            break;
        }
    }
}

fn get_varint(b: &[u8], mut at: usize) -> (usize, usize) {
    let (mut n, mut mult) = (0usize, 1usize);
    while let Some(&byte) = b.get(at) {
        n += (byte & 0x7f) as usize * mult;
        mult *= 128;
        at += 1;
        if byte & 0x80 == 0 || mult > 128 * 128 * 128 {
            break;
        }
    }
    (n, at)
}

/// Properties with their length prefix.
fn encode_props(p: &Props) -> Vec<u8> {
    let mut b = Vec::new();
    if let Some(u) = p.payload_utf8 {
        b.extend_from_slice(&[0x01, u8::from(u)]);
    }
    if let Some(e) = p.message_expiry {
        b.push(0x02);
        b.extend_from_slice(&e.to_be_bytes());
    }
    if let Some(c) = &p.content_type {
        b.push(0x03);
        put_str(&mut b, c.as_bytes());
    }
    if let Some(r) = &p.response_topic {
        b.push(0x08);
        put_str(&mut b, r.as_bytes());
    }
    if let Some(c) = &p.correlation {
        b.push(0x09);
        put_str(&mut b, c);
    }
    for (k, v) in &p.user {
        b.push(0x26);
        put_str(&mut b, k.as_bytes());
        put_str(&mut b, v.as_bytes());
    }
    let mut out = Vec::new();
    put_varint(&mut out, b.len());
    out.extend(b);
    out
}

fn str_at(b: &[u8], at: usize) -> (Vec<u8>, usize) {
    let l = u16_at(b, at) as usize;
    (b.get(at + 2..at + 2 + l).unwrap_or(&[]).to_vec(), at + 2 + l)
}

/// Properties at `at` → (the message ones, the reason string if any, the offset after them).
fn decode_props(b: &[u8], at: usize) -> (Props, Option<String>, usize) {
    let (len, mut i) = get_varint(b, at);
    let end = (i + len).min(b.len());
    let mut p = Props::default();
    let mut reason = None;
    while i < end {
        let id = b[i];
        i += 1;
        match id {
            0x01 | 0x17 | 0x19 | 0x24 | 0x25 | 0x28 | 0x29 | 0x2a => {
                if id == 0x01 {
                    p.payload_utf8 = Some(b.get(i) == Some(&1));
                }
                i += 1;
            }
            0x02 | 0x11 | 0x18 | 0x27 => {
                if id == 0x02 {
                    p.message_expiry = b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
                }
                i += 4;
            }
            0x13 | 0x21 | 0x22 | 0x23 => i += 2,
            0x0b => i = get_varint(b, i).1,
            0x26 => {
                let (k, a) = str_at(b, i);
                let (v, a2) = str_at(b, a);
                p.user.push((String::from_utf8_lossy(&k).into_owned(), String::from_utf8_lossy(&v).into_owned()));
                i = a2;
            }
            _ => {
                let (v, a) = str_at(b, i);
                let text = String::from_utf8_lossy(&v).into_owned();
                match id {
                    0x03 => p.content_type = Some(text),
                    0x08 => p.response_topic = Some(text),
                    0x09 => p.correlation = Some(v),
                    0x1f => reason = Some(text),
                    _ => {}
                }
                i = a;
            }
        }
    }
    (p, reason, end)
}

/// MQTT 5 reason codes (≥ 0x80 are failures).
fn reason_text(code: u8) -> String {
    match code {
        0x80 => "unspecified error",
        0x81 => "malformed packet",
        0x82 => "protocol error",
        0x83 => "implementation specific error",
        0x84 => "unsupported protocol version",
        0x85 => "client identifier not valid",
        0x86 => "bad user name or password",
        0x87 => "not authorized",
        0x88 => "server unavailable",
        0x89 => "server busy",
        0x8a => "banned",
        0x8b => "server shutting down",
        0x8c => "bad authentication method",
        0x8d => "keep alive timeout",
        0x8e => "session taken over",
        0x8f => "topic filter invalid",
        0x90 => "topic name invalid",
        0x93 => "receive maximum exceeded",
        0x95 => "packet too large",
        0x97 => "quota exceeded",
        0x99 => "payload format invalid",
        0x9a => "retain not supported",
        0x9b => "QoS not supported",
        0x9c => "use another server",
        0x9d => "server moved",
        0x9f => "connection rate exceeded",
        0xa2 => "wildcard subscriptions not supported",
        _ => return format!("reason code 0x{:02x}", code),
    }
    .to_string()
}

/// Commands from the program to the connection thread.
enum Cmd {
    Publish { topic: String, data: Vec<u8>, qos: QoS, retain: bool, props: Props },
    /// filter, QoS, MQTT 5 subscription options (no local, retain as published, retain handling)
    Subscribe(String, QoS, u8),
    Unsubscribe(String),
    Disconnect,
}

#[derive(Clone)]
struct Will {
    topic: String,
    data: Vec<u8>,
    qos: QoS,
    retain: bool,
}

#[derive(Clone)]
struct Opts {
    /// MQTT 5 instead of 3.1.1
    v5: bool,
    session_expiry: Option<u32>,
    host: String,
    port: u16,
    tls: bool,
    client_id: String,
    clean: bool,
    keep_alive: Duration,
    user: Option<String>,
    pass: Option<String>,
    will: Option<Will>,
    timeout: Duration,
}

fn put_str(b: &mut Vec<u8>, s: &[u8]) {
    b.extend_from_slice(&(s.len() as u16).to_be_bytes());
    b.extend_from_slice(s);
}

/// A packet: fixed header + remaining length + body.
fn packet(first: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![first];
    let mut len = body.len();
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if len == 0 {
            break;
        }
    }
    out.extend_from_slice(body);
    out
}

fn connect_packet(o: &Opts) -> Vec<u8> {
    let mut b = Vec::new();
    put_str(&mut b, b"MQTT");
    b.push(if o.v5 { 5 } else { 4 });
    let mut flags = 0u8;
    if o.clean {
        flags |= 0x02;
    }
    if let Some(w) = &o.will {
        flags |= 0x04 | ((w.qos as u8) << 3) | if w.retain { 0x20 } else { 0 };
    }
    if o.user.is_some() {
        flags |= 0x80;
        if o.pass.is_some() {
            flags |= 0x40;
        }
    }
    b.push(flags);
    b.extend_from_slice(&(o.keep_alive.as_secs().min(65535) as u16).to_be_bytes());
    if o.v5 {
        let mut props = Vec::new();
        if let Some(e) = o.session_expiry {
            props.push(0x11);
            props.extend_from_slice(&e.to_be_bytes());
        }
        put_varint(&mut b, props.len());
        b.extend(props);
    }
    put_str(&mut b, o.client_id.as_bytes());
    if let Some(w) = &o.will {
        if o.v5 {
            b.push(0); // no will properties
        }
        put_str(&mut b, w.topic.as_bytes());
        put_str(&mut b, &w.data);
    }
    if let Some(u) = &o.user {
        put_str(&mut b, u.as_bytes());
        if let Some(p) = &o.pass {
            put_str(&mut b, p.as_bytes());
        }
    }
    packet(0x10, &b)
}

enum Wire {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Wire {
    fn tcp(&self) -> &TcpStream {
        match self {
            Wire::Plain(s) => s,
            Wire::Tls(s) => s.get_ref(),
        }
    }
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

fn dial(o: &Opts) -> Result<Wire, String> {
    let addr = (o.host.as_str(), o.port).to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("no address")?;
    let tcp = TcpStream::connect_timeout(&addr, o.timeout).map_err(|e| e.to_string())?;
    tcp.set_nodelay(true).ok();
    tcp.set_read_timeout(Some(o.timeout)).ok();
    if !o.tls {
        return Ok(Wire::Plain(tcp));
    }
    let c = native_tls::TlsConnector::new().map_err(|e| format!("TLS: {}", e))?;
    Ok(Wire::Tls(Box::new(c.connect(&o.host, tcp).map_err(|e| format!("TLS handshake: {}", e))?)))
}

/// Reads one packet, or None when the short read timeout passed first.
fn read_packet(w: &mut Wire, buf: &mut Vec<u8>) -> Result<Option<(u8, Vec<u8>)>, String> {
    let mut chunk = [0u8; 8192];
    loop {
        // a complete packet in the buffer?
        if buf.len() >= 2 {
            let (mut len, mut mult, mut i) = (0usize, 1usize, 1usize);
            let mut complete = false;
            while i < buf.len() && i <= 4 {
                len += (buf[i] & 0x7f) as usize * mult;
                mult *= 128;
                if buf[i] & 0x80 == 0 {
                    complete = true;
                    break;
                }
                i += 1;
            }
            if complete && buf.len() >= i + 1 + len {
                let first = buf[0];
                let body = buf[i + 1..i + 1 + len].to_vec();
                buf.drain(..i + 1 + len);
                return Ok(Some((first, body)));
            }
        }
        match w.read(&mut chunk) {
            Ok(0) => return Err("connection closed by the broker".into()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    b.get(i..i + 2).map(|s| u16::from_be_bytes([s[0], s[1]])).unwrap_or(0)
}

fn subscribe_packet(v5: bool, id: u16, topic: &str, q: QoS, flags: u8) -> Vec<u8> {
    let mut b = id.to_be_bytes().to_vec();
    if v5 {
        b.push(0); // no properties
    }
    put_str(&mut b, topic.as_bytes());
    b.push(q as u8 | if v5 { flags } else { 0 });
    packet(0x82, &b)
}

/// The connection thread: (re)connects, sends the program's commands,
/// answers the broker, keeps the session alive.
fn run(o: Opts, state: Arc<Mutex<State>>, cmds: std::sync::mpsc::Receiver<Cmd>) {
    let mut next_id: u16 = 0;
    let mut id = || {
        next_id = next_id.wrapping_add(1).max(1);
        next_id
    };
    let mut first = true;
    loop {
        let mut w = match dial(&o) {
            Ok(w) => w,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.last_error = Some(e.clone());
                if first {
                    s.first = Some(Err(e));
                    s.closed = true;
                    return;
                }
                if s.closing {
                    s.closed = true;
                    return;
                }
                drop(s);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let mut buf = Vec::new();
        let fail = |e: String, first: bool, state: &Arc<Mutex<State>>| {
            let mut s = state.lock().unwrap();
            s.connected = false;
            s.last_error = Some(e.clone());
            if first {
                s.first = Some(Err(e));
                s.closed = true;
            }
        };
        // CONNECT → CONNACK
        if let Err(e) = w.write_all(&connect_packet(&o)) {
            fail(e.to_string(), first, &state);
            if first {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let ack = loop {
            match read_packet(&mut w, &mut buf) {
                Ok(Some(p)) => break Ok(p),
                Ok(None) => continue,
                Err(e) => break Err(e),
            }
        };
        match ack {
            Ok((0x20, b)) if b.get(1) == Some(&0) => {}
            Ok((0x20, b)) => {
                const WHY: [&str; 6] = ["", "unacceptable protocol version", "identifier rejected", "server unavailable", "bad user name or password", "not authorized"];
                let code = b.get(1).copied().unwrap_or(0);
                let e = if o.v5 {
                    let (_, why, _) = decode_props(&b, 2);
                    format!("connection refused: {}{}", reason_text(code), why.map(|w| format!(" ({})", w)).unwrap_or_default())
                } else {
                    format!("connection refused: {}", WHY.get(code as usize).copied().unwrap_or("unknown reason"))
                };
                let mut s = state.lock().unwrap();
                s.last_error = Some(e.clone());
                s.first.get_or_insert(Err(e));
                s.closed = true;
                return;
            }
            Ok(_) | Err(_) => {
                let e = ack.err().unwrap_or_else(|| "no CONNACK".into());
                fail(e, first, &state);
                if first {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        }
        // connected: subscribe again (a new session after a reconnection)
        let subs = {
            let mut s = state.lock().unwrap();
            s.connected = true;
            s.first.get_or_insert(Ok(()));
            s.subscriptions.clone()
        };
        let again = !first;
        first = false;
        let _ = w.tcp().set_read_timeout(Some(Duration::from_millis(20)));
        let mut out: Vec<Vec<u8>> = Vec::new();
        if again {
            for (t, q, f) in subs {
                out.push(subscribe_packet(o.v5, id(), &t, q, f));
            }
        }
        let ping_every = (o.keep_alive / 2).max(Duration::from_secs(1));
        let mut last_sent = Instant::now();
        // QoS 2 messages received, waiting for PUBREL
        let mut qos2_in: std::collections::HashSet<u16> = std::collections::HashSet::new();
        let error = loop {
            // the program's commands
            loop {
                match cmds.try_recv() {
                    Ok(Cmd::Publish { topic, data, qos, retain, props }) => {
                        let mut b = Vec::new();
                        put_str(&mut b, topic.as_bytes());
                        if qos != QoS::AtMostOnce {
                            b.extend_from_slice(&id().to_be_bytes());
                        }
                        if o.v5 {
                            b.extend(encode_props(&props));
                        }
                        b.extend_from_slice(&data);
                        out.push(packet(0x30 | ((qos as u8) << 1) | u8::from(retain), &b));
                    }
                    Ok(Cmd::Subscribe(t, q, f)) => out.push(subscribe_packet(o.v5, id(), &t, q, f)),
                    Ok(Cmd::Unsubscribe(t)) => {
                        let mut b = id().to_be_bytes().to_vec();
                        if o.v5 {
                            b.push(0);
                        }
                        put_str(&mut b, t.as_bytes());
                        out.push(packet(0xa2, &b));
                    }
                    Ok(Cmd::Disconnect) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = w.write_all(&[0xe0, 0]);
                        let _ = w.flush();
                        let mut s = state.lock().unwrap();
                        s.connected = false;
                        s.closed = true;
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                }
            }
            if last_sent.elapsed() >= ping_every && out.is_empty() {
                out.push(vec![0xc0, 0]);
            }
            if !out.is_empty() {
                let data: Vec<u8> = out.drain(..).flatten().collect();
                if let Err(e) = w.write_all(&data).and_then(|_| w.flush()) {
                    break e.to_string();
                }
                last_sent = Instant::now();
            }
            // the broker's packets
            let (first_byte, body) = match read_packet(&mut w, &mut buf) {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(e) => break e,
            };
            match first_byte >> 4 {
                // PUBLISH
                3 => {
                    let qos = (first_byte >> 1) & 3;
                    let retain = first_byte & 1 == 1;
                    let tl = u16_at(&body, 0) as usize;
                    let topic = String::from_utf8_lossy(body.get(2..2 + tl).unwrap_or(&[])).into_owned();
                    let mut at = 2 + tl;
                    let pid = if qos > 0 {
                        at += 2;
                        u16_at(&body, at - 2)
                    } else {
                        0
                    };
                    let props = if o.v5 {
                        let (p, _, end) = decode_props(&body, at);
                        at = end;
                        p
                    } else {
                        Props::default()
                    };
                    let data = body.get(at..).unwrap_or(&[]).to_vec();
                    let deliver = match qos {
                        1 => {
                            out.push(packet(0x40, &pid.to_be_bytes()));
                            true
                        }
                        2 => {
                            out.push(packet(0x50, &pid.to_be_bytes()));
                            qos2_in.insert(pid)
                        }
                        _ => true,
                    };
                    if deliver {
                        state.lock().unwrap().inbox.push_back(Message { topic, data, qos, retain, props });
                        sched::wake();
                    }
                }
                // PUBREC (of our QoS 2 publish) → PUBREL
                5 => out.push(packet(0x62, &body[..2.min(body.len())])),
                // PUBREL (of a QoS 2 message we got) → PUBCOMP
                6 => {
                    qos2_in.remove(&u16_at(&body, 0));
                    out.push(packet(0x70, &body[..2.min(body.len())]));
                }
                // SUBACK: a code ≥ 0x80 is a refused subscription
                9 => {
                    let codes_at = if o.v5 { decode_props(&body, 2).2 } else { 2 };
                    if let Some(&c) = body.get(codes_at..).and_then(|c| c.iter().find(|c| **c >= 0x80)) {
                        let why = if o.v5 { reason_text(c) } else { "refused".into() };
                        state.lock().unwrap().last_error = Some(format!("subscription refused by the broker: {}", why));
                    }
                }
                // DISCONNECT from the broker (MQTT 5): its reason
                14 => {
                    let code = body.first().copied().unwrap_or(0);
                    let why = decode_props(&body, 1).1;
                    break format!("disconnected by the broker: {}{}", reason_text(code), why.map(|w| format!(" ({})", w)).unwrap_or_default());
                }
                // PUBACK, PUBCOMP, UNSUBACK, PINGRESP
                _ => {}
            }
        };
        let closing = {
            let mut s = state.lock().unwrap();
            s.connected = false;
            s.last_error = Some(error);
            if s.closing {
                s.closed = true;
            }
            s.closing
        };
        if closing {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

struct Message {
    topic: String,
    data: Vec<u8>,
    qos: u8,
    retain: bool,
    props: Props,
}

#[derive(Default)]
struct State {
    inbox: VecDeque<Message>,
    connected: bool,
    /// the first connection result (connect() waits for it)
    first: Option<Result<(), String>>,
    last_error: Option<String>,
    subscriptions: Vec<(String, QoS, u8)>,
    closing: bool,
    closed: bool,
}

fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "mqtt.connect")?;
    let url = match args.first() {
        None | Some(Value::Nil) => std::env::var("MQTT_URL").unwrap_or_else(|_| "mqtt://127.0.0.1:1883".into()),
        Some(Value::Str(u)) => u.to_string(),
        Some(other) => return Err(format!("mqtt.connect expects a url, got '{}'", other.type_name())),
    };
    let o: HashMap<String, Value> = match args.get(1) {
        None | Some(Value::Nil) => HashMap::new(),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(other) => return Err(format!("mqtt.connect: options must be a map, got '{}'", other.type_name())),
    };
    // mqtt[s]://[user:pass@]host[:port]
    let (tls, rest) = match url.split_once("://") {
        Some(("mqtt" | "tcp", r)) => (false, r),
        Some(("mqtts" | "ssl" | "tls", r)) => (true, r),
        _ => return Err(format!("mqtt.connect: expected mqtt:// or mqtts://, got '{}'", url)),
    };
    let rest = rest.trim_end_matches('/');
    let (auth, hostport) = match rest.rfind('@') {
        Some(i) => (Some(&rest[..i]), &rest[i + 1..]),
        None => (None, rest),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| format!("mqtt.connect: invalid port '{}'", p))?),
        None => (hostport.to_string(), if tls { 8883 } else { 1883 }),
    };
    let s = |k: &str| match o.get(k) {
        Some(Value::Str(v)) if !v.is_empty() => Some(v.to_string()),
        _ => None,
    };
    let client_id = s("clientId").unwrap_or_else(|| format!("fogu-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]).into());
    let keep_alive = match o.get("keepAlive") {
        None | Some(Value::Nil) => Duration::from_secs(30),
        Some(v) => duration_arg(v, "mqtt.connect: keepAlive")?,
    };
    let (user, pass) = match auth.map(|a| a.split_once(':').unwrap_or((a, ""))) {
        Some((u, p)) => (Some(crate::native::util::url_decode(u)), Some(crate::native::util::url_decode(p))),
        None => (None, None),
    };
    let user = s("username").or(user);
    let pass = s("password").or(pass).or_else(|| user.as_ref().map(|_| String::new()));
    let will = match o.get("will") {
        Some(Value::Map(w)) => {
            let w = w.borrow();
            let topic = match w.get("topic") {
                Some(Value::Str(t)) => t.clone(),
                _ => return Err("mqtt.connect: will needs a 'topic'".into()),
            };
            Some(Will { topic: topic.into_string(), data: payload(w.get("payload")), qos: qos(w.get("qos"))?, retain: w.get("retain").is_some_and(|v| v.is_truthy()) })
        }
        _ => None,
    };
    let timeout = match o.get("timeout") {
        None | Some(Value::Nil) => Duration::from_secs(10),
        Some(v) => duration_arg(v, "mqtt.connect: timeout")?,
    };
    let v5 = match o.get("version") {
        None | Some(Value::Nil) => false,
        Some(v) => match v.to_display_string().as_str() {
            "5" | "5.0" => true,
            "3" | "3.1.1" | "4" => false,
            other => return Err(format!("mqtt.connect: version must be 3 (3.1.1) or 5, got '{}'", other)),
        },
    };
    let session_expiry = match o.get("sessionExpiry") {
        None | Some(Value::Nil) => None,
        Some(Value::Int(n)) => Some((*n).clamp(0, u32::MAX as i64) as u32),
        Some(v) => Some(duration_arg(v, "mqtt.connect: sessionExpiry")?.as_secs().min(u32::MAX as u64) as u32),
    };
    let opts = Opts {
        v5,
        session_expiry,
        host: host.clone(),
        port,
        tls,
        client_id: client_id.to_string(),
        clean: o.get("clean").is_none_or(|v| v.is_truthy()),
        keep_alive: keep_alive.max(Duration::from_secs(5)),
        user,
        pass,
        will,
        timeout,
    };
    let state = Arc::new(Mutex::new(State::default()));
    let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
    {
        let state = Arc::clone(&state);
        std::thread::Builder::new().name("fogu-mqtt".into()).spawn(move || run(opts, state, rx)).map_err(|e| io_err!("mqtt: {}", e))?;
    }
    // wait for the first CONNACK (or failure)
    let deadline = Instant::now() + timeout + Duration::from_secs(1);
    loop {
        if let Some(r) = state.lock().unwrap().first.clone() {
            r.map_err(|e| io_err!("mqtt: cannot connect to {}:{}: {}", host, port, e))?;
            break;
        }
        if Instant::now() >= deadline {
            let _ = tx.send(Cmd::Disconnect);
            return Err(io_err!("mqtt: cannot connect to {}:{}: timed out", host, port));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(NativeObjectRef::new(MqttValue { v5, client: tx, state, client_id }).to_value())
}

pub struct MqttValue {
    v5: bool,
    client: std::sync::mpsc::Sender<Cmd>,
    state: Arc<Mutex<State>>,
    client_id: String,
}

impl MqttValue {
    fn check(&self) -> Result<(), String> {
        let s = self.state.lock().unwrap();
        if s.closing || s.closed {
            return Err(io_err!("mqtt: the client is closed"));
        }
        Ok(())
    }
}

fn message_value(m: Message) -> Value {
    NativeObjectRef::new(MessageValue { m }).to_value()
}

impl FoguObject for MqttValue {
    fn name(&self) -> &str {
        "Mqtt"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let s = self.state.lock().unwrap();
        Some(match field {
            "connected" => Value::Bool(s.connected),
            "clientId" => Value::Str(self.client_id.clone().into()),
            "version" => Value::Int(if self.v5 { 5 } else { 3 }),
            "lastError" => s.last_error.clone().map_or(Value::Nil, Value::str),
            "pending" => Value::Int(s.inbox.len() as i64),
            "subscriptions" => crate::native::util::str_list(s.subscriptions.iter().map(|(t, _, _)| t.clone())),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // publish(topic, data, { qos, retain })
            "publish" => {
                max_args(args, 3, "publish")?;
                self.check()?;
                let topic = str_arg(args, 0, "publish")?;
                if topic.contains(['+', '#']) {
                    return Err(format!("publish: '{}': wildcards are only for subscriptions", topic));
                }
                let o = match args.get(2) {
                    Some(Value::Map(m)) => m.borrow().clone(),
                    _ => HashMap::new(),
                };
                let retain = o.get("retain").is_some_and(|v| v.is_truthy());
                // MQTT 5 properties (ignored by 3.1.1)
                let text = |k: &str| o.get(k).filter(|v| !matches!(v, Value::Nil)).map(|v| v.to_display_string());
                let props = Props {
                    payload_utf8: matches!(args.get(1), Some(Value::Str(_))).then_some(true).filter(|_| o.contains_key("contentType")),
                    message_expiry: match o.get("messageExpiry") {
                        None | Some(Value::Nil) => None,
                        Some(Value::Int(n)) => Some(*n as u32),
                        Some(v) => Some(duration_arg(v, "publish: messageExpiry")?.as_secs() as u32),
                    },
                    content_type: text("contentType"),
                    response_topic: text("responseTopic"),
                    correlation: text("correlationData").map(String::into_bytes),
                    user: match o.get("userProperties") {
                        Some(Value::Map(m)) => {
                            let m = m.borrow();
                            let mut v: Vec<(String, String)> = m.iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect();
                            v.sort();
                            v
                        }
                        _ => Vec::new(),
                    },
                };
                self.client
                    .send(Cmd::Publish { topic, data: payload(args.get(1)), qos: qos(o.get("qos"))?, retain, props })
                    .map(|_| Value::Nil)
                    .map_err(|_| io_err!("mqtt: the client is closed"))
            }
            // subscribe(filter | [filters], { qos })
            "subscribe" | "unsubscribe" => {
                max_args(args, 2, method)?;
                self.check()?;
                let topics: Vec<String> = match args.first() {
                    Some(Value::Str(t)) => vec![t.to_string()],
                    Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
                    _ => return Err(format!("{} expects a topic filter or a list of them", method)),
                };
                let (q, flags) = match args.get(1) {
                    Some(Value::Map(m)) => {
                        let m = m.borrow();
                        // MQTT 5 subscription options
                        let on = |k: &str| m.get(k).is_some_and(|v| v.is_truthy());
                        let handling = match m.get("retainHandling") {
                            Some(Value::Int(n)) if (0..=2).contains(n) => *n as u8,
                            None | Some(Value::Nil) => 0,
                            Some(other) => return Err(format!("{}: retainHandling must be 0, 1 or 2, got {}", method, other.to_display_string())),
                        };
                        (qos(m.get("qos"))?, (u8::from(on("noLocal")) << 2) | (u8::from(on("retainAsPublished")) << 3) | (handling << 4))
                    }
                    _ => (QoS::AtMostOnce, 0),
                };
                let mut s = self.state.lock().unwrap();
                for t in topics {
                    if method == "subscribe" {
                        self.client.send(Cmd::Subscribe(t.clone(), q, flags)).map_err(|_| io_err!("mqtt: the client is closed"))?;
                        s.subscriptions.retain(|(x, _, _)| *x != t);
                        s.subscriptions.push((t, q, flags));
                    } else {
                        self.client.send(Cmd::Unsubscribe(t.clone())).map_err(|_| io_err!("mqtt: the client is closed"))?;
                        s.subscriptions.retain(|(x, _, _)| *x != t);
                    }
                }
                Ok(Value::Nil)
            }
            "tryNext" => {
                max_args(args, 0, "tryNext")?;
                Ok(self.state.lock().unwrap().inbox.pop_front().map_or(Value::Nil, message_value))
            }
            "close" | "disconnect" => {
                max_args(args, 0, method)?;
                self.state.lock().unwrap().closing = true;
                let _ = self.client.send(Cmd::Disconnect);
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Mqtt (publish subscribe unsubscribe next tryNext close)", other)),
        }
    }

    /// `next(timeout?)` → Message, nil on timeout.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        let deadline = match (max_args(args, 1, "next"), args.first()) {
            (Err(e), _) => return Some(Err(e)),
            (_, None | Some(Value::Nil)) => None,
            (_, Some(v)) => match duration_arg(v, "next") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
        };
        let state = Arc::clone(&self.state);
        Some(Ok(Box::new(move || {
            let mut s = state.lock().unwrap();
            if let Some(m) = s.inbox.pop_front() {
                return Some(Ok(message_value(m)));
            }
            if s.closed {
                return Some(Err(io_err!("mqtt: the client is closed")));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            sched::add_timer(Instant::now() + Duration::from_millis(2));
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["publish", "subscribe", "unsubscribe", "next", "tryNext", "close"]
    }
}

pub struct MessageValue {
    m: Message,
}

impl FoguObject for MessageValue {
    fn name(&self) -> &str {
        "MqttMessage"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "topic" => Value::Str(self.m.topic.clone().into()),
            "data" | "payload" => Value::Str(String::from_utf8_lossy(&self.m.data).into_owned().into()),
            "qos" => Value::Int(self.m.qos as i64),
            "retain" => Value::Bool(self.m.retain),
            "size" => Value::Int(self.m.data.len() as i64),
            // MQTT 5 properties (nil / empty with 3.1.1)
            "contentType" => self.m.props.content_type.clone().map_or(Value::Nil, Value::str),
            "responseTopic" => self.m.props.response_topic.clone().map_or(Value::Nil, Value::str),
            "correlationData" => self.m.props.correlation.as_ref().map_or(Value::Nil, |c| Value::Str(String::from_utf8_lossy(c).into_owned().into())),
            "messageExpiry" => self.m.props.message_expiry.map_or(Value::Nil, |e| Value::Int(e as i64)),
            "userProperties" => {
                let m: HashMap<String, Value> = self.m.props.user.iter().map(|(k, v)| (k.clone(), Value::Str(v.clone().into()))).collect();
                Value::Map(std::rc::Rc::new(RefCell::new(m.into_iter().collect())))
            }
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "json" => parse_value(&String::from_utf8_lossy(&self.m.data)),
            // matches(filter): does the topic match a subscription filter?
            "matches" => Ok(Value::Bool(topic_matches(&str_arg(_args, 0, "matches")?, &self.m.topic))),
            other => Err(format!("method '{}' does not exist on MqttMessage (json matches)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["json", "matches"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn props_and_varints() {
        let p = Props {
            payload_utf8: Some(true),
            message_expiry: Some(60),
            content_type: Some("application/json".into()),
            response_topic: Some("r/x".into()),
            correlation: Some(b"abc".to_vec()),
            user: vec![("k".into(), "v".into())],
        };
        let b = encode_props(&p);
        let (q, reason, end) = decode_props(&b, 0);
        assert_eq!(end, b.len());
        assert!(reason.is_none());
        assert_eq!((q.message_expiry, q.content_type.as_deref(), q.response_topic.as_deref()), (Some(60), Some("application/json"), Some("r/x")));
        assert_eq!((q.correlation, q.user), (Some(b"abc".to_vec()), vec![("k".to_string(), "v".to_string())]));
        let mut v = Vec::new();
        put_varint(&mut v, 321);
        assert_eq!(v, vec![0xc1, 0x02]);
        assert_eq!(get_varint(&v, 0), (321, 2));
        assert_eq!(packet(0xc0, &[]), vec![0xc0, 0]);
    }

    #[test]
    fn wildcards() {
        assert!(topic_matches("home/+/temp", "home/kitchen/temp"));
        assert!(!topic_matches("home/+/temp", "home/kitchen/hum"));
        assert!(topic_matches("home/#", "home/a/b/c"));
        assert!(topic_matches("home/#", "home"));
        assert!(!topic_matches("home/+", "home/a/b"));
        assert!(topic_matches("a/b", "a/b"));
    }
}
