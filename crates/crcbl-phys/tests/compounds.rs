//! Compound bodies, run whole: several boxes fixed in one body's frame,
//! colliding part by part, weighed from their parts, and sleeping.
//!
//! As in `stacking.rs` and `settling.rs`, every bound was measured before it
//! was written down, and each says what it was measured at.

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    Aabb, ColliderComponent, CompoundShape, ContactBody, ContactReport, ContactSettings,
    GravityForce, PhysicsSystem, SurfaceMaterial, Transform,
};
use glam::{DQuat, DVec3};

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// Box2D's default friction, and no bounce.
const CRATE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);

/// The default settings with sleep off, for a test that watches contacts
/// being rebuilt tick after tick: asleep, nothing is rebuilt.
const AWAKE: ContactSettings = ContactSettings {
    sleep: false,
    ..ContactSettings::DEFAULT
};

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts at `settings`, Earth gravity and, if `floor`, a
/// plane at `y = 0`.
fn system(settings: ContactSettings, floor: bool) -> PhysicsSystem {
    let mut phys = PhysicsSystem::with_contacts(settings);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    if floor {
        phys.add_plane(DVec3::Y, 0.0, CRATE);
    }
    phys
}

/// A dynamic compound of `parts` at the default density, its shape's own
/// origin placed at `origin` turned by `rotation`: the body itself sits at
/// its centre of mass, as a game keeping its own origin would place it.
fn compound(
    phys: &mut PhysicsSystem,
    index: u32,
    parts: &[Aabb],
    origin: DVec3,
    rotation: DQuat,
) -> Entity {
    let e = entity(index);
    let shape = CompoundShape::from_aabbs(parts).expect("valid parts");
    let built = shape.dynamic_body(CompoundShape::DEFAULT_DENSITY);
    phys.set_body(e, built.body);
    let transform = Transform::new(origin + rotation * built.centre_of_mass, rotation);
    phys.set_transform(e, transform);
    phys.set_collider(e, &built.collider, &transform);
    phys.set_material(e, CRATE);
    e
}

/// A static body whose collider is a compound of `parts`, its origin at
/// `origin`.
fn static_compound(phys: &mut PhysicsSystem, index: u32, parts: &[Aabb], origin: DVec3) -> Entity {
    let e = entity(index);
    let transform = Transform::from_position(origin);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape: CompoundShape::from_aabbs(parts).expect("valid parts"),
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, CRATE);
    e
}

/// A static box.
fn slab(phys: &mut PhysicsSystem, index: u32, centre: DVec3, half: DVec3) -> Entity {
    let e = entity(index);
    let transform = Transform::from_position(centre);
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
    phys.set_material(e, CRATE);
    e
}

/// The touching contacts of `e`, from `e`'s side: its part, and what it
/// touches.
fn touching(phys: &PhysicsSystem, e: Entity) -> Vec<(usize, ContactBody, ContactReport)> {
    phys.contacts()
        .into_iter()
        .filter(|report| !report.manifold.points().is_empty())
        .filter_map(|report| {
            if report.a == ContactBody::Entity(e) {
                Some((report.part_a, report.b, report))
            } else if report.b == ContactBody::Entity(e) {
                Some((report.part_b, report.a, report))
            } else {
                None
            }
        })
        .collect()
}

/// Where the shape origin of the body at `e` is, for a body placed by
/// [`compound`]: its position less its turned centre of mass.
fn origin_of(phys: &PhysicsSystem, e: Entity, parts: &[Aabb]) -> DVec3 {
    let com = CompoundShape::from_aabbs(parts)
        .unwrap()
        .mass_properties(1.0)
        .centre_of_mass;
    let transform = phys.transform(e).expect("registered");
    transform.position - transform.rotation * com
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// An L lying flat: a bar along `x` and a bar along `z`, each 20 cm square
/// in section, meeting at the origin corner. Nothing is under the corner the
/// two arms do not reach, `(1, ·, 1)`, which a single box around the parts
/// would stand on.
fn flat_l() -> [Aabb; 2] {
    [
        Aabb::new(DVec3::ZERO, DVec3::new(1.0, 0.2, 0.2)),
        Aabb::new(DVec3::new(0.0, 0.0, 0.2), DVec3::new(0.2, 0.2, 1.0)),
    ]
}

/// EW's TOZ-34, the break-action shotgun that is its long gun, boxed the way
/// `asset_placement::collision_parts` boxes a model — one box per mesh
/// primitive, in the model's rest frame — for its six largest primitives:
/// the barrels, the fore-end, the receiver, the stock, the grip and the butt.
///
/// Read off `assets/models/weapons/toz34.gltf` in the EW repository on
/// 2026-09-23: each primitive's `POSITION` accessor bounds moved by its node's
/// translation (the barrels and fore-end also by the hinge node above them),
/// rounded to the millimetre. The seven small primitives — extractors,
/// triggers, the locking lever, the breech plate and a fore-end filler — are
/// left out, since each lies inside or against a part here. The parts overlap
/// as a model's meshes do: the receiver runs into the stock and the grip into
/// both.
fn toz34() -> [Aabb; 6] {
    let part = |x: f64, y: [f64; 2], z: [f64; 2]| {
        Aabb::new(DVec3::new(-x, y[0], z[0]), DVec3::new(x, y[1], z[1]))
    };
    [
        part(0.015, [0.039, 0.100], [-0.575, 0.117]),
        part(0.014, [0.034, 0.083], [-0.209, 0.058]),
        part(0.013, [-0.001, 0.095], [0.046, 0.266]),
        part(0.014, [-0.097, 0.081], [0.141, 0.565]),
        part(0.011, [-0.026, 0.048], [0.234, 0.335]),
        part(0.013, [-0.100, 0.024], [0.553, 0.575]),
    ]
}

// ---------------------------------------------------------------------------
// Resting on the right parts
// ---------------------------------------------------------------------------

/// **An L resting on the floor stands on its two arms and on nothing else.**
///
/// Placed at `(2, 0.001, 3)`, a millimetre up, it lands in a tick and rests.
/// Each arm is its own contact with the floor, a box against a plane: its
/// four bottom corners, each point midway between the corner and the floor —
/// at `x, z` the corner's own. By hand, the long arm's corners are
/// `x ∈ {2, 3}, z ∈ {3, 3.2}` and the short arm's `x ∈ {2, 2.2},
/// z ∈ {3.2, 4}`; no point is at `(3, ·, 4)`, the corner a box around the L
/// would put one at. Measured on 2026-09-23 after a second: every point within
/// 14 µm of its corner in `x` and `z` — the landing's slide — and at most
/// 0.03 mm below the floor.
#[test]
fn an_l_rests_on_the_floor_on_both_arms_at_their_corners() {
    let mut phys = system(ContactSettings::DEFAULT, true);
    let origin = DVec3::new(2.0, 0.001, 3.0);
    let l = compound(&mut phys, 0, &flat_l(), origin, DQuat::IDENTITY);
    for _ in 0..60 {
        phys.step(DT);
    }

    let contacts = touching(&phys, l);
    assert_eq!(contacts.len(), 2, "one contact per arm: {contacts:#?}");
    let corners = [
        [(2.0, 3.0), (3.0, 3.0), (2.0, 3.2), (3.0, 3.2)],
        [(2.0, 3.2), (2.2, 3.2), (2.0, 4.0), (2.2, 4.0)],
    ];
    for (part, other, report) in &contacts {
        assert!(
            matches!(other, ContactBody::Plane(_)),
            "part {part} touches {other:?}"
        );
        let points = report.manifold.points();
        assert_eq!(points.len(), 4, "part {part}: {points:#?}");
        for &(x, z) in &corners[*part] {
            let nearest = points
                .iter()
                .map(|p| (DVec3::new(p.point.x, 0.0, p.point.z) - DVec3::new(x, 0.0, z)).length())
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest < 1e-4,
                "part {part}: no point at ({x}, {z}), nearest {nearest}: {points:#?}"
            );
        }
        for p in points {
            assert!(
                p.point.y.abs() < 1e-4,
                "part {part}: a point at y = {}",
                p.point.y
            );
        }
    }
    let nowhere = phys.contacts().into_iter().flat_map(|report| {
        report
            .manifold
            .points()
            .iter()
            .map(|p| p.point)
            .collect::<Vec<_>>()
    });
    for point in nowhere {
        assert!(
            (DVec3::new(point.x, 0.0, point.z) - DVec3::new(3.0, 0.0, 4.0)).length() > 0.5,
            "a point at {point:?}, under no arm"
        );
    }
}

// ---------------------------------------------------------------------------
// Part against part
// ---------------------------------------------------------------------------

/// A U standing on the floor: a base and two posts with a 60 cm gap between
/// them, 50 cm deep.
fn u_shape() -> [Aabb; 3] {
    [
        Aabb::new(DVec3::new(-0.5, 0.0, -0.2), DVec3::new(0.5, 0.1, 0.2)),
        Aabb::new(DVec3::new(-0.5, 0.1, -0.2), DVec3::new(-0.3, 0.6, 0.2)),
        Aabb::new(DVec3::new(0.3, 0.1, -0.2), DVec3::new(0.5, 0.6, 0.2)),
    ]
}

/// A T upside down: a crossbar a metre wide, and a blade 40 cm long and
/// `blade` wide hanging under its middle.
fn peg(blade: f64) -> [Aabb; 2] {
    [
        Aabb::new(DVec3::new(-0.5, 0.0, -0.1), DVec3::new(0.5, 0.1, 0.1)),
        Aabb::new(
            DVec3::new(-0.5 * blade, -0.4, -0.1),
            DVec3::new(0.5 * blade, 0.0, 0.1),
        ),
    ]
}

/// The peg dropped over the U, blade first, and where its crossbar's bottom
/// came to rest after three seconds, with the contacts it rests on.
fn drop_peg(blade: f64) -> (f64, Vec<(usize, ContactBody, usize)>) {
    let mut phys = system(ContactSettings::DEFAULT, true);
    let u = compound(&mut phys, 0, &u_shape(), DVec3::ZERO, DQuat::IDENTITY);
    let peg_parts = peg(blade);
    // Blade tip 5 cm above the posts' tops.
    let p = compound(
        &mut phys,
        1,
        &peg_parts,
        DVec3::new(0.0, 1.05, 0.0),
        DQuat::IDENTITY,
    );
    for _ in 0..180 {
        phys.step(DT);
    }
    let crossbar_bottom = origin_of(&phys, p, &peg_parts).y;
    let contacts = touching(&phys, p)
        .into_iter()
        .map(|(part, other, report)| {
            let other_part = if report.a == other {
                report.part_a
            } else {
                report.part_b
            };
            assert_eq!(other, ContactBody::Entity(u), "the peg touches only the U");
            (part, other, other_part)
        })
        .collect();
    (crossbar_bottom, contacts)
}

/// **Two compounds collide part against part: a blade drops into a real gap,
/// and one too wide for it does not.**
///
/// Both bodies are dynamic, and the U stands on the floor. A 10 cm blade
/// falls into the U's 60 cm gap, touching neither post nor the base — its tip
/// stops 10 cm above the base — and the crossbar comes down on both posts'
/// tops at `y = 0.6`. An 80 cm blade is wider than the gap: it lands on the
/// posts itself, and the crossbar rests 40 cm higher, at `y = 1.0`. A box
/// around each body would stop both at the second height. Measured on
/// 2026-09-23: the crossbar at 0.5998 m and at 0.9998 m.
#[test]
fn a_thin_part_drops_into_a_real_gap_and_a_wide_one_does_not() {
    let (narrow, contacts) = drop_peg(0.1);
    assert!(
        (narrow - 0.6).abs() < 2e-3,
        "the crossbar came to rest at {narrow}"
    );
    let mut rests_on: Vec<(usize, usize)> = contacts
        .iter()
        .map(|&(part, _, other)| (part, other))
        .collect();
    rests_on.sort_unstable();
    assert_eq!(
        rests_on,
        [(0, 1), (0, 2)],
        "the crossbar (part 0) on both posts (parts 1 and 2), the blade on nothing"
    );

    let (wide, contacts) = drop_peg(0.8);
    assert!(
        (wide - 1.0).abs() < 2e-3,
        "the crossbar came to rest at {wide}"
    );
    let mut rests_on: Vec<(usize, usize)> = contacts
        .iter()
        .map(|&(part, _, other)| (part, other))
        .collect();
    rests_on.sort_unstable();
    assert_eq!(
        rests_on,
        [(1, 1), (1, 2)],
        "the blade (part 1) on both posts"
    );
}

/// **A static compound is solid part by part too**: world geometry built as
/// a compound — the U, held still — lets the blade through its gap exactly
/// as the dynamic one does. Measured on 2026-09-23: the crossbar at
/// 0.59997 m.
#[test]
fn a_static_compound_collides_part_by_part() {
    let mut phys = system(ContactSettings::DEFAULT, false);
    static_compound(&mut phys, 0, &u_shape(), DVec3::ZERO);
    let parts = peg(0.1);
    let p = compound(
        &mut phys,
        1,
        &parts,
        DVec3::new(0.0, 1.05, 0.0),
        DQuat::IDENTITY,
    );
    for _ in 0..180 {
        phys.step(DT);
    }
    let bottom = origin_of(&phys, p, &parts).y;
    assert!(
        (bottom - 0.6).abs() < 2e-3,
        "the crossbar came to rest at {bottom}"
    );
}

// ---------------------------------------------------------------------------
// Warm starting
// ---------------------------------------------------------------------------

/// **An L resting on the floor keeps all eight of its feature ids, tick after
/// tick**: each arm's contact is its own part pair's, so its ids and its
/// warm start carry over as a lone box's do, and neither contact begins or
/// ends once it has landed. Measured on 2026-09-23: from the second tick on,
/// 2 contacts, 8 points and 8 persisted every tick.
#[test]
fn a_resting_compound_keeps_its_feature_ids() {
    let mut phys = system(AWAKE, true);
    compound(
        &mut phys,
        0,
        &flat_l(),
        DVec3::new(0.0, 0.001, 0.0),
        DQuat::IDENTITY,
    );
    let (mut points, mut persisted) = (0, 0);
    for tick in 0..240 {
        phys.step(DT);
        let counters = phys.contact_counters();
        if tick >= 1 {
            assert_eq!(counters.touching, 2, "tick {tick}: {counters:?}");
            assert_eq!(counters.points, 8, "tick {tick}: {counters:?}");
            assert_eq!(
                counters.begun + counters.ended,
                0,
                "tick {tick}: {counters:?}"
            );
            points += counters.points;
            persisted += counters.persisted;
        }
    }
    assert_eq!(
        persisted, points,
        "{persisted} of {points} points persisted"
    );
}

// ---------------------------------------------------------------------------
// Sleep
// ---------------------------------------------------------------------------

/// **A shotgun dropped tumbling onto a static slab settles on its parts and
/// sleeps.**
///
/// The TOZ-34 fixture falls from 30 cm, turned about all three axes and
/// spinning, onto a static box — world geometry as EW has it, not a plane.
/// It comes to rest lying on its side, as a long gun does, its lowest part
/// on the slab, and then sleeps. Measured on 2026-09-23: asleep at tick 47,
/// half a second after it landed, its lowest corner 0.06 mm into the slab and
/// its thin `x` axis 0.99998 of the way to vertical.
#[test]
fn a_dropped_shotgun_settles_on_its_parts_and_sleeps() {
    let mut phys = system(ContactSettings::DEFAULT, false);
    slab(
        &mut phys,
        100,
        DVec3::new(0.0, -0.5, 0.0),
        DVec3::new(2.0, 0.5, 2.0),
    );
    let parts = toz34();
    let turn = crcbl_phys::rotation_from_scaled_axis(DVec3::new(0.4, 0.9, 1.3));
    let gun = compound(&mut phys, 0, &parts, DVec3::new(0.0, 0.3, 0.0), turn);
    phys.body_mut(gun).expect("a body").angular_velocity = DVec3::new(1.0, -2.0, 0.5);

    let mut asleep = None;
    for tick in 1..=600u32 {
        phys.step(DT);
        if phys.is_sleeping(gun) {
            asleep = Some(tick);
            break;
        }
    }
    let asleep = asleep.expect("the shotgun never slept");
    assert!(asleep < 120, "asleep only at tick {asleep}");

    let rotation = phys.transform(gun).expect("registered").rotation;
    let origin = Transform::new(origin_of(&phys, gun, &parts), rotation);
    let lowest = CompoundShape::from_aabbs(&parts)
        .unwrap()
        .world_bounds(DVec3::ZERO, &origin)
        .min
        .y;
    assert!(lowest.abs() < 1e-3, "the lowest part reaches y = {lowest}");
    // On its side: the gun's own x axis, its thinnest, stands up.
    let side = (rotation * DVec3::X).y.abs();
    assert!(side > 0.9, "the gun's x axis is {side} up");

    phys.step(DT);
    assert_eq!(
        phys.contact_counters().bodies,
        0,
        "asleep, nothing steps: {:?}",
        phys.contact_counters()
    );
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// Two shotguns and an L dropped in a heap, hashed after `ticks`, with the
/// first shotgun raised by `lift`.
fn heap_hash(ticks: u32, lift: f64) -> u64 {
    use std::hash::Hasher;
    struct Fnv(u64);
    impl Hasher for Fnv {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    let mut phys = system(AWAKE, true);
    let turn = crcbl_phys::rotation_from_scaled_axis(DVec3::new(0.4, 0.9, 1.3));
    compound(
        &mut phys,
        0,
        &toz34(),
        DVec3::new(0.0, 0.2 + lift, 0.0),
        turn,
    );
    compound(
        &mut phys,
        1,
        &toz34(),
        DVec3::new(0.05, 0.35, 0.1),
        turn.inverse(),
    );
    compound(
        &mut phys,
        2,
        &flat_l(),
        DVec3::new(-0.4, 0.5, -0.3),
        crcbl_phys::rotation_from_scaled_axis(DVec3::new(0.2, 0.0, 0.3)),
    );
    for _ in 0..ticks {
        phys.step(DT);
    }
    let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
    phys.hash_state(&mut hasher);
    hasher.finish()
}

/// **A heap of compounds hashes the same on two runs**, and the hash moves as
/// the heap settles and when one body starts a micrometre higher — so it
/// covers what the compounds do rather than passing by covering nothing.
#[test]
fn a_heap_of_compounds_hashes_the_same_on_two_runs() {
    assert_eq!(heap_hash(120, 0.0), heap_hash(120, 0.0));
    assert_ne!(heap_hash(120, 0.0), heap_hash(121, 0.0));
    assert_ne!(heap_hash(120, 0.0), heap_hash(120, 1e-6));
}
