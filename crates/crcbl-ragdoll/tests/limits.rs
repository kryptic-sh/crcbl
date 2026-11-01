//! A rig file's joint limits beyond a hard range — a neutral the limits are
//! measured from, an elliptical swing, soft zones — and its folded bones.

mod common;

use common::{EVERYTHING, LEG, entities, index, model, names, skeleton, world};
use crcbl_anim::{Palette, Pose, Skeleton};
use crcbl_phys::{
    ContactSettings, JointKind, PhysicsSystem, RevoluteJoint, SphericalJoint, Spring, Transform,
};
use crcbl_ragdoll::{
    DescError, Handoff, JointKindDesc, Ragdoll, RagdollDesc, RagdollRig, RigError, RigJoint,
    RigJointKind, SoftRange, SoftSwing, SwingTwist,
};
use glam::{DQuat, DVec3, Mat4, Quat};

const DT: f64 = 1.0 / 60.0;

fn resolve(text: &str) -> Result<RagdollRig, DescError> {
    RagdollDesc::from_ron(text)?.resolve(&skeleton(), &names(), model(0.0))
}

/// `rig` spawned at `pose`, a metre up, in `physics`.
fn spawn(physics: &mut PhysicsSystem, rig: &RagdollRig, pose: &Pose) -> Ragdoll {
    let handoff = Handoff {
        pose,
        model: model(1.0),
        previous: None,
    };
    Ragdoll::spawn(
        physics,
        rig,
        &skeleton(),
        &handoff,
        &entities(1, rig.bodies().len()),
    )
    .expect("spawned")
}

/// Each spawned joint's physics kind.
fn kinds(physics: &PhysicsSystem, ragdoll: &Ragdoll) -> Vec<JointKind> {
    ragdoll
        .joints()
        .iter()
        .map(|&id| physics.joint(id).expect("a joint").kind)
        .collect()
}

/// **Every new field survives the file**: read, written and read again, the
/// rig is the one it was; the optional fields are written bare, as they are
/// read, and a rig using none of them writes none of them.
#[test]
fn every_new_field_round_trips_through_the_file() {
    let desc = RagdollDesc::from_ron(EVERYTHING).expect("parses");
    let JointKindDesc::Ball {
        neutral,
        soft_swing,
        soft_twist,
        ..
    } = desc.joints[0].kind
    else {
        panic!("the hip is a ball");
    };
    assert_eq!(
        (neutral, soft_swing, soft_twist),
        (
            SwingTwist {
                swing_x: 0.3,
                swing_y: -0.1,
                twist: 0.05
            },
            Some(SoftSwing {
                x: 0.6,
                y: 0.3,
                hertz: 4.0,
                damping_ratio: 0.7
            }),
            Some(SoftRange {
                lower: -0.2,
                upper: 0.1,
                hertz: 3.0,
                damping_ratio: 0.5
            }),
        )
    );
    assert_eq!(desc.fold, ["foot_l"]);
    let text = desc.to_ron();
    assert!(!text.contains("Some("), "written bare:\n{text}");
    assert_eq!(RagdollDesc::from_ron(&text).expect("reads back"), desc);

    let plain = RagdollDesc::from_ron(LEG).expect("parses").to_ron();
    for absent in ["neutral", "soft", "fold", "cone"] {
        assert!(!plain.contains(absent), "{absent} written:\n{plain}");
    }
    assert!(plain.contains("swing_x: 1.2") && plain.contains("swing_y: 1.2"));
}

/// **An old file's `cone` reads as both half-angles, and spawns the joint it
/// always did**: the same physics joint as layer 1's circular cone, and a
/// file spelling the cone as two equal half-angles spawns it to the same
/// bits. A file giving both spellings, or one half-angle alone, is refused.
#[test]
fn an_old_cone_reads_as_both_half_angles() {
    let rig = resolve(LEG).expect("a rig");
    let mut physics = world();
    let ragdoll = spawn(&mut physics, &rig, &Pose::new(&skeleton()));
    assert_eq!(
        kinds(&physics, &ragdoll)[0],
        JointKind::Spherical(
            SphericalJoint::ball()
                .with_cone(1.2)
                .with_twist(-0.4, 0.4)
                .with_motor(DVec3::ZERO, 0.5)
        )
    );

    let spelled = LEG.replace("cone: 1.2", "swing_x: 1.2, swing_y: 1.2");
    let twin = resolve(&spelled).expect("a rig");
    assert_eq!(twin, rig);
    let mut other = world();
    let again = spawn(&mut other, &twin, &Pose::new(&skeleton()));
    for _ in 0..60 {
        physics.step(DT);
        other.step(DT);
    }
    assert_eq!(
        ragdoll.body_transforms(&physics),
        again.body_transforms(&other),
        "the two spellings stepped apart"
    );

    for refused in [
        LEG.replace("cone: 1.2", "cone: 1.2, swing_x: 1.2"),
        LEG.replace("cone: 1.2", "swing_x: 1.2"),
    ] {
        assert!(
            matches!(resolve(&refused), Err(DescError::Parse { .. })),
            "{refused}"
        );
    }
}

/// The child's bend from rest about the hinge's axis, in radians: its turn
/// relative to the parent body now, against the same at rest, about the axis
/// in the parent body's frame.
fn bend(rest: (Transform, Transform), now: (Transform, Transform), axis: DVec3) -> f64 {
    let relative =
        |(parent, child): (Transform, Transform)| parent.rotation.inverse() * child.rotation;
    let turn = relative(now) * relative(rest).inverse();
    let along = DVec3::new(turn.x, turn.y, turn.z).dot(axis);
    #[expect(
        clippy::disallowed_methods,
        reason = "measuring the bodies, not simulating"
    )]
    let half = along.atan2(turn.w);
    let angle = 2.0 * half;
    // The shorter way round.
    if angle > core::f64::consts::PI {
        angle - core::f64::consts::TAU
    } else if angle < -core::f64::consts::PI {
        angle + core::f64::consts::TAU
    } else {
        angle
    }
}

/// A thigh and a calf hinged at the knee, about the calf's `x`, with `kind`.
fn knee(kind: &str) -> RagdollRig {
    resolve(&format!(
        r#"(
            bodies: [
                (bone: "thigh_l", shape: Capsule(radius: 0.06, half_height: 0.15), mass: 8.0,
                    centre: (0.0, -0.225, 0.0)),
                (bone: "calf_l", shape: Capsule(radius: 0.05, half_height: 0.15), mass: 4.0,
                    centre: (0.0, -0.225, 0.0)),
            ],
            joints: [(parent: "thigh_l", child: "calf_l", kind: {kind}, axis: (1.0, 0.0, 0.0))],
        )"#
    ))
    .expect("a rig")
}

/// Every bend `rig`'s knee takes, tick by tick, in a world with no gravity:
/// left alone for a second, then the calf spun at `speed` rad/s about the
/// axis one way for a second and the other way for another.
fn bends(rig: &RagdollRig, speed: f64) -> Vec<f64> {
    let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    let ragdoll = spawn(&mut physics, rig, &Pose::new(&skeleton()));
    let [thigh, calf] = [ragdoll.entities()[0], ragdoll.entities()[1]];
    let pair = |physics: &PhysicsSystem| {
        (
            *physics.transform(thigh).expect("a thigh"),
            *physics.transform(calf).expect("a calf"),
        )
    };
    let rest = pair(&physics);
    let axis = rig.joints()[0].frame_parent.rotation * DVec3::Z;
    let mut bends = Vec::new();
    for spin in [0.0, speed, -speed] {
        let world_axis = rest.0.rotation * axis;
        let body = physics.body_mut(calf).expect("a calf");
        body.angular_velocity = world_axis * spin;
        physics.body_mut(thigh).expect("a thigh").angular_velocity = DVec3::ZERO;
        for _ in 0..60 {
            physics.step(DT);
            bends.push(bend(rest, pair(&physics), axis));
        }
    }
    bends
}

/// How far past its limits a hinge may be measured, in radians.
const LIMIT_SLACK: f64 = 0.03;

/// **A neutral moves where the limits sit**: a hinge whose neutral is 0.8
/// rad from rest and whose limits are 0.3 either side of it is pushed out of
/// rest into its range, and held between 0.5 and 1.1 rad from rest whichever
/// way it is spun, reaching each end.
#[test]
fn a_neutral_moves_where_the_limits_sit() {
    let rig = knee("Hinge(lower: -0.3, upper: 0.3, neutral: 0.8)");
    let bends = bends(&rig, 3.0);
    // The first second pushes it from rest into its range.
    let held = &bends[60..];
    let (least, most) = held
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &b| {
            (lo.min(b), hi.max(b))
        });
    assert!(
        least > 0.5 - LIMIT_SLACK && most < 1.1 + LIMIT_SLACK,
        "the knee bent from {least} to {most} rad from rest, outside 0.5 to 1.1"
    );
    assert!(
        (most - 1.1).abs() < LIMIT_SLACK && (least - 0.5).abs() < LIMIT_SLACK,
        "the knee reached {least} to {most} rad, not both ends of 0.5 to 1.1"
    );
}

/// **A soft zone from the file is layer 1's soft zone**: the spawned hinge
/// and ball are the physics joints its builders make, and the hinge meets
/// resistance past its soft bound — spun gently it turns back well inside
/// the hard limit it reaches without the zone, having passed the soft bound
/// freely.
#[test]
fn a_soft_zone_from_the_file_resists_before_the_stop() {
    let zone = Spring {
        hertz: 20.0,
        damping_ratio: 0.5,
    };
    let soft = knee(
        "Hinge(lower: -1.0, upper: 1.0, soft: (lower: -0.2, upper: 0.2, hertz: 20.0, damping_ratio: 0.5))",
    );
    let mut physics = world();
    let ragdoll = spawn(&mut physics, &soft, &Pose::new(&skeleton()));
    assert_eq!(
        kinds(&physics, &ragdoll)[0],
        JointKind::Revolute(
            RevoluteJoint::hinge()
                .with_limits(-1.0, 1.0)
                .with_motor(0.0, 0.0)
                .with_soft_limits(-0.2, 0.2, zone)
        )
    );

    let reach = |rig: &RagdollRig| {
        bends(rig, 3.0)
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let hard = reach(&knee("Hinge(lower: -1.0, upper: 1.0)"));
    let resisted = reach(&soft);
    assert!(
        hard > 1.0 - LIMIT_SLACK,
        "with no zone it stops at {hard}, short of its hard limit"
    );
    assert!(
        resisted > 0.2 && resisted < 0.5,
        "with the zone it reached {resisted}: free to 0.2, then resisted"
    );

    let rig = resolve(EVERYTHING).expect("a rig");
    let mut physics = world();
    let ragdoll = spawn(&mut physics, &rig, &Pose::new(&skeleton()));
    let hip = SphericalJoint::ball()
        .with_swing(1.0, 0.5)
        .with_twist(-0.4, 0.4)
        .with_motor(DVec3::ZERO, 0.0)
        .with_soft_swing(
            0.6,
            0.3,
            Spring {
                hertz: 4.0,
                damping_ratio: 0.7,
            },
        )
        .with_soft_twist(
            -0.2,
            0.1,
            Spring {
                hertz: 3.0,
                damping_ratio: 0.5,
            },
        );
    assert_eq!(kinds(&physics, &ragdoll)[0], JointKind::Spherical(hip));
}

/// **Limits a physics joint would refuse are refused by the rig**, each
/// naming its joint: a soft zone outside its hard range, a lone zero swing
/// half-angle, and a neutral past a half turn.
#[test]
fn limits_out_of_range_are_refused() {
    let invalid = Err(DescError::Rig(RigError::InvalidJoint { joint: 1 }));
    for (from, to) in [
        ("soft: (lower: -0.5", "soft: (lower: -1.5"),
        ("upper: 0.2, hertz: 6.0", "upper: 0.2, hertz: -6.0"),
        ("neutral: 0.4", "neutral: 4.0"),
    ] {
        assert_eq!(resolve(&EVERYTHING.replace(from, to)), invalid, "{to}");
    }
    let invalid = Err(DescError::Rig(RigError::InvalidJoint { joint: 0 }));
    for (from, to) in [
        ("swing_x: 1.0,", "swing_x: 0.0,"),
        ("x: 0.6, y: 0.3", "x: 0.6, y: 0.6"),
        ("lower: -0.2, upper: 0.1", "lower: 0.2, upper: 0.1"),
        ("neutral: (swing_x: 0.3", "neutral: (swing_x: 3.3"),
    ] {
        assert_eq!(resolve(&EVERYTHING.replace(from, to)), invalid, "{to}");
    }
    assert!(resolve(EVERYTHING).is_ok());
}

/// Every joint's world transform for `pose` placed by `model`.
fn globals(skeleton: &Skeleton, pose: &Pose, model: Mat4) -> Vec<Mat4> {
    let mut palette = Palette::new(skeleton);
    palette.compute(skeleton, pose);
    palette
        .globals()
        .iter()
        .map(|global| model * *global)
        .collect()
}

/// **A folded bone gets no body, and one that would carry a body is
/// refused** by name. The rigid following is not this crate's: `crcbl_anim`'s
/// read-back gives every bone without a body its handoff turn from its
/// parent. The motion part pins that a folded bone gets it — knocked about,
/// the foot keeps the turn it had from the calf at the handoff — so it fails
/// only if that read-back changes. A fold with nothing to follow is
/// [`a_fold_with_nothing_to_follow_is_refused`].
#[test]
fn a_folded_bone_moves_rigidly_with_its_parent() {
    let rig = resolve(EVERYTHING).expect("a rig");
    let foot = index("foot_l");
    assert!(rig.bodies().iter().all(|body| body.joint != foot));

    let skeleton = skeleton();
    let mut handoff = Pose::new(&skeleton);
    handoff.locals_mut()[foot].rotation = Quat::from_xyzw(0.247, 0.0, 0.0, 0.969).normalize();
    let held = handoff.locals()[foot].to_mat4();
    let mut physics = world();
    let mut ragdoll = spawn(&mut physics, &rig, &handoff);
    let calf_body = rig
        .bodies()
        .iter()
        .position(|body| body.joint == index("calf_l"))
        .expect("a calf body");
    physics
        .apply_impulse_at(
            ragdoll.entities()[calf_body],
            DVec3::new(3.0, 0.0, -4.0),
            physics
                .transform(ragdoll.entities()[calf_body])
                .expect("a calf")
                .position,
        )
        .expect("an impulse");
    let mut pose = handoff.clone();
    for _ in 0..90 {
        physics.step(DT);
    }
    ragdoll
        .pose_into(&physics, ragdoll.handoff_model(), &mut pose)
        .expect("a pose");
    let worlds = globals(&skeleton, &pose, ragdoll.handoff_model());
    let calf = worlds[index("calf_l")];
    let start = globals(&skeleton, &handoff, ragdoll.handoff_model())[index("calf_l")];
    assert!(
        !calf.abs_diff_eq(start, 1e-3),
        "the calf never moved, so following it shows nothing"
    );
    let relative = calf.inverse() * worlds[foot];
    assert!(
        relative.abs_diff_eq(held, 1e-4),
        "the foot moved against the calf: {relative:?} against {held:?}"
    );

    let bodied = EVERYTHING.replace("fold: [\"foot_l\"]", "fold: [\"calf_l\"]");
    assert_eq!(
        resolve(&bodied),
        Err(DescError::FoldedBody("calf_l".to_owned()))
    );
    let unknown = EVERYTHING.replace("fold: [\"foot_l\"]", "fold: [\"toe\"]");
    assert_eq!(
        resolve(&unknown),
        Err(DescError::UnknownBone("toe".to_owned()))
    );
}

/// **A fold with no body above it to follow is refused**: with only the
/// thigh and calf carrying bodies, the hips are a bodiless root and the spine
/// hangs under nothing but them.
#[test]
fn a_fold_with_nothing_to_follow_is_refused() {
    for bone in ["hips", "spine"] {
        let text = format!(
            r#"(
                bodies: [
                    (bone: "thigh_l", shape: Capsule(radius: 0.06, half_height: 0.15), mass: 8.0),
                    (bone: "calf_l", shape: Capsule(radius: 0.05, half_height: 0.15), mass: 4.0),
                ],
                fold: ["foot_l", "{bone}"],
            )"#
        );
        assert_eq!(
            resolve(&text),
            Err(DescError::FoldedWithoutParent(bone.to_owned())),
            "{bone}"
        );
    }
}

/// A frame whose turn is deliberately not of unit length, so that any
/// arithmetic on it — even a turn by the identity, then normalised — changes
/// its bits.
fn unnormalised(position: DVec3) -> Transform {
    Transform::new(position, DQuat::from_xyzw(0.1, 0.2, 0.3, 0.9))
}

/// **A side the neutral does not turn is the stored frame to the bit**: a
/// joint whose neutral is rest spawns its frames exactly as stored, a ball
/// with only a neutral twist keeps its parent's frame and one with only a
/// neutral swing its child's. The frames are not of unit length, so a
/// computed turn by the identity would show.
#[test]
fn a_side_the_neutral_leaves_is_the_stored_frame() {
    let ball = |neutral: SwingTwist| RigJointKind::Ball {
        swing_x: 0.5,
        swing_y: 0.5,
        twist_lower: -0.2,
        twist_upper: 0.2,
        neutral,
        soft_swing: None,
        soft_twist: None,
    };
    let joint = |kind: RigJointKind| RigJoint {
        parent: 0,
        child: 1,
        frame_parent: unnormalised(DVec3::X),
        frame_child: unnormalised(DVec3::Y),
        kind,
        friction: 0.0,
    };
    let rest = [
        ball(SwingTwist::REST),
        RigJointKind::Hinge {
            lower: -1.0,
            upper: 1.0,
            neutral: 0.0,
            soft: None,
        },
        RigJointKind::Fixed,
    ];
    for kind in rest {
        let joint = joint(kind);
        assert_eq!(
            joint.physics_frames(),
            (joint.frame_parent, joint.frame_child),
            "{kind:?}"
        );
    }
    let twisted = joint(ball(SwingTwist {
        twist: 0.3,
        ..SwingTwist::REST
    }));
    let (parent, child) = twisted.physics_frames();
    assert_eq!(parent, twisted.frame_parent);
    assert_ne!(child, twisted.frame_child);
    let swung = joint(ball(SwingTwist {
        swing_x: 0.3,
        ..SwingTwist::REST
    }));
    let (parent, child) = swung.physics_frames();
    assert_ne!(parent, swung.frame_parent);
    assert_eq!(child, swung.frame_child);
}

/// The two-body ball of [`a_balls_neutral_keeps_its_ellipse_on_the_parent`]:
/// hips and thigh, its swing half-angles 0.3 about x and 0.8 about y, its
/// twist 0.2 either way, all about a neutral swing of 0.5 about x and twist
/// of 0.9.
const NEUTRAL_HIP: &str = r#"(
    bodies: [
        (bone: "hips", shape: Sphere(radius: 0.12), mass: 12.0),
        (bone: "thigh_l", shape: Capsule(radius: 0.06, half_height: 0.15), mass: 8.0,
            centre: (0.0, -0.225, 0.0)),
    ],
    joints: [(
        parent: "hips",
        child: "thigh_l",
        kind: Ball(
            swing_x: 0.3,
            swing_y: 0.8,
            twist_lower: -0.2,
            twist_upper: 0.2,
            neutral: (swing_x: 0.5, twist: 0.9),
        ),
        axis: (0.0, -1.0, 0.0),
    )],
)"#;

/// The ball's swing and twist from its neutral, in radians, as `physics`
/// holds the bodies, measured against frames built here from the rig's rest
/// frames rather than through [`RigJoint::physics_frames`]: the parent's
/// rest frame tipped by the neutral swing `swing`, and the child's frame,
/// whose turn from that is the neutral twist `twist` at the neutral.
struct Hip {
    parent: crcbl_ecs::Entity,
    child: crcbl_ecs::Entity,
    frames: (Transform, Transform),
    swing: DQuat,
    twist: DQuat,
}

impl Hip {
    /// The neutral parent frame and the child frame, in the world.
    fn world_frames(&self, physics: &PhysicsSystem) -> (DQuat, DQuat) {
        let parent = physics.transform(self.parent).expect("hips");
        let child = physics.transform(self.child).expect("a thigh");
        (
            parent.rotation * self.frames.0.rotation * self.swing,
            child.rotation * self.frames.1.rotation,
        )
    }

    /// The child's swing from the neutral, and its twist from the neutral.
    fn reading(&self, physics: &PhysicsSystem) -> (f64, f64) {
        let (parent, child) = self.world_frames(physics);
        let turn = parent.inverse() * child;
        let tipped = turn * DVec3::Z;
        #[expect(
            clippy::disallowed_methods,
            reason = "measuring the bodies, not simulating"
        )]
        let (swing, twist) = (
            DVec3::Z.cross(tipped).length().atan2(DVec3::Z.dot(tipped)),
            2.0 * (turn.z * turn.w.signum()).atan2(turn.w.abs()),
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "measuring the bodies, not simulating"
        )]
        let neutral_twist = 2.0 * self.twist.z.atan2(self.twist.w);
        (swing, twist - neutral_twist)
    }

    /// Both bodies still, the child turned to the neutral about the joint.
    fn to_neutral(&self, physics: &mut PhysicsSystem) {
        let parent = *physics.transform(self.parent).expect("hips");
        let origin = parent.position + parent.rotation * self.frames.0.position;
        let rotation = (parent.rotation * self.frames.0.rotation * self.swing * self.twist)
            * self.frames.1.rotation.inverse();
        let position = origin - rotation * self.frames.1.position;
        physics.set_transform(self.child, Transform::new(position, rotation.normalize()));
        for entity in [self.parent, self.child] {
            let body = physics.body_mut(entity).expect("a body");
            body.velocity = DVec3::ZERO;
            body.angular_velocity = DVec3::ZERO;
        }
    }

    /// From the neutral, the child spun at `speed` rad/s about `axis` (the
    /// world direction it returns, given the two frames) for a second:
    /// every reading on the way.
    fn spun(
        &self,
        physics: &mut PhysicsSystem,
        speed: f64,
        axis: impl Fn(DQuat, DQuat) -> DVec3,
    ) -> Vec<(f64, f64)> {
        self.to_neutral(physics);
        let (parent, child) = self.world_frames(physics);
        physics
            .body_mut(self.child)
            .expect("a thigh")
            .angular_velocity = axis(parent, child) * speed;
        (0..60)
            .map(|_| {
                physics.step(DT);
                self.reading(physics)
            })
            .collect()
    }
}

/// **A ball's neutral keeps its ellipse on the parent's axes**: held at its
/// neutral — swung 0.5 about x and twisted 0.9 — it meets no resistance and
/// stays; swung from there about the neutral parent frame's x it stops at the
/// 0.3 half-angle, and about its y at 0.8, so the twist has not turned the
/// ellipse; and twisted from there it stops 0.2 either side of the neutral
/// twist.
#[test]
fn a_balls_neutral_keeps_its_ellipse_on_the_parent() {
    let rig = resolve(NEUTRAL_HIP).expect("a rig");
    let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    let ragdoll = spawn(&mut physics, &rig, &Pose::new(&skeleton()));
    let joint = rig.joints()[0];
    let hip = Hip {
        parent: ragdoll.entities()[0],
        child: ragdoll.entities()[1],
        frames: (joint.frame_parent, joint.frame_child),
        swing: crcbl_phys::rotation_from_scaled_axis(DVec3::X * 0.5),
        twist: crcbl_phys::rotation_from_scaled_axis(DVec3::Z * 0.9),
    };

    hip.to_neutral(&mut physics);
    for _ in 0..30 {
        physics.step(DT);
        let (swing, twist) = hip.reading(&physics);
        assert!(
            swing < 1e-6 && twist.abs() < 1e-6,
            "pushed off its neutral: swing {swing}, twist {twist}"
        );
    }

    let reach = |readings: Vec<(f64, f64)>| {
        readings
            .into_iter()
            .fold(f64::NEG_INFINITY, |most, (swing, _)| most.max(swing))
    };
    let about_x = reach(hip.spun(&mut physics, 2.0, |parent, _| parent * DVec3::X));
    let about_y = reach(hip.spun(&mut physics, 2.0, |parent, _| parent * DVec3::Y));
    assert!(
        (about_x - 0.3).abs() < LIMIT_SLACK && (about_y - 0.8).abs() < LIMIT_SLACK,
        "swung to {about_x} about the parent's x and {about_y} about its y, not 0.3 and 0.8"
    );

    let mut twists = |speed: f64| {
        hip.spun(&mut physics, speed, |_, child| child * DVec3::Z)
            .into_iter()
            .map(|(_, twist)| twist)
            .collect::<Vec<_>>()
    };
    let most = twists(2.0).into_iter().fold(f64::NEG_INFINITY, f64::max);
    let least = twists(-2.0).into_iter().fold(f64::INFINITY, f64::min);
    assert!(
        (most - 0.2).abs() < LIMIT_SLACK && (least + 0.2).abs() < LIMIT_SLACK,
        "twisted {least} to {most} from the neutral twist, not -0.2 to 0.2"
    );
}

/// **A turned roll survives the file**: a joint whose `swing_x_axis` turns
/// its frame from the default comes back with the same frame `x` once written
/// and read again, and a joint at the default roll writes none.
#[test]
fn a_turned_roll_round_trips_through_the_file() {
    let rolled = LEG.replace(
        "axis: (1.0, 0.0, 0.0),",
        "axis: (1.0, 0.0, 0.0), swing_x_axis: (0.3, 1.0, 0.0),",
    );
    let rig = resolve(&rolled).expect("a rig");
    let x_of = |rig: &RagdollRig, joint: usize| {
        let joint = rig.joints()[joint];
        let offset = rig.bodies()[joint.child].offset;
        offset.rotation * joint.frame_child.rotation * DVec3::X
    };
    let x = x_of(&rig, 1);
    assert!(x.distance(DVec3::Y) < 1e-12, "the knee's x is {x}");
    let desc = rig.to_desc(&names()).expect("named");
    assert!(desc.joints[1].swing_x_axis.is_some());
    assert_eq!(
        desc.joints[0].swing_x_axis, None,
        "the hip's roll is the default"
    );
    let back = RagdollDesc::from_ron(&desc.to_ron())
        .expect("reads")
        .resolve(&skeleton(), &names(), model(0.0))
        .expect("resolves");
    for joint in 0..rig.joints().len() {
        let (was, is) = (x_of(&rig, joint), x_of(&back, joint));
        assert!(
            was.distance(is) < 1e-12,
            "joint {joint}: {was} came back {is}"
        );
    }
}

/// **Whether rest lies inside the limits measured from the neutral** — what
/// snaps on the first steps when it does not: a hinge whose neutral is
/// farther from rest than its limit on that side, a ball whose neutral swing
/// or twist is past its range, and their neighbours that are not.
#[test]
fn rest_is_checked_against_the_limits_about_the_neutral() {
    let hinge = |neutral: f64| RigJointKind::Hinge {
        lower: -0.1,
        upper: 0.5,
        neutral,
        soft: None,
    };
    assert!(hinge(0.0).rest_is_within_limits());
    assert!(
        hinge(-0.4).rest_is_within_limits(),
        "rest 0.4 above the neutral"
    );
    assert!(
        !hinge(0.3).rest_is_within_limits(),
        "rest 0.3 below the neutral"
    );
    let ball = |neutral: SwingTwist| RigJointKind::Ball {
        swing_x: 0.3,
        swing_y: 0.8,
        twist_lower: -0.2,
        twist_upper: 0.1,
        neutral,
        soft_swing: None,
        soft_twist: None,
    };
    let at = |swing_x: f64, swing_y: f64, twist: f64| {
        ball(SwingTwist {
            swing_x,
            swing_y,
            twist,
        })
        .rest_is_within_limits()
    };
    assert!(at(0.0, 0.0, 0.0));
    assert!(at(0.25, 0.0, 0.0) && !at(0.35, 0.0, 0.0), "about x");
    assert!(at(0.0, 0.75, 0.0) && !at(0.0, 0.85, 0.0), "about y");
    // Rest twists the other way from the neutral: −0.15 is inside, 0.15 not.
    assert!(at(0.0, 0.0, 0.15) && !at(0.0, 0.0, -0.15), "twist");
    assert!(RigJointKind::Fixed.rest_is_within_limits());
    let rig = resolve(EVERYTHING).expect("a rig");
    assert!(
        rig.joints()
            .iter()
            .all(|joint| joint.kind.rest_is_within_limits())
    );
    assert!(
        !knee("Hinge(lower: -0.3, upper: 0.3, neutral: 0.8)").joints()[0]
            .kind
            .rest_is_within_limits()
    );
}
