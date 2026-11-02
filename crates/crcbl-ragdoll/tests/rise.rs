//! Standing up from a corpse: where the capsule would go, and when it cannot.

mod common;

use common::{entities, model, names, skeleton, world};
use crcbl_anim::Pose;
use crcbl_ecs::Entity;
use crcbl_phys::{CharacterConfig, ColliderComponent, PhysicsSystem, Transform};
use crcbl_ragdoll::{GenerateSettings, Handoff, Ragdoll, RagdollRig, RiseError, RiseQuery};
use glam::{DVec3, Mat4, Vec3};

const DT: f64 = 1.0 / 60.0;

/// The query layer the corpse is put on, and left out of the rise's mask.
const CORPSE: u32 = 1 << 7;

/// A rise for the default character, seeing everything but the corpse.
const QUERY: RiseQuery = RiseQuery {
    character: CharacterConfig {
        radius: 0.3,
        half_height: 0.6,
        min_ground_normal_y: core::f64::consts::FRAC_1_SQRT_2,
        step_offset: 0.4,
        skin_width: 0.01,
        max_slides: 4,
        depenetration_passes: 4,
    },
    query_mask: !CORPSE,
    max_shift: 0.5,
};

/// A static box on entity `index`, centred at `centre`.
fn block(physics: &mut PhysicsSystem, index: u32, centre: DVec3, half_extents: DVec3) {
    let entity = Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1");
    let at = Transform::from_position(centre);
    physics.set_transform(entity, at);
    physics.set_collider(
        entity,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents,
            is_trigger: false,
        },
        &at,
    );
}

/// A humanoid laid on its back a centimetre over the solver's floor and
/// settled there, on the corpse layer; with `ground`, a static floor box
/// queries see as well.
///
/// Laid, not dropped: a T-pose dropped upright lands wherever the fall
/// happens to break its symmetry. Measured on 2026-11-01, this fixture
/// dropped from 0.3 m landed in a different pose when only the solver's
/// rounding changed, and of eight tilts of the drop from −0.12 to 0.1 rad,
/// three passed the stand-on-its-own-body check below and five did not.
fn corpse(ground: bool) -> (PhysicsSystem, Ragdoll) {
    let mut physics = world();
    if ground {
        block(
            &mut physics,
            1,
            DVec3::new(0.0, -0.5, 0.0),
            DVec3::new(50.0, 0.5, 50.0),
        );
    }
    let skeleton = skeleton();
    let rig = RagdollRig::generate(&skeleton, &names(), model(0.0), &GenerateSettings::DEFAULT)
        .expect("a rig");
    let pose = Pose::new(&skeleton);
    let handoff = Handoff {
        pose: &pose,
        model: on_its_back(),
        previous: None,
    };
    let ragdoll = Ragdoll::spawn(
        &mut physics,
        &rig,
        &skeleton,
        &handoff,
        &entities(100, rig.bodies().len()),
    )
    .expect("spawned");
    for _ in 0..1200 {
        physics.step(DT);
        if ragdoll.is_settled(&physics) {
            break;
        }
    }
    ragdoll.force_settle(&mut physics);
    ragdoll.set_query_layers(&mut physics, CORPSE);
    (physics, ragdoll)
}

/// The fixture's model transform turned a quarter about `z`, its head to
/// `-x` and its face up, its bones' axes [`LYING_HEIGHT`] up.
fn on_its_back() -> Mat4 {
    Mat4::from_translation(Vec3::new(2.0, LYING_HEIGHT, -1.0))
        * Mat4::from_rotation_z(core::f32::consts::FRAC_PI_2)
        * Mat4::from_rotation_y(core::f32::consts::FRAC_PI_2)
        * Mat4::from_scale(Vec3::splat(common::SCALE))
}

/// How high [`on_its_back`] lays the bones' axes, in metres: a centimetre
/// over the radius of the fattest body the rig fits to the fixture, its
/// thighs' and calves' 9 cm.
const LYING_HEIGHT: f32 = 0.1;

/// The hips: the body nearest the skeleton root, which the generated rig
/// lists first.
fn hips(physics: &PhysicsSystem, ragdoll: &Ragdoll) -> DVec3 {
    ragdoll.body_transforms(physics).expect("bodies")[0].position
}

/// **On open ground the character stands over the hips, and nothing moves**:
/// its feet on the floor, no shift across it, every body where it lay. On a
/// mask that sees the corpse, it is stood on its own body.
#[test]
fn a_corpse_on_open_ground_rises_over_its_hips() {
    let (mut physics, ragdoll) = corpse(true);
    let lying = ragdoll.body_transforms(&physics).expect("bodies");
    let hips = hips(&physics, &ragdoll);

    let centre = ragdoll
        .rise_point(&mut physics, &QUERY)
        .expect("room to stand");
    let standing = QUERY.character.radius + QUERY.character.half_height;
    assert!(
        (centre.y - standing).abs() <= 2.0 * QUERY.character.skin_width,
        "feet on the floor: {centre}"
    );
    assert!(
        (centre - hips).with_y(0.0).length() < 1e-9,
        "{centre} vs {hips}"
    );
    assert_eq!(ragdoll.body_transforms(&physics).expect("bodies"), lying);

    let seeing_the_corpse = RiseQuery {
        query_mask: u32::MAX,
        ..QUERY
    };
    let on_itself = ragdoll
        .rise_point(&mut physics, &seeing_the_corpse)
        .expect("the corpse is ground to stand on");
    assert!(
        on_itself.y > standing + 0.05,
        "stood on its own body: {on_itself}"
    );
}

/// **Against a wall the character rises pushed clear of it, and only as far
/// as the cap allows.** The wall's face stands a tenth of a metre from the
/// hips, inside the capsule's radius.
#[test]
fn a_corpse_against_a_wall_rises_clear_of_it_or_not_at_all() {
    let (mut physics, ragdoll) = corpse(true);
    let hips = hips(&physics, &ragdoll);
    block(
        &mut physics,
        2,
        DVec3::new(hips.x + 0.1 + 0.5, 1.0, hips.z),
        DVec3::new(0.5, 1.0, 2.0),
    );

    let centre = ragdoll
        .rise_point(&mut physics, &QUERY)
        .expect("pushed clear");
    let face = hips.x + 0.1;
    assert!(
        centre.x <= face - QUERY.character.radius + 1e-6,
        "the capsule clears the wall: {centre}"
    );
    let shift = (centre - hips).with_y(0.0).length();
    assert!(shift > 0.15, "pushed out by the overlap: {shift}");

    let tight = RiseQuery {
        max_shift: 0.05,
        ..QUERY
    };
    assert!(matches!(
        ragdoll.rise_point(&mut physics, &tight),
        Err(RiseError::Crowded { shift }) if shift > 0.05
    ));
}

/// **Under a low ceiling there is no room to stand**, and with no ground the
/// queries can see there is nothing to stand on.
#[test]
fn a_low_ceiling_or_no_ground_refuses_the_rise() {
    let (mut physics, ragdoll) = corpse(true);
    let hips = hips(&physics, &ragdoll);
    block(
        &mut physics,
        2,
        DVec3::new(hips.x, 1.2 + 0.5, hips.z),
        DVec3::new(3.0, 0.5, 3.0),
    );
    assert_eq!(
        ragdoll.rise_point(&mut physics, &QUERY),
        Err(RiseError::Blocked)
    );

    let (mut floorless, ragdoll) = corpse(false);
    assert_eq!(
        ragdoll.rise_point(&mut floorless, &QUERY),
        Err(RiseError::NoGround)
    );
}

/// The low slab of these tests: a ceiling under a standing character's head,
/// covering every point within `reach` metres of the hips except those past
/// `edge` metres along +x, where it ends.
fn low_slab_ending_at(physics: &mut PhysicsSystem, hips: DVec3, edge: f64, reach: f64) {
    let near = hips.x - reach - 1.0;
    let far = hips.x + edge;
    block(
        physics,
        2,
        DVec3::new(0.5 * (near + far), 1.2 + 0.5, hips.z),
        DVec3::new(0.5 * (far - near), 0.5, reach + 1.0),
    );
}

/// **A corpse half under a low slab rises beside it**: the hips have no
/// room — the slab's edge is farther than its underside is low, so the solve
/// pushes the capsule down, not out — so the search finds the open floor past
/// the edge, within the cap, and stands the character clear of the slab.
#[test]
fn a_corpse_under_a_low_slab_rises_beside_it() {
    let (mut physics, ragdoll) = corpse(true);
    let hips = hips(&physics, &ragdoll);
    let edge = 0.6;
    low_slab_ending_at(&mut physics, hips, edge, 3.0);
    assert_eq!(
        ragdoll.rise_point(&mut physics, &QUERY),
        Err(RiseError::Blocked),
        "the hips themselves have no room"
    );
    let near = RiseQuery {
        max_shift: 1.2,
        ..QUERY
    };

    let centre = ragdoll
        .rise_point(&mut physics, &near)
        .expect("room past the edge");
    assert!(
        centre.x >= hips.x + edge + QUERY.character.radius - 1e-6,
        "clear of the slab: {centre}"
    );
    let shift = (centre - hips).with_y(0.0).length();
    assert!(shift <= near.max_shift, "within the cap: {shift}");
    let standing = QUERY.character.radius + QUERY.character.half_height;
    assert!(
        (centre.y - standing).abs() <= 2.0 * QUERY.character.skin_width,
        "feet on the floor: {centre}"
    );
}

/// **Room behind a thin wall is not room to rise into.** The slab covers
/// everything within reach but a strip past a wall a few centimetres thick;
/// the hips cannot reach the strip, so the rise is refused for the hips' own
/// reason.
#[test]
fn room_behind_a_thin_wall_is_refused() {
    let (mut physics, ragdoll) = corpse(true);
    let hips = hips(&physics, &ragdoll);
    let edge = 0.6;
    low_slab_ending_at(&mut physics, hips, edge, 3.0);
    block(
        &mut physics,
        3,
        DVec3::new(hips.x + edge - 0.02, 1.0, hips.z),
        DVec3::new(0.02, 1.0, 3.0),
    );
    let far = RiseQuery {
        max_shift: 1.5,
        ..QUERY
    };
    assert_eq!(
        ragdoll.rise_point(&mut physics, &far),
        Err(RiseError::Blocked)
    );
}

/// **A slab overhead is not a floor to rise onto.** A standing capsule with
/// its feet at the hips' height overlaps a thin slab nearer its feet than its
/// head, so the solve's push out of it is upward, onto the slab's top: a
/// character revived under a table would stand on the tabletop. A stand more
/// than a step above the hips is refused as no room, however small the shift
/// across. Measured on the fixture: before the refusal, this answered `Ok`
/// with the feet on the slab.
#[test]
fn a_slab_overhead_is_not_ground_to_rise_onto() {
    let (mut physics, ragdoll) = corpse(true);
    let hips = hips(&physics, &ragdoll);
    // Its underside 0.65 m over the hips: under half the standing capsule's
    // height, so out of it is up, and well over a step.
    block(
        &mut physics,
        2,
        DVec3::new(hips.x, hips.y + 0.7, hips.z),
        DVec3::new(3.0, 0.05, 3.0),
    );
    assert_eq!(
        ragdoll.rise_point(&mut physics, &QUERY),
        Err(RiseError::Blocked)
    );

    // A step's height up is still a stand: the same slab low enough to step
    // onto.
    let (mut physics, ragdoll) = corpse(true);
    let hips = self::hips(&physics, &ragdoll);
    let top = hips.y + QUERY.character.step_offset - 0.05;
    block(
        &mut physics,
        2,
        DVec3::new(hips.x, top - 0.05, hips.z),
        DVec3::new(3.0, 0.05, 3.0),
    );
    let centre = ragdoll
        .rise_point(&mut physics, &QUERY)
        .expect("a step up is room to stand");
    let standing = QUERY.character.radius + QUERY.character.half_height;
    assert!(
        (centre.y - standing - top).abs() <= 2.0 * QUERY.character.skin_width,
        "feet on the low slab: {centre}"
    );
}
