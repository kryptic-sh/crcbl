//! Network condition simulator for testing.
//!
//! Wraps any [`Transport`] and introduces configurable
//! impairments: packet loss, latency, jitter, duplication, and reordering.
//! Uses a deterministic LCG-based RNG so the same seed always produces the
//! same impairment pattern.
//!
//! # The impairments are the wire's; the guarantees are the transport's
//!
//! [`Transport::send_reliable`] promises ordered, lossless delivery, so a
//! simulator that dropped, duplicated or permuted what was handed to it would
//! be breaking the contract of the trait it implements. The layering here is
//! the one a real stack has: a lossy wire underneath, and on top of it the
//! reliability the transport being stood in for would provide. Loss on that
//! channel therefore costs a retransmission rather than the message, nothing
//! is duplicated because a reliability layer de-duplicates, and nothing is
//! reordered because an ordered channel holds a message until the one in front
//! of it has gone. No acknowledgement protocol is needed for any of it: the
//! simulator is the thing that decided to drop the packet, so it already knows
//! which one to send again.
//!
//! [`Transport::send_unreliable`] is where every impairment applies
//! undiminished, and is the channel [`SimConditions`] is really about.
//!
//! # Loss reproduces exactly; latency needs a clock
//!
//! Loss, duplication and the reorder window are drawn at send time from the
//! seeded LCG, so which messages they fall on is a pure function of the seed.
//! What they then cost is not: latency, jitter and a reliable retransmission
//! all leave a message due at an *instant*, and the simulator has to ask
//! something what time it is.
//!
//! That question goes through [`Clock`]. The default is [`SystemClock`], which
//! reads the wall clock and makes a latency test spend real seconds and depend
//! on how evenly the driving loop happened to be spaced. [`ManualClock`] is the
//! alternative: a handle a test advances by hand, cloned so that both ends of a
//! link share one time, which makes a latency run instant and exactly
//! reproducible. See [`ConditionSimulator::with_clock`].

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{Message, MessageKind, Transport, TransportError};

// ── Clock ────────────────────────────────────────────────────────────────────

/// The time source a [`ConditionSimulator`] schedules delayed messages against.
///
/// Time is a [`Duration`] since the clock's own epoch rather than an [`Instant`]
/// so that a hand-driven clock is plain arithmetic with nothing to overflow, and
/// so that a caller already speaking simulated time — `Server::update` and
/// `Client::update` both take a `Duration` — can hand the same value to both.
///
/// **Monotonic.** An implementation that goes backwards would make a message
/// already forwarded become pending again; nothing here defends against it.
///
/// `Send` because [`Transport`] is: a simulator holding a clock has to be as
/// movable between threads as the transport it wraps.
pub trait Clock: Send {
    /// Time since this clock's epoch.
    fn now(&self) -> Duration;
}

/// The wall clock, and what a simulator gets when no other is named.
#[derive(Clone, Debug)]
pub struct SystemClock {
    start: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl SystemClock {
    /// A clock whose epoch is now.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
}

/// A clock a test drives by hand, in nanoseconds since zero.
///
/// **Clones share one time**, which is the point: a link has two ends and each
/// gets its own [`ConditionSimulator`], so both are handed clones of one clock
/// and a single [`advance`](Self::advance) moves the whole link. A test that
/// gave each end its own clock would have two directions running on two
/// timelines.
///
/// Nanoseconds in a `u64` reach a little past five hundred years, and
/// [`advance`](Self::advance) saturates rather than wrapping, so the clock can
/// never run backwards however absurd the argument.
#[derive(Clone, Debug, Default)]
pub struct ManualClock {
    nanos: Arc<AtomicU64>,
}

impl ManualClock {
    /// A clock reading zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves every handle to this clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        let by = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        // `fetch_update` rather than `fetch_add`: an add wraps in release
        // builds, and a clock that wraps to zero would make every pending
        // message due at once — the opposite of what the caller asked for.
        let _ = self
            .nanos
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |now| {
                Some(now.saturating_add(by))
            });
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::Relaxed))
    }
}

// ── SimConditions ────────────────────────────────────────────────────────────

/// Configuration for the [`ConditionSimulator`] transport wrapper.
///
/// **Read every knob as describing the unreliable channel.** The reliable one
/// is ordered and lossless — see this module's header — so it converts loss
/// into delay and ignores duplication and reordering outright. Each field says
/// what it does on each side.
///
/// All fields are `pub` so conditions can be constructed with struct literal
/// syntax or mutated on the fly.
#[derive(Debug, Clone)]
pub struct SimConditions {
    /// Probability of the wire eating each message (0.0 = none, 1.0 = all).
    ///
    /// **Unreliable channel:** the message is gone. **Reliable channel:** the
    /// draw costs a retransmission instead, so the message arrives late rather
    /// than never, and a link that keeps eating it eventually reports
    /// [`TransportError::Disconnected`] — see `RELIABLE_RETRANSMIT_TIMEOUT`
    /// and `MAX_RELIABLE_RETRANSMISSIONS`.
    pub loss_rate: f64,
    /// Fixed delay added to each message, on both channels.
    pub latency: Duration,
    /// Random ±jitter added to the latency, on both channels.
    ///
    /// **Reliable channel:** it delays without reordering. Two draws of
    /// different sizes would otherwise let the second message overtake the
    /// first, so a reliable message is never released before its predecessor.
    pub jitter: Duration,
    /// Probability of duplicating each message.
    ///
    /// **Unreliable channel only:** a reliability layer de-duplicates, so the
    /// draw is not taken for [`Transport::send_reliable`] at all.
    pub duplicate_rate: f64,
    /// If greater than one, shuffle within runs of this many consecutive ready
    /// messages before they are forwarded to the inner transport.
    ///
    /// A message can therefore move at most `reorder_window - 1` places, which
    /// is what makes a scripted reorder test reproduce a bounded depth rather
    /// than an arbitrary permutation of everything that happened to be ready.
    ///
    /// **Unreliable channel only:** reliable messages keep the slots they were
    /// due in and the shuffle happens around them, because that channel
    /// delivers in the order it was handed.
    pub reorder_window: usize,
    /// Seed for the deterministic LCG RNG.
    pub seed: u64,
}

impl Default for SimConditions {
    fn default() -> Self {
        Self {
            loss_rate: 0.0,
            latency: Duration::ZERO,
            jitter: Duration::ZERO,
            duplicate_rate: 0.0,
            reorder_window: 0,
            seed: 0,
        }
    }
}

// ── ConditionSimulator ───────────────────────────────────────────────────────

/// Largest delay the simulator will schedule. See
/// [`ConditionSimulator::compute_delay`].
const MAX_SIMULATED_DELAY: Duration = Duration::from_secs(3600);

/// What the first retransmission of a lost reliable message waits, doubling on
/// each further loss.
///
/// **This is ENet's number and ENet's mechanism**, which is the ecosystem
/// answer closest to what this simulator stands in for: a game transport with
/// a reliable channel beside an unreliable one.
/// `ENET_PEER_DEFAULT_ROUND_TRIP_TIME` is the round-trip timeout it gives a
/// peer before any RTT sample exists, and `enet_protocol_check_timeouts` does
/// `outgoingCommand->roundTripTimeout *= 2` before requeueing a command that
/// went unacknowledged. QUIC (RFC 9002) has the same shape from a longer
/// start — a `kInitialRtt` of 333 ms yields a first PTO near TCP's 1 s initial
/// RTO — and doubles its PTO on each consecutive firing.
///
/// A sample is what this cannot have: the simulator is told a `latency`, not
/// an RTT, and the caller may configure one that has nothing to do with how
/// long an acknowledgement would take. The no-sample value is therefore the
/// honest one to use.
const RELIABLE_RETRANSMIT_TIMEOUT: Duration = Duration::from_millis(500);

/// Retransmissions a reliable message gets before the link is reported dead.
///
/// **Not a delivery policy — a diagnosis.** Past this the simulator returns
/// [`TransportError::Disconnected`], which is what a reliable transport says
/// when the far end has gone, not a message it decided to give up on.
///
/// ENet gives up when a command's doublings reach `ENET_PEER_TIMEOUT_LIMIT`
/// (32, so five of them) and at least `ENET_PEER_TIMEOUT_MINIMUM` has passed;
/// the same count of doublings from `RELIABLE_RETRANSMIT_TIMEOUT` spends a
/// total that lands between that minimum and `ENET_PEER_TIMEOUT_MAXIMUM`, so
/// the simulated link concludes the same thing after about the same wait.
///
/// The simulator itself does not go down with the verdict:
/// [`Transport::is_connected`] still reports the transport underneath, so a
/// caller that treats the error as it would any other and tries again gets a
/// fresh set of draws rather than a wrapper that has latched shut.
///
/// A cap is also what keeps the draw loop finite: `loss_rate` may be 1.0, and
/// re-rolling until a message survives would never return.
const MAX_RELIABLE_RETRANSMISSIONS: u32 = 5;

struct PendingMessage {
    msg: Message,
    /// When this message becomes due, on the simulator's [`Clock`].
    release_at: Duration,
}

/// A [`Transport`] wrapper that applies configurable network impairments.
///
/// Messages sent through the simulator are buffered internally. Their release
/// to the inner transport is governed by the configured latency, jitter, loss,
/// duplication, and reordering rules. Draining of ready messages happens on
/// every `send_*` and `recv` call so that messages eventually flow even in
/// send-only or recv-only usage.
pub struct ConditionSimulator<T: Transport, C: Clock = SystemClock> {
    inner: T,
    conditions: SimConditions,
    pending: Vec<PendingMessage>,
    /// Messages that are due (release_at <= now) but not yet forwarded.
    ready: Vec<Message>,
    /// Release time of the most recently enqueued reliable message, which no
    /// later one may be released before. See [`ConditionSimulator::enqueue`].
    last_reliable_release: Duration,
    clock: C,
}

impl<T: Transport> ConditionSimulator<T, SystemClock> {
    /// Create a new simulator wrapping `inner` with the given `conditions`.
    pub fn new(inner: T, conditions: SimConditions) -> Self {
        Self::with_clock(inner, conditions, SystemClock::new())
    }

    /// Convenience constructor: only packet loss.
    pub fn with_loss(inner: T, loss_rate: f64, seed: u64) -> Self {
        Self::new(
            inner,
            SimConditions {
                loss_rate,
                seed,
                ..Default::default()
            },
        )
    }

    /// Convenience constructor: only latency + jitter.
    pub fn with_latency(inner: T, latency: Duration, jitter: Duration, seed: u64) -> Self {
        Self::new(
            inner,
            SimConditions {
                latency,
                jitter,
                seed,
                ..Default::default()
            },
        )
    }
}

impl<T: Transport, C: Clock> ConditionSimulator<T, C> {
    /// The same, scheduling against `clock` instead of the wall clock.
    ///
    /// A [`ManualClock`] here is what makes a latency run instant and exactly
    /// reproducible; see this module's header. Both ends of a link must be
    /// given clones of **one** clock, or the two directions run on two
    /// timelines.
    pub fn with_clock(inner: T, conditions: SimConditions, clock: C) -> Self {
        Self {
            inner,
            conditions,
            pending: Vec::new(),
            ready: Vec::new(),
            last_reliable_release: Duration::ZERO,
            clock,
        }
    }

    /// Replace the current conditions.
    pub fn set_conditions(&mut self, conditions: SimConditions) {
        self.conditions = conditions;
    }

    /// Borrow the current conditions.
    pub fn conditions(&self) -> &SimConditions {
        &self.conditions
    }

    /// Consume the simulator and return the inner transport.
    ///
    /// Any messages still buffered are **lost**.
    pub fn into_inner(self) -> T {
        self.inner
    }

    // ── private helpers ──────────────────────────────────────────────────

    /// Advance the LCG and return the next `u64`.
    fn next_u64(&mut self) -> u64 {
        self.conditions.seed = self
            .conditions
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        self.conditions.seed
    }

    /// Return a deterministic `f64` in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }

    /// Compute the per-message delay: latency ± jitter, clamped to
    /// `[0, MAX_SIMULATED_DELAY]`.
    ///
    /// The upper clamp is load-bearing, not cosmetic: `Duration::from_secs_f64`
    /// panics on a non-finite or out-of-range value and `Instant + Duration`
    /// panics on overflow, both reachable from a `latency + jitter` a caller
    /// is free to configure. A simulated delay past the clamp is a
    /// configuration error, and a clamp is a better answer than a crash.
    fn compute_delay(&mut self) -> Duration {
        let latency = self.conditions.latency.min(MAX_SIMULATED_DELAY);
        if self.conditions.jitter.is_zero() {
            return latency;
        }
        let base = latency.as_secs_f64();
        let jitter = self
            .conditions
            .jitter
            .min(MAX_SIMULATED_DELAY)
            .as_secs_f64();
        let r = self.next_f64(); // [0, 1)
        let offset = (r - 0.5) * 2.0 * jitter; // [-jitter, +jitter]
        let total = base + offset;
        if total <= 0.0 {
            return Duration::ZERO;
        }
        Duration::try_from_secs_f64(total)
            .unwrap_or(latency)
            .min(MAX_SIMULATED_DELAY)
    }

    /// Draw for the loss of a reliable message and return what its
    /// retransmissions cost it.
    ///
    /// A reliable channel does not drop, so a message the wire ate is sent
    /// again rather than discarded, and the new attempt is drawn for in turn —
    /// a link that keeps eating it keeps pushing it further out, doubling the
    /// wait as ENet doubles a command's `roundTripTimeout`.
    ///
    /// # Errors
    ///
    /// [`TransportError::Disconnected`] once every attempt
    /// [`MAX_RELIABLE_RETRANSMISSIONS`] allows has been eaten: a link that
    /// cannot deliver is a dead link, not an unbounded retry.
    fn reliable_retransmit_delay(&mut self) -> Result<Duration, TransportError> {
        let mut delay = Duration::ZERO;
        let mut backoff = RELIABLE_RETRANSMIT_TIMEOUT;
        // One draw for the first transmission and one for each retransmission
        // it is allowed; surviving any of them is the message getting through.
        for _ in 0..=MAX_RELIABLE_RETRANSMISSIONS {
            if self.next_f64() >= self.conditions.loss_rate {
                return Ok(delay);
            }
            delay = delay.saturating_add(backoff);
            backoff = backoff.saturating_mul(2);
        }
        Err(TransportError::Disconnected)
    }

    /// Push a message into the internal buffer with a future release time.
    ///
    /// `retransmit` is what the message has already spent being sent again,
    /// and is [`Duration::ZERO`] for anything on the unreliable channel.
    fn enqueue(&mut self, msg: Message, retransmit: Duration) {
        let delay = self.compute_delay().saturating_add(retransmit);
        // `saturating_add` because both halves are caller-configurable and a
        // `Duration + Duration` past the type's range panics.
        let mut release_at = self.clock.now().saturating_add(delay);
        if msg.kind == MessageKind::Reliable {
            // Head-of-line blocking, which is what an ordered channel actually
            // costs: a message whose jitter draw came out short waits for the
            // one in front of it instead of overtaking it, and a retransmission
            // holds up everything queued behind it. A receiver resequencing the
            // channel would spend the same wait.
            release_at = release_at.max(self.last_reliable_release);
            self.last_reliable_release = release_at;
        }
        self.pending.push(PendingMessage { msg, release_at });
    }

    /// Move all due pending messages into `self.ready`, optionally reorder,
    /// then forward them to the inner transport.
    ///
    /// Forwarding stops at the first message the inner transport refuses, and
    /// that message stays at the head of `self.ready` for the next drain. A
    /// latency setting turns a steady stream into a burst on release, so a
    /// bounded inner channel filling up is something this type *causes*;
    /// dropping there would inject loss the caller never configured. The
    /// refusal is returned rather than held: the simulator models the
    /// conditions it was given, and a broken wire underneath is not one.
    fn drain_pending(&mut self) -> Result<(), TransportError> {
        let now = self.clock.now();

        // Collect due messages in the order they were sent. `swap_remove`
        // would be cheaper, but it permutes both what it takes and what it
        // leaves behind, and the reliable channel promises the order it was
        // handed — a burst released together must come out as it went in.
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].release_at <= now {
                let pm = self.pending.remove(i);
                self.ready.push(pm.msg);
                // Don't increment — the next message has shifted into index i.
            } else {
                i += 1;
            }
        }

        // Apply reordering if configured, one window at a time so the
        // configured depth is the depth a message can actually move — and over
        // the unreliable messages alone. A reliable one keeps the slot it was
        // due in and the shuffle happens around it.
        let window = self.conditions.reorder_window;
        if window > 1 && self.ready.len() > 1 {
            let unreliable: Vec<usize> = self
                .ready
                .iter()
                .enumerate()
                .filter(|(_, msg)| msg.kind == MessageKind::Unreliable)
                .map(|(slot, _)| slot)
                .collect();
            for run in unreliable.chunks(window) {
                deterministic_shuffle(&mut self.ready, run, &mut self.conditions.seed);
            }
        }

        // Forward to inner transport. The clone is what lets a refused
        // message be retried: the send consumes it and the error does not
        // hand it back.
        let mut forwarded = 0;
        let mut refused = None;
        for msg in &self.ready {
            let sent = match msg.kind {
                MessageKind::Reliable => self.inner.send_reliable(msg.clone()),
                MessageKind::Unreliable => self.inner.send_unreliable(msg.clone()),
            };
            match sent {
                Ok(()) => forwarded += 1,
                Err(e) => {
                    refused = Some(e);
                    break;
                }
            }
        }
        self.ready.drain(..forwarded);

        match refused {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

// ── Transport impl ───────────────────────────────────────────────────────────

impl<T: Transport, C: Clock> Transport for ConditionSimulator<T, C> {
    fn send_reliable(&mut self, mut msg: Message) -> Result<(), TransportError> {
        if !self.inner.is_connected() {
            return Err(TransportError::Disconnected);
        }
        // Flush what is already due before accepting anything new. A refusal
        // from underneath reaches the caller only here, where it is
        // unambiguous backpressure: this message has not been taken, so
        // retrying it cannot duplicate it.
        self.drain_pending()?;

        // The channel the caller chose is the channel the message keeps: the
        // simulator re-routes buffered messages by `kind`, so `kind` has to be
        // set here rather than trusted from the caller.
        msg.kind = MessageKind::Reliable;

        // Loss on this channel is a retransmission and not a drop, and there
        // is no duplicate draw at all: what the caller is talking to is a
        // reliable transport over a lossy wire, and the layer that re-sends
        // what the wire ate is the same layer that de-duplicates. The error is
        // the far end being gone, not this message being given up on.
        let retransmit = self.reliable_retransmit_delay()?;

        self.enqueue(msg, retransmit);

        // A refusal here is a delay, not a loss: the message stays in
        // `self.ready` and every entry point drains before it does anything
        // else, so a channel that is still full refuses again on the next
        // call — and reports it there, where the caller has not yet handed
        // over anything to lose.
        let _ = self.drain_pending();
        Ok(())
    }

    fn send_unreliable(&mut self, mut msg: Message) -> Result<(), TransportError> {
        if !self.inner.is_connected() {
            return Err(TransportError::Disconnected);
        }
        self.drain_pending()?;

        msg.kind = MessageKind::Unreliable;

        if self.next_f64() < self.conditions.loss_rate {
            return Ok(());
        }

        let dup = self.next_f64() < self.conditions.duplicate_rate;
        let dup_msg = if dup { Some(msg.clone()) } else { None };

        self.enqueue(msg, Duration::ZERO);
        if let Some(m) = dup_msg {
            self.enqueue(m, Duration::ZERO);
        }

        // A refusal here is a delay, not a loss: the message stays in
        // `self.ready` and every entry point drains before it does anything
        // else, so a channel that is still full refuses again on the next
        // call — and reports it there, where the caller has not yet handed
        // over anything to lose.
        let _ = self.drain_pending();
        Ok(())
    }

    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        self.drain_pending()?;
        self.inner.recv_reliable()
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        self.drain_pending()?;
        self.inner.recv()
    }

    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }
}

impl<T: Transport, C: Clock> std::fmt::Debug for ConditionSimulator<T, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConditionSimulator")
            .field("conditions", &self.conditions)
            .field("pending_len", &self.pending.len())
            .field("ready_len", &self.ready.len())
            .field("inner_connected", &self.inner.is_connected())
            .finish()
    }
}

// ── Deterministic shuffle ────────────────────────────────────────────────────

/// Fisher-Yates shuffle over the given `positions` of `items`, driven by an
/// LCG — reproducible for a given seed, and leaving every position it was not
/// given exactly where it is.
///
/// Positions rather than a subslice because the reorder window permutes the
/// unreliable messages *around* the reliable ones, which are sitting in an
/// ordered queue that must come out as it went in.
///
/// The index comes from the LCG's **high** bits. A truncating LCG's low bit
/// `k` has period `2^(k+1)`, so `seed % (i + 1)` for small `i` is close to a
/// fixed alternating pattern — the shuffle would not shuffle.
fn deterministic_shuffle<T>(items: &mut [T], positions: &[usize], seed: &mut u64) {
    for i in (1..positions.len()).rev() {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = ((*seed >> 32) % (i as u64 + 1)) as usize;
        items.swap(positions[i], positions[j]);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IN_MEMORY_CHANNEL_CAPACITY, InMemoryTransport};
    use std::thread;

    // ── Helpers ──────────────────────────────────────────────────────────

    fn msg(payload: u8) -> Message {
        Message {
            kind: MessageKind::Reliable,
            payload: vec![payload],
        }
    }

    /// The same, on the channel the impairments apply to.
    fn unreliable_msg(payload: u8) -> Message {
        Message {
            kind: MessageKind::Unreliable,
            payload: vec![payload],
        }
    }

    /// Drain all available messages from a transport.
    fn drain_all<T: Transport>(t: &mut T) -> Vec<Message> {
        let mut out = Vec::new();
        while let Ok(Some(m)) = t.recv() {
            out.push(m);
        }
        out
    }

    // ── Loss ─────────────────────────────────────────────────────────────

    #[test]
    fn loss_rate_0_delivers_all() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(a, SimConditions::default());

        for i in 0..20 {
            sim.send_reliable(msg(i)).unwrap();
        }

        let received = drain_all(&mut b);
        assert_eq!(received.len(), 20);
    }

    #[test]
    fn loss_rate_1_drops_every_unreliable_message() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                loss_rate: 1.0,
                ..Default::default()
            },
        );

        for i in 0..20 {
            sim.send_unreliable(unreliable_msg(i)).unwrap();
        }

        let received = drain_all(&mut b);
        assert_eq!(received.len(), 0);
    }

    /// **Loss on the reliable channel is delay, not loss.**
    ///
    /// [`Transport::send_reliable`] promises delivery, so the draw that would
    /// have eaten the message schedules another attempt instead. Every message
    /// handed over arrives, whatever the wire ate on the way.
    #[test]
    fn a_reliable_message_the_link_eats_is_retransmitted_rather_than_dropped() {
        let (a, mut b) = InMemoryTransport::pair();
        let clock = ManualClock::new();
        let mut sim = ConditionSimulator::with_clock(
            a,
            SimConditions {
                loss_rate: 0.5,
                seed: 42,
                ..Default::default()
            },
            clock.clone(),
        );

        let n = 20u8;
        for i in 0..n {
            sim.send_reliable(msg(i)).unwrap();
        }

        // The draws have to have fired, or this would pass just as well over a
        // link that impairs nothing: at half loss some of the twenty are
        // waiting on a retransmission and cannot be here yet.
        let mut arrived: Vec<u8> = drain_all(&mut b).iter().map(|m| m.payload[0]).collect();
        assert!(
            arrived.len() < usize::from(n),
            "all {n} messages went straight through a link losing half of what \
             it carries, so nothing was retransmitted and nothing is under test",
        );

        // Past the last doubling — the sum of the budget's waits is less than
        // the one that would come after it — everything is through.
        clock.advance(RELIABLE_RETRANSMIT_TIMEOUT * (1 << (MAX_RELIABLE_RETRANSMISSIONS + 1)));
        sim.recv().unwrap();
        arrived.extend(drain_all(&mut b).iter().map(|m| m.payload[0]));
        assert_eq!(
            arrived,
            (0..n).collect::<Vec<u8>>(),
            "a message handed to the reliable channel was dropped or overtaken",
        );
    }

    /// **A link that cannot deliver is a dead link, not an unbounded retry.**
    ///
    /// The retransmissions are capped, so total loss reports the far end gone
    /// — which is what a reliable transport says when nothing it sends is ever
    /// acknowledged — rather than re-rolling until one draw survives, which at
    /// this loss rate never happens.
    #[test]
    fn a_link_that_eats_every_retransmission_reports_the_far_end_gone() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::with_loss(a, 1.0, 0);

        assert!(
            matches!(sim.send_reliable(msg(1)), Err(TransportError::Disconnected)),
            "a reliable send over a link that delivers nothing reported success",
        );
        assert!(
            b.recv().unwrap().is_none(),
            "the message the simulator gave up on was forwarded anyway",
        );
    }

    #[test]
    fn loss_rate_50_percent_approximate() {
        // 50 % loss on the channel loss applies to — verify we get some, but
        // not all, messages.
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                loss_rate: 0.5,
                seed: 42,
                ..Default::default()
            },
        );

        let n = 200;
        for i in 0..n {
            sim.send_unreliable(unreliable_msg(i as u8)).unwrap();
        }

        let received = drain_all(&mut b);
        let ratio = received.len() as f64 / n as f64;
        // Wide tolerance: randomness, not an exact test.
        assert!(ratio > 0.2, "too few messages arrived: {ratio:.2}");
        assert!(ratio < 0.8, "too many messages arrived: {ratio:.2}");
    }

    // ── Determinism ──────────────────────────────────────────────────────

    #[test]
    fn same_seed_same_loss_pattern() {
        let n = 100;
        let seed = 12345;

        let (a1, mut b1) = InMemoryTransport::pair();
        let mut sim1 = ConditionSimulator::new(
            a1,
            SimConditions {
                loss_rate: 0.3,
                seed,
                ..Default::default()
            },
        );
        for i in 0..n {
            sim1.send_unreliable(unreliable_msg(i as u8)).unwrap();
        }
        let r1: Vec<u8> = drain_all(&mut b1).iter().map(|m| m.payload[0]).collect();

        let (a2, mut b2) = InMemoryTransport::pair();
        let mut sim2 = ConditionSimulator::new(
            a2,
            SimConditions {
                loss_rate: 0.3,
                seed,
                ..Default::default()
            },
        );
        for i in 0..n {
            sim2.send_unreliable(unreliable_msg(i as u8)).unwrap();
        }
        let r2: Vec<u8> = drain_all(&mut b2).iter().map(|m| m.payload[0]).collect();

        assert_eq!(r1, r2, "same seed must yield identical loss pattern");
    }

    // ── Duplication ──────────────────────────────────────────────────────

    #[test]
    fn duplicate_rate_0_no_duplicates() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(a, SimConditions::default());

        sim.send_reliable(msg(1)).unwrap();
        let received = drain_all(&mut b);
        assert_eq!(received.len(), 1);
    }

    #[test]
    fn duplicate_rate_1_always_duplicates() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                duplicate_rate: 1.0,
                ..Default::default()
            },
        );

        sim.send_unreliable(unreliable_msg(7)).unwrap();
        sim.send_unreliable(unreliable_msg(8)).unwrap();

        let received = drain_all(&mut b);
        assert_eq!(received.len(), 4); // two original + two duplicates
    }

    /// A reliability layer de-duplicates, so the draw is not taken on the
    /// reliable channel at all: the rate that doubles every unreliable message
    /// leaves the reliable one carrying exactly what it was handed.
    #[test]
    fn the_reliable_channel_never_duplicates_however_high_the_rate() {
        let conditions = || SimConditions {
            duplicate_rate: 1.0,
            ..Default::default()
        };

        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(a, conditions());
        for i in 0..4u8 {
            sim.send_reliable(msg(i)).unwrap();
        }
        let payloads: Vec<u8> = drain_all(&mut b).iter().map(|m| m.payload[0]).collect();
        assert_eq!(
            payloads,
            (0..4u8).collect::<Vec<u8>>(),
            "the reliable channel delivered a message twice",
        );

        // The control: the same rate on the channel it applies to, so a pass
        // above cannot be the duplication machinery being broken everywhere.
        let (c, mut d) = InMemoryTransport::pair();
        let mut lossy = ConditionSimulator::new(c, conditions());
        for i in 0..4u8 {
            lossy.send_unreliable(unreliable_msg(i)).unwrap();
        }
        assert_eq!(
            drain_all(&mut d).len(),
            8,
            "the duplicate rate did nothing on the channel it applies to",
        );
    }

    // ── Latency ──────────────────────────────────────────────────────────

    #[test]
    fn a_message_under_latency_does_not_arrive_until_the_delay_has_passed() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                latency: Duration::from_millis(50),
                seed: 0,
                ..Default::default()
            },
        );

        sim.send_reliable(msg(1)).unwrap();

        // Immediately after send, the message should not have arrived.
        assert!(b.recv().unwrap().is_none());

        // Wait for the latency to expire, then trigger drain on sim.
        thread::sleep(Duration::from_millis(60));
        sim.recv().unwrap(); // drains pending → forwards to inner
        let received = drain_all(&mut b);
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].payload, vec![1]);
    }

    /// The same message, the same delay, and no wall time spent at all.
    ///
    /// **This is the test the one above cannot be.** A wall-clock latency test
    /// has to sleep past the delay, which makes it slow, and it passes or fails
    /// on how the machine happened to schedule it — the reason every latency
    /// measurement in this workspace was `#[ignore]`d. Driving the clock by
    /// hand makes the same question exact: at one nanosecond short of the delay
    /// the message is still held, and at the delay it is through.
    #[test]
    fn a_hand_driven_clock_holds_a_message_to_the_nanosecond() {
        let (a, mut b) = InMemoryTransport::pair();
        let clock = ManualClock::new();
        let latency = Duration::from_millis(50);
        let mut sim = ConditionSimulator::with_clock(
            a,
            SimConditions {
                latency,
                seed: 0,
                ..Default::default()
            },
            clock.clone(),
        );

        sim.send_reliable(msg(1)).unwrap();
        assert!(b.recv().unwrap().is_none(), "due at once at time zero");

        // One nanosecond short: `drain_pending` compares `release_at <= now`,
        // so this is the last reading at which the message must still be held.
        clock.advance(latency - Duration::from_nanos(1));
        sim.recv().unwrap();
        assert!(
            b.recv().unwrap().is_none(),
            "a message one nanosecond short of its release time was forwarded",
        );

        clock.advance(Duration::from_nanos(1));
        sim.recv().unwrap();
        let received = drain_all(&mut b);
        assert_eq!(received.len(), 1, "the message was due and did not arrive");
        assert_eq!(received[0].payload, vec![1]);
    }

    /// Cloned handles are one clock, which is what lets a link's two ends share
    /// a timeline.
    #[test]
    fn every_handle_to_a_manual_clock_reads_the_same_time() {
        let clock = ManualClock::new();
        let other = clock.clone();
        assert_eq!(clock.now(), Duration::ZERO);

        clock.advance(Duration::from_millis(7));
        assert_eq!(other.now(), Duration::from_millis(7));

        other.advance(Duration::from_millis(3));
        assert_eq!(clock.now(), Duration::from_millis(10));
    }

    /// A clock cannot be driven backwards, however absurd the argument.
    ///
    /// `fetch_add` would wrap here and make every pending message due at once,
    /// which is the opposite of what a caller asking for a long delay wants.
    #[test]
    fn advancing_a_manual_clock_past_its_range_saturates_rather_than_wrapping() {
        let clock = ManualClock::new();
        clock.advance(Duration::from_secs(u64::MAX));
        let far = clock.now();
        assert!(far > Duration::from_secs(500 * 365 * 24 * 3600));

        clock.advance(Duration::from_secs(u64::MAX));
        assert!(
            clock.now() >= far,
            "the clock went backwards: {far:?} then {:?}",
            clock.now(),
        );
    }

    /// **Jitter delays a reliable message; it never lets one overtake another.**
    ///
    /// Two draws of different sizes would otherwise make the second message due
    /// before the first, which is reordering on a channel that promised none.
    /// The clamp that stops it is head-of-line blocking, and the cost is the
    /// later message waiting for the earlier one.
    ///
    /// **Draining a step at a time is what makes this a question.** Everything
    /// that comes due together leaves in the order it was queued whatever its
    /// release time, so one drain at the end would pass over a simulator that
    /// had no clamp at all.
    #[test]
    fn jitter_never_releases_a_reliable_message_before_the_one_in_front_of_it() {
        let latency = Duration::from_millis(50);
        let jitter = Duration::from_millis(40);
        let (a, mut b) = InMemoryTransport::pair();
        let clock = ManualClock::new();
        let mut sim = ConditionSimulator::with_clock(
            a,
            SimConditions {
                latency,
                jitter,
                seed: 123,
                ..Default::default()
            },
            clock.clone(),
        );

        let n = 16u8;
        for i in 0..n {
            sim.send_reliable(msg(i)).unwrap();
        }

        // A millisecond at a time, out to the furthest the jitter can push a
        // message, so every release the draws chose is observed where it falls.
        let step = Duration::from_millis(1);
        let mut arrived = Vec::new();
        let mut elapsed = Duration::ZERO;
        while elapsed <= latency + jitter {
            clock.advance(step);
            elapsed += step;
            sim.recv().unwrap();
            arrived.extend(drain_all(&mut b).iter().map(|m| m.payload[0]));
        }

        assert_eq!(
            arrived,
            (0..n).collect::<Vec<u8>>(),
            "jitter permuted the ordered channel",
        );
    }

    #[test]
    fn zero_latency_immediate_delivery() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(a, SimConditions::default());

        sim.send_reliable(msg(1)).unwrap();

        let received = drain_all(&mut b);
        assert_eq!(received.len(), 1);
    }

    // ── Reorder ──────────────────────────────────────────────────────────

    #[test]
    fn reorder_window_0_preserves_order() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                reorder_window: 0,
                seed: 99,
                ..Default::default()
            },
        );

        for i in 0..10 {
            sim.send_reliable(msg(i as u8)).unwrap();
        }

        let received = drain_all(&mut b);
        let payloads: Vec<u8> = received.iter().map(|m| m.payload[0]).collect();
        assert_eq!(payloads, (0..10).collect::<Vec<u8>>());
    }

    #[test]
    fn reorder_window_can_reorder() {
        // With a large reorder window, latency, and a fixed seed, messages
        // can arrive out of order. We check that the received sequence is not
        // strictly ascending — a sufficient condition for reordering.
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                reorder_window: 10,
                latency: Duration::from_millis(20),
                seed: 123,
                ..Default::default()
            },
        );

        for i in 0..10 {
            sim.send_unreliable(unreliable_msg(i as u8)).unwrap();
        }

        // All messages are buffered with latency — none delivered yet.
        assert!(b.recv().unwrap().is_none());

        // Wait for latency then drain.
        thread::sleep(Duration::from_millis(50));
        sim.recv().unwrap(); // drain pending → forwards to inner
        let received = drain_all(&mut b);
        let payloads: Vec<u8> = received.iter().map(|m| m.payload[0]).collect();
        assert_eq!(received.len(), 10);
        let sorted: Vec<u8> = (0..10).collect();
        assert_ne!(
            payloads, sorted,
            "with seed 123, latency, and window 10, order should differ; got {payloads:?}"
        );
    }

    #[test]
    fn reorder_stays_inside_the_configured_window() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                reorder_window: 3,
                seed: 7,
                ..Default::default()
            },
        );

        // Stage the ready queue directly: the windowing is what is under test,
        // not the latency scheduling that would normally fill it. Unreliable,
        // because that is the only channel a window may permute — the reliable
        // one is ordered, and the test below is where that is pinned.
        sim.ready.extend((0..12u8).map(unreliable_msg));
        sim.drain_pending().unwrap();

        let payloads: Vec<u8> = drain_all(&mut b).iter().map(|m| m.payload[0]).collect();
        assert_eq!(payloads.len(), 12);
        assert_ne!(
            payloads,
            (0..12u8).collect::<Vec<u8>>(),
            "a window of 3 over 12 messages must actually reorder some of them"
        );
        for (position, &payload) in payloads.iter().enumerate() {
            let moved = position as i64 - i64::from(payload);
            assert!(
                moved.abs() < 3,
                "message {payload} moved {moved} places, past the window of 3: {payloads:?}"
            );
        }
    }

    /// **The window permutes what may be permuted and steps over the rest.**
    ///
    /// A reorder window is an impairment of the unreliable channel, so the
    /// reliable messages sharing the same ready queue come out of it in the
    /// order they went in while the unreliable ones around them are shuffled.
    #[test]
    fn a_reorder_window_shuffles_the_unreliable_messages_around_the_reliable_ones() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                reorder_window: 4,
                seed: 7,
                ..Default::default()
            },
        );

        // Staged directly, as above. Even payloads travel reliably and odd
        // ones do not, so one queue carries both kinds through one shuffle.
        let staged = 16u8;
        sim.ready.extend((0..staged).map(|i| {
            if i % 2 == 0 {
                msg(i)
            } else {
                unreliable_msg(i)
            }
        }));
        sim.drain_pending().unwrap();

        // The two kinds arrive on two queues, so they are read back on two —
        // the interleaving is the inner transport's to lose, the order within
        // each channel is not.
        let mut reliable = Vec::new();
        while let Ok(Some(m)) = b.recv_reliable() {
            reliable.push(m.payload[0]);
        }
        let unreliable: Vec<u8> = drain_all(&mut b).iter().map(|m| m.payload[0]).collect();

        assert_eq!(
            reliable,
            (0..staged).step_by(2).collect::<Vec<u8>>(),
            "a reorder window permuted the ordered channel",
        );

        let mut sorted = unreliable.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            (0..staged).skip(1).step_by(2).collect::<Vec<u8>>(),
            "the unreliable messages did not all arrive",
        );
        assert_ne!(
            unreliable, sorted,
            "a window of 4 over eight unreliable messages must reorder some of \
             them, or the shuffle is stepping over everything: {unreliable:?}",
        );
    }

    /// Latency turns a steady stream into a burst on release, so the simulator
    /// is what fills a bounded inner channel — and a message dropped there
    /// would be loss the caller never configured, arriving as if the wire had
    /// eaten it. It has to wait instead, and the refusal has to reach the
    /// caller at the next hand-off, where nothing has been taken yet.
    #[test]
    fn a_full_inner_channel_delays_a_message_and_refuses_the_next() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                seed: 0,
                ..Default::default()
            },
        );

        for i in 0..IN_MEMORY_CHANNEL_CAPACITY {
            sim.send_reliable(msg(i as u8))
                .expect("the inner channel still has room");
        }

        // One past capacity. The simulator takes it — it is holding, not
        // dropping — so the caller has nothing to retry.
        sim.send_reliable(msg(0xAA))
            .expect("a held message must not be reported as a failed send");

        // The next hand-off has nowhere to go, and says so rather than
        // stacking behind a queue that cannot move.
        assert!(
            matches!(
                sim.send_reliable(msg(0xBB)),
                Err(TransportError::Backpressure)
            ),
            "a send into a stalled queue reported success",
        );

        // Emptying the far end unblocks the held message: it was delayed.
        assert_eq!(drain_all(&mut b).len(), IN_MEMORY_CHANNEL_CAPACITY);
        sim.recv().expect("the channel has room again");
        let held: Vec<u8> = drain_all(&mut b).iter().map(|m| m.payload[0]).collect();
        assert_eq!(held, vec![0xAA], "the held message never arrived");
    }

    #[test]
    fn shuffle_uses_high_bits_so_short_runs_are_not_a_fixed_pattern() {
        // Taking the index from the LCG's low bits makes `items.swap` for
        // small `i` nearly periodic; over many independent 2-element runs the
        // shuffle would then produce one arrangement almost every time.
        let mut seed = 1u64;
        let mut swapped = 0usize;
        for _ in 0..64 {
            let mut pair = [0u8, 1];
            deterministic_shuffle(&mut pair, &[0, 1], &mut seed);
            if pair == [1, 0] {
                swapped += 1;
            }
        }
        assert!(
            (16..48).contains(&swapped),
            "64 two-element shuffles swapped {swapped} times; expected roughly half"
        );
    }

    #[test]
    fn absurd_latency_and_jitter_do_not_panic() {
        let (a, _b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                latency: Duration::MAX,
                jitter: Duration::MAX,
                seed: 5,
                ..Default::default()
            },
        );
        for i in 0..8u8 {
            sim.send_reliable(msg(i)).unwrap();
        }
        assert_eq!(sim.pending.len(), 8);
    }

    // ── Wrap InMemoryTransport ───────────────────────────────────────────

    #[test]
    fn wrap_inmemory_pair_one_end() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                loss_rate: 0.0,
                latency: Duration::from_millis(5),
                jitter: Duration::ZERO,
                duplicate_rate: 0.0,
                reorder_window: 0,
                seed: 1,
            },
        );

        sim.send_reliable(msg(42)).unwrap();
        sim.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: vec![99],
        })
        .unwrap();

        // Messages are delayed — not yet available.
        assert!(b.recv().unwrap().is_none());

        thread::sleep(Duration::from_millis(10));
        sim.recv().unwrap(); // drain pending → forwards to inner
        let received = drain_all(&mut b);
        assert_eq!(received.len(), 2);
    }

    #[test]
    fn wrap_inmemory_pair_both_ends() {
        let (a, b) = InMemoryTransport::pair();
        let mut sim_a = ConditionSimulator::new(a, SimConditions::default());
        let mut sim_b = ConditionSimulator::new(b, SimConditions::default());

        sim_a.send_reliable(msg(1)).unwrap();
        sim_b.send_reliable(msg(2)).unwrap();

        let from_b = sim_b.recv().unwrap().unwrap();
        let from_a = sim_a.recv().unwrap().unwrap();

        assert_eq!(from_b.payload, vec![1]);
        assert_eq!(from_a.payload, vec![2]);
    }

    // ── is_connected / disconnect ────────────────────────────────────────

    #[test]
    fn the_simulator_reports_the_connectedness_of_the_transport_it_wraps() {
        let (a, _b) = InMemoryTransport::pair();
        let sim = ConditionSimulator::new(a, SimConditions::default());
        assert!(sim.is_connected());
    }

    #[test]
    fn send_on_disconnected_inner() {
        let (mut a, _b) = InMemoryTransport::pair();
        a.disconnect();
        let mut sim = ConditionSimulator::new(a, SimConditions::default());
        let err = sim.send_reliable(msg(1)).unwrap_err();
        assert!(matches!(err, TransportError::Disconnected));
    }

    // ── set_conditions ───────────────────────────────────────────────────

    #[test]
    fn set_conditions_changes_behaviour() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                loss_rate: 1.0, // all dropped
                ..Default::default()
            },
        );

        sim.send_unreliable(unreliable_msg(1)).unwrap();
        assert!(b.recv().unwrap().is_none());

        sim.set_conditions(SimConditions {
            loss_rate: 0.0, // no loss
            ..Default::default()
        });
        sim.send_unreliable(unreliable_msg(2)).unwrap();
        let m = b.recv().unwrap().unwrap();
        assert_eq!(m.payload, vec![2]);
    }

    #[test]
    fn the_simulator_reports_back_the_conditions_it_was_built_with() {
        let (a, _b) = InMemoryTransport::pair();
        let sim = ConditionSimulator::new(
            a,
            SimConditions {
                loss_rate: 0.25,
                seed: 42,
                ..Default::default()
            },
        );
        assert!((sim.conditions().loss_rate - 0.25).abs() < f64::EPSILON);
        assert_eq!(sim.conditions().seed, 42);
    }

    // ── into_inner ───────────────────────────────────────────────────────

    #[test]
    fn into_inner_hands_back_a_transport_that_still_carries_messages() {
        let (a, mut b) = InMemoryTransport::pair();
        let sim = ConditionSimulator::new(a, SimConditions::default());
        let mut inner = sim.into_inner();

        // The returned value is the transport itself, still paired with `b`:
        // dropping it and checking nothing panicked said neither half.
        inner.send_reliable(msg(9)).unwrap();
        assert_eq!(
            drain_all(&mut b).first().map(|m| m.payload.clone()),
            Some(vec![9])
        );
        b.send_reliable(msg(10)).unwrap();
        assert_eq!(
            drain_all(&mut inner).first().map(|m| m.payload.clone()),
            Some(vec![10])
        );
    }

    // ── Debug ────────────────────────────────────────────────────────────

    #[test]
    fn the_debug_output_reports_the_conditions_and_what_is_still_buffered() {
        let (a, _b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                latency: Duration::from_millis(30),
                loss_rate: 0.25,
                // The seed decides the loss draw, and the default one draws
                // almost zero — which loses the message this test needs to
                // still be buffered.
                seed: 5,
                ..Default::default()
            },
        );
        // Delayed, so it is still pending when the line is formatted — which is
        // the state a `Debug` line is read for in the first place.
        sim.send_reliable(msg(1)).unwrap();

        let debug = format!("{sim:?}");
        for expected in [
            "ConditionSimulator",
            "loss_rate: 0.25",
            "latency: 30ms",
            "pending_len: 1",
            "ready_len: 0",
            "inner_connected: true",
        ] {
            assert!(
                debug.contains(expected),
                "{expected:?} missing from {debug}"
            );
        }
    }

    // ── Convenience constructors ─────────────────────────────────────────

    #[test]
    fn the_loss_only_constructor_applies_the_loss_rate_it_was_given() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::with_loss(a, 1.0, 0);
        sim.send_unreliable(unreliable_msg(1)).unwrap();
        assert!(b.recv().unwrap().is_none());
    }

    #[test]
    fn the_latency_only_constructor_delays_delivery_and_still_delivers() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::with_latency(
            a,
            Duration::from_millis(20),
            Duration::from_millis(5),
            0,
        );
        sim.send_reliable(msg(1)).unwrap();
        // Not yet delivered.
        assert!(b.recv().unwrap().is_none());
        thread::sleep(Duration::from_millis(50));
        sim.recv().unwrap(); // drain pending
        let received = drain_all(&mut b);
        assert_eq!(received.len(), 1);
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    #[test]
    fn neither_end_of_an_idle_simulator_produces_a_message() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(a, SimConditions::default());
        // Nothing sent — recv on the wrapped end should be None.
        assert!(sim.recv().unwrap().is_none());
        // The inner peer should also be empty.
        assert!(b.recv().unwrap().is_none());
    }

    #[test]
    fn an_unreliable_send_arrives_still_marked_unreliable() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(a, SimConditions::default());
        sim.send_unreliable(Message {
            kind: MessageKind::Unreliable,
            payload: vec![7],
        })
        .unwrap();
        let received = drain_all(&mut b);
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].kind, MessageKind::Unreliable);
    }

    /// Jitter larger than the latency pushes the delay to both sides of zero,
    /// and both sides are the simulator's to handle: a negative total is
    /// clamped and released at once, a positive one is held.
    ///
    /// This used to sleep for 50 ms and then assert nothing at all, on the
    /// grounds that the outcome depended on the draw — but the draw is an LCG
    /// the caller seeds, so the outcome is a function of the seed and both
    /// seeds below are picked for the side of zero they land on. Neither case
    /// waits for anything: the clamped one is due the moment it is enqueued,
    /// and the held one is due in seconds, so the assertion is what the
    /// simulator did rather than what it managed to do in 50 ms.
    #[test]
    fn a_negative_jittered_delay_is_clamped_and_released_while_a_positive_one_is_held() {
        // Both seeds run the same two draws — loss, then jitter, the
        // duplicate draw not being taken on this channel — so the seed is what
        // decides the sign of `(r - 0.5)` on the second.
        let jittered = |seed: u64| {
            let (a, mut b) = InMemoryTransport::pair();
            let mut sim = ConditionSimulator::new(
                a,
                SimConditions {
                    jitter: Duration::from_secs(10),
                    latency: Duration::ZERO,
                    seed,
                    ..Default::default()
                },
            );
            sim.send_reliable(msg(1)).unwrap();
            sim.recv().unwrap(); // a second drain, in case the first was early
            drain_all(&mut b)
        };

        let released = jittered(2);
        assert_eq!(released.len(), 1, "a delay below zero is due immediately");
        assert_eq!(released[0].payload, vec![1]);

        assert!(
            jittered(1).is_empty(),
            "a delay of seconds is not due yet, and nothing here waited for it"
        );
    }

    #[test]
    fn multi_send_before_drain() {
        // Send many messages with latency, then drain once.
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                latency: Duration::from_millis(30),
                seed: 0,
                ..Default::default()
            },
        );

        let n = 20;
        for i in 0..n {
            sim.send_reliable(msg(i as u8)).unwrap();
        }

        // None arrived yet.
        assert!(b.recv().unwrap().is_none());

        // Wait then drain.
        thread::sleep(Duration::from_millis(50));
        sim.recv().unwrap(); // triggers drain on the sender side
        let received = drain_all(&mut b);
        assert_eq!(received.len(), n);
    }
}
