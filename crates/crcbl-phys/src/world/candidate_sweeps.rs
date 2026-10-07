//! Every hit along a straight sweep, not only the closest:
//! [`PhysicsWorld::sweep_sphere_all`] and [`PhysicsWorld::sweep_capsule_all`].
//!
//! The closest-hit sweeps keep one winner in the narrow phase, so whatever lies
//! past the first surface met — a wall beyond a ceiling, a wall beyond one the
//! sweep starts out grazing — is never reported. A caller forecasting a curved
//! path from straight probes needs those too: EW's airborne braking forecast
//! looks for wall-normal turning points behind the first hit. These run the
//! closest sweeps' own broadphase and narrow phase and keep every hit, in the
//! order the closest sweeps rank them, so the first is the closest sweep's
//! answer and a filter of any kind can be built on the list.

use crate::broadphase::Segment;
use crate::collider::Capsule;
use crate::query::ShapeHit;

use super::{
    ColliderId, OverlapQueries, PhysicsWorld, QueryFilter, QueryScratch, sweep_capsule_hits,
    sweep_order, sweep_sphere_hits,
};

impl PhysicsWorld {
    /// Every solid collider a sphere of `radius` meets moving along `segment`,
    /// written into `hits` (which is cleared first), nearest first.
    ///
    /// [`sweep_sphere_filtered`](Self::sweep_sphere_filtered) with nothing
    /// dropped: the same candidates, the same exact test against each, the same
    /// `filter` — triggers skipped, the layer mask and the excluded collider
    /// left out in the narrow phase.
    ///
    /// # One hit per collider
    ///
    /// Each collider the sweep meets is one entry, where the sweep first
    /// touches it: exactly what the closest sweep would report were that
    /// collider alone in the world. Its `t` is the share of `segment` covered
    /// before the touch, in `[0, 1]`; a collider beyond the end of the segment
    /// is not met at all. A collider the sphere starts touching or inside is
    /// an entry at `t = 0` with [`ShapeHit::started_inside`] set, and is kept
    /// rather than ending the query, so what lies behind it is still reported.
    ///
    /// A triangle mesh is one collider, so it reports the first of its
    /// triangles the sweep meets and none of the others: a floor and a wall
    /// that are one mesh hide each other as they would from the closest sweep.
    /// A compound reports each of its parts the sweep meets, as an entry of
    /// its own carrying the compound's id and the part's
    /// [`ShapeHit::part`]: a column behind another column of one item is
    /// still reported.
    ///
    /// # Order
    ///
    /// Nearer first, of two at the same `t` the one in the lower
    /// [`ColliderId::index`], and of two parts of one compound the lower
    /// part. The closest sweep breaks ties the same way, so
    /// `hits.first()` is what `sweep_sphere_filtered` returns for the same
    /// arguments, to the bit — and when nothing starts overlapping, so is the
    /// first entry without `started_inside`.
    ///
    /// # A straight segment, not a path
    ///
    /// `t` is a distance share along one straight segment. A caller that
    /// sweeps the chord of a curved path gets the colliders that chord meets,
    /// which are not the ones the curve meets nor met when it would meet them.
    pub fn sweep_sphere_all(
        &mut self,
        segment: &Segment,
        radius: f64,
        filter: QueryFilter,
        hits: &mut Vec<(ColliderId, ShapeHit)>,
    ) {
        let mut scratch = core::mem::take(&mut self.scratch);
        self.overlap_queries()
            .sweep_sphere_all(segment, radius, filter, &mut scratch, hits);
        self.scratch = scratch;
    }

    /// Every solid collider a Y-aligned capsule meets moving along `segment`,
    /// written into `hits` (which is cleared first), nearest first.
    ///
    /// [`sweep_capsule_filtered`](Self::sweep_capsule_filtered) with nothing
    /// dropped, as [`sweep_sphere_all`](Self::sweep_sphere_all) is the sphere
    /// sweep's: one entry per collider — per part, for a compound — at its
    /// first touch, starting overlaps kept and flagged, nearer first with
    /// ties in collider index and then part order, and
    /// `hits.first()` the closest sweep's answer to the bit. `segment` is the
    /// path of the capsule's centre, as the closest sweep takes it.
    ///
    /// A turned box is swept by the closest sweep's conservative advancement,
    /// which stops a little short of the contact rather than on it, and reports
    /// the normal of the turned face it meets.
    pub fn sweep_capsule_all(
        &mut self,
        segment: &Segment,
        radius: f64,
        half_height: f64,
        filter: QueryFilter,
        hits: &mut Vec<(ColliderId, ShapeHit)>,
    ) {
        let mut scratch = core::mem::take(&mut self.scratch);
        self.overlap_queries().sweep_capsule_all(
            segment,
            radius,
            half_height,
            filter,
            &mut scratch,
            hits,
        );
        self.scratch = scratch;
    }
}

impl OverlapQueries<'_> {
    /// [`PhysicsWorld::sweep_sphere_all`] under a shared borrow, working in
    /// `scratch` instead of the world's own buffers.
    pub fn sweep_sphere_all(
        &self,
        segment: &Segment,
        radius: f64,
        filter: QueryFilter,
        scratch: &mut QueryScratch,
        hits: &mut Vec<(ColliderId, ShapeHit)>,
    ) {
        hits.clear();
        sweep_sphere_hits(*self, segment, radius, filter, scratch, |id, hit| {
            hits.push((id, hit));
        });
        hits.sort_unstable_by(|a, b| sweep_order((a.0, a.1.part, a.1.t), (b.0, b.1.part, b.1.t)));
    }

    /// [`PhysicsWorld::sweep_capsule_all`] under a shared borrow, working in
    /// `scratch` instead of the world's own buffers.
    pub fn sweep_capsule_all(
        &self,
        segment: &Segment,
        radius: f64,
        half_height: f64,
        filter: QueryFilter,
        scratch: &mut QueryScratch,
        hits: &mut Vec<(ColliderId, ShapeHit)>,
    ) {
        hits.clear();
        let capsule = Capsule::new(segment.start, radius, half_height);
        sweep_capsule_hits(
            *self,
            &capsule,
            segment.end,
            filter,
            scratch,
            |_| true,
            |id, hit| {
                hits.push((id, hit));
            },
        );
        hits.sort_unstable_by(|a, b| sweep_order((a.0, a.1.part, a.1.t), (b.0, b.1.part, b.1.t)));
    }
}
