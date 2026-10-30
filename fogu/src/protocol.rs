use crate::errors::FoguError;
use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};

pub const MAGIC: [u8; 2] = [b'F', b'O'];
pub const VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    // Control
    Ping = 0x01,
    Pong = 0x02,
    Auth = 0x50,
    Error = 0xFF,

    // KV
    Get = 0x10,
    Set = 0x11,
    Del = 0x12,
    Exists = 0x13,
    Incr = 0x20,
    Decr = 0x21,

    // Channels (rooms)
    Join = 0x30,
    Leave = 0x31,
    ChannelMsg = 0x32,
    ChannelList = 0x33,
    Members = 0x34,

    // Event pub/sub
    Subscribe = 0x40,
    Publish = 0x41,

    // RPC
    Call = 0x60,
    Act = 0x61,
    Res = 0x62,

    // API Key Management (root only)
    ApiKeyCreate = 0x70,
    ApiKeyDelete = 0x71,
    ApiKeyList = 0x72,
    ApiKeyGet = 0x73,
}

impl TryFrom<u8> for Opcode {
    type Error = FoguError;
    fn try_from(v: u8) -> Result<Self, FoguError> {
        match v {
            0x01 => Ok(Opcode::Ping),
            0x02 => Ok(Opcode::Pong),
            0x50 => Ok(Opcode::Auth),
            0xFF => Ok(Opcode::Error),
            0x10 => Ok(Opcode::Get),
            0x11 => Ok(Opcode::Set),
            0x12 => Ok(Opcode::Del),
            0x13 => Ok(Opcode::Exists),
            0x20 => Ok(Opcode::Incr),
            0x21 => Ok(Opcode::Decr),
            0x30 => Ok(Opcode::Join),
            0x31 => Ok(Opcode::Leave),
            0x32 => Ok(Opcode::ChannelMsg),
            0x33 => Ok(Opcode::ChannelList),
            0x34 => Ok(Opcode::Members),
            0x40 => Ok(Opcode::Subscribe),
            0x41 => Ok(Opcode::Publish),
            0x60 => Ok(Opcode::Call),
            0x61 => Ok(Opcode::Act),
            0x62 => Ok(Opcode::Res),
            0x70 => Ok(Opcode::ApiKeyCreate),
            0x71 => Ok(Opcode::ApiKeyDelete),
            0x72 => Ok(Opcode::ApiKeyList),
            0x73 => Ok(Opcode::ApiKeyGet),
            _ => Err(FoguError::Protocol("invalid opcode")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StatusCode {
    Ok = 0x00,
    NotFound = 0x01,
    Error = 0x02,
    Invalid = 0x03,
    Unauthorized = 0x04,
    Forbidden = 0x05,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub opcode: Opcode,
    pub flags: u8,
    pub request_id: u64,
    pub payload: Bytes,
}