//! The first collider a capsule meets along a constant-acceleration path, and
//! when: [`PhysicsWorld::sweep_capsule_arc`].
//!
//! A straight sweep answers a share of a segment, and its first hit is the
//! first along that segment. A caller moving a body under gravity or braking
//! in the air wants the time on the curved path instead — EW's airborne
//! controller bisected forecasts of straight sweeps to recover it, and had to
//! widen its probes by the chord's error to see a wall the path reaches and
//! turns back from. This sweeps the path itself, by the shape-level search in
//! [`crate::query`]'s arc module, through the same broadphase, filter and
//! hit order as the closest straight sweeps.

use glam::DVec3;

use crate::collider::Aabb;
use crate::query::{
    AcceleratedPath, ArcHit, arc_capsule_vs_box, arc_capsule_vs_sphere,
    arc_capsule_vs_turned_capsule,
};

use super::entry::Primitive;
use super::{
    ColliderId, OverlapQueries, PhysicsWorld, QueryFilter, QueryScratch, ResolvedFilter, SweptHit,
    closest_swept_core,
};

impl PhysicsWorld {
    /// The first solid collider a Y-aligned capsule of `radius` and
    /// `half_height` touches while its centre follows `path`, and when.
    ///
    /// The closest straight sweep's rules, on a curved path: triggers are
    /// skipped, `filter`'s mask and excluded collider are left out in the
    /// narrow phase — a character's own registered capsule is excluded here,
    /// not found inside itself — and of two colliders met at the same time
    /// the one in the lower [`ColliderId::index`] wins, then the lower part
    /// of a compound. A triangle mesh is one collider, answered by the first
    /// of its triangles touched, from either side.
    ///
    /// # Time, not a share
    ///
    /// [`ArcHit::time`] is a time in `[0, path.duration]`, in the path's own
    /// units: what a caller integrating velocity and acceleration over the
    /// same interval needs to split it at the contact. It is never later than
    /// the first touch; how close to it the search stops is
    /// [`ARC_TIME_TOLERANCE`](crate::ARC_TIME_TOLERANCE)'s, and how many
    /// steps it may take [`ARC_MAX_ITERATIONS`](crate::ARC_MAX_ITERATIONS)'s.
    /// The search itself is conservative advancement against the plane that
    /// bounds each shape, described in [`crate::query`]'s arc module.
    /// A path that reaches a surface and turns back from it before its end is
    /// a contact, though the chord between its ends may miss the surface
    /// entirely.
    ///
    /// # Against the straight sweep
    ///
    /// With no acceleration the path is a segment, and the arc meets a box's
    /// face, a sphere, a capsule and a mesh where and when
    /// [`sweep_capsule_filtered`](Self::sweep_capsule_filtered) along that
    /// segment does. An unturned box's edge or corner is the exception: the
    /// straight sweep grows the box by the radius with square corners, and
    /// meets that corner before the round capsule reaches the box, while
    /// this measures the round capsule. A turned box or capsule is swept
    /// straight by a conservative advancement that stops a little short.
    ///
    /// # Starting overlap
    ///
    /// A collider the capsule begins touching or inside is met at time zero,
    /// with [`ArcHit::started_inside`] set, whichever way the path heads —
    /// the answer [`sweep_capsule_filtered`](Self::sweep_capsule_filtered)
    /// gives — and so it comes first. A caller that keeps its capsule clear
    /// of the surfaces it slides along never sees one; one that wants what
    /// lies past a collider it starts in excludes that collider.
    ///
    /// # What it does not cover
    ///
    /// The colliders are where they are now: geometry that moves during the
    /// interval is not swept. The acceleration is constant over the whole
    /// interval; a caller whose acceleration changes partway splits the
    /// interval there. Its validation covers flat walls, ceilings and floors,
    /// round shapes, and EW's planar braking case; tilted and curved walls,
    /// moving geometry and projectile flight are not yet validated against
    /// game fixtures.
    pub fn sweep_capsule_arc(
        &mut self,
        path: &AcceleratedPath,
        radius: f64,
        half_height: f64,
        filter: QueryFilter,
    ) -> Option<(ColliderId, ArcHit)> {
        let mut scratch = core::mem::take(&mut self.scratch);
        let hit = self.overlap_queries().sweep_capsule_arc(
            path,
            radius,
            half_height,
            filter,
            &mut scratch,
        );
        self.scratch = scratch;
        hit
    }
}

impl OverlapQueries<'_> {
    /// [`PhysicsWorld::sweep_capsule_arc`] under a shared borrow, working in
    /// `scratch` instead of the world's own buffers. The two forms are one
    /// implementation.
    #[must_use]
    pub fn sweep_capsule_arc(
        &self,
        path: &AcceleratedPath,
        radius: f64,
        half_height: f64,
        filter: QueryFilter,
        scratch: &mut QueryScratch,
    ) -> Option<(ColliderId, ArcHit)> {
        let filter = ResolvedFilter::solid(self.colliders, self.generations, filter);
        // Everything within the capsule's reach of the box round the whole
        // path, its turning points included, and not only round its ends.
        let reach = DVec3::new(radius, radius + half_height, radius);
        let bounds = path.bounds();
        self.bvh.traverse_aabb_into(
            &Aabb::new(bounds.min - reach, bounds.max + reach),
            &mut scratch.stack,
            &mut scratch.candidates,
        );
        closest_swept_core(
            self.colliders,
            self.generations,
            &scratch.candidates,
            filter,
            |shape| match shape {
                Primitive::Sphere(s) => arc_capsule_vs_sphere(path, radius, half_height, s),
                Primitive::Box(b) => arc_capsule_vs_box(path, radius, half_height, b),
                Primitive::Capsule(c) => {
                    arc_capsule_vs_turned_capsule(path, radius, half_height, c)
                }
                Primitive::Mesh(m) => {
                    m.sweep_arc(path, radius, DVec3::Y * half_height, &mut scratch.mesh)
                }
            },
        )
    }
}

impl SweptHit for ArcHit {
    fn t(&self) -> f64 {
        self.time
    }

    fn part_index(&self) -> usize {
        self.part
    }

    fn with_part(self, part: usize) -> Self {
        Self { part, ..self }
    }
}
