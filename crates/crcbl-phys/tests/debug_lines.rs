//! [`PhysicsSystem::debug_lines`]: each collider's box, telling sleeping
//! bodies and islands apart, and each contact point's normal with its impulse
//! and feature.

use crcbl_ecs::Entity;
use crcbl_phys::{
    CONTACT_NORMAL_LENGTH, ColliderComponent, ContactSettings, GravityForce, MassProperties,
    PhysicsDebugLine, PhysicsSystem, RigidBody, SurfaceMaterial, Transform,
};
use glam::DVec3;

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// Each crate's mass, in kilograms.
const MASS: f64 = 10.0;

/// Each box's half-extents, in metres.
const HALF: DVec3 = DVec3::splat(0.25);

/// How near a drawn corner must lie to the box placed there, in metres: what
/// a resting body sinks and slides in a few ticks.
const NEAR: f64 = 1e-3;

/// Edges in a box.
const EDGES: usize = 12;

/// How many ticks the crates are given to fall asleep.
const SETTLE_TICKS: usize = 300;

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts at the defaults, Earth gravity and a floor at
/// `y = 0`.
fn system() -> PhysicsSystem {
    let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    physics.add_force_provider(Box::new(GravityForce::EARTH));
    physics.add_plane(DVec3::Y, 0.0, SurfaceMaterial::DEFAULT);
    physics
}

/// A box of half-extents [`HALF`] at `at`, with `body` or as a static.
fn place(physics: &mut PhysicsSystem, index: u32, at: DVec3, body: Option<RigidBody>) {
    let e = entity(index);
    if let Some(body) = body {
        physics.set_body(e, body);
    }
    let transform = Transform::from_position(at);
    physics.set_transform(e, transform);
    physics.set_collider(
        e,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: HALF,
            is_trigger: false,
        },
        &transform,
    );
}

/// A dynamic crate of [`MASS`] at `at`.
fn crate_(physics: &mut PhysicsSystem, index: u32, at: DVec3) {
    let inertia = MassProperties::cuboid(MASS, HALF, DVec3::ZERO).inertia;
    let body = RigidBody::new_dynamic(MASS).with_inertia(inertia);
    place(physics, index, at, Some(body));
}

/// Every segment the view hands out.
fn lines(physics: &PhysicsSystem) -> Vec<(DVec3, DVec3, PhysicsDebugLine)> {
    let mut lines = Vec::new();
    physics.debug_lines(&mut |from, to, kind| lines.push((from, to, kind)));
    lines
}

/// The tags of the box edges drawn for the box of half-extents [`HALF`]
/// about `centre`, each end within [`NEAR`] of one of its corners and as long
/// as an edge of it, the box being a cube.
fn boxed_at(drawn: &[(DVec3, DVec3, PhysicsDebugLine)], centre: DVec3) -> Vec<PhysicsDebugLine> {
    let corner = |point: DVec3| (point - centre).abs().distance(HALF) < NEAR;
    drawn
        .iter()
        .filter(|(from, to, kind)| {
            let edge = (from.distance(*to) - 2.0 * HALF.x).abs() < NEAR;
            matches!(kind, PhysicsDebugLine::Bounds { .. }) && corner(*from) && corner(*to) && edge
        })
        .map(|&(_, _, kind)| kind)
        .collect()
}

/// **Each collider's box is drawn, the bodies the solver steps together
/// sharing an island, an entity with no body in none, and each body marked
/// asleep once it settles.** A crate on another shares its island; one apart
/// has its own; a static slab is in none and never sleeps.
#[test]
fn each_collider_is_boxed_telling_islands_and_sleep_apart() {
    let mut physics = system();
    let bottom = DVec3::new(0.0, HALF.y, 0.0);
    // Off centre, so no edge of its box is one of the bottom box's too.
    let top = DVec3::new(HALF.x / 2.0, 3.0 * HALF.y, 0.0);
    let apart = DVec3::new(3.0, HALF.y, 0.0);
    let slab = DVec3::new(-3.0, HALF.y, 0.0);
    crate_(&mut physics, 0, bottom);
    crate_(&mut physics, 1, top);
    crate_(&mut physics, 2, apart);
    place(&mut physics, 3, slab, None);
    physics.step(DT);

    let drawn = lines(&physics);
    let island = |centre| {
        let kinds = boxed_at(&drawn, centre);
        assert_eq!(kinds.len(), EDGES, "the box at {centre} drew {kinds:?}");
        assert!(kinds.windows(2).all(|pair| pair[0] == pair[1]));
        match kinds[0] {
            PhysicsDebugLine::Bounds { sleeping, island } => {
                assert!(!sleeping, "the box at {centre} sleeps after one tick");
                island
            }
            PhysicsDebugLine::Contact { .. } => unreachable!("filtered to bounds"),
        }
    };
    let stack = island(bottom);
    assert!(stack.is_some(), "a stepped crate in no island");
    assert_eq!(island(top), stack, "the stack in two islands");
    assert!(island(apart).is_some_and(|own| Some(own) != stack));
    assert_eq!(island(slab), None, "a static slab in an island");

    let settled = (0..SETTLE_TICKS).any(|_| {
        physics.step(DT);
        physics.contact_counters().bodies == 0
    });
    assert!(settled, "the crates never slept");
    let drawn = lines(&physics);
    for centre in [bottom, top, apart] {
        let kinds = boxed_at(&drawn, centre);
        assert_eq!(kinds.len(), EDGES, "the box at {centre} drew {kinds:?}");
        assert!(
            kinds
                .iter()
                .all(|kind| matches!(kind, PhysicsDebugLine::Bounds { sleeping: true, .. })),
            "the crate at {centre} not drawn asleep"
        );
    }
    assert!(
        boxed_at(&drawn, slab).iter().all(|kind| matches!(
            kind,
            PhysicsDebugLine::Bounds {
                sleeping: false,
                ..
            }
        )),
        "the slab drawn asleep"
    );
}

/// A contact segment drawn: its foot, its direction and its tag's fields.
#[derive(Debug)]
struct Drawn {
    foot: DVec3,
    normal: DVec3,
    touching: bool,
    impulse: f64,
    feature: u32,
}

/// The contact segments drawn.
fn contacts(physics: &PhysicsSystem) -> Vec<Drawn> {
    lines(physics)
        .into_iter()
        .filter_map(|(from, to, kind)| match kind {
            PhysicsDebugLine::Contact {
                touching,
                impulse,
                feature,
            } => Some(Drawn {
                foot: from,
                normal: to - from,
                touching,
                impulse,
                feature,
            }),
            PhysicsDebugLine::Bounds { .. } => None,
        })
        .collect()
}

/// **Each contact point draws its normal, touching, with the impulse that
/// holds its body up and a feature that stays put while it rests**: a crate
/// on the floor stands on its four bottom corners, and their impulses add up
/// to its weight over the last substep.
#[test]
fn each_contact_point_draws_its_normal_impulse_and_feature() {
    let mut physics = system();
    crate_(&mut physics, 0, DVec3::new(0.0, HALF.y, 0.0));
    physics.step(DT);
    let first = contacts(&physics);
    assert_eq!(first.len(), 4, "{first:?}");
    for contact in &first {
        assert!(contact.touching, "a resting corner not touching");
        let foot = contact.foot;
        assert!(foot.y.abs() < NEAR, "a contact off the floor at {foot}");
        assert!(
            (foot.x.abs() - HALF.x).abs() < NEAR && (foot.z.abs() - HALF.z).abs() < NEAR,
            "a contact off the corners at {foot}"
        );
        let normal = contact.normal;
        assert!(
            (normal.length() - CONTACT_NORMAL_LENGTH).abs() < 1e-12
                && normal.normalize().abs().distance(DVec3::Y) < 1e-9,
            "the normal drawn as {normal}"
        );
        let [report] = physics.contacts()[..] else {
            panic!("the crate meets more than the floor");
        };
        assert!(
            normal.normalize().distance(report.manifold.normal) < 1e-9,
            "drawn along {normal}, not the manifold's {}",
            report.manifold.normal
        );
    }
    let substeps = f64::from(ContactSettings::DEFAULT.substeps);
    let weight = MASS * -GravityForce::EARTH.acceleration.y * DT / substeps;
    let held: f64 = first.iter().map(|contact| contact.impulse).sum();
    assert!(
        (held - weight).abs() < weight * 0.05,
        "the corners hold {held}, not the substep's weight {weight}"
    );
    let features = |contacts: &[Drawn]| {
        let mut features: Vec<u32> = contacts.iter().map(|contact| contact.feature).collect();
        features.sort_unstable();
        features
    };
    physics.step(DT);
    let second = contacts(&physics);
    assert_eq!(features(&second), features(&first), "the features moved");
    assert!(
        features(&first).windows(2).all(|pair| pair[0] != pair[1]),
        "two corners share a feature"
    );
}

/// **A point the solver keeps before the surfaces meet is drawn as not
/// touching**: a crate hovering inside the speculative distance.
#[test]
fn a_speculative_point_is_drawn_not_touching() {
    let mut physics = system();
    let gap = ContactSettings::DEFAULT.speculative_distance / 2.0;
    crate_(&mut physics, 0, DVec3::new(0.0, HALF.y + gap, 0.0));
    physics.step(DT);
    let drawn = contacts(&physics);
    assert!(
        !drawn.is_empty(),
        "no contact inside the speculative distance"
    );
    assert!(drawn.iter().all(|contact| !contact.touching), "{drawn:?}");
}

/// **A system without contacts draws each box in no island and no contact.**
#[test]
fn without_contacts_only_boxes_are_drawn() {
    let mut physics = PhysicsSystem::new();
    let at = DVec3::new(0.0, 2.0, 0.0);
    crate_(&mut physics, 0, at);
    physics.step(DT);
    let drawn = lines(&physics);
    let kinds = boxed_at(&drawn, at);
    assert_eq!(kinds.len(), EDGES);
    assert_eq!(drawn.len(), EDGES, "more than the box drawn");
    let alone = PhysicsDebugLine::Bounds {
        sleeping: false,
        island: None,
    };
    assert!(kinds.iter().all(|kind| *kind == alone), "{kinds:?}");
}
