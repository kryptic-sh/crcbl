//! Audio event — the wire format for server→client sound triggers.
//!
//! An [`AudioEvent`] carries a sound id and its world-space position so the
//! client spatialiser can compute the correct cue grammar parameters.
//! It is payload of `crcbl_net::ServerToClient::Event`.

/// A spatial sound event sent from server to client.
///
/// Wire format (little-endian, 28 bytes):
///
/// ```text
///  0.. 4  sound_id    u32
///  4.. 8  x           f32
///  8..12  y           f32
/// 12..16  z           f32
/// 16..20  range       f32  (max audible distance, 0 = use default)
/// 20..24  volume      f32  (linear multiplier)
/// 24..28  pitch       f32  (pitch ratio, 1.0 = normal)
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioEvent {
    /// Sound asset identifier — look up in client's [`SoundBank`](crate::mixer::SoundBank).
    pub sound_id: u32,
    /// World-space position of the emitter.
    pub position: [f32; 3],
    /// Maximum audible distance (0 = use the cue grammar default).
    pub range: f32,
    /// Linear volume multiplier `[0, 1]`.
    pub volume: f32,
    /// Pitch ratio (1.0 = normal; > 1.0 higher/faster).
    pub pitch: f32,
}

impl AudioEvent {
    /// Wire size in bytes.
    pub const WIRE_SIZE: usize = 28;

    /// Encode to wire format (little-endian).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::WIRE_SIZE);
        buf.extend_from_slice(&self.sound_id.to_le_bytes());
        buf.extend_from_slice(&self.position[0].to_le_bytes());
        buf.extend_from_slice(&self.position[1].to_le_bytes());
        buf.extend_from_slice(&self.position[2].to_le_bytes());
        buf.extend_from_slice(&self.range.to_le_bytes());
        buf.extend_from_slice(&self.volume.to_le_bytes());
        buf.extend_from_slice(&self.pitch.to_le_bytes());
        buf
    }

    /// Decode from wire format. Returns `None` if `data` is too short.
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < Self::WIRE_SIZE {
            return None;
        }
        let sound_id = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let x = f32::from_le_bytes(data[4..8].try_into().unwrap());
        let y = f32::from_le_bytes(data[8..12].try_into().unwrap());
        let z = f32::from_le_bytes(data[12..16].try_into().unwrap());
        let range = f32::from_le_bytes(data[16..20].try_into().unwrap());
        let volume = f32::from_le_bytes(data[20..24].try_into().unwrap());
        let pitch = f32::from_le_bytes(data[24..28].try_into().unwrap());
        Some(Self {
            sound_id,
            position: [x, y, z],
            range,
            volume,
            pitch,
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let event = AudioEvent {
            sound_id: 42,
            position: [1.5, 2.5, -3.0],
            range: 100.0,
            volume: 0.75,
            pitch: 1.0,
        };
        let encoded = event.encode();
        assert_eq!(encoded.len(), AudioEvent::WIRE_SIZE);
        let decoded = AudioEvent::decode(&encoded).unwrap();
        assert_eq!(decoded.sound_id, event.sound_id);
        assert!((decoded.position[0] - event.position[0]).abs() < 1e-6);
        assert!((decoded.position[1] - event.position[1]).abs() < 1e-6);
        assert!((decoded.position[2] - event.position[2]).abs() < 1e-6);
        assert!((decoded.range - event.range).abs() < 1e-6);
        assert!((decoded.volume - event.volume).abs() < 1e-6);
        assert!((decoded.pitch - event.pitch).abs() < 1e-6);
    }

    #[test]
    fn decode_rejects_short_data() {
        assert!(AudioEvent::decode(&[0u8; 16]).is_none());
        assert!(AudioEvent::decode(&[0u8; 27]).is_none());
    }

    #[test]
    fn decode_accepts_longer_data() {
        let event = AudioEvent {
            sound_id: 1,
            position: [0.0; 3],
            range: 0.0,
            volume: 0.5,
            pitch: 1.0,
        };
        let mut encoded = event.encode();
        encoded.push(0xFF); // trailing garbage ignored
        let decoded = AudioEvent::decode(&encoded).unwrap();
        assert_eq!(decoded.sound_id, 1);
    }

    #[test]
    fn empty_position_event() {
        let event = AudioEvent {
            sound_id: 7,
            position: [0.0, 0.0, 0.0],
            range: 0.0,
            volume: 1.0,
            pitch: 1.0,
        };
        let encoded = event.encode();
        let decoded = AudioEvent::decode(&encoded).unwrap();
        assert_eq!(decoded, event);
    }
}
