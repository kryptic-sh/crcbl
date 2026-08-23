//! The frame clock: a fixed-timestep accumulator for the server tick plus the
//! variable render delta.
//!
//! The loop shape this exists to enforce:
//!
//! ```
//! use crcbl_core::time::{FrameClock, MonotonicTime, TimeSource};
//!
//! let time = MonotonicTime::new();
//! let mut clock = FrameClock::new(60);
//!
//! # for _ in 0..1 {
//! clock.update(time.elapsed());        // once per frame
//! while clock.consume_tick() {
//!     // server_tick(clock.tick(), clock.tick_dt());  — always the same dt
//! }
//! // render(clock.alpha());            — interpolate between the last two ticks
//! # }
//! ```
//!
//! Simulation advances in exact, identical steps regardless of frame rate;
//! rendering runs as fast as it can and interpolates. The split is here in
//! stage 1 because it is nearly impossible to retrofit into a loop that grew
//! around `dt = last_frame_time`.
//!
//! # Determinism
//!
//! [`FrameClock::update`] takes the current time as an argument. It never reads
//! a clock itself, because a hidden `Instant::now()` is a nondeterminism source
//! and `docs/plan/12-testing.md` requires every one of them to be injected.
//! [`MonotonicTime`] is the real driver; [`ManualTime`] is the test one.

use core::fmt;
use core::time::Duration;
use std::time::Instant;

/// Identifier of a server tick.
///
/// A distinct type rather than a bare `u64` because the network protocol,
/// snapshots and (later) rollback all key off it, and confusing a tick id with
/// a frame counter or an index is exactly the bug that costs a day.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct TickId(u64);

impl TickId {
    /// The tick before any simulation has run.
    pub const ZERO: Self = Self(0);

    /// Wraps a raw tick number.
    #[inline]
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw tick number.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next tick.
    ///
    /// Saturates rather than wrapping; at 60 Hz, `u64::MAX` ticks is ~9.7
    /// billion years, so this is a formality.
    #[inline]
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Ticks elapsed from `earlier` to `self`, saturating at zero.
    #[inline]
    #[must_use]
    pub const fn since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

impl fmt::Display for TickId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tick {}", self.0)
    }
}

/// A source of monotonically increasing timestamps, measured from an arbitrary
/// start.
///
/// Implemented by [`MonotonicTime`] (real) and [`ManualTime`] (tests). The
/// clock itself takes timestamps as arguments, so this trait exists to make
/// "where does the time come from" an explicit choice at the call site.
pub trait TimeSource: fmt::Debug {
    /// Time elapsed since this source was created.
    fn elapsed(&self) -> Duration;
}

/// The real time source: [`Instant`] deltas from process start.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicTime {
    start: Instant,
}

impl MonotonicTime {
    /// Starts the clock now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for MonotonicTime {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeSource for MonotonicTime {
    #[inline]
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// A hand-driven time source for tests, replays and headless runs.
///
/// Nothing in the engine can tell it apart from [`MonotonicTime`], which is the
/// point: a deterministic test drives the same code the game does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ManualTime {
    now: Duration,
}

impl ManualTime {
    /// A source sitting at zero.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            now: Duration::ZERO,
        }
    }

    /// Moves time forward.
    #[inline]
    pub const fn advance(&mut self, delta: Duration) {
        self.now = self.now.saturating_add(delta);
    }

    /// Moves time forward by `seconds`.
    ///
    /// # Panics
    ///
    /// If `seconds` is negative or not finite.
    #[inline]
    pub fn advance_secs(&mut self, seconds: f64) {
        self.advance(Duration::from_secs_f64(seconds));
    }

    /// Jumps to an absolute timestamp.
    #[inline]
    pub const fn set(&mut self, now: Duration) {
        self.now = now;
    }
}

impl TimeSource for ManualTime {
    #[inline]
    fn elapsed(&self) -> Duration {
        self.now
    }
}

/// When an input event happened, on the engine's monotonic clock.
///
/// # The epoch is the contract
///
/// A timestamp whose origin is unspecified is worth nothing: you cannot
/// subtract two of them across subsystems, and you cannot compare one to
/// "now". So this type states its epoch and makes matching it a backend
/// obligation:
///
/// > An `EventTime` is a duration measured from the same origin as the
/// > [`TimeSource`] driving [`FrameClock::update`] — that is, from
/// > [`MonotonicTime::new`] (process start) in a real run, and from
/// > [`ManualTime`]'s zero in a deterministic one.
///
/// That makes `event.time` and `clock.render_dt()` live on one timeline, which
/// is what the P2 input pipeline needs to age an input against the tick it
/// belongs to, and what topic 26's client prediction needs to compute
/// input-to-photon latency.
///
/// # What backends have to do about it
///
/// No window system hands us this directly, so rebasing is part of writing a
/// backend, not an optimization:
///
/// * **Wayland** stamps events with `CLOCK_MONOTONIC` milliseconds
///   (microseconds under `wp_presentation` and the high-resolution input
///   protocols). Subtract the clock reading taken when the shell was created.
/// * **X11** stamps events with the X *server's* time: a 32-bit millisecond
///   counter with an unspecified origin that wraps every 49.7 days. The backend
///   estimates the offset once at connection time and must detect the wrap; a
///   backend that forwards the raw value produces an input pipeline that
///   silently breaks after a month and a half of uptime.
/// * **Win32** has `GetMessageTime` (a `GetTickCount` sample, also wrapping) and
///   raw input's `QPC` timestamps.
/// * **The browser** gives `event.timeStamp`, already a
///   `performance.now()`-relative double.
///
/// When a backend genuinely has no timestamp for an event — a synthesized
/// focus-loss key release, for instance — it stamps the event with the *current*
/// time rather than [`EventTime::ZERO`], because a zero timestamp reads as "this
/// happened at process start" to every consumer downstream.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct EventTime(Duration);

impl EventTime {
    /// The clock's origin.
    pub const ZERO: Self = Self(Duration::ZERO);

    /// Wraps a duration measured from the epoch described in the [type
    /// docs](Self).
    #[inline]
    #[must_use]
    pub const fn from_duration(since_epoch: Duration) -> Self {
        Self(since_epoch)
    }

    /// Microseconds since the epoch — the unit Wayland's high-resolution input
    /// protocols and libinput both use.
    #[inline]
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(Duration::from_micros(micros))
    }

    /// Milliseconds since the epoch — the unit core Wayland and X11 use.
    #[inline]
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(Duration::from_millis(millis))
    }

    /// Time since the epoch.
    #[inline]
    #[must_use]
    pub const fn as_duration(self) -> Duration {
        self.0
    }

    /// Time since the epoch in seconds.
    #[inline]
    #[must_use]
    pub fn as_secs_f64(self) -> f64 {
        self.0.as_secs_f64()
    }

    /// How long after `earlier` this timestamp is, saturating at zero.
    ///
    /// Saturating rather than panicking because event queues are not perfectly
    /// ordered in practice — a compositor can deliver a relative-motion event
    /// stamped fractionally before the absolute-motion event it accompanies —
    /// and an input pipeline must not die of it.
    #[inline]
    #[must_use]
    pub fn saturating_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// This timestamp moved forward by `delta`.
    #[inline]
    #[must_use]
    pub const fn saturating_add(self, delta: Duration) -> Self {
        Self(self.0.saturating_add(delta))
    }
}

impl From<Duration> for EventTime {
    #[inline]
    fn from(since_epoch: Duration) -> Self {
        Self(since_epoch)
    }
}

impl fmt::Display for EventTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t+{:.6}s", self.0.as_secs_f64())
    }
}

/// Default ticks the simulation may catch up in a single frame — see
/// [`FrameClock::set_max_catch_up_ticks`].
pub const DEFAULT_MAX_CATCH_UP_TICKS: u32 = 8;

/// Largest `f32` strictly below 1.0. [`FrameClock::alpha`] never returns 1.0
/// itself: alpha is a *fraction of the way to the next tick*, and 1.0 would
/// mean "at a tick that has not been simulated yet".
const ALPHA_MAX: f32 = 1.0 - f32::EPSILON / 2.0;

/// Fixed-timestep accumulator plus variable render delta.
///
/// See the [module docs](self) for the loop shape.
pub struct FrameClock {
    tick_nanos: u64,
    accumulator_nanos: u64,
    max_catch_up_ticks: u32,
    last_update: Option<Duration>,
    render_dt: Duration,
    tick: TickId,
    dropped_ticks: u64,
    backwards_updates: u64,
}

impl FrameClock {
    /// A clock running the simulation at `tick_hz`.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero, or over one tick per nanosecond — the period below
    /// is truncating integer nanoseconds, so a rate past 1 GHz rounds to a
    /// period of zero and there is no clock to build. It used to reach
    /// [`with_period`](Self::with_period) and fail there, blaming a period the
    /// caller never passed.
    #[must_use]
    pub fn new(tick_hz: u32) -> Self {
        assert!(tick_hz > 0, "tick rate must be positive");
        assert!(
            tick_hz <= 1_000_000_000,
            "tick rate must be at most 1 GHz, not {tick_hz} Hz"
        );
        // Truncating division leaves the step slightly short of 1/hz (16.666666
        // ms instead of 16.666667 ms at 60 Hz). The *simulation* is unaffected
        // — its second is 60 steps by definition — and wall-clock drift is
        // ~40 ns/s, which the accumulator absorbs. Use `with_period` if an
        // exact period matters.
        Self::with_period(Duration::from_nanos(u64::from(1_000_000_000u32 / tick_hz)))
    }

    /// A clock with an explicit tick period.
    ///
    /// # Panics
    ///
    /// If `period` is zero.
    #[must_use]
    pub fn with_period(period: Duration) -> Self {
        let tick_nanos = u64::try_from(period.as_nanos()).expect("tick period must fit in u64 ns");
        assert!(tick_nanos > 0, "tick period must be positive");
        Self {
            tick_nanos,
            accumulator_nanos: 0,
            max_catch_up_ticks: DEFAULT_MAX_CATCH_UP_TICKS,
            last_update: None,
            render_dt: Duration::ZERO,
            tick: TickId::ZERO,
            dropped_ticks: 0,
            backwards_updates: 0,
        }
    }

    /// Caps how many ticks a single [`FrameClock::update`] may queue.
    ///
    /// This is the spiral-of-death guard. If a frame takes longer than the
    /// ticks it queues take to simulate, the accumulator grows, which queues
    /// more ticks, which take longer still — the simulation never catches up
    /// and the game locks solid. Past the cap, surplus wall-clock time is
    /// **discarded**: those ticks never run, so simulation time falls
    /// permanently behind wall time (a hitch is a hitch — nothing rewinds it),
    /// but the frame rate recovers instead of collapsing. The count of skipped
    /// ticks is available from [`FrameClock::dropped_ticks`] and is worth
    /// logging; in a multiplayer build it is the client's cue that it has
    /// desynced from the server's tick timeline.
    ///
    /// The default, [`DEFAULT_MAX_CATCH_UP_TICKS`], allows 8 ticks — 133 ms of
    /// catch-up at 60 Hz, enough to absorb an asset-load or shader-compile
    /// hitch without letting a slow machine dig itself into a hole.
    ///
    /// # Panics
    ///
    /// If `ticks` is zero — a clock that can never tick is never what you want.
    #[inline]
    pub const fn set_max_catch_up_ticks(&mut self, ticks: u32) {
        assert!(ticks > 0, "max catch-up ticks must be positive");
        self.max_catch_up_ticks = ticks;
    }

    /// Feeds the clock the current timestamp. Call once per frame, before the
    /// tick loop.
    ///
    /// Timestamps must come from a [`TimeSource`]. A timestamp that moves
    /// backwards (a source that is not actually monotonic) contributes zero
    /// time and bumps [`FrameClock::backwards_updates`] rather than panicking
    /// or wrapping.
    pub fn update(&mut self, now: Duration) {
        let previous = self.last_update.unwrap_or(now);
        self.last_update = Some(now);
        if now < previous {
            self.backwards_updates += 1;
            self.render_dt = Duration::ZERO;
            return;
        }
        let delta = now - previous;
        self.render_dt = delta;

        let delta_nanos = u64::try_from(delta.as_nanos()).unwrap_or(u64::MAX);
        self.accumulator_nanos = self.accumulator_nanos.saturating_add(delta_nanos);

        // Drop whole ticks only, and only the surplus above the cap: the
        // sub-tick remainder survives, so `alpha` stays continuous across the
        // hitch instead of snapping to zero.
        let pending = self.accumulator_nanos / self.tick_nanos;
        let cap = u64::from(self.max_catch_up_ticks);
        if pending > cap {
            let dropped = pending - cap;
            self.dropped_ticks += dropped;
            self.accumulator_nanos -= dropped * self.tick_nanos;
        }
    }

    /// Takes one pending tick, if the accumulator holds a whole one.
    ///
    /// Drive it as `while clock.consume_tick() { server_tick(); }`.
    #[inline]
    pub const fn consume_tick(&mut self) -> bool {
        if self.accumulator_nanos >= self.tick_nanos {
            self.accumulator_nanos -= self.tick_nanos;
            self.tick = self.tick.next();
            true
        } else {
            false
        }
    }

    /// How many ticks [`FrameClock::consume_tick`] would still yield.
    #[inline]
    #[must_use]
    pub const fn pending_ticks(&self) -> u32 {
        (self.accumulator_nanos / self.tick_nanos) as u32
    }

    /// The id of the most recently simulated tick.
    #[inline]
    #[must_use]
    pub const fn tick(&self) -> TickId {
        self.tick
    }

    /// The fixed simulation step. Every tick advances the world by exactly
    /// this much, whatever the frame rate.
    #[inline]
    #[must_use]
    pub const fn tick_dt(&self) -> Duration {
        Duration::from_nanos(self.tick_nanos)
    }

    /// [`FrameClock::tick_dt`] in seconds, for integrators.
    #[inline]
    #[must_use]
    pub fn tick_dt_secs(&self) -> f64 {
        self.tick_nanos as f64 * 1e-9
    }

    /// Wall-clock time covered by the last [`FrameClock::update`] — the
    /// variable render delta, for camera smoothing, UI animation and anything
    /// else that is not simulation.
    #[inline]
    #[must_use]
    pub const fn render_dt(&self) -> Duration {
        self.render_dt
    }

    /// [`FrameClock::render_dt`] in seconds.
    #[inline]
    #[must_use]
    pub fn render_dt_secs(&self) -> f32 {
        self.render_dt.as_secs_f32()
    }

    /// How far the render frame sits between the last simulated tick and the
    /// next one, in `[0, 1)`.
    ///
    /// Interpolated rendering wants `lerp(previous_state, current_state,
    /// alpha)`. Call it *after* draining the tick loop; called before, the
    /// accumulator may still hold whole ticks, in which case this saturates
    /// just below 1.0 rather than reporting a fraction greater than one.
    #[inline]
    #[must_use]
    pub fn alpha(&self) -> f32 {
        let alpha = self.accumulator_nanos as f32 / self.tick_nanos as f32;
        if alpha >= 1.0 { ALPHA_MAX } else { alpha }
    }

    /// Ticks discarded by the catch-up cap since the clock was created.
    #[inline]
    #[must_use]
    pub const fn dropped_ticks(&self) -> u64 {
        self.dropped_ticks
    }

    /// Number of [`FrameClock::update`] calls whose timestamp went backwards.
    /// Non-zero means the time source is broken.
    #[inline]
    #[must_use]
    pub const fn backwards_updates(&self) -> u64 {
        self.backwards_updates
    }
}

impl fmt::Debug for FrameClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameClock")
            .field("tick", &self.tick)
            .field("tick_dt", &self.tick_dt())
            .field("pending_ticks", &self.pending_ticks())
            .field("render_dt", &self.render_dt)
            .field("dropped_ticks", &self.dropped_ticks)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick_count(clock: &mut FrameClock) -> u32 {
        let mut ticks = 0;
        while clock.consume_tick() {
            ticks += 1;
        }
        ticks
    }

    #[test]
    fn tick_ids_are_ordered_and_saturating() {
        assert_eq!(TickId::ZERO.get(), 0);
        assert_eq!(TickId::ZERO.next(), TickId::from_raw(1));
        assert!(TickId::from_raw(2) > TickId::from_raw(1));
        assert_eq!(TickId::from_raw(9).since(TickId::from_raw(4)), 5);
        assert_eq!(TickId::from_raw(4).since(TickId::from_raw(9)), 0);
        assert_eq!(TickId::from_raw(u64::MAX).next().get(), u64::MAX);
        assert_eq!(TickId::from_raw(7).to_string(), "tick 7");
    }

    #[test]
    fn manual_time_drives_the_clock() {
        let mut time = ManualTime::new();
        let mut clock = FrameClock::new(60);
        assert_eq!(clock.tick(), TickId::ZERO);

        // First update establishes the baseline; no time has passed yet.
        clock.update(time.elapsed());
        assert_eq!(tick_count(&mut clock), 0);
        assert_eq!(clock.render_dt(), Duration::ZERO);

        time.advance_secs(1.0);
        clock.update(time.elapsed());
        assert_eq!(clock.render_dt(), Duration::from_secs(1));
        // One second of catch-up is far past the cap: 60 ticks' worth of time
        // arrived, 8 run, 52 are dropped.
        assert_eq!(tick_count(&mut clock), DEFAULT_MAX_CATCH_UP_TICKS);
        assert_eq!(clock.dropped_ticks(), 52);
    }

    #[test]
    fn steady_frame_rate_produces_exactly_one_tick_per_frame() {
        let mut time = ManualTime::new();
        let mut clock = FrameClock::new(60);
        clock.update(time.elapsed());

        let mut ticks = 0;
        for _ in 0..600 {
            time.advance(clock.tick_dt());
            clock.update(time.elapsed());
            ticks += tick_count(&mut clock);
        }
        assert_eq!(ticks, 600);
        assert_eq!(clock.tick(), TickId::from_raw(600));
        assert_eq!(clock.dropped_ticks(), 0);
    }

    /// A render frame rate that is not a multiple of the tick rate must still
    /// average out to the right number of ticks.
    #[test]
    fn tick_count_matches_elapsed_time_at_an_awkward_frame_rate() {
        let mut time = ManualTime::new();
        let mut clock = FrameClock::new(60);
        clock.update(time.elapsed());

        let frame = Duration::from_nanos(1_000_000_000 / 144);
        let frames = 1440u32; // exactly 10 seconds at 144 Hz
        let mut ticks = 0;
        for _ in 0..frames {
            time.advance(frame);
            clock.update(time.elapsed());
            ticks += tick_count(&mut clock);
        }
        let expected = (frame * frames).as_nanos() / clock.tick_dt().as_nanos();
        assert_eq!(u128::from(ticks), expected);
        assert_eq!(clock.dropped_ticks(), 0);
    }

    /// Half the tick rate: two frames per tick, never more.
    #[test]
    fn slow_frames_never_queue_more_than_the_elapsed_time() {
        let mut time = ManualTime::new();
        let mut clock = FrameClock::new(60);
        clock.update(time.elapsed());
        for _ in 0..10 {
            time.advance(clock.tick_dt() * 2);
            clock.update(time.elapsed());
            assert_eq!(tick_count(&mut clock), 2);
        }
    }

    #[test]
    fn alpha_stays_in_range_and_tracks_the_partial_tick() {
        let mut time = ManualTime::new();
        let mut clock = FrameClock::new(60);
        clock.update(time.elapsed());
        assert_eq!(clock.alpha(), 0.0);

        // Quarter of a tick: no tick runs, alpha reports the fraction.
        time.advance(clock.tick_dt() / 4);
        clock.update(time.elapsed());
        assert_eq!(tick_count(&mut clock), 0);
        assert!((clock.alpha() - 0.25).abs() < 1e-6, "{}", clock.alpha());

        // Alpha before draining the tick loop saturates below 1.0.
        time.advance(clock.tick_dt() * 3);
        clock.update(time.elapsed());
        assert!(clock.alpha() < 1.0);
        assert!(clock.alpha() >= 0.0);
        assert_eq!(clock.alpha(), ALPHA_MAX);
        tick_count(&mut clock);
        assert!((clock.alpha() - 0.25).abs() < 1e-6);

        // Fuzz the invariant across a jittery frame sequence.
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..5_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            time.advance(Duration::from_nanos(seed % 40_000_000));
            clock.update(time.elapsed());
            let before = clock.alpha();
            assert!(
                (0.0..1.0).contains(&before),
                "alpha {before} out of range before draining"
            );
            tick_count(&mut clock);
            let after = clock.alpha();
            assert!(
                (0.0..1.0).contains(&after),
                "alpha {after} out of range after draining"
            );
        }
    }

    #[test]
    fn spiral_of_death_cap_engages_and_reports() {
        let mut time = ManualTime::new();
        // An exact 100 Hz period keeps the arithmetic in the test obvious.
        let mut clock = FrameClock::with_period(Duration::from_millis(10));
        clock.set_max_catch_up_ticks(4);
        clock.update(time.elapsed());

        // A ten-second stall (1000 ticks' worth) yields 4 ticks, not 1000.
        time.advance_secs(10.0);
        clock.update(time.elapsed());
        assert_eq!(tick_count(&mut clock), 4);
        assert_eq!(clock.dropped_ticks(), 996);
        assert_eq!(clock.tick(), TickId::from_raw(4));
        assert!(clock.alpha() < 1.0);

        // Dropping keeps the sub-tick remainder rather than snapping it away.
        time.advance(Duration::from_millis(25));
        clock.update(time.elapsed());
        assert_eq!(tick_count(&mut clock), 2);
        assert!((clock.alpha() - 0.5).abs() < 1e-6);

        // And the clock keeps working normally afterwards.
        assert_eq!(clock.dropped_ticks(), 996);
    }

    #[test]
    fn non_monotonic_timestamps_are_absorbed() {
        let mut clock = FrameClock::new(60);
        clock.update(Duration::from_secs(10));
        clock.update(Duration::from_secs(5));
        assert_eq!(clock.backwards_updates(), 1);
        assert_eq!(tick_count(&mut clock), 0);
        assert_eq!(clock.render_dt(), Duration::ZERO);
        // Recovers from the (bogus) new baseline.
        clock.update(Duration::from_secs(5) + Duration::from_millis(20));
        assert_eq!(tick_count(&mut clock), 1);
    }

    #[test]
    fn tick_rate_and_period_agree() {
        let by_rate = FrameClock::new(60);
        let by_period = FrameClock::with_period(Duration::from_nanos(16_666_666));
        assert_eq!(by_rate.tick_dt(), by_period.tick_dt());
        assert!((by_rate.tick_dt_secs() - 1.0 / 60.0).abs() < 1e-6);

        let physics = FrameClock::new(240);
        assert_eq!(physics.tick_dt(), Duration::from_nanos(4_166_666));
        assert!(format!("{physics:?}").contains("FrameClock"));
    }

    #[test]
    fn manual_time_can_be_set_absolutely() {
        let mut time = ManualTime::new();
        assert_eq!(time.elapsed(), Duration::ZERO);
        time.set(Duration::from_secs(90));
        time.advance(Duration::from_millis(500));
        assert_eq!(time.elapsed(), Duration::from_millis(90_500));

        let mut clock = FrameClock::with_period(Duration::from_millis(100));
        clock.update(time.elapsed());
        time.advance_secs(0.35);
        clock.update(time.elapsed());
        assert_eq!(clock.pending_ticks(), 3);
        assert!((clock.render_dt_secs() - 0.35).abs() < 1e-6);
        assert_eq!(tick_count(&mut clock), 3);
        assert_eq!(clock.pending_ticks(), 0);
    }

    #[test]
    fn monotonic_time_moves_forward() {
        let time = MonotonicTime::new();
        let first = time.elapsed();
        let second = time.elapsed();
        assert!(second >= first);
        // The clock accepts a real source with no ceremony.
        let mut clock = FrameClock::new(60);
        clock.update(time.elapsed());
        assert!(clock.pending_ticks() < 2);
    }

    #[test]
    #[should_panic(expected = "tick rate must be positive")]
    fn zero_tick_rate_is_rejected() {
        let _ = FrameClock::new(0);
    }

    /// The other end of the rate range, which used to fail as a period.
    ///
    /// One tick per nanosecond is the last rate with a period to round to; the
    /// next one up truncates to zero nanoseconds, and the clock that came back
    /// blamed `with_period` for a `Duration` the caller had not written.
    #[test]
    #[should_panic(expected = "tick rate must be at most 1 GHz")]
    fn a_tick_rate_finer_than_a_nanosecond_is_rejected() {
        let _ = FrameClock::new(1_000_000_001);
    }

    /// …and exactly 1 GHz still builds, so the bound above is the right one.
    #[test]
    fn one_tick_per_nanosecond_is_the_fastest_clock() {
        let clock = FrameClock::new(1_000_000_000);
        assert_eq!(clock.tick_dt(), Duration::from_nanos(1));
    }

    #[test]
    fn event_times_share_the_time_sources_epoch() {
        let mut time = ManualTime::new();
        time.advance_secs(1.5);

        // The load-bearing property: a timestamp taken from the same source the
        // frame clock is driven by is directly comparable with it.
        let pressed = EventTime::from_duration(time.elapsed());
        time.advance(Duration::from_millis(120));
        let released = EventTime::from_duration(time.elapsed());

        assert_eq!(
            released.saturating_since(pressed),
            Duration::from_millis(120)
        );
        // Out-of-order delivery saturates instead of panicking.
        assert_eq!(pressed.saturating_since(released), Duration::ZERO);

        assert_eq!(EventTime::from_millis(1500), pressed);
        assert_eq!(EventTime::from_micros(1_500_000), pressed);
        assert_eq!(EventTime::from(Duration::from_millis(1500)), pressed);
        assert!((pressed.as_secs_f64() - 1.5).abs() < 1e-9);
        assert_eq!(pressed.as_duration(), Duration::from_millis(1500));
        assert!(EventTime::ZERO < pressed);
        assert_eq!(EventTime::default(), EventTime::ZERO);
        assert_eq!(
            EventTime::ZERO.saturating_add(Duration::from_secs(2)),
            EventTime::from_millis(2000)
        );
        assert_eq!(pressed.to_string(), "t+1.500000s");
    }
}
