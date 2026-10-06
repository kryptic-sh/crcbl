//! Where a capsule moving under constant acceleration first touches a shape,
//! as a time: the shape-level half of
//! [`PhysicsWorld::sweep_capsule_arc`](crate::PhysicsWorld::sweep_capsule_arc).
//!
//! The straight sweeps answer a share of one segment. A body falling, or
//! braking in the air, does not move along a segment: its path bows away from
//! the chord between its ends by `|a| T² / 8` at the middle, so a chord can
//! miss a wall the path reaches and leaves, and a share of the chord is not a
//! time on the path. These answer the time itself, on the path itself.
//!
//! # Conservative advancement against a supporting plane
//!
//! Each shape is advanced on alone, by conservative advancement — Mirtich,
//! _Impulse-based Dynamic Simulation of Rigid Body Systems_, PhD thesis, UC
//! Berkeley, 1996 — with the step bounded by a plane rather than by a speed:
//!
//! - measure the gap and its normal `n` with the narrow phase the straight
//!   sweeps use (the contact pipeline's `gap`, or a mesh's own triangle
//!   distance);
//! - take the plane across `n` through the shape's furthest point along it,
//!   its support: the whole shape lies behind that plane, so how far the
//!   capsule is in front of it is a lower bound on the gap, now and at every
//!   later time;
//! - along a constant-acceleration path that height changes by exactly
//!   `n · (v s + ½ a s²)` over the next `s`, a quadratic, so its first root
//!   is a time before which the capsule cannot touch the shape; step there.
//!
//! No contact is stepped over, a touch-and-leave included: the bound holds at
//! every time in the step, not only at its end, because it is the whole path
//! against the plane. Its rate of change, `n · (v + a s)`, is never faster
//! than `|v| + |a| s`, so the step is never shorter than a bound on the
//! distance's rate of change would allow, and for a flat face it lands on
//! the contact in one step. A path that never reaches the plane never
//! reaches the shape, which is what ends the search for a body sliding along
//! a wall rather than into it.
//!
//! The search reports a contact once the next safe step is no longer than
//! [`ARC_TIME_TOLERANCE`], or the gap is gone; and gives up, reporting where
//! it got to, after [`ARC_MAX_ITERATIONS`] steps. Either way the time
//! reported is never later than the first touch.

use glam::{DQuat, DVec3};

use super::{TurnedCapsule, contact_box, nearest_on_box};
use crate::collider::{Aabb, BoxCollider, Sphere};
use crate::contact::manifold::{closest_between_segments, gap};
use crate::contact::shape::ContactShape;

/// How short the next safe step of
/// [`PhysicsWorld::sweep_capsule_arc`](crate::PhysicsWorld::sweep_capsule_arc)
/// must become for the capsule to count as touching, in the path's own units
/// of time — seconds for a velocity in metres per second.
///
/// A contact is reported at most this long before the capsule could next
/// reach the plane bounding the shape; a pass that comes within the distance
/// the capsule closes in this time may be reported as a touch. EW's local
/// contact bisection stops at the same interval.
pub const ARC_TIME_TOLERANCE: f64 = 1e-9;

/// The most steps
/// [`PhysicsWorld::sweep_capsule_arc`](crate::PhysicsWorld::sweep_capsule_arc)
/// takes against one shape. A search still short of [`ARC_TIME_TOLERANCE`]
/// after this many reports a contact where its last step left it, which is
/// never past the first touch: the answer Box2D's `b2TimeOfImpact` gives when
/// it runs out of iterations. A flat face is met in one step, and an edge or
/// a round surface approached head on in a handful.
pub const ARC_MAX_ITERATIONS: usize = 64;

/// A point moving under constant acceleration for a while: the path of a
/// capsule's centre that
/// [`PhysicsWorld::sweep_capsule_arc`](crate::PhysicsWorld::sweep_capsule_arc)
/// sweeps.
///
/// At time `t` in `[0, duration]` the point is at
/// `start + velocity t + ½ acceleration t²`. Gravity and a steering
/// acceleration together are one constant acceleration; anything that
/// changes over the interval — a correction that ends, a drag — is the
/// caller's to split the interval at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceleratedPath {
    /// Where the point is at time zero.
    pub start: DVec3,
    /// Its velocity at time zero.
    pub velocity: DVec3,
    /// Its constant acceleration.
    pub acceleration: DVec3,
    /// How long it moves for: the end of the interval `[0, duration]`.
    pub duration: f64,
}

impl AcceleratedPath {
    /// A path from `start` at `velocity`, accelerating at `acceleration`, for
    /// `duration`.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `duration` is negative or not finite.
    #[must_use]
    pub fn new(start: DVec3, velocity: DVec3, acceleration: DVec3, duration: f64) -> Self {
        debug_assert!(
            duration >= 0.0 && duration.is_finite(),
            "a path's duration must be finite and non-negative"
        );
        Self {
            start,
            velocity,
            acceleration,
            duration,
        }
    }

    /// Where the point is at `time`.
    #[must_use]
    pub fn position_at(&self, time: f64) -> DVec3 {
        self.start + self.velocity * time + self.acceleration * (0.5 * time * time)
    }

    /// The point's velocity at `time`.
    #[must_use]
    pub fn velocity_at(&self, time: f64) -> DVec3 {
        self.velocity + self.acceleration * time
    }

    /// The smallest world-axis box holding the whole path: its two ends, and
    /// on each axis whose motion turns back within the interval the turning
    /// point — a jump's apex lies above both its ends.
    pub(crate) fn bounds(&self) -> Aabb {
        let end = self.position_at(self.duration);
        let (mut min, mut max) = (self.start.min(end), self.start.max(end));
        for axis in 0..3 {
            let acceleration = self.acceleration[axis];
            if acceleration == 0.0 {
                continue;
            }
            let turn = -self.velocity[axis] / acceleration;
            if turn > 0.0 && turn < self.duration {
                let at = self.position_at(turn)[axis];
                min[axis] = min[axis].min(at);
                max[axis] = max[axis].max(at);
            }
        }
        Aabb::new(min, max)
    }

    /// This path as seen from a frame at `origin` turned by `turn`'s inverse:
    /// `turn` takes a world vector into that frame.
    pub(crate) fn in_frame(&self, origin: DVec3, turn: DQuat) -> Self {
        Self {
            start: turn * (self.start - origin),
            velocity: turn * self.velocity,
            acceleration: turn * self.acceleration,
            duration: self.duration,
        }
    }
}

/// Where
/// [`PhysicsWorld::sweep_capsule_arc`](crate::PhysicsWorld::sweep_capsule_arc)
/// found a capsule first touching a collider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcHit {
    /// When the capsule touches, in `[0, duration]` of the path, and never
    /// later than the first touch. The search stops once its next safe step
    /// is no longer than [`ARC_TIME_TOLERANCE`]: a flat face is met to
    /// rounding, and an edge or a round surface to within the distance the
    /// capsule closes in that time. Zero when it started touching.
    pub time: f64,
    /// The point of the collider nearest the capsule at [`time`](Self::time).
    pub point: DVec3,
    /// Unit normal pointing away from the surface met, toward the capsule.
    pub normal: DVec3,
    /// Whether the capsule began the path already touching or inside the
    /// collider: [`time`](Self::time) is then zero whichever way it moves,
    /// as a straight sweep's [`ShapeHit::started_inside`](crate::ShapeHit)
    /// is.
    pub started_inside: bool,
    /// The part of the collider met, as
    /// [`ShapeHit::part`](crate::ShapeHit::part) means it.
    pub part: usize,
}

/// A Y-aligned capsule of `radius` and `half_height` whose centre follows
/// `path` against a sphere.
pub(crate) fn arc_capsule_vs_sphere(
    path: &AcceleratedPath,
    radius: f64,
    half_height: f64,
    target: &Sphere,
) -> Option<ArcHit> {
    let shape = ContactShape::Sphere {
        centre: target.centre,
        radius: target.radius,
    };
    arc_capsule_vs_shape(path, radius, half_height, &shape, |_, _, normal| {
        target.centre + normal * target.radius
    })
}

/// [`arc_capsule_vs_sphere`] against a box collider, turned or not.
pub(crate) fn arc_capsule_vs_box(
    path: &AcceleratedPath,
    radius: f64,
    half_height: f64,
    target: &BoxCollider,
) -> Option<ArcHit> {
    arc_capsule_vs_shape(
        path,
        radius,
        half_height,
        &contact_box(target),
        |bottom, top, _| nearest_on_box(target, bottom, top),
    )
}

/// [`arc_capsule_vs_sphere`] against a capsule, standing or turned.
pub(crate) fn arc_capsule_vs_turned_capsule(
    path: &AcceleratedPath,
    radius: f64,
    half_height: f64,
    target: &TurnedCapsule,
) -> Option<ArcHit> {
    let (a, b) = target.core();
    arc_capsule_vs_shape(
        path,
        radius,
        half_height,
        &target.contact_shape(),
        |bottom, top, normal| {
            closest_between_segments(a, b, bottom, top).0 + normal * target.capsule.radius
        },
    )
}

/// A Y-aligned capsule following `path` against a convex `target`, measured
/// by the contact pipeline's [`gap`]; `nearest` finds the target's point
/// nearest the capsule's core, given the core's ends and the normal there.
fn arc_capsule_vs_shape(
    path: &AcceleratedPath,
    radius: f64,
    half_height: f64,
    target: &ContactShape,
    nearest: impl Fn(DVec3, DVec3, DVec3) -> DVec3,
) -> Option<ArcHit> {
    let half = DVec3::Y * half_height;
    let (time, normal, started_inside) = earliest_contact(
        path,
        half,
        radius,
        |direction| support(target, direction),
        |a, b| gap(target, &ContactShape::Capsule { a, b, radius }),
    )?;
    let centre = path.position_at(time);
    Some(ArcHit {
        time,
        point: nearest(centre - half, centre + half, normal),
        normal,
        started_inside,
        part: 0,
    })
}

/// The time a capsule of `radius` about the segment from `-half` to `+half`
/// round its centre, the centre following `path`, first touches a convex
/// shape; the normal out of the shape there; and whether it started touching.
/// `None` if it does not touch within the path's duration. See the module
/// docs for the method.
///
/// `separation` measures the shape against the capsule whose core runs
/// between two points: the signed gap, and the unit normal from the shape
/// toward the capsule. `support` is the shape's support function: the
/// furthest it reaches along a unit direction, `max n · x` over its points.
pub(crate) fn earliest_contact(
    path: &AcceleratedPath,
    half: DVec3,
    radius: f64,
    support: impl Fn(DVec3) -> f64,
    separation: impl Fn(DVec3, DVec3) -> (f64, DVec3),
) -> Option<(f64, DVec3, bool)> {
    let core = |time: f64| {
        let centre = path.position_at(time);
        (centre - half, centre + half)
    };
    let (a, b) = core(0.0);
    let (distance, mut normal) = separation(a, b);
    if distance <= 0.0 {
        return Some((0.0, normal, true));
    }
    let mut time = 0.0;
    for _ in 0..ARC_MAX_ITERATIONS {
        let (a, b) = core(time);
        // The capsule's height in front of the plane that holds the shape
        // behind it. A narrow phase whose normal is not the exact closest
        // direction can leave this at or below zero while the gap is not;
        // the capsule is then as near as the bound can tell, and is touching.
        let ahead = normal.dot(a).min(normal.dot(b)) - radius - support(normal);
        let step = if ahead > 0.0 {
            first_root(
                ahead,
                normal.dot(path.velocity_at(time)),
                normal.dot(path.acceleration),
            )?
        } else {
            0.0
        };
        time += step;
        if time > path.duration {
            return None;
        }
        let (a, b) = core(time);
        let (distance, next) = separation(a, b);
        normal = next;
        if distance <= 0.0 || step <= ARC_TIME_TOLERANCE {
            return Some((time, normal, false));
        }
    }
    Some((time, normal, false))
}

/// The least `s > 0` at which `ahead + rate s + ½ curve s²` reaches zero,
/// for an `ahead` above zero, or `None` if it never does.
///
/// Each branch is the root the quadratic formula loses no digits to: the
/// one whose numerator adds two terms of the same sign.
fn first_root(ahead: f64, rate: f64, curve: f64) -> Option<f64> {
    let discriminant = rate * rate - 2.0 * curve * ahead;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    if rate < 0.0 {
        Some(2.0 * ahead / (root - rate))
    } else if curve < 0.0 {
        Some((rate + root) / -curve)
    } else {
        None
    }
}

/// How far `shape` reaches along the unit `direction`: the most
/// `direction · x` over its points, its support function.
///
/// A half-space reaches a finite distance only along its own outward normal,
/// and infinitely far along every other direction.
pub(crate) fn support(shape: &ContactShape, direction: DVec3) -> f64 {
    match *shape {
        ContactShape::Sphere { centre, radius } => direction.dot(centre) + radius,
        ContactShape::Capsule { a, b, radius } => direction.dot(a).max(direction.dot(b)) + radius,
        ContactShape::Box {
            centre,
            rotation,
            half,
        } => direction.dot(centre) + (rotation.inverse() * direction).abs().dot(half),
        ContactShape::Triangle { corners, .. } => {
            corners.iter().fold(f64::NEG_INFINITY, |far, &corner| {
                far.max(direction.dot(corner))
            })
        }
        ContactShape::Plane { normal, offset } => {
            if direction == normal {
                offset
            } else {
                f64::INFINITY
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How closely a value worked out by hand has to agree with the code's:
    /// only rounding separates them.
    const EXACT: f64 = 1e-12;

    /// **A jump's apex is inside its bounds**, though both its ends are
    /// below it; an axis that does not turn within the interval is bounded
    /// by its ends alone.
    #[test]
    fn the_bounds_hold_the_turning_point_as_well_as_the_ends() {
        let gravity = -9.81;
        let rise = 3.5;
        let path = AcceleratedPath::new(
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(0.5, rise, 0.0),
            DVec3::new(0.0, gravity, 0.0),
            // The return to the starting height, a little past it.
            2.2 * rise / -gravity,
        );
        let bounds = path.bounds();
        let apex = 2.0 + rise * rise / (2.0 * -gravity);
        let end = path.position_at(path.duration);
        assert!(end.y < path.start.y, "the ends are below the apex");
        assert!(
            (bounds.max.y - apex).abs() < EXACT,
            "the bounds reach the apex at {apex}: {bounds:?}"
        );
        assert_eq!(bounds.min.y, end.y);
        assert_eq!((bounds.min.x, bounds.max.x), (path.start.x, end.x));
        assert_eq!((bounds.min.z, bounds.max.z), (3.0, 3.0));

        // Stopped before the apex, the path's top is its end.
        let rising = AcceleratedPath {
            duration: 0.5 * rise / -gravity,
            ..path
        };
        assert_eq!(rising.bounds().max.y, rising.position_at(rising.duration).y);
    }

    /// The root is where the quadratic is zero, the first of two, and none
    /// is found for a quadratic that turns back before reaching zero or
    /// never heads there.
    #[test]
    fn the_first_root_is_the_earlier_zero_or_none() {
        let value =
            |ahead: f64, rate: f64, curve: f64, s: f64| ahead + rate * s + 0.5 * curve * s * s;
        for (ahead, rate, curve) in [
            (1.0, -2.0, 0.0),
            (1.0, -2.0, 1.0),
            (1.0, -2.0, -3.0),
            (1.0, 0.0, -2.0),
            (1.0, 3.0, -2.0),
            (1e-12, -5.0, 9.81),
        ] {
            let s = first_root(ahead, rate, curve).expect("it reaches zero");
            assert!(s > 0.0);
            assert!(
                value(ahead, rate, curve, s).abs() < EXACT,
                "({ahead}, {rate}, {curve}) at {s}"
            );
            // Nothing earlier reaches zero.
            let earlier = s * 0.999;
            assert!(value(ahead, rate, curve, earlier) > 0.0);
        }
        assert_eq!(first_root(1.0, -1.0, 1.0), None, "it turns back first");
        assert_eq!(first_root(1.0, 0.0, 0.0), None, "it never moves");
        assert_eq!(first_root(1.0, 2.0, 1.0), None, "it heads away");
    }
}
