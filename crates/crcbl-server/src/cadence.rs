//! How often one session is sent a snapshot: the rate drop.
//!
//! A session is sent a snapshot every tick while its snapshots fit their
//! budget. When they go on not fitting — updates held back
//! ([`crcbl_net::Fitted::shed`]) or removals deferred, snapshot after
//! snapshot — the session steps down to a snapshot every second tick, then
//! every third, so a link that cannot carry the world's change at the tick
//! rate is sent less often instead of being sent an ever-rotating fraction
//! of it as fast as the server ticks. A sustained run of snapshots that fit
//! with room to spare steps it back up.
//!
//! The steps are fractions of the tick rate, not rates in hertz, because the
//! server ticks at whatever rate the game gives it. The plan named 30, then
//! 20 snapshots a second for a 60 Hz server; [`SNAPSHOT_INTERVAL_STEPS`] is
//! that, restated as every second and every third tick.
//!
//! **Hysteresis.** Stepping down takes [`STEP_DOWN_AFTER`] over-budget
//! snapshots in a row and stepping up [`STEP_UP_AFTER`] snapshots in a row
//! that fit within [`HEADROOM`] of their budget, so a link hovering at its
//! limit — one snapshot shedding, the next fitting — stays where it is
//! rather than flapping between rates; a snapshot that fits but without
//! headroom moves neither count on. Stepping down is quicker than stepping
//! up because a link over budget is losing state every snapshot, while one
//! with headroom loses only freshness by waiting.
//!
//! Each session has its own cadence, so one congested peer is slowed and the
//! others are not.

/// Ticks between snapshots at each step of the rate drop, fastest first: a
/// snapshot every tick, then every second tick, then every third.
pub const SNAPSHOT_INTERVAL_STEPS: [u32; 3] = [1, 2, 3];

/// Consecutive snapshots over budget — something held back or deferred —
/// that step a session down to the next longer interval.
pub const STEP_DOWN_AFTER: u32 = 16;

/// Consecutive snapshots fitting within [`HEADROOM`] that step a session
/// back up to the next shorter interval.
pub const STEP_UP_AFTER: u32 = 64;

/// The share of its budget, as `(numerator, denominator)`, that a snapshot
/// may take and still count towards [`STEP_UP_AFTER`]. Stepping up shortens
/// the interval, so each snapshot then carries less of a tick's change;
/// demanding room to spare is what keeps the faster rate from going straight
/// back over budget.
pub const HEADROOM: (usize, usize) = (3, 4);

/// One session's snapshot cadence. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub(crate) struct SnapshotCadence {
    /// Index into [`SNAPSHOT_INTERVAL_STEPS`].
    step: usize,
    /// Ticks since this session was last due a snapshot.
    ticks_since_snapshot: u32,
    /// Consecutive snapshots over budget.
    over_budget_run: u32,
    /// Consecutive snapshots fitting within [`HEADROOM`].
    headroom_run: u32,
}

impl SnapshotCadence {
    /// Ticks between this session's snapshots now.
    pub(crate) fn interval(&self) -> u32 {
        SNAPSHOT_INTERVAL_STEPS[self.step]
    }

    /// Count one tick, returning whether the session is due a snapshot on
    /// it. Called once a tick; the first call is always due.
    pub(crate) fn due(&mut self) -> bool {
        self.ticks_since_snapshot = self.ticks_since_snapshot.saturating_add(1);
        if self.ticks_since_snapshot < self.interval() {
            return false;
        }
        self.ticks_since_snapshot = 0;
        true
    }

    /// Record how one sent snapshot fitted: whether anything was held back
    /// or deferred, and its encoded length against its budget.
    pub(crate) fn observe(&mut self, over_budget: bool, encoded_bytes: usize, budget: usize) {
        if over_budget {
            self.headroom_run = 0;
            self.over_budget_run = self.over_budget_run.saturating_add(1);
            if self.over_budget_run >= STEP_DOWN_AFTER
                && self.step + 1 < SNAPSHOT_INTERVAL_STEPS.len()
            {
                self.step += 1;
                self.over_budget_run = 0;
            }
            return;
        }
        self.over_budget_run = 0;
        let (numerator, denominator) = HEADROOM;
        if encoded_bytes.saturating_mul(denominator) > budget.saturating_mul(numerator) {
            self.headroom_run = 0;
            return;
        }
        self.headroom_run = self.headroom_run.saturating_add(1);
        if self.headroom_run >= STEP_UP_AFTER && self.step > 0 {
            self.step -= 1;
            self.headroom_run = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: usize = 1000;

    /// A snapshot that held something back.
    fn shed(cadence: &mut SnapshotCadence) {
        cadence.observe(true, BUDGET, BUDGET);
    }

    /// A snapshot that fit with room to spare.
    fn roomy(cadence: &mut SnapshotCadence) {
        cadence.observe(false, BUDGET / 2, BUDGET);
    }

    /// A snapshot that fit, but with less than [`HEADROOM`] to spare.
    fn tight(cadence: &mut SnapshotCadence) {
        cadence.observe(false, BUDGET, BUDGET);
    }

    #[test]
    fn a_session_starts_at_a_snapshot_every_tick() {
        let mut cadence = SnapshotCadence::default();
        assert_eq!(cadence.interval(), SNAPSHOT_INTERVAL_STEPS[0]);
        assert!((0..10).all(|_| cadence.due()));
    }

    /// **Sustained over-budget steps the interval down through every step
    /// and never past the last**, [`STEP_DOWN_AFTER`] snapshots a step.
    #[test]
    fn sustained_over_budget_steps_down_through_each_step_and_not_past_the_last() {
        let mut cadence = SnapshotCadence::default();
        for &interval in &SNAPSHOT_INTERVAL_STEPS[1..] {
            for _ in 1..STEP_DOWN_AFTER {
                shed(&mut cadence);
            }
            assert_ne!(cadence.interval(), interval, "stepped down early");
            shed(&mut cadence);
            assert_eq!(cadence.interval(), interval);
        }
        for _ in 0..10 * STEP_DOWN_AFTER {
            shed(&mut cadence);
        }
        assert_eq!(
            cadence.interval(),
            *SNAPSHOT_INTERVAL_STEPS.last().expect("steps")
        );
    }

    /// **A session at each interval is due exactly every that many ticks.**
    #[test]
    fn a_session_is_due_once_every_interval() {
        let mut cadence = SnapshotCadence::default();
        for (step, &interval) in SNAPSHOT_INTERVAL_STEPS.iter().enumerate() {
            if step > 0 {
                for _ in 0..STEP_DOWN_AFTER {
                    shed(&mut cadence);
                }
            }
            assert_eq!(cadence.interval(), interval);
            // Line up on a due tick, then count.
            while !cadence.due() {}
            let due: Vec<bool> = (0..3 * interval).map(|_| cadence.due()).collect();
            let expected: Vec<bool> = (1..=3 * interval).map(|t| t % interval == 0).collect();
            assert_eq!(due, expected, "at interval {interval}");
        }
    }

    /// **Hysteresis: a link alternating between fitting and shedding does not
    /// flap.** At full rate it never steps down, and once down it never steps
    /// back up, however long the pattern runs.
    #[test]
    fn an_alternating_fit_and_shed_pattern_never_changes_the_interval() {
        let mut cadence = SnapshotCadence::default();
        for _ in 0..20 * STEP_UP_AFTER {
            shed(&mut cadence);
            roomy(&mut cadence);
        }
        assert_eq!(cadence.interval(), SNAPSHOT_INTERVAL_STEPS[0]);

        for _ in 0..STEP_DOWN_AFTER {
            shed(&mut cadence);
        }
        let down = cadence.interval();
        assert_eq!(down, SNAPSHOT_INTERVAL_STEPS[1]);
        for _ in 0..20 * STEP_UP_AFTER {
            roomy(&mut cadence);
            shed(&mut cadence);
        }
        assert_eq!(cadence.interval(), down);
    }

    /// **Sustained headroom steps back up**, one step per [`STEP_UP_AFTER`]
    /// roomy snapshots, to a snapshot every tick and no further; snapshots
    /// that fit without headroom do not count.
    #[test]
    fn sustained_headroom_steps_back_up_and_a_tight_fit_does_not() {
        let mut cadence = SnapshotCadence::default();
        for _ in 0..(SNAPSHOT_INTERVAL_STEPS.len() as u32) * STEP_DOWN_AFTER {
            shed(&mut cadence);
        }
        let slowest = SNAPSHOT_INTERVAL_STEPS.len() - 1;
        assert_eq!(cadence.interval(), SNAPSHOT_INTERVAL_STEPS[slowest]);

        for _ in 0..10 * STEP_UP_AFTER {
            tight(&mut cadence);
        }
        assert_eq!(
            cadence.interval(),
            SNAPSHOT_INTERVAL_STEPS[slowest],
            "a fit without headroom stepped up"
        );

        for step in (0..slowest).rev() {
            for _ in 1..STEP_UP_AFTER {
                roomy(&mut cadence);
            }
            assert_ne!(
                cadence.interval(),
                SNAPSHOT_INTERVAL_STEPS[step],
                "stepped up early"
            );
            roomy(&mut cadence);
            assert_eq!(cadence.interval(), SNAPSHOT_INTERVAL_STEPS[step]);
        }
        for _ in 0..10 * STEP_UP_AFTER {
            roomy(&mut cadence);
        }
        assert_eq!(cadence.interval(), SNAPSHOT_INTERVAL_STEPS[0]);
    }
}
