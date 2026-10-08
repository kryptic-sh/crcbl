//! Physical body transforms back to a skeletal pose.
//!
//! Bind at the animation-to-physics handoff, using the last animated pose and
//! the rigid bodies placed around it. The captured joint/body offsets retain
//! imported skeleton scale and body centres that differ from joint origins.
//! This module does not create bodies, simulate joints, or choose death policy.

use std::fmt;

use glam::{Mat4, Vec3, Vec4};

use crate::{Palette, Pose, Skeleton, Trs};

/// Relative tolerance when reconstructing a local TRS from a physical pose.
pub const TRANSFORM_TOLERANCE: f32 = 1e-4;

/// A rigid body's world transform at handoff, and its resolved palette index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyJoint {
    /// Index in the animation skeleton, not an imported scene node index.
    pub joint: usize,
    /// Rigid world transform: rotation and translation, without scale or shear.
    pub world: Mat4,
}

/// Why a physical pose cannot be represented by the skeleton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RagdollError {
    /// The pose length differs from the bound skeleton.
    PoseSize,
    /// No body was bound.
    Empty,
    /// The supplied body count differs from the handoff.
    BodyCount,
    /// A binding names a joint outside the skeleton.
    JointOutOfRange { joint: usize },
    /// More than one body drives the same joint.
    DuplicateJoint { joint: usize },
    /// The skeleton placement is non-affine, singular, or non-finite.
    InvalidModel,
    /// A body transform is not a finite, proper rigid transform.
    InvalidBody { body: usize },
    /// A joint transform is invalid or cannot be represented without shear.
    InvalidJoint { joint: usize },
}

impl fmt::Display for RagdollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PoseSize => f.write_str("pose size differs from the ragdoll skeleton"),
            Self::Empty => f.write_str("ragdoll has no bound bodies"),
            Self::BodyCount => f.write_str("body count differs from the ragdoll binding"),
            Self::JointOutOfRange { joint } => write!(f, "ragdoll joint {joint} is out of range"),
            Self::DuplicateJoint { joint } => write!(f, "ragdoll joint {joint} is bound twice"),
            Self::InvalidModel => f.write_str("ragdoll model transform is invalid"),
            Self::InvalidBody { body } => write!(f, "ragdoll body {body} is not rigid"),
            Self::InvalidJoint { joint } => {
                write!(f, "ragdoll joint {joint} is not representable as TRS")
            }
        }
    }
}

impl std::error::Error for RagdollError {}

/// Captured body-to-joint offsets and reusable pose reconstruction storage.
///
/// Unbound joints keep their handoff local transforms, following their posed
/// parents. Body transforms passed to [`Self::pose_into`] use binding order;
/// bindings themselves need not be in parent-first order.
#[derive(Clone, Debug)]
pub struct RagdollBinding {
    skeleton: Skeleton,
    handoff: Pose,
    offsets: Vec<Mat4>,
    body_for_joint: Vec<Option<usize>>,
    scratch: Pose,
    globals: Vec<Mat4>,
}

impl RagdollBinding {
    /// Captures the last animated pose relative to the initialized bodies.
    ///
    /// `model` places the skeleton in the same world space as `bodies`.
    /// Invalid transforms, duplicate bindings and mismatched lengths return
    /// [`RagdollError`]. Scale belongs to the skeleton, never to rigid bodies.
    pub fn new(
        skeleton: &Skeleton,
        pose: &Pose,
        model: Mat4,
        bodies: &[BodyJoint],
    ) -> Result<Self, RagdollError> {
        if pose.len() != skeleton.len() {
            return Err(RagdollError::PoseSize);
        }
        if bodies.is_empty() {
            return Err(RagdollError::Empty);
        }
        affine_inverse(model).ok_or(RagdollError::InvalidModel)?;
        for (joint, local) in pose.locals().iter().enumerate() {
            if !local.rotation.is_finite()
                || !local.rotation.is_normalized()
                || affine_inverse(local.to_mat4()).is_none()
            {
                return Err(RagdollError::InvalidJoint { joint });
            }
        }
        let mut palette = Palette::new(skeleton);
        palette.compute(skeleton, pose);
        let mut body_for_joint = vec![None; skeleton.len()];
        let mut offsets = Vec::with_capacity(bodies.len());
        for (body, binding) in bodies.iter().enumerate() {
            let joint = binding.joint;
            let slot = body_for_joint
                .get_mut(joint)
                .ok_or(RagdollError::JointOutOfRange { joint })?;
            if slot.replace(body).is_some() {
                return Err(RagdollError::DuplicateJoint { joint });
            }
            let inverse = rigid_inverse(binding.world).ok_or(RagdollError::InvalidBody { body })?;
            let offset = inverse * model * palette.globals()[joint];
            if affine_inverse(offset).is_none() {
                return Err(RagdollError::InvalidJoint { joint });
            }
            offsets.push(offset);
        }
        Ok(Self {
            skeleton: skeleton.clone(),
            handoff: pose.clone(),
            offsets,
            body_for_joint,
            scratch: pose.clone(),
            globals: vec![Mat4::IDENTITY; skeleton.len()],
        })
    }

    /// Reconstructs local transforms from current rigid world transforms.
    ///
    /// No allocation occurs here. On error, `pose` is unchanged. A physical
    /// pose requiring local shear is rejected instead of silently deforming
    /// the skin. Compute the ordinary [`Palette`] from the resulting pose.
    pub fn pose_into(
        &mut self,
        model: Mat4,
        bodies: &[Mat4],
        pose: &mut Pose,
    ) -> Result<(), RagdollError> {
        if pose.len() != self.skeleton.len() {
            return Err(RagdollError::PoseSize);
        }
        if bodies.len() != self.offsets.len() {
            return Err(RagdollError::BodyCount);
        }
        affine_inverse(model).ok_or(RagdollError::InvalidModel)?;
        for (body, &world) in bodies.iter().enumerate() {
            rigid_inverse(world).ok_or(RagdollError::InvalidBody { body })?;
        }
        for (joint, definition) in self.skeleton.joints().iter().enumerate() {
            let parent = definition.parent.map_or(model, |index| self.globals[index]);
            let local = if let Some(body) = self.body_for_joint[joint] {
                let inverse = affine_inverse(parent).ok_or(RagdollError::InvalidJoint { joint })?;
                let matrix = inverse * bodies[body] * self.offsets[body];
                trs(matrix).ok_or(RagdollError::InvalidJoint { joint })?
            } else {
                self.handoff.locals()[joint]
            };
            self.scratch.locals_mut()[joint] = local;
            let world = parent * local.to_mat4();
            if affine_inverse(world).is_none() {
                return Err(RagdollError::InvalidJoint { joint });
            }
            self.globals[joint] = world;
        }
        pose.locals_mut().copy_from_slice(self.scratch.locals());
        Ok(())
    }
}

fn affine_inverse(matrix: Mat4) -> Option<Mat4> {
    if !matrix.is_finite() || !matrix.row(3).abs_diff_eq(Vec4::W, f32::EPSILON * 8.0) {
        return None;
    }
    let mut inverse = matrix.try_inverse()?;
    inverse.x_axis.w = 0.0;
    inverse.y_axis.w = 0.0;
    inverse.z_axis.w = 0.0;
    inverse.w_axis.w = 1.0;
    Some(inverse)
}

fn rigid_inverse(matrix: Mat4) -> Option<Mat4> {
    let inverse = affine_inverse(matrix)?;
    let local = trs(matrix)?;
    if !local.scale.abs_diff_eq(Vec3::ONE, TRANSFORM_TOLERANCE) {
        return None;
    }
    Some(inverse)
}

fn trs(matrix: Mat4) -> Option<Trs> {
    affine_inverse(matrix)?;
    let mut local = Trs::from_mat4(matrix);
    if !local.rotation.is_finite() || local.rotation.length_squared() == 0.0 {
        return None;
    }
    local.rotation = local.rotation.normalize();
    let reconstructed = local.to_mat4();
    for (actual, expected) in reconstructed
        .to_cols_array_2d()
        .iter()
        .zip(matrix.to_cols_array_2d())
    {
        let magnitude = expected
            .iter()
            .fold(0.0_f32, |max, value| max.max(value.abs()));
        if actual
            .iter()
            .zip(expected)
            .any(|(a, b)| !a.is_finite() || (a - b).abs() > TRANSFORM_TOLERANCE * magnitude)
        {
            return None;
        }
    }
    Some(local)
}
