//! What each pass costs **over a run**, rather than what one latent frame cost.
//!
//! [`crate::timing::PassTimers`] hands back a single [`FrameTimings`] — the
//! newest frame whose query slot has come back round — and
//! `crcbl::engine`'s `finish` used to log exactly that as the run's report. One
//! frame is a sample of one, and the numbers in `docs/plan/45-shadows.md`'s
//! eleventh decision had to be taken as medians of five hand-run binaries
//! because nothing in the tree would produce a median of anything. This is the
//! accumulator that does: every distinct frame's timings go in, and what comes
//! out is a p50 and a p95 per pass over the last [`DEFAULT_FRAME_WINDOW`] of
//! them.
//!
//! It is deliberately the same shape as [`crcbl_ui::BudgetStats`], which does
//! this for the frame *total* — the same rolling
//! [`crcbl_core::stats::Window`], the same floor below which a p95 is
//! the maximum under another name, and the same guard against counting one
//! latent report several times. What is new here is the per-pass breakdown,
//! which is the granularity a quality rung is actually chosen at.
//!
//! # A label is summed across the frame, not tracked per occurrence
//!
//! A frame draws a label more than once and routinely: `lantern` renders two
//! views, so `shadow`, `forward` and `tonemap` each appear twice in its report,
//! and the cull passes appear once per cascade. Recording the occurrences
//! separately would need a key nothing in the graph provides — position in the
//! list, which moves whenever a pass is added — so this **sums the occurrences
//! within one frame** and records one sample per label. The answer is therefore
//! "what did everything called `forward` cost this frame", which is the
//! question a budget asks. The occurrence count is on the row so a reader can
//! see it is more than one.
//!
//! # What it still does not do
//!
//! Fail. Nothing here compares against a recorded baseline, so a rung that
//! doubles the forward pass shows up in the report and reddens nothing;
//! `docs/backlog.md` carries that half.

use std::fmt::Write as _;

use crcbl_core::stats::{MIN_PERCENTILE_SAMPLES, Window};
use crcbl_ui::DEFAULT_FRAME_WINDOW;

use crate::timing::FrameTimings;

/// One pass label's rolling window.
#[derive(Clone, Debug)]
struct Pass {
    label: String,
    window: Window<DEFAULT_FRAME_WINDOW>,
    /// How many times the label appeared in the most recent frame recorded.
    occurrences: usize,
}

/// Per-pass GPU cost over a rolling window of frames.
///
/// Fed one [`FrameTimings`] a frame — the same latent report the debug overlay
/// reads — and read at the end of a run, or whenever a caller wants to know
/// what a pass normally costs rather than what it cost once. See the [module
/// docs](self).
#[derive(Clone, Debug, Default)]
pub struct PassStats {
    /// One entry per distinct label, in the order the labels were first seen.
    passes: Vec<Pass>,
    /// Which frame the newest sample was about, and the guard against counting
    /// one latent report several times: the ring hands the same `FrameTimings`
    /// back until a new slot resolves.
    frame: Option<u64>,
    /// How many distinct frames have been recorded, which is not the window's
    /// length once the window is full.
    frames: u64,
}

impl PassStats {
    /// An empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one frame's per-pass costs, ignoring a report already recorded.
    ///
    /// Reports whether it was a new frame. An empty report — a device with no
    /// timestamps, or a ring that has not come round yet — is not one.
    pub fn record(&mut self, timings: &FrameTimings) -> bool {
        if timings.is_empty() || self.frame == Some(timings.frame) {
            return false;
        }
        self.frame = Some(timings.frame);
        self.frames += 1;
        // Sum first, record second: a label that appears twice in the frame is
        // one sample of their total, not two samples of a half each. Cleared
        // rather than reallocated, so a steady-state frame does not allocate.
        for pass in &mut self.passes {
            pass.occurrences = 0;
        }
        for timed in &timings.passes {
            match self
                .passes
                .iter_mut()
                .find(|pass| pass.label == timed.label)
            {
                Some(pass) => pass.occurrences += 1,
                None => self.passes.push(Pass {
                    label: timed.label.clone(),
                    window: Window::new(),
                    occurrences: 1,
                }),
            }
        }
        for pass in &mut self.passes {
            if pass.occurrences == 0 {
                // The pass did not run this frame — a sky that is `Sky::NONE`,
                // an upscale at full scale. Recording a zero would be a
                // different claim from not recording, and the wrong one: the
                // window is what a pass costs *when it runs*.
                continue;
            }
            let nanos = timings
                .passes
                .iter()
                .filter(|timed| timed.label == pass.label)
                .map(|timed| timed.gpu_nanos)
                .sum();
            pass.window.record(nanos);
        }
        true
    }

    /// How many distinct frames have been recorded.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Whether anything has been recorded at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// The labels seen, in the order they were first seen.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.passes.iter().map(|pass| pass.label.as_str())
    }

    /// One pass's p50 and p95 in nanoseconds, or `None` when the label is
    /// unknown or its window is below [`MIN_PERCENTILE_SAMPLES`].
    #[must_use]
    pub fn percentiles(&self, label: &str) -> Option<(u64, u64)> {
        self.passes
            .iter()
            .find(|pass| pass.label == label)?
            .window
            .percentiles()
    }

    /// The sum of every pass's p50.
    ///
    /// **Not the p50 of the frame** — that is [`crcbl_ui::BudgetStats`]'s
    /// number, taken over the totals themselves. This is what the rows add up
    /// to, which is what the share column is a share of, and the two differ by
    /// however much the passes' slow frames fail to line up.
    #[must_use]
    pub fn p50_total_nanos(&self) -> u64 {
        self.passes
            .iter()
            .filter_map(|pass| pass.window.percentiles())
            .map(|(p50, _)| p50)
            .sum()
    }

    /// The report, as text.
    ///
    /// The distribution counterpart of [`FrameTimings::report`]: that says what
    /// one frame cost, this says what a pass normally costs and how far its tail
    /// runs.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn report(&self) -> String {
        if self.passes.is_empty() {
            return "gpu passes: no GPU timestamps (the device has none)\n".to_string();
        }
        let filling = self
            .passes
            .iter()
            .all(|pass| pass.window.percentiles().is_none());
        if filling {
            let held = self.passes.first().map_or(0, |pass| pass.window.len());
            return format!(
                "gpu passes: filling {held}/{MIN_PERCENTILE_SAMPLES} frames, no percentile yet\n"
            );
        }
        let total = self.p50_total_nanos();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "gpu passes (p50 / p95 over the last {} of {} frames): \
             {} label(s), {:.3} ms of p50",
            DEFAULT_FRAME_WINDOW.min(self.frames as usize),
            self.frames,
            self.passes.len(),
            total as f64 / 1.0e6,
        );
        for pass in &self.passes {
            let Some((p50, p95)) = pass.window.percentiles() else {
                let _ = writeln!(
                    out,
                    "  {:<22} {:>2}x  filling {}/{}",
                    pass.label,
                    pass.occurrences,
                    pass.window.len(),
                    MIN_PERCENTILE_SAMPLES
                );
                continue;
            };
            let share = if total == 0 {
                0.0
            } else {
                p50 as f64 * 100.0 / total as f64
            };
            let _ = writeln!(
                out,
                "  {:<22} {:>2}x  {:>8.3} / {:>8.3} ms  {:>5.1}%",
                pass.label,
                pass.occurrences,
                p50 as f64 / 1.0e6,
                p95 as f64 / 1.0e6,
                share,
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::PassTiming;

    fn frame(number: u64, passes: &[(&str, u64)]) -> FrameTimings {
        FrameTimings {
            passes: passes
                .iter()
                .map(|(label, gpu_nanos)| PassTiming {
                    label: (*label).to_string(),
                    gpu_nanos: *gpu_nanos,
                })
                .collect(),
            frame: number,
        }
    }

    /// Fills every window past the percentile floor with a constant cost.
    fn fill(stats: &mut PassStats, frames: u64, passes: &[(&str, u64)]) {
        for number in 0..frames {
            stats.record(&frame(number, passes));
        }
    }

    /// **The latent report is read every frame and must count once.** The ring
    /// hands the same `FrameTimings` back until a slot resolves, so a window fed
    /// from every read would hold duplicates of whatever the GPU was slow at.
    #[test]
    fn re_reading_the_same_frame_does_not_fill_the_window() {
        let mut stats = PassStats::new();
        let one = frame(7, &[("forward", 1_000)]);
        assert!(stats.record(&one), "the first read is a new frame");
        for _ in 0..MIN_PERCENTILE_SAMPLES * 2 {
            assert!(!stats.record(&one), "the same frame number is not new");
        }
        assert_eq!(stats.frames(), 1);
        assert_eq!(
            stats.percentiles("forward"),
            None,
            "one frame read many times is still one sample"
        );
    }

    /// **An empty report is not a frame.** A device with no timestamps and a
    /// ring that has not come round both hand back an empty `FrameTimings`, and
    /// recording it as a frame of zero passes would advance the frame count
    /// past what was measured.
    #[test]
    fn an_empty_report_records_nothing() {
        let mut stats = PassStats::new();
        assert!(!stats.record(&FrameTimings::default()));
        assert!(stats.is_empty());
        assert_eq!(stats.frames(), 0);
        assert_eq!(
            stats.report(),
            "gpu passes: no GPU timestamps (the device has none)\n"
        );
    }

    /// **Two occurrences of a label are one sample of their sum.** `lantern`
    /// renders two views, so this is the shape of every real report the
    /// accumulator sees.
    #[test]
    fn a_label_drawn_twice_is_summed_within_the_frame() {
        let mut stats = PassStats::new();
        fill(
            &mut stats,
            MIN_PERCENTILE_SAMPLES as u64,
            &[("forward", 300), ("tonemap", 40), ("forward", 200)],
        );
        assert_eq!(stats.percentiles("forward"), Some((500, 500)));
        assert_eq!(stats.percentiles("tonemap"), Some((40, 40)));
        assert_eq!(
            stats.labels().collect::<Vec<_>>(),
            ["forward", "tonemap"],
            "a repeated label is one row, in the order it was first seen"
        );
        assert_eq!(stats.p50_total_nanos(), 540);
        let report = stats.report();
        assert!(report.contains("forward                 2x"), "{report}");
        assert!(report.contains("tonemap                 1x"), "{report}");
    }

    /// **A pass that stops running keeps the window it earned.** A sky set to
    /// `Sky::NONE` mid-run adds no pass, and recording a zero for it would say
    /// the pass got cheaper rather than that it went away.
    #[test]
    fn a_pass_that_stops_running_records_no_zero() {
        let mut stats = PassStats::new();
        fill(
            &mut stats,
            MIN_PERCENTILE_SAMPLES as u64,
            &[("forward", 300), ("sky", 100)],
        );
        assert_eq!(stats.percentiles("sky"), Some((100, 100)));
        for number in 0..MIN_PERCENTILE_SAMPLES as u64 {
            stats.record(&frame(
                MIN_PERCENTILE_SAMPLES as u64 + number,
                &[("forward", 300)],
            ));
        }
        assert_eq!(
            stats.percentiles("sky"),
            Some((100, 100)),
            "the window holds what the pass cost when it ran"
        );
    }

    /// **Below the floor the report says so rather than printing a maximum as a
    /// percentile** — `crcbl_ui::budget`'s rule, and the reason the floor lives
    /// on the window rather than at each call site.
    #[test]
    fn a_short_run_reports_how_full_it_is() {
        let mut stats = PassStats::new();
        fill(
            &mut stats,
            MIN_PERCENTILE_SAMPLES as u64 - 1,
            &[("forward", 300_000)],
        );
        assert_eq!(stats.percentiles("forward"), None);
        assert_eq!(
            stats.report(),
            "gpu passes: filling 19/20 frames, no percentile yet\n"
        );
        stats.record(&frame(999, &[("forward", 300_000)]));
        let report = stats.report();
        assert!(
            report.starts_with(
                "gpu passes (p50 / p95 over the last 20 of 20 frames): 1 label(s), 0.300 ms of p50"
            ),
            "{report}"
        );
        assert!(
            report.contains("forward                 1x     0.300 /    0.300 ms  100.0%"),
            "{report}"
        );
    }

    /// **A label that arrives late is still reported**, filling behind the
    /// others rather than being dropped or silently reported as a maximum.
    #[test]
    fn a_late_label_fills_behind_the_others() {
        let mut stats = PassStats::new();
        fill(
            &mut stats,
            MIN_PERCENTILE_SAMPLES as u64,
            &[("forward", 300)],
        );
        stats.record(&frame(100, &[("forward", 300), ("ssr", 50)]));
        assert_eq!(stats.percentiles("ssr"), None);
        let report = stats.report();
        assert!(
            report.contains("ssr                     1x  filling 1/20"),
            "{report}"
        );
    }
}
