use crcbl_anim::ragdoll::{BodyJoint, RagdollBinding, RagdollError};
use crcbl_anim::{Joint, Palette, Pose, Skeleton, Trs};
use glam::{DVec3, Mat4, Quat, Vec3};

const TOLERANCE: f64 = 2e-5;

fn rig(angle: f32) -> (Skeleton, Pose) {
    let skeleton = Skeleton::new(vec![
        Joint {
            parent: None,
            inverse_bind: Mat4::IDENTITY,
            rest: Trs::IDENTITY,
        },
        Joint {
            parent: Some(0),
            inverse_bind: Mat4::IDENTITY,
            rest: Trs {
                translation: Vec3::new(100.0, 0.0, 0.0),
                ..Trs::IDENTITY
            },
        },
    ])
    .unwrap();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[0].rotation = Quat::from_rotation_z(angle);
    (skeleton, pose)
}

fn model(x: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, 5.0, 6.0)) * Mat4::from_scale(Vec3::splat(0.01))
}

fn bodies(skeleton: &Skeleton, pose: &Pose, model: Mat4) -> [BodyJoint; 2] {
    let mut palette = Palette::new(skeleton);
    palette.compute(skeleton, pose);
    [1, 0].map(|joint| {
        let (_, rotation, position) =
            (model * palette.globals()[joint]).to_scale_rotation_translation();
        BodyJoint {
            joint,
            world: Mat4::from_rotation_translation(
                rotation * Quat::from_rotation_x(0.2),
                position + rotation * Vec3::new(0.2, 0.0, 0.0),
            ),
        }
    })
}

fn near(actual: DVec3, expected: DVec3) {
    assert!(
        actual.distance(expected) < TOLERANCE,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn handoff_keeps_character_translation_and_offset_body_rotation_in_binding_order() {
    let (skeleton, current) = rig(0.4);
    let (_, previous) = rig(-0.2);
    let bodies = bodies(&skeleton, &current, model(4.0));
    let binding = RagdollBinding::new(&skeleton, &current, model(4.0), &bodies).unwrap();
    let dt = 0.25;
    let motion = binding
        .motion_from_previous_pose(&previous, model(3.0), dt)
        .unwrap();
    assert_eq!(motion.len(), bodies.len());
    for (motion, body) in motion.iter().zip(bodies) {
        let radius = body.joint as f32 + 0.2;
        let current_position =
            Vec3::new(4.0, 5.0, 6.0) + Quat::from_rotation_z(0.4) * Vec3::new(radius, 0.0, 0.0);
        let previous_position =
            Vec3::new(3.0, 5.0, 6.0) + Quat::from_rotation_z(-0.2) * Vec3::new(radius, 0.0, 0.0);
        near(motion.position, current_position.as_dvec3());
        near(
            motion.linear_velocity,
            (current_position - previous_position).as_dvec3() / dt,
        );
        near(motion.angular_velocity, DVec3::Z * (0.6 / dt));
        let (_, rotation, _) = body.world.to_scale_rotation_translation();
        near(motion.rotation * DVec3::X, rotation.as_dquat() * DVec3::X);
    }
    let slower = binding
        .motion_from_previous_pose(&previous, model(3.0), dt * 2.0)
        .unwrap();
    for (fast, slow) in motion.iter().zip(slower) {
        near(fast.linear_velocity, slow.linear_velocity * 2.0);
        near(fast.angular_velocity, slow.angular_velocity * 2.0);
    }
}

#[test]
fn stationary_samples_do_not_invent_velocity() {
    let (skeleton, pose) = rig(0.4);
    let bodies = bodies(&skeleton, &pose, model(4.0));
    let binding = RagdollBinding::new(&skeleton, &pose, model(4.0), &bodies).unwrap();
    for motion in binding
        .motion_from_previous_pose(&pose, model(4.0), 0.01)
        .unwrap()
    {
        assert_eq!(motion.linear_velocity, DVec3::ZERO);
        assert_eq!(motion.angular_velocity, DVec3::ZERO);
    }
}

#[test]
fn angular_velocity_uses_the_shorter_arc_across_quaternion_signs() {
    let (skeleton, current) = rig(60.0_f32.to_radians());
    let (_, previous) = rig((-100.0_f32).to_radians());
    let bodies = bodies(&skeleton, &current, model(0.0));
    let binding = RagdollBinding::new(&skeleton, &current, model(0.0), &bodies).unwrap();
    for motion in binding
        .motion_from_previous_pose(&previous, model(0.0), 1.0)
        .unwrap()
    {
        near(motion.angular_velocity, DVec3::Z * 160.0_f64.to_radians());
    }
}

#[test]
fn invalid_sample_intervals_and_nonrigid_history_fail_explicitly() {
    let (skeleton, pose) = rig(0.4);
    let bodies = bodies(&skeleton, &pose, model(4.0));
    let binding = RagdollBinding::new(&skeleton, &pose, model(4.0), &bodies).unwrap();
    for dt in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert_eq!(
            binding.motion_from_previous_pose(&pose, model(4.0), dt),
            Err(RagdollError::InvalidInterval)
        );
    }
    assert!(matches!(
        binding.motion_from_previous_pose(&pose, model(3.0), f64::from_bits(1)),
        Err(RagdollError::InvalidMotion { .. })
    ));
    let mut previous = pose.clone();
    previous.locals_mut()[0].scale = Vec3::splat(2.0);
    assert!(matches!(
        binding.motion_from_previous_pose(&previous, model(4.0), 1.0),
        Err(RagdollError::InvalidBody { .. })
    ));
    previous.locals_mut()[0].rotation = Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0);
    assert!(matches!(
        binding.motion_from_previous_pose(&previous, model(4.0), 1.0),
        Err(RagdollError::InvalidJoint { joint: 0 })
    ));
    assert_eq!(
        binding.motion_from_previous_pose(&pose, Mat4::ZERO, 1.0),
        Err(RagdollError::InvalidModel)
    );
}
