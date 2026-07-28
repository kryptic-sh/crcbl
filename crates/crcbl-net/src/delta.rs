//! Delta-encoding — baseline snapshots and per-tick delta computation.
//!
//! This module provides:
//!
//! * [`Baseline`] / [`BaselineStore`] — complete tick snapshots with a ring buffer
//!   for the session manager.
//! * [`Delta`] / [`SystemDelta`] — per-system diff of current state against a
//!   known baseline, minimising per-tick bandwidth.
//! * [`DeltaCodec`] — computes deltas from snapshots and applies deltas back to
//!   baselines.
//! * [`encode_delta`] / [`decode_delta`] — binary wire format for delta messages.

use std::collections::{HashMap, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use crcbl_core::TickId;

use crate::messages::SystemSnapshot;
use crate::types::{EntityBits, EntityData};

/// Maximum accepted delta packet size. Deltas target a single UDP datagram.
pub const MAX_DELTA_BYTES: usize = 64 * 1024;
/// Largest individual component payload accepted from a delta packet.
pub const MAX_COMPONENT_BYTES: usize = 60 * 1024;

/// Errors returned while constructing a [`Baseline`] from snapshots.
#[derive(Debug, thiserror::Error)]
pub enum BaselineDecodeError {
    #[error("entity blob is truncated")]
    Truncated,
    #[error("entity blob has trailing bytes: {0}")]
    TrailingBytes(usize),
    #[error("entity blob exceeds packet limit: {0}")]
    BlobTooLarge(usize),
    #[error("duplicate entity id: {0}")]
    DuplicateEntity(u64),
    #[error("component data length exceeds limit: {0}")]
    ComponentTooLarge(u32),
}

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
    /// Build a [`Baseline`] from validated [`SystemSnapshot`] entity blobs.
    pub fn from_snapshot(
        tick: TickId,
        systems: &[SystemSnapshot],
    ) -> Result<Self, BaselineDecodeError> {
        let mut map: HashMap<u32, HashMap<u64, Vec<u8>>> = HashMap::new();

        for sys in systems {
            map.insert(sys.system_id, decode_entity_blobs(&sys.data)?);
        }

        Ok(Self { tick, systems: map })
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

    /// Number of entities in a specific system.
    pub fn entity_count_for(&self, system_id: u32) -> usize {
        self.systems.get(&system_id).map(|m| m.len()).unwrap_or(0)
    }

    /// Deterministic hash of the full baseline state.
    ///
    /// Hashes every `(system_id, entity_bits, entity_data)` tuple in
    /// deterministic order (systems and entities sorted by id), suitable for
    /// state-equality checks across replica boundaries.
    pub fn state_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        let mut sys_ids: Vec<u32> = self.systems.keys().copied().collect();
        sys_ids.sort();
        for &sys_id in &sys_ids {
            sys_id.hash(&mut hasher);
            let entities = &self.systems[&sys_id];
            let mut entity_ids: Vec<u64> = entities.keys().copied().collect();
            entity_ids.sort();
            for &eb in &entity_ids {
                eb.hash(&mut hasher);
                entities[&eb].hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

/// Decode a flat binary blob into `entity_bits → Vec<u8>`.
///
/// Format: repeated `(entity_bits: u64 LE, data_len: u32 LE, data: [u8; data_len])`.
fn decode_entity_blobs(blob: &[u8]) -> Result<HashMap<u64, Vec<u8>>, BaselineDecodeError> {
    if blob.len() > MAX_DELTA_BYTES {
        return Err(BaselineDecodeError::BlobTooLarge(blob.len()));
    }
    let mut out = HashMap::new();
    let mut cursor = 0usize;

    while cursor < blob.len() {
        if blob.len() - cursor < 12 {
            return Err(BaselineDecodeError::TrailingBytes(blob.len() - cursor));
        }
        let bits_bytes: [u8; 8] = blob[cursor..cursor + 8].try_into().unwrap();
        let entity_bits = u64::from_le_bytes(bits_bytes);
        cursor += 8;
        let len_bytes: [u8; 4] = blob[cursor..cursor + 4].try_into().unwrap();
        let data_len = u32::from_le_bytes(len_bytes);
        cursor += 4;
        let data_len = data_len as usize;

        if data_len > MAX_COMPONENT_BYTES {
            return Err(BaselineDecodeError::ComponentTooLarge(data_len as u32));
        }
        if data_len > blob.len() - cursor {
            return Err(BaselineDecodeError::Truncated);
        }
        if out.contains_key(&entity_bits) {
            return Err(BaselineDecodeError::DuplicateEntity(entity_bits));
        }

        let data = blob[cursor..cursor + data_len].to_vec();
        cursor += data_len;
        out.insert(entity_bits, data);
    }

    Ok(out)
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

// ── hash_encoded ──────────────────────────────────────────────────────────────

/// Hash serialised component bytes for change detection.
///
/// Uses [`DefaultHasher`] — adequate for identity checks; not
/// cryptographically strong.
pub fn hash_encoded(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

// ── SystemDelta ───────────────────────────────────────────────────────────────

/// Per-system result of diffing current state against a baseline.
#[derive(Debug, Clone)]
pub struct SystemDelta {
    pub system_id: u32,
    /// Entities present in current but absent from baseline (or keyframe: all entities).
    pub added: Vec<EntityData>,
    /// Entity bits present in baseline but absent from current (tombstones).
    pub removed: Vec<EntityBits>,
    /// Entities present in both whose encoded bytes differ.
    pub modified: Vec<EntityData>,
    /// Count of unchanged entities (not sent on wire, informational).
    pub unchanged_count: u32,
}

// ── Delta ─────────────────────────────────────────────────────────────────────

/// A delta-encoded snapshot against a known baseline.
#[derive(Debug, Clone)]
pub struct Delta {
    pub tick: TickId,
    pub baseline_tick: Option<TickId>,
    pub is_keyframe: bool,
    pub systems: Vec<SystemDelta>,
}

// ── DeltaCodec ────────────────────────────────────────────────────────────────

/// Encodes snapshots as deltas and applies deltas back to baselines.
#[derive(Debug)]
pub struct DeltaCodec;

impl DeltaCodec {
    /// Compute a delta from `current` snapshots vs an optional `baseline`.
    ///
    /// - `baseline = None` → keyframe (all entities in `added`, `is_keyframe = true`)
    /// - `baseline` present → per-system diff:
    ///   * entities in current not in baseline → `added`
    ///   * entities in baseline not in current → `removed`
    ///   * entities in both, encoded bytes differ → `modified`
    ///   * entities in both, identical bytes → `unchanged_count++`
    pub fn encode(tick: TickId, current: &[SystemSnapshot], baseline: Option<&Baseline>) -> Delta {
        if baseline.is_none() {
            return Self::encode_keyframe(tick, current);
        }

        let baseline = baseline.unwrap();
        let mut systems = Vec::new();
        let mut seen_system_ids: Vec<u32> = Vec::new();

        for snap in current {
            let current_entities = decode_entity_blobs(&snap.data)
                .expect("DeltaCodec only encodes validated system snapshots");
            let baseline_entities = baseline.systems.get(&snap.system_id);

            seen_system_ids.push(snap.system_id);

            let mut added = Vec::new();
            let mut removed = Vec::new();
            let mut modified = Vec::new();
            let mut unchanged = 0u32;

            for (&eb, data) in &current_entities {
                match baseline_entities.and_then(|m| m.get(&eb)) {
                    None => {
                        added.push(EntityData {
                            entity_bits: eb,
                            data: data.clone(),
                        });
                    }
                    Some(baseline_data) => {
                        if hash_encoded(data) != hash_encoded(baseline_data) {
                            modified.push(EntityData {
                                entity_bits: eb,
                                data: data.clone(),
                            });
                        } else {
                            unchanged += 1;
                        }
                    }
                }
            }

            // Entities in baseline not in current → removed.
            if let Some(be) = baseline_entities {
                for &eb in be.keys() {
                    if !current_entities.contains_key(&eb) {
                        removed.push(eb);
                    }
                }
            }

            systems.push(SystemDelta {
                system_id: snap.system_id,
                added,
                removed,
                modified,
                unchanged_count: unchanged,
            });
        }

        // Systems present in baseline but absent from current → all removed.
        for (&sys_id, baseline_entities) in &baseline.systems {
            if seen_system_ids.contains(&sys_id) {
                continue;
            }
            let removed: Vec<EntityBits> = baseline_entities.keys().copied().collect();
            if !removed.is_empty() {
                systems.push(SystemDelta {
                    system_id: sys_id,
                    added: Vec::new(),
                    removed,
                    modified: Vec::new(),
                    unchanged_count: 0,
                });
            }
        }

        Delta {
            tick,
            baseline_tick: Some(baseline.tick),
            is_keyframe: false,
            systems,
        }
    }

    /// Keyframe path: every entity in every system goes into `added`.
    fn encode_keyframe(tick: TickId, current: &[SystemSnapshot]) -> Delta {
        let systems = current
            .iter()
            .map(|snap| {
                let entities = decode_entity_blobs(&snap.data)
                    .expect("DeltaCodec only encodes validated system snapshots");
                let added: Vec<EntityData> = entities
                    .into_iter()
                    .map(|(eb, data)| EntityData {
                        entity_bits: eb,
                        data,
                    })
                    .collect();
                SystemDelta {
                    system_id: snap.system_id,
                    added,
                    removed: Vec::new(),
                    modified: Vec::new(),
                    unchanged_count: 0,
                }
            })
            .collect();

        Delta {
            tick,
            baseline_tick: None,
            is_keyframe: true,
            systems,
        }
    }

    /// Apply a delta to a mutable baseline, returning the reconstructed full
    /// snapshot.
    ///
    /// On keyframe, replaces the entire baseline.
    pub fn apply(delta: &Delta, baseline: &mut Baseline) -> Vec<SystemSnapshot> {
        if delta.is_keyframe {
            // Replace entire baseline.
            baseline.tick = delta.tick;
            baseline.systems.clear();
            for sys_delta in &delta.systems {
                let mut entities = HashMap::new();
                for ed in &sys_delta.added {
                    entities.insert(ed.entity_bits, ed.data.clone());
                }
                baseline.systems.insert(sys_delta.system_id, entities);
            }
        } else {
            baseline.tick = delta.tick;
            for sys_delta in &delta.systems {
                let system_entities = baseline.systems.entry(sys_delta.system_id).or_default();

                // Apply additions.
                for ed in &sys_delta.added {
                    system_entities.insert(ed.entity_bits, ed.data.clone());
                }

                // Apply removals.
                for &eb in &sys_delta.removed {
                    system_entities.remove(&eb);
                }

                // Apply modifications.
                for ed in &sys_delta.modified {
                    system_entities.insert(ed.entity_bits, ed.data.clone());
                }
            }
        }

        baseline_to_snapshots(baseline)
    }
}

/// Reconstruct [`SystemSnapshot`]s from a [`Baseline`]'s internal maps.
fn baseline_to_snapshots(baseline: &Baseline) -> Vec<SystemSnapshot> {
    baseline
        .systems
        .iter()
        .map(|(&sys_id, entities)| {
            let mut blob = Vec::new();
            for (&eb, data) in entities {
                blob.extend_from_slice(&eb.to_le_bytes());
                blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
                blob.extend_from_slice(data);
            }
            SystemSnapshot {
                system_id: sys_id,
                data: blob,
            }
        })
        .collect()
}

// ── Wire encoding ─────────────────────────────────────────────────────────────

/// Encode a [`Delta`] into the wire format.
///
/// Wire format (all LE):
///
/// ```text
/// tick:            u64
/// baseline_tick:   u64 (None = 0)
/// is_keyframe:     u8
/// system_count:    u32
/// Per system:
///   system_id:       u32
///   added_count:     u32
///   removed_count:   u32
///   modified_count:  u32
///   Per added entity:    entity_bits u64, data_len u32, data bytes
///   Per removed entity:  entity_bits u64
///   Per modified entity: entity_bits u64, data_len u32, data bytes
/// ```
pub fn encode_delta(delta: &Delta) -> Vec<u8> {
    let mut buf = Vec::new();

    // tick
    buf.extend_from_slice(&delta.tick.get().to_le_bytes());

    // baseline_tick (None = 0)
    let baseline_tick = delta.baseline_tick.map(|t| t.get()).unwrap_or(0);
    buf.extend_from_slice(&baseline_tick.to_le_bytes());

    // is_keyframe
    buf.push(delta.is_keyframe as u8);

    // system_count
    buf.extend_from_slice(&(delta.systems.len() as u32).to_le_bytes());

    for sys in &delta.systems {
        // system_id
        buf.extend_from_slice(&sys.system_id.to_le_bytes());
        // added_count
        buf.extend_from_slice(&(sys.added.len() as u32).to_le_bytes());
        // removed_count
        buf.extend_from_slice(&(sys.removed.len() as u32).to_le_bytes());
        // modified_count
        buf.extend_from_slice(&(sys.modified.len() as u32).to_le_bytes());

        // Added entities
        for ed in &sys.added {
            buf.extend_from_slice(&ed.entity_bits.to_le_bytes());
            buf.extend_from_slice(&(ed.data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&ed.data);
        }

        // Removed entities (entity_bits only)
        for &eb in &sys.removed {
            buf.extend_from_slice(&eb.to_le_bytes());
        }

        // Modified entities
        for ed in &sys.modified {
            buf.extend_from_slice(&ed.entity_bits.to_le_bytes());
            buf.extend_from_slice(&(ed.data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&ed.data);
        }
    }

    buf
}

/// Decode a [`Delta`] from the wire format (see [`encode_delta`]).
pub fn decode_delta(payload: &[u8]) -> Result<Delta, DeltaDecodeError> {
    if payload.len() > MAX_DELTA_BYTES {
        return Err(DeltaDecodeError::InvalidLength(payload.len() as u32));
    }
    let mut cursor = 0usize;

    // tick: u64 LE
    if cursor + 8 > payload.len() {
        return Err(DeltaDecodeError::TooShort);
    }
    let tick_raw = u64::from_le_bytes(payload[cursor..cursor + 8].try_into().unwrap());
    let tick = TickId::from_raw(tick_raw);
    cursor += 8;

    // baseline_tick: u64 LE (None = 0)
    if cursor + 8 > payload.len() {
        return Err(DeltaDecodeError::TooShort);
    }
    let baseline_tick_raw = u64::from_le_bytes(payload[cursor..cursor + 8].try_into().unwrap());
    cursor += 8;
    let baseline_tick = if baseline_tick_raw == 0 {
        None
    } else {
        Some(TickId::from_raw(baseline_tick_raw))
    };

    // is_keyframe: u8
    if cursor + 1 > payload.len() {
        return Err(DeltaDecodeError::TooShort);
    }
    let is_keyframe = match payload[cursor] {
        0 => false,
        1 => true,
        flag => return Err(DeltaDecodeError::InvalidFlag(flag)),
    };
    cursor += 1;

    if is_keyframe != baseline_tick.is_none() {
        return Err(DeltaDecodeError::InvalidMetadata);
    }

    // system_count: u32 LE
    if cursor + 4 > payload.len() {
        return Err(DeltaDecodeError::TooShort);
    }
    let system_count = u32::from_le_bytes(payload[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4;
    if system_count > (payload.len() - cursor) / 16 {
        return Err(DeltaDecodeError::InvalidLength(system_count as u32));
    }

    let mut systems = Vec::with_capacity(system_count);

    for _ in 0..system_count {
        // system_id: u32 LE
        if cursor + 4 > payload.len() {
            return Err(DeltaDecodeError::TooShort);
        }
        let system_id = u32::from_le_bytes(payload[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;

        // added_count: u32 LE
        if cursor + 4 > payload.len() {
            return Err(DeltaDecodeError::TooShort);
        }
        let added_count =
            u32::from_le_bytes(payload[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;

        // removed_count: u32 LE
        if cursor + 4 > payload.len() {
            return Err(DeltaDecodeError::TooShort);
        }
        let removed_count =
            u32::from_le_bytes(payload[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;

        // modified_count: u32 LE
        if cursor + 4 > payload.len() {
            return Err(DeltaDecodeError::TooShort);
        }
        let modified_count =
            u32::from_le_bytes(payload[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;

        let minimum_entity_bytes = added_count
            .checked_mul(12)
            .and_then(|bytes| bytes.checked_add(removed_count.checked_mul(8)?))
            .and_then(|bytes| bytes.checked_add(modified_count.checked_mul(12)?))
            .ok_or(DeltaDecodeError::InvalidLength(u32::MAX))?;
        if minimum_entity_bytes > payload.len() - cursor {
            return Err(DeltaDecodeError::InvalidLength(minimum_entity_bytes as u32));
        }

        // Read added entities
        let mut added = Vec::with_capacity(added_count);
        for _ in 0..added_count {
            if cursor + 12 > payload.len() {
                return Err(DeltaDecodeError::TooShort);
            }
            let entity_bits = u64::from_le_bytes(payload[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let data_len =
                u32::from_le_bytes(payload[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            if data_len > MAX_COMPONENT_BYTES || data_len > payload.len() - cursor {
                return Err(DeltaDecodeError::InvalidLength(data_len as u32));
            }
            let data = payload[cursor..cursor + data_len].to_vec();
            cursor += data_len;
            added.push(EntityData { entity_bits, data });
        }

        // Read removed entities (entity_bits only)
        let mut removed = Vec::with_capacity(removed_count);
        for _ in 0..removed_count {
            if cursor + 8 > payload.len() {
                return Err(DeltaDecodeError::TooShort);
            }
            let entity_bits = u64::from_le_bytes(payload[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            removed.push(entity_bits);
        }

        // Read modified entities
        let mut modified = Vec::with_capacity(modified_count);
        for _ in 0..modified_count {
            if cursor + 12 > payload.len() {
                return Err(DeltaDecodeError::TooShort);
            }
            let entity_bits = u64::from_le_bytes(payload[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let data_len =
                u32::from_le_bytes(payload[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            if data_len > MAX_COMPONENT_BYTES || data_len > payload.len() - cursor {
                return Err(DeltaDecodeError::InvalidLength(data_len as u32));
            }
            let data = payload[cursor..cursor + data_len].to_vec();
            cursor += data_len;
            modified.push(EntityData { entity_bits, data });
        }

        systems.push(SystemDelta {
            system_id,
            added,
            removed,
            modified,
            unchanged_count: 0, // Not serialised on wire.
        });
    }

    // Trailing bytes check.
    if cursor < payload.len() {
        return Err(DeltaDecodeError::TrailingBytes(payload.len() - cursor));
    }

    Ok(Delta {
        tick,
        baseline_tick,
        is_keyframe,
        systems,
    })
}

// ── DeltaDecodeError ──────────────────────────────────────────────────────────

/// Errors returned by [`decode_delta`].
#[derive(Debug, thiserror::Error)]
pub enum DeltaDecodeError {
    #[error("payload too short")]
    TooShort,
    #[error("invalid data length: {0}")]
    InvalidLength(u32),
    #[error("invalid keyframe flag: {0}")]
    InvalidFlag(u8),
    #[error("inconsistent keyframe and baseline metadata")]
    InvalidMetadata,
    #[error("trailing bytes: {0}")]
    TrailingBytes(usize),
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ────────────────────────────────────────────────────────────

    /// Build a Baseline from a list of (system_id, entity_bits, data) tuples.
    fn make_snapshot(tick: TickId, system_id: u32, entities: &[(u64, &[u8])]) -> Baseline {
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
        Baseline::from_snapshot(tick, &[snap]).expect("valid test snapshot")
    }

    /// Build a `Vec<SystemSnapshot>` from a list of (system_id, entities).
    #[allow(clippy::type_complexity)]
    fn make_snapshots(systems: &[(u32, &[(u64, &[u8])])]) -> Vec<SystemSnapshot> {
        systems
            .iter()
            .map(|&(sys_id, entities)| {
                let mut blob = Vec::new();
                for &(bits, data) in entities {
                    blob.extend_from_slice(&bits.to_le_bytes());
                    blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
                    blob.extend_from_slice(data);
                }
                SystemSnapshot {
                    system_id: sys_id,
                    data: blob,
                }
            })
            .collect()
    }

    /// Extract all entity data from a slice of SystemSnapshot, grouped by system.
    fn snapshot_entities(snapshots: &[SystemSnapshot]) -> HashMap<u32, HashMap<u64, Vec<u8>>> {
        let mut out = HashMap::new();
        for snap in snapshots {
            out.insert(
                snap.system_id,
                decode_entity_blobs(&snap.data).expect("valid test snapshot"),
            );
        }
        out
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

        let baseline = Baseline::from_snapshot(TickId::from_raw(7), &[sys1, sys2])
            .expect("valid test snapshots");
        assert_eq!(baseline.system_count(), 2);
        assert_eq!(baseline.entity_count(), 3);
    }

    // ── Baseline::entity_count_for ────────────────────────────────────────

    #[test]
    fn baseline_state_hash_deterministic() {
        let b1 = make_snapshot(TickId::from_raw(1), 1, &[(10, b"pos"), (20, b"vel")]);
        let b2 = make_snapshot(TickId::from_raw(2), 1, &[(10, b"pos"), (20, b"vel")]);
        // Same data, different tick → same state hash.
        assert_eq!(b1.state_hash(), b2.state_hash());
    }

    #[test]
    fn baseline_state_hash_detects_data_difference() {
        let b1 = make_snapshot(TickId::from_raw(1), 1, &[(10, b"pos")]);
        let b2 = make_snapshot(TickId::from_raw(1), 1, &[(10, b"vel")]);
        assert_ne!(b1.state_hash(), b2.state_hash());
    }

    #[test]
    fn baseline_state_hash_detects_entity_count_difference() {
        let b1 = make_snapshot(TickId::from_raw(1), 1, &[(10, b"a")]);
        let b2 = make_snapshot(TickId::from_raw(1), 1, &[(10, b"a"), (20, b"b")]);
        assert_ne!(b1.state_hash(), b2.state_hash());
    }

    #[test]
    fn baseline_state_hash_detects_system_id_difference() {
        let b1 = make_snapshot(TickId::from_raw(1), 1, &[(10, b"a")]);
        let b2 = make_snapshot(TickId::from_raw(1), 2, &[(10, b"a")]);
        assert_ne!(b1.state_hash(), b2.state_hash());
    }

    #[test]
    fn baseline_entity_count_for() {
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
                d
            },
        };

        let baseline = Baseline::from_snapshot(TickId::from_raw(7), &[sys1, sys2])
            .expect("valid test snapshots");
        assert_eq!(baseline.entity_count_for(10), 1);
        assert_eq!(baseline.entity_count_for(20), 1);
        assert_eq!(baseline.entity_count_for(99), 0);
    }

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn debug_output() {
        let b = make_snapshot(TickId::from_raw(1), 42, &[(0, b"x")]);
        let _ = format!("{b:?}");

        let store = BaselineStore::new(2);
        let _ = format!("{store:?}");
    }

    // ── hash_encoded ──────────────────────────────────────────────────────

    #[test]
    fn hash_encoded_same_input_same_hash() {
        let a = hash_encoded(b"hello");
        let b = hash_encoded(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_encoded_different_input_different_hash() {
        let a = hash_encoded(b"hello");
        let b = hash_encoded(b"world");
        assert_ne!(a, b);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // DeltaCodec tests
    // ═══════════════════════════════════════════════════════════════════════

    /// 1. Encode without baseline → keyframe, all entities in `added`.
    #[test]
    fn empty_baseline_produces_keyframe() {
        let tick = TickId::from_raw(1);
        let snaps = make_snapshots(&[(1, &[(10, b"pos"), (20, b"vel")])]);

        let delta = DeltaCodec::encode(tick, &snaps, None);

        assert!(delta.is_keyframe);
        assert_eq!(delta.baseline_tick, None);
        assert_eq!(delta.systems.len(), 1);
        assert_eq!(delta.systems[0].added.len(), 2);
        assert_eq!(delta.systems[0].removed.len(), 0);
        assert_eq!(delta.systems[0].modified.len(), 0);
        assert_eq!(delta.systems[0].unchanged_count, 0);
    }

    /// 2. Same snapshot twice → no added/removed/modified, unchanged_count=N.
    #[test]
    fn same_state_produces_empty_delta() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);
        let snaps = make_snapshots(&[(1, &[(10, b"pos"), (20, b"vel")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps, Some(&baseline));

        assert!(!delta.is_keyframe);
        assert_eq!(delta.baseline_tick, Some(tick_a));
        assert_eq!(delta.systems.len(), 1);
        assert_eq!(delta.systems[0].added.len(), 0);
        assert_eq!(delta.systems[0].removed.len(), 0);
        assert_eq!(delta.systems[0].modified.len(), 0);
        assert_eq!(delta.systems[0].unchanged_count, 2);
    }

    /// 3. Server adds entity → delta.added has 1 entry.
    #[test]
    fn add_one_entity() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"pos")])]);
        let snaps_b = make_snapshots(&[(1, &[(10, b"pos"), (20, b"vel")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline));

        assert!(!delta.is_keyframe);
        assert_eq!(delta.systems.len(), 1);
        assert_eq!(delta.systems[0].added.len(), 1);
        assert_eq!(delta.systems[0].added[0].entity_bits, 20);
        assert_eq!(delta.systems[0].removed.len(), 0);
        assert_eq!(delta.systems[0].modified.len(), 0);
        assert_eq!(delta.systems[0].unchanged_count, 1);
    }

    /// 4. Server removes entity → delta.removed has 1 entry.
    #[test]
    fn remove_one_entity() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"pos"), (20, b"vel")])]);
        let snaps_b = make_snapshots(&[(1, &[(10, b"pos")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline));

        assert!(!delta.is_keyframe);
        assert_eq!(delta.systems.len(), 1);
        assert_eq!(delta.systems[0].added.len(), 0);
        assert_eq!(delta.systems[0].removed.len(), 1);
        assert_eq!(delta.systems[0].removed[0], 20);
        assert_eq!(delta.systems[0].modified.len(), 0);
        assert_eq!(delta.systems[0].unchanged_count, 1);
    }

    /// 5. Change entity bytes → delta.modified has 1 entry.
    #[test]
    fn modify_entity_data() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"old_data")])]);
        let snaps_b = make_snapshots(&[(1, &[(10, b"new_data")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline));

        assert!(!delta.is_keyframe);
        assert_eq!(delta.systems.len(), 1);
        assert_eq!(delta.systems[0].added.len(), 0);
        assert_eq!(delta.systems[0].removed.len(), 0);
        assert_eq!(delta.systems[0].modified.len(), 1);
        assert_eq!(delta.systems[0].modified[0].entity_bits, 10);
        assert_eq!(delta.systems[0].modified[0].data, b"new_data");
    }

    /// 6. Change entity but produce same encoded bytes → still unchanged.
    #[test]
    fn modify_entity_same_bytes() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"same")])]);
        // Same bytes, different tick — should be unchanged.
        let snaps_b = make_snapshots(&[(1, &[(10, b"same")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline));

        assert_eq!(delta.systems[0].modified.len(), 0);
        assert_eq!(delta.systems[0].added.len(), 0);
        assert_eq!(delta.systems[0].removed.len(), 0);
        assert_eq!(delta.systems[0].unchanged_count, 1);
    }

    /// 7. Encode N ticks, apply sequentially, final baseline matches latest
    ///    snapshot.
    #[test]
    fn delta_apply_roundtrip() {
        // Tick 1: entity 10 with "a"
        let snaps_t1 = make_snapshots(&[(1, &[(10, b"a")])]);
        let mut baseline =
            Baseline::from_snapshot(TickId::from_raw(1), &snaps_t1).expect("valid test snapshots");

        // Tick 2: add entity 20 with "b"
        let snaps_t2 = make_snapshots(&[(1, &[(10, b"a"), (20, b"b")])]);
        let delta_t2 = DeltaCodec::encode(TickId::from_raw(2), &snaps_t2, Some(&baseline));
        let _reconstructed = DeltaCodec::apply(&delta_t2, &mut baseline);

        // Tick 3: modify entity 10 from "a" to "a2", entity 20 unchanged
        let snaps_t3 = make_snapshots(&[(1, &[(10, b"a2"), (20, b"b")])]);
        let delta_t3 = DeltaCodec::encode(TickId::from_raw(3), &snaps_t3, Some(&baseline));
        let _reconstructed = DeltaCodec::apply(&delta_t3, &mut baseline);

        // Tick 4: remove entity 20
        let snaps_t4 = make_snapshots(&[(1, &[(10, b"a2")])]);
        let delta_t4 = DeltaCodec::encode(TickId::from_raw(4), &snaps_t4, Some(&baseline));
        let reconstructed_t4 = DeltaCodec::apply(&delta_t4, &mut baseline);

        // After applying all deltas, baseline should match snaps_t4.
        let t4_entities = snapshot_entities(&reconstructed_t4);
        let expected = snapshot_entities(&snaps_t4);

        assert_eq!(baseline.tick, TickId::from_raw(4));
        assert_eq!(t4_entities, expected);
        // Entity 20 must be gone.
        assert!(!t4_entities.get(&1).unwrap().contains_key(&20));
    }

    /// 8. Apply a removal delta, entities gone from baseline.
    #[test]
    fn delta_apply_removes_entities() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"pos"), (20, b"vel")])]);
        let snaps_b = make_snapshots(&[(1, &[(10, b"pos")])]);

        let mut baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline));
        let reconstructed = DeltaCodec::apply(&delta, &mut baseline);

        let entities = snapshot_entities(&reconstructed);
        let sys = entities.get(&1).unwrap();
        assert!(sys.contains_key(&10));
        assert!(!sys.contains_key(&20));
    }

    /// 9. encode→decode→encode → same bytes.
    #[test]
    fn encode_decode_delta_roundtrip() {
        let tick = TickId::from_raw(5);
        let snaps = make_snapshots(&[(1, &[(10, b"a"), (20, b"bb")]), (2, &[(30, b"ccc")])]);
        let baseline =
            Baseline::from_snapshot(TickId::from_raw(4), &snaps).expect("valid test snapshots");

        // Add entity 40 to system 1, modify entity 20, remove entity 30 from system 2.
        let snaps_next = make_snapshots(&[
            (1, &[(10, b"a"), (20, b"bb_modified"), (40, b"new")]),
            (2, &[]),
        ]);
        let delta = DeltaCodec::encode(tick, &snaps_next, Some(&baseline));

        let encoded = encode_delta(&delta);
        let decoded = decode_delta(&encoded).expect("decode should succeed");
        let re_encoded = encode_delta(&decoded);

        assert_eq!(encoded, re_encoded);
    }

    /// 10. Empty byte slice → error.
    #[test]
    fn decode_empty() {
        let result = decode_delta(b"");
        assert!(matches!(result, Err(DeltaDecodeError::TooShort)));
    }

    /// 11. Various truncations → error, never panic.
    #[test]
    fn decode_truncated() {
        // Build a valid encoded delta first.
        let tick = TickId::from_raw(1);
        let snaps = make_snapshots(&[(1, &[(10, b"data")])]);
        let delta = DeltaCodec::encode(tick, &snaps, None);
        let full = encode_delta(&delta);

        // Test truncation at every byte position from 1..full.len().
        for len in 1..full.len() {
            let truncated = &full[..len];
            let result = decode_delta(truncated);
            // Must never panic, must return an error.
            assert!(result.is_err(), "truncated at {len} bytes should error");
        }
    }

    /// 12. First tick, no baseline → keyframe.
    #[test]
    fn keyframe_fallback_on_fresh_join() {
        let tick = TickId::from_raw(1);
        let snaps = make_snapshots(&[(1, &[(100, b"fresh")])]);

        let delta = DeltaCodec::encode(tick, &snaps, None);

        assert!(delta.is_keyframe);
        assert!(delta.baseline_tick.is_none());
        assert_eq!(delta.systems.len(), 1);
        assert_eq!(delta.systems[0].system_id, 1);
        assert_eq!(delta.systems[0].added.len(), 1);
        assert_eq!(delta.systems[0].added[0].entity_bits, 100);
        assert_eq!(delta.systems[0].added[0].data, b"fresh");
        assert_eq!(delta.systems[0].removed.len(), 0);
        assert_eq!(delta.systems[0].modified.len(), 0);
    }

    // ── Additional DeltaCodec edge cases ──────────────────────────────────

    #[test]
    fn system_only_in_baseline_produces_removed() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"pos")]), (2, &[(20, b"vel")])]);
        // System 2 is gone in the next tick.
        let snaps_b = make_snapshots(&[(1, &[(10, b"pos")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline));

        // Should have a removed entry for system 2.
        let sys2_delta: Vec<_> = delta.systems.iter().filter(|s| s.system_id == 2).collect();
        assert_eq!(sys2_delta.len(), 1);
        assert_eq!(sys2_delta[0].removed.len(), 1);
        assert_eq!(sys2_delta[0].removed[0], 20);
    }

    #[test]
    fn system_only_in_current_produces_added() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"pos")])]);
        // System 2 is new in this tick.
        let snaps_b = make_snapshots(&[(1, &[(10, b"pos")]), (2, &[(20, b"new")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline));

        let sys2_delta: Vec<_> = delta.systems.iter().filter(|s| s.system_id == 2).collect();
        assert_eq!(sys2_delta.len(), 1);
        assert_eq!(sys2_delta[0].added.len(), 1);
        assert_eq!(sys2_delta[0].added[0].entity_bits, 20);
    }

    #[test]
    fn apply_keyframe_replaces_baseline() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"old")])]);
        let mut baseline = Baseline::from_snapshot(tick_a, &snaps_a).expect("valid test snapshots");

        // Keyframe with completely different entities.
        let snaps_b = make_snapshots(&[(1, &[(99, b"new")])]);
        let keyframe = DeltaCodec::encode(tick_b, &snaps_b, None);
        let reconstructed = DeltaCodec::apply(&keyframe, &mut baseline);

        let entities = snapshot_entities(&reconstructed);
        let sys = entities.get(&1).unwrap();
        assert!(sys.contains_key(&99));
        assert!(!sys.contains_key(&10)); // Old entity gone.
        assert_eq!(baseline.tick, tick_b);
    }

    #[test]
    fn encode_decode_delta_preserves_semantics() {
        let tick = TickId::from_raw(42);
        let baseline_tick = TickId::from_raw(41);
        let snaps = make_snapshots(&[(1, &[(10, b"a"), (20, b"bb")])]);
        let baseline =
            Baseline::from_snapshot(baseline_tick, &snaps).expect("valid test snapshots");

        let snaps_next = make_snapshots(&[(1, &[(10, b"a_modified"), (30, b"cc")])]);
        let delta = DeltaCodec::encode(tick, &snaps_next, Some(&baseline));

        let encoded = encode_delta(&delta);
        let decoded = decode_delta(&encoded).expect("decode should succeed");

        assert_eq!(decoded.tick, tick);
        assert_eq!(decoded.baseline_tick, Some(baseline_tick));
        assert!(!decoded.is_keyframe);
        assert_eq!(decoded.systems.len(), 1);
        assert_eq!(decoded.systems[0].system_id, 1);
        assert_eq!(decoded.systems[0].added.len(), 1);
        assert_eq!(decoded.systems[0].added[0].entity_bits, 30);
        assert_eq!(decoded.systems[0].removed.len(), 1);
        assert_eq!(decoded.systems[0].removed[0], 20);
        assert_eq!(decoded.systems[0].modified.len(), 1);
        assert_eq!(decoded.systems[0].modified[0].entity_bits, 10);
        assert_eq!(decoded.systems[0].modified[0].data, b"a_modified");
    }

    #[test]
    fn baseline_rejects_malformed_entity_blobs() {
        let tick = TickId::from_raw(1);
        let truncated = SystemSnapshot {
            system_id: 1,
            data: [
                1u64.to_le_bytes().as_slice(),
                4u32.to_le_bytes().as_slice(),
                b"x",
            ]
            .concat(),
        };
        assert!(matches!(
            Baseline::from_snapshot(tick, &[truncated]),
            Err(BaselineDecodeError::Truncated)
        ));

        let trailing = SystemSnapshot {
            system_id: 1,
            data: vec![0],
        };
        assert!(matches!(
            Baseline::from_snapshot(tick, &[trailing]),
            Err(BaselineDecodeError::TrailingBytes(1))
        ));

        let mut duplicate_data = Vec::new();
        for data in [b"a".as_slice(), b"b"] {
            duplicate_data.extend_from_slice(&7u64.to_le_bytes());
            duplicate_data.extend_from_slice(&(data.len() as u32).to_le_bytes());
            duplicate_data.extend_from_slice(data);
        }
        let duplicate = SystemSnapshot {
            system_id: 1,
            data: duplicate_data,
        };
        assert!(matches!(
            Baseline::from_snapshot(tick, &[duplicate]),
            Err(BaselineDecodeError::DuplicateEntity(7))
        ));
    }

    #[test]
    fn decode_delta_rejects_hostile_metadata_and_lengths() {
        let mut invalid_metadata = encode_delta(&Delta {
            tick: TickId::from_raw(1),
            baseline_tick: Some(TickId::from_raw(2)),
            is_keyframe: false,
            systems: Vec::new(),
        });
        invalid_metadata[16] = 1;
        assert!(matches!(
            decode_delta(&invalid_metadata),
            Err(DeltaDecodeError::InvalidMetadata)
        ));

        let mut huge_system_count = vec![0; 21];
        huge_system_count[16] = 1;
        huge_system_count[17..21].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_delta(&huge_system_count),
            Err(DeltaDecodeError::InvalidLength(_))
        ));

        let mut huge_component = encode_delta(&Delta {
            tick: TickId::from_raw(1),
            baseline_tick: None,
            is_keyframe: true,
            systems: vec![SystemDelta {
                system_id: 1,
                added: vec![EntityData {
                    entity_bits: 1,
                    data: Vec::new(),
                }],
                removed: Vec::new(),
                modified: Vec::new(),
                unchanged_count: 0,
            }],
        });
        huge_component[45..49].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_delta(&huge_component),
            Err(DeltaDecodeError::InvalidLength(_))
        ));
    }

    #[test]
    fn decode_delta_random_input_never_panics() {
        let mut state = 0x4352_4342_4c00_0000u64;
        for _ in 0..1000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = (state % 128) as usize;
            let mut payload = Vec::with_capacity(len);
            for _ in 0..len {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                payload.push(state as u8);
            }
            let _ = decode_delta(&payload);
        }
    }

    #[test]
    fn decode_rejects_non_canonical_keyframe_flag() {
        let mut payload = encode_delta(&Delta {
            tick: TickId::from_raw(1),
            baseline_tick: None,
            is_keyframe: true,
            systems: Vec::new(),
        });
        payload[16] = 2;
        assert!(decode_delta(&payload).is_err());
    }

    #[test]
    fn debug_coverage_delta_types() {
        let tick = TickId::from_raw(1);
        let snaps = make_snapshots(&[(1, &[(10, b"x")])]);
        let delta = DeltaCodec::encode(tick, &snaps, None);

        let _ = format!("{delta:?}");
        let _ = format!("{:?}", delta.systems[0]);

        let err = DeltaDecodeError::TooShort;
        let _ = format!("{err}");
        let _ = format!("{err:?}");

        let err = DeltaDecodeError::TrailingBytes(5);
        let _ = format!("{err}");
    }
}
