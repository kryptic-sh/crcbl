//! Rendering client: delta-apply, interpolation buffer, input send.
//!
//! The client sends its input to the server each tick and buffers incoming
//! delta-encoded snapshots. Each delta is applied to a local [`Baseline`]
//! to reconstruct the full server state. Between ticks the two most recent
//! snapshots are used to interpolate entity state for smooth rendering.

use std::fmt;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::World;
use crcbl_net::{
    Baseline, DeltaCodec, HandshakeResult, Hello, Message, MessageKind, ProtocolCompatibility,
    ResumeToken, SessionId, Transport, TransportError,
};
use glam::Vec3;

// ---------------------------------------------------------------------------
// InterpolatedState
// ---------------------------------------------------------------------------

/// Interpolated state for rendering.
#[derive(Debug, Clone)]
pub struct InterpolatedState {
    /// Current interpolated position for each entity (entity_bits -> Vec3).
    ///
    /// For now, stores entity ids; real per-component interpolation lands
    /// with the replication encoding in P3.
    pub positions: Vec<(u64 /* entity bits */, Vec3)>,
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
    /// The older of the two most recent full snapshots (after delta apply).
    prev_snapshot: Option<crcbl_net::ServerToClient>,
    /// The newer of the two most recent full snapshots (after delta apply).
    current_snapshot: Option<crcbl_net::ServerToClient>,
    /// Input data to send on the next tick.
    pending_input: Vec<u8>,
    /// Accumulated baseline for delta application — the client's mirror
    /// of the server's world state.
    baseline: Baseline,
    session_id: Option<SessionId>,
    resume_token: Option<ResumeToken>,
    compatibility: ProtocolCompatibility,
    hello_sent: bool,
    processing_error_count: u64,
}

impl<T: Transport> Client<T> {
    /// Create a client with the default protocol compatibility identifiers.
    #[must_use]
    pub fn new(world: World, transport: T, tick_hz: u32) -> Self {
        Self::new_with_compatibility(world, transport, tick_hz, ProtocolCompatibility::DEFAULT)
    }

    /// Create a client with explicit protocol compatibility identifiers.
    #[must_use]
    pub fn new_with_compatibility(
        world: World,
        transport: T,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
    ) -> Self {
        Self {
            world,
            transport,
            clock: FrameClock::new(tick_hz),
            prev_snapshot: None,
            current_snapshot: None,
            pending_input: Vec::new(),
            baseline: Baseline::from_trusted_snapshot(TickId::ZERO, &[])
                .expect("empty snapshot is valid"),
            session_id: None,
            resume_token: None,
            compatibility,
            hello_sent: false,
            processing_error_count: 0,
        }
    }

    /// Feed the current time.
    ///
    /// Drains received snapshots into the buffer, sends pending input for
    /// each consumed tick, and returns the interpolation alpha in `[0, 1)`.
    pub fn update(&mut self, now: std::time::Duration) -> f32 {
        self.clock.update(now);
        if !self.hello_sent {
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

    /// Set the input data for the next tick.
    ///
    /// The data is sent (unreliably) to the server on each consumed tick
    /// until replaced by another `set_input` call.
    pub fn set_input(&mut self, input: Vec<u8>) {
        self.pending_input = input;
    }

    /// Stub: always returns an empty [`InterpolatedState`].
    ///
    /// Real interpolation that lerps entity positions between the two most
    /// recent snapshots lands when snapshots carry per-entity component data
    /// rather than just entity counts (P3).
    #[must_use]
    pub fn interpolate(&self) -> InterpolatedState {
        InterpolatedState {
            positions: Vec::new(),
        }
    }

    /// The tick id of the most recently applied delta-encoded snapshot.
    #[must_use]
    pub fn last_applied_tick(&self) -> TickId {
        self.baseline.tick
    }

    /// Number of entities in the client's reconstructed baseline across
    /// all systems.
    #[must_use]
    pub fn baseline_entity_count(&self) -> usize {
        self.baseline.entity_count()
    }

    /// Number of systems in the client's reconstructed baseline.
    #[must_use]
    pub fn baseline_system_count(&self) -> usize {
        self.baseline.system_count()
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
        self.hello_sent = false;
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
        let result = self.transport.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_hello(&Hello {
                protocol_version: self.compatibility.protocol_version,
                engine_build_id: self.compatibility.engine_build_id,
                schema_hash: self.compatibility.schema_hash,
                session_token: self.resume_token,
            }),
        });
        if result.is_ok() {
            self.hello_sent = true;
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
                match result {
                    HandshakeResult::Accept {
                        session_id,
                        resume_token,
                        ..
                    } => {
                        self.session_id = Some(session_id);
                        self.resume_token = Some(resume_token);
                    }
                    HandshakeResult::Reject { .. } => self.processing_error_count += 1,
                }
                continue;
            }

            let delta = match if self.session_id.is_some() {
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

            if delta.tick <= self.baseline.tick {
                continue;
            }
            if !delta.is_keyframe && delta.baseline_tick != Some(self.baseline.tick) {
                if delta
                    .baseline_tick
                    .is_some_and(|baseline_tick| baseline_tick < self.baseline.tick)
                {
                    self.send_ack(self.baseline.tick);
                }
                continue;
            }
            if delta.is_keyframe && delta.baseline_tick.is_some() {
                self.processing_error_count += 1;
                continue;
            }

            // Apply the delta to our baseline, reconstruct full snapshots.
            let full_snapshots = match if self.session_id.is_some() {
                DeltaCodec::apply_trusted(&delta, &mut self.baseline)
            } else {
                DeltaCodec::apply(&delta, &mut self.baseline)
            } {
                Ok(full_snapshots) => full_snapshots,
                Err(_) => {
                    self.processing_error_count += 1;
                    continue;
                }
            };

            // Send ack for this tick.
            self.send_ack(delta.tick);

            // Build a ServerToClient from the reconstructed full snapshot
            // for the interpolation buffer.
            let snapshot = crcbl_net::ServerToClient::Snapshot {
                sector: crcbl_net::SectorId::ZERO,
                tick: delta.tick,
                systems: full_snapshots,
            };

            let is_newer = match &self.current_snapshot {
                Some(crcbl_net::ServerToClient::Snapshot {
                    tick: current_tick, ..
                }) => delta.tick > *current_tick,
                _ => true,
            };
            if is_newer {
                self.prev_snapshot = self.current_snapshot.take();
                self.current_snapshot = Some(snapshot);
            }
        }
        Ok(())
    }

    fn send_ack(&mut self, tick: TickId) {
        if self
            .transport
            .send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: crcbl_net::encode_ack(tick),
            })
            .is_err()
        {
            self.processing_error_count += 1;
        }
    }
}

impl<T: Transport> fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prev_tick = match &self.prev_snapshot {
            Some(crcbl_net::ServerToClient::Snapshot { tick, .. }) => Some(*tick),
            _ => None,
        };
        let current_tick = match &self.current_snapshot {
            Some(crcbl_net::ServerToClient::Snapshot { tick, .. }) => Some(*tick),
            _ => None,
        };
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
    fn keyframe_snapshot(tick: u64, system_data: &[(u32, Vec<u8>)]) -> Vec<u8> {
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
            DeltaCodec::encode(TickId::from_raw(tick), &snapshots, None).expect("valid snapshot");
        crcbl_net::encode_delta(&delta).expect("valid delta")
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
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload).unwrap(),
            TickId::from_raw(5)
        );
        let _ = peer.recv().unwrap().unwrap();

        let wrong_baseline = crcbl_net::Delta {
            tick: TickId::from_raw(6),
            baseline_tick: Some(TickId::from_raw(4)),
            is_keyframe: false,
            systems: Vec::new(),
        };
        let stale = crcbl_net::Delta {
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
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload).unwrap(),
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
            crcbl_net::decode_ack(&dropped_ack.payload).unwrap(),
            TickId::from_raw(2)
        );

        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: delta_payload(crcbl_net::Delta {
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
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload).unwrap(),
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
            crcbl_net::decode_ack(&peer.recv().unwrap().unwrap().payload).unwrap(),
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
        let ack_tick = crcbl_net::decode_ack(&ack_msg.payload).unwrap();
        assert_eq!(ack_tick, TickId::from_raw(1));
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
        let state = client.interpolate();
        assert!(state.positions.is_empty());
    }

    #[test]
    fn interpolate_is_stub_returns_empty_even_with_two_snapshots() {
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

        let payload2 = keyframe_snapshot(2, &[(0, vec![1, 0, 0, 0])]);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload2,
        })
        .unwrap();
        drop(peer);
        client.update(std::time::Duration::from_nanos(1));

        let state = client.interpolate();
        assert!(state.positions.is_empty(), "stub returns empty");
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
