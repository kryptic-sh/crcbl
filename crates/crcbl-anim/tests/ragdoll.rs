use crcbl_anim::ragdoll::{BodyJoint, RagdollBinding, RagdollError};
use crcbl_anim::{Joint, Palette, Pose, Skeleton, Trs};
use glam::{Mat4, Quat, Vec3};

const TOLERANCE: f32 = 2e-4;

fn rig() -> (Skeleton, Pose, Mat4, Vec<BodyJoint>) {
    let skeleton = Skeleton::new(vec![
        Joint {
            parent: None,
            inverse_bind: Mat4::IDENTITY,
            rest: Trs {
                translation: Vec3::new(0.0, 100.0, 0.0),
                rotation: Quat::from_rotation_y(0.3),
                ..Trs::IDENTITY
            },
        },
        Joint {
            parent: Some(0),
            inverse_bind: Mat4::from_translation(Vec3::new(-30.0, 0.0, 0.0)),
            rest: Trs {
                translation: Vec3::new(30.0, 0.0, 0.0),
                rotation: Quat::from_rotation_z(0.5),
                ..Trs::IDENTITY
            },
        },
        Joint {
            parent: Some(1),
            inverse_bind: Mat4::IDENTITY,
            rest: Trs {
                translation: Vec3::new(20.0, 0.0, 0.0),
                ..Trs::IDENTITY
            },
        },
    ])
    .unwrap();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[1].rotation = Quat::from_rotation_x(-0.4);
    let model = Mat4::from_scale_rotation_translation(
        Vec3::splat(0.01),
        Quat::from_rotation_z(0.2),
        Vec3::new(4.0, 2.0, -3.0),
    );
    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);
    let bodies = [1, 0].map(|joint| {
        let (_, rotation, position) =
            (model * palette.globals()[joint]).to_scale_rotation_translation();
        BodyJoint {
            joint,
            world: Mat4::from_rotation_translation(
                rotation * Quat::from_rotation_y(0.7),
                position + Vec3::new(0.05, -0.1, 0.03),
            ),
        }
    });
    (skeleton, pose, model, bodies.to_vec())
}

fn near(actual: Mat4, expected: Mat4) {
    assert!(
        actual.abs_diff_eq(expected, TOLERANCE),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn handoff_preserves_animated_scaled_pose_and_skinning_palette() {
    let (skeleton, pose, model, bodies) = rig();
    let mut binding = RagdollBinding::new(&skeleton, &pose, model, &bodies).unwrap();
    let mut output = Pose::new(&skeleton);
    binding
        .pose_into(
            model,
            &bodies.iter().map(|b| b.world).collect::<Vec<_>>(),
            &mut output,
        )
        .unwrap();
    let mut before = Palette::new(&skeleton);
    let mut after = Palette::new(&skeleton);
    before.compute(&skeleton, &pose);
    after.compute(&skeleton, &output);
    for (actual, expected) in after.matrices().iter().zip(before.matrices()) {
        near(*actual, *expected);
    }
}

#[test]
fn independently_moved_bodies_drive_bones_and_unmapped_descendants() {
    let (skeleton, pose, model, bodies) = rig();
    let mut binding = RagdollBinding::new(&skeleton, &pose, model, &bodies).unwrap();
    let mut before = Palette::new(&skeleton);
    before.compute(&skeleton, &pose);
    let movements = [
        Mat4::from_rotation_translation(Quat::from_rotation_x(0.6), Vec3::new(0.0, -1.0, 2.0)),
        Mat4::from_rotation_translation(Quat::from_rotation_z(-0.2), Vec3::new(1.0, -0.5, 0.0)),
    ];
    let moved = [
        movements[0] * bodies[0].world,
        movements[1] * bodies[1].world,
    ];
    let mut output = Pose::new(&skeleton);
    let render_model = Mat4::from_translation(Vec3::new(-2.0, 3.0, 1.0)) * model;
    binding
        .pose_into(render_model, &moved, &mut output)
        .unwrap();
    let mut after = Palette::new(&skeleton);
    after.compute(&skeleton, &output);
    for (body, movement) in bodies.iter().zip(movements) {
        near(
            render_model * after.globals()[body.joint],
            movement * model * before.globals()[body.joint],
        );
    }
    assert_eq!(output.locals()[2], pose.locals()[2]);
    near(
        render_model * after.globals()[2],
        movements[0] * model * before.globals()[2],
    );
}

#[test]
fn rejects_bad_bindings_and_nonrigid_bodies() {
    let (skeleton, pose, model, bodies) = rig();
    assert!(matches!(
        RagdollBinding::new(&skeleton, &pose, model, &[]),
        Err(RagdollError::Empty)
    ));
    let mut duplicate = bodies.clone();
    duplicate[1].joint = duplicate[0].joint;
    assert!(matches!(
        RagdollBinding::new(&skeleton, &pose, model, &duplicate),
        Err(RagdollError::DuplicateJoint { .. })
    ));
    duplicate[1].joint = skeleton.len();
    assert!(matches!(
        RagdollBinding::new(&skeleton, &pose, model, &duplicate),
        Err(RagdollError::JointOutOfRange { .. })
    ));
    for invalid in [
        Mat4::ZERO,
        Mat4::from_scale(Vec3::splat(2.0)),
        Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
        Mat4::from_cols_array(&[f32::NAN; 16]),
    ] {
        let mut invalid_bodies = bodies.clone();
        invalid_bodies[0].world = invalid;
        assert!(matches!(
            RagdollBinding::new(&skeleton, &pose, model, &invalid_bodies),
            Err(RagdollError::InvalidBody { body: 0 })
        ));
    }
}

#[test]
fn failed_update_preserves_output_and_does_not_poison_the_next_update() {
    let (skeleton, pose, model, bodies) = rig();
    let mut binding = RagdollBinding::new(&skeleton, &pose, model, &bodies).unwrap();
    let mut output = pose.clone();
    let worlds: Vec<_> = bodies.iter().map(|body| body.world).collect();
    assert_eq!(
        binding.pose_into(model, &[], &mut output),
        Err(RagdollError::BodyCount)
    );
    assert_eq!(output, pose);
    assert_eq!(
        binding.pose_into(Mat4::ZERO, &worlds, &mut output),
        Err(RagdollError::InvalidModel)
    );
    assert_eq!(output, pose);
    let mut bad = worlds.clone();
    bad[1] = Mat4::ZERO;
    assert_eq!(
        binding.pose_into(model, &bad, &mut output),
        Err(RagdollError::InvalidBody { body: 1 })
    );
    assert_eq!(output, pose);
    binding.pose_into(model, &worlds, &mut output).unwrap();
    for (actual, expected) in output.locals().iter().zip(pose.locals()) {
        near(actual.to_mat4(), expected.to_mat4());
    }
}

#[test]
fn unrepresentable_shear_fails_after_the_parent_without_changing_output() {
    let skeleton = Skeleton::new(vec![
        Joint {
            parent: None,
            inverse_bind: Mat4::IDENTITY,
            rest: Trs {
                scale: Vec3::new(2.0, 1.0, 1.0),
                ..Trs::IDENTITY
            },
        },
        Joint {
            parent: Some(0),
            inverse_bind: Mat4::IDENTITY,
            rest: Trs::IDENTITY,
        },
    ])
    .unwrap();
    let pose = Pose::new(&skeleton);
    let bodies = [0, 1].map(|joint| BodyJoint {
        joint,
        world: Mat4::IDENTITY,
    });
    let mut binding = RagdollBinding::new(&skeleton, &pose, Mat4::IDENTITY, &bodies).unwrap();
    let mut output = pose.clone();
    let moved_parent = Mat4::from_translation(Vec3::X);
    assert_eq!(
        binding.pose_into(
            Mat4::IDENTITY,
            &[moved_parent, Mat4::from_rotation_z(0.7)],
            &mut output
        ),
        Err(RagdollError::InvalidJoint { joint: 1 })
    );
    assert_eq!(output, pose);
    binding
        .pose_into(Mat4::IDENTITY, &[moved_parent; 2], &mut output)
        .unwrap();
    assert_eq!(output.locals()[0].translation, Vec3::X);
    assert_eq!(output.locals()[1], pose.locals()[1]);
}

#[test]
fn malformed_pose_and_model_inputs_are_rejected_before_handoff() {
    let (skeleton, pose, model, bodies) = rig();
    let empty = Pose::new(&Skeleton::new(Vec::new()).unwrap());
    assert!(matches!(
        RagdollBinding::new(&skeleton, &empty, model, &bodies),
        Err(RagdollError::PoseSize)
    ));
    for local in [
        Trs {
            translation: Vec3::splat(f32::INFINITY),
            ..Trs::IDENTITY
        },
        Trs {
            scale: Vec3::ZERO,
            ..Trs::IDENTITY
        },
        Trs {
            rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            ..Trs::IDENTITY
        },
        Trs {
            rotation: Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0),
            ..Trs::IDENTITY
        },
    ] {
        let mut invalid = pose.clone();
        invalid.locals_mut()[2] = local;
        assert!(matches!(
            RagdollBinding::new(&skeleton, &invalid, model, &bodies),
            Err(RagdollError::InvalidJoint { joint: 2 })
        ));
    }
    let mut projective = model;
    projective.x_axis.w = 0.1;
    for invalid in [
        Mat4::ZERO,
        projective,
        Mat4::from_cols_array(&[f32::NAN; 16]),
    ] {
        assert!(matches!(
            RagdollBinding::new(&skeleton, &pose, invalid, &bodies),
            Err(RagdollError::InvalidModel)
        ));
    }
}
