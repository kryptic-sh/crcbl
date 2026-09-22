//! Two-bone IK and model-space joint rotation, read back through the crate's
//! own palette rather than a hand copy of the hierarchy.

use crcbl_anim::{
    IkError, IkInput, Joint, Palette, Pose, Skeleton, Trs, rotate_joint, solve_two_bone,
};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::f32::consts::FRAC_PI_2;

/// Model-space positions are tens of units across, so a few ulps of `f32`
/// arithmetic per joint stays well inside this.
const TOLERANCE: f32 = 1e-4;

const BODY: usize = 0;
const SHOULDER: usize = 1;
const ELBOW: usize = 2;
const WRIST: usize = 3;
const ARM: [usize; 3] = [SHOULDER, ELBOW, WRIST];

/// Upper arm 3 and forearm 2 in the skeleton's units, doubled by [`model`].
const UPPER: f32 = 6.0;
const LOWER: f32 = 4.0;

fn joint(parent: Option<usize>, translation: Vec3) -> Joint {
    Joint {
        parent,
        inverse_bind: Mat4::IDENTITY,
        rest: Trs {
            translation,
            ..Trs::IDENTITY
        },
    }
}

/// A body turned about `+Y`, and an arm hanging down its local `-Y` from a
/// shoulder one unit out along its `+X`.
fn arm() -> Skeleton {
    let mut body = joint(None, Vec3::new(0.0, 5.0, 0.0));
    body.rest.rotation = Quat::from_rotation_y(0.4);
    Skeleton::new(vec![
        body,
        joint(Some(BODY), Vec3::new(1.0, 0.0, 0.0)),
        joint(Some(SHOULDER), Vec3::new(0.0, -3.0, 0.0)),
        joint(Some(ELBOW), Vec3::new(0.0, -2.0, 0.0)),
    ])
    .expect("parents precede children")
}

/// The skeleton placed in model space: scaled by two, tipped and moved, so
/// that nothing passes by accident of an identity.
fn model() -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::splat(2.0),
        Quat::from_rotation_x(0.3),
        Vec3::new(10.0, 0.0, -4.0),
    )
}

fn posed(skeleton: &Skeleton, pose: &Pose) -> Palette {
    let mut palette = Palette::new(skeleton);
    palette.compute(skeleton, pose);
    palette
}

/// A joint's model-space position, through the palette.
fn at(palette: &Palette, joint: usize) -> Vec3 {
    (model() * palette.globals()[joint]).transform_point3(Vec3::ZERO)
}

fn assert_near(actual: Vec3, expected: Vec3, what: &str) {
    assert!(
        actual.distance(expected) < TOLERANCE,
        "{what}: {actual:?}, expected {expected:?}"
    );
}

fn solve(target: Vec3, pole: Vec3) -> (Skeleton, Pose, Palette) {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    solve_two_bone(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        ARM,
        target,
        pole,
    )
    .expect("a well-formed arm solves");
    (skeleton, pose, palette)
}

/// Where the shoulder is in model space; the solve never moves it.
fn shoulder() -> Vec3 {
    let skeleton = arm();
    at(&posed(&skeleton, &Pose::new(&skeleton)), SHOULDER)
}

#[test]
fn a_reachable_target_is_reached() {
    let target = shoulder() + Vec3::new(3.0, -4.0, 4.0);
    let (_, _, palette) = solve(target, Vec3::NEG_Z);
    assert_near(at(&palette, WRIST), target, "the wrist");
    assert_near(at(&palette, SHOULDER), shoulder(), "the shoulder");
}

#[test]
fn bone_lengths_translations_and_scales_are_kept() {
    let target = shoulder() + Vec3::new(-2.0, 1.0, 5.0);
    let (skeleton, pose, palette) = solve(target, Vec3::Y);
    let upper = at(&palette, SHOULDER).distance(at(&palette, ELBOW));
    let lower = at(&palette, ELBOW).distance(at(&palette, WRIST));
    assert!((upper - UPPER).abs() < TOLERANCE, "upper arm is {upper}");
    assert!((lower - LOWER).abs() < TOLERANCE, "forearm is {lower}");
    for (index, (local, rest)) in pose.locals().iter().zip(skeleton.joints()).enumerate() {
        assert_eq!(local.translation, rest.rest.translation, "joint {index}");
        assert_eq!(local.scale, rest.rest.scale, "joint {index}");
    }
    // Only the two turned joints change.
    assert_eq!(pose.locals()[BODY], skeleton.joints()[BODY].rest);
    assert_eq!(pose.locals()[WRIST], skeleton.joints()[WRIST].rest);
}

/// The palette is recomputed by the call, not left for the caller.
#[test]
fn the_palette_is_current_afterwards() {
    let (skeleton, pose, palette) = solve(shoulder() + Vec3::new(0.0, 0.0, 7.0), Vec3::Y);
    assert_eq!(palette, posed(&skeleton, &pose));
}

#[test]
fn the_pole_picks_the_side_the_elbow_bends_to() {
    let target = shoulder() + Vec3::new(0.0, -5.0, 0.0);
    for pole in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
        let (_, _, palette) = solve(target, pole);
        let elbow = at(&palette, ELBOW) - shoulder();
        assert_near(at(&palette, WRIST), target, "the wrist");
        // A 6–4–5 triangle: the elbow sits 6·sin(acos(0.75)) off the line.
        let side = elbow.dot(pole);
        assert!(
            (side - UPPER * (1.0 - 0.75_f32 * 0.75).sqrt()).abs() < TOLERANCE,
            "pole {pole:?}: the elbow is {side} towards it"
        );
    }
}

#[test]
fn an_unreachable_target_extends_the_arm_towards_it() {
    let direction = Vec3::new(1.0, 2.0, -2.0).normalize();
    let (_, _, palette) = solve(shoulder() + direction * 50.0, Vec3::Z);
    assert_near(
        at(&palette, ELBOW),
        shoulder() + direction * UPPER,
        "the elbow",
    );
    assert_near(
        at(&palette, WRIST),
        shoulder() + direction * (UPPER + LOWER),
        "the wrist",
    );
}

#[test]
fn a_target_too_close_folds_the_arm_flat() {
    let direction = Vec3::new(0.0, 1.0, 1.0).normalize();
    let (_, _, palette) = solve(shoulder() + direction * 0.5, Vec3::X);
    assert_near(
        at(&palette, ELBOW),
        shoulder() + direction * UPPER,
        "the elbow",
    );
    assert_near(
        at(&palette, WRIST),
        shoulder() + direction * (UPPER - LOWER),
        "the wrist",
    );
}

/// Equal bones fold all the way back: the end lands on the root and the
/// middle sits a bone's length out along the pole.
#[test]
fn equal_bones_fold_onto_the_root() {
    let skeleton = Skeleton::new(vec![
        joint(None, Vec3::ZERO),
        joint(Some(0), Vec3::new(0.0, -2.0, 0.0)),
        joint(Some(1), Vec3::new(0.0, -2.0, 0.0)),
    ])
    .expect("parents precede children");
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    solve_two_bone(
        &skeleton,
        Mat4::IDENTITY,
        &mut pose,
        &mut palette,
        [0, 1, 2],
        Vec3::ZERO,
        Vec3::X,
    )
    .expect("a well-formed chain solves");
    let point = |joint: usize| palette.globals()[joint].transform_point3(Vec3::ZERO);
    assert_near(point(2), Vec3::ZERO, "the end");
    assert_near(point(1), Vec3::new(2.0, 0.0, 0.0), "the middle");
}

/// Root, target and pole on one line, with the arm straight along it too:
/// nothing gives a bend plane, and the answer is still a finite, reached one.
#[test]
fn a_pole_along_the_target_line_still_solves() {
    let skeleton = arm();
    let down = at(&posed(&skeleton, &Pose::new(&skeleton)), WRIST) - shoulder();
    let target = shoulder() + down.normalize() * 7.0;
    let (_, pose, palette) = solve(target, down);
    assert!(
        pose.locals().iter().all(|local| local.rotation.is_finite()),
        "{pose:?}"
    );
    assert_near(at(&palette, WRIST), target, "the wrist");
    let upper = at(&palette, SHOULDER).distance(at(&palette, ELBOW));
    assert!((upper - UPPER).abs() < TOLERANCE, "upper arm is {upper}");
}

/// A bent arm and a useless pole: the elbow stays on the side it was bent to.
///
/// The pole is along the target line but for a lean towards `-X` far inside
/// [`crcbl_anim::PARALLEL_TOLERANCE`] — close enough to be rounding, so it
/// must not be what picks the side.
#[test]
fn a_useless_pole_keeps_the_current_bend() {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    let target = shoulder() + Vec3::new(0.0, -6.0, 0.0);
    solve_two_bone(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        ARM,
        target,
        Vec3::X,
    )
    .expect("a well-formed arm solves");
    let target = shoulder() + Vec3::new(0.0, -8.0, 0.0);
    solve_two_bone(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        ARM,
        target,
        Vec3::new(-1e-6, 1.0, 0.0),
    )
    .expect("a well-formed arm solves");
    assert_near(at(&palette, WRIST), target, "the wrist");
    let side = (at(&palette, ELBOW) - shoulder()).x;
    assert!(side > 1.0, "the elbow moved off +X: {side}");
}

/// A target on the root has no direction of its own: the chain folds along
/// the direction to its current end — not the upper bone's, which the bent
/// elbow here makes a different one.
#[test]
fn a_target_on_the_root_folds_towards_the_current_end() {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[ELBOW].rotation = Quat::from_rotation_z(FRAC_PI_2);
    let mut palette = posed(&skeleton, &pose);
    let towards = (at(&palette, WRIST) - shoulder()).normalize();
    solve_two_bone(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        ARM,
        shoulder(),
        Vec3::X,
    )
    .expect("a well-formed arm solves");
    assert_near(
        at(&palette, ELBOW),
        shoulder() + towards * UPPER,
        "the elbow",
    );
    assert_near(
        at(&palette, WRIST),
        shoulder() + towards * (UPPER - LOWER),
        "the wrist",
    );
}

/// A non-uniform scale on a joint's own transform is applied after its
/// rotation, so it bends nothing: the solve still lands. Here the wrist is
/// squashed, and the middle joint's scale halves the forearm to 2.
#[test]
fn a_joint_scaled_non_uniformly_itself_still_solves() {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[WRIST].scale = Vec3::new(1.0, 3.0, 0.5);
    pose.locals_mut()[ELBOW].scale = Vec3::new(1.5, 0.5, 1.0);
    let mut palette = Palette::new(&skeleton);
    let target = shoulder() + Vec3::new(4.0, -3.0, 2.0);
    solve_two_bone(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        ARM,
        target,
        Vec3::Z,
    )
    .expect("only the frames above the turned joints must be conformal");
    assert_near(at(&palette, WRIST), target, "the wrist");
}

/// A mirrored body is a rotation times a reflection: conformal, and solved.
#[test]
fn a_mirrored_parent_frame_still_solves() {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[BODY].scale = Vec3::new(-1.0, 1.0, 1.0);
    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);
    let shoulder = at(&palette, SHOULDER);
    let target = shoulder + Vec3::new(4.0, -3.0, 2.0);
    solve_two_bone(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        ARM,
        target,
        Vec3::Z,
    )
    .expect("a mirror is conformal");
    assert_near(at(&palette, WRIST), target, "the wrist");
    assert!((at(&palette, ELBOW) - shoulder).z > 0.0, "the pole side");
}

/// Solves `ARM` on a posed copy and asserts it is refused with `expected`,
/// leaving the pose and palette untouched.
fn assert_refused(
    skeleton: &Skeleton,
    pose: Pose,
    joints: [usize; 3],
    target: Vec3,
    pole: Vec3,
    expected: IkError,
) {
    let mut palette = Palette::new(skeleton);
    let mut after = pose.clone();
    let error = solve_two_bone(
        skeleton,
        model(),
        &mut after,
        &mut palette,
        joints,
        target,
        pole,
    )
    .expect_err("the call should be refused");
    assert_eq!(error, expected);
    // Compared through `Debug`, because a pose holding a `NaN` is never `==`
    // to itself.
    assert_eq!(
        format!("{after:?}"),
        format!("{pose:?}"),
        "a refusal writes nothing"
    );
    assert_eq!(palette, Palette::new(skeleton), "a refusal writes nothing");
}

#[test]
fn refuses_a_pose_or_palette_for_another_skeleton() {
    let skeleton = arm();
    let small = Skeleton::new(vec![joint(None, Vec3::ZERO)]).expect("one root");
    let mut pose = Pose::new(&small);
    let mut palette = Palette::new(&skeleton);
    let error = solve_two_bone(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        ARM,
        Vec3::ZERO,
        Vec3::X,
    );
    assert_eq!(
        error,
        Err(IkError::PoseMismatch {
            pose: 1,
            skeleton: 4
        })
    );
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&small);
    let error = rotate_joint(
        &skeleton,
        model(),
        &mut pose,
        &mut palette,
        WRIST,
        Quat::IDENTITY,
    );
    assert_eq!(
        error,
        Err(IkError::PaletteMismatch {
            palette: 1,
            skeleton: 4
        })
    );
}

#[test]
fn refuses_a_joint_that_is_not_there() {
    let skeleton = arm();
    assert_refused(
        &skeleton,
        Pose::new(&skeleton),
        [SHOULDER, ELBOW, 9],
        Vec3::ZERO,
        Vec3::X,
        IkError::JointOutOfRange {
            joint: 9,
            joints: 4,
        },
    );
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    assert_eq!(
        rotate_joint(
            &skeleton,
            model(),
            &mut pose,
            &mut palette,
            4,
            Quat::IDENTITY
        ),
        Err(IkError::JointOutOfRange {
            joint: 4,
            joints: 4
        })
    );
}

#[test]
fn refuses_joints_that_are_not_a_chain() {
    let skeleton = arm();
    assert_refused(
        &skeleton,
        Pose::new(&skeleton),
        [SHOULDER, WRIST, ELBOW],
        Vec3::ZERO,
        Vec3::X,
        IkError::NotAChain {
            joint: ELBOW,
            ancestor: WRIST,
        },
    );
    assert_refused(
        &skeleton,
        Pose::new(&skeleton),
        [SHOULDER, SHOULDER, WRIST],
        Vec3::ZERO,
        Vec3::X,
        IkError::NotAChain {
            joint: SHOULDER,
            ancestor: SHOULDER,
        },
    );
}

#[test]
fn refuses_a_zero_length_bone() {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[ELBOW].translation = Vec3::ZERO;
    assert_refused(
        &skeleton,
        pose,
        ARM,
        Vec3::ZERO,
        Vec3::X,
        IkError::ZeroLengthBone { joint: SHOULDER },
    );
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[WRIST].translation = Vec3::ZERO;
    assert_refused(
        &skeleton,
        pose,
        ARM,
        Vec3::ZERO,
        Vec3::X,
        IkError::ZeroLengthBone { joint: ELBOW },
    );
}

#[test]
fn refuses_inputs_that_are_not_finite() {
    let skeleton = arm();
    let pose = Pose::new(&skeleton);
    for (target, pole, input) in [
        (Vec3::new(f32::NAN, 0.0, 0.0), Vec3::X, IkInput::Target),
        (
            Vec3::ZERO,
            Vec3::new(0.0, f32::INFINITY, 0.0),
            IkInput::Pole,
        ),
    ] {
        assert_refused(
            &skeleton,
            pose.clone(),
            ARM,
            target,
            pole,
            IkError::NotFinite { input },
        );
    }
    let mut broken = pose.clone();
    broken.locals_mut()[BODY].rotation = Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0);
    assert_refused(
        &skeleton,
        broken,
        ARM,
        Vec3::ZERO,
        Vec3::X,
        IkError::NotFinite {
            input: IkInput::Pose,
        },
    );

    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    let infinite = Mat4::from_translation(Vec3::splat(f32::INFINITY));
    assert_eq!(
        solve_two_bone(
            &skeleton,
            infinite,
            &mut pose,
            &mut palette,
            ARM,
            Vec3::ZERO,
            Vec3::X
        ),
        Err(IkError::NotFinite {
            input: IkInput::Model
        })
    );
    assert_eq!(
        rotate_joint(
            &skeleton,
            infinite,
            &mut pose,
            &mut palette,
            ELBOW,
            Quat::IDENTITY
        ),
        Err(IkError::NotFinite {
            input: IkInput::Model
        })
    );
    let nan = Quat::from_xyzw(0.0, f32::NAN, 0.0, 1.0);
    assert_eq!(
        rotate_joint(&skeleton, model(), &mut pose, &mut palette, ELBOW, nan),
        Err(IkError::NotFinite {
            input: IkInput::Rotation
        })
    );
    let zero = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
    assert_eq!(
        rotate_joint(&skeleton, model(), &mut pose, &mut palette, ELBOW, zero),
        Err(IkError::ZeroRotation)
    );
    assert_eq!(pose, Pose::new(&skeleton), "a refusal writes nothing");
}

/// A non-uniform scale above a turned joint makes a model-space rotation of it
/// inexpressible as a local one: refused for the joint whose frame it is.
#[test]
fn refuses_a_parent_frame_scaled_non_uniformly() {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[BODY].scale = Vec3::new(1.0, 2.0, 1.0);
    assert_refused(
        &skeleton,
        pose.clone(),
        ARM,
        Vec3::ZERO,
        Vec3::X,
        IkError::NonConformalFrame { joint: SHOULDER },
    );
    let mut palette = Palette::new(&skeleton);
    assert_eq!(
        rotate_joint(
            &skeleton,
            model(),
            &mut pose,
            &mut palette,
            SHOULDER,
            Quat::IDENTITY
        ),
        Err(IkError::NonConformalFrame { joint: SHOULDER })
    );

    // The upper joint's own scale is the middle joint's frame.
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[SHOULDER].scale = Vec3::new(2.0, 1.0, 1.0);
    assert_refused(
        &skeleton,
        pose,
        ARM,
        Vec3::ZERO,
        Vec3::X,
        IkError::NonConformalFrame { joint: ELBOW },
    );

    // A singular model is no frame at all.
    let mut pose = Pose::new(&skeleton);
    assert_eq!(
        rotate_joint(
            &skeleton,
            Mat4::from_scale(Vec3::ZERO),
            &mut pose,
            &mut palette,
            BODY,
            Quat::IDENTITY
        ),
        Err(IkError::NonConformalFrame { joint: BODY })
    );
}

#[test]
fn errors_say_which_joint() {
    assert_eq!(
        IkError::NotAChain {
            joint: 3,
            ancestor: 5
        }
        .to_string(),
        "joint 3 does not descend from joint 5"
    );
    assert_eq!(
        IkError::NotFinite {
            input: IkInput::Pole
        }
        .to_string(),
        "the pole is not finite"
    );
}

/// By hand: a root turned a quarter about `+Z`, and a child one unit along its
/// local `+X` (so at model `(0, 1, 0)`). Turning the child a quarter about
/// model `+X` is, in the parent's space, a quarter turn about
/// `Rz(-90°)·X = -Y`.
#[test]
fn rotate_joint_turns_in_model_space() {
    let mut root = joint(None, Vec3::ZERO);
    root.rest.rotation = Quat::from_rotation_z(FRAC_PI_2);
    let skeleton = Skeleton::new(vec![root, joint(Some(0), Vec3::X)]).expect("one chain");
    let mut pose = Pose::new(&skeleton);
    let mut palette = Palette::new(&skeleton);
    let turn = Quat::from_rotation_x(FRAC_PI_2);
    rotate_joint(&skeleton, Mat4::IDENTITY, &mut pose, &mut palette, 1, turn)
        .expect("a conformal frame");

    let expected = Quat::from_axis_angle(Vec3::NEG_Y, FRAC_PI_2);
    let local = pose.locals()[1];
    assert!(
        local.rotation.dot(expected).abs() > 1.0 - 1e-6,
        "{:?}, expected {expected:?}",
        local.rotation
    );
    assert_eq!(local.translation, Vec3::X);
    assert_eq!(pose.locals()[0], skeleton.joints()[0].rest);
    // Model space: the child's +Y (model -X) turns about model +X, unmoved;
    // its +Z (model +Z) turns to model -Y.
    let global = palette.globals()[1];
    assert_near(global.transform_point3(Vec3::ZERO), Vec3::Y, "the origin");
    assert_near(global.transform_vector3(Vec3::Y), Vec3::NEG_X, "local +Y");
    assert_near(global.transform_vector3(Vec3::Z), Vec3::NEG_Y, "local +Z");
}

/// The defining property, under a model that scales, rotates, moves and
/// mirrors: the joint's model-space basis becomes the rotation times the old
/// one, and its origin does not move.
#[test]
fn rotate_joint_composes_on_the_left_in_model_space() {
    let skeleton = arm();
    let model = Mat4::from_scale_rotation_translation(
        Vec3::new(-2.0, 2.0, 2.0),
        Quat::from_rotation_y(1.1),
        Vec3::new(3.0, -1.0, 0.5),
    );
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[ELBOW].rotation = Quat::from_rotation_z(0.7);
    let before = model * posed(&skeleton, &pose).globals()[ELBOW];
    let mut palette = Palette::new(&skeleton);
    let turn = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0).normalize(), 0.9);
    rotate_joint(&skeleton, model, &mut pose, &mut palette, ELBOW, turn)
        .expect("a mirror is conformal");
    let after = model * palette.globals()[ELBOW];
    let expected = Mat3::from_quat(turn) * Mat3::from_mat4(before);
    assert!(
        Mat3::from_mat4(after).abs_diff_eq(expected, 1e-5),
        "{after:?}, expected {expected:?}"
    );
    assert_near(
        after.transform_point3(Vec3::ZERO),
        before.transform_point3(Vec3::ZERO),
        "the origin",
    );
}

/// EW's own semantics for a proper frame: `P⁻¹ · delta · P · local`, with `P`
/// the decomposed rotation of the parent's model transform.
#[test]
fn rotate_joint_matches_ew_on_a_proper_frame() {
    let skeleton = arm();
    let mut pose = Pose::new(&skeleton);
    let delta = Quat::from_rotation_arc(Vec3::X, Vec3::new(0.0, 0.6, 0.8));
    let parent = (model() * posed(&skeleton, &pose).globals()[SHOULDER])
        .to_scale_rotation_translation()
        .1;
    let ew = (parent.inverse() * delta * parent * pose.locals()[ELBOW].rotation).normalize();
    let mut palette = Palette::new(&skeleton);
    rotate_joint(&skeleton, model(), &mut pose, &mut palette, ELBOW, delta)
        .expect("a conformal frame");
    assert!(pose.locals()[ELBOW].rotation.dot(ew).abs() > 1.0 - 1e-6);
}
