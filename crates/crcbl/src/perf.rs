//! The frame's phases, as span names, and what the frame cost on the CPU.
//!
//! `docs/plan/40-profiling.md`'s first missing piece: "Nothing measures the
//! tick, the ECS schedule, physics, asset upload, culling's CPU half or the
//! shell's frame. The GPU report says which pass cost what; nothing says whether
//! the frame was GPU-bound at all." [`crcbl_core::trace`] is the mechanism;
//! [`engine::Loop::frame`](crate::engine::Loop::frame) is the caller, and this
//! module is the vocabulary the two agree on plus the one rule that reads a
//! number back out.
//!
//! # The phases are the loop's, not a wish list
//!
//! The plan's "CPU breakdown" row names tick, schedule, physics, upload, record
//! and present-wait. Four of those are phases the engine's loop does not have
//! yet — the schedule and physics belong to a game's `tick`, and there is no
//! asset upload in the frame at all — so they are not here. What is here is what
//! [`Loop::frame`](crate::engine::Loop::frame) actually does, in the order it
//! does it:
//!
//! ```text
//! frame                       the whole of one Loop::frame
//! ├── input                   pump the shell, route the pointer, apply menu actions
//! ├── pace                    the frame limiter's sleep
//! ├── tick                    the fixed-step ticks the clock owes
//! ├── draw                    the game's draw, the menu, the overlay, the hand-over
//! └── present                 record, submit and present
//!     └── present-wait        the swapchain wait inside GpuContext::acquire
//! ```
//!
//! `present-wait` appears only when the frame goes through
//! [`GpuContext`](crate::engine::GpuContext), which is every sample's `Gpu` and
//! not `apps/bare`'s hand-written loop; spans nest by where they are opened, so
//! one recorded from a loop of somebody else's is simply at whatever depth that
//! loop left.
//!
//! # What counts as CPU frame time
//!
//! [`frame_cpu_time`] is the frame span's duration **less every span in which
//! the loop was blocked**: [`PACE_SPAN`] and [`PRESENT_WAIT_SPAN`], the two
//! places the loop deliberately stops so something else can catch up. Nothing
//! else is subtracted, and the shell's idle wait is not in the frame span at all
//! — [`Loop::frame`](crate::engine::Loop::frame) opens the span after it.
//!
//! # The frame's counters go on the same trace
//!
//! [`sample_counters`] is the other half of this module's vocabulary: the four
//! names a frame's [`FrameCounters`] are sampled under, so a trace carries what
//! the frame *drew* beside how long it took. The numbers themselves are
//! [`crcbl_render::counters`]' — nothing here recounts them.
//!
//! **The subtraction is the whole point of the row it feeds.** Under vsync the
//! present wait is most of a frame, so a "CPU frame time" that included it would
//! read as the display's period on every machine and every frame, be larger than
//! the GPU total whatever the GPU was doing, and answer "CPU-bound" to a question
//! it had not looked at.

use std::time::Duration;

use crcbl_core::trace::{RecordKind, Snapshot, ThreadTrace};
use crcbl_render::FrameCounters;

/// The frame itself: the outermost span, one per [`Loop::frame`](crate::engine::Loop::frame).
pub const FRAME_SPAN: &str = "frame";

/// Pumping the shell, routing the pointer and applying whatever the menu fired.
pub const INPUT_SPAN: &str = "input";

/// The frame limiter's sleep — [`Clock::advance`](crate::engine::Clock::advance).
///
/// Waiting, not working: subtracted by [`frame_cpu_time`].
pub const PACE_SPAN: &str = "pace";

/// The fixed-step ticks the clock owed this frame.
pub const TICK_SPAN: &str = "tick";

/// The game's draw, the menu, the debug overlay, and handing the draw list over.
pub const DRAW_SPAN: &str = "draw";

/// Recording, submitting and presenting — [`GameGpu::frame`](crate::engine::GameGpu::frame).
pub const PRESENT_SPAN: &str = "present";

/// The swapchain wait inside [`GpuContext::acquire`](crate::engine::GpuContext::acquire):
/// the closed-loop present wait and the acquire itself.
///
/// Waiting, not working: subtracted by [`frame_cpu_time`].
pub const PRESENT_WAIT_SPAN: &str = "present-wait";

/// The spans in which the loop is blocked on something outside it.
///
/// Named once, here, so the rule [`frame_cpu_time`] applies is a list a reader
/// can check against the tree in the module docs rather than a condition spread
/// across a subtraction.
pub const IDLE_SPANS: [&str; 2] = [PACE_SPAN, PRESENT_WAIT_SPAN];

/// Draw calls the frame recorded — [`FrameCounters::draws`].
pub const DRAWS_COUNTER: &str = "draws";

/// Instances the frame submitted to be drawn or culled —
/// [`FrameCounters::instances`].
pub const INSTANCES_COUNTER: &str = "instances";

/// Instances the frame's draws actually covered — [`FrameCounters::drawn`].
///
/// **Absent from a frame that drew indirectly**, rather than sampled as zero;
/// see [`sample_counters`].
pub const DRAWN_COUNTER: &str = "instances-drawn";

/// Triangles those draws covered — [`FrameCounters::triangles`]. Absent on the
/// same terms as [`DRAWN_COUNTER`].
pub const TRIANGLES_COUNTER: &str = "triangles";

/// Puts this frame's counters on the trace, beside its spans.
///
/// `docs/plan/40-profiling.md`'s "counters are spans' siblings", and the caller
/// is [`Loop::frame`](crate::engine::Loop::frame): sampled inside the frame
/// span and before the drain that closes it, so a snapshot holds a frame's
/// counters and that frame's spans together.
///
/// # A counter with no number is not sampled at all
///
/// [`FrameCounters::drawn`] and [`FrameCounters::triangles`] are [`None`] on a
/// frame that drew indirectly — see [`crcbl_render::counters`] — and a `None`
/// sampled as `0` would be indistinguishable in the trace from a frame that
/// genuinely drew nothing. So it is left out, and a consumer that finds no
/// [`DRAWN_COUNTER`] record for a frame is being told the truth rather than a
/// number.
pub fn sample_counters(counters: FrameCounters) {
    crcbl_core::trace::counter(DRAWS_COUNTER, counters.draws);
    crcbl_core::trace::counter(INSTANCES_COUNTER, counters.instances);
    if let Some(drawn) = counters.drawn {
        crcbl_core::trace::counter(DRAWN_COUNTER, drawn);
    }
    if let Some(triangles) = counters.triangles {
        crcbl_core::trace::counter(TRIANGLES_COUNTER, triangles);
    }
}

/// What the frame in `snapshot` cost on the CPU, or `None` if there is no whole
/// frame in it.
///
/// The rule is the module docs': the outermost [`FRAME_SPAN`] less every
/// [`IDLE_SPANS`] span recorded on the same thread.
///
/// `None` rather than a guess when the snapshot holds no complete frame span —
/// a drain taken mid-frame splits one, and half a frame is not a shorter frame.
#[must_use]
pub fn frame_cpu_time(snapshot: &Snapshot) -> Option<Duration> {
    snapshot.threads.iter().find_map(track_cpu_time)
}

/// One track's frame time, or `None` if it recorded no whole frame.
fn track_cpu_time(track: &ThreadTrace) -> Option<Duration> {
    // (name, begin) per span still open, so an end can be paired with its begin.
    // A snapshot may open with an end whose begin was in the previous one; the
    // stack is empty then and there is nothing to pair, which is the same answer
    // as "not this frame's".
    let mut open: Vec<(&'static str, u64)> = Vec::new();
    let mut frame = None;
    let mut idle = 0u64;
    for record in &track.records {
        match record.kind {
            RecordKind::SpanBegin => open.push((record.name, record.nanos)),
            RecordKind::SpanEnd => {
                let Some((name, began)) = open.pop() else {
                    continue;
                };
                let elapsed = record.nanos.saturating_sub(began);
                if name == FRAME_SPAN && record.depth == 0 {
                    frame = Some(elapsed);
                } else if IDLE_SPANS.contains(&name) {
                    idle = idle.saturating_add(elapsed);
                }
            }
            RecordKind::Counter(_) => {}
        }
    }
    // The idle spans are the frame's own children, so this cannot go negative —
    // and `saturating_sub` rather than an assertion because a profiler that
    // panics on a clock that stepped oddly is worse than one that reports zero.
    Some(Duration::from_nanos(frame?.saturating_sub(idle)))
}

#[cfg(test)]
mod tests {
    use crcbl_core::trace::{Record, ThreadTrack};

    use super::*;

    /// Builds a track out of `(name, kind, depth, nanos)` tuples.
    fn track(records: &[(&'static str, RecordKind, u16, u64)]) -> Snapshot {
        Snapshot {
            threads: vec![ThreadTrace {
                track: ThreadTrack::new(0),
                name: Some("main".to_owned()),
                records: records
                    .iter()
                    .map(|&(name, kind, depth, nanos)| Record {
                        name,
                        kind,
                        depth,
                        nanos,
                    })
                    .collect(),
                dropped: 0,
            }],
            refused_tracks: 0,
        }
    }

    const BEGIN: RecordKind = RecordKind::SpanBegin;
    const END: RecordKind = RecordKind::SpanEnd;

    /// **The rule, on a frame with one of everything.** The frame runs from 0 to
    /// 20 ms; the pace sleep costs 3 ms and the present wait 9 ms, so the CPU
    /// spent 8 ms of it working. A version that forgot to subtract reports 20,
    /// one that subtracted the wrong spans reports 11 or 17.
    #[test]
    fn the_cpu_time_is_the_frame_less_the_spans_it_spent_waiting() {
        let snapshot = track(&[
            (FRAME_SPAN, BEGIN, 0, 0),
            (INPUT_SPAN, BEGIN, 1, 1_000_000),
            (INPUT_SPAN, END, 1, 2_000_000),
            (PACE_SPAN, BEGIN, 1, 2_000_000),
            (PACE_SPAN, END, 1, 5_000_000),
            (TICK_SPAN, BEGIN, 1, 5_000_000),
            (TICK_SPAN, END, 1, 7_000_000),
            (DRAW_SPAN, BEGIN, 1, 7_000_000),
            (DRAW_SPAN, END, 1, 9_000_000),
            (PRESENT_SPAN, BEGIN, 1, 9_000_000),
            (PRESENT_WAIT_SPAN, BEGIN, 2, 10_000_000),
            (PRESENT_WAIT_SPAN, END, 2, 19_000_000),
            (PRESENT_SPAN, END, 1, 20_000_000),
            (FRAME_SPAN, END, 0, 20_000_000),
        ]);
        assert_eq!(
            frame_cpu_time(&snapshot),
            Some(Duration::from_millis(20 - 3 - 9))
        );
    }

    /// A frame that waited for nothing keeps its whole span.
    #[test]
    fn a_frame_with_no_waiting_in_it_is_its_own_span() {
        let snapshot = track(&[
            (FRAME_SPAN, BEGIN, 0, 0),
            (TICK_SPAN, BEGIN, 1, 1_000_000),
            (TICK_SPAN, END, 1, 4_000_000),
            (FRAME_SPAN, END, 0, 6_000_000),
        ]);
        assert_eq!(frame_cpu_time(&snapshot), Some(Duration::from_millis(6)));
    }

    /// **Half a frame is not a frame.** A drain taken while the frame span is
    /// open puts its begin in one snapshot and its end in the next; neither is a
    /// frame time, and reporting the fragment as one would feed the row a number
    /// that has nothing to do with a frame.
    #[test]
    fn a_split_frame_reports_nothing_rather_than_a_fragment() {
        let opened = track(&[
            (FRAME_SPAN, BEGIN, 0, 0),
            (INPUT_SPAN, BEGIN, 1, 1_000_000),
            (INPUT_SPAN, END, 1, 2_000_000),
        ]);
        assert_eq!(frame_cpu_time(&opened), None);

        // And the other half: an end whose begin was drained away pairs with
        // nothing, so it is not mistaken for a frame that started at zero.
        let closed = track(&[
            (DRAW_SPAN, END, 1, 3_000_000),
            (FRAME_SPAN, END, 0, 4_000_000),
        ]);
        assert_eq!(frame_cpu_time(&closed), None);
    }

    /// An empty snapshot, and one with nothing but a worker's spans in it.
    #[test]
    fn a_snapshot_with_no_frame_span_in_it_reports_nothing() {
        assert_eq!(frame_cpu_time(&Snapshot::default()), None);
        let worker = track(&[
            ("job", BEGIN, 0, 0),
            (PACE_SPAN, BEGIN, 1, 1_000_000),
            (PACE_SPAN, END, 1, 2_000_000),
            ("job", END, 0, 3_000_000),
        ]);
        assert_eq!(frame_cpu_time(&worker), None);
    }

    /// A frame's own spans are subtracted; a worker thread's identically named
    /// ones are not, because they are not the frame's time to spend.
    #[test]
    fn only_the_frames_own_track_is_read() {
        let mut snapshot = track(&[
            (FRAME_SPAN, BEGIN, 0, 0),
            (PACE_SPAN, BEGIN, 1, 1_000_000),
            (PACE_SPAN, END, 1, 3_000_000),
            (FRAME_SPAN, END, 0, 10_000_000),
        ]);
        let mut worker = track(&[
            (PRESENT_WAIT_SPAN, BEGIN, 0, 0),
            (PRESENT_WAIT_SPAN, END, 0, 5_000_000),
        ]);
        snapshot.threads.append(&mut worker.threads);
        assert_eq!(frame_cpu_time(&snapshot), Some(Duration::from_millis(8)));
    }

    /// The names are the ones the loop opens spans with, spelled once. A rename
    /// on one side and not the other silently stops the row being fed, so the
    /// list is asserted rather than left to two files agreeing by habit.
    #[test]
    fn the_idle_list_is_the_spans_the_loop_spends_blocked() {
        assert_eq!(IDLE_SPANS, [PACE_SPAN, PRESENT_WAIT_SPAN]);
        assert!(!IDLE_SPANS.contains(&FRAME_SPAN));
        assert!(!IDLE_SPANS.contains(&PRESENT_SPAN));
        assert!(!IDLE_SPANS.contains(&TICK_SPAN));
    }
}
