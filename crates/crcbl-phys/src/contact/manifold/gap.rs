//! How far apart two shapes are: what rung 4's sweeps advance by.
//!
//! A manifold answers where two shapes touch; a sweep asks something smaller
//! and more often — how far apart they are, and in which direction — at each
//! step of conservative advancement. [`gap`] answers it with the signed
//! distance between the shapes where that is exact and cheap, and with a
//! lower bound on it where it is not:
//!
//! | pair                        | answer                                        |
//! | --------------------------- | --------------------------------------------- |
//! | sphere or capsule, anything | the core's distance, less the radii: exact    |
//! | box and plane               | the lowest corner's height: exact             |
//! | box and box                 | the best of the fifteen axes: a lower bound   |
//!
//! A lower bound is what makes advancement safe: stepping by less than the
//! true distance never steps past a contact. In overlap the answer is minus a
//! depth — exact for spheres, a box against a plane and two boxes; for a
//! capsule whose core has entered a box, the depth of the core's point that
//! the search for the nearest landed on.

use glam::{DQuat, DVec3};

use super::{box_box, closest_between_segments, closest_on_segment, closest_on_segment_to_box};
use crate::contact::shape::ContactShape;

/// The signed distance between `a` and `b` — positive apart, negative in
/// overlap — or a lower bound on it, and the unit normal from `a` towards
/// `b` along which it was measured. Either may rank above the other.
///
/// Two planes are never apart by any finite amount that means anything, and
/// answer an infinite gap.
pub(crate) fn gap(a: &ContactShape, b: &ContactShape) -> (f64, DVec3) {
    if a.rank() > b.rank() {
        let (distance, normal) = ordered(b, a);
        return (distance, -normal);
    }
    ordered(a, b)
}

/// [`gap`] for a pair whose `a` does not rank above `b`.
fn ordered(a: &ContactShape, b: &ContactShape) -> (f64, DVec3) {
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
        ) => points(ca, cb, ra + rb),
        (
            Sphere { centre, radius },
            Capsule {
                a: sa,
                b: sb,
                radius: rc,
            },
        ) => points(centre, closest_on_segment(centre, sa, sb).1, radius + rc),
        (
            Sphere { centre, radius },
            Box {
                centre: bc,
                rotation,
                half,
            },
        ) => {
            let (distance, outward) = point_box(rotation.inverse() * (centre - bc), half);
            (distance - radius, -(rotation * outward))
        }
        (Sphere { centre, radius }, Plane { normal, offset }) => {
            (normal.dot(centre) - offset - radius, -normal)
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
        ) => {
            let (c1, c2) = closest_between_segments(a1, b1, a2, b2);
            points(c1, c2, r1 + r2)
        }
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
        ) => {
            let inverse = rotation.inverse();
            let p0 = inverse * (sa - centre);
            let along = inverse * (sb - centre) - p0;
            let t = closest_on_segment_to_box(p0, along, half);
            let (distance, outward) = point_box(p0 + along * t, half);
            (distance - radius, -(rotation * outward))
        }
        (
            Capsule {
                a: sa,
                b: sb,
                radius,
            },
            Plane { normal, offset },
        ) => (
            normal.dot(sa).min(normal.dot(sb)) - offset - radius,
            -normal,
        ),
        (
            Box {
                centre,
                rotation,
                half,
            },
            Plane { normal, offset },
        ) => (
            lowest_corner(centre, rotation, half, normal) - offset,
            -normal,
        ),
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
        ) => box_box::gap(ca, ra, ha, cb, rb, hb),
        _ => (f64::INFINITY, DVec3::Y),
    }
}

/// Two points `radius` fattened: their distance less `radius`, and the
/// direction from the first to the second — any fixed one if they coincide.
fn points(a: DVec3, b: DVec3, radius: f64) -> (f64, DVec3) {
    let d = b - a;
    let distance = d.length();
    let normal = if distance > 0.0 {
        d / distance
    } else {
        DVec3::Y
    };
    (distance - radius, normal)
}

/// The signed distance from `local`, in a box's frame, to a box of
/// half-extents `half` at the origin, and the box's outward normal there:
/// out through the nearest face from inside.
fn point_box(local: DVec3, half: DVec3) -> (f64, DVec3) {
    let clamped = local.clamp(-half, half);
    if local != clamped {
        let diff = local - clamped;
        let distance = diff.length();
        return (distance, diff / distance);
    }
    let depth = half - local.abs();
    let axis = if depth.x <= depth.y && depth.x <= depth.z {
        0
    } else if depth.y <= depth.z {
        1
    } else {
        2
    };
    let mut outward = DVec3::ZERO;
    outward[axis] = if local[axis] < 0.0 { -1.0 } else { 1.0 };
    (-depth[axis], outward)
}

/// The least `normal · corner` over a box's corners.
fn lowest_corner(centre: DVec3, rotation: DQuat, half: DVec3, normal: DVec3) -> f64 {
    let local = rotation.inverse() * normal;
    normal.dot(centre) - local.abs().dot(half)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn unit_box(centre: DVec3, rotation: DQuat) -> ContactShape {
        ContactShape::Box {
            centre,
            rotation,
            half: DVec3::splat(0.5),
        }
    }

    /// Every pair with a round side answers the exact distance, apart and in
    /// overlap, with its normal pointing from the first shape to the second
    /// whichever way round they are asked.
    #[test]
    fn a_round_pair_answers_its_exact_distance_either_way_round() {
        let ball = ContactShape::Sphere {
            centre: DVec3::new(0.0, 2.0, 0.0),
            radius: 0.5,
        };
        let floor = ContactShape::Plane {
            normal: DVec3::Y,
            offset: 0.0,
        };
        let (d, n) = gap(&ball, &floor);
        assert!(close(d, 1.5) && n == -DVec3::Y, "{d} {n:?}");
        let (d, n) = gap(&floor, &ball);
        assert!(close(d, 1.5) && n == DVec3::Y, "{d} {n:?}");

        let cube = unit_box(DVec3::new(3.0, 2.0, 0.0), DQuat::IDENTITY);
        let (d, n) = gap(&ball, &cube);
        assert!(close(d, 2.0) && n == DVec3::X, "{d} {n:?}");
        let inside = ContactShape::Sphere {
            centre: DVec3::new(2.7, 2.0, 0.0),
            radius: 0.1,
        };
        let (d, _) = gap(&inside, &cube);
        assert!(close(d, -0.3), "a ball 0.2 into a face and 0.1 round: {d}");

        let pill = ContactShape::Capsule {
            a: DVec3::new(-1.0, 0.3, 0.0),
            b: DVec3::new(1.0, 0.3, 0.0),
            radius: 0.1,
        };
        let (d, n) = gap(&pill, &floor);
        assert!(close(d, 0.2) && n == -DVec3::Y, "{d} {n:?}");
        let (d, _) = gap(&pill, &unit_box(DVec3::new(0.0, 1.0, 0.0), DQuat::IDENTITY));
        assert!(close(d, 0.1), "{d}");
    }

    /// Two boxes corner to corner are further apart than any one axis says,
    /// and the answer is that axis's: a lower bound, never more than the
    /// distance. Face to face, it is the distance.
    #[test]
    fn two_boxes_answer_a_lower_bound_that_is_exact_face_to_face() {
        let a = unit_box(DVec3::ZERO, DQuat::IDENTITY);
        let (d, n) = gap(&a, &unit_box(DVec3::new(1.25, 0.0, 0.0), DQuat::IDENTITY));
        assert!(close(d, 0.25) && n == DVec3::X, "{d} {n:?}");

        let diagonal = unit_box(DVec3::new(1.5, 1.5, 1.5), DQuat::IDENTITY);
        let (d, _) = gap(&a, &diagonal);
        let corners = (3.0f64 * 0.5 * 0.5).sqrt();
        assert!(d <= corners + 1e-12, "{d} against the true {corners}");
        assert!(close(d, 0.5), "{d}");

        let (d, _) = gap(&a, &unit_box(DVec3::new(0.8, 0.0, 0.0), DQuat::IDENTITY));
        assert!(close(d, -0.2), "{d}");
    }

    /// A turned box's lowest corner is where the floor measures it from.
    #[test]
    fn a_turned_box_is_as_high_as_its_lowest_corner() {
        let turned = crate::rotation_from_scaled_axis(DVec3::Z * core::f64::consts::FRAC_PI_4);
        let (d, _) = gap(
            &unit_box(DVec3::new(0.0, 1.0, 0.0), turned),
            &ContactShape::Plane {
                normal: DVec3::Y,
                offset: 0.0,
            },
        );
        assert!((d - (1.0 - 0.5 * 2.0f64.sqrt())).abs() < 1e-12, "{d}");
    }
}
