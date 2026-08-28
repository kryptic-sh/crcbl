//! Nearest-rank percentiles, and the sample count below which one is a lie.
//!
//! `docs/plan/40-profiling.md` decides that a benchmark and a debug row alike
//! report p50, p95, p99 and max and never a mean, because "frame time is a tail
//! problem — a mean hides exactly the stutter a player notices". This module is
//! the arithmetic behind that decision, in one place: `crcbl_ui::budget`'s
//! rolling window of frame costs and `crcbl bench`'s per-iteration samples ask
//! the same question of different numbers, and a second copy of the rank
//! calculation is a second chance for the off-by-one it exists to avoid.
//!
//! It lives here for the reason [`trace`](mod@crate::trace) does — the facility
//! every other crate reaches for and none of them owns.
//!
//! **There is deliberately no mean here.** Adding one would be adding the number
//! the plan decided not to report, and a helper that offers it is an invitation
//! to print it.

/// Samples a window needs before a nearest-rank p95 is a percentile rather than
/// the maximum under another name.
///
/// Nearest-rank p95 picks element `ceil(0.95 * n)` of `n` sorted samples, which
/// is `n` itself — the maximum — for every `n` below this. Twenty is the first
/// count at which the rank is `19` and the answer is therefore a percentile
/// rather than the worst sample in the window under another name.
pub const MIN_PERCENTILE_SAMPLES: usize = 20;

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
#[must_use]
pub fn percentile_of(sorted: &[u64], pct: usize) -> Option<u64> {
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

/// A rolling window of the newest `N` samples, and the percentiles read off it.
///
/// The samples are nanoseconds by convention rather than by type: every caller
/// so far is timing something, and a `Duration` here would push the conversion
/// into the percentile arithmetic instead of leaving it at the edges where the
/// unit is known.
///
/// **`N` is a const parameter and not a field** so that
/// [`percentiles`](Self::percentiles) can sort into a stack array. This runs
/// once a frame while a debug panel is up, and a fresh `Vec` per call would be
/// an allocation on that path for a buffer whose size the window already bounds.
#[derive(Clone, Debug)]
pub struct Window<const N: usize> {
    /// Newest last, at most `N` of them.
    samples: std::collections::VecDeque<u64>,
}

impl<const N: usize> Default for Window<N> {
    fn default() -> Self {
        Self {
            samples: std::collections::VecDeque::with_capacity(N),
        }
    }
}

impl<const N: usize> Window<N> {
    /// An empty window.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one sample, evicting the oldest when the window is full.
    pub fn record(&mut self, sample: u64) {
        if self.samples.len() >= N {
            self.samples.pop_front();
        }
        if N > 0 {
            self.samples.push_back(sample);
        }
    }

    /// How many samples the window holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The window's p50 and p95, or `None` below [`MIN_PERCENTILE_SAMPLES`].
    ///
    /// The floor is the whole reason this is one type rather than a `sort` at
    /// each call site: a p95 over fewer samples is the maximum wearing a
    /// percentile's name, and a caller that forgets the check reports it anyway.
    #[must_use]
    pub fn percentiles(&self) -> Option<(u64, u64)> {
        if self.samples.len() < MIN_PERCENTILE_SAMPLES {
            return None;
        }
        let mut sorted = [0u64; N];
        let sorted = &mut sorted[..self.samples.len()];
        for (slot, sample) in sorted.iter_mut().zip(&self.samples) {
            *slot = *sample;
        }
        sorted.sort_unstable();
        Some((percentile_of(sorted, 50)?, percentile_of(sorted, 95)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The arithmetic, against a sequence whose answers are hand-computed.**
    ///
    /// Twenty samples of 1..=20. Nearest rank puts p50 at `ceil(0.50 * 20) = 10`
    /// — the tenth smallest, which is 10 — and p95 at `ceil(0.95 * 20) = 19`,
    /// which is 19 and is *not* the maximum. An off-by-one in either direction
    /// lands on 9, 11, 18 or 20, all of which this catches.
    #[test]
    fn the_percentile_is_the_sample_at_the_nearest_rank() {
        let sorted: Vec<u64> = (1..=20).collect();
        assert_eq!(percentile_of(&sorted, 50), Some(10));
        assert_eq!(percentile_of(&sorted, 95), Some(19));
        assert_eq!(percentile_of(&sorted, 99), Some(20));
        assert_eq!(percentile_of(&sorted, 100), Some(20));
        // The first rank, and the clamp that keeps a small percentile of a small
        // window from indexing at minus one.
        assert_eq!(percentile_of(&sorted, 1), Some(1));
        assert_eq!(percentile_of(&[7], 50), Some(7));
        assert_eq!(percentile_of(&[7], 1), Some(7));
        assert_eq!(percentile_of(&[], 50), None);
    }

    /// **[`MIN_PERCENTILE_SAMPLES`] is derived, and this is the derivation.**
    ///
    /// Its doc comment claims p95 and the maximum are the same sample for every
    /// count below it, which a spot check at one or two counts does not
    /// establish. This walks all of them, so a change to the constant or to the
    /// rank arithmetic fails here rather than leaving a threshold that quietly
    /// no longer means what it says.
    /// **The order samples arrive in must not change the answer**, which is
    /// what the sort inside [`Window::percentiles`] is for.
    #[test]
    fn the_window_sorts_before_it_picks() {
        let mut ascending = Window::<120>::new();
        let mut descending = Window::<120>::new();
        for value in 1..=MIN_PERCENTILE_SAMPLES as u64 {
            ascending.record(value);
        }
        for value in (1..=MIN_PERCENTILE_SAMPLES as u64).rev() {
            descending.record(value);
        }
        assert_eq!(ascending.percentiles(), Some((10, 19)));
        assert_eq!(descending.percentiles(), ascending.percentiles());
    }

    /// **The floor is the window's, not the caller's.** One sample short of
    /// [`MIN_PERCENTILE_SAMPLES`] there is no percentile at all, and the sample
    /// that completes it is the one that produces a number.
    #[test]
    fn a_short_window_has_no_percentile() {
        let mut window = Window::<120>::new();
        assert!(window.is_empty());
        assert_eq!(window.percentiles(), None);
        for _ in 0..MIN_PERCENTILE_SAMPLES - 1 {
            window.record(7);
        }
        assert_eq!(window.len(), MIN_PERCENTILE_SAMPLES - 1);
        assert_eq!(window.percentiles(), None, "one short is still short");
        window.record(7);
        assert_eq!(window.percentiles(), Some((7, 7)));
    }

    /// **The oldest sample leaves when the window is full**, so a window that
    /// has seen a slow patch and then a fast one reports the fast one.
    #[test]
    fn the_window_evicts_the_oldest() {
        const N: usize = 24;
        let mut window = Window::<N>::new();
        for _ in 0..N {
            window.record(1_000);
        }
        assert_eq!(window.len(), N);
        assert_eq!(window.percentiles(), Some((1_000, 1_000)));
        for _ in 0..N {
            window.record(1);
        }
        assert_eq!(window.len(), N, "the window does not grow past its bound");
        assert_eq!(window.percentiles(), Some((1, 1)));
    }

    #[test]
    fn the_minimum_is_the_first_count_at_which_p95_stops_being_the_maximum() {
        for count in 1..MIN_PERCENTILE_SAMPLES {
            let sorted: Vec<u64> = (1..=count as u64).collect();
            assert_eq!(
                percentile_of(&sorted, 95),
                percentile_of(&sorted, 100),
                "at {count} samples a p95 is still the maximum"
            );
        }
        let sorted: Vec<u64> = (1..=MIN_PERCENTILE_SAMPLES as u64).collect();
        assert_ne!(
            percentile_of(&sorted, 95),
            percentile_of(&sorted, 100),
            "at {MIN_PERCENTILE_SAMPLES} samples they must part company"
        );
    }
}
