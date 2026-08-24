//! The palette, against a two-joint chain composed by hand, and the three
//! sampling cases that are shapes rather than errors.

use crcbl_anim::{Channel, Clip, Interpolation, Joint, Palette, Pose, Skeleton, Track, Trs};
use glam::{Mat4, Quat, Vec3};

/// Every entry of two matrices within `1e-5`.
fn assert_close(actual: Mat4, expected: Mat4, what: &str) {
    assert!(
        actual.abs_diff_eq(expected, 1e-5),
        "{what}:\n  actual   {actual:?}\n  expected {expected:?}"
    );
}

/// A quarter turn about `+Z`, which sends `+X` to `+Y` and `+Y` to `-X`.
fn quarter_turn_z() -> Quat {
    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
}

/// Two joints, the second hanging two metres up the first's local `+Y`.
///
/// Bind pose: joint 0 at the origin, joint 1 at `(0, 2, 0)`. The inverse bind
/// matrices are the inverses of those, so `inverse_bind[0]` is the identity and
/// `inverse_bind[1]` is a translation by `(0, -2, 0)`.
fn chain() -> Skeleton {
    Skeleton::new(vec![
        Joint {
            parent: None,
            inverse_bind: Mat4::IDENTITY,
            rest: Trs::IDENTITY,
        },
        Joint {
            parent: Some(0),
            inverse_bind: Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0)),
            rest: Trs {
                translation: Vec3::new(0.0, 2.0, 0.0),
                ..Trs::IDENTITY
            },
        },
    ])
    .expect("the parent precedes the child")
}

/// The composed palette of a two-joint chain, worked out by hand.
///
/// Pose: joint 0 moved to `(1, 0, 0)` and turned a quarter turn about `+Z`;
/// joint 1 left at its rest offset of `(0, 2, 0)` but scaled by two.
///
/// By hand, writing `R` for the quarter turn:
///
/// * `global[0] = T(1,0,0) · R`. Its origin is `(1, 0, 0)` and its `+X` column
///   points along `+Y`.
/// * `global[1] = global[0] · T(0,2,0) · S(2)`. The offset `(0, 2, 0)` turns
///   into `(-2, 0, 0)`, so the origin is `(1,0,0) + (-2,0,0) = (-1, 0, 0)`, and
///   the basis is `R` scaled by two.
/// * `palette[0] = global[0] · I = global[0]`.
/// * `palette[1] = global[1] · T(0,-2,0)`. The basis is unchanged and the
///   origin moves by `2R · (0,-2,0) = (4, 0, 0)`, to `(3, 0, 0)`.
#[test]
fn composes_a_two_joint_chain() {
    let skeleton = chain();
    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[0] = Trs {
        translation: Vec3::new(1.0, 0.0, 0.0),
        rotation: quarter_turn_z(),
        scale: Vec3::ONE,
    };
    pose.locals_mut()[1] = Trs {
        translation: Vec3::new(0.0, 2.0, 0.0),
        rotation: Quat::IDENTITY,
        scale: Vec3::splat(2.0),
    };

    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);

    let expected_global_0 = Mat4::from_cols_array_2d(&[
        [0.0, 1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0, 1.0],
    ]);
    let expected_global_1 = Mat4::from_cols_array_2d(&[
        [0.0, 2.0, 0.0, 0.0],
        [-2.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
        [-1.0, 0.0, 0.0, 1.0],
    ]);
    let expected_palette_1 = Mat4::from_cols_array_2d(&[
        [0.0, 2.0, 0.0, 0.0],
        [-2.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
        [3.0, 0.0, 0.0, 1.0],
    ]);

    assert_close(palette.globals()[0], expected_global_0, "global[0]");
    assert_close(palette.globals()[1], expected_global_1, "global[1]");
    assert_close(palette.matrices()[0], expected_global_0, "palette[0]");
    assert_close(palette.matrices()[1], expected_palette_1, "palette[1]");

    // What the matrices mean, checked independently of their entries: a point
    // one metre above joint 1 in the bind pose ends up one *scaled* local unit
    // along the posed joint's `+Y`, which the quarter turn sends to `-X`.
    let tip = palette.matrices()[1].transform_point3(Vec3::new(0.0, 3.0, 0.0));
    assert!((tip - Vec3::new(-3.0, 0.0, 0.0)).length() < 1e-5, "{tip:?}");
}

/// The inverse bind matrix is applied on the *right*, not the left.
///
/// The two orders differ the moment the joint is neither at the origin nor
/// unrotated, and this is the smallest case that separates them: a single joint
/// bound at `(0, 5, 0)`, turned a quarter turn where it stands. Applied
/// correctly, a vertex at the bind position stays at `(0, 5, 0)`, because
/// nothing moved the joint. The reversed product sends it somewhere else
/// entirely.
#[test]
fn folds_the_inverse_bind_on_the_right() {
    let bind = Mat4::from_translation(Vec3::new(0.0, 5.0, 0.0));
    let skeleton = Skeleton::new(vec![Joint {
        parent: None,
        inverse_bind: bind.inverse(),
        rest: Trs {
            translation: Vec3::new(0.0, 5.0, 0.0),
            ..Trs::IDENTITY
        },
    }])
    .expect("a single root");

    let mut pose = Pose::new(&skeleton);
    pose.locals_mut()[0].rotation = quarter_turn_z();

    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);

    let matrix = palette.matrices()[0];
    let pinned = matrix.transform_point3(Vec3::new(0.0, 5.0, 0.0));
    assert!(
        (pinned - Vec3::new(0.0, 5.0, 0.0)).length() < 1e-5,
        "the joint's own bind position is its own pivot, was {pinned:?}"
    );

    let reversed = skeleton.joints()[0].inverse_bind * pose.locals()[0].to_mat4();
    assert!(
        !reversed.abs_diff_eq(matrix, 1e-3),
        "the case is only a test if the two orders disagree"
    );
}

/// A joint no channel drives keeps its **rest** transform, not the identity.
///
/// Identity would collapse joint 1 onto joint 0's origin, which is the whole
/// reason a rest pose is stored on the joint at all.
#[test]
fn an_undriven_joint_keeps_its_rest_pose() {
    let skeleton = chain();
    let spin = Channel::new(
        0,
        vec![0.0, 1.0],
        Interpolation::Linear,
        Track::Rotation(vec![Quat::IDENTITY, quarter_turn_z()]),
    )
    .expect("well formed");
    let clip = Clip::new(vec![spin]);

    let mut pose = Pose::new(&skeleton);
    clip.sample_into(1.0, &skeleton, &mut pose);

    assert_eq!(pose.locals()[1], skeleton.joints()[1].rest);

    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);
    let origin = palette.globals()[1].transform_point3(Vec3::ZERO);
    assert!(
        (origin - Vec3::new(-2.0, 0.0, 0.0)).length() < 1e-5,
        "joint 1 should still be two metres up its parent's turned +Y, was {origin:?}"
    );
}

/// A channel driving only one component leaves the other two at rest.
#[test]
fn a_partially_driven_joint_keeps_its_other_components() {
    let skeleton = chain();
    let spin = Channel::new(
        1,
        vec![0.0, 1.0],
        Interpolation::Linear,
        Track::Rotation(vec![Quat::IDENTITY, quarter_turn_z()]),
    )
    .expect("well formed");
    let clip = Clip::new(vec![spin]);

    let mut pose = Pose::new(&skeleton);
    clip.sample_into(1.0, &skeleton, &mut pose);

    assert_eq!(pose.locals()[1].translation, Vec3::new(0.0, 2.0, 0.0));
    assert_eq!(pose.locals()[1].scale, Vec3::ONE);
    assert!(
        pose.locals()[1]
            .rotation
            .abs_diff_eq(quarter_turn_z(), 1e-6)
    );
}

/// A channel naming a joint this skeleton has not got is skipped, and the rest
/// of the clip still plays.
#[test]
fn a_channel_for_another_skeleton_is_skipped() {
    let skeleton = chain();
    let stray = Channel::new(
        99,
        vec![0.0, 1.0],
        Interpolation::Linear,
        Track::Translation(vec![Vec3::ZERO, Vec3::new(1000.0, 0.0, 0.0)]),
    )
    .expect("well formed");
    let real = Channel::new(
        0,
        vec![0.0, 1.0],
        Interpolation::Linear,
        Track::Translation(vec![Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)]),
    )
    .expect("well formed");
    let clip = Clip::new(vec![stray, real]);

    let mut pose = Pose::new(&skeleton);
    clip.sample_into(1.0, &skeleton, &mut pose);

    assert_eq!(pose.locals()[0].translation, Vec3::new(4.0, 0.0, 0.0));
    assert_eq!(pose.locals()[1], skeleton.joints()[1].rest);
}

/// Sampling a second clip into the same pose leaves nothing of the first.
///
/// The buffer is reused frame to frame, which is what keeps sampling
/// allocation-free; it is also how a stale joint would survive a clip change if
/// the reset were dropped.
#[test]
fn sampling_resets_the_whole_pose() {
    let skeleton = chain();
    let moves_both = Clip::new(vec![
        Channel::new(
            0,
            vec![0.0],
            Interpolation::Step,
            Track::Translation(vec![Vec3::new(9.0, 9.0, 9.0)]),
        )
        .expect("well formed"),
        Channel::new(
            1,
            vec![0.0],
            Interpolation::Step,
            Track::Scale(vec![Vec3::splat(7.0)]),
        )
        .expect("well formed"),
    ]);
    let moves_one = Clip::new(vec![
        Channel::new(
            0,
            vec![0.0],
            Interpolation::Step,
            Track::Translation(vec![Vec3::new(1.0, 0.0, 0.0)]),
        )
        .expect("well formed"),
    ]);

    let mut reused = Pose::new(&skeleton);
    moves_both.sample_into(0.0, &skeleton, &mut reused);
    moves_one.sample_into(0.0, &skeleton, &mut reused);

    let mut fresh = Pose::new(&skeleton);
    moves_one.sample_into(0.0, &skeleton, &mut fresh);

    assert_eq!(reused, fresh);
    assert_eq!(reused.locals()[1].scale, Vec3::ONE);
}

/// A pose built for a different skeleton is a programming error, not a shape
/// with an answer.
#[test]
#[should_panic(expected = "different joint count")]
fn sampling_into_a_foreign_pose_panics() {
    let skeleton = chain();
    let other = Skeleton::new(vec![Joint {
        parent: None,
        inverse_bind: Mat4::IDENTITY,
        rest: Trs::IDENTITY,
    }])
    .expect("a single root");
    let mut pose = Pose::new(&other);
    Clip::new(Vec::new()).sample_into(0.0, &skeleton, &mut pose);
}
