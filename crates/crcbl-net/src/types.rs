//! Shared types for the protocol layer: sector identifiers, session tokens,
//! and entity encoding helpers.

/// Identifies a sector in the world grid. Three i64 coordinates.
///
/// MVP scenes occupy one sector; the envelope degenerates at zero cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SectorId {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

impl SectorId {
    /// The canonical single-sector for MVP (origin sector).
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };
}

/// Opaque routing identifier for a session. This is not a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// Cryptographically random credential required to resume a session.
///
/// Its `Debug` implementation deliberately redacts the secret, and equality
/// compares every byte so mismatches do not reveal a prefix length.
#[derive(Clone, Copy)]
pub struct ResumeToken([u8; 32]);

impl ResumeToken {
    /// Create a credential from its wire representation.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl PartialEq for ResumeToken {
    fn eq(&self, other: &Self) -> bool {
        let mut difference = 0u8;
        for (left, right) in self.0.iter().zip(other.0) {
            difference |= left ^ right;
        }
        difference == 0
    }
}

impl Eq for ResumeToken {}

impl std::fmt::Debug for ResumeToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ResumeToken([REDACTED])")
    }
}

/// Immutable identifiers which must agree before two peers exchange state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolCompatibility {
    /// Protocol version, incremented for breaking wire changes.
    pub protocol_version: u32,
    /// Engine and game build identifier supplied by the embedding application.
    pub engine_build_id: u64,
    /// Replicated schema identifier supplied by the embedding application.
    pub schema_hash: u64,
}

impl ProtocolCompatibility {
    /// Wire-format compatibility with placeholder embedding identifiers.
    ///
    /// Tests and protocol tooling may use this value directly. Networked
    /// embeddings must pass explicit, non-zero engine and schema identifiers to
    /// client and server constructors.
    pub const DEFAULT: Self = Self {
        protocol_version: 5,
        engine_build_id: 0,
        schema_hash: 0,
    };

    /// Assert that embedding-specific identifiers protect the handshake.
    ///
    /// # Panics
    ///
    /// Panics if either identifier is zero.
    pub fn assert_explicit(self) {
        assert!(
            self.engine_build_id != 0 && self.schema_hash != 0,
            "engine_build_id and schema_hash must be non-zero"
        );
    }
}

/// Entity identifier in packed form (generation + index from `crcbl_core::Pool`).
pub type EntityBits = u64;

/// A blob of per-entity component data for one system, tagged with the
/// entity it belongs to.
#[derive(Debug, Clone)]
pub struct EntityData {
    /// Packed entity identifier (generation + index).
    pub entity_bits: EntityBits,
    /// Serialised component data.
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use crate::handshake::HandshakeResult;
    use crcbl_core::TickId;

    use super::*;

    #[test]
    fn sector_id_zero_is_origin() {
        assert_eq!(SectorId::ZERO.x, 0);
        assert_eq!(SectorId::ZERO.y, 0);
        assert_eq!(SectorId::ZERO.z, 0);
    }

    #[test]
    fn sector_id_eq_and_hash() {
        use std::collections::HashSet;
        let a = SectorId { x: 1, y: 2, z: 3 };
        let b = SectorId { x: 1, y: 2, z: 3 };
        let c = SectorId { x: 0, y: 0, z: 0 };
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn session_id_equality() {
        let a = SessionId(42);
        let b = SessionId(42);
        let c = SessionId(7);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn resume_token_equality_checks_credentials() {
        let token = ResumeToken::from_bytes([0xA5; 32]);
        assert_eq!(token, token);
        assert_ne!(token, ResumeToken::from_bytes([0; 32]));
    }

    #[test]
    fn resume_token_redacts_debug_output() {
        let token = ResumeToken::from_bytes([0xA5; 32]);
        assert!(!format!("{token:?}").contains("165"));
    }

    #[test]
    fn accept_debug_redacts_resume_token() {
        let token = ResumeToken::from_bytes([0xA5; 32]);
        let result = HandshakeResult::Accept {
            generation: 1,
            session_id: SessionId(1),
            resume_token: token,
            server_tick: TickId::ZERO,
        };
        let debug = format!("{result:?}");
        assert!(!debug.contains("165"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn protocol_compatibility_default_is_explicit() {
        assert_eq!(ProtocolCompatibility::DEFAULT.protocol_version, 5);
    }

    #[test]
    #[should_panic(expected = "engine_build_id and schema_hash must be non-zero")]
    fn placeholder_compatibility_fails_assertion() {
        ProtocolCompatibility::DEFAULT.assert_explicit();
    }

    #[test]
    fn entity_data_debug() {
        let ed = EntityData {
            entity_bits: 0xDEAD_BEEF,
            data: vec![1, 2, 3],
        };
        let s = format!("{ed:?}");
        assert!(s.contains("3735928559"));
        assert!(s.contains("1, 2, 3"));
    }
}
