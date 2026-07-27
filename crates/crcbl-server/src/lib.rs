//! Authoritative server: fixed-tick simulation loop and snapshot emission.
//!
//! The server is the single source of truth. Each tick it drains client inputs,
//! advances the ECS world, and broadcasts a per-system snapshot over an
//! unreliable transport channel.

pub mod sim_hash;

use std::fmt;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{Inspector, World};
use crcbl_net::{Message, MessageKind, SnapshotWriter, Transport};

// ---------------------------------------------------------------------------
// Wire format (shared with crcbl-client — kept simple until P3 replication)
// ---------------------------------------------------------------------------

/// Serialise a [`crcbl_net::ServerToClient`] into the opaque payload of a
/// [`Message`].
fn encode_server_to_client(msg: &crcbl_net::ServerToClient) -> Vec<u8> {
    match msg {
        crcbl_net::ServerToClient::Snapshot {
            sector: _sector,
            tick,
            systems,
        } => {
            let mut buf = Vec::new();
            buf.push(0u8); // tag: Snapshot
            buf.extend_from_slice(&tick.get().to_le_bytes());
            buf.extend_from_slice(&(systems.len() as u32).to_le_bytes());
            for sys in systems {
                buf.extend_from_slice(&sys.system_id.to_le_bytes());
                buf.extend_from_slice(&(sys.data.len() as u32).to_le_bytes());
                buf.extend_from_slice(&sys.data);
            }
            buf
        }
        crcbl_net::ServerToClient::Event { data } => {
            let mut buf = Vec::new();
            buf.push(1u8); // tag: Event
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
            buf
        }
    }
}

/// Serialise a [`crcbl_net::ClientToServer`] into the opaque payload of a
/// [`Message`].
#[allow(dead_code)]
fn encode_client_to_server(msg: &crcbl_net::ClientToServer) -> Vec<u8> {
    match msg {
        crcbl_net::ClientToServer::Input { tick, data } => {
            let mut buf = Vec::new();
            buf.push(0u8); // tag: Input
            buf.extend_from_slice(&tick.get().to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
            buf
        }
        crcbl_net::ClientToServer::Command { data } => {
            let mut buf = Vec::new();
            buf.push(1u8); // tag: Command
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
            buf
        }
    }
}

/// Deserialise a [`crcbl_net::ClientToServer`] from a [`Message`] payload.
fn decode_client_to_server(payload: &[u8]) -> Option<crcbl_net::ClientToServer> {
    if payload.is_empty() {
        return None;
    }
    match payload[0] {
        0 => {
            // Input
            if payload.len() < 13 {
                return None;
            }
            let tick = TickId::from_raw(u64::from_le_bytes(payload[1..9].try_into().ok()?));
            let data_len = u32::from_le_bytes(payload[9..13].try_into().ok()?) as usize;
            if payload.len() < 13 + data_len {
                return None;
            }
            let data = payload[13..13 + data_len].to_vec();
            Some(crcbl_net::ClientToServer::Input { tick, data })
        }
        1 => {
            // Command
            if payload.len() < 5 {
                return None;
            }
            let data_len = u32::from_le_bytes(payload[1..5].try_into().ok()?) as usize;
            if payload.len() < 5 + data_len {
                return None;
            }
            let data = payload[5..5 + data_len].to_vec();
            Some(crcbl_net::ClientToServer::Command { data })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// The authoritative server: owns the ECS [`World`], drives the fixed-tick
/// simulation loop, and emits per-tick snapshots over the [`Transport`].
pub struct Server<T: Transport> {
    world: World,
    transport: T,
    clock: FrameClock,
}

impl<T: Transport> Server<T> {
    /// Create a server running at `tick_hz` (e.g. 60).
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    #[must_use]
    pub fn new(world: World, transport: T, tick_hz: u32) -> Self {
        Self {
            world,
            transport,
            clock: FrameClock::new(tick_hz),
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

    /// Run one tick: consume inputs from transport, tick the world, emit
    /// snapshot.
    fn tick(&mut self) {
        // 1. Drain available client inputs (non-blocking).
        self.drain_inputs();

        // 2. Advance the simulation.
        self.world.tick();

        // 3. Build and send a snapshot.
        self.emit_snapshot();
    }

    /// Consume every queued client message from the transport.
    fn drain_inputs(&mut self) {
        loop {
            match self.transport.recv() {
                Ok(Some(msg)) => {
                    // Decode and (for now) discard — input application
                    // lands in P3 when the input system exists.
                    let _ = decode_client_to_server(&msg.payload);
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    /// Build a [`SnapshotWriter`], populate it from the ECS schedule, serialise
    /// and send.
    fn emit_snapshot(&mut self) {
        let tick = self.clock.tick();
        let mut writer = SnapshotWriter::new(tick);

        let stats = Inspector::collect(&self.world);
        for (idx, stat) in stats.iter().enumerate() {
            // For now the payload is just the entity count (4 bytes LE).
            // Real per-entity component data lands with the replication
            // encoding in P3.
            let mut data = Vec::with_capacity(4);
            data.extend_from_slice(&(stat.entity_count as u32).to_le_bytes());
            writer.write_system(idx as u32, data);
        }

        let snapshot = writer.finish();
        let payload = encode_server_to_client(&snapshot);
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

        // Establish baseline, then advance one tick.
        server.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        let ticks = server.update(tick_dt);
        assert_eq!(ticks, 1);
        assert_eq!(server.tick_id(), TickId::from_raw(1));
    }

    #[test]
    fn update_sets_render_dt_baseline_correctly() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);

        // First update: no prior timestamp → zero delta, zero ticks.
        let ticks = server.update(std::time::Duration::from_secs(0));
        assert_eq!(ticks, 0);

        // Second update: still zero delta (same timestamp), zero ticks.
        let ticks = server.update(std::time::Duration::from_secs(0));
        assert_eq!(ticks, 0);

        // Third: advance enough for one tick.
        let ticks = server.update(std::time::Duration::from_nanos(16_666_667));
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

        // After one tick the entity should still be alive (no despawn).
        let stats = Inspector::collect(server.world());
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].entity_count, 1);
    }

    #[test]
    fn tick_loop_with_no_inputs_does_not_panic() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), transport, 60);

        // Run many ticks with no client input.
        server.update(std::time::Duration::ZERO);
        // 5 ticks at 60 Hz (under the catch-up cap of 8).
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

        // The client end should have received a snapshot message.
        let msg = client_transport.recv().unwrap().unwrap();
        assert_eq!(msg.kind, MessageKind::Unreliable);
        assert!(!msg.payload.is_empty());
    }

    #[test]
    fn snapshot_contains_system_data() {
        let (server_transport, mut client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), server_transport, 60);

        server.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        server.update(tick_dt);

        let msg = client_transport.recv().unwrap().unwrap();
        // Decode the payload as ServerToClient::Snapshot
        let decoded = decode_server_to_client_for_test(&msg.payload);
        assert!(decoded.is_some());
    }

    #[test]
    fn multiple_ticks_produce_multiple_snapshots() {
        let (server_transport, mut client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(world_with_one_entity(), server_transport, 60);

        server.update(std::time::Duration::ZERO);
        // 3 ticks at 60 Hz (under the catch-up cap of 8).
        server.update(std::time::Duration::from_nanos(3 * 16_666_667));

        // Should have 3 snapshots on the client end.
        for i in 0..3 {
            let msg = client_transport.recv().unwrap().unwrap();
            assert_eq!(msg.kind, MessageKind::Unreliable);
            let _ = i; // used for diagnostics
        }
        // No more messages.
        assert!(client_transport.recv().unwrap().is_none());
    }

    // ── Input consumption ──────────────────────────────────────────────────

    #[test]
    fn server_consumes_client_inputs_without_error() {
        let (server_transport, client_transport) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), server_transport, 60);

        // Simulate a client sending input to the server.
        let input = crcbl_net::ClientToServer::Input {
            tick: TickId::from_raw(1),
            data: vec![1, 2, 3],
        };
        let payload = encode_client_to_server(&input);
        // The server's transport receives from client_transport; we need to
        // send FROM the peer side.  InMemoryTransport pair: server_transport
        // is the server's end, client_transport sends to it.
        let mut peer = client_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
        .unwrap();

        // Tick: server should drain the input.
        server.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::from_nanos(16_666_667));
        // No assertion possible from outside (input is discarded), but we
        // verify it doesn't panic or error.
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

    // ── Test-only decode helper (avoids duplicating the client's decoder) ──

    /// Minimal decode for assertions in server tests.
    fn decode_server_to_client_for_test(payload: &[u8]) -> Option<crcbl_net::ServerToClient> {
        if payload.is_empty() {
            return None;
        }
        match payload[0] {
            0 => {
                if payload.len() < 13 {
                    return None;
                }
                let tick = TickId::from_raw(u64::from_le_bytes(payload[1..9].try_into().ok()?));
                let system_count = u32::from_le_bytes(payload[9..13].try_into().ok()?) as usize;
                let mut offset = 13;
                let mut systems = Vec::with_capacity(system_count);
                for _ in 0..system_count {
                    if offset + 8 > payload.len() {
                        return None;
                    }
                    let system_id =
                        u32::from_le_bytes(payload[offset..offset + 4].try_into().ok()?);
                    let data_len =
                        u32::from_le_bytes(payload[offset + 4..offset + 8].try_into().ok()?)
                            as usize;
                    offset += 8;
                    if offset + data_len > payload.len() {
                        return None;
                    }
                    let data = payload[offset..offset + data_len].to_vec();
                    offset += data_len;
                    systems.push(crcbl_net::SystemSnapshot { system_id, data });
                }
                Some(crcbl_net::ServerToClient::Snapshot {
                    sector: crcbl_net::SectorId::ZERO,
                    tick,
                    systems,
                })
            }
            _ => None,
        }
    }
}
