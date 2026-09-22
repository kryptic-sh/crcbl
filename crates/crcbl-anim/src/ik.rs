//! Two-bone inverse kinematics, and the joint rotation it is built from.
//!
//! The first IK of `docs/plan/17-animation.md`'s post ops ("two-bone IK +
//! look-at"), and only that: no look-at, no full-body solve and no weights. A
//! partial solve is the caller's to blend, because which pose it blends
//! against is the caller's question.
//!
//! ```text
//! rotate_joint     turn one joint by a rotation spelled in model space
//! solve_two_bone   upper, middle, end joint: put the end on a target, the
//!                  middle bent towards a pole — two rotate_joint calls
//! ```
//!
//! # Spaces
//!
//! Both functions take a `model` matrix, the skeleton's placement in the space
//! the caller's targets are in. A joint's *model-space* transform is `model`
//! times its global transform ([`Palette::globals`]); targets, poles and
//! rotations are all given in that space. Pass [`Mat4::IDENTITY`] to work in
//! the skeleton's own space.
//!
//! Each function writes the new local rotations into the [`Pose`] — translation
//! and scale are never touched — and then recomputes the [`Palette`] from it,
//! so the palette is current when the call returns. Positions are read from the
//! pose, never from the palette, so a palette that is stale on entry changes
//! nothing about the answer.
//!
//! # What is refused, and what is handled
//!
//! Every [`IkError`] is detected before anything is written: a refused call
//! leaves the pose and the palette exactly as they were.
//!
//! * **Refused** — a pose or palette built for another skeleton, a joint index
//!   that is not a joint, three joints that are not an ancestor chain, a bone of
//!   zero length (no direction to rotate), a `NaN` or infinite input, a zero
//!   quaternion, and a joint whose parent frame is not a rotation times a
//!   uniform scale (see [`FRAME_TOLERANCE`]). Under a non-uniform scale or a
//!   shear a model-space rotation of the joint cannot be written as a change to
//!   its local rotation at all, so there is no honest answer to approximate.
//! * **Handled** — a target beyond reach extends the chain fully towards it; a
//!   target nearer the root than the two bones can fold to stops at that
//!   nearest reachable distance; a pole parallel to the target direction falls
//!   back to the chain's current bend plane. A non-uniform scale on a joint's
//!   *own* local transform is fine: the rotation is applied before it, and only
//!   the frames *above* the rotated joint have to be conformal. A mirrored
//!   parent frame (negative determinant) is conformal and handled exactly.

use std::fmt;

use glam::{Mat3, Mat4, Quat, Vec3, Vec4};

use crate::{Palette, Pose, Skeleton};

/// How far a parent frame may be from a rotation times a uniform scale, as a
/// fraction of that scale, before [`IkError::NonConformalFrame`] refuses it.
///
/// Measured on the frame's basis columns: each column's length may differ from
/// the longest by this fraction of it, and each pair's dot product may be this
/// fraction of the square of it. Chosen well above the error `f32` accumulates
/// composing a deep hierarchy of exact rotations and uniform scales, and far
/// below any non-uniform scale an artist would author on purpose.
pub const FRAME_TOLERANCE: f32 = 1e-4;

/// The sine of the angle below which [`solve_two_bone`] treats a pole, or the
/// chain's current bend, as lying on the root–target line and so giving no
/// bend direction.
///
/// Chosen far above [`f32::EPSILON`], where the part of a vector off the line
/// is rounding error pointing anywhere, and far below any angle a caller would
/// pick a pole at on purpose.
pub const PARALLEL_TOLERANCE: f32 = 1e-4;

/// Which input to an IK call held a `NaN` or an infinity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IkInput {
    /// The `model` matrix.
    Model,
    /// The target position.
    Target,
    /// The pole direction.
    Pole,
    /// [`rotate_joint`]'s `model_rotation`.
    Rotation,
    /// The pose: a local transform on the path from a root to a joint the call
    /// reads.
    Pose,
}

impl fmt::Display for IkInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Model => "the model matrix",
            Self::Target => "the target",
            Self::Pole => "the pole",
            Self::Rotation => "the rotation",
            Self::Pose => "the pose",
        })
    }
}

/// Why an IK call refused its inputs. See the [module docs](self) for why
/// each of these is refused rather than repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IkError {
    /// The pose was built for a skeleton with a different joint count.
    PoseMismatch {
        /// The pose's joint count.
        pose: usize,
        /// The skeleton's joint count.
        skeleton: usize,
    },
    /// The palette was built for a skeleton with a different joint count.
    PaletteMismatch {
        /// The palette's joint count.
        palette: usize,
        /// The skeleton's joint count.
        skeleton: usize,
    },
    /// A joint index is not a joint of the skeleton.
    JointOutOfRange {
        /// The index given.
        joint: usize,
        /// The skeleton's joint count.
        joints: usize,
    },
    /// A chain joint does not hang below the joint before it, so turning that
    /// joint would not move it.
    NotAChain {
        /// The joint that should be a descendant.
        joint: usize,
        /// The joint it should descend from.
        ancestor: usize,
    },
    /// An input was `NaN` or infinite.
    NotFinite {
        /// Which one.
        input: IkInput,
    },
    /// The rotation was the zero quaternion, which is no rotation at all.
    ZeroRotation,
    /// A chain bone has zero length in model space, so it has no direction to
    /// turn.
    ZeroLengthBone {
        /// The joint the bone starts at: the upper or the middle joint.
        joint: usize,
    },
    /// A joint's parent frame in model space is not a rotation times a uniform
    /// scale: it scales non-uniformly, shears or is singular.
    NonConformalFrame {
        /// The joint whose parent frame it is.
        joint: usize,
    },
}

impl fmt::Display for IkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::PoseMismatch { pose, skeleton } => write!(
                f,
                "the pose has {pose} joint(s) but the skeleton has {skeleton}"
            ),
            Self::PaletteMismatch { palette, skeleton } => write!(
                f,
                "the palette has {palette} joint(s) but the skeleton has {skeleton}"
            ),
            Self::JointOutOfRange { joint, joints } => write!(
                f,
                "joint {joint} is not a joint of a {joints}-joint skeleton"
            ),
            Self::NotAChain { joint, ancestor } => {
                write!(f, "joint {joint} does not descend from joint {ancestor}")
            }
            Self::NotFinite { input } => write!(f, "{input} is not finite"),
            Self::ZeroRotation => f.write_str("the rotation is the zero quaternion"),
            Self::ZeroLengthBone { joint } => {
                write!(f, "the bone starting at joint {joint} has zero length")
            }
            Self::NonConformalFrame { joint } => write!(
                f,
                "joint {joint}'s parent frame is not a rotation times a uniform scale"
            ),
        }
    }
}

impl std::error::Error for IkError {}

/// Turns `joint` about its own origin by `model_rotation`, a rotation **in
/// model space** — the frame of `model` times the palette's globals, the one
/// targets and poles are given in — not in the joint's or its parent's local
/// space.
///
/// Afterwards the joint's model-space basis is `model_rotation` times what it
/// was, and so is every descendant's, pivoting on the joint: a torso rolled
/// about a spine joint, a wrist turned onto a socket's frame. Only the joint's
/// local rotation changes; the palette is recomputed from the pose before
/// returning. `model_rotation` need not be exactly unit length: it is
/// normalised first.
///
/// For a parent frame that is a rotation `P` times a uniform scale this is
/// `local = P⁻¹ · model_rotation · P · local`, the conjugation that moves a
/// model-space rotation into the parent's space. It is computed with the
/// frame's orthonormal basis rather than a decomposed quaternion, so a mirrored
/// frame gets the same guarantee.
///
/// # Errors
///
/// [`IkError`] for a mismatched pose or palette, a joint out of range, a
/// non-finite `model`, `model_rotation` or pose, a zero `model_rotation`, or a
/// parent frame that is not conformal. Nothing is written when it errors.
pub fn rotate_joint(
    skeleton: &Skeleton,
    model: Mat4,
    pose: &mut Pose,
    palette: &mut Palette,
    joint: usize,
    model_rotation: Quat,
) -> Result<(), IkError> {
    check_sizes(skeleton, pose, palette)?;
    check_joint(skeleton, joint)?;
    check_finite(model.is_finite(), IkInput::Model)?;
    check_finite(model_rotation.is_finite(), IkInput::Rotation)?;
    let rotation = Vec4::from(model_rotation)
        .try_normalize()
        .map(Quat::from_vec4)
        .ok_or(IkError::ZeroRotation)?;
    check_finite(pose.locals()[joint].rotation.is_finite(), IkInput::Pose)?;
    let frame = parent_frame(skeleton, pose, model, joint)?;
    turn(pose, joint, frame, rotation);
    palette.compute(skeleton, pose);
    Ok(())
}

/// Bends the chain `joints = [upper, middle, end]` so that `end` lands on
/// `target`, with `middle` bent towards `pole`.
///
/// The analytic two-bone solve: the triangle root–middle–target is fixed by the
/// two bone lengths and the root–target distance (law of cosines), and `pole`
/// picks which way out of the root–target line it opens. `target` is a model
/// space position and `pole` a model space direction; only the part of `pole`
/// perpendicular to the root–target line matters, and its length does not.
///
/// `upper` then `middle` are turned with [`rotate_joint`]'s rule, so only their
/// local rotations change. The middle joint need not be the upper joint's
/// child, nor the end the middle's: twist joints between them are carried
/// along. Bone lengths are the distances between the three joints in model
/// space, and they are never stretched.
///
/// * A target farther than the two bones reach extends the chain straight
///   towards it.
/// * A target nearer than `|upper - middle|` bone length folds the chain flat
///   and puts the end that distance out along the target direction. With equal
///   bones that distance is zero: the end folds back onto the root.
/// * A target on the root takes the direction to the current end effector, or
///   failing that, the upper bone's, as its direction.
/// * A pole with no usable component off the root–target line — zero, or
///   within [`PARALLEL_TOLERANCE`] of parallel to it — keeps the chain's
///   current bend plane; if the chain is that straight along the target line
///   too, an arbitrary perpendicular is used.
///
/// # Errors
///
/// [`IkError`] for a mismatched pose or palette, a joint out of range, joints
/// that are not an ancestor chain, a non-finite `model`, `target`, `pole` or
/// pose, a zero-length bone, or an upper or middle parent frame that is not
/// conformal. Nothing is written when it errors.
pub fn solve_two_bone(
    skeleton: &Skeleton,
    model: Mat4,
    pose: &mut Pose,
    palette: &mut Palette,
    joints: [usize; 3],
    target: Vec3,
    pole: Vec3,
) -> Result<(), IkError> {
    check_sizes(skeleton, pose, palette)?;
    for joint in joints {
        check_joint(skeleton, joint)?;
    }
    for pair in joints.windows(2) {
        if !descends(skeleton, pair[1], pair[0]) {
            return Err(IkError::NotAChain {
                joint: pair[1],
                ancestor: pair[0],
            });
        }
    }
    check_finite(model.is_finite(), IkInput::Model)?;
    check_finite(target.is_finite(), IkInput::Target)?;
    check_finite(pole.is_finite(), IkInput::Pole)?;

    let [root, middle_now, end_now] =
        joints.map(|joint| model_position(skeleton, pose, model, joint));
    check_finite(
        root.is_finite() && middle_now.is_finite() && end_now.is_finite(),
        IkInput::Pose,
    )?;
    let upper_bone = middle_now - root;
    let upper_direction = upper_bone
        .try_normalize()
        .ok_or(IkError::ZeroLengthBone { joint: joints[0] })?;
    if (end_now - middle_now).try_normalize().is_none() {
        return Err(IkError::ZeroLengthBone { joint: joints[1] });
    }
    // The middle joint's frame is only needed after the upper joint turns, but
    // it is checked now so a refusal writes nothing. Turning the upper joint
    // rotates it rigidly, which keeps a conformal frame conformal.
    let upper_frame = parent_frame(skeleton, pose, model, joints[0])?;
    let middle_frame = parent_frame(skeleton, pose, model, joints[1])?;

    let upper = upper_bone.length();
    let lower = (end_now - middle_now).length();
    let offset = target - root;
    let forward = offset
        .try_normalize()
        .or_else(|| (end_now - root).try_normalize())
        .unwrap_or(upper_direction);
    let distance = offset.length().clamp((upper - lower).abs(), upper + lower);
    let bend = perpendicular(pole, forward)
        .or_else(|| perpendicular(upper_bone, forward))
        .unwrap_or_else(|| forward.any_orthonormal_vector());
    // How far along `forward` the middle joint sits, from the law of cosines.
    // `distance` is zero only when the bones are equal and fold flat, where the
    // limit of the expression is zero.
    let along = if distance > 0.0 {
        (upper * upper - lower * lower + distance * distance) / (2.0 * distance)
    } else {
        0.0
    };
    let out = (upper * upper - along * along).max(0.0).sqrt();
    let middle = root + forward * along + bend * out;
    let end = root + forward * distance;

    let upper_turn = Quat::from_rotation_arc(upper_direction, (middle - root).normalize());
    turn(pose, joints[0], upper_frame, upper_turn);
    let middle_now = model_position(skeleton, pose, model, joints[1]);
    let end_now = model_position(skeleton, pose, model, joints[2]);
    let middle_turn = Quat::from_rotation_arc(
        (end_now - middle_now).normalize(),
        (end - middle_now).normalize(),
    );
    turn(
        pose,
        joints[1],
        Mat3::from_quat(upper_turn) * middle_frame,
        middle_turn,
    );
    palette.compute(skeleton, pose);
    Ok(())
}

fn check_sizes(skeleton: &Skeleton, pose: &Pose, palette: &Palette) -> Result<(), IkError> {
    if pose.len() != skeleton.len() {
        return Err(IkError::PoseMismatch {
            pose: pose.len(),
            skeleton: skeleton.len(),
        });
    }
    if palette.len() != skeleton.len() {
        return Err(IkError::PaletteMismatch {
            palette: palette.len(),
            skeleton: skeleton.len(),
        });
    }
    Ok(())
}

fn check_joint(skeleton: &Skeleton, joint: usize) -> Result<(), IkError> {
    if joint < skeleton.len() {
        Ok(())
    } else {
        Err(IkError::JointOutOfRange {
            joint,
            joints: skeleton.len(),
        })
    }
}

fn check_finite(finite: bool, input: IkInput) -> Result<(), IkError> {
    if finite {
        Ok(())
    } else {
        Err(IkError::NotFinite { input })
    }
}

/// Whether `joint` hangs somewhere below `ancestor`, not counting itself.
fn descends(skeleton: &Skeleton, joint: usize, ancestor: usize) -> bool {
    let mut next = skeleton.joints()[joint].parent;
    while let Some(parent) = next {
        if parent == ancestor {
            return true;
        }
        next = skeleton.joints()[parent].parent;
    }
    false
}

/// `joint`'s global transform, composed from the pose down its ancestors —
/// the same product [`Palette::compute`] forms, without needing the palette to
/// be current.
fn global(skeleton: &Skeleton, pose: &Pose, joint: usize) -> Mat4 {
    let mut global = pose.locals()[joint].to_mat4();
    let mut next = skeleton.joints()[joint].parent;
    while let Some(parent) = next {
        global = pose.locals()[parent].to_mat4() * global;
        next = skeleton.joints()[parent].parent;
    }
    global
}

fn model_position(skeleton: &Skeleton, pose: &Pose, model: Mat4, joint: usize) -> Vec3 {
    (model * global(skeleton, pose, joint)).transform_point3(Vec3::ZERO)
}

/// The orthonormal basis of `joint`'s parent frame in model space: the frame's
/// rotation, or rotation and mirror, with its uniform scale divided out.
fn parent_frame(
    skeleton: &Skeleton,
    pose: &Pose,
    model: Mat4,
    joint: usize,
) -> Result<Mat3, IkError> {
    let frame = match skeleton.joints()[joint].parent {
        Some(parent) => model * global(skeleton, pose, parent),
        None => model,
    };
    let linear = Mat3::from_mat4(frame);
    check_finite(linear.is_finite(), IkInput::Pose)?;
    let columns = [linear.x_axis, linear.y_axis, linear.z_axis];
    let lengths = columns.map(Vec3::length);
    let scale = lengths[0].max(lengths[1]).max(lengths[2]);
    let tolerance = FRAME_TOLERANCE * scale;
    let conformal = scale > 0.0
        && lengths.iter().all(|&length| scale - length <= tolerance)
        && columns[0].dot(columns[1]).abs() <= tolerance * scale
        && columns[1].dot(columns[2]).abs() <= tolerance * scale
        && columns[2].dot(columns[0]).abs() <= tolerance * scale;
    if !conformal {
        return Err(IkError::NonConformalFrame { joint });
    }
    Ok(Mat3::from_cols(
        columns[0] / lengths[0],
        columns[1] / lengths[1],
        columns[2] / lengths[2],
    ))
}

/// Applies a model-space `rotation` to `joint`'s local rotation, given the
/// orthonormal basis of its parent frame.
fn turn(pose: &mut Pose, joint: usize, frame: Mat3, rotation: Quat) {
    let local_turn = Quat::from_mat3(&(frame.transpose() * Mat3::from_quat(rotation) * frame));
    let local = &mut pose.locals_mut()[joint];
    local.rotation = (local_turn * local.rotation).normalize();
}

/// The unit part of `vector` perpendicular to the unit `axis`, unless `vector`
/// is within [`PARALLEL_TOLERANCE`] of the axis line.
///
/// A near-parallel vector's remainder is mostly rounding error, which points
/// anywhere — including back along the axis — so it is refused rather than
/// normalised into a bend direction. What survives is projected a second time
/// to take the rounding out of it.
fn perpendicular(vector: Vec3, axis: Vec3) -> Option<Vec3> {
    let remainder = vector - axis * vector.dot(axis);
    if remainder.length() <= PARALLEL_TOLERANCE * vector.length() {
        return None;
    }
    (remainder - axis * remainder.dot(axis)).try_normalize()
}
