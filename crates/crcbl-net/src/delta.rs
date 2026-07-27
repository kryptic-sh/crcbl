//! Delta-encoding stub — baseline snapshots for the session manager.
//!
//! This module currently provides [`Baseline`] and [`BaselineStore`] so
//! [`crate::session::SessionManager`] has a ring buffer of complete tick
//! snapshots to compute deltas against.
//!
//! The full delta encoding (Delta, SystemDelta, DeltaCodec) will be built
//! in a separate task.

use std::collections::{HashMap, VecDeque};

use crcbl_core::TickId;

use crate::messages::SystemSnapshot;

// ── Baseline ──────────────────────────────────────────────────────────────────

/// A complete baseline snapshot for one tick, storing every system's
/// per-entity component data.
#[derive(Debug, Clone)]
pub struct Baseline {
    pub tick: TickId,
    /// `system_id → (entity_bits → encoded component data)`.
    systems: HashMap<u32, HashMap<u64, Vec<u8>>>,
}

impl Baseline {
    /// Build a [`Baseline`] from a slice of [`SystemSnapshot`]s.
    ///
    /// Each system snapshot is assumed to carry a flat list of entities
    /// serialised as entity_bits (8 bytes LE) + len-prefixed data.
    /// This mapping is opaque to the baseline — it stores raw `Vec<u8>`
    /// per entity.
    pub fn from_snapshot(tick: TickId, systems: &[SystemSnapshot]) -> Self {
        let mut map: HashMap<u32, HashMap<u64, Vec<u8>>> = HashMap::new();

        for sys in systems {
            let entities = decode_entity_blobs(&sys.data);
            map.insert(sys.system_id, entities);
        }

        Self { tick, systems: map }
    }

    /// Number of systems present in this baseline.
    #[allow(dead_code)]
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Total entity count across all systems in this baseline.
    #[allow(dead_code)]
    pub fn entity_count(&self) -> usize {
        self.systems.values().map(|m| m.len()).sum()
    }
}

/// Decode a flat binary blob into `entity_bits → Vec<u8>`.
///
/// Format: repeated `(entity_bits: u64 LE, data_len: u32 LE, data: [u8; data_len])`.
fn decode_entity_blobs(blob: &[u8]) -> HashMap<u64, Vec<u8>> {
    let mut out = HashMap::new();
    let mut cursor = 0usize;

    while cursor + 12 <= blob.len() {
        // Read entity_bits (8 bytes LE).
        let bits_bytes: [u8; 8] = blob[cursor..cursor + 8].try_into().unwrap();
        let entity_bits = u64::from_le_bytes(bits_bytes);
        cursor += 8;

        // Read data_len (4 bytes LE).
        let len_bytes: [u8; 4] = blob[cursor..cursor + 4].try_into().unwrap();
        let data_len = u32::from_le_bytes(len_bytes) as usize;
        cursor += 4;

        if cursor + data_len > blob.len() {
            // Truncated blob — stop processing (in a real codec this would
            // be an error; for the stub we tolerate it).
            break;
        }

        let data = blob[cursor..cursor + data_len].to_vec();
        cursor += data_len;

        out.insert(entity_bits, data);
    }

    out
}

// ── BaselineStore ─────────────────────────────────────────────────────────────

/// Bounded ring buffer of [`Baseline`]s keyed by [`TickId`].
///
/// Older baselines are evicted once capacity is reached. The store answers
/// "is this tick too old to delta against?" efficiently by checking whether
/// the tick falls behind the oldest retained tick.
#[derive(Debug)]
pub struct BaselineStore {
    capacity: usize,
    ring: VecDeque<Baseline>,
}

impl BaselineStore {
    /// Create a new store with the given ring capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            ring: VecDeque::with_capacity(capacity),
        }
    }

    /// Insert a baseline. Evicts the oldest if at capacity.
    pub fn insert(&mut self, baseline: Baseline) {
        if self.ring.len() >= self.capacity {
            self.ring.pop_front();
        }
        self.ring.push_back(baseline);
    }

    /// Look up a baseline by tick id.
    pub fn get(&self, tick: TickId) -> Option<&Baseline> {
        self.ring.iter().find(|b| b.tick == tick)
    }

    /// Whether the given tick is older than the oldest retained baseline
    /// (i.e. a delta-encode from it is impossible).
    pub fn is_too_old(&self, tick: TickId) -> bool {
        match self.ring.front() {
            Some(oldest) => tick < oldest.tick,
            None => true, // No baselines stored — everything is "too old".
        }
    }

    /// The newest retained baseline, if any.
    pub fn newest(&self) -> Option<&Baseline> {
        self.ring.back()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(tick: TickId, system_id: u32, entities: &[(u64, &[u8])]) -> Baseline {
        // Build the binary blob manually: entity_bits LE + data_len LE + data.
        let mut blob = Vec::new();
        for &(bits, data) in entities {
            blob.extend_from_slice(&bits.to_le_bytes());
            blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
            blob.extend_from_slice(data);
        }

        let snap = SystemSnapshot {
            system_id,
            data: blob,
        };
        Baseline::from_snapshot(tick, &[snap])
    }

    // ── BaselineStore insert/get ──────────────────────────────────────────

    #[test]
    fn baseline_store_insert_and_get() {
        let mut store = BaselineStore::new(4);
        let b = make_snapshot(TickId::from_raw(10), 42, &[(1, b"hello")]);
        store.insert(b.clone());
        let found = store.get(TickId::from_raw(10));
        assert!(found.is_some());
        assert_eq!(found.unwrap().tick, TickId::from_raw(10));
    }

    #[test]
    fn baseline_store_get_missing() {
        let store = BaselineStore::new(4);
        assert!(store.get(TickId::from_raw(99)).is_none());
    }

    // ── Eviction ──────────────────────────────────────────────────────────

    #[test]
    fn baseline_store_eviction() {
        let mut store = BaselineStore::new(3);
        for i in 0..4 {
            let b = make_snapshot(TickId::from_raw(i), 1, &[(i, b"x")]);
            store.insert(b);
        }

        // Tick 0 should be evicted.
        assert!(store.is_too_old(TickId::from_raw(0)));
        assert!(store.get(TickId::from_raw(0)).is_none());

        // Ticks 1-3 should still be present.
        for i in 1..4 {
            assert!(!store.is_too_old(TickId::from_raw(i)));
            assert!(store.get(TickId::from_raw(i)).is_some());
        }
    }

    // ── is_too_old (empty store) ──────────────────────────────────────────

    #[test]
    fn baseline_store_empty_is_too_old() {
        let store = BaselineStore::new(4);
        assert!(store.is_too_old(TickId::from_raw(0)));
    }

    // ── newest ────────────────────────────────────────────────────────────

    #[test]
    fn baseline_store_newest() {
        let mut store = BaselineStore::new(4);
        assert!(store.newest().is_none());

        store.insert(make_snapshot(TickId::from_raw(5), 1, &[(1, b"a")]));
        assert_eq!(store.newest().unwrap().tick, TickId::from_raw(5));

        store.insert(make_snapshot(TickId::from_raw(8), 1, &[(2, b"b")]));
        assert_eq!(store.newest().unwrap().tick, TickId::from_raw(8));
    }

    // ── Baseline::from_snapshot ───────────────────────────────────────────

    #[test]
    fn baseline_from_snapshot() {
        let baseline = make_snapshot(
            TickId::from_raw(42),
            1,
            &[(100, b"data_a"), (200, b"data_b")],
        );
        assert_eq!(baseline.tick, TickId::from_raw(42));
        assert_eq!(baseline.system_count(), 1);
        assert_eq!(baseline.entity_count(), 2);
    }

    #[test]
    fn baseline_from_snapshot_multi_system() {
        let sys1 = SystemSnapshot {
            system_id: 10,
            data: {
                let mut d = Vec::new();
                d.extend_from_slice(&1u64.to_le_bytes());
                d.extend_from_slice(&3u32.to_le_bytes());
                d.extend_from_slice(b"abc");
                d
            },
        };
        let sys2 = SystemSnapshot {
            system_id: 20,
            data: {
                let mut d = Vec::new();
                d.extend_from_slice(&2u64.to_le_bytes());
                d.extend_from_slice(&2u32.to_le_bytes());
                d.extend_from_slice(b"xy");
                d.extend_from_slice(&3u64.to_le_bytes());
                d.extend_from_slice(&1u32.to_le_bytes());
                d.extend_from_slice(b"z");
                d
            },
        };

        let baseline = Baseline::from_snapshot(TickId::from_raw(7), &[sys1, sys2]);
        assert_eq!(baseline.system_count(), 2);
        assert_eq!(baseline.entity_count(), 3);
    }

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn debug_output() {
        let b = make_snapshot(TickId::from_raw(1), 42, &[(0, b"x")]);
        let _ = format!("{b:?}");

        let store = BaselineStore::new(2);
        let _ = format!("{store:?}");
    }
}
