//! A multi-session host: one world, and a session for each of up to
//! [`HostConfig::max_peers`] clients.
//!
//! The host owns the sessions — admission, per-peer resume, and ending a
//! session in a way the client can tell from a dead link — and hands each
//! peer's input to the game by [`PeerId`]. Authority stays with the game: the
//! host decides who is in the session, never what their input means.
//!
//! Transports are `Box<dyn Transport>` because a listen host's are mixed by
//! nature: its own player arrives over an in-memory pair, the others over the
//! network. A transport the game [`add`](Host::add)s is pending until its
//! hello admits it as a new peer or re-attaches it to a peer whose link
//! dropped.

use std::fmt;
use std::time::Duration;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{ClientInputs, World};
use crcbl_net::rate_limit::{InboundRateLimitConfig, InboundRateLimiter};
use crcbl_net::{
    HandshakeGate, HandshakeResult, Hello, ProtocolCompatibility, RejectReason, ResumeToken,
    SectorId, SessionConfig, SessionEndReason, SessionId, SessionState, Transport, TransportError,
};

use crate::peer::{self, Counters, PeerSession};

/// How long a pending transport may go without a hello before it is dropped.
///
/// A client whose hello was refused (the host was full, say) says hello again
/// after its backoff, so the limit sits well beyond a client's longest
/// handshake backoff; a transport that never speaks is what it removes.
const PENDING_SILENCE_LIMIT: Duration = Duration::from_secs(20);

/// A host's settings.
#[derive(Debug, Clone, Copy)]
pub struct HostConfig {
    /// The most sessions the host holds at once, the host's own player
    /// included when they connect as a client. A peer whose link dropped keeps
    /// its place until it resumes or its grace period runs out.
    pub max_peers: usize,
    /// Simulation ticks per second.
    pub tick_hz: u32,
    /// What a client must match to be admitted. There is no default, for the
    /// reason given on [`ProtocolCompatibility::assert_explicit`].
    pub compatibility: ProtocolCompatibility,
}

/// One admitted session, stable from admission to its end and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(u64);

/// A change in the host's sessions, read with [`Host::events`].
///
/// Sessions the game ends itself — [`Host::kick`], [`Host::shutdown`] — raise
/// none: the game already knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerEvent {
    /// A new session was admitted.
    Joined(PeerId),
    /// The peer's link dropped. Its session waits, holding its place, for the
    /// grace period in [`SessionConfig::reconnect_grace_period`].
    Lost(PeerId),
    /// A lost peer came back within its grace period, to the same session.
    Resumed(PeerId),
    /// A lost peer's grace period ran out and its session is gone. If it
    /// comes back, it joins as a new peer.
    Left(PeerId),
}

/// Per-tick game logic for a [`Host`].
pub trait HostModule: Send {
    /// Called every host tick after the ECS schedule has run, with the input
    /// each peer sent since the previous tick. Whatever the module despawns
    /// is swept before the tick's snapshot is taken.
    fn tick(&mut self, world: &mut World, inputs: PeerInputs<'_>);
}

/// Every peer's input for one tick, as handed to [`HostModule::tick`].
#[derive(Clone, Copy)]
pub struct PeerInputs<'a> {
    peers: &'a [Peer],
}

impl<'a> PeerInputs<'a> {
    /// Each admitted peer with the frames it sent this tick, in admission
    /// order. A lost peer is listed, with nothing.
    pub fn iter(self) -> impl Iterator<Item = (PeerId, ClientInputs<'a>)> {
        self.peers.iter().map(|peer| {
            (
                peer.id,
                ClientInputs::new(&peer.link.client_inputs, peer.link.dropped_inputs),
            )
        })
    }
}

impl fmt::Debug for PeerInputs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerInputs")
            .field("peers", &self.peers.len())
            .finish()
    }
}

/// An admitted session and, while it is connected, its transport.
struct Peer {
    id: PeerId,
    /// `None` from the moment the link drops until the peer resumes.
    transport: Option<Box<dyn Transport>>,
    link: PeerSession,
    /// Whether the session was connected when the current tick began; only
    /// such a session is sent the tick's snapshot, as in `Server`.
    was_connected: bool,
}

impl Peer {
    fn is_connected(&self) -> bool {
        self.transport.is_some() && self.link.session.state() == SessionState::Connected
    }
}

/// A transport that has not been admitted yet.
struct Pending {
    transport: Box<dyn Transport>,
    /// Everything a pending transport sends is charged here: before
    /// admission there is only the hello to read, on whichever channel.
    limiter: InboundRateLimiter,
    /// When it was added or last said hello.
    last_heard: Duration,
}

/// The multi-session authoritative host. See the [module docs](self).
pub struct Host {
    world: World,
    clock: FrameClock,
    max_peers: usize,
    session_config: SessionConfig,
    handshake_gate: HandshakeGate,
    rate_limit_config: InboundRateLimitConfig,
    now: Duration,
    counters: Counters,
    peers: Vec<Peer>,
    pending: Vec<Pending>,
    next_peer_id: u64,
    next_session_id: u64,
    events: Vec<PeerEvent>,
    module: Option<Box<dyn HostModule>>,
}

impl Host {
    /// A host with no peers.
    ///
    /// The world's fixed timestep is set from `config.tick_hz`.
    ///
    /// # Panics
    ///
    /// Panics if `config.max_peers` or `config.tick_hz` is zero, or if either
    /// compatibility identifier is zero.
    #[must_use]
    pub fn new(mut world: World, config: HostConfig) -> Self {
        config.compatibility.assert_explicit();
        assert!(config.max_peers > 0, "a host must admit at least one peer");
        let clock = FrameClock::new(config.tick_hz);
        world.set_tick_dt(clock.tick_dt_secs());
        Self {
            world,
            clock,
            max_peers: config.max_peers,
            session_config: SessionConfig::default(),
            handshake_gate: HandshakeGate::new(config.compatibility),
            rate_limit_config: InboundRateLimitConfig::default(),
            now: Duration::ZERO,
            counters: Counters::default(),
            peers: Vec::new(),
            pending: Vec::new(),
            next_peer_id: 1,
            next_session_id: 1,
            events: Vec::new(),
            module: None,
        }
    }

    /// Hand the host a newly connected transport. Its hello decides whether
    /// it becomes a new peer, resumes a lost one, or is refused.
    pub fn add(&mut self, transport: Box<dyn Transport>) {
        self.pending.push(Pending {
            transport,
            limiter: InboundRateLimiter::new(self.rate_limit_config, self.now),
            last_heard: self.now,
        });
    }

    /// Feed the current time. Returns how many ticks ran.
    pub fn update(&mut self, now: Duration) -> u32 {
        self.now = now;
        self.clock.update(now);
        let mut ticks = 0u32;
        while self.clock.consume_tick() {
            self.tick();
            ticks += 1;
        }
        ticks
    }

    fn tick(&mut self) {
        for peer in &mut self.peers {
            peer.was_connected = peer.is_connected();
            peer.link.begin_tick();
        }
        self.drain_peers();
        self.drain_pending();
        self.update_sessions();
        self.world.tick();
        if let Some(module) = self.module.as_mut() {
            module.tick(&mut self.world, PeerInputs { peers: &self.peers });
        }
        // As in `Server::tick`: the module's despawns must be swept before
        // the snapshot, or it replicates entities that are already gone.
        self.world.sweep();
        self.emit_snapshots();
    }

    /// Read every connected peer's transport, reliable channel first, each
    /// channel under the peer's own budget.
    fn drain_peers(&mut self) {
        let tick = self.clock.tick();
        for peer in &mut self.peers {
            let Some(transport) = peer.transport.as_mut() else {
                continue;
            };
            for reliable in [true, false] {
                loop {
                    let received = if reliable {
                        transport.recv_reliable()
                    } else {
                        transport.recv()
                    };
                    match received {
                        Ok(Some(msg)) => {
                            if !peer.link.charge_inbound_budget(
                                reliable,
                                msg.payload.len(),
                                self.now,
                                &mut self.counters,
                            ) {
                                break;
                            }
                            match msg.payload.first().copied() {
                                Some(crcbl_net::codec::HELLO_TAG) => {
                                    match crcbl_net::decode_hello(&msg.payload) {
                                        Ok(hello) => {
                                            let result = rehello(
                                                &self.handshake_gate,
                                                &peer.link,
                                                &hello,
                                                tick,
                                            );
                                            peer::send_handshake_result(
                                                transport.as_mut(),
                                                &result,
                                                &mut self.counters,
                                            );
                                        }
                                        Err(_) => self.counters.processing_errors += 1,
                                    }
                                }
                                Some(crcbl_net::auth::AUTH_TAG) => {
                                    peer.link.process_authenticated_message(
                                        &msg.payload,
                                        &mut self.counters,
                                    )
                                }
                                _ => self.counters.processing_errors += 1,
                            }
                        }
                        Ok(None) | Err(TransportError::Disconnected) => break,
                        Err(_) => {
                            self.counters.processing_errors += 1;
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Read every pending transport up to its first hello, and act on it.
    fn drain_pending(&mut self) {
        let mut index = 0;
        while index < self.pending.len() {
            let Some(hello) = self.next_hello(index) else {
                index += 1;
                continue;
            };
            let pending = self.pending.remove(index);
            if let Some(pending) = self.admit(pending, &hello) {
                self.pending.insert(index, pending);
                index += 1;
            }
        }
    }

    /// The next hello on pending transport `index`, discarding (and counting)
    /// anything else it sent.
    fn next_hello(&mut self, index: usize) -> Option<Hello> {
        let pending = &mut self.pending[index];
        loop {
            let msg = match pending.transport.recv() {
                Ok(Some(msg)) => msg,
                Ok(None) | Err(TransportError::Disconnected) => return None,
                Err(_) => {
                    self.counters.processing_errors += 1;
                    return None;
                }
            };
            if !peer::charge(
                &mut pending.limiter,
                msg.payload.len(),
                self.now,
                &mut self.counters,
            ) {
                return None;
            }
            match msg.payload.first().copied() {
                Some(crcbl_net::codec::HELLO_TAG) => match crcbl_net::decode_hello(&msg.payload) {
                    Ok(hello) => {
                        pending.last_heard = self.now;
                        return Some(hello);
                    }
                    Err(_) => self.counters.processing_errors += 1,
                },
                // Sealed, but there is no session to hold the key yet.
                Some(crcbl_net::auth::AUTH_TAG) => self.counters.auth_failures += 1,
                _ => self.counters.processing_errors += 1,
            }
        }
    }

    /// Answer a pending transport's hello. Returns the transport when it
    /// stays pending — refused, but free to say hello again.
    fn admit(&mut self, mut pending: Pending, hello: &Hello) -> Option<Pending> {
        let tick = self.clock.tick();
        // The gate checks only compatibility; the credential it would echo
        // into an `Accept` is filled in below, so this placeholder is never
        // sent. Checked first, so an incompatible client learns that — which
        // is permanent — rather than a transient "full".
        let checked = self.handshake_gate.validate(
            hello,
            SessionId(0),
            ResumeToken::from_bytes([0; 32]),
            tick,
        );
        if matches!(checked, HandshakeResult::Reject { .. }) {
            self.reply(&mut pending, &checked);
            return Some(pending);
        }
        let resume_token = match peer::generate_resume_token() {
            Ok(token) => token,
            Err(error) => {
                self.counters.processing_errors += 1;
                self.reply(
                    &mut pending,
                    &peer::entropy_failure(hello.generation, error),
                );
                return Some(pending);
            }
        };
        match hello.session_token {
            None => self.admit_new(pending, hello, resume_token),
            Some(token) => self.resume(pending, hello, token, resume_token),
        }
    }

    fn admit_new(
        &mut self,
        mut pending: Pending,
        hello: &Hello,
        resume_token: ResumeToken,
    ) -> Option<Pending> {
        // A lost peer still holds its place: counting only the connected
        // ones would let a newcomer take it, and the lost peer's resume would
        // then make one more than the host allows.
        if self.peers.len() >= self.max_peers {
            let full = HandshakeResult::Reject {
                generation: hello.generation,
                reason: RejectReason {
                    code: RejectReason::SERVER_FULL,
                    msg: format!("the host holds its {} sessions", self.max_peers),
                },
            };
            self.reply(&mut pending, &full);
            return Some(pending);
        }
        let session_id = SessionId(self.next_session_id);
        let accept = HandshakeResult::Accept {
            generation: hello.generation,
            session_id,
            resume_token,
            server_tick: self.clock.tick(),
        };
        if !self.reply(&mut pending, &accept) {
            // A transport that cannot carry the accept cannot carry the
            // session either.
            return None;
        }
        self.next_session_id = self.next_session_id.wrapping_add(1);
        let mut link = PeerSession::new(
            session_id,
            &self.session_config,
            resume_token,
            self.rate_limit_config,
            self.now,
        );
        link.session.begin_handshake();
        link.session
            .on_connected(hello.engine_build_id, hello.schema_hash);
        link.adopt_session_key();
        let id = PeerId(self.next_peer_id);
        self.next_peer_id = self.next_peer_id.wrapping_add(1);
        self.peers.push(Peer {
            id,
            transport: Some(pending.transport),
            link,
            was_connected: false,
        });
        self.events.push(PeerEvent::Joined(id));
        None
    }

    /// Re-attach a pending transport to the lost session its token names.
    fn resume(
        &mut self,
        mut pending: Pending,
        hello: &Hello,
        token: ResumeToken,
        rotated: ResumeToken,
    ) -> Option<Pending> {
        let Some(index) = self
            .peers
            .iter()
            .position(|peer| peer.link.resume_token == token)
        else {
            let reject = peer::invalid_session_token(hello.generation, "unknown session token");
            self.reply(&mut pending, &reject);
            return Some(pending);
        };
        let peer = &mut self.peers[index];
        if peer.link.session.state() != SessionState::Reconnecting {
            // Its owner is still connected: this is someone else holding the
            // token, or the owner on a second link. Either way the session
            // stays where it is.
            let reject = peer::invalid_session_token(
                hello.generation,
                "the session is connected on another link",
            );
            self.reply(&mut pending, &reject);
            return Some(pending);
        }
        if !peer
            .link
            .session
            .can_reconnect(self.now, hello.engine_build_id, hello.schema_hash)
        {
            peer.link.session.expire_if_timed_out(self.now);
            let reject =
                peer::invalid_session_token(hello.generation, "reconnect grace period expired");
            self.reply(&mut pending, &reject);
            self.remove_ended_sessions();
            return Some(pending);
        }
        let accept = HandshakeResult::Accept {
            generation: hello.generation,
            session_id: peer.link.session.session_id(),
            resume_token: rotated,
            server_tick: self.clock.tick(),
        };
        if !peer::send_handshake_result(pending.transport.as_mut(), &accept, &mut self.counters) {
            return None;
        }
        if peer
            .link
            .session
            .try_reconnect(self.now, hello.engine_build_id, hello.schema_hash)
        {
            // Rotating the token rotates the MAC key, which restarts the
            // replay counter space for the resumed session.
            peer.link.resume_token = rotated;
            peer.link.adopt_session_key();
            peer.transport = Some(pending.transport);
            let id = peer.id;
            self.events.push(PeerEvent::Resumed(id));
        }
        None
    }

    fn reply(&mut self, pending: &mut Pending, result: &HandshakeResult) -> bool {
        peer::send_handshake_result(pending.transport.as_mut(), result, &mut self.counters)
    }

    /// Notice dropped links and expired grace periods, and drop pending
    /// transports that closed or went silent.
    fn update_sessions(&mut self) {
        for peer in &mut self.peers {
            if peer
                .transport
                .as_ref()
                .is_some_and(|transport| !transport.is_connected())
            {
                peer.transport = None;
                if peer.link.session.state() == SessionState::Connected {
                    peer.link
                        .session
                        .on_disconnect(self.now, &self.session_config);
                    self.events.push(PeerEvent::Lost(peer.id));
                }
            }
            peer.link.session.expire_if_timed_out(self.now);
        }
        self.remove_ended_sessions();
        let now = self.now;
        self.pending.retain(|pending| {
            pending.transport.is_connected()
                && now.saturating_sub(pending.last_heard) < PENDING_SILENCE_LIMIT
        });
    }

    /// Remove every session whose grace period ran out, raising
    /// [`PeerEvent::Left`] for each.
    fn remove_ended_sessions(&mut self) {
        let events = &mut self.events;
        self.peers.retain(|peer| {
            let ended = peer.link.session.state() == SessionState::Disconnected;
            if ended {
                events.push(PeerEvent::Left(peer.id));
            }
            !ended
        });
    }

    /// Serialise the world once, then delta-encode and seal it for each
    /// peer that was connected all tick.
    fn emit_snapshots(&mut self) {
        if !self
            .peers
            .iter()
            .any(|peer| peer.was_connected && peer.is_connected())
        {
            return;
        }
        let tick = self.clock.tick();
        let sector = SectorId::ZERO;
        let Some(current) = peer::current_baseline(&self.world, sector, tick, &mut self.counters)
        else {
            return;
        };
        for peer in &mut self.peers {
            if !(peer.was_connected && peer.is_connected()) {
                continue;
            }
            let Some(transport) = peer.transport.as_mut() else {
                continue;
            };
            peer.link.send_snapshot(
                transport.as_mut(),
                sector,
                current.clone(),
                &mut self.counters,
            );
        }
    }

    /// End one peer's session: it is told it was kicked, then its transport
    /// is closed. A lost peer is simply forgotten. Returns whether `peer` was
    /// a session of this host.
    pub fn kick(&mut self, peer: PeerId) -> bool {
        let Some(index) = self.peers.iter().position(|p| p.id == peer) else {
            return false;
        };
        let mut peer = self.peers.remove(index);
        Self::end(&mut peer, SessionEndReason::KICKED, &mut self.counters);
        true
    }

    /// End every session: each connected peer is told `reason` —
    /// [`SessionEndReason::HOST_LEFT`] when the host player quits,
    /// [`SessionEndReason::SHUTTING_DOWN`] for a server going away — and then
    /// its transport is closed. Pending transports are closed too. The host
    /// itself carries on, empty, and admits whoever connects next.
    pub fn shutdown(&mut self, reason: SessionEndReason) {
        for mut peer in self.peers.drain(..) {
            Self::end(&mut peer, reason, &mut self.counters);
        }
        self.pending.clear();
    }

    /// Send `reason` ahead of the close that dropping `peer` performs.
    fn end(peer: &mut Peer, reason: SessionEndReason, counters: &mut Counters) {
        if let Some(transport) = peer.transport.as_mut() {
            peer.link
                .send_session_end(transport.as_mut(), reason, counters);
        }
    }

    /// Take the session changes since the last call, oldest first.
    pub fn events(&mut self) -> impl Iterator<Item = PeerEvent> + '_ {
        self.events.drain(..)
    }

    /// Every admitted peer, lost ones included, in admission order.
    pub fn peers(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers.iter().map(|peer| peer.id)
    }

    /// How many sessions the host holds, lost ones included.
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// How many transports are waiting for their hello to be accepted.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// `peer`'s session state, or `None` once its session has ended.
    #[must_use]
    pub fn peer_state(&self, peer: PeerId) -> Option<SessionState> {
        self.peers
            .iter()
            .find(|p| p.id == peer)
            .map(|p| p.link.session.state())
    }

    /// Reconnect grace configuration. Applies to links that drop from now on.
    pub fn set_session_config(&mut self, session_config: SessionConfig) {
        self.session_config = session_config;
    }

    /// Configure the inbound traffic limits, per peer and per channel, and
    /// for each pending transport. Every bucket resets to one second of the
    /// new budget.
    pub fn set_inbound_rate_limit_config(&mut self, config: InboundRateLimitConfig) {
        self.rate_limit_config = config;
        for peer in &mut self.peers {
            peer.link.reconfigure_rate_limit(config, self.now);
        }
        for pending in &mut self.pending {
            pending.limiter.reconfigure(config, self.now);
        }
    }

    /// Attach the game logic that runs every tick; replaces any earlier one.
    pub fn set_module(&mut self, module: Box<dyn HostModule>) {
        self.module = Some(module);
    }

    /// Messages dropped because a message-rate budget was exhausted.
    #[must_use]
    pub fn rate_limited_message_count(&self) -> u64 {
        self.counters.rate_limited_messages
    }

    /// Messages dropped because a byte-rate budget was exhausted.
    #[must_use]
    pub fn rate_limited_byte_count(&self) -> u64 {
        self.counters.rate_limited_bytes
    }

    /// Input frames refused because a peer's tick already held
    /// [`MAX_CLIENT_INPUTS_PER_TICK`](crate::MAX_CLIENT_INPUTS_PER_TICK).
    #[must_use]
    pub fn dropped_input_count(&self) -> u64 {
        self.counters.dropped_inputs
    }

    /// Messages rejected because they were unauthenticated, carried a bad
    /// MAC, or replayed a counter.
    #[must_use]
    pub fn auth_failure_count(&self) -> u64 {
        self.counters.auth_failures
    }

    /// Unrecoverable transport, encoding, decoding or lifecycle errors.
    #[must_use]
    pub fn processing_error_count(&self) -> u64 {
        self.counters.processing_errors
    }

    /// Borrow the world.
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

/// The answer to a hello on a connected peer's own link: the same session
/// again for its own token, as `Server` answers one, and a refusal otherwise.
fn rehello(
    gate: &HandshakeGate,
    link: &PeerSession,
    hello: &Hello,
    tick: TickId,
) -> HandshakeResult {
    let result = gate.validate(hello, link.session.session_id(), link.resume_token, tick);
    if matches!(result, HandshakeResult::Accept { .. })
        && !hello
            .session_token
            .is_some_and(|token| token == link.resume_token)
    {
        return peer::invalid_session_token(hello.generation, "session token does not match");
    }
    result
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("world", &self.world)
            .field("clock", &self.clock)
            .field("max_peers", &self.max_peers)
            .field("peers", &self.peers.len())
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
