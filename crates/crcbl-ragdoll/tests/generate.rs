//! The fallback rig fitted to the synthetic humanoid.

mod common;

use common::{index, model, skeleton};
use crcbl_phys::Transform;
use crcbl_ragdoll::{BodyShape, GenerateError, GenerateSettings, RagdollRig, RigJointKind};
use glam::{DVec3, Mat4, Vec3};

const TOLERANCE: f64 = 1e-5;

fn rig() -> RagdollRig {
    RagdollRig::generate(&skeleton(), model(0.0), &GenerateSettings::DEFAULT).expect("a rig")
}

fn body_at(rig: &RagdollRig, bone: &str) -> Option<usize> {
    rig.bodies()
        .iter()
        .position(|body| body.joint == index(bone))
}

/// Where a rig body stands in the rest pose under `model`, worked out from
/// the fixture rather than the crate.
fn rest_world(bone: usize, model: Mat4, offset: &Transform) -> DVec3 {
    let mut position = Vec3::ZERO;
    let mut joint = Some(bone);
    while let Some(current) = joint {
        position += Vec3::from(common::BONES[current].2);
        joint = common::BONES[current].1;
    }
    let (_, rotation, _) = model.to_scale_rotation_translation();
    model.transform_point3(position).as_dvec3() + rotation.as_dquat() * offset.position
}

/// **Every bone long enough carries one body, of the kind its children call
/// for.** Every fixture bone is long enough, leaves included.
#[test]
fn every_long_bone_carries_a_body_of_the_kind_its_children_call_for() {
    let rig = rig();
    assert_eq!(
        rig.bodies().len(),
        common::BONES.len(),
        "every bone is long"
    );
    let shape = |bone: &str| rig.bodies()[body_at(&rig, bone).expect("a body")].shape;
    // One child: a capsule from the joint to it, radius a fifth its length.
    let BodyShape::Capsule {
        radius,
        half_height,
    } = shape("forearm_l")
    else {
        panic!("a forearm is a capsule: {:?}", shape("forearm_l"));
    };
    assert!((radius - 0.05).abs() < TOLERANCE, "radius {radius}");
    assert!((half_height - (0.125 - 0.05)).abs() < TOLERANCE);
    // Several children: a sphere.
    for bone in ["hips", "chest"] {
        assert!(matches!(shape(bone), BodyShape::Sphere { .. }), "{bone}");
    }
    // No children: a sphere just past the joint, sized from its own bone.
    let BodyShape::Sphere { radius } = shape("hand_l") else {
        panic!("a hand is a sphere");
    };
    assert!((radius - 0.05).abs() < TOLERANCE, "radius {radius}");
}

/// **The capsule lies along its bone.** The forearm's body stands midway
/// between the elbow and the wrist, wherever the model turns it.
#[test]
fn a_capsule_lies_midway_along_its_bone() {
    let rig = rig();
    let body = rig.bodies()[body_at(&rig, "forearm_l").expect("a body")];
    let elbow = rest_world(index("forearm_l"), model(0.0), &Transform::IDENTITY);
    let wrist = rest_world(index("hand_l"), model(0.0), &Transform::IDENTITY);
    let centre = rest_world(index("forearm_l"), model(0.0), &body.offset);
    assert!(
        centre.distance(0.5 * (elbow + wrist)) < TOLERANCE,
        "centre {centre}, elbow {elbow}, wrist {wrist}"
    );
}

/// **Mass is shared by volume and adds up.** The whole ragdoll weighs what
/// was asked, and a thick thigh outweighs a thin forearm.
#[test]
fn mass_is_shared_by_volume() {
    let rig = rig();
    assert!((rig.mass() - GenerateSettings::DEFAULT.total_mass).abs() < 1e-9);
    let mass = |bone: &str| rig.bodies()[body_at(&rig, bone).expect("a body")].mass;
    assert!(mass("thigh_l") > 4.0 * mass("forearm_l"));
}

/// **The bodies make one tree of ball joints.** Each body but the root hangs
/// from its nearest ancestor's body, and the joint's frame is the same point
/// in the world seen from either body.
#[test]
fn the_bodies_hang_in_one_tree_of_ball_joints() {
    let rig = rig();
    assert_eq!(rig.joints().len(), rig.bodies().len() - 1);
    let hips = body_at(&rig, "hips").expect("a body");
    for joint in rig.joints() {
        assert_ne!(joint.child, hips, "the root hangs from nothing");
        assert!(matches!(joint.kind, RigJointKind::Ball { .. }));
        let parent = rig.bodies()[joint.parent];
        let child = rig.bodies()[joint.child];
        let skeleton_parent = common::BONES[child.joint].1.expect("not the root");
        assert_eq!(parent.joint, skeleton_parent, "every bone has a body here");
        let world = |bone: usize, offset: &Transform, frame: &Transform| {
            let body = rest_world(bone, model(0.0), offset);
            let (_, rotation, _) = model(0.0).to_scale_rotation_translation();
            body + rotation.as_dquat() * offset.rotation * frame.position
        };
        let from_parent = world(parent.joint, &parent.offset, &joint.frame_parent);
        let from_child = world(child.joint, &child.offset, &joint.frame_child);
        let at = rest_world(child.joint, model(0.0), &Transform::IDENTITY);
        assert!(
            from_parent.distance(at) < TOLERANCE,
            "{from_parent} vs {at}"
        );
        assert!(from_child.distance(at) < TOLERANCE, "{from_child} vs {at}");
    }
}

/// **Shapes that overlap at rest without a joint are kept apart.** Fatter
/// limbs make the two thighs meet below the pelvis, and the generator
/// ignores that pair; at the default thickness they clear each other.
#[test]
fn shapes_overlapping_at_rest_are_ignored() {
    let thighs = |rig: &RagdollRig| {
        let pair = (
            body_at(rig, "thigh_l").expect("a body"),
            body_at(rig, "thigh_r").expect("a body"),
        );
        rig.ignored()
            .iter()
            .any(|&(a, b)| (a, b) == pair || (b, a) == pair)
    };
    assert!(!thighs(&rig()), "slim thighs clear each other");
    let fat = GenerateSettings {
        radius_ratio: 0.3,
        ..GenerateSettings::DEFAULT
    };
    let rig = RagdollRig::generate(&skeleton(), model(0.0), &fat).expect("a rig");
    assert!(thighs(&rig), "fat thighs overlap and are ignored");
    for &(a, b) in rig.ignored() {
        let jointed = rig
            .joints()
            .iter()
            .any(|j| (j.parent, j.child) == (a, b) || (j.parent, j.child) == (b, a));
        assert!(!jointed, "a jointed pair needs no ignoring");
    }
}

/// Settings out of range and a model that flattens the skeleton are refused.
#[test]
fn bad_settings_and_a_degenerate_model_are_refused() {
    let skeleton = skeleton();
    for settings in [
        GenerateSettings {
            total_mass: 0.0,
            ..GenerateSettings::DEFAULT
        },
        GenerateSettings {
            radius_ratio: 0.5,
            ..GenerateSettings::DEFAULT
        },
        GenerateSettings {
            cone: f64::NAN,
            ..GenerateSettings::DEFAULT
        },
    ] {
        assert_eq!(
            RagdollRig::generate(&skeleton, model(0.0), &settings),
            Err(GenerateError::InvalidSettings)
        );
    }
    let flat = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
    assert!(matches!(
        RagdollRig::generate(&skeleton, flat, &GenerateSettings::DEFAULT),
        Err(GenerateError::InvalidFrame { .. })
    ));
    // Every bone shorter than the minimum: nothing carries a body.
    let tiny = GenerateSettings {
        min_length: 10.0,
        ..GenerateSettings::DEFAULT
    };
    assert!(matches!(
        RagdollRig::generate(&skeleton, model(0.0), &tiny),
        Err(GenerateError::Rig(_))
    ));
}

/// **No body weighs more than the ratio allows against a body it is jointed
/// to, and each joint resists turning in proportion to its lighter body.**
/// By volume alone, the hips sphere would weigh a fraction of a thigh.
#[test]
fn jointed_masses_stay_within_the_ratio_and_set_the_friction() {
    let rig = rig();
    let settings = GenerateSettings::DEFAULT;
    for joint in rig.joints() {
        let (a, b) = (
            rig.bodies()[joint.parent].mass,
            rig.bodies()[joint.child].mass,
        );
        assert!(
            a.max(b) <= settings.max_mass_ratio * a.min(b) * (1.0 + 1e-9),
            "{a} kg jointed to {b} kg"
        );
        let friction = settings.joint_friction * a.min(b);
        assert!((joint.friction - friction).abs() < 1e-12);
    }
    let hips = rig.bodies()[body_at(&rig, "hips").expect("a body")];
    let thigh = rig.bodies()[body_at(&rig, "thigh_l").expect("a body")];
    assert!(
        hips.shape.volume() * settings.max_mass_ratio < thigh.shape.volume(),
        "the fixture must exercise the ratio"
    );
}
