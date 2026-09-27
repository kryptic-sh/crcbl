//! Fixed-rate sampling at exact sub-tick times, staggered per agent.
//!
//! A [`FixedRateSchedule`] answers one question for one agent: *during the
//! stretch of simulated time I was just handed, at which instants was I due to
//! sample?* An AI that perceives at a fixed rate, a sensor that polls, a
//! beacon that pings — each owns a schedule, feeds it the tick's elapsed time,
//! and gets back every sample that fell inside it, with the exact simulation
//! time it was scheduled for and how far into the update that was.
//!
//! ```
//! use crcbl_core::schedule::{FixedRateSchedule, FixedRateScheduleConfig};
//!
//! let config = FixedRateScheduleConfig { interval_seconds: 0.1 };
//! // Agents are staggered so they do not all sample on the same tick.
//! let mut schedule = FixedRateSchedule::new(config, 0.025)?;
//!
//! let mut samples = Vec::new();
//! schedule.advance(0.2, |sample| samples.push(sample))?;
//! let times: Vec<f64> = samples
//!     .iter()
//!     .map(|sample| sample.scheduled_simulation_time_seconds)
//!     .collect();
//! assert_eq!(times, [0.025, 0.125]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Ported from EW, where it was `PerceptionSchedule`. Nothing in it knows about
//! perception, so the engine names drop the word; every method, field, error
//! variant and signature is otherwise EW's own.
//!
//! # Why this is not the frame clock
//!
//! [`FrameClock`](crate::time::FrameClock) is the simulation's own tick: one
//! clock per loop, integer nanoseconds, driven by wall time, dropping ticks past
//! its catch-up cap. This is the rung below it. It runs *inside* a tick, one per
//! agent, at a rate that has nothing to do with the tick rate — a 10 Hz sense
//! inside a 60 Hz simulation — and it reports samples at times *between* tick
//! boundaries, so an agent can interpolate what it saw to the instant it looked.
//! It never drops a sample: an update long enough to span several samples emits
//! all of them, and time the agent genuinely was not running for goes through
//! [`skip_elapsed`](FixedRateSchedule::skip_elapsed) instead.
//!
//! # Determinism
//!
//! Sample times are a pure function of the configuration, never of how the
//! elapsed time was chopped up. The schedule counts samples with an integer
//! index `k` and the `k`th sample is scheduled at exactly
//!
//! ```text
//! phase_offset_seconds + (k as f64) * interval_seconds
//! ```
//!
//! — one multiplication and one addition, each rounded once, which IEEE-754
//! pins down on every target. The time is *not* accumulated by adding the
//! interval to the previous sample time: that sum picks up a rounding error per
//! sample, so the millionth sample of a long run would drift from where the
//! formula puts it, and a schedule resumed by [`skip_elapsed`]
//! (which has to jump straight to a later sample) would land somewhere a
//! schedule that advanced continuously never reaches. EW's original did
//! accumulate; the port changed only this, and every one of EW's tests still
//! holds.
//!
//! The index is exact while it fits an `f64`'s mantissa, which
//! [`MAX_SAMPLE_INDEX`] bounds; [`skip_elapsed`] refuses a jump past it, and
//! [`advance`](FixedRateSchedule::advance) only ever steps it by one per
//! emitted sample, so no run reaches it by advancing.
//!
//! The *elapsed* time an update covers is still accumulated — it is whatever
//! the caller passes — so an update's end can land a rounding error short of a
//! sample that is, in intent, exactly on it (ten updates of `0.1` sum to just
//! below `1.0`). A sample within [`BOUNDARY_TOLERANCE_SCALE`] of the update's
//! end, relative to the larger of the two magnitudes (and to one second, below
//! it), is therefore due in *this* update rather than the next. Without it the
//! sample on the boundary would fire one update late, on some partitions of
//! the same time and not others.
//!
//! Within one update samples are emitted in increasing scheduled time, and each
//! one's `offset_within_update_seconds` is measured from the update's start.
//!
//! [`skip_elapsed`]: FixedRateSchedule::skip_elapsed

use core::fmt;

/// The sample interval a default configuration uses: ten samples a second.
pub const DEFAULT_INTERVAL_SECONDS: f64 = 0.1;

/// How close, relative to the times involved, a sample has to be to an
/// update's end to count as due in that update — see the [module
/// docs](self#determinism) for why there is a tolerance at all.
///
/// Far above an `f64`'s own rounding error, so a sum of update lengths that
/// meant to land on a sample always reaches it, and far below any interval a
/// schedule is useful at. An interval under this scale of the current time
/// cannot be told apart from the boundary it straddles, which is why
/// [`FixedRateSchedule::skip_elapsed`] refuses one.
pub const BOUNDARY_TOLERANCE_SCALE: f64 = 1e-12;

/// The largest sample index the schedule will hold: `2^f64::MANTISSA_DIGITS`,
/// the last integer from which every smaller one converts to an `f64` exactly.
///
/// Past it, two indices map to one `f64` and the time formula in the [module
/// docs](self#determinism) stops being exact, so
/// [`FixedRateSchedule::skip_elapsed`] refuses to jump there.
pub const MAX_SAMPLE_INDEX: u64 = 1 << f64::MANTISSA_DIGITS;

/// Configurable spacing between samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedRateScheduleConfig {
    /// Seconds of simulation time between one sample and the next. Finite and
    /// positive; [`is_valid`](Self::is_valid) says whether it is.
    pub interval_seconds: f64,
}

impl Default for FixedRateScheduleConfig {
    /// [`DEFAULT_INTERVAL_SECONDS`].
    fn default() -> Self {
        Self {
            interval_seconds: DEFAULT_INTERVAL_SECONDS,
        }
    }
}

impl FixedRateScheduleConfig {
    /// Whether the interval is finite and positive — what
    /// [`FixedRateSchedule::new`] requires.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.interval_seconds.is_finite() && self.interval_seconds > 0.0
    }
}

/// Construction rejected invalid sample timing or phase offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixedRateScheduleConfigError {
    /// The interval is not finite and positive.
    InvalidInterval,
    /// The phase offset is not finite, or falls outside `[0, interval)`.
    InvalidPhaseOffset,
}

impl fmt::Display for FixedRateScheduleConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInterval => "sample interval must be finite and positive",
            Self::InvalidPhaseOffset => {
                "phase offset must be finite, at least zero and less than the interval"
            }
        })
    }
}

impl std::error::Error for FixedRateScheduleConfigError {}

/// An elapsed-time update was rejected without changing the schedule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixedRateScheduleAdvanceError {
    /// The elapsed time is negative or not finite, the time it would reach is
    /// not finite, or — for [`FixedRateSchedule::skip_elapsed`] — the next
    /// sample after it cannot be represented apart from the skip's end.
    InvalidElapsed,
}

impl fmt::Display for FixedRateScheduleAdvanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidElapsed => {
                "elapsed time must be finite, not negative, and reach a representable next sample"
            }
        })
    }
}

impl std::error::Error for FixedRateScheduleAdvanceError {}

/// One scheduled sample within an [`advance`](FixedRateSchedule::advance) call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedRateSample {
    /// The simulation time the sample was scheduled for, on the schedule's own
    /// clock (zero at construction).
    pub scheduled_simulation_time_seconds: f64,
    /// How far into this update the sample falls, from the update's start —
    /// what an agent interpolates its view of the world by.
    pub offset_within_update_seconds: f64,
    /// Time since the previous sample, or since construction or the end of the
    /// last [`skip_elapsed`](FixedRateSchedule::skip_elapsed) for the first
    /// sample after either. What an agent decays its senses by.
    pub elapsed_since_previous_sample_seconds: f64,
}

/// Per-agent fixed-rate sample schedule.
///
/// See the [module docs](self) for what it is for and what it guarantees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedRateSchedule {
    config: FixedRateScheduleConfig,
    phase_offset_seconds: f64,
    simulation_time_seconds: f64,
    last_sample_time_seconds: f64,
    /// `k` in the time formula of the [module docs](self#determinism): the
    /// index of the next sample not yet emitted or skipped.
    next_sample_index: u64,
}

impl Default for FixedRateSchedule {
    /// [`FixedRateScheduleConfig::default`] with no phase offset.
    fn default() -> Self {
        Self::new(FixedRateScheduleConfig::default(), 0.0)
            .expect("default fixed-rate schedule is valid")
    }
}

impl FixedRateSchedule {
    /// A schedule at simulation time zero, sampling every
    /// `config.interval_seconds` starting at `phase_offset_seconds`.
    ///
    /// Giving each agent a different phase in `[0, interval)` spreads their
    /// samples across the interval instead of stacking them on one tick. A
    /// phase of zero first samples one whole interval in, not at time zero, so
    /// a freshly built agent does not sample before anything has happened.
    ///
    /// # Errors
    ///
    /// [`InvalidInterval`](FixedRateScheduleConfigError::InvalidInterval) if
    /// the interval is not [valid](FixedRateScheduleConfig::is_valid), and
    /// [`InvalidPhaseOffset`](FixedRateScheduleConfigError::InvalidPhaseOffset)
    /// if the phase is not finite or falls outside `[0, interval)`.
    pub fn new(
        config: FixedRateScheduleConfig,
        phase_offset_seconds: f64,
    ) -> Result<Self, FixedRateScheduleConfigError> {
        if !config.is_valid() {
            return Err(FixedRateScheduleConfigError::InvalidInterval);
        }
        if !phase_offset_seconds.is_finite()
            || phase_offset_seconds < 0.0
            || phase_offset_seconds >= config.interval_seconds
        {
            return Err(FixedRateScheduleConfigError::InvalidPhaseOffset);
        }
        // Sample zero sits at the phase itself, which for a zero phase is time
        // zero — the sample the doc comment above promises not to take.
        let next_sample_index = u64::from(phase_offset_seconds == 0.0);
        Ok(Self {
            config,
            phase_offset_seconds,
            simulation_time_seconds: 0.0,
            last_sample_time_seconds: 0.0,
            next_sample_index,
        })
    }

    /// The phase this schedule was built with.
    #[must_use]
    pub fn phase_offset_seconds(&self) -> f64 {
        self.phase_offset_seconds
    }

    /// Simulation time this schedule has been advanced or skipped to.
    #[must_use]
    pub fn simulation_time_seconds(&self) -> f64 {
        self.simulation_time_seconds
    }

    /// Advances the local clock without emitting samples for unavailable time.
    ///
    /// The next sample remains on the configured phase — exactly the one a
    /// schedule [advanced](Self::advance) through the same time would reach
    /// next — while its elapsed baseline starts at the skipped endpoint, so
    /// sensory state cannot replay historical samples when an agent resumes.
    ///
    /// # Errors
    ///
    /// [`InvalidElapsed`](FixedRateScheduleAdvanceError::InvalidElapsed),
    /// leaving the schedule untouched, if `elapsed_seconds` is negative or not
    /// finite, if the time it reaches is not finite, or if the next sample
    /// after it is past [`MAX_SAMPLE_INDEX`] or within
    /// [`BOUNDARY_TOLERANCE_SCALE`] of the skip's end — an interval too small
    /// for the time reached to tell one sample from the next.
    pub fn skip_elapsed(
        &mut self,
        elapsed_seconds: f64,
    ) -> Result<(), FixedRateScheduleAdvanceError> {
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            return Err(FixedRateScheduleAdvanceError::InvalidElapsed);
        }
        if elapsed_seconds == 0.0 {
            return Ok(());
        }
        let skipped_end_seconds = self.simulation_time_seconds + elapsed_seconds;
        if !skipped_end_seconds.is_finite() {
            return Err(FixedRateScheduleAdvanceError::InvalidElapsed);
        }

        let mut next_sample_index = self.next_sample_index;
        if due_at_or_before(
            self.sample_time_seconds(next_sample_index),
            skipped_end_seconds,
        ) {
            // The first index strictly after the end, in exact arithmetic. The
            // quotient is rounded, so the estimate can sit one either side of
            // the index advancing would reach; the two corrections below put it
            // back, and anything they cannot fix is an interval too small to
            // schedule at this time and is refused below.
            let estimate = ((skipped_end_seconds - self.phase_offset_seconds)
                / self.config.interval_seconds)
                .floor()
                + 1.0;
            // A power of two, so the conversion is exact.
            #[allow(clippy::cast_precision_loss)]
            let max_index = MAX_SAMPLE_INDEX as f64;
            // An interval tiny against the time reached overflows the quotient.
            if !estimate.is_finite() || estimate > max_index {
                return Err(FixedRateScheduleAdvanceError::InvalidElapsed);
            }
            // Integral and at most the bound, so the conversion is exact; a
            // negative estimate (an end a rounding error before the phase)
            // saturates to zero, and `max` then keeps the index moving forward
            // past the sample that was due.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let estimate = estimate as u64;
            next_sample_index = estimate.max(next_sample_index + 1);
            if next_sample_index - 1 > self.next_sample_index
                && !due_at_or_before(
                    self.sample_time_seconds(next_sample_index - 1),
                    skipped_end_seconds,
                )
            {
                next_sample_index -= 1;
            }
            if due_at_or_before(
                self.sample_time_seconds(next_sample_index),
                skipped_end_seconds,
            ) {
                next_sample_index += 1;
            }
        }
        let next_sample_time_seconds = self.sample_time_seconds(next_sample_index);
        if next_sample_index > MAX_SAMPLE_INDEX
            || !next_sample_time_seconds.is_finite()
            || due_at_or_before(next_sample_time_seconds, skipped_end_seconds)
        {
            return Err(FixedRateScheduleAdvanceError::InvalidElapsed);
        }

        self.simulation_time_seconds = skipped_end_seconds;
        self.last_sample_time_seconds = skipped_end_seconds;
        self.next_sample_index = next_sample_index;
        Ok(())
    }

    /// Invokes `emit` once for every sample due in this elapsed-time update,
    /// in increasing scheduled time.
    ///
    /// A sample is due when it is at or before the update's end, or within
    /// [`BOUNDARY_TOLERANCE_SCALE`] of it; see the [module
    /// docs](self#determinism). An update spanning several intervals emits
    /// every sample in it — none are dropped or merged.
    ///
    /// # Errors
    ///
    /// [`InvalidElapsed`](FixedRateScheduleAdvanceError::InvalidElapsed),
    /// leaving the schedule untouched and emitting nothing, if
    /// `elapsed_seconds` is negative or not finite, or if the time it reaches
    /// is not finite.
    pub fn advance(
        &mut self,
        elapsed_seconds: f64,
        mut emit: impl FnMut(FixedRateSample),
    ) -> Result<(), FixedRateScheduleAdvanceError> {
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            return Err(FixedRateScheduleAdvanceError::InvalidElapsed);
        }
        let update_start_seconds = self.simulation_time_seconds;
        let update_end_seconds = update_start_seconds + elapsed_seconds;
        if !update_end_seconds.is_finite() {
            return Err(FixedRateScheduleAdvanceError::InvalidElapsed);
        }

        loop {
            let scheduled_simulation_time_seconds = self.next_sample_time_seconds();
            if !due_at_or_before(scheduled_simulation_time_seconds, update_end_seconds) {
                break;
            }
            emit(FixedRateSample {
                scheduled_simulation_time_seconds,
                offset_within_update_seconds: scheduled_simulation_time_seconds
                    - update_start_seconds,
                elapsed_since_previous_sample_seconds: scheduled_simulation_time_seconds
                    - self.last_sample_time_seconds,
            });
            self.last_sample_time_seconds = scheduled_simulation_time_seconds;
            // One step per emitted sample from at most `MAX_SAMPLE_INDEX`,
            // which `new` and `skip_elapsed` both hold to, so this cannot
            // overflow in any run that terminates.
            self.next_sample_index += 1;
        }
        self.simulation_time_seconds = update_end_seconds;
        Ok(())
    }

    /// When the next sample not yet emitted or skipped is scheduled.
    fn next_sample_time_seconds(&self) -> f64 {
        self.sample_time_seconds(self.next_sample_index)
    }

    /// The time formula of the [module docs](self#determinism).
    fn sample_time_seconds(&self, index: u64) -> f64 {
        // Exact for every index up to `MAX_SAMPLE_INDEX`, the only ones the
        // schedule holds.
        #[allow(clippy::cast_precision_loss)]
        let index = index as f64;
        self.phase_offset_seconds + index * self.config.interval_seconds
    }
}

/// Whether a sample scheduled at `scheduled_seconds` is due in an update
/// ending at `update_end_seconds`: at or before it, or within
/// [`BOUNDARY_TOLERANCE_SCALE`] of it relative to the larger magnitude, and
/// never less than that scale of one second.
fn due_at_or_before(scheduled_seconds: f64, update_end_seconds: f64) -> bool {
    scheduled_seconds <= update_end_seconds
        || scheduled_seconds - update_end_seconds
            <= BOUNDARY_TOLERANCE_SCALE
                * scheduled_seconds
                    .abs()
                    .max(update_end_seconds.abs())
                    .max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-12;

    fn event_times(events: &[FixedRateSample]) -> Vec<f64> {
        events
            .iter()
            .map(|event| event.scheduled_simulation_time_seconds)
            .collect()
    }

    fn assert_times_match(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < EPSILON);
        }
    }

    /// Every sample `schedule` emits across `steps`, in order.
    fn collect(schedule: &mut FixedRateSchedule, steps: &[f64]) -> Vec<FixedRateSample> {
        let mut events = Vec::new();
        for &step in steps {
            schedule.advance(step, |event| events.push(event)).unwrap();
        }
        events
    }

    // ----- EW's tests, renamed and otherwise unchanged -----

    #[test]
    fn default_schedule_emits_at_ten_hertz_boundaries() {
        let mut schedule = FixedRateSchedule::default();
        let mut events = Vec::new();

        schedule.advance(0.099, |event| events.push(event)).unwrap();
        assert!(events.is_empty());

        schedule.advance(0.001, |event| events.push(event)).unwrap();
        assert_eq!(events.len(), 1);
        assert!((events[0].scheduled_simulation_time_seconds - 0.1).abs() < EPSILON);
        assert!((events[0].offset_within_update_seconds - 0.001).abs() < EPSILON);
        assert!((events[0].elapsed_since_previous_sample_seconds - 0.1).abs() < EPSILON);

        events.clear();
        schedule.advance(0.2, |event| events.push(event)).unwrap();
        assert_times_match(&event_times(&events), &[0.2, 0.3]);
    }

    #[test]
    fn coarse_and_partitioned_updates_emit_the_same_schedule() {
        let mut coarse = FixedRateSchedule::default();
        let mut partitioned = FixedRateSchedule::default();
        let mut coarse_events = Vec::new();
        let mut partitioned_events = Vec::new();

        coarse
            .advance(0.35, |event| coarse_events.push(event))
            .unwrap();
        for _ in 0..7 {
            partitioned
                .advance(0.05, |event| partitioned_events.push(event))
                .unwrap();
        }

        assert_times_match(
            &event_times(&coarse_events),
            &event_times(&partitioned_events),
        );
        assert!(
            (coarse.simulation_time_seconds() - partitioned.simulation_time_seconds()).abs()
                < EPSILON
        );
    }

    #[test]
    fn phase_offsets_stagger_the_first_and_later_samples() {
        let mut schedule = FixedRateSchedule::new(FixedRateScheduleConfig::default(), 0.025)
            .expect("phase within the interval");
        let mut events = Vec::new();

        schedule.advance(0.024, |event| events.push(event)).unwrap();
        assert!(events.is_empty());
        schedule.advance(0.001, |event| events.push(event)).unwrap();
        assert_times_match(&event_times(&events), &[0.025]);
        assert!((events[0].elapsed_since_previous_sample_seconds - 0.025).abs() < EPSILON);

        events.clear();
        schedule.advance(0.1, |event| events.push(event)).unwrap();
        assert_times_match(&event_times(&events), &[0.125]);
    }

    #[test]
    fn invalid_configuration_and_elapsed_time_preserve_schedule_state() {
        let invalid_interval = FixedRateScheduleConfig {
            interval_seconds: 0.0,
        };
        assert_eq!(
            FixedRateSchedule::new(invalid_interval, 0.0),
            Err(FixedRateScheduleConfigError::InvalidInterval)
        );
        assert_eq!(
            FixedRateSchedule::new(FixedRateScheduleConfig::default(), 0.1),
            Err(FixedRateScheduleConfigError::InvalidPhaseOffset)
        );

        let mut schedule = FixedRateSchedule::default();
        schedule.advance(0.04, |_| {}).unwrap();
        let before = schedule;
        assert_eq!(
            schedule.advance(f64::NAN, |_| {}),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(schedule, before);
        assert_eq!(
            schedule.advance(-0.1, |_| {}),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(schedule, before);
        assert_eq!(
            schedule.skip_elapsed(f64::INFINITY),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(schedule, before);
    }

    #[test]
    fn skipping_elapsed_time_keeps_phase_without_replaying_samples() {
        let mut schedule = FixedRateSchedule::new(
            FixedRateScheduleConfig {
                interval_seconds: 0.25,
            },
            0.1,
        )
        .unwrap();
        schedule.advance(0.05, |_| {}).unwrap();
        schedule.skip_elapsed(0.96).unwrap();

        assert!((schedule.simulation_time_seconds - 1.01).abs() < EPSILON);
        assert!((schedule.last_sample_time_seconds - 1.01).abs() < EPSILON);
        assert!((schedule.next_sample_time_seconds() - 1.1).abs() < EPSILON);

        let mut events = Vec::new();
        schedule.advance(0.09, |event| events.push(event)).unwrap();
        assert_times_match(&event_times(&events), &[1.1]);
        assert!((events[0].elapsed_since_previous_sample_seconds - 0.09).abs() < EPSILON);
    }

    #[test]
    fn skipping_unrepresentable_or_tolerance_due_time_preserves_schedule() {
        let mut tiny_interval = FixedRateSchedule::new(
            FixedRateScheduleConfig {
                interval_seconds: f64::MIN_POSITIVE,
            },
            0.0,
        )
        .unwrap();
        let tiny_before = tiny_interval;
        assert_eq!(
            tiny_interval.skip_elapsed(1.0),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(tiny_interval, tiny_before);

        let mut tolerance_due = FixedRateSchedule::new(
            FixedRateScheduleConfig {
                interval_seconds: 1e-13,
            },
            0.0,
        )
        .unwrap();
        let tolerance_before = tolerance_due;
        assert_eq!(
            tolerance_due.skip_elapsed(1.0),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(tolerance_due, tolerance_before);
    }

    #[test]
    fn skipping_a_large_elapsed_interval_uses_a_future_sample_boundary() {
        let mut schedule = FixedRateSchedule::default();
        schedule.skip_elapsed(1_000_000_000.0).unwrap();
        assert!(!due_at_or_before(
            schedule.next_sample_time_seconds(),
            schedule.simulation_time_seconds
        ));
        assert_eq!(
            schedule.last_sample_time_seconds,
            schedule.simulation_time_seconds
        );
    }

    // ----- The engine's own -----

    #[test]
    fn every_sample_lands_exactly_on_the_index_formula_over_a_long_run() {
        // Ten minutes of a 60 Hz simulation: thousands of samples, far more
        // than enough for an accumulated time to drift off the formula.
        let phase = 0.03;
        let config = FixedRateScheduleConfig::default();
        let mut schedule = FixedRateSchedule::new(config, phase).unwrap();
        let events = collect(&mut schedule, &[1.0 / 60.0; 60 * 600]);

        assert!(events.len() > 5000, "the run emitted {}", events.len());
        for (k, event) in events.iter().enumerate() {
            let expected = phase + k as f64 * config.interval_seconds;
            assert_eq!(
                event.scheduled_simulation_time_seconds.to_bits(),
                expected.to_bits(),
                "sample {k}"
            );
        }
    }

    #[test]
    fn a_sample_on_the_update_boundary_fires_in_that_update() {
        // Summed updates of the interval land a rounding error short of some
        // samples that are, in intent, exactly on their ends (ten of them sum
        // to just under 1.0); every sample is still due in its own update.
        let mut schedule = FixedRateSchedule::default();
        let mut per_update = Vec::new();
        for _ in 0..10 {
            let mut count = 0;
            schedule.advance(0.1, |_| count += 1).unwrap();
            per_update.push(count);
        }
        assert!(schedule.simulation_time_seconds() < 1.0);
        assert_eq!(per_update, [1; 10]);
    }

    #[test]
    fn split_and_whole_updates_emit_bit_identical_samples() {
        let config = FixedRateScheduleConfig {
            interval_seconds: 0.07,
        };
        let mut whole = FixedRateSchedule::new(config, 0.011).unwrap();
        let mut split = FixedRateSchedule::new(config, 0.011).unwrap();

        let whole_events = collect(&mut whole, &[12.5]);
        // Uneven steps, some shorter and some longer than the interval.
        let steps: Vec<f64> = [0.013, 0.2, 0.0, 0.041, 0.5, 0.0005]
            .iter()
            .copied()
            .cycle()
            .take(1000)
            .collect();
        let total: f64 = steps.iter().sum();
        let split_events = collect(&mut split, &steps);

        // The totals differ by rounding, so compare the samples both covered.
        let covered = total.min(12.5) - config.interval_seconds;
        let whole_times: Vec<u64> = whole_events
            .iter()
            .filter(|event| event.scheduled_simulation_time_seconds < covered)
            .map(|event| event.scheduled_simulation_time_seconds.to_bits())
            .collect();
        let split_times: Vec<u64> = split_events
            .iter()
            .filter(|event| event.scheduled_simulation_time_seconds < covered)
            .map(|event| event.scheduled_simulation_time_seconds.to_bits())
            .collect();
        assert!(whole_times.len() > 100);
        assert_eq!(whole_times, split_times);
    }

    #[test]
    fn two_schedules_fed_the_same_inputs_agree_bit_for_bit() {
        let config = FixedRateScheduleConfig {
            interval_seconds: 1.0 / 30.0,
        };
        let steps = [0.016, 0.017, 0.2, 0.001, 0.033];
        let mut a = FixedRateSchedule::new(config, 0.005).unwrap();
        let mut b = FixedRateSchedule::new(config, 0.005).unwrap();
        let a_events = collect(&mut a, &steps);
        let b_events = collect(&mut b, &steps);

        assert!(!a_events.is_empty());
        let bits = |event: &FixedRateSample| {
            [
                event.scheduled_simulation_time_seconds.to_bits(),
                event.offset_within_update_seconds.to_bits(),
                event.elapsed_since_previous_sample_seconds.to_bits(),
            ]
        };
        assert_eq!(
            a_events.iter().map(bits).collect::<Vec<_>>(),
            b_events.iter().map(bits).collect::<Vec<_>>()
        );
        assert_eq!(a, b);
    }

    #[test]
    fn samples_are_ordered_and_offsets_lie_within_their_update() {
        let mut schedule = FixedRateSchedule::new(
            FixedRateScheduleConfig {
                interval_seconds: 0.02,
            },
            0.007,
        )
        .unwrap();
        let mut previous = f64::NEG_INFINITY;
        for step in [0.1, 0.013, 0.5, 0.019] {
            schedule
                .advance(step, |event| {
                    assert!(event.scheduled_simulation_time_seconds > previous);
                    previous = event.scheduled_simulation_time_seconds;
                    assert!(event.offset_within_update_seconds > 0.0);
                    assert!(event.offset_within_update_seconds <= step + EPSILON);
                })
                .unwrap();
        }
    }

    #[test]
    fn skipping_emits_nothing_and_resumes_where_advancing_would() {
        let config = FixedRateScheduleConfig {
            interval_seconds: 0.1,
        };
        for phase in [0.0, 0.013, 0.05, 0.099] {
            for skip in [0.05, 0.3, 1.0, 7.77, 1234.5678] {
                let mut skipped = FixedRateSchedule::new(config, phase).unwrap();
                let mut advanced = skipped;
                skipped.advance(0.02, |_| {}).unwrap();
                advanced.advance(0.02, |_| {}).unwrap();

                skipped.skip_elapsed(skip).unwrap();
                advanced.advance(skip, |_| {}).unwrap();

                let after = 0.25;
                let skipped_events = collect(&mut skipped, &[after]);
                let advanced_events = collect(&mut advanced, &[after]);
                assert_eq!(
                    event_times(&skipped_events)
                        .iter()
                        .map(|time| time.to_bits())
                        .collect::<Vec<_>>(),
                    event_times(&advanced_events)
                        .iter()
                        .map(|time| time.to_bits())
                        .collect::<Vec<_>>(),
                    "phase {phase}, skip {skip}"
                );
                // The first sample after a skip measures from the skip's end,
                // not from a sample that was never taken.
                let first = skipped_events[0];
                assert!(
                    (first.elapsed_since_previous_sample_seconds
                        - first.offset_within_update_seconds)
                        .abs()
                        < EPSILON
                );
            }
        }
    }

    #[test]
    fn skipping_emits_nothing_on_the_next_update_for_the_skipped_time() {
        let mut schedule = FixedRateSchedule::default();
        schedule.skip_elapsed(5.0).unwrap();
        assert!(collect(&mut schedule, &[0.0]).is_empty());
        assert!(collect(&mut schedule, &[0.09]).is_empty());
    }

    #[test]
    fn a_rejected_skip_past_the_index_bound_leaves_the_schedule_alone() {
        let mut schedule = FixedRateSchedule::new(
            FixedRateScheduleConfig {
                interval_seconds: 1e-3,
            },
            0.0,
        )
        .unwrap();
        schedule.advance(0.0105, |_| {}).unwrap();
        let before = schedule;
        // Enough time for far more samples than the index can hold exactly.
        assert_eq!(
            schedule.skip_elapsed(1e14),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(schedule, before);
        assert_eq!(
            schedule.skip_elapsed(-1.0),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(schedule, before);
        assert_eq!(
            schedule.advance(f64::INFINITY, |_| panic!("emitted on a rejected update")),
            Err(FixedRateScheduleAdvanceError::InvalidElapsed)
        );
        assert_eq!(schedule, before);
    }

    #[test]
    fn phase_stagger_spreads_agents_across_the_interval() {
        let config = FixedRateScheduleConfig::default();
        let agents = 8_u32;
        let mut schedules: Vec<FixedRateSchedule> = (0..agents)
            .map(|i| {
                let phase = f64::from(i) * config.interval_seconds / f64::from(agents);
                FixedRateSchedule::new(config, phase).unwrap()
            })
            .collect();

        // Warm past the zero-phase agent's first interval, then look at one
        // full interval: every agent samples once, each at a different time.
        for schedule in &mut schedules {
            schedule.advance(config.interval_seconds, |_| {}).unwrap();
        }
        let mut times = Vec::new();
        for schedule in &mut schedules {
            let events = collect(schedule, &[config.interval_seconds]);
            assert_eq!(events.len(), 1, "phase {}", schedule.phase_offset_seconds());
            times.push(events[0].scheduled_simulation_time_seconds);
        }
        times.sort_by(f64::total_cmp);
        let spacing = config.interval_seconds / f64::from(agents);
        for pair in times.windows(2) {
            assert!((pair[1] - pair[0] - spacing).abs() < EPSILON);
        }
    }

    #[test]
    fn errors_display_what_was_wrong() {
        assert!(
            FixedRateScheduleConfigError::InvalidInterval
                .to_string()
                .contains("interval")
        );
        assert!(
            FixedRateScheduleConfigError::InvalidPhaseOffset
                .to_string()
                .contains("phase")
        );
        assert!(
            FixedRateScheduleAdvanceError::InvalidElapsed
                .to_string()
                .contains("elapsed")
        );
    }
}
