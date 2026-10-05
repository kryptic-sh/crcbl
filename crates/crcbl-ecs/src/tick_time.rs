//! How long each system's [`SystemTrait::tick`](crate::SystemTrait::tick) took,
//! as the [`Schedule`](crate::Schedule)'s clock measured it — see [`TickTime`].

use core::time::Duration;
use std::collections::VecDeque;

/// How many ticks a system's [`TickTime::mean`] averages over.
///
/// One second of ticks at [`World::DEFAULT_TICK_DT`](crate::World::DEFAULT_TICK_DT)
/// — the default is asserted below. Long enough that a system costing a few
/// microseconds reads as a steady number rather than timer noise; short enough
/// that a change in its cost shows while you are still looking at the panel.
pub const TICK_TIME_WINDOW: usize = 60;

// The window's claim to be a second at the default rate, checked where it is
// made rather than restated.
const _: () = assert!(TICK_TIME_WINDOW as f64 * crate::World::DEFAULT_TICK_DT == 1.0);

/// One system's measured tick cost: the last tick, and the mean over the last
/// [`TICK_TIME_WINDOW`] ticks.
///
/// # Wall time, kept out of the simulation
///
/// A tick time is the one number in this crate that comes from a real clock,
/// which `docs/notes/process.md` forbids anywhere a simulation could read it.
/// It is sound here because nothing that decides state can: the times live
/// beside the systems in the schedule, are read back only through
/// [`Inspector::collect`](crate::Inspector::collect), and never reach
/// [`Schedule::hash_state`](crate::Schedule::hash_state), a snapshot or a save.
/// The clock is injected ([`Schedule::set_clock`](crate::Schedule::set_clock))
/// rather than read here, so a test drives it by hand and a world nobody gave a
/// clock reads none at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickTime {
    /// How long the system's most recent tick took.
    pub last: Duration,
    /// The mean over the ticks in the window — every tick measured so far,
    /// until there have been [`TICK_TIME_WINDOW`] of them.
    pub mean: Duration,
}

/// A rolling window of one system's tick durations.
///
/// The mean is `total / len`, kept incrementally so recording is O(1);
/// `Duration` addition is exact, so the total cannot drift from the samples.
#[derive(Debug, Default)]
pub(crate) struct TickWindow {
    samples: VecDeque<Duration>,
    total: Duration,
}

impl TickWindow {
    /// Records one tick, evicting the oldest once the window is full.
    pub(crate) fn record(&mut self, took: Duration) {
        if self.samples.len() == TICK_TIME_WINDOW
            && let Some(oldest) = self.samples.pop_front()
        {
            self.total -= oldest;
        }
        self.samples.push_back(took);
        self.total += took;
    }

    /// The last tick and the window's mean, or `None` before the first tick.
    pub(crate) fn read(&self) -> Option<TickTime> {
        let last = *self.samples.back()?;
        // The window is bounded by `TICK_TIME_WINDOW`, so its length fits.
        let len = u32::try_from(self.samples.len()).unwrap_or(u32::MAX);
        Some(TickTime {
            last,
            mean: self.total / len,
        })
    }

    /// Forgets every sample: the clock they were measured on is gone.
    pub(crate) fn clear(&mut self) {
        self.samples.clear();
        self.total = Duration::ZERO;
    }
}
