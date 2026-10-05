//! The jitter buffer end to end: a client fed snapshots of one entity moving
//! along a straight line, over an in-memory link that a
//! [`ConditionSimulator`] on a [`ManualClock`] delays, jitters or cuts — so
//! every run is instant and reproduces exactly.

use std::time::Duration;

use crcbl_net::auth::SessionCrypto;
use crcbl_net::{
    ConditionSimulator, IN_MEMORY_CHANNEL_CAPACITY, InMemoryTransport, ManualClock, Message,
    SimConditions,
};

use super::*;
use crate::playout::{MAX_PLAYOUT_DELAY, MAX_PLAYOUT_RATE_DEVIATION, PLAYOUT_MARGIN, PlayoutStats};
use crate::tests::{
    COMPATIBILITY, TICK, client, connect, keyframe_snapshot, physics, transform_blob,
};

/// The entity every snapshot carries.
const ENTITY: u64 = (1 << 32) | 1;

/// Metres the entity moves per server tick, so its position names the tick
/// it was snapshotted at and stays well inside the quantized range.
const STEP: f64 = 0.01;

/// Slack on a per-frame playback rate, for the float arithmetic of a clock
/// running in `f64` server ticks.
const RATE_EPSILON: f64 = 1e-6;

/// What one rendered frame showed.
#[derive(Debug, Clone, Copy)]
struct Shown {
    playback: Option<f64>,
    /// The entity's interpolated position, once a snapshot holds it.
    x: Option<f64>,
    alpha: f32,
    stats: PlayoutStats,
}

/// A server sending snapshots over a conditioned link to a real client, one
/// server tick (and one rendered frame) per step.
struct Link {
    client: Client<InMemoryTransport>,
    server: ConditionSimulator<InMemoryTransport, ManualClock>,
    crypto: SessionCrypto,
    clock: ManualClock,
    /// One server tick, and one rendered frame.
    tick_duration: Duration,
    now: Duration,
    tick: u64,
    /// The newest tick sent, whether or not it arrived.
    sent: u64,
}

impl Link {
    fn new() -> Self {
        Self::at_tick_rate(60)
    }

    fn at_tick_rate(tick_hz: u32) -> Self {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new_with_compatibility(
            World::new(),
            transport,
            tick_hz,
            COMPATIBILITY,
            PlayerId::from_seed(115),
        );
        let crypto = connect(&mut client, &mut peer, Duration::ZERO);
        let clock = ManualClock::new();
        let server = ConditionSimulator::with_clock(peer, SimConditions::default(), clock.clone());
        Self {
            client,
            server,
            crypto,
            clock,
            tick_duration: Duration::from_secs(1) / tick_hz,
            now: Duration::ZERO,
            tick: 0,
            sent: 0,
        }
    }

    fn set_conditions(&mut self, conditions: SimConditions) {
        self.server.set_conditions(conditions);
    }

    /// One server tick: a snapshot when `interval` divides it, whatever the
    /// link lets through by now, and one client update.
    fn step(&mut self, interval: u64) -> Shown {
        self.now += self.tick_duration;
        self.clock.advance(self.tick_duration);
        self.tick += 1;
        if self.tick.is_multiple_of(interval) {
            let snapshot = keyframe_snapshot(
                self.tick,
                &[(
                    physics(),
                    transform_blob(&[(ENTITY, self.tick as f64 * STEP)]),
                )],
            );
            let sealed = self.crypto.seal(&snapshot).expect("counter space");
            self.server
                .send_unreliable(Message::unreliable(sealed))
                .expect("the link is up");
            self.sent = self.tick;
        }
        // Releases whatever has come due; what it reads are the client's acks.
        while self.server.recv().expect("the link is up").is_some() {}
        let alpha = self.client.update(self.now);
        Shown {
            playback: self.client.playback_tick(),
            x: self
                .client
                .interpolate(alpha)
                .transforms
                .iter()
                .find(|(bits, _)| *bits == ENTITY)
                .map(|(_, transform)| transform.position.x),
            alpha,
            stats: self.client.playout_stats(),
        }
    }

    fn run(&mut self, ticks: u64, interval: u64) -> Vec<Shown> {
        (0..ticks).map(|_| self.step(interval)).collect()
    }
}

/// How long a run takes to settle: long enough for every estimate to have
/// converged at [`playout::ESTIMATOR_GAIN`] from where it started.
const SETTLE_TICKS: u64 = 600;

/// The playout delay a perfectly steady stream at `interval` converges to.
fn steady_delay(interval: u64) -> Duration {
    TICK * u32::try_from(interval).expect("small") + PLAYOUT_MARGIN
}

fn assert_near(actual: Duration, expected: Duration, tolerance: Duration, what: &str) {
    assert!(
        actual.abs_diff(expected) <= tolerance,
        "{what}: {actual:?}, expected {expected:?} ± {tolerance:?}"
    );
}

/// Every frame's playback rate against the server's tick rate, as a
/// deviation: `0.0` is playback moving one server tick per tick. Frames
/// where playback held at the newest snapshot — an underrun — are left out.
fn rate_deviations(shown: &[Shown]) -> Vec<f64> {
    shown
        .windows(2)
        .filter(|pair| pair[1].stats.underruns == pair[0].stats.underruns)
        .filter_map(|pair| Some(pair[1].playback? - pair[0].playback? - 1.0))
        .collect()
}

fn max_rate_deviation(shown: &[Shown]) -> f64 {
    rate_deviations(shown)
        .into_iter()
        .map(f64::abs)
        .fold(0.0, f64::max)
}

fn assert_monotonic(shown: &[Shown]) {
    for pair in shown.windows(2) {
        let (Some(before), Some(after)) = (pair[0].x, pair[1].x) else {
            continue;
        };
        assert!(
            after >= before,
            "the entity moved backwards along its path: {before} then {after}"
        );
    }
}

/// **A steady stream at every snapshot interval the rate drop uses plays
/// without a single underrun**, at a delay of the interval plus
/// [`PLAYOUT_MARGIN`] — the slowest interval included — and playback moves
/// one server tick every tick.
#[test]
fn a_steady_stream_at_each_interval_never_runs_dry() {
    for interval in [1, 2, 3] {
        let mut link = Link::new();
        link.run(SETTLE_TICKS, interval);
        let settled = link.client.playout_stats();
        let shown = link.run(SETTLE_TICKS, interval);
        let last = shown.last().expect("frames").stats;

        assert_eq!(
            last.underruns, settled.underruns,
            "interval {interval} ran dry"
        );
        assert_eq!(last.steps, 0, "interval {interval} stepped");
        assert_near(
            last.delay,
            steady_delay(interval),
            Duration::from_micros(100),
            &format!("interval {interval}'s delay"),
        );
        assert!(
            max_rate_deviation(&shown) < RATE_EPSILON,
            "interval {interval}: playback ran off the tick rate by {}",
            max_rate_deviation(&shown)
        );
        assert_monotonic(&shown);
        // Each snapshot arrives the tick it is sent, so on the tick of the
        // newest playback is exactly the delay behind it.
        assert_eq!(link.tick, link.sent, "the run ends on a snapshot");
        let lag = link.sent as f64 - shown.last().expect("frames").playback.expect("playing");
        assert!(
            (lag - last.delay.div_duration_f64(TICK)).abs() < 1e-3,
            "interval {interval}: playback trails the newest by {lag} ticks, not the delay"
        );
        // Frames behind playback go as it passes them: what is left spans
        // the delay, plus the frame either side.
        let held = link.client.frames[&SectorId::ZERO].len() as f64;
        let spanned = last.delay.div_duration_f64(TICK) / interval as f64;
        assert!(
            held <= spanned.ceil() + 2.0,
            "interval {interval} holds {held} frames across a delay of {spanned} snapshots"
        );
    }
}

/// **The alpha spans the pair either side of playback in their server
/// ticks**, not one local tick: with snapshots four ticks apart, each local
/// tick moves it a quarter of the way.
#[test]
fn alpha_spans_the_buffered_snapshot_ticks_not_the_local_tick() {
    const INTERVAL: u64 = 4;
    let mut link = Link::new();
    link.run(SETTLE_TICKS, INTERVAL);
    let shown = link.run(4 * INTERVAL, INTERVAL);
    for pair in shown.windows(2) {
        let step = pair[1].alpha - pair[0].alpha;
        if step < 0.0 {
            // Playback crossed into the next pair.
            continue;
        }
        assert!(
            (step - 0.25).abs() < 1e-4,
            "one local tick moved the alpha by {step} across a four-tick span"
        );
    }
}

/// **Jitter grows the delay until no snapshot is late, and when the jitter
/// stops the delay shrinks back — gradually.** Snapshots are delayed by a
/// latency drawn within a bound (which also reorders them, and the client
/// drops whatever was overtaken). Once the delay has grown to cover the
/// spread nothing runs dry; when the link calms, playback speeds up to close
/// the extra delay, never faster or slower than
/// [`MAX_PLAYOUT_RATE_DEVIATION`] off the tick rate, and the delay returns to
/// the steady one. The entity never moves backwards along its path.
#[test]
fn jitter_grows_the_delay_without_underruns_and_calm_shrinks_it_gradually() {
    const JITTER: Duration = Duration::from_millis(40);
    let mut link = Link::new();
    link.set_conditions(SimConditions {
        latency: Duration::from_millis(60),
        jitter: JITTER,
        seed: 0x6A17_7E55,
        ..SimConditions::default()
    });
    let warming = link.run(SETTLE_TICKS, 1);
    let settled = link.client.playout_stats();
    let jittered = link.run(SETTLE_TICKS, 1);
    let peak = jittered.last().expect("frames").stats;
    assert_eq!(
        peak.underruns, settled.underruns,
        "a converged delay still ran dry"
    );
    assert!(
        peak.delay > steady_delay(1) + JITTER,
        "the delay {:?} did not grow to cover ±{JITTER:?} of jitter",
        peak.delay
    );

    link.set_conditions(SimConditions {
        latency: Duration::from_millis(60),
        ..SimConditions::default()
    });
    let calming = link.run(2 * SETTLE_TICKS, 1);
    let calm = calming.last().expect("frames").stats;
    assert_eq!(calm.underruns, peak.underruns, "calming ran dry");
    assert_near(
        calm.delay,
        steady_delay(1),
        Duration::from_millis(1),
        "the calm delay",
    );
    assert!(
        calming.iter().any(|shown| shown.stats.delay < peak.delay
            && shown.stats.delay > calm.delay + Duration::from_millis(10)),
        "the delay fell straight to the calm one instead of shrinking"
    );

    let whole: Vec<Shown> = [warming, jittered, calming].concat();
    assert_monotonic(&whole);
    let fastest = max_rate_deviation(&whole);
    assert!(
        fastest <= MAX_PLAYOUT_RATE_DEVIATION + RATE_EPSILON,
        "playback ran {fastest} off the tick rate in one frame"
    );
    assert!(
        rate_deviations(&whole)
            .iter()
            .any(|&deviation| deviation > MAX_PLAYOUT_RATE_DEVIATION / 2.0),
        "playback never sped up to close the delay"
    );
    assert_eq!(whole.last().expect("frames").stats.steps, 0);
}

/// **Jitter that queues rather than reorders never runs playback dry once
/// the delay has converged.** Each snapshot is delayed by up to a bound but
/// none overtakes another — a queue on the path, the shape most links have —
/// so a long delay holds up the snapshots behind it and they arrive in a
/// burst, every one of them used.
#[test]
fn queued_jitter_never_runs_dry_once_converged() {
    for bound in [20, 50, 100].map(Duration::from_millis) {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut client = client(transport);
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
        let mut in_flight: VecDeque<(Duration, u64)> = VecDeque::new();
        let mut now = Duration::ZERO;
        let mut settled = None;
        for server_tick in 1..=3 * SETTLE_TICKS {
            now += TICK;
            let delay = bound.mul_f64(crcbl_core::rand::hash_unit(0x51EE_DEDD, server_tick));
            let due = in_flight
                .back()
                .map_or(now + delay, |&(last, _)| last.max(now + delay));
            in_flight.push_back((due, server_tick));
            while let Some(&(due, sent)) = in_flight.front()
                && due <= now
            {
                in_flight.pop_front();
                crate::tests::send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(sent, &[]));
            }
            client.update(now);
            // The client's acks, which a full channel would refuse.
            while peer.recv().expect("the link is up").is_some() {}
            if server_tick == SETTLE_TICKS {
                settled = Some(client.playout_stats());
            }
        }
        let settled = settled.expect("the run settled");
        let end = client.playout_stats();
        assert_eq!(client.processing_error_count(), 0);
        assert_eq!(
            end.underruns, settled.underruns,
            "{bound:?} of queued jitter ran dry at a delay of {:?}",
            end.delay
        );
        assert!(end.delay > steady_delay(1), "{bound:?}: no jitter measured");
    }
}

/// **A link that delivers in bursts is covered once the delay converges.**
/// A server ticking at [`BURST_TICK_HZ`] has every snapshot held on the way
/// and released [`BURST`] at a time, so playback sees nothing for a burst's
/// length and then everything at once. Once converged, playback never runs
/// dry, and on the frame each burst lands it trails the newest snapshot by
/// at least the burst's length — enough to play on until the next one.
#[test]
fn bursty_delivery_never_runs_dry_once_converged() {
    let mut settled = None;
    let mut shortest_lag = f64::INFINITY;
    let client = run_burst_probe(|client, server_tick, released| {
        if server_tick == burst_settle_ticks() {
            settled = Some(client.playout_stats());
        }
        if released && server_tick > burst_settle_ticks() {
            let lag = server_tick as f64 - client.playback_tick().expect("playing");
            shortest_lag = shortest_lag.min(lag);
        }
    });
    let settled = settled.expect("the run settled");
    let end = client.playout_stats();
    assert_eq!(
        end.underruns, settled.underruns,
        "bursts of {BURST} ran dry at a delay of {:?}",
        end.delay
    );
    assert!(
        shortest_lag >= BURST as f64,
        "playback trailed a burst by {shortest_lag} ticks, under the {BURST} to the next"
    );
}

/// **Bursty delivery keeps the picture moving every frame once converged**,
/// not only the playout clock. Just after a burst lands playback trails the
/// newest snapshot by more than a burst, so a buffer that held fewer frames
/// than that dropped ones playback had yet to reach: playback then sat
/// behind the oldest frame held and the entity stood still until playback
/// caught up with it, once per burst. Every frame, playback lies within the
/// pair it interpolates, and the entity moves on along its path.
#[test]
fn bursty_delivery_keeps_the_picture_moving_once_converged() {
    let mut previous_x = None;
    let mut frames_watched = 0;
    run_burst_probe(|client, server_tick, _| {
        if server_tick <= burst_settle_ticks() {
            return;
        }
        let playback = client.playback_tick().expect("playing");
        let (prev, current) = client.playback_pair(SectorId::ZERO).expect("a pair held");
        assert!(
            prev.tick.get() as f64 <= playback && playback <= current.tick.get() as f64,
            "tick {server_tick}: playback {playback} outside the pair {} to {}",
            prev.tick.get(),
            current.tick.get()
        );
        let x = client
            .interpolate(client.interpolation_alpha())
            .transforms
            .iter()
            .find(|(bits, _)| *bits == ENTITY)
            .map(|(_, transform)| transform.position.x)
            .expect("the entity");
        if let Some(previous_x) = previous_x {
            assert!(
                x > previous_x,
                "tick {server_tick}: the entity stood still at {x} (playback {playback})"
            );
        }
        previous_x = Some(x);
        frames_watched += 1;
    });
    assert_eq!(
        frames_watched,
        burst_settle_ticks(),
        "every converged frame watched"
    );
}

/// Drive the backlog's burst probe: a client at [`BURST_TICK_HZ`] fed
/// snapshots of [`ENTITY`] moving [`STEP`] a tick, held and released
/// [`BURST`] at a time, for twice [`BURST_SETTLE`]. After each client update
/// `frame` sees the client, the server tick, and whether a burst landed on
/// it. Returns the client, with no snapshot having failed to apply.
fn run_burst_probe(
    mut frame: impl FnMut(&Client<InMemoryTransport>, u64, bool),
) -> Client<InMemoryTransport> {
    let tick = Duration::from_secs(1) / BURST_TICK_HZ;
    let (transport, mut peer) = InMemoryTransport::pair();
    let mut client = Client::new_with_compatibility(
        World::new(),
        transport,
        BURST_TICK_HZ,
        COMPATIBILITY,
        PlayerId::from_seed(116),
    );
    client.set_inbound_rate_limit_config(InboundRateLimitConfig {
        messages_per_second: 100_000,
        bytes_per_second: 100_000_000,
    });
    let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
    let mut now = Duration::ZERO;
    for server_tick in 1..=2 * burst_settle_ticks() {
        now += tick;
        let released = server_tick.is_multiple_of(BURST);
        if released {
            for sent in server_tick + 1 - BURST..=server_tick {
                let snapshot = keyframe_snapshot(
                    sent,
                    &[(physics(), transform_blob(&[(ENTITY, sent as f64 * STEP)]))],
                );
                crate::tests::send_sealed(&mut peer, &mut crypto, &snapshot);
            }
        }
        client.update(now);
        // The client's acks, which a full channel would refuse.
        while peer.recv().expect("the link is up").is_some() {}
        frame(&client, server_tick, released);
    }
    assert_eq!(client.processing_error_count(), 0);
    client
}

/// The server tick rate of the burst probe ([`run_burst_probe`]), the
/// backlog's.
const BURST_TICK_HZ: u32 = 240;

/// Snapshots per burst, and server ticks between bursts.
const BURST: u64 = 40;

/// How long the burst run takes to settle, and then how long it is watched.
const BURST_SETTLE: Duration = Duration::from_secs(10);

/// [`BURST_SETTLE`] in server ticks at [`BURST_TICK_HZ`].
fn burst_settle_ticks() -> u64 {
    BURST_SETTLE.as_secs() * u64::from(BURST_TICK_HZ)
}

/// **A gap longer than the longest delay holds the last state, counts one
/// underrun, and recovers.** Every snapshot is lost for twice
/// [`MAX_PLAYOUT_DELAY`]: playback stands at the newest snapshot it holds —
/// nothing extrapolates — and the dry spell counts once. When snapshots
/// return, playback steps once to its target, the gap does not inflate the
/// measured interval past twice what it was, and the stream plays on without
/// another underrun.
#[test]
fn a_gap_longer_than_the_max_delay_holds_counts_one_underrun_and_recovers() {
    let mut link = Link::new();
    link.run(SETTLE_TICKS, 1);
    let before = link.client.playout_stats();
    let held_tick = link.sent;
    let held_x = held_tick as f64 * STEP;

    link.set_conditions(SimConditions {
        loss_rate: 1.0,
        ..SimConditions::default()
    });
    let gap_ticks = 2 * u64::from(MAX_PLAYOUT_DELAY.div_duration_f64(TICK).ceil() as u32);
    let gap = link.run(gap_ticks, 1);
    let dry = gap.last().expect("frames");
    assert_eq!(
        dry.playback,
        Some(held_tick as f64),
        "playback extrapolated"
    );
    assert_eq!(dry.alpha, 1.0, "the hold is the end of the last pair");
    assert!(
        (dry.x.expect("the entity") - held_x).abs() < 1e-3,
        "the last state was not held: {:?}",
        dry.x
    );
    assert_eq!(dry.stats.underruns, before.underruns + 1);
    assert_eq!(dry.stats.steps, 0, "stepped with nothing to step into");

    link.set_conditions(SimConditions::default());
    let recovering = link.run(SETTLE_TICKS, 1);
    let first = recovering.first().expect("frames").stats;
    assert!(
        first.snapshot_interval_ticks <= 2.0,
        "the gap was taken for a {}-tick cadence",
        first.snapshot_interval_ticks
    );
    let recovered = recovering.last().expect("frames").stats;
    assert_eq!(recovered.steps, 1);
    assert_eq!(recovered.underruns, before.underruns + 1, "ran dry again");
    assert_near(
        recovered.delay,
        steady_delay(1),
        Duration::from_micros(100),
        "the recovered delay",
    );
    assert_monotonic(&[gap, recovering].concat());
}

/// **A snapshot overtaken or repeated never enters the buffer, and an
/// overtaken one takes back the spacing it inflated.** Tick 3 arrives
/// before tick 2, which takes the interval to 2 at once; tick 2 then
/// arrives, and tick 3 again, and tick 4. The buffer holds 1, 3 and 4 in
/// order, and the interval is what 1, 2, 3 and 4 in order make of it.
#[test]
fn an_overtaken_or_duplicate_snapshot_is_dropped() {
    let (transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(transport);
    let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
    for tick in [1, 3] {
        crate::tests::send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(tick, &[]));
    }
    client.update(TICK);
    assert_eq!(client.playout_stats().snapshot_interval_ticks, 2.0);
    for tick in [2, 3, 4] {
        crate::tests::send_sealed(&mut peer, &mut crypto, &keyframe_snapshot(tick, &[]));
    }
    client.update(2 * TICK);

    let buffered: Vec<u64> = client.frames[&SectorId::ZERO]
        .iter()
        .map(|frame| frame.tick.get())
        .collect();
    assert_eq!(buffered, vec![1, 3, 4]);
    assert_eq!(
        client.processing_error_count(),
        0,
        "dropping them is no error"
    );
    assert_eq!(client.playout_stats().snapshot_interval_ticks, 1.0);
}

/// **Reordering does not pass for a slower cadence.** Under the latency
/// spread of [`jitter_grows_the_delay_without_underruns_and_calm_shrinks_it_gradually`]
/// a snapshot every tick is overtaken often, and the client drops what was
/// overtaken; the interval still reads one tick, not the spacing between
/// survivors. A gap stands until the snapshots overtaken in it arrive, so
/// for a frame or two the interval can read wider — on average it does not.
#[test]
fn reordering_does_not_inflate_the_interval() {
    let mut link = Link::new();
    link.set_conditions(SimConditions {
        latency: Duration::from_millis(60),
        jitter: Duration::from_millis(40),
        seed: 0x6A17_7E55,
        ..SimConditions::default()
    });
    link.run(SETTLE_TICKS, 1);
    let shown = link.run(SETTLE_TICKS, 1);
    let mean = shown
        .iter()
        .map(|shown| shown.stats.snapshot_interval_ticks)
        .sum::<f64>()
        / shown.len() as f64;
    eprintln!("mean {mean}");
    assert!(mean < 1.5, "reordering took the interval to {mean} ticks");
}

/// **Another sector's copy of a tick is not another arrival.** Two sectors
/// sent the same tick every two ticks measure an interval of two, not one
/// collapsing towards zero between the copies.
#[test]
fn the_same_tick_in_two_sectors_is_one_arrival() {
    let (transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(transport);
    let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
    let other = SectorId { x: 1, y: 0, z: 0 };
    client.set_subscribed_sectors([SectorId::ZERO, other]);
    let mut now = Duration::ZERO;
    for tick in (2..=40).step_by(2) {
        now += 2 * TICK;
        for sector in [SectorId::ZERO, other] {
            crate::tests::send_sealed(
                &mut peer,
                &mut crypto,
                &crate::tests::keyframe_snapshot_for_sector(sector, tick, &[]),
            );
        }
        client.update(now);
    }
    assert_eq!(client.playout_stats().snapshot_interval_ticks, 2.0);
}

/// **The buffer holds at most its tick rate's capacity, dropping the
/// oldest, and a tick rate past any a game runs at is held to
/// [`playout::MAX_JITTER_BUFFER_FRAMES`].** A server floods more snapshots
/// than the buffer holds, all before playback has moved at all.
/// At [`BURST_TICK_HZ`] the capacity is the frames across
/// [`playout::MAX_PLAYBACK_LAG`] plus [`playout::JITTER_BUFFER_SLACK`]; at
/// the most [`crcbl_core::FrameClock`] accepts, the same arithmetic would
/// ask for hundreds of millions of frames, and the hard maximum holds it.
#[test]
fn the_buffer_is_bounded() {
    const FASTEST_TICK_HZ: u32 = 1_000_000_000;
    for (tick_hz, capacity) in [
        (BURST_TICK_HZ, 128),
        (FASTEST_TICK_HZ, playout::MAX_JITTER_BUFFER_FRAMES),
    ] {
        let (transport, mut peer) = InMemoryTransport::pair();
        let mut client = Client::new_with_compatibility(
            World::new(),
            transport,
            tick_hz,
            COMPATIBILITY,
            PlayerId::from_seed(117),
        );
        assert_eq!(client.frame_capacity, capacity, "at {tick_hz} Hz");
        client.set_inbound_rate_limit_config(InboundRateLimitConfig {
            messages_per_second: 100_000,
            bytes_per_second: 100_000_000,
        });
        // Every update at a later time would run the client's input clock
        // through each tick since; at the fastest rate that is millions.
        let mut crypto = connect(&mut client, &mut peer, Duration::ZERO);
        let newest = capacity as u64 + 8;
        let ticks: Vec<u64> = (1..=newest).collect();
        // In drains the in-memory link's queues hold, acks and all.
        for drain in ticks.chunks(IN_MEMORY_CHANNEL_CAPACITY / 2) {
            for &server_tick in drain {
                crate::tests::send_sealed(
                    &mut peer,
                    &mut crypto,
                    &keyframe_snapshot(server_tick, &[]),
                );
            }
            client.recv_snapshots().expect("the link is up");
            while peer.recv().expect("the link is up").is_some() {}
        }

        assert_eq!(client.processing_error_count(), 0);
        assert_eq!(client.last_applied_tick(), TickId::from_raw(newest));
        let buffered = &client.frames[&SectorId::ZERO];
        assert_eq!(buffered.len(), capacity, "at {tick_hz} Hz");
        assert_eq!(
            buffered.front().map(|frame| frame.tick.get()),
            Some(newest + 1 - capacity as u64),
            "at {tick_hz} Hz, the oldest went"
        );
    }
}
