use super::*;
use crate::broadphase::Segment;

#[test]
fn short_box_sweeps_keep_their_contact_time() {
    for scale in [1e300, 1e100, 1e-6, 1e-10, 1e-100, 1e-300] {
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            let target = Aabb::new(DVec3::ZERO, DVec3::splat(10.0 * scale));
            let start = DVec3::splat(2.0 * scale) - axis * (4.0 * scale);
            let hit = swept_sphere_vs_aabb(
                &Segment::new(start, start + axis * (2.0 * scale)),
                scale,
                &target,
            )
            .unwrap_or_else(|| panic!("short moving sweep missed: scale={scale} axis={axis:?}"));
            assert!((hit.t - 0.5).abs() < 1e-12);
            assert_eq!(hit.normal, -axis);
            assert!(!hit.started_inside);
            assert!(
                swept_sphere_vs_aabb(
                    &Segment::new(start, start + axis * (0.5 * scale)),
                    scale,
                    &target,
                )
                .is_none()
            );
            assert!(swept_sphere_vs_aabb(&Segment::new(start, start), scale, &target).is_none());
            let inside = start + axis * (1.5 * scale);
            let resting = swept_sphere_vs_aabb(&Segment::new(inside, inside), scale, &target)
                .expect("stationary overlap remains a contact");
            assert_eq!(resting.t, 0.0);
            assert_eq!(resting.normal, -axis);
            assert!(resting.started_inside);
        }
    }
}

/// EW's box edge (2026-10-23): a box whose top face is at `y = 0` and whose
/// edge is at `x = 0.5`, and a capsule of radius 0.3 falling at `x` beside
/// it.
fn falling_beside_the_edge(x: f64, z: f64) -> Option<ShapeHit> {
    let target = Aabb::new(DVec3::new(-0.5, -1.0, -0.5), DVec3::new(0.5, 0.0, 0.5));
    let path = Segment::new(DVec3::new(x, 2.0, z), DVec3::new(x, -2.0, z));
    crate::query::swept_capsule_vs_aabb(&path, 0.3, 0.6, &target)
}

/// **A capsule falling within its radius of a box's edge meets the edge's
/// curve**, with the normal from the edge towards the capsule's axis, and not
/// the inflated box's flat top, which stood EW's character on air: 0.29 from
/// the edge, the contact is where the rounding reaches, 0.0768 above the top,
/// at a normal far steeper than any slope a character stands on.
#[test]
fn a_capsule_beside_a_box_edge_meets_the_rounded_edge() {
    let hit = falling_beside_the_edge(0.79, 0.0).expect("within its radius of the edge");
    let rise = (0.3_f64 * 0.3 - 0.29 * 0.29).sqrt();
    let expected = DVec3::new(0.29, rise, 0.0) / 0.3;
    assert!(
        (hit.normal - expected).length() < 1e-9,
        "normal {:?}, expected {expected:?}",
        hit.normal
    );
    let centre_y = 2.0 - 4.0 * hit.t;
    assert!((centre_y - (0.6 + rise)).abs() < 1e-9, "met at {centre_y}");
    assert!((hit.point - DVec3::new(0.5, 0.0, 0.0)).length() < 1e-9);
    assert!(!hit.started_inside);
}

/// Past the edge by more than the radius it falls clear, and diagonally
/// beyond a corner, 0.29 out on both axes and so 0.41 from it, too; over the
/// face it still lands flat on the top.
#[test]
fn a_capsule_clear_of_the_rounded_box_misses_and_one_over_it_lands_flat() {
    assert!(falling_beside_the_edge(0.81, 0.0).is_none());
    assert!(
        falling_beside_the_edge(0.79, 0.79).is_none(),
        "beyond the corner"
    );
    let flat = falling_beside_the_edge(0.4, 0.0).expect("over the top face");
    assert_eq!(flat.normal, DVec3::Y);
    assert!((2.0 - 4.0 * flat.t - 0.9).abs() < 1e-9);
}

/// At a corner the capsule meets the corner's sphere: within its radius of
/// the corner on the diagonal, the normal points from the corner to it.
#[test]
fn a_capsule_at_a_box_corner_meets_the_corner_sphere() {
    let out = 0.15;
    let hit =
        falling_beside_the_edge(0.5 + out, 0.5 + out).expect("within its radius of the corner");
    let rise = (0.3_f64 * 0.3 - 2.0 * out * out).sqrt();
    let expected = DVec3::new(out, rise, out) / 0.3;
    assert!((hit.normal - expected).length() < 1e-9, "{:?}", hit.normal);
    assert!((hit.point - DVec3::new(0.5, 0.0, 0.5)).length() < 1e-9);
}
