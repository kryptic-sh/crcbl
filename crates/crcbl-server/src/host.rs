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
//!
//! A `Flags::SIM` console set — from the host's own console or from its own
//! player — applies at the start of the next tick and is recorded for replay;
//! `sim`'s module docs say who may make one and in what order they apply.
//!
//! A scene edit (`crcbl_net::edit`) is the caller's to apply, not the host's:
//! a host told to [`serve_edits`](Host::serve_edits) hands each one over with
//! the peer that sent it ([`Host::take_edit_requests`]), and sends the answer
//! and the notice of what applied for it ([`Host::send_edit_reply`],
//! [`Host::broadcast_edit_notice`]). **Any admitted peer may send one**: who
//! is in the session is the host's, and what an edit means is the caller's,
//! as with input. A host serving no scene refuses every edit as not editable,
//! so a game's host answers one rather than leaving it unread.
//!
//! A scene fetch is the caller's to answer too ([`Host::take_scene_fetches`]),
//! with the scene's files it writes ([`Host::send_scene`]) or a refusal
//! ([`Host::refuse_scene_fetch`]); the host paces the parts out and holds one
//! fetch a peer in flight, refusing another as busy — `scene_stream`'s module
//! docs have why.
//!
//! What the module is handed of its peers — the roster and each peer's frames
//! — goes through one step whether it came off the transports or out of a
//! recording, and is recorded on request ([`Host::record_peer_inputs`]);
//! `record`'s module docs have why.

use std::fmt;
use std::time::Duration;

use crcbl_console::{Registry, SimVars};
use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::{ClientInputs, World};
use crcbl_net::rate_limit::{InboundRateLimitConfig, InboundRateLimiter};
use crcbl_net::reliable::EndpointStats;
use crcbl_net::{
    ConsoleReply, ConsoleSet, EditNotice, EditRefusal, EditReply, EditRequest, EditTooLong,
    HandshakeGate, HandshakeResult, Hello, ProtocolCompatibility, RejectReason, ResumeToken,
    SceneOutcome, SceneReply, SectorId, SessionConfig, SessionEndReason, SessionId, SessionState,
    Transport, TransportError,
};

use crate::peer::{
    self, Counters, PartSent, PeerSession, PeerStats, SnapshotTooLarge, UpdateTooLarge,
};

mod record;
mod resim;
mod scene_stream;
mod sim;

use record::PeerLog;
pub use record::{PeerFrames, RosterChange, TickInputs};
pub use resim::{FramesFault, ResimError, RosterFault};
pub use scene_stream::SCENE_FETCH_BYTES_PER_SECOND;
use scene_stream::SceneStream;
pub use sim::AppliedSimSet;
use sim::{Origin, SimConsole};

/// How long a pending transport may go without a hello before it is dropped.
///
/// A client whose hello was refused (the host was full, say) says hello again
/// after its backoff, so the limit sits well beyond a client's longest
/// handshake backoff; a transport that never speaks is what it removes.
const PENDING_SILENCE_LIMIT: Duration = Duration::from_secs(20);

/// How long an admitted peer may hold its place without one message that
/// opens under its session key.
///
/// A client acknowledges the snapshots the host sends it every tick, so one
/// that holds the key proves it within a round trip; the limit sits far beyond
/// that. What it removes is a session its client never took up — one whose
/// `Accept` the client dropped, or one it abandoned — which would otherwise
/// keep one of [`HostConfig::max_peers`] places for as long as its link stays
/// up.
const AUTHENTICATION_DEADLINE: Duration = Duration::from_secs(10);

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

impl PeerId {
    /// The peer a host numbered `raw` — how a recording names it.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The number the host gave this peer.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

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
    /// A connected peer's client said hello again on its own link and was
    /// accepted again, to the same session. A client does that when no
    /// answer to its hello came within its handshake timeout, and then drops
    /// the `Accept` that does come as an old one — so the session's key
    /// started over with this `Accept`, and nothing sealed to the peer
    /// before it opens on the client's side. What the game sent the peer on
    /// [`Joined`](Self::Joined) is lost, and this is when to send it again;
    /// a client that did read it the first time may be sent it twice.
    Reaccepted(PeerId),
    /// The peer's session is gone: a lost peer's grace period ran out, or a
    /// connected peer sent nothing under its session key for a while after
    /// it was admitted or resumed (its client never took the session up). If
    /// it comes back, it joins as a new peer.
    Left(PeerId),
}

/// Why [`Host::send_event`], [`Host::send_edit_reply`],
/// [`Host::send_scene`] or [`Host::refuse_scene_fetch`] sent nothing.
#[derive(Debug)]
pub enum EventNotSent {
    /// No session of this host has that id: it never had one, or it ended.
    NoSuchPeer(PeerId),
    /// The peer's link is down — it is waiting out its grace period — so
    /// there is nothing to send on.
    NotConnected(PeerId),
    /// The event is longer than a client reads one:
    /// [`MAX_FIELD_BYTES`](crcbl_net::codec::MAX_FIELD_BYTES), the wire's
    /// limit on one opaque field. For an edit reply or a scene refusal, its
    /// message is past
    /// [`MAX_EDIT_MESSAGE_BYTES`](crcbl_net::MAX_EDIT_MESSAGE_BYTES); for a
    /// scene, its files are empty or past
    /// [`MAX_SCENE_BYTES`](crcbl_net::MAX_SCENE_BYTES).
    TooLarge {
        /// The event's length, or the message's, in bytes.
        size: usize,
        /// The most a client accepts, in bytes.
        limit: usize,
    },
    /// The session's key has sealed all it may.
    Seal(crcbl_net::AuthError),
    /// The transport refused the message.
    Transport(TransportError),
}

impl fmt::Display for EventNotSent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchPeer(peer) => write!(f, "{peer:?} is no session of this host"),
            Self::NotConnected(peer) => write!(f, "{peer:?}'s link is down"),
            Self::TooLarge { size, limit } => write!(
                f,
                "the message is {size} bytes, past the {limit} a client reads"
            ),
            Self::Seal(error) => write!(f, "the event could not be sealed: {error}"),
            Self::Transport(error) => write!(f, "the transport refused the event: {error}"),
        }
    }
}

impl std::error::Error for EventNotSent {}

/// Per-tick game logic for a [`Host`].
pub trait HostModule: Send {
    /// Called every host tick after the ECS schedule has run, with the input
    /// each peer sent since the previous tick and the simulation variables
    /// as the tick's boundary left them. Whatever the module despawns is
    /// swept before the tick's snapshot is taken.
    fn tick(&mut self, world: &mut World, inputs: PeerInputs<'_>);
}

/// Every peer's input for one tick, as handed to [`HostModule::tick`].
#[derive(Clone, Copy)]
pub struct PeerInputs<'a> {
    peers: &'a [PeerFrames],
    sim_vars: &'a SimVars,
}

impl<'a> PeerInputs<'a> {
    /// Each admitted peer with the frames it sent this tick, in admission
    /// order. A lost peer is listed, with nothing.
    pub fn iter(self) -> impl Iterator<Item = (PeerId, ClientInputs<'a>)> {
        self.peers
            .iter()
            .map(|peer| (peer.peer, ClientInputs::new(&peer.frames, peer.dropped)))
    }

    /// The host's simulation variables, with every set this tick's boundary
    /// applied ([`Host::set_sim_registry`]) — a tick input like the frames
    /// above, which is why it comes with them. Empty for a host given no
    /// registry.
    #[must_use]
    pub const fn sim_vars(self) -> &'a SimVars {
        self.sim_vars
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
    /// Whether the session is sent the current tick's snapshot: connected
    /// all tick, and due one by its own cadence.
    snapshot_due: bool,
    /// When the peer's current session key was adopted, which starts the
    /// [`AUTHENTICATION_DEADLINE`].
    keyed_at: Duration,
    /// Whether this is the host's own player, whose console sets the host
    /// takes ([`Host::add_host_player`]).
    host_player: bool,
    /// Whether one of this peer's scene fetches was handed to the caller
    /// ([`Host::take_scene_fetches`]) and not answered yet.
    fetch_waiting: bool,
    /// The scene being sent to this peer, part by part; dropped with the
    /// link, since the parts sent on it went with it.
    scene_stream: Option<SceneStream>,
}

impl Peer {
    fn is_connected(&self) -> bool {
        self.transport.is_some() && self.link.session.state() == SessionState::Connected
    }

    /// Whether a fetch of this peer's is waiting to be answered or being
    /// sent: the one a peer may have in flight.
    const fn fetch_in_flight(&self) -> bool {
        self.fetch_waiting || self.scene_stream.is_some()
    }

    /// Send every part of this peer's scene stream that is due at `now`, and
    /// drop the stream once its last part went or a part could not go.
    fn pump_scene_stream(&mut self, now: Duration, counters: &mut Counters) {
        let (Some(stream), Some(transport)) = (self.scene_stream.as_mut(), self.transport.as_mut())
        else {
            return;
        };
        while let Some(part) = stream.due(now) {
            let len = part.bytes().len();
            let reply = SceneReply {
                fetch_id: stream.fetch_id(),
                outcome: SceneOutcome::Part(part),
            };
            let payload = crcbl_net::encode_scene_reply(&reply)
                .expect("a part has no message to be too long");
            match self
                .link
                .send_scene_part(transport.as_mut(), &payload, counters)
            {
                PartSent::Sent => stream.sent(len, now),
                PartSent::Later => return,
                PartSent::Failed => {
                    self.scene_stream = None;
                    return;
                }
            }
        }
        if stream.is_done() {
            self.scene_stream = None;
        }
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
    /// Whether it was added as the host's own player.
    host_player: bool,
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
    events: PeerLog,
    /// The frames the current tick hands the module, one entry a peer; kept
    /// between ticks so its buffers are reused rather than reallocated.
    tick_inputs: Vec<PeerFrames>,
    module: Option<Box<dyn HostModule>>,
    sim: SimConsole,
    /// Whether peers' edits are held for the caller ([`Host::serve_edits`])
    /// rather than refused.
    serving_edits: bool,
    /// The edits held for [`Host::take_edit_requests`], in the order read.
    edit_requests: Vec<(PeerId, EditRequest)>,
    /// The scene fetches held for [`Host::take_scene_fetches`], in the order
    /// read.
    scene_fetches: Vec<(PeerId, u64)>,
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
            events: PeerLog::default(),
            tick_inputs: Vec::new(),
            module: None,
            sim: SimConsole::default(),
            serving_edits: false,
            edit_requests: Vec::new(),
            scene_fetches: Vec::new(),
        }
    }

    /// Hand the host a newly connected transport. Its hello decides whether
    /// it becomes a new peer, resumes a lost one, or is refused.
    pub fn add(&mut self, transport: Box<dyn Transport>) {
        self.add_pending(transport, false);
    }

    /// Hand the host its own player's transport — a listen host's in-memory
    /// pair. Admitted like any other, and the one peer whose console sets of
    /// simulation variables the host applies; every other peer's are refused.
    /// A dedicated server adds none, and takes sets from its console alone.
    pub fn add_host_player(&mut self, transport: Box<dyn Transport>) {
        self.add_pending(transport, true);
    }

    fn add_pending(&mut self, transport: Box<dyn Transport>, host_player: bool) {
        self.pending.push(Pending {
            transport,
            limiter: InboundRateLimiter::new(self.rate_limit_config, self.now),
            last_heard: self.now,
            host_player,
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
        for peer in &mut self.peers {
            peer.pump_scene_stream(now, &mut self.counters);
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
        let mut inputs = std::mem::take(&mut self.tick_inputs);
        take_queued_inputs(&mut self.peers, &mut inputs);
        let roster = self.events.take_roster();
        self.step(roster, &inputs);
        self.tick_inputs = inputs;
    }

    /// The rest of a tick, once its peers' input is in hand: the tick
    /// boundary for simulation variables, the schedule, the module — handed
    /// `peers`, every admitted peer in admission order — and the snapshots.
    /// The live tick and a re-simulation both run it, so a replayed tick
    /// reaches the module exactly as a live one does; it is also where the
    /// input record is kept, so what it records is what the module read.
    /// `roster` is the roster's changes since the tick before, for the
    /// record.
    fn step(&mut self, roster: Vec<RosterChange>, peers: &[PeerFrames]) {
        self.apply_sim_sets();
        self.world.tick();
        if let Some(module) = self.module.as_mut() {
            module.tick(
                &mut self.world,
                PeerInputs {
                    peers,
                    sim_vars: self.sim.vars(),
                },
            );
        }
        self.events.record_tick(self.clock.tick(), roster, peers);
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
                                            // The session's key starts over
                                            // with the client's, which adopts
                                            // it afresh on every Accept.
                                            let accepted =
                                                matches!(result, HandshakeResult::Accept { .. });
                                            if accepted {
                                                peer.link.adopt_session_key();
                                                peer.keyed_at = self.now;
                                            }
                                            let sent = peer::send_handshake_result(
                                                transport.as_mut(),
                                                &result,
                                                &mut self.counters,
                                            );
                                            if accepted && sent {
                                                self.events.push(PeerEvent::Reaccepted(peer.id));
                                            }
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
            for set in peer.link.console_sets.drain(..) {
                self.sim.submit(Origin::Peer(peer.id), set);
            }
            for request in std::mem::take(&mut peer.link.edit_requests) {
                if self.serving_edits {
                    self.edit_requests.push((peer.id, request));
                } else if let Some(transport) = peer.transport.as_mut() {
                    let reply = peer::not_serving_edits(&request);
                    peer.link
                        .send_edit_reply(transport.as_mut(), &reply, &mut self.counters);
                }
            }
            for fetch_id in std::mem::take(&mut peer.link.scene_fetches) {
                let refusal = if !self.serving_edits {
                    peer::not_serving_scene(fetch_id)
                } else if peer.fetch_in_flight() {
                    busy(fetch_id)
                } else {
                    peer.fetch_waiting = true;
                    self.scene_fetches.push((peer.id, fetch_id));
                    continue;
                };
                if let Some(transport) = peer.transport.as_mut() {
                    peer.link
                        .send_scene_refusal(transport.as_mut(), &refusal, &mut self.counters);
                }
            }
        }
    }

    /// The tick boundary for simulation variables: every set read since the
    /// last one applies, or is refused, before the world ticks — and each
    /// peer that made one is told which.
    fn apply_sim_sets(&mut self) {
        let tick = self.clock.tick();
        let peers = &self.peers;
        let replies = self.sim.begin_tick(tick, |id| is_host_player(peers, id));
        for (id, reply) in replies {
            let Some(peer) = self.peers.iter_mut().find(|peer| peer.id == id) else {
                // Gone since it asked: there is no one to tell.
                continue;
            };
            if !peer.is_connected() {
                continue;
            }
            if let Some(transport) = peer.transport.as_mut() {
                peer.link
                    .send_console_reply(transport.as_mut(), &reply, &mut self.counters);
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
            snapshot_due: false,
            keyed_at: self.now,
            host_player: pending.host_player,
            fetch_waiting: false,
            scene_stream: None,
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
            peer.keyed_at = self.now;
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
                peer.scene_stream = None;
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
        self.end_unauthenticated_sessions();
        let now = self.now;
        self.pending.retain(|pending| {
            pending.transport.is_connected()
                && now.saturating_sub(pending.last_heard) < PENDING_SILENCE_LIMIT
        });
    }

    /// End every connected session whose client has not proved it holds the
    /// session key within [`AUTHENTICATION_DEADLINE`], raising
    /// [`PeerEvent::Left`] for each. The client is told it was kicked, though
    /// one that never took the session up cannot read it.
    fn end_unauthenticated_sessions(&mut self) {
        let now = self.now;
        let mut index = 0;
        while index < self.peers.len() {
            let peer = &self.peers[index];
            if peer.is_connected()
                && !peer.link.authenticated
                && now.saturating_sub(peer.keyed_at) >= AUTHENTICATION_DEADLINE
            {
                let mut peer = self.peers.remove(index);
                Self::end(&mut peer, SessionEndReason::KICKED, &mut self.counters);
                self.events.push(PeerEvent::Left(peer.id));
            } else {
                index += 1;
            }
        }
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
    /// peer that was connected all tick and is due a snapshot by its own
    /// cadence ([`crate::cadence`]).
    fn emit_snapshots(&mut self) {
        let mut any_due = false;
        for peer in &mut self.peers {
            peer.snapshot_due =
                peer.was_connected && peer.is_connected() && peer.link.snapshot_due();
            any_due |= peer.snapshot_due;
        }
        if !any_due {
            return;
        }
        let tick = self.clock.tick();
        let sector = SectorId::ZERO;
        let Some(current) = peer::current_baseline(&self.world, sector, tick, &mut self.counters)
        else {
            return;
        };
        for peer in &mut self.peers {
            if !peer.snapshot_due {
                continue;
            }
            let Some(transport) = peer.transport.as_mut() else {
                continue;
            };
            peer.link.send_snapshot(
                transport.as_mut(),
                sector,
                current.clone(),
                &self.world,
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
        self.events.ended(peer.id);
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
            self.events.ended(peer.id);
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

    /// Send `data` to `peer` as a [`ServerToClient::Event`], sealed with its
    /// session's key and on the reliable channel, so it arrives once, whole
    /// and in order with the handshake and everything else sent reliably —
    /// which is what a game sends a newly [`Joined`](PeerEvent::Joined) peer
    /// the state it needs before its first snapshot means anything, and
    /// sends again when it is [`Reaccepted`](PeerEvent::Reaccepted). The
    /// client reads it with `crcbl_client::Client::events`.
    ///
    /// # Errors
    ///
    /// [`EventNotSent`], naming why: `peer` is not a session of this host,
    /// its link is down, `data` is past what a client reads, or the key or
    /// the transport refused it. Nothing was sent.
    ///
    /// [`ServerToClient::Event`]: crcbl_net::ServerToClient::Event
    pub fn send_event(&mut self, peer: PeerId, data: Vec<u8>) -> Result<(), EventNotSent> {
        let limit = crcbl_net::codec::MAX_FIELD_BYTES;
        if data.len() > limit {
            return Err(EventNotSent::TooLarge {
                size: data.len(),
                limit,
            });
        }
        let payload =
            crcbl_net::encode_server_to_client(&crcbl_net::ServerToClient::Event { data });
        self.send_sealed(peer, &payload)
    }

    /// Hold every edit a peer sends for [`take_edit_requests`](Self::take_edit_requests)
    /// from now on, rather than refusing it as not editable — what a server
    /// serving a scene for editing says once, before any peer connects. The
    /// caller applies each, answers it with
    /// [`send_edit_reply`](Self::send_edit_reply), and announces what applied
    /// with [`broadcast_edit_notice`](Self::broadcast_edit_notice).
    pub fn serve_edits(&mut self) {
        self.serving_edits = true;
    }

    /// Take the edits peers sent since the last call, each with the peer that
    /// sent it, in the order the host read them — peers in admission order,
    /// each peer's in arrival order. Empty for a host serving no scene, which
    /// has refused them already. Taken after each [`update`](Self::update);
    /// the inbound budgets bound how many one update can hold.
    pub fn take_edit_requests(&mut self) -> Vec<(PeerId, EditRequest)> {
        std::mem::take(&mut self.edit_requests)
    }

    /// Answer one of `peer`'s edits, sealed with its session's key and on the
    /// reliable channel, which the client reads with
    /// `crcbl_client::Client::edit_replies`.
    ///
    /// # Errors
    ///
    /// [`EventNotSent`], naming why: `peer` is not a session of this host,
    /// its link is down, the refusal's message is past what a client reads,
    /// or the key or the transport refused it. Nothing was sent.
    pub fn send_edit_reply(&mut self, peer: PeerId, reply: &EditReply) -> Result<(), EventNotSent> {
        let payload =
            crcbl_net::encode_edit_reply(reply).map_err(|too_long| EventNotSent::TooLarge {
                size: too_long.len,
                limit: too_long.limit,
            })?;
        self.send_sealed(peer, &payload)
    }

    /// Tell every connected peer of an edit the caller applied — its author
    /// included, so every copy of the scene follows the same way — sealed
    /// and on the reliable channel, which a client reads with
    /// `crcbl_client::Client::edit_notices`. Returns how many peers it was
    /// sent to. A peer whose link is down is passed over, and a send that
    /// fails is counted in [`processing_error_count`](Self::processing_error_count):
    /// either way that peer's copy misses this edit, which the notice's
    /// revision lets it see.
    ///
    /// # Errors
    ///
    /// [`EditTooLong`] for an operation past
    /// [`MAX_EDIT_OP_BYTES`](crcbl_net::MAX_EDIT_OP_BYTES), sent to nobody —
    /// which an operation read off a request never is, the two sharing the
    /// limit.
    pub fn broadcast_edit_notice(&mut self, notice: &EditNotice) -> Result<usize, EditTooLong> {
        let payload = crcbl_net::encode_edit_notice(notice)?;
        let mut sent = 0;
        for peer in &mut self.peers {
            if !peer.is_connected() {
                continue;
            }
            let Some(transport) = peer.transport.as_mut() else {
                continue;
            };
            if peer
                .link
                .send_edit_notice(transport.as_mut(), &payload, &mut self.counters)
            {
                sent += 1;
            }
        }
        Ok(sent)
    }

    /// Take the scene fetches peers sent since the last call, each with the
    /// peer that sent it and the id it numbered it with, in the order the
    /// host read them. Each is answered with [`send_scene`](Self::send_scene)
    /// or [`refuse_scene_fetch`](Self::refuse_scene_fetch); until it is, a
    /// further fetch from that peer is refused as busy, as one is while its
    /// scene is being sent. Empty for a host serving no scene, which has
    /// refused them already.
    pub fn take_scene_fetches(&mut self) -> Vec<(PeerId, u64)> {
        std::mem::take(&mut self.scene_fetches)
    }

    /// Answer `peer`'s fetch `fetch_id` with `scene` — the scene's files as
    /// [`encode_scene_files`](crcbl_net::encode_scene_files) wrote them, at
    /// `revision` — sent a part at a time, sealed and on the reliable
    /// channel, which the client reads with
    /// `crcbl_client::Client::scene_fetches`. The parts go from the next
    /// [`update`](Self::update) on, at [`SCENE_FETCH_BYTES_PER_SECOND`]; a
    /// part the key or the transport refuses ends the stream and is counted
    /// in [`processing_error_count`](Self::processing_error_count), and the
    /// stream is dropped if the peer's link drops. Either way the client's
    /// fetch never completes, and it fetches again.
    ///
    /// # Errors
    ///
    /// [`EventNotSent`], naming why nothing will be sent: `peer` is not a
    /// session of this host, its link is down, or `scene` is empty or past
    /// [`MAX_SCENE_BYTES`](crcbl_net::MAX_SCENE_BYTES). The fetch is answered
    /// either way: the peer may fetch again.
    pub fn send_scene(
        &mut self,
        peer: PeerId,
        fetch_id: u64,
        revision: u64,
        scene: Vec<u8>,
    ) -> Result<(), EventNotSent> {
        let now = self.now;
        let target = self.answer_fetch(peer)?;
        if crcbl_net::scene_part(revision, &scene, 0).is_none() {
            return Err(EventNotSent::TooLarge {
                size: scene.len(),
                limit: crcbl_net::MAX_SCENE_BYTES,
            });
        }
        target.scene_stream = Some(SceneStream::new(fetch_id, revision, scene, now));
        Ok(())
    }

    /// Answer `peer`'s fetch `fetch_id` with a refusal, sealed and on the
    /// reliable channel.
    ///
    /// # Errors
    ///
    /// [`EventNotSent`], naming why: `peer` is not a session of this host,
    /// its link is down, `message` is past what a client reads, or the key or
    /// the transport refused it. The fetch is answered either way.
    pub fn refuse_scene_fetch(
        &mut self,
        peer: PeerId,
        fetch_id: u64,
        reason: EditRefusal,
        message: String,
    ) -> Result<(), EventNotSent> {
        self.answer_fetch(peer)?;
        let reply = SceneReply {
            fetch_id,
            outcome: SceneOutcome::Refused { reason, message },
        };
        let payload =
            crcbl_net::encode_scene_reply(&reply).map_err(|too_long| EventNotSent::TooLarge {
                size: too_long.len,
                limit: too_long.limit,
            })?;
        self.send_sealed(peer, &payload)
    }

    /// `peer`, its waiting fetch answered, if it is connected — what both
    /// answers to a fetch start with.
    fn answer_fetch(&mut self, peer: PeerId) -> Result<&mut Peer, EventNotSent> {
        let Some(target) = self.peers.iter_mut().find(|p| p.id == peer) else {
            return Err(EventNotSent::NoSuchPeer(peer));
        };
        target.fetch_waiting = false;
        if !target.is_connected() {
            return Err(EventNotSent::NotConnected(peer));
        }
        Ok(target)
    }

    /// Seal `payload` with `peer`'s session key and send it on the reliable
    /// channel — the body [`send_event`](Self::send_event),
    /// [`send_edit_reply`](Self::send_edit_reply) and
    /// [`refuse_scene_fetch`](Self::refuse_scene_fetch) share.
    fn send_sealed(&mut self, peer: PeerId, payload: &[u8]) -> Result<(), EventNotSent> {
        let Some(target) = self.peers.iter_mut().find(|p| p.id == peer) else {
            return Err(EventNotSent::NoSuchPeer(peer));
        };
        if !target.is_connected() {
            return Err(EventNotSent::NotConnected(peer));
        }
        // A connected session was keyed when it was admitted or resumed, so
        // both of these are there whenever `is_connected` holds.
        let (Some(transport), Some(crypto)) = (
            target.transport.as_mut(),
            target.link.session_crypto.as_mut(),
        ) else {
            return Err(EventNotSent::NotConnected(peer));
        };
        let sealed = crypto.seal(payload).map_err(EventNotSent::Seal)?;
        transport
            .send_reliable(crcbl_net::Message::reliable(sealed))
            .map_err(EventNotSent::Transport)
    }

    /// Take the session changes since the last call, oldest first.
    pub fn events(&mut self) -> impl Iterator<Item = PeerEvent> + '_ {
        self.events.drain()
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

    /// What `peer`'s session reports about the snapshots it is sent — its
    /// current snapshot interval and the updates withheld as too long — or
    /// `None` once its session has ended.
    #[must_use]
    pub fn peer_stats(&self, peer: PeerId) -> Option<PeerStats> {
        self.peers
            .iter()
            .find(|p| p.id == peer)
            .map(|p| p.link.stats())
    }

    /// What `peer`'s transport measures of its link — round trip, jitter,
    /// loss, resends and bytes ([`Transport::link_stats`]) — or `None` while
    /// its link is down, once its session has ended, or when its transport
    /// measures nothing, as an in-memory one does not.
    #[must_use]
    pub fn peer_link_stats(&self, peer: PeerId) -> Option<EndpointStats> {
        self.peers
            .iter()
            .find(|p| p.id == peer)?
            .transport
            .as_ref()?
            .link_stats()
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

    /// Take console sets of the `Flags::SIM` variables in `registry`, every
    /// one starting at its declared default; the module reads them through
    /// [`PeerInputs::sim_vars`]. Until this is called every set is refused.
    /// Replaces any earlier registry, and the values with it.
    pub fn set_sim_registry(&mut self, registry: Registry) {
        self.sim.set_registry(registry);
    }

    /// Queue a set from the host's own console for the start of the next
    /// tick. Its answer is read with
    /// [`take_console_replies`](Self::take_console_replies).
    pub fn submit_console_set(&mut self, set: ConsoleSet) {
        self.sim.submit(Origin::Console, set);
    }

    /// Take the answers to the console's own sets — and to a replayed
    /// record's entries — since the last call, oldest first.
    pub fn take_console_replies(&mut self) -> Vec<ConsoleReply> {
        self.sim.take_console_replies()
    }

    /// The simulation variables as the latest tick boundary left them.
    #[must_use]
    pub const fn sim_vars(&self) -> &SimVars {
        self.sim.vars()
    }

    /// Every set this host applied, with its tick, in the order applied —
    /// the replay stream [`replay_sim_record`](Self::replay_sim_record) takes.
    #[must_use]
    pub fn sim_record(&self) -> &[AppliedSimSet] {
        self.sim.record()
    }

    /// Take every set applied since the last take, in the order applied,
    /// leaving [`sim_record`](Self::sim_record) empty: what a recorder
    /// streaming a session to a file drains after each update, so the record
    /// holds only the sets since then rather than every one the host ever
    /// applied.
    pub fn take_sim_record(&mut self) -> Vec<AppliedSimSet> {
        self.sim.take_record()
    }

    /// Replay `record` — another host's [`sim_record`](Self::sim_record):
    /// each entry applies at the start of the tick it names, ahead of any
    /// live set of that tick, through the same checks, and is recorded
    /// again. A host built like the recorded one, with the same world,
    /// module and registry and fed the same input, reaches the same state.
    /// An entry whose tick has already passed is refused, through
    /// [`take_console_replies`](Self::take_console_replies).
    pub fn replay_sim_record(&mut self, record: impl IntoIterator<Item = AppliedSimSet>) {
        self.sim.schedule(record);
    }

    /// Record what the module is handed of its peers, from the next tick on:
    /// read with [`peer_input_record`](Self::peer_input_record), and replayed
    /// by [`resimulate`](Self::resimulate). Peers already in session open the
    /// record as joining at the next tick — and being lost, for one whose
    /// link is down — so the record is whole from its first entry. Does
    /// nothing while already recording.
    ///
    /// Off until called, unlike [`sim_record`](Self::sim_record): a set is a
    /// rare event, and frames arrive from every peer every tick.
    pub fn record_peer_inputs(&mut self) {
        let roster = self.peers.iter().flat_map(|peer| {
            let lost = (!peer.is_connected()).then_some(RosterChange::Lost(peer.id));
            std::iter::once(RosterChange::Joined(peer.id)).chain(lost)
        });
        self.events.start_recording(roster);
    }

    /// Every tick's input the module was handed since
    /// [`record_peer_inputs`](Self::record_peer_inputs), in tick order, one
    /// entry for each tick that had a roster change or a peer handed
    /// anything — empty while not recording.
    #[must_use]
    pub fn peer_input_record(&self) -> &[TickInputs] {
        self.events.recorded()
    }

    /// Take every tick's input recorded since the last take, in tick order,
    /// leaving [`peer_input_record`](Self::peer_input_record) empty and the
    /// host still recording — empty while not recording. A recorder that
    /// streams the session to a file drains it after each update, so the
    /// record holds the ticks since then rather than the whole session.
    pub fn take_peer_input_record(&mut self) -> Vec<TickInputs> {
        self.events.take_recorded()
    }

    /// Whether `peer` is the host's own player
    /// ([`add_host_player`](Self::add_host_player)).
    #[must_use]
    pub fn is_host_player(&self, peer: PeerId) -> bool {
        is_host_player(&self.peers, peer)
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

    /// Snapshots a peer's transport refused as too long for its unreliable
    /// channel. Each is counted in
    /// [`processing_error_count`](Self::processing_error_count) too.
    #[must_use]
    pub fn oversized_snapshot_count(&self) -> u64 {
        self.counters.oversized_snapshots
    }

    /// The latest snapshot a peer's transport refused as too long, with the
    /// limit it named: what a game logs when
    /// [`oversized_snapshot_count`](Self::oversized_snapshot_count) moves.
    #[must_use]
    pub fn last_oversized_snapshot(&self) -> Option<SnapshotTooLarge> {
        self.counters.last_oversized_snapshot
    }

    /// Entity updates withheld from peers' snapshots because no snapshot
    /// their transport carries could hold one, summed over every snapshot
    /// sent; [`peer_stats`](Self::peer_stats) has each peer's share. The rest
    /// of each snapshot was sent. A figure that climbs every snapshot is one
    /// entity whose replicated component outgrew the wire.
    #[must_use]
    pub fn oversized_update_count(&self) -> u64 {
        self.counters.oversized_updates
    }

    /// The latest update withheld as too long for any snapshot, naming its
    /// system and entity: what a game logs when
    /// [`oversized_update_count`](Self::oversized_update_count) moves.
    #[must_use]
    pub fn last_oversized_update(&self) -> Option<&UpdateTooLarge> {
        self.counters.last_oversized_update.as_ref()
    }

    /// The longest sealed snapshot any peer's transport has accepted, in
    /// bytes — the figure to hold against a transport's unreliable limit.
    #[must_use]
    pub fn largest_snapshot_bytes(&self) -> usize {
        self.counters.largest_snapshot_bytes
    }

    /// Entity updates held back to fit snapshots to their peers' unreliable
    /// limits, summed over every snapshot sent (see [`crcbl_net::budget`]).
    /// Each is sent in a later snapshot; a figure that climbs every tick is a
    /// sector with more changing than one datagram carries, rotating.
    #[must_use]
    pub fn held_back_update_count(&self) -> u64 {
        self.counters.held_back_updates
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

/// Move the frames each of `peers`' sessions queued this tick into `inputs`,
/// one entry a peer in admission order. The two swap buffers, so neither
/// reallocates tick after tick.
fn take_queued_inputs(peers: &mut [Peer], inputs: &mut Vec<PeerFrames>) {
    inputs.truncate(peers.len());
    inputs.resize_with(peers.len(), || PeerFrames::none(PeerId(0)));
    for (slot, peer) in inputs.iter_mut().zip(peers) {
        slot.peer = peer.id;
        slot.frames.clear();
        std::mem::swap(&mut slot.frames, &mut peer.link.client_inputs);
        slot.dropped = peer.link.dropped_inputs;
    }
}

/// Whether `id` is one of `peers` and the host's own player.
/// The refusal of a fetch from a peer that has one in flight already.
fn busy(fetch_id: u64) -> SceneReply {
    SceneReply {
        fetch_id,
        outcome: SceneOutcome::Refused {
            reason: EditRefusal::BUSY,
            message: "a scene fetch of this client's is already being answered; \
                      fetch again once it arrives"
                .to_owned(),
        },
    }
}

fn is_host_player(peers: &[Peer], id: PeerId) -> bool {
    peers.iter().any(|peer| peer.id == id && peer.host_player)
}

/// The answer to a hello on a connected peer's own link: the same session
/// again, as `Server` answers one, for its own token or for none — and a
/// refusal for any other token.
///
/// **A token-less hello is answered with the session, because the link is the
/// credential.** A client that hears nothing within its handshake timeout says
/// hello again without a token; refusing that one while the client dropped the
/// first `Accept` as an old generation left it retrying for ever, holding a
/// place.
fn rehello(
    gate: &HandshakeGate,
    link: &PeerSession,
    hello: &Hello,
    tick: TickId,
) -> HandshakeResult {
    let result = gate.validate(hello, link.session.session_id(), link.resume_token, tick);
    if matches!(result, HandshakeResult::Accept { .. })
        && hello
            .session_token
            .is_some_and(|token| token != link.resume_token)
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

#[cfg(test)]
mod rate_tests;

#[cfg(test)]
mod sim_tests;

#[cfg(test)]
mod frames_tests;

#[cfg(test)]
mod fetch_tests;

// The UDP transport is native only, by the no-web-networking rule.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod udp_tests;
