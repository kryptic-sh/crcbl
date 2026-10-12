//! Coming to rest: the settled pose on the wire and back, and the budget
//! that settles ragdolls past a timeout or a cap.

mod common;

use common::{entities, model, skeleton, world};
use crcbl_anim::Pose;
use crcbl_phys::{PhysicsSystem, QueryFilter, Transform};
use crcbl_ragdoll::{
    GenerateSettings, Handoff, Ragdoll, RagdollRig, SettleBudget, SettleError, SettledPose,
};
use glam::{DQuat, DVec3};

const DT: f64 = 1.0 / 60.0;

fn rig() -> RagdollRig {
    RagdollRig::generate(&skeleton(), model(0.0), &GenerateSettings::DEFAULT).expect("a rig")
}

/// A ragdoll standing at `x`, half a metre up, on entities from `first`.
fn spawn(physics: &mut PhysicsSystem, rig: &RagdollRig, x: f32, first: u32) -> Ragdoll {
    let skeleton = skeleton();
    let pose = Pose::new(&skeleton);
    let placed = glam::Mat4::from_translation(glam::Vec3::X * x) * model(0.5);
    let handoff = Handoff {
        pose: &pose,
        model: placed,
        previous: None,
    };
    Ragdoll::spawn(
        physics,
        rig,
        &skeleton,
        &handoff,
        &entities(first, rig.bodies().len()),
    )
    .expect("spawned")
}

/// **A settled pose crosses the wire within the quantum and is refused
/// when malformed.** A position past the quantized range is carried exactly.
#[test]
fn a_settled_pose_round_trips_and_refuses_bad_bytes() {
    let pose = SettledPose::new(vec![
        Transform::new(DVec3::new(1.25, 0.1, -3.0), DQuat::from_rotation_y(0.7)),
        Transform::new(DVec3::new(10_000.0, 2.0, 0.0), DQuat::IDENTITY),
    ]);
    let mut bytes = Vec::new();
    pose.encode(&mut bytes);
    let read = SettledPose::decode(&bytes).expect("decodes");
    assert_eq!(read.bodies().len(), 2);
    let quantum =
        2.0 * Transform::WIRE_POSITION_EXTENT / f64::from(1_u32 << Transform::WIRE_POSITION_BITS);
    assert!(
        read.bodies()[0]
            .position
            .distance(pose.bodies()[0].position)
            <= quantum
    );
    assert!(
        read.bodies()[0]
            .rotation
            .angle_between(pose.bodies()[0].rotation)
            < 1e-4
    );
    assert_eq!(
        read.bodies()[1],
        pose.bodies()[1],
        "outside the range, exact"
    );

    assert_eq!(SettledPose::decode(&[]), None);
    let mut other = bytes.clone();
    other[0] = 99;
    assert_eq!(SettledPose::decode(&other), None, "another format");
    assert_eq!(
        SettledPose::decode(&bytes[..bytes.len() - 1]),
        None,
        "short"
    );
    let mut long = bytes.clone();
    long.push(0);
    assert_eq!(SettledPose::decode(&long), None, "trailing bytes");
}

/// **A client takes the server's settled corpse**: its own ragdoll, spawned
/// elsewhere, ends asleep in the server's pose to within the quantum.
#[test]
fn settle_to_puts_a_ragdoll_in_the_sent_pose_asleep() {
    let rig = rig();
    let mut server = world();
    let corpse = spawn(&mut server, &rig, 0.0, 100);
    for _ in 0..1200 {
        server.step(DT);
        if corpse.is_settled(&server) {
            break;
        }
    }
    corpse.force_settle(&mut server);
    let mut bytes = Vec::new();
    corpse
        .settled_pose(&server)
        .expect("every body")
        .encode(&mut bytes);

    let mut client = world();
    let mut mirror = spawn(&mut client, &rig, 3.0, 100);
    for _ in 0..10 {
        client.step(DT);
    }
    let sent = SettledPose::decode(&bytes).expect("decodes");
    assert_eq!(mirror.settle_to(&mut client, &sent), Ok(()));
    assert!(mirror.is_settled(&client));
    for _ in 0..30 {
        client.step(DT);
    }
    assert!(mirror.is_settled(&client), "the corpse stayed asleep");
    let here = mirror.body_transforms(&client).expect("every body");
    let there = corpse.body_transforms(&server).expect("every body");
    for (a, b) in here.iter().zip(&there) {
        assert!(a.position.distance(b.position) < 1e-3, "{a:?} vs {b:?}");
    }
    assert_eq!(mirror.awake_for(), 0.0);

    let short = SettledPose::new(here[1..].to_vec());
    assert!(matches!(
        mirror.settle_to(&mut client, &short),
        Err(SettleError::BodyCount { .. })
    ));
    let mut broken = here.clone();
    broken[0].rotation = DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0);
    assert_eq!(
        mirror.settle_to(&mut client, &SettledPose::new(broken)),
        Err(SettleError::InvalidTransform)
    );
}

/// **The timeout settles a ragdoll that stays awake too long**, and the
/// count restarts once it sleeps.
#[test]
fn the_timeout_settles_a_ragdoll_still_awake() {
    let rig = rig();
    let mut physics = world();
    let mut ragdolls = vec![spawn(&mut physics, &rig, 0.0, 100)];
    let budget = SettleBudget {
        max_awake: 8,
        timeout: 0.25,
    };
    let mut settled_at = None;
    for tick in 0..30 {
        physics.step(DT);
        let settled = budget.enforce(&mut physics, &mut ragdolls, DT);
        if !settled.is_empty() {
            assert_eq!(settled, [0]);
            settled_at = Some(tick);
            break;
        }
    }
    // Awake for more than 0.25 s first happens on the sixteenth step.
    assert_eq!(settled_at, Some(15));
    assert!(ragdolls[0].is_settled(&physics));
    assert_eq!(ragdolls[0].awake_for(), 0.0);
}

/// **Past the cap, the longest-awake ragdolls are settled first**; the
/// newest keep falling.
#[test]
fn the_cap_settles_the_longest_awake() {
    let rig = rig();
    let mut physics = world();
    let count = rig.bodies().len() as u32;
    let budget = SettleBudget {
        max_awake: 1,
        timeout: 60.0,
    };
    let mut ragdolls = vec![spawn(&mut physics, &rig, 0.0, 100)];
    for _ in 0..5 {
        physics.step(DT);
        assert!(budget.enforce(&mut physics, &mut ragdolls, DT).is_empty());
    }
    ragdolls.push(spawn(&mut physics, &rig, 5.0, 100 + count));
    physics.step(DT);
    assert_eq!(budget.enforce(&mut physics, &mut ragdolls, DT), [0]);
    assert!(ragdolls[0].is_settled(&physics));
    assert!(
        !ragdolls[1].is_settled(&physics),
        "the new one keeps falling"
    );
}

/// **A far ragdoll steps coarser, and a settled one is not woken to say
/// so.**
#[test]
fn distance_lod_sets_substeps_on_an_awake_ragdoll_only() {
    let rig = rig();
    assert_eq!(rig.substeps(), GenerateSettings::DEFAULT.substeps);
    let mut physics = world();
    let ragdoll = spawn(&mut physics, &rig, 0.0, 100);
    assert!(ragdoll.set_substeps(&mut physics, 0));
    for &entity in ragdoll.entities() {
        assert_eq!(physics.substeps(entity), Some(0));
    }
    assert!(ragdoll.force_settle(&mut physics));
    assert!(!ragdoll.set_substeps(&mut physics, 8));
    assert!(ragdoll.is_settled(&physics), "the LOD change woke it");
    assert_eq!(physics.substeps(ragdoll.entities()[0]), Some(0));
}

/// **A corpse on its own query layer is invisible to queries that leave
/// that layer out** — a revival's clearance check, a character walking over
/// it — and seen by those that include it.
#[test]
fn a_corpse_on_its_own_layer_is_left_out_of_masked_queries() {
    const CORPSES: u32 = 0b10;
    const SCENERY: u32 = 0b01;
    let rig = rig();
    let mut physics = world();
    let ragdoll = spawn(&mut physics, &rig, 0.0, 100);
    ragdoll.set_query_layers(&mut physics, CORPSES);
    let centre = ragdoll.body_transforms(&physics).expect("every body")[0].position;
    let seen = |physics: &mut PhysicsSystem, mask: u32| {
        physics
            .overlap_sphere_filtered(centre, 0.3, QueryFilter::masked(mask))
            .iter()
            .filter(|(entity, _)| ragdoll.entities().contains(entity))
            .count()
    };
    assert_eq!(
        seen(&mut physics, SCENERY),
        0,
        "the corpse blocked a scenery query"
    );
    assert!(
        seen(&mut physics, CORPSES) > 0,
        "the corpse layer saw nothing"
    );
}
