//! The frame's budget row: CPU frame time against GPU frame time, and which of
//! the two the frame is actually costing.
//!
//! `docs/plan/40-profiling.md`'s first debug-panel row, and the reason it is
//! first: "'GPU-bound' is the first question and nothing answers it today". The
//! two numbers come from opposite ends of the engine — the CPU one from the
//! frame span `crcbl_core::trace` records, the GPU one from
//! `crcbl_render::timing`'s per-pass timestamps — and neither of those knows
//! about the other, so this is where they meet. It knows about neither in turn:
//! it takes nanoseconds.
//!
//! # The two numbers are about different frames, and the row does not pretend
//! otherwise
//!
//! The GPU report is **frames latent by design** — a ring of query sets resolved
//! only when a slot comes back round, so the newest GPU number is about a frame a
//! few frames ago, and `docs/plan/40-profiling.md` is explicit that no consumer
//! "fixes" that by stalling. So a row pairing *this* frame's CPU cost with *this*
//! frame's GPU cost would be stating something it cannot know.
//!
//! What it shows instead is two **distributions**: p50 and p95 over a rolling
//! window of [`DEFAULT_FRAME_WINDOW`] frames each. A window that long is not
//! moved by the two or three frames the ring lags — the samples in it are the
//! same frames give or take the offset — while a single-frame pairing would be
//! wrong by exactly that offset. The lag is shown rather than hidden: the row
//! carries the frame number the newest GPU sample came from, so a reader can
//! watch it trail the frame counter.
//!
//! # It reports nothing until it can report a percentile
//!
//! A p95 over five samples is the largest of the five, which is a maximum wearing
//! a percentile's name. [`MIN_PERCENTILE_SAMPLES`] is the point below which that
//! is true, so below it the row says how full the window is and nothing else.

use core::fmt;
use core::time::Duration;
use std::collections::VecDeque;

use crate::debug::{DEFAULT_FRAME_WINDOW, DebugModule, DebugSection};

/// Samples a window needs before [`BudgetStats`] reports a percentile.
///
/// Nearest-rank p95 picks element `ceil(0.95 * n)` of `n` sorted samples, which
/// is `n` itself — the maximum — for every `n` below this. Twenty is the first
/// count at which the rank is `19` and the answer is therefore a percentile
/// rather than the worst frame in the window under another name.
pub const MIN_PERCENTILE_SAMPLES: usize = 20;

/// Which of the CPU and the GPU is the frame's budget.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    /// Neither window holds [`MIN_PERCENTILE_SAMPLES`] yet.
    Unknown,
    /// The CPU p50 is the larger.
    Cpu,
    /// The GPU p50 is the larger.
    Gpu,
    /// The two p50s are equal.
    Tied,
}

impl Bound {
    /// How the row spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Tied => "tied",
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The percentile of a **sorted** slice, by nearest rank.
///
/// The rank is `ceil(pct * n / 100)`, clamped to at least one, and the answer is
/// the sample at that rank — so it is always a sample that actually happened
/// rather than an interpolation between two that did. `pct` is a whole percent
/// in `1..=100`; `percentile_of(xs, 100)` is the maximum.
///
/// Computed in integer arithmetic: `(pct * n)` for a window this size is nowhere
/// near overflowing, and a float rank rounds `0.95 * 20` to something that is
/// either side of 19 depending on the value, which is exactly the off-by-one this
/// avoids.
fn percentile_of(sorted: &[u64], pct: usize) -> Option<u64> {
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

/// One rolling window of per-frame nanosecond costs.
#[derive(Clone, Debug, Default)]
struct Window {
    /// Newest last, at most [`DEFAULT_FRAME_WINDOW`] of them.
    samples: VecDeque<u64>,
}

impl Window {
    fn record(&mut self, nanos: u64) {
        if self.samples.len() == DEFAULT_FRAME_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(nanos);
    }

    fn len(&self) -> usize {
        self.samples.len()
    }

    /// The window's p50 and p95, or `None` below [`MIN_PERCENTILE_SAMPLES`].
    ///
    /// Sorted into a fixed buffer rather than a fresh `Vec`: this runs once a
    /// frame while the panel is up, and the window's own bound is what says the
    /// buffer is big enough.
    fn percentiles(&self) -> Option<(u64, u64)> {
        if self.samples.len() < MIN_PERCENTILE_SAMPLES {
            return None;
        }
        let mut sorted = [0u64; DEFAULT_FRAME_WINDOW];
        let sorted = &mut sorted[..self.samples.len()];
        for (slot, sample) in sorted.iter_mut().zip(&self.samples) {
            *slot = *sample;
        }
        sorted.sort_unstable();
        Some((percentile_of(sorted, 50)?, percentile_of(sorted, 95)?))
    }
}

/// CPU against GPU frame time, over a rolling window of each.
///
/// Fed once a frame from both ends — [`BudgetStats::record_cpu`] with the frame
/// span's duration, [`BudgetStats::record_gpu`] with the pass total the timers
/// resolved — and read by the debug panel. See the [module docs](self) for why
/// the two are shown as distributions rather than paired frame by frame.
#[derive(Clone, Debug, Default)]
pub struct BudgetStats {
    cpu: Window,
    gpu: Window,
    /// Which frame the newest GPU sample was about. Also the guard against
    /// counting one latent report several times: the ring hands the same
    /// `FrameTimings` back until a new slot resolves.
    gpu_frame: Option<u64>,
}

impl BudgetStats {
    /// Empty windows.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records what this frame cost on the CPU.
    pub fn record_cpu(&mut self, cpu: Duration) {
        self.cpu.record(nanos_of(cpu));
    }

    /// Records what frame `frame` cost on the GPU, ignoring a report already
    /// recorded.
    ///
    /// Reports whether it was a new frame. The caller reads a *latent* report
    /// that answers with the same frame until the ring comes round, so counting
    /// every read would fill the window with duplicates of whatever the GPU
    /// happened to be slow at.
    pub fn record_gpu(&mut self, frame: u64, gpu: Duration) -> bool {
        if self.gpu_frame == Some(frame) {
            return false;
        }
        self.gpu_frame = Some(frame);
        self.gpu.record(nanos_of(gpu));
        true
    }

    /// Whether either window holds anything.
    ///
    /// What the overlay asks before adding the section: a run that never turned
    /// the trace on and has no GPU timers has nothing to say here, and three rows
    /// of `none` in every sample is clutter rather than information.
    #[must_use]
    pub fn has_samples(&self) -> bool {
        self.cpu.len() > 0 || self.gpu.len() > 0
    }

    /// The CPU window's p50 and p95, or `None` below
    /// [`MIN_PERCENTILE_SAMPLES`].
    #[must_use]
    pub fn cpu(&self) -> Option<(Duration, Duration)> {
        self.cpu.percentiles().map(durations_of)
    }

    /// The GPU window's p50 and p95, or `None` below
    /// [`MIN_PERCENTILE_SAMPLES`].
    #[must_use]
    pub fn gpu(&self) -> Option<(Duration, Duration)> {
        self.gpu.percentiles().map(durations_of)
    }

    /// The frame the newest GPU sample came from, which trails the frame being
    /// drawn by the timers' ring latency.
    #[must_use]
    pub const fn gpu_frame(&self) -> Option<u64> {
        self.gpu_frame
    }

    /// Which of the two the frame is costing.
    ///
    /// **The rule, in full**: the budget is whichever side has the larger p50,
    /// and it is [`Bound::Unknown`] until *both* windows hold
    /// [`MIN_PERCENTILE_SAMPLES`]. p50 rather than p95 because the question is
    /// what a frame normally costs — a p95 comparison answers "which side stalls
    /// worse", which is a different and rarer question, and the p95s are on the
    /// row for whoever is asking it.
    #[must_use]
    pub fn bound(&self) -> Bound {
        let (Some((cpu, _)), Some((gpu, _))) = (self.cpu.percentiles(), self.gpu.percentiles())
        else {
            return Bound::Unknown;
        };
        match gpu.cmp(&cpu) {
            core::cmp::Ordering::Greater => Bound::Gpu,
            core::cmp::Ordering::Less => Bound::Cpu,
            core::cmp::Ordering::Equal => Bound::Tied,
        }
    }
}

/// A window's contribution to the row: its two percentiles, or why it has none.
fn write_window(out: &mut DebugSection, label: &str, window: &Window) {
    match window.percentiles() {
        Some((p50, p95)) => out.row(
            label,
            format_args!("{:.2} / {:.2} ms", millis(p50), millis(p95)),
        ),
        None if window.len() == 0 => out.row_str(label, "none"),
        // Deliberately not a number: a p95 over this many samples is the
        // maximum, and printing it as a percentile is the lie this avoids.
        None => out.row(
            label,
            format_args!("filling {}/{}", window.len(), MIN_PERCENTILE_SAMPLES),
        ),
    }
}

fn millis(nanos: u64) -> f64 {
    nanos as f64 / 1.0e6
}

fn nanos_of(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn durations_of((p50, p95): (u64, u64)) -> (Duration, Duration) {
    (Duration::from_nanos(p50), Duration::from_nanos(p95))
}

/// The row itself.
///
/// Labels are deliberately long enough to be unique across the whole panel:
/// `DebugModule` labels share one namespace and nothing detects a collision, so
/// a bare `cpu` beside the GPU section's per-pass labels would be a row a reader
/// — or a test searching the draw list — could pick up as the wrong one.
impl DebugModule for BudgetStats {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("budget");
        write_window(out, "cpu p50/p95", &self.cpu);
        write_window(out, "gpu p50/p95", &self.gpu);
        out.row_str("bound", self.bound().as_str());
        match self.gpu_frame {
            Some(frame) => out.row("gpu frame", format_args!("{frame}")),
            None => out.row_str("gpu frame", "pending"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// Fills a window with `n` samples of `millis` each.
    fn fill(stats: &mut BudgetStats, n: usize, cpu: u64, gpu: u64) {
        for frame in 0..n {
            stats.record_cpu(ms(cpu));
            stats.record_gpu(frame as u64, ms(gpu));
        }
    }

    /// **The arithmetic, against a sequence whose answers are hand-computed.**
    ///
    /// Twenty samples of 1..=20 ns. Nearest rank puts p50 at `ceil(0.50 * 20) =
    /// 10` — the tenth smallest, which is 10 — and p95 at `ceil(0.95 * 20) = 19`,
    /// which is 19 and is *not* the maximum. An off-by-one in either direction
    /// lands on 9, 11, 18 or 20, all of which this catches.
    #[test]
    fn the_percentile_is_the_sample_at_the_nearest_rank() {
        let sorted: Vec<u64> = (1..=20).collect();
        assert_eq!(percentile_of(&sorted, 50), Some(10));
        assert_eq!(percentile_of(&sorted, 95), Some(19));
        assert_eq!(percentile_of(&sorted, 100), Some(20));
        // The first rank, and the clamp that keeps a small percentile of a small
        // window from indexing at minus one.
        assert_eq!(percentile_of(&sorted, 1), Some(1));
        assert_eq!(percentile_of(&[7], 50), Some(7));
        assert_eq!(percentile_of(&[7], 1), Some(7));
        assert_eq!(percentile_of(&[], 50), None);

        // Ten samples: p95's rank is `ceil(9.5) = 10`, the maximum — which is
        // exactly why MIN_PERCENTILE_SAMPLES is what it is.
        let ten: Vec<u64> = (1..=10).collect();
        assert_eq!(percentile_of(&ten, 95), Some(10));
        assert_eq!(percentile_of(&ten, 95), percentile_of(&ten, 100));
        // And at twenty they part company.
        assert_ne!(percentile_of(&sorted, 95), percentile_of(&sorted, 100));
    }

    /// The order samples arrive in must not change the answer.
    #[test]
    fn the_window_sorts_before_it_picks() {
        let mut ascending = Window::default();
        let mut descending = Window::default();
        for value in 1..=MIN_PERCENTILE_SAMPLES as u64 {
            ascending.record(value);
        }
        for value in (1..=MIN_PERCENTILE_SAMPLES as u64).rev() {
            descending.record(value);
        }
        assert_eq!(ascending.percentiles(), Some((10, 19)));
        assert_eq!(descending.percentiles(), ascending.percentiles());
    }

    /// **Below the minimum there is no percentile**, and the row says which of
    /// the two reasons it has none.
    #[test]
    fn a_short_window_reports_how_full_it_is_rather_than_a_percentile() {
        let mut stats = BudgetStats::new();
        assert!(!stats.has_samples());
        assert_eq!(stats.cpu(), None);
        assert_eq!(stats.bound(), Bound::Unknown);

        assert_eq!(rendered(&stats, "cpu p50/p95"), "none");
        assert_eq!(rendered(&stats, "gpu frame"), "pending");

        fill(&mut stats, MIN_PERCENTILE_SAMPLES - 1, 4, 8);
        assert!(stats.has_samples());
        assert_eq!(stats.cpu(), None, "one sample short is still short");
        assert_eq!(stats.bound(), Bound::Unknown);
        assert_eq!(rendered(&stats, "cpu p50/p95"), "filling 19/20");
        assert_eq!(rendered(&stats, "bound"), "unknown");

        // The sample that completes the window is the one that produces a
        // number: the boundary, not somewhere near it.
        fill(&mut stats, 1, 4, 8);
        assert_eq!(stats.cpu(), Some((ms(4), ms(4))));
        assert_eq!(stats.bound(), Bound::Gpu);
    }

    /// **The budget rule, both ways round**, so an inverted comparison cannot
    /// pass by agreeing with itself.
    #[test]
    fn the_budget_is_whichever_side_has_the_larger_p50() {
        let mut gpu_bound = BudgetStats::new();
        fill(&mut gpu_bound, MIN_PERCENTILE_SAMPLES, 4, 9);
        assert_eq!(gpu_bound.cpu(), Some((ms(4), ms(4))));
        assert_eq!(gpu_bound.gpu(), Some((ms(9), ms(9))));
        assert_eq!(gpu_bound.bound(), Bound::Gpu);
        assert_eq!(gpu_bound.bound().to_string(), "gpu");

        let mut cpu_bound = BudgetStats::new();
        fill(&mut cpu_bound, MIN_PERCENTILE_SAMPLES, 9, 4);
        assert_eq!(cpu_bound.bound(), Bound::Cpu);

        let mut tied = BudgetStats::new();
        fill(&mut tied, MIN_PERCENTILE_SAMPLES, 5, 5);
        assert_eq!(tied.bound(), Bound::Tied);

        // One side alone is not an answer, however many samples it has.
        let mut half = BudgetStats::new();
        for _ in 0..MIN_PERCENTILE_SAMPLES {
            half.record_cpu(ms(4));
        }
        assert!(half.cpu().is_some());
        assert_eq!(half.gpu(), None);
        assert_eq!(half.bound(), Bound::Unknown);
    }

    /// **The latent GPU report is counted once**, not once per frame it is read.
    ///
    /// The ring hands back the same `FrameTimings` until a slot comes round, so a
    /// window that took every read would be a window of duplicates — and a
    /// stalled ring would look like a steady frame rate.
    #[test]
    fn re_reading_the_same_gpu_frame_does_not_fill_the_window() {
        let mut stats = BudgetStats::new();
        assert!(stats.record_gpu(7, ms(3)), "a frame not seen before");
        assert!(!stats.record_gpu(7, ms(3)), "the same frame again");
        assert!(!stats.record_gpu(7, ms(99)), "even at a different cost");
        assert_eq!(stats.gpu.len(), 1);
        assert_eq!(stats.gpu_frame(), Some(7));

        assert!(stats.record_gpu(8, ms(3)));
        assert_eq!(stats.gpu.len(), 2);
        assert_eq!(stats.gpu_frame(), Some(8));
    }

    /// The window evicts at its bound, so a stall that has scrolled off stops
    /// counting — and the bound is the frame row's, so the two rows cover the
    /// same frames.
    #[test]
    fn the_window_holds_the_frame_rows_worth_of_frames_and_then_evicts() {
        let mut stats = BudgetStats::new();
        // One enormous frame, then a full window of small ones on top of it.
        stats.record_cpu(ms(500));
        for _ in 0..DEFAULT_FRAME_WINDOW {
            stats.record_cpu(ms(4));
        }
        assert_eq!(stats.cpu.len(), DEFAULT_FRAME_WINDOW);
        assert_eq!(
            stats.cpu(),
            Some((ms(4), ms(4))),
            "the 500 ms frame has scrolled out of the window"
        );
    }

    /// **The row's text changes when the numbers do.** A row that rendered a
    /// constant would pass a test that only checked the row existed.
    #[test]
    fn the_row_renders_the_numbers_it_was_given() {
        let mut stats = BudgetStats::new();
        fill(&mut stats, MIN_PERCENTILE_SAMPLES, 4, 9);
        let mut section = DebugSection::default();
        stats.debug_section(&mut section);
        assert_eq!(section.title(), "budget");
        assert_eq!(rendered(&stats, "cpu p50/p95"), "4.00 / 4.00 ms");
        assert_eq!(rendered(&stats, "gpu p50/p95"), "9.00 / 9.00 ms");
        assert_eq!(rendered(&stats, "bound"), "gpu");
        assert_eq!(
            rendered(&stats, "gpu frame"),
            (MIN_PERCENTILE_SAMPLES - 1).to_string(),
        );

        // Different numbers in, different strings out — including the bound.
        let mut other = BudgetStats::new();
        for frame in 0..MIN_PERCENTILE_SAMPLES {
            other.record_cpu(ms(20));
            other.record_gpu(frame as u64, ms(1));
        }
        assert_eq!(rendered(&other, "cpu p50/p95"), "20.00 / 20.00 ms");
        assert_eq!(rendered(&other, "gpu p50/p95"), "1.00 / 1.00 ms");
        assert_eq!(rendered(&other, "bound"), "cpu");
    }

    /// A spread window, so the two percentiles are actually different numbers
    /// and the row is not just printing one value twice.
    #[test]
    fn the_two_percentiles_are_read_off_the_same_window_separately() {
        let mut stats = BudgetStats::new();
        // 1..=20 milliseconds: p50 is the 10th, p95 the 19th.
        for value in 1..=MIN_PERCENTILE_SAMPLES as u64 {
            stats.record_cpu(ms(value));
        }
        assert_eq!(stats.cpu(), Some((ms(10), ms(19))));
        assert_eq!(rendered(&stats, "cpu p50/p95"), "10.00 / 19.00 ms");
    }

    /// Every label this section writes, so a rename is visible and a collision
    /// with another module's row is something a reader can check against this
    /// list.
    #[test]
    fn the_sections_labels_are_its_own() {
        let mut section = DebugSection::default();
        BudgetStats::new().debug_section(&mut section);
        let labels: Vec<&str> = section
            .rows()
            .iter()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(labels, ["cpu p50/p95", "gpu p50/p95", "bound", "gpu frame"],);
    }

    /// One row's value, by label, off a section written from scratch — the way
    /// the panel writes one, which clears before every `debug_section`.
    fn rendered(stats: &BudgetStats, label: &str) -> String {
        let mut section = DebugSection::default();
        stats.debug_section(&mut section);
        section
            .rows()
            .iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("no {label:?} row in {:?}", section.rows()))
            .value
            .clone()
    }
}
