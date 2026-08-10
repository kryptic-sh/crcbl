//! Network condition simulator for testing.
//!
//! Wraps any [`Transport`] and introduces configurable
//! impairments: packet loss, latency, jitter, duplication, and reordering.
//! Uses a deterministic LCG-based RNG so the same seed always produces the
//! same impairment pattern.

use std::time::{Duration, Instant};

use crate::{Message, MessageKind, Transport, TransportError};

// ── SimConditions ────────────────────────────────────────────────────────────

/// Configuration for the [`ConditionSimulator`] transport wrapper.
///
/// All fields are `pub` so conditions can be constructed with struct literal
/// syntax or mutated on the fly.
#[derive(Debug, Clone)]
pub struct SimConditions {
    /// Probability of dropping each message (0.0 = none, 1.0 = all).
    pub loss_rate: f64,
    /// Fixed delay added to each message.
    pub latency: Duration,
    /// Random ±jitter added to the latency.
    pub jitter: Duration,
    /// Probability of duplicating each message.
    pub duplicate_rate: f64,
    /// If greater than one, shuffle within runs of this many consecutive ready
    /// messages before they are forwarded to the inner transport.
    ///
    /// A message can therefore move at most `reorder_window - 1` places, which
    /// is what makes a scripted reorder test reproduce a bounded depth rather
    /// than an arbitrary permutation of everything that happened to be ready.
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

struct PendingMessage {
    msg: Message,
    release_at: Instant,
}

/// A [`Transport`] wrapper that applies configurable network impairments.
///
/// Messages sent through the simulator are buffered internally. Their release
/// to the inner transport is governed by the configured latency, jitter, loss,
/// duplication, and reordering rules. Draining of ready messages happens on
/// every `send_*` and `recv` call so that messages eventually flow even in
/// send-only or recv-only usage.
pub struct ConditionSimulator<T: Transport> {
    inner: T,
    conditions: SimConditions,
    pending: Vec<PendingMessage>,
    /// Messages that are due (release_at <= now) but not yet forwarded.
    ready: Vec<Message>,
}

impl<T: Transport> ConditionSimulator<T> {
    /// Create a new simulator wrapping `inner` with the given `conditions`.
    pub fn new(inner: T, conditions: SimConditions) -> Self {
        Self {
            inner,
            conditions,
            pending: Vec::new(),
            ready: Vec::new(),
        }
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

    /// Push a message into the internal buffer with a future release time.
    fn enqueue(&mut self, msg: Message) {
        let delay = self.compute_delay();
        let release_at = if delay.is_zero() {
            Instant::now()
        } else {
            Instant::now() + delay
        };
        self.pending.push(PendingMessage { msg, release_at });
    }

    /// Move all due pending messages into `self.ready`, optionally reorder,
    /// then forward them to the inner transport.
    fn drain_pending(&mut self) {
        let now = Instant::now();

        // Collect due messages via swap_remove — order of the *remaining*
        // (future-due) messages does not matter.
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].release_at <= now {
                let pm = self.pending.swap_remove(i);
                self.ready.push(pm.msg);
                // Don't increment — a new element now occupies index i.
            } else {
                i += 1;
            }
        }

        // Apply reordering if configured, one window at a time so the
        // configured depth is the depth a message can actually move.
        let window = self.conditions.reorder_window;
        if window > 1 && self.ready.len() > 1 {
            for run in self.ready.chunks_mut(window) {
                deterministic_shuffle(run, &mut self.conditions.seed);
            }
        }

        // Forward to inner transport.
        for msg in self.ready.drain(..) {
            match msg.kind {
                MessageKind::Reliable => {
                    let _ = self.inner.send_reliable(msg);
                }
                MessageKind::Unreliable => {
                    let _ = self.inner.send_unreliable(msg);
                }
            }
        }
    }
}

// ── Transport impl ───────────────────────────────────────────────────────────

impl<T: Transport> Transport for ConditionSimulator<T> {
    fn send_reliable(&mut self, mut msg: Message) -> Result<(), TransportError> {
        if !self.inner.is_connected() {
            return Err(TransportError::Disconnected);
        }
        // The channel the caller chose is the channel the message keeps: the
        // simulator re-routes buffered messages by `kind`, so `kind` has to be
        // set here rather than trusted from the caller.
        msg.kind = MessageKind::Reliable;

        // Loss check (before consuming the message).
        if self.next_f64() < self.conditions.loss_rate {
            self.drain_pending();
            return Ok(());
        }

        // Duplicate check (before consuming the message).
        let dup = self.next_f64() < self.conditions.duplicate_rate;
        let dup_msg = if dup { Some(msg.clone()) } else { None };

        self.enqueue(msg);
        if let Some(m) = dup_msg {
            self.enqueue(m);
        }

        self.drain_pending();
        Ok(())
    }

    fn send_unreliable(&mut self, mut msg: Message) -> Result<(), TransportError> {
        if !self.inner.is_connected() {
            return Err(TransportError::Disconnected);
        }
        msg.kind = MessageKind::Unreliable;

        if self.next_f64() < self.conditions.loss_rate {
            self.drain_pending();
            return Ok(());
        }

        let dup = self.next_f64() < self.conditions.duplicate_rate;
        let dup_msg = if dup { Some(msg.clone()) } else { None };

        self.enqueue(msg);
        if let Some(m) = dup_msg {
            self.enqueue(m);
        }

        self.drain_pending();
        Ok(())
    }

    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        self.drain_pending();
        self.inner.recv_reliable()
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        self.drain_pending();
        self.inner.recv()
    }

    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }
}

impl<T: Transport> std::fmt::Debug for ConditionSimulator<T> {
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

/// Fisher-Yates shuffle driven by an LCG — reproducible for a given seed.
///
/// The index comes from the LCG's **high** bits. A truncating LCG's low bit
/// `k` has period `2^(k+1)`, so `seed % (i + 1)` for small `i` is close to a
/// fixed alternating pattern — the shuffle would not shuffle.
fn deterministic_shuffle<T>(items: &mut [T], seed: &mut u64) {
    for i in (1..items.len()).rev() {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = ((*seed >> 32) % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryTransport;
    use std::thread;

    // ── Helpers ──────────────────────────────────────────────────────────

    fn msg(payload: u8) -> Message {
        Message {
            kind: MessageKind::Reliable,
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
    fn loss_rate_1_drops_all() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                loss_rate: 1.0,
                ..Default::default()
            },
        );

        for i in 0..20 {
            sim.send_reliable(msg(i)).unwrap();
        }

        let received = drain_all(&mut b);
        assert_eq!(received.len(), 0);
    }

    #[test]
    fn loss_rate_50_percent_approximate() {
        // 50 % loss — verify we get some, but not all, messages.
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
            sim.send_reliable(msg(i as u8)).unwrap();
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
            sim1.send_reliable(msg(i as u8)).unwrap();
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
            sim2.send_reliable(msg(i as u8)).unwrap();
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

        sim.send_reliable(msg(7)).unwrap();
        sim.send_reliable(msg(8)).unwrap();

        let received = drain_all(&mut b);
        assert_eq!(received.len(), 4); // two original + two duplicates
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
            sim.send_reliable(msg(i as u8)).unwrap();
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
        // not the latency scheduling that would normally fill it.
        sim.ready.extend((0..12u8).map(msg));
        sim.drain_pending();

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

    #[test]
    fn shuffle_uses_high_bits_so_short_runs_are_not_a_fixed_pattern() {
        // Taking the index from the LCG's low bits makes `items.swap` for
        // small `i` nearly periodic; over many independent 2-element runs the
        // shuffle would then produce one arrangement almost every time.
        let mut seed = 1u64;
        let mut swapped = 0usize;
        for _ in 0..64 {
            let mut pair = [0u8, 1];
            deterministic_shuffle(&mut pair, &mut seed);
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

        sim.send_reliable(msg(1)).unwrap();
        assert!(b.recv().unwrap().is_none());

        sim.set_conditions(SimConditions {
            loss_rate: 0.0, // no loss
            ..Default::default()
        });
        sim.send_reliable(msg(2)).unwrap();
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
    fn into_inner_returns_transport() {
        let (a, b) = InMemoryTransport::pair();
        let sim = ConditionSimulator::new(a, SimConditions::default());
        let inner = sim.into_inner();
        // b's sender should still be alive (inner still holds one end).
        drop(inner);
        drop(b);
    }

    // ── Debug ────────────────────────────────────────────────────────────

    #[test]
    fn debug_does_not_panic() {
        let (a, _b) = InMemoryTransport::pair();
        let sim = ConditionSimulator::new(a, SimConditions::default());
        let _ = format!("{sim:?}");
    }

    // ── Convenience constructors ─────────────────────────────────────────

    #[test]
    fn the_loss_only_constructor_applies_the_loss_rate_it_was_given() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::with_loss(a, 1.0, 0);
        sim.send_reliable(msg(1)).unwrap();
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

    #[test]
    fn jitter_does_not_panic() {
        let (a, mut b) = InMemoryTransport::pair();
        let mut sim = ConditionSimulator::new(
            a,
            SimConditions {
                jitter: Duration::from_secs(10), // huge jitter, may produce negative offset
                latency: Duration::ZERO,
                seed: 1,
                ..Default::default()
            },
        );
        // Should not panic even if jitter > latency (negative total clamped to 0).
        sim.send_reliable(msg(1)).unwrap();
        // Allow time for delivery.
        thread::sleep(Duration::from_millis(50));
        sim.recv().unwrap(); // drain pending
        let received = drain_all(&mut b);
        // Message may or may not be delivered depending on jitter, but no panic.
        let _ = received;
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
