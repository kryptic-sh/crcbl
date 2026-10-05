//! The playout clock: where in server time the client renders, and how far
//! behind the newest snapshot it holds that is.
//!
//! Snapshots do not arrive evenly. The server sends one every tick or every
//! few (its rate drop), and the network delays each by a different amount, so
//! a client that rendered the newest snapshot it had would stand still
//! whenever one came late. The client therefore renders at a **playout
//! delay** behind the server: far enough back that the snapshot it needs next
//! has normally arrived by the time playback reaches it, and no further,
//! because every tick of delay is a tick of latency.
//!
//! # The estimator
//!
//! Each snapshot newer than any before it is an arrival: its server tick `S`
//! and the local time `R` it was drained at, both in server ticks (local time
//! is converted at the client's tick rate). Three smoothed quantities follow
//! from consecutive arrivals `i` and `j`, each updated by [`ESTIMATOR_GAIN`]:
//!
//! - **The interarrival jitter `J`**, exactly as RFC 3550 §6.4.1 computes
//!   it: `D = (Rj − Ri) − (Sj − Si)`, then `J += (|D| − J) · gain`. It is a
//!   smoothed mean absolute deviation of the transit time, reported in
//!   [`PlayoutStats`] for a netgraph; the delay is not sized from it, because
//!   averaging consecutive changes reads a stream that arrives in bursts as
//!   nearly steady.
//! - **The snapshot interval `I`**, the server ticks between consecutive
//!   arrivals. A longer spacing takes over at once, so a rate drop is covered
//!   from its first slower snapshot instead of starving playback while an
//!   average catches up, and a shorter one is approached by the gain. A
//!   spacing is first capped at [`LOSS_SPACING_CAP`] times the interval held:
//!   a run of lost snapshots is an outage, not a cadence, and taking it at
//!   face value would leave the delay at the outage's length. The spacings
//!   are taken in tick order, not arrival order: a snapshot that arrives
//!   after one newer than it — overtaken, and dropped from the buffer —
//!   still takes its place among the recent ticks, so reordering does not
//!   read as a slower cadence.
//! - **The clock offset `O`**, `S − R` smoothed: the server's tick as seen
//!   from here, minus the mean transit time.
//!
//! Playback aims at `R_now + O − delay` — the latest server time, estimated
//! from the arrivals, less the delay — so it reaches a tick `S` at local time
//! `S − O + delay`, and runs dry when it passes the newest snapshot held
//! before the next has arrived. Each arrival therefore has a **relative
//! arrival delay** `R + O − S_prev`, with `S_prev` the newest tick held
//! before it and `O` as it stood before it: how long after a playback with
//! no delay would have passed `S_prev` the snapshot after it came. Playback
//! at delay `d`, its offset unchanged, has it in time when it is at most
//! `d`. A steady stream's is its interval; a burst's first snapshot's is
//! near the time since the burst before.
//!
//! The delay covers [`DELAY_QUANTILE`] of the relative arrival delays, read
//! from a histogram that forgets, in the manner of WebRTC NetEq's delay
//! manager (its `UnderrunOptimizer` and `Histogram`; see `histogram.rs`
//! beside this module): buckets [`DELAY_BUCKET_WIDTH`] wide, one sample per
//! [`DELAY_RESAMPLE_INTERVAL`] — the largest relative delay seen in it, so a
//! burst counts by its worst snapshot rather than being outvoted by the
//! punctual ones behind it — and each older sample weighed
//! [`DELAY_FORGET_FACTOR`] less. The quantile's bucket gives its lower edge,
//! and [`PLAYOUT_MARGIN`], never narrower than a bucket, covers the rest of
//! it. The delay is `max(I, quantile) + PLAYOUT_MARGIN`, no shorter than
//! [`MIN_PLAYOUT_DELAY`], and no longer than [`MAX_PLAYOUT_DELAY`] unless
//! `I + PLAYOUT_MARGIN` alone is: a delay shorter than the spacing starves
//! on every snapshot, whatever the histogram says. When late arrivals stop,
//! their weight fades by the forget factor and the quantile falls once it is
//! no more than `1 − DELAY_QUANTILE`.
//!
//! # The clock
//!
//! Playback never jumps to that target. It runs at the server's tick rate,
//! sped up or slowed by at most [`MAX_PLAYOUT_RATE_DEVIATION`] in proportion
//! to how far it is from the target, closing the gap over about
//! [`PLAYOUT_CORRECTION_TIME`] — so the delay changes gradually and motion
//! never stutters. The one exception is playback behind its target by more
//! than [`MAX_PLAYOUT_DELAY`] once a snapshot past it has arrived: after an
//! outage that long, catching up at the rate bound would replay the outage
//! slowly, so playback steps to the target and the step is counted.
//!
//! Playback never passes the newest snapshot held. When it would — the buffer
//! has run dry — it holds there, the last state stays on screen, and the
//! underrun is counted. Nothing extrapolates: guessing past the newest
//! snapshot is prediction's job, and prediction is hooks, not an
//! implementation (`docs/notes/simulation.md`).

use std::collections::VecDeque;
use std::time::Duration;

use crcbl_core::TickId;

use histogram::DelayHistogram;

mod histogram;

/// The shortest playout delay chosen whatever the snapshot interval: one
/// display frame at 60 Hz. A client's arrivals are only seen once a frame, so
/// a delay under a frame cannot be told from none.
pub const MIN_PLAYOUT_DELAY: Duration = Duration::from_millis(16);

/// The longest playout delay jitter can raise playback to. Past it a stutter
/// costs less than the latency that would hide it. A snapshot interval longer
/// than this still gets a delay of its own length: see the module docs.
pub const MAX_PLAYOUT_DELAY: Duration = Duration::from_millis(250);

/// Added to every playout delay, so that a perfectly steady stream — every
/// relative arrival delay exactly its interval — still reaches each snapshot
/// a little after it arrives rather than in the same instant.
pub const PLAYOUT_MARGIN: Duration = Duration::from_millis(4);

/// The share of relative arrival delays the playout delay covers: NetEq's
/// default quantile (`DelayManager::Config::quantile`). Its histogram takes
/// the worst arrival of each [`DELAY_RESAMPLE_INTERVAL`], so the share left
/// out is of those intervals, not of snapshots.
pub const DELAY_QUANTILE: f64 = 0.95;

/// How much less each older sample of the relative arrival delay weighs
/// than the one after it: NetEq's default (the `forget_factor` of
/// `DelayManager::Config`). Delays that stop fade from the histogram by
/// this factor per [`DELAY_RESAMPLE_INTERVAL`].
pub const DELAY_FORGET_FACTOR: f64 = 0.983;

/// How far a new histogram's forget factor starts from evenly weighing the
/// samples it has, ramping to [`DELAY_FORGET_FACTOR`]: NetEq's default
/// (`DelayManager::Config::start_forget_weight`).
pub const DELAY_START_FORGET_WEIGHT: f64 = 2.0;

/// The width of one bucket of the relative arrival delay histogram. NetEq's
/// is 20 ms (`kBucketSizeMs`), as long as a typical audio packet; a server
/// tick can be far shorter, and this is no wider than [`PLAYOUT_MARGIN`],
/// which covers the part of a bucket its lower edge leaves out.
pub const DELAY_BUCKET_WIDTH: Duration = Duration::from_millis(4);

const _: () = assert!(DELAY_BUCKET_WIDTH.as_nanos() <= PLAYOUT_MARGIN.as_nanos());

/// How often the histogram takes a sample: the largest relative arrival
/// delay since the last. NetEq resamples every 500 ms, which holds a delay
/// long after its jitter stops; this brings it back sooner (`playout::tests`
/// times it) and still spans several snapshots at the slowest rate drop
/// interval.
pub const DELAY_RESAMPLE_INTERVAL: Duration = Duration::from_millis(100);

/// The histogram's buckets: enough to reach [`MAX_PLAYOUT_DELAY`], past
/// which the delay is capped anyway.
const DELAY_BUCKETS: usize = histogram::bucket_count(DELAY_BUCKET_WIDTH, MAX_PLAYOUT_DELAY);

/// The gain every smoothed playout estimate moves by per arrival: RFC 3550
/// §6.4.1's `1/16`, chosen there for "a good noise reduction ratio while
/// maintaining a reasonable rate of convergence".
pub const ESTIMATOR_GAIN: f64 = 1.0 / 16.0;

/// The most playback runs faster or slower than the server's tick rate while
/// it drifts towards its target, as a fraction of that rate.
pub const MAX_PLAYOUT_RATE_DEVIATION: f64 = 0.1;

/// The time constant of playback's correction towards its target: a gap of
/// this long would be closed in this long at the rate it starts at, were the
/// rate not capped by [`MAX_PLAYOUT_RATE_DEVIATION`].
pub const PLAYOUT_CORRECTION_TIME: Duration = Duration::from_millis(100);

/// The furthest playback trails the newest snapshot without stepping: a
/// delay that jitter has raised to [`MAX_PLAYOUT_DELAY`], plus as far again
/// behind its target, which playback may fall before it steps — as it does
/// when the link's latency drops and the estimated server time moves ahead
/// faster than playback's bounded rate follows. Every snapshot within it is
/// one playback can still reach and show.
pub const MAX_PLAYBACK_LAG: Duration = MAX_PLAYOUT_DELAY.saturating_mul(2);

/// Frames a sector's buffer holds beyond the ticks [`MAX_PLAYBACK_LAG`]
/// spans: the frame at or behind playback, and the pairs a slow server
/// needs, whose snapshot interval — and so its delay — can outgrow the lag
/// at a tick rate low enough that the lag spans a tick or less.
pub const JITTER_BUFFER_SLACK: usize = 8;

/// The most frames any sector's buffer holds, whatever the tick rate. The
/// capacity grows with the tick rate, which the client is constructed with
/// and [`crcbl_core::FrameClock`] accepts far beyond any a game runs at;
/// without a ceiling, a server sending that fast would have the client hold
/// a frame per tick across all of [`MAX_PLAYBACK_LAG`]. Past the tick rate
/// whose lag fills it, the oldest frames go and the picture can stall once
/// playback trails the newest by more than the buffer holds. A frame is one
/// transform per replicated entity in the sector.
pub const MAX_JITTER_BUFFER_FRAMES: usize = 256;

/// The most snapshots a sector's buffer holds at `tick_hz`: every one from
/// playback to the newest across [`MAX_PLAYBACK_LAG`], plus
/// [`JITTER_BUFFER_SLACK`], and never more than [`MAX_JITTER_BUFFER_FRAMES`].
/// Frames behind playback are dropped as it passes them, so the capacity
/// binds only when more have arrived ahead of playback than it can reach
/// without stepping; the oldest then go.
pub(crate) fn jitter_buffer_capacity(tick_hz: u32) -> usize {
    let lag_ticks = (MAX_PLAYBACK_LAG.as_nanos() * u128::from(tick_hz))
        .div_ceil(Duration::from_secs(1).as_nanos());
    usize::try_from(lag_ticks)
        .unwrap_or(usize::MAX)
        .saturating_add(JITTER_BUFFER_SLACK)
        .min(MAX_JITTER_BUFFER_FRAMES)
}

/// The most recent server ticks the snapshot interval is measured over. A
/// snapshot overtaken by more than this many newer ones says nothing about
/// the cadence now. Every arrival walks the window again, so it stays this
/// size whatever the tick rate rather than growing with the frame buffer.
pub const INTERVAL_TICK_WINDOW: usize = 32;

/// The snapshot interval assumed before two arrivals have measured one: a
/// session starts at a snapshot every tick.
const INITIAL_INTERVAL_TICKS: f64 = 1.0;

/// The longest spacing between arrivals taken as a snapshot interval, as a
/// multiple of the interval already held. A rate drop steps one interval at a
/// time, so its next interval is always within this of the last.
pub const LOSS_SPACING_CAP: f64 = 2.0;

/// How the playout delay and its clock stand: what a netgraph would show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayoutStats {
    /// How far behind the estimated latest server time playback aims.
    pub delay: Duration,
    /// The relative arrival delay the playout delay covers: the lower edge
    /// of the histogram's [`DELAY_QUANTILE`] bucket, zero before its first
    /// sample. See the [module docs](self).
    pub relative_delay: Duration,
    /// The measured interarrival jitter (RFC 3550 §6.4.1). Reported, not
    /// what the delay is sized from.
    pub jitter: Duration,
    /// The measured server ticks between snapshots.
    pub snapshot_interval_ticks: f64,
    /// Times playback reached the newest snapshot held and had to stand
    /// still waiting for the next: one per dry spell, however long.
    pub underruns: u64,
    /// Times playback stepped forward to its target instead of drifting
    /// there, after falling behind by more than [`MAX_PLAYOUT_DELAY`].
    pub steps: u64,
    /// The playout buffer's depth: how far the newest snapshot held runs
    /// ahead of playback, as server time. Zero while playback holds at the
    /// newest, run dry; `None` before playback has a position.
    pub buffered: Option<Duration>,
}

/// One arrival: a server tick and when it was drained, both in server ticks.
#[derive(Debug, Clone, Copy)]
struct Arrival {
    tick: f64,
    at: f64,
}

/// The largest relative arrival delay since a resample interval began, in
/// server ticks, and when it began.
#[derive(Debug, Clone, Copy)]
struct Resample {
    began_at: f64,
    largest: f64,
}

/// The interval after a spacing of `spacing` ticks follows `interval`: a
/// longer spacing, up to [`LOSS_SPACING_CAP`] times the interval, takes over
/// at once, and a shorter one is approached by [`ESTIMATOR_GAIN`].
fn next_interval(interval: f64, spacing: f64) -> f64 {
    let spacing = spacing.min(LOSS_SPACING_CAP * interval);
    if spacing > interval {
        spacing
    } else {
        interval + (spacing - interval) * ESTIMATOR_GAIN
    }
}

/// The playout delay estimator and the playback clock it steers. See the
/// [module docs](self).
#[derive(Debug, Clone)]
pub(crate) struct Playout {
    tick_rate_hz: f64,
    newest: Option<Arrival>,
    /// The last [`INTERVAL_TICK_WINDOW`] ticks received, oldest first,
    /// overtaken ones in their place: the interval is their spacings taken
    /// in tick order. One older than all of them is ignored.
    recent_ticks: VecDeque<f64>,
    /// The interval as measured up to the oldest of `recent_ticks`.
    interval_through_oldest: f64,
    interval_ticks: f64,
    delays: DelayHistogram,
    resample: Option<Resample>,
    jitter_ticks: f64,
    offset_ticks: f64,
    /// Where playback is, in server ticks; `None` until the first arrival.
    playback_tick: Option<f64>,
    last_update: Option<Duration>,
    /// Whether playback is holding at the newest arrival, so one dry spell
    /// counts once.
    dry: bool,
    underruns: u64,
    steps: u64,
}

impl Playout {
    pub(crate) fn new(tick_rate_hz: f64) -> Self {
        Self {
            tick_rate_hz,
            newest: None,
            recent_ticks: VecDeque::with_capacity(INTERVAL_TICK_WINDOW + 1),
            interval_through_oldest: INITIAL_INTERVAL_TICKS,
            interval_ticks: INITIAL_INTERVAL_TICKS,
            delays: DelayHistogram::new(DELAY_BUCKETS),
            resample: None,
            jitter_ticks: 0.0,
            offset_ticks: 0.0,
            playback_tick: None,
            last_update: None,
            dry: false,
            underruns: 0,
            steps: 0,
        }
    }

    fn ticks(&self, duration: Duration) -> f64 {
        duration.as_secs_f64() * self.tick_rate_hz
    }

    fn duration(&self, ticks: f64) -> Duration {
        Duration::from_secs_f64((ticks / self.tick_rate_hz).max(0.0))
    }

    /// Record a snapshot of `tick` drained at `now`. One no newer than the
    /// newest already recorded — another sector's copy of the same tick, or
    /// one overtaken on the way — is not an arrival; an overtaken one only
    /// corrects the interval (see the module docs).
    pub(crate) fn observe(&mut self, tick: TickId, now: Duration) {
        let tick = tick.get() as f64;
        let at = self.ticks(now);
        match self.newest {
            Some(newest) if tick < newest.tick => {
                self.take_tick(tick);
                return;
            }
            Some(newest) if tick == newest.tick => return,
            Some(newest) => {
                let spacing = tick - newest.tick;
                let transit_change = (at - newest.at) - spacing;
                self.jitter_ticks += (transit_change.abs() - self.jitter_ticks) * ESTIMATOR_GAIN;
                self.take_tick(tick);
                self.record_relative_delay(at + self.offset_ticks - newest.tick, at);
                self.offset_ticks += ((tick - at) - self.offset_ticks) * ESTIMATOR_GAIN;
            }
            None => {
                self.take_tick(tick);
                self.offset_ticks = tick - at;
            }
        }
        self.newest = Some(Arrival { tick, at });
        self.dry = false;
    }

    /// Put `tick` among the recent ticks in its place and measure the
    /// interval again from them. A tick already there, or older than all of
    /// them, changes nothing.
    fn take_tick(&mut self, tick: f64) {
        let index = match self
            .recent_ticks
            .binary_search_by(|recent| recent.total_cmp(&tick))
        {
            Ok(_) => return,
            Err(0) if !self.recent_ticks.is_empty() => return,
            Err(index) => index,
        };
        self.recent_ticks.insert(index, tick);
        if self.recent_ticks.len() > INTERVAL_TICK_WINDOW
            && let Some(oldest) = self.recent_ticks.pop_front()
            && let Some(&next) = self.recent_ticks.front()
        {
            self.interval_through_oldest =
                next_interval(self.interval_through_oldest, next - oldest);
        }
        let mut interval = self.interval_through_oldest;
        for (&earlier, &later) in self
            .recent_ticks
            .iter()
            .zip(self.recent_ticks.iter().skip(1))
        {
            interval = next_interval(interval, later - earlier);
        }
        self.interval_ticks = interval;
    }

    /// Take a relative arrival delay of `delay` ticks, arrived at `at`, into
    /// the current resample interval, and the interval's largest into the
    /// histogram once it has run [`DELAY_RESAMPLE_INTERVAL`]: NetEq's
    /// `UnderrunOptimizer::Update`. NetEq restarts the interval at the
    /// arrival that closes it; here intervals keep to a fixed grid from the
    /// first, because a frame clock a hair short of the interval — six
    /// frames of a 60 Hz `Duration` are just under 100 ms — would otherwise
    /// stretch every interval by a frame.
    fn record_relative_delay(&mut self, delay: f64, at: f64) {
        let window = self.ticks(DELAY_RESAMPLE_INTERVAL);
        let Some(resample) = &mut self.resample else {
            self.resample = Some(Resample {
                began_at: at,
                largest: delay,
            });
            return;
        };
        let elapsed = ((at - resample.began_at) / window).floor();
        if elapsed < 1.0 {
            resample.largest = resample.largest.max(delay);
            return;
        }
        let largest = resample.largest / self.tick_rate_hz;
        resample.began_at += elapsed * window;
        resample.largest = delay;
        self.delays.add(histogram::bucket_of(
            largest,
            DELAY_BUCKET_WIDTH,
            DELAY_BUCKETS,
        ));
    }

    /// The relative arrival delay the playout delay covers, in server ticks.
    fn relative_delay_ticks(&self) -> f64 {
        self.delays
            .quantile(DELAY_QUANTILE)
            .map_or(0.0, |bucket| bucket as f64 * self.ticks(DELAY_BUCKET_WIDTH))
    }

    /// The playout delay now, in server ticks.
    fn delay_ticks(&self) -> f64 {
        let margin = self.ticks(PLAYOUT_MARGIN);
        let floor = (self.interval_ticks + margin).max(self.ticks(MIN_PLAYOUT_DELAY));
        let ceiling = self.ticks(MAX_PLAYOUT_DELAY).max(floor);
        (self.relative_delay_ticks() + margin).clamp(floor, ceiling)
    }

    /// Move playback on to `now`. See the [module docs](self).
    pub(crate) fn advance(&mut self, now: Duration) {
        let elapsed = self
            .last_update
            .map_or(Duration::ZERO, |previous| now.saturating_sub(previous));
        self.last_update = Some(now);
        let Some(newest) = self.newest else {
            return;
        };
        let target = self.ticks(now) + self.offset_ticks - self.delay_ticks();
        let playback = match self.playback_tick {
            None => target,
            // Held at the newest snapshot through an outage, playback falls
            // ever further behind; it steps once there is something to step
            // into, not on every frame of the outage.
            Some(playback)
                if playback < newest.tick && target - playback > self.ticks(MAX_PLAYOUT_DELAY) =>
            {
                self.steps += 1;
                target
            }
            Some(playback) => {
                let elapsed = self.ticks(elapsed);
                // Measured from where the tick rate alone would take
                // playback, so that on target it runs at exactly that rate.
                let advanced = playback + elapsed;
                let error = target - advanced;
                let rate_deviation = (error / self.ticks(PLAYOUT_CORRECTION_TIME))
                    .clamp(-MAX_PLAYOUT_RATE_DEVIATION, MAX_PLAYOUT_RATE_DEVIATION);
                // Never past the target: on a long frame the proportional
                // correction would overshoot it and swing back.
                advanced + (rate_deviation * elapsed).clamp(-error.abs(), error.abs())
            }
        };
        if playback > newest.tick {
            if !self.dry {
                self.underruns += 1;
                self.dry = true;
            }
            self.playback_tick = Some(newest.tick);
        } else {
            self.playback_tick = Some(playback);
        }
    }

    /// Where playback is, in server ticks; `None` before the first arrival.
    pub(crate) fn playback_tick(&self) -> Option<f64> {
        self.playback_tick
    }

    pub(crate) fn stats(&self) -> PlayoutStats {
        PlayoutStats {
            delay: self.duration(self.delay_ticks()),
            relative_delay: self.duration(self.relative_delay_ticks()),
            jitter: self.duration(self.jitter_ticks),
            snapshot_interval_ticks: self.interval_ticks,
            underruns: self.underruns,
            steps: self.steps,
            buffered: self
                .newest
                .zip(self.playback_tick)
                .map(|(newest, playback)| self.duration(newest.tick - playback)),
        }
    }

    /// The newest snapshot's tick; `None` before the first arrival.
    pub(crate) fn newest_tick(&self) -> Option<f64> {
        self.newest.map(|newest| newest.tick)
    }

    /// The clock offset `O`: the server's tick less local time, both in
    /// ticks, so local time plus it is the server time the snapshots show;
    /// `None` before the first arrival. The input lead runs ahead of it.
    pub(crate) fn server_offset(&self) -> Option<f64> {
        self.newest.map(|_| self.offset_ticks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A playout clock at one tick a second, so ticks and seconds coincide.
    fn at_one_hertz() -> Playout {
        Playout::new(1.0)
    }

    fn arrive(playout: &mut Playout, tick: u64, at_secs: f64) {
        playout.observe(TickId::from_raw(tick), Duration::from_secs_f64(at_secs));
    }

    /// **The jitter is RFC 3550 §6.4.1's**, worked by hand: transit changes
    /// of +0.5 and then −0.5 ticks give `J = 0.5/16`, then
    /// `J + (0.5 − J)/16`; the offset follows its samples by the same gain.
    #[test]
    fn the_jitter_and_offset_follow_rfc_3550s_arithmetic() {
        let mut playout = at_one_hertz();
        arrive(&mut playout, 10, 0.0);
        arrive(&mut playout, 11, 1.5);
        let first = 0.5 / 16.0;
        assert!((playout.jitter_ticks - first).abs() < 1e-12);
        arrive(&mut playout, 12, 2.0);
        let second = first + (0.5 - first) / 16.0;
        assert!((playout.jitter_ticks - second).abs() < 1e-12);
        assert!((playout.stats().jitter.as_secs_f64() - second).abs() < 1e-9);

        // Samples of S − R: 10, then 9.5, then 10.
        let offset = 10.0 + (9.5 - 10.0) / 16.0;
        let offset = offset + (10.0 - offset) / 16.0;
        assert!((playout.offset_ticks - offset).abs() < 1e-12);
    }

    /// **The delay is the larger of the interval and the relative delay
    /// covered, plus the margin, held between its floor and ceiling.** A
    /// relative delay of ten buckets outgrows an interval of one 60 Hz tick
    /// and sets the delay; one of two buckets does not, and the interval
    /// does. A relative delay in the last bucket is capped at
    /// [`MAX_PLAYOUT_DELAY`]; an interval longer than that cap still gets a
    /// delay of its own length plus the margin; and a server ticking so fast
    /// that a tick and the margin fall under [`MIN_PLAYOUT_DELAY`] gets that.
    #[test]
    fn the_delay_is_held_between_its_floor_and_ceiling() {
        let mut late = Playout::new(60.0);
        late.delays.add(10);
        assert_eq!(late.stats().relative_delay, DELAY_BUCKET_WIDTH * 10);
        assert_near(late.stats().delay, DELAY_BUCKET_WIDTH * 10 + PLAYOUT_MARGIN);

        let mut punctual = Playout::new(60.0);
        punctual.delays.add(2);
        assert_eq!(punctual.stats().relative_delay, DELAY_BUCKET_WIDTH * 2);
        assert!(DELAY_BUCKET_WIDTH * 2 < Duration::from_secs(1) / 60);
        assert_near(
            punctual.stats().delay,
            Duration::from_secs(1) / 60 + PLAYOUT_MARGIN,
        );

        let mut jittery = Playout::new(60.0);
        jittery.delays.add(DELAY_BUCKETS - 1);
        assert_eq!(jittery.stats().delay, MAX_PLAYOUT_DELAY);

        let mut sparse = Playout::new(60.0);
        sparse.interval_ticks = 60.0;
        sparse.delays.add(DELAY_BUCKETS - 1);
        assert_eq!(
            sparse.stats().delay,
            Duration::from_secs(1) + PLAYOUT_MARGIN
        );

        let fast = Playout::new(10_000.0);
        assert_eq!(fast.stats().delay, MIN_PLAYOUT_DELAY);
    }

    /// **The buffer's depth is the newest snapshot less playback.** At one
    /// tick a second, playback starts one delay behind the first snapshot,
    /// so that much is buffered; past it, run dry and holding at the newest,
    /// nothing is. Before any snapshot there is no depth to give.
    #[test]
    fn the_buffer_depth_is_the_newest_snapshot_less_playback() {
        let mut playout = at_one_hertz();
        playout.advance(Duration::ZERO);
        assert_eq!(playout.stats().buffered, None);

        arrive(&mut playout, 10, 0.0);
        playout.advance(Duration::ZERO);
        let stats = playout.stats();
        let playback = playout.playback_tick.expect("playing");
        assert_near(
            stats.buffered.expect("a snapshot is held"),
            Duration::from_secs_f64(10.0 - playback),
        );
        assert_near(stats.buffered.expect("a snapshot is held"), stats.delay);

        playout.advance(Duration::from_secs(5));
        assert_eq!(playout.stats().underruns, 1, "run dry");
        assert_eq!(playout.stats().buffered, Some(Duration::ZERO));
    }

    fn assert_near(actual: Duration, expected: Duration) {
        assert!(
            actual.abs_diff(expected) < Duration::from_nanos(10),
            "{actual:?} against {expected:?}"
        );
    }

    /// **The buffer's capacity spans [`MAX_PLAYBACK_LAG`] at the tick rate,
    /// plus the slack, up to the hard maximum.** At 240 Hz the lag is 120
    /// ticks; at one tick a second it rounds up to one; and no tick rate,
    /// however large, overflows the arithmetic or passes the ceiling.
    #[test]
    fn the_capacity_spans_the_lag_up_to_the_hard_maximum() {
        assert_eq!(MAX_PLAYBACK_LAG, Duration::from_millis(500));
        assert_eq!(jitter_buffer_capacity(240), 120 + JITTER_BUFFER_SLACK);
        assert_eq!(jitter_buffer_capacity(1), 1 + JITTER_BUFFER_SLACK);
        assert_eq!(jitter_buffer_capacity(u32::MAX), MAX_JITTER_BUFFER_FRAMES);
    }

    /// A playout clock at a thousand ticks a second, so a tick is a
    /// millisecond and [`DELAY_BUCKET_WIDTH`] a whole number of them.
    fn at_one_kilohertz() -> Playout {
        Playout::new(1000.0)
    }

    /// **The relative arrival delay is `R + O − S_prev`, and the histogram
    /// takes the largest of each resample interval**, by hand at one tick a
    /// millisecond. Ticks 0 to 49 arrive on time, so the offset is exactly
    /// zero; tick 50 arrives 20 ms late with ticks 51 to 70 behind it, then
    /// the stream is on time again. Tick 50's relative delay is
    /// `70 + 0 − 49 = 21`, the largest in the interval that began at tick
    /// 1's arrival; until an arrival a whole [`DELAY_RESAMPLE_INTERVAL`]
    /// later closes it nothing is sampled, and then the 21 lands in the
    /// bucket from 20 ms, which sets the delay.
    #[test]
    fn the_worst_relative_delay_of_an_interval_is_sampled() {
        assert_eq!(DELAY_RESAMPLE_INTERVAL, Duration::from_millis(100));
        assert_eq!(DELAY_BUCKET_WIDTH, Duration::from_millis(4));
        let mut playout = at_one_kilohertz();
        for tick in 0..50 {
            arrive_ms(&mut playout, tick, tick);
        }
        assert_eq!(playout.offset_ticks, 0.0);
        for tick in 50..=70 {
            arrive_ms(&mut playout, tick, 70);
        }
        for tick in 71..=100 {
            arrive_ms(&mut playout, tick, tick);
        }
        assert_eq!(playout.stats().relative_delay, Duration::ZERO);
        assert_eq!(playout.stats().delay, MIN_PLAYOUT_DELAY);
        arrive_ms(&mut playout, 101, 101);
        assert_eq!(playout.stats().relative_delay, Duration::from_millis(20));
        assert_near(
            playout.stats().delay,
            Duration::from_millis(20) + PLAYOUT_MARGIN,
        );
    }

    fn arrive_ms(playout: &mut Playout, tick: u64, at_ms: u64) {
        playout.observe(TickId::from_raw(tick), Duration::from_millis(at_ms));
    }

    /// **A delay whose cause stops holds, then falls, at the forget rate.**
    /// Every resample interval for a long while carries the same late burst,
    /// so every sample lands in one bucket; then the stream is on time. The
    /// delay holds at the burst's for as many samples as it takes the
    /// histogram's old weight, `DELAY_FORGET_FACTOR^n`, to come down to
    /// `1 − DELAY_QUANTILE`, and drops to the steady delay on that sample:
    /// a step, not a slide, and not before.
    #[test]
    fn a_stopped_delay_holds_then_falls_at_the_forget_rate() {
        const WINDOW: u64 = 100;
        assert_eq!(DELAY_RESAMPLE_INTERVAL, Duration::from_millis(WINDOW));
        let held = ((1.0 - DELAY_QUANTILE).ln() / DELAY_FORGET_FACTOR.ln()).ceil() as u64;
        let mut playout = at_one_kilohertz();
        let mut tick = 0;
        for _ in 0..1000 {
            // Twenty snapshots held back and released together, then the
            // rest of the interval on time.
            for _ in 0..20 {
                arrive_ms(&mut playout, tick, tick - tick % WINDOW + 20);
                tick += 1;
            }
            for _ in 20..WINDOW {
                arrive_ms(&mut playout, tick, tick);
                tick += 1;
            }
        }
        let bursty = playout.stats().delay;

        // The delay after each punctual sample: one taken from a resample
        // interval whose largest relative delay was under a bucket.
        let mut delays = Vec::new();
        for _ in 0..2 * held * WINDOW {
            let open = playout.resample.expect("sampling");
            let samples = playout.delays.samples;
            arrive_ms(&mut playout, tick, tick);
            tick += 1;
            if playout.delays.samples > samples {
                if open.largest >= playout.ticks(DELAY_BUCKET_WIDTH) {
                    assert!(delays.is_empty(), "a late sample after punctual ones");
                } else {
                    delays.push(playout.stats().delay);
                }
            }
        }
        let punctual = delays.iter().position(|&delay| delay != bursty);
        assert_eq!(
            punctual.map(|position| position + 1),
            Some(held as usize),
            "the delay left {bursty:?} after {punctual:?} punctual samples, not {held}"
        );
        assert_eq!(delays[held as usize - 1], MIN_PLAYOUT_DELAY);
    }

    /// **A rate drop is covered from its first slower snapshot**: as the
    /// server steps from every tick to every second and then every third,
    /// one arrival at each new spacing takes the interval to it at once,
    /// where the gain alone would have it barely moved. An outage is not a
    /// cadence: a spacing of ten after an interval of one counts as two.
    #[test]
    fn a_longer_spacing_takes_over_the_interval_at_once() {
        let mut playout = at_one_hertz();
        for tick in 0..=20 {
            arrive(&mut playout, tick, tick as f64);
        }
        arrive(&mut playout, 22, 22.0);
        assert_eq!(playout.interval_ticks, 2.0);
        arrive(&mut playout, 25, 25.0);
        assert_eq!(playout.interval_ticks, 3.0);

        let mut outage = at_one_hertz();
        for tick in 0..=20 {
            arrive(&mut outage, tick, tick as f64);
        }
        arrive(&mut outage, 30, 30.0);
        assert_eq!(outage.interval_ticks, LOSS_SPACING_CAP);
    }

    /// **The interval is the spacings in tick order, and remembers those
    /// older than the recent ticks.** An overtaken tick takes its place: 0,
    /// 2, 1 measures 0 → 1 → 2, an interval of one, not two, and a second
    /// copy of a tick is no spacing of zero. Forty spacings of one after an
    /// interval of three bring it to
    /// `1 + 2 · (15/16)^40` — the gain applied forty times — although only
    /// the last [`INTERVAL_TICK_WINDOW`] ticks are kept, and no more.
    #[test]
    fn the_interval_takes_spacings_in_tick_order() {
        let mut playout = at_one_hertz();
        arrive(&mut playout, 0, 0.0);
        arrive(&mut playout, 2, 1.0);
        assert_eq!(playout.interval_ticks, 2.0);
        arrive(&mut playout, 1, 2.0);
        assert_eq!(playout.interval_ticks, 1.0);

        let mut playout = at_one_hertz();
        for tick in [0, 2, 5] {
            arrive(&mut playout, tick, tick as f64);
        }
        const SPACINGS: u64 = 40;
        const { assert!(SPACINGS as usize > INTERVAL_TICK_WINDOW) };
        let mut expected = 3.0;
        assert_eq!(playout.interval_ticks, expected);
        for tick in 6..6 + SPACINGS {
            arrive(&mut playout, tick, tick as f64);
            expected += (1.0 - expected) * ESTIMATOR_GAIN;
            if tick == 6 {
                arrive(&mut playout, 5, 6.0);
                assert_eq!(playout.interval_ticks, expected, "a copy of 5 counted");
            }
        }
        assert_eq!(playout.recent_ticks.len(), INTERVAL_TICK_WINDOW);
        assert!(
            (playout.interval_ticks - expected).abs() < 1e-12,
            "interval {}, expected {expected}",
            playout.interval_ticks
        );
        assert!((expected - (1.0 + 2.0 * (15.0_f64 / 16.0).powi(40))).abs() < 1e-9);
    }

    /// **A long frame lands playback on its target, not past it.** Over a
    /// frame longer than [`PLAYOUT_CORRECTION_TIME`] the proportional
    /// correction would carry it twice as far as the gap — and shorter than
    /// [`MAX_PLAYOUT_DELAY`], so it is not a step.
    #[test]
    fn a_long_frame_does_not_overshoot_the_target() {
        let mut playout = Playout::new(60.0);
        arrive(&mut playout, 0, 0.0);
        playout.advance(Duration::ZERO);
        // Far more snapshots than playback will reach, all on time.
        playout.newest = Some(Arrival {
            tick: 10_000.0,
            at: 0.0,
        });
        let target_now = playout.playback_tick.expect("playing");
        playout.playback_tick = Some(target_now - 0.5);
        let frame = PLAYOUT_CORRECTION_TIME * 2;
        assert!(frame < MAX_PLAYOUT_DELAY);
        playout.advance(frame);
        let target_then = target_now + frame.as_secs_f64() * 60.0;
        let playback = playout.playback_tick.expect("playing");
        assert!(
            (playback - target_then).abs() < 1e-9,
            "playback {playback}, target {target_then}"
        );
        assert_eq!(playout.steps, 0);
    }
}
