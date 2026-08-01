//! Rendering client: delta-apply, interpolation buffer, input send.
//!
//! The client sends its input to the server each tick and buffers incoming
//! delta-encoded snapshots. Each delta is applied to a local [`Baseline`]
//! to reconstruct the full server state. Between ticks the two most recent
//! snapshots are used to interpolate entity state for smooth rendering.
//!
//! Every message except the handshake itself carries a per-session MAC (see
//! [`crcbl_net::auth`]); a snapshot that does not verify never reaches the
//! baseline.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Duration;

use crcbl_core::{FrameClock, TickId};
use crcbl_ecs::World;
use crcbl_net::auth::SessionCrypto;
use crcbl_net::rate_limit::{InboundRateLimitConfig, InboundRateLimiter};
use crcbl_net::{
    Baseline, DeltaCodec, HandshakeResult, Hello, Message, MessageKind, ProtocolCompatibility,
    ResumeToken, SectorId, SessionId, Transport, TransportError, Trust,
};
use crcbl_phys::Transform;

/// How long the client waits for a handshake reply before assuming the hello
/// (or its answer) was lost and trying again.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
/// First retry delay after a retryable rejection; doubles per attempt.
const HANDSHAKE_RETRY_BASE: Duration = Duration::from_millis(250);
/// Ceiling on the retry delay.
const HANDSHAKE_RETRY_MAX: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// InterpolatedState
// ---------------------------------------------------------------------------

/// Interpolated state for rendering.
#[derive(Debug, Clone)]
pub struct InterpolatedState {
    /// Interpolated world-space transform for each entity, keyed by entity
    /// bits ([`crcbl_ecs::Entity::to_bits`]). Entities present in only one of
    /// the two buffered snapshots appear at their most recent transform.
    pub transforms: Vec<(u64 /* entity bits */, Transform)>,
}

/// One buffered server state, tagged with the tick it represents.
///
/// The tick is what makes interpolation correct: the gap between two buffered
/// snapshots is a number of *server* ticks, and interpolating across it with a
/// local render fraction is only right when the two happen to coincide.
#[derive(Debug, Clone)]
struct Frame {
    tick: TickId,
    transforms: HashMap<u64, Transform>,
}

/// Read the transforms out of a reconstructed baseline.
///
/// Entries whose payload is not a valid [`Transform`] encoding (e.g. the
/// 4-byte synthetic count emitted for non-replicated systems) are skipped.
fn frame_from_baseline(baseline: &Baseline) -> Frame {
    let transforms = baseline
        .iter_entities()
        .filter_map(|(_, entity_bits, data)| {
            Transform::decode(data).map(|transform| (entity_bits, transform))
        })
        .collect();
    Frame {
        tick: baseline.tick,
        transforms,
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// The rendering client: sends input to the server, applies incoming
/// delta-encoded snapshots, and provides interpolated entity state for smooth
/// frame-rate rendering.
pub struct Client<T: Transport> {
    /// Local ECS world (receives snapshots).
    world: World,
    /// Transport to the server.
    transport: T,
    /// Client-side frame clock for input-tick cadence and render alpha.
    clock: FrameClock,
    /// Sectors this client currently accepts replication for.
    subscribed_sectors: HashSet<SectorId>,
    /// Older buffered frames after delta apply, keyed by sector.
    prev_frames: HashMap<SectorId, Frame>,
    /// Newer buffered frames after delta apply, keyed by sector.
    current_frames: HashMap<SectorId, Frame>,
    /// Input data to send on the next tick.
    pending_input: Vec<u8>,
    /// Accumulated baselines for delta application, keyed by sector.
    baselines: HashMap<SectorId, Baseline>,
    /// Playback position in server ticks, advanced by wall time.
    playback_tick: f64,
    /// Server ticks per second, used to advance `playback_tick`.
    tick_rate_hz: f64,
    last_update: Option<Duration>,
    now: Duration,
    session_id: Option<SessionId>,
    resume_token: Option<ResumeToken>,
    /// Authenticated channel; `None` until an [`HandshakeResult::Accept`]
    /// hands over a resume token to key it with.
    session_crypto: Option<SessionCrypto>,
    compatibility: ProtocolCompatibility,
    handshake_generation: u64,
    outstanding_handshake_generation: Option<u64>,
    /// When the outstanding hello stops being worth waiting for.
    handshake_deadline: Option<Duration>,
    /// Earliest time the next hello may be sent, after a retryable rejection.
    handshake_retry_at: Option<Duration>,
    handshake_attempts: u32,
    handshake_complete: bool,
    /// Set only by a rejection this build can never satisfy.
    handshake_blocked: bool,
    reliable_rate_limiter: InboundRateLimiter,
    unreliable_rate_limiter: InboundRateLimiter,
    processing_error_count: u64,
    auth_failure_count: u64,
    rate_limited_message_count: u64,
    rate_limited_byte_count: u64,
}

impl<T: Transport> Client<T> {
    /// Create a client with explicit protocol compatibility identifiers.
    ///
    /// There is deliberately no constructor that defaults them:
    /// [`ProtocolCompatibility::DEFAULT`] carries zero engine and schema ids,
    /// which protect nothing.
    ///
    /// # Panics
    ///
    /// Panics if `tick_hz` is zero, or if either compatibility identifier is
    /// zero.
    #[must_use]
    pub fn new_with_compatibility(
        world: World,
        transport: T,
        tick_hz: u32,
        compatibility: ProtocolCompatibility,
    ) -> Self {
        compatibility.assert_explicit();
        let clock = FrameClock::new(tick_hz);
        let tick_rate_hz = 1.0 / clock.tick_dt_secs();
        let rate_limit_config = InboundRateLimitConfig::default();
        Self {
            world,
            transport,
            clock,
            subscribed_sectors: HashSet::from([SectorId::ZERO]),
            prev_frames: HashMap::new(),
            current_frames: HashMap::new(),
            pending_input: Vec::new(),
            baselines: HashMap::new(),
            playback_tick: 0.0,
            tick_rate_hz,
            last_update: None,
            now: Duration::ZERO,
            session_id: None,
            resume_token: None,
            session_crypto: None,
            compatibility,
            handshake_generation: 0,
            outstanding_handshake_generation: None,
            handshake_deadline: None,
            handshake_retry_at: None,
            handshake_attempts: 0,
            handshake_complete: false,
            handshake_blocked: false,
            reliable_rate_limiter: InboundRateLimiter::new(rate_limit_config, Duration::ZERO),
            unreliable_rate_limiter: InboundRateLimiter::new(rate_limit_config, Duration::ZERO),
            processing_error_count: 0,
            auth_failure_count: 0,
            rate_limited_message_count: 0,
            rate_limited_byte_count: 0,
        }
    }

    /// Feed the current time.
    ///
    /// Drains received snapshots into the buffer, sends pending input for
    /// each consumed tick, and returns the interpolation alpha in `[0, 1]`.
    ///
    /// The alpha spans the two buffered snapshots' *server* ticks, so it stays
    /// correct when a snapshot is lost or when the server ticks at a different
    /// rate than this client. With fewer than two snapshots buffered it falls
    /// back to the local frame clock's fraction.
    pub fn update(&mut self, now: std::time::Duration) -> f32 {
        self.now = now;
        self.clock.update(now);
        self.drive_handshake();
        while self.clock.consume_tick() {
            let tick = self.clock.tick();
            if self.send_input(tick).is_err() {
                self.processing_error_count += 1;
            }
        }
        if self.recv_snapshots().is_err() {
            self.processing_error_count += 1;
        }
        self.advance_playback(now);
        self.interpolation_alpha()
    }

    /// The interpolation alpha for the currently buffered snapshot pair.
    #[must_use]
    pub fn interpolation_alpha(&self) -> f32 {
        self.snapshot_alpha().unwrap_or_else(|| self.clock.alpha())
    }

    fn snapshot_alpha(&self) -> Option<f32> {
        let current = self.current_frames.get(&SectorId::ZERO)?;
        let prev = self.prev_frames.get(&SectorId::ZERO)?;
        let span = current.tick.get().checked_sub(prev.tick.get())?;
        if span == 0 {
            return Some(1.0);
        }
        let position = self.playback_tick - prev.tick.get() as f64;
        Some((position / span as f64).clamp(0.0, 1.0) as f32)
    }

    /// Advance the playback position by wall time, bounded by the snapshots
    /// actually held. Without the clamp a stalled stream would extrapolate.
    fn advance_playback(&mut self, now: Duration) {
        let elapsed = self
            .last_update
            .map_or(Duration::ZERO, |previous| now.saturating_sub(previous));
        self.last_update = Some(now);
        self.playback_tick += elapsed.as_secs_f64() * self.tick_rate_hz;

        if let (Some(prev), Some(current)) = (
            self.prev_frames.get(&SectorId::ZERO),
            self.current_frames.get(&SectorId::ZERO),
        ) {
            let low = prev.tick.get() as f64;
            let high = current.tick.get() as f64;
            self.playback_tick = self.playback_tick.clamp(low.min(high), low.max(high));
        }
    }

    /// Replace the accepted replication sector set.
    ///
    /// State for sectors omitted from the new set is dropped immediately.
    /// The default zero sector remains accepted only if included explicitly.
    pub fn set_subscribed_sectors(&mut self, sectors: impl IntoIterator<Item = SectorId>) {
        self.subscribed_sectors = sectors.into_iter().collect();
        self.baselines
            .retain(|sector, _| self.subscribed_sectors.contains(sector));
        self.prev_frames
            .retain(|sector, _| self.subscribed_sectors.contains(sector));
        self.current_frames
            .retain(|sector, _| self.subscribed_sectors.contains(sector));
    }

    /// Configure the inbound traffic budget.
    ///
    /// Applies to each delivery channel independently. Every accepted snapshot
    /// costs a baseline mutation and a reconstruction pass, so an unbounded
    /// drain lets whoever is sending decide how much work this client does.
    pub fn set_inbound_rate_limit_config(&mut self, config: InboundRateLimitConfig) {
        self.reliable_rate_limiter.reconfigure(config, self.now);
        self.unreliable_rate_limiter.reconfigure(config, self.now);
    }

    /// Set the input data for the next tick.
    ///
    /// The data is sent (unreliably, and authenticated once the session is
    /// established) to the server on each consumed tick until replaced by
    /// another `set_input` call.
    pub fn set_input(&mut self, input: Vec<u8>) {
        self.pending_input = input;
    }

    /// Interpolated per-entity transforms for rendering.
    ///
    /// Lerps between the two most recent buffered snapshots in the default
    /// sector at the given `alpha` — the value returned by [`Self::update`].
    /// Entities present in only one snapshot appear at that snapshot's
    /// transform; with fewer than two snapshots the newest is used as-is.
    #[must_use]
    pub fn interpolate(&self, alpha: f32) -> InterpolatedState {
        let empty = HashMap::new();
        let current = self
            .current_frames
            .get(&SectorId::ZERO)
            .map_or(&empty, |frame| &frame.transforms);
        let prev = self
            .prev_frames
            .get(&SectorId::ZERO)
            .map_or(&empty, |frame| &frame.transforms);

        let alpha = f64::from(alpha.clamp(0.0, 1.0));
        let mut transforms: Vec<(u64, Transform)> = current
            .iter()
            .map(|(&entity_bits, current_transform)| {
                let transform = prev
                    .get(&entity_bits)
                    .map_or(*current_transform, |prev_transform| {
                        prev_transform.lerp(current_transform, alpha)
                    });
                (entity_bits, transform)
            })
            .collect();
        for (&entity_bits, prev_transform) in prev {
            if !current.contains_key(&entity_bits) {
                transforms.push((entity_bits, *prev_transform));
            }
        }
        transforms.sort_by_key(|(entity_bits, _)| *entity_bits);
        InterpolatedState { transforms }
    }

    /// The tick id of the most recently applied delta-encoded snapshot in the
    /// default sector.
    #[must_use]
    pub fn last_applied_tick(&self) -> TickId {
        self.last_applied_tick_in(SectorId::ZERO)
    }

    /// The tick id of the most recently applied delta-encoded snapshot in `sector`.
    #[must_use]
    pub fn last_applied_tick_in(&self, sector: SectorId) -> TickId {
        self.baselines
            .get(&sector)
            .map_or(TickId::ZERO, |baseline| baseline.tick)
    }

    /// Number of entities in the client's reconstructed baseline in the default
    /// sector.
    #[must_use]
    pub fn baseline_entity_count(&self) -> usize {
        self.baseline_entity_count_in(SectorId::ZERO)
    }

    /// Number of entities in the client's reconstructed baseline in `sector`.
    #[must_use]
    pub fn baseline_entity_count_in(&self, sector: SectorId) -> usize {
        self.baselines
            .get(&sector)
            .map_or(0, Baseline::entity_count)
    }

    /// Number of systems in the client's reconstructed baseline in the default
    /// sector.
    #[must_use]
    pub fn baseline_system_count(&self) -> usize {
        self.baseline_system_count_in(SectorId::ZERO)
    }

    /// Number of systems in the client's reconstructed baseline in `sector`.
    #[must_use]
    pub fn baseline_system_count_in(&self, sector: SectorId) -> usize {
        self.baselines
            .get(&sector)
            .map_or(0, Baseline::system_count)
    }

    /// Process-local hash of the reconstructed baseline in `sector`.
    #[must_use]
    pub fn baseline_state_hash_in(&self, sector: SectorId) -> Option<u64> {
        self.baselines.get(&sector).map(Baseline::state_hash)
    }

    /// The accepted server session id, if the handshake has completed.
    #[must_use]
    pub fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// Whether the client has given up handshaking with this server.
    ///
    /// True only after a rejection this build can never satisfy — a protocol,
    /// schema, or engine-build mismatch. Every other rejection is retried with
    /// backoff, because a client that latched on a transient one could be
    /// wedged by a single forged packet.
    #[must_use]
    pub fn handshake_blocked(&self) -> bool {
        self.handshake_blocked
    }

    /// Request a fresh handshake or resume the accepted session on a replacement
    /// transport. The caller must provide a newly connected transport.
    pub fn reconnect(&mut self, transport: T) {
        self.transport = transport;
        self.outstanding_handshake_generation = None;
        self.handshake_deadline = None;
        self.handshake_retry_at = None;
        self.handshake_attempts = 0;
        self.handshake_complete = false;
        self.handshake_blocked = false;
        self.session_crypto = None;
    }

    /// Number of unrecoverable transport, encoding, or decoding errors.
    #[must_use]
    pub fn processing_error_count(&self) -> u64 {
        self.processing_error_count
    }

    /// Number of messages rejected because they were unauthenticated, carried
    /// a bad MAC, or replayed a counter.
    #[must_use]
    pub fn auth_failure_count(&self) -> u64 {
        self.auth_failure_count
    }

    /// Number of inbound messages dropped by the message-rate budget.
    #[must_use]
    pub fn rate_limited_message_count(&self) -> u64 {
        self.rate_limited_message_count
    }

    /// Number of inbound messages dropped by the byte-rate budget.
    #[must_use]
    pub fn rate_limited_byte_count(&self) -> u64 {
        self.rate_limited_byte_count
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

    /// Send a hello when one is due.
    ///
    /// An outstanding hello that goes unanswered times out rather than
    /// blocking every future attempt, and a retryable rejection backs off
    /// rather than latching.
    fn drive_handshake(&mut self) {
        if self.handshake_complete || self.handshake_blocked {
            return;
        }
        if let Some(deadline) = self.handshake_deadline
            && self.now >= deadline
        {
            // The wait itself is the backoff — one hello per timeout is
            // already a bounded rate — so this retries immediately rather
            // than stacking another delay on top.
            self.outstanding_handshake_generation = None;
            self.handshake_deadline = None;
            self.handshake_retry_at = None;
        }
        if self.outstanding_handshake_generation.is_some() {
            return;
        }
        if self
            .handshake_retry_at
            .is_some_and(|retry_at| self.now < retry_at)
        {
            return;
        }
        self.send_hello();
    }

    fn schedule_handshake_retry(&mut self) {
        self.handshake_attempts = self.handshake_attempts.saturating_add(1);
        let backoff = HANDSHAKE_RETRY_BASE
            .checked_mul(
                1u32.checked_shl(self.handshake_attempts - 1)
                    .unwrap_or(u32::MAX),
            )
            .unwrap_or(HANDSHAKE_RETRY_MAX)
            .min(HANDSHAKE_RETRY_MAX);
        self.handshake_retry_at = Some(self.now.saturating_add(backoff));
    }

    /// Send a fresh or resume handshake.
    ///
    /// # Panics
    ///
    /// Panics if the 2^64 generation space is exhausted, which would otherwise
    /// let a stale reply be matched against a new hello.
    fn send_hello(&mut self) {
        self.handshake_generation = self
            .handshake_generation
            .checked_add(1)
            .expect("handshake generation exhausted; recreate the client");
        let generation = self.handshake_generation;
        let result = self
            .transport
            .send_reliable(Message::reliable(crcbl_net::encode_hello(&Hello {
                protocol_version: self.compatibility.protocol_version,
                engine_build_id: self.compatibility.engine_build_id,
                schema_hash: self.compatibility.schema_hash,
                generation,
                session_token: self.resume_token,
            })));
        if result.is_ok() {
            self.outstanding_handshake_generation = Some(generation);
            self.handshake_deadline = Some(self.now.saturating_add(HANDSHAKE_TIMEOUT));
        } else {
            self.processing_error_count += 1;
        }
    }

    /// Send pending input to the server for the given tick.
    fn send_input(&mut self, tick: TickId) -> Result<(), TransportError> {
        if self.pending_input.is_empty() {
            return Ok(());
        }
        let payload = crcbl_net::encode_client_to_server(&crcbl_net::ClientToServer::Input {
            tick,
            data: self.pending_input.clone(),
        });
        self.send_authenticated(payload)
    }

    /// Seal and send a payload on the unreliable channel.
    ///
    /// Before the handshake completes there is no key, and therefore nothing
    /// the server would accept — so the payload is dropped rather than sent in
    /// the clear.
    fn send_authenticated(&mut self, payload: Vec<u8>) -> Result<(), TransportError> {
        let Some(crypto) = self.session_crypto.as_mut() else {
            return Ok(());
        };
        let Ok(sealed) = crypto.seal(&payload) else {
            self.processing_error_count += 1;
            return Ok(());
        };
        self.transport.send_unreliable(Message::unreliable(sealed))
    }

    /// Charge one inbound message against its channel's budget, returning
    /// whether the caller may keep reading.
    fn charge_inbound_budget(&mut self, kind: MessageKind, bytes: usize) -> bool {
        let now = self.now;
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let limiter = match kind {
            MessageKind::Reliable => &mut self.reliable_rate_limiter,
            MessageKind::Unreliable => &mut self.unreliable_rate_limiter,
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

    /// Drain available messages from the transport, apply snapshots to the
    /// local baseline, and slide the two-slot interpolation buffer.
    fn recv_snapshots(&mut self) -> Result<(), TransportError> {
        // Sectors whose baseline needs re-announcing because the server is
        // delta-encoding against a tick this client does not hold. At most one
        // repair ack per sector per drain, so a flood of mismatched deltas
        // cannot be turned into a flood of acks.
        let mut repairs: HashMap<SectorId, TickId> = HashMap::new();

        while let Some(msg) = self.transport.recv()? {
            if !self.charge_inbound_budget(msg.kind, msg.payload.len()) {
                break;
            }
            match msg.payload.first().copied() {
                Some(crcbl_net::codec::ACCEPT_TAG | crcbl_net::codec::REJECT_TAG) => {
                    self.handle_handshake_result(&msg.payload);
                }
                Some(crcbl_net::auth::AUTH_TAG) => {
                    self.handle_sealed_snapshot(&msg.payload, &mut repairs);
                }
                _ => self.processing_error_count += 1,
            }
        }

        for (sector, tick) in repairs {
            self.send_ack(sector, tick);
        }
        Ok(())
    }

    fn handle_handshake_result(&mut self, payload: &[u8]) {
        let Ok(result) = crcbl_net::decode_handshake_result(payload) else {
            self.processing_error_count += 1;
            return;
        };
        let generation = match &result {
            HandshakeResult::Accept { generation, .. }
            | HandshakeResult::Reject { generation, .. } => *generation,
        };
        if self.outstanding_handshake_generation != Some(generation) {
            self.processing_error_count += 1;
            return;
        }
        self.outstanding_handshake_generation = None;
        self.handshake_deadline = None;
        match result {
            HandshakeResult::Accept {
                session_id,
                resume_token,
                server_tick,
                ..
            } => {
                self.session_id = Some(session_id);
                self.resume_token = Some(resume_token);
                // The resume token is the shared secret; deriving the channel
                // from it is what makes every later message checkable.
                self.session_crypto = Some(SessionCrypto::from_token(&resume_token));
                self.handshake_complete = true;
                self.handshake_attempts = 0;
                self.handshake_retry_at = None;
                // Start playback at the server's clock rather than zero, so
                // the first snapshot pair does not have to drag it forwards.
                self.playback_tick = server_tick.get() as f64;
            }
            HandshakeResult::Reject { reason, .. } => {
                self.processing_error_count += 1;
                if reason.is_permanent() {
                    self.handshake_blocked = true;
                } else {
                    self.schedule_handshake_retry();
                }
            }
        }
    }

    fn handle_sealed_snapshot(&mut self, envelope: &[u8], repairs: &mut HashMap<SectorId, TickId>) {
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
        // The MAC verified, so the sender holds this session's key: the
        // hostile-input system cap no longer buys anything here.
        let delta = match crcbl_net::decode_delta(&payload, Trust::Authenticated) {
            Ok(delta) => delta,
            Err(_) => {
                self.processing_error_count += 1;
                return;
            }
        };

        let sector = delta.sector;
        if !self.subscribed_sectors.contains(&sector) {
            self.processing_error_count += 1;
            return;
        }
        // Applied in place, not to a copy: `DeltaCodec::apply` validates the
        // whole delta before it writes anything, so a rejected one leaves this
        // baseline exactly as it was. Cloning it here would have put a second
        // copy of the full state on the path of every packet.
        let baseline = self.baselines.entry(sector).or_insert_with(|| {
            Baseline::from_snapshot(TickId::ZERO, &[], Trust::Authenticated)
                .expect("empty snapshot is valid")
        });
        if delta.tick <= baseline.tick {
            return;
        }
        if delta.is_keyframe && delta.baseline_tick.is_some() {
            self.processing_error_count += 1;
            return;
        }
        if !delta.is_keyframe && delta.baseline_tick != Some(baseline.tick) {
            // The server is encoding against a baseline this client does not
            // hold — older, newer, or one it never received. Re-announce what
            // it does hold, whichever it is. Only announcing for an *older*
            // baseline left the newer case with no way out at all: the client
            // rejected every delta in silence and the server never learned
            // why. If the announcement does not help either (the server has
            // evicted that tick), the server's own stall detector sends a
            // keyframe.
            if baseline.tick > TickId::ZERO {
                repairs.insert(sector, baseline.tick);
            }
            return;
        }

        if DeltaCodec::apply(&delta, baseline, Trust::Authenticated).is_err() {
            self.processing_error_count += 1;
            return;
        }

        let frame = frame_from_baseline(baseline);
        self.send_ack(sector, delta.tick);

        let is_newer = self
            .current_frames
            .get(&sector)
            .is_none_or(|current| delta.tick > current.tick);
        if is_newer && let Some(current) = self.current_frames.insert(sector, frame) {
            self.prev_frames.insert(sector, current);
        }
    }

    fn send_ack(&mut self, sector: SectorId, tick: TickId) {
        if self
            .send_authenticated(crcbl_net::encode_ack(sector, tick))
            .is_err()
        {
            self.processing_error_count += 1;
        }
    }
}

impl<T: Transport> fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("world", &self.world)
            .field("clock", &self.clock)
            .field("connected", &self.transport.is_connected())
            .field(
                "prev_snapshot_tick",
                &self
                    .prev_frames
                    .get(&SectorId::ZERO)
                    .map(|frame| frame.tick),
            )
            .field(
                "current_snapshot_tick",
                &self
                    .current_frames
                    .get(&SectorId::ZERO)
                    .map(|frame| frame.tick),
            )
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
    use crcbl_net::auth::AUTH_TAG;
    use crcbl_net::{Delta, InMemoryTransport, RejectReason};

    // ── Helpers ────────────────────────────────────────────────────────────

    const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 0x0043_5243_424C,
        schema_hash: 0x0053_5256,
    };

    const TICK: Duration = Duration::from_nanos(16_666_667);

    fn client(transport: InMemoryTransport) -> Client<InMemoryTransport> {
        Client::new_with_compatibility(World::new(), transport, 60, COMPATIBILITY)
    }

    /// Play the server side of a handshake and return the channel the peer
    /// must seal its snapshots with.
    fn connect(
        client: &mut Client<InMemoryTransport>,
        peer: &mut InMemoryTransport,
        now: Duration,
    ) -> SessionCrypto {
        accept_hello(client, peer, now, ResumeToken::from_bytes([0xA5; 32]))
    }

    fn accept_hello(
        client: &mut Client<InMemoryTransport>,
        peer: &mut InMemoryTransport,
        now: Duration,
        token: ResumeToken,
    ) -> SessionCrypto {
        client.update(now);
        let hello =
            crcbl_net::decode_hello(&peer.recv().unwrap().expect("client sends a hello").payload)
                .expect("client hello decodes");
        peer.send_reliable(Message::reliable(crcbl_net::encode_handshake_result(
            &HandshakeResult::Accept {
                generation: hello.generation,
                session_id: SessionId(1),
                resume_token: token,
                server_tick: TickId::ZERO,
            },
        )))
        .unwrap();
        client.update(now);
        assert_eq!(client.session_id(), Some(SessionId(1)));
        SessionCrypto::from_token(&token)
    }

    fn send_sealed(peer: &mut InMemoryTransport, crypto: &mut SessionCrypto, payload: &[u8]) {
        let sealed = crypto.seal(payload).expect("counter space available");
        peer.send_unreliable(Message::unreliable(sealed)).unwrap();
    }

    /// Open the next message from the client and decode it as an ack.
    fn expect_ack(peer: &mut InMemoryTransport, crypto: &mut SessionCrypto) -> crcbl_net::Ack {
        let msg = peer.recv().unwrap().expect("client sends a message");
        assert_eq!(msg.payload.first(), Some(&AUTH_TAG));
        let payload = crypto.open(&msg.payload).expect("client seals its acks");
        crcbl_net::decode_ack(payload).expect("payload is an ack")
    }

    /// Build a keyframe delta payload for `sector` at `tick`.
    fn keyframe_snapshot_for_sector(
        sector: SectorId,
        tick: u64,
        system_data: &[(u32, Vec<u8>)],
    ) -> Vec<u8> {
        let snapshots: Vec<_> = system_data
            .iter()
            .map(|&(system_id, ref data)| crcbl_net::SystemSnapshot {
                system_id,
                data: entity_blob_of(data),
            })
            .collect();
        let delta =
            DeltaCodec::encode_with_sector(sector, TickId::from_raw(tick), &snapshots, None)
                .expect("valid snapshot");
        crcbl_net::encode_delta(&delta).expect("valid delta")
    }

    /// Wrap raw component bytes as a single-entity blob unless they already
    /// are one.
    fn entity_blob_of(data: &[u8]) -> Vec<u8> {
        let mut cursor = 0usize;
        let is_entity_blob = loop {
            if cursor == data.len() {
                break true;
            }
            if data.len() - cursor < 12 {
                break false;
            }
            let len =
                u32::from_le_bytes(data[cursor + 8..cursor + 12].try_into().unwrap()) as usize;
            cursor += 12;
            if len > data.len() - cursor {
                break false;
            }
            cursor += len;
        };
        if is_entity_blob {
            return data.to_vec();
        }
        let mut blob = Vec::new();
        crcbl_net::encode_entity_entry(&mut blob, 0, data);
        blob
    }

    fn keyframe_snapshot(tick: u64, system_data: &[(u32, Vec<u8>)]) -> Vec<u8> {
        keyframe_snapshot_for_sector(SectorId::ZERO, tick, system_data)
    }

    fn delta_payload(delta: Delta) -> Vec<u8> {
        crcbl_net::encode_delta(&delta).expect("valid delta")
    }

    fn transform_blob(entities: &[(u64, f64)]) -> Vec<u8> {
        let mut blob = Vec::new();
        for &(bits, x) in entities {
            let mut data = Vec::new();
            Transform::from_position(glam::DVec3::new(x, 0.0, 0.0)).encode(&mut data);
            crcbl_net::encode_entity_entry(&mut blob, bits, &data);
        }
        blob
    }

    // ── Construction ───────────────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "engine_build_id and schema_hash must be non-zero")]
    fn placeholder_compatibility_is_refused() {
        let (transport, _peer) = InMemoryTransport::pair();
        let _ = Client::new_with_compatibility(
            World::new(),
            transport,
            60,
            ProtocolCompatibility::DEFAULT,
        );
    }

    #[test]
    fn client_creates_with_empty_buffers() {
        let (transport, _peer) = InMemoryTransport::pair();
        let client = client(transport);
        assert!(client.is_connected());
        let debug = format!("{client:?}");
        assert!(debug.contains("prev_snapshot_tick: None"));
        assert!(debug.contains("current_snapshot_tick: None"));
    }

    // ── Authentication ─────────────────────────────────────────────────────

    #[test]
    fn an_unauthenticated_snapshot_is_never_applied() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        connect(&mut client, &mut peer, Duration::ZERO);

        // Bare, exactly as the old protocol sent it.
        peer.send_unreliable(Message::unreliable(keyframe_snapshot(1, &[])))
            .unwrap();
        // Sealed under a key that is not this session's.
        let mut forged = SessionCrypto::from_token(&ResumeToken::from_bytes([0x11; 32]));
        send_sealed(&mut peer, &mut forged, &keyframe_snapshot(1, &[]));
        client.update(TICK);

        assert_eq!(client.last_applied_tick(), TickId::ZERO);
        assert_eq!(client.auth_failure_count(), 1);
        assert_eq!(client.processing_error_count(), 1);
    }

    #[test]
    fn a_replayed_snapshot_is_rejected() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        let sealed = crypto
            .seal(&keyframe_snapshot(1, &[]))
            .expect("counter space available");
        peer.send_unreliable(Message::unreliable(sealed.clone()))
            .unwrap();
        client.update(TICK);
        assert_eq!(client.last_applied_tick(), TickId::from_raw(1));
        assert_eq!(client.auth_failure_count(), 0);

        peer.send_unreliable(Message::unreliable(sealed)).unwrap();
        client.update(2 * TICK);
        assert_eq!(client.auth_failure_count(), 1);
    }

    // ── Handshake recovery ─────────────────────────────────────────────────

    #[test]
    fn a_forged_transient_reject_does_not_wedge_the_client() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);

        client.update(Duration::ZERO);
        let generation = crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload)
            .unwrap()
            .generation;
        // The handshake carries no MAC, so anyone who saw the hello can forge
        // this. It must cost a retry delay, not the session.
        peer.send_reliable(Message::reliable(crcbl_net::encode_handshake_result(
            &HandshakeResult::Reject {
                generation,
                reason: RejectReason {
                    code: RejectReason::SERVER_FULL,
                    msg: "full".into(),
                },
            },
        )))
        .unwrap();
        client.update(TICK);
        assert!(!client.handshake_blocked());
        // Backoff holds the retry briefly...
        client.update(2 * TICK);
        assert!(peer.recv().unwrap().is_none());

        // ...and then the client tries again on its own.
        client.update(Duration::from_secs(1));
        let retry = crcbl_net::decode_hello(
            &peer
                .recv()
                .unwrap()
                .expect("client retries after a transient rejection")
                .payload,
        )
        .unwrap();
        assert!(retry.generation > generation);
    }

    #[test]
    fn a_permanent_reject_stops_the_client() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);

        client.update(Duration::ZERO);
        let generation = crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload)
            .unwrap()
            .generation;
        peer.send_reliable(Message::reliable(crcbl_net::encode_handshake_result(
            &HandshakeResult::Reject {
                generation,
                reason: RejectReason {
                    code: RejectReason::SCHEMA_MISMATCH,
                    msg: "incompatible".into(),
                },
            },
        )))
        .unwrap();
        client.update(TICK);

        assert!(client.handshake_blocked());
        client.update(Duration::from_secs(60));
        assert!(
            peer.recv().unwrap().is_none(),
            "a schema mismatch cannot be resolved by retrying"
        );
        assert_eq!(client.session_id(), None);
    }

    #[test]
    fn an_unanswered_hello_times_out_and_is_retried() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);

        client.update(Duration::ZERO);
        let first = crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload)
            .unwrap()
            .generation;
        client.update(TICK);
        assert!(peer.recv().unwrap().is_none(), "no reply yet, no retry yet");

        client.update(HANDSHAKE_TIMEOUT + Duration::from_secs(10));
        let second = crcbl_net::decode_hello(
            &peer
                .recv()
                .unwrap()
                .expect("a lost hello is eventually resent")
                .payload,
        )
        .unwrap()
        .generation;
        assert!(second > first);
    }

    #[test]
    fn rejects_stale_and_unsolicited_handshake_replies() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let initial_token = ResumeToken::from_bytes([1; 32]);
        accept_hello(&mut client, &mut peer, Duration::ZERO, initial_token);
        let stale_generation = client.handshake_generation;

        let (client_transport, mut peer) = InMemoryTransport::pair();
        client.reconnect(client_transport);
        client.update(TICK);
        let reconnect_generation = crcbl_net::decode_hello(&peer.recv().unwrap().unwrap().payload)
            .unwrap()
            .generation;
        assert!(reconnect_generation > stale_generation);

        // A reply carrying the previous generation must not replace the
        // credential the client is still waiting to renew.
        peer.send_reliable(Message::reliable(crcbl_net::encode_handshake_result(
            &HandshakeResult::Accept {
                generation: stale_generation,
                session_id: SessionId(99),
                resume_token: ResumeToken::from_bytes([9; 32]),
                server_tick: TickId::ZERO,
            },
        )))
        .unwrap();
        client.update(2 * TICK);
        assert_eq!(client.session_id(), Some(SessionId(1)));
        assert_eq!(client.resume_token, Some(initial_token));
        assert_eq!(client.processing_error_count(), 1);
    }

    #[test]
    #[should_panic(expected = "handshake generation exhausted; recreate the client")]
    fn handshake_generation_never_wraps() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut client = client(transport);
        client.handshake_generation = u64::MAX;
        client.send_hello();
    }

    // ── Input send ─────────────────────────────────────────────────────────

    #[test]
    fn set_input_sends_sealed_input_on_each_tick() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
        client.set_input(vec![1, 2, 3]);

        client.update(2 * TICK);

        let msg = peer.recv().unwrap().expect("input is sent");
        assert_eq!(msg.kind, MessageKind::Unreliable);
        let payload = crypto.open(&msg.payload).expect("input is sealed");
        assert!(crcbl_net::decode_client_to_server(payload).is_ok());
    }

    #[test]
    fn no_input_sends_nothing_but_the_hello() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);

        client.update(Duration::ZERO);
        client.update(TICK);

        let hello = peer.recv().unwrap().unwrap();
        assert!(crcbl_net::decode_hello(&hello.payload).is_ok());
        assert!(peer.recv().unwrap().is_none());
    }

    // ── Snapshot receive ───────────────────────────────────────────────────

    #[test]
    fn applies_a_keyframe_and_acks_it() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        send_sealed(
            &mut peer,
            &mut crypto,
            &keyframe_snapshot(1, &[(0, vec![1, 0, 0, 0])]),
        );
        client.update(TICK);

        assert_eq!(client.last_applied_tick(), TickId::from_raw(1));
        assert_eq!(expect_ack(&mut peer, &mut crypto).tick, TickId::from_raw(1));
        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(1))"));
    }

    #[test]
    fn accepts_matching_baseline_delta_and_acks_it() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(1, &[]));
        client.update(TICK);
        assert_eq!(expect_ack(&mut peer, &mut crypto).tick, TickId::from_raw(1));

        send_sealed(
            &mut peer,
            &mut crypto,
            &delta_payload(Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(2),
                baseline_tick: Some(TickId::from_raw(1)),
                is_keyframe: false,
                systems: Vec::new(),
            }),
        );
        client.update(2 * TICK);

        assert_eq!(client.last_applied_tick(), TickId::from_raw(2));
        assert_eq!(expect_ack(&mut peer, &mut crypto).tick, TickId::from_raw(2));
    }

    #[test]
    fn re_announces_its_baseline_for_any_mismatched_delta() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(5, &[]));
        client.update(TICK);
        assert_eq!(expect_ack(&mut peer, &mut crypto).tick, TickId::from_raw(5));

        // The server delta-encodes against tick 9, which this client never
        // reached. Before, the client dropped this in silence forever; the
        // baseline it does hold has to be re-announced or nothing recovers.
        send_sealed(
            &mut peer,
            &mut crypto,
            &delta_payload(Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(10),
                baseline_tick: Some(TickId::from_raw(9)),
                is_keyframe: false,
                systems: Vec::new(),
            }),
        );
        client.update(2 * TICK);

        assert_eq!(client.last_applied_tick(), TickId::from_raw(5));
        assert_eq!(expect_ack(&mut peer, &mut crypto).tick, TickId::from_raw(5));

        // An older mismatched baseline re-announces too.
        send_sealed(
            &mut peer,
            &mut crypto,
            &delta_payload(Delta {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(11),
                baseline_tick: Some(TickId::from_raw(4)),
                is_keyframe: false,
                systems: Vec::new(),
            }),
        );
        client.update(3 * TICK);
        assert_eq!(expect_ack(&mut peer, &mut crypto).tick, TickId::from_raw(5));
    }

    #[test]
    fn a_mismatched_delta_flood_produces_one_repair_ack() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(5, &[]));
        client.update(TICK);
        let _ = expect_ack(&mut peer, &mut crypto);

        for tick in 10..30 {
            send_sealed(
                &mut peer,
                &mut crypto,
                &delta_payload(Delta {
                    sector: SectorId::ZERO,
                    tick: TickId::from_raw(tick),
                    baseline_tick: Some(TickId::from_raw(9)),
                    is_keyframe: false,
                    systems: Vec::new(),
                }),
            );
        }
        client.update(2 * TICK);

        assert_eq!(expect_ack(&mut peer, &mut crypto).tick, TickId::from_raw(5));
        assert!(
            peer.recv().unwrap().is_none(),
            "20 mismatched deltas must not become 20 acks"
        );
    }

    #[test]
    fn stale_deltas_are_ignored() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(5, &[]));
        client.update(TICK);
        let _ = expect_ack(&mut peer, &mut crypto);

        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(3, &[]));
        client.update(2 * TICK);

        assert_eq!(client.last_applied_tick(), TickId::from_raw(5));
        assert!(peer.recv().unwrap().is_none());
    }

    #[test]
    fn reconstructs_same_tick_different_sector_values_independently() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
        let left = SectorId { x: 1, y: 0, z: 0 };
        let right = SectorId { x: 2, y: 0, z: 0 };
        client.set_subscribed_sectors([left, right]);

        for (sector, value) in [(left, 11u32), (right, 22u32)] {
            send_sealed(
                &mut peer,
                &mut crypto,
                &keyframe_snapshot_for_sector(sector, 7, &[(42, value.to_le_bytes().to_vec())]),
            );
        }
        client.update(TICK);

        let first = expect_ack(&mut peer, &mut crypto);
        let second = expect_ack(&mut peer, &mut crypto);
        assert_eq!(first.tick, TickId::from_raw(7));
        assert_eq!(second.tick, TickId::from_raw(7));
        assert_ne!(first.sector, second.sector);
        assert_eq!(client.baseline_entity_count_in(left), 1);
        assert_eq!(client.baseline_entity_count_in(right), 1);
        assert_ne!(
            client.baseline_state_hash_in(left),
            client.baseline_state_hash_in(right),
            "same tick and system id with distinct values must retain independent baselines"
        );
    }

    #[test]
    fn unknown_sectors_do_not_allocate_replication_state() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        for x in 1..=100 {
            send_sealed(
                &mut peer,
                &mut crypto,
                &keyframe_snapshot_for_sector(SectorId { x, y: 0, z: 0 }, 1, &[]),
            );
        }
        client.update(TICK);

        assert!(client.baselines.is_empty());
        assert!(client.current_frames.is_empty());
        assert!(client.prev_frames.is_empty());
        assert_eq!(client.processing_error_count(), 100);
        assert!(peer.recv().unwrap().is_none());
    }

    #[test]
    fn changing_subscriptions_drops_inactive_sector_state() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
        let sector = SectorId { x: 1, y: 0, z: 0 };
        client.set_subscribed_sectors([sector]);
        send_sealed(
            &mut peer,
            &mut crypto,
            &keyframe_snapshot_for_sector(sector, 1, &[]),
        );
        client.update(TICK);
        assert!(client.baselines.contains_key(&sector));

        client.set_subscribed_sectors([]);
        assert!(client.baselines.is_empty());
        assert!(client.current_frames.is_empty());
        assert!(client.prev_frames.is_empty());
    }

    #[test]
    fn an_unbounded_snapshot_flood_is_budgeted() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
        client.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 4,
            bytes_per_second: 1_024 * 1_024,
        });

        for tick in 1..=32 {
            send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(tick, &[]));
        }
        client.update(TICK);

        assert_eq!(
            client.last_applied_tick(),
            TickId::from_raw(4),
            "the client must stop draining when its budget is spent"
        );
        assert_eq!(client.rate_limited_message_count(), 1);
    }

    // ── Interpolation buffer ───────────────────────────────────────────────

    #[test]
    fn newer_snapshot_slides_buffer_and_older_does_not() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        send_sealed(
            &mut peer,
            &mut crypto,
            &keyframe_snapshot(1, &[(0, vec![1, 0, 0, 0])]),
        );
        client.update(TICK);
        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(1))"));
        assert!(debug.contains("prev_snapshot_tick: None"));

        send_sealed(
            &mut peer,
            &mut crypto,
            &keyframe_snapshot(2, &[(0, vec![2, 0, 0, 0])]),
        );
        client.update(2 * TICK);
        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(2))"));
        assert!(debug.contains("prev_snapshot_tick: Some(TickId(1))"));

        send_sealed(
            &mut peer,
            &mut crypto,
            &keyframe_snapshot(1, &[(0, vec![9, 0, 0, 0])]),
        );
        client.update(3 * TICK);
        let debug = format!("{client:?}");
        assert!(debug.contains("current_snapshot_tick: Some(TickId(2))"));
    }

    #[test]
    fn baseline_entity_count_tracks_applied_snapshot() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        let mut data = Vec::new();
        for i in 0u64..3 {
            crcbl_net::encode_entity_entry(&mut data, i, &((i * 10) as u32).to_le_bytes());
        }
        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(5, &[(1, data)]));
        client.update(TICK);

        assert_eq!(client.baseline_entity_count(), 3);
        assert_eq!(client.baseline_system_count(), 1);
    }

    // ── Interpolation alpha ────────────────────────────────────────────────

    #[test]
    fn alpha_falls_back_to_the_local_clock_without_two_snapshots() {
        let (transport, _peer) = InMemoryTransport::pair();
        let mut client = client(transport);

        assert!((client.update(TICK) - 0.0).abs() < 0.01);
        client.update(Duration::ZERO);
        let alpha = client.update(Duration::from_nanos(8_333_333));
        assert!((alpha - 0.5).abs() < 0.01, "expected ~0.5, got {alpha}");
    }

    #[test]
    fn alpha_spans_the_buffered_snapshot_ticks_not_the_local_tick() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        // Two snapshots four server ticks apart — what packet loss, or a
        // server ticking slower than this client, produces.
        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(10, &[]));
        client.update(Duration::ZERO);
        send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(14, &[]));
        let alpha = client.update(Duration::ZERO);
        assert!(
            alpha.abs() < 1e-6,
            "playback starts at the older snapshot, got {alpha}"
        );

        // One local tick of wall time covers one server tick, so a quarter of
        // the four-tick span — not a whole one, which is what re-lerping the
        // same pair against the local frame clock produced.
        let alpha = client.update(TICK);
        assert!(
            (alpha - 0.25).abs() < 0.02,
            "expected ~0.25 across a four-tick span, got {alpha}"
        );
        let alpha = client.update(3 * TICK);
        assert!((alpha - 0.75).abs() < 0.02, "expected ~0.75, got {alpha}");

        // Playback never runs past the newest snapshot it holds.
        let alpha = client.update(Duration::from_secs(10));
        assert!(
            (alpha - 1.0).abs() < 1e-6,
            "expected clamp to 1.0, got {alpha}"
        );
    }

    #[test]
    fn interpolate_returns_empty_when_no_snapshots() {
        let (transport, _peer) = InMemoryTransport::pair();
        let client = client(transport);
        assert!(client.interpolate(0.5).transforms.is_empty());
    }

    #[test]
    fn interpolate_skips_non_transform_payloads() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        for tick in [1, 2] {
            send_sealed(
                &mut peer,
                &mut crypto,
                &keyframe_snapshot(tick, &[(0, vec![1, 0, 0, 0])]),
            );
            client.update(Duration::from_nanos(tick));
        }

        assert!(client.interpolate(0.5).transforms.is_empty());
    }

    #[test]
    fn interpolate_lerps_transforms_between_snapshots() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        let entity_bits = (1u64 << 32) | 7;
        for (tick, x) in [(1u64, 0.0f64), (2, 10.0)] {
            let mut data = Vec::new();
            Transform::from_position(glam::DVec3::new(x, 1.0, -2.0)).encode(&mut data);
            let mut blob = Vec::new();
            crcbl_net::encode_entity_entry(&mut blob, entity_bits, &data);
            send_sealed(
                &mut peer,
                &mut crypto,
                &keyframe_snapshot(tick, &[(0, blob)]),
            );
            client.update(Duration::from_nanos(tick));
        }

        let state = client.interpolate(0.25);
        assert_eq!(state.transforms.len(), 1);
        let (bits, transform) = state.transforms[0];
        assert_eq!(bits, entity_bits);
        assert!((transform.position.x - 2.5).abs() < 1e-9);
        assert!((transform.position.y - 1.0).abs() < 1e-9);
        assert!((transform.position.z - -2.0).abs() < 1e-9);

        let newest = client.interpolate(1.0).transforms[0].1;
        assert!((newest.position.x - 10.0).abs() < 1e-9);
        let oldest = client.interpolate(0.0).transforms[0].1;
        assert!((oldest.position.x - 0.0).abs() < 1e-9);
    }

    #[test]
    fn interpolate_keeps_entities_present_in_only_one_snapshot() {
        let (client_transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(client_transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);

        let gone = (1u64 << 32) | 1;
        let appeared = (1u64 << 32) | 2;

        send_sealed(
            &mut peer,
            &mut crypto,
            &keyframe_snapshot(1, &[(0, transform_blob(&[(gone, 5.0)]))]),
        );
        client.update(Duration::ZERO);
        send_sealed(
            &mut peer,
            &mut crypto,
            &keyframe_snapshot(2, &[(0, transform_blob(&[(appeared, 7.0)]))]),
        );
        client.update(Duration::from_nanos(1));

        let state = client.interpolate(0.5);
        assert_eq!(state.transforms.len(), 2);
        assert_eq!(state.transforms[0].0, gone);
        assert!((state.transforms[0].1.position.x - 5.0).abs() < 1e-9);
        assert_eq!(state.transforms[1].0, appeared);
        assert!((state.transforms[1].1.position.x - 7.0).abs() < 1e-9);
    }

    // ── Debug ──────────────────────────────────────────────────────────────

    #[test]
    fn debug_format() {
        let (transport, _peer) = InMemoryTransport::pair();
        let client = client(transport);
        let s = format!("{client:?}");
        assert!(s.contains("Client"));
        assert!(s.contains("connected"));
    }
}
