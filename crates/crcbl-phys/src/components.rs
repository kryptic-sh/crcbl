//! ECS-ready component types for physics simulation.
//!
//! These are plain data structs designed to be stored in
//! [`crcbl_ecs::System<T>`] arrays. They carry no storage or scheduling logic —
//! that belongs to the physics system (see [`crate::system`]).

use glam::{DMat3, DQuat, DVec3};

use crate::compound_shape::CompoundShape;

// ---------------------------------------------------------------------------
// RigidBody
// ---------------------------------------------------------------------------

/// Dynamics data for a physics entity.
///
/// Mass and velocity are in SI units (kg, m/s); angular velocity is in rad/s
/// and inertia in kg·m². The force and torque accumulators are cleared every
/// substep after integration.
///
/// # Rotation
///
/// A body rotates at [`angular_velocity`](Self::angular_velocity) whatever its
/// inertia, so a kinematic body spun by a game turns just as it moves. What the
/// inertia decides is how the spin *changes*: a torque turns into angular
/// acceleration through [`inverse_local_inertia`](Self::inverse_local_inertia),
/// and a body whose inertia is not isotropic precesses and tumbles under the
/// gyroscopic term — see [`crate::integrator::SemiImplicitEuler`].
///
/// A body starts with **no rotational inertia at all**: both tensors are zero,
/// which this crate reads as "torque does nothing and the spin never changes".
/// That keeps every body built before rotation existed exactly as it was, and a
/// body that should tumble says so with [`with_inertia`](Self::with_inertia) —
/// [`crate::mass::MassProperties`] computes the tensor from collider shapes.
///
/// The inertia is about the body's origin, which is taken to be its centre of
/// mass: a centre of mass away from the origin is not modelled yet, so a
/// compound body places its parts about the centre
/// [`MassProperties::combine`](crate::mass::MassProperties::combine) reports.
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
    /// Angular velocity in rad/s (world-space): the axis is the direction and
    /// the rate is the length.
    pub angular_velocity: DVec3,
    /// Net torque accumulated this substep, in newton-metres (world-space).
    /// Cleared after integration.
    pub torque_accum: DVec3,
    /// The inertia tensor about the centre of mass, in the body's own frame,
    /// in kg·m². Zero for a body with no rotational inertia; see the type docs.
    pub local_inertia: DMat3,
    /// The inverse of [`local_inertia`](Self::local_inertia), or zero where
    /// that is zero.
    pub inverse_local_inertia: DMat3,
}

impl RigidBody {
    /// Create a dynamic rigid body with no rotational inertia; see
    /// [`with_inertia`](Self::with_inertia).
    ///
    /// # Panics
    ///
    /// Panics if `mass <= 0`.
    #[inline]
    #[must_use]
    pub fn new_dynamic(mass: f64) -> Self {
        assert!(mass > 0.0, "dynamic mass must be positive");
        Self {
            mass,
            inverse_mass: 1.0 / mass,
            ..Self::new_kinematic()
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
            angular_velocity: DVec3::ZERO,
            torque_accum: DVec3::ZERO,
            local_inertia: DMat3::ZERO,
            inverse_local_inertia: DMat3::ZERO,
        }
    }

    /// This body with `local_inertia` as its inertia tensor, about its centre
    /// of mass in its own frame.
    ///
    /// # Panics
    ///
    /// Panics if the body is kinematic — its inertia is infinite, and a finite
    /// tensor would make torque turn it — or if the tensor is not symmetric
    /// positive definite, which no physical body's is.
    #[must_use]
    pub fn with_inertia(self, local_inertia: DMat3) -> Self {
        assert!(self.is_dynamic(), "a kinematic body has no finite inertia");
        let m = local_inertia;
        assert!(
            m.x_axis.y == m.y_axis.x && m.x_axis.z == m.z_axis.x && m.y_axis.z == m.z_axis.y,
            "an inertia tensor is symmetric: {m:?}"
        );
        // Sylvester's criterion: every leading principal minor is positive.
        let minor2 = m.x_axis.x * m.y_axis.y - m.y_axis.x * m.x_axis.y;
        assert!(
            m.x_axis.x > 0.0 && minor2 > 0.0 && m.determinant() > 0.0,
            "an inertia tensor is positive definite: {m:?}"
        );
        Self {
            local_inertia,
            inverse_local_inertia: local_inertia.inverse(),
            ..self
        }
    }

    /// Whether this body responds to forces (inverse mass > 0).
    #[inline]
    #[must_use]
    pub fn is_dynamic(&self) -> bool {
        self.inverse_mass > 0.0
    }

    /// Whether this body responds to torque: it is dynamic and has been given
    /// an inertia tensor.
    #[inline]
    #[must_use]
    pub fn has_rotational_inertia(&self) -> bool {
        self.is_dynamic() && self.inverse_local_inertia != DMat3::ZERO
    }

    /// Clear the force and torque accumulators (called after integration).
    #[inline]
    pub fn clear_forces(&mut self) {
        self.force_accum = DVec3::ZERO;
        self.torque_accum = DVec3::ZERO;
    }

    /// Apply a world-space force for this substep.
    #[inline]
    pub fn apply_force(&mut self, force: DVec3) {
        self.force_accum += force;
    }

    /// Apply a world-space torque for this substep. A body with no rotational
    /// inertia ignores it when it integrates.
    #[inline]
    pub fn apply_torque(&mut self, torque: DVec3) {
        self.torque_accum += torque;
    }

    /// Apply an impulse (instantaneous velocity change).
    #[inline]
    pub fn apply_impulse(&mut self, impulse: DVec3) {
        self.velocity += impulse * self.inverse_mass;
    }

    /// The angular momentum about the centre of mass, world-space, for a body
    /// oriented at `rotation`: `R · I · Rᵀ · ω`.
    #[must_use]
    pub fn angular_momentum(&self, rotation: DQuat) -> DVec3 {
        let local = rotation.inverse() * self.angular_velocity;
        rotation * (self.local_inertia * local)
    }

    /// Kinetic energy in joules: `½ m v²` plus `½ ωᵀ I ω`.
    ///
    /// Zero for a kinematic body, whose mass is infinite — what it carries is
    /// not energy the simulation can exchange. The rotational half is zero for
    /// a body with no rotational inertia, for the same reason.
    #[must_use]
    pub fn kinetic_energy(&self, rotation: DQuat) -> f64 {
        if !self.is_dynamic() {
            return 0.0;
        }
        let local = rotation.inverse() * self.angular_velocity;
        0.5 * self.mass * self.velocity.length_squared()
            + 0.5 * local.dot(self.local_inertia * local)
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
    pub const ENCODED_LEN: usize = 7 * 8;

    /// How far a decoded quaternion's length² may sit from 1 before
    /// [`Self::decode`] rejects it.
    ///
    /// Wide enough to absorb the round-trip error of an `f64` quaternion built
    /// from Euler angles; far too tight to admit a degenerate or zero one.
    const ROTATION_TOLERANCE: f64 = 1e-6;

    /// Byte length of the replication encoding written by [`Self::encode`].
    ///
    /// Always [`Self::ENCODED_LEN`] — the encoding is fixed-width, so this
    /// does not depend on `self`. It takes a receiver only so call sites can
    /// write `transform.encoded_len()` next to `transform.encode(..)`.
    #[inline]
    #[must_use]
    pub const fn encoded_len(&self) -> usize {
        Self::ENCODED_LEN
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
    /// Returns `None` if `data` is not exactly [`Self::ENCODED_LEN`] bytes, if
    /// any component is not finite, or if the rotation is not (near enough) a
    /// unit quaternion.
    ///
    /// This runs on bytes a peer sent us. A NaN or infinite position poisons
    /// every AABB it reaches and, through them, the whole BVH; a degenerate
    /// quaternion turns [`Self::forward`] / [`Self::right`] / [`Self::up`]
    /// into garbage. Neither is representable by [`Self::encode`], so
    /// rejecting them here costs a well-behaved peer nothing.
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != Self::ENCODED_LEN {
            return None;
        }
        let mut values = [0.0f64; 7];
        for (i, value) in values.iter_mut().enumerate() {
            let bytes: [u8; 8] = data[i * 8..(i + 1) * 8].try_into().ok()?;
            *value = f64::from_le_bytes(bytes);
            if !value.is_finite() {
                return None;
            }
        }

        let rotation = DQuat::from_xyzw(values[3], values[4], values[5], values[6]);
        if (rotation.length_squared() - 1.0).abs() > Self::ROTATION_TOLERANCE {
            return None;
        }

        Some(Self {
            position: DVec3::new(values[0], values[1], values[2]),
            rotation,
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
    /// Several boxes fixed in the body's frame, turning with it: see
    /// [`CompoundShape`].
    ///
    /// Unlike the shapes above, the offset and every part are turned by the
    /// body's rotation wherever they are placed. In a system with contacts
    /// each part collides on its own; the query world
    /// ([`crate::PhysicsSystem::world`]) holds one box around all of them, so
    /// a ray or an overlap there answers for the bounds, and
    /// [`crate::AabbCompound`] is the per-part query.
    Compound {
        /// Offset of the shape's frame from the entity's
        /// [`Transform::position`], in the body's frame: minus the centre of
        /// mass for a body [`CompoundShape::dynamic_body`] builds.
        offset: DVec3,
        /// The parts.
        shape: CompoundShape,
        /// Whether this collider is a trigger.
        is_trigger: bool,
    },
}

impl ColliderComponent {
    /// How many shapes it is to the contact pipeline: its parts for a
    /// compound, one for anything else.
    #[must_use]
    pub fn part_count(&self) -> usize {
        match self {
            Self::Compound { shape, .. } => shape.parts().len(),
            Self::Sphere { .. } | Self::Box { .. } | Self::Capsule { .. } => 1,
        }
    }
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
    #[should_panic(expected = "dynamic mass must be positive")]
    fn new_dynamic_rejects_zero_mass() {
        let _ = RigidBody::new_dynamic(0.0);
    }

    #[test]
    #[should_panic(expected = "a kinematic body has no finite inertia")]
    fn a_kinematic_body_refuses_an_inertia() {
        let _ = RigidBody::new_kinematic().with_inertia(DMat3::IDENTITY);
    }

    #[test]
    #[should_panic(expected = "positive definite")]
    fn an_inertia_that_is_not_positive_definite_is_refused() {
        let _ = RigidBody::new_dynamic(1.0)
            .with_inertia(DMat3::from_diagonal(DVec3::new(1.0, -1.0, 1.0)));
    }

    #[test]
    #[should_panic(expected = "symmetric")]
    fn an_asymmetric_inertia_is_refused() {
        let mut inertia = DMat3::IDENTITY;
        inertia.x_axis.y = 0.1;
        let _ = RigidBody::new_dynamic(1.0).with_inertia(inertia);
    }

    /// A body spinning at `ω` about a principal axis of moment `I`: momentum
    /// `I ω` along the axis, whichever way the body is turned, and energy
    /// `½ m v² + ½ I ω²`.
    #[test]
    fn momentum_and_energy_read_the_inertia_in_the_bodys_own_frame() {
        let mut body = RigidBody::new_dynamic(2.0)
            .with_inertia(DMat3::from_diagonal(DVec3::new(1.0, 2.0, 3.0)));
        body.velocity = DVec3::new(3.0, 0.0, 0.0);
        // Turned a quarter about Z, the body's X lies along world Y; spinning
        // about world Y is spinning about the body's X, moment 1.
        let quarter = DQuat::from_xyzw(
            0.0,
            0.0,
            core::f64::consts::FRAC_1_SQRT_2,
            core::f64::consts::FRAC_1_SQRT_2,
        );
        body.angular_velocity = DVec3::new(0.0, 4.0, 0.0);
        let momentum = body.angular_momentum(quarter);
        assert!(
            (momentum - DVec3::new(0.0, 4.0, 0.0)).length() < 1e-12,
            "{momentum:?}"
        );
        let energy = body.kinetic_energy(quarter);
        assert!((energy - (9.0 + 8.0)).abs() < 1e-12, "{energy}");
        assert_eq!(RigidBody::new_kinematic().kinetic_energy(quarter), 0.0);
    }

    #[test]
    fn applied_forces_sum_into_the_accumulator_and_clearing_zeroes_it() {
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
    fn decode_rejects_non_finite_and_degenerate_payloads() {
        let good = Transform::new(
            DVec3::new(1.0, 2.0, 3.0),
            crate::rotation_from_scaled_axis(DVec3::Y * 0.75),
        );
        let mut buf = Vec::new();
        good.encode(&mut buf);
        assert!(Transform::decode(&buf).is_some());

        // Field 0 is position.x, field 3 the first quaternion component.
        let poison = |field: usize, value: f64| {
            let mut data = buf.clone();
            data[field * 8..(field + 1) * 8].copy_from_slice(&value.to_le_bytes());
            Transform::decode(&data)
        };

        assert!(poison(0, f64::NAN).is_none(), "NaN position");
        assert!(poison(1, f64::INFINITY).is_none(), "infinite position");
        assert!(poison(2, f64::NEG_INFINITY).is_none(), "-inf position");
        assert!(poison(6, f64::NAN).is_none(), "NaN quaternion component");

        // A zero quaternion has length 0: `forward()` on it is meaningless.
        let mut zero_rot = buf.clone();
        for field in 3..7 {
            zero_rot[field * 8..(field + 1) * 8].copy_from_slice(&0.0f64.to_le_bytes());
        }
        assert!(Transform::decode(&zero_rot).is_none(), "zero quaternion");

        // So is a scaled one.
        let scaled = Transform::new(good.position, DQuat::from_xyzw(0.0, 0.0, 0.0, 4.0));
        let mut data = Vec::new();
        scaled.encode(&mut data);
        assert!(Transform::decode(&data).is_none(), "non-unit quaternion");
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
