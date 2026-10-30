use crate::error::ClientError;
use crate::frame::{Framer, Frame, Reader, Writer};
use crate::opcodes::*;
use crate::url::parse_fogumq_url;
use bytes::Bytes;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex, Notify};
use tokio::time::timeout;

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

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
    /// Namespace informativo (usado em `api_key_create`). Vazio = default.
    pub namespace: String,
    pub debug: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 7222,
            client_id: "rust-client".into(),
            auth_token: String::new(),
            max_frame: 1 << 20,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(5),
            max_channels: 256,
            auto_reconnect: false,
            reconnect_backoff: Duration::from_secs(1),
            namespace: String::new(),
            debug: std::env::var("FLOWMQ_DEBUG").as_deref() == Ok("1"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tipos públicos de callbacks
// ---------------------------------------------------------------------------

pub type MessageHandler = Arc<dyn Fn(String, String, Bytes) + Send + Sync + 'static>;
pub type HandlerFuture =
    Pin<Box<dyn Future<Output = Result<Bytes, String>> + Send + 'static>>;
pub type JobHandler = Arc<dyn Fn(Job) -> JobFuture + Send + Sync + 'static>;
pub type JobFuture = Pin<Box<dyn Future<Output = JobAction> + Send + 'static>>;

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

#[derive(Debug, Clone)]
pub struct BrokerError {
    pub code: u16,
    pub message: String,
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "broker error {}: {}", self.code, self.message)
    }
}

// ---------------------------------------------------------------------------
// Estado interno
// ---------------------------------------------------------------------------

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
    #[allow(clippy::type_complexity)]
    func: Arc<dyn Fn(String, Bytes) -> HandlerFuture + Send + Sync + 'static>,
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

struct Inner {
    conn: Option<tokio::net::tcp::OwnedWriteHalf>,
    connected: bool,
    closing: bool,
    hello_wait: Option<oneshot::Sender<Result<(), ClientError>>>,

    // Negociado
    session_id: String,
    max_frame: usize,
    heartbeat: Duration,
    request_timeout: Duration,
    max_channels: u16,
    auth_mode: String,
    key_id: String,
    namespace: String,

    // Estado por canal
    channels: HashMap<u16, ()>,
    next_request_id: HashMap<u16, u32>,
    pending: HashMap<(u16, u32), Pending>,
    handlers: HashMap<String, HandlerEntry>,
    subscriptions: HashMap<u16, Vec<Subscription>>,
    workers: HashMap<u16, WorkerEntry>,

    // Desired state (para reconexão)
    desired_channels: HashMap<u16, ()>,
    desired_subscriptions: HashMap<u16, Vec<Subscription>>,
    desired_handlers: HashMap<u16, Vec<HandlerEntry>>,
    desired_workers: HashMap<u16, Vec<WorkerEntry>>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            conn: None,
            connected: false,
            closing: false,
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

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Client {
    opts: Options,
    inner: Arc<Mutex<Inner>>,
    closed: Arc<Notify>,
    write_gate: Arc<Mutex<()>>,
    /// Sinal de reconexão. `handle_disconnect` só envia aqui; quem
    /// efetivamente reconecta é o supervisor, fora do future de `connect`.
    reconnect_tx: mpsc::UnboundedSender<()>,
}

impl Client {
    pub fn new(opts: Options) -> Self {
        let (reconnect_tx, reconnect_rx) = mpsc::unbounded_channel::<()>();

        let client = Self {
            opts: opts.clone(),
            inner: Arc::new(Mutex::new(Inner::default())),
            closed: Arc::new(Notify::new()),
            write_gate: Arc::new(Mutex::new(())),
            reconnect_tx,
        };

        // Supervisor de reconexão: fica fora do future de `connect`, então
        // não fecha ciclo de inferência de tipos.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let me = client.clone();
            let mut rx = reconnect_rx;
            handle.spawn(async move {
                while rx.recv().await.is_some() {
                    {
                        let g = me.inner.lock().await;
                        if g.closing {
                            continue;
                        }
                    }
                    tokio::time::sleep(me.opts.reconnect_backoff).await;
                    if me.connect().await.is_ok() {
                        me.reapply_desired().await;
                    }
                }
            });
        }

        client
    }

    pub fn from_url(url: &str) -> Result<Self, ClientError> {
        let parsed = parse_fogumq_url(url).map_err(ClientError::Url)?;
        Ok(Self::new(parsed.options))
    }

    pub async fn connect_url(url: &str) -> Result<Self, ClientError> {
        let c = Self::from_url(url)?;
        c.connect().await?;
        Ok(c)
    }

    // -----------------------------------------------------------------------
    // Conexão
    // -----------------------------------------------------------------------

    pub async fn connect(&self) -> Result<(), ClientError> {
        {
            let g = self.inner.lock().await;
            if g.connected {
                return Ok(());
            }
        }
        let addr = format!("{}:{}", self.opts.host, self.opts.port);
        let stream = timeout(self.opts.connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| ClientError::Timeout(self.opts.connect_timeout))?
            .map_err(ClientError::Io)?;
        stream.set_nodelay(true).ok();

        let (mut rd, wr) = stream.into_split();
        let (hello_tx, hello_rx) = oneshot::channel();

        {
            let mut g = self.inner.lock().await;
            g.conn = Some(wr);
            g.connected = true;
            g.closing = false;
            g.hello_wait = Some(hello_tx);
            let rid = Self::bump_request_id(&mut g, 0);
            drop(g);
            let payload = Writer::new()
                .u8(PROTOCOL_VERSION)
                .str(&self.opts.client_id)
                .u32(self.opts.max_frame as u32)
                .str(&self.opts.auth_token)
                .build();
            let f = Frame {
                opcode: OP_HELLO,
                channel: 0,
                request_id: rid,
                payload,
                ..Default::default()
            };
            self.write_frame(&f).await?;
        }

        let me = self.clone();
        tokio::spawn(async move { me.read_loop(rd).await });

        match timeout(self.opts.connect_timeout, hello_rx).await {
            Err(_) => {
                self.force_close().await;
                Err(ClientError::Timeout(self.opts.connect_timeout))
            }
            Ok(Err(_)) => {
                self.force_close().await;
                Err(ClientError::Disconnected)
            }
            Ok(Ok(Ok(()))) => {
                self.start_ping_loop();
                Ok(())
            }
            Ok(Ok(Err(e))) => {
                self.force_close().await;
                Err(e)
            }
        }
    }

    pub async fn close(&self, _reason: &str) -> Result<(), ClientError> {
        {
            let mut g = self.inner.lock().await;
            if g.closing {
                return Ok(());
            }
            g.closing = true;
            g.connected = false;
            if let Some(mut wr) = g.conn.take() {
                let f = Frame { opcode: OP_CLOSE, channel: 0, ..Default::default() };
                let _ = wr.write_all(&f.encode()).await;
                let _ = wr.shutdown().await;
            }
        }
        self.closed.notify_waiters();
        Ok(())
    }

    async fn force_close(&self) {
        let mut g = self.inner.lock().await;
        if let Some(mut wr) = g.conn.take() {
            let _ = wr.shutdown().await;
        }
        g.connected = false;
    }

    fn bump_request_id(g: &mut Inner, ch: u16) -> u32 {
        let cur = g.next_request_id.entry(ch).or_insert(0);
        *cur = cur.wrapping_add(1);
        if *cur == 0 {
            *cur = 1;
        }
        *cur
    }

    async fn write_frame(&self, f: &Frame) -> Result<(), ClientError> {
        let _guard = self.write_gate.lock().await;
        let mut g = self.inner.lock().await;
        let wr = g.conn.as_mut().ok_or(ClientError::Disconnected)?;
        wr.write_all(&f.encode()).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // readLoop
    // -----------------------------------------------------------------------

    async fn read_loop(&self, mut rd: tokio::net::tcp::OwnedReadHalf) {
        let mut framer = Framer::new(self.opts.max_frame);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match rd.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            let frames = match framer.feed(&buf[..n]) {
                Ok(f) => f,
                Err(e) => {
                    self.emit_error(e.into()).await;
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
        let mut g = self.inner.lock().await;
        let was = g.connected;
        let closing = g.closing;
        let auto = self.opts.auto_reconnect;
        g.connected = false;

        if let Some(tx) = g.hello_wait.take() {
            let _ = tx.send(Err(ClientError::Disconnected));
        }

        let pend = std::mem::take(&mut g.pending);
        drop(g);
        for (_, p) in pend {
            let _ = p.tx.send(Err(ClientError::Disconnected));
        }

        if was && !closing && auto {
            // Apenas sinaliza o supervisor. Não chama `connect` daqui —
            // isso quebraria o ciclo de inferência de tipos.
            let _ = self.reconnect_tx.send(());
        }
    }

    // -----------------------------------------------------------------------
    // Desired state
    // -----------------------------------------------------------------------

    async fn reapply_desired(&self) {
        let (channels, subs, handlers, workers) = {
            let g = self.inner.lock().await;
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
            (chs, subs, hs, ws)
        };

        for ch in channels {
            if let Err(e) = self.channel_open(ch).await {
                self.emit_error(e).await;
            }
        }
        for (ch, s) in subs {
            let cb = s.cb.clone();
            let p = s.pattern.clone();
            let gr = s.group.clone();
            if let Err(e) = self
                .subscribe(ch, &p, &gr, move |t, g, payload| cb(t, g, payload))
                .await
            {
                self.emit_error(e).await;
            }
        }
        for (ch, h) in handlers {
            let f = h.func.clone();
            let svc = h.service.clone();
            let tp = h.topic.clone();
            if let Err(e) = self
                .serve(ch, &svc, &tp, move |topic, payload| f(topic, payload))
                .await
            {
                self.emit_error(e).await;
            }
        }
        for (ch, w) in workers {
            let f = w.handler.clone();
            let q = w.queue.clone();
            let filt = w.filter.clone();
            if let Err(e) = self
                .register_worker(ch, &q, &filt, w.prefetch, w.visibility_ms, move |job| f(job))
                .await
            {
                self.emit_error(e).await;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Dispatch
    // -----------------------------------------------------------------------

    async fn dispatch(&self, f: Frame) {
        match f.opcode {
            OP_HELLO_OK => self.handle_hello_ok(f).await,
            OP_PING => {
                let pong = Frame { opcode: OP_PONG, ..Default::default() };
                let _ = self.write_frame(&pong).await;
            }
            OP_PONG => {}
            OP_CLOSE => {
                let mut g = self.inner.lock().await;
                g.closing = true;
                if let Some(mut wr) = g.conn.take() {
                    let _ = wr.shutdown().await;
                }
            }
            OP_ERROR => self.handle_error(f).await,
            OP_CHANNEL_CLOSE => self.handle_channel_close(f).await,
            OP_ACK => {
                self.resolve(f.channel, f.request_id, Response {
                    code: 0, message: String::new(), payload: Bytes::new(),
                }).await
            }
            OP_MESSAGE => self.handle_message(f).await,
            OP_REQUEST => self.handle_request(f).await,
            OP_RESPONSE => {
                self.resolve(f.channel, f.request_id, Response {
                    code: f.error_code,
                    message: f.error_message.clone(),
                    payload: f.payload.clone(),
                }).await
            }
            OP_ENQUEUED => {
                self.resolve(f.channel, f.request_id, Response {
                    code: 0, message: String::new(), payload: f.payload.clone(),
                }).await
            }
            OP_JOB => self.handle_job(f).await,
            OP_APIKEY_CREATE | OP_APIKEY_GET | OP_APIKEY_LIST => {
                self.resolve(f.channel, f.request_id, Response {
                    code: 0, message: String::new(), payload: f.payload.clone(),
                }).await
            }
            _ => {}
        }
    }

    async fn handle_hello_ok(&self, f: Frame) {
        if self.opts.debug {
            eprintln!("[flowmq] HELLO_OK payload ({} bytes): {:02x?}",
                      f.payload.len(), &f.payload[..]);
        }
        let mut r = Reader::new(f.payload.clone());
        let session = r.str().unwrap_or_default();
        let max_frame = r.u32().unwrap_or(1 << 20) as usize;
        let hb = r.u16().unwrap_or(30) as u64;
        let rt = r.u32().unwrap_or(5000) as u64;
        let mc = r.u16().unwrap_or(256);

        let mut mode = String::new();
        let mut key_id = String::new();
        let mut ns = String::new();
        let had_auth = r.remaining() > 0;
        if had_auth {
            mode = r.str().unwrap_or_default();
            if r.remaining() > 0 {
                key_id = r.str().unwrap_or_default();
            }
            if r.remaining() > 0 {
                ns = r.str().unwrap_or_default();
            }
        }

        let mut hello_tx = None;
        {
            let mut g = self.inner.lock().await;
            g.session_id = session;
            g.max_frame = max_frame;
            g.heartbeat = Duration::from_secs(hb.max(1));
            g.request_timeout = Duration::from_millis(rt.max(1));
            g.max_channels = mc;
            g.auth_mode = if mode.is_empty() { "unknown".into() } else { mode };
            g.key_id = key_id;
            g.namespace = ns;
            hello_tx = g.hello_wait.take();
        }

        if !had_auth {
            self.emit_error(ClientError::Other(
                "HELLO_OK sem auth_mode/key_id — provavelmente broker v3".into(),
            )).await;
        }

        if let Some(tx) = hello_tx {
            let _ = tx.send(Ok(()));
        }
    }

    async fn handle_error(&self, f: Frame) {
        if f.request_id != 0 {
            self.resolve(f.channel, f.request_id, Response {
                code: f.error_code,
                message: f.error_message.clone(),
                payload: Bytes::new(),
            }).await;
        }
        if f.channel == 0 && f.request_id != 0 {
            let mut g = self.inner.lock().await;
            if let Some(tx) = g.hello_wait.take() {
                let _ = tx.send(Err(ClientError::Broker {
                    code: f.error_code,
                    message: f.error_message.clone(),
                }));
            }
        }
        self.emit_error(ClientError::Broker {
            code: f.error_code,
            message: f.error_message.clone(),
        }).await;
    }

    async fn handle_channel_close(&self, f: Frame) {
        let mut g = self.inner.lock().await;
        g.channels.remove(&f.channel);
        g.subscriptions.remove(&f.channel);
        g.workers.remove(&f.channel);
        let keys: Vec<_> = g.pending.keys()
            .filter(|(ch, _)| *ch == f.channel)
            .cloned()
            .collect();
        let pend: Vec<_> = keys.iter().filter_map(|k| g.pending.remove(k)).collect();
        drop(g);
        for p in pend {
            let _ = p.tx.send(Err(ClientError::ChannelNotOpen(f.channel)));
        }
    }

    async fn handle_message(&self, f: Frame) {
        let subs = {
            let g = self.inner.lock().await;
            g.subscriptions.get(&f.channel).cloned().unwrap_or_default()
        };
        for s in subs {
            if match_topic(&s.pattern, &f.topic) {
                let cb = s.cb.clone();
                let topic = f.topic.clone();
                let group = f.service.clone();
                let payload = f.payload.clone();
                tokio::spawn(async move { cb(topic, group, payload) });
            }
        }
    }

    async fn handle_request(&self, f: Frame) {
        let key = format!("{}|{}", f.service, f.topic);
        let entry = {
            let g = self.inner.lock().await;
            let mut found = g.handlers.get(&key).cloned();
            if found.is_none() {
                for (k, v) in g.handlers.iter() {
                    let (svc, pat) = split_handler_key(k);
                    if svc == f.service && match_topic(pat, &f.topic) {
                        found = Some(v.clone());
                        break;
                    }
                }
            }
            found
        };
        let Some(entry) = entry else {
            let resp = Frame {
                opcode: OP_RESPONSE,
                channel: f.channel,
                request_id: f.request_id,
                error_code: ERR_NO_HANDLER,
                error_message: "sem handler".into(),
                ..Default::default()
            };
            let _ = self.write_frame(&resp).await;
            return;
        };
        let topic = f.topic.clone();
        let payload = f.payload.clone();
        let me = self.clone();
        let ch = f.channel;
        let rid = f.request_id;
        tokio::spawn(async move {
            let out = (entry.func)(topic, payload).await;
            let resp = match out {
                Ok(bytes) => Frame {
                    opcode: OP_RESPONSE,
                    channel: ch,
                    request_id: rid,
                    payload: bytes,
                    ..Default::default()
                },
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
        let entry = {
            let g = self.inner.lock().await;
            g.workers.get(&f.channel).cloned()
        };
        let Some(w) = entry else {
            let resp = Frame {
                opcode: OP_JOB_NACK,
                channel: f.channel,
                service: f.service.clone(),
                error_code: ERR_JOB_NOT_FOUND,
                error_message: "sem worker nesse canal".into(),
                payload: Writer::new().str("").u8(0).u32(0).build(),
                ..Default::default()
            };
            let _ = self.write_frame(&resp).await;
            return;
        };

        let mut r = Reader::new(f.payload.clone());
        let id = r.str().unwrap_or_default();
        let attempt = r.u8().unwrap_or(1);
        let deadline = r.u32().unwrap_or(0);
        let data = r.rest();
        let job = Job {
            id: id.clone(),
            attempt,
            deadline_ms: deadline,
            data,
            queue: f.service.clone(),
            topic: f.topic.clone(),
            channel: f.channel,
        };
        let me = self.clone();
        let q = f.service.clone();
        let ch = f.channel;
        tokio::spawn(async move {
            let action = (w.handler)(job).await;
            let _ = match action {
                JobAction::Ack => me.job_ack(ch, &q, &id).await,
                JobAction::Nack { requeue, delay_ms, reason } =>
                    me.job_nack(ch, &q, &id, requeue, delay_ms, &reason).await,
                JobAction::Extend { extend_ms } =>
                    me.job_extend(ch, &q, &id, extend_ms).await,
            };
        });
    }

    async fn resolve(&self, ch: u16, rid: u32, resp: Response) {
        let mut g = self.inner.lock().await;
        if let Some(p) = g.pending.remove(&(ch, rid)) {
            let _ = p.tx.send(Ok(resp));
        }
    }

    async fn rpc(&self, ch: u16, rid: u32, f: Frame) -> Result<Response, ClientError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut g = self.inner.lock().await;
            g.pending.insert((ch, rid), Pending { tx });
        }
        if let Err(e) = self.write_frame(&f).await {
            let mut g = self.inner.lock().await;
            g.pending.remove(&(ch, rid));
            return Err(e);
        }
        let to = self.opts.request_timeout;
        match timeout(to, rx).await {
            Err(_) => {
                let mut g = self.inner.lock().await;
                g.pending.remove(&(ch, rid));
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

    // -----------------------------------------------------------------------
    // Canais
    // -----------------------------------------------------------------------

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
        self.rpc(ch, rid, Frame {
            opcode: OP_CHANNEL_OPEN,
            channel: ch,
            request_id: rid,
            ..Default::default()
        }).await?;
        let mut g = self.inner.lock().await;
        g.channels.insert(ch, ());
        g.desired_channels.insert(ch, ());
        Ok(())
    }

    pub async fn channel_close(&self, ch: u16) -> Result<(), ClientError> {
        if ch == 0 {
            return Err(ClientError::ChannelNotOpen(0));
        }
        let open = {
            let g = self.inner.lock().await;
            g.channels.contains_key(&ch)
        };
        if !open {
            return Ok(());
        }
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        let _ = self.rpc(ch, rid, Frame {
            opcode: OP_CHANNEL_CLOSE,
            channel: ch,
            request_id: rid,
            ..Default::default()
        }).await;
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

    // -----------------------------------------------------------------------
    // Pub/Sub
    // -----------------------------------------------------------------------

    pub async fn subscribe<F>(&self, ch: u16, pattern: &str, group: &str, cb: F)
        -> Result<(), ClientError>
    where
        F: Fn(String, String, Bytes) + Send + Sync + 'static,
    {
        self.ensure_open(ch).await?;
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        self.rpc(ch, rid, Frame {
            opcode: OP_SUBSCRIBE,
            channel: ch,
            request_id: rid,
            topic: pattern.to_string(),
            service: group.to_string(),
            ..Default::default()
        }).await?;
        let mut g = self.inner.lock().await;
        let sub = Subscription {
            pattern: pattern.to_string(),
            group: group.to_string(),
            cb: Arc::new(cb),
        };
        g.subscriptions.entry(ch).or_default().push(sub.clone());
        g.desired_subscriptions.entry(ch).or_default().push(sub);
        Ok(())
    }

    pub async fn unsubscribe(&self, ch: u16, pattern: &str, group: &str) -> Result<(), ClientError> {
        self.ensure_open(ch).await?;
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        self.rpc(ch, rid, Frame {
            opcode: OP_UNSUBSCRIBE,
            channel: ch,
            request_id: rid,
            topic: pattern.to_string(),
            service: group.to_string(),
            ..Default::default()
        }).await?;
        let mut g = self.inner.lock().await;
        if let Some(v) = g.subscriptions.get_mut(&ch) {
            v.retain(|s| !(s.pattern == pattern && s.group == group));
        }
        if let Some(v) = g.desired_subscriptions.get_mut(&ch) {
            v.retain(|s| !(s.pattern == pattern && s.group == group));
        }
        Ok(())
    }

    pub async fn publish(&self, ch: u16, topic: &str, data: impl Into<Bytes>)
        -> Result<(), ClientError>
    {
        self.ensure_open(ch).await?;
        let f = Frame {
            opcode: OP_PUBLISH,
            channel: ch,
            request_id: 0,
            topic: topic.to_string(),
            payload: data.into(),
            ..Default::default()
        };
        self.write_frame(&f).await
    }

    pub async fn publish_sync(&self, ch: u16, topic: &str, data: impl Into<Bytes>)
        -> Result<(), ClientError>
    {
        self.ensure_open(ch).await?;
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        self.rpc(ch, rid, Frame {
            opcode: OP_PUBLISH,
            channel: ch,
            request_id: rid,
            topic: topic.to_string(),
            payload: data.into(),
            ..Default::default()
        }).await.map(|_| ())
    }

    // -----------------------------------------------------------------------
    // Request/Reply
    // -----------------------------------------------------------------------

    pub async fn serve<F, Fut>(&self, ch: u16, service: &str, topic: &str, f: F)
        -> Result<(), ClientError>
    where
        F: Fn(String, Bytes) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Bytes, String>> + Send + 'static,
    {
        self.ensure_open(ch).await?;
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        self.rpc(ch, rid, Frame {
            opcode: OP_SERVE,
            channel: ch,
            request_id: rid,
            service: service.to_string(),
            topic: topic.to_string(),
            ..Default::default()
        }).await?;

        let func: Arc<dyn Fn(String, Bytes) -> HandlerFuture + Send + Sync + 'static> =
            Arc::new(move |topic: String, payload: Bytes| -> HandlerFuture {
                Box::pin(f(topic, payload))
            });
        let mut g = self.inner.lock().await;
        let entry = HandlerEntry {
            channel: ch,
            service: service.to_string(),
            topic: topic.to_string(),
            func,
        };
        g.handlers.insert(format!("{}|{}", service, topic), entry.clone());
        g.desired_handlers.entry(ch).or_default().push(entry);
        Ok(())
    }

    pub async fn unserve(&self, ch: u16, service: &str, topic: &str) -> Result<(), ClientError> {
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        self.rpc(ch, rid, Frame {
            opcode: OP_UNSERVE,
            channel: ch,
            request_id: rid,
            service: service.to_string(),
            topic: topic.to_string(),
            ..Default::default()
        }).await?;
        let mut g = self.inner.lock().await;
        g.handlers.remove(&format!("{}|{}", service, topic));
        if let Some(v) = g.desired_handlers.get_mut(&ch) {
            v.retain(|h| !(h.service == service && h.topic == topic));
        }
        Ok(())
    }

    pub async fn request(&self, ch: u16, service: &str, topic: &str,
        data: impl Into<Bytes>) -> Result<Bytes, ClientError> {
        self.ensure_open(ch).await?;
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        let r = self.rpc(ch, rid, Frame {
            opcode: OP_REQUEST,
            channel: ch,
            request_id: rid,
            service: service.to_string(),
            topic: topic.to_string(),
            payload: data.into(),
            ..Default::default()
        }).await?;
        Ok(r.payload)
    }

    // -----------------------------------------------------------------------
    // Worker/Job
    // -----------------------------------------------------------------------

    pub async fn register_worker<F, Fut>(&self, ch: u16, queue: &str, filter: &str,
        prefetch: u16, visibility_ms: u32, f: F) -> Result<(), ClientError>
    where
        F: Fn(Job) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = JobAction> + Send + 'static,
    {
        self.ensure_open(ch).await?;
        let filter = if filter.is_empty() { ">" } else { filter };
        let prefetch = prefetch.max(1);
        let visibility_ms = if visibility_ms == 0 { 30_000 } else { visibility_ms };
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        let payload = Writer::new().u16(prefetch).u32(visibility_ms).build();
        self.rpc(ch, rid, Frame {
            opcode: OP_WORKER,
            channel: ch,
            request_id: rid,
            topic: filter.to_string(),
            service: queue.to_string(),
            payload,
            ..Default::default()
        }).await?;
        let handler: JobHandler = Arc::new(move |j| -> JobFuture { Box::pin(f(j)) });
        let mut g = self.inner.lock().await;
        let entry = WorkerEntry {
            queue: queue.to_string(),
            filter: filter.to_string(),
            prefetch,
            visibility_ms,
            handler,
        };
        g.workers.insert(ch, entry.clone());
        g.desired_workers.entry(ch).or_default().push(entry);
        Ok(())
    }

    pub async fn worker_off(&self, ch: u16, queue: &str) -> Result<(), ClientError> {
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        self.write_frame(&Frame {
            opcode: OP_WORKER_OFF,
            channel: ch,
            request_id: rid,
            service: queue.to_string(),
            ..Default::default()
        }).await?;
        let mut g = self.inner.lock().await;
        g.workers.remove(&ch);
        g.desired_workers.remove(&ch);
        Ok(())
    }

    pub async fn enqueue(&self, ch: u16, queue: &str, job_type: &str,
        data: impl Into<Bytes>, opts: EnqueueOptions) -> Result<Enqueued, ClientError> {
        self.ensure_open(ch).await?;
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        let payload = Writer::new()
            .str(&opts.job_id)
            .u8(opts.priority)
            .u32(opts.delay_ms)
            .u8(opts.retries)
            .u32(opts.ttl_ms)
            .raw(&data.into())
            .build();
        let r = self.rpc(ch, rid, Frame {
            opcode: OP_ENQUEUE,
            channel: ch,
            request_id: rid,
            service: queue.to_string(),
            topic: job_type.to_string(),
            payload,
            ..Default::default()
        }).await?;
        Ok(decode_enqueued(r.payload))
    }

    pub async fn job_ack(&self, ch: u16, queue: &str, job_id: &str) -> Result<(), ClientError> {
        self.write_frame(&Frame {
            opcode: OP_JOB_ACK,
            channel: ch,
            service: queue.to_string(),
            payload: Writer::new().str(job_id).build(),
            ..Default::default()
        }).await
    }

    pub async fn job_nack(&self, ch: u16, queue: &str, job_id: &str,
        requeue: bool, delay_ms: u32, reason: &str) -> Result<(), ClientError> {
        let q = if requeue { 1u8 } else { 0u8 };
        self.write_frame(&Frame {
            opcode: OP_JOB_NACK,
            channel: ch,
            service: queue.to_string(),
            error_code: ERR_HANDLER_ERROR,
            error_message: reason.to_string(),
            payload: Writer::new().str(job_id).u8(q).u32(delay_ms).build(),
            ..Default::default()
        }).await
    }

    pub async fn job_extend(&self, ch: u16, queue: &str, job_id: &str, extend_ms: u32)
        -> Result<(), ClientError>
    {
        self.write_frame(&Frame {
            opcode: OP_JOB_EXTEND,
            channel: ch,
            service: queue.to_string(),
            payload: Writer::new().str(job_id).u32(extend_ms).build(),
            ..Default::default()
        }).await
    }

    // -----------------------------------------------------------------------
    // Admin: API keys
    // -----------------------------------------------------------------------

    pub async fn auth_mode(&self) -> String {
        self.inner.lock().await.auth_mode.clone()
    }

    pub async fn key_id(&self) -> String {
        self.inner.lock().await.key_id.clone()
    }

    pub async fn session_id(&self) -> String {
        self.inner.lock().await.session_id.clone()
    }

    /// Namespace negociado no HELLO_OK. Vazio em brokers antigos.
    pub async fn namespace(&self) -> String {
        self.inner.lock().await.namespace.clone()
    }

    /// Cria uma API key. `namespace` vazio = default no broker.
    pub async fn api_key_create(&self, ch: u16, name: &str,
        perms: serde_json::Value, labels: &[String], namespace: &str)
        -> Result<APIKeyCreateResult, ClientError>
    {
        if self.auth_mode().await != "master" {
            return Err(ClientError::RequiresMaster);
        }
        let perms_json = serde_json::to_string(&perms)
            .map_err(|e| ClientError::Other(e.to_string()))?;
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        let payload = Writer::new()
            .str(name)
            .str(&perms_json)
            .str(&labels.join(","))
            .str(namespace)
            .build();
        let r = self.rpc(ch, rid, Frame {
            opcode: OP_APIKEY_CREATE,
            channel: ch,
            request_id: rid,
            payload,
            ..Default::default()
        }).await?;
        let mut rr = Reader::new(r.payload);
        Ok(APIKeyCreateResult {
            id: rr.str().unwrap_or_default(),
            token: rr.str().unwrap_or_default(),
            permissions: rr.str().unwrap_or_default(),
        })
    }

    pub async fn api_key_revoke(&self, ch: u16, id: &str) -> Result<(), ClientError> {
        if self.auth_mode().await != "master" {
            return Err(ClientError::RequiresMaster);
        }
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        self.rpc(ch, rid, Frame {
            opcode: OP_APIKEY_REVOKE,
            channel: ch,
            request_id: rid,
            payload: Writer::new().str(id).build(),
            ..Default::default()
        }).await.map(|_| ())
    }

    pub async fn api_key_get(&self, ch: u16, id: &str) -> Result<String, ClientError> {
        if self.auth_mode().await != "master" {
            return Err(ClientError::RequiresMaster);
        }
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        let r = self.rpc(ch, rid, Frame {
            opcode: OP_APIKEY_GET,
            channel: ch,
            request_id: rid,
            payload: Writer::new().str(id).build(),
            ..Default::default()
        }).await?;
        let mut rr = Reader::new(r.payload);
        Ok(rr.str().unwrap_or_default())
    }

    pub async fn api_key_list(&self, ch: u16) -> Result<String, ClientError> {
        if self.auth_mode().await != "master" {
            return Err(ClientError::RequiresMaster);
        }
        let rid = {
            let mut g = self.inner.lock().await;
            Self::bump_request_id(&mut g, ch)
        };
        let r = self.rpc(ch, rid, Frame {
            opcode: OP_APIKEY_LIST,
            channel: ch,
            request_id: rid,
            ..Default::default()
        }).await?;
        let mut rr = Reader::new(r.payload);
        Ok(rr.str().unwrap_or_default())
    }

    // -----------------------------------------------------------------------
    // Internos
    // -----------------------------------------------------------------------

    async fn ensure_open(&self, ch: u16) -> Result<(), ClientError> {
        let g = self.inner.lock().await;
        if !g.channels.contains_key(&ch) {
            return Err(ClientError::ChannelNotOpen(ch));
        }
        Ok(())
    }

    fn start_ping_loop(&self) {
        let me = self.clone();
        tokio::spawn(async move {
            let interval = {
                let g = me.inner.lock().await;
                (g.heartbeat / 2).max(Duration::from_secs(1))
            };
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                {
                    let g = me.inner.lock().await;
                    if !g.connected {
                        return;
                    }
                }
                let _ = me.write_frame(&Frame {
                    opcode: OP_PING,
                    ..Default::default()
                }).await;
            }
        });
    }

    async fn emit_error(&self, e: ClientError) {
        if self.opts.debug {
            eprintln!("[flowmq] error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn decode_enqueued(p: Bytes) -> Enqueued {
    let mut r = Reader::new(p);
    Enqueued {
        job_id: r.str().unwrap_or_default(),
        seq: r.u64().unwrap_or(0),
    }
}

pub fn match_topic(pattern: &str, topic: &str) -> bool {
    let p: Vec<&str> = pattern.split('.').collect();
    let t: Vec<&str> = topic.split('.').collect();
    let mut i = 0;
    for tok in p {
        if tok == ">" {
            return i < t.len();
        }
        if i >= t.len() {
            return false;
        }
        if tok == "*" {
            i += 1;
            continue;
        }
        if tok != t[i] {
            return false;
        }
        i += 1;
    }
    i == t.len()
}

fn split_handler_key(k: &str) -> (&str, &str) {
    if let Some(i) = k.find('|') {
        (&k[..i], &k[i + 1..])
    } else {
        (k, "")
    }
}



use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("url inválida: {0}")]
    Url(String),

    #[error("frame inválido: {0}")]
    BadFrame(String),

    #[error("frame grande demais: {0} > {1}")]
    FrameTooLarge(usize, usize),

    #[error("broker error {code}: {message}")]
    Broker { code: u16, message: String },

    #[error("canal {0} não está aberto")]
    ChannelNotOpen(u16),

    #[error("canal {0} já está aberto")]
    ChannelAlreadyOpen(u16),

    #[error("timeout após {0:?}")]
    Timeout(std::time::Duration),

    #[error("conexão fechada")]
    Disconnected,

    #[error("cliente fechado")]
    Closed,

    #[error("operação exige master token")]
    RequiresMaster,

    #[error("{0}")]
    Other(String),
}

impl ClientError {
    pub fn broker_code(&self) -> Option<u16> {
        match self {
            ClientError::Broker { code, .. } => Some(*code),
            _ => None,
        }
    }
}



use crate::error::ClientError;
use crate::opcodes::MIN_FRAME_BODY;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("bad frame: {0}")]
    Bad(String),
    #[error("frame too large: {0} > {1}")]
    TooLarge(usize, usize),
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
    pub fn new(opcode: u8) -> Self {
        Self { opcode, ..Default::default() }
    }

    pub fn encode(&self) -> BytesMut {
        let mut body = BytesMut::with_capacity(64);
        body.put_u8(self.opcode);
        body.put_u16(self.channel);
        body.put_u32(self.request_id);
        put_str(&mut body, &self.topic);
        put_str(&mut body, &self.service);
        body.put_u16(self.error_code);
        put_str(&mut body, &self.error_message);
        body.extend_from_slice(&self.payload);

        let mut out = BytesMut::with_capacity(4 + body.len());
        out.put_u32(body.len() as u32);
        out.extend_from_slice(&body);
        out
    }

    pub fn decode_body(mut body: Bytes) -> Result<Self, FrameError> {
        if body.len() < MIN_FRAME_BODY - 4 {
            return Err(FrameError::Bad(format!("corpo curto: {}", body.len())));
        }
        let opcode = body.get_u8();
        let channel = body.get_u16();
        let request_id = body.get_u32();
        let topic = get_str(&mut body)?;
        let service = get_str(&mut body)?;
        let error_code = body.get_u16();
        let error_message = get_str(&mut body)?;
        Ok(Self {
            opcode, channel, request_id, topic, service,
            error_code, error_message, payload: body,
        })
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
    let s = std::str::from_utf8(&buf[..n])
        .map_err(|_| FrameError::Bad("utf8".into()))?
        .to_owned();
    buf.advance(n);
    Ok(s)
}

// ---------------------------------------------------------------------------
// Writer / Reader para payloads
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Writer {
    pub buf: BytesMut,
}

impl Writer {
    pub fn new() -> Self { Self { buf: BytesMut::new() } }
    pub fn u8(mut self, v: u8) -> Self { self.buf.put_u8(v); self }
    pub fn u16(mut self, v: u16) -> Self { self.buf.put_u16(v); self }
    pub fn u32(mut self, v: u32) -> Self { self.buf.put_u32(v); self }
    pub fn u64(mut self, v: u64) -> Self { self.buf.put_u64(v); self }
    pub fn str(mut self, s: &str) -> Self { put_str(&mut self.buf, s); self }
    pub fn raw(mut self, d: &[u8]) -> Self { self.buf.extend_from_slice(d); self }
    pub fn build(self) -> Bytes { self.buf.freeze() }
}

pub struct Reader {
    buf: Bytes,
}

impl Reader {
    pub fn new(b: Bytes) -> Self { Self { buf: b } }
    pub fn u8(&mut self) -> Result<u8, FrameError> {
        if self.buf.is_empty() { return Err(FrameError::Bad("u8".into())); }
        Ok(self.buf.get_u8())
    }
    pub fn u16(&mut self) -> Result<u16, FrameError> {
        if self.buf.len() < 2 { return Err(FrameError::Bad("u16".into())); }
        Ok(self.buf.get_u16())
    }
    pub fn u32(&mut self) -> Result<u32, FrameError> {
        if self.buf.len() < 4 { return Err(FrameError::Bad("u32".into())); }
        Ok(self.buf.get_u32())
    }
    pub fn u64(&mut self) -> Result<u64, FrameError> {
        if self.buf.len() < 8 { return Err(FrameError::Bad("u64".into())); }
        Ok(self.buf.get_u64())
    }
    pub fn str(&mut self) -> Result<String, FrameError> { get_str(&mut self.buf) }
    pub fn rest(mut self) -> Bytes { std::mem::take(&mut self.buf) }
    pub fn remaining(&self) -> usize { self.buf.len() }
}

// ---------------------------------------------------------------------------
// Framer incremental
// ---------------------------------------------------------------------------

pub struct Framer {
    buf: BytesMut,
    max_frame: usize,
}

impl Framer {
    pub fn new(max_frame: usize) -> Self {
        Self { buf: BytesMut::with_capacity(8 * 1024), max_frame }
    }

    pub fn set_max(&mut self, max: usize) { self.max_frame = max; }
    pub fn max(&self) -> usize { self.max_frame }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FrameError> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 4 { break; }
            let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            if len > self.max_frame {
                return Err(FrameError::TooLarge(len, self.max_frame));
            }
            if len < MIN_FRAME_BODY - 4 {
                return Err(FrameError::Bad(format!("length baixo: {len}")));
            }
            if self.buf.len() < 4 + len { break; }
            self.buf.advance(4);
            let body = self.buf.split_to(len).freeze();
            out.push(Frame::decode_body(body)?);
        }
        Ok(out)
    }
}

impl From<FrameError> for ClientError {
    fn from(e: FrameError) -> Self {
        match e {
            FrameError::TooLarge(a, b) => ClientError::FrameTooLarge(a, b),
            FrameError::Bad(s) => ClientError::BadFrame(s),
        }
    }
}



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



use crate::client::Options;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ParsedUrl {
    pub options: Options,
}

/// Aceita:
///   fogumq://host
///   fogumq://host:7222
///   fogumq://token@host
///   fogumq://token@host:7222
///   fogumq://token@[::1]:7222/?clientId=app&maxFrame=1048576&namespace=acme
pub fn parse_fogumq_url(raw: &str) -> Result<ParsedUrl, String> {
    if raw.is_empty() {
        return Err("URL vazia".into());
    }
    let norm = if raw.starts_with("fogumq:") && !raw.starts_with("fogumq://") {
        format!("fogumq://{}", &raw["fogumq:".len()..])
    } else {
        raw.to_string()
    };

    let rest = norm
        .strip_prefix("fogumq://")
        .ok_or_else(|| format!("esquema esperado fogumq://, veio {raw}"))?;

    let (before_q, query) = match rest.split_once('?') {
        Some((a, b)) => (a, b),
        None => (rest, ""),
    };

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
    if let Some(rest6) = host_port.strip_prefix('[') {
        let end = rest6
            .find(']')
            .ok_or_else(|| format!("URL inválida (IPv6 sem ']'): {raw}"))?;
        host = rest6[..end].to_string();
        let after = &rest6[end + 1..];
        if let Some(p) = after.strip_prefix(':') {
            port = p.parse().map_err(|_| format!("porta inválida: {p}"))?;
        }
    } else if let Some(i) = host_port.rfind(':') {
        host = if i == 0 { "localhost".into() } else { host_port[..i].to_string() };
        let p = &host_port[i + 1..];
        port = p.parse().map_err(|_| format!("porta inválida: {p}"))?;
    } else if !host_port.is_empty() {
        host = host_port.to_string();
    }

    let auth_token = if userinfo.is_empty() {
        String::new()
    } else {
        userinfo.to_string()
    };

    let mut opts = Options { host, port, auth_token, ..Default::default() };

    for pair in query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = match pair.split_once('=') {
            Some((a, b)) => (a, b),
            None => (pair, ""),
        };
        let v_dec = percent_decode(v);
        match k {
            "clientId" | "client_id" => opts.client_id = v_dec,
            "maxFrame" | "max_frame" => {
                if let Ok(n) = v_dec.parse::<usize>() { opts.max_frame = n; }
            }
            "requestTimeout" | "request_timeout" => {
                if let Ok(n) = v_dec.parse::<u64>() {
                    opts.request_timeout = Duration::from_millis(n);
                }
            }
            "connectTimeout" | "connect_timeout" => {
                if let Ok(n) = v_dec.parse::<u64>() {
                    opts.connect_timeout = Duration::from_millis(n);
                }
            }
            "maxChannels" | "max_channels" => {
                if let Ok(n) = v_dec.parse::<u16>() { opts.max_channels = n; }
            }
            "autoReconnect" | "auto_reconnect" => {
                opts.auto_reconnect = matches!(v_dec.as_str(), "1" | "true" | "yes");
            }
            "reconnectBackoff" | "reconnect_backoff" => {
                if let Ok(n) = v_dec.parse::<u64>() {
                    opts.reconnect_backoff = Duration::from_millis(n);
                }
            }
            "namespace" | "ns" => opts.namespace = v_dec,
            _ => {}
        }
    }

    Ok(ParsedUrl { options: opts })
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push(((h * 16 + l) as u8) as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

url de test
fogumq://a7589807-e3b7-44a1-9b55-89cca684ea516a1bab7c-3398-83eb-8fc7-d541645eb50a@37.27.39.202:7222