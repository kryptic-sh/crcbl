//! Instantaneous impulses applied at world-space points.

use std::fmt;

use crcbl_ecs::Entity;
use glam::{DMat3, DQuat, DVec3};

use crate::{PhysicsSystem, RigidBody, Transform};

/// Why a point impulse was refused without changing the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpulseError {
    /// The entity has no rigid body or transform.
    UnknownBody,
    /// The impulse contains a NaN or infinity.
    NonFiniteImpulse,
    /// The application point contains a NaN or infinity.
    NonFinitePoint,
    /// The body's position is non-finite or its rotation is not a unit quaternion.
    InvalidTransform,
    /// Applying the impulse would produce a non-finite velocity.
    NonFiniteVelocity,
}

impl fmt::Display for ImpulseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownBody => "point impulse target has no body or transform",
            Self::NonFiniteImpulse => "point impulse is not finite",
            Self::NonFinitePoint => "impulse application point is not finite",
            Self::InvalidTransform => "point impulse target transform is invalid",
            Self::NonFiniteVelocity => "point impulse would produce non-finite velocity",
        })
    }
}

impl std::error::Error for ImpulseError {}

impl RigidBody {
    pub(crate) fn inverse_world_inertia(&self, rotation: DQuat) -> DMat3 {
        if self.has_rotational_inertia() {
            let turn = DMat3::from_quat(rotation);
            turn * self.inverse_local_inertia * turn.transpose()
        } else {
            DMat3::ZERO
        }
    }

    /// Applies a world-space impulse at `point`, in N·s.
    ///
    /// Linear momentum changes as in [`Self::apply_impulse`]; angular momentum
    /// changes by the lever arm crossed with the impulse, using the body's
    /// oriented inertia. Kinematic bodies retain their velocities. A dynamic
    /// body without rotational inertia receives only the linear change.
    ///
    /// Returns [`ImpulseError`] for invalid inputs or non-finite results,
    /// leaving all body fields unchanged. Force accumulators are not involved.
    pub fn apply_impulse_at(
        &mut self,
        impulse: DVec3,
        point: DVec3,
        transform: &Transform,
    ) -> Result<(), ImpulseError> {
        if !impulse.is_finite() {
            return Err(ImpulseError::NonFiniteImpulse);
        }
        if !point.is_finite() {
            return Err(ImpulseError::NonFinitePoint);
        }
        if !transform.position.is_finite()
            || !transform.rotation.is_finite()
            || !transform.rotation.is_normalized()
        {
            return Err(ImpulseError::InvalidTransform);
        }
        let mut candidate = *self;
        candidate.apply_impulse(impulse);
        if self.has_rotational_inertia() {
            let angular_impulse = (point - transform.position).cross(impulse);
            candidate.angular_velocity +=
                self.inverse_world_inertia(transform.rotation) * angular_impulse;
        }
        if !candidate.velocity.is_finite() || !candidate.angular_velocity.is_finite() {
            return Err(ImpulseError::NonFiniteVelocity);
        }
        self.velocity = candidate.velocity;
        self.angular_velocity = candidate.angular_velocity;
        Ok(())
    }
}

impl PhysicsSystem {
    /// Applies an impulse immediately and wakes the target body's island.
    ///
    /// `impulse` and `point` are world-space, on
    /// [`RigidBody::apply_impulse_at`]'s terms. Invalid input and missing bodies
    /// return [`ImpulseError`] without changing velocities or waking an island.
    /// A recorded [`crate::KineticContact`] has already affected its struck
    /// body; transferring it to a newly created ragdoll body is the caller's
    /// explicit action, not an automatic replay onto the original target.
    pub fn apply_impulse_at(
        &mut self,
        entity: Entity,
        impulse: DVec3,
        point: DVec3,
    ) -> Result<(), ImpulseError> {
        let mut candidate = *self.body(entity).ok_or(ImpulseError::UnknownBody)?;
        let transform = self.transform(entity).ok_or(ImpulseError::UnknownBody)?;
        candidate.apply_impulse_at(impulse, point, transform)?;
        let body = self
            .body_mut(entity)
            .expect("the validated body still exists");
        body.velocity = candidate.velocity;
        body.angular_velocity = candidate.angular_velocity;
        Ok(())
    }
}
