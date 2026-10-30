use crate::{
    protocol::{Frame, Opcode, StatusCode},
};
use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tracing::{info, warn};

type ClientTx = mpsc::UnboundedSender<Frame>;

pub enum BrokerEvent {
    Join {
        channel: String,
        client_id: String,
        alias: String,
        tx: ClientTx,
    },
    Leave {
        channel: String,
        client_id: String,
    },
    ChannelMsg {
        channel: String,
        sender_id: String,
        data: Bytes,
    },
    ChannelList {
        reply_tx: ClientTx,
        request_id: u64,
    },
    Members {
        channel: String,
        reply_tx: ClientTx,
        request_id: u64,
    },
    Subscribe {
        topic: String,
        client_id: String,
        tx: ClientTx,
    },
    Unsubscribe {
        topic: String,
        client_id: String,
    },
    Publish {
        topic: String,
        data: Bytes,
    },
    RegisterAction {
        method: String,
        client_id: String,
        tx: ClientTx,
    },
    UnregisterAction {
        method: String,
        client_id: String,
    },
    Call {
        request_id: u64,
        method: String,
        params: Bytes,
        caller_tx: ClientTx,
    },
    Response {
        broker_req_id: u64,
        result: Bytes,
    },
    Disconnect {
        client_id: String,
    },
}

struct Channel {
    members: Vec<(String, String, ClientTx)>,
}

impl Channel {
    fn new() -> Self {
        Self {
            members: Vec::new(),
        }
    }

    fn add(&mut self, client_id: String, alias: String, tx: ClientTx) {
        if !self.members.iter().any(|(id, _, _)| id == &client_id) {
            self.members.push((client_id, alias, tx));
        }
    }

    fn remove(&mut self, client_id: &str) {
        self.members.retain(|(id, _, _)| id != client_id);
    }

    fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    fn broadcast(&self, frame: Frame) {
        for (_, _, tx) in &self.members {
            let _ = tx.send(frame.clone());
        }
    }

    fn info(&self, name: &str) -> ChannelInfo {
        ChannelInfo {
            name: name.to_string(),
            member_count: self.members.len(),
        }
    }

    fn member_list(&self) -> Vec<MemberInfo> {
        self.members
            .iter()
            .map(|(id, alias, _)| MemberInfo {
                client_id: id.clone(),
                alias: alias.clone(),
            })
            .collect()
    }
}

struct PendingCall {
    caller_tx: ClientTx,
    original_request_id: u64,
}

pub async fn broker_loop(mut rx: UnboundedReceiver<BrokerEvent>) {
    let mut channels: HashMap<String, Channel> = HashMap::new();
    let mut topics: HashMap<String, Vec<(String, ClientTx)>> = HashMap::new();
    let mut actions: HashMap<String, (String, ClientTx)> = HashMap::new();
    let mut pending: HashMap<u64, PendingCall> = HashMap::new();
    let mut next_req_id: u64 = 1;

    while let Some(event) = rx.recv().await {
        match event {
            BrokerEvent::Join {
                channel,
                client_id,
                alias,
                tx,
            } => {
                let ch = channels
                    .entry(channel.clone())
                    .or_insert_with(Channel::new);
                ch.add(client_id.clone(), alias.clone(), tx);
                info!(
                    "CHANNEL '{}' ← joined by {} ({})",
                    channel, alias, client_id
                );
            }

            BrokerEvent::Leave { channel, client_id } => {
                if let Some(ch) = channels.get_mut(&channel) {
                    ch.remove(&client_id);
                    if ch.is_empty() {
                        channels.remove(&channel);
                        info!("CHANNEL '{}' removed (empty)", channel);
                    }
                }
            }

            BrokerEvent::ChannelMsg {
                channel,
                sender_id,
                data,
            } => {
                if let Some(ch) = channels.get(&channel) {
                    if ch.members.iter().any(|(id, _, _)| id == &sender_id) {
                        ch.broadcast(Frame {
                            opcode: Opcode::ChannelMsg,
                            flags: 0,
                            request_id: 0,
                            payload: data,
                        });
                    } else {
                        warn!(
                            "ChannelMsg: {} is not a member of '{}'",
                            sender_id, channel
                        );
                    }
                }
            }

            BrokerEvent::ChannelList {
                reply_tx,
                request_id,
            } => {
                let list: Vec<ChannelInfo> = channels
                    .iter()
                    .map(|(name, ch)| ch.info(name))
                    .collect();
                let _ = reply_tx.send(ok_frame(
                    request_id,
                    Opcode::ChannelList,
                    &ResponsePayload::channels(list),
                ));
            }

            BrokerEvent::Members {
                channel,
                reply_tx,
                request_id,
            } => {
                let list = channels
                    .get(&channel)
                    .map(|ch| ch.member_list())
                    .unwrap_or_default();
                let _ = reply_tx.send(ok_frame(
                    request_id,
                    Opcode::Members,
                    &ResponsePayload::members(list),
                ));
            }

            BrokerEvent::Subscribe {
                topic,
                client_id,
                tx,
            } => {
                topics.entry(topic).or_default().push((client_id, tx));
            }

            BrokerEvent::Unsubscribe { topic, client_id } => {
                if let Some(list) = topics.get_mut(&topic) {
                    list.retain(|(id, _)| id != &client_id);
                    if list.is_empty() {
                        topics.remove(&topic);
                    }
                }
            }

            BrokerEvent::Publish { topic, data } => {
                for (pattern, list) in &topics {
                    if crate::permissions::matches_pattern(pattern, &topic) || crate::permissions::matches_pattern(&topic, pattern) {
                        for (_, tx) in list {
                            let _ = tx.send(Frame {
                                opcode: Opcode::Publish,
                                flags: 0,
                                
                                request_id: 0,
                                payload: data.clone(),
                            });
                        }
                    }
                }
            }

            BrokerEvent::RegisterAction {
                method,
                client_id,
                tx,
            } => {
                info!("RPC '{}' registered by {}", method, client_id);
                actions.insert(method, (client_id, tx));
            }

            BrokerEvent::UnregisterAction { method, client_id } => {
                if actions
                    .get(&method)
                    .map_or(false, |(id, _)| id == &client_id)
                {
                    actions.remove(&method);
                }
            }

            // ✅ CORRIGIDO: Inclui o método no payload
            BrokerEvent::Call {
                request_id,
                method,
                params,
                caller_tx,
            } => match actions.get(&method) {
                Some((_, handler_tx)) => {
                    let brq = next_req_id;
                    next_req_id += 1;
                    pending.insert(
                        brq,
                        PendingCall {
                            caller_tx,
                            original_request_id: request_id,
                        },
                    );
                    
                    // ✅ ENVIA método + params no payload
                    let payload = serde_json::json!({
                        "method": method,
                        "params": serde_json::from_slice::<serde_json::Value>(&params)
                            .unwrap_or(serde_json::Value::Null),
                    });
                    let data = Bytes::from(serde_json::to_vec(&payload).unwrap_or_default());
                    
                    let fwd = Frame {
                        opcode: Opcode::Call,
                        flags: 0,
                        request_id: brq,
                        payload: data,  // ← Agora contém método + params
                    };
                    if handler_tx.send(fwd).is_err() {
                        if let Some(pc) = pending.remove(&brq) {
                            let _ = pc
                                .caller_tx
                                .send(error_frame(pc.original_request_id, "RPC handler disconnected"));
                        }
                    }
                }
                None => {
                    let _ = caller_tx.send(error_frame(
                        request_id,
                        &format!("no handler for '{method}'"),
                    ));
                }
            },

            BrokerEvent::Response {
                broker_req_id,
                result,
            } => {
                if let Some(pc) = pending.remove(&broker_req_id) {
                    let frame = Frame {
                        opcode: Opcode::Res,
                        flags: 0,
                        request_id: pc.original_request_id,
                        payload: result,
                    };
                    let _ = pc.caller_tx.send(frame);
                } else {
                    warn!("Response for unknown broker_req_id={broker_req_id}");
                }
            }

            BrokerEvent::Disconnect { client_id } => {
                for ch in channels.values_mut() {
                    ch.remove(&client_id);
                }
                channels.retain(|_, ch| !ch.is_empty());
                for list in topics.values_mut() {
                    list.retain(|(id, _)| id != &client_id);
                }
                topics.retain(|_, v| !v.is_empty());
                actions.retain(|_, (id, _)| id != &client_id);
                info!("Broker: client disconnected {client_id}");
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub name: String,
    pub member_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    pub client_id: String,
    pub alias: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelMsgPayload {
    pub channel: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinPayload {
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeavePayload {
    pub channel: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MembersPayload {
    pub channel: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribePayload {
    pub topic: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishPayload {
    pub topic: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActPayload {
    pub method: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CallPayload {
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResPayload {
    pub result: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ResponsePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<Vec<ChannelInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<MemberInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<crate::api_key::ApiKeyView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_keys: Option<Vec<crate::api_key::ApiKeyView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ResponsePayload {
    pub fn value(v: serde_json::Value) -> Self {
        Self {
            value: Some(v),
            ..Default::default()
        }
    }
    pub fn exists(e: bool) -> Self {
        Self {
            exists: Some(e),
            ..Default::default()
        }
    }
    pub fn count(c: usize) -> Self {
        Self {
            count: Some(c),
            ..Default::default()
        }
    }
    pub fn result(r: serde_json::Value) -> Self {
        Self {
            result: Some(r),
            ..Default::default()
        }
    }
    pub fn channels(c: Vec<ChannelInfo>) -> Self {
        Self {
            channels: Some(c),
            ..Default::default()
        }
    }
    pub fn members(m: Vec<MemberInfo>) -> Self {
        Self {
            members: Some(m),
            ..Default::default()
        }
    }
    pub fn api_key(k: crate::api_key::ApiKeyView) -> Self {
        Self {
            api_key: Some(k),
            ..Default::default()
        }
    }
    pub fn api_keys(k: Vec<crate::api_key::ApiKeyView>) -> Self {
        Self {
            api_keys: Some(k),
            ..Default::default()
        }
    }
    pub fn err(msg: String) -> Self {
        Self {
            error: Some(msg),
            ..Default::default()
        }
    }
}

fn ok_frame(request_id: u64, opcode: Opcode, resp: &ResponsePayload) -> Frame {
    let mut buf = BytesMut::new();
    buf.put_u8(StatusCode::Ok as u8);
    buf.extend_from_slice(&serde_json::to_vec(resp).unwrap_or_default());
    Frame {
        opcode,
        flags: 0,
        request_id,
        payload: buf.freeze(),
    }
}

pub fn error_frame(request_id: u64, msg: &str) -> Frame {
    let resp = ResponsePayload::err(msg.to_string());
    let mut buf = BytesMut::new();
    buf.put_u8(StatusCode::Error as u8);
    buf.extend_from_slice(&serde_json::to_vec(&resp).unwrap_or_default());
    Frame {
        opcode: Opcode::Error,
        flags: 0,
        request_id,
        payload: buf.freeze(),
    }
}

pub fn forbidden_frame(request_id: u64, msg: &str) -> Frame {
    let resp = ResponsePayload::err(msg.to_string());
    let mut buf = BytesMut::new();
    buf.put_u8(StatusCode::Forbidden as u8);
    buf.extend_from_slice(&serde_json::to_vec(&resp).unwrap_or_default());
    Frame {
        opcode: Opcode::Error,
        flags: 0,
        request_id,
        payload: buf.freeze(),
    }
}