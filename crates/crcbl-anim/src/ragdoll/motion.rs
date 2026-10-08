//! Velocities at animation-to-physics handoff.

use glam::{DQuat, DVec3, Mat4};

use super::{RagdollBinding, RagdollError, affine_inverse, rigid_inverse, validate_pose};
use crate::{Palette, Pose};

/// A body's initial world pose and velocity, in physics units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyMotion {
    /// Body centre in world space.
    pub position: DVec3,
    /// Body orientation in world space.
    pub rotation: DQuat,
    /// Body-centre displacement per second, including character movement.
    pub linear_velocity: DVec3,
    /// Shortest rotation between samples, in world radians per second.
    pub angular_velocity: DVec3,
}

impl RagdollBinding {
    /// Infers initial body motion from a pose sampled before the handoff.
    ///
    /// `previous_model` places that earlier pose in world space. `seconds`
    /// is the interval from that sample to the pose passed to [`Self::new`].
    /// Results use the original binding order. Sampling must resolve turns
    /// smaller than half a revolution; complete turns between samples cannot
    /// be recovered from orientations alone.
    ///
    /// This allocates at handoff, not during simulation. Animation inputs are
    /// still single precision; double precision outputs do not make animation
    /// evaluation deterministic. Invalid samples return [`RagdollError`].
    pub fn motion_from_previous_pose(
        &self,
        previous: &Pose,
        previous_model: Mat4,
        seconds: f64,
    ) -> Result<Vec<BodyMotion>, RagdollError> {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(RagdollError::InvalidInterval);
        }
        validate_pose(&self.skeleton, previous)?;
        affine_inverse(previous_model).ok_or(RagdollError::InvalidModel)?;
        let mut previous_palette = Palette::new(&self.skeleton);
        previous_palette.compute(&self.skeleton, previous);
        let mut current_palette = Palette::new(&self.skeleton);
        current_palette.compute(&self.skeleton, &self.handoff);
        let mut output = vec![
            BodyMotion {
                position: DVec3::ZERO,
                rotation: DQuat::IDENTITY,
                linear_velocity: DVec3::ZERO,
                angular_velocity: DVec3::ZERO,
            };
            self.offsets.len()
        ];
        for (joint, body) in self.body_for_joint.iter().enumerate() {
            let Some(body) = *body else { continue };
            let offset_inverse =
                affine_inverse(self.offsets[body]).ok_or(RagdollError::InvalidBody { body })?;
            let before = previous_model * previous_palette.globals()[joint] * offset_inverse;
            let current = self.handoff_model * current_palette.globals()[joint] * offset_inverse;
            rigid_inverse(before).ok_or(RagdollError::InvalidBody { body })?;
            rigid_inverse(current).ok_or(RagdollError::InvalidBody { body })?;
            let (_, before_rotation, before_position) = before.to_scale_rotation_translation();
            let (_, current_rotation, current_position) = current.to_scale_rotation_translation();
            let rotation = current_rotation.as_dquat().normalize();
            let mut turn =
                (rotation * before_rotation.as_dquat().normalize().conjugate()).normalize();
            if turn.w < 0.0 {
                turn = -turn;
            }
            let linear_velocity =
                (current_position.as_dvec3() - before_position.as_dvec3()) / seconds;
            let angular_velocity = turn.to_scaled_axis() / seconds;
            if !linear_velocity.is_finite() || !angular_velocity.is_finite() {
                return Err(RagdollError::InvalidMotion { body });
            }
            output[body] = BodyMotion {
                position: current_position.as_dvec3(),
                rotation,
                linear_velocity,
                angular_velocity,
            };
        }
        Ok(output)
    }
}
