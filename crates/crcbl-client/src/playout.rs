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
//!   smoothed mean absolute deviation of the transit time, so a snapshot lost
//!   on the way moves it not at all: the next one is on time.
//! - **The snapshot interval `I`**, the server ticks between consecutive
//!   arrivals. A longer spacing takes over at once, so a rate drop is covered
//!   from its first slower snapshot instead of starving playback while an
//!   average catches up, and a shorter one is approached by the gain. A
//!   spacing is first capped at [`LOSS_SPACING_CAP`] times the interval held:
//!   a run of lost snapshots is an outage, not a cadence, and taking it at
//!   face value would leave the delay at the outage's length.
//! - **The clock offset `O`**, `S − R` smoothed: the server's tick as seen
//!   from here, minus the mean transit time.
//!
//! The delay is `I + JITTER_MULTIPLE · J + PLAYOUT_MARGIN`, no shorter than
//! `I + PLAYOUT_MARGIN` and [`MIN_PLAYOUT_DELAY`], and no longer than
//! [`MAX_PLAYOUT_DELAY`] unless the interval alone is: a delay shorter than
//! the spacing starves on every snapshot, whatever the jitter. Playback aims
//! at `R_now + O − delay` — the latest server time, estimated from the
//! arrivals, less the delay.
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

use std::time::Duration;

use crcbl_core::TickId;

/// The shortest playout delay chosen whatever the snapshot interval: one
/// display frame at 60 Hz. A client's arrivals are only seen once a frame, so
/// a delay under a frame cannot be told from none.
pub const MIN_PLAYOUT_DELAY: Duration = Duration::from_millis(16);

/// The longest playout delay jitter can raise playback to. Past it a stutter
/// costs less than the latency that would hide it. A snapshot interval longer
/// than this still gets a delay of its own length: see the module docs.
pub const MAX_PLAYOUT_DELAY: Duration = Duration::from_millis(250);

/// Added to every playout delay, so that a perfectly steady stream —
/// measured jitter zero — still reaches each snapshot a little after it
/// arrives rather than in the same instant.
pub const PLAYOUT_MARGIN: Duration = Duration::from_millis(4);

/// How many measured jitters the playout delay holds beyond the snapshot
/// interval. Four is the multiple RFC 6298 puts on its smoothed deviation
/// (`K`) for the retransmission timeout, the same shape of question — how
/// long to wait so that a late arrival is rarely too late.
pub const JITTER_MULTIPLE: f64 = 4.0;

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

/// The most snapshots buffered per sector. Playback needs only the ones
/// from just behind its position to the newest, and frames behind it are
/// dropped as it passes them, so this binds only for a server sending more
/// snapshots within [`MAX_PLAYOUT_DELAY`] than it holds; the oldest then go,
/// and playback trails the newest by less than the delay asked for.
pub const JITTER_BUFFER_CAPACITY: usize = 32;

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
    /// The measured interarrival jitter (RFC 3550 §6.4.1).
    pub jitter: Duration,
    /// The measured server ticks between snapshots.
    pub snapshot_interval_ticks: f64,
    /// Times playback reached the newest snapshot held and had to stand
    /// still waiting for the next: one per dry spell, however long.
    pub underruns: u64,
    /// Times playback stepped forward to its target instead of drifting
    /// there, after falling behind by more than [`MAX_PLAYOUT_DELAY`].
    pub steps: u64,
}

/// One arrival: a server tick and when it was drained, both in server ticks.
#[derive(Debug, Clone, Copy)]
struct Arrival {
    tick: f64,
    at: f64,
}

/// The playout delay estimator and the playback clock it steers. See the
/// [module docs](self).
#[derive(Debug, Clone)]
pub(crate) struct Playout {
    tick_rate_hz: f64,
    newest: Option<Arrival>,
    interval_ticks: f64,
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
            interval_ticks: INITIAL_INTERVAL_TICKS,
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
    /// newest already recorded — another sector's copy of the same tick —
    /// changes nothing.
    pub(crate) fn observe(&mut self, tick: TickId, now: Duration) {
        let tick = tick.get() as f64;
        let at = self.ticks(now);
        match self.newest {
            Some(newest) if tick <= newest.tick => return,
            Some(newest) => {
                let spacing = tick - newest.tick;
                let transit_change = (at - newest.at) - spacing;
                self.jitter_ticks += (transit_change.abs() - self.jitter_ticks) * ESTIMATOR_GAIN;
                let spacing = spacing.min(LOSS_SPACING_CAP * self.interval_ticks);
                if spacing > self.interval_ticks {
                    self.interval_ticks = spacing;
                } else {
                    self.interval_ticks += (spacing - self.interval_ticks) * ESTIMATOR_GAIN;
                }
                self.offset_ticks += ((tick - at) - self.offset_ticks) * ESTIMATOR_GAIN;
            }
            None => self.offset_ticks = tick - at,
        }
        self.newest = Some(Arrival { tick, at });
        self.dry = false;
    }

    /// The playout delay now, in server ticks.
    fn delay_ticks(&self) -> f64 {
        let margin = self.ticks(PLAYOUT_MARGIN);
        let floor = (self.interval_ticks + margin).max(self.ticks(MIN_PLAYOUT_DELAY));
        let ceiling = self.ticks(MAX_PLAYOUT_DELAY).max(floor);
        (self.interval_ticks + JITTER_MULTIPLE * self.jitter_ticks + margin).clamp(floor, ceiling)
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
            jitter: self.duration(self.jitter_ticks),
            snapshot_interval_ticks: self.interval_ticks,
            underruns: self.underruns,
            steps: self.steps,
        }
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

    /// **The delay is the interval, the jitter allowance and the margin,
    /// held between its floor and ceiling.** Huge jitter is capped at
    /// [`MAX_PLAYOUT_DELAY`]; an interval longer than that cap still gets a
    /// delay of its own length plus the margin; and a server ticking so fast
    /// that a tick and the margin fall under [`MIN_PLAYOUT_DELAY`] gets that.
    #[test]
    fn the_delay_is_held_between_its_floor_and_ceiling() {
        let mut jittery = Playout::new(60.0);
        jittery.jitter_ticks = 1000.0;
        assert_eq!(jittery.stats().delay, MAX_PLAYOUT_DELAY);

        let mut sparse = Playout::new(60.0);
        sparse.interval_ticks = 60.0;
        assert_eq!(
            sparse.stats().delay,
            Duration::from_secs(1) + PLAYOUT_MARGIN
        );

        let fast = Playout::new(10_000.0);
        assert_eq!(fast.stats().delay, MIN_PLAYOUT_DELAY);

        let mut plain = Playout::new(60.0);
        plain.jitter_ticks = 0.5;
        let expected = (1.0 + JITTER_MULTIPLE * 0.5) / 60.0 + PLAYOUT_MARGIN.as_secs_f64();
        assert!((plain.stats().delay.as_secs_f64() - expected).abs() < 1e-9);
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
