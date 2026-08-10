//! Authoritative server: fixed-tick simulation loop and snapshot emission.
//!
//! The server is the single source of truth. Each tick it drains client inputs,
//! advances the ECS world, and broadcasts a per-system snapshot over an
//! unreliable transport channel. Snapshots are delta-encoded against the
//! client's last-acked baseline (P2b protocol) and carry a per-session MAC
//! (see [`crcbl_net::auth`]) — an unauthenticated packet reaches nothing but
//! the error counter.

pub mod sim_hash;

pub use crcbl_net::rate_limit;
pub use crcbl_net::rate_limit::InboundRateLimitConfig;

use std::fmt;
use std::time::Duration;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{GameModule, Inspector, World};
use crcbl_net::auth::SessionCrypto;
use crcbl_net::rate_limit::InboundRateLimiter;
use crcbl_net::{
    Baseline, DeltaCodec, HandshakeGate, HandshakeResult, Message, ProtocolCompatibility,
    RejectReason, ResumeToken, SectorId, SessionConfig, SessionId, SessionManager, SessionState,
    SnapshotWriter, Transport, Trust,
};

/// Ticks the server will keep delta-encoding against the same acked baseline
/// before it gives up and sends a keyframe.
///
/// A client whose acks stop making progress — because the delta it needs was
/// lost, or because its baseline was evicted from the ring — cannot recover on
/// its own: it will reject every delta that targets a baseline it does not
/// have, forever. This bounds that stall, and is the server half of the same
/// guarantee the client makes by re-announcing its baseline.
const KEYFRAME_RECOVERY_TICKS: u32 = 32;

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
    /// Authenticated channel for this session; `None` until a handshake is
    /// accepted, and replaced whenever the resume token rotates.
    session_crypto: Option<SessionCrypto>,
    next_session_id: u64,
    session_terminated: bool,
    handshake_gate: HandshakeGate,
    rate_limit_config: InboundRateLimitConfig,
    /// One limiter per delivery channel: a flood of unreliable state must not
    /// consume the budget that reliable control traffic needs to be read at
    /// all, which is the whole point of having two channels.
    reliable_rate_limiter: InboundRateLimiter,
    unreliable_rate_limiter: InboundRateLimiter,
    now: Duration,
    /// Ticks since `last_acked_tick` last advanced; drives keyframe recovery.
    ticks_since_ack_progress: u32,
    last_ack_progress: Option<TickId>,
    processing_error_count: u64,
    auth_failure_count: u64,
    rate_limited_message_count: u64,
    rate_limited_byte_count: u64,
    /// Optional game logic module (ticked after the ECS schedule).
    module: Option<Box<dyn GameModule>>,
}

impl<T: Transport> Server<T> {
    /// Create a server with explicit protocol compatibility identifiers.
    ///
    /// There is deliberately no constructor that defaults them:
    /// [`ProtocolCompatibility::DEFAULT`] carries zero engine and schema ids,
    /// which protect nothing, and a networked embedding must supply its own.
    ///
    /// The world's fixed timestep is set from `tick_hz`, so every system
    /// integrates at the rate the server actually runs.
    ///
    /// # Panics
    ///
    /// Panics if `tick_hz` is zero, or if either compatibility identifier is
    /// zero.
    ///
    /// # Errors
    ///
    /// Returns the operating-system entropy error rather than issuing a
    /// predictable resume credential.
    pub fn try_new_with_compatibility(
        mut world: World,
        transport: T,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
    ) -> Result<Self, getrandom::Error> {
        compatibility.assert_explicit();
        let clock = FrameClock::new(tick_hz);
        world.set_tick_dt(clock.tick_dt_secs());
        let config = SessionConfig::default();
        let session_id = SessionId(1);
        let resume_token = Self::generate_resume_token()?;
        let rate_limit_config = InboundRateLimitConfig::default();
        Ok(Self {
            world,
            transport,
            clock,
            session: SessionManager::new(session_id, &config),
            session_config: config,
            resume_token,
            session_crypto: None,
            next_session_id: session_id.0 + 1,
            session_terminated: false,
            handshake_gate: HandshakeGate::new(compatibility),
            rate_limit_config,
            reliable_rate_limiter: InboundRateLimiter::new(rate_limit_config, Duration::ZERO),
            unreliable_rate_limiter: InboundRateLimiter::new(rate_limit_config, Duration::ZERO),
            now: Duration::ZERO,
            ticks_since_ack_progress: 0,
            last_ack_progress: None,
            processing_error_count: 0,
            auth_failure_count: 0,
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
    ///
    /// Each channel is drained under its own budget, so exhausting one leaves
    /// the other readable.
    fn drain_inputs(&mut self) {
        for reliable in [true, false] {
            loop {
                let received = if reliable {
                    self.transport.recv_reliable()
                } else {
                    self.transport.recv()
                };
                match received {
                    Ok(Some(msg)) => {
                        if !self.charge_inbound_budget(reliable, msg.payload.len()) {
                            break;
                        }
                        self.process_inbound_message(&msg.payload);
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
    }

    /// Charge one inbound message against its channel's budget, returning
    /// whether the caller may keep reading that channel.
    fn charge_inbound_budget(&mut self, reliable: bool, bytes: usize) -> bool {
        let now = self.now;
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let limiter = if reliable {
            &mut self.reliable_rate_limiter
        } else {
            &mut self.unreliable_rate_limiter
        };
        match limiter.allow(now, bytes) {
            Ok(()) => true,
            Err((messages_limited, bytes_limited)) => {
                self.rate_limited_message_count = self
                    .rate_limited_message_count
                    .saturating_add(u64::from(messages_limited));
                self.rate_limited_byte_count = self
                    .rate_limited_byte_count
                    .saturating_add(u64::from(bytes_limited));
                false
            }
        }
    }

    /// Dispatch one inbound payload on its tag byte.
    ///
    /// Only the handshake travels unauthenticated — it is what establishes the
    /// key. Everything else must carry a valid session MAC, so a spoofer who
    /// can read tick ids off the wire still cannot move this session's state.
    fn process_inbound_message(&mut self, payload: &[u8]) {
        match payload.first().copied() {
            Some(crcbl_net::codec::HELLO_TAG) => match crcbl_net::decode_hello(payload) {
                Ok(hello) => self.handle_hello(hello),
                Err(_) => self.processing_error_count += 1,
            },
            Some(crcbl_net::auth::AUTH_TAG) => self.process_authenticated_message(payload),
            _ => self.processing_error_count += 1,
        }
    }

    fn process_authenticated_message(&mut self, envelope: &[u8]) {
        // Authenticated traffic only means anything for an established
        // session; before and after that there is no key to check it against.
        if self.session.state() != SessionState::Connected {
            self.auth_failure_count += 1;
            return;
        }
        let Some(crypto) = self.session_crypto.as_mut() else {
            self.auth_failure_count += 1;
            return;
        };
        let payload = match crypto.open(envelope) {
            Ok(payload) => payload.to_vec(),
            Err(_) => {
                self.auth_failure_count += 1;
                return;
            }
        };

        match payload.first().copied() {
            Some(crcbl_net::codec::ACK_TAG) => match crcbl_net::decode_ack(&payload) {
                Ok(ack) => self.session.handle_ack(ack.sector, ack.tick),
                Err(_) => self.processing_error_count += 1,
            },
            // Inputs are decoded to validate them and discarded — P3 wires
            // them into the simulation.
            Some(crcbl_net::codec::INPUT_TAG | crcbl_net::codec::COMMAND_TAG) => {
                if crcbl_net::decode_client_to_server(&payload).is_err() {
                    self.processing_error_count += 1;
                }
            }
            _ => self.processing_error_count += 1,
        }
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
                        self.adopt_session_key();
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
                                if self.session.can_reconnect(
                                    self.now,
                                    hello.engine_build_id,
                                    hello.schema_hash,
                                ) {
                                    if let HandshakeResult::Accept {
                                        resume_token: accepted_token,
                                        ..
                                    } = &mut result
                                    {
                                        *accepted_token = resume_token;
                                    }
                                    if self.send_handshake_result(result)
                                        && self.session.try_reconnect(
                                            self.now,
                                            hello.engine_build_id,
                                            hello.schema_hash,
                                        )
                                    {
                                        // Rotating the token rotates the MAC
                                        // key, which restarts the replay
                                        // counter space for the new session.
                                        self.resume_token = resume_token;
                                        self.adopt_session_key();
                                    }
                                    return;
                                }
                                self.session.expire_if_timed_out(self.now);
                                if self.session.state() == SessionState::Disconnected {
                                    self.session_terminated = true;
                                }
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

    /// Key this session's authenticated channel from the current resume token.
    fn adopt_session_key(&mut self) {
        self.session_crypto = Some(SessionCrypto::from_token(&self.resume_token));
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
        self.session_crypto = None;
        self.session_terminated = false;
        self.ticks_since_ack_progress = 0;
        self.last_ack_progress = None;
        Ok(())
    }

    fn send_handshake_result(&mut self, result: HandshakeResult) -> bool {
        if self
            .transport
            .send_reliable(Message::reliable(crcbl_net::encode_handshake_result(
                &result,
            )))
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
                code: RejectReason::ENTROPY_FAILURE,
                msg: format!("unable to generate resume credential: {error}"),
            },
        }
    }

    fn invalid_session_token(generation: u64, message: &str) -> HandshakeResult {
        HandshakeResult::Reject {
            generation,
            reason: RejectReason {
                code: RejectReason::INVALID_SESSION_TOKEN,
                msg: message.into(),
            },
        }
    }

    /// Build snapshots from the ECS schedule, delta-encode against the
    /// client's baseline, and send.
    fn emit_snapshot(&mut self) {
        let tick = self.clock.tick();
        let sector = SectorId::ZERO;
        let Some(systems) = self.collect_systems(sector, tick) else {
            return;
        };

        // Parse the freshly written blobs exactly once: the same decoded
        // baseline is both what gets diffed and what gets retained.
        let current = match Baseline::from_snapshot(tick, &systems, Trust::Authenticated) {
            Ok(baseline) => baseline,
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };

        // Borrow the retained baseline rather than cloning it: the delta is
        // finished with it before anything needs the store mutably again.
        let previous_tick = self.delta_baseline_tick(sector);
        let delta = {
            let previous = previous_tick.and_then(|tick| {
                self.session
                    .baseline_store(sector)
                    .and_then(|store| store.get(tick))
            });
            DeltaCodec::encode_from_baseline(sector, &current, previous)
        };

        let payload = match crcbl_net::encode_delta(&delta) {
            Ok(payload) => payload,
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };
        let Some(crypto) = self.session_crypto.as_mut() else {
            self.processing_error_count += 1;
            return;
        };
        let payload = match crypto.seal(&payload) {
            Ok(payload) => payload,
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };

        if self
            .transport
            .send_unreliable(Message::unreliable(payload))
            .is_err()
        {
            self.processing_error_count += 1;
            return;
        }

        // Store this full snapshot as a new baseline for future deltas — only
        // once the transport accepted it. A baseline whose snapshot never left
        // the server must not be the reference future deltas encode against:
        // that is what evicts the client's real baseline and makes the desync
        // permanent.
        self.session.baseline_store_mut(sector).insert(current);
    }

    /// Serialise every replicated system into snapshot blobs.
    ///
    /// Returns `None` when two systems collide on a replicated id, which would
    /// otherwise let one system's data land in another's baseline.
    fn collect_systems(
        &mut self,
        sector: SectorId,
        tick: TickId,
    ) -> Option<Vec<crcbl_net::SystemSnapshot>> {
        let mut writer = SnapshotWriter::new_with_sector(sector, tick);
        let stats = Inspector::collect(&self.world);
        let mut seen = std::collections::HashSet::new();

        for (system, stat) in self.world.schedule().iter().zip(stats.iter()) {
            let system_id = replicated_system_id(system.name());
            if !seen.insert(system_id) {
                // Two systems sharing a replicated id would silently overwrite
                // each other in the client's baseline. Drop the whole snapshot
                // rather than replicate a lie.
                self.processing_error_count += 1;
                return None;
            }
            // Systems with a replication impl emit their real per-entity
            // component data; the rest fall back to one synthetic entity
            // carrying only the entity count (4 bytes LE).
            let mut data = Vec::new();
            if !system.replicate(&mut data) {
                crcbl_net::encode_entity_entry(
                    &mut data,
                    0,
                    &(stat.entity_count as u32).to_le_bytes(),
                );
            }
            writer.write_system(system_id, data);
        }

        match writer.finish() {
            crcbl_net::ServerToClient::Snapshot { systems, .. } => Some(systems),
            crcbl_net::ServerToClient::Event { .. } => None,
        }
    }

    /// The tick this delta should be encoded against, or `None` for a keyframe.
    ///
    /// Returns `None` — forcing a keyframe — once the client's acks have
    /// stopped advancing for [`KEYFRAME_RECOVERY_TICKS`], because at that
    /// point the client is provably not applying what it is being sent.
    fn delta_baseline_tick(&mut self, sector: SectorId) -> Option<TickId> {
        let last_acked = self.session.last_acked_tick(sector);
        if last_acked == self.last_ack_progress {
            self.ticks_since_ack_progress = self.ticks_since_ack_progress.saturating_add(1);
        } else {
            self.last_ack_progress = last_acked;
            self.ticks_since_ack_progress = 0;
        }
        if self.ticks_since_ack_progress >= KEYFRAME_RECOVERY_TICKS {
            self.ticks_since_ack_progress = 0;
            return None;
        }

        // Only a tick still in the ring can be delta-encoded against; an
        // evicted one falls back to a keyframe.
        last_acked.filter(|&tick| {
            self.session
                .baseline_store(sector)
                .is_some_and(|store| store.get(tick).is_some())
        })
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
    /// The budget applies to each delivery channel independently, and
    /// reconfiguration resets both buckets to one second of the new budget.
    pub fn set_inbound_rate_limit_config(&mut self, config: InboundRateLimitConfig) {
        self.rate_limit_config = config;
        self.reliable_rate_limiter.reconfigure(config, self.now);
        self.unreliable_rate_limiter.reconfigure(config, self.now);
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

    /// Number of messages rejected because they were unauthenticated, carried
    /// a bad MAC, or replayed a counter.
    ///
    /// A non-zero and growing value on a healthy link means someone is
    /// injecting packets.
    #[must_use]
    pub fn auth_failure_count(&self) -> u64 {
        self.auth_failure_count
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

/// The replicated id for a system, derived from its name.
///
/// Derived from the name rather than the schedule position, because the
/// position changes whenever a system is registered or removed and the client
/// would then apply one system's blobs into another's baseline without any
/// error. FNV-1a: short, stable across builds and platforms, and adequate for
/// an identifier space the server also checks for collisions.
#[must_use]
pub fn replicated_system_id(name: &str) -> u32 {
    const OFFSET_BASIS: u32 = 0x811c_9dc5;
    const PRIME: u32 = 0x0100_0193;
    let mut hash = OFFSET_BASIS;
    for byte in name.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
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
    use crcbl_net::auth::{AUTH_OVERHEAD, AUTH_TAG};
    use crcbl_net::{InMemoryTransport, MAX_IN_MEMORY_MESSAGE_BYTES, MessageKind};

    // ── Helpers ────────────────────────────────────────────────────────────

    /// Explicit identifiers, as a networked embedding must supply. The default
    /// protocol version is used deliberately, so the shipped constant is what
    /// these tests exercise.
    const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 0x0043_5243_424C,
        schema_hash: 0x0053_5256,
    };

    const TICK: Duration = Duration::from_nanos(16_666_667);

    fn server(world: World, transport: InMemoryTransport) -> Server<InMemoryTransport> {
        Server::try_new_with_compatibility(world, transport, 60, COMPATIBILITY)
            .expect("OS CSPRNG available")
    }

    /// Build a world with one system ("position") containing one entity.
    fn world_with_one_entity() -> World {
        let mut world = World::new();
        let e = world.spawn();
        let mut sys = System::<f32>::new("position");
        sys.attach(e, 0.0);
        world.register_system(Box::new(sys));
        world
    }

    fn hello(generation: u64, session_token: Option<ResumeToken>) -> Vec<u8> {
        crcbl_net::encode_hello(&crcbl_net::Hello {
            protocol_version: COMPATIBILITY.protocol_version,
            engine_build_id: COMPATIBILITY.engine_build_id,
            schema_hash: COMPATIBILITY.schema_hash,
            generation,
            session_token,
        })
    }

    /// Drain all messages from a transport, returning the payloads.
    fn drain_payloads(transport: &mut InMemoryTransport) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(Some(msg)) = transport.recv() {
            out.push(msg.payload);
        }
        out
    }

    /// Complete a handshake and return the peer's end of the authenticated
    /// channel, which is what lets a test send an ack the server will accept.
    fn connect(
        server: &mut Server<InMemoryTransport>,
        peer: &mut InMemoryTransport,
    ) -> SessionCrypto {
        peer.send_reliable(Message::reliable(hello(1, None)))
            .unwrap();
        server.update(Duration::ZERO);
        server.update(TICK);

        let mut crypto = None;
        while let Some(msg) = peer.recv().unwrap() {
            if let Ok(HandshakeResult::Accept { resume_token, .. }) =
                crcbl_net::decode_handshake_result(&msg.payload)
            {
                crypto = Some(SessionCrypto::from_token(&resume_token));
            }
        }
        assert_eq!(server.session_state(), SessionState::Connected);
        crypto.expect("server must accept a matching hello")
    }

    fn send_sealed(peer: &mut InMemoryTransport, crypto: &mut SessionCrypto, payload: &[u8]) {
        let sealed = crypto.seal(payload).expect("counter space available");
        peer.send_unreliable(Message::unreliable(sealed)).unwrap();
    }

    fn ack(tick: u64) -> Vec<u8> {
        crcbl_net::encode_ack(SectorId::ZERO, TickId::from_raw(tick))
    }

    fn retain_ack_baselines(
        server: &mut Server<InMemoryTransport>,
        ticks: impl IntoIterator<Item = u64>,
    ) {
        for tick in ticks {
            let tick = TickId::from_raw(tick);
            server.session.baseline_store_mut(SectorId::ZERO).insert(
                Baseline::from_snapshot(tick, &[], Trust::Authenticated)
                    .expect("empty snapshot is valid"),
            );
        }
    }

    /// Serialise a world the way `emit_snapshot` does and return the wire
    /// length of the keyframe it would emit.
    ///
    /// Mirrors `collect_systems`: one synthetic `(0, entity_count)` entry per
    /// system that does not replicate, the same replicated ids, and the same
    /// delta wire format. The result is the encoded size before the seal.
    fn keyframe_payload_len(world: &World) -> usize {
        let stats = Inspector::collect(world);
        let mut systems = Vec::new();
        for (system, stat) in world.schedule().iter().zip(stats.iter()) {
            let mut data = Vec::new();
            if !system.replicate(&mut data) {
                crcbl_net::encode_entity_entry(
                    &mut data,
                    0,
                    &(stat.entity_count as u32).to_le_bytes(),
                );
            }
            systems.push(crcbl_net::SystemSnapshot {
                system_id: replicated_system_id(system.name()),
                data,
            });
        }
        let baseline = Baseline::from_snapshot(TickId::from_raw(1), &systems, Trust::Authenticated)
            .expect("test world must serialise like a real one");
        let keyframe = DeltaCodec::encode_from_baseline(SectorId::ZERO, &baseline, None);
        crcbl_net::encode_delta(&keyframe)
            .expect("probe worlds stay below the encode cap")
            .len()
    }

    // ── Construction ───────────────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "engine_build_id and schema_hash must be non-zero")]
    fn placeholder_compatibility_is_refused() {
        let (transport, _peer) = InMemoryTransport::pair();
        let _ = Server::try_new_with_compatibility(
            World::new(),
            transport,
            60,
            ProtocolCompatibility::DEFAULT,
        );
    }

    #[test]
    fn world_timestep_matches_the_configured_tick_rate() {
        for tick_hz in [30u32, 60, 128] {
            let (transport, _peer) = InMemoryTransport::pair();
            let server =
                Server::try_new_with_compatibility(World::new(), transport, tick_hz, COMPATIBILITY)
                    .expect("OS CSPRNG available");
            let expected = FrameClock::new(tick_hz).tick_dt_secs();
            assert!(
                (server.world().tick_dt() - expected).abs() < 1e-12,
                "at {tick_hz} Hz the world ticks at {}, not {expected}",
                server.world().tick_dt(),
            );
            assert_ne!(
                server.world().tick_dt(),
                World::DEFAULT_TICK_DT,
                "{tick_hz} Hz must not silently run at the 60 Hz default"
            );
        }
    }

    #[test]
    fn server_starts_at_tick_zero_and_connected() {
        let (transport, _peer) = InMemoryTransport::pair();
        let server = server(World::new(), transport);
        assert_eq!(server.tick_id(), TickId::ZERO);
        assert!(server.is_connected());
    }

    // ── Session lifecycle ──────────────────────────────────────────────────

    #[test]
    fn failed_reconnect_accept_keeps_previous_credential() {
        let (transport, peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        server.session.begin_handshake();
        server
            .session
            .on_connected(COMPATIBILITY.engine_build_id, COMPATIBILITY.schema_hash);
        server
            .session
            .on_disconnect(Duration::ZERO, &server.session_config);
        let token = server.resume_token;
        drop(peer);

        server.handle_hello(crcbl_net::Hello {
            protocol_version: COMPATIBILITY.protocol_version,
            engine_build_id: COMPATIBILITY.engine_build_id,
            schema_hash: COMPATIBILITY.schema_hash,
            generation: 1,
            session_token: Some(token),
        });

        assert_eq!(server.session.state(), SessionState::Reconnecting);
        assert_eq!(server.resume_token, token);
        assert_eq!(server.processing_error_count, 1);
    }

    #[test]
    fn rotating_session_clears_baselines_acks_and_the_session_key() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        let old_session_id = server.session.session_id();
        let old_token = server.resume_token;
        let tick = TickId::from_raw(1);
        retain_ack_baselines(&mut server, [1]);
        server.session.handle_ack(SectorId::ZERO, tick);
        server.adopt_session_key();

        server.rotate_session().expect("OS CSPRNG available");

        assert_ne!(server.session.session_id(), old_session_id);
        assert_ne!(server.resume_token, old_token);
        assert_eq!(server.session.last_acked_tick(SectorId::ZERO), None);
        assert!(server.session.baseline_store(SectorId::ZERO).is_none());
        assert!(server.session_crypto.is_none());
    }

    // ── Tick loop ──────────────────────────────────────────────────────────

    #[test]
    fn update_with_no_elapsed_time_does_nothing() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        assert_eq!(server.update(Duration::ZERO), 0);
        assert_eq!(server.tick_id(), TickId::ZERO);
    }

    #[test]
    fn update_runs_ticks_for_elapsed_time() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);

        server.update(Duration::ZERO);
        assert_eq!(server.update(TICK), 1);
        assert_eq!(server.tick_id(), TickId::from_raw(1));

        let stats = Inspector::collect(server.world());
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].entity_count, 1);
    }

    #[test]
    fn tick_loop_with_no_inputs_does_not_panic() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);

        server.update(Duration::ZERO);
        assert_eq!(server.update(5 * TICK), 5);
        assert_eq!(server.tick_id(), TickId::from_raw(5));
    }

    // ── Snapshot emission ──────────────────────────────────────────────────

    #[test]
    fn snapshots_are_sealed_and_delta_encodable() {
        let (server_transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), server_transport);
        let mut crypto = connect(&mut server, &mut peer);

        server.update(2 * TICK);

        let msg = peer.recv().unwrap().unwrap();
        assert_eq!(msg.kind, MessageKind::Unreliable);
        assert_eq!(msg.payload.first(), Some(&AUTH_TAG));
        let payload = crypto
            .open(&msg.payload)
            .expect("server seals its snapshots");
        assert!(
            crcbl_net::decode_delta(payload, Trust::Authenticated).is_ok(),
            "snapshot payload must decode as Delta"
        );
    }

    #[test]
    fn multiple_ticks_produce_multiple_snapshots() {
        let (server_transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), server_transport);
        connect(&mut server, &mut peer);

        server.update(4 * TICK);

        assert_eq!(drain_payloads(&mut peer).len(), 3);
    }

    #[test]
    fn replicated_system_ids_follow_the_name_not_the_schedule_position() {
        let build = |names: &[&str]| {
            let mut world = World::new();
            for name in names {
                world.register_system(Box::new(System::<f32>::new(*name)));
            }
            let (transport, mut peer) = InMemoryTransport::pair();
            let mut server = server(world, transport);
            let mut crypto = connect(&mut server, &mut peer);
            server.update(2 * TICK);
            let msg = peer.recv().unwrap().unwrap();
            let payload = crypto.open(&msg.payload).expect("sealed snapshot");
            let mut ids: Vec<u32> = crcbl_net::decode_delta(payload, Trust::Authenticated)
                .expect("valid delta")
                .systems
                .iter()
                .map(|system| system.system_id)
                .collect();
            ids.sort_unstable();
            ids
        };

        // Registering a new system first used to shift every later system's
        // replicated id by one, so the client applied one system's blobs into
        // another's baseline.
        let before = build(&["physics", "render"]);
        let after = build(&["audio", "physics", "render"]);
        assert_eq!(before.len(), 2);
        assert_eq!(after.len(), 3);
        for id in &before {
            assert!(
                after.contains(id),
                "system id {id} changed when an unrelated system was registered"
            );
        }
    }

    #[test]
    fn colliding_system_names_refuse_to_replicate() {
        let mut world = World::new();
        world.register_system(Box::new(System::<f32>::new("duplicate")));
        world.register_system(Box::new(System::<f32>::new("duplicate")));
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world, transport);
        connect(&mut server, &mut peer);

        server.update(2 * TICK);

        assert!(
            drain_payloads(&mut peer).is_empty(),
            "a snapshot whose system ids collide must not be sent at all"
        );
        assert_eq!(server.processing_error_count(), 1);
    }

    #[test]
    fn oversized_delta_is_not_retained_as_a_baseline() {
        // A delta in (MAX_IN_MEMORY_MESSAGE_BYTES - AUTH_OVERHEAD,
        // MAX_IN_MEMORY_MESSAGE_BYTES] encodes under the old cap but seals to
        // more than the transport accepts. The server must refuse it at encode
        // time — and must not retain it as the next delta's baseline.
        //
        // `System<T>` does not replicate, so every system costs a fixed 32
        // wire bytes (16-byte system header + one synthetic 16-byte entity);
        // the size knob is the system count.
        const FIXTURE_SYSTEMS: usize = 2046;

        // The full fixture's delta exceeds the encode cap once it leaves room
        // for the seal, so the encoder cannot measure it directly. Measure the
        // real per-system wire cost on small worlds and extrapolate.
        let probe_len = |count: usize| {
            let mut world = World::new();
            for i in 0..count {
                world.register_system(Box::new(System::<f32>::new(format!("position_{i}"))));
            }
            keyframe_payload_len(&world)
        };
        let one_system = probe_len(1);
        let per_system_bytes = probe_len(2) - one_system;
        assert_eq!(
            per_system_bytes, 32,
            "a non-replicating system must cost 16 header + 12 entry + 4 count bytes"
        );
        let fixture_len = one_system + per_system_bytes * (FIXTURE_SYSTEMS - 1);
        assert!(
            fixture_len > MAX_IN_MEMORY_MESSAGE_BYTES - AUTH_OVERHEAD
                && fixture_len <= MAX_IN_MEMORY_MESSAGE_BYTES,
            "fixture delta of {fixture_len} bytes must fall in the drop window \
             ({}, {}]",
            MAX_IN_MEMORY_MESSAGE_BYTES - AUTH_OVERHEAD,
            MAX_IN_MEMORY_MESSAGE_BYTES,
        );

        let mut world = World::new();
        for i in 0..FIXTURE_SYSTEMS {
            world.register_system(Box::new(System::<f32>::new(format!("position_{i}"))));
        }
        let (server_transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world, server_transport);
        let mut crypto = connect(&mut server, &mut peer);

        // The client acks tick 1, whose retained baseline is empty; the next
        // delta is the whole world, sized to be dropped by the transport.
        retain_ack_baselines(&mut server, [1]);
        send_sealed(&mut peer, &mut crypto, &ack(1));
        server.update(2 * TICK);

        assert_eq!(server.processing_error_count(), 1);
        let emitted_tick = server.tick_id();
        let store = server.session.baseline_store(SectorId::ZERO).unwrap();
        assert!(
            store.get(emitted_tick).is_none(),
            "a snapshot the transport dropped must not become the next delta's \
             baseline (tick {emitted_tick} was retained)"
        );
        assert!(
            store.get(TickId::from_raw(1)).is_some(),
            "the client's acked baseline must survive"
        );
    }

    // ── Authentication ─────────────────────────────────────────────────────

    #[test]
    fn a_forged_ack_cannot_move_the_session() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        connect(&mut server, &mut peer);
        retain_ack_baselines(&mut server, [1, 2, 3]);

        // The attacker knows the tick — it is in every snapshot in cleartext —
        // but not the session key.
        let mut forged = SessionCrypto::from_token(&ResumeToken::from_bytes([0xFF; 32]));
        send_sealed(&mut peer, &mut forged, &ack(3));
        // ...and a bare, unauthenticated ack, which is what the old protocol
        // accepted.
        peer.send_unreliable(Message::unreliable(ack(3))).unwrap();
        server.update(2 * TICK);

        assert_eq!(server.session.last_acked_tick(SectorId::ZERO), None);
        assert_eq!(server.auth_failure_count(), 1);
        assert_eq!(server.processing_error_count(), 1);
    }

    #[test]
    fn a_genuine_ack_is_accepted_and_its_replay_is_not() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        retain_ack_baselines(&mut server, [1, 2]);

        let sealed = crypto.seal(&ack(1)).expect("counter space available");
        peer.send_unreliable(Message::unreliable(sealed.clone()))
            .unwrap();
        server.update(2 * TICK);
        assert_eq!(
            server.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(server.auth_failure_count(), 0);

        // A captured packet replayed verbatim carries a valid MAC; only the
        // replay counter rejects it.
        peer.send_unreliable(Message::unreliable(sealed)).unwrap();
        send_sealed(&mut peer, &mut crypto, &ack(2));
        server.update(3 * TICK);
        assert_eq!(
            server.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(2))
        );
        assert_eq!(server.auth_failure_count(), 1);
    }

    #[test]
    fn a_forged_ack_cannot_stall_keyframe_recovery() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);
        let mut crypto = connect(&mut server, &mut peer);

        // One honest ack, so the server has a baseline to delta against.
        server.update(2 * TICK);
        let msg = peer.recv().unwrap().unwrap();
        let payload = crypto.open(&msg.payload).expect("sealed snapshot");
        let tick = crcbl_net::decode_delta(payload, Trust::Authenticated)
            .expect("valid delta")
            .tick;
        send_sealed(
            &mut peer,
            &mut crypto,
            &crcbl_net::encode_ack(SectorId::ZERO, tick),
        );
        server.update(3 * TICK);

        // The client then goes quiet — which is what a desynced client looks
        // like from here. The server must stop delta-encoding against a
        // baseline the client is provably not applying.
        let mut now = 3 * TICK;
        let mut sent_keyframe = false;
        for _ in 0..(KEYFRAME_RECOVERY_TICKS + 4) {
            now += TICK;
            server.update(now);
            for msg in drain_payloads(&mut peer) {
                let payload = crypto.open(&msg).expect("sealed snapshot");
                if crcbl_net::decode_delta(payload, Trust::Authenticated)
                    .expect("valid delta")
                    .is_keyframe
                {
                    sent_keyframe = true;
                }
            }
        }
        assert!(
            sent_keyframe,
            "a client whose acks stop advancing must eventually be sent a keyframe"
        );
    }

    #[test]
    fn authenticated_inputs_are_accepted_and_junk_is_counted() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        let mut crypto = connect(&mut server, &mut peer);

        let input = crcbl_net::encode_client_to_server(&crcbl_net::ClientToServer::Input {
            tick: TickId::from_raw(1),
            data: vec![1, 2, 3],
        });
        send_sealed(&mut peer, &mut crypto, &input);
        send_sealed(&mut peer, &mut crypto, &[0xEE]);
        server.update(2 * TICK);

        assert_eq!(server.processing_error_count(), 1);
        assert_eq!(server.auth_failure_count(), 0);
    }

    // ── Inbound rate limiting ──────────────────────────────────────────────

    #[test]
    fn inbound_message_limit_accepts_boundary_then_drops_next() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 2,
            bytes_per_second: 1_024,
        });
        retain_ack_baselines(&mut server, 1..=3);
        for tick in 1..=3 {
            send_sealed(&mut peer, &mut crypto, &ack(tick));
        }

        server.update(2 * TICK);

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
        let mut server = server(World::new(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        let sealed_len = (crcbl_net::auth::AUTH_OVERHEAD + ack(1).len()) as u64;
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 2,
            bytes_per_second: sealed_len,
        });
        retain_ack_baselines(&mut server, [1, 2]);
        send_sealed(&mut peer, &mut crypto, &ack(1));
        send_sealed(&mut peer, &mut crypto, &ack(2));

        server.update(2 * TICK);

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
        let mut server = server(World::new(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 1,
            bytes_per_second: 1_024,
        });
        retain_ack_baselines(&mut server, 1..=4);

        for tick in 1..=2 {
            send_sealed(&mut peer, &mut crypto, &ack(tick));
        }
        server.update(2 * TICK);
        assert_eq!(
            server.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(server.rate_limited_message_count(), 1);

        // A second of injected time restores exactly one message of budget.
        send_sealed(&mut peer, &mut crypto, &ack(3));
        server.update(Duration::from_secs(1) + 2 * TICK);
        assert_eq!(
            server.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(3))
        );
        assert_eq!(server.rate_limited_message_count(), 1);
        assert_eq!(server.rate_limited_byte_count(), 0);
    }

    #[test]
    fn oversized_and_malformed_packets_are_limited_before_decode() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        connect(&mut server, &mut peer);
        server.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 2,
            bytes_per_second: 3,
        });
        peer.send_unreliable(Message::unreliable(vec![0; 4]))
            .unwrap();
        server.update(2 * TICK);
        assert_eq!(server.rate_limited_byte_count(), 1);
        assert_eq!(server.processing_error_count(), 0);
        assert_eq!(server.auth_failure_count(), 0);
    }

    #[test]
    fn an_unreliable_flood_cannot_head_of_line_block_the_handshake() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        let budget = InboundRateLimitConfig::default().messages_per_second;
        // Twice the whole per-second budget of junk: under a single shared
        // limiter this exhausted it and the reliable queue stopped being read.
        for _ in 0..(2 * budget) {
            peer.send_unreliable(Message::unreliable(vec![0xff]))
                .unwrap();
        }
        peer.send_reliable(Message::reliable(hello(1, None)))
            .unwrap();

        server.update(Duration::ZERO);
        server.update(TICK);

        assert_eq!(server.session_state(), SessionState::Connected);
        assert!(matches!(
            crcbl_net::decode_handshake_result(&peer.recv().unwrap().unwrap().payload),
            Ok(HandshakeResult::Accept { .. })
        ));
        assert!(server.rate_limited_message_count() > 0);
    }

    #[test]
    fn inbound_limits_are_isolated_per_server() {
        let (a_transport, mut a_peer) = InMemoryTransport::pair();
        let (b_transport, mut b_peer) = InMemoryTransport::pair();
        let mut server_a = server(World::new(), a_transport);
        let mut server_b = server(World::new(), b_transport);
        let mut crypto_a = connect(&mut server_a, &mut a_peer);
        let mut crypto_b = connect(&mut server_b, &mut b_peer);
        let limits = InboundRateLimitConfig {
            messages_per_second: 1,
            bytes_per_second: 1_024,
        };
        server_a.set_inbound_rate_limit_config(limits);
        server_b.set_inbound_rate_limit_config(limits);
        retain_ack_baselines(&mut server_a, [1]);
        retain_ack_baselines(&mut server_b, [1]);
        send_sealed(&mut a_peer, &mut crypto_a, &ack(1));
        send_sealed(&mut b_peer, &mut crypto_b, &ack(1));

        server_a.update(2 * TICK);
        server_b.update(2 * TICK);

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

    // ── System ids ─────────────────────────────────────────────────────────

    #[test]
    fn replicated_system_id_is_stable_and_name_specific() {
        assert_eq!(
            replicated_system_id("physics"),
            replicated_system_id("physics")
        );
        assert_ne!(
            replicated_system_id("physics"),
            replicated_system_id("render")
        );
        // FNV-1a of the empty string is the offset basis.
        assert_eq!(replicated_system_id(""), 0x811c_9dc5);
    }

    // ── Debug ──────────────────────────────────────────────────────────────

    #[test]
    fn the_servers_debug_output_names_it_and_reports_whether_it_is_connected() {
        let (transport, _peer) = InMemoryTransport::pair();
        let server = server(World::new(), transport);
        let s = format!("{server:?}");
        assert!(s.contains("Server"));
        assert!(s.contains("connected"));
    }
}
