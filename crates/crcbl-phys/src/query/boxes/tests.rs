//! A box turned 45° answers by its turned faces where the unturned box, which
//! the query world answered before boxes could turn, gives the other answer.
//!
//! Every case uses a cube of half extent 1 at the origin. Turned 45° about
//! `+Y`, its footprint is the diamond `|x| + |z| ≤ √2`, its faces' normals
//! `(±1, 0, ±1) / √2`, each face 1 from the centre.

use core::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_4, SQRT_2};

use super::*;

/// The unit cube, unturned.
fn unturned() -> BoxCollider {
    BoxCollider::new(DVec3::ZERO, DVec3::ONE)
}

/// The unit cube turned 45° about `axis`.
fn turned_about(axis: DVec3) -> BoxCollider {
    unturned().with_rotation(crate::rotation_from_scaled_axis(axis * FRAC_PI_4))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

fn near(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-12
}

/// The diamond's face towards `-X`, `+Z`.
fn face_normal() -> DVec3 {
    DVec3::new(-1.0, 0.0, 1.0) / SQRT_2
}

/// **The bounds are the turned box's reach**: `√2` across X and Z, 1 up Y.
#[test]
fn a_turned_boxs_bounds_reach_its_corners() {
    let bounds = turned_about(DVec3::Y).aabb();
    assert!(
        near(bounds.max, DVec3::new(SQRT_2, 1.0, SQRT_2)),
        "{bounds:?}"
    );
    assert!(near(bounds.min, -bounds.max), "{bounds:?}");
}

/// **A ray past the unturned box's side strikes the turned box's face**: at
/// `z = 1.2` a ray along `+X` passes the unturned cube, and meets the diamond
/// where `-x + z = √2`, on its `(-1, 0, 1) / √2` face.
#[test]
fn a_ray_beside_the_unturned_box_hits_the_turned_face() {
    let ray = Ray::new(DVec3::new(-5.0, 0.0, 1.2), DVec3::X);
    assert!(ray_vs_box(&ray, &unturned()).is_none());

    let hit = ray_vs_box(&ray, &turned_about(DVec3::Y)).expect("the turned face");
    let x = 1.2 - SQRT_2;
    assert!(close(hit.t, x + 5.0), "{hit:?}");
    assert!(near(hit.point, DVec3::new(x, 0.0, 1.2)), "{hit:?}");
    assert!(near(hit.normal, face_normal()), "{hit:?}");
    assert!(!hit.started_inside);
}

/// **A ray across the unturned box's corner misses the turned box**: along
/// `(1, 0, -1)`, `x + z` stays 1.7 — inside the unturned cube's corner, which
/// reaches `x + z = 2`, and outside the diamond, which reaches `√2`.
#[test]
fn a_ray_across_the_unturned_corner_misses_the_turned_box() {
    let ray = Ray::new(
        DVec3::new(-3.0, 0.0, 4.7),
        DVec3::new(1.0, 0.0, -1.0).normalize(),
    );
    assert!(ray_vs_box(&ray, &unturned()).is_some());
    assert!(ray_vs_box(&ray, &turned_about(DVec3::Y)).is_none());
}

/// **A swept sphere meets the turned face**: a sphere of radius 0.1 along
/// `+X` at `z = 1.25` clears the unturned cube, and touches the diamond's face
/// when its centre is 1.1 from the box's centre along that face's normal:
/// `(-x + 1.25) / √2 = 1.1`.
#[test]
fn a_swept_sphere_meets_the_turned_face() {
    let path = Segment::new(DVec3::new(-5.0, 0.0, 1.25), DVec3::new(5.0, 0.0, 1.25));
    assert!(swept_sphere_vs_box(&path, 0.1, &unturned()).is_none());

    let hit = swept_sphere_vs_box(&path, 0.1, &turned_about(DVec3::Y)).expect("the turned face");
    let x = 1.25 - 1.1 * SQRT_2;
    assert!(close(hit.t, (x + 5.0) / 10.0), "{hit:?}");
    assert!(near(hit.normal, face_normal()), "{hit:?}");
    let centre = DVec3::new(x, 0.0, 1.25);
    assert!(near(hit.point, centre - face_normal() * 0.1), "{hit:?}");
}

/// **A sphere overlaps what the turned box covers**: one by the diamond's
/// corner on `+X`, past the unturned cube's face, overlaps; one inside the
/// unturned cube's corner, 0.2 off the diamond's face, does not.
#[test]
fn a_sphere_overlaps_the_turned_box_where_it_reaches() {
    let by_the_corner = Sphere::new(DVec3::new(1.2, 0.0, 0.0), 0.1);
    assert!(!sphere_overlaps_box(&by_the_corner, &unturned()));
    assert!(sphere_overlaps_box(&by_the_corner, &turned_about(DVec3::Y)));

    let in_the_old_corner = Sphere::new(DVec3::new(0.85, 0.0, 0.85), 0.05);
    assert!(sphere_overlaps_box(&in_the_old_corner, &unturned()));
    assert!(!sphere_overlaps_box(
        &in_the_old_corner,
        &turned_about(DVec3::Y)
    ));
}

/// How far short of the contact a capsule sweep against a turned box may stop,
/// as a share of a ten-metre path: the advancement's tolerance and no more.
const ADVANCE_SHARE: f64 = 2.0e-4;

/// Whether `t` is at the contact `expected` or short of it by no more than
/// [`ADVANCE_SHARE`] — never past it, beyond rounding: an advancement whose
/// first step closes a straight gap lands on the contact itself.
fn just_short(t: f64, expected: f64) -> bool {
    t <= expected + 1e-12 && expected - t < ADVANCE_SHARE
}

/// **A capsule falling onto a ridge lands on the ridge**: turned 45° about
/// `Z`, the cube's top is an edge along `Z`, `√2` up, where the unturned
/// cube's top is a face at 1. A capsule of radius 0.1 and half-height 0.5
/// falling from `y = 5` touches it when its centre is `√2 + 0.6` up.
#[test]
fn a_falling_capsule_lands_on_the_turned_ridge() {
    let path = Segment::new(DVec3::new(0.0, 5.0, 0.0), DVec3::ZERO);
    let flat = swept_capsule_vs_box(&path, 0.1, 0.5, &unturned()).expect("the top");
    assert!(close(flat.t, (5.0 - 1.6) / 5.0), "{flat:?}");

    let hit = swept_capsule_vs_box(&path, 0.1, 0.5, &turned_about(DVec3::Z)).expect("the ridge");
    let expected = (5.0 - (SQRT_2 + 0.6)) / 5.0;
    assert!(
        just_short(hit.t, expected),
        "met at {} of the fall, not just short of {expected}: {hit:?}",
        hit.t,
    );
    assert!((hit.normal - DVec3::Y).length() < 1e-6, "{hit:?}");
    assert!(near(hit.point, DVec3::new(0.0, SQRT_2, 0.0)), "{hit:?}");
    assert!(!hit.started_inside);
}

/// **A capsule sweeping sideways meets the turned face**, as the sphere does
/// (its core stays upright in a box turned about `Y`), a little short of it.
#[test]
fn a_sideways_capsule_meets_the_turned_face() {
    let path = Segment::new(DVec3::new(-5.0, 0.0, 1.25), DVec3::new(5.0, 0.0, 1.25));
    assert!(swept_capsule_vs_box(&path, 0.1, 0.5, &unturned()).is_none());

    let hit = swept_capsule_vs_box(&path, 0.1, 0.5, &turned_about(DVec3::Y)).expect("the face");
    let expected = (1.25 - 1.1 * SQRT_2 + 5.0) / 10.0;
    assert!(
        just_short(hit.t, expected),
        "met at {}, not just short of {expected}: {hit:?}",
        hit.t,
    );
    assert!((hit.normal - face_normal()).length() < 1e-6, "{hit:?}");
    // On the face, a radius from the core along the normal: the core is at
    // z = 1.25, so the point is a radius's share of z below it.
    let on_face = hit.point.z - hit.point.x;
    assert!((on_face - SQRT_2).abs() < 1e-3, "{hit:?}");
    assert!(
        (hit.point.z - (1.25 - 0.1 * FRAC_1_SQRT_2)).abs() < 1e-3,
        "{hit:?}"
    );
}

/// **A capsule that starts inside the turned box meets it at once**, and one
/// standing still clear of it meets nothing.
#[test]
fn a_capsule_sweep_starting_inside_or_standing_still() {
    let turned = turned_about(DVec3::Y);
    let inside = Segment::new(DVec3::new(1.2, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
    let hit = swept_capsule_vs_box(&inside, 0.1, 0.5, &turned).expect("inside");
    assert!(hit.started_inside && hit.t == 0.0, "{hit:?}");

    let still = Segment::new(DVec3::new(3.0, 0.0, 0.0), DVec3::new(3.0, 0.0, 0.0));
    assert!(swept_capsule_vs_box(&still, 0.1, 0.5, &turned).is_none());
}

/// **A capsule is pushed out along the turned face**: an upright capsule of
/// radius 0.3 whose core stands 1.25 from the centre along the diamond's
/// `(1, 0, 1) / √2` face normal is 0.05 into that face, and pushed out along
/// it. 1.35 out it is clear — though inside the unturned cube both times.
#[test]
fn a_capsule_is_pushed_out_along_the_turned_face() {
    let turned = turned_about(DVec3::Y);
    let normal = DVec3::new(1.0, 0.0, 1.0) / SQRT_2;
    let into = Capsule::new(normal * 1.25, 0.3, 0.5);
    let push = capsule_penetration_vs_box(&into, &turned).expect("overlapping the face");
    assert!(close(push.depth, 0.05), "{push:?}");
    assert!(near(push.normal, normal), "{push:?}");
    assert!(capsule_penetration_vs_box(&into, &unturned()).is_some_and(|old| old.normal != normal));

    let clear = Capsule::new(normal * 1.35, 0.3, 0.5);
    assert!(capsule_penetration_vs_box(&clear, &turned).is_none());
    assert!(capsule_penetration_vs_box(&clear, &unturned()).is_some());
}

/// **An unturned box is the world-axis function's answer, to the bit**, from
/// a centre the local-frame route would round differently.
#[test]
fn an_unturned_box_answers_exactly_as_its_aabb() {
    let target = BoxCollider::new(DVec3::new(0.1, 0.7, -0.3), DVec3::new(0.3, 0.2, 0.9));
    let ray = Ray::new(
        DVec3::new(-5.0, 0.75, 0.1),
        DVec3::new(1.0, 0.01, 0.02).normalize(),
    );
    assert_eq!(ray_vs_box(&ray, &target), ray_vs_aabb(&ray, &target.aabb()));
    let path = Segment::new(DVec3::new(-5.0, 0.73, 0.1), DVec3::new(5.0, 0.71, 0.0));
    assert_eq!(
        swept_sphere_vs_box(&path, 0.07, &target),
        swept_sphere_vs_aabb(&path, 0.07, &target.aabb()),
    );
    assert_eq!(
        swept_capsule_vs_box(&path, 0.07, 0.3, &target),
        swept_capsule_vs_aabb(&path, 0.07, 0.3, &target.aabb()),
    );
}

/// **A short sweep keeps its contact time against a turned box**, as
/// [`swept_sphere_vs_aabb`] does against an unturned one, at every scale: the
/// turned box is that sweep in the box's frame. That sweep inflates the box by
/// the radius rather than rounding it, so a sphere of radius `s` heading along
/// `+X` straight at the diamond's vertical edge meets the inflated box's edge,
/// `2√2 s` out, when its centre is at `-2√2 s`: `0.59` of the way from `-4 s`
/// to `-2 s`.
#[test]
fn a_short_sweep_keeps_its_contact_time_against_a_turned_box() {
    for scale in [1e100, 1.0, 1e-6, 1e-10, 1e-100] {
        let target = BoxCollider::new(DVec3::ZERO, DVec3::splat(scale))
            .with_rotation(crate::rotation_from_scaled_axis(DVec3::Y * FRAC_PI_4));
        let path = Segment::new(
            DVec3::new(-4.0 * scale, 0.0, 0.0),
            DVec3::new(-2.0 * scale, 0.0, 0.0),
        );
        let hit = swept_sphere_vs_box(&path, scale, &target)
            .unwrap_or_else(|| panic!("a short sweep missed at scale {scale}"));
        let expected = (4.0 - 2.0 * SQRT_2) / 2.0;
        assert!(
            (hit.t - expected).abs() < 1e-12,
            "met at {} of the way, not {expected}, at scale {scale}",
            hit.t,
        );
        assert!(!hit.started_inside);
    }
}
