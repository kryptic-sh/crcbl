//! Ignored pairs, run whole: [`PhysicsSystem::ignore_contacts`] keeps two
//! bodies from colliding with each other while each still collides with
//! everything else, and [`PhysicsSystem::restore_contacts`] undoes it.

use std::hash::Hasher;

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, ContactPairError, ContactSettings, MassProperties, PhysicsSystem, RigidBody,
    Transform,
};
use glam::DVec3;

const DT: f64 = 1.0 / 60.0;

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts, no gravity and sleep off.
fn weightless() -> PhysicsSystem {
    PhysicsSystem::with_contacts(ContactSettings {
        sleep: false,
        ..ContactSettings::DEFAULT
    })
}

/// A dynamic half-metre box of one kilogram at `at`.
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

/// **An ignored pair overlaps undisturbed, and still meets everything
/// else.** Two cubes overlapping by 0.4 m stay where they are while ignored;
/// a third overlapping one of them is pushed away; restored, the first two
/// push apart.
#[test]
fn an_ignored_pair_overlaps_until_restored() {
    let mut phys = weightless();
    let a = cube(&mut phys, 1, DVec3::ZERO);
    let b = cube(&mut phys, 2, DVec3::new(0.6, 0.0, 0.0));
    let c = cube(&mut phys, 3, DVec3::new(-0.6, 0.0, 0.0));
    assert_eq!(phys.ignore_contacts(a, b), Ok(true));
    assert!(phys.ignores_contacts(b, a), "the pair has no order");
    for _ in 0..30 {
        phys.step(DT);
    }
    assert!(speed(&phys, b) < 1.0e-9, "the ignored pair pushed b");
    assert!(speed(&phys, c) > 0.1, "c was not pushed off a");

    assert_eq!(phys.restore_contacts(b, a), Ok(true));
    assert!(!phys.ignores_contacts(a, b));
    for _ in 0..30 {
        phys.step(DT);
    }
    assert!(speed(&phys, b) > 0.1, "restored, the pair did not collide");
}

/// **Restoring a still pair lets it collide.** Nothing moves while the pair
/// is ignored, so only restoring it can make the broadphase look again.
#[test]
fn restoring_a_still_pair_lets_it_collide() {
    let mut phys = weightless();
    let a = cube(&mut phys, 1, DVec3::ZERO);
    let b = cube(&mut phys, 2, DVec3::new(0.6, 0.0, 0.0));
    assert_eq!(phys.ignore_contacts(a, b), Ok(true));
    for _ in 0..10 {
        phys.step(DT);
    }
    assert!(speed(&phys, a) + speed(&phys, b) < 1.0e-9, "they moved");
    assert_eq!(phys.restore_contacts(a, b), Ok(true));
    for _ in 0..10 {
        phys.step(DT);
    }
    assert!(
        speed(&phys, b) > 0.1,
        "restored, the still pair did not collide"
    );
}

/// **Ignoring a pair that already touches ends its contacts.** The cubes
/// begin pushing apart; once ignored, nothing more is added to b's motion.
#[test]
fn ignoring_a_touching_pair_ends_its_contacts() {
    let mut phys = weightless();
    let a = cube(&mut phys, 1, DVec3::ZERO);
    let b = cube(&mut phys, 2, DVec3::new(0.6, 0.0, 0.0));
    phys.step(DT);
    assert!(phys.contact_counters().touching > 0, "they touch first");
    assert_eq!(phys.ignore_contacts(a, b), Ok(true));
    let before = phys.body(b).expect("a body").velocity;
    phys.step(DT);
    assert_eq!(phys.contact_counters().touching, 0);
    let after = phys.body(b).expect("a body").velocity;
    assert!(
        (after - before).length() < 1.0e-12,
        "b was still pushed: {before} to {after}"
    );
}

/// Repeats report themselves, bad pairs are refused, and removing a body
/// forgets its pairs.
#[test]
fn pairs_are_checked_and_forgotten_with_their_bodies() {
    let mut phys = weightless();
    let a = cube(&mut phys, 1, DVec3::ZERO);
    let b = cube(&mut phys, 2, DVec3::new(3.0, 0.0, 0.0));
    assert_eq!(phys.restore_contacts(a, b), Ok(false));
    assert_eq!(phys.ignore_contacts(a, b), Ok(true));
    assert_eq!(phys.ignore_contacts(b, a), Ok(false));
    assert_eq!(
        phys.ignore_contacts(a, entity(9)),
        Err(ContactPairError::UnknownBody(entity(9)))
    );
    assert_eq!(phys.ignore_contacts(a, a), Err(ContactPairError::SameBody));

    phys.remove_entity(b);
    let mut never_ignored = weightless();
    cube(&mut never_ignored, 1, DVec3::ZERO);
    let gone = cube(&mut never_ignored, 2, DVec3::new(3.0, 0.0, 0.0));
    never_ignored.remove_entity(gone);
    assert_eq!(
        hash(&phys),
        hash(&never_ignored),
        "a pair outlived its body"
    );
    let b = cube(&mut phys, 2, DVec3::new(0.6, 0.0, 0.0));
    assert!(
        !phys.ignores_contacts(a, b),
        "the removed body's pair stayed"
    );
    for _ in 0..30 {
        phys.step(DT);
    }
    assert!(speed(&phys, b) > 0.1, "the re-added body was still ignored");
}

/// The pairs are simulation state: the hash sees them, and not the order
/// they were named in.
#[test]
fn the_hash_covers_ignored_pairs() {
    let build = |ignore: Option<bool>| {
        let mut phys = weightless();
        let a = cube(&mut phys, 1, DVec3::ZERO);
        let b = cube(&mut phys, 2, DVec3::new(3.0, 0.0, 0.0));
        match ignore {
            Some(true) => assert_eq!(phys.ignore_contacts(a, b), Ok(true)),
            Some(false) => assert_eq!(phys.ignore_contacts(b, a), Ok(true)),
            None => {}
        }
        hash(&phys)
    };
    assert_ne!(build(None), build(Some(true)));
    assert_eq!(build(Some(true)), build(Some(false)));
}
