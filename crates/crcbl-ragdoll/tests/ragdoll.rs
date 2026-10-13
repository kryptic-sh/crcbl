//! A generated ragdoll run whole: spawned at a moving pose, struck by a
//! recorded contact, fallen to the floor, settled, and read back into the
//! pose that skins the mesh.

mod common;

use common::{entities, index, model, skeleton, world};
use crcbl_anim::{Palette, Pose, Trs};
use crcbl_ecs::Entity;
use crcbl_phys::{JointKind, KineticContact, KineticSource, PhysicsSystem, Spring, Transform};
use crcbl_ragdoll::{
    BodyShape, GenerateSettings, Handoff, PreviousPose, Ragdoll, RagdollRig, SpawnError,
    StrikeError, TetherError,
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

/// How a client's detailed ragdoll follows the server's in [`chase`].
#[derive(Clone, Copy, Debug)]
enum Follow {
    /// Not at all.
    Loose,
    /// [`Ragdoll::pull_towards`] at this many hertz.
    Pull(f64),
    /// A [`Tether`] at this many hertz.
    Tether(f64),
}

/// The server simulates a coarse rig of the synthetic skeleton — no
/// clavicles, no neck — and the client the full one; both take the same blow
/// on the chest and fall for ten seconds, the client following as `follow`
/// says each tick. The farthest any joint the server has stands from the
/// client's, at its peak and at the end, and the client's chest body's
/// width.
fn chase(follow: Follow) -> (f64, f64, f64) {
    let skeleton = skeleton();
    let coarse = RagdollRig::generate(
        &skeleton,
        model(0.0),
        &GenerateSettings {
            min_length: 0.2,
            ..GenerateSettings::DEFAULT
        },
    )
    .expect("a coarse rig");
    let detailed = rig();
    assert!(coarse.bodies().len() < detailed.bodies().len());
    let mut server = world();
    let mut client = world();
    let lead = spawn_running(&mut server, &coarse);
    let client_ragdoll = spawn_running(&mut client, &detailed);
    let chest_of = |rig: &RagdollRig| {
        rig.bodies()
            .iter()
            .position(|body| body.joint == index("chest"))
            .expect("a chest")
    };
    for (ragdoll, rig, physics) in [
        (&lead, &coarse, &mut server),
        (&client_ragdoll, &detailed, &mut client),
    ] {
        let entity = ragdoll.entities()[chest_of(rig)];
        let at = body_world(physics, entity).position;
        physics
            .apply_impulse_at(entity, DVec3::NEG_Z * 30.0, at)
            .unwrap();
    }
    let BodyShape::Sphere { radius } = detailed.bodies()[chest_of(&detailed)].shape else {
        panic!("the chest has several children, so it is a sphere");
    };
    let tether = match follow {
        Follow::Tether(hertz) => {
            let targets = lead.joint_positions(&server).unwrap();
            let anchors = entities(500, targets.len());
            Some(
                client_ragdoll
                    .tether(&mut client, &targets, &anchors, hertz)
                    .expect("tethered"),
            )
        }
        Follow::Loose | Follow::Pull(_) => None,
    };
    let gap = |server: &PhysicsSystem, client: &PhysicsSystem| {
        let here = client_ragdoll.joint_positions(client).unwrap();
        lead.joint_positions(server)
            .unwrap()
            .into_iter()
            .map(|(joint, there)| {
                let mine = here
                    .iter()
                    .find(|(j, _)| *j == joint)
                    .expect("a shared joint");
                mine.1.distance(there)
            })
            .fold(0.0_f64, f64::max)
    };
    let mut peak: f64 = 0.0;
    for _ in 0..600 {
        let targets = lead.joint_positions(&server).unwrap();
        match (follow, &tether) {
            (Follow::Pull(hertz), _) => {
                client_ragdoll.pull_towards(&mut client, &targets, hertz, DT);
            }
            (Follow::Tether(_), Some(tether)) => {
                tether.follow(&mut client, &targets);
            }
            _ => {}
        }
        server.step(DT);
        client.step(DT);
        peak = peak.max(gap(&server, &client));
    }
    (peak, gap(&server, &client), 2.0 * radius)
}

/// **A client's detailed ragdoll pulled towards the server's simplified one
/// converges on it, and diverges less on the way.** Each tick the client
/// pulls every joint the server has towards where the server's joint is.
///
/// Measured on 2026-10-12 with this pull at 6 Hz: the shared joints peak
/// 0.62 m apart where the two land, against 1.26 m unpulled — the rigs meet
/// the floor differently, and a pull applied between steps does not overrule
/// a step's contacts — and end 2 cm apart, against 0.61 m. Through the landing
/// the plan's "never more than a body width" takes a [`Tether`].
#[test]
fn a_detailed_client_converges_on_a_simplified_server() {
    /// How far apart the shared joints may end, in metres.
    const SETTLED_GAP: f64 = 0.05;
    let (loose_peak, loose_end, _) = chase(Follow::Loose);
    let (pulled_peak, pulled_end, _) = chase(Follow::Pull(6.0));
    assert!(pulled_end < SETTLED_GAP, "they ended {pulled_end} m apart");
    assert!(
        loose_end > 2.0 * SETTLED_GAP,
        "the rigs agree unpulled: the test shows nothing"
    );
    assert!(
        pulled_peak < loose_peak,
        "the pull did not narrow the landing: {pulled_peak} vs {loose_peak}"
    );
}

/// **Tethered inside the solver, the client stays within its chest's width
/// of the server through the landing**, where the pull between steps at the
/// same stiffness does not, and the two end together.
///
/// Measured on 2026-10-13: tethered at 45 Hz, the shared joints peak 0.14 m
/// apart against a 0.17 m chest; pulled at 60 Hz, the stiffest pull tried,
/// 0.26 m.
#[test]
fn a_tethered_client_holds_within_a_body_width_of_the_server() {
    /// The tether's and the pull's stiffness, in hertz.
    const HERTZ: f64 = 45.0;
    /// How far apart the shared joints may end, in metres.
    const SETTLED_GAP: f64 = 0.01;
    let (tethered_peak, tethered_end, width) = chase(Follow::Tether(HERTZ));
    let (pulled_peak, _, _) = chase(Follow::Pull(HERTZ));
    assert!(
        tethered_peak < width,
        "tethered, they parted {tethered_peak} m, wider than the {width} m chest"
    );
    assert!(
        pulled_peak > width,
        "pulled, they kept within the chest: the test shows nothing"
    );
    assert!(
        tethered_end < SETTLED_GAP,
        "they ended {tethered_end} m apart"
    );
}

/// **A tether refuses a bad spring, too few anchors and an anchor already
/// in use, and takes its anchors and springs out again**: one anchor and
/// spring per body with a target, each anchor moved by `follow`.
#[test]
fn a_tether_checks_its_anchors_and_removes_them() {
    let rig = rig();
    let mut physics = world();
    let ragdoll = spawn_running(&mut physics, &rig);
    let targets = ragdoll.joint_positions(&physics).unwrap();
    let anchors = entities(500, targets.len());
    let joints = physics.joint_count();
    assert_eq!(
        ragdoll.tether(&mut physics, &targets, &anchors, 0.0).err(),
        Some(TetherError::Hertz(0.0))
    );
    assert_eq!(
        ragdoll
            .tether(&mut physics, &targets, &anchors[1..], 10.0)
            .err(),
        Some(TetherError::AnchorCount {
            anchors: targets.len() - 1,
            needed: targets.len()
        })
    );
    let body = ragdoll.entities()[0];
    let mut taken = anchors.clone();
    taken[2] = body;
    assert_eq!(
        ragdoll.tether(&mut physics, &targets, &taken, 10.0).err(),
        Some(TetherError::AnchorInUse(body))
    );
    assert_eq!(physics.joint_count(), joints, "a refusal left a spring");

    // A target with no position is no tether.
    let mut lost = targets.clone();
    lost[1].1 = DVec3::NAN;
    let short = ragdoll
        .tether(&mut physics, &lost, &anchors, 10.0)
        .expect("tethered");
    assert_eq!(short.len(), targets.len() - 1);
    short.remove(&mut physics);

    let tether = ragdoll
        .tether(&mut physics, &targets, &anchors, 10.0)
        .expect("tethered");
    assert_eq!(tether.len(), targets.len());
    assert_eq!(physics.joint_count(), joints + targets.len());
    let springs = physics
        .joints()
        .filter(|(_, joint)| anchors.contains(&joint.body_b))
        .map(|(_, joint)| joint.kind)
        .collect::<Vec<_>>();
    assert_eq!(springs.len(), targets.len());
    for kind in springs {
        let JointKind::Distance(spring) = kind else {
            panic!("a tether's spring is a distance joint: {kind:?}");
        };
        assert!(spring.enable_spring && spring.length == 0.0, "{spring:?}");
        assert_eq!(
            spring.spring,
            Spring {
                hertz: 10.0,
                damping_ratio: 1.0
            },
            "not critically damped at the asked frequency"
        );
    }
    let moved: Vec<(usize, DVec3)> = targets
        .iter()
        .map(|&(joint, at)| (joint, at + DVec3::Y))
        .collect();
    assert_eq!(tether.follow(&mut physics, &moved), targets.len());
    let first = body_world(&physics, anchors[0]).position;
    assert_eq!(first, moved[0].1, "the anchor did not follow");
    tether.remove(&mut physics);
    assert_eq!(physics.joint_count(), joints, "a spring stayed");
    assert!(
        anchors
            .iter()
            .all(|&anchor| physics.transform(anchor).is_none()),
        "an anchor stayed"
    );
}

/// Every body's position and rotation, as bits, after each of `ticks` steps
/// of a ragdoll spawned running and struck off-centre on the head, from a rig
/// and a world of its own.
fn struck_run(ticks: usize, blow: DVec3) -> Vec<Vec<[u64; 7]>> {
    let rig = rig();
    let mut physics = world();
    let ragdoll = spawn_running(&mut physics, &rig);
    let head = ragdoll.entities()[rig
        .bodies()
        .iter()
        .position(|body| body.joint == index("head"))
        .expect("a head")];
    let point = body_world(&physics, head).position + DVec3::new(0.02, 0.03, 0.0);
    physics
        .apply_impulse_at(head, blow, point)
        .expect("the head takes the blow");
    (0..ticks)
        .map(|_| {
            physics.step(DT);
            ragdoll
                .entities()
                .iter()
                .map(|&entity| {
                    let Transform {
                        position: p,
                        rotation: r,
                        ..
                    } = body_world(&physics, entity);
                    [p.x, p.y, p.z, r.x, r.y, r.z, r.w].map(f64::to_bits)
                })
                .collect()
        })
        .collect()
}

/// **One death steps to the same bits every time**: two runs, each with its
/// rig generated and its world built afresh, agree on every body's pose at
/// every tick to the last bit — what lets the server's ragdoll ride the tick
/// hash. Each run's hash maps are seeded apart, so an order one leaked into
/// the solve would show here. A blow one bit harder shows that the bits are
/// what is compared.
#[test]
fn one_death_steps_to_the_same_bits() {
    const TICKS: usize = 180;
    let blow = DVec3::NEG_Z * 20.0;
    let first = struck_run(TICKS, blow);
    let second = struck_run(TICKS, blow);
    assert_eq!(first.len(), TICKS);
    assert_ne!(first[0], first[TICKS - 1], "the bodies never moved");
    if let Some(tick) = (0..TICKS).find(|&tick| first[tick] != second[tick]) {
        panic!("the runs part at tick {tick}");
    }
    let harder = DVec3::new(0.0, 0.0, f64::from_bits(blow.z.to_bits() + 1));
    assert_ne!(
        struck_run(TICKS, harder),
        first,
        "a harder blow changed nothing"
    );
}

/// **A client tethered to a server at rest falls asleep, and wakes when the
/// server's joints move**: following its own joints the ragdoll comes to
/// rest, and held to where it rests by targets that no longer change it falls
/// asleep; anchors lifted a few centimetres wake it, and it rises after them.
#[test]
fn a_moved_tether_wakes_a_sleeping_ragdoll() {
    /// The most ticks it may take to fall asleep.
    const SETTLE_TICKS: usize = 3600;
    /// How far the anchors are lifted, in metres.
    const LIFT_BY: f64 = 0.05;
    let rig = rig();
    let mut physics = world();
    let ragdoll = spawn_running(&mut physics, &rig);
    let targets = ragdoll.joint_positions(&physics).unwrap();
    let anchors = entities(500, targets.len());
    let tether = ragdoll
        .tether(&mut physics, &targets, &anchors, 10.0)
        .expect("tethered");
    let asleep = |physics: &PhysicsSystem| {
        ragdoll
            .entities()
            .iter()
            .all(|&entity| physics.is_sleeping(entity))
    };
    let mut ticks = 0;
    while !asleep(&physics) {
        assert!(ticks < SETTLE_TICKS, "it never came to rest");
        let here = ragdoll.joint_positions(&physics).unwrap();
        tether.follow(&mut physics, &here);
        physics.step(DT);
        ticks += 1;
    }
    // A server at rest: the same targets every tick, which move the anchors
    // once, onto where it rests, and then never again.
    let rest = ragdoll.joint_positions(&physics).unwrap();
    for tick in 0..SETTLE_TICKS {
        let moved = tether.follow(&mut physics, &rest);
        assert!(tick == 0 || moved == 0, "a still anchor moved");
        physics.step(DT);
    }
    assert!(
        asleep(&physics),
        "it never fell asleep under a still tether"
    );

    let lifted: Vec<(usize, DVec3)> = rest
        .iter()
        .map(|&(joint, at)| (joint, at + DVec3::Y * LIFT_BY))
        .collect();
    assert_eq!(tether.follow(&mut physics, &lifted), rest.len());
    physics.step(DT);
    assert!(!asleep(&physics), "the lifted tether left it asleep");
    for _ in 0..60 {
        tether.follow(&mut physics, &lifted);
        physics.step(DT);
    }
    let risen = ragdoll.joint_positions(&physics).unwrap();
    let rose = risen[0].1.y - rest[0].1.y;
    assert!(rose > LIFT_BY / 2.0, "it rose {rose} m of {LIFT_BY}");
}
