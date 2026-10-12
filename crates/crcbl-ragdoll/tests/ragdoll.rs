//! A generated ragdoll run whole: spawned at a moving pose, struck by a
//! recorded contact, fallen to the floor, settled, and read back into the
//! pose that skins the mesh.

mod common;

use common::{entities, index, model, skeleton, world};
use crcbl_anim::{Palette, Pose, Trs};
use crcbl_ecs::Entity;
use crcbl_phys::{KineticContact, KineticSource, PhysicsSystem, Transform};
use crcbl_ragdoll::{
    GenerateSettings, Handoff, PreviousPose, Ragdoll, RagdollRig, SpawnError, StrikeError,
};
use glam::{DVec3, Mat4, Quat, Vec3};

const DT: f64 = 1.0 / 60.0;
/// The height the character stands at when it dies, in metres.
const LIFT: f32 = 0.5;
/// How fast it was running along `x`, in m/s.
const RUN: f64 = 1.5;

fn rig() -> RagdollRig {
    RagdollRig::generate(&skeleton(), model(0.0), &GenerateSettings::DEFAULT).expect("a rig")
}

fn body_world(physics: &PhysicsSystem, entity: Entity) -> Transform {
    *physics.transform(entity).expect("a body")
}

/// The mechanical energy of the bodies: kinetic plus potential under
/// standard gravity, in joules.
fn energy(physics: &PhysicsSystem, ragdoll: &Ragdoll) -> f64 {
    ragdoll
        .entities()
        .iter()
        .map(|&entity| {
            let body = physics.body(entity).expect("a body");
            let at = body_world(physics, entity);
            body.kinetic_energy(at.rotation) + body.mass * 9.81 * at.position.y
        })
        .sum()
}

/// Spawns the rig on a character running along `x`, whose last two poses
/// are the rest pose one tick apart.
fn spawn_running(physics: &mut PhysicsSystem, rig: &RagdollRig) -> Ragdoll {
    let skeleton = skeleton();
    let pose = Pose::new(&skeleton);
    let previous_model = Mat4::from_translation(Vec3::X * -(RUN * DT) as f32) * model(LIFT);
    let handoff = Handoff {
        pose: &pose,
        model: model(LIFT),
        previous: Some(PreviousPose {
            pose: &pose,
            model: previous_model,
            seconds: DT,
        }),
    };
    Ragdoll::spawn(
        physics,
        rig,
        &skeleton,
        &handoff,
        &entities(100, rig.bodies().len()),
    )
    .expect("spawned")
}

/// **The ragdoll carries on as the character was moving, holds its joints,
/// takes the killing blow in the limb it landed on, falls, settles, and its
/// skin follows its bodies all the way.**
///
/// A point near each carrying joint, skinned through the pose the ragdoll
/// writes, lands where that joint's body has carried it. The mechanical
/// energy never rises above what the blow left it with.
#[test]
fn a_struck_ragdoll_falls_settles_and_skins_where_its_bodies_are() {
    let skeleton = skeleton();
    let rig = rig();
    let mut physics = world();
    let mut ragdoll = spawn_running(&mut physics, &rig);

    for &entity in ragdoll.entities() {
        let velocity = physics.body(entity).expect("a body").velocity;
        assert!(
            velocity.distance(DVec3::X * RUN) < 1e-3,
            "it was running: {velocity}"
        );
    }
    for &joint in ragdoll.joints() {
        let drift = physics.joint_drift(joint).expect("a joint");
        assert!(drift.linear < 1e-5, "a joint starts apart: {drift:?}");
    }

    // The killing blow struck the living character's capsule, which the
    // ragdoll replaced: the solver applied it to that capsule, not to these.
    let head = rig
        .bodies()
        .iter()
        .position(|body| body.joint == index("head"))
        .expect("a head");
    let head_at = body_world(&physics, ragdoll.entities()[head]).position;
    let capsule = entities(1, 1)[0];
    let blow = KineticContact {
        source: KineticSource::Contact,
        impactor: None,
        struck: capsule,
        point: head_at + DVec3::Y * 0.02,
        normal: DVec3::NEG_Z,
        relative_velocity: DVec3::NEG_Z * 300.0,
        impactor_mass: 0.01,
        energy_deposited: 400.0,
        impulse: 20.0,
    };
    let before = physics
        .body(ragdoll.entities()[head])
        .expect("a body")
        .velocity;
    assert_eq!(ragdoll.strike_with(&mut physics, &blow), Ok(head));
    let head_body = physics.body(ragdoll.entities()[head]).expect("a body");
    let expected = before + DVec3::NEG_Z * (20.0 / head_body.mass);
    assert!(
        head_body.velocity.distance(expected) < 1e-9,
        "{} vs {expected}",
        head_body.velocity
    );

    // A bind-space point a little off each carrying joint, where it stands
    // at the handoff and on which body.
    let handoff_model = ragdoll.handoff_model();
    let rest = common::joint_positions(&skeleton, &Pose::new(&skeleton), Mat4::IDENTITY);
    let marks: Vec<(usize, Vec3, DVec3, Transform)> = rig
        .bodies()
        .iter()
        .zip(ragdoll.entities())
        .map(|(body, &entity)| {
            let bind = rest[body.joint] + Vec3::new(1.0, 2.0, 0.5);
            let at = handoff_model.transform_point3(bind).as_dvec3();
            (body.joint, bind, at, body_world(&physics, entity))
        })
        .collect();

    let limit = energy(&physics, &ragdoll);
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    let mut settled_at = None;
    for tick in 0..1200 {
        physics.step(DT);
        ragdoll
            .pose_into(&physics, handoff_model, &mut pose)
            .expect("a representable pose");
        palette.compute(&skeleton, &pose);
        for (&(joint, bind, at, start), &entity) in marks.iter().zip(ragdoll.entities()) {
            let now = body_world(&physics, entity);
            let carried =
                now.position + now.rotation * (start.rotation.inverse() * (at - start.position));
            let skinned = (handoff_model * palette.matrices()[joint]).transform_point3(bind);
            assert!(
                skinned.as_dvec3().distance(carried) < 2e-3,
                "tick {tick}, joint {joint}: skin {skinned}, body {carried}"
            );
            assert!(now.position.y > -0.02, "tick {tick}: a body sank: {now:?}");
        }
        let now = energy(&physics, &ragdoll);
        assert!(
            now <= limit + 1e-6 * limit.abs().max(1.0),
            "tick {tick}: energy rose from {limit} to {now}"
        );
        if ragdoll.is_settled(&physics) {
            settled_at = Some(tick);
            break;
        }
    }
    let settled_at = settled_at.expect("the ragdoll never settled in 20 seconds");
    assert!(
        settled_at > 30,
        "it settled at tick {settled_at}, before falling"
    );
    let hips = rig
        .bodies()
        .iter()
        .position(|body| body.joint == index("hips"))
        .expect("hips");
    let resting = body_world(&physics, ragdoll.entities()[hips]).position;
    assert!(resting.y < 0.4, "the corpse is lying down: {resting}");
    assert!(resting.x > 2.0, "it fell the way it was running: {resting}");
    for &joint in ragdoll.joints() {
        let drift = physics.joint_drift(joint).expect("no joint broke");
        assert!(drift.linear < 0.01, "a joint came apart: {drift:?}");
    }
}

/// **The joint frames hold at any pose.** Spawned with an elbow bent and a
/// hip turned, every joint's two anchors still meet.
#[test]
fn a_bent_handoff_pose_starts_with_its_joints_together() {
    let skeleton = skeleton();
    let rig = rig();
    let mut physics = world();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[index("forearm_l")].rotation = Quat::from_rotation_z(1.0);
    pose.locals_mut()[index("thigh_r")].rotation = Quat::from_rotation_x(-0.6);
    pose.locals_mut()[index("hips")] = Trs {
        rotation: Quat::from_rotation_y(0.4),
        ..pose.locals()[index("hips")]
    };
    let handoff = Handoff {
        pose: &pose,
        model: model(LIFT),
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
    for &joint in ragdoll.joints() {
        let drift = physics.joint_drift(joint).expect("a joint");
        assert!(drift.linear < 1e-5, "a joint starts apart: {drift:?}");
    }
    let mut out = Pose::new(&skeleton);
    let mut ragdoll = ragdoll;
    ragdoll
        .pose_into(&physics, model(LIFT), &mut out)
        .expect("the handoff pose reads back");
    let read = common::joint_positions(&skeleton, &out, model(LIFT));
    let given = common::joint_positions(&skeleton, &pose, model(LIFT));
    for (joint, (a, b)) in read.iter().zip(&given).enumerate() {
        assert!(a.distance(*b) < 1e-4, "joint {joint}: {a} vs {b}");
    }
}

/// **A bad spawn leaves the system as it was.**
#[test]
fn a_refused_spawn_touches_nothing() {
    let skeleton = skeleton();
    let rig = rig();
    let mut physics = world();
    let pose = Pose::new(&skeleton);
    let handoff = Handoff {
        pose: &pose,
        model: model(LIFT),
        previous: None,
    };
    let count = rig.bodies().len();
    let refused = Ragdoll::spawn(
        &mut physics,
        &rig,
        &skeleton,
        &handoff,
        &entities(100, count - 1),
    );
    assert!(matches!(refused, Err(SpawnError::EntityCount { .. })));

    let mut taken = entities(100, count);
    taken[3] = taken[1];
    let refused = Ragdoll::spawn(&mut physics, &rig, &skeleton, &handoff, &taken);
    assert_eq!(refused.err(), Some(SpawnError::EntityInUse(taken[1])));

    let still = Handoff {
        previous: Some(PreviousPose {
            pose: &pose,
            model: model(LIFT),
            seconds: 0.0,
        }),
        ..handoff
    };
    let refused = Ragdoll::spawn(&mut physics, &rig, &skeleton, &still, &entities(100, count));
    assert!(matches!(refused, Err(SpawnError::Binding(_))));
    assert_eq!(
        physics.body_count(),
        0,
        "a refused spawn left bodies behind"
    );
    assert_eq!(physics.joint_count(), 0);

    physics.set_transform(entities(105, 1)[0], Transform::IDENTITY);
    let refused = Ragdoll::spawn(
        &mut physics,
        &rig,
        &skeleton,
        &handoff,
        &entities(100, count),
    );
    assert_eq!(
        refused.err(),
        Some(SpawnError::EntityInUse(entities(105, 1)[0]))
    );
    assert_eq!(physics.body_count(), 0);
}

/// **A contact on the ragdoll itself is not applied twice**, and a strike
/// lands on the body nearest its point.
#[test]
fn strikes_land_once_on_the_nearest_body() {
    let rig = rig();
    let mut physics = world();
    let ragdoll = spawn_running(&mut physics, &rig);
    let foot = rig
        .bodies()
        .iter()
        .position(|body| body.joint == index("foot_r"))
        .expect("a foot");
    let foot_at = body_world(&physics, ragdoll.entities()[foot]).position;
    let own = KineticContact {
        source: KineticSource::Contact,
        impactor: None,
        struck: ragdoll.entities()[foot],
        point: foot_at,
        normal: DVec3::Y,
        relative_velocity: DVec3::Y,
        impactor_mass: f64::INFINITY,
        energy_deposited: 1.0,
        impulse: 5.0,
    };
    let velocities: Vec<DVec3> = ragdoll
        .entities()
        .iter()
        .map(|&e| physics.body(e).expect("a body").velocity)
        .collect();
    assert_eq!(
        ragdoll.strike_with(&mut physics, &own),
        Err(StrikeError::AlreadyApplied)
    );
    for (&entity, before) in ragdoll.entities().iter().zip(&velocities) {
        assert_eq!(physics.body(entity).expect("a body").velocity, *before);
    }
    assert_eq!(
        ragdoll.strike(&mut physics, DVec3::X, foot_at + DVec3::X * 0.03),
        Ok(foot)
    );
    let (nearest, distance) = ragdoll
        .nearest_body(&physics, foot_at)
        .expect("every body is there");
    assert_eq!(nearest, foot);
    assert!(distance < 0.0, "the foot's centre is inside it: {distance}");
}

/// **Force-settling sleeps the ragdoll where it stands; removing it takes
/// every body and joint out.**
#[test]
fn force_settle_and_remove() {
    let rig = rig();
    let mut physics = world();
    let ragdoll = spawn_running(&mut physics, &rig);
    for _ in 0..5 {
        physics.step(DT);
    }
    let before = ragdoll.body_transforms(&physics).expect("every body");
    assert!(!ragdoll.is_settled(&physics), "still falling");
    assert!(ragdoll.force_settle(&mut physics));
    for _ in 0..30 {
        physics.step(DT);
    }
    assert_eq!(
        ragdoll.body_transforms(&physics).expect("every body"),
        before,
        "a settled ragdoll moved"
    );
    let entities = ragdoll.entities().to_vec();
    ragdoll.remove(&mut physics);
    assert_eq!(physics.body_count(), 0);
    assert_eq!(physics.joint_count(), 0);
    for pair in entities.windows(2) {
        assert!(!physics.ignores_contacts(pair[0], pair[1]));
    }
}
