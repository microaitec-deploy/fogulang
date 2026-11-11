//! `fogu:webrtc` module — peer-to-peer data channels (WebRTC) with other Fogu
//! programs and with browsers, on top of [str0m](https://github.com/algesten/str0m)
//! (sans-IO: ICE, DTLS, SCTP; pure-Rust crypto).
//!
//! ```fogu
//! import webrtc from "fogu:webrtc"
//!
//! const a = webrtc.peer()
//! const b = webrtc.peer()
//! const ch = a.channel("chat")
//! const answer = b.acceptOffer(a.createOffer()?)?
//! a.acceptAnswer(answer)?
//! const other = b.accept(5s)          // the channel `a` opened
//! ch.send("hi")
//! print(other.recv(5s))               // hi
//! ```
//!
//! Every peer is a task on the shared tokio runtime: one UDP socket per local
//! address feeds packets to the `Rtc`, which is driven by its timeouts. The
//! program talks to it through a mutex (each mutation is followed by a full
//! drain of `poll_output`, str0m's contract); waits (`recv`, `accept`, `wait`)
//! are cooperative and woken by the runtime. Handlers registered with `on` run
//! in `fg` tasks started by a pump task of the peer (Fogu values never leave the
//! interpreter thread).
//!
//! The signaling helpers (`signaling`, `signalingServer`, `connect`,
//! `serveHttp`, `connectHttp`) are Fogu code in `webrtc.fg`, run once per
//! interpreter; the browser side is `webrtc_client.js`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ahash::AHashMap as HashMap;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelConfig, ChannelId, Reliability};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Notify;

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::lexer::Lexer;
use crate::runtime::parser::Parser;
use crate::runtime::sched::{self, IoGuard};
use crate::runtime::value::{ModuleValue, NativeFnValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;
use crate::native::util::{bytes_of, duration_arg, list, max_args, str_arg};

const SOURCE: &str = include_str!("webrtc.fg");
/// The browser client, served by `webrtc.signaling` at `<path>/client.js`.
pub const CLIENT_JS: &str = include_str!("webrtc_client.js");
const DEFAULT_STUN: &str = "stun:stun.l.google.com:19302";

/// Functions run by the interpreter (`intercept`): the peer factory starts the
/// task that runs `on` handlers; the others are the Fogu helpers of `webrtc.fg`.
const PEER: &str = "webrtc.peer";
const HELPERS: [&str; 5] = ["connect", "signaling", "signalingServer", "serveHttp", "connectHttp"];

fn native(name: &str) -> Value {
    Value::NativeFn(Rc::new(NativeFnValue {
        name: name.into(),
        arity: 0,
        variadic: true,
        func: |_| Err("this function runs through the interpreter".into()),
        intercept: Default::default(),
        typed: None,
    }))
}

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("peer".into(), native(PEER));
    for h in HELPERS {
        e.insert(h.into(), native(&format!("webrtc.{}", h)));
    }
    e.insert("clientJs".into(), Value::str(CLIENT_JS));
    ModuleValue { name: "fogu:webrtc".into(), exports: RefCell::new(e) }
}

/// The names [`intercept`] handles (`Host::may_intercept_native`).
pub fn is_intercepted(name: &str) -> bool {
    name == PEER || name.strip_prefix("webrtc.").is_some_and(|h| HELPERS.contains(&h))
}

thread_local! {
    /// The map of helper functions, once `webrtc.fg` has run.
    static HELPER_FNS: RefCell<Option<Value>> = const { RefCell::new(None) };
}

pub fn intercept(callee: &Value, args: &[Value], it: &Interpreter, env: &Rc<RefCell<Environment>>) -> Option<Result<Value, String>> {
    let Value::NativeFn(f) = callee else { return None };
    if f.name == PEER {
        return Some(new_peer(args, it, env));
    }
    let helper = f.name.strip_prefix("webrtc.").filter(|h| HELPERS.contains(h))?;
    Some(call_helper(helper, args, it))
}

fn call_helper(name: &str, args: &[Value], it: &Interpreter) -> Result<Value, String> {
    let fns = match HELPER_FNS.with(|f| f.borrow().clone()) {
        Some(f) => f,
        None => {
            let file = "fogu:webrtc/webrtc.fg";
            let tokens = Lexer::new(SOURCE).tokenize().map_err(|e| format!("{}: {}", file, e))?;
            let program = Parser::new(tokens).parse_program().map_err(|e| format!("{}: {}", file, e))?;
            let env = it.vm_env(false, file);
            env.borrow_mut().define("newPeer", native(PEER), false);
            env.borrow_mut().define("CLIENT_JS", Value::str(CLIENT_JS), false);
            let f = it.vm_run(&program, env, file, None)?;
            HELPER_FNS.with(|c| *c.borrow_mut() = Some(f.clone()));
            f
        }
    };
    let func = match &fns {
        Value::Map(m) => m.borrow().get(name).cloned(),
        _ => None,
    }
    .ok_or_else(|| format!("webrtc: missing helper '{}'", name))?;
    it.call_value(func, args.to_vec(), it.globals_ref().clone())
}

// ---------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct ChanState {
    id: Option<ChannelId>,
    label: String,
    config: ChannelConfig,
    open: bool,
    closed: bool,
    /// opened by the other side
    remote: bool,
    /// handed to the program (by `accept`, a `channel` handler or `channel(label)`)
    claimed: bool,
    inbox: VecDeque<(bool, Vec<u8>)>,
    /// written before the channel opened (or while SCTP's buffer was full)
    outbox: VecDeque<(bool, Vec<u8>)>,
}

/// What the pump hands to `on` handlers, in the order it happened.
enum Note {
    State(&'static str),
    Open(usize),
    Data(usize),
    Close(usize),
    Candidate(String),
}

struct Inner {
    rtc: Rtc,
    sockets: Vec<(SocketAddr, Arc<UdpSocket>)>,
    pending: Option<SdpPendingOffer>,
    /// the session has its data (application) section: channels open directly
    negotiated: bool,
    state: &'static str,
    /// closed by either side: waits end
    closed: bool,
    /// the driver may stop (closed, and str0m sent its goodbyes)
    stop_at: Option<Instant>,
    chans: Vec<ChanState>,
    incoming: VecDeque<usize>,
    notes: VecDeque<Note>,
    local: Vec<String>,
    error: Option<String>,
    deadline: Instant,
    /// since when ICE has been disconnected (the peer closes after `lost`)
    lost_at: Option<Instant>,
    lost: Duration,
}

impl Inner {
    fn key_of(&self, id: ChannelId) -> Option<usize> {
        self.chans.iter().position(|c| c.id == Some(id))
    }

    fn set_state(&mut self, s: &'static str) {
        if self.state != s && self.state != "closed" {
            self.state = s;
            self.lost_at = (s == "disconnected").then(|| Instant::now() + self.lost);
            self.notes.push_back(Note::State(s));
        }
    }

    fn set_closed(&mut self) {
        if !self.closed {
            self.closed = true;
            self.stop_at.get_or_insert(Instant::now() + Duration::from_secs(2));
            for k in 0..self.chans.len() {
                let c = &mut self.chans[k];
                if !c.closed {
                    c.closed = true;
                    c.open = false;
                    self.notes.push_back(Note::Close(k));
                }
            }
            self.set_state("closed");
        }
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Connected => self.set_state("connected"),
            Event::IceConnectionStateChange(s) => match s {
                IceConnectionState::Checking if self.state == "new" => self.set_state("connecting"),
                IceConnectionState::Disconnected if self.state == "connected" => self.set_state("disconnected"),
                IceConnectionState::Connected | IceConnectionState::Completed if self.state == "disconnected" => self.set_state("connected"),
                _ => {}
            },
            Event::ChannelOpen(id, label) => {
                let k = match self.key_of(id) {
                    Some(k) => k,
                    None => {
                        let config = ChannelConfig { label: label.clone(), ..Default::default() };
                        self.chans.push(ChanState {
                            id: Some(id),
                            label,
                            config,
                            open: false,
                            closed: false,
                            remote: true,
                            claimed: false,
                            inbox: VecDeque::new(),
                            outbox: VecDeque::new(),
                        });
                        let k = self.chans.len() - 1;
                        self.incoming.push_back(k);
                        k
                    }
                };
                self.chans[k].open = true;
                self.notes.push_back(Note::Open(k));
            }
            Event::ChannelData(d) => {
                if let Some(k) = self.key_of(d.id) {
                    self.chans[k].inbox.push_back((d.binary, d.data));
                    self.notes.push_back(Note::Data(k));
                }
            }
            Event::ChannelClose(id) => {
                if let Some(k) = self.key_of(id) {
                    let c = &mut self.chans[k];
                    if !c.closed {
                        c.closed = true;
                        c.open = false;
                        self.notes.push_back(Note::Close(k));
                    }
                }
            }
            _ => {}
        }
    }

    /// Drains `poll_output` until the next timeout: packets go out, events
    /// update the state. Returns whether anything happened for the program.
    fn drain(&mut self) -> bool {
        let mut news = false;
        loop {
            match self.rtc.poll_output() {
                Ok(Output::Timeout(t)) => {
                    self.deadline = t;
                    break;
                }
                Ok(Output::Transmit(t)) => {
                    if let Some((_, s)) = self.sockets.iter().find(|(a, _)| *a == t.source) {
                        let _ = s.try_send_to(&t.contents, t.destination);
                    }
                }
                Ok(Output::Event(e)) => {
                    self.event(e);
                    news = true;
                }
                Err(e) => {
                    self.error.get_or_insert(e.to_string());
                    self.set_closed();
                    self.stop_at = Some(Instant::now());
                    return true;
                }
            }
        }
        if !self.rtc.is_alive() {
            if !self.closed {
                news = true;
            }
            self.set_closed();
            self.stop_at = Some(Instant::now());
        }
        news
    }

    /// Writes what waits in the outboxes of open channels. Returns whether
    /// anything was written (a mutation: drain again).
    fn flush(&mut self) -> bool {
        let mut wrote = false;
        for c in self.chans.iter_mut() {
            let (Some(id), true) = (c.id, c.open) else { continue };
            if c.outbox.is_empty() {
                continue;
            }
            let Some(mut ch) = self.rtc.channel(id) else { continue };
            while let Some((binary, data)) = c.outbox.front() {
                match ch.write(*binary, data) {
                    Ok(true) => {
                        c.outbox.pop_front();
                        wrote = true;
                    }
                    _ => break,
                }
            }
        }
        wrote
    }

    /// After any mutation: drain, write queued data, drain again.
    fn step(&mut self) -> bool {
        let mut news = self.drain();
        while !self.closed && self.flush() {
            news |= self.drain();
        }
        news
    }

    /// Gives an id to the channels created before the session had its data section.
    fn bind_channels(&mut self) {
        if !self.negotiated {
            return;
        }
        let unbound: Vec<usize> = (0..self.chans.len()).filter(|&k| self.chans[k].id.is_none() && !self.chans[k].closed).collect();
        if unbound.is_empty() {
            return;
        }
        let mut api = self.rtc.sdp_api();
        let mut ids = Vec::new();
        for &k in &unbound {
            ids.push((k, api.add_channel_with_config(self.chans[k].config.clone())));
        }
        let _ = api.apply();
        for (k, id) in ids {
            self.chans[k].id = Some(id);
        }
    }
}

#[derive(Clone)]
struct Core {
    inner: Arc<Mutex<Inner>>,
    notify: Arc<Notify>,
    handlers: Rc<RefCell<Handlers>>,
}

#[derive(Default)]
struct Handlers {
    peer: HashMap<String, Vec<Value>>,
    chans: HashMap<usize, HashMap<String, Vec<Value>>>,
}

impl Handlers {
    fn any(&self) -> bool {
        self.peer.values().any(|v| !v.is_empty()) || self.chans.values().any(|m| m.values().any(|v| !v.is_empty()))
    }
}

impl Core {
    /// Runs a mutation, then drains (str0m's contract) and tells the driver
    /// its deadline may have changed.
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let r = {
            let mut g = lock(&self.inner);
            let r = f(&mut g);
            g.step();
            r
        };
        self.notify.notify_one();
        r
    }

    fn channel_value(&self, key: usize) -> Value {
        NativeObjectRef::new(ChannelValue { core: self.clone(), key }).to_value()
    }
}

// ---------------------------------------------------------------------
// Creating a peer: sockets, STUN, the driver task
// ---------------------------------------------------------------------

type Packet = (Vec<u8>, SocketAddr, SocketAddr);

struct PeerOpts {
    stun: Vec<String>,
    hosts: Option<Vec<IpAddr>>,
    port: u16,
    /// how long ICE may stay disconnected before the peer closes
    lost: Duration,
}

fn peer_opts(v: Option<&Value>) -> Result<PeerOpts, String> {
    let mut o = PeerOpts { stun: vec![DEFAULT_STUN.into()], hosts: None, port: 0, lost: Duration::from_secs(15) };
    let m = match v {
        None | Some(Value::Nil) => return Ok(o),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(other) => return Err(format!("webrtc.peer: options must be a map, got '{}'", other.type_name())),
    };
    if let Some(servers) = m.get("iceServers") {
        o.stun.clear();
        let Value::List(l) = servers else {
            return Err("webrtc.peer: iceServers must be a list of { urls, username?, credential? }".into());
        };
        for s in l.borrow().iter() {
            let urls = match s {
                Value::Str(u) => vec![u.to_string()],
                Value::Map(m) => match m.borrow().get("urls") {
                    Some(Value::Str(u)) => vec![u.to_string()],
                    Some(Value::List(us)) => us.borrow().iter().map(|u| u.to_display_string()).collect(),
                    _ => return Err("webrtc.peer: every ice server needs urls".into()),
                },
                _ => return Err("webrtc.peer: iceServers must be a list of { urls, username?, credential? }".into()),
            };
            // TURN relays are not supported (str0m has no TURN client): skipped
            o.stun.extend(urls.into_iter().filter(|u| u.starts_with("stun:")));
        }
    }
    if let Some(Value::List(h)) = m.get("hosts") {
        let mut ips = Vec::new();
        for v in h.borrow().iter() {
            let s = v.to_display_string();
            ips.push(s.parse::<IpAddr>().map_err(|_| format!("webrtc.peer: '{}' is not an IP address", s))?);
        }
        o.hosts = Some(ips);
    }
    match m.get("port") {
        None | Some(Value::Nil) => {}
        Some(Value::Int(p)) if (0..=65535).contains(p) => o.port = *p as u16,
        Some(other) => return Err(format!("webrtc.peer: port must be 0..65535, got {}", other.to_display_string())),
    }
    if let Some(v) = m.get("disconnectTimeout").filter(|v| !matches!(v, Value::Nil)) {
        o.lost = duration_arg(v, "webrtc.peer: disconnectTimeout")?;
    }
    Ok(o)
}

/// The address of the interface with the default route (no packet is sent),
/// and loopback, so two peers on one machine connect even offline.
fn default_hosts() -> Vec<IpAddr> {
    let mut ips = Vec::new();
    if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if s.connect("192.0.2.1:9").is_ok() {
            if let Ok(a) = s.local_addr() {
                if !a.ip().is_loopback() && !a.ip().is_unspecified() {
                    ips.push(a.ip());
                }
            }
        }
    }
    ips.push(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ips
}

/// One STUN binding request (RFC 5389): the address the server saw.
async fn stun_query(sock: &UdpSocket, server: &str, wait: Duration) -> Option<SocketAddr> {
    let hostport = server.trim_start_matches("stun:");
    let hostport = if hostport.contains(':') { hostport.to_string() } else { format!("{}:3478", hostport) };
    let want_v4 = sock.local_addr().ok()?.is_ipv4();
    let addr = tokio::time::timeout(wait, tokio::net::lookup_host(hostport)).await.ok()?.ok()?.find(|a| a.is_ipv4() == want_v4)?;
    let mut req = vec![0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42];
    let tid: [u8; 12] = std::array::from_fn(|i| fastrand_u8() ^ i as u8);
    req.extend_from_slice(&tid);
    let mut buf = [0u8; 1500];
    let deadline = tokio::time::Instant::now() + wait;
    for _ in 0..3 {
        sock.send_to(&req, addr).await.ok()?;
        let until = (tokio::time::Instant::now() + Duration::from_millis(500)).min(deadline);
        while let Ok(Ok((n, _))) = tokio::time::timeout_at(until, sock.recv_from(&mut buf)).await {
            if let Some(a) = parse_binding_response(&buf[..n], &tid) {
                return Some(a);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
    }
    None
}

fn fastrand_u8() -> u8 {
    let mut b = [0u8; 1];
    let _ = getrandom::getrandom(&mut b);
    b[0]
}

fn parse_binding_response(p: &[u8], tid: &[u8; 12]) -> Option<SocketAddr> {
    if p.len() < 20 || p[0] != 0x01 || p[1] != 0x01 || p[8..20] != tid[..] {
        return None;
    }
    let len = u16::from_be_bytes([p[2], p[3]]) as usize;
    let body = p.get(20..20 + len)?;
    let mut i = 0;
    let mut mapped = None;
    while i + 4 <= body.len() {
        let ty = u16::from_be_bytes([body[i], body[i + 1]]);
        let l = u16::from_be_bytes([body[i + 2], body[i + 3]]) as usize;
        let v = body.get(i + 4..i + 4 + l)?;
        if (ty == 0x0020 || ty == 0x0001) && l >= 8 && v[1] == 0x01 {
            let xor = ty == 0x0020;
            let mut port = u16::from_be_bytes([v[2], v[3]]);
            let mut ip = [v[4], v[5], v[6], v[7]];
            if xor {
                port ^= 0x2112;
                for (b, m) in ip.iter_mut().zip([0x21, 0x12, 0xA4, 0x42]) {
                    *b ^= m;
                }
            }
            let a = SocketAddr::from((ip, port));
            if xor {
                return Some(a);
            }
            mapped = Some(a);
        }
        i += 4 + l.div_ceil(4) * 4;
    }
    mapped
}

/// Binds the sockets, asks STUN for the public address, builds the `Rtc`
/// with every local candidate and starts the driver.
fn start(o: PeerOpts) -> Result<(Arc<Mutex<Inner>>, Arc<Notify>), String> {
    let hosts = o.hosts.clone().unwrap_or_else(default_hosts);
    let lost = o.lost;
    let (rtc, sockets, local) = rt::block_on(async move {
        let mut rtc = Rtc::builder().build(Instant::now());
        let mut sockets = Vec::new();
        let mut local = Vec::new();
        for ip in hosts {
            let sock = match UdpSocket::bind(SocketAddr::new(ip, o.port)).await {
                Ok(s) => s,
                Err(e) => return Err(io_err!("webrtc: cannot bind {}: {}", ip, e)),
            };
            let addr = sock.local_addr().map_err(|e| io_err!("webrtc: {}", e))?;
            if let Ok(c) = Candidate::host(addr, "udp") {
                if let Some(c) = rtc.add_local_candidate(c) {
                    local.push(c.to_sdp_string());
                }
            }
            if !ip.is_loopback() {
                for server in &o.stun {
                    if let Some(public) = stun_query(&sock, server, Duration::from_millis(1500)).await {
                        if public != addr {
                            if let Ok(c) = Candidate::server_reflexive(public, addr, "udp") {
                                if let Some(c) = rtc.add_local_candidate(c) {
                                    local.push(c.to_sdp_string());
                                }
                            }
                        }
                        break;
                    }
                }
            }
            sockets.push((addr, Arc::new(sock)));
        }
        if sockets.is_empty() {
            return Err(io_err!("webrtc: no local address to use"));
        }
        Ok((rtc, sockets, local))
    })??;
    let inner = Arc::new(Mutex::new(Inner {
        rtc,
        sockets: sockets.clone(),
        pending: None,
        negotiated: false,
        state: "new",
        closed: false,
        stop_at: None,
        chans: Vec::new(),
        incoming: VecDeque::new(),
        notes: VecDeque::new(),
        local,
        error: None,
        deadline: Instant::now() + Duration::from_secs(3600),
        lost_at: None,
        lost,
    }));
    let notify = Arc::new(Notify::new());
    let (tx, rx) = unbounded_channel();
    let readers: Vec<_> = sockets.into_iter().map(|(addr, s)| rt::tokio().spawn(read_socket(s, addr, tx.clone()))).collect();
    rt::tokio().spawn(drive(Arc::clone(&inner), rx, Arc::clone(&notify), readers));
    Ok((inner, notify))
}

async fn read_socket(sock: Arc<UdpSocket>, local: SocketAddr, tx: UnboundedSender<Packet>) {
    let mut buf = vec![0u8; 2048];
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((n, from)) => {
                if tx.send((buf[..n].to_vec(), from, local)).is_err() {
                    return;
                }
            }
            // e.g. an ICMP "port unreachable" reported on the socket (Windows)
            Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
        }
    }
}

/// The peer's task: feeds packets and timeouts to the `Rtc` until it is closed.
async fn drive(inner: Arc<Mutex<Inner>>, mut rx: UnboundedReceiver<Packet>, notify: Arc<Notify>, readers: Vec<tokio::task::JoinHandle<()>>) {
    loop {
        let deadline = {
            let mut g = lock(&inner);
            if g.lost_at.is_some_and(|t| Instant::now() >= t) {
                g.error.get_or_insert_with(|| "the connection was lost".into());
                g.rtc.disconnect();
                g.set_closed();
                g.stop_at = Some(Instant::now());
                drop(g);
                sched::wake();
                continue;
            }
            let deadline = match g.lost_at {
                Some(t) => g.deadline.min(t),
                None => g.deadline,
            };
            match g.stop_at {
                Some(t) if Instant::now() >= t || !g.rtc.is_alive() => break,
                Some(t) => deadline.min(t),
                None => deadline,
            }
        };
        let at = tokio::time::Instant::from_std(deadline.max(Instant::now()));
        let news = tokio::select! {
            p = rx.recv() => {
                let Some((data, source, destination)) = p else { break };
                let mut g = lock(&inner);
                let mut news = feed(&mut g, &data, source, destination);
                // everything already queued, each followed by its drain
                while let Ok((data, source, destination)) = rx.try_recv() {
                    news |= feed(&mut g, &data, source, destination);
                }
                news
            }
            _ = tokio::time::sleep_until(at) => {
                let mut g = lock(&inner);
                if g.rtc.is_alive() {
                    let _ = g.rtc.handle_input(Input::Timeout(Instant::now()));
                }
                g.step()
            }
            _ = notify.notified() => false,
        };
        if news {
            sched::wake();
        }
    }
    for r in readers {
        r.abort();
    }
    let mut g = lock(&inner);
    g.set_closed();
    drop(g);
    sched::wake();
}

fn feed(g: &mut Inner, data: &[u8], source: SocketAddr, destination: SocketAddr) -> bool {
    if !g.rtc.is_alive() {
        return false;
    }
    let Ok(contents) = data.try_into() else { return false };
    let input = Input::Receive(Instant::now(), Receive { proto: Protocol::Udp, source, destination, contents });
    if !g.rtc.accepts(&input) {
        return false;
    }
    let _ = g.rtc.handle_input(input);
    g.step()
}

// ---------------------------------------------------------------------
// The peer (and its handler pump)
// ---------------------------------------------------------------------

fn new_peer(args: &[Value], it: &Interpreter, env: &Rc<RefCell<Environment>>) -> Result<Value, String> {
    max_args(args, 1, "webrtc.peer")?;
    let o = peer_opts(args.first())?;
    let (inner, notify) = start(o)?;
    let core = Core { inner, notify, handlers: Rc::new(RefCell::new(Handlers::default())) };
    start_pump(it, env, core.clone())?;
    Ok(NativeObjectRef::new(PeerValue { core }).to_value())
}

/// Calls a handler in its own task (one that waits does not hold the others).
fn run_handler(it: &Interpreter, env: &Rc<RefCell<Environment>>, f: Value, args: Vec<Value>) {
    let env = Rc::clone(env);
    let _ = it.spawn_task(Box::new(move |it| it.call_value(f, args, env).map(|_| ())));
}

fn message_value(binary: bool, data: Vec<u8>) -> Value {
    if binary {
        Value::buffer(data)
    } else {
        Value::str(String::from_utf8_lossy(&data).into_owned())
    }
}

/// The task that runs the `on` handlers of a peer and its channels, in the
/// order things happened; ends when the peer is closed.
fn start_pump(it: &Interpreter, env: &Rc<RefCell<Environment>>, core: Core) -> Result<(), String> {
    let env = Rc::clone(env);
    it.spawn_task(Box::new(move |it| {
        // yield once: the program registers its handlers first
        let mut first = true;
        sched::wait_until(|| !std::mem::take(&mut first))?;
        let mut guard: Option<IoGuard> = None;
        loop {
            let mut jobs: Vec<(Value, Vec<Value>)> = Vec::new();
            let mut done = false;
            sched::wait_until(|| {
                done = collect(&core, &mut jobs);
                // while handlers wait for this peer, waiting is not a deadlock
                let want = !done && core.handlers.borrow().any();
                if want && guard.is_none() {
                    guard = Some(IoGuard::new());
                } else if !want {
                    guard = None;
                }
                !jobs.is_empty() || done
            })?;
            for (f, args) in jobs {
                run_handler(it, &env, f, args);
            }
            if done {
                return Ok(());
            }
        }
    }))
}

/// Takes what the handlers must see; returns whether the peer is finished.
fn collect(core: &Core, jobs: &mut Vec<(Value, Vec<Value>)>) -> bool {
    let mut g = lock(&core.inner);
    let h = core.handlers.borrow();
    let peer_fns = |ev: &str| h.peer.get(ev).cloned().unwrap_or_default();
    let chan_fns = |k: usize, ev: &str| h.chans.get(&k).and_then(|m| m.get(ev)).cloned().unwrap_or_default();
    while let Some(n) = g.notes.pop_front() {
        match n {
            Note::State(s) => {
                for f in peer_fns("state") {
                    jobs.push((f, vec![Value::str(s)]));
                }
                let ev = match s {
                    "connected" => "connected",
                    "closed" => "close",
                    _ => continue,
                };
                for f in peer_fns(ev) {
                    jobs.push((f, Vec::new()));
                }
            }
            Note::Open(k) => {
                for f in chan_fns(k, "open") {
                    jobs.push((f, vec![core.channel_value(k)]));
                }
            }
            Note::Data(k) => {
                let fns = chan_fns(k, "message");
                if !fns.is_empty() {
                    if let Some((b, d)) = g.chans[k].inbox.pop_front() {
                        let v = message_value(b, d);
                        for f in fns {
                            jobs.push((f, vec![v.clone()]));
                        }
                    }
                }
            }
            Note::Close(k) => {
                for f in chan_fns(k, "close") {
                    jobs.push((f, vec![core.channel_value(k)]));
                }
            }
            Note::Candidate(c) => {
                for f in peer_fns("candidate") {
                    jobs.push((f, vec![Value::str(c.as_str())]));
                }
            }
        }
    }
    // channels the other side opened
    let on_channel = peer_fns("channel");
    if !on_channel.is_empty() {
        while let Some(k) = g.incoming.pop_front() {
            g.chans[k].claimed = true;
            let ch = core.channel_value(k);
            for f in &on_channel {
                jobs.push((f.clone(), vec![ch.clone()]));
            }
        }
    }
    // messages that arrived before a message handler was registered
    for (k, evs) in h.chans.iter() {
        let Some(fns) = evs.get("message").filter(|f| !f.is_empty()) else { continue };
        while let Some((b, d)) = g.chans[*k].inbox.pop_front() {
            let v = message_value(b, d);
            for f in fns {
                jobs.push((f.clone(), vec![v.clone()]));
            }
        }
    }
    g.closed && g.notes.is_empty()
}

fn deadline_arg(args: &[Value], i: usize, what: &str) -> Result<Option<Instant>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(None),
        Some(v) => Ok(Some(Instant::now() + duration_arg(v, what)?)),
    }
}

/// A cooperative wait: woken by the runtime (it holds an `IoGuard`), with a
/// timer only for its real deadline.
fn waiter(
    deadline: Option<Instant>,
    mut ready: impl FnMut() -> Option<Result<Value, String>> + 'static,
    on_timeout: impl Fn() -> Result<Value, String> + 'static,
) -> Poller {
    let mut guard = Some(IoGuard::new());
    Box::new(move || {
        if let Some(r) = ready() {
            guard.take();
            return Some(r);
        }
        match deadline {
            Some(d) if Instant::now() >= d => {
                guard.take();
                Some(on_timeout())
            }
            Some(d) => {
                sched::add_timer(d);
                None
            }
            None => None,
        }
    })
}

/// Drops the candidates a browser hides behind mDNS names (`xxxx.local`):
/// str0m only takes IP addresses (the browser is still reached through
/// peer-reflexive candidates).
fn clean_sdp(sdp: &str) -> String {
    let mut out = String::with_capacity(sdp.len());
    for line in sdp.split_inclusive('\n') {
        let t = line.trim_end();
        if let Some(c) = t.strip_prefix("a=candidate:") {
            let addr = c.split_whitespace().nth(4).unwrap_or("");
            if addr.parse::<IpAddr>().is_err() {
                continue;
            }
        }
        out.push_str(t);
        out.push_str("\r\n");
    }
    out
}

fn channel_config(label: String, o: Option<&Value>) -> Result<ChannelConfig, String> {
    let mut c = ChannelConfig { label, ..Default::default() };
    let m = match o {
        None | Some(Value::Nil) => return Ok(c),
        Some(Value::Map(m)) => m.borrow().clone(),
        Some(other) => return Err(format!("channel: options must be a map, got '{}'", other.type_name())),
    };
    if let Some(Value::Bool(b)) = m.get("ordered") {
        c.ordered = *b;
    }
    match (m.get("maxRetransmits"), m.get("maxPacketLifeTime")) {
        (Some(Value::Int(n)), _) if *n >= 0 => c.reliability = Reliability::MaxRetransmits { retransmits: (*n).min(u16::MAX as i64) as u16 },
        (_, Some(Value::Int(ms))) if *ms >= 0 => c.reliability = Reliability::MaxPacketLifetime { lifetime: (*ms).min(u16::MAX as i64) as u16 },
        (_, Some(v @ Value::Duration(_))) => {
            let d = duration_arg(v, "channel: maxPacketLifeTime")?;
            c.reliability = Reliability::MaxPacketLifetime { lifetime: d.as_millis().min(u16::MAX as u128) as u16 };
        }
        (None | Some(Value::Nil), None | Some(Value::Nil)) => {}
        _ => return Err("channel: maxRetransmits / maxPacketLifeTime must be ints >= 0".into()),
    }
    if let Some(Value::Str(p)) = m.get("protocol") {
        c.protocol = p.to_string();
    }
    if let Some(Value::Int(id)) = m.get("id") {
        if (0..=65534).contains(id) {
            c.negotiated = Some(*id as u16);
        }
    }
    Ok(c)
}

fn sdp_text(sdp: &str) -> Value {
    Value::str(sdp)
}

pub struct PeerValue {
    core: Core,
}

impl FoguObject for PeerValue {
    fn name(&self) -> &str {
        "Peer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let g = lock(&self.core.inner);
        Some(match field {
            "state" => Value::str(g.state),
            "connected" => Value::Bool(g.state == "connected"),
            "closed" => Value::Bool(g.closed),
            "candidates" => list(g.local.iter().map(|c| Value::str(c.as_str())).collect()),
            "addresses" => list(g.sockets.iter().map(|(a, _)| Value::str(a.to_string())).collect()),
            "error" => g.error.as_deref().map_or(Value::Nil, Value::str),
            "channels" => {
                let keys: Vec<usize> = (0..g.chans.len()).filter(|&k| !g.chans[k].closed && (!g.chans[k].remote || g.chans[k].claimed)).collect();
                drop(g);
                list(keys.into_iter().map(|k| self.core.channel_value(k)).collect())
            }
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let check_open = |g: &Inner| -> Result<(), String> {
            if g.closed {
                return Err(io_err!("webrtc: the peer is closed{}", g.error.as_ref().map(|e| format!(" ({})", e)).unwrap_or_default()));
            }
            Ok(())
        };
        match method {
            // channel(label, { ordered, maxRetransmits, maxPacketLifeTime, protocol, id }) — get or create
            "channel" => {
                max_args(args, 2, "channel")?;
                let label = str_arg(args, 0, "channel")?;
                let config = channel_config(label.clone(), args.get(1))?;
                let key = self.core.with(|g| {
                    check_open(g)?;
                    if let Some(k) = g.chans.iter().position(|c| c.label == label && !c.closed) {
                        g.chans[k].claimed = true;
                        g.incoming.retain(|&i| i != k);
                        return Ok(k);
                    }
                    g.chans.push(ChanState {
                        id: None,
                        label,
                        config,
                        open: false,
                        closed: false,
                        remote: false,
                        claimed: true,
                        inbox: VecDeque::new(),
                        outbox: VecDeque::new(),
                    });
                    g.bind_channels();
                    Ok::<usize, String>(g.chans.len() - 1)
                })?;
                Ok(self.core.channel_value(key))
            }
            // createOffer() → SDP (every local candidate included: no trickle needed)
            "createOffer" => {
                max_args(args, 0, "createOffer")?;
                self.core.with(|g| {
                    check_open(g)?;
                    if g.negotiated {
                        return Err(io_err!("webrtc: the session is already negotiated"));
                    }
                    // an offer needs a data section: a default channel when none was asked for
                    if g.chans.is_empty() {
                        g.chans.push(ChanState {
                            id: None,
                            label: "data".into(),
                            config: ChannelConfig { label: "data".into(), ..Default::default() },
                            open: false,
                            closed: false,
                            remote: false,
                            claimed: false,
                            inbox: VecDeque::new(),
                            outbox: VecDeque::new(),
                        });
                    }
                    let mut api = g.rtc.sdp_api();
                    let mut ids = Vec::new();
                    for (k, c) in g.chans.iter().enumerate() {
                        if c.id.is_none() {
                            ids.push((k, api.add_channel_with_config(c.config.clone())));
                        }
                    }
                    let (offer, pending) = api.apply().ok_or_else(|| io_err!("webrtc: nothing to offer"))?;
                    for (k, id) in ids {
                        g.chans[k].id = Some(id);
                    }
                    g.pending = Some(pending);
                    g.set_state("connecting");
                    Ok(sdp_text(&offer.to_sdp_string()))
                })
            }
            // acceptOffer(sdp) → answer SDP
            "acceptOffer" => {
                max_args(args, 1, "acceptOffer")?;
                let sdp = clean_sdp(&sdp_arg(args, "acceptOffer")?);
                let offer = SdpOffer::from_sdp_string(&sdp).map_err(|e| io_err!("webrtc: invalid offer: {}", e))?;
                self.core.with(|g| {
                    check_open(g)?;
                    let answer = g.rtc.sdp_api().accept_offer(offer).map_err(|e| io_err!("webrtc: cannot accept the offer: {}", e))?;
                    g.negotiated = true;
                    g.set_state("connecting");
                    g.bind_channels();
                    Ok(sdp_text(&answer.to_sdp_string()))
                })
            }
            "acceptAnswer" => {
                max_args(args, 1, "acceptAnswer")?;
                let sdp = clean_sdp(&sdp_arg(args, "acceptAnswer")?);
                let answer = SdpAnswer::from_sdp_string(&sdp).map_err(|e| io_err!("webrtc: invalid answer: {}", e))?;
                self.core.with(|g| {
                    check_open(g)?;
                    let pending = g.pending.take().ok_or_else(|| io_err!("webrtc: acceptAnswer without a pending offer (call createOffer first)"))?;
                    g.rtc.sdp_api().accept_answer(pending, answer).map_err(|e| io_err!("webrtc: cannot accept the answer: {}", e))?;
                    g.negotiated = true;
                    g.bind_channels();
                    Ok(Value::Nil)
                })
            }
            // addCandidate("candidate:…" | { candidate }) → true, false when skipped (mDNS name, end of candidates)
            "addCandidate" => {
                max_args(args, 1, "addCandidate")?;
                let text = match args.first() {
                    Some(Value::Str(s)) => s.to_string(),
                    Some(Value::Map(m)) => match m.borrow().get("candidate") {
                        Some(Value::Str(s)) => s.to_string(),
                        _ => String::new(),
                    },
                    None | Some(Value::Nil) => String::new(),
                    Some(other) => return Err(format!("addCandidate: expected a candidate str or map, got '{}'", other.type_name())),
                };
                let text = text.trim().trim_start_matches("a=").to_string();
                if text.is_empty() || text.split_whitespace().nth(4).is_none_or(|a| a.parse::<IpAddr>().is_err()) {
                    return Ok(Value::Bool(false));
                }
                let c = Candidate::from_sdp_string(&text).map_err(|e| io_err!("webrtc: invalid candidate: {}", e))?;
                self.core.with(|g| {
                    check_open(g)?;
                    g.rtc.add_remote_candidate(c);
                    Ok(Value::Bool(true))
                })
            }
            // on(event, fn): state(s) · connected() · channel(ch) · candidate(c) · close()
            "on" => {
                max_args(args, 2, "on")?;
                let ev = str_arg(args, 0, "on")?;
                if !matches!(ev.as_str(), "state" | "connected" | "channel" | "candidate" | "close") {
                    return Err(format!("on: unknown Peer event '{}' (state connected channel candidate close)", ev));
                }
                let f = args.get(1).cloned().ok_or("on(event, fn) expects a function")?;
                self.core.handlers.borrow_mut().peer.entry(ev.clone()).or_default().push(f);
                let mut g = lock(&self.core.inner);
                // the candidates are known from the start: replay them for the new handler
                if ev == "candidate" {
                    let cs: Vec<String> = g.local.clone();
                    g.notes.extend(cs.into_iter().map(Note::Candidate));
                }
                if ev == "connected" && g.state == "connected" {
                    g.notes.push_back(Note::State("connected"));
                }
                drop(g);
                sched::wake();
                Ok(Value::Nil)
            }
            "close" => {
                max_args(args, 0, "close")?;
                self.core.with(|g| {
                    if !g.closed {
                        let _ = g.rtc.close();
                        g.set_closed();
                    }
                });
                sched::wake();
                Ok(Value::Nil)
            }
            other => Err(format!(
                "method '{}' does not exist on Peer (channel createOffer acceptOffer acceptAnswer addCandidate accept wait on close)",
                other
            )),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        match method {
            // accept(timeout?) → a channel the other side opened, nil on timeout / close
            "accept" => {
                let deadline = match max_args(args, 1, "accept").and_then(|_| deadline_arg(args, 0, "accept")) {
                    Ok(d) => d,
                    Err(e) => return Some(Err(e)),
                };
                let core = self.core.clone();
                Some(Ok(waiter(
                    deadline,
                    move || {
                        let mut g = lock(&core.inner);
                        while let Some(k) = g.incoming.pop_front() {
                            if g.chans[k].claimed {
                                continue;
                            }
                            g.chans[k].claimed = true;
                            drop(g);
                            return Some(Ok(core.channel_value(k)));
                        }
                        g.closed.then_some(Ok(Value::Nil))
                    },
                    || Ok(Value::Nil),
                )))
            }
            // wait(state = "connected", timeout?) → true · error on timeout or when closed first
            "wait" => {
                if let Err(e) = max_args(args, 2, "wait") {
                    return Some(Err(e));
                }
                let want = match args.first() {
                    None | Some(Value::Nil) => "connected".to_string(),
                    Some(Value::Str(s)) => s.to_string(),
                    Some(other) => return Some(Err(format!("wait: state must be str, got '{}'", other.type_name()))),
                };
                let deadline = match deadline_arg(args, 1, "wait") {
                    Ok(d) => d,
                    Err(e) => return Some(Err(e)),
                };
                let core = self.core.clone();
                let core2 = self.core.clone();
                let want2 = want.clone();
                Some(Ok(waiter(
                    deadline,
                    move || {
                        let g = lock(&core.inner);
                        if g.state == want {
                            return Some(Ok(Value::Bool(true)));
                        }
                        if g.closed {
                            return Some(Err(io_err!(
                                "webrtc: the peer closed before reaching '{}'{}",
                                want,
                                g.error.as_ref().map(|e| format!(" ({})", e)).unwrap_or_default()
                            )));
                        }
                        None
                    },
                    move || {
                        let s = lock(&core2.inner).state;
                        Err(io_err!("webrtc: timed out waiting for '{}' (state: {})", want2, s))
                    },
                )))
            }
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["channel", "createOffer", "acceptOffer", "acceptAnswer", "addCandidate", "accept", "wait", "on", "close"]
    }
}

fn sdp_arg(args: &[Value], what: &str) -> Result<String, String> {
    match args.first() {
        Some(Value::Str(s)) => Ok(s.to_string()),
        // { type, sdp } as a browser's RTCSessionDescription
        Some(Value::Map(m)) => match m.borrow().get("sdp") {
            Some(Value::Str(s)) => Ok(s.to_string()),
            _ => Err(format!("{}: expected an SDP str or {{ type, sdp }}", what)),
        },
        _ => Err(format!("{}: expected an SDP str or {{ type, sdp }}", what)),
    }
}

// ---------------------------------------------------------------------
// Data channels
// ---------------------------------------------------------------------

pub struct ChannelValue {
    core: Core,
    key: usize,
}

impl FoguObject for ChannelValue {
    fn name(&self) -> &str {
        "DataChannel"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let g = lock(&self.core.inner);
        let c = &g.chans[self.key];
        Some(match field {
            "label" => Value::str(c.label.as_str()),
            "open" => Value::Bool(c.open),
            "closed" => Value::Bool(c.closed),
            "ordered" => Value::Bool(c.config.ordered),
            "protocol" => Value::str(c.config.protocol.as_str()),
            "pending" => Value::Int(c.inbox.len() as i64),
            "buffered" => Value::Int(c.outbox.iter().map(|(_, d)| d.len() as i64).sum()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let key = self.key;
        match method {
            // send(str | buffer | [bytes]); queued until the channel opens
            "send" => {
                max_args(args, 1, "send")?;
                let (binary, data) = match args.first() {
                    Some(Value::Str(s)) => (false, s.as_bytes().to_vec()),
                    Some(v @ (Value::Buffer(_) | Value::List(_))) => (true, bytes_of(v, "send")?),
                    Some(other) => (false, crate::native::json::stringify_value(other).into_bytes()),
                    None => return Err("send(data) expects 1 argument".into()),
                };
                self.core.with(|g| {
                    if g.chans[key].closed {
                        return Err(io_err!("webrtc: the channel '{}' is closed", g.chans[key].label));
                    }
                    g.chans[key].outbox.push_back((binary, data));
                    Ok(Value::Nil)
                })
            }
            "tryRecv" => {
                max_args(args, 0, "tryRecv")?;
                let mut g = lock(&self.core.inner);
                Ok(g.chans[key].inbox.pop_front().map_or(Value::Nil, |(b, d)| message_value(b, d)))
            }
            // on(event, fn): open(ch) · message(data) · close(ch)
            "on" => {
                max_args(args, 2, "on")?;
                let ev = str_arg(args, 0, "on")?;
                if !matches!(ev.as_str(), "open" | "message" | "close") {
                    return Err(format!("on: unknown DataChannel event '{}' (open message close)", ev));
                }
                let f = args.get(1).cloned().ok_or("on(event, fn) expects a function")?;
                self.core.handlers.borrow_mut().chans.entry(key).or_default().entry(ev.clone()).or_default().push(f);
                let mut g = lock(&self.core.inner);
                // already open (a channel the other side opened): tell the new handler
                if ev == "open" && g.chans[key].open {
                    g.notes.push_back(Note::Open(key));
                }
                drop(g);
                sched::wake();
                Ok(Value::Nil)
            }
            "close" => {
                max_args(args, 0, "close")?;
                self.core.with(|g| {
                    let c = &mut g.chans[key];
                    if !c.closed {
                        c.closed = true;
                        c.open = false;
                        if let Some(id) = c.id {
                            g.rtc.direct_api().close_data_channel(id);
                        }
                        g.notes.push_back(Note::Close(key));
                    }
                });
                sched::wake();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on DataChannel (send recv tryRecv wait on close)", other)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let key = self.key;
        let core = self.core.clone();
        match method {
            // recv(timeout?) → str (text) · buffer (binary) · nil on timeout or close
            "recv" => {
                let deadline = match max_args(args, 1, "recv").and_then(|_| deadline_arg(args, 0, "recv")) {
                    Ok(d) => d,
                    Err(e) => return Some(Err(e)),
                };
                Some(Ok(waiter(
                    deadline,
                    move || {
                        let mut g = lock(&core.inner);
                        if let Some((b, d)) = g.chans[key].inbox.pop_front() {
                            return Some(Ok(message_value(b, d)));
                        }
                        (g.chans[key].closed || g.closed).then_some(Ok(Value::Nil))
                    },
                    || Ok(Value::Nil),
                )))
            }
            // wait(timeout?) → true once open · error on timeout or close
            "wait" => {
                let deadline = match max_args(args, 1, "wait").and_then(|_| deadline_arg(args, 0, "wait")) {
                    Ok(d) => d,
                    Err(e) => return Some(Err(e)),
                };
                let core2 = core.clone();
                Some(Ok(waiter(
                    deadline,
                    move || {
                        let g = lock(&core.inner);
                        let c = &g.chans[key];
                        if c.open {
                            return Some(Ok(Value::Bool(true)));
                        }
                        (c.closed || g.closed).then(|| Err(io_err!("webrtc: the channel '{}' closed before opening", c.label)))
                    },
                    move || Err(io_err!("webrtc: timed out waiting for the channel '{}' to open", lock(&core2.inner).chans[key].label)),
                )))
            }
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["send", "recv", "tryRecv", "wait", "on", "close"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_response() {
        let tid = [7u8; 12];
        // XOR-MAPPED-ADDRESS 203.0.113.5:40000
        let port = 40000u16 ^ 0x2112;
        let ip = [203 ^ 0x21, 0 ^ 0x12, 113 ^ 0xA4, 5 ^ 0x42];
        let mut p = vec![0x01, 0x01, 0x00, 12, 0x21, 0x12, 0xA4, 0x42];
        p.extend_from_slice(&tid);
        p.extend_from_slice(&[0x00, 0x20, 0x00, 0x08, 0x00, 0x01]);
        p.extend_from_slice(&port.to_be_bytes());
        p.extend_from_slice(&ip);
        assert_eq!(parse_binding_response(&p, &tid), Some("203.0.113.5:40000".parse().unwrap()));
        assert_eq!(parse_binding_response(&p, &[0u8; 12]), None);
    }

    #[test]
    fn mdns_candidates_are_dropped() {
        let sdp = "v=0\r\na=candidate:1 1 udp 2113937151 abc.local 5000 typ host\r\na=candidate:2 1 udp 2113937151 10.0.0.2 5000 typ host\r\n";
        assert_eq!(clean_sdp(sdp), "v=0\r\na=candidate:2 1 udp 2113937151 10.0.0.2 5000 typ host\r\n");
    }
}
