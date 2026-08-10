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
//!
//! # Entity blobs
//!
//! One framing carries per-entity component data everywhere in this protocol:
//! repeated `(entity_bits: u64 LE, data_len: u32 LE, data: [u8; data_len])`.
//! [`encode_entity_entry`] writes it and `read_entity_entry` reads it; no
//! other code in the workspace should open-code the offsets.

use std::collections::{HashMap, HashSet, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use crcbl_core::TickId;

use crate::auth::AUTH_OVERHEAD;
use crate::codec::{ByteReader, DecodeError};
use crate::messages::SystemSnapshot;
use crate::transport::MAX_IN_MEMORY_MESSAGE_BYTES;
use crate::types::{EntityBits, EntityData, SectorId};

/// The largest encoded delta whose *sealed* form fits the transport: sealing
/// adds [`crate::auth::AUTH_OVERHEAD`] bytes, and `send_unreliable` rejects
/// anything past [`crate::transport::MAX_IN_MEMORY_MESSAGE_BYTES`]. A delta
/// that encoded fine and then was dropped by the transport is a client that
/// desyncs for good — the 32-tick keyframe fallback is a full snapshot,
/// larger, and dropped too.
pub const MAX_DELTA_BYTES: usize = MAX_IN_MEMORY_MESSAGE_BYTES - AUTH_OVERHEAD;
/// Largest individual component payload accepted from a delta packet.
pub const MAX_COMPONENT_BYTES: usize = 60 * 1024;
/// Maximum systems accepted from an unauthenticated delta packet.
pub const MAX_BASELINE_SYSTEMS: usize = 256;
/// Maximum systems accepted once a packet's session MAC has verified.
///
/// This is the physical ceiling — a system costs 16 bytes of header, so a
/// 64 KiB packet cannot describe more. The payload-derived check in
/// [`decode_delta`] reaches the same conclusion first for a real packet; the
/// constant is what bounds a [`Baseline`] assembled by other means.
pub const MAX_AUTHENTICATED_SYSTEMS: usize = MAX_DELTA_BYTES / 16;
/// Maximum entities retained across all baseline systems.
pub const MAX_BASELINE_ENTITIES: usize = 4_096;
/// Maximum encoded entity bytes retained in one baseline.
pub const MAX_BASELINE_ENCODED_BYTES: usize = 256 * 1024;
/// Bytes of framing each entity entry costs on the wire and in a baseline.
const ENTITY_ENTRY_HEADER_BYTES: usize = 12;

// ── Trust ─────────────────────────────────────────────────────────────────────

/// How far the caller trusts the bytes being decoded.
///
/// The difference is one number — how many systems a packet may declare — but
/// it is the number that decides whether a hostile packet can make the
/// receiver allocate thousands of maps, so it is spelled out rather than
/// implied by which function was called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Bytes from a peer that has not proved it holds the session key. Every
    /// limit is at its hostile-input setting.
    Untrusted,
    /// Bytes whose per-session MAC verified — see [`crate::auth`]. Packet,
    /// component and duplicate-id validation is unchanged; only the system
    /// count is raised to the packet's physical maximum.
    Authenticated,
}

impl Trust {
    /// Maximum systems a packet or baseline may contain at this trust level.
    #[must_use]
    pub const fn max_systems(self) -> usize {
        match self {
            Self::Untrusted => MAX_BASELINE_SYSTEMS,
            Self::Authenticated => MAX_AUTHENTICATED_SYSTEMS,
        }
    }
}

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
    #[error("duplicate system id: {0}")]
    DuplicateSystem(u32),
    #[error("baseline exceeds retained state limits")]
    BaselineTooLarge,
    #[error("component data length exceeds limit: {0}")]
    ComponentTooLarge(u32),
    #[error("invalid {operation} for entity {entity_bits} in system {system_id}")]
    InvalidEntityOperation {
        system_id: u32,
        entity_bits: u64,
        operation: &'static str,
    },
    #[error("delta tick {delta:?} is not newer than baseline tick {baseline:?}")]
    StaleDeltaTick { delta: TickId, baseline: TickId },
    #[error("delta baseline tick {actual:?} does not match baseline tick {expected:?}")]
    WrongBaselineTick {
        expected: TickId,
        actual: Option<TickId>,
    },
    #[error("keyframe must not specify a baseline tick")]
    KeyframeHasBaselineTick,
    #[error("keyframe contains removed or modified entities")]
    InvalidKeyframeOperations,
}

// ── Entity blob framing ───────────────────────────────────────────────────────

/// Append one `(entity_bits, data_len, data)` entry to an entity blob.
///
/// This is the only writer of the framing; see the [module docs](self).
pub fn encode_entity_entry(out: &mut Vec<u8>, entity_bits: EntityBits, data: &[u8]) {
    out.extend_from_slice(&entity_bits.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

/// Why one entity entry could not be read.
#[derive(Debug, Clone, Copy)]
enum EntityEntryError {
    /// The 12-byte header is not fully present.
    Header,
    /// The declared payload length exceeds the component cap or the bytes left.
    Length(u32),
}

impl From<EntityEntryError> for DeltaDecodeError {
    fn from(error: EntityEntryError) -> Self {
        match error {
            EntityEntryError::Header => Self::TooShort,
            EntityEntryError::Length(len) => Self::InvalidLength(len),
        }
    }
}

impl From<EntityEntryError> for BaselineDecodeError {
    fn from(error: EntityEntryError) -> Self {
        match error {
            EntityEntryError::Length(len) if len as usize > MAX_COMPONENT_BYTES => {
                Self::ComponentTooLarge(len)
            }
            EntityEntryError::Header | EntityEntryError::Length(_) => Self::Truncated,
        }
    }
}

/// Read one `(entity_bits, data)` entry, enforcing the component-size cap.
///
/// This is the only reader of the framing; see the [module docs](self).
fn read_entity_entry<'a>(
    reader: &mut ByteReader<'a>,
) -> Result<(EntityBits, &'a [u8]), EntityEntryError> {
    if reader.remaining() < ENTITY_ENTRY_HEADER_BYTES {
        return Err(EntityEntryError::Header);
    }
    let entity_bits = reader.read_u64().map_err(|_| EntityEntryError::Header)?;
    let data_len = reader.read_u32().map_err(|_| EntityEntryError::Header)? as usize;
    if data_len > MAX_COMPONENT_BYTES || data_len > reader.remaining() {
        return Err(EntityEntryError::Length(data_len as u32));
    }
    let data = reader
        .read_bytes(data_len)
        .map_err(|_| EntityEntryError::Length(data_len as u32))?;
    Ok((entity_bits, data))
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
    /// Build a [`Baseline`] from snapshots, enforcing the retained-state limits
    /// for `trust`.
    pub fn from_snapshot(
        tick: TickId,
        systems: &[SystemSnapshot],
        trust: Trust,
    ) -> Result<Self, BaselineDecodeError> {
        let mut map: HashMap<u32, HashMap<u64, Vec<u8>>> = HashMap::new();

        for sys in systems {
            if map.contains_key(&sys.system_id) {
                return Err(BaselineDecodeError::DuplicateSystem(sys.system_id));
            }
            map.insert(sys.system_id, decode_entity_blobs(&sys.data)?);
        }
        validate_baseline_limits(&map, trust)?;

        Ok(Self { tick, systems: map })
    }

    /// Number of systems present in this baseline.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Total entity count across all systems in this baseline.
    pub fn entity_count(&self) -> usize {
        self.systems.values().map(HashMap::len).sum()
    }

    /// Number of entities in a specific system.
    pub fn entity_count_for(&self, system_id: u32) -> usize {
        self.systems.get(&system_id).map_or(0, HashMap::len)
    }

    /// Iterate every `(system_id, entity_bits, component data)` triple.
    ///
    /// Consumers that only want the state — a renderer reading transforms, for
    /// instance — should read it here rather than round-tripping the baseline
    /// through [`Baseline::to_snapshots`] and parsing the bytes back out.
    pub fn iter_entities(&self) -> impl Iterator<Item = (u32, EntityBits, &[u8])> {
        self.systems.iter().flat_map(|(&system_id, entities)| {
            entities
                .iter()
                .map(move |(&entity_bits, data)| (system_id, entity_bits, data.as_slice()))
        })
    }

    /// Re-serialise this baseline into one [`SystemSnapshot`] per system.
    ///
    /// This walks and copies the whole retained state; prefer
    /// [`Baseline::iter_entities`] unless the snapshot framing is what you
    /// actually need.
    pub fn to_snapshots(&self) -> Vec<SystemSnapshot> {
        self.systems
            .iter()
            .map(|(&system_id, entities)| {
                let mut data = Vec::new();
                for (&entity_bits, entity_data) in entities {
                    encode_entity_entry(&mut data, entity_bits, entity_data);
                }
                SystemSnapshot { system_id, data }
            })
            .collect()
    }

    /// Process-local hash of the full baseline state.
    ///
    /// Hashes every `(system_id, entity_bits, entity_data)` tuple in
    /// deterministic order (systems and entities sorted by id). The result is
    /// suitable for equality checks in one process using the same Rust version
    /// and platform; [`DefaultHasher`] does not define a cross-version or
    /// cross-platform stable format.
    pub fn state_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        let mut sys_ids: Vec<u32> = self.systems.keys().copied().collect();
        sys_ids.sort_unstable();
        for &sys_id in &sys_ids {
            sys_id.hash(&mut hasher);
            let entities = &self.systems[&sys_id];
            let mut entity_ids: Vec<u64> = entities.keys().copied().collect();
            entity_ids.sort_unstable();
            for &eb in &entity_ids {
                eb.hash(&mut hasher);
                entities[&eb].hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

/// Decode a flat entity blob into `entity_bits → Vec<u8>`.
fn decode_entity_blobs(blob: &[u8]) -> Result<HashMap<u64, Vec<u8>>, BaselineDecodeError> {
    if blob.len() > MAX_DELTA_BYTES {
        return Err(BaselineDecodeError::BlobTooLarge(blob.len()));
    }
    let mut out = HashMap::new();
    let mut reader = ByteReader::new(blob);

    while reader.remaining() > 0 {
        if reader.remaining() < ENTITY_ENTRY_HEADER_BYTES {
            return Err(BaselineDecodeError::TrailingBytes(reader.remaining()));
        }
        let (entity_bits, data) = read_entity_entry(&mut reader)?;
        if out.contains_key(&entity_bits) {
            return Err(BaselineDecodeError::DuplicateEntity(entity_bits));
        }
        out.insert(entity_bits, data.to_vec());
    }

    Ok(out)
}

fn validate_baseline_limits(
    systems: &HashMap<u32, HashMap<u64, Vec<u8>>>,
    trust: Trust,
) -> Result<(), BaselineDecodeError> {
    if systems.len() > trust.max_systems() {
        return Err(BaselineDecodeError::BaselineTooLarge);
    }

    let mut entities = 0usize;
    let mut encoded_bytes = 0usize;
    for system in systems.values() {
        entities = entities
            .checked_add(system.len())
            .ok_or(BaselineDecodeError::BaselineTooLarge)?;
        for data in system.values() {
            encoded_bytes = encoded_bytes
                .checked_add(ENTITY_ENTRY_HEADER_BYTES)
                .and_then(|bytes| bytes.checked_add(data.len()))
                .ok_or(BaselineDecodeError::BaselineTooLarge)?;
        }
    }
    if entities > MAX_BASELINE_ENTITIES || encoded_bytes > MAX_BASELINE_ENCODED_BYTES {
        return Err(BaselineDecodeError::BaselineTooLarge);
    }
    Ok(())
}

// ── BaselineStore ─────────────────────────────────────────────────────────────

/// Bounded ring buffer of [`Baseline`]s keyed by [`TickId`].
///
/// The ring is kept sorted by tick: [`BaselineStore::insert`] ignores a
/// baseline older than the newest retained one and replaces an equal one, so
/// [`BaselineStore::is_too_old`] can read the oldest tick off the front and
/// [`BaselineStore::get`] can binary-search. Older baselines are evicted once
/// capacity is reached; a capacity of zero disables storage.
#[derive(Debug)]
pub struct BaselineStore {
    capacity: usize,
    ring: VecDeque<Baseline>,
}

impl BaselineStore {
    /// Create a new store with the given ring capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            ring: VecDeque::with_capacity(capacity),
        }
    }

    /// Insert a baseline, evicting the oldest if at capacity.
    ///
    /// Ticks only ever move forwards, so a baseline older than the newest
    /// retained one is a caller error and is dropped rather than corrupting
    /// the ordering the lookups rely on. An equal tick replaces in place.
    pub fn insert(&mut self, baseline: Baseline) {
        if self.capacity == 0 {
            return;
        }
        if let Some(newest) = self.ring.back_mut() {
            if baseline.tick < newest.tick {
                return;
            }
            if baseline.tick == newest.tick {
                *newest = baseline;
                return;
            }
        }
        if self.ring.len() >= self.capacity {
            self.ring.pop_front();
        }
        self.ring.push_back(baseline);
    }

    /// Look up a baseline by tick id.
    #[must_use]
    pub fn get(&self, tick: TickId) -> Option<&Baseline> {
        let index = self
            .ring
            .binary_search_by(|baseline| baseline.tick.cmp(&tick))
            .ok()?;
        self.ring.get(index)
    }

    /// Whether the given tick is older than the oldest retained baseline
    /// (i.e. a delta-encode from it is impossible).
    #[must_use]
    pub fn is_too_old(&self, tick: TickId) -> bool {
        match self.ring.front() {
            Some(oldest) => tick < oldest.tick,
            None => true, // No baselines stored — everything is "too old".
        }
    }

    /// The newest retained baseline, if any.
    #[must_use]
    pub fn newest(&self) -> Option<&Baseline> {
        self.ring.back()
    }
}

// ── hash_encoded ──────────────────────────────────────────────────────────────

/// Hash serialised component bytes.
///
/// Uses [`DefaultHasher`] — adequate for a process-local identity check, not
/// cryptographically strong, and deliberately *not* used for change detection:
/// the encoder has both buffers in hand, so it compares them (see
/// [`DeltaCodec::encode_from_baseline`]).
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
    pub sector: SectorId,
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
    pub fn encode(
        tick: TickId,
        current: &[SystemSnapshot],
        baseline: Option<&Baseline>,
    ) -> Result<Delta, BaselineDecodeError> {
        Self::encode_with_sector(SectorId::ZERO, tick, current, baseline)
    }

    /// Compute a sector-scoped delta from `current` snapshots vs an optional baseline.
    ///
    /// The snapshots are parsed into a [`Baseline`] first, so a caller that
    /// already holds one — the server, which must retain it anyway — should
    /// call [`DeltaCodec::encode_from_baseline`] and skip the parse.
    pub fn encode_with_sector(
        sector: SectorId,
        tick: TickId,
        current: &[SystemSnapshot],
        baseline: Option<&Baseline>,
    ) -> Result<Delta, BaselineDecodeError> {
        let current = Baseline::from_snapshot(tick, current, Trust::Authenticated)?;
        Ok(Self::encode_from_baseline(sector, &current, baseline))
    }

    /// Diff two decoded baselines directly.
    ///
    /// Systems are emitted in ascending id order so the same pair of baselines
    /// always produces the same packet.
    #[must_use]
    pub fn encode_from_baseline(
        sector: SectorId,
        current: &Baseline,
        baseline: Option<&Baseline>,
    ) -> Delta {
        let Some(baseline) = baseline else {
            return Self::encode_keyframe(sector, current);
        };

        let mut system_ids: Vec<u32> = current
            .systems
            .keys()
            .chain(baseline.systems.keys())
            .copied()
            .collect();
        system_ids.sort_unstable();
        system_ids.dedup();

        let mut systems = Vec::with_capacity(system_ids.len());
        for system_id in system_ids {
            let current_entities = current.systems.get(&system_id);
            let baseline_entities = baseline.systems.get(&system_id);

            let mut added = Vec::new();
            let mut removed = Vec::new();
            let mut modified = Vec::new();
            let mut unchanged = 0u32;

            for (&entity_bits, data) in current_entities.into_iter().flatten() {
                match baseline_entities.and_then(|entities| entities.get(&entity_bits)) {
                    None => added.push(EntityData {
                        entity_bits,
                        data: data.clone(),
                    }),
                    // Both buffers are in hand: compare them. A 64-bit hash
                    // comparison here would be slower for a small blob and
                    // would silently drop a real update on a collision.
                    Some(baseline_data) if data != baseline_data => modified.push(EntityData {
                        entity_bits,
                        data: data.clone(),
                    }),
                    Some(_) => unchanged += 1,
                }
            }

            for &entity_bits in baseline_entities
                .into_iter()
                .flatten()
                .map(|(bits, _)| bits)
            {
                if !current_entities.is_some_and(|entities| entities.contains_key(&entity_bits)) {
                    removed.push(entity_bits);
                }
            }

            // A system present in the baseline and absent from current with no
            // entities to tombstone contributes nothing.
            if current_entities.is_none() && removed.is_empty() {
                continue;
            }

            systems.push(SystemDelta {
                system_id,
                added,
                removed,
                modified,
                unchanged_count: unchanged,
            });
        }

        Delta {
            sector,
            tick: current.tick,
            baseline_tick: Some(baseline.tick),
            is_keyframe: false,
            systems,
        }
    }

    /// Keyframe path: every entity in every system goes into `added`.
    fn encode_keyframe(sector: SectorId, current: &Baseline) -> Delta {
        let mut system_ids: Vec<u32> = current.systems.keys().copied().collect();
        system_ids.sort_unstable();
        let systems = system_ids
            .into_iter()
            .map(|system_id| SystemDelta {
                system_id,
                added: current.systems[&system_id]
                    .iter()
                    .map(|(&entity_bits, data)| EntityData {
                        entity_bits,
                        data: data.clone(),
                    })
                    .collect(),
                removed: Vec::new(),
                modified: Vec::new(),
                unchanged_count: 0,
            })
            .collect();

        Delta {
            sector,
            tick: current.tick,
            baseline_tick: None,
            is_keyframe: true,
            systems,
        }
    }

    /// Apply a delta transactionally, advancing `baseline` to the delta's tick.
    ///
    /// Every operation and the resulting size are checked before anything is
    /// written, so a rejected delta leaves `baseline` untouched — which is why
    /// this needs no defensive copy of the whole state. Read the result with
    /// [`Baseline::iter_entities`] or [`Baseline::to_snapshots`].
    pub fn apply(
        delta: &Delta,
        baseline: &mut Baseline,
        trust: Trust,
    ) -> Result<(), BaselineDecodeError> {
        validate_delta_metadata(delta, baseline)?;
        validate_delta_operations(delta)?;
        validate_delta_against_baseline(delta, baseline, trust)?;

        if delta.is_keyframe {
            baseline.systems.clear();
        }
        for sys_delta in &delta.systems {
            let entities = baseline.systems.entry(sys_delta.system_id).or_default();
            for entity in &sys_delta.added {
                entities.insert(entity.entity_bits, entity.data.clone());
            }
            for &entity_bits in &sys_delta.removed {
                entities.remove(&entity_bits);
            }
            for entity in &sys_delta.modified {
                entities.insert(entity.entity_bits, entity.data.clone());
            }
        }
        baseline.tick = delta.tick;
        Ok(())
    }
}

fn validate_delta_metadata(delta: &Delta, baseline: &Baseline) -> Result<(), BaselineDecodeError> {
    if delta.tick <= baseline.tick {
        return Err(BaselineDecodeError::StaleDeltaTick {
            delta: delta.tick,
            baseline: baseline.tick,
        });
    }
    if delta.is_keyframe {
        if delta.baseline_tick.is_some() {
            return Err(BaselineDecodeError::KeyframeHasBaselineTick);
        }
        if delta
            .systems
            .iter()
            .any(|system| !system.removed.is_empty() || !system.modified.is_empty())
        {
            return Err(BaselineDecodeError::InvalidKeyframeOperations);
        }
    } else if delta.baseline_tick != Some(baseline.tick) {
        return Err(BaselineDecodeError::WrongBaselineTick {
            expected: baseline.tick,
            actual: delta.baseline_tick,
        });
    }
    Ok(())
}

fn validate_delta_operations(delta: &Delta) -> Result<(), BaselineDecodeError> {
    let mut system_ids = HashSet::new();
    for system in &delta.systems {
        if !system_ids.insert(system.system_id) {
            return Err(BaselineDecodeError::DuplicateSystem(system.system_id));
        }
        let mut entity_ids = HashSet::new();
        for entity in system.added.iter().chain(&system.modified) {
            if entity.data.len() > MAX_COMPONENT_BYTES {
                return Err(BaselineDecodeError::ComponentTooLarge(
                    entity.data.len() as u32
                ));
            }
            if !entity_ids.insert(entity.entity_bits) {
                return Err(BaselineDecodeError::DuplicateEntity(entity.entity_bits));
            }
        }
        for &entity_bits in &system.removed {
            if !entity_ids.insert(entity_bits) {
                return Err(BaselineDecodeError::DuplicateEntity(entity_bits));
            }
        }
    }
    Ok(())
}

/// Check every operation against the current baseline and project the size of
/// the result, so [`DeltaCodec::apply`] can mutate in place knowing it cannot
/// fail half-way.
fn validate_delta_against_baseline(
    delta: &Delta,
    baseline: &Baseline,
    trust: Trust,
) -> Result<(), BaselineDecodeError> {
    let mut system_count = if delta.is_keyframe {
        0usize
    } else {
        baseline.systems.len()
    };
    let mut entities = if delta.is_keyframe {
        0usize
    } else {
        baseline.entity_count()
    };
    let mut encoded_bytes = if delta.is_keyframe {
        0usize
    } else {
        baseline
            .systems
            .values()
            .flat_map(HashMap::values)
            .map(|data| ENTITY_ENTRY_HEADER_BYTES + data.len())
            .sum()
    };

    for sys_delta in &delta.systems {
        // A keyframe replaces the whole state, so nothing survives from the
        // baseline to collide with or to be removed.
        let existing = if delta.is_keyframe {
            None
        } else {
            baseline.systems.get(&sys_delta.system_id)
        };
        if existing.is_none() {
            system_count += 1;
        }

        for entity in &sys_delta.added {
            if existing.is_some_and(|entities| entities.contains_key(&entity.entity_bits)) {
                return Err(BaselineDecodeError::InvalidEntityOperation {
                    system_id: sys_delta.system_id,
                    entity_bits: entity.entity_bits,
                    operation: "add",
                });
            }
            entities += 1;
            encoded_bytes += ENTITY_ENTRY_HEADER_BYTES + entity.data.len();
        }
        for &entity_bits in &sys_delta.removed {
            let Some(previous) = existing.and_then(|entities| entities.get(&entity_bits)) else {
                return Err(BaselineDecodeError::InvalidEntityOperation {
                    system_id: sys_delta.system_id,
                    entity_bits,
                    operation: "remove",
                });
            };
            entities -= 1;
            encoded_bytes -= ENTITY_ENTRY_HEADER_BYTES + previous.len();
        }
        for entity in &sys_delta.modified {
            let Some(previous) = existing.and_then(|entities| entities.get(&entity.entity_bits))
            else {
                return Err(BaselineDecodeError::InvalidEntityOperation {
                    system_id: sys_delta.system_id,
                    entity_bits: entity.entity_bits,
                    operation: "modify",
                });
            };
            encoded_bytes = encoded_bytes + entity.data.len() - previous.len();
        }
    }

    if system_count > trust.max_systems()
        || entities > MAX_BASELINE_ENTITIES
        || encoded_bytes > MAX_BASELINE_ENCODED_BYTES
    {
        return Err(BaselineDecodeError::BaselineTooLarge);
    }
    Ok(())
}

// ── Wire encoding ─────────────────────────────────────────────────────────────

/// Encode a [`Delta`] into the wire format.
///
/// Wire format (all LE):
///
/// ```text
/// sector:          x, y, z as i64
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
pub fn encode_delta(delta: &Delta) -> Result<Vec<u8>, BaselineDecodeError> {
    validate_delta_operations(delta)?;
    let mut capacity = 45usize;
    for system in &delta.systems {
        capacity = capacity
            .checked_add(16)
            .ok_or(BaselineDecodeError::BaselineTooLarge)?;
        for entity in system.added.iter().chain(&system.modified) {
            capacity = capacity
                .checked_add(ENTITY_ENTRY_HEADER_BYTES)
                .and_then(|bytes| bytes.checked_add(entity.data.len()))
                .ok_or(BaselineDecodeError::BaselineTooLarge)?;
        }
        capacity = capacity
            .checked_add(
                system
                    .removed
                    .len()
                    .checked_mul(8)
                    .ok_or(BaselineDecodeError::BaselineTooLarge)?,
            )
            .ok_or(BaselineDecodeError::BaselineTooLarge)?;
    }
    if capacity > MAX_DELTA_BYTES {
        return Err(BaselineDecodeError::BlobTooLarge(capacity));
    }
    let mut buf = Vec::with_capacity(capacity);

    buf.extend_from_slice(&delta.sector.x.to_le_bytes());
    buf.extend_from_slice(&delta.sector.y.to_le_bytes());
    buf.extend_from_slice(&delta.sector.z.to_le_bytes());
    buf.extend_from_slice(&delta.tick.get().to_le_bytes());
    // baseline_tick (None = 0)
    let baseline_tick = delta.baseline_tick.map_or(0, TickId::get);
    buf.extend_from_slice(&baseline_tick.to_le_bytes());
    buf.push(u8::from(delta.is_keyframe));
    buf.extend_from_slice(&(delta.systems.len() as u32).to_le_bytes());

    for sys in &delta.systems {
        buf.extend_from_slice(&sys.system_id.to_le_bytes());
        buf.extend_from_slice(&(sys.added.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(sys.removed.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(sys.modified.len() as u32).to_le_bytes());

        for entity in &sys.added {
            encode_entity_entry(&mut buf, entity.entity_bits, &entity.data);
        }
        // Removed entities carry no payload, so they are bare ids rather than
        // entity entries.
        for &entity_bits in &sys.removed {
            buf.extend_from_slice(&entity_bits.to_le_bytes());
        }
        for entity in &sys.modified {
            encode_entity_entry(&mut buf, entity.entity_bits, &entity.data);
        }
    }

    Ok(buf)
}

/// Decode a [`Delta`] from a wire packet (see [`encode_delta`]) at `trust`.
pub fn decode_delta(payload: &[u8], trust: Trust) -> Result<Delta, DeltaDecodeError> {
    if payload.len() > MAX_DELTA_BYTES {
        return Err(DeltaDecodeError::InvalidLength(payload.len() as u32));
    }
    let mut reader = ByteReader::new(payload);

    let sector = SectorId {
        x: reader.read_i64()?,
        y: reader.read_i64()?,
        z: reader.read_i64()?,
    };
    let tick = TickId::from_raw(reader.read_u64()?);

    // baseline_tick: 0 means "none".
    let baseline_tick = match reader.read_u64()? {
        0 => None,
        raw => Some(TickId::from_raw(raw)),
    };

    let is_keyframe = match reader.read_u8()? {
        0 => false,
        1 => true,
        flag => return Err(DeltaDecodeError::InvalidFlag(flag)),
    };
    if is_keyframe != baseline_tick.is_none() {
        return Err(DeltaDecodeError::InvalidMetadata);
    }

    let system_count = reader.read_u32()? as usize;
    if system_count > trust.max_systems() {
        return Err(DeltaDecodeError::InvalidLength(system_count as u32));
    }
    // Every system costs 16 bytes of header, so a count the packet cannot
    // physically contain is rejected before anything is allocated for it.
    if system_count > reader.remaining() / 16 {
        return Err(DeltaDecodeError::InvalidLength(system_count as u32));
    }

    let mut systems = Vec::with_capacity(system_count);
    let mut system_ids = HashSet::with_capacity(system_count);

    for _ in 0..system_count {
        let system_id = reader.read_u32()?;
        if !system_ids.insert(system_id) {
            return Err(DeltaDecodeError::DuplicateSystem(system_id));
        }

        let added_count = reader.read_u32()? as usize;
        let removed_count = reader.read_u32()? as usize;
        let modified_count = reader.read_u32()? as usize;

        // Same argument as the system count, one level down: each entity costs
        // at least its header, so the declared counts are checked against the
        // bytes remaining before any of them is allocated for.
        let minimum_entity_bytes = added_count
            .checked_mul(ENTITY_ENTRY_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(removed_count.checked_mul(8)?))
            .and_then(|bytes| {
                bytes.checked_add(modified_count.checked_mul(ENTITY_ENTRY_HEADER_BYTES)?)
            })
            .ok_or(DeltaDecodeError::InvalidLength(u32::MAX))?;
        if minimum_entity_bytes > reader.remaining() {
            return Err(DeltaDecodeError::InvalidLength(minimum_entity_bytes as u32));
        }

        let mut entity_ids = HashSet::with_capacity(
            added_count
                .checked_add(removed_count)
                .and_then(|count| count.checked_add(modified_count))
                .ok_or(DeltaDecodeError::InvalidLength(u32::MAX))?,
        );

        let mut added = Vec::with_capacity(added_count);
        for _ in 0..added_count {
            let (entity_bits, data) = read_entity_entry(&mut reader)?;
            if !entity_ids.insert(entity_bits) {
                return Err(DeltaDecodeError::DuplicateEntity(entity_bits));
            }
            added.push(EntityData {
                entity_bits,
                data: data.to_vec(),
            });
        }

        let mut removed = Vec::with_capacity(removed_count);
        for _ in 0..removed_count {
            let entity_bits = reader.read_u64()?;
            if !entity_ids.insert(entity_bits) {
                return Err(DeltaDecodeError::DuplicateEntity(entity_bits));
            }
            removed.push(entity_bits);
        }

        let mut modified = Vec::with_capacity(modified_count);
        for _ in 0..modified_count {
            let (entity_bits, data) = read_entity_entry(&mut reader)?;
            if !entity_ids.insert(entity_bits) {
                return Err(DeltaDecodeError::DuplicateEntity(entity_bits));
            }
            modified.push(EntityData {
                entity_bits,
                data: data.to_vec(),
            });
        }

        systems.push(SystemDelta {
            system_id,
            added,
            removed,
            modified,
            unchanged_count: 0, // Not serialised on wire.
        });
    }

    if reader.remaining() > 0 {
        return Err(DeltaDecodeError::TrailingBytes(reader.remaining()));
    }

    Ok(Delta {
        sector,
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
    #[error("duplicate system id: {0}")]
    DuplicateSystem(u32),
    #[error("duplicate entity id: {0}")]
    DuplicateEntity(u64),
}

impl From<DecodeError> for DeltaDecodeError {
    fn from(error: DecodeError) -> Self {
        match error {
            DecodeError::TooShort { .. } => Self::TooShort,
            DecodeError::InvalidLength(len) => Self::InvalidLength(len),
            DecodeError::TrailingBytes(count) => Self::TrailingBytes(count),
            DecodeError::DuplicateSystem(id) => Self::DuplicateSystem(id),
            DecodeError::UnknownTag { tag } => Self::InvalidFlag(tag),
        }
    }
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
        Baseline::from_snapshot(tick, &[snap], Trust::Authenticated).expect("valid test snapshot")
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

    #[test]
    fn baseline_store_zero_capacity_stays_empty() {
        let mut store = BaselineStore::new(0);
        store.insert(make_snapshot(TickId::from_raw(1), 1, &[(1, b"a")]));
        assert!(store.newest().is_none());
    }

    // ── Eviction ──────────────────────────────────────────────────────────

    #[test]
    fn the_baseline_store_evicts_its_oldest_tick_and_then_calls_it_too_old() {
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

    #[test]
    fn baseline_store_keeps_its_ring_sorted() {
        let mut store = BaselineStore::new(4);
        for tick in [10u64, 20, 30] {
            store.insert(make_snapshot(TickId::from_raw(tick), 1, &[(tick, b"x")]));
        }

        // An out-of-order insert would break both the binary search in `get`
        // and the "oldest is at the front" assumption in `is_too_old`.
        store.insert(make_snapshot(TickId::from_raw(15), 1, &[(15, b"late")]));
        assert!(store.get(TickId::from_raw(15)).is_none());
        assert_eq!(store.newest().map(|b| b.tick), Some(TickId::from_raw(30)));
        assert!(store.is_too_old(TickId::from_raw(9)));
        assert!(!store.is_too_old(TickId::from_raw(10)));
        for tick in [10u64, 20, 30] {
            assert_eq!(
                store.get(TickId::from_raw(tick)).map(|b| b.tick),
                Some(TickId::from_raw(tick))
            );
        }

        // Re-inserting the newest tick replaces it rather than duplicating it.
        store.insert(make_snapshot(TickId::from_raw(30), 1, &[(99, b"redo")]));
        assert_eq!(store.newest().map(|b| b.tick), Some(TickId::from_raw(30)));
        assert_eq!(store.get(TickId::from_raw(30)).unwrap().entity_count(), 1);
        assert!(
            store
                .get(TickId::from_raw(30))
                .unwrap()
                .iter_entities()
                .any(|(_, bits, _)| bits == 99)
        );
    }

    // ── is_too_old (empty store) ──────────────────────────────────────────

    #[test]
    fn baseline_store_empty_is_too_old() {
        let store = BaselineStore::new(4);
        assert!(store.is_too_old(TickId::from_raw(0)));
    }

    // ── newest ────────────────────────────────────────────────────────────

    #[test]
    fn the_baseline_store_has_no_newest_until_one_is_inserted_then_tracks_the_latest() {
        let mut store = BaselineStore::new(4);
        assert!(store.newest().is_none());

        store.insert(make_snapshot(TickId::from_raw(5), 1, &[(1, b"a")]));
        assert_eq!(store.newest().unwrap().tick, TickId::from_raw(5));

        store.insert(make_snapshot(TickId::from_raw(8), 1, &[(2, b"b")]));
        assert_eq!(store.newest().unwrap().tick, TickId::from_raw(8));
    }

    // ── Baseline::from_snapshot ───────────────────────────────────────────

    #[test]
    fn a_baseline_carries_the_tick_and_the_system_and_entity_counts_of_its_snapshot() {
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

        let baseline =
            Baseline::from_snapshot(TickId::from_raw(7), &[sys1, sys2], Trust::Authenticated)
                .expect("valid test snapshots");
        assert_eq!(baseline.system_count(), 2);
        assert_eq!(baseline.entity_count(), 3);
    }

    #[test]
    fn trusted_baseline_allows_more_than_256_systems() {
        let systems: Vec<_> = (0..300)
            .map(|system_id| SystemSnapshot {
                system_id,
                data: Vec::new(),
            })
            .collect();
        assert!(matches!(
            Baseline::from_snapshot(TickId::from_raw(1), &systems, Trust::Untrusted),
            Err(BaselineDecodeError::BaselineTooLarge)
        ));
        let baseline = Baseline::from_snapshot(TickId::from_raw(1), &systems, Trust::Authenticated)
            .expect("trusted snapshots are not packet-limited");
        assert_eq!(baseline.system_count(), 300);
    }

    #[test]
    fn untrusted_decoder_rejects_more_than_256_systems() {
        let delta = Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(1),
            baseline_tick: None,
            is_keyframe: true,
            systems: (0..=MAX_BASELINE_SYSTEMS as u32)
                .map(|system_id| SystemDelta {
                    system_id,
                    added: Vec::new(),
                    removed: Vec::new(),
                    modified: Vec::new(),
                    unchanged_count: 0,
                })
                .collect(),
        };
        let payload = encode_delta(&delta).expect("packet remains below byte limit");
        assert!(matches!(
            decode_delta(&payload, Trust::Untrusted),
            Err(DeltaDecodeError::InvalidLength(_))
        ));
        assert_eq!(
            decode_delta(&payload, Trust::Authenticated)
                .expect("authenticated peer may exceed hostile system cap")
                .systems
                .len(),
            MAX_BASELINE_SYSTEMS + 1
        );
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

        let baseline =
            Baseline::from_snapshot(TickId::from_raw(7), &[sys1, sys2], Trust::Authenticated)
                .expect("valid test snapshots");
        assert_eq!(baseline.entity_count_for(10), 1);
        assert_eq!(baseline.entity_count_for(20), 1);
        assert_eq!(baseline.entity_count_for(99), 0);
    }

    // ── Debug coverage ────────────────────────────────────────────────────

    #[test]
    fn a_baseline_and_its_store_debug_print_without_panicking() {
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

        let delta = DeltaCodec::encode(tick, &snaps, None).expect("valid snapshot");

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

        let baseline = Baseline::from_snapshot(tick_a, &snaps, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps, Some(&baseline)).expect("valid snapshot");

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
    fn an_entity_added_since_the_baseline_appears_once_in_the_deltas_added_list() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"pos")])]);
        let snaps_b = make_snapshots(&[(1, &[(10, b"pos"), (20, b"vel")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline)).expect("valid snapshot");

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
    fn an_entity_gone_since_the_baseline_appears_once_in_the_deltas_removed_list() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"pos"), (20, b"vel")])]);
        let snaps_b = make_snapshots(&[(1, &[(10, b"pos")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline)).expect("valid snapshot");

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
    fn changed_entity_bytes_appear_as_a_modified_entry_carrying_the_new_data() {
        let tick_a = TickId::from_raw(1);
        let tick_b = TickId::from_raw(2);

        let snaps_a = make_snapshots(&[(1, &[(10, b"old_data")])]);
        let snaps_b = make_snapshots(&[(1, &[(10, b"new_data")])]);

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline)).expect("valid snapshot");

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

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline)).expect("valid snapshot");

        assert_eq!(delta.systems[0].modified.len(), 0);
        assert_eq!(delta.systems[0].added.len(), 0);
        assert_eq!(delta.systems[0].removed.len(), 0);
        assert_eq!(delta.systems[0].unchanged_count, 1);
    }

    /// 7. Encode N ticks, apply sequentially, final baseline matches latest
    ///    snapshot.
    #[test]
    fn applying_a_run_of_deltas_leaves_the_baseline_equal_to_the_latest_snapshot() {
        // Tick 1: entity 10 with "a"
        let snaps_t1 = make_snapshots(&[(1, &[(10, b"a")])]);
        let mut baseline =
            Baseline::from_snapshot(TickId::from_raw(1), &snaps_t1, Trust::Authenticated)
                .expect("valid test snapshots");

        // Tick 2: add entity 20 with "b"
        let snaps_t2 = make_snapshots(&[(1, &[(10, b"a"), (20, b"b")])]);
        let delta_t2 = DeltaCodec::encode(TickId::from_raw(2), &snaps_t2, Some(&baseline))
            .expect("valid snapshot");
        DeltaCodec::apply(&delta_t2, &mut baseline, Trust::Authenticated).expect("valid delta");

        // Tick 3: modify entity 10 from "a" to "a2", entity 20 unchanged
        let snaps_t3 = make_snapshots(&[(1, &[(10, b"a2"), (20, b"b")])]);
        let delta_t3 = DeltaCodec::encode(TickId::from_raw(3), &snaps_t3, Some(&baseline))
            .expect("valid snapshot");
        DeltaCodec::apply(&delta_t3, &mut baseline, Trust::Authenticated).expect("valid delta");

        // Tick 4: remove entity 20
        let snaps_t4 = make_snapshots(&[(1, &[(10, b"a2")])]);
        let delta_t4 = DeltaCodec::encode(TickId::from_raw(4), &snaps_t4, Some(&baseline))
            .expect("valid snapshot");
        DeltaCodec::apply(&delta_t4, &mut baseline, Trust::Authenticated).expect("valid delta");

        // After applying all deltas, baseline should match snaps_t4.
        let t4_entities = snapshot_entities(&baseline.to_snapshots());
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

        let mut baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline)).expect("valid snapshot");
        DeltaCodec::apply(&delta, &mut baseline, Trust::Authenticated).expect("valid delta");

        let entities = snapshot_entities(&baseline.to_snapshots());
        let sys = entities.get(&1).unwrap();
        assert!(sys.contains_key(&10));
        assert!(!sys.contains_key(&20));
    }

    /// 9. encode→decode→encode → same bytes.
    #[test]
    fn encode_decode_delta_roundtrip() {
        let tick = TickId::from_raw(5);
        let snaps = make_snapshots(&[(1, &[(10, b"a"), (20, b"bb")]), (2, &[(30, b"ccc")])]);
        let baseline = Baseline::from_snapshot(TickId::from_raw(4), &snaps, Trust::Authenticated)
            .expect("valid test snapshots");

        // Add entity 40 to system 1, modify entity 20, remove entity 30 from system 2.
        let snaps_next = make_snapshots(&[
            (1, &[(10, b"a"), (20, b"bb_modified"), (40, b"new")]),
            (2, &[]),
        ]);
        let sector = SectorId { x: -4, y: 5, z: 6 };
        let delta = DeltaCodec::encode_with_sector(sector, tick, &snaps_next, Some(&baseline))
            .expect("valid snapshot");

        let encoded = encode_delta(&delta).expect("valid delta");
        let decoded = decode_delta(&encoded, Trust::Untrusted).expect("decode should succeed");
        let re_encoded = encode_delta(&decoded).expect("valid delta");

        assert_eq!(decoded.sector, sector);
        assert_eq!(encoded, re_encoded);
    }

    /// 10. Empty byte slice → error.
    #[test]
    fn an_empty_delta_payload_is_reported_as_too_short() {
        let result = decode_delta(b"", Trust::Untrusted);
        assert!(matches!(result, Err(DeltaDecodeError::TooShort)));
    }

    /// 11. Various truncations → error, never panic.
    #[test]
    fn a_delta_truncated_at_any_length_errors_instead_of_panicking() {
        // Build a valid encoded delta first.
        let tick = TickId::from_raw(1);
        let snaps = make_snapshots(&[(1, &[(10, b"data")])]);
        let delta = DeltaCodec::encode(tick, &snaps, None).expect("valid snapshot");
        let full = encode_delta(&delta).expect("valid delta");

        // Test truncation at every byte position from 1..full.len().
        for len in 1..full.len() {
            let truncated = &full[..len];
            let result = decode_delta(truncated, Trust::Untrusted);
            // Must never panic, must return an error.
            assert!(result.is_err(), "truncated at {len} bytes should error");
        }
    }

    #[test]
    fn decode_rejects_sector_header_boundaries_and_trailing_bytes() {
        let delta = DeltaCodec::encode_with_sector(
            SectorId { x: -1, y: 2, z: 3 },
            TickId::from_raw(1),
            &[],
            None,
        )
        .expect("valid keyframe");
        let full = encode_delta(&delta).expect("valid delta");

        for len in 0..24 {
            assert!(
                matches!(
                    decode_delta(&full[..len], Trust::Untrusted),
                    Err(DeltaDecodeError::TooShort)
                ),
                "sector header truncated at {len} bytes"
            );
        }
        let mut trailing = full;
        trailing.push(0);
        assert!(matches!(
            decode_delta(&trailing, Trust::Untrusted),
            Err(DeltaDecodeError::TrailingBytes(1))
        ));
    }

    /// 12. First tick, no baseline → keyframe.
    #[test]
    fn keyframe_fallback_on_fresh_join() {
        let tick = TickId::from_raw(1);
        let snaps = make_snapshots(&[(1, &[(100, b"fresh")])]);

        let delta = DeltaCodec::encode(tick, &snaps, None).expect("valid snapshot");

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

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline)).expect("valid snapshot");

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

        let baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");
        let delta = DeltaCodec::encode(tick_b, &snaps_b, Some(&baseline)).expect("valid snapshot");

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
        let mut baseline = Baseline::from_snapshot(tick_a, &snaps_a, Trust::Authenticated)
            .expect("valid test snapshots");

        // Keyframe with completely different entities.
        let snaps_b = make_snapshots(&[(1, &[(99, b"new")])]);
        let keyframe = DeltaCodec::encode(tick_b, &snaps_b, None).expect("valid snapshot");
        DeltaCodec::apply(&keyframe, &mut baseline, Trust::Authenticated).expect("valid delta");

        let entities = snapshot_entities(&baseline.to_snapshots());
        let sys = entities.get(&1).unwrap();
        assert!(sys.contains_key(&99));
        assert!(!sys.contains_key(&10)); // Old entity gone.
        assert_eq!(baseline.tick, tick_b);
    }

    #[test]
    fn apply_rejects_wrong_baseline_tick_without_mutating() {
        let snapshots = make_snapshots(&[(1, &[(10, b"old")])]);
        let mut baseline =
            Baseline::from_snapshot(TickId::from_raw(2), &snapshots, Trust::Authenticated)
                .expect("valid test snapshots");
        let original_hash = baseline.state_hash();
        let delta = Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(3),
            baseline_tick: Some(TickId::from_raw(1)),
            is_keyframe: false,
            systems: Vec::new(),
        };

        assert!(matches!(
            DeltaCodec::apply(&delta, &mut baseline, Trust::Authenticated),
            Err(BaselineDecodeError::WrongBaselineTick {
                expected,
                actual: Some(actual),
            }) if expected == TickId::from_raw(2) && actual == TickId::from_raw(1)
        ));
        assert_eq!(baseline.tick, TickId::from_raw(2));
        assert_eq!(baseline.state_hash(), original_hash);
    }

    #[test]
    fn apply_rejects_stale_tick_without_mutating() {
        let snapshots = make_snapshots(&[(1, &[(10, b"old")])]);
        let mut baseline =
            Baseline::from_snapshot(TickId::from_raw(2), &snapshots, Trust::Authenticated)
                .expect("valid test snapshots");
        let original_hash = baseline.state_hash();
        let delta = Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(2),
            baseline_tick: Some(TickId::from_raw(2)),
            is_keyframe: false,
            systems: Vec::new(),
        };

        assert!(matches!(
            DeltaCodec::apply(&delta, &mut baseline, Trust::Authenticated),
            Err(BaselineDecodeError::StaleDeltaTick { .. })
        ));
        assert_eq!(baseline.tick, TickId::from_raw(2));
        assert_eq!(baseline.state_hash(), original_hash);
    }

    #[test]
    fn apply_rejects_malformed_keyframe_without_mutating() {
        let snapshots = make_snapshots(&[(1, &[(10, b"old")])]);
        let mut baseline =
            Baseline::from_snapshot(TickId::from_raw(2), &snapshots, Trust::Authenticated)
                .expect("valid test snapshots");
        let original_hash = baseline.state_hash();
        let delta = Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(3),
            baseline_tick: None,
            is_keyframe: true,
            systems: vec![SystemDelta {
                system_id: 1,
                added: Vec::new(),
                removed: vec![10],
                modified: Vec::new(),
                unchanged_count: 0,
            }],
        };

        assert!(matches!(
            DeltaCodec::apply(&delta, &mut baseline, Trust::Authenticated),
            Err(BaselineDecodeError::InvalidKeyframeOperations)
        ));
        assert_eq!(baseline.tick, TickId::from_raw(2));
        assert_eq!(baseline.state_hash(), original_hash);
    }

    #[test]
    fn apply_rejects_invalid_entity_lifecycle_without_mutating() {
        let snapshots = make_snapshots(&[(1, &[(10, b"old")])]);
        let baseline =
            Baseline::from_snapshot(TickId::from_raw(2), &snapshots, Trust::Authenticated)
                .expect("valid test snapshots");

        let cases = [
            SystemDelta {
                system_id: 1,
                added: vec![EntityData {
                    entity_bits: 10,
                    data: b"overwrite".to_vec(),
                }],
                removed: Vec::new(),
                modified: Vec::new(),
                unchanged_count: 0,
            },
            SystemDelta {
                system_id: 1,
                added: Vec::new(),
                removed: Vec::new(),
                modified: vec![EntityData {
                    entity_bits: 99,
                    data: b"created".to_vec(),
                }],
                unchanged_count: 0,
            },
            SystemDelta {
                system_id: 1,
                added: Vec::new(),
                removed: vec![99],
                modified: Vec::new(),
                unchanged_count: 0,
            },
        ];

        for system in cases {
            let mut candidate = baseline.clone();
            let hash = candidate.state_hash();
            let delta = Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(3),
                baseline_tick: Some(TickId::from_raw(2)),
                is_keyframe: false,
                systems: vec![system],
            };
            assert!(matches!(
                DeltaCodec::apply(&delta, &mut candidate, Trust::Authenticated),
                Err(BaselineDecodeError::InvalidEntityOperation { .. })
            ));
            assert_eq!(candidate.tick, TickId::from_raw(2));
            assert_eq!(candidate.state_hash(), hash);
        }
    }

    #[test]
    fn oversized_component_reports_component_too_large() {
        let delta = Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(1),
            baseline_tick: None,
            is_keyframe: true,
            systems: vec![SystemDelta {
                system_id: 1,
                added: vec![EntityData {
                    entity_bits: 7,
                    data: vec![0; MAX_COMPONENT_BYTES + 1],
                }],
                removed: Vec::new(),
                modified: Vec::new(),
                unchanged_count: 0,
            }],
        };
        assert!(matches!(
            encode_delta(&delta),
            Err(BaselineDecodeError::ComponentTooLarge(size))
                if size as usize == MAX_COMPONENT_BYTES + 1
        ));
    }

    #[test]
    fn encode_decode_delta_preserves_semantics() {
        let tick = TickId::from_raw(42);
        let baseline_tick = TickId::from_raw(41);
        let snaps = make_snapshots(&[(1, &[(10, b"a"), (20, b"bb")])]);
        let baseline = Baseline::from_snapshot(baseline_tick, &snaps, Trust::Authenticated)
            .expect("valid test snapshots");

        let snaps_next = make_snapshots(&[(1, &[(10, b"a_modified"), (30, b"cc")])]);
        let delta = DeltaCodec::encode(tick, &snaps_next, Some(&baseline)).expect("valid snapshot");

        let encoded = encode_delta(&delta).expect("valid delta");
        let decoded = decode_delta(&encoded, Trust::Untrusted).expect("decode should succeed");

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
            Baseline::from_snapshot(tick, &[truncated], Trust::Authenticated),
            Err(BaselineDecodeError::Truncated)
        ));

        let trailing = SystemSnapshot {
            system_id: 1,
            data: vec![0],
        };
        assert!(matches!(
            Baseline::from_snapshot(tick, &[trailing], Trust::Authenticated),
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
            Baseline::from_snapshot(tick, &[duplicate], Trust::Authenticated),
            Err(BaselineDecodeError::DuplicateEntity(7))
        ));
    }

    #[test]
    fn decode_delta_rejects_hostile_metadata_and_lengths() {
        let mut invalid_metadata = encode_delta(&Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(1),
            baseline_tick: Some(TickId::from_raw(2)),
            is_keyframe: false,
            systems: Vec::new(),
        })
        .expect("valid delta");
        invalid_metadata[40] = 1;
        assert!(matches!(
            decode_delta(&invalid_metadata, Trust::Untrusted),
            Err(DeltaDecodeError::InvalidMetadata)
        ));

        let mut huge_system_count = vec![0; 45];
        huge_system_count[40] = 1;
        huge_system_count[41..45].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_delta(&huge_system_count, Trust::Untrusted),
            Err(DeltaDecodeError::InvalidLength(_))
        ));

        let mut huge_component = encode_delta(&Delta {
            sector: SectorId::ZERO,
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
        })
        .expect("valid delta");
        huge_component[69..73].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_delta(&huge_component, Trust::Untrusted),
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
            let _ = decode_delta(&payload, Trust::Untrusted);
        }
    }

    #[test]
    fn decode_rejects_non_canonical_keyframe_flag() {
        let mut payload = encode_delta(&Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(1),
            baseline_tick: None,
            is_keyframe: true,
            systems: Vec::new(),
        })
        .expect("valid delta");
        payload[40] = 2;
        assert!(decode_delta(&payload, Trust::Untrusted).is_err());
    }

    #[test]
    fn encode_rejects_malformed_snapshots_without_panicking() {
        let malformed = SystemSnapshot {
            system_id: 1,
            data: vec![0],
        };
        let result = std::panic::catch_unwind(|| {
            DeltaCodec::encode(TickId::from_raw(1), &[malformed], None)
        });
        assert!(matches!(
            result,
            Ok(Err(BaselineDecodeError::TrailingBytes(1)))
        ));
    }

    #[test]
    fn baseline_rejects_duplicate_system_ids() {
        let systems = [
            SystemSnapshot {
                system_id: 1,
                data: Vec::new(),
            },
            SystemSnapshot {
                system_id: 1,
                data: Vec::new(),
            },
        ];
        assert!(matches!(
            Baseline::from_snapshot(TickId::from_raw(1), &systems, Trust::Authenticated),
            Err(BaselineDecodeError::DuplicateSystem(1))
        ));
    }

    #[test]
    fn decode_delta_rejects_duplicate_entities_and_systems() {
        let header = |system_count: u32| {
            let mut payload = Vec::new();
            payload.extend_from_slice(&0i64.to_le_bytes());
            payload.extend_from_slice(&0i64.to_le_bytes());
            payload.extend_from_slice(&0i64.to_le_bytes());
            payload.extend_from_slice(&1u64.to_le_bytes());
            payload.extend_from_slice(&0u64.to_le_bytes());
            payload.push(1);
            payload.extend_from_slice(&system_count.to_le_bytes());
            payload
        };
        let mut same_list = header(1u32);
        same_list.extend_from_slice(&1u32.to_le_bytes());
        same_list.extend_from_slice(&2u32.to_le_bytes());
        same_list.extend_from_slice(&0u32.to_le_bytes());
        same_list.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..2 {
            same_list.extend_from_slice(&7u64.to_le_bytes());
            same_list.extend_from_slice(&0u32.to_le_bytes());
        }
        assert!(matches!(
            decode_delta(&same_list, Trust::Untrusted),
            Err(DeltaDecodeError::DuplicateEntity(7))
        ));

        let mut cross_list = header(1u32);
        cross_list.extend_from_slice(&1u32.to_le_bytes());
        cross_list.extend_from_slice(&1u32.to_le_bytes());
        cross_list.extend_from_slice(&1u32.to_le_bytes());
        cross_list.extend_from_slice(&0u32.to_le_bytes());
        cross_list.extend_from_slice(&7u64.to_le_bytes());
        cross_list.extend_from_slice(&0u32.to_le_bytes());
        cross_list.extend_from_slice(&7u64.to_le_bytes());
        assert!(matches!(
            decode_delta(&cross_list, Trust::Untrusted),
            Err(DeltaDecodeError::DuplicateEntity(7))
        ));

        let mut duplicate_system = header(2u32);
        for _ in 0..2 {
            duplicate_system.extend_from_slice(&1u32.to_le_bytes());
            duplicate_system.extend_from_slice(&0u32.to_le_bytes());
            duplicate_system.extend_from_slice(&0u32.to_le_bytes());
            duplicate_system.extend_from_slice(&0u32.to_le_bytes());
        }
        assert!(matches!(
            decode_delta(&duplicate_system, Trust::Untrusted),
            Err(DeltaDecodeError::DuplicateSystem(1))
        ));
    }

    #[test]
    fn apply_rejects_growth_without_mutating_baseline() {
        let mut baseline = Baseline::from_snapshot(TickId::ZERO, &[], Trust::Authenticated)
            .expect("empty baseline");
        for entity_bits in 0..MAX_BASELINE_ENTITIES as u64 {
            let delta = Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(entity_bits + 1),
                baseline_tick: Some(baseline.tick),
                is_keyframe: false,
                systems: vec![SystemDelta {
                    system_id: 1,
                    added: vec![EntityData {
                        entity_bits,
                        data: Vec::new(),
                    }],
                    removed: Vec::new(),
                    modified: Vec::new(),
                    unchanged_count: 0,
                }],
            };
            DeltaCodec::apply(&delta, &mut baseline, Trust::Authenticated)
                .expect("within baseline limit");
        }
        let hash = baseline.state_hash();
        let tick = baseline.tick;
        let overflow = Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(MAX_BASELINE_ENTITIES as u64 + 1),
            baseline_tick: Some(tick),
            is_keyframe: false,
            systems: vec![SystemDelta {
                system_id: 1,
                added: vec![EntityData {
                    entity_bits: u64::MAX,
                    data: Vec::new(),
                }],
                removed: Vec::new(),
                modified: Vec::new(),
                unchanged_count: 0,
            }],
        };
        assert!(matches!(
            DeltaCodec::apply(&overflow, &mut baseline, Trust::Authenticated),
            Err(BaselineDecodeError::BaselineTooLarge)
        ));
        assert_eq!(baseline.tick, tick);
        assert_eq!(baseline.state_hash(), hash);
    }

    /// A decode failure is read out of a log, so the identifier or count that
    /// says *which* duplicate, tag or overrun it was has to survive into the
    /// message. `crate::codec` asserts the same for [`DecodeError`], which
    /// these are converted from.
    ///
    /// This test used to format three of the variants and discard the strings,
    /// under a name announcing that it existed to move a coverage number.
    #[test]
    fn each_delta_decode_error_prints_the_value_that_identifies_it() {
        for (error, expected) in [
            (DeltaDecodeError::TooShort, "payload too short"),
            (
                DeltaDecodeError::InvalidLength(999),
                "invalid data length: 999",
            ),
            (DeltaDecodeError::InvalidFlag(7), "invalid keyframe flag: 7"),
            (
                DeltaDecodeError::InvalidMetadata,
                "inconsistent keyframe and baseline metadata",
            ),
            (DeltaDecodeError::TrailingBytes(5), "trailing bytes: 5"),
            (
                DeltaDecodeError::DuplicateSystem(3),
                "duplicate system id: 3",
            ),
            (
                DeltaDecodeError::DuplicateEntity(42),
                "duplicate entity id: 42",
            ),
        ] {
            assert_eq!(format!("{error}"), expected);
        }
    }
}
