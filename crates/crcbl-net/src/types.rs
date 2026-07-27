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

/// Opaque session token that survives transport drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

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
    fn entity_data_debug() {
        let ed = EntityData {
            entity_bits: 0xDEAD_BEEF,
            data: vec![1, 2, 3],
        };
        let s = format!("{ed:?}");
        assert!(s.contains("DEADBEEF"));
        assert!(s.contains("1, 2, 3"));
    }
}
