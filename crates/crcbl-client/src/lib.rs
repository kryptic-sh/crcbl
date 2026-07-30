//! Rendering client: delta-apply, interpolation buffer, input send.
//!
//! The client sends its input to the server each tick and buffers incoming
//! delta-encoded snapshots. Each delta is applied to a local [`Baseline`]
//! to reconstruct the full server state. Between ticks the two most recent
//! snapshots are used to interpolate entity state for smooth rendering.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::World;
use crcbl_net::{
    Baseline, DeltaCodec, HandshakeResult, Hello, Message, MessageKind, ProtocolCompatibility,
    ResumeToken, SectorId, SessionId, Transport, TransportError,
};
use crcbl_phys::Transform;

// ---------------------------------------------------------------------------
// InterpolatedState
// ---------------------------------------------------------------------------

/// Interpolated state for rendering.
#[derive(Debug, Clone)]
pub struct InterpolatedState {
    /// Interpolated world-space transform for each entity, keyed by entity
    /// bits ([`crcbl_ecs::Entity::to_bits`]). Entities present in only one of
    /// the two buffered snapshots appear at their most recent transform.
    pub transforms: Vec<(u64 /* entity bits */, Transform)>,
}

/// Decode one system's snapshot blob into `entity_bits → Transform` pairs.
///
/// The blob is a sequence of `(entity_bits: u64, data_len: u32, data: [u8])`
/// triples (all little-endian) — the same framing `crcbl-net`'s delta codec
/// uses. Entries whose payload is not a valid [`Transform`] encoding (e.g.
/// the 4-byte synthetic count emitted for non-replicated systems) are
/// skipped.
fn decode_transforms(snapshot: &crcbl_net::ServerToClient) -> HashMap<u64, Transform> {
    let crcbl_net::ServerToClient::Snapshot { systems, .. } = snapshot else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for system in systems {
        let mut cursor = 0usize;
        while system.data.len() - cursor >= 12 {
            let entity_bits =
                u64::from_le_bytes(system.data[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let data_len =
                u32::from_le_bytes(system.data[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            if data_len > system.data.len() - cursor {
                break;
            }
            if let Some(transform) = Transform::decode(&system.data[cursor..cursor + data_len]) {
                out.insert(entity_bits, transform);
            }
            cursor += data_len;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// The rendering client: sends input to the server, applies incoming
/// delta-encoded snapshots, and provides interpolated entity state for smooth
/// frame-rate rendering.
pub struct Client<T: Transport> {
    /// Local ECS world (receives snapshots).
    world: World,
    /// Transport to the server.
    transport: T,
    /// Client-side frame clock for input-tick cadence and render alpha.
    clock: FrameClock,
    /// Sectors this client currently accepts replication for.
    subscribed_sectors: HashSet<SectorId>,
    /// Older full snapshots after delta apply, keyed by sector.
    prev_snapshots: HashMap<SectorId, crcbl_net::ServerToClient>,
    /// Newer full snapshots after delta apply, keyed by sector.
    current_snapshots: HashMap<SectorId, crcbl_net::ServerToClient>,
    /// Input data to send on the next tick.
    pending_input: Vec<u8>,
    /// Accumulated baselines for delta application, keyed by sector.
    baselines: HashMap<SectorId, Baseline>,
    session_id: Option<SessionId>,
    resume_token: Option<ResumeToken>,
    compatibility: ProtocolCompatibility,
    handshake_generation: u64,
    outstanding_handshake_generation: Option<u64>,
    handshake_complete: bool,
    handshake_rejected: bool,
    processing_error_count: u64,
}

impl<T: Transport> Client<T> {
    /// Create a client with the default protocol compatibility identifiers.
    ///
    /// # Panics
    ///
    /// Panics outside unit tests because the default embedding identifiers
    /// provide no compatibility protection.
    #[must_use]
    pub fn new(world: World, transport: T, tick_hz: u32) -> Self {
        let compatibility = ProtocolCompatibility::DEFAULT;
        if !cfg!(test) {
            compatibility.assert_explicit();
        }
        Self::new_with_compatibility(world, transport, tick_hz, compatibility)
    }

    /// Create a client with explicit protocol compatibility identifiers.
    #[must_use]
    pub fn new_with_compatibility(
        world: World,
        transport: T,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
    ) -> Self {
        if !cfg!(test) {
            compatibility.assert_explicit();
        }
        Self {
            world,
            transport,
            clock: FrameClock::new(tick_hz),
            subscribed_sectors: HashSet::from([SectorId::ZERO]),
            prev_snapshots: HashMap::new(),
            current_snapshots: HashMap::new(),
            pending_input: Vec::new(),
            baselines: HashMap::new(),
            session_id: None,
            resume_token: None,
            compatibility,
            handshake_generation: 0,
            outstanding_handshake_generation: None,
            handshake_complete: false,
            handshake_rejected: false,
            processing_error_count: 0,
        }
    }

    /// Feed the current time.
    ///
    /// Drains received snapshots into the buffer, sends pending input for
    /// each consumed tick, and returns the interpolation alpha in `[0, 1)`.
    pub fn update(&mut self, now: std::time::Duration) -> f32 {
        self.clock.update(now);
        if !self.handshake_complete
            && !self.handshake_rejected
            && self.outstanding_handshake_generation.is_none()
        {
            self.send_hello();
        }
        while self.clock.consume_tick() {
            let tick = self.clock.tick();
            if self.send_input(tick).is_err() {
                self.processing_error_count += 1;
            }
        }
        if self.recv_snapshots().is_err() {
            self.processing_error_count += 1;
        }
        self.clock.alpha()
    }

    /// Replace the accepted replication sector set.
    ///
    /// State for sectors omitted from the new set is dropped immediately.
    /// The default zero sector remains accepted only if included explicitly.
    pub fn set_subscribed_sectors(&mut self, sectors: impl IntoIterator<Item = SectorId>) {
        self.subscribed_sectors = sectors.into_iter().collect();
        self.baselines
            .retain(|sector, _| self.subscribed_sectors.contains(sector));
        self.prev_snapshots
            .retain(|sector, _| self.subscribed_sectors.contains(sector));
        self.current_snapshots
            .retain(|sector, _| self.subscribed_sectors.contains(sector));
    }

    /// Set the input data for the next tick.
    ///
    /// The data is sent (unreliably) to the server on each consumed tick
    /// until replaced by another `set_input` call.
    pub fn set_input(&mut self, input: Vec<u8>) {
        self.pending_input = input;
    }

    /// Interpolated per-entity transforms for rendering.
    ///
    /// Lerps between the two most recent buffered snapshots in the default
    /// sector at the given `alpha` (the value returned by [`Self::update`]).
    /// Entities present in only one snapshot appear at that snapshot's
    /// transform; with fewer than two snapshots the newest is used as-is.
    #[must_use]
    pub fn interpolate(&self, alpha: f32) -> InterpolatedState {
        let current = self
            .current_snapshots
            .get(&SectorId::ZERO)
            .map(decode_transforms)
            .unwrap_or_default();
        let prev = self
            .prev_snapshots
            .get(&SectorId::ZERO)
            .map(decode_transforms)
            .unwrap_or_default();

        let alpha = f64::from(alpha.clamp(0.0, 1.0));
        let mut transforms: Vec<(u64, Transform)> = current
            .iter()
            .map(|(&entity_bits, current_transform)| {
                let transform = prev
                    .get(&entity_bits)
                    .map_or(*current_transform, |prev_transform| {
                        prev_transform.lerp(current_transform, alpha)
                    });
                (entity_bits, transform)
            })
            .collect();
        for (&entity_bits, prev_transform) in &prev {
            if !current.contains_key(&entity_bits) {
                transforms.push((entity_bits, *prev_transform));
            }
        }
        transforms.sort_by_key(|(entity_bits, _)| *entity_bits);
        InterpolatedState { transforms }
    }

    /// The tick id of the most recently applied delta-encoded snapshot in the
    /// default sector.
    #[must_use]
    pub fn last_applied_tick(&self) -> TickId {
        self.last_applied_tick_in(SectorId::ZERO)
    }

    /// The tick id of the most recently applied delta-encoded snapshot in `sector`.
    #[must_use]
    pub fn last_applied_tick_in(&self, sector: SectorId) -> TickId {
        self.baselines
            .get(&sector)
            .map_or(TickId::ZERO, |baseline| baseline.tick)
    }

    /// Number of entities in the client's reconstructed baseline in the default
    /// sector.
    #[must_use]
    pub fn baseline_entity_count(&self) -> usize {
        self.baseline_entity_count_in(SectorId::ZERO)
    }

    /// Number of entities in the client's reconstructed baseline in `sector`.
    #[must_use]
    pub fn baseline_entity_count_in(&self, sector: SectorId) -> usize {
        self.baselines
            .get(&sector)
            .map_or(0, Baseline::entity_count)
    }

    /// Number of systems in the client's reconstructed baseline in the default
    /// sector.
    #[must_use]
    pub fn baseline_system_count(&self) -> usize {
        self.baseline_system_count_in(SectorId::ZERO)
    }

    /// Number of systems in the client's reconstructed baseline in `sector`.
    #[must_use]
    pub fn baseline_system_count_in(&self, sector: SectorId) -> usize {
        self.baselines
            .get(&sector)
            .map_or(0, Baseline::system_count)
    }

    /// Process-local hash of the reconstructed baseline in `sector`.
    #[must_use]
    pub fn baseline_state_hash_in(&self, sector: SectorId) -> Option<u64> {
        self.baselines.get(&sector).map(Baseline::state_hash)
    }

    /// The accepted server session id, if the handshake has completed.
    #[must_use]
    pub fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// Request a fresh handshake or resume the accepted session on a replacement
    /// transport. The caller must provide a newly connected transport.
    pub fn reconnect(&mut self, transport: T) {
        self.transport = transport;
        self.outstanding_handshake_generation = None;
        self.handshake_complete = false;
        self.handshake_rejected = false;
    }

    /// Number of unrecoverable transport, encoding, or decoding errors.
    #[must_use]
    pub fn processing_error_count(&self) -> u64 {
        self.processing_error_count
    }

    /// Whether the transport is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    /// Borrow the local world.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Mutably borrow the local world.
    #[must_use]
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Send a fresh or resume handshake.
    fn send_hello(&mut self) {
        self.handshake_generation = self
            .handshake_generation
            .checked_add(1)
            .expect("handshake generation exhausted; recreate the client");
        let generation = self.handshake_generation;
        let result = self.transport.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_hello(&Hello {
                protocol_version: self.compatibility.protocol_version,
                engine_build_id: self.compatibility.engine_build_id,
                schema_hash: self.compatibility.schema_hash,
                generation,
                session_token: self.resume_token,
            }),
        });
        if result.is_ok() {
            self.outstanding_handshake_generation = Some(generation);
        } else {
            self.processing_error_count += 1;
        }
    }

    /// Send pending input to the server for the given tick.
    fn send_input(&mut self, tick: TickId) -> Result<(), TransportError> {
        if self.pending_input.is_empty() {
            return Ok(());
        }
        let msg = crcbl_net::ClientToServer::Input {
            tick,
            data: self.pending_input.clone(),
        };
        let payload = crcbl_net::encode_client_to_server(&msg);
        self.transport.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
    }

    /// Drain available delta-encoded snapshots from the transport, apply
    /// them to the local baseline, and slide the two-slot interpolation
    /// buffer. Sends an ack for each applied tick.
    fn recv_snapshots(&mut self) -> Result<(), TransportError> {
        while let Some(msg) = self.transport.recv()? {
            if let Ok(result) = crcbl_net::decode_handshake_result(&msg.payload) {
                let generation = match &result {
                    HandshakeResult::Accept { generation, .. }
                    | HandshakeResult::Reject { generation, .. } => *generation,
                };
                if self.outstanding_handshake_generation != Some(generation) {
                    self.processing_error_count += 1;
                    continue;
                }
                self.outstanding_handshake_generation = None;
                match result {
                    HandshakeResult::Accept {
                        session_id,
                        resume_token,
                        ..
                    } => {
                        self.session_id = Some(session_id);
                        self.resume_token = Some(resume_token);
                        self.handshake_complete = true;
                    }
                    HandshakeResult::Reject { .. } => {
                        self.handshake_rejected = true;
                        self.processing_error_count += 1;
                    }
                }
                continue;
            }

            let delta = match if self.handshake_complete {
                crcbl_net::decode_trusted_delta(&msg.payload)
            } else {
                crcbl_net::decode_delta(&msg.payload)
            } {
                Ok(delta) => delta,
                Err(_) => {
                    self.processing_error_count += 1;
                    continue;
                }
            };

            let sector = delta.sector;
            if !self.subscribed_sectors.contains(&sector) {
                self.processing_error_count += 1;
                continue;
            }
            let mut baseline = self.baselines.get(&sector).cloned().unwrap_or_else(|| {
                Baseline::from_trusted_snapshot(TickId::ZERO, &[]).expect("empty snapshot is valid")
            });
            if delta.tick <= baseline.tick {
                continue;
            }
            if !delta.is_keyframe && delta.baseline_tick != Some(baseline.tick) {
                let ack_tick = delta
                    .baseline_tick
                    .filter(|baseline_tick| *baseline_tick < baseline.tick)
                    .map(|_| baseline.tick);
                if let Some(ack_tick) = ack_tick {
                    self.send_ack(sector, ack_tick);
                }
                continue;
            }
            if delta.is_keyframe && delta.baseline_tick.is_some() {
                self.processing_error_count += 1;
                continue;
            }

            // Apply the delta to this sector's baseline, reconstruct full snapshots.
            let full_snapshots = match if self.handshake_complete {
                DeltaCodec::apply_trusted(&delta, &mut baseline)
            } else {
                DeltaCodec::apply(&delta, &mut baseline)
            } {
                Ok(full_snapshots) => full_snapshots,
                Err(_) => {
                    self.processing_error_count += 1;
                    continue;
                }
            };

            self.baselines.insert(sector, baseline);

            // Send an ack for this sector and tick.
            self.send_ack(sector, delta.tick);

            // Build a ServerToClient from the reconstructed full snapshot
            // for the interpolation buffer.
            let snapshot = crcbl_net::ServerToClient::Snapshot {
                sector,
                tick: delta.tick,
                systems: full_snapshots,
            };

            let is_newer = self.current_snapshots.get(&sector).is_none_or(|current| {
                matches!(
                    current,
                    crcbl_net::ServerToClient::Snapshot {
                        tick: current_tick,
                        ..
                    } if delta.tick > *current_tick
                )
            });
            if is_newer && let Some(current) = self.current_snapshots.insert(sector, snapshot) {
                self.prev_snapshots.insert(sector, current);
            }
        }
        Ok(())
    }

    fn send_ack(&mut self, sector: SectorId, tick: TickId) {
        if self
            .transport
            .send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: crcbl_net::encode_ack(sector, tick),
            })
            .is_err()
        {
            self.processing_error_count += 1;
        }
    }
}

impl<T: Transport> fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prev_tick = self.prev_snapshots.get(&SectorId::ZERO).map(|snapshot| {
            let crcbl_net::ServerToClient::Snapshot { tick, .. } = snapshot else {
                unreachable!("snapshot buffers only contain snapshots");
            };
            *tick
        });
        let current_tick = self.current_snapshots.get(&SectorId::ZERO).map(|snapshot| {
            let crcbl_net::ServerToClient::Snapshot { tick, .. } = snapshot else {
                unreachable!("snapshot buffers only contain snapshots");
            };
            *tick
        });
        f.debug_struct("Client")
            .field("world", &self.world)
            .field("clock", &self.clock)
            .field("connected", &self.transport.is_connected())
            .field("prev_snapshot_tick", &prev_tick)
            .field("current_snapshot_tick", &current_tick)
            .field("pending_input_len", &self.pending_input.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_net::InMemoryTransport;

    // ── Helpers ────────────────────────────────────────────────────────────

    fn empty_world() -> World {
        World::new()
    }

    /// Build a delta-encoded payload representing a snapshot with
    /// `system_count` at `tick` (all entities in `added` — keyframe shape).
    fn keyframe_snapshot_for_sector(
        sector: SectorId,
        tick: u64,
        system_data: &[(u32, Vec<u8>)],
    ) -> Vec<u8> {
        let snapshots: Vec<_> = system_data
            .iter()
            .map(|&(sys_id, ref data)| {
                let mut entity_blob = data.clone();
                let mut cursor = 0usize;
                let is_entity_blob = loop {
                    if cursor == entity_blob.len() {
                        break true;
                    }
                    if entity_blob.len() - cursor < 12 {
                        break false;
                    }
                    let len = u32::from_le_bytes(
                        entity_blob[cursor + 8..cursor + 12].try_into().unwrap(),
                    ) as usize;
                    cursor += 12;
                    if len > entity_blob.len() - cursor {
                        break false;
                    }
                    cursor += len;
                };
                if !is_entity_blob {
                    entity_blob.clear();
                    entity_blob.extend_from_slice(&0u64.to_le_bytes());
                    entity_blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
                    entity_blob.extend_from_slice(data);
                }
                crcbl_net::SystemSnapshot {
                    system_id: sys_id,
                    data: entity_blob,
                }
            })
            .collect();
        let delta =
            DeltaCodec::encode_with_sector(sector, TickId::from_raw(tick), &snapshots, None)
                .expect("valid snapshot");
        crcbl_net::encode_delta(&delta).expect("valid delta")
    }

    fn keyframe_snapshot(tick: u64, system_data: &[(u32, Vec<u8>)]) -> Vec<u8> {
        keyframe_snapshot_for_sector(SectorId::ZERO, tick, system_data)
    }

    fn delta_payload(delta: crcbl_net::Delta) -> Vec<u8> {
        crcbl_net::encode_delta(&delta).expect("valid delta")
    }

    #[test]
    fn rejects_reordered_deltas_and_reacks_for_an_older_baseline() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: keyframe_snapshot(5, &[]),
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        assert_eq!(
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload)
                .unwrap()
                .tick,
            TickId::from_raw(5)
        );
        let _ = peer.recv().unwrap().unwrap();

        let wrong_baseline = crcbl_net::Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(6),
            baseline_tick: Some(TickId::from_raw(4)),
            is_keyframe: false,
            systems: Vec::new(),
        };
        let stale = crcbl_net::Delta {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(5),
            baseline_tick: Some(TickId::from_raw(5)),
            is_keyframe: false,
            systems: Vec::new(),
        };
        for delta in [wrong_baseline, stale] {
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: delta_payload(delta),
            })
            .unwrap();
        }
        client.update(std::time::Duration::from_nanos(1));

        assert_eq!(client.last_applied_tick(), TickId::from_raw(5));
        assert_eq!(
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload)
                .unwrap()
                .tick,
            TickId::from_raw(5)
        );
        assert!(peer.recv().unwrap().is_none());
    }

    #[test]
    fn reacks_current_baseline_when_server_uses_an_older_baseline() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: keyframe_snapshot(1, &[]),
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        let _ = peer.recv().unwrap().unwrap();

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: delta_payload(crcbl_net::Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(2),
                baseline_tick: Some(TickId::from_raw(1)),
                is_keyframe: false,
                systems: Vec::new(),
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(1));
        let dropped_ack = peer.recv().unwrap().unwrap();
        assert_eq!(
            crcbl_net::decode_ack(&dropped_ack.payload).unwrap().tick,
            TickId::from_raw(2)
        );

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: delta_payload(crcbl_net::Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(3),
                baseline_tick: Some(TickId::from_raw(1)),
                is_keyframe: false,
                systems: Vec::new(),
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(2));

        assert_eq!(client.last_applied_tick(), TickId::from_raw(2));
        assert_eq!(
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload)
                .unwrap()
                .tick,
            TickId::from_raw(2)
        );
    }

    #[test]
    fn accepts_matching_baseline_delta_and_acks_it() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: keyframe_snapshot(1, &[]),
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        let _ = peer.recv().unwrap().unwrap();

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: delta_payload(crcbl_net::Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(2),
                baseline_tick: Some(TickId::from_raw(1)),
                is_keyframe: false,
                systems: Vec::new(),
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(1));

        assert_eq!(client.last_applied_tick(), TickId::from_raw(2));
        assert_eq!(
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload)
                .unwrap()
                .tick,
            TickId::from_raw(2)
        );
    }

    // ── Creation ───────────────────────────────────────────────────────────

    #[test]
    fn client_creates_with_empty_buffers() {
        let (transport, _peer) = InMemoryTransport::pair();
        let client = Client::new(empty_world(), transport, 60);
        assert!(client.is_connected());
        let debug = format!("{client:?}");
        assert!(debug.contains("prev_snapshot_tick: None"));
        assert!(debug.contains("current_snapshot_tick: None"));
    }

    // ── Input send ─────────────────────────────────────────────────────────

    #[test]
    fn set_input_sends_on_tick() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        client.set_input(vec![1, 2, 3]);

        client.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        client.update(tick_dt);

        let mut peer = server_transport;
        let msg = peer.recv().unwrap().unwrap();
        assert_eq!(msg.kind, MessageKind::Unreliable);
        assert!(!msg.payload.is_empty());
    }

    #[test]
    fn no_input_sends_nothing() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        client.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        client.update(tick_dt);

        let mut peer = server_transport;
        let hello = peer.recv().unwrap().unwrap();
        assert!(crcbl_net::decode_hello(&hello.payload).is_ok());
        assert!(peer.recv().unwrap().is_none());
    }

    #[test]
    fn set_input_persists_across_ticks() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        client.set_input(vec![42]);

        client.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(33_333_334);
        let alpha = client.update(tick_dt);

        let mut peer = server_transport;
        let msg1 = peer.recv().unwrap().unwrap();
        let msg2 = peer.recv().unwrap().unwrap();
        assert_eq!(msg1.kind, MessageKind::Unreliable);
        assert_eq!(msg2.kind, MessageKind::Unreliable);
        assert!((0.0..1.0).contains(&alpha));
    }

    // ── Snapshot receive (delta-encoded) ───────────────────────────────────

    #[test]
    fn receives_delta_into_buffer() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let payload = keyframe_snapshot(1, &[(0, vec![1, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
        .unwrap();
        drop(peer);

        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(1))"));
    }

    #[test]
    fn client_sends_ack_after_applying_delta() {
        let (client_transport, mut server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        // Send a keyframe snapshot from server side.
        {
            let payload = keyframe_snapshot(1, &[(0, vec![1, 0, 0, 0])]);
            server_transport
                .send_unreliable(Message {
                    kind: MessageKind::Unreliable,
                    payload,
                })
                .unwrap();
        }

        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        // The client should have sent an ack back.
        let ack_msg = server_transport.recv().unwrap().unwrap();
        let ack_tick = crcbl_net::decode_ack(&ack_msg.payload).unwrap().tick;
        assert_eq!(ack_tick, TickId::from_raw(1));
    }

    #[test]
    fn reconstructs_same_tick_different_sector_values_independently() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);
        let left = SectorId { x: 1, y: 0, z: 0 };
        let right = SectorId { x: 2, y: 0, z: 0 };
        client.set_subscribed_sectors([left, right]);

        for (sector, value) in [(left, 11u32), (right, 22u32)] {
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: keyframe_snapshot_for_sector(
                    sector,
                    7,
                    &[(42, value.to_le_bytes().to_vec())],
                ),
            })
            .unwrap();
        }
        client.update(std::time::Duration::ZERO);

        let first_ack = crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload).unwrap();
        let second_ack = crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload).unwrap();
        assert_eq!(first_ack.tick, TickId::from_raw(7));
        assert_eq!(second_ack.tick, TickId::from_raw(7));
        assert_ne!(first_ack.sector, second_ack.sector);
        assert_eq!(client.last_applied_tick_in(left), TickId::from_raw(7));
        assert_eq!(client.last_applied_tick_in(right), TickId::from_raw(7));
        assert_eq!(client.baseline_entity_count_in(left), 1);
        assert_eq!(client.baseline_entity_count_in(right), 1);
        assert_ne!(
            client.baseline_state_hash_in(left),
            client.baseline_state_hash_in(right),
            "same tick and system id with distinct values must retain independent baselines"
        );
        assert!(client.current_snapshots.contains_key(&left));
        assert!(client.current_snapshots.contains_key(&right));
    }

    #[test]
    fn unknown_sectors_do_not_allocate_replication_state() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        for x in 1..=100 {
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: keyframe_snapshot_for_sector(SectorId { x, y: 0, z: 0 }, 1, &[]),
            })
            .unwrap();
        }
        client.update(std::time::Duration::ZERO);

        assert!(client.baselines.is_empty());
        assert!(client.current_snapshots.is_empty());
        assert!(client.prev_snapshots.is_empty());
        assert_eq!(client.processing_error_count(), 100);
        let hello = peer
            .recv()
            .unwrap()
            .expect("client sends its initial hello");
        assert!(crcbl_net::decode_hello(&hello.payload).is_ok());
        assert!(peer.recv().unwrap().is_none());
    }

    #[test]
    fn changing_subscriptions_drops_inactive_sector_state() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);
        let sector = SectorId { x: 1, y: 0, z: 0 };
        client.set_subscribed_sectors([sector]);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: keyframe_snapshot_for_sector(sector, 1, &[]),
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        assert!(client.baselines.contains_key(&sector));

        client.set_subscribed_sectors([]);
        assert!(client.baselines.is_empty());
        assert!(client.current_snapshots.is_empty());
        assert!(client.prev_snapshots.is_empty());
    }

    // ── Interpolation buffer sliding (delta-encoded) ───────────────────────

    #[test]
    fn newer_snapshot_slides_buffer() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let payload1 = keyframe_snapshot(1, &[(0, vec![1, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload1,
        })
        .unwrap();

        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(1))"));
        assert!(debug.contains("prev_snapshot_tick: None"));

        let payload2 = keyframe_snapshot(2, &[(0, vec![2, 0, 0, 0])]);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload2,
        })
        .unwrap();
        drop(peer);

        client.update(std::time::Duration::from_nanos(1));

        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(2))"));
        assert!(debug.contains("prev_snapshot_tick: Some(TickId(1))"));
    }

    #[test]
    fn older_snapshot_does_not_slide_buffer() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let payload5 = keyframe_snapshot(5, &[(0, vec![5, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload5,
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        let payload3 = keyframe_snapshot(3, &[(0, vec![3, 0, 0, 0])]);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload3,
        })
        .unwrap();
        drop(peer);

        client.update(std::time::Duration::from_nanos(1));

        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(5))"));
    }

    // ── Interpolation alpha ────────────────────────────────────────────────

    #[test]
    fn alpha_is_zero_at_tick_boundary() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), transport, 60);

        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        let alpha = client.update(tick_dt);
        assert!((alpha - 0.0).abs() < 0.01, "alpha was {alpha}");
    }

    #[test]
    fn alpha_grows_between_ticks() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), transport, 60);

        client.update(std::time::Duration::ZERO);

        let half_tick = std::time::Duration::from_nanos(8_333_333);
        let alpha = client.update(std::time::Duration::ZERO + half_tick);
        assert!((alpha - 0.5).abs() < 0.01, "expected ~0.5, got {alpha}");
    }

    #[test]
    fn interpolate_returns_empty_when_no_snapshots() {
        let (transport, _peer) = InMemoryTransport::pair();
        let client = Client::new(empty_world(), transport, 60);
        let state = client.interpolate(0.5);
        assert!(state.transforms.is_empty());
    }

    #[test]
    fn interpolate_skips_non_transform_payloads() {
        // The synthetic count payload emitted for non-replicated systems is
        // 4 bytes — not a Transform encoding — so it yields no entities.
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let mut peer = server_transport;
        for tick in [1, 2] {
            let payload = keyframe_snapshot(tick, &[(0, vec![1, 0, 0, 0])]);
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload,
            })
            .unwrap();
        }
        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));
        client.update(std::time::Duration::from_nanos(2));
        drop(peer);

        let state = client.interpolate(0.5);
        assert!(state.transforms.is_empty());
    }

    #[test]
    fn interpolate_lerps_transforms_between_snapshots() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let entity_bits = (1u64 << 32) | 7;
        for (tick, x) in [(1u64, 0.0f64), (2, 10.0)] {
            let mut data = Vec::new();
            Transform::from_position(glam::DVec3::new(x, 1.0, -2.0)).encode(&mut data);
            let mut blob = Vec::new();
            blob.extend_from_slice(&entity_bits.to_le_bytes());
            blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
            blob.extend_from_slice(&data);
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: keyframe_snapshot(tick, &[(0, blob)]),
            })
            .unwrap();
            client.update(std::time::Duration::from_nanos(tick));
        }

        let state = client.interpolate(0.25);
        assert_eq!(state.transforms.len(), 1);
        let (bits, transform) = state.transforms[0];
        assert_eq!(bits, entity_bits);
        assert!((transform.position.x - 2.5).abs() < 1e-9);
        assert!((transform.position.y - 1.0).abs() < 1e-9);
        assert!((transform.position.z - -2.0).abs() < 1e-9);

        // alpha = 1 snaps to the newest snapshot; alpha = 0 to the oldest.
        let newest = client.interpolate(1.0).transforms[0].1;
        assert!((newest.position.x - 10.0).abs() < 1e-9);
        let oldest = client.interpolate(0.0).transforms[0].1;
        assert!((oldest.position.x - 0.0).abs() < 1e-9);
    }

    #[test]
    fn interpolate_keeps_entities_present_in_only_one_snapshot() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let gone = (1u64 << 32) | 1;
        let appeared = (1u64 << 32) | 2;
        let encode_blob = |entities: &[(u64, f64)]| {
            let mut blob = Vec::new();
            for &(bits, x) in entities {
                let mut data = Vec::new();
                Transform::from_position(glam::DVec3::new(x, 0.0, 0.0)).encode(&mut data);
                blob.extend_from_slice(&bits.to_le_bytes());
                blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
                blob.extend_from_slice(&data);
            }
            blob
        };

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: keyframe_snapshot(1, &[(0, encode_blob(&[(gone, 5.0)]))]),
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: keyframe_snapshot(2, &[(0, encode_blob(&[(appeared, 7.0)]))]),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(1));

        let state = client.interpolate(0.5);
        assert_eq!(state.transforms.len(), 2);
        // Despawned entity holds its last known transform; newly spawned
        // appears at its newest.
        assert_eq!(state.transforms[0].0, gone);
        assert!((state.transforms[0].1.position.x - 5.0).abs() < 1e-9);
        assert_eq!(state.transforms[1].0, appeared);
        assert!((state.transforms[1].1.position.x - 7.0).abs() < 1e-9);
    }

    // ── last_applied_tick / baseline_entity_count ──────────────────────────

    #[test]
    fn last_applied_tick_starts_at_zero() {
        let (transport, _peer) = InMemoryTransport::pair();
        let client = Client::new(empty_world(), transport, 60);
        assert_eq!(client.last_applied_tick(), TickId::ZERO);
    }

    #[test]
    fn last_applied_tick_advances_after_delta_apply() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let payload = keyframe_snapshot(1, &[(0, vec![1, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
        .unwrap();
        drop(peer);

        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        assert_eq!(client.last_applied_tick(), TickId::from_raw(1));
    }

    #[test]
    fn baseline_entity_count_tracks_applied_snapshot() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        // Build a snapshot with 3 entities in system 1.
        let mut data = Vec::new();
        for i in 0u64..3u64 {
            let component = (i * 10) as u32;
            data.extend_from_slice(&i.to_le_bytes());
            data.extend_from_slice(&4u32.to_le_bytes());
            data.extend_from_slice(&component.to_le_bytes());
        }
        let payload = keyframe_snapshot(5, &[(1, data)]);

        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
        .unwrap();
        drop(peer);

        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        assert_eq!(client.baseline_entity_count(), 3);
    }

    #[test]
    fn rejects_stale_and_unsolicited_handshake_accepts() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        client.update(std::time::Duration::ZERO);
        let initial_hello =
            crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload).unwrap();
        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_handshake_result(&HandshakeResult::Accept {
                generation: initial_hello.generation,
                session_id: SessionId(1),
                resume_token: ResumeToken::from_bytes([1; 32]),
                server_tick: TickId::ZERO,
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(1));
        assert_eq!(client.session_id(), Some(SessionId(1)));
        assert_eq!(client.resume_token, Some(ResumeToken::from_bytes([1; 32])));

        let (client_transport, mut peer) = InMemoryTransport::pair();
        client.reconnect(client_transport);
        client.update(std::time::Duration::from_nanos(2));
        let reconnect_hello =
            crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload).unwrap();
        assert!(reconnect_hello.generation > initial_hello.generation);

        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_handshake_result(&HandshakeResult::Accept {
                generation: initial_hello.generation,
                session_id: SessionId(99),
                resume_token: ResumeToken::from_bytes([9; 32]),
                server_tick: TickId::ZERO,
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(3));
        assert_eq!(client.session_id(), Some(SessionId(1)));
        assert_eq!(client.resume_token, Some(ResumeToken::from_bytes([1; 32])));
        assert_eq!(client.processing_error_count(), 1);

        let oversized_system_set: Vec<_> = (0..=crcbl_net::MAX_BASELINE_SYSTEMS as u32)
            .map(|system_id| (system_id, Vec::new()))
            .collect();
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: keyframe_snapshot(1, &oversized_system_set),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(4));
        assert_eq!(client.last_applied_tick(), TickId::ZERO);
        assert_eq!(client.processing_error_count(), 2);

        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_handshake_result(&HandshakeResult::Accept {
                generation: reconnect_hello.generation,
                session_id: SessionId(1),
                resume_token: ResumeToken::from_bytes([2; 32]),
                server_tick: TickId::ZERO,
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(5));
        assert_eq!(client.session_id(), Some(SessionId(1)));
        assert_eq!(client.resume_token, Some(ResumeToken::from_bytes([2; 32])));

        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_handshake_result(&HandshakeResult::Accept {
                generation: reconnect_hello.generation,
                session_id: SessionId(100),
                resume_token: ResumeToken::from_bytes([10; 32]),
                server_tick: TickId::ZERO,
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(6));
        assert_eq!(client.session_id(), Some(SessionId(1)));
        assert_eq!(client.resume_token, Some(ResumeToken::from_bytes([2; 32])));
        assert_eq!(client.processing_error_count(), 3);
    }

    #[test]
    fn matching_reject_is_terminal_and_stale_reject_preserves_new_handshake() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        client.update(std::time::Duration::ZERO);
        let initial_generation = crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload)
            .unwrap()
            .generation;
        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_handshake_result(&HandshakeResult::Reject {
                generation: initial_generation,
                reason: crcbl_net::RejectReason {
                    code: 1,
                    msg: "incompatible".into(),
                },
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(1));
        client.update(std::time::Duration::from_nanos(2));
        assert!(peer.recv().unwrap().is_none());
        assert_eq!(client.session_id(), None);
        assert_eq!(client.processing_error_count(), 1);

        let (client_transport, mut peer) = InMemoryTransport::pair();
        client.reconnect(client_transport);
        client.update(std::time::Duration::from_nanos(3));
        let next_generation = crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload)
            .unwrap()
            .generation;
        assert!(next_generation > initial_generation);

        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_handshake_result(&HandshakeResult::Reject {
                generation: initial_generation,
                reason: crcbl_net::RejectReason {
                    code: 1,
                    msg: "stale".into(),
                },
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(4));
        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_handshake_result(&HandshakeResult::Accept {
                generation: next_generation,
                session_id: SessionId(1),
                resume_token: ResumeToken::from_bytes([1; 32]),
                server_tick: TickId::ZERO,
            }),
        })
        .unwrap();
        client.update(std::time::Duration::from_nanos(5));
        assert_eq!(client.session_id(), Some(SessionId(1)));
        assert_eq!(client.processing_error_count(), 2);
    }

    #[test]
    #[should_panic(expected = "handshake generation exhausted; recreate the client")]
    fn handshake_generation_never_wraps() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), transport, 60);
        client.handshake_generation = u64::MAX;
        client.send_hello();
    }

    // ── Debug ──────────────────────────────────────────────────────────────

    #[test]
    fn debug_format() {
        let (transport, _peer) = InMemoryTransport::pair();
        let client = Client::new(empty_world(), transport, 60);
        let s = format!("{client:?}");
        assert!(s.contains("Client"));
        assert!(s.contains("connected"));
    }
}
