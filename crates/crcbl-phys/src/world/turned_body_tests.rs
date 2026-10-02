//! A [`PhysicsSystem`] puts a turned body's shapes in the query world where
//! the contact pipeline puts them: offsets turned with the body.
//!
//! The body turns a quarter about `+Z`, which takes its `+Y` to the world's
//! `-X`: a shape offset 2 up the body sits 2 along the world's `-X`.

use core::f64::consts::FRAC_PI_2;

use glam::{DQuat, DVec3};

use crate::broadphase::Ray;
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::system::PhysicsSystem;

/// A quarter turn about `+Z`.
fn quarter() -> DQuat {
    crate::rotation_from_scaled_axis(DVec3::Z * FRAC_PI_2)
}

/// How closely a placed centre has to match the turned offset: the quarter
/// turn's sine and cosine are within rounding of 1 and 0.
const CLOSE: f64 = 1e-12;

/// A kinematic body at the origin, turned by `rotation`, carrying `component`.
fn body(rotation: DQuat, component: &ColliderComponent) -> (PhysicsSystem, crcbl_ecs::Entity) {
    let entity =
        crcbl_ecs::Entity::from_bits((1u64 << 32) | 3).expect("generation 1 is never zero");
    let transform = Transform::new(DVec3::ZERO, rotation);
    let mut phys = PhysicsSystem::new();
    phys.set_body(entity, RigidBody::new_kinematic());
    phys.set_transform(entity, transform);
    phys.set_collider(entity, component, &transform);
    (phys, entity)
}

/// **A sphere offset up a turned body is hit where the turn puts it**: at
/// `(-2, 0, 0)`, by a ray straight down onto its top, and not at `(0, 2, 0)`,
/// where the unturned offset would leave it.
#[test]
fn a_turned_bodys_sphere_is_hit_at_its_turned_offset() {
    let sphere = ColliderComponent::Sphere {
        offset: DVec3::new(0.0, 2.0, 0.0),
        radius: 0.5,
        is_trigger: false,
    };
    let (mut phys, entity) = body(quarter(), &sphere);

    let down = Ray::new(DVec3::new(-2.0, 5.0, 0.0), DVec3::NEG_Y);
    let (hit, at) = phys
        .cast_ray(&down)
        .expect("the sphere at its turned offset");
    assert_eq!(hit, entity);
    assert!((at.t - 4.5).abs() < CLOSE, "{at:?}");
    assert!((at.normal - DVec3::Y).length() < CLOSE, "{at:?}");

    let unturned = Ray::new(DVec3::new(5.0, 2.0, 0.0), DVec3::NEG_X);
    assert_eq!(
        phys.cast_ray(&unturned),
        None,
        "nothing at the unturned offset"
    );

    // And the step that moves the body places it there again.
    phys.step(1.0 / 60.0);
    assert!(phys.cast_ray(&down).is_some(), "placed again after a step");
}

/// **A capsule offset up a turned body is centred at its turned offset**.
#[test]
fn a_turned_bodys_capsule_is_centred_at_its_turned_offset() {
    let capsule = ColliderComponent::Capsule {
        offset: DVec3::new(0.0, 2.0, 0.0),
        radius: 0.25,
        half_height: 0.5,
        is_trigger: false,
    };
    let (phys, entity) = body(quarter(), &capsule);
    let bounds = phys
        .world()
        .aabb_of(phys.collider_of(entity).expect("a collider"))
        .expect("in the world");
    let centre = bounds.centre();
    assert!(
        (centre - DVec3::new(-2.0, 0.0, 0.0)).length() < CLOSE,
        "{bounds:?}"
    );
}
