//! Every shape's overlap against a closed-form answer: the depth, the normal
//! and the point, for a sphere against a sphere, a capsule (side, cap, turned)
//! and a box (face, edge, corner, turned), with the centre inside, on the
//! degenerate centre each rule names, and on the touching line.
//!
//! The collider set has no convex hull, so there is no sphere–hull pair to
//! check; `docs/backlog.md` records it with the hull collider itself.

use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};

use glam::DVec3;

use super::*;
use crate::collider::BoxCollider;
use crate::integrator::rotation_from_scaled_axis;
use crate::query::{TurnedCapsule, sphere_overlap_vs_box, sphere_overlap_vs_turned_capsule};

/// How closely a measured answer has to agree with one worked out by hand.
/// The inputs are chosen so every answer is a few roundings from exact.
const EXACT: f64 = 1e-12;

fn assert_hit(hit: Option<OverlapHit>, point: DVec3, normal: DVec3, depth: f64) {
    let hit = hit.expect("an overlap");
    assert!(
        (hit.point - point).length() < EXACT,
        "point: {hit:?}, wanted {point}"
    );
    assert!(
        (hit.normal - normal).length() < EXACT,
        "normal: {hit:?}, wanted {normal}"
    );
    assert!(
        (hit.depth - depth).abs() < EXACT,
        "depth: {hit:?}, wanted {depth}"
    );
    assert!(
        (hit.normal.length() - 1.0).abs() < EXACT,
        "not unit: {hit:?}"
    );
    assert!(hit.depth > 0.0, "a hit with no depth: {hit:?}");
    assert_eq!(hit.part, 0);
}

/// The answer's own consistency: moved `depth` along `normal`, the sphere
/// touches the shape at `point`.
fn assert_pushed_out_touches(sphere: &Sphere, hit: OverlapHit) {
    let moved = sphere.centre + hit.normal * hit.depth;
    assert!(
        ((moved - hit.point).length() - sphere.radius).abs() < EXACT,
        "the pushed-out sphere does not touch the point: {hit:?}",
    );
}

// ── Sphere ──────────────────────────────────────────────────────────

/// **Two spheres three apart, radii 1.5 and 2, overlap by a half** along the
/// line between the centres, touching at the target's surface.
#[test]
fn a_sphere_inside_a_sphere_is_pushed_out_along_the_centres() {
    let query = Sphere::new(DVec3::new(3.0, 0.0, 0.0), 1.5);
    let target = Sphere::new(DVec3::ZERO, 2.0);
    let hit = sphere_overlap_vs_sphere(&query, &target);
    assert_hit(hit, DVec3::new(2.0, 0.0, 0.0), DVec3::X, 0.5);
    assert_pushed_out_touches(&query, hit.unwrap());
}

/// **Coincident centres leave along `+Y`**, by the whole of both radii.
#[test]
fn coincident_spheres_leave_along_plus_y() {
    let query = Sphere::new(DVec3::new(1.0, 2.0, 3.0), 1.5);
    let target = Sphere::new(DVec3::new(1.0, 2.0, 3.0), 2.0);
    assert_hit(
        sphere_overlap_vs_sphere(&query, &target),
        DVec3::new(1.0, 4.0, 3.0),
        DVec3::Y,
        3.5,
    );
}

/// **Touching is not overlapping**: centres exactly the sum of the radii apart
/// give no hit, and neither does anything further.
#[test]
fn spheres_that_only_touch_or_are_apart_do_not_overlap() {
    let target = Sphere::new(DVec3::ZERO, 2.0);
    let touching = Sphere::new(DVec3::new(3.5, 0.0, 0.0), 1.5);
    assert_eq!(sphere_overlap_vs_sphere(&touching, &target), None);
    let apart = Sphere::new(DVec3::new(0.0, 0.0, -4.0), 1.5);
    assert_eq!(sphere_overlap_vs_sphere(&apart, &target), None);
}

// ── Capsule ─────────────────────────────────────────────────────────

/// The capsule every capsule case is against: radius ½ about `y ∈ [-1, 1]`.
fn capsule() -> Capsule {
    Capsule::new(DVec3::ZERO, 0.5, 1.0)
}

/// **Beside the core, out sideways**: a centre one from the core with radii
/// ¾ and ½ is a quarter deep.
#[test]
fn a_sphere_beside_a_capsule_is_pushed_out_sideways() {
    let query = Sphere::new(DVec3::new(1.0, 0.5, 0.0), 0.75);
    let hit = sphere_overlap_vs_capsule(&query, &capsule());
    assert_hit(hit, DVec3::new(0.5, 0.5, 0.0), DVec3::X, 0.25);
    assert_pushed_out_touches(&query, hit.unwrap());
}

/// **Over the cap, out through the cap**: one above the top of the core.
#[test]
fn a_sphere_over_a_capsule_cap_is_pushed_out_through_it() {
    let query = Sphere::new(DVec3::new(0.0, 2.0, 0.0), 1.25);
    assert_hit(
        sphere_overlap_vs_capsule(&query, &capsule()),
        DVec3::new(0.0, 1.5, 0.0),
        DVec3::Y,
        0.75,
    );
}

/// **A centre on the core leaves along `+Y`**, by both radii. Half way up
/// the core's top half, where the nearest core point is exact in `f64`: at
/// `y = 0.3` it rounds a hair above the centre, and the centre then leaves
/// along `-Y` — see the module docs.
#[test]
fn a_centre_on_a_capsule_core_leaves_along_plus_y() {
    let query = Sphere::new(DVec3::new(0.0, 0.5, 0.0), 0.75);
    assert_hit(
        sphere_overlap_vs_capsule(&query, &capsule()),
        DVec3::new(0.0, 1.0, 0.0),
        DVec3::Y,
        1.25,
    );
}

#[test]
fn a_sphere_that_only_touches_a_capsule_does_not_overlap_it() {
    let query = Sphere::new(DVec3::new(1.25, 0.0, 0.0), 0.75);
    assert_eq!(sphere_overlap_vs_capsule(&query, &capsule()), None);
}

/// **A turned capsule answers in its own frame**: lying along `X` (a quarter
/// turn about `Z`), a sphere above its middle is pushed up, and one centred
/// on its core leaves along the core — the capsule's own `+Y`, turned.
#[test]
fn a_turned_capsule_is_measured_in_its_own_frame() {
    let turn = rotation_from_scaled_axis(DVec3::Z * FRAC_PI_2);
    let lying = TurnedCapsule::new(capsule(), turn);

    let above = Sphere::new(DVec3::new(0.5, 1.0, 0.0), 0.75);
    assert_hit(
        sphere_overlap_vs_turned_capsule(&above, &lying),
        DVec3::new(0.5, 0.5, 0.0),
        DVec3::Y,
        0.25,
    );

    let on_core = Sphere::new(DVec3::new(0.5, 0.0, 0.0), 0.75);
    let along = turn * DVec3::Y;
    assert_hit(
        sphere_overlap_vs_turned_capsule(&on_core, &lying),
        on_core.centre + along * 0.5,
        along,
        1.25,
    );
}

// ── Box ─────────────────────────────────────────────────────────────

/// The box every box case is against: half-extents 1, 2 and 3 at the origin.
fn boxed() -> BoxCollider {
    BoxCollider::new(DVec3::ZERO, DVec3::new(1.0, 2.0, 3.0))
}

#[test]
fn a_sphere_against_a_box_face_is_pushed_out_through_the_face() {
    let query = Sphere::new(DVec3::new(1.5, 0.5, -1.0), 1.0);
    let hit = sphere_overlap_vs_box(&query, &boxed());
    assert_hit(hit, DVec3::new(1.0, 0.5, -1.0), DVec3::X, 0.5);
    assert_pushed_out_touches(&query, hit.unwrap());
}

/// **Off an edge, out along the diagonal from it**: a centre one past each of
/// two faces is `√2` from the edge.
#[test]
fn a_sphere_against_a_box_edge_is_pushed_out_from_the_edge() {
    let query = Sphere::new(DVec3::new(2.0, 3.0, 0.0), 1.5);
    let hit = sphere_overlap_vs_box(&query, &boxed());
    assert_hit(
        hit,
        DVec3::new(1.0, 2.0, 0.0),
        DVec3::new(1.0, 1.0, 0.0) / SQRT_2,
        1.5 - SQRT_2,
    );
    assert_pushed_out_touches(&query, hit.unwrap());
}

/// **Off a corner, out along the diagonal from it**: one past each of three
/// faces is `√3` from the corner.
#[test]
fn a_sphere_against_a_box_corner_is_pushed_out_from_the_corner() {
    let query = Sphere::new(DVec3::new(2.0, 3.0, 4.0), 2.0);
    let root_three = 3.0_f64.sqrt();
    assert_hit(
        sphere_overlap_vs_box(&query, &boxed()),
        DVec3::new(1.0, 2.0, 3.0),
        DVec3::ONE / root_three,
        2.0 - root_three,
    );
}

/// **A centre inside leaves through the nearest face**, by its distance to
/// that face plus the radius, and the point is on that face.
#[test]
fn a_centre_inside_a_box_leaves_through_the_nearest_face() {
    let query = Sphere::new(DVec3::new(0.7, -0.5, 1.0), 0.1);
    assert_hit(
        sphere_overlap_vs_box(&query, &boxed()),
        DVec3::new(1.0, -0.5, 1.0),
        DVec3::X,
        0.4,
    );
}

/// **The centre of a cube is equally near all six faces, and leaves through
/// `-X`**, the first of the documented order.
#[test]
fn the_centre_of_a_cube_leaves_through_minus_x() {
    let cube = BoxCollider::new(DVec3::new(5.0, 0.0, 0.0), DVec3::ONE);
    let query = Sphere::new(DVec3::new(5.0, 0.0, 0.0), 0.25);
    assert_hit(
        sphere_overlap_vs_box(&query, &cube),
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::NEG_X,
        1.25,
    );
}

#[test]
fn a_sphere_that_only_touches_a_box_does_not_overlap_it() {
    let face = Sphere::new(DVec3::new(2.0, 0.0, 0.0), 1.0);
    assert_eq!(sphere_overlap_vs_box(&face, &boxed()), None);
    let edge = Sphere::new(DVec3::new(4.0, 6.0, 0.0), 5.0);
    assert_eq!(sphere_overlap_vs_box(&edge, &boxed()), None);
}

/// **A turned box answers in its own frame**: a unit cube turned an eighth
/// about `Y` has a vertical edge `√2` out along `+X`, so a unit sphere at
/// `x = 2` is pushed straight out from that edge by `√2 − 1`.
#[test]
fn a_turned_box_is_measured_in_its_own_frame() {
    let cube = BoxCollider::new(DVec3::ZERO, DVec3::ONE)
        .with_rotation(rotation_from_scaled_axis(DVec3::Y * FRAC_PI_4));
    let query = Sphere::new(DVec3::new(2.0, 0.0, 0.0), 1.0);
    let hit = sphere_overlap_vs_box(&query, &cube);
    assert_hit(hit, DVec3::new(SQRT_2, 0.0, 0.0), DVec3::X, SQRT_2 - 1.0);
    assert_pushed_out_touches(&query, hit.unwrap());
}

/// **An AABB is the box's own answer**, for the unturned box with its bounds.
#[test]
fn an_aabb_answers_as_the_box_with_its_bounds() {
    let query = Sphere::new(DVec3::new(2.0, 3.0, 0.0), 1.5);
    assert_eq!(
        sphere_overlap_vs_aabb(&query, &boxed().aabb()),
        sphere_overlap_vs_box(&query, &boxed()),
    );
}
