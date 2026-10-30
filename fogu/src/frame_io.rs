use crate::{
    errors::FoguError,
    protocol::{Frame, MAGIC, VERSION},
};
use bytes::{BufMut, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

// Wire format: magic(2) | version(1) | opcode(1) | flags(1) | request_id(8 BE) | payload_len(4 BE) | payload
pub async fn write_frame(w: &mut OwnedWriteHalf, f: &Frame) -> Result<(), FoguError> {
    let mut buf = BytesMut::with_capacity(17 + f.payload.len());
    buf.extend_from_slice(&MAGIC);
    buf.put_u8(VERSION);
    buf.put_u8(f.opcode as u8);
    buf.put_u8(f.flags);
    buf.extend_from_slice(&f.request_id.to_be_bytes());
    buf.put_u32(f.payload.len() as u32);
    buf.extend_from_slice(&f.payload);
    w.write_all(&buf).await?;
    Ok(())
}

pub async fn read_frame(r: &mut OwnedReadHalf) -> Result<Frame, FoguError> {
    let mut header = [0u8; 17];
    r.read_exact(&mut header).await?;

    if header[0] != MAGIC[0] || header[1] != MAGIC[1] {
        return Err(FoguError::Protocol("invalid magic"));
    }
    if header[2] != VERSION {
        return Err(FoguError::App(format!(
            "unsupported version {}",
            header[2]
        )));
    }

    let opcode = crate::protocol::Opcode::try_from(header[3])?;
    let flags = header[4];
    let request_id = u64::from_be_bytes(header[5..13].try_into().unwrap());
    let payload_len = u32::from_be_bytes(header[13..17].try_into().unwrap()) as usize;

    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        r.read_exact(&mut payload).await?;
    }
    Ok(Frame {
        opcode,
        flags,
        request_id,
        payload: bytes::Bytes::from(payload),
    })
}