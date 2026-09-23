//! Rigid compounds: a set of boxes fixed in one body's local space, queried
//! at a pose.
//!
//! A game item or a body built from several meshes is bounded tighter by one
//! box per part than by a single box around the lot, and those parts turn with
//! the body. [`AabbCompound`] holds the parts as axis-aligned boxes in the
//! body's own frame and answers queries at whatever [`Transform`] the body has
//! this frame: each query moves its input into local space, runs the existing
//! single-box primitive ([`ray_vs_aabb`], [`Aabb::closest_point`]) against
//! every part, and moves the answer back out. Rotation preserves length, so a
//! distance measured in local space is the world-space distance.
//!
//! What a game does with the answer — reach limits, line of sight, ranking one
//! target against another — is the game's, not this module's.

use crate::broadphase::Ray;
use crate::collider::Aabb;
use crate::components::Transform;
use crate::query::{ShapeHit, ray_vs_aabb};
use glam::DVec3;
use std::fmt;

/// Why [`AabbCompound::new`] refused a set of parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundError {
    /// A corner of the part at `index` holds a `NaN` or an infinity. A
    /// poisoned part would turn every query that reaches it into a `NaN`, and
    /// an infinite one bounds nothing a body can be.
    NonFinitePart {
        /// The part's index in the slice given to [`AabbCompound::new`].
        index: usize,
    },
    /// The part at `index` has `min` past `max` on an axis, so it is empty
    /// ([`Aabb::is_empty`]) and not a part of anything. A flat box, with `min`
    /// equal to `max` on an axis, is accepted.
    InvertedPart {
        /// The part's index in the slice given to [`AabbCompound::new`].
        index: usize,
    },
}

impl fmt::Display for CompoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinitePart { index } => {
                write!(f, "compound part {index} has a non-finite corner")
            }
            Self::InvertedPart { index } => {
                write!(f, "compound part {index} has min past max on an axis")
            }
        }
    }
}

impl std::error::Error for CompoundError {}

/// A ray's nearest hit on an [`AabbCompound`], in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompoundHit {
    /// Index of the part struck, in the slice the compound was built from.
    pub part: usize,
    /// The hit itself: `t` along the world ray, the world point
    /// `ray.origin + ray.dir * t`, and the part's outward face normal turned
    /// into world space. `started_inside` means the ray began inside this part
    /// and the hit is its exit face, as [`ray_vs_aabb`] reports.
    pub hit: ShapeHit,
}

/// The point of one part of an [`AabbCompound`] nearest a query point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompoundPoint {
    /// Index of the part, in the slice the compound was built from.
    pub part: usize,
    /// The nearest world-space point on or in that part. When the query point
    /// is inside the part this is the query point itself, bit for bit.
    pub point: DVec3,
    /// World-space distance from the query point to [`point`](Self::point);
    /// exactly `0.0` inside the part.
    pub distance: f64,
}

/// Axis-aligned boxes fixed in one rigid body's local frame.
///
/// Borrows its parts, so a game keeps them in whatever storage it already has
/// and builds this view per query at the cost of one validation pass. Every
/// part is finite and non-inverted by construction; an empty set of parts is
/// allowed, and every query on it answers `None` (or nothing).
///
/// Queries take the body's pose as a [`Transform`] and refuse — answer `None`
/// — a pose whose position is not finite or whose rotation is not a finite
/// unit quaternion, and a query point or ray that is not finite. None of them
/// skips a `NaN` silently: a poisoned input is never read as a miss that some
/// other part then wins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AabbCompound<'a> {
    parts: &'a [Aabb],
}

impl<'a> AabbCompound<'a> {
    /// A compound of `parts`, each an axis-aligned box in the body's local
    /// frame. Part indices in every answer are indices into `parts`.
    ///
    /// # Errors
    ///
    /// [`CompoundError::NonFinitePart`] for a part with a `NaN` or infinite
    /// corner, [`CompoundError::InvertedPart`] for one with `min` past `max`,
    /// each naming the first such part.
    pub fn new(parts: &'a [Aabb]) -> Result<Self, CompoundError> {
        for (index, part) in parts.iter().enumerate() {
            if !part.min.is_finite() || !part.max.is_finite() {
                return Err(CompoundError::NonFinitePart { index });
            }
            if part.is_empty() {
                return Err(CompoundError::InvertedPart { index });
            }
        }
        Ok(Self { parts })
    }

    /// The parts, in the order given to [`new`](Self::new).
    #[inline]
    #[must_use]
    pub fn parts(&self) -> &'a [Aabb] {
        self.parts
    }

    /// The nearest part `ray` strikes with the body at `pose`, or `None` for a
    /// miss, an empty compound, or a refused input.
    ///
    /// `t` is in units of `ray.dir`'s length, as for [`ray_vs_aabb`], and is
    /// the same in world and local space because rotation preserves length;
    /// `ray.t_min`/`t_max` bound it in those units. A ray that starts inside a
    /// part reports that part's exit face with
    /// [`started_inside`](ShapeHit::started_inside) set — the rule
    /// [`ray_vs_aabb`] follows — so a part the ray enters before that exit
    /// wins. Two parts struck at the same `t` resolve to the lower index.
    ///
    /// Refused, besides a bad `pose`: a ray whose origin or direction is not
    /// finite, whose direction is zero, whose `t_min` is not finite, or whose
    /// `t_max` is `NaN`, or whose origin overflows once taken relative to the
    /// pose. A zero direction has no hit to report, and an unbounded `t_min`
    /// would put the hit at infinity.
    #[must_use]
    pub fn ray_cast(&self, pose: &Transform, ray: &Ray) -> Option<CompoundHit> {
        if !valid_pose(pose)
            || !ray.origin.is_finite()
            || !ray.dir.is_finite()
            || ray.dir == DVec3::ZERO
            || !ray.t_min.is_finite()
            || ray.t_max.is_nan()
        {
            return None;
        }
        let to_local = pose.rotation.inverse();
        let local = Ray {
            origin: to_local * (ray.origin - pose.position),
            dir: to_local * ray.dir,
            ..*ray
        };
        // Finite inputs can still overflow on the way in (an origin and a
        // position at opposite ends of `f64`), and a `NaN` from that would
        // miss every part silently rather than be refused.
        if !local.origin.is_finite() || !local.dir.is_finite() {
            return None;
        }
        let (part, local_hit) = self
            .parts
            .iter()
            .enumerate()
            .filter_map(|(part, aabb)| ray_vs_aabb(&local, aabb).map(|hit| (part, hit)))
            .min_by(|(_, a), (_, b)| a.t.total_cmp(&b.t))?;
        Some(CompoundHit {
            part,
            hit: ShapeHit {
                // Rebuilt from the world ray rather than rotated back, so the
                // point lies on the ray the caller cast.
                point: ray.origin + ray.dir * local_hit.t,
                normal: pose.rotation * local_hit.normal,
                ..local_hit
            },
        })
    }

    /// The nearest point of each part to `point`, with the body at `pose`, in
    /// part order; `None` for a refused input (a bad `pose`, or a `point` that
    /// is not finite or overflows once taken relative to the pose). An empty
    /// compound yields nothing.
    ///
    /// For a caller that filters parts before choosing — by reach or line of
    /// sight — and so needs every candidate rather than only the nearest; see
    /// [`closest_point`](Self::closest_point) for that.
    #[must_use]
    pub fn closest_points(
        &self,
        pose: &Transform,
        point: DVec3,
    ) -> Option<impl Iterator<Item = CompoundPoint> + use<'a>> {
        if !valid_pose(pose) || !point.is_finite() {
            return None;
        }
        let pose = *pose;
        let local = pose.rotation.inverse() * (point - pose.position);
        // Overflow on the way in, refused for the reason `ray_cast` gives.
        if !local.is_finite() {
            return None;
        }
        Some(self.parts.iter().enumerate().map(move |(part, aabb)| {
            let nearest = aabb
                .closest_point(local)
                .expect("parts are finite and non-inverted by construction, the point finite");
            if nearest == local {
                // Inside: the query point is its own answer. Rotating the local
                // copy back out would return it off by a rounding.
                CompoundPoint {
                    part,
                    point,
                    distance: 0.0,
                }
            } else {
                CompoundPoint {
                    part,
                    point: pose.position + pose.rotation * nearest,
                    distance: local.distance(nearest),
                }
            }
        }))
    }

    /// The nearest point on any part to `point`, with the body at `pose`, or
    /// `None` for an empty compound or a refused input (see
    /// [`closest_points`](Self::closest_points)). Two parts at the same
    /// distance resolve to the lower index.
    #[must_use]
    pub fn closest_point(&self, pose: &Transform, point: DVec3) -> Option<CompoundPoint> {
        self.closest_points(pose, point)?
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }
}

/// Whether `pose` can place a body: a finite position and a finite unit
/// rotation. A rotation that is not unit length scales as it turns, so the
/// local-space answers would not map back to world space.
fn valid_pose(pose: &Transform) -> bool {
    pose.position.is_finite() && pose.rotation.is_finite() && pose.rotation.is_normalized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::FRAC_1_SQRT_2;
    use glam::DQuat;

    const EPS: f64 = 1e-12;

    /// A quarter turn about `+Z`: local `+X` is world `+Y`, local `+Y` is
    /// world `-X`. Written out rather than built with
    /// `DQuat::from_rotation_z`, which this crate's determinism lint refuses.
    fn quarter_turn_z() -> DQuat {
        DQuat::from_xyzw(0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2)
    }

    fn pose() -> Transform {
        Transform::new(DVec3::new(10.0, 20.0, 30.0), quarter_turn_z())
    }

    /// EW's fixture: a unit cube at the origin and one two metres along `+X`,
    /// listed far one first so the nearest is not simply the first.
    fn ew_parts() -> [Aabb; 2] {
        [
            Aabb::new(DVec3::new(2.0, 0.0, 0.0), DVec3::new(3.0, 1.0, 1.0)),
            Aabb::new(DVec3::ZERO, DVec3::ONE),
        ]
    }

    fn assert_near(actual: DVec3, expected: DVec3) {
        assert!(
            actual.distance(expected) < EPS,
            "got {actual:?}, expected {expected:?}"
        );
    }

    // -- ray cast ------------------------------------------------------------

    /// EW's `interaction_ray_hit_returns_the_nearest_rigid_compound_part`.
    ///
    /// The ray starts at world `(9.5, 17, 30.5)` heading `+Y`. Relative to the
    /// pose that is `(-0.5, -3, 0.5)`, which the inverse quarter turn takes to
    /// local `(-3, 0.5, 0.5)` heading local `+X`. It enters the cube at the
    /// origin (part 1) at local `x = 0`, `t = 3`, before the far cube (part 0)
    /// at `t = 5`: world point `(9.5, 20, 30.5)`, and the struck face's local
    /// normal `-X` is world `-Y`.
    #[test]
    fn a_ray_hits_the_nearest_part_of_a_rotated_compound() {
        let parts = ew_parts();
        let compound = AabbCompound::new(&parts).unwrap();
        let ray = Ray::new(DVec3::new(9.5, 17.0, 30.5), DVec3::Y);
        let hit = compound.ray_cast(&pose(), &ray).expect("the ray hits");
        assert_eq!(hit.part, 1);
        assert!((hit.hit.t - 3.0).abs() < EPS, "t = {}", hit.hit.t);
        assert_near(hit.hit.point, DVec3::new(9.5, 20.0, 30.5));
        assert_near(hit.hit.normal, DVec3::NEG_Y);
        assert!(!hit.hit.started_inside);
    }

    /// `t` is in units of the direction's length, as for `ray_vs_aabb`: the
    /// same ray with a direction twice as long reaches the same point at half
    /// the `t`.
    #[test]
    fn a_ray_cast_measures_t_in_units_of_its_direction() {
        let parts = ew_parts();
        let compound = AabbCompound::new(&parts).unwrap();
        let ray = Ray::new(DVec3::new(9.5, 17.0, 30.5), DVec3::Y * 2.0);
        let hit = compound.ray_cast(&pose(), &ray).expect("the ray hits");
        assert!((hit.hit.t - 1.5).abs() < EPS, "t = {}", hit.hit.t);
        assert_near(hit.hit.point, DVec3::new(9.5, 20.0, 30.5));
    }

    /// Aimed back the way it came, beside the body, or cut short of it: none
    /// of these touch a part.
    #[test]
    fn a_ray_that_misses_every_part_is_none() {
        let parts = ew_parts();
        let compound = AabbCompound::new(&parts).unwrap();
        let origin = DVec3::new(9.5, 17.0, 30.5);
        assert_eq!(
            compound.ray_cast(&pose(), &Ray::new(origin, DVec3::NEG_Y)),
            None
        );
        // Local y = 0.5 + 3 is above every part.
        assert_eq!(
            compound.ray_cast(&pose(), &Ray::new(origin - DVec3::X * 3.0, DVec3::Y)),
            None
        );
        assert_eq!(
            compound.ray_cast(&pose(), &Ray::new(origin, DVec3::Y).with_bounds(0.0, 2.5)),
            None
        );
    }

    /// Starting at local `(0.5, 0.5, 0.5)`, the centre of part 1, heading
    /// local `+X` (world `+Y`): part 1's exit face is at `t = 0.5`, nearer
    /// than part 0's entry at `t = 1.5`, so part 1 is reported, flagged as
    /// started inside, with the exit face's normal (local `+X`, world `+Y`).
    #[test]
    fn a_ray_starting_inside_a_part_reports_its_exit_face() {
        let parts = ew_parts();
        let compound = AabbCompound::new(&parts).unwrap();
        let origin = DVec3::new(9.5, 20.5, 30.5);
        let hit = compound
            .ray_cast(&pose(), &Ray::new(origin, DVec3::Y))
            .expect("a ray from inside hits its exit face");
        assert_eq!(hit.part, 1);
        assert!(hit.hit.started_inside);
        assert!((hit.hit.t - 0.5).abs() < EPS, "t = {}", hit.hit.t);
        assert_near(hit.hit.point, DVec3::new(9.5, 21.0, 30.5));
        assert_near(hit.hit.normal, DVec3::Y);
    }

    /// EW's refusals, a zero direction and a rotation that is not unit length,
    /// plus every `NaN` input.
    #[test]
    fn a_ray_cast_refuses_non_finite_or_degenerate_input() {
        let parts = ew_parts();
        let compound = AabbCompound::new(&parts).unwrap();
        let good = Ray::new(DVec3::new(9.5, 17.0, 30.5), DVec3::Y);
        assert!(compound.ray_cast(&pose(), &good).is_some());

        let nan = f64::NAN;
        let bad_rays = [
            Ray::new(DVec3::ZERO, DVec3::ZERO),
            Ray::new(DVec3::new(nan, 17.0, 30.5), DVec3::Y),
            Ray::new(good.origin, DVec3::new(0.0, nan, 0.0)),
            Ray::new(good.origin, DVec3::new(0.0, f64::INFINITY, 0.0)),
            good.with_bounds(nan, f64::INFINITY),
            good.with_bounds(f64::NEG_INFINITY, f64::INFINITY),
            good.with_bounds(0.0, nan),
        ];
        for ray in bad_rays {
            assert_eq!(compound.ray_cast(&pose(), &ray), None, "{ray:?}");
        }

        let bad_poses = [
            Transform::new(DVec3::new(10.0, nan, 30.0), quarter_turn_z()),
            Transform::new(pose().position, DQuat::from_xyzw(0.0, 0.0, nan, 1.0)),
        ];
        for bad in bad_poses {
            assert_eq!(compound.ray_cast(&bad, &good), None, "{bad:?}");
        }

        // EW's non-unit rotation case. The ray lines up with part 1 whether
        // or not the rotation scales it, so only the refusal makes it `None`.
        let position = pose().position;
        let ray = Ray::new(position + DVec3::new(-3.0, 0.125, 0.125), DVec3::X);
        let unit = Transform::from_position(position);
        assert!(compound.ray_cast(&unit, &ray).is_some());
        let scaling = Transform::new(position, DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0));
        assert_eq!(compound.ray_cast(&scaling, &ray), None);

        // Every input finite, but the origin relative to the pose overflows.
        let far = Transform::new(DVec3::new(-f64::MAX, 20.0, 30.0), quarter_turn_z());
        let ray = Ray::new(DVec3::new(f64::MAX, 20.0, 30.0), DVec3::Y);
        assert_eq!(compound.ray_cast(&far, &ray), None);
    }

    // -- closest point -------------------------------------------------------

    /// Local query `(3, -1, 0.5)` against the unit cube clamps to
    /// `(1, 0, 0.5)`, a corner edge `√5` away. At the pose the query is world
    /// `(10 + 1, 20 + 3, 30.5)` and the answer world `(10, 21, 30.5)`.
    #[test]
    fn the_closest_point_outside_a_rotated_part_is_on_its_surface() {
        let parts = [Aabb::new(DVec3::ZERO, DVec3::ONE)];
        let compound = AabbCompound::new(&parts).unwrap();
        let nearest = compound
            .closest_point(&pose(), DVec3::new(11.0, 23.0, 30.5))
            .unwrap();
        assert_eq!(nearest.part, 0);
        assert_near(nearest.point, DVec3::new(10.0, 21.0, 30.5));
        assert!(
            (nearest.distance - 5.0_f64.sqrt()).abs() < EPS,
            "distance {}",
            nearest.distance
        );
    }

    /// A point placed at local `(0.1, 0.1, 0.3)`, inside the unit cube: the
    /// answer is the query point itself, bit for bit, at distance zero.
    ///
    /// The body sits at the origin because a translation of tens of metres
    /// absorbs the last-bit error of a round trip through local space, and
    /// the quarter turn's `FRAC_1_SQRT_2` components are not exactly unit
    /// length, so rotating this point out and back again lands its `z` off in
    /// the last bits — which is what an answer rotated back out would return.
    #[test]
    fn the_closest_point_to_a_point_inside_a_part_is_that_point() {
        let parts = [Aabb::new(DVec3::ZERO, DVec3::ONE)];
        let compound = AabbCompound::new(&parts).unwrap();
        let at_origin = Transform::new(DVec3::ZERO, quarter_turn_z());
        let inside = at_origin.rotation * DVec3::new(0.1, 0.1, 0.3);
        let nearest = compound.closest_point(&at_origin, inside).unwrap();
        assert_eq!(nearest.part, 0);
        assert_eq!(nearest.point, inside);
        assert_eq!(nearest.distance, 0.0);
    }

    /// Local query `(1.75, 0.5, 0.5)` is `0.75` from the unit cube (part 1),
    /// `0.25` from the cube at `x = 2` (part 2) and `5.75` from the one at
    /// `x = -5` (part 0). Part 2 wins, at local `(2, 0.5, 0.5)`, world
    /// `(9.5, 22, 30.5)`; `closest_points` reports all three in part order.
    #[test]
    fn the_closest_point_is_chosen_from_the_nearest_of_several_parts() {
        let parts = [
            Aabb::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(-4.0, 1.0, 1.0)),
            Aabb::new(DVec3::ZERO, DVec3::ONE),
            Aabb::new(DVec3::new(2.0, 0.0, 0.0), DVec3::new(3.0, 1.0, 1.0)),
        ];
        let compound = AabbCompound::new(&parts).unwrap();
        let query = DVec3::new(9.5, 21.75, 30.5);

        let nearest = compound.closest_point(&pose(), query).unwrap();
        assert_eq!(nearest.part, 2);
        assert_near(nearest.point, DVec3::new(9.5, 22.0, 30.5));
        assert!((nearest.distance - 0.25).abs() < EPS);

        let every: Vec<_> = compound.closest_points(&pose(), query).unwrap().collect();
        let found: Vec<_> = every.iter().map(|p| (p.part, p.distance)).collect();
        assert_eq!(found.len(), 3);
        for ((part, distance), (want_part, want)) in
            found.into_iter().zip([(0, 5.75), (1, 0.75), (2, 0.25)])
        {
            assert_eq!(part, want_part);
            assert!((distance - want).abs() < EPS, "part {part}: {distance}");
        }
    }

    #[test]
    fn a_closest_point_query_refuses_non_finite_input() {
        let parts = [Aabb::new(DVec3::ZERO, DVec3::ONE)];
        let compound = AabbCompound::new(&parts).unwrap();
        let query = DVec3::new(11.0, 23.0, 30.5);
        assert!(compound.closest_point(&pose(), query).is_some());

        let nan = f64::NAN;
        for bad in [
            DVec3::new(nan, 23.0, 30.5),
            DVec3::new(11.0, f64::INFINITY, 30.5),
        ] {
            assert_eq!(compound.closest_point(&pose(), bad), None, "{bad:?}");
            assert!(compound.closest_points(&pose(), bad).is_none(), "{bad:?}");
        }
        for bad in [
            Transform::new(DVec3::new(nan, 20.0, 30.0), quarter_turn_z()),
            Transform::new(pose().position, DQuat::from_xyzw(nan, 0.0, 0.0, 1.0)),
            Transform::new(pose().position, DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0)),
        ] {
            assert_eq!(compound.closest_point(&bad, query), None, "{bad:?}");
            assert!(compound.closest_points(&bad, query).is_none(), "{bad:?}");
        }

        // Every input finite, but the point relative to the pose overflows.
        let far = Transform::new(DVec3::new(-f64::MAX, 20.0, 30.0), quarter_turn_z());
        let point = DVec3::new(f64::MAX, 20.0, 30.0);
        assert_eq!(compound.closest_point(&far, point), None);
    }

    // -- construction --------------------------------------------------------

    #[test]
    fn an_empty_compound_answers_none() {
        let compound = AabbCompound::new(&[]).unwrap();
        let ray = Ray::new(DVec3::new(9.5, 17.0, 30.5), DVec3::Y);
        assert_eq!(compound.ray_cast(&pose(), &ray), None);
        assert_eq!(compound.closest_point(&pose(), DVec3::ZERO), None);
        assert_eq!(
            compound
                .closest_points(&pose(), DVec3::ZERO)
                .map(Iterator::count),
            Some(0)
        );
    }

    #[test]
    fn a_compound_refuses_a_non_finite_or_inverted_part() {
        let unit = Aabb::new(DVec3::ZERO, DVec3::ONE);
        let nan = Aabb::new(DVec3::new(0.0, f64::NAN, 0.0), DVec3::ONE);
        let infinite = Aabb::new(DVec3::ZERO, DVec3::new(1.0, 1.0, f64::INFINITY));
        let inverted = Aabb::new(DVec3::ONE, DVec3::new(2.0, 0.5, 2.0));
        let flat = Aabb::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0));

        assert_eq!(
            AabbCompound::new(&[unit, nan]),
            Err(CompoundError::NonFinitePart { index: 1 })
        );
        assert_eq!(
            AabbCompound::new(&[infinite, unit]),
            Err(CompoundError::NonFinitePart { index: 0 })
        );
        assert_eq!(
            AabbCompound::new(&[unit, unit, inverted]),
            Err(CompoundError::InvertedPart { index: 2 })
        );
        assert_eq!(
            AabbCompound::new(&[Aabb::EMPTY]),
            Err(CompoundError::NonFinitePart { index: 0 }),
            "Aabb::EMPTY's corners are infinite"
        );
        assert!(AabbCompound::new(&[unit, flat]).is_ok());
    }
}
