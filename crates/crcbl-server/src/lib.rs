//! Authoritative server: fixed-tick simulation loop and snapshot emission.
//!
//! The server is the single source of truth. Each tick it drains client inputs,
//! advances the ECS world, and broadcasts a per-system snapshot over an
//! unreliable transport channel. Snapshots are delta-encoded against the
//! client's last-acked baseline (P2b protocol).

pub mod rate_limit;
pub mod sim_hash;

pub use rate_limit::InboundRateLimitConfig;

use std::fmt;
use std::time::Duration;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{GameModule, Inspector, World};
use crcbl_net::{
    Baseline, DeltaCodec, HandshakeGate, HandshakeResult, Message, MessageKind,
    ProtocolCompatibility, RejectReason, ResumeToken, SectorId, SessionConfig, SessionId,
    SessionManager, SessionState, SnapshotWriter, Transport,
};
use rate_limit::InboundRateLimiter;

#[cfg(test)]
const PROTOCOL_VERSION: u32 = ProtocolCompatibility::DEFAULT.protocol_version;
#[cfg(test)]
const ENGINE_BUILD_ID: u64 = ProtocolCompatibility::DEFAULT.engine_build_id;
#[cfg(test)]
const SCHEMA_HASH: u64 = ProtocolCompatibility::DEFAULT.schema_hash;

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
    resume_token: ResumeToken,
    next_session_id: u64,
    session_terminated: bool,
    handshake_gate: HandshakeGate,
    inbound_rate_limiter: InboundRateLimiter,
    now: Duration,
    processing_error_count: u64,
    rate_limited_message_count: u64,
    rate_limited_byte_count: u64,
    /// Optional game logic module (ticked after the ECS schedule).
    module: Option<Box<dyn GameModule>>,
}

impl<T: Transport> Server<T> {
    /// Create a server with the default protocol compatibility identifiers.
    ///
    /// # Panics
    ///
    /// Panics if `tick_hz` is zero or the operating system CSPRNG is unavailable.
    #[must_use]
    pub fn new(world: World, transport: T, tick_hz: u32) -> Self {
        let compatibility = ProtocolCompatibility::DEFAULT;
        if !cfg!(test) {
            compatibility.assert_explicit();
        }
        Self::try_new_with_compatibility(world, transport, tick_hz, compatibility)
            .expect("operating system CSPRNG must be available to create a server")
    }

    /// Create a server with the default protocol compatibility identifiers.
    ///
    /// Returns the operating-system entropy error rather than issuing a predictable
    /// resume credential.
    pub fn try_new(world: World, transport: T, tick_hz: u32) -> Result<Self, getrandom::Error> {
        let compatibility = ProtocolCompatibility::DEFAULT;
        if !cfg!(test) {
            compatibility.assert_explicit();
        }
        Self::try_new_with_compatibility(world, transport, tick_hz, compatibility)
    }

    /// Create a server with explicit protocol compatibility identifiers.
    ///
    /// # Panics
    ///
    /// Panics if `tick_hz` is zero.
    ///
    /// Returns the operating-system entropy error rather than issuing a predictable
    /// resume credential.
    pub fn try_new_with_compatibility(
        world: World,
        transport: T,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
    ) -> Result<Self, getrandom::Error> {
        if !cfg!(test) {
            compatibility.assert_explicit();
        }
        let config = SessionConfig::default();
        let session_id = SessionId(1);
        let resume_token = Self::generate_resume_token()?;
        Ok(Self {
            world,
            transport,
            clock: FrameClock::new(tick_hz),
            session: SessionManager::new(session_id, &config),
            session_config: config,
            resume_token,
            next_session_id: session_id.0 + 1,
            session_terminated: false,
            handshake_gate: HandshakeGate::new(compatibility),
            inbound_rate_limiter: InboundRateLimiter::new(
                InboundRateLimitConfig::default(),
                Duration::ZERO,
            ),
            now: Duration::ZERO,
            processing_error_count: 0,
            rate_limited_message_count: 0,
            rate_limited_byte_count: 0,
            module: None,
        })
    }

    /// Feed the current time from a [`crcbl_core::time::TimeSource`].
    ///
    /// Returns how many ticks ran this frame.
    pub fn update(&mut self, now: std::time::Duration) -> u32 {
        self.now = now;
        self.clock.update(now);
        let mut ticks = 0u32;
        while self.clock.consume_tick() {
            self.tick();
            ticks += 1;
        }
        ticks
    }

    /// Run one tick: consume inputs from transport (including acks), tick the
    /// world (ECS schedule), tick the game module (if any), emit delta-encoded
    /// snapshot.
    fn tick(&mut self) {
        let was_connected = self.session.state() == SessionState::Connected;
        self.drain_inputs();
        self.update_session_for_transport();
        self.world.tick();
        if let Some(ref mut module) = self.module {
            module.tick(&mut self.world);
        }
        if was_connected && self.session.state() == SessionState::Connected {
            self.emit_snapshot();
        }
    }

    fn update_session_for_transport(&mut self) {
        if !self.transport.is_connected() && self.session.state() == SessionState::Connected {
            self.session.on_disconnect(self.now, &self.session_config);
        }
        let was_reconnecting = self.session.state() == SessionState::Reconnecting;
        self.session.expire_if_timed_out(self.now);
        if was_reconnecting && self.session.state() == SessionState::Disconnected {
            self.session_terminated = true;
        }
    }

    /// Consume queued client messages: handshake, inputs (discarded — P3), and acks.
    fn drain_inputs(&mut self) {
        loop {
            match self.transport.recv_reliable() {
                Ok(Some(msg)) => {
                    if !self.process_inbound_message(msg) {
                        break;
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
        loop {
            match self.transport.recv() {
                Ok(Some(msg)) => {
                    if !self.process_inbound_message(msg) {
                        break;
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

    fn process_inbound_message(&mut self, msg: Message) -> bool {
        let bytes = u64::try_from(msg.payload.len()).unwrap_or(u64::MAX);
        if let Err((messages_limited, bytes_limited)) =
            self.inbound_rate_limiter.allow(self.now, bytes)
        {
            self.rate_limited_message_count = self
                .rate_limited_message_count
                .saturating_add(u64::from(messages_limited));
            self.rate_limited_byte_count = self
                .rate_limited_byte_count
                .saturating_add(u64::from(bytes_limited));
            return false;
        }
        if let Ok(hello) = crcbl_net::decode_hello(&msg.payload) {
            self.handle_hello(hello);
        } else if let Ok(ack) = crcbl_net::decode_ack(&msg.payload) {
            self.session.handle_ack(ack.sector, ack.tick);
        } else if crcbl_net::decode_client_to_server(&msg.payload).is_err() {
            self.processing_error_count += 1;
        }
        true
    }

    fn handle_hello(&mut self, hello: crcbl_net::Hello) {
        if self.session_terminated
            && self.session.state() == SessionState::Disconnected
            && hello.session_token.is_none()
            && let Err(error) = self.rotate_session()
        {
            self.processing_error_count += 1;
            self.send_handshake_result(Self::entropy_failure(hello.generation, error));
            return;
        }
        let mut result = self.handshake_gate.validate(
            &hello,
            self.session.session_id(),
            self.resume_token,
            self.clock.tick(),
        );
        if matches!(result, HandshakeResult::Accept { .. }) {
            let expected_token = self.resume_token;
            match self.session.state() {
                SessionState::Disconnected => {
                    if hello.session_token.is_some() {
                        result = Self::invalid_session_token(
                            hello.generation,
                            "fresh handshake must not include a session token",
                        );
                    } else {
                        self.session.begin_handshake();
                        self.session
                            .on_connected(hello.engine_build_id, hello.schema_hash);
                    }
                }
                SessionState::Reconnecting => {
                    if !hello
                        .session_token
                        .is_some_and(|token| token == expected_token)
                    {
                        result = Self::invalid_session_token(
                            hello.generation,
                            "reconnect session token does not match",
                        );
                    } else {
                        match Self::generate_resume_token() {
                            Ok(resume_token) => {
                                let reconnects = self.session.can_reconnect(
                                    self.now,
                                    hello.engine_build_id,
                                    hello.schema_hash,
                                );
                                if reconnects {
                                    if let HandshakeResult::Accept {
                                        resume_token: accepted_token,
                                        ..
                                    } = &mut result
                                    {
                                        *accepted_token = resume_token;
                                    }
                                    if self.send_handshake_result(result) {
                                        assert!(self.session.try_reconnect(
                                            self.now,
                                            hello.engine_build_id,
                                            hello.schema_hash,
                                            &self.session_config,
                                        ));
                                        self.resume_token = resume_token;
                                    }
                                    return;
                                }
                                self.session.expire_if_timed_out(self.now);
                                result = Self::invalid_session_token(
                                    hello.generation,
                                    "reconnect grace period expired",
                                );
                            }
                            Err(error) => {
                                self.processing_error_count += 1;
                                result = Self::entropy_failure(hello.generation, error);
                            }
                        }
                    }
                }
                SessionState::Handshaking => {
                    result = Self::invalid_session_token(
                        hello.generation,
                        "handshake is already in progress",
                    );
                }
                SessionState::Connected => {
                    if !hello
                        .session_token
                        .is_some_and(|token| token == expected_token)
                    {
                        result = Self::invalid_session_token(
                            hello.generation,
                            "session token does not match",
                        );
                    }
                }
            }
        }
        self.send_handshake_result(result);
    }

    fn generate_resume_token() -> Result<ResumeToken, getrandom::Error> {
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes[..])?;
        Ok(ResumeToken::from_bytes(bytes))
    }

    fn rotate_session(&mut self) -> Result<(), getrandom::Error> {
        let resume_token = Self::generate_resume_token()?;
        let session_id = SessionId(self.next_session_id);
        self.next_session_id = self.next_session_id.wrapping_add(1);
        self.session = SessionManager::new(session_id, &self.session_config);
        self.resume_token = resume_token;
        self.session_terminated = false;
        Ok(())
    }

    fn send_handshake_result(&mut self, result: HandshakeResult) -> bool {
        if self
            .transport
            .send_reliable(Message {
                kind: MessageKind::Reliable,
                payload: crcbl_net::encode_handshake_result(&result),
            })
            .is_err()
        {
            self.processing_error_count += 1;
            false
        } else {
            true
        }
    }

    fn entropy_failure(generation: u64, error: getrandom::Error) -> HandshakeResult {
        HandshakeResult::Reject {
            generation,
            reason: RejectReason {
                code: 0x06,
                msg: format!("unable to generate resume credential: {error}"),
            },
        }
    }

    fn invalid_session_token(generation: u64, message: &str) -> HandshakeResult {
        HandshakeResult::Reject {
            generation,
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
        let sector = SectorId::ZERO;
        let mut writer = SnapshotWriter::new_with_sector(sector, tick);

        let stats = Inspector::collect(&self.world);
        for (idx, (system, stat)) in self.world.schedule().iter().zip(stats.iter()).enumerate() {
            // Systems with a replication impl emit their real per-entity
            // component data; the rest fall back to one synthetic entity
            // carrying only the entity count (4 bytes LE).
            let mut data = Vec::new();
            if !system.replicate(&mut data) {
                data.extend_from_slice(&0u64.to_le_bytes());
                data.extend_from_slice(&4u32.to_le_bytes());
                data.extend_from_slice(&(stat.entity_count as u32).to_le_bytes());
            }
            writer.write_system(idx as u32, data);
        }

        let snapshot = writer.finish();
        let systems: Vec<_> = match &snapshot {
            crcbl_net::ServerToClient::Snapshot { systems, .. } => systems.to_vec(),
            _ => return,
        };

        // Delta-encode against the client's baseline.
        let baseline = match Baseline::from_trusted_snapshot(tick, &systems) {
            Ok(baseline) => baseline,
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };
        let last_acked = self.session.last_acked_tick(sector);
        let previous = last_acked.and_then(|tick| {
            self.session
                .baseline_store(sector)
                .and_then(|store| store.get(tick))
                .cloned()
        });
        let delta = match DeltaCodec::encode_with_sector(sector, tick, &systems, previous.as_ref())
        {
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
        self.session.baseline_store_mut(sector).insert(baseline);

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

    /// Reconnect grace configuration for deterministic tests and embeddings.
    pub fn set_session_config(&mut self, session_config: SessionConfig) {
        self.session_config = session_config;
    }

    /// Configure deterministic per-client inbound traffic limits.
    ///
    /// Reconfiguration resets the bucket to one second of the new budget.
    pub fn set_inbound_rate_limit_config(&mut self, config: InboundRateLimitConfig) {
        self.inbound_rate_limiter.reconfigure(config, self.now);
    }

    /// Attach a [`GameModule`] to drive game-specific per-tick logic.
    ///
    /// The module's [`GameModule::tick`] is called every server tick after the
    /// ECS schedule runs. Only one module can be attached at a time; calling
    /// this again replaces any existing module.
    pub fn set_module(&mut self, module: Box<dyn GameModule>) {
        self.module = Some(module);
    }

    /// Number of messages dropped because their message-rate budget was exhausted.
    #[must_use]
    pub fn rate_limited_message_count(&self) -> u64 {
        self.rate_limited_message_count
    }

    /// Number of messages dropped because their byte-rate budget was exhausted.
    #[must_use]
    pub fn rate_limited_byte_count(&self) -> u64 {
        self.rate_limited_byte_count
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
                generation: 1,
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

    fn retain_ack_baselines(
        server: &mut Server<InMemoryTransport>,
        ticks: impl IntoIterator<Item = u64>,
    ) {
        for tick in ticks {
            let tick = TickId::from_raw(tick);
            server.session.baseline_store_mut(SectorId::ZERO).insert(
                Baseline::from_trusted_snapshot(tick, &[]).expect("empty snapshot is valid"),
            );
        }
    }

    // ── Creation ───────────────────────────────────────────────────────────

    #[test]
    fn failed_reconnect_accept_keeps_previous_credential() {
        let (transport, peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        server.session.begin_handshake();
        server.session.on_connected(ENGINE_BUILD_ID, SCHEMA_HASH);
        server
            .session
            .on_disconnect(Duration::ZERO, &server.session_config);
        let token = server.resume_token;
        drop(peer);

        server.handle_hello(crcbl_net::Hello {
            protocol_version: PROTOCOL_VERSION,
            engine_build_id: ENGINE_BUILD_ID,
            schema_hash: SCHEMA_HASH,
            generation: 1,
            session_token: Some(token),
        });

        assert_eq!(server.session.state(), SessionState::Reconnecting);
        assert_eq!(server.resume_token, token);
        assert_eq!(server.processing_error_count, 1);
    }

    #[test]
    fn rotating_session_clears_baselines_and_acks() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        let old_session_id = server.session.session_id();
        let old_token = server.resume_token;
        let tick = TickId::from_raw(1);
        server
            .session
            .baseline_store_mut(SectorId::ZERO)
            .insert(Baseline::from_trusted_snapshot(tick, &[]).expect("empty snapshot is valid"));
        server.session.handle_ack(SectorId::ZERO, tick);

        server.rotate_session().expect("OS CSPRNG available");

        assert_ne!(server.session.session_id(), old_session_id);
        assert_ne!(server.resume_token, old_token);
        assert_eq!(server.session.last_acked_tick(SectorId::ZERO), None);
        assert!(server.session.baseline_store(SectorId::ZERO).is_none());
    }

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
        let ack_payload = crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(1));
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

    #[test]
    fn inbound_message_limit_accepts_boundary_then_drops_next() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 2,
            bytes_per_second: 1_024,
        });
        retain_ack_baselines(&mut server, 1..=2);
        for tick in 1..=3 {
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(tick)),
            })
            .unwrap();
        }

        server.update(Duration::ZERO);
        server.update(Duration::from_nanos(16_666_667));

        assert_eq!(
            server.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(2))
        );
        assert_eq!(server.rate_limited_message_count(), 1);
        assert_eq!(server.rate_limited_byte_count(), 0);
        assert_eq!(server.processing_error_count(), 0);
        assert!(server.is_connected());
    }

    #[test]
    fn inbound_byte_limit_is_independent_of_message_limit() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        let ack = crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(1));
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 2,
            bytes_per_second: ack.len() as u64,
        });
        retain_ack_baselines(&mut server, [1]);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: ack.clone(),
        })
        .unwrap();
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: ack,
        })
        .unwrap();

        server.update(Duration::ZERO);
        server.update(Duration::from_nanos(16_666_667));

        assert_eq!(
            server.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(server.rate_limited_message_count(), 0);
        assert_eq!(server.rate_limited_byte_count(), 1);
        assert_eq!(server.processing_error_count(), 0);
    }

    #[test]
    fn inbound_limits_refill_only_when_injected_time_advances() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 1,
            bytes_per_second: 1_024,
        });
        retain_ack_baselines(&mut server, 1..=4);
        for tick in 1..=2 {
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(tick)),
            })
            .unwrap();
        }
        server.update(Duration::ZERO);
        server.update(Duration::from_nanos(16_666_667));
        server.update(Duration::from_nanos(16_666_667));
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(3)),
        })
        .unwrap();
        server.update(Duration::ZERO);
        server.update(Duration::from_secs(1) + Duration::from_nanos(16_666_667));
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(4)),
        })
        .unwrap();
        server.update(Duration::from_secs(2) + Duration::from_nanos(16_666_667));

        assert_eq!(
            server.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(4))
        );
        assert_eq!(server.rate_limited_message_count(), 1);
        assert_eq!(server.rate_limited_byte_count(), 0);
    }

    #[test]
    fn oversized_and_malformed_packets_are_limited_before_decode() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 2,
            bytes_per_second: 3,
        });
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: vec![0; 4],
        })
        .unwrap();
        server.update(Duration::ZERO);
        server.update(Duration::from_nanos(16_666_667));
        assert_eq!(server.rate_limited_byte_count(), 1);
        assert_eq!(server.processing_error_count(), 0);

        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 1,
            bytes_per_second: 1_024,
        });
        retain_ack_baselines(&mut server, [1]);
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: vec![0xff],
        })
        .unwrap();
        peer.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(1)),
        })
        .unwrap();
        server.update(Duration::from_nanos(33_333_334));

        assert_eq!(server.processing_error_count(), 1);
        assert_eq!(server.rate_limited_message_count(), 1);
        assert_eq!(server.session.last_acked_tick(SectorId::ZERO), None);
    }

    #[test]
    fn reliable_handshake_bypasses_unreliable_backlog() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = Server::new(World::new(), transport, 60);
        for _ in 0..InboundRateLimitConfig::default().messages_per_second {
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: vec![0xff],
            })
            .unwrap();
        }
        peer.send_reliable(Message {
            kind: MessageKind::Reliable,
            payload: crcbl_net::encode_hello(&crcbl_net::Hello {
                protocol_version: PROTOCOL_VERSION,
                engine_build_id: ENGINE_BUILD_ID,
                schema_hash: SCHEMA_HASH,
                generation: 1,
                session_token: None,
            }),
        })
        .unwrap();

        server.update(Duration::ZERO);
        server.update(Duration::from_nanos(16_666_667));

        assert_eq!(server.session_state(), SessionState::Connected);
        assert!(matches!(
            crcbl_net::decode_handshake_result(&peer.recv().unwrap().unwrap().payload),
            Ok(HandshakeResult::Accept { .. })
        ));
        assert_eq!(server.rate_limited_message_count(), 1);
    }

    #[test]
    fn inbound_limits_are_isolated_per_server() {
        let (a_transport, mut a_peer) = InMemoryTransport::pair();
        let (b_transport, mut b_peer) = InMemoryTransport::pair();
        let mut server_a = Server::new(World::new(), a_transport, 60);
        let mut server_b = Server::new(World::new(), b_transport, 60);
        let limits = InboundRateLimitConfig {
            messages_per_second: 1,
            bytes_per_second: 1_024,
        };
        server_a.set_inbound_rate_limit_config(limits);
        server_b.set_inbound_rate_limit_config(limits);
        retain_ack_baselines(&mut server_a, [1]);
        retain_ack_baselines(&mut server_b, [1]);
        for peer in [&mut a_peer, &mut b_peer] {
            peer.send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload: crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(1)),
            })
            .unwrap();
        }

        server_a.update(Duration::ZERO);
        server_b.update(Duration::ZERO);
        server_a.update(Duration::from_nanos(16_666_667));
        server_b.update(Duration::from_nanos(16_666_667));

        assert_eq!(
            server_a.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(
            server_b.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(server_a.rate_limited_message_count(), 0);
        assert_eq!(server_b.rate_limited_message_count(), 0);
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
