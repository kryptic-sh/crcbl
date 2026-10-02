//! The query world answers a turned capsule by its turned core, in every
//! query, and a [`PhysicsSystem`] puts a body's capsule there turned with the
//! body.
//!
//! The capsule is centred at the origin, radius 0.5 and half-height 1, turned
//! a quarter about `+Z`, which lays its core along the world's `X` from
//! `x = -1` to `x = 1`. Each query is one the upright capsule answers
//! differently.

use core::f64::consts::FRAC_PI_2;

use glam::{DQuat, DVec3};

use crate::broadphase::{Ray, Segment};
use crate::collider::{Capsule, LyingCapsule};
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::system::PhysicsSystem;

use super::{PhysicsWorld, QueryFilter};

/// How closely a hit has to match the closed form: the quarter turn's sine
/// and cosine are within rounding of 1 and 0.
const CLOSE: f64 = 1e-12;

/// How far short of the contact the conservative advancement an upright
/// capsule is swept by may stop: within a quarter of a linear slop, and this
/// is a whole one.
const SWEEP_TOLERANCE: f64 = 5e-3;

const RADIUS: f64 = 0.5;
const HALF_HEIGHT: f64 = 1.0;

/// A quarter turn about `+Z`: the capsule's `+Y` goes to the world's `-X`.
fn lay_down() -> DQuat {
    crate::rotation_from_scaled_axis(DVec3::Z * FRAC_PI_2)
}

fn capsule() -> Capsule {
    Capsule::new(DVec3::ZERO, RADIUS, HALF_HEIGHT)
}

/// A world holding the capsule laid down, and one holding it upright.
fn worlds() -> (PhysicsWorld, PhysicsWorld) {
    let mut lying = PhysicsWorld::new();
    lying.add_turned_capsule(capsule(), lay_down());
    let mut upright = PhysicsWorld::new();
    upright.add_capsule(capsule());
    (lying, upright)
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < CLOSE
}

/// **A ray meets the laid-down capsule's core where the upright one is not**:
/// straight down at `x = 1.2`, past the upright one's radius, onto the top of
/// the lying one's cap end, and along `X` onto its cap rather than its side.
#[test]
fn a_ray_meets_the_turned_core() {
    let (mut lying, mut upright) = worlds();
    let down = Ray::new(DVec3::new(1.2, 5.0, 0.0), DVec3::NEG_Y);
    let (_, hit) = lying.cast_ray(&down).expect("the lying capsule");
    // The cap's centre is at x = 1: the surface over x = 1.2 is at
    // y = √(0.25 - 0.04).
    let y = (RADIUS * RADIUS - 0.2 * 0.2).sqrt();
    assert!((hit.t - (5.0 - y)).abs() < CLOSE, "{hit:?}");
    assert!(
        close(hit.normal, DVec3::new(0.2, y, 0.0) / RADIUS),
        "{hit:?}"
    );
    assert_eq!(upright.cast_ray(&down), None);

    let along = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X);
    let (_, hit) = lying.cast_ray(&along).expect("the lying capsule");
    assert!((hit.t - 3.5).abs() < CLOSE, "the cap at x = -1.5: {hit:?}");
    assert!(close(hit.normal, DVec3::NEG_X), "{hit:?}");
    let (_, hit) = upright.cast_ray(&along).expect("the upright capsule");
    assert!((hit.t - 4.5).abs() < CLOSE, "its side at x = -0.5: {hit:?}");
}

/// **A sphere sweep and a sphere overlap meet the laid-down capsule only.**
#[test]
fn a_sphere_sweep_and_overlap_meet_the_turned_core() {
    let (mut lying, mut upright) = worlds();
    let down = Segment::new(DVec3::new(0.9, 5.0, 0.0), DVec3::new(0.9, -5.0, 0.0));
    let (_, hit) = lying.sweep_sphere(&down, 0.25).expect("the lying capsule");
    // Over its side, where the top is at y = 0.5: the centre stops at 0.75.
    assert!((hit.t - (5.0 - 0.75) / 10.0).abs() < CLOSE, "{hit:?}");
    assert!(close(hit.normal, DVec3::Y), "{hit:?}");
    assert!(close(hit.point, DVec3::new(0.9, 0.5, 0.0)), "{hit:?}");
    assert_eq!(upright.sweep_sphere(&down, 0.25), None);

    // 0.67 from the cap's centre at x = 1, inside 0.5 + 0.2.
    let near_the_end = DVec3::new(1.3, 0.6, 0.0);
    assert_eq!(lying.overlap_sphere(near_the_end, 0.2).len(), 1);
    assert!(upright.overlap_sphere(near_the_end, 0.2).is_empty());
}

/// **An upright capsule is pushed out of the laid-down one along the line
/// between the cores**, and swept against it stops just short of the
/// contact.
#[test]
fn an_upright_capsule_is_pushed_out_and_stopped_by_the_turned_core() {
    let (mut lying, mut upright) = worlds();

    // Its core runs up from (1.2, 0.4, 0); the lying core ends at (1, 0, 0).
    let character = Capsule::new(DVec3::new(1.2, 0.9, 0.0), 0.5, 0.5);
    let mut out = Vec::new();
    lying.capsule_penetrations_into(&character, None, &mut out);
    let [(_, push)] = out[..] else {
        panic!("one push-out, not {out:?}");
    };
    let between = DVec3::new(0.2, 0.4, 0.0);
    assert!(
        (push.depth - (1.0 - between.length())).abs() < CLOSE,
        "{push:?}"
    );
    assert!(close(push.normal, between.normalize()), "{push:?}");
    upright.capsule_penetrations_into(&character, None, &mut out);
    assert!(out.is_empty(), "{out:?}");

    // Down at x = 0.9, over the lying core and past the upright one's reach:
    // the swept capsule's bottom end, 0.25 + 0.25 below its centre, meets the
    // top at y = 0.5 with its centre at y = 1.
    let down = Segment::new(DVec3::new(0.9, 5.0, 0.0), DVec3::new(0.9, -5.0, 0.0));
    let (_, hit) = lying
        .sweep_capsule(&down, 0.25, 0.25)
        .expect("the lying capsule");
    let exact = (5.0 - 1.0) / 10.0;
    assert!(
        hit.t <= exact && hit.t >= exact - SWEEP_TOLERANCE / 10.0,
        "{hit:?}"
    );
    assert!((hit.normal - DVec3::Y).length() < 1e-6, "{hit:?}");
    assert!(
        (hit.point - DVec3::new(0.9, 0.5, 0.0)).length() < 1e-6,
        "{hit:?}"
    );
    assert_eq!(upright.sweep_capsule(&down, 0.25, 0.25), None);
}

/// **A body lying along `Z` beside the laid-down capsule is blocked by it**:
/// 0.3 above its core, inside the radii's 0.75, and 0.9 from the upright
/// one's.
#[test]
fn a_lying_body_is_blocked_by_the_turned_core() {
    let (mut lying, mut upright) = worlds();
    let body = LyingCapsule::new(DVec3::new(0.9, 0.3, -1.0), 0.0, 0.25, 2.0);
    assert!(
        lying
            .lying_capsule_blocker(&body, QueryFilter::ALL)
            .is_some()
    );
    assert_eq!(upright.lying_capsule_blocker(&body, QueryFilter::ALL), None);
}

/// **A body lying along `Z` lowered onto the laid-down capsule stops on
/// it**, its core a radius sum above the capsule's, where it would pass the
/// upright one by.
#[test]
fn a_lying_body_lowered_onto_the_turned_core_stops_on_it() {
    let (mut lying, mut upright) = worlds();
    let body = LyingCapsule::new(DVec3::new(0.9, 1.5, -1.0), 0.0, 0.25, 2.0);
    let down = DVec3::new(0.0, -2.0, 0.0);
    let (_, hit) = lying
        .sweep_lying_capsule(&body, down, QueryFilter::ALL)
        .expect("the lying capsule");
    // The body's core stops at y = 0.5 + 0.25, from 1.5, of a 2 m move.
    let exact = (1.5 - 0.75) / 2.0;
    assert!(
        hit.t <= exact && hit.t >= exact - SWEEP_TOLERANCE / 2.0,
        "{hit:?}"
    );
    assert!((hit.normal - DVec3::Y).length() < 1e-6, "{hit:?}");
    assert_eq!(
        upright.sweep_lying_capsule(&body, down, QueryFilter::ALL),
        None
    );
}

/// **The broadphase holds the box around the turned core**: its ends grown
/// by the radius, laid along `X` by a quarter turn about `Z` and along `Z` by
/// one about `X`.
#[test]
fn the_bounds_are_the_turned_cores() {
    let along_z = crate::rotation_from_scaled_axis(DVec3::X * FRAC_PI_2);
    for (turn, half) in [
        (lay_down(), DVec3::new(1.5, 0.5, 0.5)),
        (along_z, DVec3::new(0.5, 0.5, 1.5)),
    ] {
        let mut world = PhysicsWorld::new();
        let id = world.add_turned_capsule(capsule(), turn);
        let bounds = world.aabb_of(id).expect("in the world");
        assert!(close(bounds.min, -half), "{bounds:?}");
        assert!(close(bounds.max, half), "{bounds:?}");
    }
}

/// **A capsule turned about `Y` alone still stands, and answers as the
/// upright one does, to the bit** — a character's facing turns its body
/// about `Y`, and must not move a single query answer.
#[test]
fn a_capsule_turned_about_y_answers_as_the_upright_one_to_the_bit() {
    let facing = crate::rotation_from_scaled_axis(DVec3::Y * 0.7);
    assert_ne!(facing, DQuat::IDENTITY);
    let centre = DVec3::new(0.3, 0.2, -0.1);
    let mut turned = PhysicsWorld::new();
    let turned_id = turned.add_turned_capsule(Capsule::new(centre, RADIUS, HALF_HEIGHT), facing);
    let mut upright = PhysicsWorld::new();
    let upright_id = upright.add_capsule(Capsule::new(centre, RADIUS, HALF_HEIGHT));

    assert_eq!(turned.aabb_of(turned_id), upright.aabb_of(upright_id));
    // Aimed off the centre, so neither meets the core head on.
    let aimed = |from: DVec3, at: DVec3| Ray::new(from, at - from);
    let rays = [
        aimed(DVec3::new(-5.0, 0.7, 0.1), DVec3::new(0.35, 0.5, -0.05)),
        aimed(DVec3::new(0.6, 5.0, -0.3), DVec3::new(0.4, 0.3, -0.1)),
    ];
    for ray in rays {
        let hit = turned.cast_ray(&ray).map(|(_, hit)| hit);
        assert!(hit.is_some(), "the ray is aimed at the capsule");
        assert_eq!(hit, upright.cast_ray(&ray).map(|(_, hit)| hit));
    }
    let path = Segment::new(DVec3::new(-4.0, 1.4, 0.2), DVec3::new(4.0, 0.9, -0.3));
    let sphere = turned.sweep_sphere(&path, 0.3).map(|(_, hit)| hit);
    assert!(sphere.is_some(), "the sweep is aimed at the capsule");
    assert_eq!(sphere, upright.sweep_sphere(&path, 0.3).map(|(_, hit)| hit));
    let swept = turned.sweep_capsule(&path, 0.3, 0.4).map(|(_, hit)| hit);
    assert!(swept.is_some(), "the sweep is aimed at the capsule");
    assert_eq!(
        swept,
        upright.sweep_capsule(&path, 0.3, 0.4).map(|(_, hit)| hit)
    );

    let character = Capsule::new(DVec3::new(0.9, 0.6, 0.1), 0.5, 0.5);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    turned.capsule_penetrations_into(&character, None, &mut a);
    upright.capsule_penetrations_into(&character, None, &mut b);
    assert_eq!(a.len(), 1, "the character is inside the capsule");
    assert_eq!(a[0].1, b[0].1);
}

/// **A body's capsule turns with the body in the query world**: laid down by
/// the body's quarter turn, it is met at `x = 1.2`, and stood up again by an
/// unturned placement it is not.
#[test]
fn a_bodys_capsule_turns_with_the_body() {
    let entity =
        crcbl_ecs::Entity::from_bits((1u64 << 32) | 7).expect("generation 1 is never zero");
    let transform = Transform::new(DVec3::ZERO, lay_down());
    let mut phys = PhysicsSystem::new();
    phys.set_body(entity, RigidBody::new_kinematic());
    phys.set_transform(entity, transform);
    phys.set_collider(
        entity,
        &ColliderComponent::Capsule {
            offset: DVec3::ZERO,
            radius: RADIUS,
            half_height: HALF_HEIGHT,
            is_trigger: false,
        },
        &transform,
    );
    let down = Ray::new(DVec3::new(1.2, 5.0, 0.0), DVec3::NEG_Y);
    assert_eq!(phys.cast_ray(&down).map(|(hit, _)| hit), Some(entity));

    phys.set_transform(entity, Transform::IDENTITY);
    assert_eq!(phys.cast_ray(&down), None, "stood up again");
    phys.set_transform(entity, transform);
    assert_eq!(phys.cast_ray(&down).map(|(hit, _)| hit), Some(entity));
}
