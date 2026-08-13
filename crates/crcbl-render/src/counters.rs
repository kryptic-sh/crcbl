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
//! Three of the five are [`Option`], and that is the whole design. A pass that
//! draws **indirectly** — every pass a [`ForwardRenderer`](crate::ForwardRenderer)
//! adds — has its instance count and its index count in a buffer
//! `draw_gen.slang` wrote on the GPU; the CPU records the call and does not know
//! what it covers. So those counters report [`None`] and the row says
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
//! # Instances drawn comes back off the GPU, a few frames late
//!
//! [`drawn`](FrameCounters::drawn) is a number on the GPU-driven path now, and
//! it is the plan's culling win: `cull.slang`'s survivor count, copied out of
//! [`DrawGen::visible_count`](crate::draw_gen::DrawGen::visible_count) by a pass
//! the render graph schedules and read off [`crate::cull_stats`]'s delayed ring.
//! **It is about an older frame than everything beside it**, and
//! [`cull_frame`](FrameCounters::cull_frame) is how a reader can tell: it is the
//! frame the readback came from, and it is [`None`] on a frame whose counters
//! are all the CPU's own.
//!
//! So the section reads as two lags rather than one, deliberately stated rather
//! than smoothed over:
//!
//! - `draws recorded`, `instances submitted` and `triangles drawn` are the
//!   previous frame's, uniformly — see below.
//! - `instances drawn` and `clusters drawn` carry the GPU's answer from the
//!   frame named in `cull frame`, which is
//!   [`CullStatsRing::latency`](crate::cull_stats::CullStatsRing::latency)
//!   frames older again.
//!
//! A frame that mixes an indirect pass with direct ones has a `drawn` total
//! whose GPU-sourced part is that old and whose CPU-sourced part is not.
//! `cull frame` names the oldest frame in the total, so the row can be read as
//! "no part of this is older than that" rather than as one instant.
//!
//! # The row is one frame behind, all of it
//!
//! The engine's loop gathers the debug panel *before* it hands the frame to the
//! GPU — `Loop::frame_body` draws the overlay and then calls `GameGpu::frame`,
//! which is where every renderer's `begin_frame` and `add_passes` run. So the
//! counters a panel shows are the previous frame's, uniformly — and the two
//! rows above are older still, which is what `cull frame` exists to say out
//! loud. That is the arrangement [`crate::timing`]'s ring and
//! `crcbl_ui::budget`'s two windows have to be careful about.
//! `apps/horde`'s `Gpu::scene_stats` already documents the same lag for the same
//! reason.

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
    /// Instances the recorded draws actually cover, or [`None`] where nothing
    /// counted them.
    ///
    /// The pair with [`instances`](Self::instances) is the plan's "instances
    /// submitted vs drawn". Where a pass culls nothing the two are equal and
    /// deliberately so — that is the true answer, not a placeholder.
    ///
    /// On the GPU-driven path this is what the cull kept, off
    /// [`crate::cull_stats`]'s ring, and therefore a number about the frame
    /// [`cull_frame`](Self::cull_frame) names. [`None`] there means the ring has
    /// not come round yet, or the device would not give one — see the module
    /// docs.
    pub drawn: Option<u64>,
    /// Triangles those draws cover, or [`None`] where a pass draws indirectly.
    ///
    /// Exact where it is `Some`: a sprite is six vertices of a triangle list and
    /// the UI pass draws an index count it wrote itself. Nothing here is a
    /// per-mesh average, and nothing derives one from a cluster count.
    pub triangles: Option<u64>,
    /// Clusters the amplification stage kept, or [`None`] where nothing counted
    /// them.
    ///
    /// [`None`] on the two indirect geometry paths and on a device with
    /// `Features::MESH_SHADER` and no `Features::TASK_SHADER`: those frames have
    /// no amplification stage, so nothing rejects a cluster and nothing counts
    /// one. The row says [`UNKNOWN`] there, which is a different claim from the
    /// zero that word would otherwise be — "every cluster was rejected" about a
    /// frame that drew all of them.
    ///
    /// `Some(0)` for a frame of sprites and UI, which really did draw no
    /// clusters. That is what makes [`Default`] the identity of
    /// [`plus`](Self::plus) here as it is for the others.
    pub clusters: Option<u64>,
    /// Which frame [`drawn`](Self::drawn) and [`clusters`](Self::clusters) came
    /// back from, where either came off the GPU.
    ///
    /// [`None`] on a frame whose counters are entirely the CPU's own, which is
    /// every frame with no [`ForwardRenderer`](crate::ForwardRenderer) in it —
    /// there is no second lag to declare, so there is no row.
    pub cull_frame: Option<u64>,
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
            clusters: Some(0),
            // Nothing here came off the GPU, so there is no second lag to
            // declare — and `plus` keeps whichever side does have one.
            cull_frame: None,
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
            clusters: match (self.clusters, other.clusters) {
                (Some(mine), Some(theirs)) => Some(mine + theirs),
                _ => None,
            },
            // **The oldest frame either side drew on**, so the row is read as
            // "no part of this total is older than that". A sum of a latent
            // counter and a live one is genuinely two ages; naming the newer of
            // them would be the half that is not stale claiming the whole.
            cull_frame: match (self.cull_frame, other.cull_frame) {
                (Some(mine), Some(theirs)) if theirs < mine => Some(theirs),
                (Some(mine), _) => Some(mine),
                (None, theirs) => theirs,
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

/// How the cluster counter with no number spells itself.
///
/// A different word from [`INDIRECT`] because it is a different claim. `indirect`
/// says the draw carried its counts in a buffer; this says **nothing in the
/// frame counted the thing at all** — there is no amplification stage on three
/// of the four geometry paths, so no cluster was ever accepted or rejected.
pub const UNKNOWN: &str = "unknown";

/// The panel row.
///
/// Labels are long enough to be unique across the whole panel: `DebugModule`
/// labels share one namespace and nothing detects a collision, so a bare `drawn`
/// here would be the same label `apps/horde`'s scene section already uses, and a
/// test searching the draw list by label text would read whichever came first.
///
/// The last row is the frame stamp, and it appears only when something in the
/// section came off the GPU — see the [module docs](self) on the two lags this
/// section carries and why the newer one is not allowed to speak for both.
impl DebugModule for FrameCounters {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("counters");
        out.row("draws recorded", format_args!("{}", self.draws));
        out.row("instances submitted", format_args!("{}", self.instances));
        write_known(out, "instances drawn", self.drawn, INDIRECT);
        write_known(out, "triangles drawn", self.triangles, INDIRECT);
        write_known(out, "clusters drawn", self.clusters, UNKNOWN);
        if let Some(frame) = self.cull_frame {
            out.row("cull frame", format_args!("{frame}"));
        }
    }
}

/// One row that is either a number or the word that says why it is not.
fn write_known(out: &mut DebugSection, label: &str, value: Option<u64>, absent: &str) {
    match value {
        Some(value) => out.row(label, format_args!("{value}")),
        None => out.row_str(label, absent),
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

    /// A pass that knows everything it drew: a sprite or UI pass, which draws
    /// directly and counts no clusters because it has none.
    fn known(draws: u64, instances: u64, triangles: u64) -> FrameCounters {
        FrameCounters {
            draws,
            instances,
            drawn: Some(instances),
            triangles: Some(triangles),
            clusters: Some(0),
            cull_frame: None,
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
                ("clusters drawn".to_owned(), "0".to_owned()),
            ]
        );
        assert_eq!(
            rows(&known(5, 9, 18)),
            vec![
                ("draws recorded".to_owned(), "5".to_owned()),
                ("instances submitted".to_owned(), "9".to_owned()),
                ("instances drawn".to_owned(), "9".to_owned()),
                ("triangles drawn".to_owned(), "18".to_owned()),
                ("clusters drawn".to_owned(), "0".to_owned()),
            ]
        );
    }

    /// **The frame stamp is a row of its own, and only when there is something
    /// to stamp.** A GPU-sourced counter beside CPU-sourced ones with no way to
    /// tell them apart is the lie this row exists to prevent — and a frame with
    /// no such counter must not grow an empty row saying so.
    #[test]
    fn the_cull_frame_row_appears_only_when_a_counter_came_off_the_gpu() {
        let labels: Vec<String> = rows(&known(3, 412, 824))
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert!(
            !labels.contains(&"cull frame".to_owned()),
            "nothing here came off the GPU: {labels:?}",
        );

        let latent = FrameCounters {
            draws: 15,
            instances: 4097,
            drawn: Some(2043),
            triangles: None,
            clusters: Some(881),
            cull_frame: Some(37),
        };
        assert_eq!(
            rows(&latent),
            vec![
                ("draws recorded".to_owned(), "15".to_owned()),
                ("instances submitted".to_owned(), "4097".to_owned()),
                ("instances drawn".to_owned(), "2043".to_owned()),
                ("triangles drawn".to_owned(), INDIRECT.to_owned()),
                ("clusters drawn".to_owned(), "881".to_owned()),
                ("cull frame".to_owned(), "37".to_owned()),
            ]
        );
    }

    /// **A total mixing two ages names the older one.** A latent culling count
    /// summed with a live sprite count is genuinely two frames' worth, and the
    /// stamp has to be the stale half or it understates how old the row is.
    #[test]
    fn adding_keeps_the_oldest_frame_either_side_drew_on() {
        let sprites = known(2, 30, 60);
        let latent = FrameCounters {
            cull_frame: Some(37),
            ..known(1, 5, 10)
        };
        let older = FrameCounters {
            cull_frame: Some(31),
            ..known(1, 5, 10)
        };
        assert_eq!(sprites.plus(latent).cull_frame, Some(37));
        assert_eq!(latent.plus(sprites).cull_frame, Some(37));
        assert_eq!(latent.plus(older).cull_frame, Some(31));
        assert_eq!(older.plus(latent).cull_frame, Some(31));
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
            clusters: None,
            cull_frame: None,
        };
        assert_eq!(
            rows(&indirect),
            vec![
                ("draws recorded".to_owned(), "4".to_owned()),
                ("instances submitted".to_owned(), "7".to_owned()),
                ("instances drawn".to_owned(), INDIRECT.to_owned()),
                ("triangles drawn".to_owned(), INDIRECT.to_owned()),
                ("clusters drawn".to_owned(), UNKNOWN.to_owned()),
            ]
        );
        assert_ne!(
            INDIRECT, UNKNOWN,
            "the two absences are different claims and must read differently",
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
            clusters: None,
            cull_frame: Some(9),
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
                clusters: Some(0),
                cull_frame: None,
            }
        );
        // Order does not matter.
        assert_eq!(sprites.plus(ui), ui.plus(sprites));

        let indirect = FrameCounters {
            draws: 15,
            instances: 6,
            drawn: None,
            triangles: None,
            clusters: None,
            cull_frame: None,
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
        assert_eq!(total.clusters, None);
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
        assert_eq!(nothing.clusters, Some(0));
        assert_eq!(nothing.cull_frame, None);
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
        let labels: Vec<String> = rows(&FrameCounters {
            cull_frame: Some(1),
            ..FrameCounters::default()
        })
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
                "clusters drawn",
                "cull frame",
            ]
        );
    }
}
