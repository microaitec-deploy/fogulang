use crate::{
    api_key::{ApiKeyStore},
    broker::{BrokerEvent, error_frame, forbidden_frame},
    errors::FoguError,
    frame_io::{read_frame, write_frame},
    kv_store::KvStore,
    permissions::{has_access, ApiKeyPerms},
    protocol::{Frame, Opcode, StatusCode},
};
use std::collections::HashMap;
use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};
use uuid::Uuid;

// Client role
#[derive(Debug, Clone)]
pub enum ClientRole {
    /// Authenticated with root token — unlimited access.
    Root,
    /// Authenticated with an API key — scoped access.
    ApiKeyUser(crate::permissions::ApiKeyPerms),
}

impl ClientRole {
    pub fn can_channel(&self, channel: &str) -> bool {
        match self {
            Self::Root => true,
            Self::ApiKeyUser(p) => has_access(&p.channels, channel),
        }
    }
    pub fn can_kv(&self, key: &str) -> bool {
        match self {
            Self::Root => true,
            Self::ApiKeyUser(p) => has_access(&p.kv, key),
        }
    }
    pub fn can_topic(&self, topic: &str) -> bool {
        match self {
            Self::Root => true,
            Self::ApiKeyUser(p) => has_access(&p.topics, topic),
        }
    }
    pub fn can_rpc(&self, method: &str) -> bool {
        match self {
            Self::Root => true,
            Self::ApiKeyUser(p) => has_access(&p.rpc, method),
        }
    }

    /*
    pub fn can_rpc(&self, method: &str) -> bool {
        match self {
            Self::Root => true,
            Self::ApiKeyUser(p) => has_access(&p.rpc, method),
        }
    }
    */
    pub fn is_root(&self) -> bool {
        matches!(self, Self::Root)
    }
}

// Payloads
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthPayload {
    pub token: String,
}

pub struct Client {
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    id: Uuid,
    client_id: String,
    alias: String,
    role: Option<ClientRole>, // None = not authenticated yet
    kv: Arc<KvStore>,
    apikeys: Arc<ApiKeyStore>,
    broker_tx: mpsc::UnboundedSender<BrokerEvent>,
    root_token: String,
    self_tx: mpsc::UnboundedSender<Frame>,
    channels: Vec<String>,
    topics: Vec<String>,
    actions: Vec<String>,
}

impl Client {
    pub fn new(
        stream: TcpStream,
        kv: Arc<KvStore>,
        apikeys: Arc<ApiKeyStore>,
        broker_tx: mpsc::UnboundedSender<BrokerEvent>,
        root_token: String,
    ) -> Self {
        let id = Uuid::new_v4();
        let client_id = id.to_string();
        let alias = client_id[..8].to_string();

        let (reader, write_half) = stream.into_split();
        let writer = Arc::new(Mutex::new(write_half));
        let (self_tx, mut push_rx) = mpsc::unbounded_channel::<Frame>();

        let writer_clone = writer.clone();
        tokio::spawn(async move {
            while let Some(frame) = push_rx.recv().await {
                let mut w = writer_clone.lock().await;
                if let Err(e) = write_frame(&mut w, &frame).await {
                    error!("Push write error: {e}");
                    break;
                }
            }
        });

        Self {
            reader,
            writer,
            id,
            client_id,
            alias,
            role: None,
            kv,
            apikeys,
            broker_tx,
            root_token,
            self_tx,
            channels: Vec::new(),
            topics: Vec::new(),
            actions: Vec::new(),
        }
    }

    fn authenticated(&self) -> bool {
        self.role.is_some()
    }

    fn role(&self) -> &ClientRole {
        self.role.as_ref().expect("not authenticated")
    }

    async fn send_raw(
        &self,
        request_id: u64,
        status: StatusCode,
        opcode: Opcode,
        body: Bytes,
    ) -> Result<(), FoguError> {
        let mut p = BytesMut::with_capacity(1 + body.len());
        p.put_u8(status as u8);
        p.extend_from_slice(&body);
        let frame = Frame {
            opcode,
            flags: 0,
            request_id,
            payload: p.freeze(),
        };
        write_frame(&mut *self.writer.lock().await, &frame).await
    }

    async fn send_ok(
        &self,
        req_id: u64,
        opcode: Opcode,
        resp: &crate::broker::ResponsePayload,
    ) -> Result<(), FoguError> {
        self.send_raw(
            req_id,
            StatusCode::Ok,
            opcode,
            Bytes::from(serde_json::to_vec(resp)?),
        )
        .await
    }

    async fn send_error(&self, req_id: u64, msg: &str) -> Result<(), FoguError> {
        self.send_raw(
            req_id,
            StatusCode::Error,
            Opcode::Error,
            Bytes::from(serde_json::to_vec(
                &crate::broker::ResponsePayload::err(msg.to_string()),
            )?),
        )
        .await
    }

    async fn send_forbidden(&self, req_id: u64, msg: &str) -> Result<(), FoguError> {
        self.send_raw(
            req_id,
            StatusCode::Forbidden,
            Opcode::Error,
            Bytes::from(serde_json::to_vec(
                &crate::broker::ResponsePayload::err(msg.to_string()),
            )?),
        )
        .await
    }

    async fn handle_frame(&mut self, frame: Frame) -> Result<(), FoguError> {
        let req_id = frame.request_id;

        // ── Auth ──────────────────────────────────────────────────────────
        if frame.opcode == Opcode::Auth {
            if self.authenticated() {
                return self.send_error(req_id, "already authenticated").await;
            }
            let p: AuthPayload = serde_json::from_slice(&frame.payload)?;

            if p.token == self.root_token {
                self.role = Some(ClientRole::Root);
                info!("Client [{}] authenticated as ROOT", self.id);
                return self
                    .send_ok(
                        req_id,
                        Opcode::Auth,
                        &crate::broker::ResponsePayload::result(serde_json::json!({ "role": "root" })),
                    )
                    .await;
            }

            match self.apikeys.find_by_token(&p.token)? {
                Some(key) => {
                    info!(
                        "Client [{}] authenticated with API key '{}' ({})",
                        self.id, key.name, key.id
                    );
                    self.role = Some(ClientRole::ApiKeyUser(key.perms));
                    return self
                        .send_ok(
                            req_id,
                            Opcode::Auth,
                            &crate::broker::ResponsePayload::result(serde_json::json!({ "role": "api_key" })),
                        )
                        .await;
                }
                None => {
                    return self
                        .send_raw(
                            req_id,
                            StatusCode::Unauthorized,
                            Opcode::Error,
                            Bytes::from("invalid token"),
                        )
                        .await;
                }
            }
        }

        if !self.authenticated() {
            return self
                .send_raw(
                    req_id,
                    StatusCode::Unauthorized,
                    Opcode::Error,
                    Bytes::from("not authenticated"),
                )
                .await;
        }

        match frame.opcode {
            // ── Control ──────────────────────────────────────────────────
            Opcode::Ping => self
                .send_raw(req_id, StatusCode::Ok, Opcode::Pong, Bytes::new())
                .await,

            // ── KV ───────────────────────────────────────────────────────
            Opcode::Get => {
                let p: crate::kv_store::GetPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_kv(&p.key) {
                    return self
                        .send_forbidden(req_id, &format!("no kv access to '{}'", p.key))
                        .await;
                }
                let v = self.kv.get(&p.key)?.unwrap_or(serde_json::Value::Null);
                self.send_ok(req_id, Opcode::Get, &crate::broker::ResponsePayload::value(v))
                    .await
            }
            Opcode::Set => {
                let p: crate::kv_store::SetPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_kv(&p.key) {
                    return self
                        .send_forbidden(req_id, &format!("no kv access to '{}'", p.key))
                        .await;
                }
                self.kv.set(&p.key, p.value, p.ttl)?;
                self.send_ok(
                    req_id,
                    Opcode::Set,
                    &crate::broker::ResponsePayload::result(serde_json::json!("ok")),
                )
                .await
            }
            Opcode::Del => {
                let p: crate::kv_store::DelPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_kv(&p.key) {
                    return self
                        .send_forbidden(req_id, &format!("no kv access to '{}'", p.key))
                        .await;
                }
                let n = if self.kv.del(&p.key)? { 1 } else { 0 };
                self.send_ok(req_id, Opcode::Del, &crate::broker::ResponsePayload::count(n))
                    .await
            }
            Opcode::Exists => {
                let p: crate::kv_store::ExistsPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_kv(&p.key) {
                    return self
                        .send_forbidden(req_id, &format!("no kv access to '{}'", p.key))
                        .await;
                }
                self.send_ok(
                    req_id,
                    Opcode::Exists,
                    &crate::broker::ResponsePayload::exists(self.kv.exists(&p.key)?),
                )
                .await
            }
            Opcode::Incr => {
                let p: crate::kv_store::IncrDecrPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_kv(&p.key) {
                    return self
                        .send_forbidden(req_id, &format!("no kv access to '{}'", p.key))
                        .await;
                }
                let v = self.kv.incr(&p.key, p.delta)?;
                self.send_ok(
                    req_id,
                    Opcode::Incr,
                    &crate::broker::ResponsePayload::value(serde_json::json!(v)),
                )
                .await
            }
            Opcode::Decr => {
                let p: crate::kv_store::IncrDecrPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_kv(&p.key) {
                    return self
                        .send_forbidden(req_id, &format!("no kv access to '{}'", p.key))
                        .await;
                }
                let v = self.kv.incr(&p.key, -p.delta)?;
                self.send_ok(
                    req_id,
                    Opcode::Decr,
                    &crate::broker::ResponsePayload::value(serde_json::json!(v)),
                )
                .await
            }

            // ── Channels ─────────────────────────────────────────────────
            Opcode::Join => {
                let p: crate::broker::JoinPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_channel(&p.channel) {
                    return self
                        .send_forbidden(
                            req_id,
                            &format!("no access to channel '{}'", p.channel),
                        )
                        .await;
                }
                let alias = p.alias.clone().unwrap_or_else(|| self.alias.clone());
                if !self.channels.contains(&p.channel) {
                    self.channels.push(p.channel.clone());
                }
                let _ = self.broker_tx.send(BrokerEvent::Join {
                    channel: p.channel.clone(),
                    client_id: self.client_id.clone(),
                    alias,
                    tx: self.self_tx.clone(),
                });
                self.send_ok(
                    req_id,
                    Opcode::Join,
                    &crate::broker::ResponsePayload::result(serde_json::json!({
                        "channel": p.channel,
                        "status":  "joined",
                    })),
                )
                .await
            }

            Opcode::Leave => {
                let p: crate::broker::LeavePayload = serde_json::from_slice(&frame.payload)?;
                self.channels.retain(|c| c != &p.channel);
                let _ = self.broker_tx.send(BrokerEvent::Leave {
                    channel: p.channel.clone(),
                    client_id: self.client_id.clone(),
                });
                self.send_ok(
                    req_id,
                    Opcode::Leave,
                    &crate::broker::ResponsePayload::result(serde_json::json!({
                        "channel": p.channel,
                        "status":  "left",
                    })),
                )
                .await
            }

            Opcode::ChannelMsg => {
                let p: crate::broker::ChannelMsgPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_channel(&p.channel) {
                    return self
                        .send_forbidden(
                            req_id,
                            &format!("no access to channel '{}'", p.channel),
                        )
                        .await;
                }
                let envelope = serde_json::json!({
                    "channel":   p.channel,
                    "sender_id": self.client_id,
                    "alias":     self.alias,
                    "data":      p.data,
                });
                let channel_name = p.channel.clone();
                let data = Bytes::from(serde_json::to_vec(&envelope)?);
                let _ = self.broker_tx.send(BrokerEvent::ChannelMsg {
                    channel: p.channel,
                    sender_id: self.client_id.clone(),
                    data,
                });
                self.send_ok(
                    req_id,
                    Opcode::ChannelMsg,
                    &crate::broker::ResponsePayload::result(serde_json::json!({
                        "channel": channel_name,
                        "status":  "sent",
                    })),
                )
                .await
            }

            Opcode::ChannelList => {
                let _ = self.broker_tx.send(BrokerEvent::ChannelList {
                    reply_tx: self.self_tx.clone(),
                    request_id: req_id,
                });
                Ok(())
            }

            Opcode::Members => {
                let p: crate::broker::MembersPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_channel(&p.channel) {
                    return self
                        .send_forbidden(
                            req_id,
                            &format!("no access to channel '{}'", p.channel),
                        )
                        .await;
                }
                let _ = self.broker_tx.send(BrokerEvent::Members {
                    channel: p.channel,
                    reply_tx: self.self_tx.clone(),
                    request_id: req_id,
                });
                Ok(())
            }

            // ── Pub/Sub ──────────────────────────────────────────────────
            Opcode::Subscribe => {
                let p: crate::broker::SubscribePayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_topic(&p.topic) {
                    return self
                        .send_forbidden(req_id, &format!("no access to topic '{}'", p.topic))
                        .await;
                }
                if !self.topics.contains(&p.topic) {
                    self.topics.push(p.topic.clone());
                }
                let _ = self.broker_tx.send(BrokerEvent::Subscribe {
                    topic: p.topic.clone(),
                    client_id: self.client_id.clone(),
                    tx: self.self_tx.clone(),
                });
                self.send_ok(
                    req_id,
                    Opcode::Subscribe,
                    &crate::broker::ResponsePayload::result(serde_json::json!({
                        "topic":  p.topic,
                        "status": "subscribed",
                    })),
                )
                .await
            }

Opcode::Publish => {
    let p: crate::broker::PublishPayload =
        serde_json::from_slice(&frame.payload)?;

    if !self.role().can_topic(&p.topic) {
        return self
            .send_forbidden(
                req_id,
                &format!("no access to topic '{}'", p.topic),
            )
            .await;
    }

    let topic = p.topic.clone();

    let data = Bytes::from(serde_json::to_vec(
        &serde_json::json!({
            "data": p.data,
            "topic": p.topic,
        }),
    )?);

    let _ = self
        .broker_tx
        .send(BrokerEvent::Publish {
            topic,
            data,
        });

    self.send_ok(
        req_id,
        Opcode::Publish,
        &crate::broker::ResponsePayload::result(
            serde_json::json!("published"),
        ),
    )
    .await
}

            // ── RPC ──────────────────────────────────────────────────────
            Opcode::Act => {
                let p: crate::broker::ActPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_rpc(&p.method) {
                    return self
                        .send_forbidden(req_id, &format!("no rpc access to '{}'", p.method))
                        .await;
                }


         
              
                if !self.actions.contains(&p.method) {
                    self.actions.push(p.method.clone());
                }
                let _ = self.broker_tx.send(BrokerEvent::RegisterAction {
                    method: p.method.clone(),
                    client_id: self.client_id.clone(),
                    tx: self.self_tx.clone(),
                });
                self.send_ok(
                    req_id,
                    Opcode::Act,
                    &crate::broker::ResponsePayload::result(serde_json::json!({
                        "method": p.method,
                        "status": "registered",
                    })),
                )
                .await
            }

            Opcode::Call => {
                let p: crate::broker::CallPayload = serde_json::from_slice(&frame.payload)?;
                if !self.role().can_rpc(&p.method) {
                    return self
                        .send_forbidden(req_id, &format!("no rpc access to '{}'", p.method))
                        .await;
                }
                let params = Bytes::from(serde_json::to_vec(&p.params)?);
                let _ = self.broker_tx.send(BrokerEvent::Call {
                    request_id: req_id,
                    method: p.method,
                    params,
                    caller_tx: self.self_tx.clone(),
                });
                Ok(())
            }

            Opcode::Res => {
                let _ = self.broker_tx.send(BrokerEvent::Response {
                    broker_req_id: req_id,
                    result: frame.payload,
                });
                Ok(())
            }

            // ── API Key Management (root only) ────────────────────────────
            Opcode::ApiKeyCreate => {
                if !self.role().is_root() {
                    return self
                        .send_forbidden(req_id, "only root can manage API keys")
                        .await;
                }
                let p: crate::api_key::ApiKeyCreatePayload =
                    serde_json::from_slice(&frame.payload)?;
                let key = crate::api_key::ApiKey::new(
                    p.name,
                    crate::permissions::ApiKeyPerms {
                        channels: p.channels,
                        kv: p.kv,
                        topics: p.topics,
                        rpc: p.rpc,
                    },
                );
                self.apikeys.insert(&key)?;
                info!("API key created: '{}' ({})", key.name, key.id);
                self.send_ok(
                    req_id,
                    Opcode::ApiKeyCreate,
                    &crate::broker::ResponsePayload::api_key(crate::api_key::ApiKeyView::with_token(&key)),
                )
                .await
            }

            Opcode::ApiKeyDelete => {
                if !self.role().is_root() {
                    return self
                        .send_forbidden(req_id, "only root can manage API keys")
                        .await;
                }
                let p: crate::api_key::ApiKeyDeletePayload =
                    serde_json::from_slice(&frame.payload)?;
                let deleted = self.apikeys.delete(&p.id)?;
                if deleted {
                    info!("API key deleted: {}", p.id);
                    self.send_ok(
                        req_id,
                        Opcode::ApiKeyDelete,
                        &crate::broker::ResponsePayload::result(serde_json::json!({ "id": p.id, "status": "deleted" })),
                    )
                    .await
                } else {
                    self.send_error(req_id, &format!("API key '{}' not found", p.id))
                        .await
                }
            }

            Opcode::ApiKeyList => {
                if !self.role().is_root() {
                    return self
                        .send_forbidden(req_id, "only root can manage API keys")
                        .await;
                }
                let keys: Vec<crate::api_key::ApiKeyView> = self
                    .apikeys
                    .list()?
                    .iter()
                    .map(crate::api_key::ApiKeyView::from)
                    .collect();
                self.send_ok(
                    req_id,
                    Opcode::ApiKeyList,
                    &crate::broker::ResponsePayload::api_keys(keys),
                )
                .await
            }

            Opcode::ApiKeyGet => {
                if !self.role().is_root() {
                    return self
                        .send_forbidden(req_id, "only root can manage API keys")
                        .await;
                }
                let p: crate::api_key::ApiKeyGetPayload = serde_json::from_slice(&frame.payload)?;
                match self.apikeys.find_by_id(&p.id)? {
                    Some(k) => {
                        self.send_ok(
                            req_id,
                            Opcode::ApiKeyGet,
                            &crate::broker::ResponsePayload::api_key(crate::api_key::ApiKeyView::from(&k)),
                        )
                        .await
                    }
                    None => {
                        self.send_error(req_id, &format!("API key '{}' not found", p.id))
                            .await
                    }
                }
            }

            Opcode::Pong | Opcode::Error => Ok(()),
            _ => self.send_error(req_id, "unexpected opcode").await,
        }
    }

    pub async fn run(&mut self) {
        info!("Client connected [{}]", self.id);
        loop {
            match read_frame(&mut self.reader).await {
                Ok(frame) => {
                    if let Err(e) = self.handle_frame(frame).await {
                        error!("Frame error [{}]: {e}", self.id);
                    }
                }
                Err(e) => {
                    warn!("Connection closed [{}]: {e}", self.id);
                    break;
                }
            }
        }

        for ch in self.channels.drain(..) {
            let _ = self.broker_tx.send(BrokerEvent::Leave {
                channel: ch,
                client_id: self.client_id.clone(),
            });
        }
        for t in self.topics.drain(..) {
            let _ = self.broker_tx.send(BrokerEvent::Unsubscribe {
                topic: t,
                client_id: self.client_id.clone(),
            });
        }
        for m in self.actions.drain(..) {
            let _ = self.broker_tx.send(BrokerEvent::UnregisterAction {
                method: m,
                client_id: self.client_id.clone(),
            });
        }
        let _ = self.broker_tx.send(BrokerEvent::Disconnect {
            client_id: self.client_id.clone(),
        });
        info!("Client disconnected [{}]", self.id);
    }
}