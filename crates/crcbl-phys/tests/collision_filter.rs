//! The solver's collision filter, run whole: bodies in categories each
//! other's masks leave out pass through one another, and every rule a filter
//! changes takes effect on the next step.

use std::hash::Hasher;

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, CollisionFilter, ContactSettings, GravityForce, MassProperties,
    PhysicsSystem, RigidBody, SurfaceMaterial, Transform,
};
use glam::DVec3;

const DT: f64 = 1.0 / 60.0;

/// The categories these tests sort bodies into.
const ITEMS: u32 = 0b10;
const CORPSES: u32 = 0b100;

/// A corpse body: in its own category, colliding with everything but items.
const CORPSE: CollisionFilter = CollisionFilter {
    category: CORPSES,
    mask: !ITEMS,
};
/// A loose item: colliding with everything but corpses.
const ITEM: CollisionFilter = CollisionFilter {
    category: ITEMS,
    mask: !CORPSES,
};

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

fn weightless() -> PhysicsSystem {
    PhysicsSystem::with_contacts(ContactSettings {
        sleep: false,
        ..ContactSettings::DEFAULT
    })
}

/// A dynamic half-metre cube of one kilogram at `at`.
fn cube(phys: &mut PhysicsSystem, index: u32, at: DVec3) -> Entity {
    let e = entity(index);
    let half = DVec3::splat(0.5);
    let inertia = MassProperties::cuboid(1.0, half, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(1.0).with_inertia(inertia));
    let transform = Transform::from_position(at);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: half,
            is_trigger: false,
        },
        &transform,
    );
    e
}

fn speed(phys: &PhysicsSystem, e: Entity) -> f64 {
    phys.body(e).expect("a body").velocity.length()
}

fn hash(phys: &PhysicsSystem) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    phys.hash_state(&mut hasher);
    hasher.finish()
}

/// **A corpse and an item overlap undisturbed, and each still meets the
/// rest.** A default cube overlapping the corpse alone pushes it into the
/// item, which never moves.
#[test]
fn excluded_categories_pass_through_each_other() {
    assert!(!CORPSE.collides_with(ITEM));
    assert!(CORPSE.collides_with(CollisionFilter::DEFAULT));
    let mut phys = weightless();
    let corpse = cube(&mut phys, 1, DVec3::ZERO);
    let item = cube(&mut phys, 2, DVec3::new(0.6, 0.0, 0.0));
    let wall = cube(&mut phys, 3, DVec3::new(-0.6, 0.0, 0.0));
    assert!(phys.set_collision_filter(corpse, CORPSE));
    assert!(phys.set_collision_filter(item, ITEM));
    assert_eq!(phys.collision_filter(item), Some(ITEM));
    for _ in 0..30 {
        phys.step(DT);
    }
    assert!(
        speed(&phys, wall) > 0.1,
        "the default cube was not pushed off"
    );
    assert!(speed(&phys, corpse) > 0.1, "the corpse was not pushed");
    assert!(speed(&phys, item) < 1e-9, "the corpse pushed the item");
}

/// **A filter changed on a touching pair ends its contacts**, and changed
/// back on a still pair lets it collide again.
#[test]
fn changing_a_filter_ends_and_restores_contacts() {
    let mut phys = weightless();
    let a = cube(&mut phys, 1, DVec3::ZERO);
    let b = cube(&mut phys, 2, DVec3::new(0.6, 0.0, 0.0));
    phys.step(DT);
    assert!(phys.contact_counters().touching > 0, "they touch first");
    assert!(phys.set_collision_filter(a, CORPSE));
    assert!(phys.set_collision_filter(b, ITEM));
    let before = phys.body(b).unwrap().velocity;
    phys.step(DT);
    assert_eq!(phys.contact_counters().touching, 0);
    assert!((phys.body(b).unwrap().velocity - before).length() < 1e-12);

    // Stop them, let them sit filtered, then let them collide.
    for e in [a, b] {
        let body = phys.body_mut(e).unwrap();
        body.velocity = DVec3::ZERO;
        body.angular_velocity = DVec3::ZERO;
    }
    for _ in 0..10 {
        phys.step(DT);
    }
    assert!(
        speed(&phys, a) + speed(&phys, b) < 1e-9,
        "a filtered pair moved"
    );
    assert!(phys.set_collision_filter(b, CollisionFilter::DEFAULT));
    for _ in 0..10 {
        phys.step(DT);
    }
    assert!(speed(&phys, b) > 0.1, "the restored pair did not collide");
}

/// **Static bodies are filtered too, and planes are not.** An item falls
/// through a slab in the corpse category but lands on a plane below it.
#[test]
fn statics_obey_filters_and_planes_always_collide() {
    let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_plane(DVec3::Y, -5.0, SurfaceMaterial::DEFAULT);
    let slab = entity(9);
    let at = Transform::from_position(DVec3::new(0.0, -0.5, 0.0));
    phys.set_transform(slab, at);
    phys.set_collider(
        slab,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::new(5.0, 0.5, 5.0),
            is_trigger: false,
        },
        &at,
    );
    assert!(phys.set_collision_filter(slab, CORPSE));
    let item = cube(&mut phys, 1, DVec3::new(0.0, 2.0, 0.0));
    assert!(phys.set_collision_filter(item, ITEM));
    for _ in 0..240 {
        phys.step(DT);
    }
    let y = phys.transform(item).unwrap().position.y;
    assert!(
        (y - -4.5).abs() < 0.05,
        "the item rests at {y}, not on the plane"
    );
}

/// Unknown entities are refused, and the hash sees a filter.
#[test]
fn filters_are_checked_and_hashed() {
    let mut phys = weightless();
    assert!(!phys.set_collision_filter(entity(9), CORPSE));
    assert_eq!(phys.collision_filter(entity(9)), None);
    let a = cube(&mut phys, 1, DVec3::ZERO);
    assert_eq!(phys.collision_filter(a), Some(CollisionFilter::DEFAULT));
    let plain = hash(&phys);
    assert!(phys.set_collision_filter(a, CORPSE));
    assert_ne!(hash(&phys), plain);
    assert!(phys.set_collision_filter(a, CollisionFilter::DEFAULT));
    assert_eq!(hash(&phys), plain, "the default filter hashes as none");
    let in_category = |category: u32, phys: &mut PhysicsSystem| {
        let filter = CollisionFilter {
            category,
            mask: u32::MAX,
        };
        assert!(phys.set_collision_filter(a, filter));
        hash(phys)
    };
    let items = in_category(ITEMS, &mut phys);
    assert_ne!(
        items,
        in_category(CORPSES, &mut phys),
        "the category is not hashed"
    );
}
