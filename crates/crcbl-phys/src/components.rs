//! ECS-ready component types for physics simulation.
//!
//! These are plain data structs designed to be stored in
//! [`crcbl_ecs::System<T>`] arrays. They carry no storage or scheduling logic —
//! that belongs to the physics system (see [`crate::system`]).

use crcbl_ecs::quantize::{self, Codec, Field, Fixed, Quantized, SmallestThree};
use glam::{DMat3, DQuat, DVec3};

use crate::compound_shape::CompoundShape;
use crate::mesh::TriangleMesh;

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
    /// Whether this body is a **bullet**: in a system with contacts, a
    /// dynamic bullet is swept every tick it moves, however slowly, and
    /// against every other body as well as the static ones. See
    /// [`crate::contact`]'s continuous collision. Off by default, and ignored
    /// for a kinematic body, which nothing sweeps.
    pub bullet: bool,
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
            bullet: false,
        }
    }

    /// This body with its [`bullet`](Self::bullet) flag set to `bullet`.
    #[must_use]
    pub const fn with_bullet(self, bullet: bool) -> Self {
        Self { bullet, ..self }
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

    /// Half the width of the range a replicated position axis is quantized
    /// over, in metres, centred on the sector's origin.
    ///
    /// Wire positions are sector-local, and today every snapshot is
    /// `crcbl_net::SectorId::ZERO` with physics never rebasing, so sector-local
    /// is world space. The range is bounded by the scenes that replicate, not
    /// by `crcbl_core::SECTOR_SIZE`: a whole sector at
    /// [`Self::WIRE_POSITION_BITS`] would be a sixteenth of a metre per step,
    /// too coarse to watch a crate settle. A body outside it replicates
    /// exactly (see [`Self::encode_wire`]).
    pub const WIRE_POSITION_EXTENT: f64 = 4096.0;

    /// Bits per replicated position axis.
    pub const WIRE_POSITION_BITS: u32 = 24;

    /// A replicated position axis: [`Self::WIRE_POSITION_BITS`] of fixed point
    /// over `±`[`Self::WIRE_POSITION_EXTENT`]. The width is a power of two, so
    /// the quantum is too (asserted in the tests) and a position on that grid
    /// replicates exactly.
    pub const WIRE_POSITION: Fixed = Fixed::new(
        -Self::WIRE_POSITION_EXTENT,
        Self::WIRE_POSITION_EXTENT,
        Self::WIRE_POSITION_BITS,
    );

    /// Bits per sent component of a replicated rotation. With the index bits
    /// and the three position axes it fills [`Self::QUANTIZED_LEN`] with no
    /// padding (asserted below the impl), so the precision costs nothing the
    /// payload's last byte was not already spending.
    pub const WIRE_ROTATION_BITS: u32 = 18;

    /// A replicated rotation: smallest-three at [`Self::WIRE_ROTATION_BITS`].
    pub const WIRE_ROTATION: SmallestThree = SmallestThree::new(Self::WIRE_ROTATION_BITS);

    /// The quantized replication schema: each position axis as
    /// [`Self::WIRE_POSITION`] fixed point, the rotation as
    /// [`Self::WIRE_ROTATION`].
    pub const WIRE_SCHEMA: &'static [Field] = &[
        Field {
            name: "position.x",
            codec: Codec::Fixed(Self::WIRE_POSITION),
        },
        Field {
            name: "position.y",
            codec: Codec::Fixed(Self::WIRE_POSITION),
        },
        Field {
            name: "position.z",
            codec: Codec::Fixed(Self::WIRE_POSITION),
        },
        Field {
            name: "rotation",
            codec: Codec::Rotation(Self::WIRE_ROTATION),
        },
    ];

    /// Byte length of the quantized form [`Self::encode_wire`] writes.
    pub const QUANTIZED_LEN: usize = quantize::encoded_len(Self::WIRE_SCHEMA);

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
        }
        Self::from_parts(&values)
    }

    /// A transform from position then rotation `x, y, z, w`, or `None` if any
    /// is not finite or the rotation is not (near enough) a unit quaternion.
    fn from_parts(values: &[f64; 7]) -> Option<Self> {
        if values.iter().any(|value| !value.is_finite()) {
            return None;
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

    /// Append the replicated form to `out`: [`Self::WIRE_SCHEMA`]'s
    /// [`Self::QUANTIZED_LEN`] bytes when the position is inside
    /// [`Self::WIRE_POSITION`], otherwise the exact [`Self::encode`] form.
    ///
    /// The fallback is what keeps an entity outside the declared range where
    /// it is: clamping it to the range's edge would move it on every client
    /// and say nothing. The two lengths differ, which is how
    /// [`Self::decode_wire`] tells them apart.
    pub fn encode_wire(&self, out: &mut Vec<u8>) {
        // A refusal writes nothing, so the exact form lands in its place.
        if quantize::encode(self, out).is_err() {
            self.encode(out);
        }
    }

    /// Decode either form [`Self::encode_wire`] writes, telling them apart by
    /// length.
    ///
    /// Returns `None` for any other length, and for what [`Self::decode`]
    /// refuses. A quantized payload always holds a unit quaternion and a
    /// finite position; one with a padding bit or a code the encoder never
    /// writes is refused.
    #[must_use]
    pub fn decode_wire(data: &[u8]) -> Option<Self> {
        match data.len() {
            Self::QUANTIZED_LEN => quantize::decode(data).ok(),
            Self::ENCODED_LEN => Self::decode(data),
            _ => None,
        }
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

/// The quantized wire form [`Transform::encode_wire`] prefers.
impl Quantized for Transform {
    const SCHEMA: &'static [Field] = Self::WIRE_SCHEMA;
    type Values = [f64; 7];

    fn to_values(&self) -> [f64; 7] {
        [
            self.position.x,
            self.position.y,
            self.position.z,
            self.rotation.x,
            self.rotation.y,
            self.rotation.z,
            self.rotation.w,
        ]
    }

    fn from_values(values: &[f64; 7]) -> Option<Self> {
        Self::from_parts(values)
    }
}

// The quantized form must not be mistaken for the exact one, and the schema
// must consume exactly the seven values `to_values` supplies.
const _: () = assert!(Transform::QUANTIZED_LEN != Transform::ENCODED_LEN);
const _: () = assert!(quantize::value_count(Transform::WIRE_SCHEMA) == 7);
const _: () = assert!(
    3 * Transform::WIRE_POSITION_BITS + Transform::WIRE_ROTATION.encoded_bits()
        == 8 * Transform::QUANTIZED_LEN as u32
);

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
    /// A sphere collider, its offset turned with its body in the contact
    /// pipeline and the query world ([`crate::PhysicsSystem::world`]) alike.
    Sphere {
        /// Offset from the entity's [`Transform::position`] in the body's
        /// frame.
        offset: DVec3,
        /// Radius in metres.
        radius: f64,
        /// Whether this collider is a trigger (non-solid, overlap-only).
        is_trigger: bool,
    },
    /// A box collider, turned with its body: the offset and the faces are
    /// turned by the body's rotation, in the contact pipeline and the query
    /// world ([`crate::PhysicsSystem::world`]) alike.
    Box {
        /// Offset from the entity's [`Transform::position`] in the body's
        /// frame.
        offset: DVec3,
        /// Half-extents along the body's own axes, in metres.
        half_extents: DVec3,
        /// Whether this collider is a trigger.
        is_trigger: bool,
    },
    /// A capsule collider along the body's `Y`: the contact pipeline turns it
    /// with the body, while the query world turns its offset with the body
    /// but keeps the capsule itself upright along the world's `Y`, whatever
    /// the rotation.
    Capsule {
        /// Offset from the entity's [`Transform::position`] in the body's
        /// frame.
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
    /// The offset and every part are turned by the body's rotation wherever
    /// they are placed. In a system with contacts each part collides on its
    /// own; in the query world ([`crate::PhysicsSystem::world`]) the compound
    /// is one collider whose rays, sweeps and overlaps answer for the parts
    /// themselves, each hit naming its part in
    /// [`ShapeHit::part`](crate::ShapeHit::part)
    /// ([`crate::PhysicsWorld::add_compound`]).
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
    /// A static triangle mesh, its vertices in the body's frame, turning with
    /// it: see [`TriangleMesh`].
    ///
    /// **For static and kinematic bodies only**: a mesh has no volume to
    /// weigh, so [`crate::PhysicsSystem::set_collider`] and
    /// [`crate::PhysicsSystem::set_body`] refuse to put one on a dynamic body.
    /// In a system with contacts each triangle collides on its own, one-sided;
    /// the query world ([`crate::PhysicsSystem::world`]) holds the mesh
    /// itself, and its rays, sweeps and overlaps hit the triangles exactly.
    Mesh {
        /// The mesh.
        mesh: TriangleMesh,
        /// Whether this collider is a trigger.
        is_trigger: bool,
    },
}

impl ColliderComponent {
    /// How far its farthest point is from the body's origin, which the body
    /// turns about, in metres: what an angular speed is multiplied by to give
    /// the fastest a point of the body moves.
    ///
    /// Exact for a sphere, a box and a compound (their farthest corners), and
    /// for a capsule (the farther cap). A mesh answers the farthest vertex;
    /// it is never on a dynamic body, which is the only kind that sleeps.
    #[must_use]
    pub fn max_extent(&self) -> f64 {
        let farthest_corner = |centre: DVec3, rotation: DQuat, half: DVec3| {
            let mut farthest: f64 = 0.0;
            for x in [-half.x, half.x] {
                for y in [-half.y, half.y] {
                    for z in [-half.z, half.z] {
                        farthest = farthest.max((centre + rotation * DVec3::new(x, y, z)).length());
                    }
                }
            }
            farthest
        };
        match self {
            Self::Sphere { offset, radius, .. } => offset.length() + radius,
            Self::Box {
                offset,
                half_extents,
                ..
            } => farthest_corner(*offset, DQuat::IDENTITY, *half_extents),
            Self::Capsule {
                offset,
                radius,
                half_height,
                ..
            } => {
                let cap = DVec3::new(0.0, *half_height, 0.0);
                (*offset + cap).length().max((*offset - cap).length()) + radius
            }
            Self::Compound { offset, shape, .. } => shape
                .parts()
                .iter()
                .map(|part| {
                    farthest_corner(*offset + part.centre, part.rotation, part.half_extents)
                })
                .fold(0.0, f64::max),
            Self::Mesh { mesh, .. } => mesh
                .vertices()
                .iter()
                .map(|vertex| vertex.length())
                .fold(0.0, f64::max),
        }
    }

    /// How many shapes it is to the contact pipeline: its parts for a
    /// compound, its triangles for a mesh, one for anything else.
    #[must_use]
    pub fn part_count(&self) -> usize {
        match self {
            Self::Compound { shape, .. } => shape.parts().len(),
            Self::Mesh { mesh, .. } => mesh.triangle_count(),
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

    /// **The exact form is unchanged**: seven `f64`s, little-endian, position
    /// then rotation `x, y, z, w` — what every replicated transform was before
    /// quantization, and what one outside the quantized range still is.
    #[test]
    fn the_exact_encoding_is_the_seven_values_little_endian() {
        let t = Transform::new(
            DVec3::new(1.25, -2.5, 1e6),
            crate::rotation_from_scaled_axis(DVec3::X * 0.5),
        );
        let mut buf = Vec::new();
        t.encode(&mut buf);
        let raw: Vec<u8> = [
            t.position.x,
            t.position.y,
            t.position.z,
            t.rotation.x,
            t.rotation.y,
            t.rotation.z,
            t.rotation.w,
        ]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
        assert_eq!(buf, raw);
    }

    #[test]
    fn the_wire_position_quantum_is_a_power_of_two() {
        assert_eq!(Transform::WIRE_POSITION.quantum(), 1.0 / 2048.0);
        assert_eq!(Transform::QUANTIZED_LEN, 16);
    }

    /// A transform drawn from `i`: a position anywhere inside the quantized
    /// range and an arbitrary rotation.
    fn drawn(i: u64) -> Transform {
        use crcbl_core::rand::hash_unit;
        let extent = Transform::WIRE_POSITION_EXTENT;
        let unit = |k: u64| hash_unit(0x7157, 8 * i + k) * 2.0 - 1.0;
        let axis = DVec3::new(unit(3), unit(4), unit(5));
        Transform::new(
            DVec3::new(unit(0), unit(1), unit(2)) * extent * (1.0 - 1e-9),
            crate::rotation_from_scaled_axis(axis * std::f64::consts::PI),
        )
    }

    /// **A quantized transform decodes within its bounds**: half a quantum per
    /// position axis, and the rotation within smallest-three's four half-quanta
    /// per component (derived beside `crcbl_ecs::quantize`'s own test), after
    /// lining `q` and `-q` up.
    #[test]
    fn a_quantized_transform_round_trips_within_its_bounds() {
        let half = Transform::WIRE_POSITION.quantum() / 2.0;
        let rotation_bound = 2.0 * Transform::WIRE_ROTATION.quantum();
        for i in 0..5_000 {
            let t = drawn(i);
            let mut buf = Vec::new();
            t.encode_wire(&mut buf);
            assert_eq!(buf.len(), Transform::QUANTIZED_LEN, "{t:?} is in range");
            let back = Transform::decode_wire(&buf).expect("a quantized payload decodes");
            assert!((back.position - t.position).abs().max_element() <= half);
            let aligned = if back.rotation.dot(t.rotation) < 0.0 {
                -back.rotation
            } else {
                back.rotation
            };
            let error = (aligned - t.rotation).to_array();
            assert!(
                error.iter().all(|e| e.abs() <= rotation_bound),
                "{error:?} past {rotation_bound:e}"
            );
        }
        // The range's own edges: its bottom corner and its last code.
        let top = Transform::WIRE_POSITION_EXTENT - Transform::WIRE_POSITION.quantum();
        for corner in [
            DVec3::splat(-Transform::WIRE_POSITION_EXTENT),
            DVec3::splat(top),
        ] {
            let mut buf = Vec::new();
            Transform::from_position(corner).encode_wire(&mut buf);
            assert_eq!(buf.len(), Transform::QUANTIZED_LEN);
            assert_eq!(Transform::decode_wire(&buf).unwrap().position, corner);
        }
    }

    /// **Outside the quantized range a transform replicates exactly**, rather
    /// than being clamped to the edge, and the decoder reads that form too.
    #[test]
    fn a_transform_outside_the_range_falls_back_to_the_exact_form() {
        let far = Transform::new(
            DVec3::new(Transform::WIRE_POSITION_EXTENT, -3.0, 0.125),
            crate::rotation_from_scaled_axis(DVec3::Z * 1.1),
        );
        let mut buf = Vec::new();
        far.encode_wire(&mut buf);
        assert_eq!(buf.len(), Transform::ENCODED_LEN);
        assert_eq!(Transform::decode_wire(&buf), Some(far), "bit for bit");

        for len in [
            0,
            4,
            Transform::QUANTIZED_LEN - 1,
            Transform::QUANTIZED_LEN + 1,
        ] {
            assert_eq!(Transform::decode_wire(&vec![0; len]), None, "{len} bytes");
        }
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

    /// **Each shape's farthest point from the body's origin**, checked
    /// against values worked by hand.
    #[test]
    fn max_extent_is_the_farthest_point_of_each_shape() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        let sphere = ColliderComponent::Sphere {
            offset: DVec3::new(3.0, 4.0, 0.0),
            radius: 1.0,
            is_trigger: false,
        };
        assert!(close(sphere.max_extent(), 6.0));
        let cuboid = ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::new(1.0, 2.0, 2.0),
            is_trigger: false,
        };
        assert!(close(cuboid.max_extent(), 3.0));
        let capsule = ColliderComponent::Capsule {
            offset: DVec3::new(0.0, 1.0, 0.0),
            radius: 0.5,
            half_height: 1.0,
            is_trigger: false,
        };
        assert!(close(capsule.max_extent(), 2.5), "the upper cap");
        let shape = CompoundShape::from_aabbs(&[crate::collider::Aabb::from_centre_half(
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(1.0, 2.0, 2.0),
        )])
        .expect("one part");
        let compound = ColliderComponent::Compound {
            offset: DVec3::new(1.0, 0.0, 0.0),
            shape,
            is_trigger: false,
        };
        // The corner at (1 + 2 + 1, 2, 2).
        assert!(close(compound.max_extent(), 24.0_f64.sqrt()));
    }
}
