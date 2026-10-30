use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};
use thiserror::Error;

// ============================================================
// Protocol constants
// ============================================================

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Ping         = 0x01,
    Pong         = 0x02,
    Auth         = 0x50,
    Error        = 0xFF,
    Get          = 0x10,
    Set          = 0x11,
    Del          = 0x12,
    Exists       = 0x13,
    Incr         = 0x20,
    Decr         = 0x21,
    Join         = 0x30,
    Leave        = 0x31,
    ChannelMsg   = 0x32,
    ChannelList  = 0x33,
    Members      = 0x34,
    Subscribe    = 0x40,
    Publish      = 0x41,
    Call         = 0x60,
    Act          = 0x61,
    Res          = 0x62,
    ApiKeyCreate = 0x70,
    ApiKeyDelete = 0x71,
    ApiKeyList   = 0x72,
    ApiKeyGet    = 0x73,
}

impl TryFrom<u8> for Opcode {
    type Error = FoguError;
    fn try_from(v: u8) -> Result<Self> {
        match v {
            0x01 => Ok(Self::Ping),       0x02 => Ok(Self::Pong),
            0x50 => Ok(Self::Auth),       0xFF => Ok(Self::Error),
            0x10 => Ok(Self::Get),        0x11 => Ok(Self::Set),
            0x12 => Ok(Self::Del),        0x13 => Ok(Self::Exists),
            0x20 => Ok(Self::Incr),       0x21 => Ok(Self::Decr),
            0x30 => Ok(Self::Join),       0x31 => Ok(Self::Leave),
            0x32 => Ok(Self::ChannelMsg), 0x33 => Ok(Self::ChannelList),
            0x34 => Ok(Self::Members),    0x40 => Ok(Self::Subscribe),
            0x41 => Ok(Self::Publish),    0x60 => Ok(Self::Call),
            0x61 => Ok(Self::Act),        0x62 => Ok(Self::Res),
            0x70 => Ok(Self::ApiKeyCreate), 0x71 => Ok(Self::ApiKeyDelete),
            0x72 => Ok(Self::ApiKeyList), 0x73 => Ok(Self::ApiKeyGet),
            _ => Err(FoguError::Protocol(format!("opcode desconhecido: 0x{v:02X}"))),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok           = 0x00,
    NotFound     = 0x01,
    Error        = 0x02,
    Invalid      = 0x03,
    Unauthorized = 0x04,
    Forbidden    = 0x05,
}

impl TryFrom<u8> for Status {
    type Error = ();
    fn try_from(v: u8) -> std::result::Result<Self, ()> {
        match v {
            0x00 => Ok(Self::Ok),    0x01 => Ok(Self::NotFound),
            0x02 => Ok(Self::Error), 0x03 => Ok(Self::Invalid),
            0x04 => Ok(Self::Unauthorized), 0x05 => Ok(Self::Forbidden),
            _ => Err(()),
        }
    }
}

const HEADER_SIZE: usize = 17;
const MAGIC_0: u8 = 0x46;
const MAGIC_1: u8 = 0x4F;
const VERSION:  u8 = 0x01;

// ============================================================
// Error type
// ============================================================

#[derive(Error, Debug)]
pub enum FoguError {
    #[error("{0}")]
    Message(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("timeout")]
    Timeout,
    #[error("não conectado")]
    NotConnected,
    #[error("não autenticado")]
    NotAuthenticated,
    #[error("operação exclusiva do root")]
    Forbidden,
    #[error("conexão encerrada")]
    ConnectionClosed,
    #[error("server error: {0} (status={1:?})")]
    ServerError(String, Option<u8>),
}

pub type Result<T> = std::result::Result<T, FoguError>;

// ============================================================
// Frame builder
// ============================================================

fn build_frame(opcode: Opcode, request_id: u64, payload: &[u8], flags: u8) -> Bytes {
    let mut buf = BytesMut::with_capacity(HEADER_SIZE + payload.len());
    buf.put_u8(MAGIC_0);
    buf.put_u8(MAGIC_1);
    buf.put_u8(VERSION);
    buf.put_u8(opcode as u8);
    buf.put_u8(flags);
    buf.put_u64(request_id);
    buf.put_u32(payload.len() as u32);
    buf.put_slice(payload);
    buf.freeze()
}

// ============================================================
// URL parser  fogu://token@host:port
// ============================================================

struct FoguUrl { token: String, host: String, port: u16 }

fn parse_fogu_url(url: &str) -> Result<FoguUrl> {
    let re = regex::Regex::new(r"^fogu://(?:[^:@]+:)?([^@]+)@([^:]+):(\d+)$").unwrap();
    let c  = re.captures(url).ok_or_else(|| {
        FoguError::Message("URL inválida. Formato esperado: fogu://token@host:port".into())
    })?;
    Ok(FoguUrl {
        token: c[1].to_string(),
        host:  c[2].to_string(),
        port:  c[3].parse().map_err(|_| FoguError::Message("porta inválida".into()))?,
    })
}

// ============================================================
// Internal types
// ============================================================

type FrameResult = std::result::Result<(Status, Value), FoguError>;
type PendingTx   = oneshot::Sender<FrameResult>;
type MsgHandler  = Arc<dyn Fn(Value) + Send + Sync + 'static>;
type RpcHandler  = Arc<dyn Fn(Value, RpcCtx) + Send + Sync + 'static>;

// ── RPC context passed to act() handlers ─────────────────────
pub struct RpcCtx {
    pub method:     String,
    pub request_id: u64,
    write_tx:       tokio::sync::mpsc::UnboundedSender<Bytes>,
}

impl RpcCtx {
    pub fn reply(&self, result: Value, error: Option<String>) {
        let body = match error {
            Some(e) => serde_json::json!({ "result": null,   "error": e    }),
            None    => serde_json::json!({ "result": result, "error": null }),
        };
        let _ = self.write_tx.send(
            build_frame(Opcode::Res, self.request_id, body.to_string().as_bytes(), 0)
        );
    }
}

// ── Shared mutable state ──────────────────────────────────────
struct Inner {
    connected:        bool,
    authenticated:    bool,
    is_root:          bool,
    pending:          HashMap<u64, PendingTx>,
    channel_handlers: HashMap<String, Vec<MsgHandler>>,
    topic_handlers:   HashMap<String, Vec<MsgHandler>>,
    action_handlers:  HashMap<String, RpcHandler>,
    write_tx:         Option<tokio::sync::mpsc::UnboundedSender<Bytes>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            connected: false, authenticated: false, is_root: false,
            pending: HashMap::new(),
            channel_handlers: HashMap::new(),
            topic_handlers:   HashMap::new(),
            action_handlers:  HashMap::new(),
            write_tx: None,
        }
    }
}

// ============================================================
// FoguClient
// ============================================================

#[derive(Clone)]
pub struct FoguClient {
    inner:  Arc<Mutex<Inner>>,
    id_ctr: Arc<AtomicU64>,
}

impl Default for FoguClient {
    fn default() -> Self { Self::new() }
}

impl FoguClient {
    pub fn new() -> Self {
        Self {
            inner:  Arc::new(Mutex::new(Inner::new())),
            id_ctr: Arc::new(AtomicU64::new(1)),
        }
    }

    fn next_id(&self) -> u64 {
        self.id_ctr.fetch_add(1, Ordering::Relaxed)
    }

    // ── connect ───────────────────────────────────────────────

    pub async fn connect(&self, url: &str) -> Result<()> {
        let parsed = parse_fogu_url(url)?;
        let addr   = format!("{}:{}", parsed.host, parsed.port);

        let stream = timeout(Duration::from_secs(30), TcpStream::connect(&addr))
            .await
            .map_err(|_| FoguError::Timeout)??;

        let (mut reader, writer) = stream.into_split();
        let (write_tx, mut write_rx) = tokio::sync::mpsc::unbounded_channel::<Bytes>();

        {
            let mut g = self.inner.lock().await;
            g.connected = true;
            g.write_tx  = Some(write_tx.clone());
        }

        // Writer task
        tokio::spawn(async move {
            let mut w = writer;
            while let Some(frame) = write_rx.recv().await {
                if w.write_all(&frame).await.is_err() { break; }
            }
        });

        // Reader task
        let inner_r = self.inner.clone();
        let wtx     = write_tx.clone();
        tokio::spawn(async move {
            let mut buf = Vec::<u8>::new();
            let mut tmp = [0u8; 4096];
            loop {
                match reader.read(&mut tmp).await {
                    Ok(0) | Err(_) => {
                        let mut g = inner_r.lock().await;
                        g.connected = false;
                        g.authenticated = false;
                        g.is_root = false;
                        for (_, tx) in g.pending.drain() {
                            let _ = tx.send(Err(FoguError::ConnectionClosed));
                        }
                        break;
                    }
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        Self::process_buffer(&mut buf, &inner_r, &wtx).await;
                    }
                }
            }
        });

        // Send Auth
        let auth_id  = self.next_id();
        let auth_pay = serde_json::json!({ "token": parsed.token });
        write_tx.send(build_frame(Opcode::Auth, auth_id, auth_pay.to_string().as_bytes(), 0))
            .map_err(|_| FoguError::NotConnected)?;

        let (tx, rx) = oneshot::channel();
        { self.inner.lock().await.pending.insert(auth_id, tx); }

        let res = timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| FoguError::Timeout)?
            .map_err(|_| FoguError::ConnectionClosed)?;

        match res {
            Ok((_, data)) => {
                let is_root = data.pointer("/result/role")
                    .and_then(|v| v.as_str())
                    .map(|r| r == "root")
                    .unwrap_or(false);
                let mut g = self.inner.lock().await;
                g.authenticated = true;
                g.is_root = is_root;
                Ok(())
            }
            Err(e) => {
                self.inner.lock().await.connected = false;
                Err(e)
            }
        }
    }

    // ── Buffer processor ──────────────────────────────────────

    async fn process_buffer(
        buf:      &mut Vec<u8>,
        inner:    &Arc<Mutex<Inner>>,
        write_tx: &tokio::sync::mpsc::UnboundedSender<Bytes>,
    ) {
        loop {
            if buf.len() < HEADER_SIZE { break; }
            if buf[0] != MAGIC_0 || buf[1] != MAGIC_1 {
                eprintln!("[FOGU] magic inválido — buffer descartado");
                buf.clear();
                break;
            }
            let payload_len = u32::from_be_bytes([buf[13], buf[14], buf[15], buf[16]]) as usize;
            let total_len   = HEADER_SIZE + payload_len;
            if buf.len() < total_len { break; }

            let opcode_byte = buf[3];
            let request_id  = u64::from_be_bytes(buf[5..13].try_into().unwrap());
            let payload     = buf[17..total_len].to_vec();
            buf.drain(..total_len);

            let opcode = match Opcode::try_from(opcode_byte) {
                Ok(o)  => o,
                Err(_) => { eprintln!("[FOGU] opcode desconhecido: 0x{opcode_byte:02X}"); continue; }
            };

            Self::dispatch_frame(opcode, request_id, payload, inner, write_tx).await;
        }
    }

    // ── Frame dispatcher ──────────────────────────────────────

    async fn dispatch_frame(
        opcode:    Opcode,
        request_id: u64,
        payload:   Vec<u8>,
        inner:     &Arc<Mutex<Inner>>,
        write_tx:  &tokio::sync::mpsc::UnboundedSender<Bytes>,
    ) {
        // Ping → Pong
        if opcode == Opcode::Ping {
            let _ = write_tx.send(build_frame(Opcode::Pong, request_id, &[], 0));
            return;
        }

        // ChannelMsg push
        if opcode == Opcode::ChannelMsg {
            let is_ack = {
                let g = inner.lock().await;
                g.pending.contains_key(&request_id) && payload.first().map(|&b| b <= 0x05).unwrap_or(false)
            };
            if !is_ack {
                let envelope: Value = serde_json::from_slice(&payload).unwrap_or(Value::Null);
                let ch = envelope.get("channel").and_then(|v| v.as_str()).map(String::from);
                if let Some(ch) = ch {
                    let handlers: Vec<MsgHandler> = {
                        let g = inner.lock().await;
                        g.channel_handlers.get(&ch).cloned().unwrap_or_default()
                    };
                    for h in handlers { h(envelope.clone()); }
                }
                return;
            }
        }

        // Publish push
        if opcode == Opcode::Publish {
            let has_pending = inner.lock().await.pending.contains_key(&request_id);
            if !has_pending {
                let data: Value = serde_json::from_slice(&payload).unwrap_or(Value::Null);
                let handlers: Vec<MsgHandler> = {
                    let g = inner.lock().await;
                    g.topic_handlers.values().flatten().cloned().collect()
                };
                for h in handlers { h(data.clone()); }
                return;
            }
        }

        // ✅ CORRIGIDO: Call push (RPC server side) - roteamento correto
        if opcode == Opcode::Call {
            let has_pending = inner.lock().await.pending.contains_key(&request_id);
            if !has_pending {
                // ✅ Extrai método e params do payload
                let call_data: Value = serde_json::from_slice(&payload).unwrap_or(Value::Null);
                let method = call_data.get("method")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_default();
                let params = call_data.get("params").cloned().unwrap_or(Value::Null);

                // ✅ Busca APENAS o handler específico para este método
                let handler_opt = {
                    let g = inner.lock().await;
                    g.action_handlers.get(&method).cloned()
                };

                match handler_opt {
                    Some(handler) => {
                        let ctx = RpcCtx { 
                            method: method.clone(), 
                            request_id, 
                            write_tx: write_tx.clone() 
                        };
                        handler(params, ctx);
                    }
                    None => {
                        // ✅ Método não encontrado - responde com erro
                        let error_body = serde_json::json!({
                            "result": null,
                            "error": format!("handler '{}' not found", method)
                        });
                        let _ = write_tx.send(
                            build_frame(Opcode::Res, request_id, error_body.to_string().as_bytes(), 0)
                        );
                    }
                }
                return;
            }
        }

        // Res (async RPC reply)
        if opcode == Opcode::Res {
            if let Some(tx) = inner.lock().await.pending.remove(&request_id) {
                let body: Value = serde_json::from_slice(&payload).unwrap_or(Value::Null);
                if let Some(err) = body.get("error").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    let _ = tx.send(Err(FoguError::ServerError(err.to_string(), None)));
                } else {
                    let result = body.get("result").cloned().unwrap_or(body);
                    let _ = tx.send(Ok((Status::Ok, result)));
                }
            }
            return;
        }

        // Error frame
        if opcode == Opcode::Error {
            if let Some(tx) = inner.lock().await.pending.remove(&request_id) {
                let status_byte = payload.first().copied();
                let msg_raw = String::from_utf8_lossy(if payload.len() > 1 { &payload[1..] } else { &payload }).to_string();
                let err_msg = serde_json::from_str::<Value>(&msg_raw)
                    .ok()
                    .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                    .unwrap_or(msg_raw);
                let _ = tx.send(Err(FoguError::ServerError(
                    if err_msg.is_empty() { "erro desconhecido".into() } else { err_msg },
                    status_byte,
                )));
            }
            return;
        }

        // Normal reply
        if let Some(tx) = inner.lock().await.pending.remove(&request_id) {
            if payload.is_empty() {
                let _ = tx.send(Err(FoguError::Protocol("payload vazio".into())));
                return;
            }
            let status_byte = payload[0];
            let body        = &payload[1..];
            let status      = Status::try_from(status_byte).unwrap_or(Status::Error);

            match status {
                Status::Ok | Status::NotFound => {
                    let result: Value = if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(body)
                            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).into()))
                    };
                    let _ = tx.send(Ok((status, result)));
                }
                _ => {
                    let err_str = String::from_utf8_lossy(body).to_string();
                    let err_msg = serde_json::from_str::<Value>(&err_str)
                        .ok()
                        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                        .unwrap_or(err_str);
                    let _ = tx.send(Err(FoguError::ServerError(err_msg, Some(status_byte))));
                }
            }
        }
    }

    // ── Guards ────────────────────────────────────────────────

    async fn require_auth(&self) -> Result<()> {
        let g = self.inner.lock().await;
        if !g.connected     { return Err(FoguError::NotConnected);     }
        if !g.authenticated { return Err(FoguError::NotAuthenticated); }
        Ok(())
    }

    async fn require_root(&self) -> Result<()> {
        self.require_auth().await?;
        if !self.inner.lock().await.is_root { return Err(FoguError::Forbidden); }
        Ok(())
    }

    async fn send_frame(&self, opcode: Opcode, id: u64, payload: &[u8], flags: u8) -> Result<()> {
        let g = self.inner.lock().await;
        if !g.connected { return Err(FoguError::NotConnected); }
        if let Some(tx) = &g.write_tx {
            tx.send(build_frame(opcode, id, payload, flags))
                .map_err(|_| FoguError::NotConnected)?;
        }
        Ok(())
    }

    async fn make_request(&self, opcode: Opcode, payload: Option<&Value>, timeout_ms: u64) -> Result<(Status, Value)> {
        self.require_auth().await?;
        let id  = self.next_id();
        let buf = payload.map(|v| v.to_string().into_bytes()).unwrap_or_default();
        self.send_frame(opcode, id, &buf, 0).await?;

        let (tx, rx) = oneshot::channel();
        self.inner.lock().await.pending.insert(id, tx);

        timeout(Duration::from_millis(timeout_ms), rx)
            .await
            .map_err(|_| {
                let inner = self.inner.clone();
                tokio::spawn(async move { inner.lock().await.pending.remove(&id); });
                FoguError::Timeout
            })?
            .map_err(|_| FoguError::ConnectionClosed)?
    }

    // ── Public state getters ──────────────────────────────────

    pub async fn is_connected(&self)     -> bool { self.inner.lock().await.connected     }
    pub async fn is_authenticated(&self) -> bool { self.inner.lock().await.authenticated }
    pub async fn is_root(&self)          -> bool { self.inner.lock().await.is_root       }

    pub async fn disconnect(&self) {
        let mut g = self.inner.lock().await;
        g.connected = false;
        g.authenticated = false;
        g.is_root = false;
        g.write_tx = None;
    }

    // ── Ping ──────────────────────────────────────────────────

    pub async fn ping(&self) -> Result<()> {
        self.require_auth().await?;
        let id = self.next_id();
        self.send_frame(Opcode::Ping, id, &[], 0).await?;

        let (tx, rx) = oneshot::channel();
        self.inner.lock().await.pending.insert(id, tx);

        timeout(Duration::from_millis(5000), rx)
            .await
            .map_err(|_| FoguError::Timeout)?
            .map_err(|_| FoguError::ConnectionClosed)?
            .map(|_| ())
    }

    // ── Sub-clients ───────────────────────────────────────────

    pub fn kv(&self)      -> KvClient      { KvClient      { c: self.clone() } }
    pub fn apikeys(&self) -> ApiKeyClient  { ApiKeyClient  { c: self.clone() } }
    pub fn channel(&self, name: impl Into<String>) -> ChannelClient {
        ChannelClient { c: self.clone(), name: name.into() }
    }
}

// ============================================================
// KvClient
// ============================================================

pub struct KvClient { c: FoguClient }

impl KvClient {
    pub async fn get(&self, key: &str) -> Result<Option<Value>> {
        let (_, d) = self.c.make_request(Opcode::Get, Some(&serde_json::json!({ "key": key })), 5000).await?;
        Ok(d.get("value").cloned())
    }

    pub async fn set(&self, key: &str, value: Value, ttl: Option<u64>) -> Result<bool> {
        let p = match ttl {
            Some(t) => serde_json::json!({ "key": key, "value": value, "ttl": t }),
            None    => serde_json::json!({ "key": key, "value": value }),
        };
        self.c.make_request(Opcode::Set, Some(&p), 5000).await?;
        Ok(true)
    }

    pub async fn del(&self, key: &str) -> Result<bool> {
        let (_, d) = self.c.make_request(Opcode::Del, Some(&serde_json::json!({ "key": key })), 5000).await?;
        Ok(d.get("count").and_then(|v| v.as_u64()).unwrap_or(0) > 0)
    }

    pub async fn exists(&self, key: &str) -> Result<bool> {
        let (_, d) = self.c.make_request(Opcode::Exists, Some(&serde_json::json!({ "key": key })), 5000).await?;
        Ok(d.get("exists").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn incr(&self, key: &str, delta: i64) -> Result<i64> {
        let (_, d) = self.c.make_request(Opcode::Incr, Some(&serde_json::json!({ "key": key, "delta": delta })), 5000).await?;
        Ok(d.get("value").and_then(|v| v.as_i64()).unwrap_or(0))
    }

    pub async fn decr(&self, key: &str, delta: i64) -> Result<i64> {
        let (_, d) = self.c.make_request(Opcode::Decr, Some(&serde_json::json!({ "key": key, "delta": delta })), 5000).await?;
        Ok(d.get("value").and_then(|v| v.as_i64()).unwrap_or(0))
    }
}

// ============================================================
// ApiKeyClient
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Permissions {
    pub channels: Vec<String>,
    pub kv:       Vec<String>,
    pub topics:   Vec<String>,
    pub rpc:      Vec<String>,
}

pub struct ApiKeyClient { c: FoguClient }

impl ApiKeyClient {
    pub async fn create(&self, name: &str, perms: Permissions) -> Result<Value> {
        self.c.require_root().await?;
        let p = serde_json::json!({
            "name": name, "channels": perms.channels,
            "kv": perms.kv, "topics": perms.topics, "rpc": perms.rpc,
        });
        let (_, d) = self.c.make_request(Opcode::ApiKeyCreate, Some(&p), 5000).await?;
        Ok(d.get("api_key").cloned().unwrap_or(d))
    }

    pub async fn delete(&self, id: &str) -> Result<Value> {
        self.c.require_root().await?;
        let (_, d) = self.c.make_request(Opcode::ApiKeyDelete, Some(&serde_json::json!({ "id": id })), 5000).await?;
        Ok(d.get("result").cloned().unwrap_or(d))
    }

    pub async fn list(&self) -> Result<Vec<Value>> {
        self.c.require_root().await?;
        let (_, d) = self.c.make_request(Opcode::ApiKeyList, None, 5000).await?;
        Ok(d.get("api_keys").and_then(|v| v.as_array()).cloned().unwrap_or_default())
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        self.c.require_root().await?;
        let (_, d) = self.c.make_request(Opcode::ApiKeyGet, Some(&serde_json::json!({ "id": id })), 5000).await?;
        Ok(d.get("api_key").cloned().unwrap_or(d))
    }
}

// ============================================================
// ChannelClient
// ============================================================

pub struct ChannelClient { c: FoguClient, name: String }

impl ChannelClient {
    fn ns(&self, sub: Option<&str>) -> String {
        match sub { Some(s) => format!("{}/{}", self.name, s), None => self.name.clone() }
    }

    pub async fn join(&self, alias: Option<&str>) -> Result<Value> {
        let p = match alias {
            Some(a) => serde_json::json!({ "channel": self.name, "alias": a }),
            None    => serde_json::json!({ "channel": self.name }),
        };
        let (_, d) = self.c.make_request(Opcode::Join, Some(&p), 5000).await?;
        Ok(d.get("result").cloned().unwrap_or(d))
    }

    pub async fn leave(&self) -> Result<Value> {
        let (_, d) = self.c.make_request(Opcode::Leave, Some(&serde_json::json!({ "channel": self.name })), 5000).await?;
        Ok(d.get("result").cloned().unwrap_or(d))
    }

    pub async fn send(&self, data: Value) -> Result<bool> {
        self.c.make_request(Opcode::ChannelMsg, Some(&serde_json::json!({ "channel": self.name, "data": data })), 5000).await?;
        Ok(true)
    }

    /// Registers a message handler. Returns an async unsubscribe fn.
    pub async fn on_message<F: Fn(Value) + Send + Sync + 'static>(
        &self, f: F,
    ) -> impl Fn() + Send + Sync {
        let handler: MsgHandler = Arc::new(f);
        let name    = self.name.clone();
        let inner   = self.c.inner.clone();
        inner.lock().await.channel_handlers.entry(name.clone()).or_default().push(handler.clone());

        let ptr = Arc::as_ptr(&handler) as *const () as usize;
        move || {
            let inner2 = inner.clone();
            let name2  = name.clone();
            tokio::spawn(async move {
                let mut g = inner2.lock().await;
                if let Some(list) = g.channel_handlers.get_mut(&name2) {
                    list.retain(|h| Arc::as_ptr(h) as *const () as usize != ptr);
                    if list.is_empty() { g.channel_handlers.remove(&name2); }
                }
            });
        }
    }

    pub async fn members(&self) -> Result<Vec<Value>> {
        let (_, d) = self.c.make_request(Opcode::Members, Some(&serde_json::json!({ "channel": self.name })), 5000).await?;
        Ok(d.get("members").and_then(|v| v.as_array()).cloned().unwrap_or_default())
    }

    pub async fn list(&self) -> Result<Vec<Value>> {
        let (_, d) = self.c.make_request(Opcode::ChannelList, None, 5000).await?;
        Ok(d.get("channels").and_then(|v| v.as_array()).cloned().unwrap_or_default())
    }

    // ── Pub/Sub ───────────────────────────────────────────────

    pub async fn subscribe<F: Fn(Value) + Send + Sync + 'static>(
        &self, topic: &str, f: F,
    ) -> Result<impl Fn() + Send + Sync> {
        let key: String = self.ns(Some(topic));
        let handler: MsgHandler = Arc::new(f);
        self.c.inner.lock().await.topic_handlers.entry(key.clone()).or_default().push(handler.clone());

        self.c.make_request(Opcode::Subscribe, Some(&serde_json::json!({ "topic": key })), 5000).await?;

        let inner = self.c.inner.clone();
        let ptr   = Arc::as_ptr(&handler) as *const () as usize;
        Ok(move || {
            let inner2 = inner.clone();
            let key2   = key.clone();
            tokio::spawn(async move {
                let mut g = inner2.lock().await;
                if let Some(list) = g.topic_handlers.get_mut(&key2) {
                    list.retain(|h| Arc::as_ptr(h) as *const () as usize != ptr);
                    if list.is_empty() { g.topic_handlers.remove(&key2); }
                }
            });
        })
    }

    pub async fn publish(&self, topic: &str, data: Value) -> Result<bool> {
        self.c.make_request(
            Opcode::Publish,
            Some(&serde_json::json!({ "topic": self.ns(Some(topic)), "data": data })),
            5000,
        ).await?;
        Ok(true)
    }

    // ── RPC ───────────────────────────────────────────────────

    pub async fn act<F: Fn(Value, RpcCtx) + Send + Sync + 'static>(
        &self, method: &str, f: F,
    ) -> Result<bool> {
        let key = self.ns(Some(method));
        self.c.inner.lock().await.action_handlers.insert(key.clone(), Arc::new(f));
        self.c.make_request(Opcode::Act, Some(&serde_json::json!({ "method": key })), 5000).await?;
        Ok(true)
    }

    pub async fn call(&self, method: &str, params: Value, timeout_ms: u64) -> Result<Value> {
        self.c.require_auth().await?;
        let key = self.ns(Some(method));
        let id  = self.c.next_id();
        // ✅ Envia método + params no payload
        let pay = serde_json::json!({ "method": key, "params": params });
        self.c.send_frame(Opcode::Call, id, pay.to_string().as_bytes(), 0).await?;

        let (tx, rx) = oneshot::channel();
        self.c.inner.lock().await.pending.insert(id, tx);

        let res = timeout(Duration::from_millis(timeout_ms), rx)
            .await
            .map_err(|_| {
                let inner = self.c.inner.clone();
                tokio::spawn(async move { inner.lock().await.pending.remove(&id); });
                FoguError::Timeout
            })?
            .map_err(|_| FoguError::ConnectionClosed)??;

        Ok(res.1)
    }
}