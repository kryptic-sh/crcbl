use glam::DVec3;

use crate::broadphase::Ray;
use crate::collider::{BoxCollider, Capsule, Sphere};

use super::{PhysicsWorld, QueryScratch};

fn ray() -> Ray {
    Ray::new(DVec3::ZERO, DVec3::X)
}

#[test]
fn ray_excluding_a_sphere_keeps_the_wall_behind_it() {
    let mut world = PhysicsWorld::new();
    let own = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 0.5));
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::new(0.5, 5.0, 5.0),
    ));

    assert_eq!(world.cast_ray(&ray()).map(|(id, _)| id), Some(own));
    assert_eq!(
        world
            .cast_ray_excluding(&ray(), Some(own))
            .map(|(id, _)| id),
        Some(wall),
        "excluding the closest sphere must retain the wall behind it"
    );
}

#[test]
fn ray_excluding_a_capsule_keeps_the_wall_behind_it() {
    let mut world = PhysicsWorld::new();
    let own = world.add_capsule(Capsule::new(DVec3::new(2.0, 0.0, 0.0), 0.5, 1.0));
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::new(0.5, 5.0, 5.0),
    ));

    assert_eq!(world.cast_ray(&ray()).map(|(id, _)| id), Some(own));
    assert_eq!(
        world
            .cast_ray_excluding(&ray(), Some(own))
            .map(|(id, _)| id),
        Some(wall),
        "excluding the closest capsule must retain the wall behind it"
    );
}

#[test]
fn shared_and_mutable_ray_exclusion_agree() {
    let mut world = PhysicsWorld::new();
    let own = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 0.5));
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::new(0.5, 5.0, 5.0),
    ));
    let mut scratch = QueryScratch::new();

    let shared = world
        .overlap_queries()
        .cast_ray_excluding(&ray(), Some(own), &mut scratch);
    let mutable = world.cast_ray_excluding(&ray(), Some(own));

    assert_eq!(shared, mutable);
    assert_eq!(mutable.map(|(id, _)| id), Some(wall));
}

#[test]
fn a_stale_ray_exclusion_does_not_skip_a_recycled_collider() {
    let mut world = PhysicsWorld::new();
    let stale = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 0.5));
    assert!(world.remove(stale));
    let recycled = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 0.5));
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::new(0.5, 5.0, 5.0),
    ));

    assert_eq!(recycled.index(), stale.index(), "the slot must be recycled");
    assert_ne!(recycled, stale, "the generation must change");
    assert_eq!(
        world
            .cast_ray_excluding(&ray(), Some(stale))
            .map(|(id, _)| id),
        Some(recycled),
        "a stale id must not exclude the recycled collider and reveal the wall"
    );
    assert_eq!(
        world
            .cast_ray_excluding(&ray(), Some(recycled))
            .map(|(id, _)| id),
        Some(wall)
    );
}

#[test]
fn ray_exclusion_preserves_trigger_and_none_semantics() {
    let mut world = PhysicsWorld::new();
    let trigger = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 0.5));
    assert!(world.set_trigger(trigger, true));
    let closest = world.add_sphere(Sphere::new(DVec3::new(4.0, 0.0, 0.0), 0.5));

    let ordinary = world.cast_ray(&ray());
    assert_eq!(ordinary.map(|(id, _)| id), Some(closest));
    assert_eq!(
        world.cast_ray_excluding(&ray(), None),
        ordinary,
        "None must delegate to ordinary ray-cast semantics"
    );
    assert_eq!(
        world
            .cast_ray_excluding(&ray(), Some(trigger))
            .map(|(id, _)| id),
        Some(closest),
        "excluding a trigger must not change the already-transparent trigger behavior"
    );
    assert!(
        world.is_trigger(trigger),
        "the query must not mutate trigger state"
    );
}
