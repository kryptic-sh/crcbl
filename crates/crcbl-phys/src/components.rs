//! ECS-ready component types for physics simulation.
//!
//! These are plain data structs designed to be stored in
//! [`crcbl_ecs::System<T>`] arrays. They carry no storage or scheduling logic —
//! that belongs to the physics system (see [`crate::system`]).

use glam::DQuat;
use glam::DVec3;

// ---------------------------------------------------------------------------
// RigidBody
// ---------------------------------------------------------------------------

/// Dynamics data for a physics entity.
///
/// Mass and velocity are in SI units (kg, m/s). The accumulated force vector is
/// cleared every substep after integration.
///
/// An entity with a [`RigidBody`] but no [`Transform`] component is a bug
/// (detected at system registration time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigidBody {
    /// Mass in kilograms. Must be > 0; `inverse_mass` is `1.0 / mass`.
    pub mass: f64,
    /// Pre-computed inverse mass (`0.0` for kinematic / infinite-mass bodies).
    pub inverse_mass: f64,
    /// Linear velocity in m/s (world-space).
    pub velocity: DVec3,
    /// Net force accumulated this substep, in newtons. Cleared after
    /// integration.
    pub force_accum: DVec3,
}

impl RigidBody {
    /// Create a dynamic rigid body.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `mass <= 0`.
    #[inline]
    #[must_use]
    pub fn new_dynamic(mass: f64) -> Self {
        debug_assert!(mass > 0.0, "dynamic mass must be positive");
        Self {
            mass,
            inverse_mass: 1.0 / mass,
            velocity: DVec3::ZERO,
            force_accum: DVec3::ZERO,
        }
    }

    /// Create a kinematic body (infinite mass, not affected by forces).
    #[inline]
    #[must_use]
    pub fn new_kinematic() -> Self {
        Self {
            mass: f64::INFINITY,
            inverse_mass: 0.0,
            velocity: DVec3::ZERO,
            force_accum: DVec3::ZERO,
        }
    }

    /// Whether this body responds to forces (inverse mass > 0).
    #[inline]
    #[must_use]
    pub fn is_dynamic(&self) -> bool {
        self.inverse_mass > 0.0
    }

    /// Clear the force accumulator (called after integration).
    #[inline]
    pub fn clear_forces(&mut self) {
        self.force_accum = DVec3::ZERO;
    }

    /// Apply a world-space force for this substep.
    #[inline]
    pub fn apply_force(&mut self, force: DVec3) {
        self.force_accum += force;
    }

    /// Apply an impulse (instantaneous velocity change).
    #[inline]
    pub fn apply_impulse(&mut self, impulse: DVec3) {
        self.velocity += impulse * self.inverse_mass;
    }
}

impl Default for RigidBody {
    fn default() -> Self {
        Self::new_kinematic()
    }
}

// ---------------------------------------------------------------------------
// Transform
// ---------------------------------------------------------------------------

/// World-space transform for a physics entity.
///
/// This is the authoritative position used by the server simulation.
/// The renderer receives a camera-relative transform derived from this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// World-space position in metres.
    pub position: DVec3,
    /// World-space orientation (unit quaternion).
    pub rotation: DQuat,
}

impl Transform {
    /// Identity transform at the origin.
    pub const IDENTITY: Self = Self {
        position: DVec3::ZERO,
        rotation: DQuat::IDENTITY,
    };

    /// Create a transform at `position` with the given `rotation`.
    #[inline]
    #[must_use]
    pub fn new(position: DVec3, rotation: DQuat) -> Self {
        Self { position, rotation }
    }

    /// Create a transform at `position` with identity rotation.
    #[inline]
    #[must_use]
    pub fn from_position(position: DVec3) -> Self {
        Self {
            position,
            rotation: DQuat::IDENTITY,
        }
    }

    /// The forward direction (local -Z in right-handed coordinates, or
    /// local +Z depending on convention; matches glam's default).
    #[inline]
    #[must_use]
    pub fn forward(&self) -> DVec3 {
        self.rotation * DVec3::NEG_Z
    }

    /// The right direction (local +X).
    #[inline]
    #[must_use]
    pub fn right(&self) -> DVec3 {
        self.rotation * DVec3::X
    }

    /// The up direction (local +Y).
    #[inline]
    #[must_use]
    pub fn up(&self) -> DVec3 {
        self.rotation * DVec3::Y
    }

    /// Byte length of the replication encoding written by [`Self::encode`].
    ///
    /// The encoding is position (3 × f64 LE) followed by rotation
    /// quaternion (4 × f64 LE, x/y/z/w) — 56 bytes.
    #[inline]
    #[must_use]
    pub const fn encoded_len(&self) -> usize {
        7 * 8
    }

    /// Append the replication encoding to `out` (little-endian, platform-
    /// and run-deterministic).
    pub fn encode(&self, out: &mut Vec<u8>) {
        for value in [
            self.position.x,
            self.position.y,
            self.position.z,
            self.rotation.x,
            self.rotation.y,
            self.rotation.z,
            self.rotation.w,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }

    /// Decode a transform from the encoding written by [`Self::encode`].
    ///
    /// Returns `None` if `data` is not exactly [`Self::encoded_len`] bytes.
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != Transform::IDENTITY.encoded_len() {
            return None;
        }
        let mut values = [0.0f64; 7];
        for (i, value) in values.iter_mut().enumerate() {
            let bytes: [u8; 8] = data[i * 8..(i + 1) * 8].try_into().ok()?;
            *value = f64::from_le_bytes(bytes);
        }
        Some(Self {
            position: DVec3::new(values[0], values[1], values[2]),
            rotation: DQuat::from_xyzw(values[3], values[4], values[5], values[6]),
        })
    }

    /// Linearly interpolate between `self` and `other` at `alpha ∈ [0, 1]`.
    ///
    /// Position is lerped; rotation is glam's nlerp (shortest-path
    /// corrected, renormalised). This is presentation-only smoothing for
    /// the client render path — simulation state is never interpolated.
    #[must_use]
    pub fn lerp(&self, other: &Self, alpha: f64) -> Self {
        Self {
            position: self.position.lerp(other.position, alpha),
            rotation: self.rotation.lerp(other.rotation, alpha),
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

// ---------------------------------------------------------------------------
// ColliderComponent
// ---------------------------------------------------------------------------

/// A shape attached to an entity for physics queries.
///
/// This bundles a collider shape with a trigger flag. The shape is stored
/// by-value; there is no indirection to the `crate::PhysicsWorld` — the
/// physics system is responsible for syncing colliders into the world.
#[derive(Debug, Clone, PartialEq)]
pub enum ColliderComponent {
    /// A sphere collider.
    Sphere {
        /// Offset from the entity's [`Transform::position`] in local space.
        offset: DVec3,
        /// Radius in metres.
        radius: f64,
        /// Whether this collider is a trigger (non-solid, overlap-only).
        is_trigger: bool,
    },
    /// An axis-aligned box collider.
    Box {
        /// Offset from the entity's [`Transform::position`] in local space.
        offset: DVec3,
        /// Half-extents on each axis, in metres.
        half_extents: DVec3,
        /// Whether this collider is a trigger.
        is_trigger: bool,
    },
    /// A Y-aligned capsule collider.
    Capsule {
        /// Offset from the entity's [`Transform::position`] in local space.
        offset: DVec3,
        /// Radius in metres.
        radius: f64,
        /// Half the length of the cylindrical section.
        half_height: f64,
        /// Whether this collider is a trigger.
        is_trigger: bool,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_body_has_inverse_mass() {
        let body = RigidBody::new_dynamic(2.0);
        assert!((body.inverse_mass - 0.5).abs() < 1e-12);
        assert!(body.is_dynamic());
    }

    #[test]
    fn kinematic_body_is_not_dynamic() {
        let body = RigidBody::new_kinematic();
        assert_eq!(body.inverse_mass, 0.0);
        assert!(!body.is_dynamic());
    }

    #[test]
    fn apply_force_accumulates() {
        let mut body = RigidBody::new_dynamic(1.0);
        body.apply_force(DVec3::new(1.0, 0.0, 0.0));
        body.apply_force(DVec3::new(0.0, 2.0, 0.0));
        assert_eq!(body.force_accum, DVec3::new(1.0, 2.0, 0.0));
        body.clear_forces();
        assert_eq!(body.force_accum, DVec3::ZERO);
    }

    #[test]
    fn apply_impulse_changes_velocity() {
        let mut body = RigidBody::new_dynamic(2.0);
        body.apply_impulse(DVec3::new(4.0, 0.0, 0.0));
        assert_eq!(body.velocity, DVec3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn kinematic_body_ignores_impulse() {
        let mut body = RigidBody::new_kinematic();
        body.apply_impulse(DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(body.velocity, DVec3::ZERO);
    }

    #[test]
    fn transform_directions_are_orthogonal() {
        let t = Transform::IDENTITY;
        assert_eq!(t.forward(), DVec3::NEG_Z);
        assert_eq!(t.right(), DVec3::X);
        assert_eq!(t.up(), DVec3::Y);
    }

    #[test]
    fn collider_component_is_clone() {
        let c = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        let c2 = c.clone();
        assert_eq!(c, c2);
    }
}
