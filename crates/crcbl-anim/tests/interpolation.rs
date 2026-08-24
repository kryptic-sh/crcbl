//! The three glTF interpolation modes, against values worked out by hand from
//! Appendix C of the specification rather than against this crate's own output.
//!
//! The cases are chosen so that a plausible wrong implementation gives a
//! visibly different answer: a rotation turned far enough that a normalized
//! *linear* interpolation and the specification's *spherical* one are nowhere
//! near each other, a cubic segment whose duration is not one second so the
//! tangent scaling has an effect, and a spline on a rotation whose unnormalized
//! result is measurably short.

use crcbl_anim::{Channel, Clip, Interpolation, Joint, Pose, Skeleton, Track, Trs};
use glam::{Mat4, Quat, Vec3};

/// A skeleton of `count` unparented joints, each resting at the identity.
fn loose_joints(count: usize) -> Skeleton {
    Skeleton::new(
        (0..count)
            .map(|_| Joint {
                parent: None,
                inverse_bind: Mat4::IDENTITY,
                rest: Trs::IDENTITY,
            })
            .collect(),
    )
    .expect("unparented joints are trivially in hierarchy order")
}

/// Samples one channel on a one-joint skeleton and hands back that joint.
fn sample_one(channel: Channel, time: f32) -> Trs {
    let skeleton = loose_joints(1);
    let clip = Clip::new(vec![channel]);
    let mut pose = Pose::new(&skeleton);
    clip.sample_into(time, &skeleton, &mut pose);
    pose.locals()[0]
}

/// The rotation angle a quaternion represents, in radians, taking the shorter
/// of the two arcs — `w = cos(angle / 2)` for a unit quaternion.
fn angle_of(rotation: Quat) -> f32 {
    2.0 * rotation.w.abs().min(1.0).acos()
}

fn rotation_channel(times: Vec<f32>, values: Vec<Quat>, interpolation: Interpolation) -> Channel {
    Channel::new(0, times, interpolation, Track::Rotation(values)).expect("well formed")
}

fn translation_channel(
    times: Vec<f32>,
    values: Vec<Vec3>,
    interpolation: Interpolation,
) -> Channel {
    Channel::new(0, times, interpolation, Track::Translation(values)).expect("well formed")
}

/// §C.2, on a translation: `v_t = (1 - t) * v_k + t * v_{k+1}`.
///
/// Times 0 and 2, sampled at 0.5, so `t = 0.25` — a quarter of the way, not a
/// half, which is the difference between dividing by the segment duration and
/// forgetting to.
#[test]
fn linear_translation_is_the_weighted_sum() {
    let channel = translation_channel(
        vec![0.0, 2.0],
        vec![Vec3::ZERO, Vec3::new(4.0, -2.0, 10.0)],
        Interpolation::Linear,
    );
    assert_eq!(
        sample_one(channel, 0.5).translation,
        Vec3::new(1.0, -0.5, 2.5)
    );
}

/// §C.3, on a rotation: spherical, not linear.
///
/// The property that separates the two is that slerp has **constant angular
/// velocity** — a quarter of the way along a turn is a quarter of the turn.
/// Normalized linear interpolation is not: it moves fastest in the middle. A
/// 170° turn is far enough that the gap is degrees rather than a rounding
/// error, and the second assertion is what pins that the two really did
/// disagree here rather than the tolerance being loose.
#[test]
fn linear_rotation_turns_at_a_constant_rate() {
    let total = 170.0_f32.to_radians();
    let channel = rotation_channel(
        vec![0.0, 1.0],
        vec![Quat::IDENTITY, Quat::from_rotation_y(total)],
        Interpolation::Linear,
    );

    for (fraction, expected) in [(0.25, 0.25), (0.5, 0.5), (0.75, 0.75)] {
        let sampled = sample_one(channel.clone(), fraction).rotation;
        assert!(
            (angle_of(sampled) - expected * total).abs() < 1e-4,
            "at {fraction} of the way the turn should be {expected} of {total} rad, was {}",
            angle_of(sampled)
        );
    }

    let quarter = sample_one(channel, 0.25).rotation;
    let nlerp = (Quat::IDENTITY * 0.75 + Quat::from_rotation_y(total) * 0.25).normalize();
    assert!(
        angle_of(quarter) - angle_of(nlerp) > 0.1,
        "a normalized linear interpolation should lag the spherical one here"
    );
}

/// §C.3's implementation note: the absolute value and the sign together
/// "ensure that the spherical interpolation follows the short path".
///
/// `q` and `-q` are the same rotation, so a 20° turn written with a negated
/// end quaternion has a negative dot product. Handled, its midpoint is 10°;
/// unhandled, the interpolation goes the long way round and the midpoint is
/// 170° — the same visible bug as a character's arm swinging behind it.
#[test]
fn linear_rotation_takes_the_short_path() {
    let channel = rotation_channel(
        vec![0.0, 1.0],
        vec![
            Quat::IDENTITY,
            -Quat::from_rotation_y(20.0_f32.to_radians()),
        ],
        Interpolation::Linear,
    );
    let midpoint = sample_one(channel, 0.5).rotation;
    assert!(
        (angle_of(midpoint) - 10.0_f32.to_radians()).abs() < 1e-4,
        "the short arc's midpoint is 10 degrees, was {} degrees",
        angle_of(midpoint).to_degrees()
    );
}

/// §C.1: `v_t = v_k`. The **earlier** keyframe's value, held right up to the
/// next one — not the nearer keyframe's, which would flip at the midpoint.
#[test]
fn step_holds_the_earlier_keyframe() {
    let values = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(5.0, 0.0, 0.0),
        Vec3::new(9.0, 0.0, 0.0),
    ];
    let channel = translation_channel(vec![0.0, 1.0, 2.0], values, Interpolation::Step);

    for (time, expected) in [
        (0.001, Vec3::ZERO),
        (0.5, Vec3::ZERO),
        (0.999, Vec3::ZERO),
        (1.0, Vec3::new(5.0, 0.0, 0.0)),
        (1.75, Vec3::new(5.0, 0.0, 0.0)),
        (2.0, Vec3::new(9.0, 0.0, 0.0)),
    ] {
        assert_eq!(
            sample_one(channel.clone(), time).translation,
            expected,
            "STEP at {time}"
        );
    }
}

/// §C.1 again, on a **rotation**, because that is a different arm of the code
/// than the translation above.
///
/// `sample_quat` and `sample_vec3` hold the earlier keyframe separately, and a
/// STEP rotation that answered `v_{k+1}` passed every other test in this crate
/// — the translation case above cannot reach the quaternion path at all. The
/// angles are far apart so a wrong keyframe is not a near miss.
#[test]
fn step_holds_the_earlier_keyframe_for_a_rotation() {
    let channel = rotation_channel(
        vec![0.0, 1.0],
        vec![Quat::IDENTITY, Quat::from_rotation_y(90.0_f32.to_radians())],
        Interpolation::Step,
    );

    for (time, degrees) in [(0.001_f32, 0.0_f32), (0.5, 0.0), (0.999, 0.0), (1.0, 90.0)] {
        let held = sample_one(channel.clone(), time).rotation;
        assert!(
            (angle_of(held) - degrees.to_radians()).abs() < 1e-4,
            "STEP at {time} should hold {degrees} degrees, was {}",
            angle_of(held).to_degrees(),
        );
    }
}

/// §C.4, evaluated by hand.
///
/// Keyframes at 0 s and 2 s, so `t_d = 2`, sampled at 1 s, so `t = 0.5`. The
/// four basis terms are then `2(0.125) - 3(0.25) + 1 = 0.5`,
/// `2(0.125 - 0.5 + 0.5) = 0.25`, `-2(0.125) + 3(0.25) = 0.5` and
/// `2(0.125 - 0.25) = -0.25`, and with `v_k = 0`, `b_k = 1`, `v_{k+1} = 4` and
/// `a_{k+1} = 2` on the x axis that is
/// `0 + 0.25(1) + 0.5(4) - 0.25(2) = 1.75`.
///
/// Every term is a dyadic rational, so the assertion is exact rather than
/// within a tolerance: nothing in the evaluation rounds.
#[test]
fn cubic_spline_matches_a_hand_evaluated_segment() {
    let channel = translation_channel(
        vec![0.0, 2.0],
        vec![
            Vec3::ZERO,               // a_0, unused
            Vec3::ZERO,               // v_0
            Vec3::new(1.0, 0.0, 0.0), // b_0
            Vec3::new(2.0, 0.0, 0.0), // a_1
            Vec3::new(4.0, 0.0, 0.0), // v_1
            Vec3::ZERO,               // b_1, unused
        ],
        Interpolation::CubicSpline,
    );
    assert_eq!(
        sample_one(channel, 1.0).translation,
        Vec3::new(1.75, 0.0, 0.0)
    );
}

/// The same spline over a one-second segment, which is a different curve.
///
/// §C.4 scales both tangent terms by `t_d`, so halving the segment duration
/// halves their contribution: `0 + 0.125(1) + 0.5(4) - 0.125(2) = 1.875`. An
/// implementation that dropped the `t_d` factor would answer this for the two-
/// second segment above as well, and the pair is what catches it.
#[test]
fn cubic_spline_scales_its_tangents_by_the_segment_duration() {
    let channel = translation_channel(
        vec![0.0, 1.0],
        vec![
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::ZERO,
        ],
        Interpolation::CubicSpline,
    );
    assert_eq!(
        sample_one(channel, 0.5).translation,
        Vec3::new(1.875, 0.0, 0.0)
    );
}

/// §C.4: "the interpolated quaternion **MUST** be normalized before applying
/// the result to the node's rotation."
///
/// With both tangents zero the polynomial reduces to `0.5 v_k + 0.5 v_{k+1}`,
/// which for a quarter turn is measurably shorter than a unit quaternion — a
/// rotation that also shrinks the bone it drives.
#[test]
fn cubic_spline_normalizes_a_rotation() {
    let end = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let channel = rotation_channel(
        vec![0.0, 1.0],
        vec![
            Quat::IDENTITY, // a_0, unused
            Quat::IDENTITY, // v_0
            Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            end,            // v_1
            Quat::IDENTITY, // b_1, unused
        ],
        Interpolation::CubicSpline,
    );

    let raw = Quat::IDENTITY * 0.5 + end * 0.5;
    assert!(
        (raw.length() - 1.0).abs() > 1e-3,
        "the case is only a test if the unnormalized polynomial is short"
    );

    let sampled = sample_one(channel, 0.5).rotation;
    assert!(
        (sampled.length() - 1.0).abs() < 1e-6,
        "the sampled rotation should be a unit quaternion, its length was {}",
        sampled.length()
    );
    assert!(sampled.abs_diff_eq(raw.normalize(), 1e-6));
}

/// §C: "When the current (requested) timestamp exists in the animation data,
/// its associated property value **MUST** be used as-is, without
/// interpolation."
///
/// As-is means bit for bit. The slerp of §C.3 divides by `sin(a)` and would
/// answer a keyframe's own value only to within a few ulps, so the exact hit is
/// a case of its own rather than something the arithmetic falls into.
#[test]
fn an_exact_keyframe_time_is_used_as_is() {
    let middle = Quat::from_rotation_x(1.234);
    let channel = rotation_channel(
        vec![0.0, 1.0, 2.0],
        vec![Quat::IDENTITY, middle, Quat::from_rotation_z(0.5)],
        Interpolation::Linear,
    );
    assert_eq!(sample_one(channel, 1.0).rotation, middle);
}

/// A time before the first keyframe and after the last holds the nearest one.
///
/// Clamped, not wrapped: looping is the caller's modulo, and a clip that must
/// hold its final pose could not say so otherwise.
#[test]
fn a_time_outside_the_clip_holds_the_nearest_keyframe() {
    let channel = translation_channel(
        vec![1.0, 2.0],
        vec![Vec3::new(3.0, 0.0, 0.0), Vec3::new(7.0, 0.0, 0.0)],
        Interpolation::Linear,
    );
    assert_eq!(
        sample_one(channel.clone(), 0.0).translation,
        Vec3::new(3.0, 0.0, 0.0)
    );
    assert_eq!(
        sample_one(channel.clone(), -1e9).translation,
        Vec3::new(3.0, 0.0, 0.0)
    );
    assert_eq!(
        sample_one(channel.clone(), 5.0).translation,
        Vec3::new(7.0, 0.0, 0.0)
    );
    // Not wrapped: 3 s into a clip that ends at 2 s is not 1 s into it.
    assert_ne!(
        sample_one(channel, 3.0).translation,
        Vec3::new(3.0, 0.0, 0.0)
    );
}
