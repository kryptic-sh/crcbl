//! Authoritative server: fixed-tick simulation loop and snapshot emission.
//!
//! The server is the single source of truth. Each tick it drains client inputs,
//! advances the ECS world, and broadcasts a per-system snapshot over an
//! unreliable transport channel. Snapshots are delta-encoded against the
//! client's last-acked baseline (P2b protocol).

pub mod sim_hash;

use std::fmt;
use std::time::Instant;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{Inspector, World};
use crcbl_net::{
    Baseline, DeltaCodec, HandshakeGate, HandshakeResult, Message, MessageKind, RejectReason,
    SessionConfig, SessionId, SessionManager, SessionState, SnapshotWriter, Transport,
};

const PROTOCOL_VERSION: u32 = 1;
const ENGINE_BUILD_ID: u64 = 0;
const SCHEMA_HASH: u64 = 0;

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
    session_config: SessionConfig,
    handshake_gate: HandshakeGate,
    processing_error_count: u64,
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
            session_config: config,
            handshake_gate: HandshakeGate::new(PROTOCOL_VERSION, ENGINE_BUILD_ID, SCHEMA_HASH),
            processing_error_count: 0,
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
        let was_connected = self.session.state() == SessionState::Connected;
        self.drain_inputs();
        self.update_session_for_transport();
        self.world.tick();
        if was_connected && self.session.state() == SessionState::Connected {
            self.emit_snapshot();
        }
    }

    fn update_session_for_transport(&mut self) {
        if self.transport.is_connected() {
            return;
        }
        if self.session.state() == SessionState::Connected {
            self.session
                .on_disconnect(Instant::now(), &self.session_config);
        }
        self.session.expire_if_timed_out(Instant::now());
    }

    /// Consume queued client messages: handshake, inputs (discarded — P3), and acks.
    fn drain_inputs(&mut self) {
        loop {
            match self.transport.recv() {
                Ok(Some(msg)) => {
                    if let Ok(hello) = crcbl_net::decode_hello(&msg.payload) {
                        self.handle_hello(hello);
                    } else if let Ok(tick) = crcbl_net::decode_ack(&msg.payload) {
                        self.session.handle_ack(tick);
                    } else if crcbl_net::decode_client_to_server(&msg.payload).is_err() {
                        self.processing_error_count += 1;
                    }
                }
                Ok(None) => break,
                Err(crcbl_net::TransportError::Disconnected) => break,
                Err(_) => {
                    self.processing_error_count += 1;
                    break;
                }
            }
        }
    }

    fn handle_hello(&mut self, hello: crcbl_net::Hello) {
        let mut result =
            self.handshake_gate
                .validate(&hello, self.session.session_id(), self.clock.tick());
        if matches!(result, HandshakeResult::Accept { .. }) {
            let expected_token = self.session.session_id();
            match self.session.state() {
                SessionState::Disconnected => {
                    if hello.session_token.is_some() {
                        result = Self::invalid_session_token(
                            "fresh handshake must not include a session token",
                        );
                    } else {
                        self.session.begin_handshake();
                        self.session
                            .on_connected(hello.engine_build_id, hello.schema_hash);
                    }
                }
                SessionState::Reconnecting => {
                    if hello.session_token != Some(expected_token) {
                        result =
                            Self::invalid_session_token("reconnect session token does not match");
                    } else if !self.session.try_reconnect(
                        Instant::now(),
                        hello.engine_build_id,
                        hello.schema_hash,
                        &self.session_config,
                    ) {
                        result = Self::invalid_session_token("reconnect grace period expired");
                    }
                }
                SessionState::Handshaking => {
                    result = Self::invalid_session_token("handshake is already in progress");
                }
                SessionState::Connected => {
                    if hello.session_token != Some(expected_token) {
                        result = Self::invalid_session_token("session token does not match");
                    }
                }
            }
        }
        if self
            .transport
            .send_reliable(Message {
                kind: MessageKind::Reliable,
                payload: crcbl_net::encode_handshake_result(&result),
            })
            .is_err()
        {
            self.processing_error_count += 1;
        }
    }

    fn invalid_session_token(message: &str) -> HandshakeResult {
        HandshakeResult::Reject {
            reason: RejectReason {
                code: 0x04,
                msg: message.into(),
            },
        }
    }

    /// Build snapshots from the ECS schedule, delta-encode against the
    /// client's baseline, and send.
    fn emit_snapshot(&mut self) {
        let tick = self.clock.tick();
        let mut writer = SnapshotWriter::new(tick);

        let stats = Inspector::collect(&self.world);
        for (idx, stat) in stats.iter().enumerate() {
            // Payload: one synthetic entity with its count (4 bytes LE); real
            // per-entity component data lands with P3 replication encoding.
            let mut data = Vec::with_capacity(16);
            data.extend_from_slice(&0u64.to_le_bytes());
            data.extend_from_slice(&4u32.to_le_bytes());
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
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };
        let last_acked = self.session.last_acked_tick();
        let previous = last_acked.and_then(|t| self.session.baseline_store().get(t).cloned());
        let delta = match DeltaCodec::encode(tick, &systems, previous.as_ref()) {
            Ok(delta) => delta,
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };

        let payload = match crcbl_net::encode_delta(&delta) {
            Ok(payload) => payload,
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };

        // Store this full snapshot as a new baseline for future deltas.
        self.session.baseline_store_mut().insert(baseline);

        if self
            .transport
            .send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload,
            })
            .is_err()
        {
            self.processing_error_count += 1;
        }
    }

    /// Replace the transport after a disconnect. The next valid resume handshake
    /// within the configured grace period returns the session to `Connected`.
    pub fn reconnect(&mut self, transport: T) {
        self.transport = transport;
    }

    /// Current session lifecycle state.
    #[must_use]
    pub fn session_state(&self) -> SessionState {
        self.session.state()
    }

    /// Number of unrecoverable transport, encoding, decoding, or lifecycle errors.
    #[must_use]
    pub fn processing_error_count(&self) -> u64 {
        self.processing_error_count
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

    fn connect_server(server: &mut Server<InMemoryTransport>, peer: &mut InMemoryTransport) {
        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_hello(&crcbl_net::Hello {
                protocol_version: PROTOCOL_VERSION,
                engine_build_id: ENGINE_BUILD_ID,
                schema_hash: SCHEMA_HASH,
                session_token: None,
            }),
        })
        .unwrap();
        server.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::from_nanos(16_666_667));
        let mut accepted = false;
        while let Some(result) = peer.recv().unwrap() {
            if matches!(
                crcbl_net::decode_handshake_result(&result.payload),
                Ok(HandshakeResult::Accept { .. })
            ) {
                accepted = true;
            }
        }
        assert!(accepted, "server must accept a matching hello");
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
        connect_server(&mut server, &mut client_transport);

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
        connect_server(&mut server, &mut client_transport);

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
        connect_server(&mut server, &mut client_transport);

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
