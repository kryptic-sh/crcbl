//! Rendering client: interpolation buffer, input send, snapshot application.
//!
//! The client sends its input to the server each tick and buffers incoming
//! snapshots. Between ticks it uses the two most recent snapshots to
//! interpolate entity state for smooth rendering.

use std::fmt;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::World;
use crcbl_net::{Message, MessageKind, ServerToClient, SystemSnapshot, Transport, TransportError};
use glam::Vec3;

// ---------------------------------------------------------------------------
// Wire format (mirrors the server-side encoding)
// ---------------------------------------------------------------------------

/// Deserialise a [`ServerToClient`] from a [`Message`] payload.
fn decode_server_to_client(payload: &[u8]) -> Option<ServerToClient> {
    if payload.is_empty() {
        return None;
    }
    match payload[0] {
        0 => {
            // Snapshot
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
                let system_id = u32::from_le_bytes(payload[offset..offset + 4].try_into().ok()?);
                let data_len =
                    u32::from_le_bytes(payload[offset + 4..offset + 8].try_into().ok()?) as usize;
                offset += 8;
                if offset + data_len > payload.len() {
                    return None;
                }
                let data = payload[offset..offset + data_len].to_vec();
                offset += data_len;
                systems.push(SystemSnapshot { system_id, data });
            }
            Some(ServerToClient::Snapshot { tick, systems })
        }
        1 => {
            // Event
            if payload.len() < 5 {
                return None;
            }
            let data_len = u32::from_le_bytes(payload[1..5].try_into().ok()?) as usize;
            if payload.len() < 5 + data_len {
                return None;
            }
            let data = payload[5..5 + data_len].to_vec();
            Some(ServerToClient::Event { data })
        }
        _ => None,
    }
}

/// Serialise a [`crcbl_net::ClientToServer`] into the opaque payload of a
/// [`Message`].
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

/// The rendering client: sends input to the server, buffers incoming
/// snapshots, and provides interpolated entity state for smooth frame-rate
/// rendering.
pub struct Client<T: Transport> {
    /// Local ECS world (receives snapshots).
    world: World,
    /// Transport to the server.
    transport: T,
    /// Client-side frame clock for input-tick cadence and render alpha.
    clock: FrameClock,
    /// The older of the two most recent snapshots.
    prev_snapshot: Option<ServerToClient>,
    /// The newer of the two most recent snapshots.
    current_snapshot: Option<ServerToClient>,
    /// Input data to send on the next tick.
    pending_input: Vec<u8>,
}

impl<T: Transport> Client<T> {
    /// Create a client running at the same `tick_hz` as the server.
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
            prev_snapshot: None,
            current_snapshot: None,
            pending_input: Vec::new(),
        }
    }

    /// Feed the current time.
    ///
    /// Drains received snapshots into the buffer, sends pending input for
    /// each consumed tick, and returns the interpolation alpha in `[0, 1)`.
    pub fn update(&mut self, now: std::time::Duration) -> f32 {
        self.clock.update(now);
        while self.clock.consume_tick() {
            let tick = self.clock.tick();
            let _ = self.send_input(tick);
        }
        let _ = self.recv_snapshots();
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
        // Snapshots only carry entity counts right now — no per-entity
        // component data to interpolate between.  Real interpolation lands
        // with the replication encoding in P3.
        InterpolatedState {
            positions: Vec::new(),
        }
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

    /// Send pending input to the server for the given tick.
    fn send_input(&mut self, tick: TickId) -> Result<(), TransportError> {
        if self.pending_input.is_empty() {
            return Ok(());
        }
        let msg = crcbl_net::ClientToServer::Input {
            tick,
            data: self.pending_input.clone(),
        };
        let payload = encode_client_to_server(&msg);
        self.transport.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
    }

    /// Drain available snapshots from the transport, sliding the two-slot
    /// interpolation buffer so the newest is always `current_snapshot` and the
    /// previous is `prev_snapshot`.
    fn recv_snapshots(&mut self) -> Result<(), TransportError> {
        while let Some(msg) = self.transport.recv()? {
            let Some(server_msg) = decode_server_to_client(&msg.payload) else {
                continue;
            };
            if let ServerToClient::Snapshot { tick, .. } = &server_msg {
                let is_newer = match &self.current_snapshot {
                    Some(ServerToClient::Snapshot {
                        tick: current_tick, ..
                    }) => *tick > *current_tick,
                    _ => true,
                };
                if is_newer {
                    self.prev_snapshot = self.current_snapshot.take();
                    self.current_snapshot = Some(server_msg);
                }
            }
        }
        Ok(())
    }
}

impl<T: Transport> fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prev_tick = match &self.prev_snapshot {
            Some(ServerToClient::Snapshot { tick, .. }) => Some(*tick),
            _ => None,
        };
        let current_tick = match &self.current_snapshot {
            Some(ServerToClient::Snapshot { tick, .. }) => Some(*tick),
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

    /// Build an empty world.
    fn empty_world() -> World {
        World::new()
    }

    /// Build a snapshot message for a given tick and system data.
    fn snapshot_msg(tick: u64, system_data: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0u8); // tag: Snapshot
        buf.extend_from_slice(&tick.to_le_bytes());
        buf.extend_from_slice(&(system_data.len() as u32).to_le_bytes());
        for &(sys_id, ref data) in system_data {
            buf.extend_from_slice(&sys_id.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
        }
        buf
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

        // Establish clock baseline, then advance one tick.
        client.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        client.update(tick_dt);

        // The server end should have an input message.
        let mut peer = server_transport;
        let msg = peer.recv().unwrap().unwrap();
        assert_eq!(msg.kind, MessageKind::Unreliable);
        assert!(!msg.payload.is_empty());
    }

    #[test]
    fn no_input_sends_nothing() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        // Establish clock baseline, then advance one tick.
        client.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        client.update(tick_dt);

        // No input was set, so nothing should be sent.
        let mut peer = server_transport;
        assert!(peer.recv().unwrap().is_none());
    }

    #[test]
    fn set_input_persists_across_ticks() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        client.set_input(vec![42]);

        // Establish clock baseline, then advance two ticks.
        client.update(std::time::Duration::ZERO);
        let tick_dt = std::time::Duration::from_nanos(33_333_334);
        let alpha = client.update(tick_dt);

        // Two ticks should have been consumed, and input sent for both.
        let mut peer = server_transport;
        let msg1 = peer.recv().unwrap().unwrap();
        let msg2 = peer.recv().unwrap().unwrap();
        assert_eq!(msg1.kind, MessageKind::Unreliable);
        assert_eq!(msg2.kind, MessageKind::Unreliable);
        // Alpha should be partial (remainder after 2 ticks at 60 Hz).
        assert!((0.0..1.0).contains(&alpha));
    }

    // ── Snapshot receive ───────────────────────────────────────────────────

    #[test]
    fn receives_snapshot_into_buffer() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        // Simulate the server sending a snapshot.
        let payload = snapshot_msg(1, &[(0, vec![1, 0, 0, 0])]); // system 0, entity_count = 1
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
        .unwrap();
        drop(peer); // release borrow so client can recv

        // Client doesn't need a tick to recv — update drains snapshots even
        // with zero elapsed time (as long as the clock hasn't seen any time).
        // But first update sets baseline, second with zero delta processes.
        client.update(std::time::Duration::ZERO);
        // Send a tiny delta so the clock advances enough for the recv to
        // happen in the update loop.
        client.update(std::time::Duration::from_nanos(1));

        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(1))"));
    }

    // ── Interpolation buffer sliding ───────────────────────────────────────

    #[test]
    fn newer_snapshot_slides_buffer() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        // Send snapshot for tick 1.
        let payload1 = snapshot_msg(1, &[(0, vec![1, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload1,
        })
        .unwrap();

        // Establish clock baseline and drain snap1.
        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(1))"));
        assert!(debug.contains("prev_snapshot_tick: None"));

        // Send snapshot for tick 2 using the same peer handle.
        let payload2 = snapshot_msg(2, &[(0, vec![2, 0, 0, 0])]);
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

        // Send snapshot for tick 5.
        let payload5 = snapshot_msg(5, &[(0, vec![5, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload5,
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        // Now send tick 3 (older) using the same peer handle.
        let payload3 = snapshot_msg(3, &[(0, vec![3, 0, 0, 0])]);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload3,
        })
        .unwrap();
        drop(peer);

        client.update(std::time::Duration::from_nanos(1));

        let debug = format!("{client:?}");
        // Current should still be tick 5 (older tick 3 ignored).
        assert!(debug.contains("current_snapshot_tick: Some(TickId(5))"));
    }

    // ── Interpolation alpha ────────────────────────────────────────────────

    #[test]
    fn alpha_is_zero_at_tick_boundary() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), transport, 60);

        // Exactly one tick of time: alpha should be near zero after
        // consuming the tick.
        let tick_dt = std::time::Duration::from_nanos(16_666_667);
        let alpha = client.update(tick_dt);
        assert!((alpha - 0.0).abs() < 0.01, "alpha was {alpha}");
    }

    #[test]
    fn alpha_grows_between_ticks() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), transport, 60);

        // Establish clock baseline.
        client.update(std::time::Duration::ZERO);

        // Half a tick: no tick consumed, alpha ~0.5.
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
    fn interpolate_returns_empty_when_only_one_snapshot() {
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        let payload = snapshot_msg(1, &[(0, vec![1, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload,
        })
        .unwrap();
        drop(peer);

        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        let state = client.interpolate();
        // One snapshot is enough for current but not prev; positions empty.
        assert!(state.positions.is_empty());
    }

    #[test]
    fn interpolate_is_stub_returns_empty_even_with_two_snapshots() {
        // P2a: snapshots carry only entity counts, not per-entity component
        // data, so interpolation is a stub.  This test documents that
        // limitation; when P3 implements real interpolation this test must
        // be updated to assert non-empty positions.
        let (client_transport, server_transport) = InMemoryTransport::pair();
        let mut client = Client::new(empty_world(), client_transport, 60);

        // Feed two snapshots into the buffer.
        let payload1 = snapshot_msg(1, &[(0, vec![1, 0, 0, 0])]);
        let mut peer = server_transport;
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: payload1,
        })
        .unwrap();
        client.update(std::time::Duration::ZERO);
        client.update(std::time::Duration::from_nanos(1));

        let payload2 = snapshot_msg(2, &[(0, vec![1, 0, 0, 0])]);
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
