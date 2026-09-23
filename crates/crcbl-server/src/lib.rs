//! Authoritative server: fixed-tick simulation loop and snapshot emission.
//!
//! The server is the single source of truth. Each tick it drains client inputs,
//! advances the ECS world, and broadcasts a per-system snapshot over an
//! unreliable transport channel. Snapshots are delta-encoded against the
//! client's last-acked baseline (P2b protocol) and carry a per-session MAC
//! (see [`crcbl_net::auth`]) — an unauthenticated packet reaches nothing but
//! the error counter.

mod peer;
pub mod sim_hash;

pub use crcbl_net::rate_limit;
pub use crcbl_net::rate_limit::InboundRateLimitConfig;

use std::fmt;
use std::time::Duration;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{ClientInputs, GameModule, World};
use crcbl_net::{
    HandshakeGate, HandshakeResult, ProtocolCompatibility, SectorId, SessionConfig, SessionId,
    SessionState, Transport,
};

use peer::{Counters, PeerSession};

/// Ticks the server will keep delta-encoding against the same acked baseline
/// before it gives up and sends a keyframe.
///
/// A client whose acks stop making progress — because the delta it needs was
/// lost, or because its baseline was evicted from the ring — cannot recover on
/// its own: it will reject every delta that targets a baseline it does not
/// have, forever. This bounds that stall, and is the server half of the same
/// guarantee the client makes by re-announcing its baseline.
const KEYFRAME_RECOVERY_TICKS: u32 = 32;

/// The most client input frames one tick will hold.
///
/// **How many arrive is the peer's choice, not ours.** A client sends one
/// frame per tick its own clock consumed, and
/// [`DEFAULT_MAX_CATCH_UP_TICKS`](crcbl_core::time::DEFAULT_MAX_CATCH_UP_TICKS)
/// is the most a well-behaved one hands itself for a single frame — so twice
/// that leaves room for a client running ahead of this server's rate and still
/// bounds what a peer that simply keeps sending can make the server allocate.
/// Whatever a tick does not hold is refused, and
/// [`Server::dropped_input_count`] is what says so.
///
/// Public because it is a statement to the other end of the wire: a client
/// that sends more input than this for one server tick is sending some of it
/// into a counter.
pub const MAX_CLIENT_INPUTS_PER_TICK: usize =
    2 * crcbl_core::time::DEFAULT_MAX_CATCH_UP_TICKS as usize;

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
    /// The one client's session, credential, channel, budgets and inputs.
    peer: PeerSession,
    session_config: SessionConfig,
    next_session_id: u64,
    session_terminated: bool,
    handshake_gate: HandshakeGate,
    rate_limit_config: InboundRateLimitConfig,
    now: Duration,
    counters: Counters,
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
    /// Returns the entropy source's error — the OS CSPRNG failing on native, or
    /// an unseeded source on `wasm32` — rather than issuing a predictable resume
    /// credential.
    pub fn try_new_with_compatibility(
        mut world: World,
        transport: T,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
    ) -> Result<Self, crcbl_rand::Error> {
        compatibility.assert_explicit();
        let clock = FrameClock::new(tick_hz);
        world.set_tick_dt(clock.tick_dt_secs());
        let config = SessionConfig::default();
        let session_id = SessionId(1);
        let resume_token = peer::generate_resume_token()?;
        let rate_limit_config = InboundRateLimitConfig::default();
        Ok(Self {
            world,
            transport,
            clock,
            peer: PeerSession::new(
                session_id,
                &config,
                resume_token,
                rate_limit_config,
                Duration::ZERO,
            ),
            session_config: config,
            next_session_id: session_id.0 + 1,
            session_terminated: false,
            handshake_gate: HandshakeGate::new(compatibility),
            rate_limit_config,
            now: Duration::ZERO,
            counters: Counters::default(),
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
    /// world (ECS schedule), tick the game module (if any), sweep what the
    /// module destroyed, emit delta-encoded snapshot.
    fn tick(&mut self) {
        let was_connected = self.peer.session.state() == SessionState::Connected;
        self.peer.begin_tick();
        self.drain_inputs();
        self.update_session_for_transport();
        self.world.tick();
        if let Some(ref mut module) = self.module {
            // Three disjoint field borrows: the module, the world it mutates,
            // and the inputs it reads. That is why the inputs are an argument
            // and not something the module reaches back through `Server` for.
            module.tick(
                &mut self.world,
                ClientInputs::new(&self.peer.client_inputs, self.peer.dropped_inputs),
            );
        }
        // `World::despawn` only marks: the entity stays in the pool and in
        // every system's storage until a sweep. `World::tick` sweeps at its own
        // end, which is *before* the module ran — so without this second sweep
        // the snapshot below serialises everything the module just destroyed
        // and the client is told about entities the server no longer has. The
        // queue is almost always empty here and `sweep` returns immediately
        // when it is.
        self.world.sweep();
        if was_connected && self.peer.session.state() == SessionState::Connected {
            self.emit_snapshot();
        }
    }

    fn update_session_for_transport(&mut self) {
        if !self.transport.is_connected() && self.peer.session.state() == SessionState::Connected {
            self.peer
                .session
                .on_disconnect(self.now, &self.session_config);
        }
        let was_reconnecting = self.peer.session.state() == SessionState::Reconnecting;
        self.peer.session.expire_if_timed_out(self.now);
        if was_reconnecting && self.peer.session.state() == SessionState::Disconnected {
            self.session_terminated = true;
        }
    }

    /// Consume queued client messages: handshake, inputs, and acks.
    ///
    /// Inputs are queued for this tick's [`GameModule::tick`].
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
                        if !self.peer.charge_inbound_budget(
                            reliable,
                            msg.payload.len(),
                            self.now,
                            &mut self.counters,
                        ) {
                            break;
                        }
                        self.process_inbound_message(&msg.payload);
                    }
                    Ok(None) => break,
                    Err(crcbl_net::TransportError::Disconnected) => break,
                    Err(_) => {
                        self.counters.processing_errors += 1;
                        break;
                    }
                }
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
                Err(_) => self.counters.processing_errors += 1,
            },
            Some(crcbl_net::auth::AUTH_TAG) => self
                .peer
                .process_authenticated_message(payload, &mut self.counters),
            _ => self.counters.processing_errors += 1,
        }
    }

    fn handle_hello(&mut self, hello: crcbl_net::Hello) {
        if self.session_terminated
            && self.peer.session.state() == SessionState::Disconnected
            && hello.session_token.is_none()
            && let Err(error) = self.rotate_session()
        {
            self.counters.processing_errors += 1;
            self.send_handshake_result(peer::entropy_failure(hello.generation, error));
            return;
        }
        let mut result = self.handshake_gate.validate(
            &hello,
            self.peer.session.session_id(),
            self.peer.resume_token,
            self.clock.tick(),
        );
        if matches!(result, HandshakeResult::Accept { .. }) {
            let expected_token = self.peer.resume_token;
            match self.peer.session.state() {
                SessionState::Disconnected => {
                    if hello.session_token.is_some() {
                        result = peer::invalid_session_token(
                            hello.generation,
                            "fresh handshake must not include a session token",
                        );
                    } else {
                        self.peer.session.begin_handshake();
                        self.peer
                            .session
                            .on_connected(hello.engine_build_id, hello.schema_hash);
                        self.peer.adopt_session_key();
                    }
                }
                SessionState::Reconnecting => {
                    if !hello
                        .session_token
                        .is_some_and(|token| token == expected_token)
                    {
                        result = peer::invalid_session_token(
                            hello.generation,
                            "reconnect session token does not match",
                        );
                    } else {
                        match peer::generate_resume_token() {
                            Ok(resume_token) => {
                                if self.peer.session.can_reconnect(
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
                                        && self.peer.session.try_reconnect(
                                            self.now,
                                            hello.engine_build_id,
                                            hello.schema_hash,
                                        )
                                    {
                                        // Rotating the token rotates the MAC
                                        // key, which restarts the replay
                                        // counter space for the new session.
                                        self.peer.resume_token = resume_token;
                                        self.peer.adopt_session_key();
                                    }
                                    return;
                                }
                                self.peer.session.expire_if_timed_out(self.now);
                                if self.peer.session.state() == SessionState::Disconnected {
                                    self.session_terminated = true;
                                }
                                result = peer::invalid_session_token(
                                    hello.generation,
                                    "reconnect grace period expired",
                                );
                            }
                            Err(error) => {
                                self.counters.processing_errors += 1;
                                result = peer::entropy_failure(hello.generation, error);
                            }
                        }
                    }
                }
                SessionState::Handshaking => {
                    result = peer::invalid_session_token(
                        hello.generation,
                        "handshake is already in progress",
                    );
                }
                SessionState::Connected => {
                    if !hello
                        .session_token
                        .is_some_and(|token| token == expected_token)
                    {
                        result = peer::invalid_session_token(
                            hello.generation,
                            "session token does not match",
                        );
                    }
                }
            }
        }
        self.send_handshake_result(result);
    }

    fn rotate_session(&mut self) -> Result<(), crcbl_rand::Error> {
        let resume_token = peer::generate_resume_token()?;
        let session_id = SessionId(self.next_session_id);
        self.next_session_id = self.next_session_id.wrapping_add(1);
        self.peer
            .replace_session(session_id, &self.session_config, resume_token);
        self.session_terminated = false;
        Ok(())
    }

    fn send_handshake_result(&mut self, result: HandshakeResult) -> bool {
        peer::send_handshake_result(&mut self.transport, &result, &mut self.counters)
    }

    /// Build snapshots from the ECS schedule, delta-encode against the
    /// client's baseline, and send.
    fn emit_snapshot(&mut self) {
        let tick = self.clock.tick();
        let sector = SectorId::ZERO;
        let Some(current) = peer::current_baseline(&self.world, sector, tick, &mut self.counters)
        else {
            return;
        };
        self.peer
            .send_snapshot(&mut self.transport, sector, current, &mut self.counters);
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
        self.peer.reconfigure_rate_limit(config, self.now);
    }

    /// Attach a [`GameModule`] to drive game-specific per-tick logic.
    ///
    /// The module's [`GameModule::tick`] is called every server tick after the
    /// ECS schedule runs, and is handed the [`ClientInputs`] that arrived since
    /// the previous tick — which is the only way client input reaches game
    /// logic. Only one module can be attached at a time; calling this again
    /// replaces any existing module.
    ///
    /// Whatever the module despawns is swept before the tick's snapshot is
    /// serialised, so a destruction is replicated on the tick it happened.
    /// Within one call to [`GameModule::tick`] a despawned entity is still
    /// readable — the sweep runs once that call returns — but it is gone by the
    /// next tick, and no snapshot ever carries it.
    pub fn set_module(&mut self, module: Box<dyn GameModule>) {
        self.module = Some(module);
    }

    /// Number of messages dropped because their message-rate budget was exhausted.
    #[must_use]
    pub fn rate_limited_message_count(&self) -> u64 {
        self.counters.rate_limited_messages
    }

    /// Number of messages dropped because their byte-rate budget was exhausted.
    #[must_use]
    pub fn rate_limited_byte_count(&self) -> u64 {
        self.counters.rate_limited_bytes
    }

    /// Number of input frames refused because the tick they arrived in was
    /// already holding [`MAX_CLIENT_INPUTS_PER_TICK`].
    ///
    /// A growing value means a peer is sending input faster than this server
    /// will hold it. The frames were never stored and are not recoverable;
    /// this is what keeps their loss from being silent.
    #[must_use]
    pub fn dropped_input_count(&self) -> u64 {
        self.counters.dropped_inputs
    }

    /// Number of messages rejected because they were unauthenticated, carried
    /// a bad MAC, or replayed a counter.
    ///
    /// A non-zero and growing value on a healthy link means someone is
    /// injecting packets.
    #[must_use]
    pub fn auth_failure_count(&self) -> u64 {
        self.counters.auth_failures
    }

    /// Current session lifecycle state.
    #[must_use]
    pub fn session_state(&self) -> SessionState {
        self.peer.session.state()
    }

    /// Number of unrecoverable transport, encoding, decoding, or lifecycle errors.
    #[must_use]
    pub fn processing_error_count(&self) -> u64 {
        self.counters.processing_errors
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
            .field("session_state", &self.peer.session.state())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_ecs::Inspector;
    use crcbl_ecs::System;
    use crcbl_net::auth::SessionCrypto;
    use crcbl_net::auth::{AUTH_OVERHEAD, AUTH_TAG};
    use crcbl_net::{
        Baseline, DeltaCodec, InMemoryTransport, MAX_IN_MEMORY_MESSAGE_BYTES, Message, MessageKind,
        ResumeToken, Trust,
    };

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
            server
                .peer
                .session
                .baseline_store_mut(SectorId::ZERO)
                .insert(
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

    /// A world whose one system replicates real per-entity blobs, plus the
    /// entities in it.
    ///
    /// `System<T>` does not replicate, so a snapshot of one carries a synthetic
    /// entity *count* and no ids at all — it cannot answer "which entity is on
    /// the wire". `PhysicsSystem` publishes a transform per entity keyed by
    /// entity bits, which is what a client decodes.
    fn world_with_replicated_entities(n: usize) -> (World, Vec<crcbl_ecs::Entity>) {
        let mut world = World::new();
        let mut phys = crcbl_phys::PhysicsSystem::new();
        let entities: Vec<_> = (0..n).map(|_| world.spawn()).collect();
        for (i, &entity) in entities.iter().enumerate() {
            phys.set_transform(
                entity,
                crcbl_phys::Transform::from_position(glam::DVec3::new(i as f64, 0.0, 0.0)),
            );
        }
        world.register_system(Box::new(phys));
        (world, entities)
    }

    /// A [`GameModule`] that despawns `victim` on the first tick after the test
    /// arms it.
    ///
    /// Armed from outside rather than counting its own ticks, so a test says
    /// exactly which `Server::update` call is the one that kills the entity
    /// without having to know how the clock maps elapsed time onto ticks.
    struct DespawnWhenArmed {
        victim: crcbl_ecs::Entity,
        armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl GameModule for DespawnWhenArmed {
        fn name(&self) -> &str {
            "despawn-when-armed"
        }

        fn register(&self, _world: &mut World) {}

        fn tick(&mut self, world: &mut World, _inputs: ClientInputs<'_>) {
            if self.armed.swap(false, std::sync::atomic::Ordering::Relaxed) {
                world.despawn(self.victim);
            }
        }
    }

    /// What one [`GameModule::tick`] call was handed, recorded from inside the
    /// server's own tick.
    ///
    /// Shared rather than read back off the module, because `set_module` moves
    /// the module into the `Server` and nothing hands it back.
    #[derive(Debug, Default)]
    struct SeenInputs {
        /// One entry per tick the module ran: the frames of that tick.
        ticks: Vec<Vec<(TickId, Vec<u8>)>>,
        /// The drop count of the last tick's view.
        dropped: u32,
    }

    /// A [`GameModule`] that records the [`ClientInputs`] of every tick it runs.
    struct RecordInputs {
        seen: std::sync::Arc<std::sync::Mutex<SeenInputs>>,
    }

    impl GameModule for RecordInputs {
        fn name(&self) -> &str {
            "record-inputs"
        }

        fn register(&self, _world: &mut World) {}

        fn tick(&mut self, _world: &mut World, inputs: ClientInputs<'_>) {
            let mut seen = self.seen.lock().expect("test module is not poisoned");
            seen.ticks.push(
                inputs
                    .iter()
                    .map(|(tick, data)| (tick, data.to_vec()))
                    .collect(),
            );
            seen.dropped = inputs.dropped();
        }
    }

    /// Attaches a [`RecordInputs`] module to `server` and returns what it sees.
    fn record_inputs(
        server: &mut Server<InMemoryTransport>,
    ) -> std::sync::Arc<std::sync::Mutex<SeenInputs>> {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(SeenInputs::default()));
        server.set_module(Box::new(RecordInputs {
            seen: std::sync::Arc::clone(&seen),
        }));
        seen
    }

    /// One `ClientToServer::Input` on the wire.
    fn input(tick: u64, data: &[u8]) -> Vec<u8> {
        crcbl_net::encode_client_to_server(&crcbl_net::ClientToServer::Input {
            tick: TickId::from_raw(tick),
            data: data.to_vec(),
        })
    }

    /// Unseal and decode one snapshot payload the way a client does.
    fn open_delta(crypto: &mut SessionCrypto, payload: &[u8]) -> crcbl_net::Delta {
        let opened = crypto.open(payload).expect("server seals its snapshots");
        crcbl_net::decode_delta(opened, Trust::Authenticated).expect("valid delta")
    }

    /// Every entity a client would hold state for after applying `delta`:
    /// keyframe contents, newly added entities, and updated ones.
    ///
    /// Unchanged entities are deliberately absent — they are not on the wire —
    /// so this answers "does this packet describe entity X", which is the
    /// question the despawn tests ask of it.
    fn entities_described(delta: &crcbl_net::Delta) -> Vec<u64> {
        delta
            .systems
            .iter()
            .flat_map(|system| system.added.iter().chain(system.modified.iter()))
            .map(|entity| entity.entity_bits)
            .collect()
    }

    /// Every entity `delta` tombstones.
    fn entities_removed(delta: &crcbl_net::Delta) -> Vec<u64> {
        delta
            .systems
            .iter()
            .flat_map(|system| system.removed.iter().copied())
            .collect()
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
        server.peer.session.begin_handshake();
        server
            .peer
            .session
            .on_connected(COMPATIBILITY.engine_build_id, COMPATIBILITY.schema_hash);
        server
            .peer
            .session
            .on_disconnect(Duration::ZERO, &server.session_config);
        let token = server.peer.resume_token;
        drop(peer);

        server.handle_hello(crcbl_net::Hello {
            protocol_version: COMPATIBILITY.protocol_version,
            engine_build_id: COMPATIBILITY.engine_build_id,
            schema_hash: COMPATIBILITY.schema_hash,
            generation: 1,
            session_token: Some(token),
        });

        assert_eq!(server.peer.session.state(), SessionState::Reconnecting);
        assert_eq!(server.peer.resume_token, token);
        assert_eq!(server.counters.processing_errors, 1);
    }

    #[test]
    fn rotating_session_clears_baselines_acks_and_the_session_key() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut server = server(World::new(), transport);
        let old_session_id = server.peer.session.session_id();
        let old_token = server.peer.resume_token;
        let tick = TickId::from_raw(1);
        retain_ack_baselines(&mut server, [1]);
        server.peer.session.handle_ack(SectorId::ZERO, tick);
        server.peer.adopt_session_key();

        server.rotate_session().expect("OS CSPRNG available");

        assert_ne!(server.peer.session.session_id(), old_session_id);
        assert_ne!(server.peer.resume_token, old_token);
        assert_eq!(server.peer.session.last_acked_tick(SectorId::ZERO), None);
        assert!(server.peer.session.baseline_store(SectorId::ZERO).is_none());
        assert!(server.peer.session_crypto.is_none());
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

    // ── Client input ───────────────────────────────────────────────────────

    /// **The bytes a client sealed reach the module.** The frame goes over the
    /// transport, through the session MAC, and out the other side with the tick
    /// the client stamped it with and the payload it carried — a server that
    /// decoded it only to drop it hands the module an empty view.
    #[test]
    fn a_sealed_input_frame_reaches_the_module_with_its_tick_and_bytes() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        let seen = record_inputs(&mut server);

        send_sealed(&mut peer, &mut crypto, &input(41, &[7, 8, 9]));
        assert_eq!(server.update(2 * TICK), 1);

        let seen = seen.lock().expect("test module is not poisoned");
        assert_eq!(
            seen.ticks,
            vec![vec![(TickId::from_raw(41), vec![7, 8, 9])]],
            "the module was handed {:?}",
            seen.ticks,
        );
        assert_eq!(seen.dropped, 0);
        assert_eq!(server.processing_error_count(), 0);
        assert_eq!(server.dropped_input_count(), 0);
    }

    /// **A frame is offered to one tick and then it is gone.** The queue is
    /// emptied at the start of every tick, so the tick after the one an input
    /// arrived in sees nothing — holding it until the tick it names is the
    /// jitter buffer this deliberately is not.
    #[test]
    fn an_input_frame_is_cleared_after_the_tick_that_was_offered_it() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        let seen = record_inputs(&mut server);

        send_sealed(&mut peer, &mut crypto, &input(41, &[7]));
        assert_eq!(server.update(2 * TICK), 1);
        assert_eq!(server.update(3 * TICK), 1);

        let seen = seen.lock().expect("test module is not poisoned");
        assert_eq!(seen.ticks.len(), 2, "two ticks must have run");
        assert_eq!(seen.ticks[0].len(), 1);
        assert!(
            seen.ticks[1].is_empty(),
            "the second tick was handed {:?} again",
            seen.ticks[1],
        );
    }

    /// **The cap is what a peer cannot spend past.** One tick's worth of
    /// frames past [`MAX_CLIENT_INPUTS_PER_TICK`] is refused: the module sees
    /// exactly the cap, in the order they were sent, and both the view and the
    /// server's counter say how many were dropped.
    #[test]
    fn input_past_the_per_tick_cap_is_refused_and_counted() {
        const EXCESS: usize = 3;

        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        let seen = record_inputs(&mut server);

        for i in 0..MAX_CLIENT_INPUTS_PER_TICK + EXCESS {
            send_sealed(&mut peer, &mut crypto, &input(i as u64, &[i as u8]));
        }
        assert_eq!(server.update(2 * TICK), 1);

        let seen = seen.lock().expect("test module is not poisoned");
        let frames = seen.ticks.first().expect("the module ran a tick");
        assert_eq!(
            frames.len(),
            MAX_CLIENT_INPUTS_PER_TICK,
            "the module was handed {} frames",
            frames.len(),
        );
        // The *first* frames sent, not the last: the cap refuses the newest.
        assert_eq!(frames[0].0, TickId::ZERO);
        assert_eq!(
            frames[MAX_CLIENT_INPUTS_PER_TICK - 1].0,
            TickId::from_raw(MAX_CLIENT_INPUTS_PER_TICK as u64 - 1),
        );
        assert_eq!(seen.dropped, EXCESS as u32, "the view must say it dropped");
        assert_eq!(server.dropped_input_count(), EXCESS as u64);
        // The refused frames are not errors: the peer sent well-formed input
        // and this server chose not to hold it.
        assert_eq!(server.processing_error_count(), 0);
    }

    /// A malformed input frame still costs the peer its error budget, and
    /// reaches no module.
    #[test]
    fn a_malformed_input_frame_counts_as_an_error_and_queues_nothing() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        let seen = record_inputs(&mut server);

        // The tag of an input, then a truncated body.
        send_sealed(&mut peer, &mut crypto, &[crcbl_net::codec::INPUT_TAG, 1, 2]);
        assert_eq!(server.update(2 * TICK), 1);

        assert_eq!(server.processing_error_count(), 1);
        let seen = seen.lock().expect("test module is not poisoned");
        assert_eq!(seen.ticks, vec![Vec::new()]);
    }

    /// A command is decoded and goes no further: it is not per-tick state, and
    /// queueing one as input would hand the module a frame no client meant as
    /// one.
    #[test]
    fn a_command_is_not_queued_as_input() {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world_with_one_entity(), transport);
        let mut crypto = connect(&mut server, &mut peer);
        let seen = record_inputs(&mut server);

        let command = crcbl_net::encode_client_to_server(&crcbl_net::ClientToServer::Command {
            data: vec![1, 2, 3],
        });
        send_sealed(&mut peer, &mut crypto, &command);
        assert_eq!(server.update(2 * TICK), 1);

        assert_eq!(server.processing_error_count(), 0);
        let seen = seen.lock().expect("test module is not poisoned");
        assert_eq!(seen.ticks, vec![Vec::new()]);
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
    fn a_snapshot_never_carries_an_entity_the_module_destroyed_on_that_tick() {
        // `World::despawn` only marks, and `World::tick` sweeps before
        // `GameModule::tick` runs — so the tick's snapshot used to serialise
        // the module's freshly destroyed entities out of storage that had not
        // been swept yet, and a client was told about entities the server no
        // longer had.
        let (world, entities) = world_with_replicated_entities(2);
        let (survivor, victim) = (entities[0], entities[1]);

        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world, transport);
        let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        server.set_module(Box::new(DespawnWhenArmed {
            victim,
            armed: std::sync::Arc::clone(&armed),
        }));
        let mut crypto = connect(&mut server, &mut peer);

        server.update(2 * TICK);
        drain_payloads(&mut peer);

        armed.store(true, std::sync::atomic::Ordering::Relaxed);
        server.update(3 * TICK);

        let payloads = drain_payloads(&mut peer);
        assert_eq!(payloads.len(), 1, "one tick ran, so one snapshot was sent");
        let delta = open_delta(&mut crypto, &payloads[0]);
        let described = entities_described(&delta);
        assert!(
            described.contains(&survivor.to_bits()),
            "the snapshot describes {described:?}, which does not include the \
             surviving entity {:#x} — the check below would pass vacuously",
            survivor.to_bits(),
        );
        assert!(
            !described.contains(&victim.to_bits()),
            "the snapshot for tick {:?} describes entity {:#x}, which the module \
             destroyed on that tick",
            delta.tick,
            victim.to_bits(),
        );
    }

    #[test]
    fn a_module_despawn_reaches_the_client_as_a_tombstone_on_the_same_tick() {
        // **Removals are replicated explicitly.** A snapshot delta-encoded
        // against a baseline the client acked carries the destroyed entity in
        // `SystemDelta::removed`; absence is the encoding only in a keyframe,
        // which has no baseline to tombstone against. This test acks every
        // snapshot so the server stays on the delta path, and asserts the
        // tombstone arrives on the tick the module despawned — not a tick
        // later, and never with the entity silently gone from a snapshot that
        // says nothing about it.
        let (world, entities) = world_with_replicated_entities(2);
        let victim = entities[1];

        let (transport, mut peer) = InMemoryTransport::pair();
        let mut server = server(world, transport);
        let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        server.set_module(Box::new(DespawnWhenArmed {
            victim,
            armed: std::sync::Arc::clone(&armed),
        }));
        let mut crypto = connect(&mut server, &mut peer);

        // Tick 2: the keyframe that puts both entities in the client's baseline.
        server.update(2 * TICK);
        let payloads = drain_payloads(&mut peer);
        assert_eq!(payloads.len(), 1);
        let keyframe = open_delta(&mut crypto, &payloads[0]);
        assert!(keyframe.is_keyframe);
        assert!(
            entities_described(&keyframe).contains(&victim.to_bits()),
            "the entity has to be replicated before its removal can be"
        );
        send_sealed(
            &mut peer,
            &mut crypto,
            &crcbl_net::encode_ack(SectorId::ZERO, keyframe.tick),
        );

        // Tick 3: nothing died, so the delta tombstones nothing.
        server.update(3 * TICK);
        let payloads = drain_payloads(&mut peer);
        assert_eq!(payloads.len(), 1);
        let quiet = open_delta(&mut crypto, &payloads[0]);
        assert!(
            !quiet.is_keyframe,
            "the acked baseline must put the server on the delta path"
        );
        assert!(
            entities_removed(&quiet).is_empty(),
            "tick {:?} removed {:?} with nothing despawned",
            quiet.tick,
            entities_removed(&quiet),
        );
        send_sealed(
            &mut peer,
            &mut crypto,
            &crcbl_net::encode_ack(SectorId::ZERO, quiet.tick),
        );

        // Tick 4: the module destroys the entity, and this tick's delta is what
        // tells the client so.
        armed.store(true, std::sync::atomic::Ordering::Relaxed);
        server.update(4 * TICK);
        let payloads = drain_payloads(&mut peer);
        assert_eq!(payloads.len(), 1);
        let despawn = open_delta(&mut crypto, &payloads[0]);
        assert!(!despawn.is_keyframe);
        assert_eq!(
            entities_removed(&despawn),
            vec![victim.to_bits()],
            "tick {:?} must tombstone the destroyed entity and nothing else",
            despawn.tick,
        );
        assert!(
            !entities_described(&despawn).contains(&victim.to_bits()),
            "the same packet must not also carry state for it"
        );

        // And it stays gone: the next tick says nothing more about it.
        send_sealed(
            &mut peer,
            &mut crypto,
            &crcbl_net::encode_ack(SectorId::ZERO, despawn.tick),
        );
        server.update(5 * TICK);
        let payloads = drain_payloads(&mut peer);
        assert_eq!(payloads.len(), 1);
        let after = open_delta(&mut crypto, &payloads[0]);
        assert!(entities_removed(&after).is_empty());
        assert!(!entities_described(&after).contains(&victim.to_bits()));
        assert_eq!(server.processing_error_count(), 0);
        assert_eq!(server.auth_failure_count(), 0);
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
        let store = server.peer.session.baseline_store(SectorId::ZERO).unwrap();
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

        assert_eq!(server.peer.session.last_acked_tick(SectorId::ZERO), None);
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
            server.peer.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(server.auth_failure_count(), 0);

        // A captured packet replayed verbatim carries a valid MAC; only the
        // replay counter rejects it.
        peer.send_unreliable(Message::unreliable(sealed)).unwrap();
        send_sealed(&mut peer, &mut crypto, &ack(2));
        server.update(3 * TICK);
        assert_eq!(
            server.peer.session.last_acked_tick(SectorId::ZERO),
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
            server.peer.session.last_acked_tick(SectorId::ZERO),
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
            server.peer.session.last_acked_tick(SectorId::ZERO),
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
            server.peer.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(server.rate_limited_message_count(), 1);

        // A second of injected time restores exactly one message of budget.
        send_sealed(&mut peer, &mut crypto, &ack(3));
        server.update(Duration::from_secs(1) + 2 * TICK);
        assert_eq!(
            server.peer.session.last_acked_tick(SectorId::ZERO),
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
            server_a.peer.session.last_acked_tick(SectorId::ZERO),
            Some(TickId::from_raw(1))
        );
        assert_eq!(
            server_b.peer.session.last_acked_tick(SectorId::ZERO),
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
