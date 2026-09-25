//! One client's session on the server: its lifecycle, its resume credential,
//! its authenticated channel, its inbound budgets, its ack progress and the
//! input it sent this tick — everything that exists once per connected peer.
//!
//! [`Server`](crate::Server) holds one; [`Host`](crate::Host) holds one per
//! peer. What is not per peer — the world, the clock, the handshake gate —
//! stays with them.

use std::time::Duration;

use crcbl_core::TickId;
use crcbl_ecs::{Inspector, World};
use crcbl_net::auth::SessionCrypto;
use crcbl_net::rate_limit::{InboundRateLimitConfig, InboundRateLimiter};
use crcbl_net::{
    Baseline, DeltaCodec, HandshakeResult, Message, RejectReason, ResumeToken, SectorId,
    SessionConfig, SessionEndReason, SessionId, SessionManager, SessionState, SnapshotWriter,
    Transport, Trust,
};

use crate::{KEYFRAME_RECOVERY_TICKS, MAX_CLIENT_INPUTS_PER_TICK, replicated_system_id};

/// The failure counters a server reports, shared by every session it runs.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) processing_errors: u64,
    pub(crate) auth_failures: u64,
    pub(crate) rate_limited_messages: u64,
    pub(crate) rate_limited_bytes: u64,
    /// Frames the per-tick input cap has refused since the server was built.
    pub(crate) dropped_inputs: u64,
}

/// One peer's session state.
#[derive(Debug)]
pub(crate) struct PeerSession {
    pub(crate) session: SessionManager,
    pub(crate) resume_token: ResumeToken,
    /// Authenticated channel for this session; `None` until a handshake is
    /// accepted, and replaced whenever the resume token rotates.
    pub(crate) session_crypto: Option<SessionCrypto>,
    /// Whether a message sealed under the current key has opened: false
    /// from each [`adopt_session_key`](Self::adopt_session_key) until the
    /// client proves it holds that key.
    pub(crate) authenticated: bool,
    /// One limiter per delivery channel: a flood of unreliable state must not
    /// consume the budget that reliable control traffic needs to be read at
    /// all, which is the whole point of having two channels.
    reliable_rate_limiter: InboundRateLimiter,
    unreliable_rate_limiter: InboundRateLimiter,
    /// Ticks since `last_acked_tick` last advanced; drives keyframe recovery.
    ticks_since_ack_progress: u32,
    last_ack_progress: Option<TickId>,
    /// The client input frames that arrived since the current tick began, in
    /// arrival order, handed to the module as
    /// [`ClientInputs`](crcbl_ecs::ClientInputs) and emptied at the start of
    /// every tick. Bounded by [`MAX_CLIENT_INPUTS_PER_TICK`].
    pub(crate) client_inputs: Vec<(TickId, Vec<u8>)>,
    /// Frames the cap refused during the current tick.
    pub(crate) dropped_inputs: u32,
}

impl PeerSession {
    /// A session that has not handshaken yet, holding `resume_token`.
    pub(crate) fn new(
        session_id: SessionId,
        config: &SessionConfig,
        resume_token: ResumeToken,
        rate_limit_config: InboundRateLimitConfig,
        now: Duration,
    ) -> Self {
        Self {
            session: SessionManager::new(session_id, config),
            resume_token,
            session_crypto: None,
            authenticated: false,
            reliable_rate_limiter: InboundRateLimiter::new(rate_limit_config, now),
            unreliable_rate_limiter: InboundRateLimiter::new(rate_limit_config, now),
            ticks_since_ack_progress: 0,
            last_ack_progress: None,
            client_inputs: Vec::new(),
            dropped_inputs: 0,
        }
    }

    /// Replace the session and its credential, forgetting its key and ack
    /// progress; the inbound budgets carry over.
    pub(crate) fn replace_session(
        &mut self,
        session_id: SessionId,
        config: &SessionConfig,
        resume_token: ResumeToken,
    ) {
        self.session = SessionManager::new(session_id, config);
        self.resume_token = resume_token;
        self.session_crypto = None;
        self.ticks_since_ack_progress = 0;
        self.last_ack_progress = None;
    }

    /// Last tick's inputs go before this tick's are read: a frame is offered
    /// to exactly one `GameModule::tick`, and holding it for a later one is the
    /// jitter buffer this deliberately is not.
    pub(crate) fn begin_tick(&mut self) {
        self.client_inputs.clear();
        self.dropped_inputs = 0;
    }

    /// Reconfigure both inbound budgets, resetting them to one second of the
    /// new budget.
    pub(crate) fn reconfigure_rate_limit(&mut self, config: InboundRateLimitConfig, now: Duration) {
        self.reliable_rate_limiter.reconfigure(config, now);
        self.unreliable_rate_limiter.reconfigure(config, now);
    }

    /// Charge one inbound message against its channel's budget, returning
    /// whether the caller may keep reading that channel.
    pub(crate) fn charge_inbound_budget(
        &mut self,
        reliable: bool,
        bytes: usize,
        now: Duration,
        counters: &mut Counters,
    ) -> bool {
        let limiter = if reliable {
            &mut self.reliable_rate_limiter
        } else {
            &mut self.unreliable_rate_limiter
        };
        charge(limiter, bytes, now, counters)
    }

    /// Queue one decoded input frame for this tick's `GameModule::tick`,
    /// or refuse it once the tick is holding [`MAX_CLIENT_INPUTS_PER_TICK`].
    ///
    /// The **newest** frame is refused rather than the oldest evicted: the
    /// frames already queued are the ones the module is about to read in
    /// arrival order, and dropping from the front would hand it a reordered
    /// prefix of what the client said. Refusing costs nothing and keeps the
    /// order the peer sent.
    pub(crate) fn queue_input(&mut self, tick: TickId, data: Vec<u8>, counters: &mut Counters) {
        if self.client_inputs.len() >= MAX_CLIENT_INPUTS_PER_TICK {
            self.dropped_inputs = self.dropped_inputs.saturating_add(1);
            counters.dropped_inputs = counters.dropped_inputs.saturating_add(1);
            return;
        }
        self.client_inputs.push((tick, data));
    }

    pub(crate) fn process_authenticated_message(
        &mut self,
        envelope: &[u8],
        counters: &mut Counters,
    ) {
        // Authenticated traffic only means anything for an established
        // session; before and after that there is no key to check it against.
        if self.session.state() != SessionState::Connected {
            counters.auth_failures += 1;
            return;
        }
        let Some(crypto) = self.session_crypto.as_mut() else {
            counters.auth_failures += 1;
            return;
        };
        let payload = match crypto.open(envelope) {
            Ok(payload) => payload.to_vec(),
            Err(_) => {
                counters.auth_failures += 1;
                return;
            }
        };
        self.authenticated = true;

        match payload.first().copied() {
            Some(crcbl_net::codec::ACK_TAG) => match crcbl_net::decode_ack(&payload) {
                Ok(ack) => self.session.handle_ack(ack.sector, ack.tick),
                Err(_) => counters.processing_errors += 1,
            },
            Some(crcbl_net::codec::INPUT_TAG | crcbl_net::codec::COMMAND_TAG) => {
                match crcbl_net::decode_client_to_server(&payload) {
                    Ok(crcbl_net::ClientToServer::Input { tick, data }) => {
                        self.queue_input(tick, data, counters);
                    }
                    // **A command is not this tick's state.** `Input` is a
                    // sample of what the player was doing when the client
                    // sampled it, which is why it is queued and cleared every
                    // tick; a command ("ready up", a chat line) is a request
                    // that has to be answered once and stay answered, and
                    // nothing on this server consumes one yet. Decoding it
                    // still charges a malformed frame against this session's
                    // error budget, which is what stops a peer sending rubbish
                    // cheaply — but a caller must not read this arm as a
                    // command being acted on.
                    Ok(crcbl_net::ClientToServer::Command { .. }) => {}
                    Err(_) => counters.processing_errors += 1,
                }
            }
            _ => counters.processing_errors += 1,
        }
    }

    /// Key this session's authenticated channel from the current resume token.
    pub(crate) fn adopt_session_key(&mut self) {
        self.session_crypto = Some(SessionCrypto::from_token(&self.resume_token));
        self.authenticated = false;
    }

    /// Delta-encode `current` against this client's baseline, seal it, and
    /// send it on `transport`.
    pub(crate) fn send_snapshot<T: Transport + ?Sized>(
        &mut self,
        transport: &mut T,
        sector: SectorId,
        current: Baseline,
        counters: &mut Counters,
    ) {
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
                counters.processing_errors += 1;
                return;
            }
        };
        let Some(crypto) = self.session_crypto.as_mut() else {
            counters.processing_errors += 1;
            return;
        };
        let payload = match crypto.seal(&payload) {
            Ok(payload) => payload,
            Err(_) => {
                counters.processing_errors += 1;
                return;
            }
        };

        if transport
            .send_unreliable(Message::unreliable(payload))
            .is_err()
        {
            counters.processing_errors += 1;
            return;
        }

        // Store this full snapshot as a new baseline for future deltas — only
        // once the transport accepted it. A baseline whose snapshot never left
        // the server must not be the reference future deltas encode against:
        // that is what evicts the client's real baseline and makes the desync
        // permanent.
        self.session.baseline_store_mut(sector).insert(current);
    }

    /// Tell the client, sealed and on the reliable channel, why its session is
    /// ending — sent before the transport is closed, so the client reads it
    /// ahead of the disconnect. A failure is counted: the client then reads
    /// the close as a lost link.
    pub(crate) fn send_session_end<T: Transport + ?Sized>(
        &mut self,
        transport: &mut T,
        reason: SessionEndReason,
        counters: &mut Counters,
    ) {
        let Some(crypto) = self.session_crypto.as_mut() else {
            // No key, so no session the client could be told about.
            return;
        };
        let Ok(sealed) = crypto.seal(&crcbl_net::encode_session_ended(reason)) else {
            counters.processing_errors += 1;
            return;
        };
        if transport.send_reliable(Message::reliable(sealed)).is_err() {
            counters.processing_errors += 1;
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
}

/// Charge one inbound message of `bytes` against `limiter`, counting what it
/// refuses; returns whether the caller may keep reading.
pub(crate) fn charge(
    limiter: &mut InboundRateLimiter,
    bytes: usize,
    now: Duration,
    counters: &mut Counters,
) -> bool {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    match limiter.allow(now, bytes) {
        Ok(()) => true,
        Err((messages_limited, bytes_limited)) => {
            counters.rate_limited_messages = counters
                .rate_limited_messages
                .saturating_add(u64::from(messages_limited));
            counters.rate_limited_bytes = counters
                .rate_limited_bytes
                .saturating_add(u64::from(bytes_limited));
            false
        }
    }
}

/// Draw a fresh resume credential from the entropy source.
pub(crate) fn generate_resume_token() -> Result<ResumeToken, crcbl_rand::Error> {
    let mut bytes = [0; 32];
    crcbl_rand::entropy(&mut bytes[..])?;
    Ok(ResumeToken::from_bytes(bytes))
}

/// Send `result` on the reliable channel, counting a failed send.
pub(crate) fn send_handshake_result<T: Transport + ?Sized>(
    transport: &mut T,
    result: &HandshakeResult,
    counters: &mut Counters,
) -> bool {
    if transport
        .send_reliable(Message::reliable(crcbl_net::encode_handshake_result(
            result,
        )))
        .is_err()
    {
        counters.processing_errors += 1;
        false
    } else {
        true
    }
}

pub(crate) fn entropy_failure(generation: u64, error: crcbl_rand::Error) -> HandshakeResult {
    HandshakeResult::Reject {
        generation,
        reason: RejectReason {
            code: RejectReason::ENTROPY_FAILURE,
            msg: format!("unable to generate resume credential: {error}"),
        },
    }
}

pub(crate) fn invalid_session_token(generation: u64, message: &str) -> HandshakeResult {
    HandshakeResult::Reject {
        generation,
        reason: RejectReason {
            code: RejectReason::INVALID_SESSION_TOKEN,
            msg: message.into(),
        },
    }
}

/// Serialise every replicated system of `world` at `tick` and parse the
/// blobs into the baseline each client's delta is encoded from.
///
/// Returns `None` when two systems collide on a replicated id, which would
/// otherwise let one system's data land in another's baseline, or when the
/// blobs do not parse.
pub(crate) fn current_baseline(
    world: &World,
    sector: SectorId,
    tick: TickId,
    counters: &mut Counters,
) -> Option<Baseline> {
    let systems = collect_systems(world, sector, tick, counters)?;
    // Parse the freshly written blobs exactly once: the same decoded
    // baseline is both what gets diffed and what gets retained.
    match Baseline::from_snapshot(tick, &systems, Trust::Authenticated) {
        Ok(baseline) => Some(baseline),
        Err(_) => {
            counters.processing_errors += 1;
            None
        }
    }
}

/// Serialise every replicated system into snapshot blobs.
///
/// Returns `None` when two systems collide on a replicated id, which would
/// otherwise let one system's data land in another's baseline.
fn collect_systems(
    world: &World,
    sector: SectorId,
    tick: TickId,
    counters: &mut Counters,
) -> Option<Vec<crcbl_net::SystemSnapshot>> {
    let mut writer = SnapshotWriter::new_with_sector(sector, tick);
    let stats = Inspector::collect(world);
    let mut seen = std::collections::HashSet::new();

    for (system, stat) in world.schedule().iter().zip(stats.iter()) {
        let system_id = replicated_system_id(system.name());
        if !seen.insert(system_id) {
            // Two systems sharing a replicated id would silently overwrite
            // each other in the client's baseline. Drop the whole snapshot
            // rather than replicate a lie.
            counters.processing_errors += 1;
            return None;
        }
        // Systems with a replication impl emit their real per-entity
        // component data; the rest fall back to one synthetic entity
        // carrying only the entity count (4 bytes LE).
        let mut data = Vec::new();
        if !system.replicate(&mut data) {
            crcbl_net::encode_entity_entry(&mut data, 0, &(stat.entity_count as u32).to_le_bytes());
        }
        writer.write_system(system_id, data);
    }

    match writer.finish() {
        crcbl_net::ServerToClient::Snapshot { systems, .. } => Some(systems),
        crcbl_net::ServerToClient::Event { .. } => None,
    }
}
