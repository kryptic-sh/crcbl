//! The skinning cull: which of a frame's planned skinning ranges anything the
//! frame records can draw.
//!
//! The camera, light and occlusion culls already drop a skinned object whose
//! own box is out of their reach — see [`SkinnedInstanceDesc::bounds`]. The
//! dispatch that deforms its vertices ran regardless, so an off-screen
//! character still cost its whole skinning pass. This is the host-side
//! question that lets the dispatch be dropped with it, asked once per frame as
//! the passes are recorded, when every view the frame draws has been begun.
//!
//! [`SkinnedInstanceDesc::bounds`]: super::SkinnedInstanceDesc::bounds

use super::ForwardRenderer;
use super::shadow_inputs::CasterFootprint;
use super::view::ViewTarget;
use crate::cull::Frustum;

/// One planned region's answer, in [`SkinCull::verdicts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    /// Some object drawn out of it is reached by a cull the frame records.
    Seen,
    /// Every object drawn out of it is out of every cull, and the frame's plan
    /// did not name it: there is no dispatch to drop.
    Unseen,
    /// [`Verdict::Unseen`], and the plan named it: its dispatch is dropped.
    Skipped,
}

/// The skinning cull's per-frame storage, kept on the renderer so a frame
/// allocates nothing once it has grown — see
/// [`ForwardRenderer::cull_skinned_regions`].
#[derive(Debug, Default)]
pub(super) struct SkinCull {
    /// The frame's culls: the cameras of the views it records, then
    /// [`ForwardRenderer::skin_shadow_reach`].
    culls: Vec<Frustum>,
    /// Every skinned object's region and footprint, sorted by region.
    members: Vec<([u32; 2], CasterFootprint)>,
    /// One entry per region an object is drawn out of, sorted by region.
    verdicts: Vec<([u32; 2], Verdict)>,
}

impl SkinCull {
    /// Whether the last [`ForwardRenderer::cull_skinned_regions`] dropped
    /// `region`'s dispatch.
    pub(super) fn skips(&self, region: [u32; 2]) -> bool {
        self.verdicts
            .binary_search_by_key(&region, |(region, _)| *region)
            .is_ok_and(|at| self.verdicts[at].1 == Verdict::Skipped)
    }
}

impl ForwardRenderer {
    /// Decides which regions of this frame's skinning plan, `planned`,
    /// **nothing this frame draws can read**, and so need no dispatch: the
    /// frame's skinning cull, for the primary camera and the secondary `views`
    /// the frame records. [`SkinCull::skips`] answers it afterwards.
    ///
    /// # The test, and why it is conservative
    ///
    /// A region is skipped when at least one object is drawn out of it and
    /// **every** such object's [`CasterFootprint`] reaches none of:
    ///
    /// * the primary camera's frustum, and each of `views`' — the very
    ///   matrices their culls were handed, see `View::camera_view_proj`;
    /// * every shadow cull the frame redraws, and the probe updater's cascade
    ///   and light culls on a frame with shadows off — see
    ///   [`ForwardRenderer::skin_shadow_reach`].
    ///
    /// The footprint is the shadow cache's own: the record's box carried by
    /// its transform with slack in the safe direction, so the host answers
    /// "outside" only where the device's cull would too. An object with no box
    /// is [`CasterFootprint::Everywhere`] and keeps its region, which is
    /// today's behaviour for every caller that passes `None`; so does a region
    /// no object is drawn out of, which only a readback could want.
    ///
    /// **The footprint is of the record the device holds for that slot this
    /// frame**, the one `note_caster_writes` took when the frame's writes were
    /// uploaded — every skinned object is re-pointed, and so written, on every
    /// skinned frame. A caller's write between the frame's begin and its passes
    /// is not uploaded until the next frame, so it does not decide this one
    /// either: an object placed since, into a reused slot, is judged by what
    /// that slot drew before it, which is all the device can draw from it this
    /// frame. A slot past every footprint taken has never been uploaded, and
    /// is taken as everywhere. A view's mask and the occlusion cull only drop
    /// more, so they are not asked.
    ///
    /// # Coming back into view
    ///
    /// A skipped region's halves keep the poses of the last frame that skinned
    /// it, and the parity goes on alternating. Every object drawn out of it is
    /// therefore marked **not pointed** — the rule a fresh object and a change
    /// of region already go through, see `SkinnedInstance::pointed`. The next
    /// skinned frame then writes its current half into both base lanes, so
    /// the frame it returns on reports no deformation motion rather than one
    /// against a pose it held frames ago, and the frame after that subtracts
    /// against the half its return filled. Its transform history is the
    /// pool's and is untouched: a character that walked while it was out of
    /// view still reports the walk.
    ///
    /// # The shadow cache
    ///
    /// Nothing changes there, by construction. A skipped object is still
    /// re-pointed every skinned frame, which is the instance write that keeps
    /// a held map honest; its footprint before and after that write is what
    /// `shadow_groups_reached` tests. A box that has just left a light's reach
    /// redraws that light's map through the *before* half, and the map is
    /// redrawn without the object, which its cull now drops. And a box that
    /// reaches no cull at all reaches no group's record, so skipping its
    /// region holds every map it held.
    ///
    /// # What it costs
    ///
    /// Nothing past one pass over the objects where no skinned object owns a
    /// box, which is every caller that passes `None`. Otherwise a sort of the
    /// objects by region, one footprint test per object until its region is
    /// seen, and a binary search per object and per planned range — in storage
    /// the renderer keeps, see [`SkinCull`].
    ///
    /// With [`set_skinning_culls`](Self::set_skinning_culls) off, nothing is
    /// skipped and nothing is marked.
    pub(super) fn cull_skinned_regions(
        &mut self,
        planned: impl Iterator<Item = [u32; 2]>,
        views: &[ViewTarget],
    ) {
        let cull = &mut self.skin_cull;
        cull.culls.clear();
        cull.members.clear();
        cull.verdicts.clear();
        self.skinned_skipped = 0;
        if !self.skinning_culls {
            return;
        }

        let footprints = &self.shadow_footprints;
        let instances = &self.instances;
        let footprint = |handle| {
            instances
                .index(handle)
                .and_then(|element| footprints.get(element as usize))
                .copied()
                .unwrap_or(CasterFootprint::Everywhere)
        };
        cull.members.extend(
            self.skinned_instances
                .iter()
                .map(|entry| (entry.bases, footprint(entry.handle))),
        );
        if cull
            .members
            .iter()
            .all(|(_, footprint)| *footprint == CasterFootprint::Everywhere)
        {
            return;
        }

        cull.culls
            .push(Frustum::from_view_projection(self.primary.camera_view_proj));
        cull.culls.extend(views.iter().filter_map(|target| {
            self.views
                .get(target.view.index() - 1)
                .and_then(Option::as_ref)
                .map(|view| Frustum::from_view_projection(view.camera_view_proj))
        }));
        cull.culls.extend_from_slice(&self.skin_shadow_reach);
        // A plane that is not finite compares false everywhere, which reads as
        // "outside" — the one answer this must never give by accident. Such a
        // cull reaches everything, so nothing is skipped.
        if cull
            .culls
            .iter()
            .any(|frustum| !frustum.planes.iter().all(|plane| plane.is_finite()))
        {
            return;
        }

        cull.members.sort_unstable_by_key(|(region, _)| *region);
        let culls = &cull.culls;
        for &(region, footprint) in &cull.members {
            let fresh = cull.verdicts.last().is_none_or(|(last, _)| *last != region);
            if fresh {
                cull.verdicts.push((region, Verdict::Unseen));
            }
            let verdict = &mut cull.verdicts.last_mut().expect("pushed above").1;
            if *verdict == Verdict::Unseen && culls.iter().any(|frustum| footprint.reaches(frustum))
            {
                *verdict = Verdict::Seen;
            }
        }

        for region in planned {
            if let Ok(at) = cull
                .verdicts
                .binary_search_by_key(&region, |(region, _)| *region)
                && cull.verdicts[at].1 == Verdict::Unseen
            {
                cull.verdicts[at].1 = Verdict::Skipped;
                self.skinned_skipped += 1;
            }
        }
        if self.skinned_skipped > 0 {
            for entry in &mut self.skinned_instances {
                if cull.skips(entry.bases) {
                    entry.pointed = false;
                }
            }
        }
    }

    /// How many regions the last frame recorded through
    /// [`add_skinned_passes`](Self::add_skinned_passes) — or another entry
    /// point handed a [`Skinning`](crate::Skinning) — had planned and did not
    /// dispatch, because nothing that frame drew could read them. See
    /// [`set_skinning_culls`](Self::set_skinning_culls).
    ///
    /// What a pricing run or a test reads to know the cull did anything: the
    /// colour is the same either way, and the motion target differs only on
    /// the frame a skipped object returns — see
    /// [`set_skinning_culls`](Self::set_skinning_culls).
    #[must_use]
    pub const fn skinned_regions_skipped(&self) -> u32 {
        self.skinned_skipped
    }
}
