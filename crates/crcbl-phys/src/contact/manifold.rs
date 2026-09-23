//! Contact manifolds: the analytic pairs of rung 1 and the box pair of rung 2.
//!
//! `docs/plan/36-contact-solver.md` decision 2 makes sphere, capsule and their
//! pairs analytic, and boxes and hulls a cached separating axis test with
//! clipping:
//!
//! | A \ B   | sphere | capsule | box      | plane |
//! | ------- | ------ | ------- | -------- | ----- |
//! | sphere  | 1 pt   | 1 pt    | 1 pt     | 1 pt  |
//! | capsule |        | 1–2 pts | 1–2 pts  | 1–2   |
//! | box     |        |         | 1–4 pts  | 1–4   |
//!
//! **Box against box** is rung 2's, in `box_box`: the fifteen-axis separating
//! axis test with the pair's last axis cached in a [`SatCache`], the incident
//! face clipped to the reference face, and four points kept of up to eight.
//! Box against a plane is the box's corners below the plane, which needs no
//! clipping. Sphere and capsule against a box stay analytic rather than going
//! through GJK, which the plan names for spheres and capsules against _hulls_:
//! against a box the closest point is a clamp, exact and cheaper, and GJK
//! arrives with the general hull it is needed for. Sphere against sphere is
//! not in the plan's rung 1 row, which names only static boxes and planes; a
//! ball pit is balls against balls, so it is here.
//!
//! # Conventions
//!
//! Shape `A` is the one of lower [`ContactShape::rank`]. The normal points from
//! `A` into `B`, a separation is positive across a gap and negative in
//! overlap, and a point sits midway between the two surfaces. A point is kept
//! when its separation is within the speculative distance the caller passes,
//! so a contact exists a little before it touches.
//!
//! Every point carries a **feature id** that names the same geometric feature
//! from one tick to the next — an end of a capsule, a corner of a box — which
//! is what warm starting matches impulses by. Because `A` is always the lower
//! rank, a pair is never seen the other way round, so the ids need no flipping.
//!
//! All arithmetic is `+ − × ÷` and `sqrt`, so every target reaches the same
//! bits.

mod box_box;
mod gap;

use glam::{DQuat, DVec3};

pub use self::box_box::SatCache;
pub(crate) use self::gap::gap;
use super::shape::ContactShape;

/// The most points a manifold holds.
pub const MAX_POINTS: usize = 4;

/// Box2D's linear slop, in metres: the scale every tolerance of the box pair
/// and of the sweeps is set against, and a quarter of
/// [`crate::ContactSettings::DEFAULT`]'s speculative distance.
pub(crate) const LINEAR_SLOP: f64 = 0.005;

/// Golden-section iterations [`closest_on_segment_to_box`] takes. Fixed, so
/// every run does the same arithmetic; each shrinks the bracket by the golden
/// ratio, so this many leave it under 10⁻¹⁰ of the segment.
const GOLDEN_ITERATIONS: usize = 48;

/// How far from parallel two capsules' axes may be, as the sine of the angle
/// between them, and still rest on each other at two points rather than one.
const PARALLEL_SINE: f64 = 0.05;

/// One point of a [`Manifold`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ManifoldPoint {
    /// Where it is, in the world: midway between the two surfaces.
    pub point: DVec3,
    /// The distance between the surfaces along the normal: negative in overlap.
    pub separation: f64,
    /// The feature it belongs to, stable while the shapes stay in the same
    /// configuration.
    pub id: u32,
}

impl ManifoldPoint {
    const ZERO: Self = Self {
        point: DVec3::ZERO,
        separation: 0.0,
        id: 0,
    };
}

/// Where two shapes touch, or are about to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Manifold {
    /// The unit normal from shape `A` into shape `B`.
    pub normal: DVec3,
    points: [ManifoldPoint; MAX_POINTS],
    count: usize,
}

impl Manifold {
    /// No points: the shapes are further apart than the speculative distance,
    /// or the pair has no manifold at all.
    pub const EMPTY: Self = Self {
        normal: DVec3::ZERO,
        points: [ManifoldPoint::ZERO; MAX_POINTS],
        count: 0,
    };

    fn with_normal(normal: DVec3) -> Self {
        Self {
            normal,
            ..Self::EMPTY
        }
    }

    fn push(&mut self, point: DVec3, separation: f64, id: u32) {
        debug_assert!(self.count < MAX_POINTS, "a manifold holds four points");
        self.points[self.count] = ManifoldPoint {
            point,
            separation,
            id,
        };
        self.count += 1;
    }

    /// The points, in the order the pair function produced them.
    #[must_use]
    pub fn points(&self) -> &[ManifoldPoint] {
        &self.points[..self.count]
    }

    /// The points, mutably, for the pipeline's own bookkeeping.
    pub(crate) fn points_mut(&mut self) -> &mut [ManifoldPoint] {
        &mut self.points[..self.count]
    }
}

/// The manifold between `a` and `b`, keeping points within `speculative` of
/// touching.
///
/// `a` must not rank above `b`; see [`ContactShape::rank`]. Two planes have no
/// manifold and answer [`Manifold::EMPTY`]. A box pair runs its separating axis
/// test from nothing; [`collide_cached`] is the same with the pair's cached
/// axis, and answers the same.
#[must_use]
pub fn collide(a: &ContactShape, b: &ContactShape, speculative: f64) -> Manifold {
    collide_cached(a, b, speculative, &mut SatCache::default())
}

/// [`collide`], trying a box pair's last separating axis first and leaving its
/// new one in `cache`. Every other pair ignores the cache.
#[must_use]
pub fn collide_cached(
    a: &ContactShape,
    b: &ContactShape,
    speculative: f64,
    cache: &mut SatCache,
) -> Manifold {
    debug_assert!(a.rank() <= b.rank(), "shape A ranks above shape B");
    use ContactShape::{Box, Capsule, Plane, Sphere};
    match (*a, *b) {
        (
            Sphere {
                centre: ca,
                radius: ra,
            },
            Sphere {
                centre: cb,
                radius: rb,
            },
        ) => spheres(ca, ra, cb, rb, speculative, 0),
        (
            Sphere { centre, radius },
            Capsule {
                a: sa,
                b: sb,
                radius: rc,
            },
        ) => {
            let (_, q) = closest_on_segment(centre, sa, sb);
            spheres(centre, radius, q, rc, speculative, 0)
        }
        (
            Sphere { centre, radius },
            Box {
                centre: bc,
                rotation,
                half,
            },
        ) => sphere_box(centre, radius, bc, rotation, half, speculative),
        (Sphere { centre, radius }, Plane { normal, offset }) => {
            let delta = normal.dot(centre) - offset;
            let mut manifold = Manifold::with_normal(-normal);
            if delta - radius <= speculative {
                manifold.push(
                    centre - normal * (0.5 * (radius + delta)),
                    delta - radius,
                    0,
                );
            }
            manifold
        }
        (
            Capsule {
                a: a1,
                b: b1,
                radius: r1,
            },
            Capsule {
                a: a2,
                b: b2,
                radius: r2,
            },
        ) => capsules(a1, b1, r1, a2, b2, r2, speculative),
        (
            Capsule {
                a: sa,
                b: sb,
                radius,
            },
            Box {
                centre,
                rotation,
                half,
            },
        ) => capsule_box(sa, sb, radius, centre, rotation, half, speculative),
        (
            Capsule {
                a: sa,
                b: sb,
                radius,
            },
            Plane { normal, offset },
        ) => {
            let mut manifold = Manifold::with_normal(-normal);
            let ends: &[DVec3] = if sa == sb { &[sa] } else { &[sa, sb] };
            for (id, &end) in (0u32..).zip(ends) {
                let delta = normal.dot(end) - offset;
                if delta - radius <= speculative {
                    manifold.push(end - normal * (0.5 * (radius + delta)), delta - radius, id);
                }
            }
            manifold
        }
        (
            Box {
                centre,
                rotation,
                half,
            },
            Plane { normal, offset },
        ) => box_plane(centre, rotation, half, normal, offset, speculative),
        (
            Box {
                centre: ca,
                rotation: ra,
                half: ha,
            },
            Box {
                centre: cb,
                rotation: rb,
                half: hb,
            },
        ) => box_box::boxes(ca, ra, ha, cb, rb, hb, speculative, cache),
        _ => Manifold::EMPTY,
    }
}

/// Two spheres, or a sphere and the nearest sphere of a capsule's core.
fn spheres(ca: DVec3, ra: f64, cb: DVec3, rb: f64, speculative: f64, id: u32) -> Manifold {
    let d = cb - ca;
    let reach = ra + rb + speculative;
    let distance_squared = d.length_squared();
    if distance_squared > reach * reach {
        return Manifold::EMPTY;
    }
    let distance = distance_squared.sqrt();
    // Concentric spheres have no direction between them; any fixed one keeps
    // the answer the same on every run.
    let normal = if distance > 0.0 {
        d / distance
    } else {
        DVec3::Y
    };
    let mut manifold = Manifold::with_normal(normal);
    let on_a = ca + normal * ra;
    let on_b = cb - normal * rb;
    manifold.push((on_a + on_b) * 0.5, distance - ra - rb, id);
    manifold
}

/// A sphere against an oriented box, worked in the box's frame.
fn sphere_box(
    centre: DVec3,
    radius: f64,
    box_centre: DVec3,
    rotation: DQuat,
    half: DVec3,
    speculative: f64,
) -> Manifold {
    let local = rotation.inverse() * (centre - box_centre);
    let clamped = local.clamp(-half, half);
    let (outward, separation, on_box) = if local == clamped {
        // Inside: out through the nearest face.
        let depth = half - local.abs();
        let axis = if depth.x <= depth.y && depth.x <= depth.z {
            0
        } else if depth.y <= depth.z {
            1
        } else {
            2
        };
        let sign = if local[axis] < 0.0 { -1.0 } else { 1.0 };
        let mut outward = DVec3::ZERO;
        outward[axis] = sign;
        let mut on_box = local;
        on_box[axis] = sign * half[axis];
        (outward, -depth[axis] - radius, on_box)
    } else {
        let diff = local - clamped;
        let distance_squared = diff.length_squared();
        let reach = radius + speculative;
        if distance_squared > reach * reach {
            return Manifold::EMPTY;
        }
        let distance = distance_squared.sqrt();
        (diff / distance, distance - radius, clamped)
    };
    let on_sphere = local - outward * radius;
    let mut manifold = Manifold::with_normal(-(rotation * outward));
    if separation <= speculative {
        manifold.push(
            box_centre + rotation * ((on_box + on_sphere) * 0.5),
            separation,
            box_box::feature_near(local, half),
        );
    }
    manifold
}

/// Two capsules: two points where they lie along each other, one elsewhere.
#[allow(clippy::too_many_arguments)]
fn capsules(
    a1: DVec3,
    b1: DVec3,
    r1: f64,
    a2: DVec3,
    b2: DVec3,
    r2: f64,
    speculative: f64,
) -> Manifold {
    let (c1, c2) = closest_between_segments(a1, b1, a2, b2);
    let d = c2 - c1;
    let reach = r1 + r2 + speculative;
    let distance_squared = d.length_squared();
    if distance_squared > reach * reach {
        return Manifold::EMPTY;
    }
    let u1 = b1 - a1;
    let u2 = b2 - a2;
    let distance = distance_squared.sqrt();
    let normal = if distance > 0.0 {
        d / distance
    } else {
        // Crossing axes: across both, or any way across a lone one.
        let across = u1.cross(u2);
        if across != DVec3::ZERO {
            across.normalize()
        } else if u1 != DVec3::ZERO {
            orthonormal_basis(u1.normalize()).0
        } else {
            DVec3::Y
        }
    };
    let mut manifold = Manifold::with_normal(normal);

    let (l1, l2) = (u1.length_squared(), u2.length_squared());
    let parallel = l1 > 0.0
        && l2 > 0.0
        && u1.cross(u2).length_squared() <= PARALLEL_SINE * PARALLEL_SINE * l1 * l2;
    if parallel {
        // Capsule 2's ends, projected onto capsule 1's axis, bound the stretch
        // the two lie along each other over.
        let t = |p: DVec3| ((p - a1).dot(u1) / l1).clamp(0.0, 1.0);
        let (ta, tb) = (t(a2), t(b2));
        let (lo, hi) = (ta.min(tb), ta.max(tb));
        if hi > lo {
            for (id, param) in [(0u32, lo), (1, hi)] {
                let p = a1 + u1 * param;
                let (_, q) = closest_on_segment(p, a2, b2);
                let gap = (q - p).dot(normal);
                let separation = gap - r1 - r2;
                if separation <= speculative {
                    manifold.push(p + normal * (0.5 * (r1 + gap - r2)), separation, id);
                }
            }
            if manifold.count > 0 {
                return manifold;
            }
        }
    }
    manifold.push(
        c1 + normal * (0.5 * (r1 + distance - r2)),
        distance - r1 - r2,
        2,
    );
    manifold
}

/// A capsule against an oriented box, worked in the box's frame: two points
/// where it lies along a face, one where it meets an edge or a corner.
#[allow(clippy::too_many_arguments)]
fn capsule_box(
    a: DVec3,
    b: DVec3,
    radius: f64,
    box_centre: DVec3,
    rotation: DQuat,
    half: DVec3,
    speculative: f64,
) -> Manifold {
    let inverse = rotation.inverse();
    let p0 = inverse * (a - box_centre);
    let p1 = inverse * (b - box_centre);
    let along = p1 - p0;

    let t = closest_on_segment_to_box(p0, along, half);
    let p = p0 + along * t;
    let q = p.clamp(-half, half);
    let distance_squared = (p - q).length_squared();
    let reach = radius + speculative;
    if distance_squared > reach * reach {
        return Manifold::EMPTY;
    }
    let distance = distance_squared.sqrt();

    // The face the capsule is against, if it is against a face: the one axis
    // its nearest point lies outside; or, where it lies outside two, the face
    // of the two the segment lies along and is furthest out from — a segment
    // across a face and past its end is nearest just past the end, over an
    // edge, and still rests on the face; or, in overlap, the face whose
    // outward direction the whole segment is least deep along.
    let face = if distance > 0.0 {
        let side = |k: usize| (k, if p[k] < 0.0 { -1.0 } else { 1.0 });
        let mut outside = (0..3).filter(|&k| p[k].abs() > half[k]);
        match (outside.next(), outside.next()) {
            (Some(k), None) => Some(side(k)),
            (Some(i), Some(j)) => {
                let lies_along = |k: usize| {
                    along[k] * along[k] <= PARALLEL_SINE * PARALLEL_SINE * along.length_squared()
                };
                let excess = |k: usize| p[k].abs() - half[k];
                let (first, second) = if excess(j) > excess(i) {
                    (j, i)
                } else {
                    (i, j)
                };
                [first, second]
                    .into_iter()
                    .find(|&k| lies_along(k))
                    .map(side)
            }
            _ => None,
        }
    } else {
        let mut best = (0, 1.0);
        let mut best_separation = f64::NEG_INFINITY;
        for k in 0..3 {
            for sign in [1.0, -1.0] {
                let separation = (sign * p0[k]).min(sign * p1[k]) - half[k];
                if separation > best_separation {
                    best_separation = separation;
                    best = (k, sign);
                }
            }
        }
        Some(best)
    };

    if let Some((k, sign)) = face {
        let mut outward = DVec3::ZERO;
        outward[k] = sign;
        let mut manifold = Manifold::with_normal(-(rotation * outward));
        if let Some((lo, hi)) = clip_to_face(p0, along, half, k) {
            let params: &[(u32, f64)] = if hi > lo {
                &[(0, lo), (1, hi)]
            } else {
                &[(0, lo)]
            };
            for &(id, param) in params {
                let point = p0 + along * param;
                let delta = sign * point[k] - half[k];
                let separation = delta - radius;
                if separation <= speculative {
                    manifold.push(
                        box_centre + rotation * (point - outward * (0.5 * (radius + delta))),
                        separation,
                        (box_box::face_feature(k, sign > 0.0) << 8) | id,
                    );
                }
            }
        }
        if manifold.count > 0 {
            return manifold;
        }
    }

    // An edge or a corner, or a face the segment does not reach over.
    if distance == 0.0 {
        // In overlap with no face to push out through: out along the axis the
        // nearest point is shallowest on.
        let depth = half - p.abs();
        let k = if depth.x <= depth.y && depth.x <= depth.z {
            0
        } else if depth.y <= depth.z {
            1
        } else {
            2
        };
        let sign = if p[k] < 0.0 { -1.0 } else { 1.0 };
        let mut outward = DVec3::ZERO;
        outward[k] = sign;
        let mut manifold = Manifold::with_normal(-(rotation * outward));
        let delta = -depth[k];
        manifold.push(
            box_centre + rotation * (p - outward * (0.5 * (radius + delta))),
            delta - radius,
            (box_box::feature_near(p, half) << 8) | 2,
        );
        return manifold;
    }
    let outward = (p - q) / distance;
    let mut manifold = Manifold::with_normal(-(rotation * outward));

    // Against an edge the segment lies along, the nearest point is anywhere
    // on the stretch they share, so a single point would wander along it from
    // tick to tick: rest on both ends of that stretch instead, as along a face.
    // Where along the edge the nearest point lands is a tie, and may land just
    // past the edge's end, so the edge is the one the segment is parallel to
    // with the nearest point outside on both the other axes.
    let edge = (0..3).find(|&k| {
        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
        p[i].abs() > half[i]
            && p[j].abs() > half[j]
            && along.length_squared() - along[k] * along[k]
                <= PARALLEL_SINE * PARALLEL_SINE * along.length_squared()
    });
    if let Some(k) = edge
        && let Some((lo, hi)) = clip_to_edge(p0, along, half, k)
        && hi > lo
    {
        for (id, param) in [(0u32, lo), (1, hi)] {
            let point = p0 + along * param;
            let on_edge = point.clamp(-half, half);
            let gap = (point - on_edge).dot(outward);
            let separation = gap - radius;
            if separation <= speculative {
                manifold.push(
                    box_centre + rotation * (point - outward * (0.5 * (radius + gap))),
                    separation,
                    (box_box::feature_near(p, half) << 8) | id,
                );
            }
        }
        if manifold.count > 0 {
            return manifold;
        }
    }

    manifold.push(
        box_centre + rotation * (p - outward * (0.5 * (radius + distance))),
        distance - radius,
        (box_box::feature_near(p, half) << 8) | 2,
    );
    manifold
}

/// The stretch of the segment `p0 + along · t`, `t ∈ [0, 1]`, that lies
/// alongside the box's edges on axis `k`: inside the box's extent on that
/// axis.
fn clip_to_edge(p0: DVec3, along: DVec3, half: DVec3, k: usize) -> Option<(f64, f64)> {
    if along[k] == 0.0 {
        return (p0[k].abs() <= half[k]).then_some((0.0, 1.0));
    }
    let t0 = (-half[k] - p0[k]) / along[k];
    let t1 = (half[k] - p0[k]) / along[k];
    let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
    (lo <= hi).then_some((lo, hi))
}

/// The stretch of the segment `p0 + along · t`, `t ∈ [0, 1]`, that lies over
/// the box's face on axis `k`: inside the box's extent on the other two axes.
fn clip_to_face(p0: DVec3, along: DVec3, half: DVec3, k: usize) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for j in (0..3).filter(|&j| j != k) {
        if along[j] == 0.0 {
            if p0[j].abs() > half[j] {
                return None;
            }
            continue;
        }
        let t0 = (-half[j] - p0[j]) / along[j];
        let t1 = (half[j] - p0[j]) / along[j];
        lo = lo.max(t0.min(t1));
        hi = hi.min(t0.max(t1));
    }
    (lo <= hi).then_some((lo, hi))
}

/// A box's corners within `speculative` of a plane, the deepest four if more.
fn box_plane(
    centre: DVec3,
    rotation: DQuat,
    half: DVec3,
    normal: DVec3,
    offset: f64,
    speculative: f64,
) -> Manifold {
    let mut kept: [(f64, u32, DVec3); 8] = [(0.0, 0, DVec3::ZERO); 8];
    let mut count = 0;
    for id in 0..8u32 {
        let sign = |bit: u32| if id & (1 << bit) == 0 { -1.0 } else { 1.0 };
        let corner = centre + rotation * (half * DVec3::new(sign(0), sign(1), sign(2)));
        let delta = normal.dot(corner) - offset;
        if delta <= speculative {
            kept[count] = (delta, id, corner);
            count += 1;
        }
    }
    // Deepest first, and by corner on a tie, so the four kept are the same
    // four on every run.
    kept[..count].sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    let mut manifold = Manifold::with_normal(-normal);
    for &(delta, id, corner) in &kept[..count.min(MAX_POINTS)] {
        manifold.push(corner - normal * (0.5 * delta), delta, id);
    }
    manifold
}

/// The point of the segment `a`–`b` nearest `p`, and its parameter.
#[must_use]
pub fn closest_on_segment(p: DVec3, a: DVec3, b: DVec3) -> (f64, DVec3) {
    let ab = b - a;
    let length_squared = ab.length_squared();
    if length_squared == 0.0 {
        return (0.0, a);
    }
    let t = ((p - a).dot(ab) / length_squared).clamp(0.0, 1.0);
    (t, a + ab * t)
}

/// The nearest points of two segments — Ericson, *Real-Time Collision
/// Detection*, §5.1.9.
fn closest_between_segments(p1: DVec3, q1: DVec3, p2: DVec3, q2: DVec3) -> (DVec3, DVec3) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);
    let (s, t) = if a == 0.0 && e == 0.0 {
        (0.0, 0.0)
    } else if a == 0.0 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e == 0.0 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;
            let s = if denominator > 0.0 {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

/// The parameter along `p0 + along · t`, `t ∈ [0, 1]`, nearest a box of
/// half-extents `half` at the origin.
///
/// The squared distance from a segment to a convex set is convex along it, so
/// a golden-section search finds its minimum; the two ends are compared as
/// well, because the search's last bracket never quite reaches them.
fn closest_on_segment_to_box(p0: DVec3, along: DVec3, half: DVec3) -> f64 {
    let distance = |t: f64| {
        let p = p0 + along * t;
        (p - p.clamp(-half, half)).length_squared()
    };
    // 1/φ, the fraction of the bracket each iteration keeps.
    const INVERSE_PHI: f64 = 0.618_033_988_749_894_8;
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let mut x1 = hi - INVERSE_PHI * (hi - lo);
    let mut x2 = lo + INVERSE_PHI * (hi - lo);
    let (mut f1, mut f2) = (distance(x1), distance(x2));
    for _ in 0..GOLDEN_ITERATIONS {
        if f1 <= f2 {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - INVERSE_PHI * (hi - lo);
            f1 = distance(x1);
        } else {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + INVERSE_PHI * (hi - lo);
            f2 = distance(x2);
        }
    }
    let middle = 0.5 * (lo + hi);
    let mut best = (distance(middle), middle);
    for end in [0.0, 1.0] {
        let d = distance(end);
        if d < best.0 {
            best = (d, end);
        }
    }
    best.1
}

/// Two unit vectors that make a right-handed orthonormal basis with the unit
/// vector `n` — Duff et al., "Building an Orthonormal Basis, Revisited", 2017.
#[must_use]
pub fn orthonormal_basis(n: DVec3) -> (DVec3, DVec3) {
    let sign = if n.z >= 0.0 { 1.0 } else { -1.0 };
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    (
        DVec3::new(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x),
        DVec3::new(b, sign + n.y * n.y * a, -n.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: f64 = 0.02;

    fn sphere(centre: DVec3, radius: f64) -> ContactShape {
        ContactShape::Sphere { centre, radius }
    }

    fn floor() -> ContactShape {
        ContactShape::Plane {
            normal: DVec3::Y,
            offset: 0.0,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A sphere resting a centimetre into a floor: one point, a centimetre
    /// deep, midway between the surfaces, pushing down into the floor.
    #[test]
    fn a_sphere_on_a_plane_is_one_point_at_its_depth() {
        let m = collide(&sphere(DVec3::new(2.0, 0.49, 0.0), 0.5), &floor(), SPEC);
        assert_eq!(m.points().len(), 1);
        assert!(close(m.points()[0].separation, -0.01), "{m:?}");
        assert_eq!(m.normal, -DVec3::Y);
        assert!((m.points()[0].point - DVec3::new(2.0, -0.005, 0.0)).length() < 1e-12);
        // And beyond the speculative distance, nothing.
        let far = collide(&sphere(DVec3::new(0.0, 0.6, 0.0), 0.5), &floor(), SPEC);
        assert!(far.points().is_empty(), "{far:?}");
    }

    /// Two spheres' normal runs from A to B, and a sphere against a sphere of
    /// the same size overlapping by a centimetre is a centimetre deep.
    #[test]
    fn two_spheres_touch_along_the_line_between_them() {
        let m = collide(
            &sphere(DVec3::ZERO, 0.5),
            &sphere(DVec3::new(0.0, 0.0, 0.99), 0.5),
            SPEC,
        );
        assert_eq!(m.normal, DVec3::Z);
        assert!(close(m.points()[0].separation, -0.01), "{m:?}");
    }

    /// A capsule lying on a floor, on a box's top face and along another
    /// capsule rests on both its ends, with the two ends' ids.
    #[test]
    fn a_capsule_lying_along_a_surface_rests_on_two_points() {
        let lying = ContactShape::Capsule {
            a: DVec3::new(-0.5, 0.2, 0.0),
            b: DVec3::new(0.5, 0.2, 0.0),
            radius: 0.2,
        };
        let on_floor = collide(&lying, &floor(), SPEC);
        assert_eq!(on_floor.points().len(), 2, "{on_floor:?}");
        assert_eq!(
            on_floor.points().iter().map(|p| p.id).collect::<Vec<_>>(),
            [0, 1]
        );

        let slab = ContactShape::Box {
            centre: DVec3::new(0.0, -0.5, 0.0),
            rotation: DQuat::IDENTITY,
            half: DVec3::new(2.0, 0.5, 2.0),
        };
        let on_box = collide(&lying, &slab, SPEC);
        assert_eq!(on_box.points().len(), 2, "{on_box:?}");
        assert!((on_box.normal + DVec3::Y).length() < 1e-12, "{on_box:?}");
        for point in on_box.points() {
            assert!(close(point.separation, 0.0), "{on_box:?}");
        }

        let under = ContactShape::Capsule {
            a: DVec3::new(-0.3, -0.19, 0.0),
            b: DVec3::new(0.8, -0.19, 0.0),
            radius: 0.2,
        };
        let along = collide(&lying, &under, SPEC);
        assert_eq!(along.points().len(), 2, "{along:?}");
        assert!((along.normal + DVec3::Y).length() < 1e-9, "{along:?}");
        assert!(close(along.points()[0].separation, -0.01), "{along:?}");
    }

    /// A capsule crossing a box's edge square on meets it at one point: its
    /// core passes 0.1 out from the edge both ways, at the crossing.
    ///
    /// Rung 1's version of this test laid the capsule _along_ the edge and
    /// asserted one point, which was the wandering point
    /// `a_capsule_lying_along_a_box_edge_rests_on_two_points` replaces.
    #[test]
    fn a_capsule_crossing_a_box_edge_meets_it_at_one_point() {
        let slab = ContactShape::Box {
            centre: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
            half: DVec3::splat(0.5),
        };
        let crossing = ContactShape::Capsule {
            a: DVec3::new(-0.4, 1.6, 0.2),
            b: DVec3::new(1.6, -0.4, 0.2),
            radius: 0.15,
        };
        let m = collide(&crossing, &slab, SPEC);
        assert_eq!(m.points().len(), 1, "{m:?}");
        let expected = (0.1f64 * 0.1 + 0.1 * 0.1).sqrt() - 0.15;
        assert!(close(m.points()[0].separation, expected), "{m:?}");
        // Against the box's edge along Z at +X, +Y: edge 2 · 4 + 1 + 2 = 11,
        // whose code is 9 + 11.
        let id = m.points()[0].id;
        assert_eq!(id & 0xff, 2, "{m:?}");
        assert_eq!(id >> 8, 20, "{m:?}");
    }

    /// **A capsule lying along a box's edge rests on two points**, one at each
    /// end of the stretch it lies along the edge over, not on one point whose
    /// place along the edge is anyone's guess. A capsule of radius 0.15 along
    /// the top +Z edge of a unit box, its core 0.1 out from the edge both
    /// ways, is `√0.02 − 0.15` from it at both ends of its core.
    #[test]
    fn a_capsule_lying_along_a_box_edge_rests_on_two_points() {
        let slab = ContactShape::Box {
            centre: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
            half: DVec3::splat(0.5),
        };
        let along = ContactShape::Capsule {
            a: DVec3::new(-0.4, 0.6, 0.6),
            b: DVec3::new(0.3, 0.6, 0.6),
            radius: 0.15,
        };
        let m = collide(&along, &slab, SPEC);
        assert_eq!(m.points().len(), 2, "{m:?}");
        let expected = 0.02f64.sqrt() - 0.15;
        let diagonal = DVec3::new(0.0, -1.0, -1.0).normalize();
        assert!((m.normal - diagonal).length() < 1e-9, "{m:?}");
        let mut xs: Vec<f64> = m.points().iter().map(|p| p.point.x).collect();
        xs.sort_by(f64::total_cmp);
        assert!(close(xs[0], -0.4) && close(xs[1], 0.3), "{m:?}");
        for point in m.points() {
            assert!(close(point.separation, expected), "{m:?}");
        }
        // And over the end of the edge, only the stretch over the box counts.
        let over = ContactShape::Capsule {
            a: DVec3::new(0.2, 0.6, 0.6),
            b: DVec3::new(0.9, 0.6, 0.6),
            radius: 0.15,
        };
        let m = collide(&over, &slab, SPEC);
        let mut xs: Vec<f64> = m.points().iter().map(|p| p.point.x).collect();
        xs.sort_by(f64::total_cmp);
        assert_eq!(xs.len(), 2, "{m:?}");
        assert!(close(xs[0], 0.2) && close(xs[1], 0.5), "{m:?}");
    }

    /// **A capsule lying across a face and past its ends, tilted a little
    /// towards it, rests on the face at both its ends** — not on one point at
    /// the end it is nearer, though its nearest point lies just past that end,
    /// over an edge. A long capsule over the +X face of a unit box, its core
    /// 0.1075 out at the face's lower end and 0.1125 at its upper, of radius
    /// 0.15, touches at both, 4.25 cm and 3.75 cm deep.
    #[test]
    fn a_capsule_across_a_face_and_past_it_rests_on_the_face() {
        let slab = ContactShape::Box {
            centre: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
            half: DVec3::splat(0.5),
        };
        let across = ContactShape::Capsule {
            a: DVec3::new(0.6, -2.0, 0.0),
            b: DVec3::new(0.62, 2.0, 0.0),
            radius: 0.15,
        };
        let m = collide(&across, &slab, SPEC);
        assert_eq!(m.points().len(), 2, "{m:?}");
        assert_eq!(m.normal, -DVec3::X, "{m:?}");
        let mut points: Vec<(f64, f64)> = m
            .points()
            .iter()
            .map(|p| (p.point.y, p.separation))
            .collect();
        points.sort_by(|p, q| p.0.total_cmp(&q.0));
        assert!(close(points[0].0, -0.5) && close(points[1].0, 0.5), "{m:?}");
        assert!(close(points[0].1, -0.0425), "{m:?}");
        assert!(close(points[1].1, -0.0375), "{m:?}");
    }

    /// A sphere sunk into a turned box is pushed out through the nearest face
    /// of the box as turned, not of its bounds.
    #[test]
    fn a_sphere_inside_a_turned_box_leaves_through_the_nearest_turned_face() {
        let turn = crate::rotation_from_scaled_axis(DVec3::new(0.0, 0.0, 0.4));
        let slab = ContactShape::Box {
            centre: DVec3::ZERO,
            rotation: turn,
            half: DVec3::new(2.0, 0.5, 2.0),
        };
        let m = collide(&sphere(turn * DVec3::new(0.0, 0.4, 0.0), 0.2), &slab, SPEC);
        assert_eq!(m.points().len(), 1);
        assert!((m.normal + turn * DVec3::Y).length() < 1e-12, "{m:?}");
        assert!(close(m.points()[0].separation, -0.3), "{m:?}");
    }

    /// A box set flat on a floor rests on its four lower corners, and a box
    /// tilted onto an edge on the two corners of that edge.
    #[test]
    fn a_box_on_a_plane_rests_on_the_corners_below_it() {
        let flat = ContactShape::Box {
            centre: DVec3::new(0.0, 0.3, 0.0),
            rotation: DQuat::IDENTITY,
            half: DVec3::splat(0.3),
        };
        let m = collide(&flat, &floor(), SPEC);
        assert_eq!(m.points().len(), 4, "{m:?}");
        let mut ids: Vec<u32> = m.points().iter().map(|p| p.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, [0, 1, 4, 5], "the corners with -y");

        let tilt = crate::rotation_from_scaled_axis(DVec3::new(0.0, 0.0, 0.3));
        let tilted = ContactShape::Box {
            centre: DVec3::new(0.0, 0.4, 0.0),
            rotation: tilt,
            half: DVec3::splat(0.3),
        };
        let low = collide(&tilted, &floor(), 0.1);
        assert_eq!(low.points().len(), 2, "{low:?}");
    }

    /// The basis is orthonormal whichever way the normal points.
    #[test]
    fn the_basis_is_orthonormal() {
        for n in [
            DVec3::Y,
            -DVec3::Z,
            DVec3::new(0.3, -0.4, 0.5).normalize(),
            DVec3::new(0.0, 0.0, -1.0),
        ] {
            let (t1, t2) = orthonormal_basis(n);
            assert!(t1.dot(n).abs() < 1e-12 && t2.dot(n).abs() < 1e-12);
            assert!(t1.dot(t2).abs() < 1e-12);
            assert!((t1.length() - 1.0).abs() < 1e-12 && (t2.length() - 1.0).abs() < 1e-12);
            assert!((t1.cross(t2) - n).length() < 1e-12, "{n:?}");
        }
    }
}
