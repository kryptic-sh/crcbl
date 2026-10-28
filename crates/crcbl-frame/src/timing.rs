//! Per-pass GPU timestamps, and the frame-timing report they feed.
//!
//! Stage 2 §2.4's last bullet: "GPU timestamp per pass,
//! exposed as a frame-timing report (feeds the stage 7 profiler HUD)". This is
//! that, and it is wired into [`CompiledGraph::execute`](crate::graph::CompiledGraph::execute)
//! rather than bolted on, which is the stated reason (`docs/notes/backends.md`)
//! for putting timestamp queries in the seam at P0: "a profiler bolted on
//! afterwards is one that never covers the passes written before it".
//!
//! # The report is frames latent, on purpose
//!
//! Reading a timestamp the GPU has not written yet is either a stall or a lie.
//! So the timers keep a ring of query sets — two more than the frames the loop
//! keeps in flight — and resolve a slot only when it is about to come back
//! round, by which point the submission that wrote it has certainly completed.
//! The second spare slot is for [the statistics' reset](#a-statistics-set-is-reset-a-frame-ahead). The report is
//! therefore about a frame a few frames ago, which is exactly what a profiler
//! HUD wants and exactly the shape topic 03 §3.6
//! calls for ("N frames latent").
//!
//! No fence, no wait, no `wait_idle`: the latency *is* the synchronisation.
//!
//! # Timestamps are the pass's own boundaries, and a copy pass has none
//!
//! The seam has no free-standing timestamp: two queries are named by the pass
//! descriptor itself, as [`PassTimestampWrites`], and the backend takes them
//! where the pass opens and closes. So the number reported for a pass is the
//! pass alone, and the barriers the graph inserted around it are attributed to
//! nothing. That is a narrower number than the one this reported before — the
//! seam used to bracket each pass from before its barriers to after its close —
//! and it is the only one every backend can produce, because Metal samples at
//! stage boundaries and WebGPU only through `timestampWrites`.
//!
//! It also means a [`PassKind::Copy`](crate::graph::PassKind) pass is not
//! timed at all: it opens no scope, so there is no boundary to sample at. Such a
//! pass gets no query pair and **no row in the report**, rather than a row
//! reading 0.000 ms that a reader would take for a measurement.
//!
//! # How many passes a ring slot holds
//!
//! Each slot is sized once, at [`PassTimers::new`], because a query set cannot
//! grow and resizing one mid-frame would throw away the very submissions the
//! ring is waiting on. So the capacity is a number the caller has to know before
//! the first frame — and it is a fact about the renderers, not about the game.
//! Each renderer carries a `MAX_PASSES`, [`UiRenderer::MAX_PASSES`] among them,
//! and a caller passes the sum for the renderers its frame runs:
//! `crcbl_render::MAX_TIMED_PASSES` is that sum for the engine's, and a frame
//! that draws only a UI passes the UI pass's alone, plus any passes of its own.
//!
//! [`UiRenderer::MAX_PASSES`]: crate::ui_pass::UiRenderer::MAX_PASSES
//!
//! A frame with more passes than the capacity is timed as far as the capacity
//! goes and the rest are dropped, with one warning. One, not one per frame: the
//! condition holds for every frame once it holds for any, and this is a
//! profiler.
//!
//! # Degrading, not breaking
//!
//! A device without [`Features::TIMESTAMP_QUERY`] cannot create a timestamp set.
//! [`PassTimers::new`] reports that as `None`, the graph runs with no timers at
//! all, and the report is empty. The seam says the same thing about
//! [`PassTimestampWrites`] — accepted and dropped — because topic 10's browsers
//! are where this actually happens.
//!
//! # What a pass did, when asked
//!
//! [`STATISTICS_VAR`] set to `1` makes the timers count each timed pass's work
//! as well: one [`PassStatisticsWrites`] query per pass, in a statistics set
//! beside each slot's timestamps, read back with them into
//! [`PassTiming::statistics`]. Off by default, because a count is a question
//! for a profiling run, not for every frame. A device without
//! [`Features::PIPELINE_STATISTICS_QUERY`] reports none, and the timings carry
//! on without them.
//!
//! # A statistics set is reset a frame ahead
//!
//! Each frame reads the slot the *next* frame will write, and resets its
//! statistics set in this frame's command buffer, so a slot's counts are reset
//! a whole submission before any pass is counted into them. Resetting in the
//! same command buffer as the passes is what the seam allows, and on AMD's
//! Windows Vulkan driver (25.10.36 on the RX 7900 XTX, 25.10.42 on the Raphael
//! iGPU, measured 2026-10-28) it loses counts: the reset lands after some of the
//! first passes' queries, which then read as never written — up to 27 of
//! lantern's 90 passes on the iGPU, and none once the reset moved a frame ahead.
//! The timestamps' reset stays where it was, since no driver here loses them.

use core::fmt::Write as _;

use crcbl_hal::{
    CommandEncoder, Device, Features, PassStatisticsWrites, PassTimestampWrites,
    PipelineStatistics, QueryKind, QuerySetDesc, QuerySetHandle,
};
use crcbl_ui::debug::{DebugModule, DebugSection};

use crate::graph::{CompiledPass, PassKind};

/// What one pass cost on the GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassTiming {
    /// The pass's label, as it appears in the graph dump and in captures.
    pub label: String,
    /// When the pass began, in nanoseconds on the GPU's timestamp clock: where
    /// a timeline puts it, comparable with every other pass's but not with a
    /// CPU time.
    pub begin_nanos: u64,
    /// Nanoseconds between pass boundaries, including inter-stage scheduling
    /// gaps. These intervals can overlap on a pipelined GPU.
    pub gpu_nanos: u64,
    /// What the GPU counted over the pass, when [`STATISTICS_VAR`] asked for
    /// it and the device can count.
    pub statistics: Option<PipelineStatistics>,
}

/// The variable that makes [`PassTimers`] count each pass's work, with `1`.
pub const STATISTICS_VAR: &str = "CRCBL_PASS_STATISTICS";

/// Whether [`STATISTICS_VAR`] asks for the counts: `1` and nothing else, as
/// `CRCBL_VK_SHADER_STATS` reads.
fn statistics_wanted(value: Option<&str>) -> bool {
    value.map(str::trim) == Some("1")
}

/// A pass's counts as report columns: each stage the device counted, by a
/// short name, and nothing for a stage it could not.
///
/// What [`FrameTimings::report`] and `crcbl_render::PassStats`' report both
/// print after a pass's time.
#[must_use]
pub fn statistics_columns(statistics: &PipelineStatistics) -> String {
    let columns = [
        ("ia-vtx", statistics.input_vertices),
        ("ia-prim", statistics.input_primitives),
        ("vs", statistics.vertex_invocations),
        ("clip", statistics.clipper_invocations),
        ("clip-out", statistics.clipper_primitives),
        ("fs", statistics.fragment_invocations),
        ("cs", statistics.compute_invocations),
        ("task", statistics.task_invocations),
        ("mesh", statistics.mesh_invocations),
        ("mesh-prim", statistics.mesh_primitives),
    ];
    let mut out = String::new();
    for (name, count) in columns {
        if let Some(count) = count {
            if !out.is_empty() {
                out.push(' ');
            }
            let _ = write!(out, "{name} {count}");
        }
    }
    out
}

/// A whole frame's per-pass costs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameTimings {
    /// One entry per pass, in execution order.
    pub passes: Vec<PassTiming>,
    /// Which frame these came from — a few frames behind the one being
    /// recorded; see the module docs.
    pub frame: u64,
    /// Elapsed interval from the earliest measured pass start to the latest
    /// measured pass end. Includes gaps/contention, but not untimed work before
    /// or after those boundaries. `None` means the timestamps were incomplete
    /// or invalid; it must not be replaced with a sum of overlapping spans.
    pub elapsed_nanos: Option<u64>,
}

impl FrameTimings {
    /// The sum of every pass's elapsed span.
    ///
    /// This double-counts overlapping stages and includes scheduling gaps.
    /// It is neither frame duration nor active GPU time. See `elapsed_nanos`
    /// for the outer measured interval.
    #[must_use]
    pub fn total_nanos(&self) -> u64 {
        self.passes.iter().map(|pass| pass.gpu_nanos).sum()
    }

    /// Whether anything was measured at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// The report, as text.
    ///
    /// The other half of the graph's "explain itself" obligation: the dump says
    /// what the frame *is* and this says what it *cost*.
    #[must_use]
    pub fn report(&self) -> String {
        if self.passes.is_empty() {
            return "frame timing: no GPU timestamps (the device has none)\n".to_string();
        }
        let mut out = String::new();
        let total = self.total_nanos();
        let _ = writeln!(
            out,
            "frame timing (frame {}): {} pass(es), {:.3} ms sum of pass spans",
            self.frame,
            self.passes.len(),
            total as f64 / 1.0e6
        );
        if let Some(elapsed) = self.elapsed_nanos {
            let _ = writeln!(
                out,
                "  measured GPU elapsed: {:.3} ms",
                elapsed as f64 / 1.0e6
            );
        }
        for pass in &self.passes {
            let share = if total == 0 {
                0.0
            } else {
                pass.gpu_nanos as f64 * 100.0 / total as f64
            };
            let _ = write!(
                out,
                "  {:<24} {:>9.3} ms  {:>5.1}%",
                pass.label,
                pass.gpu_nanos as f64 / 1.0e6,
                share
            );
            if let Some(statistics) = &pass.statistics {
                let _ = write!(out, "  {}", statistics_columns(statistics));
            }
            out.push('\n');
        }
        out
    }
}

/// The renderer's contribution to the debug overlay.
///
/// The UI plan's first profiler surface (debug item 1 in `docs/notes/tooling.md`) was "stage 2/3 GPU pass
/// timestamps"; this is the adapter that puts them there. It lives here, not in
/// `crcbl-ui`, because the overlay is not allowed to know that a render pass
/// exists — the module is contributed by the system it reports on. `crcbl-render`
/// already depends on `crcbl-ui` (it owns the UI pass), so this is the direction
/// the dependency already runs.
///
/// Every row is a number [`PassTimers`] resolved from a timestamp query. No row
/// is derived from anything else, and there is no CPU number here: this struct
/// measures the GPU and says so.
impl DebugModule for FrameTimings {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("gpu");
        if self.passes.is_empty() {
            // Reached when the device has timers but the ring has not come round
            // yet — the report is deliberately frames latent. A device with no
            // timestamps at all has no `PassTimers`, so the caller adds no
            // section.
            out.row_str("timings", "pending");
            return;
        }
        for pass in &self.passes {
            out.row(
                &pass.label,
                format_args!("{:.3} ms", pass.gpu_nanos as f64 / 1.0e6),
            );
        }
        out.row(
            "span sum",
            format_args!("{:.3} ms", self.total_nanos() as f64 / 1.0e6),
        );
        if let Some(elapsed) = self.elapsed_nanos {
            out.row("elapsed", format_args!("{:.3} ms", elapsed as f64 / 1.0e6));
        }
        out.row("frame", format_args!("{}", self.frame));
    }
}

#[derive(Debug)]
struct Slot {
    set: QuerySetHandle,
    /// One statistics query per timed pass, when [`STATISTICS_VAR`] asked for
    /// them; see the module docs.
    statistics: Option<QuerySetHandle>,
    /// The labels recorded into this slot, or empty if it has never been used.
    labels: Vec<String>,
    /// Which frame number was recorded into it.
    frame: u64,
}

/// A ring of timestamp query sets, one per frame in flight plus two; see the
/// module docs.
#[derive(Debug)]
pub struct PassTimers {
    slots: Vec<Slot>,
    current: usize,
    /// Which query pair each pass of the frame being recorded was given, indexed
    /// by its position in the frame. `None` for a pass that is not timed. Built
    /// by [`PassTimers::begin_frame`] and read by
    /// [`PassTimers::pass_writes`] as the graph walks the same list.
    pairs: Vec<Option<u32>>,
    capacity: u32,
    frames: u64,
    latest: FrameTimings,
    /// Whether a frame with more passes than [`PassTimers::capacity`] has
    /// already been reported. The condition holds for every frame once it holds
    /// for one, and a line per frame forever is a line nobody reads.
    warned: bool,
}

impl PassTimers {
    /// Creates timers for up to `max_passes` passes across `frames_in_flight`
    /// frames, or `None` if the device has no timestamp queries.
    ///
    /// `max_passes` is the sum of the `MAX_PASSES` of the renderers the frame
    /// runs, plus the caller's own passes — see the module docs. A frame longer than it is timed as far as it goes.
    ///
    /// `None` is not an error: the seam is explicit that timestamps degrade
    /// rather than break, and the caller's answer is to pass `None` to
    /// [`CompiledGraph::execute`](crate::graph::CompiledGraph::execute) and get
    /// an empty report.
    ///
    /// The passes are counted as well when [`STATISTICS_VAR`] is `1`; see
    /// [`PassTimers::counting`].
    #[must_use]
    pub fn new(device: &dyn Device, frames_in_flight: usize, max_passes: u32) -> Option<Self> {
        let counted = statistics_wanted(std::env::var(STATISTICS_VAR).ok().as_deref());
        Self::create(device, frames_in_flight, max_passes, counted)
    }

    /// [`PassTimers::new`], counting each timed pass's work whatever
    /// [`STATISTICS_VAR`] says: what a profiler that asks for the counts
    /// itself creates. On a device without
    /// [`Features::PIPELINE_STATISTICS_QUERY`] the passes are timed and not
    /// counted, as under the variable.
    #[must_use]
    pub fn counting(device: &dyn Device, frames_in_flight: usize, max_passes: u32) -> Option<Self> {
        Self::create(device, frames_in_flight, max_passes, true)
    }

    fn create(
        device: &dyn Device,
        frames_in_flight: usize,
        max_passes: u32,
        counted: bool,
    ) -> Option<Self> {
        if !device.caps().features.contains(Features::TIMESTAMP_QUERY) {
            crcbl_core::log::debug!(
                "graph: no TIMESTAMP_QUERY on this device; per-pass timing is off"
            );
            return None;
        }
        // Two queries per pass, and the query index is a `u32` — so a capacity
        // above half the range would overflow every `capacity * 2` below and
        // reset a range shorter than the one it wrote.
        let Some(query_count) = max_passes.checked_mul(2) else {
            crcbl_core::log::debug!(
                "graph: {max_passes} passes needs {} timestamp queries, more than a u32 index \
                 holds; per-pass timing is off",
                u64::from(max_passes) * 2
            );
            return None;
        };
        let counted = counted && {
            let able = device
                .caps()
                .features
                .contains(Features::PIPELINE_STATISTICS_QUERY);
            if !able {
                crcbl_core::log::info!(
                    "graph: passes were to be counted and this device has no \
                     PIPELINE_STATISTICS_QUERY; they are timed, not counted"
                );
            }
            able
        };
        // Two more than the frames in flight, so the slot after the one being
        // written is always one whose submission has completed: it is read,
        // and its statistics reset, a frame before it is reused.
        let count = frames_in_flight + 2;
        let mut slots: Vec<Slot> = Vec::with_capacity(count);
        let refused = |slots: &[Slot], what: &str, index: usize, error: crcbl_hal::HalError| {
            crcbl_core::log::debug!("graph: {what} set {index} refused ({error}); timing is off");
            for slot in slots {
                device.destroy_query_set(slot.set);
                if let Some(statistics) = slot.statistics {
                    device.destroy_query_set(statistics);
                }
            }
        };
        for index in 0..count {
            let set = match device.create_query_set(&QuerySetDesc {
                label: Some("graph pass timers"),
                kind: QueryKind::Timestamp,
                // Two per pass: one before its barriers, one after it closes.
                count: query_count,
            }) {
                Ok(set) => set,
                Err(error) => {
                    refused(&slots, "timestamp", index, error);
                    return None;
                }
            };
            let statistics = if counted {
                match device.create_query_set(&QuerySetDesc {
                    label: Some("graph pass statistics"),
                    kind: QueryKind::PipelineStatistics,
                    count: max_passes,
                }) {
                    Ok(statistics) => Some(statistics),
                    Err(error) => {
                        device.destroy_query_set(set);
                        refused(&slots, "statistics", index, error);
                        return None;
                    }
                }
            } else {
                None
            };
            slots.push(Slot {
                set,
                statistics,
                labels: Vec::new(),
                frame: 0,
            });
        }
        Some(Self {
            slots,
            current: 0,
            pairs: Vec::new(),
            capacity: max_passes,
            frames: 0,
            latest: FrameTimings::default(),
            warned: false,
        })
    }

    /// The most recent frame whose timestamps have actually landed.
    ///
    /// Empty until the ring has come round once.
    #[must_use]
    pub fn latest(&self) -> &FrameTimings {
        &self.latest
    }

    /// How many passes these timers can bracket.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Starts a frame: resolves the slot the next frame reuses, and resets the
    /// timestamps of the one this frame writes and the statistics of the next.
    ///
    /// Called by the graph; a caller never has to.
    pub(crate) fn begin_frame(
        &mut self,
        device: &dyn Device,
        encoder: &mut dyn CommandEncoder,
        passes: &[CompiledPass<'_>],
    ) {
        self.frames += 1;
        self.current = (self.current + 1) % self.slots.len();
        let next = (self.current + 1) % self.slots.len();

        // The next slot is about to come round: whatever was recorded into it
        // belongs to a frame that has completed, so reading it neither stalls
        // nor lies. The current slot was read when it was the next.
        self.resolve(device, next);

        // A copy pass opens no scope, so it has no boundary a backend could
        // sample at and gets no query pair. Everything below counts *timed*
        // passes for that reason, including the warning.
        let timeable = passes
            .iter()
            .filter(|pass| pass.kind() != PassKind::Copy)
            .count();
        let used = timeable.min(self.capacity as usize);
        if timeable > used && !self.warned {
            self.warned = true;
            crcbl_core::log::warn!(
                "graph: {timeable} timed passes but timers hold {}; the last {} are untimed (said \
                 once)",
                self.capacity,
                timeable - used
            );
        }
        let slot = &mut self.slots[self.current];
        slot.labels.clear();
        self.pairs.clear();
        for pass in passes {
            let pair = if pass.kind() == PassKind::Copy {
                None
            } else {
                let next = u32::try_from(slot.labels.len()).unwrap_or(u32::MAX);
                (next < self.capacity).then(|| {
                    slot.labels.push(pass.label().to_string());
                    next
                })
            };
            self.pairs.push(pair);
        }
        slot.frame = self.frames;
        // Vulkan requires a reset before every write; the seam makes callers
        // always do it so the Vulkan path is never the special case.
        encoder.reset_query_set(slot.set, 0..self.capacity.saturating_mul(2));
        // A slot's statistics are reset by the frame before it, see the module
        // docs; the first frame has no frame before it.
        let ahead = self.slots[next].statistics;
        let behind = self.slots[self.current]
            .statistics
            .filter(|_| self.frames == 1);
        for statistics in behind.into_iter().chain(ahead) {
            encoder.reset_query_set(statistics, 0..self.capacity);
        }
    }

    /// The two queries pass `index` of this frame is bracketed by, or `None`
    /// when it is not timed — a copy pass, or one past the ring's capacity.
    ///
    /// **What the graph puts in the pass descriptor.** There is no call to
    /// record: the backend takes both samples where the pass opens and closes.
    pub(crate) fn pass_writes(&self, index: usize) -> Option<PassTimestampWrites> {
        let pair = *self.pairs.get(index)?;
        pair.map(|pair| PassTimestampWrites {
            set: self.slots[self.current].set,
            beginning_of_pass: pair * 2,
            end_of_pass: pair * 2 + 1,
        })
    }

    /// The statistics query pass `index` of this frame is counted into, or
    /// `None` when passes are not counted or this one is not timed.
    ///
    /// The pass's timestamp pair has the same index, so a pass is counted
    /// exactly when it is timed.
    pub(crate) fn pass_statistics(&self, index: usize) -> Option<PassStatisticsWrites> {
        let pair = (*self.pairs.get(index)?)?;
        let set = self.slots[self.current].statistics?;
        Some(PassStatisticsWrites { set, query: pair })
    }

    fn resolve(&mut self, device: &dyn Device, index: usize) {
        let slot = &self.slots[index];
        if slot.labels.is_empty() {
            return;
        }
        // Nanoseconds, not ticks: `Device::query_results` converts inside the
        // backend, where the scale is something the API actually reports.
        let mut nanos = vec![0u64; slot.labels.len() * 2];
        if let Err(error) = device.query_results(slot.set, 0, &mut nanos) {
            crcbl_core::log::debug!(
                "graph: timestamp read failed ({error}); dropping this frame's timing"
            );
            return;
        }
        // The counts are extra, so a failed read drops them and keeps the
        // times.
        let counts = slot.statistics.and_then(|set| {
            let mut counts = vec![PipelineStatistics::default(); slot.labels.len()];
            match device.pipeline_statistics(set, 0, &mut counts) {
                Ok(()) => Some(counts),
                Err(error) => {
                    crcbl_core::log::debug!(
                        "graph: statistics read failed ({error}); this frame is timed, not \
                         counted"
                    );
                    None
                }
            }
        });
        let passes = slot
            .labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                // `saturating_sub` rather than an assert: a device whose
                // timestamps are unsupported writes zeros, and a zero-length
                // pass is a more useful report than a panic in a profiler.
                PassTiming {
                    label: label.clone(),
                    begin_nanos: nanos[index * 2],
                    gpu_nanos: nanos[index * 2 + 1].saturating_sub(nanos[index * 2]),
                    statistics: counts.as_ref().map(|counts| counts[index]),
                }
            })
            .collect();
        self.latest = FrameTimings {
            passes,
            frame: slot.frame,
            elapsed_nanos: measured_span(&nanos),
        };
    }

    /// Releases the query sets. Call after the device is idle.
    pub fn destroy(&mut self, device: &dyn Device) {
        for slot in &self.slots {
            device.destroy_query_set(slot.set);
            if let Some(statistics) = slot.statistics {
                device.destroy_query_set(statistics);
            }
        }
        self.slots.clear();
    }
}

/// Reject incomplete pairs rather than manufacturing a plausible budget value.
fn measured_span(nanos: &[u64]) -> Option<u64> {
    if nanos.is_empty() || !nanos.len().is_multiple_of(2) {
        return None;
    }
    let mut first = u64::MAX;
    let mut last = 0;
    for pair in nanos.as_chunks::<2>().0 {
        let (start, end) = (pair[0], pair[1]);
        if start == 0 || end < start {
            return None;
        }
        first = first.min(start);
        last = last.max(end);
    }
    Some(last - first)
}

// `Instance::create_device` is native-only: see the `crcbl_hal::device` module docs.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn overlapping_passes_measure_the_outer_interval_not_their_sum() {
        assert_eq!(measured_span(&[100, 500, 200, 900]), Some(800));
        assert_eq!(measured_span(&[100, 200, 400, 700]), Some(600));
        assert_eq!(measured_span(&[500, 500]), Some(0));
    }

    #[test]
    fn a_missing_or_backwards_timestamp_cannot_supply_a_budget() {
        for values in [
            &[][..],
            &[100][..],
            &[0, 200][..],
            &[100, 0][..],
            &[200, 100][..],
        ] {
            assert_eq!(measured_span(values), None);
        }
        assert_eq!(measured_span(&[100, 200, 0, 400]), None);
    }

    #[test]
    fn an_empty_report_says_so_rather_than_printing_nothing() {
        let timings = FrameTimings::default();
        assert!(timings.is_empty());
        assert_eq!(timings.total_nanos(), 0);
        assert!(timings.report().contains("no GPU timestamps"));
    }

    #[test]
    fn the_report_lists_every_pass_with_its_share() {
        let timings = FrameTimings {
            elapsed_nanos: None,
            frame: 42,
            passes: vec![
                PassTiming {
                    label: "forward".to_string(),
                    begin_nanos: 0,
                    gpu_nanos: 3_000_000,
                    statistics: None,
                },
                PassTiming {
                    label: "tonemap".to_string(),
                    begin_nanos: 0,
                    gpu_nanos: 1_000_000,
                    statistics: None,
                },
            ],
        };
        assert_eq!(timings.total_nanos(), 4_000_000);
        let report = timings.report();
        assert!(report.contains("frame 42"), "{report}");
        assert!(report.contains("forward"), "{report}");
        assert!(report.contains("tonemap"), "{report}");
        assert!(report.contains("4.000 ms"), "{report}");
        assert!(
            report.contains("75.0%"),
            "the shares must be there: {report}"
        );
        assert!(report.contains("25.0%"), "{report}");
    }

    /// **The counts follow the time, stage by stage, and a stage the device
    /// could not count is left out** rather than printed as a zero a reader
    /// would take for no work.
    #[test]
    fn a_counted_pass_reports_the_stages_it_counted() {
        let statistics = PipelineStatistics {
            vertex_invocations: Some(3),
            fragment_invocations: Some(788),
            task_invocations: Some(0),
            mesh_invocations: None,
            ..PipelineStatistics::default()
        };
        assert_eq!(statistics_columns(&statistics), "vs 3 fs 788 task 0");
        assert_eq!(statistics_columns(&PipelineStatistics::default()), "");

        let timings = FrameTimings {
            elapsed_nanos: None,
            frame: 3,
            passes: vec![
                PassTiming {
                    label: "depth".to_string(),
                    begin_nanos: 0,
                    gpu_nanos: 1_000_000,
                    statistics: Some(statistics),
                },
                PassTiming {
                    label: "tonemap".to_string(),
                    begin_nanos: 0,
                    gpu_nanos: 1_000_000,
                    statistics: None,
                },
            ],
        };
        let report = timings.report();
        let depth = report
            .lines()
            .find(|line| line.contains("depth"))
            .expect("a depth row");
        assert!(depth.ends_with("  vs 3 fs 788 task 0"), "{report}");
        let tonemap = report
            .lines()
            .find(|line| line.contains("tonemap"))
            .expect("a tonemap row");
        assert!(
            tonemap.ends_with('%'),
            "an uncounted pass has no columns: {report}"
        );
    }

    /// `1` turns the counts on and nothing else does, as
    /// `CRCBL_VK_SHADER_STATS` reads.
    #[test]
    fn only_a_one_asks_for_the_counts() {
        assert!(statistics_wanted(Some("1")));
        assert!(statistics_wanted(Some(" 1 ")));
        for value in [None, Some(""), Some("0"), Some("true"), Some("yes")] {
            assert!(!statistics_wanted(value), "{value:?}");
        }
    }

    /// A frame in which every pass measured zero must not divide by zero.
    #[test]
    fn a_zero_length_frame_reports_zero_shares_rather_than_nan() {
        let timings = FrameTimings {
            elapsed_nanos: None,
            frame: 1,
            passes: vec![PassTiming {
                label: "clear".to_string(),
                begin_nanos: 0,
                gpu_nanos: 0,
                statistics: None,
            }],
        };
        let report = timings.report();
        assert!(!report.contains("NaN"), "{report}");
        assert!(report.contains("0.0%"), "{report}");
    }

    /// **The numbers on the overlay are the renderer's own.** Every pass the
    /// timers resolved appears as a row, with the label the graph gave it and
    /// the milliseconds its own nanoseconds convert to — and the whole thing
    /// reaches the draw list, not just the section.
    #[test]
    fn the_debug_section_carries_the_passes_the_renderer_reported() {
        use crcbl_ui::debug::{DebugOverlay, DebugRow};
        use crcbl_ui::draw_list::{DrawCommand, DrawList};
        use crcbl_ui::text::FontAtlas;
        use glam::Vec2;

        let timings = FrameTimings {
            elapsed_nanos: None,
            frame: 7,
            passes: vec![
                PassTiming {
                    label: "sprites".to_string(),
                    begin_nanos: 0,
                    gpu_nanos: 2_500_000,
                    statistics: None,
                },
                PassTiming {
                    label: "ui".to_string(),
                    begin_nanos: 0,
                    gpu_nanos: 250_000,
                    statistics: None,
                },
            ],
        };

        let mut section = DebugSection::default();
        timings.debug_section(&mut section);
        assert_eq!(section.title(), "gpu");
        let rows: Vec<(&str, &str)> = section
            .rows()
            .iter()
            .map(|DebugRow { label, value }| (label.as_str(), value.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("sprites", "2.500 ms"),
                ("ui", "0.250 ms"),
                ("span sum", "2.750 ms"),
                ("frame", "7"),
            ],
        );

        // And through the panel: the same strings land in the draw list under
        // the frame section, in the order the modules were added.
        let mut overlay = DebugOverlay::with_visible(true);
        overlay.record(core::time::Duration::from_millis(16));
        overlay.begin_frame();
        overlay.add(&timings);
        let mut dl = DrawList::new();
        overlay.render(&mut dl, Vec2::new(1280.0, 720.0), &FontAtlas::built_in());

        let drawn: Vec<&str> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(drawn.first(), Some(&"frame"), "{drawn:?}");
        let gpu = drawn
            .iter()
            .position(|text| *text == "gpu")
            .expect("the gpu section is drawn");
        assert_eq!(
            &drawn[gpu..],
            [
                "gpu", "sprites", "2.500 ms", "ui", "0.250 ms", "span sum", "2.750 ms", "frame",
                "7",
            ],
        );
    }

    /// A device that has timers but has not resolved a frame yet says so rather
    /// than reporting a 0.000 ms frame that never ran.
    #[test]
    fn a_latent_report_renders_as_pending_rather_than_zero() {
        let mut section = DebugSection::default();
        FrameTimings::default().debug_section(&mut section);
        assert_eq!(section.title(), "gpu");
        assert_eq!(section.rows().len(), 1);
        assert_eq!(section.rows()[0].value, "pending");
    }

    /// The device this runs against has no timestamp queries, so the whole
    /// subsystem must decline rather than fail.
    #[test]
    fn a_device_without_timestamps_gets_no_timers_and_no_error() {
        use crcbl_hal::null::NullInstance;
        use crcbl_hal::{DeviceDesc, Instance};

        let instance = NullInstance::portable();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        assert!(
            !device.caps().features.contains(Features::TIMESTAMP_QUERY),
            "this test is only meaningful against a device that has none"
        );
        assert!(
            PassTimers::new(device.as_ref(), 2, 8).is_none(),
            "timers must decline rather than fail"
        );
    }
}
