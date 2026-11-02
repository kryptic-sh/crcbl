//! The humanoid profile of `RagdollRig::generate`: joint types, bend
//! directions, neutrals and limits from bone names and the bind pose, the
//! bones it folds away, and how the result moves. The humanoids are in
//! `anatomy/humanoids.rs`.

mod common;
#[path = "anatomy/humanoids.rs"]
mod humanoids;

use common::{entities, state_hash, world};
use crcbl_anim::Pose;
use crcbl_ecs::Entity;
use crcbl_phys::{
    ContactSettings, Joint, MassProperties, PhysicsSystem, RigidBody, Transform,
    rotation_from_scaled_axis,
};
use crcbl_ragdoll::{
    BodyShape, GenerateError, GenerateSettings, Handoff, Ragdoll, RagdollRig, RigJointKind,
};
use glam::{DQuat, DVec3, Quat, Vec3};
use humanoids::{
    Bone, Naming, body, ew, generate, joint, long_necked, model, names, profiled, skeleton, unreal,
    without, without_zones,
};

const DT: f64 = 1.0 / 60.0;

const QUARTER: f32 = core::f32::consts::FRAC_PI_2;

/// The world position of bone `name` in the bind pose placed by `model`.
fn bone_at(bones: &[Bone], name: &str, model: glam::Mat4) -> DVec3 {
    let skeleton = skeleton(bones);
    let index = bones.iter().position(|b| b.0 == name).expect(name);
    common::joint_positions(&skeleton, &Pose::new(&skeleton), model)[index].as_dvec3()
}

/// The world axis of the hinge at `name` in the bind pose placed by
/// `model`: the joint frame's z.
fn hinge_axis(rig: &RagdollRig, bones: &[Bone], name: &str, model: glam::Mat4) -> DVec3 {
    let skeleton = skeleton(bones);
    let worlds = rig
        .bodies_at(&skeleton, &Pose::new(&skeleton), model)
        .expect("placed");
    let joint = joint(rig, bones, name);
    worlds[joint.parent].rotation * joint.frame_parent.rotation * DVec3::Z
}

/// Where turning the bone from `name` to `end` a little about its hinge
/// moves `end`, as a unit vector.
fn bends_towards(
    rig: &RagdollRig,
    bones: &[Bone],
    [name, end]: [&str; 2],
    model: glam::Mat4,
) -> DVec3 {
    let axis = hinge_axis(rig, bones, name, model);
    let bone = bone_at(bones, end, model) - bone_at(bones, name, model);
    (rotation_from_scaled_axis(axis * 0.1) * bone - bone).normalize()
}

/// **Knees and elbows are hinges whose positive bend is the anatomical
/// one**, on every naming, T-posed and A-posed, facing any way: turning the
/// calf a little about the hinge's axis moves the foot backwards, and the
/// forearm the hand forwards. The bind's straight limbs stand at the hard
/// stop on extension, NASA's posture is the neutral, and the far stop is
/// the range's flexion.
#[test]
fn knees_and_elbows_bend_the_anatomical_way() {
    for naming in Naming::ALL {
        for drop in [0.0, 45.0] {
            let bones = naming.bones(drop);
            let model = model(0.0, QUARTER);
            let forward = model.transform_vector3(Vec3::Z).normalize().as_dvec3();
            let rig = generate(&bones, model, &profiled(false));
            for (k, [_, middle, end]) in naming.limbs().into_iter().enumerate() {
                let joint = joint(&rig, &bones, middle);
                let RigJointKind::Hinge {
                    lower,
                    upper,
                    neutral,
                    ..
                } = joint.kind
                else {
                    panic!("{middle} is {:?}, not a hinge", joint.kind);
                };
                let towards = if k < 2 { -forward } else { forward };
                let moved = bends_towards(&rig, &bones, [middle, end], model).dot(towards);
                assert!(
                    moved > 0.99,
                    "{middle} (drop {drop}) bends {moved} of the way it should"
                );
                let (nasa, most) = if k < 2 {
                    (47.0_f64, 140.0)
                } else {
                    (58.0, 145.0)
                };
                let near = |a: f64, b: f64| (a - b).abs() < 1e-4;
                assert!(
                    near(neutral, nasa.to_radians())
                        && near(lower, -nasa.to_radians())
                        && near(upper, (most - nasa).to_radians()),
                    "{middle}: neutral {neutral}, limits {lower} to {upper}"
                );
            }
        }
    }
}

/// **Fingers flex into the palm, and toes and the ball of the foot down**,
/// on every naming and facing any way: the T-posed hands are palm down, so
/// a finger segment's positive bend moves its tip down, as it does a toe's.
#[test]
fn fingers_and_toes_bend_the_anatomical_way() {
    let cases: [(Naming, [[&str; 2]; 4]); 3] = [
        (
            Naming::Ew,
            [
                ["indexFinger_01_l", "indexFinger_02_l"],
                ["finger_02_r", "finger_03_r"],
                ["ball_l", "toes_l"],
                ["thumb_02_l", "thumb_03_l"],
            ],
        ),
        (
            Naming::Mixamo,
            [
                ["mixamorig:LeftHandIndex1", "mixamorig:LeftHandIndex2"],
                ["mixamorig:RightHandPinky2", "mixamorig:RightHandPinky3"],
                ["mixamorig:LeftToeBase", "mixamorig:LeftToe_End"],
                ["mixamorig:LeftHandThumb2", "mixamorig:LeftHandThumb3"],
            ],
        ),
        (
            Naming::Unreal,
            [
                ["index_01_l", "index_02_l"],
                ["pinky_02_r", "pinky_03_r"],
                ["ball_l", "ball_leaf_l"],
                ["thumb_02_l", "thumb_03_l"],
            ],
        ),
    ];
    let model = model(0.0, QUARTER);
    let down = DVec3::NEG_Y;
    for (naming, [finger, other_finger, toe, thumb]) in cases {
        let bones = naming.bones(0.0);
        let rig = generate(&bones, model, &profiled(false));
        for pair in [finger, other_finger, toe] {
            assert!(
                matches!(
                    joint(&rig, &bones, pair[0]).kind,
                    RigJointKind::Hinge { .. }
                ),
                "{} is not a hinge",
                pair[0]
            );
            let moved = bends_towards(&rig, &bones, pair, model).dot(down);
            assert!(moved > 0.99, "{} bends {moved} of the way down", pair[0]);
        }
        // A thumb bends across the palm: down, and away from its own side
        // towards the little finger, which is backwards in a T-pose.
        let backward = -model.transform_vector3(Vec3::Z).normalize().as_dvec3();
        let moved = bends_towards(&rig, &bones, thumb, model);
        assert!(
            moved.dot(down) > 0.5 && moved.dot(backward) > 0.5,
            "{} bends towards {moved}",
            thumb[0]
        );
    }
}

/// A two-body world: the parent of the ball at `child` held fixed where the
/// bind puts it, the child turned from its bind to `zero` about the joint,
/// joined by the profiled ball with its soft zones taken out.
struct Drive {
    physics: PhysicsSystem,
    child: Entity,
    pivot: DVec3,
    start: DQuat,
}

impl Drive {
    fn new(bones: &[Bone], child: &str, zero: DVec3) -> Self {
        let model = model(0.0, 0.0);
        let rig = without_zones(&generate(bones, model, &profiled(false)));
        let skeleton = skeleton(bones);
        let worlds = rig
            .bodies_at(&skeleton, &Pose::new(&skeleton), model)
            .expect("placed");
        let joint = *joint(&rig, bones, child);
        let (parent_world, child_world) = (worlds[joint.parent], worlds[joint.child]);
        let frame = child_world.rotation * joint.frame_child.rotation;
        let pivot = child_world.position + child_world.rotation * joint.frame_child.position;
        let to_zero = DQuat::from_rotation_arc(frame * DVec3::Z, zero);
        let start = Transform::new(
            pivot + to_zero * (child_world.position - pivot),
            (to_zero * child_world.rotation).normalize(),
        );
        let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        let [parent, child] = [entities(1, 2)[0], entities(1, 2)[1]];
        physics.set_transform(parent, parent_world);
        let body = rig.bodies()[joint.child];
        let collider = body.shape.collider();
        let inertia = MassProperties::of_collider(&collider, body.mass).inertia;
        physics.set_body(
            child,
            RigidBody::new_dynamic(body.mass).with_inertia(inertia),
        );
        physics.set_transform(child, start);
        physics.set_substeps(child, rig.substeps());
        let (frame_parent, frame_child) = joint.physics_frames();
        physics
            .add_joint(Joint::new(
                parent,
                child,
                frame_parent,
                frame_child,
                joint.kind.joint_kind(0.0),
            ))
            .expect("a joint");
        Self {
            physics,
            child,
            pivot,
            start: start.rotation,
        }
    }

    /// Spins the child about `axis` until it stops, and returns the most it
    /// turned: the angle `measure` reads off its turn from the start.
    ///
    /// A turn is read only while `zero`, turned, stays within a degree of
    /// the plane square to `axis`, or of `zero` itself for a twist about the
    /// bone: driven on into a stop, the solver turns the bone along the
    /// stop's edge, out of the plane the stop is in, and a twist held at its
    /// stop swings the bone (`docs/backlog.md`, joint limits layer 1).
    fn reach(mut self, zero: DVec3, axis: DVec3, measure: impl Fn(DQuat) -> f64) -> f64 {
        /// Slow enough that the stop is met within a degree of a tick.
        const SPEED: f64 = 0.5;
        let in_plane = crcbl_core::trig::sin(1.0_f64.to_radians());
        let mut most = f64::NEG_INFINITY;
        for _ in 0..(8.0 / DT) as usize {
            let at = self.physics.transform(self.child).expect("a body").position;
            let body = self.physics.body_mut(self.child).expect("a body");
            body.angular_velocity = axis * SPEED;
            body.velocity = (axis * SPEED).cross(at - self.pivot);
            self.physics.step(DT);
            let now = self.physics.transform(self.child).expect("a body").rotation;
            let turn = now * self.start.inverse();
            // A twist is read while the bone stays on its zero, a swing while
            // it stays in its plane.
            let held = if zero.dot(axis).abs() > 0.5 {
                (turn * zero).cross(zero).length() < in_plane
            } else {
                (turn * zero).dot(axis).abs() < in_plane
            };
            if held {
                most = most.max(measure(turn));
            }
        }
        most
    }
}

/// The angle `turn` takes `zero` round `axis` by, in degrees.
#[expect(
    clippy::disallowed_methods,
    reason = "measuring the bodies, not simulating"
)]
fn swung(turn: DQuat, zero: DVec3, axis: DVec3) -> f64 {
    let now = turn * zero;
    zero.cross(now).dot(axis).atan2(zero.dot(now)).to_degrees()
}

/// The angle of `turn` about `axis`, in degrees.
#[expect(
    clippy::disallowed_methods,
    reason = "measuring the bodies, not simulating"
)]
fn twisted(turn: DQuat, axis: DVec3) -> f64 {
    let turn = if turn.w < 0.0 { -turn } else { turn };
    (2.0 * DVec3::new(turn.x, turn.y, turn.z).dot(axis).atan2(turn.w)).to_degrees()
}

/// How far, in degrees, a ball driven into a stop may be read from it: the
/// drive's own tick and the rigid row's give.
const REACH_SLACK: f64 = 1.5;

/// One ball's anatomical axes and stops for the reach test: from the
/// anatomical zero, the axis flexion turns about, the axis abduction turns
/// about, and the four stops, in degrees.
struct Ball {
    bone: &'static str,
    zero: DVec3,
    flexes: DVec3,
    abducts: DVec3,
    stops: [f64; 4],
    /// The axis a twist turns about and its two stops, where the source is
    /// lopsided enough to show a sign.
    twist: Option<(DVec3, [f64; 2])>,
}

/// The balls of the T-posed `naming`, unturned: facing `+z`, left at `+x`,
/// up `+y`, the hands palm down with the thumbs forward. Every axis and stop
/// is written out from that, not read from the profile.
fn balls(naming: Naming) -> Vec<Ball> {
    let (x, y, z) = (DVec3::X, DVec3::Y, DVec3::Z);
    let foot = DVec3::new(0.0, -7.0, 12.0).normalize();
    let (spine, cervical) = match naming {
        Naming::Ew => (3.0, 2.0),
        // `neck_02` is spliced into `neck_01`.
        Naming::Unreal => (5.0, 2.0),
        Naming::Mixamo => unreachable!("not driven"),
    };
    let l = |ew: &'static str, unreal: &'static str| if naming == Naming::Ew { ew } else { unreal };
    let shoulder = 170.0;
    vec![
        // Hip: flexion forward about -x, abduction outward.
        Ball {
            bone: l("Thigh_L", "thigh_l"),
            zero: -y,
            flexes: -x,
            abducts: z,
            stops: [125.0, 30.0, 45.0, 20.0],
            twist: None,
        },
        Ball {
            bone: l("Thigh_R", "thigh_r"),
            zero: -y,
            flexes: -x,
            abducts: -z,
            stops: [125.0, 30.0, 45.0, 20.0],
            twist: None,
        },
        // Shoulder: the same axes, stopped short of overhead.
        Ball {
            bone: l("UpperArm_L", "upperarm_l"),
            zero: -y,
            flexes: -x,
            abducts: z,
            stops: [shoulder, 50.0, shoulder, 30.0],
            twist: None,
        },
        Ball {
            bone: l("UpperArm_R", "upperarm_r"),
            zero: -y,
            flexes: -x,
            abducts: -z,
            stops: [shoulder, 50.0, shoulder, 30.0],
            twist: None,
        },
        // Wrist: palm down, so flexion turns the fingers down; radial
        // deviation towards the thumb, forward. Pronation turns a left hand
        // right-handed about its fingers, a right hand left-handed: both
        // turn about +x, which tips either thumb down.
        Ball {
            bone: l("Hand_L", "hand_l"),
            zero: x,
            flexes: -z,
            abducts: -y,
            stops: [80.0, 70.0, 20.0, 45.0],
            twist: Some((x, [80.0, 85.0])),
        },
        Ball {
            bone: l("Hand_R", "hand_r"),
            zero: -x,
            flexes: z,
            abducts: y,
            stops: [80.0, 70.0, 20.0, 45.0],
            twist: Some((x, [80.0, 85.0])),
        },
        // Ankle: plantar flexion turns the toes down; eversion turns a left
        // sole outward, right-handed about its toes.
        Ball {
            bone: l("Foot_L", "foot_l"),
            zero: foot,
            flexes: foot.cross(-y).normalize(),
            abducts: foot.cross(x).normalize(),
            stops: [45.0, 20.0, 10.0, 10.0],
            twist: Some((foot, [20.0, 30.0])),
        },
        Ball {
            bone: l("Foot_R", "foot_r"),
            zero: foot,
            flexes: foot.cross(-y).normalize(),
            abducts: foot.cross(-x).normalize(),
            stops: [45.0, 20.0, 10.0, 10.0],
            twist: Some((foot, [30.0, 20.0])),
        },
        // The trunk, its range split across its joints.
        Ball {
            bone: "spine_01",
            zero: y,
            flexes: x,
            abducts: -z,
            stops: [90.0 / spine, 30.0 / spine, 30.0 / spine, 30.0 / spine],
            twist: None,
        },
        Ball {
            bone: "neck_01",
            zero: y,
            flexes: x,
            abducts: -z,
            stops: [45.0 / cervical; 4],
            twist: None,
        },
        Ball {
            bone: "head",
            zero: y,
            flexes: x,
            abducts: -z,
            stops: [45.0 / cervical; 4],
            twist: None,
        },
    ]
}

/// **Each ball stops where its range says**: driven from its anatomical
/// zero purely along each anatomical direction — flexion, extension,
/// abduction, adduction, and a lopsided twist both ways — with its soft
/// zones taken out, it stops at the sourced value. A swing axis rolled,
/// a side's sign flipped or an ellipse centred without fitting would each
/// stop it elsewhere. On EW's naming and Unreal's, whose thigh and upper arm
/// carry twist and corrective bones.
#[test]
fn each_ball_stops_where_its_range_says() {
    for naming in [Naming::Ew, Naming::Unreal] {
        let bones = naming.bones(0.0);
        for ball in balls(naming) {
            let drives = [
                (ball.flexes, ball.stops[0], "flexion"),
                (-ball.flexes, ball.stops[1], "extension"),
                (ball.abducts, ball.stops[2], "abduction"),
                (-ball.abducts, ball.stops[3], "adduction"),
            ];
            for (axis, stop, motion) in drives {
                let reached =
                    Drive::new(&bones, ball.bone, ball.zero)
                        .reach(ball.zero, axis, |turn| swung(turn, ball.zero, axis));
                assert!(
                    (reached - stop).abs() < REACH_SLACK,
                    "{naming:?} {}: {motion} stopped at {reached:.2}°, not {stop}°",
                    ball.bone
                );
            }
            if let Some((axis, [forward, back])) = ball.twist {
                for (axis, stop) in [(axis, forward), (-axis, back)] {
                    let reached =
                        Drive::new(&bones, ball.bone, ball.zero)
                            .reach(ball.zero, axis, |turn| twisted(turn, axis));
                    assert!(
                        (reached - stop).abs() < REACH_SLACK,
                        "{naming:?} {}: twist about {axis} stopped at {reached:.2}°, not {stop}°",
                        ball.bone
                    );
                }
            }
        }
    }
    // A shoulder's twist, internal and external 90° (CFR), from its
    // anatomical zero — the arm hanging, reached by the shortest swing from
    // the bind — whether the bind is a T-pose or an A-pose.
    for drop in [0.0, 45.0] {
        let bones = ew(drop);
        for arm in ["UpperArm_L", "UpperArm_R"] {
            for axis in [DVec3::NEG_Y, DVec3::Y] {
                let reached =
                    Drive::new(&bones, arm, DVec3::NEG_Y)
                        .reach(DVec3::NEG_Y, axis, |turn| twisted(turn, axis));
                assert!(
                    (reached - 90.0).abs() < REACH_SLACK,
                    "{arm}, arms dropped {drop}°: twist about {axis} stopped at {reached:.2}°, not 90°"
                );
            }
        }
    }
}

/// The joint origins of `rig`'s bodies on `names`, now.
fn origins(
    physics: &PhysicsSystem,
    ragdoll: &Ragdoll,
    rig: &RagdollRig,
    bones: &[Bone],
    names: [&str; 3],
) -> [DVec3; 3] {
    let at = ragdoll.joint_positions(physics).expect("bodies");
    names.map(|name| at[body(rig, bones, name).expect(name)].1)
}

/// The flexion at the middle of three joint origins, in radians: the bend
/// of the far bone from the near one's line, positive towards `flexes` —
/// the way it flexes at spawn, carried by the near body since.
fn flexion(points: [DVec3; 3], flexes: DVec3) -> f64 {
    let near = (points[1] - points[0]).normalize();
    let far = points[2] - points[1];
    let flexes = (flexes - near * near.dot(flexes)).normalize();
    #[expect(
        clippy::disallowed_methods,
        reason = "measuring the bodies, not simulating"
    )]
    let angle = far.dot(flexes).atan2(far.dot(near));
    angle
}

/// How far past an anatomical stop a joint may be measured, in radians:
/// the stops are the solver's soft-step rows, which a landing compresses —
/// `tests/limits.rs`'s slack for a hinge's limits. Measured on 2026-11-01
/// over every run below, the worst was 0.015 past an elbow's.
const STOP_SLACK: f64 = 0.03;

/// The extremes of the knees' and elbows' flexion over a run.
struct Extremes {
    least: [f64; 4],
    most: [f64; 4],
}

/// The profiled ragdoll of `bones` spawned in `pose`, `lift` metres up,
/// unturned, in [`world`].
fn spawn(rig: &RagdollRig, bones: &[Bone], pose: &Pose, lift: f32) -> (PhysicsSystem, Ragdoll) {
    let skeleton = skeleton(bones);
    let mut physics = world();
    let handoff = Handoff {
        pose,
        model: model(lift, 0.0),
        previous: None,
    };
    let ragdoll = Ragdoll::spawn(
        &mut physics,
        rig,
        &skeleton,
        &handoff,
        &entities(100, rig.bodies().len()),
    )
    .expect("spawned");
    (physics, ragdoll)
}

/// Strikes the body on `name` at its centre with `impulse`, if it is not
/// zero.
fn strike(
    physics: &mut PhysicsSystem,
    ragdoll: &Ragdoll,
    rig: &RagdollRig,
    bones: &[Bone],
    name: &str,
    impulse: DVec3,
) {
    if impulse == DVec3::ZERO {
        return;
    }
    let index = body(rig, bones, name).expect(name);
    let at = physics
        .transform(ragdoll.entities()[index])
        .expect("a body")
        .position;
    ragdoll.strike(physics, impulse, at).expect("struck");
}

/// Spawns `naming`'s profiled ragdoll standing on the floor, strikes the
/// body on `name` with `impulse`, runs `seconds`, and returns the extremes
/// of its knees' and elbows' flexion.
fn struck(naming: Naming, name: &str, impulse: DVec3, seconds: f64) -> Extremes {
    let bones = naming.bones(0.0);
    let rig = generate(&bones, model(0.0, 0.0), &profiled(false));
    let (mut physics, ragdoll) = spawn(&rig, &bones, &Pose::new(&skeleton(&bones)), 0.02);
    strike(&mut physics, &ragdoll, &rig, &bones, name, impulse);
    let limbs = naming.limbs();
    let near_bodies =
        limbs.map(|[near, _, _]| ragdoll.entities()[body(&rig, &bones, near).expect(near)]);
    let rest = near_bodies.map(|e| physics.transform(e).expect("a body").rotation);
    let mut extremes = Extremes {
        least: [f64::INFINITY; 4],
        most: [f64::NEG_INFINITY; 4],
    };
    for _ in 0..(seconds / DT).round() as usize {
        physics.step(DT);
        for (k, names) in limbs.into_iter().enumerate() {
            let now = physics.transform(near_bodies[k]).expect("a body").rotation;
            // A knee flexes its heel backwards, an elbow its hand forwards.
            let flexes = now * rest[k].inverse() * if k < 2 { DVec3::NEG_Z } else { DVec3::Z };
            let angle = flexion(origins(&physics, &ragdoll, &rig, &bones, names), flexes);
            extremes.least[k] = extremes.least[k].min(angle);
            extremes.most[k] = extremes.most[k].max(angle);
        }
    }
    extremes
}

/// **Dropped or struck, a knee never bends forwards and an elbow never
/// hyperextends**, past the stops' give: the ragdoll falls from standing,
/// is struck at the head four ways, at a calf from in front, and at a hand
/// from in front, on every naming. Neither passes its flexion stop either.
#[test]
fn a_dropped_or_struck_ragdoll_bends_no_knee_or_elbow_backwards() {
    for naming in Naming::ALL {
        let [left_leg, _, left_arm, _] = naming.limbs();
        let strikes = [
            (naming.head(), DVec3::ZERO),
            (naming.head(), DVec3::X * 30.0),
            (naming.head(), DVec3::NEG_X * 30.0),
            (naming.head(), DVec3::Z * 30.0),
            (naming.head(), DVec3::NEG_Z * 30.0),
            (left_leg[1], DVec3::NEG_Z * 40.0),
            (left_arm[2], DVec3::NEG_Z * 15.0),
        ];
        for (name, impulse) in strikes {
            let extremes = struck(naming, name, impulse, 4.0);
            for (k, limb) in naming.limbs().into_iter().enumerate() {
                let most = if k < 2 { 140.0_f64 } else { 145.0 }.to_radians();
                assert!(
                    extremes.least[k] > -STOP_SLACK && extremes.most[k] < most + STOP_SLACK,
                    "{naming:?}, {name} struck with {impulse}: {} flexed from {} to {} rad",
                    limb[1],
                    extremes.least[k],
                    extremes.most[k]
                );
            }
        }
    }
}

/// **Every profiled joint's rest is reported against its limits about its
/// neutral.** A T-pose and an A-pose stand inside every limit on every
/// naming: a ball's ellipse passes through its range's stops, which hold
/// both, and a hinge's stop on extension is at a straight bind. Neither is
/// NASA's posture: a straight knee stands 47° from its neutral, and a
/// standing thigh or a hanging arm inside its soft zone — pushed towards the
/// posture, as the owner's rules ask. An elbow bound a degree past straight
/// is inside too, its stop moved to its bind; a knee bound 8.5° past
/// straight is outside, and is the only joint that is: spawned there it
/// snaps inside.
#[test]
fn every_profiled_rest_is_reported_against_its_limits() {
    for naming in Naming::ALL {
        for drop in [0.0, 45.0] {
            let bones = naming.bones(drop);
            let rig = generate(&bones, model(0.0, QUARTER), &profiled(false));
            for joint in rig.joints() {
                let bone = &bones[rig.bodies()[joint.child].joint].0;
                assert!(
                    joint.kind.rest_is_within_limits(),
                    "{naming:?}, drop {drop}: {bone}'s rest is outside {:?}",
                    joint.kind
                );
            }
        }
    }

    // The left hand a degree behind straight: the elbow hyperextended.
    let mut bones = ew(0.0);
    let hand = bones.iter().position(|b| b.0 == "Hand_L").expect("a hand");
    let (sin, cos) = 1.0_f32.to_radians().sin_cos();
    bones[hand].2 = [25.0 * cos, 0.0, -25.0 * sin];
    let rig = generate(&bones, model(0.0, QUARTER), &profiled(false));
    let RigJointKind::Hinge { lower, .. } = joint(&rig, &bones, "lowerarm_l").kind else {
        panic!("an elbow hinge");
    };
    assert!(
        (lower - (-1.0_f64 - 58.0).to_radians()).abs() < 1e-4,
        "{lower}"
    );
    assert!(rig.joints().iter().all(|j| j.kind.rest_is_within_limits()));

    let mut bones = ew(0.0);
    let foot = bones.iter().position(|b| b.0 == "Foot_L").expect("a foot");
    bones[foot].2 = [0.0, -40.0, 6.0];
    let rig = generate(&bones, model(0.0, QUARTER), &profiled(false));
    let outside: Vec<&str> = rig
        .joints()
        .iter()
        .filter(|joint| !joint.kind.rest_is_within_limits())
        .map(|joint| bones[rig.bodies()[joint.child].joint].0.as_str())
        .collect();
    assert_eq!(outside, ["calf_l"]);
}

/// Whether two rigs of the same bones, under different skeletons, have the
/// same bodies on the same bones and the same joints between them, to the
/// bit.
fn same_by_bone(a: &RagdollRig, a_bones: &[Bone], b: &RagdollRig, b_bones: &[Bone]) {
    assert_eq!(a.bodies().len(), b.bodies().len());
    for (index, body_a) in a.bodies().iter().enumerate() {
        let name = &a_bones[body_a.joint].0;
        let other = body(b, b_bones, name).unwrap_or_else(|| panic!("no body on {name}"));
        let body_b = b.bodies()[other];
        assert_eq!(
            (body_a.shape, body_a.mass, body_a.offset),
            (body_b.shape, body_b.mass, body_b.offset),
            "{name}"
        );
        let mine = a.joints().iter().find(|j| j.child == index);
        let theirs = b.joints().iter().find(|j| j.child == other);
        match (mine, theirs) {
            (Some(mine), Some(theirs)) => {
                assert_eq!(
                    a_bones[a.bodies()[mine.parent].joint].0,
                    b_bones[b.bodies()[theirs.parent].joint].0
                );
                assert_eq!(
                    (
                        mine.kind,
                        mine.frame_parent,
                        mine.frame_child,
                        mine.friction
                    ),
                    (
                        theirs.kind,
                        theirs.frame_parent,
                        theirs.frame_child,
                        theirs.friction
                    ),
                    "{name}"
                );
            }
            (None, None) => {}
            _ => panic!("{name} is jointed in one rig only"),
        }
    }
}

/// **Folded bones carry no body, and their parents are fitted as if they
/// were not there**: the helpers, the eyes and eyebrows, the clavicles and
/// the root above the pelvis carry nothing, and with digits folded nor do
/// the fingers and toes; the rig is the one the skeleton without them gets,
/// an upper arm hanging from the chest, to the bit. Without the profile,
/// the eyes make the head a sphere among its children.
#[test]
fn folded_bones_carry_no_body_and_leave_their_parents_alone() {
    let bones = ew(0.0);
    let model = model(0.0, QUARTER);
    let helper = |name: &str| {
        name == "root"
            || name.starts_with("ik_")
            || name.starts_with("clavicle_")
            || name == "eyes"
            || name == "eyebrows"
    };
    let digit = |name: &str| {
        name.starts_with("finger_")
            || name.starts_with("indexFinger_")
            || name.starts_with("thumb_")
            || name.starts_with("toes_")
    };
    for fold_digits in [false, true] {
        let gone = |name: &str| helper(name) || (fold_digits && digit(name));
        let rig = generate(&bones, model, &profiled(fold_digits));
        for (name, ..) in bones.iter().filter(|(name, ..)| gone(name)) {
            assert!(
                body(&rig, &bones, name).is_none(),
                "{name} carries a body, digits folded: {fold_digits}"
            );
        }
        let bare = without(&bones, gone);
        let bare_rig = generate(&bare, model, &profiled(fold_digits));
        same_by_bone(&rig, &bones, &bare_rig, &bare);
    }

    let off = generate(&bones, model, &GenerateSettings::DEFAULT);
    let bare = without(&bones, helper);
    let bare_off = generate(&bare, model, &GenerateSettings::DEFAULT);
    let head =
        |rig: &RagdollRig, bones: &[Bone]| rig.bodies()[body(rig, bones, "head").expect("a head")];
    assert_ne!(head(&off, &bones).offset, head(&bare_off, &bare).offset);
}

/// **Unreal 5's helpers carry no body and leave each limb one bone**: the
/// twist and corrective bones, the metacarpals, the second neck bone, and
/// the `center_of_mass` beside the pelvis; so a thigh is a capsule down its own bone to the knee,
/// and the hip's frame runs down it, rather than a sphere among the twist
/// bones turned towards the pelvis.
#[test]
fn unreal_helpers_leave_each_limb_one_bone() {
    let bones = unreal(0.0);
    let model = model(0.0, 0.0);
    let rig = generate(&bones, model, &profiled(false));
    for (name, ..) in &bones {
        let helper = name.contains("twist")
            || name.contains("corrective")
            || name.contains("metacarpal")
            || name == "center_of_mass"
            || name == "neck_02"
            || name == "root"
            || name.starts_with("ik_")
            || name.starts_with("clavicle_");
        if helper {
            assert!(body(&rig, &bones, name).is_none(), "{name} carries a body");
        }
    }
    for (bone, child) in [
        ("thigh_l", "calf_l"),
        ("upperarm_l", "lowerarm_l"),
        ("calf_l", "foot_l"),
        ("lowerarm_l", "hand_l"),
    ] {
        let index = body(&rig, &bones, bone).expect(bone);
        assert!(
            matches!(rig.bodies()[index].shape, BodyShape::Capsule { .. }),
            "{bone} is {:?}",
            rig.bodies()[index].shape
        );
        let along = (bone_at(&bones, child, model) - bone_at(&bones, bone, model)).normalize();
        let skeleton = skeleton(&bones);
        let worlds = rig
            .bodies_at(&skeleton, &Pose::new(&skeleton), model)
            .expect("placed");
        let capsule_axis = worlds[index].rotation * DVec3::Y;
        assert!(
            capsule_axis.dot(along) > 1.0 - 1e-9,
            "{bone}'s capsule is off its bone"
        );
    }
    let hip = joint(&rig, &bones, "thigh_l");
    let skeleton = skeleton(&bones);
    let worlds = rig
        .bodies_at(&skeleton, &Pose::new(&skeleton), model)
        .expect("placed");
    let frame_z = worlds[hip.child].rotation * hip.frame_child.rotation * DVec3::Z;
    assert!(
        frame_z.dot(DVec3::NEG_Y) > 1.0 - 1e-9,
        "the hip's frame runs {frame_z}"
    );
}

/// **A twist bone in the limb's chain is spliced, not folded with the limb
/// below it**: Unreal 5's humanoid with its left forearm hung from the upper
/// arm's twist bone, as Daz hangs `lForearmBend` from `lShldrTwist`, keeps
/// its forearm and hand, the elbow a hinge, and the upper arm a capsule down
/// to the elbow.
#[test]
fn a_twist_bone_in_the_chain_keeps_the_limb_below() {
    let mut bones = unreal(0.0);
    let at = |bones: &[Bone], name: &str| bones.iter().position(|b| b.0 == name).expect(name);
    let (twist, lower) = (at(&bones, "upperarm_twist_01_l"), at(&bones, "lowerarm_l"));
    let [tx, ty, tz] = bones[twist].2;
    let [lx, ly, lz] = bones[lower].2;
    bones[lower].1 = Some(twist);
    bones[lower].2 = [lx - tx, ly - ty, lz - tz];
    let model = model(0.0, 0.0);
    let rig = generate(&bones, model, &profiled(false));
    assert!(body(&rig, &bones, "upperarm_twist_01_l").is_none());
    assert!(
        body(&rig, &bones, "hand_l").is_some(),
        "the hand went with the twist bone"
    );
    assert!(matches!(
        joint(&rig, &bones, "lowerarm_l").kind,
        RigJointKind::Hinge { .. }
    ));
    let upper = body(&rig, &bones, "upperarm_l").expect("an upper arm");
    let BodyShape::Capsule {
        half_height,
        radius,
    } = rig.bodies()[upper].shape
    else {
        panic!("the upper arm is {:?}", rig.bodies()[upper].shape);
    };
    let elbow = bone_at(&bones, "lowerarm_l", model).distance(bone_at(&bones, "upperarm_l", model));
    assert!(
        (2.0 * (half_height + radius) - elbow).abs() < 1e-6,
        "{half_height} {radius} {elbow}"
    );
}

/// **Toes straight off the foot are the ball of the foot**: with no ball
/// bone, the toes' joint is the metatarsophalangeal hinge, which bends up
/// as well as down, not a toe's that bends only down.
#[test]
fn toes_with_no_ball_bend_both_ways() {
    let bones = without(&ew(0.0), |name| name.starts_with("ball_"));
    let rig = generate(&bones, model(0.0, 0.0), &profiled(false));
    let RigJointKind::Hinge { lower, upper, .. } = joint(&rig, &bones, "toes_l").kind else {
        panic!("a hinge at the toes");
    };
    assert!(
        lower < -0.5 && upper > 0.0,
        "the toes bend from {lower} to {upper}"
    );
}

/// **A head with no bone past it is a head's weight**: EW's head, its eyes
/// and eyebrows folded and nothing else above the neck, is a sphere whose
/// mass is within a sixth of de Leva's head (6.68% of the body for women,
/// 6.94% for men: "Adjustments to Zatsiorsky-Seluyanov's segment inertia
/// parameters", J. Biomech. 29(9), 1996, table 4), as are Mixamo's, sized
/// from its crown end bone, Unreal 5's, with no face bones at all, and a
/// long-necked Unreal 5 humanoid's, whose neck bones are long and short and
/// whose face sits close over its head joint: the head is sized from the
/// body's height, which none of them was chosen for. Measured on
/// 2026-11-02: 7.80%, 7.73%, 7.88% and 7.13%.
#[test]
fn a_head_weighs_a_heads_share() {
    let humanoids =
        Naming::ALL.map(|naming| (format!("{naming:?}"), naming.bones(0.0), naming.head()));
    for (naming, bones, head) in
        humanoids
            .into_iter()
            .chain([("long-necked".to_owned(), long_necked(), "head")])
    {
        let rig = generate(&bones, model(0.0, 0.0), &profiled(false));
        let head = rig.bodies()[body(&rig, &bones, head).expect("a head")];
        assert!(
            matches!(head.shape, BodyShape::Sphere { .. }),
            "{:?}",
            head.shape
        );
        let share = 100.0 * head.mass / rig.mass();
        assert!(
            (6.68 * 5.0 / 6.0..=6.94 * 7.0 / 6.0).contains(&share),
            "{naming}'s head is {share:.2}% of the body"
        );
    }
}

/// **Names that are not one per joint are refused**, whether the profile
/// would read them or not.
#[test]
fn names_not_one_per_joint_are_refused() {
    let bones = ew(0.0);
    let skeleton = skeleton(&bones);
    let names = names(&bones);
    for settings in [GenerateSettings::DEFAULT, profiled(false)] {
        assert_eq!(
            RagdollRig::generate(&skeleton, &names[1..], model(0.0, 0.0), &settings),
            Err(GenerateError::NameCount {
                names: names.len() - 1,
                joints: names.len(),
            })
        );
    }
}

/// The creep test's strikes at the head: none, and 30 N·s along `±x`.
const STRIKES: [DVec3; 3] = [
    DVec3::ZERO,
    DVec3::new(30.0, 0.0, 0.0),
    DVec3::new(-30.0, 0.0, 0.0),
];

/// How long a profiled ragdoll may take to sleep, in seconds: the creep
/// test's bound for the default rig.
const SLEEP_WITHIN: f64 = 15.0;

/// **A profiled ragdoll falls asleep on a plane** under the creep test's
/// conditions: EW's, standing in its bind pose two centimetres up, unstruck
/// or struck at the head along `±x`, at the default substeps.
#[test]
fn a_profiled_ragdoll_sleeps_on_a_plane() {
    let bones = ew(0.0);
    let rig = generate(&bones, model(0.0, 0.0), &profiled(false));
    assert_eq!(rig.substeps(), GenerateSettings::DEFAULT.substeps);
    for impulse in STRIKES {
        let (mut physics, ragdoll) = spawn(&rig, &bones, &Pose::new(&skeleton(&bones)), 0.02);
        strike(&mut physics, &ragdoll, &rig, &bones, "head", impulse);
        let ticks = (SLEEP_WITHIN / DT).round() as usize;
        let slept = (0..ticks).any(|_| {
            physics.step(DT);
            ragdoll.is_settled(&physics)
        });
        assert!(
            slept,
            "struck with {impulse}, still awake after {SLEEP_WITHIN} s"
        );
    }
}

/// The mechanical energy of the bodies: kinetic plus potential under
/// standard gravity, in joules.
fn energy(physics: &PhysicsSystem, ragdoll: &Ragdoll) -> f64 {
    ragdoll
        .entities()
        .iter()
        .map(|&entity| {
            let body = physics.body(entity).expect("a body");
            let at: Transform = *physics.transform(entity).expect("a body");
            body.kinetic_energy(at.rotation) + body.mass * 9.81 * at.position.y
        })
        .sum()
}

/// Runs `rig` from `pose`, `lift` metres up, struck at the head with each
/// of five impulses, and panics if its mechanical energy ever rises above
/// what it had once struck, before it sleeps or 15 s pass.
fn never_gains_energy(
    label: &str,
    rig: &RagdollRig,
    bones: &[Bone],
    head: &str,
    pose: &Pose,
    lift: f32,
) {
    for impulse in [
        STRIKES[0],
        STRIKES[1],
        STRIKES[2],
        DVec3::Z * 30.0,
        DVec3::NEG_Z * 30.0,
    ] {
        let (mut physics, ragdoll) = spawn(rig, bones, pose, lift);
        strike(&mut physics, &ragdoll, rig, bones, head, impulse);
        let start = energy(&physics, &ragdoll);
        for tick in 0..(SLEEP_WITHIN / DT).round() as usize {
            physics.step(DT);
            let now = energy(&physics, &ragdoll);
            assert!(
                now <= start + 1e-6 * start.abs().max(1.0),
                "{label}, struck with {impulse}, tick {tick}: energy rose from {start} to {now}"
            );
            if ragdoll.is_settled(&physics) {
                break;
            }
        }
    }
}

/// The humanoid in NASA's posture where it bends the knees, hips and
/// elbows: the pose no soft zone of the profile resists.
fn nasa_posture(bones: &[Bone], naming: Naming) -> Pose {
    let skeleton = skeleton(bones);
    let mut pose = Pose::new(&skeleton);
    let degrees = f32::to_radians;
    let [left_leg, right_leg, left_arm, right_arm] = naming.limbs();
    for (index, (name, ..)) in bones.iter().enumerate() {
        let turn = if name == left_leg[1] || name == right_leg[1] {
            Quat::from_rotation_x(degrees(47.0))
        } else if name == left_leg[0] {
            Quat::from_rotation_z(degrees(12.0)) * Quat::from_rotation_x(degrees(-52.0))
        } else if name == right_leg[0] {
            Quat::from_rotation_z(degrees(-12.0)) * Quat::from_rotation_x(degrees(-52.0))
        } else if name == left_arm[1] {
            Quat::from_rotation_y(degrees(-58.0))
        } else if name == right_arm[1] {
            Quat::from_rotation_y(degrees(58.0))
        } else {
            continue;
        };
        pose.locals_mut()[index].rotation = turn;
    }
    pose
}

/// **A profiled ragdoll never gains energy**, every naming: dropped from
/// half a metre in the posture its zones leave free — a bind pose starts
/// knees, elbows and hips inside their zones, which push them towards it,
/// energy a spring held that kinetic and potential energy alone count as
/// gained — unstruck or struck at the head along `±x` and `±z`.
#[test]
fn a_profiled_ragdoll_never_gains_energy() {
    for naming in Naming::ALL {
        let bones = naming.bones(0.0);
        let rig = generate(&bones, model(0.0, 0.0), &profiled(false));
        let pose = nasa_posture(&bones, naming);
        never_gains_energy(
            &format!("{naming:?}"),
            &rig,
            &bones,
            naming.head(),
            &pose,
            0.5,
        );
    }
}

/// **Its tight ranges never gain energy from the bind either**: every
/// naming with its soft zones taken out, so nothing a spring held is
/// released, dropped from the T-pose and the A-pose — Unreal 5's five-bone
/// spine of 18° of flexion a joint, its two necks, and every ankle's 10° of
/// toeing in and out among them.
#[test]
fn its_hard_limits_never_gain_energy_from_the_bind() {
    for naming in Naming::ALL {
        for drop in [0.0, 45.0] {
            let bones = naming.bones(drop);
            let rig = without_zones(&generate(&bones, model(0.0, 0.0), &profiled(false)));
            let label = format!("{naming:?}, drop {drop}");
            let pose = Pose::new(&skeleton(&bones));
            never_gains_energy(&label, &rig, &bones, naming.head(), &pose, 0.02);
        }
    }
}

/// The profiled state, freshly spawned and two seconds into the unstruck
/// fall, taken on x86-64 Linux on 2026-11-02: the first is the profile's,
/// the generator's and the spawn's work, the second the solver's.
const PINNED_AT_SPAWN: u64 = 0x8c80_081f_df1a_6f5d;
const PINNED_AT_TWO_SECONDS: u64 = 0x0e99_03fe_90ba_0a6f;

/// **The profiled ragdoll is the same to the bit on every target**, spawned
/// and after two seconds: the profile measures angles with
/// `crcbl_core::trig::atan2` and turns them with `rotation_from_scaled_axis`,
/// so no platform transcendental reaches the rig.
#[test]
fn the_profiled_ragdoll_steps_to_the_same_bits_on_every_target() {
    let bones = ew(0.0);
    let rig = generate(&bones, model(0.0, 0.0), &profiled(false));
    let (mut physics, _) = spawn(&rig, &bones, &Pose::new(&skeleton(&bones)), 0.02);
    let at_spawn = state_hash(&physics);
    for _ in 0..120 {
        physics.step(DT);
    }
    let at_two_seconds = state_hash(&physics);
    assert_eq!(
        (at_spawn, at_two_seconds),
        (PINNED_AT_SPAWN, PINNED_AT_TWO_SECONDS),
        "spawned {at_spawn:#018x}, after two seconds {at_two_seconds:#018x}"
    );
}
