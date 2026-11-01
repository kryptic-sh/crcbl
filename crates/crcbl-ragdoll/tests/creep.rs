//! A generated ragdoll over a full humanoid, fingers and toes included,
//! coming to rest on a plane: a rig comparable to the one in EW's report of
//! 2026-11-01, not EW's own.

mod common;

use common::{entities, world};
use crcbl_anim::{Joint, Pose, Skeleton, Trs};
use crcbl_phys::PhysicsSystem;
use crcbl_ragdoll::{GenerateSettings, Handoff, Ragdoll, RagdollRig};
use glam::{DVec3, Mat4, Vec3};

const DT: f64 = 1.0 / 60.0;

/// A mannequin's skeleton, in centimetres, T-posed facing `+z`: a pelvis at
/// its root, four spine joints, a neck, a head and its end, two arms of a
/// clavicle, upper arm, forearm and hand with three phalanges and a tip to
/// each of five fingers, and two legs of a thigh, calf, foot, ball and toe
/// end. Each joint's name, parent and offset from its parent at rest, in
/// palette order, parents first.
fn bones() -> Vec<(String, Option<usize>, [f32; 3])> {
    let mut bones: Vec<(String, Option<usize>, [f32; 3])> = Vec::new();
    let add = |bones: &mut Vec<_>, name: String, parent: Option<usize>, offset: [f32; 3]| {
        bones.push((name, parent, offset));
        bones.len() - 1
    };
    let pelvis = add(&mut bones, "pelvis".into(), None, [0.0, 95.0, 0.0]);
    let mut spine = pelvis;
    for (k, rise) in [10.0, 10.0, 12.0, 14.0].into_iter().enumerate() {
        spine = add(
            &mut bones,
            format!("spine_0{}", k + 1),
            Some(spine),
            [0.0, rise, 0.0],
        );
    }
    let neck = add(&mut bones, "neck_01".into(), Some(spine), [0.0, 12.0, 0.0]);
    let head = add(&mut bones, "head".into(), Some(neck), [0.0, 10.0, 0.0]);
    add(&mut bones, "head_end".into(), Some(head), [0.0, 22.0, 0.0]);
    for (side, x) in [("l", 1.0_f32), ("r", -1.0)] {
        let clavicle = add(
            &mut bones,
            format!("clavicle_{side}"),
            Some(spine),
            [3.0 * x, 8.0, 0.0],
        );
        let upper = add(
            &mut bones,
            format!("upperarm_{side}"),
            Some(clavicle),
            [15.0 * x, 0.0, 0.0],
        );
        let lower = add(
            &mut bones,
            format!("lowerarm_{side}"),
            Some(upper),
            [28.0 * x, 0.0, 0.0],
        );
        let hand = add(
            &mut bones,
            format!("hand_{side}"),
            Some(lower),
            [25.0 * x, 0.0, 0.0],
        );
        // Each finger's knuckle on the hand, then the lengths of its first
        // two phalanges and its tip. The phalanges reach the generator's
        // `min_length`, so each of the three joints carries a body — the
        // lightest of the rig — and the tip is too short to.
        let fingers: [(&str, [f32; 3], [f32; 3]); 5] = [
            ("thumb", [3.0, -1.5, 3.0], [4.4, 4.2, 2.0]),
            ("index", [9.0, 0.0, 2.5], [4.6, 4.3, 2.0]),
            ("middle", [9.5, 0.0, 0.5], [4.8, 4.4, 2.0]),
            ("ring", [9.0, 0.0, -1.5], [4.6, 4.3, 2.0]),
            ("pinky", [8.0, 0.0, -3.2], [4.3, 4.1, 2.0]),
        ];
        for (finger, base, lengths) in fingers {
            let mut joint = add(
                &mut bones,
                format!("{finger}_01_{side}"),
                Some(hand),
                [base[0] * x, base[1], base[2]],
            );
            for (k, length) in lengths.into_iter().enumerate() {
                joint = add(
                    &mut bones,
                    format!("{finger}_0{}_{side}", k + 2),
                    Some(joint),
                    [length * x, 0.0, 0.0],
                );
            }
        }
    }
    for (side, x) in [("l", 1.0_f32), ("r", -1.0)] {
        let thigh = add(
            &mut bones,
            format!("thigh_{side}"),
            Some(pelvis),
            [9.0 * x, -5.0, 0.0],
        );
        let calf = add(
            &mut bones,
            format!("calf_{side}"),
            Some(thigh),
            [0.0, -42.0, 0.0],
        );
        let foot = add(
            &mut bones,
            format!("foot_{side}"),
            Some(calf),
            [0.0, -40.0, 0.0],
        );
        let ball = add(
            &mut bones,
            format!("ball_{side}"),
            Some(foot),
            [0.0, -7.0, 12.0],
        );
        add(
            &mut bones,
            format!("toe_end_{side}"),
            Some(ball),
            [0.0, 0.0, 5.0],
        );
    }
    bones
}

/// The skeleton of `bones`, its inverse bind matrices from the rest pose.
fn skeleton(bones: &[(String, Option<usize>, [f32; 3])]) -> Skeleton {
    let mut globals: Vec<Mat4> = Vec::with_capacity(bones.len());
    let joints = bones
        .iter()
        .map(|&(_, parent, offset)| {
            let rest = Trs {
                translation: Vec3::from(offset),
                ..Trs::IDENTITY
            };
            let global = parent.map_or(Mat4::IDENTITY, |p| globals[p]) * rest.to_mat4();
            globals.push(global);
            Joint {
                parent,
                inverse_bind: global.inverse(),
                rest,
            }
        })
        .collect();
    Skeleton::new(joints).expect("parents come first")
}

/// Centimetres to metres, standing `lift` metres up.
fn model(lift: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(0.0, lift, 0.0)) * Mat4::from_scale(Vec3::splat(0.01))
}

/// One ragdoll in its own world.
struct Run {
    physics: PhysicsSystem,
    ragdoll: Ragdoll,
    names: Vec<String>,
}

/// The generated rig and its bones' names, in rig order.
fn rig() -> (RagdollRig, Vec<String>) {
    let bones = bones();
    let rig = RagdollRig::generate(&skeleton(&bones), model(0.0), &GenerateSettings::DEFAULT)
        .expect("a rig");
    let names = rig
        .bodies()
        .iter()
        .map(|body| bones[body.joint].0.clone())
        .collect();
    (rig, names)
}

/// The ragdoll standing in its rest pose, its toes two centimetres over the
/// floor of [`world`].
fn spawn() -> Run {
    let skeleton = skeleton(&bones());
    let (rig, names) = rig();
    let mut physics = world();
    let pose = Pose::new(&skeleton);
    let handoff = Handoff {
        pose: &pose,
        model: model(0.02),
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
    Run {
        physics,
        ragdoll,
        names,
    }
}

/// Steps `run` for up to `seconds`, and returns whether it fell asleep.
fn sleeps_within(run: &mut Run, seconds: f64) -> bool {
    let ticks = (seconds / DT).round() as usize;
    (0..ticks).any(|_| {
        run.physics.step(DT);
        run.ragdoll.is_settled(&run.physics)
    })
}

/// Strikes the head with `impulse`, if it is not zero.
fn strike(run: &mut Run, impulse: DVec3) {
    if impulse == DVec3::ZERO {
        return;
    }
    let head = run.names.iter().position(|n| n == "head").expect("a head");
    let at = run
        .physics
        .transform(run.ragdoll.entities()[head])
        .expect("a head body")
        .position;
    run.ragdoll
        .strike(&mut run.physics, impulse, at)
        .expect("struck");
}

/// Four of EW's five runs: no launch, and a strike along each of `±x` and
/// `-z`. The fifth, along `+z`, is left out: there the light spine chatters
/// and keeps the ragdoll awake, a separate cause the backlog's entry on
/// generated ragdolls above 24 substeps records.
const STRIKES: [DVec3; 4] = [
    DVec3::ZERO,
    DVec3::new(30.0, 0.0, 0.0),
    DVec3::new(-30.0, 0.0, 0.0),
    DVec3::new(0.0, 0.0, -30.0),
];

/// How long each of [`STRIKES`] may take to sleep, in seconds.
///
/// Measured on 2026-11-01 at the default sixteen substeps: they slept after
/// 3.3, 4.1, 2.2 and 9.1 s. Before the solver stopped turning a body by the
/// whole of the part of its joints' change its contacts cancel, none of them
/// slept in 40 s.
const SLEEP_WITHIN: f64 = 15.0;

/// Spawns the ragdoll, strikes it with `impulse`, and says whether it fell
/// asleep within `seconds`.
fn struck_sleeps_within(impulse: DVec3, seconds: f64) -> bool {
    let mut run = spawn();
    strike(&mut run, impulse);
    sleeps_within(&mut run, seconds)
}

/// **A generated ragdoll falling onto a plane comes to rest and sleeps**,
/// struck or not, on a rig like the one in EW's report of 2026-11-01: fingers
/// of ten grams and thighs of 12.5 kg, where a joint and a floor contact
/// holding a body still between them turned it by half the joint's
/// warm-started impulse each substep, and no run slept.
#[test]
fn a_generated_ragdoll_on_a_plane_falls_asleep() {
    // The mass spread EW's rig has: the fingertips light, the thighs heavy.
    let (rig, names) = rig();
    let mass = |name: &str| rig.bodies()[names.iter().position(|n| n == name).expect(name)].mass;
    let lightest = rig
        .bodies()
        .iter()
        .map(|body| body.mass)
        .fold(f64::INFINITY, f64::min);
    assert!(lightest < 0.015, "the lightest body weighs {lightest} kg");
    assert!(mass("index_03_l") < 0.015 && mass("thigh_l") > 10.0);
    // The changelog's and the backlog's count.
    assert_eq!((bones().len(), rig.bodies().len()), (66, 56));

    for impulse in STRIKES {
        assert!(
            struck_sleeps_within(impulse, SLEEP_WITHIN),
            "struck with {impulse}, still awake after {SLEEP_WITHIN} s"
        );
    }
}
