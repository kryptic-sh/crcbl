//! The authored rig file: read, resolved against bone names, and spawned.

mod common;

use common::{LEG, entities, index, model, names, skeleton, world};
use crcbl_anim::Pose;
use crcbl_phys::JointKind;
use crcbl_ragdoll::{DescError, Handoff, Ragdoll, RagdollDesc, RigError, RigJointKind};
use glam::Mat4;

fn resolve(text: &str) -> Result<crcbl_ragdoll::RagdollRig, DescError> {
    RagdollDesc::from_ron(text)?.resolve(&skeleton(), &names(), model(0.0))
}

/// **A file resolves into the rig it describes, and spawns its joints with
/// their kinds, limits and friction.**
#[test]
fn a_leg_resolves_and_spawns_its_joints() {
    let rig = resolve(LEG).expect("a rig");
    assert_eq!(rig.bodies().len(), 4);
    assert_eq!(rig.bodies()[1].joint, index("thigh_l"));
    assert_eq!(rig.substeps(), 6);
    assert!(matches!(
        rig.joints()[1].kind,
        RigJointKind::Hinge {
            lower: -2.4,
            upper: 0.0
        }
    ));
    assert_eq!(rig.joints()[0].friction, 0.5);
    assert_eq!(rig.joints()[1].friction, 0.0, "friction defaults to none");
    // The hips sphere reaches the thigh's capsule, but they are jointed.
    assert!(rig.ignored().is_empty(), "{:?}", rig.ignored());

    let skeleton = skeleton();
    let pose = Pose::new(&skeleton);
    let mut physics = world();
    let handoff = Handoff {
        pose: &pose,
        model: model(1.0),
        previous: None,
    };
    let ragdoll =
        Ragdoll::spawn(&mut physics, &rig, &skeleton, &handoff, &entities(10, 4)).expect("spawned");
    let kinds: Vec<JointKind> = ragdoll
        .joints()
        .iter()
        .map(|&id| physics.joint(id).expect("a joint").kind)
        .collect();
    let JointKind::Spherical(hip) = kinds[0] else {
        panic!("the hip is a ball: {:?}", kinds[0]);
    };
    assert!(hip.enable_motor && hip.max_motor_torque == 0.5 && hip.cone_angle == 1.2);
    let JointKind::Revolute(knee) = kinds[1] else {
        panic!("the knee is a hinge: {:?}", kinds[1]);
    };
    assert!(knee.enable_limit && knee.lower_angle == -2.4);
    assert!(matches!(kinds[2], JointKind::Weld(_)));
    for &id in ragdoll.joints() {
        let drift = physics.joint_drift(id).expect("a joint");
        assert!(drift.linear < 1e-5, "a joint starts apart: {drift:?}");
    }
    for &entity in ragdoll.entities() {
        assert_eq!(physics.substeps(entity), Some(6));
    }
}

/// **Listed pairs and pairs overlapping at rest are kept from colliding**
/// once spawned, unless the file turns the overlap rule off.
#[test]
fn ignored_pairs_reach_the_physics_system() {
    // The two thighs, fat enough to overlap below the pelvis, with no joint
    // between them; and a listed pair of calves that never touch.
    let text = r#"(
        bodies: [
            (bone: "thigh_l", shape: Capsule(radius: 0.12, half_height: 0.02), mass: 8.0,
                centre: (0.0, -0.225, 0.0)),
            (bone: "thigh_r", shape: Capsule(radius: 0.12, half_height: 0.02), mass: 8.0,
                centre: (0.0, -0.225, 0.0)),
            (bone: "calf_l", shape: Sphere(radius: 0.05), mass: 2.0),
            (bone: "calf_r", shape: Sphere(radius: 0.05), mass: 2.0),
        ],
        ignore: [("calf_l", "calf_r")],
        OVERLAPS
    )"#;
    let rig = resolve(&text.replace("OVERLAPS", "")).expect("a rig");
    assert_eq!(rig.ignored(), &[(2, 3), (0, 1)]);
    let off = resolve(&text.replace("OVERLAPS", "ignore_rest_overlaps: false,")).expect("a rig");
    assert_eq!(off.ignored(), &[(2, 3)]);

    let skeleton = skeleton();
    let pose = Pose::new(&skeleton);
    // The overlap is there to be asked of either rig, and the listed calves,
    // which never touch, are not part of it.
    for rig in [&rig, &off] {
        assert_eq!(
            rig.rest_overlaps(&skeleton, &pose, model(0.0)),
            Some(vec![(0, 1)])
        );
    }
    let mut physics = world();
    let handoff = Handoff {
        pose: &pose,
        model: model(1.0),
        previous: None,
    };
    let ragdoll =
        Ragdoll::spawn(&mut physics, &rig, &skeleton, &handoff, &entities(10, 4)).expect("spawned");
    let e = ragdoll.entities();
    assert!(physics.ignores_contacts(e[0], e[1]));
    assert!(physics.ignores_contacts(e[2], e[3]));
    assert!(!physics.ignores_contacts(e[0], e[2]));
}

/// Unknown, ambiguous and bodiless bones, axes with no direction, bad
/// numbers and malformed text are each named.
#[test]
fn bad_files_are_refused_by_name() {
    let unknown = LEG.replace("\"foot_l\", shape", "\"toe\", shape");
    assert_eq!(
        resolve(&unknown).err(),
        Some(DescError::UnknownBone("toe".to_owned()))
    );

    let mut twice = names();
    twice[index("foot_r")] = "foot_l";
    let ambiguous =
        RagdollDesc::from_ron(LEG)
            .expect("parses")
            .resolve(&skeleton(), &twice, model(0.0));
    assert_eq!(
        ambiguous.err(),
        Some(DescError::AmbiguousBone("foot_l".to_owned()))
    );

    let bodiless = LEG.replace("child: \"foot_l\"", "child: \"foot_r\"");
    assert_eq!(
        resolve(&bodiless).err(),
        Some(DescError::NoBody("foot_r".to_owned()))
    );

    let flat = LEG.replace("axis: (1.0, 0.0, 0.0)", "axis: (0.0, 0.0, 0.0)");
    assert_eq!(
        resolve(&flat).err(),
        Some(DescError::ZeroAxis("calf_l".to_owned()))
    );

    let heavy = LEG.replace("mass: 1.0", "mass: -1.0");
    assert_eq!(
        resolve(&heavy).err(),
        Some(DescError::Rig(RigError::InvalidBody { body: 3 }))
    );

    let loose = LEG.replace("friction: 0.5", "friction: -0.5");
    assert_eq!(
        resolve(&loose).err(),
        Some(DescError::Rig(RigError::InvalidJoint { joint: 0 }))
    );

    let short = RagdollDesc::from_ron(LEG).expect("parses").resolve(
        &skeleton(),
        &names()[1..],
        Mat4::IDENTITY,
    );
    assert!(matches!(short, Err(DescError::NameCount { .. })));

    let typo = LEG.replace("substeps", "substep");
    assert!(matches!(resolve(&typo), Err(DescError::Parse { .. })));
}

/// **A rig written as a file and read back is the rig it was**: every body on
/// the same joint, shape and mass, centred and pointed the same; every joint
/// between the same bodies with the same kind and friction, standing at the
/// same point with the same axis; the same ignored pairs, whether listed or
/// left to the rule for rest overlaps.
#[test]
fn a_rig_round_trips_through_its_file() {
    let skeleton = skeleton();
    let names = names();
    let fitted = crcbl_ragdoll::RagdollRig::generate(
        &skeleton,
        model(0.0),
        &crcbl_ragdoll::GenerateSettings {
            radius_ratio: 0.3,
            ..crcbl_ragdoll::GenerateSettings::DEFAULT
        },
    )
    .expect("a rig");
    assert!(!fitted.ignored().is_empty(), "fat limbs, so pairs to carry");
    let text = fitted.to_desc(&names).expect("named").to_ron();
    assert!(!text.contains('\r'), "LF line ends");
    let back = RagdollDesc::from_ron(&text)
        .expect("reads")
        .resolve(&skeleton, &names, model(0.0))
        .expect("resolves");
    let close = |a: glam::DVec3, b: glam::DVec3| a.distance(b) < 1e-9;
    assert_eq!(back.bodies().len(), fitted.bodies().len());
    for (a, b) in back.bodies().iter().zip(fitted.bodies()) {
        assert_eq!((a.joint, a.shape), (b.joint, b.shape));
        assert!((a.mass - b.mass).abs() < 1e-12);
        assert!(close(a.offset.position, b.offset.position));
        assert!(close(
            a.offset.rotation * glam::DVec3::Y,
            b.offset.rotation * glam::DVec3::Y
        ));
    }
    assert_eq!(back.joints().len(), fitted.joints().len());
    // A joint in its bones' frames — the body's offset, then the joint's frame
    // on the body — which is what the file keeps: a body's turn about its own
    // axis is not stored, and a sphere or capsule does not care.
    let in_bone = |rig: &crcbl_ragdoll::RagdollRig, body: usize, frame: &crcbl_phys::Transform| {
        let offset = rig.bodies()[body].offset;
        (
            offset.position + offset.rotation * frame.position,
            offset.rotation * frame.rotation * glam::DVec3::Z,
        )
    };
    for (a, b) in back.joints().iter().zip(fitted.joints()) {
        assert_eq!((a.parent, a.child, a.kind), (b.parent, b.child, b.kind));
        assert!((a.friction - b.friction).abs() < 1e-12);
        for (side_a, side_b) in [
            (
                in_bone(&back, a.child, &a.frame_child),
                in_bone(&fitted, b.child, &b.frame_child),
            ),
            (
                in_bone(&back, a.parent, &a.frame_parent),
                in_bone(&fitted, b.parent, &b.frame_parent),
            ),
        ] {
            assert!(close(side_a.0, side_b.0), "{side_a:?} vs {side_b:?}");
            assert!(close(side_a.1, side_b.1), "{side_a:?} vs {side_b:?}");
        }
    }
    let pairs = |rig: &crcbl_ragdoll::RagdollRig| {
        let mut pairs: Vec<(usize, usize)> = rig
            .ignored()
            .iter()
            .map(|&(a, b)| (a.min(b), a.max(b)))
            .collect();
        pairs.sort_unstable();
        pairs
    };
    assert_eq!(pairs(&back), pairs(&fitted));
    // The fitter ignores exactly the pairs overlapping at rest, so a file
    // leaving them to the rule resolves to the same pairs: what an editor
    // writes for a fitted rig, so a body grown later is covered too.
    let mut by_rule = fitted.to_desc(&names).expect("named");
    by_rule.ignore.clear();
    by_rule.ignore_rest_overlaps = true;
    let by_rule = by_rule
        .resolve(&skeleton, &names, model(0.0))
        .expect("resolves");
    assert_eq!(pairs(&by_rule), pairs(&fitted));
    assert!(matches!(
        fitted.to_desc(&names[1..]),
        Err(DescError::NameCount { .. })
    ));
}
