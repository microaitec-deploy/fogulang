//! `fogu:foguMQ-client` module — client for the FoguMQ v6 wire protocol:
//! channels, pub/sub, RPC, job queues and API keys.
//!
//! ```fogu
//! import mqc from "fogu:foguMQ-client"
//!
//! const mq = mqc.connect("fogumq://token@host:7222?namespace=acme")?
//! print(mq.namespace, mq.authMode)
//!
//! mq.channelOpen(1)?
//! mq.channelOpen(2)?
//!
//! const sub = mq.subscribe(2, "orders.>")?
//! mq.publishSync(1, "orders.created", "order 42")?
//! print(sub.next(2s))                              // { topic, group, data }
//!
//! const svc = mq.serve(2, "users", "get")?        // RPC server side
//! fg fn() { const r = svc.next(); r.reply("user " + r.data) }()
//! print(mq.request(1, "users", "get", "42")?)      // user 42
//!
//! const jobs = mq.registerWorker(2, "emails", { prefetch: 3 })?
//! print(mq.enqueue(1, "emails", "welcome", "ok")?) // { jobId, seq }
//! const job = jobs.next(5s)
//! job.ack()
//!
//! mq.close()
//! ```
//!
//! Every method sends exactly one protocol frame. The socket is driven by
//! tasks on the shared tokio runtime (`rt::tokio()`); methods that wait for
//! the broker wait cooperatively, so other `fg` tasks keep running — a task
//! can answer the requests another one makes. Messages, requests and jobs
//! are queued in `Subscription` / `Service` / `Worker` objects and taken with
//! `next(timeout?)`. Network and broker failures are `error` values.
//!
//! Protocol:
//!   frame:  len:u32 | opcode:u8 | channel:u16 | request_id:u32 |
//!           topic:str | service:str | error_code:u16 | error_message:str |
//!           payload
//!   strings: len:u16 + utf8 (big endian)
//!   HELLO_OK: session(str) max_frame(u32) heartbeat(u16) request_timeout(u32)
//!             max_channels(u16) auth_mode(str)? key_id(str)? namespace(str)?
//!   URL: fogumq://[token@]host[:port]/?clientId=..&namespace=..&...

use std::cell::RefCell;
use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex, Notify};
use tokio::time::timeout;

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::stringify_value;
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::rt;
use crate::native::util::{duration_arg, map, max_args, nfn_var, opt_arg, str_arg};

// ===========================================================================
// 1. Errors
// ===========================================================================

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Url(String),
    BadFrame(String),
    FrameTooLarge(usize, usize),
    Broker { code: u16, message: String },
    ChannelNotOpen(u16),
    ChannelAlreadyOpen(u16),
    Timeout(Duration),
    Disconnected,
    RequiresMaster,
    Other(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "io: {}", e),
            ClientError::Url(s) => write!(f, "invalid url: {}", s),
            ClientError::BadFrame(s) => write!(f, "bad frame: {}", s),
            ClientError::FrameTooLarge(a, b) => write!(f, "frame too large: {} > {}", a, b),
            ClientError::Broker { code, message } => write!(f, "broker error {}: {}", code, message),
            ClientError::ChannelNotOpen(ch) => write!(f, "channel {} is not open", ch),
            ClientError::ChannelAlreadyOpen(ch) => write!(f, "channel {} is already open", ch),
            ClientError::Timeout(d) => write!(f, "timeout after {:?}", d),
            ClientError::Disconnected => write!(f, "connection closed"),
            ClientError::RequiresMaster => write!(f, "operation requires a master token"),
            ClientError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        ClientError::Io(e)
    }
}

// ===========================================================================
// 2. Opcodes / error codes (v6)
// ===========================================================================

pub const OP_HELLO: u8 = 0x01;
pub const OP_HELLO_OK: u8 = 0x02;
pub const OP_PING: u8 = 0x03;
pub const OP_PONG: u8 = 0x04;
pub const OP_ACK: u8 = 0x05;
pub const OP_ERROR: u8 = 0x06;
pub const OP_CLOSE: u8 = 0x07;
pub const OP_CHANNEL_OPEN: u8 = 0x08;
pub const OP_CHANNEL_CLOSE: u8 = 0x09;
pub const OP_SUBSCRIBE: u8 = 0x10;
pub const OP_UNSUBSCRIBE: u8 = 0x11;
pub const OP_PUBLISH: u8 = 0x12;
pub const OP_MESSAGE: u8 = 0x13;
pub const OP_SERVE: u8 = 0x20;
pub const OP_UNSERVE: u8 = 0x21;
pub const OP_REQUEST: u8 = 0x22;
pub const OP_RESPONSE: u8 = 0x23;
pub const OP_ENQUEUE: u8 = 0x30;
pub const OP_ENQUEUED: u8 = 0x31;
pub const OP_WORKER: u8 = 0x32;
pub const OP_WORKER_OFF: u8 = 0x33;
pub const OP_JOB: u8 = 0x34;
pub const OP_JOB_ACK: u8 = 0x35;
pub const OP_JOB_NACK: u8 = 0x36;
pub const OP_JOB_EXTEND: u8 = 0x37;
pub const OP_APIKEY_CREATE: u8 = 0x40;
pub const OP_APIKEY_REVOKE: u8 = 0x41;
pub const OP_APIKEY_LIST: u8 = 0x42;
pub const OP_APIKEY_GET: u8 = 0x43;

pub const ERR_BAD_FRAME: u16 = 1000;
pub const ERR_UNSUPPORTED_OP: u16 = 1001;
pub const ERR_FRAME_TOO_LARGE: u16 = 1002;
pub const ERR_UNSUPPORTED_VERSION: u16 = 1003;
pub const ERR_INVALID_CHANNEL: u16 = 1004;
pub const ERR_CHANNEL_EXISTS: u16 = 1005;
pub const ERR_UNAUTHORIZED: u16 = 2000;
pub const ERR_FORBIDDEN: u16 = 2001;
pub const ERR_NO_HANDLER: u16 = 3000;
pub const ERR_TIMEOUT: u16 = 3001;
pub const ERR_HANDLER_ERROR: u16 = 3002;
pub const ERR_QUEUE_NOT_FOUND: u16 = 4000;
pub const ERR_JOB_NOT_FOUND: u16 = 4001;
pub const ERR_QUEUE_FULL: u16 = 4002;
pub const ERR_INTERNAL: u16 = 5000;

pub const PROTOCOL_VERSION: u8 = 3;
pub const MIN_FRAME_BODY: usize = 19;

// ===========================================================================
// 3. Frames
// ===========================================================================

#[derive(Debug)]
pub enum FrameError {
    Bad(String),
    TooLarge(usize, usize),
}

impl From<FrameError> for ClientError {
    fn from(e: FrameError) -> Self {
        match e {
            FrameError::TooLarge(a, b) => ClientError::FrameTooLarge(a, b),
            FrameError::Bad(s) => ClientError::BadFrame(s),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub opcode: u8,
    pub channel: u16,
    pub request_id: u32,
    pub topic: String,
    pub service: String,
    pub error_code: u16,
    pub error_message: String,
    pub payload: Bytes,
}

impl Frame {
    pub fn encode(&self) -> BytesMut {
        let mut b = BytesMut::with_capacity(64);
        b.put_u8(self.opcode);
        b.put_u16(self.channel);
        b.put_u32(self.request_id);
        put_str(&mut b, &self.topic);
        put_str(&mut b, &self.service);
        b.put_u16(self.error_code);
        put_str(&mut b, &self.error_message);
        b.extend_from_slice(&self.payload);
        let mut out = BytesMut::with_capacity(4 + b.len());
        out.put_u32(b.len() as u32);
        out.extend_from_slice(&b);
        out
    }

    pub fn decode_body(mut body: Bytes) -> Result<Self, FrameError> {
        if body.len() < MIN_FRAME_BODY - 4 {
            return Err(FrameError::Bad(format!("short body: {}", body.len())));
        }
        let opcode = body.get_u8();
        let channel = body.get_u16();
        let request_id = body.get_u32();
        let topic = get_str(&mut body)?;
        let service = get_str(&mut body)?;
        if body.len() < 2 {
            return Err(FrameError::Bad("error code".into()));
        }
        let error_code = body.get_u16();
        let error_message = get_str(&mut body)?;
        Ok(Self { opcode, channel, request_id, topic, service, error_code, error_message, payload: body })
    }
}

fn put_str(buf: &mut BytesMut, s: &str) {
    let b = s.as_bytes();
    buf.put_u16(b.len() as u16);
    buf.extend_from_slice(b);
}

fn get_str(buf: &mut Bytes) -> Result<String, FrameError> {
    if buf.len() < 2 {
        return Err(FrameError::Bad("str len".into()));
    }
    let n = buf.get_u16() as usize;
    if buf.len() < n {
        return Err(FrameError::Bad("str body".into()));
    }
    let s = std::str::from_utf8(&buf[..n]).map_err(|_| FrameError::Bad("utf8".into()))?.to_owned();
    buf.advance(n);
    Ok(s)
}

#[derive(Default)]
pub struct Writer {
    pub buf: BytesMut,
}

impl Writer {
    pub fn new() -> Self {
        Self { buf: BytesMut::new() }
    }
    pub fn u8(mut self, v: u8) -> Self {
        self.buf.put_u8(v);
        self
    }
    pub fn u16(mut self, v: u16) -> Self {
        self.buf.put_u16(v);
        self
    }
    pub fn u32(mut self, v: u32) -> Self {
        self.buf.put_u32(v);
        self
    }
    pub fn u64(mut self, v: u64) -> Self {
        self.buf.put_u64(v);
        self
    }
    pub fn str(mut self, s: &str) -> Self {
        put_str(&mut self.buf, s);
        self
    }
    pub fn raw(mut self, d: &[u8]) -> Self {
        self.buf.extend_from_slice(d);
        self
    }
    pub fn build(self) -> Bytes {
        self.buf.freeze()
    }
}

pub struct Reader {
    buf: Bytes,
}

impl Reader {
    pub fn new(b: Bytes) -> Self {
        Self { buf: b }
    }
    pub fn u8(&mut self) -> Result<u8, FrameError> {
        if self.buf.is_empty() {
            return Err(FrameError::Bad("u8".into()));
        }
        Ok(self.buf.get_u8())
    }
    pub fn u16(&mut self) -> Result<u16, FrameError> {
        if self.buf.len() < 2 {
            return Err(FrameError::Bad("u16".into()));
        }
        Ok(self.buf.get_u16())
    }
    pub fn u32(&mut self) -> Result<u32, FrameError> {
        if self.buf.len() < 4 {
            return Err(FrameError::Bad("u32".into()));
        }
        Ok(self.buf.get_u32())
    }
    pub fn u64(&mut self) -> Result<u64, FrameError> {
        if self.buf.len() < 8 {
            return Err(FrameError::Bad("u64".into()));
        }
        Ok(self.buf.get_u64())
    }
    pub fn str(&mut self) -> Result<String, FrameError> {
        get_str(&mut self.buf)
    }
    pub fn rest(mut self) -> Bytes {
        std::mem::take(&mut self.buf)
    }
    pub fn remaining(&self) -> usize {
        self.buf.len()
    }
}

/// Splits a byte stream into frames.
pub struct Framer {
    buf: BytesMut,
    max_frame: usize,
}

impl Framer {
    pub fn new(max_frame: usize) -> Self {
        Self { buf: BytesMut::with_capacity(8 * 1024), max_frame }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FrameError> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 4 {
                break;
            }
            let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            if len > self.max_frame {
                return Err(FrameError::TooLarge(len, self.max_frame));
            }
            if len < MIN_FRAME_BODY - 4 {
                return Err(FrameError::Bad(format!("length too small: {}", len)));
            }
            if self.buf.len() < 4 + len {
                break;
            }
            self.buf.advance(4);
            let body = self.buf.split_to(len).freeze();
            out.push(Frame::decode_body(body)?);
        }
        Ok(out)
    }
}

// ===========================================================================
// 4. Options + URL
// ===========================================================================

#[derive(Debug, Clone)]
pub struct Options {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub auth_token: String,
    pub max_frame: usize,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_channels: u16,
    pub auto_reconnect: bool,
    pub reconnect_backoff: Duration,
    pub namespace: String,
    pub debug: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 7222,
            client_id: "fogu-client".into(),
            auth_token: String::new(),
            max_frame: 1 << 20,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(5),
            max_channels: 256,
            auto_reconnect: false,
            reconnect_backoff: Duration::from_secs(1),
            namespace: String::new(),
            debug: std::env::var("FoguMQ_DEBUG").as_deref() == Ok("1"),
        }
    }
}

pub fn parse_fogumq_url(raw: &str) -> Result<Options, String> {
    if raw.is_empty() {
        return Err("empty URL".into());
    }
    let norm = if raw.starts_with("fogumq:") && !raw.starts_with("fogumq://") {
        format!("fogumq://{}", &raw["fogumq:".len()..])
    } else {
        raw.to_string()
    };
    let rest = norm
        .strip_prefix("fogumq://")
        .ok_or_else(|| format!("expected a fogumq:// URL, got '{}'", raw))?;
    let (before_q, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (userinfo, host_part) = match before_q.rfind('@') {
        Some(i) => (&before_q[..i], &before_q[i + 1..]),
        None => ("", before_q),
    };
    let host_port = match host_part.find('/') {
        Some(i) => &host_part[..i],
        None => host_part,
    };
    let mut host = "localhost".to_string();
    let mut port: u16 = 7222;
    if let Some(r6) = host_port.strip_prefix('[') {
        let end = r6.find(']').ok_or_else(|| format!("invalid URL (IPv6 without ']'): {}", raw))?;
        host = r6[..end].to_string();
        if let Some(p) = r6[end + 1..].strip_prefix(':') {
            port = p.parse().map_err(|_| format!("invalid port: {}", p))?;
        }
    } else if let Some(i) = host_port.rfind(':') {
        if i > 0 {
            host = host_port[..i].to_string();
        }
        let p = &host_port[i + 1..];
        port = p.parse().map_err(|_| format!("invalid port: {}", p))?;
    } else if !host_port.is_empty() {
        host = host_port.to_string();
    }
    let mut opts = Options { host, port, auth_token: percent_decode(userinfo), ..Default::default() };
    for pair in query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = percent_decode(v);
        match k {
            "clientId" | "client_id" => opts.client_id = v,
            "maxFrame" | "max_frame" => {
                if let Ok(n) = v.parse() {
                    opts.max_frame = n;
                }
            }
            "requestTimeout" | "request_timeout" => {
                if let Ok(n) = v.parse::<u64>() {
                    opts.request_timeout = Duration::from_millis(n);
                }
            }
            "connectTimeout" | "connect_timeout" => {
                if let Ok(n) = v.parse::<u64>() {
                    opts.connect_timeout = Duration::from_millis(n);
                }
            }
            "maxChannels" | "max_channels" => {
                if let Ok(n) = v.parse() {
                    opts.max_channels = n;
                }
            }
            "autoReconnect" | "auto_reconnect" => opts.auto_reconnect = matches!(v.as_str(), "1" | "true" | "yes"),
            "reconnectBackoff" | "reconnect_backoff" => {
                if let Ok(n) = v.parse::<u64>() {
                    opts.reconnect_backoff = Duration::from_millis(n);
                }
            }
            "namespace" | "ns" => opts.namespace = v,
            _ => {}
        }
    }
    Ok(opts)
}

fn percent_decode(s: &str) -> String {
    crate::native::util::url_decode(s)
}

// ===========================================================================
// 5. Public types
// ===========================================================================

pub type MessageHandler = Arc<dyn Fn(String, String, Bytes) + Send + Sync + 'static>;
pub type HandlerFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Bytes, String>> + Send>>;
pub type JobHandler = Arc<dyn Fn(Job) -> JobFuture + Send + Sync + 'static>;
pub type JobFuture = std::pin::Pin<Box<dyn std::future::Future<Output = JobAction> + Send>>;
type ServeHandler = Arc<dyn Fn(String, Bytes) -> HandlerFuture + Send + Sync + 'static>;

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub attempt: u8,
    pub deadline_ms: u32,
    pub data: Bytes,
    pub queue: String,
    pub topic: String,
    pub channel: u16,
}

#[derive(Debug, Clone)]
pub enum JobAction {
    Ack,
    Nack { requeue: bool, delay_ms: u32, reason: String },
    Extend { extend_ms: u32 },
}

#[derive(Debug, Clone, Default)]
pub struct EnqueueOptions {
    pub job_id: String,
    pub priority: u8,
    pub delay_ms: u32,
    pub retries: u8,
    pub ttl_ms: u32,
}

#[derive(Debug, Clone)]
pub struct Enqueued {
    pub job_id: String,
    pub seq: u64,
}

#[derive(Debug, Clone)]
pub struct APIKeyCreateResult {
    pub id: String,
    pub token: String,
    pub permissions: String,
}

// ===========================================================================
// 6. Client state
// ===========================================================================

struct Pending {
    tx: oneshot::Sender<Result<Response, ClientError>>,
}

struct Response {
    code: u16,
    message: String,
    payload: Bytes,
}

#[derive(Clone)]
struct HandlerEntry {
    channel: u16,
    service: String,
    topic: String,
    func: ServeHandler,
}

#[derive(Clone)]
struct Subscription {
    pattern: String,
    group: String,
    cb: MessageHandler,
}

#[derive(Clone)]
struct WorkerEntry {
    queue: String,
    filter: String,
    prefetch: u16,
    visibility_ms: u32,
    handler: JobHandler,
}

/// Protocol state. Never held across socket I/O (the writer has its own lock).
struct Inner {
    connected: bool,
    closing: bool,
    /// Bumped on every connect: old ping loops stop.
    generation: u64,
    hello_wait: Option<oneshot::Sender<Result<(), ClientError>>>,
    session_id: String,
    max_frame: usize,
    heartbeat: Duration,
    request_timeout: Duration,
    max_channels: u16,
    auth_mode: String,
    key_id: String,
    namespace: String,
    channels: HashMap<u16, ()>,
    next_request_id: HashMap<u16, u32>,
    pending: HashMap<(u16, u32), Pending>,
    handlers: HashMap<String, HandlerEntry>,
    subscriptions: HashMap<u16, Vec<Subscription>>,
    workers: HashMap<u16, WorkerEntry>,
    desired_channels: HashMap<u16, ()>,
    desired_subscriptions: HashMap<u16, Vec<Subscription>>,
    desired_handlers: HashMap<u16, Vec<HandlerEntry>>,
    desired_workers: HashMap<u16, Vec<WorkerEntry>>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            connected: false,
            closing: false,
            generation: 0,
            hello_wait: None,
            session_id: String::new(),
            max_frame: 1 << 20,
            heartbeat: Duration::from_secs(30),
            request_timeout: Duration::from_secs(5),
            max_channels: 256,
            auth_mode: "unknown".into(),
            key_id: String::new(),
            namespace: String::new(),
            channels: HashMap::new(),
            next_request_id: HashMap::new(),
            pending: HashMap::new(),
            handlers: HashMap::new(),
            subscriptions: HashMap::new(),
            workers: HashMap::new(),
            desired_channels: HashMap::new(),
            desired_subscriptions: HashMap::new(),
            desired_handlers: HashMap::new(),
            desired_workers: HashMap::new(),
        }
    }
}

// ===========================================================================
// 7. Client (async, runs on the shared tokio runtime)
// ===========================================================================

#[derive(Clone)]
pub struct Client {
    opts: Options,
    inner: Arc<Mutex<Inner>>,
    writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
    closed: Arc<Notify>,
}

impl Client {
    pub fn new(opts: Options) -> Self {
        Self {
            opts,
            inner: Arc::new(Mutex::new(Inner::default())),
            writer: Arc::new(Mutex::new(None)),
            closed: Arc::new(Notify::new()),
        }
    }

    pub fn opts(&self) -> &Options {
        &self.opts
    }

    /// Reads the state from a non-async context (the lock is never held long).
    fn snapshot<T>(&self, f: impl Fn(&Inner) -> T) -> T {
        loop {
            if let Ok(g) = self.inner.try_lock() {
                return f(&g);
            }
            std::thread::yield_now();
        }
    }

    /// Boxed: a reconnect calls it again from the reader task.
    pub fn connect(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ClientError>> + Send + '_>> {
        Box::pin(self.connect_inner())
    }

    async fn connect_inner(&self) -> Result<(), ClientError> {
        if self.inner.lock().await.connected {
            return Ok(());
        }
        let addr = format!("{}:{}", self.opts.host, self.opts.port);
        let stream = timeout(self.opts.connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| ClientError::Timeout(self.opts.connect_timeout))??;
        stream.set_nodelay(true).ok();
        let (rd, wr) = stream.into_split();
        let (htx, hrx) = oneshot::channel();
        *self.writer.lock().await = Some(wr);
        let rid = {
            let mut g = self.inner.lock().await;
            g.connected = true;
            g.closing = false;
            g.generation += 1;
            g.hello_wait = Some(htx);
            Self::bump_request_id(&mut g, 0)
        };
        let payload = Writer::new()
            .u8(PROTOCOL_VERSION)
            .str(&self.opts.client_id)
            .u32(self.opts.max_frame as u32)
            .str(&self.opts.auth_token)
            .build();
        self.write_frame(&Frame { opcode: OP_HELLO, channel: 0, request_id: rid, payload, ..Default::default() })
            .await?;
        let me = self.clone();
        tokio::spawn(async move { me.read_loop(rd).await });
        match timeout(self.opts.connect_timeout, hrx).await {
            Err(_) => {
                self.force_close().await;
                Err(ClientError::Timeout(self.opts.connect_timeout))
            }
            Ok(Err(_)) => {
                self.force_close().await;
                Err(ClientError::Disconnected)
            }
            Ok(Ok(Ok(()))) => {
                self.start_ping_loop().await;
                Ok(())
            }
            Ok(Ok(Err(e))) => {
                self.force_close().await;
                Err(e)
            }
        }
    }

    pub async fn close(&self) -> Result<(), ClientError> {
        {
            let mut g = self.inner.lock().await;
            if g.closing {
                return Ok(());
            }
            g.closing = true;
            g.connected = false;
        }
        if let Some(mut wr) = self.writer.lock().await.take() {
            let f = Frame { opcode: OP_CLOSE, ..Default::default() };
            let _ = wr.write_all(&f.encode()).await;
            let _ = wr.shutdown().await;
        }
        self.closed.notify_waiters();
        Ok(())
    }

    async fn force_close(&self) {
        if let Some(mut wr) = self.writer.lock().await.take() {
            let _ = wr.shutdown().await;
        }
        self.inner.lock().await.connected = false;
    }

    fn bump_request_id(g: &mut Inner, ch: u16) -> u32 {
        let cur = g.next_request_id.entry(ch).or_insert(0);
        *cur = cur.wrapping_add(1);
        if *cur == 0 {
            *cur = 1;
        }
        *cur
    }

    async fn next_rid(&self, ch: u16) -> u32 {
        Self::bump_request_id(&mut *self.inner.lock().await, ch)
    }

    async fn write_frame(&self, f: &Frame) -> Result<(), ClientError> {
        let mut w = self.writer.lock().await;
        let wr = w.as_mut().ok_or(ClientError::Disconnected)?;
        wr.write_all(&f.encode()).await?;
        Ok(())
    }

    async fn read_loop(&self, mut rd: tokio::net::tcp::OwnedReadHalf) {
        let mut framer = Framer::new(self.opts.max_frame);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let frames = match framer.feed(&buf[..n]) {
                Ok(f) => f,
                Err(e) => {
                    self.emit_error(e.into());
                    break;
                }
            };
            for f in frames {
                self.dispatch(f).await;
            }
        }
        self.handle_disconnect().await;
    }

    async fn handle_disconnect(&self) {
        let (was, closing, pend) = {
            let mut g = self.inner.lock().await;
            let was = g.connected;
            g.connected = false;
            if let Some(tx) = g.hello_wait.take() {
                let _ = tx.send(Err(ClientError::Disconnected));
            }
            (was, g.closing, std::mem::take(&mut g.pending))
        };
        self.writer.lock().await.take();
        for (_, p) in pend {
            let _ = p.tx.send(Err(ClientError::Disconnected));
        }
        sched::wake();
        if was && !closing && self.opts.auto_reconnect {
            let me = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(me.opts.reconnect_backoff).await;
                if me.connect().await.is_ok() {
                    me.reapply_desired().await;
                }
            });
        }
    }

    /// After a reconnect: opens the channels and registers everything again.
    async fn reapply_desired(&self) {
        let (channels, subs, handlers, workers) = {
            let mut g = self.inner.lock().await;
            let chs: Vec<u16> = g.desired_channels.keys().copied().collect();
            let subs: Vec<(u16, Subscription)> = g
                .desired_subscriptions
                .iter()
                .flat_map(|(ch, v)| v.iter().map(move |s| (*ch, s.clone())))
                .collect();
            let hs: Vec<(u16, HandlerEntry)> = g
                .desired_handlers
                .iter()
                .flat_map(|(ch, v)| v.iter().map(move |h| (*ch, h.clone())))
                .collect();
            let ws: Vec<(u16, WorkerEntry)> = g
                .desired_workers
                .iter()
                .flat_map(|(ch, v)| v.iter().map(move |w| (*ch, w.clone())))
                .collect();
            // registering again pushes them back
            g.desired_subscriptions.clear();
            g.desired_handlers.clear();
            g.desired_workers.clear();
            (chs, subs, hs, ws)
        };
        for ch in channels {
            if let Err(e) = self.channel_open(ch).await {
                self.emit_error(e);
            }
        }
        for (ch, s) in subs {
            let cb = s.cb.clone();
            if let Err(e) = self.subscribe(ch, &s.pattern, &s.group, move |t, g, p| cb(t, g, p)).await {
                self.emit_error(e);
            }
        }
        for (ch, h) in handlers {
            let f = h.func.clone();
            if let Err(e) = self.serve(ch, &h.service, &h.topic, move |t, p| f(t, p)).await {
                self.emit_error(e);
            }
        }
        for (ch, w) in workers {
            let f = w.handler.clone();
            if let Err(e) = self.register_worker(ch, &w.queue, &w.filter, w.prefetch, w.visibility_ms, move |j| f(j)).await {
                self.emit_error(e);
            }
        }
    }

    async fn dispatch(&self, f: Frame) {
        match f.opcode {
            OP_HELLO_OK => self.handle_hello_ok(f).await,
            OP_PING => {
                let _ = self.write_frame(&Frame { opcode: OP_PONG, ..Default::default() }).await;
            }
            OP_PONG => {}
            OP_CLOSE => {
                self.inner.lock().await.closing = true;
                if let Some(mut wr) = self.writer.lock().await.take() {
                    let _ = wr.shutdown().await;
                }
            }
            OP_ERROR => self.handle_error(f).await,
            OP_CHANNEL_CLOSE => self.handle_channel_close(f).await,
            OP_ACK => self.resolve(f.channel, f.request_id, Response { code: 0, message: String::new(), payload: Bytes::new() }).await,
            OP_MESSAGE => self.handle_message(f).await,
            OP_REQUEST => self.handle_request(f).await,
            OP_RESPONSE => {
                let resp = Response { code: f.error_code, message: f.error_message, payload: f.payload };
                self.resolve(f.channel, f.request_id, resp).await
            }
            OP_ENQUEUED | OP_APIKEY_CREATE | OP_APIKEY_GET | OP_APIKEY_LIST => {
                let resp = Response { code: 0, message: String::new(), payload: f.payload };
                self.resolve(f.channel, f.request_id, resp).await
            }
            OP_JOB => self.handle_job(f).await,
            _ => {}
        }
    }

    async fn handle_hello_ok(&self, f: Frame) {
        let mut r = Reader::new(f.payload);
        let session = r.str().unwrap_or_default();
        let max_frame = r.u32().unwrap_or(1 << 20) as usize;
        let hb = r.u16().unwrap_or(30) as u64;
        let rt = r.u32().unwrap_or(5000) as u64;
        let mc = r.u16().unwrap_or(256);
        let (mut mode, mut kid, mut ns) = (String::new(), String::new(), String::new());
        if r.remaining() > 0 {
            mode = r.str().unwrap_or_default();
        }
        if r.remaining() > 0 {
            kid = r.str().unwrap_or_default();
        }
        if r.remaining() > 0 {
            ns = r.str().unwrap_or_default();
        }
        let htx = {
            let mut g = self.inner.lock().await;
            g.session_id = session;
            g.max_frame = max_frame;
            g.heartbeat = Duration::from_secs(hb.max(1));
            g.request_timeout = Duration::from_millis(rt.max(1));
            g.max_channels = mc;
            g.auth_mode = if mode.is_empty() { "unknown".into() } else { mode };
            g.key_id = kid;
            g.namespace = ns;
            g.hello_wait.take()
        };
        if let Some(tx) = htx {
            let _ = tx.send(Ok(()));
        }
    }

    async fn handle_error(&self, f: Frame) {
        if f.request_id != 0 {
            let resp = Response { code: f.error_code, message: f.error_message.clone(), payload: Bytes::new() };
            self.resolve(f.channel, f.request_id, resp).await;
        }
        if f.channel == 0 && f.request_id != 0 {
            if let Some(tx) = self.inner.lock().await.hello_wait.take() {
                let _ = tx.send(Err(ClientError::Broker { code: f.error_code, message: f.error_message.clone() }));
            }
        }
        self.emit_error(ClientError::Broker { code: f.error_code, message: f.error_message });
    }

    async fn handle_channel_close(&self, f: Frame) {
        let pend = {
            let mut g = self.inner.lock().await;
            g.channels.remove(&f.channel);
            g.subscriptions.remove(&f.channel);
            g.workers.remove(&f.channel);
            let keys: Vec<_> = g.pending.keys().filter(|(ch, _)| *ch == f.channel).cloned().collect();
            keys.iter().filter_map(|k| g.pending.remove(k)).collect::<Vec<_>>()
        };
        for p in pend {
            let _ = p.tx.send(Err(ClientError::ChannelNotOpen(f.channel)));
        }
    }

    async fn handle_message(&self, f: Frame) {
        let subs = self.inner.lock().await.subscriptions.get(&f.channel).cloned().unwrap_or_default();
        for s in subs {
            if match_topic(&s.pattern, &f.topic) {
                (s.cb)(f.topic.clone(), f.service.clone(), f.payload.clone());
            }
        }
    }

    async fn handle_request(&self, f: Frame) {
        let found = {
            let g = self.inner.lock().await;
            g.handlers.get(&format!("{}|{}", f.service, f.topic)).cloned().or_else(|| {
                g.handlers
                    .iter()
                    .find(|(k, _)| {
                        let (svc, pat) = split_handler_key(k);
                        svc == f.service && match_topic(pat, &f.topic)
                    })
                    .map(|(_, v)| v.clone())
            })
        };
        let Some(entry) = found else {
            let resp = Frame {
                opcode: OP_RESPONSE,
                channel: f.channel,
                request_id: f.request_id,
                error_code: ERR_NO_HANDLER,
                error_message: "no handler".into(),
                ..Default::default()
            };
            let _ = self.write_frame(&resp).await;
            return;
        };
        let me = self.clone();
        let (ch, rid) = (f.channel, f.request_id);
        tokio::spawn(async move {
            let resp = match (entry.func)(f.topic, f.payload).await {
                Ok(bytes) => Frame { opcode: OP_RESPONSE, channel: ch, request_id: rid, payload: bytes, ..Default::default() },
                Err(msg) => Frame {
                    opcode: OP_RESPONSE,
                    channel: ch,
                    request_id: rid,
                    error_code: ERR_HANDLER_ERROR,
                    error_message: msg,
                    ..Default::default()
                },
            };
            let _ = me.write_frame(&resp).await;
        });
    }

    async fn handle_job(&self, f: Frame) {
        let entry = self.inner.lock().await.workers.get(&f.channel).cloned();
        let Some(w) = entry else {
            let resp = Frame {
                opcode: OP_JOB_NACK,
                channel: f.channel,
                service: f.service,
                error_code: ERR_JOB_NOT_FOUND,
                error_message: "no worker".into(),
                payload: Writer::new().str("").u8(0).u32(0).build(),
                ..Default::default()
            };
            let _ = self.write_frame(&resp).await;
            return;
        };
        let mut r = Reader::new(f.payload);
        let id = r.str().unwrap_or_default();
        let attempt = r.u8().unwrap_or(1);
        let deadline_ms = r.u32().unwrap_or(0);
        let data = r.rest();
        let job = Job { id: id.clone(), attempt, deadline_ms, data, queue: f.service.clone(), topic: f.topic, channel: f.channel };
        let me = self.clone();
        let (q, ch) = (f.service, f.channel);
        tokio::spawn(async move {
            let _ = match (w.handler)(job).await {
                JobAction::Ack => me.job_ack(ch, &q, &id).await,
                JobAction::Nack { requeue, delay_ms, reason } => me.job_nack(ch, &q, &id, requeue, delay_ms, &reason).await,
                JobAction::Extend { extend_ms } => me.job_extend(ch, &q, &id, extend_ms).await,
            };
        });
    }

    async fn resolve(&self, ch: u16, rid: u32, resp: Response) {
        if let Some(p) = self.inner.lock().await.pending.remove(&(ch, rid)) {
            let _ = p.tx.send(Ok(resp));
        }
    }

    /// Sends `f` and waits for its answer (ACK, RESPONSE, ERROR…).
    async fn rpc(&self, ch: u16, rid: u32, f: Frame) -> Result<Response, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.inner.lock().await.pending.insert((ch, rid), Pending { tx });
        if let Err(e) = self.write_frame(&f).await {
            self.inner.lock().await.pending.remove(&(ch, rid));
            return Err(e);
        }
        let to = self.opts.request_timeout;
        match timeout(to, rx).await {
            Err(_) => {
                self.inner.lock().await.pending.remove(&(ch, rid));
                Err(ClientError::Timeout(to))
            }
            Ok(Err(_)) => Err(ClientError::Disconnected),
            Ok(Ok(r)) => {
                let r = r?;
                if r.code != 0 {
                    return Err(ClientError::Broker { code: r.code, message: r.message });
                }
                Ok(r)
            }
        }
    }

    // --- one method per v6 frame ---

    pub async fn channel_open(&self, ch: u16) -> Result<(), ClientError> {
        if ch == 0 {
            return Err(ClientError::ChannelNotOpen(0));
        }
        let rid = {
            let mut g = self.inner.lock().await;
            if g.channels.contains_key(&ch) {
                return Err(ClientError::ChannelAlreadyOpen(ch));
            }
            Self::bump_request_id(&mut g, ch)
        };
        self.rpc(ch, rid, Frame { opcode: OP_CHANNEL_OPEN, channel: ch, request_id: rid, ..Default::default() }).await?;
        let mut g = self.inner.lock().await;
        g.channels.insert(ch, ());
        g.desired_channels.insert(ch, ());
        Ok(())
    }

    pub async fn channel_close(&self, ch: u16) -> Result<(), ClientError> {
        if ch == 0 {
            return Err(ClientError::ChannelNotOpen(0));
        }
        if !self.inner.lock().await.channels.contains_key(&ch) {
            return Ok(());
        }
        let rid = self.next_rid(ch).await;
        let _ = self.rpc(ch, rid, Frame { opcode: OP_CHANNEL_CLOSE, channel: ch, request_id: rid, ..Default::default() }).await;
        let mut g = self.inner.lock().await;
        g.channels.remove(&ch);
        g.subscriptions.remove(&ch);
        g.workers.remove(&ch);
        g.handlers.retain(|_, h| h.channel != ch);
        g.desired_channels.remove(&ch);
        g.desired_subscriptions.remove(&ch);
        g.desired_handlers.remove(&ch);
        g.desired_workers.remove(&ch);
        Ok(())
    }

    pub async fn subscribe<F>(&self, ch: u16, pattern: &str, group: &str, cb: F) -> Result<(), ClientError>
    where
        F: Fn(String, String, Bytes) + Send + Sync + 'static,
    {
        self.ensure_open(ch).await?;
        let rid = self.next_rid(ch).await;
        self.rpc(ch, rid, Frame {
            opcode: OP_SUBSCRIBE,
            channel: ch,
            request_id: rid,
            topic: pattern.into(),
            service: group.into(),
            ..Default::default()
        })
        .await?;
        let mut g = self.inner.lock().await;
        let sub = Subscription { pattern: pattern.into(), group: group.into(), cb: Arc::new(cb) };
        g.subscriptions.entry(ch).or_default().push(sub.clone());
        g.desired_subscriptions.entry(ch).or_default().push(sub);
        Ok(())
    }

    pub async fn unsubscribe(&self, ch: u16, pattern: &str, group: &str) -> Result<(), ClientError> {
        self.ensure_open(ch).await?;
        let rid = self.next_rid(ch).await;
        self.rpc(ch, rid, Frame {
            opcode: OP_UNSUBSCRIBE,
            channel: ch,
            request_id: rid,
            topic: pattern.into(),
            service: group.into(),
            ..Default::default()
        })
        .await?;
        let mut g = self.inner.lock().await;
        let keep = |s: &Subscription| !(s.pattern == pattern && s.group == group);
        if let Some(v) = g.subscriptions.get_mut(&ch) {
            v.retain(keep);
        }
        if let Some(v) = g.desired_subscriptions.get_mut(&ch) {
            v.retain(keep);
        }
        Ok(())
    }

    /// Fire and forget (request id 0: the broker does not answer).
    pub async fn publish(&self, ch: u16, topic: &str, data: impl Into<Bytes>) -> Result<(), ClientError> {
        self.ensure_open(ch).await?;
        self.write_frame(&Frame { opcode: OP_PUBLISH, channel: ch, request_id: 0, topic: topic.into(), payload: data.into(), ..Default::default() })
            .await
    }

    pub async fn publish_sync(&self, ch: u16, topic: &str, data: impl Into<Bytes>) -> Result<(), ClientError> {
        self.ensure_open(ch).await?;
        let rid = self.next_rid(ch).await;
        self.rpc(ch, rid, Frame { opcode: OP_PUBLISH, channel: ch, request_id: rid, topic: topic.into(), payload: data.into(), ..Default::default() })
            .await
            .map(|_| ())
    }

    pub async fn serve<F, Fut>(&self, ch: u16, service: &str, topic: &str, f: F) -> Result<(), ClientError>
    where
        F: Fn(String, Bytes) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Bytes, String>> + Send + 'static,
    {
        self.ensure_open(ch).await?;
        let rid = self.next_rid(ch).await;
        self.rpc(ch, rid, Frame {
            opcode: OP_SERVE,
            channel: ch,
            request_id: rid,
            service: service.into(),
            topic: topic.into(),
            ..Default::default()
        })
        .await?;
        let func: ServeHandler = Arc::new(move |t, p| -> HandlerFuture { Box::pin(f(t, p)) });
        let mut g = self.inner.lock().await;
        let entry = HandlerEntry { channel: ch, service: service.into(), topic: topic.into(), func };
        g.handlers.insert(format!("{}|{}", service, topic), entry.clone());
        g.desired_handlers.entry(ch).or_default().push(entry);
        Ok(())
    }

    pub async fn unserve(&self, ch: u16, service: &str, topic: &str) -> Result<(), ClientError> {
        let rid = self.next_rid(ch).await;
        self.rpc(ch, rid, Frame {
            opcode: OP_UNSERVE,
            channel: ch,
            request_id: rid,
            service: service.into(),
            topic: topic.into(),
            ..Default::default()
        })
        .await?;
        let mut g = self.inner.lock().await;
        g.handlers.remove(&format!("{}|{}", service, topic));
        if let Some(v) = g.desired_handlers.get_mut(&ch) {
            v.retain(|h| !(h.service == service && h.topic == topic));
        }
        Ok(())
    }

    pub async fn request(&self, ch: u16, service: &str, topic: &str, data: impl Into<Bytes>) -> Result<Bytes, ClientError> {
        self.ensure_open(ch).await?;
        let rid = self.next_rid(ch).await;
        let r = self
            .rpc(ch, rid, Frame {
                opcode: OP_REQUEST,
                channel: ch,
                request_id: rid,
                service: service.into(),
                topic: topic.into(),
                payload: data.into(),
                ..Default::default()
            })
            .await?;
        Ok(r.payload)
    }

    pub async fn register_worker<F, Fut>(&self, ch: u16, queue: &str, filter: &str, prefetch: u16, visibility_ms: u32, f: F) -> Result<(), ClientError>
    where
        F: Fn(Job) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = JobAction> + Send + 'static,
    {
        self.ensure_open(ch).await?;
        let filter = if filter.is_empty() { ">" } else { filter };
        let prefetch = prefetch.max(1);
        let visibility_ms = if visibility_ms == 0 { 30_000 } else { visibility_ms };
        let rid = self.next_rid(ch).await;
        let payload = Writer::new().u16(prefetch).u32(visibility_ms).build();
        self.rpc(ch, rid, Frame {
            opcode: OP_WORKER,
            channel: ch,
            request_id: rid,
            topic: filter.into(),
            service: queue.into(),
            payload,
            ..Default::default()
        })
        .await?;
        let handler: JobHandler = Arc::new(move |j| -> JobFuture { Box::pin(f(j)) });
        let mut g = self.inner.lock().await;
        let entry = WorkerEntry { queue: queue.into(), filter: filter.into(), prefetch, visibility_ms, handler };
        g.workers.insert(ch, entry.clone());
        g.desired_workers.entry(ch).or_default().push(entry);
        Ok(())
    }

    pub async fn worker_off(&self, ch: u16, queue: &str) -> Result<(), ClientError> {
        let rid = self.next_rid(ch).await;
        self.write_frame(&Frame { opcode: OP_WORKER_OFF, channel: ch, request_id: rid, service: queue.into(), ..Default::default() })
            .await?;
        let mut g = self.inner.lock().await;
        g.workers.remove(&ch);
        g.desired_workers.remove(&ch);
        Ok(())
    }

    pub async fn enqueue(&self, ch: u16, queue: &str, job_type: &str, data: impl Into<Bytes>, opts: EnqueueOptions) -> Result<Enqueued, ClientError> {
        self.ensure_open(ch).await?;
        let rid = self.next_rid(ch).await;
        let payload = Writer::new()
            .str(&opts.job_id)
            .u8(opts.priority)
            .u32(opts.delay_ms)
            .u8(opts.retries)
            .u32(opts.ttl_ms)
            .raw(&data.into())
            .build();
        let r = self
            .rpc(ch, rid, Frame {
                opcode: OP_ENQUEUE,
                channel: ch,
                request_id: rid,
                service: queue.into(),
                topic: job_type.into(),
                payload,
                ..Default::default()
            })
            .await?;
        let mut rr = Reader::new(r.payload);
        Ok(Enqueued { job_id: rr.str().unwrap_or_default(), seq: rr.u64().unwrap_or(0) })
    }

    pub async fn job_ack(&self, ch: u16, queue: &str, job_id: &str) -> Result<(), ClientError> {
        self.write_frame(&Frame { opcode: OP_JOB_ACK, channel: ch, service: queue.into(), payload: Writer::new().str(job_id).build(), ..Default::default() })
            .await
    }

    pub async fn job_nack(&self, ch: u16, queue: &str, job_id: &str, requeue: bool, delay_ms: u32, reason: &str) -> Result<(), ClientError> {
        self.write_frame(&Frame {
            opcode: OP_JOB_NACK,
            channel: ch,
            service: queue.into(),
            error_code: ERR_HANDLER_ERROR,
            error_message: reason.into(),
            payload: Writer::new().str(job_id).u8(requeue as u8).u32(delay_ms).build(),
            ..Default::default()
        })
        .await
    }

    pub async fn job_extend(&self, ch: u16, queue: &str, job_id: &str, extend_ms: u32) -> Result<(), ClientError> {
        self.write_frame(&Frame {
            opcode: OP_JOB_EXTEND,
            channel: ch,
            service: queue.into(),
            payload: Writer::new().str(job_id).u32(extend_ms).build(),
            ..Default::default()
        })
        .await
    }

    async fn require_master(&self) -> Result<(), ClientError> {
        if self.inner.lock().await.auth_mode != "master" {
            return Err(ClientError::RequiresMaster);
        }
        Ok(())
    }

    pub async fn api_key_create(&self, ch: u16, name: &str, perms: serde_json::Value, labels: &[String], namespace: &str) -> Result<APIKeyCreateResult, ClientError> {
        self.require_master().await?;
        let pjson = serde_json::to_string(&perms).map_err(|e| ClientError::Other(e.to_string()))?;
        let rid = self.next_rid(ch).await;
        let payload = Writer::new().str(name).str(&pjson).str(&labels.join(",")).str(namespace).build();
        let r = self.rpc(ch, rid, Frame { opcode: OP_APIKEY_CREATE, channel: ch, request_id: rid, payload, ..Default::default() }).await?;
        let mut rr = Reader::new(r.payload);
        Ok(APIKeyCreateResult {
            id: rr.str().unwrap_or_default(),
            token: rr.str().unwrap_or_default(),
            permissions: rr.str().unwrap_or_default(),
        })
    }

    pub async fn api_key_revoke(&self, ch: u16, id: &str) -> Result<(), ClientError> {
        self.require_master().await?;
        let rid = self.next_rid(ch).await;
        self.rpc(ch, rid, Frame { opcode: OP_APIKEY_REVOKE, channel: ch, request_id: rid, payload: Writer::new().str(id).build(), ..Default::default() })
            .await
            .map(|_| ())
    }

    pub async fn api_key_get(&self, ch: u16, id: &str) -> Result<String, ClientError> {
        self.require_master().await?;
        let rid = self.next_rid(ch).await;
        let r = self
            .rpc(ch, rid, Frame { opcode: OP_APIKEY_GET, channel: ch, request_id: rid, payload: Writer::new().str(id).build(), ..Default::default() })
            .await?;
        Ok(Reader::new(r.payload).str().unwrap_or_default())
    }

    pub async fn api_key_list(&self, ch: u16) -> Result<String, ClientError> {
        self.require_master().await?;
        let rid = self.next_rid(ch).await;
        let r = self.rpc(ch, rid, Frame { opcode: OP_APIKEY_LIST, channel: ch, request_id: rid, ..Default::default() }).await?;
        Ok(Reader::new(r.payload).str().unwrap_or_default())
    }

    async fn ensure_open(&self, ch: u16) -> Result<(), ClientError> {
        if !self.inner.lock().await.channels.contains_key(&ch) {
            return Err(ClientError::ChannelNotOpen(ch));
        }
        Ok(())
    }

    async fn start_ping_loop(&self) {
        let (interval, generation) = {
            let g = self.inner.lock().await;
            ((g.heartbeat / 2).max(Duration::from_secs(1)), g.generation)
        };
        let me = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            tick.tick().await; // the first tick is immediate
            loop {
                tick.tick().await;
                {
                    let g = me.inner.lock().await;
                    if !g.connected || g.generation != generation {
                        return;
                    }
                }
                let _ = me.write_frame(&Frame { opcode: OP_PING, ..Default::default() }).await;
            }
        });
    }

    fn emit_error(&self, e: ClientError) {
        if self.opts.debug {
            eprintln!("[foguMQ-client] {}", e);
        }
    }
}

// ===========================================================================
// 8. Helpers
// ===========================================================================

/// `*` matches one token, `>` one or more trailing tokens.
pub fn match_topic(pattern: &str, topic: &str) -> bool {
    let t: Vec<&str> = topic.split('.').collect();
    let mut i = 0;
    for tok in pattern.split('.') {
        if tok == ">" {
            return i < t.len();
        }
        if i >= t.len() {
            return false;
        }
        if tok != "*" && tok != t[i] {
            return false;
        }
        i += 1;
    }
    i == t.len()
}

fn split_handler_key(k: &str) -> (&str, &str) {
    k.split_once('|').unwrap_or((k, ""))
}

// ===========================================================================
// 9. The Fogu module
// ===========================================================================

const MOD: &str = "foguMQ-client";

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    ModuleValue {
        name: "fogu:foguMQ-client".into(),
        exports: RefCell::new(e),
    }
}

/// An operational failure as an `error` value.
fn op_err(e: ClientError) -> String {
    io_err!("{}: {}", MOD, e)
}

/// Keeps the scheduler polling while an inbox waits.
fn poll_again() {
    sched::add_timer(Instant::now() + Duration::from_millis(2));
}

/// Runs `fut` on the shared runtime; the caller waits cooperatively and
/// `finish` turns the result into a Fogu value on the interpreter thread.
fn async_call<T: Send + 'static>(
    fut: impl std::future::Future<Output = Result<T, ClientError>> + Send + 'static,
    finish: impl FnOnce(T) -> Value + 'static,
) -> Poller {
    let pending = rt::spawn(fut);
    let mut finish = Some(finish);
    Box::new(move || match pending.try_take()? {
        Ok(v) => Some(Ok(finish.take().expect("polled after completion")(v))),
        Err(e) => Some(Err(op_err(e))),
    })
}

/// Payloads: a str is sent as is, anything else as JSON.
fn payload(v: &Value) -> Bytes {
    match v {
        Value::Str(s) => Bytes::from(s.as_bytes().to_vec()),
        Value::Nil => Bytes::new(),
        other => Bytes::from(stringify_value(other).into_bytes()),
    }
}

fn text(b: &Bytes) -> Value {
    Value::Str(String::from_utf8_lossy(b).into_owned().into())
}

fn int_arg(args: &[Value], i: usize, what: &str) -> Result<i64, String> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(format!("{}: argument {} must be int, got '{}'", what, i + 1, other.type_name())),
        None => Err(format!("{}: missing argument {}", what, i + 1)),
    }
}

fn channel_arg(args: &[Value], what: &str) -> Result<u16, String> {
    let n = int_arg(args, 0, what)?;
    u16::try_from(n).ok().filter(|&c| c > 0).ok_or_else(|| format!("{}: channel must be 1..65535, got {}", what, n))
}

fn opt_str(args: &[Value], i: usize, what: &str) -> Result<String, String> {
    match opt_arg(args, i) {
        None => Ok(String::new()),
        Some(_) => str_arg(args, i, what),
    }
}

/// `opts.key` as a non-negative int that fits `max`.
fn opt_int(m: &HashMap<String, Value>, key: &str, max: i64, what: &str) -> Result<Option<i64>, String> {
    match m.get(key) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Int(n)) if (0..=max).contains(n) => Ok(Some(*n)),
        Some(_) => Err(format!("{}: '{}' must be an int in 0..{}", what, key, max)),
    }
}

/// `opts.key` as milliseconds (a duration or an int).
fn opt_ms(m: &HashMap<String, Value>, key: &str, what: &str) -> Result<Option<u32>, String> {
    match m.get(key) {
        None | Some(Value::Nil) => Ok(None),
        Some(v) => {
            let d = duration_arg(v, &format!("{}: {}", what, key))?;
            u32::try_from(d.as_millis()).map(Some).map_err(|_| format!("{}: '{}' is too long", what, key))
        }
    }
}

fn opts_map(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match opt_arg(args, i) {
        None => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

/// `connect(url, { timeout, requestTimeout, clientId, autoReconnect }?)`
fn connect(args: &[Value]) -> Result<Value, String> {
    let what = "foguMQ-client.connect";
    max_args(args, 2, what)?;
    let mut opts = parse_fogumq_url(&str_arg(args, 0, what)?).map_err(|e| format!("{}: {}", what, e))?;
    let m = opts_map(args, 1, what)?;
    if let Some(v) = m.get("timeout") {
        opts.connect_timeout = duration_arg(v, &format!("{}: timeout", what))?;
    }
    if let Some(v) = m.get("requestTimeout") {
        opts.request_timeout = duration_arg(v, &format!("{}: requestTimeout", what))?;
    }
    match m.get("clientId") {
        None => {}
        Some(Value::Str(s)) => opts.client_id = s.to_string(),
        Some(_) => return Err(format!("{}: 'clientId' must be a str", what)),
    }
    match m.get("autoReconnect") {
        None => {}
        Some(Value::Bool(b)) => opts.auto_reconnect = *b,
        Some(_) => return Err(format!("{}: 'autoReconnect' must be a bool", what)),
    }
    let client = Client::new(opts);
    let c = client.clone();
    rt::block_on(async move { c.connect().await })?.map_err(op_err)?;
    Ok(NativeObjectRef::new(MqClient { client }).to_value())
}

// ---------------------------------------------------------------------
// Client object
// ---------------------------------------------------------------------

struct MqClient {
    client: Client,
}

const CLIENT_METHODS: &[&str] = &[
    "channelOpen", "channelClose", "subscribe", "unsubscribe", "publish", "publishSync", "serve", "unserve",
    "request", "registerWorker", "workerOff", "enqueue", "apikeyCreate", "apikeyRevoke", "apikeyGet",
    "apikeyList", "close",
];

impl MqClient {
    fn start(&self, method: &str, args: &[Value]) -> Result<Poller, String> {
        let c = self.client.clone();
        Ok(match method {
            "channelOpen" | "channelClose" => {
                max_args(args, 1, method)?;
                let ch = channel_arg(args, method)?;
                if method == "channelOpen" {
                    async_call(async move { c.channel_open(ch).await }, |_| Value::Nil)
                } else {
                    async_call(async move { c.channel_close(ch).await }, |_| Value::Nil)
                }
            }
            "subscribe" => {
                max_args(args, 3, method)?;
                let ch = channel_arg(args, method)?;
                let pattern = str_arg(args, 1, method)?;
                let group = opt_str(args, 2, method)?;
                let inbox = Inbox::new(InboxKind::Subscription, ch, pattern.clone(), group.clone(), self.client.clone());
                let q = Arc::clone(&inbox.queue);
                let cb = move |topic, group, data| push(&q, Item::Msg { topic, group, data });
                async_call(async move { c.subscribe(ch, &pattern, &group, cb).await }, move |_| {
                    NativeObjectRef::new(inbox).to_value()
                })
            }
            "unsubscribe" => {
                max_args(args, 3, method)?;
                let ch = channel_arg(args, method)?;
                let pattern = str_arg(args, 1, method)?;
                let group = opt_str(args, 2, method)?;
                async_call(async move { c.unsubscribe(ch, &pattern, &group).await }, |_| Value::Nil)
            }
            "publish" | "publishSync" => {
                max_args(args, 3, method)?;
                let ch = channel_arg(args, method)?;
                let topic = str_arg(args, 1, method)?;
                let data = payload(args.get(2).unwrap_or(&Value::Nil));
                if method == "publish" {
                    async_call(async move { c.publish(ch, &topic, data).await }, |_| Value::Nil)
                } else {
                    async_call(async move { c.publish_sync(ch, &topic, data).await }, |_| Value::Nil)
                }
            }
            "serve" => {
                max_args(args, 3, method)?;
                let ch = channel_arg(args, method)?;
                let service = str_arg(args, 1, method)?;
                let topic = str_arg(args, 2, method)?;
                let inbox = Inbox::new(InboxKind::Service, ch, service.clone(), topic.clone(), self.client.clone());
                let q = Arc::clone(&inbox.queue);
                let handler = move |topic: String, data: Bytes| {
                    let (tx, rx) = oneshot::channel();
                    push(&q, Item::Call { topic, data, tx });
                    async move { rx.await.unwrap_or_else(|_| Err("the request was not answered".into())) }
                };
                async_call(async move { c.serve(ch, &service, &topic, handler).await }, move |_| {
                    NativeObjectRef::new(inbox).to_value()
                })
            }
            "unserve" => {
                max_args(args, 3, method)?;
                let ch = channel_arg(args, method)?;
                let service = str_arg(args, 1, method)?;
                let topic = str_arg(args, 2, method)?;
                async_call(async move { c.unserve(ch, &service, &topic).await }, |_| Value::Nil)
            }
            "request" => {
                max_args(args, 4, method)?;
                let ch = channel_arg(args, method)?;
                let service = str_arg(args, 1, method)?;
                let topic = str_arg(args, 2, method)?;
                let data = payload(args.get(3).unwrap_or(&Value::Nil));
                async_call(async move { c.request(ch, &service, &topic, data).await }, |b| text(&b))
            }
            "registerWorker" => {
                max_args(args, 3, method)?;
                let ch = channel_arg(args, method)?;
                let queue = str_arg(args, 1, method)?;
                let m = opts_map(args, 2, method)?;
                let filter = match m.get("filter") {
                    None | Some(Value::Nil) => ">".to_string(),
                    Some(Value::Str(s)) => s.to_string(),
                    Some(_) => return Err(format!("{}: 'filter' must be a str", method)),
                };
                let prefetch = opt_int(&m, "prefetch", u16::MAX as i64, method)?.unwrap_or(1) as u16;
                let visibility = opt_ms(&m, "visibility", method)?.unwrap_or(30_000);
                let inbox = Inbox::new(InboxKind::Worker, ch, queue.clone(), filter.clone(), self.client.clone());
                let q = Arc::clone(&inbox.queue);
                let handler = move |job: Job| {
                    let (tx, rx) = oneshot::channel();
                    push(&q, Item::Job { job, tx });
                    // a job dropped without an answer goes back to the queue
                    async move {
                        rx.await.unwrap_or(JobAction::Nack { requeue: true, delay_ms: 0, reason: "not handled".into() })
                    }
                };
                async_call(
                    async move { c.register_worker(ch, &queue, &filter, prefetch, visibility, handler).await },
                    move |_| NativeObjectRef::new(inbox).to_value(),
                )
            }
            "workerOff" => {
                max_args(args, 2, method)?;
                let ch = channel_arg(args, method)?;
                let queue = str_arg(args, 1, method)?;
                async_call(async move { c.worker_off(ch, &queue).await }, |_| Value::Nil)
            }
            "enqueue" => {
                max_args(args, 5, method)?;
                let ch = channel_arg(args, method)?;
                let queue = str_arg(args, 1, method)?;
                let job_type = str_arg(args, 2, method)?;
                let data = payload(args.get(3).unwrap_or(&Value::Nil));
                let m = opts_map(args, 4, method)?;
                let job_id = match m.get("jobId") {
                    None | Some(Value::Nil) => String::new(),
                    Some(Value::Str(s)) => s.to_string(),
                    Some(_) => return Err(format!("{}: 'jobId' must be a str", method)),
                };
                let o = EnqueueOptions {
                    job_id,
                    priority: opt_int(&m, "priority", 255, method)?.unwrap_or(0) as u8,
                    delay_ms: opt_ms(&m, "delay", method)?.unwrap_or(0),
                    retries: opt_int(&m, "retries", 255, method)?.unwrap_or(0) as u8,
                    ttl_ms: opt_ms(&m, "ttl", method)?.unwrap_or(0),
                };
                async_call(async move { c.enqueue(ch, &queue, &job_type, data, o).await }, |r| {
                    map(vec![("jobId", Value::Str(r.job_id.into())), ("seq", Value::Int(r.seq as i64))])
                })
            }
            "apikeyCreate" => {
                max_args(args, 4, method)?;
                let ch = channel_arg(args, method)?;
                let name = str_arg(args, 1, method)?;
                let perms = match args.get(2) {
                    None | Some(Value::Nil) => serde_json::Value::Object(Default::default()),
                    Some(v) => serde_json::from_str(&stringify_value(v)).unwrap_or(serde_json::Value::Null),
                };
                let labels = match opt_arg(args, 3) {
                    None => Vec::new(),
                    Some(Value::List(l)) => l
                        .borrow()
                        .iter()
                        .map(|x| match x {
                            Value::Str(s) => Ok(s.to_string()),
                            _ => Err(format!("{}: labels must be strs", method)),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(other) => return Err(format!("{}: labels must be a list, got '{}'", method, other.type_name())),
                };
                let ns = self.client.snapshot(|g| g.namespace.clone());
                async_call(async move { c.api_key_create(ch, &name, perms, &labels, &ns).await }, |r| {
                    map(vec![
                        ("id", Value::Str(r.id.into())),
                        ("token", Value::Str(r.token.into())),
                        ("permissions", Value::Str(r.permissions.into())),
                    ])
                })
            }
            "apikeyRevoke" | "apikeyGet" => {
                max_args(args, 2, method)?;
                let ch = channel_arg(args, method)?;
                let id = str_arg(args, 1, method)?;
                if method == "apikeyRevoke" {
                    async_call(async move { c.api_key_revoke(ch, &id).await }, |_| Value::Nil)
                } else {
                    async_call(async move { c.api_key_get(ch, &id).await }, Value::str)
                }
            }
            "apikeyList" => {
                max_args(args, 1, method)?;
                let ch = channel_arg(args, method)?;
                async_call(async move { c.api_key_list(ch).await }, Value::str)
            }
            "close" => {
                max_args(args, 0, method)?;
                async_call(async move { c.close().await }, |_| Value::Nil)
            }
            _ => unreachable!(),
        })
    }
}

impl FoguObject for MqClient {
    fn name(&self) -> &str {
        "FoguMQClient"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let o = &self.client.opts;
        Some(match field {
            "connected" => Value::Bool(self.client.snapshot(|g| g.connected)),
            "namespace" => Value::Str(self.client.snapshot(|g| g.namespace.clone()).into()),
            "authMode" => Value::Str(self.client.snapshot(|g| g.auth_mode.clone()).into()),
            "keyId" => Value::Str(self.client.snapshot(|g| g.key_id.clone()).into()),
            "sessionId" => Value::Str(self.client.snapshot(|g| g.session_id.clone()).into()),
            "url" => Value::Str(format!("fogumq://{}:{}", o.host, o.port).into()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!(
            "method '{}' does not exist on FoguMQClient ({}; fields connected namespace authMode keyId sessionId url)",
            method,
            CLIENT_METHODS.join(" ")
        ))
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if !CLIENT_METHODS.contains(&method) {
            return None;
        }
        Some(self.start(method, args))
    }

    fn methods(&self) -> Vec<&'static str> {
        CLIENT_METHODS.to_vec()
    }
}

// ---------------------------------------------------------------------
// Inboxes: Subscription, Service, Worker
// ---------------------------------------------------------------------

enum Item {
    Msg { topic: String, group: String, data: Bytes },
    Call { topic: String, data: Bytes, tx: oneshot::Sender<Result<Bytes, String>> },
    Job { job: Job, tx: oneshot::Sender<JobAction> },
}

#[derive(Default)]
struct Queue {
    items: VecDeque<Item>,
    closed: bool,
}

type SharedQueue = Arc<std::sync::Mutex<Queue>>;

fn lock(q: &SharedQueue) -> std::sync::MutexGuard<'_, Queue> {
    q.lock().unwrap_or_else(|p| p.into_inner())
}

/// Called on a runtime thread: queues the item and wakes the interpreter.
fn push(q: &SharedQueue, item: Item) {
    {
        let mut g = lock(q);
        if g.closed {
            return; // dropping a Call/Job answers it (error / requeue)
        }
        g.items.push_back(item);
    }
    sched::wake();
}

#[derive(Clone, Copy, PartialEq)]
enum InboxKind {
    Subscription,
    Service,
    Worker,
}

struct Inbox {
    kind: InboxKind,
    channel: u16,
    /// pattern / service / queue
    a: String,
    /// group / topic / filter
    b: String,
    queue: SharedQueue,
    client: Client,
}

impl Inbox {
    fn new(kind: InboxKind, channel: u16, a: String, b: String, client: Client) -> Self {
        Inbox { kind, channel, a, b, queue: Arc::default(), client }
    }

    fn type_name(&self) -> &'static str {
        match self.kind {
            InboxKind::Subscription => "Subscription",
            InboxKind::Service => "Service",
            InboxKind::Worker => "Worker",
        }
    }

    fn wrap(&self, item: Item) -> Value {
        match item {
            Item::Msg { topic, group, data } => {
                map(vec![("topic", Value::Str(topic.into())), ("group", Value::Str(group.into())), ("data", text(&data))])
            }
            Item::Call { topic, data, tx } => {
                NativeObjectRef::new(Request { service: self.a.clone(), topic, data, tx: Some(tx) }).to_value()
            }
            Item::Job { job, tx } => NativeObjectRef::new(JobObj { job, tx: Some(tx) }).to_value(),
        }
    }

    /// Why nothing more will arrive, if so.
    fn ended(queue: &SharedQueue, client: &Client, name: &str, key: &str) -> Option<String> {
        if lock(queue).closed {
            return Some(io_err!("{}: {} '{}' was closed", MOD, name, key));
        }
        if !client.opts.auto_reconnect && !client.snapshot(|g| g.connected) {
            return Some(op_err(ClientError::Disconnected));
        }
        None
    }
}

impl FoguObject for Inbox {
    fn name(&self) -> &str {
        self.type_name()
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match (self.kind, field) {
            (_, "channel") => Value::Int(self.channel as i64),
            (_, "pending") => Value::Int(lock(&self.queue).items.len() as i64),
            (InboxKind::Subscription, "pattern") | (InboxKind::Service, "service") | (InboxKind::Worker, "queue") => {
                Value::Str(self.a.clone().into())
            }
            (InboxKind::Subscription, "group") | (InboxKind::Service, "topic") | (InboxKind::Worker, "filter") => {
                Value::Str(self.b.clone().into())
            }
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // non-blocking: the next item or nil
            "tryNext" => {
                max_args(args, 0, method)?;
                let item = lock(&self.queue).items.pop_front();
                match item {
                    Some(i) => Ok(self.wrap(i)),
                    None => Self::ended(&self.queue, &self.client, self.type_name(), &self.a).map_or(Ok(Value::Nil), Err),
                }
            }
            other => Err(format!("method '{}' does not exist on {} (next tryNext close)", other, self.type_name())),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        match method {
            // `next(timeout?)`: waits for the next item; nil after the timeout
            "next" => {
                let deadline = match (max_args(args, 1, "next"), opt_arg(args, 0)) {
                    (Err(e), _) => return Some(Err(e)),
                    (_, None) => None,
                    (_, Some(v)) => match duration_arg(v, "next") {
                        Ok(d) => Some(Instant::now() + d),
                        Err(e) => return Some(Err(e)),
                    },
                };
                let me = Inbox {
                    kind: self.kind,
                    channel: self.channel,
                    a: self.a.clone(),
                    b: self.b.clone(),
                    queue: Arc::clone(&self.queue),
                    client: self.client.clone(),
                };
                Some(Ok(Box::new(move || {
                    let item = lock(&me.queue).items.pop_front();
                    if let Some(i) = item {
                        return Some(Ok(me.wrap(i)));
                    }
                    if let Some(e) = Self::ended(&me.queue, &me.client, me.type_name(), &me.a) {
                        return Some(Err(e));
                    }
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Some(Ok(Value::Nil));
                    }
                    poll_again();
                    None
                })))
            }
            // `close()`: unsubscribe / unserve / workerOff, and drop what is queued
            "close" => {
                if let Err(e) = max_args(args, 0, "close") {
                    return Some(Err(e));
                }
                {
                    let mut g = lock(&self.queue);
                    g.closed = true;
                    g.items.clear();
                }
                let (c, ch, a, b) = (self.client.clone(), self.channel, self.a.clone(), self.b.clone());
                Some(Ok(match self.kind {
                    InboxKind::Subscription => async_call(async move { c.unsubscribe(ch, &a, &b).await }, |_| Value::Nil),
                    InboxKind::Service => async_call(async move { c.unserve(ch, &a, &b).await }, |_| Value::Nil),
                    InboxKind::Worker => async_call(async move { c.worker_off(ch, &a).await }, |_| Value::Nil),
                }))
            }
            _ => None,
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "close"]
    }
}

/// An RPC request received by `serve`: answer it with `reply` or `fail`.
struct Request {
    service: String,
    topic: String,
    data: Bytes,
    tx: Option<oneshot::Sender<Result<Bytes, String>>>,
}

impl FoguObject for Request {
    fn name(&self) -> &str {
        "Request"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "service" => Value::Str(self.service.clone().into()),
            "topic" => Value::Str(self.topic.clone().into()),
            "data" => text(&self.data),
            "answered" => Value::Bool(self.tx.is_none()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let answer = match method {
            "reply" => {
                max_args(args, 1, method)?;
                Ok(payload(args.first().unwrap_or(&Value::Nil)))
            }
            "fail" => {
                max_args(args, 1, method)?;
                Err(match args.first() {
                    Some(Value::Error(e)) => e.message.clone(),
                    Some(v) => v.to_display_string(),
                    None => "handler error".into(),
                })
            }
            other => return Err(format!("method '{}' does not exist on Request (reply fail)", other)),
        };
        let tx = self.tx.take().ok_or_else(|| format!("{}: this request was already answered", method))?;
        let _ = tx.send(answer);
        Ok(Value::Nil)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["reply", "fail"]
    }
}

/// A job received by a worker: finish it with `ack`, `nack` or `extend`.
struct JobObj {
    job: Job,
    tx: Option<oneshot::Sender<JobAction>>,
}

impl FoguObject for JobObj {
    fn name(&self) -> &str {
        "Job"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let j = &self.job;
        Some(match field {
            "id" => Value::Str(j.id.clone().into()),
            "attempt" => Value::Int(j.attempt as i64),
            "deadline" => Value::Int(j.deadline_ms as i64),
            "queue" => Value::Str(j.queue.clone().into()),
            "type" => Value::Str(j.topic.clone().into()),
            "data" => text(&j.data),
            "channel" => Value::Int(j.channel as i64),
            "done" => Value::Bool(self.tx.is_none()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let action = match method {
            "ack" => {
                max_args(args, 0, method)?;
                JobAction::Ack
            }
            // nack({ requeue, delay, reason }?)
            "nack" => {
                max_args(args, 1, method)?;
                let m = opts_map(args, 0, method)?;
                let requeue = match m.get("requeue") {
                    None | Some(Value::Nil) => true,
                    Some(Value::Bool(b)) => *b,
                    Some(_) => return Err(format!("{}: 'requeue' must be a bool", method)),
                };
                let reason = match m.get("reason") {
                    None | Some(Value::Nil) => String::new(),
                    Some(Value::Str(s)) => s.to_string(),
                    Some(_) => return Err(format!("{}: 'reason' must be a str", method)),
                };
                JobAction::Nack { requeue, delay_ms: opt_ms(&m, "delay", method)?.unwrap_or(0), reason }
            }
            "extend" => {
                max_args(args, 1, method)?;
                let d = duration_arg(args.first().unwrap_or(&Value::Nil), method)?;
                let extend_ms = u32::try_from(d.as_millis()).map_err(|_| format!("{}: too long", method))?;
                JobAction::Extend { extend_ms }
            }
            other => return Err(format!("method '{}' does not exist on Job (ack nack extend)", other)),
        };
        let tx = self.tx.take().ok_or_else(|| format!("{}: this job was already finished", method))?;
        let _ = tx.send(action);
        Ok(Value::Nil)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["ack", "nack", "extend"]
    }
}

// ===========================================================================
// 10. Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let f = Frame {
            opcode: OP_PUBLISH,
            channel: 1,
            request_id: 7,
            topic: "a.b".into(),
            service: "svc".into(),
            error_code: 0,
            error_message: String::new(),
            payload: Bytes::from_static(b"hi"),
        };
        let enc = f.encode();
        let mut fr = Framer::new(1 << 20);
        // split in two chunks: the framer waits for the rest
        assert!(fr.feed(&enc[..5]).unwrap().is_empty());
        let out = fr.feed(&enc[5..]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].opcode, out[0].channel, out[0].request_id), (OP_PUBLISH, 1, 7));
        assert_eq!((out[0].topic.as_str(), out[0].service.as_str()), ("a.b", "svc"));
        assert_eq!(&out[0].payload[..], b"hi");
    }

    #[test]
    fn urls() {
        let o = parse_fogumq_url("fogumq://tok@10.0.0.1:7300?namespace=acme&requestTimeout=250").unwrap();
        assert_eq!((o.auth_token.as_str(), o.host.as_str(), o.port, o.namespace.as_str()), ("tok", "10.0.0.1", 7300, "acme"));
        assert_eq!(o.request_timeout, Duration::from_millis(250));
        let o = parse_fogumq_url("fogumq://[::1]:9000").unwrap();
        assert_eq!((o.host.as_str(), o.port), ("::1", 9000));
        assert!(parse_fogumq_url("http://x").is_err());
    }

    #[test]
    fn topics() {
        assert!(match_topic("orders.>", "orders.created"));
        assert!(!match_topic("orders.>", "orders"));
        assert!(!match_topic("orders.>", "billing"));
        assert!(match_topic("orders.*.shipped", "orders.a.shipped"));
        assert!(!match_topic("orders.*", "orders.a.b"));
    }

    /// A minimal v6 broker: HELLO_OK, ACKs, and echoes a PUBLISH to the
    /// subscriber on the same channel.
    #[test]
    fn talks_to_a_broker() {
        let rt = rt::tokio();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        rt.spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut framer = Framer::new(1 << 20);
            let mut buf = vec![0u8; 4096];
            loop {
                let n = match s.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                for f in framer.feed(&buf[..n]).unwrap() {
                    let mut out = Vec::new();
                    match f.opcode {
                        OP_HELLO => out.push(Frame {
                            opcode: OP_HELLO_OK,
                            request_id: f.request_id,
                            payload: Writer::new().str("s1").u32(1 << 20).u16(30).u32(5000).u16(16).str("master").str("k1").str("acme").build(),
                            ..Default::default()
                        }),
                        OP_PUBLISH => {
                            out.push(Frame { opcode: OP_MESSAGE, channel: f.channel, topic: f.topic.clone(), payload: f.payload.clone(), ..Default::default() });
                            out.push(Frame { opcode: OP_ACK, channel: f.channel, request_id: f.request_id, ..Default::default() });
                        }
                        OP_CLOSE => return,
                        _ => out.push(Frame { opcode: OP_ACK, channel: f.channel, request_id: f.request_id, ..Default::default() }),
                    }
                    for o in out {
                        s.write_all(&o.encode()).await.unwrap();
                    }
                }
            }
        });
        let c = Client::new(parse_fogumq_url(&format!("fogumq://tok@127.0.0.1:{}", port)).unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let c2 = c.clone();
        rt.block_on(async move {
            c2.connect().await.unwrap();
            c2.channel_open(1).await.unwrap();
            assert!(matches!(c2.channel_open(1).await, Err(ClientError::ChannelAlreadyOpen(1))));
            c2.subscribe(1, "orders.>", "", move |t, _, p| tx.send((t, p)).unwrap()).await.unwrap();
            c2.publish_sync(1, "orders.created", "42").await.unwrap();
        });
        let (topic, data) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!((topic.as_str(), &data[..]), ("orders.created", &b"42"[..]));
        assert_eq!(c.snapshot(|g| (g.auth_mode.clone(), g.namespace.clone())), ("master".into(), "acme".into()));
        rt.block_on(c.close()).unwrap();
        assert!(!c.snapshot(|g| g.connected));
    }
}
