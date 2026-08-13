//! What one frame's passes actually recorded: draws, instances, triangles.
//!
//! `docs/plan/40-profiling.md`'s seventh missing piece — "Counters are
//! piecemeal. `SceneStats`, `visible_count` and each sample's own rows exist;
//! there is no one place a frame's draw count, instance count, cluster count or
//! triangle count is reported." This is that one place, and it lives beside
//! [`crate::timing`] for the reason `FrameTimings` does: the overlay is not
//! allowed to know a render pass exists, so the module is contributed by the
//! system it reports on.
//!
//! Every renderer in this crate says what it recorded —
//! [`SpriteRenderer::counters`](crate::sprite_pass::SpriteRenderer::counters),
//! [`MenuRenderer::counters`](crate::menu::MenuRenderer::counters),
//! [`UiRenderer::counters`](crate::ui_pass::UiRenderer::counters) and
//! [`ForwardRenderer::counters`](crate::forward::ForwardRenderer::counters) —
//! and a caller [`plus`](FrameCounters::plus)es them into one, exactly as it
//! sums their `MAX_PASSES` into
//! [`MAX_TIMED_PASSES`](crate::timing::MAX_TIMED_PASSES). A number is produced
//! once, by whichever renderer wrote the draw.
//!
//! # A counter is either a number the frame has or nothing at all
//!
//! Two of the four are [`Option`], and that is the whole design. A pass that
//! draws **indirectly** — every pass a [`ForwardRenderer`](crate::ForwardRenderer)
//! adds — has its instance count and its index count in a buffer
//! `draw_gen.slang` wrote on the GPU; the CPU records the call and does not know
//! what it covers. So those two counters report [`None`] there and the row says
//! `indirect`, rather than a zero that reads as "nothing was drawn" or a product
//! of a cluster count and a nominal triangles-per-cluster that reads as
//! authoritative and is not.
//!
//! [`FrameCounters::plus`] propagates that: a frame holding one indirect pass
//! has no honest total for either counter, so the sum is `None` however many
//! direct passes it also holds. A counter that silently dropped the indirect
//! pass's share would be `docs/plan/40-profiling.md`'s "counters that lie by
//! omission" written down.
//!
//! # What is **not** here, and why
//!
//! - **Instances drawn on the GPU-driven path**, and with it the culling win the
//!   plan's row is really about. `cull.slang`'s survivor count is in
//!   [`DrawGen::visible_count`](crate::draw_gen::DrawGen::visible_count), which
//!   is device-local and carries `TRANSFER_SRC` so it *can* be copied out — but
//!   nothing copies it. Topic 03 §3.6's readback ring is a buffer and a
//!   `ReadbackDesc` shape today, not a ring: no staging buffer, no copy in the
//!   frame graph, and no consumer. Until one exists this reports what the CPU
//!   recorded and says `indirect` for the rest.
//! - **Clusters drawn and their level histogram.** Same buffer, same gap, and
//!   one more on top: the cluster word is written by the amplification stage, so
//!   it does not exist at all on the two indirect tails or on a device with
//!   `Features::MESH_SHADER` and no `Features::TASK_SHADER`. A cluster row would
//!   be blank on three of the four ways this engine draws.
//!
//! # The row is one frame behind, all of it
//!
//! The engine's loop gathers the debug panel *before* it hands the frame to the
//! GPU — `Loop::frame_body` draws the overlay and then calls `GameGpu::frame`,
//! which is where every renderer's `begin_frame` and `add_passes` run. So the
//! counters a panel shows are the previous frame's, uniformly: there is no live
//! counter beside a latent one here, which is the arrangement
//! [`crate::timing`]'s ring and `crcbl_ui::budget`'s two windows have to be
//! careful about. `apps/horde`'s `Gpu::scene_stats` already documents the same
//! lag for the same reason.

use crcbl_ui::debug::{DebugModule, DebugSection};

/// Draw calls, instances and triangles one frame recorded.
///
/// Built by [`plus`](Self::plus)ing each renderer's contribution onto
/// [`FrameCounters::default`]; see the [module docs](self) for what the two
/// [`Option`]s mean and when they are [`None`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCounters {
    /// Draw calls the frame's passes recorded.
    ///
    /// Always known, on every geometry path: an indirect call is still a call
    /// the CPU wrote into the command buffer. On the GPU-driven path this is
    /// the number `docs/plan/03-gpu-driven-rendering.md` §3.3 is about — one
    /// per bucket, independent of what the scene holds.
    pub draws: u64,
    /// Instances the frame submitted to be drawn or culled.
    ///
    /// Also always known: a sprite pass uploads its instances and a
    /// [`ForwardRenderer`](crate::ForwardRenderer) knows how many live
    /// instances its pool holds, which is what the cull dispatch is given.
    pub instances: u64,
    /// Instances the recorded draws actually cover, or [`None`] where a pass
    /// draws indirectly.
    ///
    /// The pair with [`instances`](Self::instances) is the plan's "instances
    /// submitted vs drawn". Where a pass culls nothing the two are equal and
    /// deliberately so — that is the true answer, not a placeholder.
    pub drawn: Option<u64>,
    /// Triangles those draws cover, or [`None`] where a pass draws indirectly.
    ///
    /// Exact where it is `Some`: a sprite is six vertices of a triangle list and
    /// the UI pass draws an index count it wrote itself. Nothing here is a
    /// per-mesh average.
    pub triangles: Option<u64>,
}

/// A frame that recorded nothing — **and knows it did**.
///
/// Hand-written rather than derived, because a derived `Default` would leave the
/// two [`Option`]s at [`None`], which is this type's word for "the GPU knows and
/// the CPU does not". Those are opposite claims, and the accumulator has to
/// start from the first one: this value is the identity of
/// [`plus`](FrameCounters::plus), so summing a frame's renderers onto it reports
/// exactly what they recorded.
impl Default for FrameCounters {
    fn default() -> Self {
        Self {
            draws: 0,
            instances: 0,
            drawn: Some(0),
            triangles: Some(0),
        }
    }
}

impl FrameCounters {
    /// These and another pass's contribution, added.
    ///
    /// The two totals sum. The two [`Option`]s sum **only when both sides know**
    /// — one unknown makes the total unknown, because a sum that dropped the
    /// unknown side's share would be a smaller number wearing a total's name.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            draws: self.draws + other.draws,
            instances: self.instances + other.instances,
            drawn: match (self.drawn, other.drawn) {
                (Some(mine), Some(theirs)) => Some(mine + theirs),
                _ => None,
            },
            triangles: match (self.triangles, other.triangles) {
                (Some(mine), Some(theirs)) => Some(mine + theirs),
                _ => None,
            },
        }
    }
}

/// How a counter with no honest number spells itself.
///
/// One place, so the row and the assertion that reads it back cannot drift, and
/// deliberately a word rather than a zero or a dash: `indirect` says *why* there
/// is no number.
pub const INDIRECT: &str = "indirect";

/// The panel row.
///
/// Labels are long enough to be unique across the whole panel: `DebugModule`
/// labels share one namespace and nothing detects a collision, so a bare `drawn`
/// here would be the same label `apps/horde`'s scene section already uses, and a
/// test searching the draw list by label text would read whichever came first.
impl DebugModule for FrameCounters {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("counters");
        out.row("draws recorded", format_args!("{}", self.draws));
        out.row("instances submitted", format_args!("{}", self.instances));
        write_known(out, "instances drawn", self.drawn);
        write_known(out, "triangles drawn", self.triangles);
    }
}

/// One row that is either a number or [`INDIRECT`].
fn write_known(out: &mut DebugSection, label: &str, value: Option<u64>) {
    match value {
        Some(value) => out.row(label, format_args!("{value}")),
        None => out.row_str(label, INDIRECT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A section's rows as `(label, value)`, written the way the panel writes
    /// one — cleared before every `debug_section`.
    fn rows(counters: &FrameCounters) -> Vec<(String, String)> {
        let mut section = DebugSection::default();
        counters.debug_section(&mut section);
        assert_eq!(section.title(), "counters");
        section
            .rows()
            .iter()
            .map(|row| (row.label.clone(), row.value.clone()))
            .collect()
    }

    fn known(draws: u64, instances: u64, triangles: u64) -> FrameCounters {
        FrameCounters {
            draws,
            instances,
            drawn: Some(instances),
            triangles: Some(triangles),
        }
    }

    /// **The row renders the numbers it was given, and a different frame renders
    /// different text.** A row wired to a constant passes a test that only
    /// checks the row is there.
    #[test]
    fn the_row_renders_the_counters_it_was_given() {
        assert_eq!(
            rows(&known(3, 412, 824)),
            vec![
                ("draws recorded".to_owned(), "3".to_owned()),
                ("instances submitted".to_owned(), "412".to_owned()),
                ("instances drawn".to_owned(), "412".to_owned()),
                ("triangles drawn".to_owned(), "824".to_owned()),
            ]
        );
        assert_eq!(
            rows(&known(5, 9, 18)),
            vec![
                ("draws recorded".to_owned(), "5".to_owned()),
                ("instances submitted".to_owned(), "9".to_owned()),
                ("instances drawn".to_owned(), "9".to_owned()),
                ("triangles drawn".to_owned(), "18".to_owned()),
            ]
        );
    }

    /// **An unknown counter says `indirect`, never `0`.** A zero in either of
    /// these rows means "the frame drew nothing", and a frame drawing a
    /// GPU-driven scene has drawn plenty.
    #[test]
    fn an_unknown_counter_is_a_word_and_not_a_zero() {
        let indirect = FrameCounters {
            draws: 4,
            instances: 7,
            drawn: None,
            triangles: None,
        };
        assert_eq!(
            rows(&indirect),
            vec![
                ("draws recorded".to_owned(), "4".to_owned()),
                ("instances submitted".to_owned(), "7".to_owned()),
                ("instances drawn".to_owned(), INDIRECT.to_owned()),
                ("triangles drawn".to_owned(), INDIRECT.to_owned()),
            ]
        );
        // And a genuine zero is still a zero, so the two are told apart.
        assert_eq!(
            rows(&known(0, 0, 0))[2],
            ("instances drawn".to_owned(), "0".to_owned())
        );
    }

    /// **Submitted and drawn are separate rows off separate fields.** A version
    /// that rendered one field twice, or swapped the pair, agrees with itself on
    /// a frame that culls nothing — so the frame here culls something.
    #[test]
    fn submitted_and_drawn_are_not_the_same_field() {
        let culled = FrameCounters {
            draws: 1,
            instances: 900,
            drawn: Some(120),
            triangles: Some(240),
        };
        let rendered = rows(&culled);
        assert_eq!(
            rendered[1],
            ("instances submitted".to_owned(), "900".to_owned())
        );
        assert_eq!(
            rendered[2],
            ("instances drawn".to_owned(), "120".to_owned())
        );
    }

    /// The totals sum, and one unknown side makes the whole total unknown.
    #[test]
    fn adding_sums_what_is_known_and_gives_up_on_what_is_not() {
        let sprites = known(3, 400, 800);
        let ui = known(1, 1, 26);
        assert_eq!(
            sprites.plus(ui),
            FrameCounters {
                draws: 4,
                instances: 401,
                drawn: Some(401),
                triangles: Some(826),
            }
        );
        // Order does not matter.
        assert_eq!(sprites.plus(ui), ui.plus(sprites));

        let indirect = FrameCounters {
            draws: 15,
            instances: 6,
            drawn: None,
            triangles: None,
        };
        let total = sprites.plus(indirect);
        assert_eq!(
            total.draws, 18,
            "the draws are still known: the CPU records them"
        );
        assert_eq!(total.instances, 406);
        assert_eq!(
            total.drawn, None,
            "400 of an unknown number is not a total of instances drawn"
        );
        assert_eq!(total.triangles, None);
    }

    /// **The accumulator starts from "nothing, and I know it"**, not from
    /// "nothing, and I have no idea" — a `Default` at `None` would make every
    /// sum unknown for ever, whatever the renderers reported.
    #[test]
    fn the_empty_value_is_zero_and_known_and_is_the_identity_of_plus() {
        let nothing = FrameCounters::default();
        assert_eq!(nothing.draws, 0);
        assert_eq!(nothing.instances, 0);
        assert_eq!(nothing.drawn, Some(0));
        assert_eq!(nothing.triangles, Some(0));
        assert_eq!(nothing.plus(nothing), nothing);

        let sprites = known(2, 30, 60);
        assert_eq!(nothing.plus(sprites), sprites);
        assert_eq!(sprites.plus(nothing), sprites);
    }

    /// Every label this section writes, so a rename is visible and a collision
    /// with another module's row is something a reader can check against this
    /// list.
    #[test]
    fn the_sections_labels_are_its_own() {
        let labels: Vec<String> = rows(&FrameCounters::default())
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(
            labels,
            [
                "draws recorded",
                "instances submitted",
                "instances drawn",
                "triangles drawn",
            ]
        );
    }
}
