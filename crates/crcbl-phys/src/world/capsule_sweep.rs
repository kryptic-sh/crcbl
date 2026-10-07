//! Oriented capsule sweeps shared by lying bodies and character clearance.

use glam::DVec3;

use crate::broadphase::Segment;
use crate::collider::{Aabb, Capsule, LyingCapsule};
use crate::contact::manifold::gap;
use crate::contact::shape::ContactShape;
use crate::contact::sweep::time_of_contact;
use crate::query;

use super::{
    ColliderId, OverlapQueries, Primitive, QueryFilter, QueryScratch, ResolvedFilter, SweptContact,
    SweptHit, closest_swept_core,
};

pub(crate) enum CapsuleSweepShape {
    Upright(Capsule),
    Lying(LyingCapsule),
}

#[derive(Clone, Copy)]
pub(crate) struct CapsuleSweepHit {
    pub(crate) contact: SweptContact,
    pub(crate) penetration: f64,
}

impl SweptHit for CapsuleSweepHit {
    fn t(&self) -> f64 {
        self.contact.t
    }

    fn part_index(&self) -> usize {
        self.contact.part
    }

    fn with_part(self, part: usize) -> Self {
        Self {
            contact: SweptContact {
                part,
                ..self.contact
            },
            ..self
        }
    }
}

impl OverlapQueries<'_> {
    pub(crate) fn sweep_upright_where(
        &self,
        capsule: &Capsule,
        end: DVec3,
        filter: QueryFilter,
        scratch: &mut QueryScratch,
        accept: impl Fn(&crate::query::ShapeHit) -> bool,
    ) -> Option<(ColliderId, crate::query::ShapeHit)> {
        let mut closest = None;
        super::sweep_capsule_hits(*self, capsule, end, filter, scratch, accept, |id, hit| {
            super::keep_closest(&mut closest, id, hit);
        });
        closest
    }

    /// Sweep a capsule, selecting before closest-hit reduction,
    /// including within a triangle mesh. Starting overlaps carry their depth.
    pub(crate) fn sweep_capsule_shape(
        &self,
        body: CapsuleSweepShape,
        motion: DVec3,
        filter: QueryFilter,
        scratch: &mut QueryScratch,
        accept: impl Fn(&SweptContact) -> bool,
    ) -> Option<(ColliderId, CapsuleSweepHit)> {
        let filter = ResolvedFilter::solid(self.colliders, self.generations, filter);
        let (head, feet, radius) = match body {
            CapsuleSweepShape::Upright(c) => (
                c.centre - DVec3::Y * c.half_height,
                c.centre + DVec3::Y * c.half_height,
                c.radius,
            ),
            CapsuleSweepShape::Lying(c) => (c.head, c.feet(), c.radius),
        };
        let reach = DVec3::splat(radius);
        let bounds = Aabb::new(
            head.min(feet).min(head + motion).min(feet + motion) - reach,
            head.max(feet).max(head + motion).max(feet + motion) + reach,
        );
        self.bvh
            .traverse_aabb_into(&bounds, &mut scratch.stack, &mut scratch.candidates);
        let at = |t: f64| ContactShape::Capsule {
            a: head + motion * t,
            b: feet + motion * t,
            radius,
        };
        let advance = |target: ContactShape| {
            let start = gap(&target, &at(0.0));
            let started_inside = start.0 <= 0.0;
            let t = if started_inside {
                0.0
            } else {
                time_of_contact(&target, at, motion, start)?
            };
            Some(CapsuleSweepHit {
                contact: SweptContact {
                    t,
                    normal: if started_inside {
                        start.1
                    } else {
                        gap(&target, &at(t)).1
                    },
                    started_inside,
                    part: 0,
                },
                penetration: (-start.0).max(0.0),
            })
        };
        let centre = (head + feet) * 0.5;
        let path = Segment::new(centre, centre + motion);
        closest_swept_core(
            self.colliders,
            self.generations,
            &scratch.candidates,
            filter,
            |shape| {
                let hit = match shape {
                    Primitive::Sphere(s) => advance(ContactShape::Sphere {
                        centre: s.centre,
                        radius: s.radius,
                    }),
                    Primitive::Box(b) => advance(query::contact_box(b)),
                    Primitive::Capsule(c) => advance(c.contact_shape()),
                    Primitive::Mesh(m) => m
                        .sweep_where(
                            &path,
                            radius,
                            (head - feet) * 0.5,
                            &mut scratch.mesh,
                            |hit| accept(&SweptContact::from(*hit)),
                        )
                        .map(|hit| CapsuleSweepHit {
                            contact: hit.into(),
                            penetration: if hit.started_inside {
                                (radius
                                    - (head - hit.point)
                                        .dot(hit.normal)
                                        .min((feet - hit.point).dot(hit.normal)))
                                .max(0.0)
                            } else {
                                0.0
                            },
                        }),
                }?;
                accept(&hit.contact).then_some(hit)
            },
        )
    }
}
