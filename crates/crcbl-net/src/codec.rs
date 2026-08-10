//! Hardened byte-level encode/decode for all wire protocol messages.
//!
//! Every decoder is length-gated: malformed input returns [`DecodeError`] and
//! no function panics on arbitrary byte slices.

use std::collections::HashSet;

use crate::handshake::{HandshakeResult, Hello, RejectReason};
use crate::messages::{ClientToServer, ServerToClient, SystemSnapshot};
use crate::types::{ResumeToken, SectorId, SessionId};
use crcbl_core::TickId;

// ── Error type ────────────────────────────────────────────────────────────────

/// All decode errors.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("payload too short: need {needed} bytes at offset {offset}, have {remaining}")]
    TooShort {
        needed: usize,
        offset: usize,
        remaining: usize,
    },
    #[error("unknown message tag: 0x{tag:02x}")]
    UnknownTag { tag: u8 },
    #[error("invalid data length: {0}")]
    InvalidLength(u32),
    #[error("trailing bytes after message: {0} bytes")]
    TrailingBytes(usize),
    #[error("duplicate system id: {0}")]
    DuplicateSystem(u32),
}

// ── Message tags ──────────────────────────────────────────────────────────────
//
// Every message this codec produces starts with one of these. A receiver
// dispatches on the tag rather than trial-decoding each type in turn: a trial
// decode makes "unknown message" indistinguishable from "malformed known
// message", and silently depends on no two decoders accepting the same bytes.

/// `ClientToServer::Input`.
pub const INPUT_TAG: u8 = 0x00;
/// `ClientToServer::Command`.
pub const COMMAND_TAG: u8 = 0x01;
/// `ServerToClient::Snapshot`.
pub const SNAPSHOT_TAG: u8 = 0x10;
/// `ServerToClient::Event`.
pub const EVENT_TAG: u8 = 0x11;
/// [`Hello`].
pub const HELLO_TAG: u8 = 0x20;
/// [`HandshakeResult::Accept`].
pub const ACCEPT_TAG: u8 = 0x21;
/// [`HandshakeResult::Reject`].
pub const REJECT_TAG: u8 = 0x22;
/// [`Ack`].
pub const ACK_TAG: u8 = 0x30;

/// Maximum accepted wire payload. Snapshots target a single UDP datagram.
pub const MAX_WIRE_PAYLOAD_BYTES: usize = 64 * 1024;
/// Largest individual opaque field accepted from the wire.
pub const MAX_FIELD_BYTES: usize = 60 * 1024;

fn validate_payload_size(payload: &[u8]) -> Result<(), DecodeError> {
    if payload.len() > MAX_WIRE_PAYLOAD_BYTES {
        return Err(DecodeError::InvalidLength(payload.len() as u32));
    }
    Ok(())
}

fn validate_field_len(len: usize) -> Result<(), DecodeError> {
    if len > MAX_FIELD_BYTES {
        return Err(DecodeError::InvalidLength(len as u32));
    }
    Ok(())
}

// ── ByteReader helper ─────────────────────────────────────────────────────────

/// Cursor over a wire payload that bounds-checks every read.
///
/// Every decoder in the crate reads through this: a hand-rolled
/// `if cursor + N > payload.len()` at each field is one more chance to get a
/// bound wrong, and there are dozens of fields.
pub(crate) struct ByteReader<'a> {
    buf: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.offset)
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, DecodeError> {
        if self.remaining() < 1 {
            return Err(DecodeError::TooShort {
                needed: 1,
                offset: self.offset,
                remaining: self.remaining(),
            });
        }
        let val = self.buf[self.offset];
        self.offset += 1;
        Ok(val)
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, DecodeError> {
        if self.remaining() < 2 {
            return Err(DecodeError::TooShort {
                needed: 2,
                offset: self.offset,
                remaining: self.remaining(),
            });
        }
        let bytes: [u8; 2] = self.buf[self.offset..self.offset + 2].try_into().unwrap();
        self.offset += 2;
        Ok(u16::from_le_bytes(bytes))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, DecodeError> {
        if self.remaining() < 4 {
            return Err(DecodeError::TooShort {
                needed: 4,
                offset: self.offset,
                remaining: self.remaining(),
            });
        }
        let bytes: [u8; 4] = self.buf[self.offset..self.offset + 4].try_into().unwrap();
        self.offset += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, DecodeError> {
        if self.remaining() < 8 {
            return Err(DecodeError::TooShort {
                needed: 8,
                offset: self.offset,
                remaining: self.remaining(),
            });
        }
        let bytes: [u8; 8] = self.buf[self.offset..self.offset + 8].try_into().unwrap();
        self.offset += 8;
        Ok(u64::from_le_bytes(bytes))
    }

    pub(crate) fn read_i64(&mut self) -> Result<i64, DecodeError> {
        if self.remaining() < 8 {
            return Err(DecodeError::TooShort {
                needed: 8,
                offset: self.offset,
                remaining: self.remaining(),
            });
        }
        let bytes: [u8; 8] = self.buf[self.offset..self.offset + 8].try_into().unwrap();
        self.offset += 8;
        Ok(i64::from_le_bytes(bytes))
    }

    pub(crate) fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining() < len {
            return Err(DecodeError::TooShort {
                needed: len,
                offset: self.offset,
                remaining: self.remaining(),
            });
        }
        let slice = &self.buf[self.offset..self.offset + len];
        self.offset += len;
        Ok(slice)
    }

    pub(crate) fn assert_empty(&self) -> Result<(), DecodeError> {
        let rem = self.remaining();
        if rem > 0 {
            Err(DecodeError::TrailingBytes(rem))
        } else {
            Ok(())
        }
    }
}

// ── Client → Server encode/decode ─────────────────────────────────────────────

/// Tag 0x00 = Input { tick: u64 LE, data_len: u32 LE, data: bytes }
/// Tag 0x01 = Command { data_len: u32 LE, data: bytes }
pub fn encode_client_to_server(msg: &ClientToServer) -> Vec<u8> {
    match msg {
        ClientToServer::Input { tick, data } => {
            let mut buf = Vec::with_capacity(1 + 8 + 4 + data.len());
            buf.push(INPUT_TAG);
            buf.extend_from_slice(&tick.get().to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
            buf
        }
        ClientToServer::Command { data } => {
            let mut buf = Vec::with_capacity(1 + 4 + data.len());
            buf.push(COMMAND_TAG);
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
            buf
        }
    }
}

pub fn decode_client_to_server(payload: &[u8]) -> Result<ClientToServer, DecodeError> {
    validate_payload_size(payload)?;
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    match tag {
        INPUT_TAG => {
            let tick_raw = r.read_u64()?;
            let data_len = r.read_u32()? as usize;
            validate_field_len(data_len)?;
            let data = r.read_bytes(data_len)?.to_vec();
            r.assert_empty()?;
            Ok(ClientToServer::Input {
                tick: TickId::from_raw(tick_raw),
                data,
            })
        }
        COMMAND_TAG => {
            let data_len = r.read_u32()? as usize;
            validate_field_len(data_len)?;
            let data = r.read_bytes(data_len)?.to_vec();
            r.assert_empty()?;
            Ok(ClientToServer::Command { data })
        }
        _ => Err(DecodeError::UnknownTag { tag }),
    }
}

// ── Server → Client encode/decode ─────────────────────────────────────────────

/// Tag 0x10 = Snapshot { sector: SectorId (3×i64 LE), tick: u64 LE,
///   system_count: u32 LE, then per-system: system_id: u32 LE,
///   data_len: u32 LE, data: bytes }
/// Tag 0x11 = Event { data_len: u32 LE, data: bytes }
pub fn encode_server_to_client(msg: &ServerToClient) -> Vec<u8> {
    match msg {
        ServerToClient::Snapshot {
            sector,
            tick,
            systems,
        } => {
            let mut cap = 1 + 24 + 8 + 4; // tag + sector + tick + system_count
            for sys in systems {
                cap += 4 + 4 + sys.data.len(); // system_id + data_len + data
            }
            let mut buf = Vec::with_capacity(cap);
            buf.push(SNAPSHOT_TAG);
            buf.extend_from_slice(&sector.x.to_le_bytes());
            buf.extend_from_slice(&sector.y.to_le_bytes());
            buf.extend_from_slice(&sector.z.to_le_bytes());
            buf.extend_from_slice(&tick.get().to_le_bytes());
            buf.extend_from_slice(&(systems.len() as u32).to_le_bytes());
            for sys in systems {
                buf.extend_from_slice(&sys.system_id.to_le_bytes());
                buf.extend_from_slice(&(sys.data.len() as u32).to_le_bytes());
                buf.extend_from_slice(&sys.data);
            }
            buf
        }
        ServerToClient::Event { data } => {
            let mut buf = Vec::with_capacity(1 + 4 + data.len());
            buf.push(EVENT_TAG);
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
            buf
        }
    }
}

pub fn decode_server_to_client(payload: &[u8]) -> Result<ServerToClient, DecodeError> {
    validate_payload_size(payload)?;
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    match tag {
        SNAPSHOT_TAG => {
            let sx = r.read_i64()?;
            let sy = r.read_i64()?;
            let sz = r.read_i64()?;
            let tick_raw = r.read_u64()?;
            let system_count = r.read_u32()? as usize;
            if system_count > r.remaining() / 8 {
                return Err(DecodeError::InvalidLength(system_count as u32));
            }
            let mut systems = Vec::with_capacity(system_count);
            let mut system_ids = HashSet::with_capacity(system_count);
            for _ in 0..system_count {
                let system_id = r.read_u32()?;
                if !system_ids.insert(system_id) {
                    return Err(DecodeError::DuplicateSystem(system_id));
                }
                let data_len = r.read_u32()? as usize;
                validate_field_len(data_len)?;
                let data = r.read_bytes(data_len)?.to_vec();
                systems.push(SystemSnapshot { system_id, data });
            }
            r.assert_empty()?;
            Ok(ServerToClient::Snapshot {
                sector: SectorId {
                    x: sx,
                    y: sy,
                    z: sz,
                },
                tick: TickId::from_raw(tick_raw),
                systems,
            })
        }
        EVENT_TAG => {
            let data_len = r.read_u32()? as usize;
            validate_field_len(data_len)?;
            let data = r.read_bytes(data_len)?.to_vec();
            r.assert_empty()?;
            Ok(ServerToClient::Event { data })
        }
        _ => Err(DecodeError::UnknownTag { tag }),
    }
}

// ── Handshake encode/decode ───────────────────────────────────────────────────

/// Tag 0x20 = Hello { protocol_version: u32 LE, engine_build_id: u64 LE,
///   schema_hash: u64 LE, generation: u64 LE, has_session: u8,
///   resume_token: [u8; 32] (if has_session) }
pub fn encode_hello(hello: &Hello) -> Vec<u8> {
    let has_session = hello.session_token.is_some();
    let mut buf = Vec::with_capacity(1 + 4 + 8 + 8 + 8 + 1 + usize::from(has_session) * 32);
    buf.push(HELLO_TAG);
    buf.extend_from_slice(&hello.protocol_version.to_le_bytes());
    buf.extend_from_slice(&hello.engine_build_id.to_le_bytes());
    buf.extend_from_slice(&hello.schema_hash.to_le_bytes());
    buf.extend_from_slice(&hello.generation.to_le_bytes());
    buf.push(if has_session { 1u8 } else { 0u8 });
    if let Some(token) = &hello.session_token {
        buf.extend_from_slice(token.as_bytes());
    }
    buf
}

pub fn decode_hello(payload: &[u8]) -> Result<Hello, DecodeError> {
    validate_payload_size(payload)?;
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    if tag != HELLO_TAG {
        return Err(DecodeError::UnknownTag { tag });
    }
    let protocol_version = r.read_u32()?;
    let engine_build_id = r.read_u64()?;
    let schema_hash = r.read_u64()?;
    let generation = r.read_u64()?;
    let has_session = r.read_u8()?;
    let session_token = match has_session {
        0 => None,
        1 => Some(ResumeToken::from_bytes(
            r.read_bytes(32)?.try_into().unwrap(),
        )),
        _ => return Err(DecodeError::InvalidLength(has_session.into())),
    };
    r.assert_empty()?;
    Ok(Hello {
        protocol_version,
        engine_build_id,
        schema_hash,
        generation,
        session_token,
    })
}

/// Tag 0x21 = Accept { generation: u64 LE, session_id: u64 LE, resume_token: [u8; 32], server_tick: u64 LE }
/// Tag 0x22 = Reject { generation: u64 LE, code: u8, msg_len: u16 LE, msg: UTF-8 bytes }
pub fn encode_handshake_result(result: &HandshakeResult) -> Vec<u8> {
    match result {
        HandshakeResult::Accept {
            generation,
            session_id,
            resume_token,
            server_tick,
        } => {
            let mut buf = Vec::with_capacity(1 + 8 + 8 + 32 + 8);
            buf.push(ACCEPT_TAG);
            buf.extend_from_slice(&generation.to_le_bytes());
            buf.extend_from_slice(&session_id.0.to_le_bytes());
            buf.extend_from_slice(resume_token.as_bytes());
            buf.extend_from_slice(&server_tick.get().to_le_bytes());
            buf
        }
        HandshakeResult::Reject { generation, reason } => {
            let msg_bytes = reason.msg.as_bytes();
            let mut buf = Vec::with_capacity(1 + 8 + 1 + 2 + msg_bytes.len());
            buf.push(REJECT_TAG);
            buf.extend_from_slice(&generation.to_le_bytes());
            buf.push(reason.code);
            buf.extend_from_slice(&(msg_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(msg_bytes);
            buf
        }
    }
}

pub fn decode_handshake_result(payload: &[u8]) -> Result<HandshakeResult, DecodeError> {
    validate_payload_size(payload)?;
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    match tag {
        ACCEPT_TAG => {
            let generation = r.read_u64()?;
            let raw_session = r.read_u64()?;
            let resume_token = ResumeToken::from_bytes(r.read_bytes(32)?.try_into().unwrap());
            let raw_tick = r.read_u64()?;
            r.assert_empty()?;
            Ok(HandshakeResult::Accept {
                generation,
                session_id: SessionId(raw_session),
                resume_token,
                server_tick: TickId::from_raw(raw_tick),
            })
        }
        REJECT_TAG => {
            let generation = r.read_u64()?;
            let code = r.read_u8()?;
            let msg_len = r.read_u16()? as usize;
            validate_field_len(msg_len)?;
            let msg_bytes = r.read_bytes(msg_len)?;
            let msg = String::from_utf8(msg_bytes.to_vec())
                .map_err(|_| DecodeError::InvalidLength(msg_len as u32))?;
            r.assert_empty()?;
            Ok(HandshakeResult::Reject {
                generation,
                reason: RejectReason { code, msg },
            })
        }
        _ => Err(DecodeError::UnknownTag { tag }),
    }
}

// ── Ack ───────────────────────────────────────────────────────────────────────

/// Tag 0x30 = Ack { sector: SectorId (3×i64 LE), tick: u64 LE }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ack {
    pub sector: SectorId,
    pub tick: TickId,
}

pub fn encode_ack(sector: SectorId, tick: TickId) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 24 + 8);
    buf.push(ACK_TAG);
    buf.extend_from_slice(&sector.x.to_le_bytes());
    buf.extend_from_slice(&sector.y.to_le_bytes());
    buf.extend_from_slice(&sector.z.to_le_bytes());
    buf.extend_from_slice(&tick.get().to_le_bytes());
    buf
}

pub fn decode_ack(payload: &[u8]) -> Result<Ack, DecodeError> {
    validate_payload_size(payload)?;
    let mut r = ByteReader::new(payload);
    let tag = r.read_u8()?;
    if tag != ACK_TAG {
        return Err(DecodeError::UnknownTag { tag });
    }
    let sector = SectorId {
        x: r.read_i64()?,
        y: r.read_i64()?,
        z: r.read_i64()?,
    };
    let tick = TickId::from_raw(r.read_u64()?);
    r.assert_empty()?;
    Ok(Ack { sector, tick })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Roundtrip tests ────────────────────────────────────────────────────

    #[test]
    fn roundtrip_client_to_server() {
        // Input
        let input = ClientToServer::Input {
            tick: TickId::from_raw(42),
            data: b"input_data".to_vec(),
        };
        let encoded = encode_client_to_server(&input);
        let decoded = decode_client_to_server(&encoded).unwrap();
        match decoded {
            ClientToServer::Input { tick, data } => {
                assert_eq!(tick.get(), 42);
                assert_eq!(data, b"input_data");
            }
            _ => panic!("expected Input"),
        }

        // Command
        let cmd = ClientToServer::Command {
            data: b"cmd_data".to_vec(),
        };
        let encoded = encode_client_to_server(&cmd);
        let decoded = decode_client_to_server(&encoded).unwrap();
        match decoded {
            ClientToServer::Command { data } => {
                assert_eq!(data, b"cmd_data");
            }
            _ => panic!("expected Command"),
        }
    }

    #[test]
    fn roundtrip_server_to_client() {
        // Snapshot (with multiple systems)
        let snap = ServerToClient::Snapshot {
            sector: SectorId { x: 1, y: -2, z: 3 },
            tick: TickId::from_raw(100),
            systems: vec![
                SystemSnapshot {
                    system_id: 7,
                    data: b"phys".to_vec(),
                },
                SystemSnapshot {
                    system_id: 8,
                    data: b"rend".to_vec(),
                },
            ],
        };
        let encoded = encode_server_to_client(&snap);
        let decoded = decode_server_to_client(&encoded).unwrap();
        match decoded {
            ServerToClient::Snapshot {
                sector,
                tick,
                systems,
            } => {
                assert_eq!(sector, SectorId { x: 1, y: -2, z: 3 });
                assert_eq!(tick.get(), 100);
                assert_eq!(systems.len(), 2);
                assert_eq!(systems[0].system_id, 7);
                assert_eq!(systems[0].data, b"phys");
                assert_eq!(systems[1].system_id, 8);
                assert_eq!(systems[1].data, b"rend");
            }
            _ => panic!("expected Snapshot"),
        }

        // Empty snapshot
        let snap_empty = ServerToClient::Snapshot {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(0),
            systems: vec![],
        };
        let encoded = encode_server_to_client(&snap_empty);
        let decoded = decode_server_to_client(&encoded).unwrap();
        match decoded {
            ServerToClient::Snapshot { systems, .. } => {
                assert!(systems.is_empty());
            }
            _ => panic!("expected Snapshot"),
        }

        // Event
        let event = ServerToClient::Event {
            data: b"evt".to_vec(),
        };
        let encoded = encode_server_to_client(&event);
        let decoded = decode_server_to_client(&encoded).unwrap();
        match decoded {
            ServerToClient::Event { data } => {
                assert_eq!(data, b"evt");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn a_hello_survives_a_roundtrip_with_or_without_a_session_token() {
        // With session token
        let hello = Hello {
            protocol_version: 1,
            engine_build_id: 0xCAFE_BABE,
            schema_hash: 0xDEAD_BEEF,
            generation: 1,
            session_token: Some(ResumeToken::from_bytes([0xA5; 32])),
        };
        let encoded = encode_hello(&hello);
        let decoded = decode_hello(&encoded).unwrap();
        assert_eq!(decoded.protocol_version, 1);
        assert_eq!(decoded.engine_build_id, 0xCAFE_BABE);
        assert_eq!(decoded.schema_hash, 0xDEAD_BEEF);
        assert_eq!(decoded.generation, 1);
        assert_eq!(
            decoded.session_token,
            Some(ResumeToken::from_bytes([0xA5; 32]))
        );

        // Without session token (fresh join)
        let hello_fresh = Hello {
            protocol_version: 2,
            engine_build_id: 0,
            schema_hash: 0,
            generation: 1,
            session_token: None,
        };
        let encoded = encode_hello(&hello_fresh);
        let decoded = decode_hello(&encoded).unwrap();
        assert_eq!(decoded.session_token, None);
    }

    #[test]
    fn an_accept_keeps_its_session_and_tick_and_a_reject_keeps_its_reason() {
        // Accept
        let accept = HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(999),
            resume_token: ResumeToken::from_bytes([0; 32]),
            server_tick: TickId::from_raw(5),
        };
        let encoded = encode_handshake_result(&accept);
        let decoded = decode_handshake_result(&encoded).unwrap();
        match decoded {
            HandshakeResult::Accept {
                generation,
                session_id,
                resume_token,
                server_tick,
                ..
            } => {
                assert_eq!(session_id, SessionId(999));
                assert_eq!(resume_token, ResumeToken::from_bytes([0; 32]));
                assert_eq!(server_tick.get(), 5);
                assert_eq!(generation, 1);
            }
            _ => panic!("expected Accept"),
        }

        // Reject
        let reject = HandshakeResult::Reject {
            generation: 1,
            reason: RejectReason {
                code: 0x01,
                msg: "version mismatch".to_string(),
            },
        };
        let encoded = encode_handshake_result(&reject);
        let decoded = decode_handshake_result(&encoded).unwrap();
        match decoded {
            HandshakeResult::Reject { generation, reason } => {
                assert_eq!(generation, 1);
                assert_eq!(reason.code, 0x01);
                assert_eq!(reason.msg, "version mismatch");
            }
            _ => panic!("expected Reject"),
        }
    }

    #[test]
    fn an_ack_roundtrips_with_its_negative_sector_coordinates_intact() {
        let sector = SectorId { x: -1, y: 2, z: 3 };
        let tick = TickId::from_raw(0xABCD);
        let encoded = encode_ack(sector, tick);
        let decoded = decode_ack(&encoded).unwrap();
        assert_eq!(decoded.sector, sector);
        assert_eq!(decoded.tick, tick);
    }

    #[test]
    fn ack_rejects_truncated_sector() {
        assert!(matches!(
            decode_ack(&[0x30; 24]),
            Err(DecodeError::TooShort { .. })
        ));
    }

    // ── Error tests ────────────────────────────────────────────────────────

    #[test]
    fn every_wire_decoder_errors_on_an_empty_payload() {
        let empty: &[u8] = &[];
        assert!(decode_client_to_server(empty).is_err());
        assert!(decode_server_to_client(empty).is_err());
        assert!(decode_hello(empty).is_err());
        assert!(decode_handshake_result(empty).is_err());
        assert!(decode_ack(empty).is_err());
    }

    #[test]
    fn an_unrecognised_tag_is_reported_as_unknown_with_the_byte_that_was_read() {
        let buf = [0xFFu8];
        assert!(matches!(
            decode_client_to_server(&buf),
            Err(DecodeError::UnknownTag { tag: 0xFF })
        ));
        assert!(matches!(
            decode_server_to_client(&buf),
            Err(DecodeError::UnknownTag { tag: 0xFF })
        ));
        assert!(matches!(
            decode_hello(&buf),
            Err(DecodeError::UnknownTag { tag: 0xFF })
        ));
        assert!(matches!(
            decode_handshake_result(&buf),
            Err(DecodeError::UnknownTag { tag: 0xFF })
        ));
        assert!(matches!(
            decode_ack(&buf),
            Err(DecodeError::UnknownTag { tag: 0xFF })
        ));
    }

    #[test]
    fn truncating_any_wire_message_yields_too_short_rather_than_a_partial_decode() {
        // Input truncated (tag + tick but no data_len)
        let mut input_enc = encode_client_to_server(&ClientToServer::Input {
            tick: TickId::from_raw(1),
            data: b"hello world!".to_vec(),
        });
        // Truncate to just tag + tick (9 bytes)
        input_enc.truncate(9);
        assert!(matches!(
            decode_client_to_server(&input_enc),
            Err(DecodeError::TooShort { .. })
        ));

        // Truncate to tag + tick + data_len but missing data
        input_enc.truncate(13);
        assert!(matches!(
            decode_client_to_server(&input_enc),
            Err(DecodeError::TooShort { .. })
        ));

        // Command truncated (tag only)
        let cmd_enc = encode_client_to_server(&ClientToServer::Command {
            data: b"test".to_vec(),
        });
        // Truncate to just the tag
        assert!(matches!(
            decode_client_to_server(&cmd_enc[..1]),
            Err(DecodeError::TooShort { .. })
        ));

        // Command tag + partial data_len
        assert!(matches!(
            decode_client_to_server(&cmd_enc[..3]),
            Err(DecodeError::TooShort { .. })
        ));

        // Snapshot truncated: tag + partial sector
        let snap_enc = encode_server_to_client(&ServerToClient::Snapshot {
            sector: SectorId { x: 1, y: 2, z: 3 },
            tick: TickId::from_raw(1),
            systems: vec![],
        });
        // Just tag + first 4 bytes of sector x
        assert!(matches!(
            decode_server_to_client(&snap_enc[..5]),
            Err(DecodeError::TooShort { .. })
        ));

        // Event truncated: tag + partial data_len
        let evt_enc = encode_server_to_client(&ServerToClient::Event {
            data: b"data".to_vec(),
        });
        assert!(matches!(
            decode_server_to_client(&evt_enc[..3]),
            Err(DecodeError::TooShort { .. })
        ));

        // Event tag + data_len but missing data
        assert!(matches!(
            decode_server_to_client(&evt_enc[..5]),
            Err(DecodeError::TooShort { .. })
        ));

        // Hello truncated: just tag
        assert!(matches!(
            decode_hello(&[0x20]),
            Err(DecodeError::TooShort { .. })
        ));

        // Hello truncated: tag + partial fields
        let hello_enc = encode_hello(&Hello {
            protocol_version: 1,
            engine_build_id: 2,
            schema_hash: 3,
            generation: 1,
            session_token: Some(ResumeToken::from_bytes([4; 32])),
        });
        assert!(matches!(
            decode_hello(&hello_enc[..5]),
            Err(DecodeError::TooShort { .. })
        ));

        // Handshake Accept truncated
        let accept_enc = encode_handshake_result(&HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(1),
            resume_token: ResumeToken::from_bytes([0; 32]),
            server_tick: TickId::from_raw(1),
        });
        assert!(matches!(
            decode_handshake_result(&accept_enc[..5]),
            Err(DecodeError::TooShort { .. })
        ));

        // Ack truncated
        assert!(matches!(
            decode_ack(&[0x30]),
            Err(DecodeError::TooShort { .. })
        ));
        assert!(matches!(
            decode_ack(&[0x30, 0x00, 0x00]),
            Err(DecodeError::TooShort { .. })
        ));
    }

    #[test]
    fn bytes_after_a_complete_message_are_refused_rather_than_ignored() {
        // ClientToServer with trailing bytes
        let mut enc = encode_client_to_server(&ClientToServer::Input {
            tick: TickId::from_raw(1),
            data: b"abc".to_vec(),
        });
        enc.extend_from_slice(b"extra_junk");
        assert!(matches!(
            decode_client_to_server(&enc),
            Err(DecodeError::TrailingBytes { .. })
        ));

        // ServerToClient with trailing bytes
        let mut enc = encode_server_to_client(&ServerToClient::Event {
            data: b"evt".to_vec(),
        });
        enc.push(0xFF);
        assert!(matches!(
            decode_server_to_client(&enc),
            Err(DecodeError::TrailingBytes { .. })
        ));

        // Hello with trailing bytes
        let mut enc = encode_hello(&Hello {
            protocol_version: 1,
            engine_build_id: 0,
            schema_hash: 0,
            generation: 1,
            session_token: None,
        });
        enc.push(0xAA);
        assert!(matches!(
            decode_hello(&enc),
            Err(DecodeError::TrailingBytes { .. })
        ));

        // HandshakeResult with trailing bytes
        let mut enc = encode_handshake_result(&HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(1),
            resume_token: ResumeToken::from_bytes([0; 32]),
            server_tick: TickId::from_raw(1),
        });
        enc.push(0xBB);
        assert!(matches!(
            decode_handshake_result(&enc),
            Err(DecodeError::TrailingBytes { .. })
        ));

        // Ack with trailing bytes
        let mut enc = encode_ack(SectorId::ZERO, TickId::from_raw(1));
        enc.push(0xCC);
        assert!(matches!(
            decode_ack(&enc),
            Err(DecodeError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn decode_rejects_huge_declared_lengths_and_counts() {
        let command = [0x01, 0xff, 0xff, 0xff, 0xff];
        assert!(matches!(
            decode_client_to_server(&command),
            Err(DecodeError::InvalidLength(_))
        ));

        let mut snapshot = vec![0x10];
        snapshot.extend_from_slice(&[0; 32]);
        snapshot.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_server_to_client(&snapshot),
            Err(DecodeError::InvalidLength(_))
        ));
    }

    #[test]
    fn decode_rejects_non_canonical_session_flag() {
        let mut payload = encode_hello(&Hello {
            protocol_version: 1,
            engine_build_id: 2,
            schema_hash: 3,
            generation: 1,
            session_token: None,
        });
        payload[29] = 2;
        assert!(matches!(
            decode_hello(&payload),
            Err(DecodeError::InvalidLength(2))
        ));
    }

    // ── Fuzz / panic tests ─────────────────────────────────────────────────

    #[test]
    fn fuzz_corpus_never_panics() {
        let corpus: &[&[u8]] = &[
            // Completely empty
            &[],
            // Single byte, unknown tag
            &[0xFF],
            // Valid tag but no body
            &[0x00],
            &[0x01],
            &[0x10],
            &[0x11],
            &[0x20],
            &[0x21],
            &[0x22],
            &[0x30],
            // Tag + partial data
            &[0x00, 0x01],
            &[0x00, 0x01, 0x02, 0x03],
            // Huge declared data length
            &[0x01, 0xFF, 0xFF, 0xFF, 0xFF],
            // Almost-valid but truncated
            &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
            // All zeros (valid enough structure but data_len 0)
            &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00,
            ],
            // Negative sector coordinates (high bytes set)
            &[0x10, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            // All-0xFF blob
            &[0xFF; 256],
        ];

        for &payload in corpus {
            let _ = decode_client_to_server(payload);
            let _ = decode_server_to_client(payload);
            let _ = decode_hello(payload);
            let _ = decode_handshake_result(payload);
            let _ = decode_ack(payload);
            let _ = crate::delta::decode_delta(payload, crate::delta::Trust::Untrusted);
            let _ = crate::delta::decode_delta(payload, crate::delta::Trust::Authenticated);
        }
    }

    #[test]
    fn fuzz_random_never_panics() {
        // Deterministic pseudo-random sequence seeded by "CRCBL\0\0\0".
        let mut state: u64 = 0x4352_4342_4c00_0000;
        for _ in 0..1000 {
            // Simple xorshift64
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;

            let len = (state % 128) as usize;
            let mut payload = Vec::with_capacity(len);
            let mut h = state;
            for _ in 0..len {
                h ^= h << 13;
                h ^= h >> 7;
                h ^= h << 17;
                payload.push(h as u8);
            }

            let _ = decode_client_to_server(&payload);
            let _ = decode_server_to_client(&payload);
            let _ = decode_hello(&payload);
            let _ = decode_handshake_result(&payload);
            let _ = decode_ack(&payload);
        }
    }

    // ── Error display ──────────────────────────────────────────────────────

    #[test]
    fn decode_server_snapshot_rejects_duplicate_system_ids() {
        let mut payload = vec![0x10];
        payload.extend_from_slice(&0i64.to_le_bytes());
        payload.extend_from_slice(&0i64.to_le_bytes());
        payload.extend_from_slice(&0i64.to_le_bytes());
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        for _ in 0..2 {
            payload.extend_from_slice(&7u32.to_le_bytes());
            payload.extend_from_slice(&0u32.to_le_bytes());
        }
        assert!(matches!(
            decode_server_to_client(&payload),
            Err(DecodeError::DuplicateSystem(7))
        ));
    }

    #[test]
    fn each_decode_error_prints_the_offsets_and_lengths_that_explain_it() {
        assert_eq!(
            format!(
                "{}",
                DecodeError::TooShort {
                    needed: 8,
                    offset: 1,
                    remaining: 3,
                }
            ),
            "payload too short: need 8 bytes at offset 1, have 3"
        );
        assert_eq!(
            format!("{}", DecodeError::UnknownTag { tag: 0xAB }),
            "unknown message tag: 0xab"
        );
        assert_eq!(
            format!("{}", DecodeError::InvalidLength(999)),
            "invalid data length: 999"
        );
        assert_eq!(
            format!("{}", DecodeError::TrailingBytes(5)),
            "trailing bytes after message: 5 bytes"
        );
    }

    /// [`Hello`] derives its `Debug`, so the redaction
    /// [`ResumeToken`](crate::types::ResumeToken) does for itself is the only
    /// thing between a logged handshake and a session credential in a log file
    /// — and a derive is exactly the kind of thing that gets replaced by a
    /// hand-written impl that spells the bytes out.
    ///
    /// `crate::types` asserts the same property for
    /// [`HandshakeResult::Accept`], the other message that carries one. This
    /// used to format four values and discard all four.
    #[test]
    fn a_hellos_debug_output_redacts_the_session_token_it_carries() {
        let hello = Hello {
            protocol_version: 1,
            engine_build_id: 2,
            schema_hash: 3,
            generation: 1,
            session_token: Some(ResumeToken::from_bytes([0xA5; 32])),
        };

        let debug = format!("{hello:?}");
        assert!(debug.contains("REDACTED"), "{debug}");
        assert!(
            !debug.contains("165"),
            "the token's bytes, in decimal: {debug}"
        );
        assert!(
            !debug.contains("0xA5"),
            "the token's bytes, in hex: {debug}"
        );
        // The fields that are not secret are the reason to log the line at all.
        for expected in [
            "protocol_version: 1",
            "engine_build_id: 2",
            "schema_hash: 3",
            "generation: 1",
        ] {
            assert!(
                debug.contains(expected),
                "{expected:?} missing from {debug}"
            );
        }
    }

    #[test]
    fn reject_reason_partial_eq() {
        let a = RejectReason {
            code: 1,
            msg: "a".into(),
        };
        let b = RejectReason {
            code: 1,
            msg: "a".into(),
        };
        let c = RejectReason {
            code: 2,
            msg: "a".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn handshake_result_partial_eq() {
        let a = HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(1),
            resume_token: ResumeToken::from_bytes([0; 32]),
            server_tick: TickId::from_raw(2),
        };
        let b = HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(1),
            resume_token: ResumeToken::from_bytes([0; 32]),
            server_tick: TickId::from_raw(2),
        };
        assert_eq!(a, b);

        let c = HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(2),
            resume_token: ResumeToken::from_bytes([0; 32]),
            server_tick: TickId::from_raw(2),
        };
        assert_ne!(a, c);
    }
}
