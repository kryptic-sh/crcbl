//! Per-pass GPU timestamps, and the frame-timing report they feed.
//!
//! `docs/plan/02-vulkan-backend.md` §2.4's last bullet: "GPU timestamp per pass,
//! exposed as a frame-timing report (feeds the stage 7 profiler HUD)". This is
//! that, and it is wired into [`CompiledGraph::execute`](crate::graph::CompiledGraph::execute)
//! rather than bolted on, which is `docs/plan/01-foundations.md` §1.3's stated
//! reason for putting timestamp queries in the seam at P0: "a profiler bolted on
//! afterwards is one that never covers the passes written before it".
//!
//! # The report is frames latent, on purpose
//!
//! Reading a timestamp the GPU has not written yet is either a stall or a lie.
//! So the timers keep a ring of query sets — one more than the frames the loop
//! keeps in flight — and resolve a slot only when it comes back round, by which
//! point the submission that wrote it has certainly completed. The report is
//! therefore about a frame a few frames ago, which is exactly what a profiler
//! HUD wants and exactly the shape `docs/plan/03-gpu-driven-rendering.md` §3.5
//! calls for ("N frames latent").
//!
//! No fence, no wait, no `wait_idle`: the latency *is* the synchronisation.
//!
//! # Timestamps sit outside the passes they measure
//!
//! `crcbl-hal`'s encoder scope rules put query writes "only **outside** any pass
//! — Vulkan forbids them inside dynamic rendering". So a pass's span runs from
//! before its barriers to after its `end_render_pass`, which means the number
//! reported for a pass **includes the transitions the graph inserted for it**.
//! That is the more useful number anyway: a pass whose barriers cost more than
//! its draws is a real finding, and one that hid its barriers in a neighbour's
//! bucket would not surface it.
//!
//! # How many passes a ring slot holds
//!
//! Each slot is sized once, at [`PassTimers::new`], because a query set cannot
//! grow and resizing one mid-frame would throw away the very submissions the
//! ring is waiting on. So the capacity is a number the caller has to know before
//! the first frame — and it is a fact about the renderer, not about the game, so
//! [`MAX_TIMED_PASSES`] is what a caller passes rather than a number of its own.
//! A caller whose frame is a graph of its own — `crcbl-vk`'s end-to-end suite
//! builds one out of a bare [`ForwardRenderer`] and no overlay — sizes it for
//! that graph instead.
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
//! [`write_timestamp`](crcbl_hal::CommandEncoder::write_timestamp) — "accepted
//! and dropped" — because topic 10's browsers are where this actually happens.

use core::fmt::Write as _;

use crcbl_hal::{CommandEncoder, Device, Features, QueryKind, QuerySetDesc, QuerySetHandle};
use crcbl_ui::debug::{DebugModule, DebugSection};

use crate::forward::ForwardRenderer;
use crate::graph::CompiledPass;
use crate::menu::MenuRenderer;
use crate::sprite_pass::SpriteRenderer;
use crate::ui_pass::UiRenderer;

/// How many passes one frame of this crate's renderers records, at most.
///
/// **What a caller passes [`PassTimers::new`] instead of a number of its own.**
/// Every renderer here says how many passes it adds — [`ForwardRenderer`],
/// [`SpriteRenderer`], [`MenuRenderer`] and [`UiRenderer`] each carry a
/// `MAX_PASSES` — and this is their sum, so a pass added anywhere below moves
/// this number with it. A game is in no position to know the count: it is a fact
/// about the renderer, and every sample that guessed it guessed low. The
/// samples' `8` was picked when a frame had three passes and was still `8` when
/// the shadow cascades and the light grid had taken it to fourteen, at which
/// point the timers bracketed the first eight and dropped the rest.
///
/// A caller that declares passes of its own — a clear, a readback, a pass of its
/// own devising — records that many more than this. The samples' clears fit
/// because none of them also runs a [`ForwardRenderer`], and a caller that
/// outgrows the bound gets the one warning [`PassTimers`] logs rather than a
/// silent truncation.
pub const MAX_TIMED_PASSES: u32 = ForwardRenderer::MAX_PASSES
    + SpriteRenderer::MAX_PASSES
    + MenuRenderer::MAX_PASSES
    + UiRenderer::MAX_PASSES;

/// What one pass cost on the GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassTiming {
    /// The pass's label, as it appears in the graph dump and in captures.
    pub label: String,
    /// Nanoseconds between the timestamps bracketing it, barriers included.
    pub gpu_nanos: u64,
}

/// A whole frame's per-pass costs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameTimings {
    /// One entry per pass, in execution order.
    pub passes: Vec<PassTiming>,
    /// Which frame these came from — a few frames behind the one being
    /// recorded; see the module docs.
    pub frame: u64,
}

impl FrameTimings {
    /// The sum of every pass's cost.
    ///
    /// Not the frame time: passes do not overlap in this graph, but the gaps
    /// between them and the present are not measured here.
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
            "frame timing (frame {}): {} pass(es), {:.3} ms total",
            self.frame,
            self.passes.len(),
            total as f64 / 1.0e6
        );
        for pass in &self.passes {
            let share = if total == 0 {
                0.0
            } else {
                pass.gpu_nanos as f64 * 100.0 / total as f64
            };
            let _ = writeln!(
                out,
                "  {:<24} {:>9.3} ms  {:>5.1}%",
                pass.label,
                pass.gpu_nanos as f64 / 1.0e6,
                share
            );
        }
        out
    }
}

/// The renderer's contribution to the debug overlay.
///
/// `docs/plan/07-ui-debug.md`'s first profiler surface is "stage 2/3 GPU pass
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
            "total",
            format_args!("{:.3} ms", self.total_nanos() as f64 / 1.0e6),
        );
        out.row("frame", format_args!("{}", self.frame));
    }
}

#[derive(Debug)]
struct Slot {
    set: QuerySetHandle,
    /// The labels recorded into this slot, or empty if it has never been used.
    labels: Vec<String>,
    /// Which frame number was recorded into it.
    frame: u64,
}

/// A ring of timestamp query sets, one per frame in flight plus one.
#[derive(Debug)]
pub struct PassTimers {
    slots: Vec<Slot>,
    current: usize,
    capacity: u32,
    period_ns: f32,
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
    /// `max_passes` is [`MAX_TIMED_PASSES`] for a caller whose frame is this
    /// crate's renderers, and its own count for one that builds its own graph —
    /// see the module docs. A frame longer than it is timed as far as it goes.
    ///
    /// `None` is not an error: the seam is explicit that timestamps degrade
    /// rather than break, and the caller's answer is to pass `None` to
    /// [`CompiledGraph::execute`](crate::graph::CompiledGraph::execute) and get
    /// an empty report.
    #[must_use]
    pub fn new(device: &dyn Device, frames_in_flight: usize, max_passes: u32) -> Option<Self> {
        if !device.caps().features.contains(Features::TIMESTAMP_QUERY) {
            log::debug!("graph: no TIMESTAMP_QUERY on this device; per-pass timing is off");
            return None;
        }
        // Two queries per pass, and the query index is a `u32` — so a capacity
        // above half the range would overflow every `capacity * 2` below and
        // reset a range shorter than the one it wrote.
        let Some(query_count) = max_passes.checked_mul(2) else {
            log::debug!(
                "graph: {max_passes} passes needs {} timestamp queries, more than a u32 index \
                 holds; per-pass timing is off",
                u64::from(max_passes) * 2
            );
            return None;
        };
        let period_ns = device.caps().limits.timestamp_period_ns;
        // One more than the frames in flight, so the slot about to be reused is
        // always one whose submission has completed.
        let count = frames_in_flight + 1;
        let mut slots = Vec::with_capacity(count);
        for index in 0..count {
            match device.create_query_set(&QuerySetDesc {
                label: Some("graph pass timers"),
                kind: QueryKind::Timestamp,
                // Two per pass: one before its barriers, one after it closes.
                count: query_count,
            }) {
                Ok(set) => slots.push(Slot {
                    set,
                    labels: Vec::new(),
                    frame: 0,
                }),
                Err(error) => {
                    log::debug!("graph: timestamp set {index} refused ({error}); timing is off");
                    for slot in &slots {
                        device.destroy_query_set(slot.set);
                    }
                    return None;
                }
            }
        }
        Some(Self {
            slots,
            current: 0,
            capacity: max_passes,
            period_ns,
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

    /// Starts a frame: resolves the slot about to be reused, then resets it.
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

        // The slot has come round: whatever was recorded into it belongs to a
        // frame that has completed, so reading it neither stalls nor lies.
        self.resolve(device);

        let used = passes.len().min(self.capacity as usize);
        if passes.len() > used && !self.warned {
            self.warned = true;
            log::warn!(
                "graph: {} passes but timers hold {}; the last {} are untimed (said once)",
                passes.len(),
                self.capacity,
                passes.len() - used
            );
        }
        let slot = &mut self.slots[self.current];
        slot.labels.clear();
        slot.labels.extend(
            passes
                .iter()
                .take(used)
                .map(|pass| pass.label().to_string()),
        );
        slot.frame = self.frames;
        // Vulkan requires a reset before every write; the seam makes callers
        // always do it so the Vulkan path is never the special case.
        encoder.reset_query_set(slot.set, 0..self.capacity.saturating_mul(2));
    }

    /// Writes the timestamp that opens pass `index`.
    pub(crate) fn pass_begin(&self, encoder: &mut dyn CommandEncoder, index: usize) {
        if let Ok(index) = u32::try_from(index)
            && index < self.capacity
        {
            encoder.write_timestamp(self.slots[self.current].set, index * 2);
        }
    }

    /// Writes the timestamp that closes pass `index`.
    pub(crate) fn pass_end(&self, encoder: &mut dyn CommandEncoder, index: usize) {
        if let Ok(index) = u32::try_from(index)
            && index < self.capacity
        {
            encoder.write_timestamp(self.slots[self.current].set, index * 2 + 1);
        }
    }

    fn resolve(&mut self, device: &dyn Device) {
        let slot = &self.slots[self.current];
        if slot.labels.is_empty() {
            return;
        }
        let mut raw = vec![0u64; slot.labels.len() * 2];
        if let Err(error) = device.query_results(slot.set, 0, &mut raw) {
            log::debug!("graph: timestamp read failed ({error}); dropping this frame's timing");
            return;
        }
        let passes = slot
            .labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                // `saturating_sub` rather than an assert: a device whose
                // timestamps are unsupported writes zeros, and a zero-length
                // pass is a more useful report than a panic in a profiler.
                let ticks = raw[index * 2 + 1].saturating_sub(raw[index * 2]);
                PassTiming {
                    label: label.clone(),
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    gpu_nanos: (ticks as f64 * f64::from(self.period_ns)) as u64,
                }
            })
            .collect();
        self.latest = FrameTimings {
            passes,
            frame: slot.frame,
        };
    }

    /// Releases the query sets. Call after the device is idle.
    pub fn destroy(&mut self, device: &dyn Device) {
        for slot in &self.slots {
            device.destroy_query_set(slot.set);
        }
        self.slots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            frame: 42,
            passes: vec![
                PassTiming {
                    label: "forward".to_string(),
                    gpu_nanos: 3_000_000,
                },
                PassTiming {
                    label: "tonemap".to_string(),
                    gpu_nanos: 1_000_000,
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

    /// A frame in which every pass measured zero must not divide by zero.
    #[test]
    fn a_zero_length_frame_reports_zero_shares_rather_than_nan() {
        let timings = FrameTimings {
            frame: 1,
            passes: vec![PassTiming {
                label: "clear".to_string(),
                gpu_nanos: 0,
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
            frame: 7,
            passes: vec![
                PassTiming {
                    label: "sprites".to_string(),
                    gpu_nanos: 2_500_000,
                },
                PassTiming {
                    label: "ui".to_string(),
                    gpu_nanos: 250_000,
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
                ("total", "2.750 ms"),
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
                "gpu", "sprites", "2.500 ms", "ui", "0.250 ms", "total", "2.750 ms", "frame", "7",
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
