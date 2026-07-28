//! Authoritative server: fixed-tick simulation loop and snapshot emission.
//!
//! The server is the single source of truth. Each tick it drains client inputs,
//! advances the ECS world, and broadcasts a per-system snapshot over an
//! unreliable transport channel. Snapshots are delta-encoded against the
//! client's last-acked baseline (P2b protocol).

pub mod sim_hash;

use std::fmt;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{Inspector, World};
use crcbl_net::{
    Baseline, DeltaCodec, Message, MessageKind, SessionConfig, SessionId, SessionManager,
    SnapshotWriter, Transport,
};

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// The authoritative server: owns the ECS [`World`], drives the fixed-tick
/// simulation loop, and emits per-tick delta-encoded snapshots over the
/// [`Transport`].
pub struct Server<T: Transport> {
    world: World,
    transport: T,
    clock: FrameClock,
    session: SessionManager,
}

impl<T: Transport> Server<T> {
    /// Create a server running at `tick_hz` (e.g. 60).
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    #[must_use]
    pub fn new(world: World, transport: T, tick_hz: u32) -> Self {
        let config = SessionConfig::default();
        // Single-client server: use a fixed session id.
        let session_id = SessionId(1);
        Self {
            world,
            transport,
            clock: FrameClock::new(tick_hz),
            session: SessionManager::new(session_id, &config),
        }
    }

    /// Feed the current time from a [`crcbl_core::time::TimeSource`].
    ///
    /// Returns how many ticks ran this frame.
    pub fn update(&mut self, now: std::time::Duration) -> u32 {
        self.clock.update(now);
        let mut ticks = 0u32;
        while self.clock.consume_tick() {
            self.tick();
            ticks += 1;
        }
        ticks
    }

    /// Run one tick: consume inputs from transport (including acks), tick the
    /// world, emit delta-encoded snapshot.
    fn tick(&mut self) {
        self.drain_inputs();
        self.world.tick();
        self.emit_snapshot();
    }

    /// Consume queued client messages: inputs (discarded — P3) and acks.
    fn drain_inputs(&mut self) {
        loop {
            match self.transport.recv() {
                Ok(Some(msg)) => {
                    // Try to decode as ack (tag 0x30). If that fails, try
                    // client-to-server (input/command). Either way, no error
                    // means the message is consumed.
                    if let Ok(tick) = crcbl_net::decode_ack(&msg.payload) {
                        self.session.handle_ack(tick);
                    } else {
                        let _ = crcbl_net::decode_client_to_server(&msg.payload);
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    /// Build snapshots from the ECS schedule, delta-encode against the
    /// client's baseline, and send.
    fn emit_snapshot(&mut self) {
        let tick = self.clock.tick();
        let mut writer = SnapshotWriter::new(tick);

        let stats = Inspector::collect(&self.world);
        for (idx, stat) in stats.iter().enumerate() {
            // Payload: entity count (4 bytes LE) — real per-entity component
            // data lands with P3 replication encoding.
            let mut data = Vec::with_capacity(4);
            data.extend_from_slice(&(stat.entity_count as u32).to_le_bytes());
            writer.write_system(idx as u32, data);
        }

        let snapshot = writer.finish();
        let systems: Vec<_> = match &snapshot {
            crcbl_net::ServerToClient::Snapshot { systems, .. } => systems.to_vec(),
            _ => return,
        };

        // Delta-encode against the client's baseline.
        let baseline = match Baseline::from_snapshot(tick, &systems) {
            Ok(baseline) => baseline,
            Err(_) => return,
        };
        let last_acked = self.session.last_acked_tick();
        let previous = last_acked.and_then(|t| self.session.baseline_store().get(t).cloned());
        let Ok(delta) = DeltaCodec::encode(tick, &systems, previous.as_ref()) else {
            return;
        };

        // Store this full snapshot as a new baseline for future deltas.
        self.session.baseline_store_mut().insert(baseline);

        let payload = crcbl_net::encode_delta(&delta).expect("valid delta");
        let _ = self.transport.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        });
    }

    /// Whether the transport is still connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    /// Borrow the world (for inspection).
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Mutably borrow the world.
    #[must_use]
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The current tick id.
    #[must_use]
    pub fn tick_id(&self) -> TickId {
        self.clock.tick()
    }
}

impl<T: Transport> fmt::Debug for Server<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("world", &self.world)
            .field("clock", &self.clock)
            .field("connected", &self.transport.is_connected())
            .field("session_state", &self.session.state())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_ecs::System;
    use crcbl_net::InMemoryTransport;

    // ── Helpers ────────────────────────────────────────────────────────────

    /// Build a world with one system ("position") containing one entity.
    fn world_with_one_entity() -> World {
        let mut world = World::new();
        let e = world.spawn();
        let mut sys = System::<f32>::new("position");
        sys.attach(e, 0.0);
        world.register_system(Box::new(sys));
        world
    }

    /// Drain all messages from a transport, returning the payloads.
    fn drain_payloads(transport: &mut InMemoryTransport) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(Some(msg)) = transport.recv() {
            out.push(msg.payload);
        }
        out
    }

    // ── Creation ───────────────────────────────────────────────────────────

    #[test]
    fn server_starts_at_tick_zero() {
        let (transport, _peer) = InMemoryTransport::pair();
        let server = Server::new(World::new(), transport, 60);
        assert_eq!(server.tick_id(), TickId::ZERO);
    }

    #[test]
    fn server_is_connected_initially() {
        let (transport, _peer) = InMemoryTransport::pair();
        let server = Server::new(World::new(), transport, 60);
        assert!(server.is_connected());
    }

    // ── Tick loop ──────────────────────────────────────────────────────────

    #[test]
    fn update_with_no_elapsed_time_does_nothing() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        let ticks = server.update(std::time::Duration::ZERO);
        assert_eq!(ticks, 0);
        assert_eq!(server.tick_id(), TickId::ZERO);
    }

    #[test]
    fn update_runs_ticks_for_elapsed_time() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), transport, 60);

        server.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        let ticks = server.update(tick_dt);
        assert_eq!(ticks, 1);
        assert_eq!(server.tick_id(), TickId::from_raw(1));
    }

    #[test]
    fn tick_advances_world() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), transport, 60);

        server.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        server.update(tick_dt);

        let stats = Inspector::collect(server.world());
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].entity_count, 1);
    }

    #[test]
    fn tick_loop_with_no_inputs_does_not_panic() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), transport, 60);

        server.update(std::time::Duration::ZERO);
        let ticks = server.update(std::time::Duration::from_nanos(5 * 16_666_667));
        assert_eq!(ticks, 5);
        assert_eq!(server.tick_id(), TickId::from_raw(5));
    }

    // ── Snapshot emission ──────────────────────────────────────────────────

    #[test]
    fn snapshot_is_sent_after_tick() {
        let (server_transport, mut client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), server_transport, 60);

        server.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        server.update(tick_dt);

        let msg = client_transport.recv().unwrap().unwrap();
        assert_eq!(msg.kind, MessageKind::Unreliable);
        assert!(!msg.payload.is_empty());
    }

    #[test]
    fn snapshot_is_delta_encodable() {
        // Verify the emitted payload decodes as a valid Delta (new codec).
        let (server_transport, mut client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), server_transport, 60);

        server.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::from_nanos(16_666_667));

        let msg = client_transport.recv().unwrap().unwrap();
        let delta = crcbl_net::decode_delta(&msg.payload);
        assert!(delta.is_ok(), "snapshot payload must decode as Delta");
    }

    #[test]
    fn multiple_ticks_produce_multiple_snapshots() {
        let (server_transport, mut client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), server_transport, 60);

        server.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::from_nanos(3 * 16_666_667));

        let payloads = drain_payloads(&mut client_transport);
        assert_eq!(payloads.len(), 3);
    }

    // ── Input + ack consumption ────────────────────────────────────────────

    #[test]
    fn server_handles_ack() {
        let (server_transport, mut client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), server_transport, 60);

        // Send an ack from the "client" side.
        let ack_payload = crcbl_net::encode_ack(TickId::from_raw(1));
        client_transport
            .send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: ack_payload,
            })
            .unwrap();

        server.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::from_nanos(16_666_667));
        // Ack was consumed — just verifying no panic.
    }

    #[test]
    fn server_consumes_client_inputs_without_error() {
        let (server_transport, client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), server_transport, 60);

        let input = crcbl_net::ClientToServer::Input {
            tick: TickId::from_raw(1),
            data: vec![1, 2, 3],
        };
        let payload = crcbl_net::encode_client_to_server(&input);
        let mut peer = client_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
        .unwrap();

        server.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::from_nanos(16_666_667));
    }

    // ── Debug ──────────────────────────────────────────────────────────────

    #[test]
    fn debug_format() {
        let (transport, _peer) = InMemoryTransport::pair();
        let server = Server::new(World::new(), transport, 60);
        let s = format!("{server:?}");
        assert!(s.contains("Server"));
        assert!(s.contains("connected"));
    }
}
