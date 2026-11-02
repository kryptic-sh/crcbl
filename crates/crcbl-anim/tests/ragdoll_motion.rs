use crcbl_anim::ragdoll::{BodyJoint, RagdollBinding, RagdollError};
use crcbl_anim::{Joint, Palette, Pose, Skeleton, Trs};
use glam::{DQuat, DVec3, Mat4, Quat, Vec3};

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

/// The f64 turn from `previous` to `current`, the shorter way round, as a
/// scaled axis, and whether taking the shorter way flipped its sign: the
/// reference the handoff's angular velocity is held to.
#[expect(
    clippy::disallowed_methods,
    reason = "glam's platform-atan2 scaled axis is an independent reference, held to a tolerance"
)]
fn reference_turn(current: DQuat, previous: DQuat) -> (DVec3, bool) {
    let turn = (current.normalize() * previous.normalize().conjugate()).normalize();
    let flipped = turn.w < 0.0;
    let turn = if flipped { -turn } else { turn };
    (turn.to_scaled_axis(), flipped)
}

/// **A handoff's angular velocity is the f64 turn between its poses.** The
/// poses are built from quaternions whose components are exact decimals
/// (`0.6² + 0.8² = 1`) and the models and body offsets are pure translations,
/// so each body's rotation is its joint's global one, and the reference
/// composes those globals from the same components in f64. Two previous poses:
/// one whose turn quaternion has `w > 0`, and one whose has `w < 0` and must
/// be negated to take the shorter way round.
///
/// No digest is pinned across targets here, on purpose: the poses pass through
/// glam's `f32` palette, whose `Quat * Quat` groups its sums differently on
/// x86-64 (SSE2), on aarch64 (NEON, fused with `vfmaq_f32`) and on wasm32 and
/// the browser (scalar, no `simd128`), so the bits legitimately differ by
/// target. The target-independent pin is
/// `the_scaled_axes_hash_to_the_pinned_digest` in `src/ragdoll/motion.rs`,
/// over the f64 step alone.
#[test]
fn a_handoff_motion_is_the_f64_turn_between_its_poses() {
    /// The f32 palette's rounding, relative to the turn rate. Measured worst
    /// on x86-64 Linux, 2026-11-01: 4.3e-8, so this leaves a factor of over
    /// twenty for another target's f32 path.
    const RELATIVE: f64 = 1e-6;
    let (skeleton, mut current) = rig(0.0);
    let root_now = Quat::from_xyzw(0.0, 0.0, 0.28, 0.96);
    let limb_now = Quat::from_xyzw(0.6, 0.0, 0.0, 0.8);
    current.locals_mut()[0].rotation = root_now;
    current.locals_mut()[1].rotation = limb_now;
    let model = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &current);
    // Each body a quarter of a unit along its joint's x axis, so the offsets
    // are not the identity, with no rotation that would need a sine.
    let bodies = [1, 0].map(|joint| BodyJoint {
        joint,
        world: model * palette.globals()[joint] * Mat4::from_translation(Vec3::new(0.25, 0.0, 0.0)),
    });
    let binding = RagdollBinding::new(&skeleton, &current, model, &bodies).unwrap();
    let dt = 1.0 / 60.0;
    let previous_model = Mat4::from_translation(Vec3::new(0.5, 2.0, 3.0));
    let mut flips = 0;
    for (root_then, limb_then) in [
        (
            Quat::from_xyzw(0.0, 0.28, 0.0, 0.96),
            Quat::from_xyzw(0.0, 0.0, -0.28, 0.96),
        ),
        // The root written with `w < 0`, as glam's matrix-to-quaternion
        // leaves it (its largest component, `z`, positive), so the turn from
        // it to now has `w = 0.96·-0.6 + 0.28·0.8 < 0`.
        (
            Quat::from_xyzw(0.0, 0.0, 0.8, -0.6),
            Quat::from_xyzw(0.0, 0.0, -0.28, 0.96),
        ),
    ] {
        let mut previous = current.clone();
        previous.locals_mut()[0].rotation = root_then;
        previous.locals_mut()[1].rotation = limb_then;
        let motion = binding
            .motion_from_previous_pose(&previous, previous_model, dt)
            .unwrap();
        assert_eq!(
            motion,
            binding
                .motion_from_previous_pose(&previous, previous_model, dt)
                .unwrap(),
            "two runs over the same poses disagree"
        );
        let (root_now, limb_now) = (root_now.as_dquat(), limb_now.as_dquat());
        let (root_then, limb_then) = (root_then.as_dquat(), limb_then.as_dquat());
        // Binding order: body 0 is joint 1, the limb, under the root.
        let expected = [
            reference_turn(root_now * limb_now, root_then * limb_then),
            reference_turn(root_now, root_then),
        ];
        for (body, (motion, (expected, flipped))) in motion.iter().zip(expected).enumerate() {
            flips += usize::from(flipped);
            let expected = expected / dt;
            assert!(expected.length() > 1.0, "body {body} hardly turns");
            let error = motion.angular_velocity.distance(expected) / expected.length();
            assert!(
                error < RELATIVE,
                "body {body} turns at {:?}, {error:e} from the f64 {expected:?}",
                motion.angular_velocity
            );
        }
    }
    assert!(flips > 0, "no turn took the shorter way round");
}
