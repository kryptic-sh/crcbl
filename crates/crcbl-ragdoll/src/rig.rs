//! The validated rig: which joints carry bodies, their shapes and masses, the
//! physical joints between them, and the pairs kept from colliding.
//!
//! Lengths are metres and masses kilograms. A body is placed in its skeleton
//! joint's *rigid frame* — the joint's world rotation and origin with the
//! skeleton's scale taken out — so a rig authored in metres fits a skeleton
//! imported at centimetre scale. Joint frames are in each body's own frame, as
//! [`crcbl_phys::Joint::new`] takes them, and were fixed in the skeleton's
//! rest pose: a joint's limits are measured from the rest pose, whatever pose
//! the ragdoll is spawned in.

use crcbl_phys::{
    ColliderComponent, JointKind, RevoluteJoint, SphericalJoint, Transform, WeldJoint,
};
use glam::{DQuat, DVec3};

/// A body's collision shape, centred on the body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyShape {
    /// A capsule along the body's `y`.
    Capsule {
        /// In metres.
        radius: f64,
        /// Half the length of the cylinder between the caps, in metres.
        half_height: f64,
    },
    /// A sphere.
    Sphere {
        /// In metres.
        radius: f64,
    },
    /// A box along the body's axes.
    Box {
        /// In metres.
        half_extents: DVec3,
    },
}

impl BodyShape {
    /// The physics collider for this shape.
    #[must_use]
    pub const fn collider(&self) -> ColliderComponent {
        match *self {
            Self::Capsule {
                radius,
                half_height,
            } => ColliderComponent::Capsule {
                offset: DVec3::ZERO,
                radius,
                half_height,
                is_trigger: false,
            },
            Self::Sphere { radius } => ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius,
                is_trigger: false,
            },
            Self::Box { half_extents } => ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents,
                is_trigger: false,
            },
        }
    }

    /// Its volume, in cubic metres.
    #[must_use]
    pub fn volume(&self) -> f64 {
        use core::f64::consts::PI;
        match *self {
            Self::Capsule {
                radius,
                half_height,
            } => PI * radius * radius * (2.0 * half_height + 4.0 / 3.0 * radius),
            Self::Sphere { radius } => 4.0 / 3.0 * PI * radius.powi(3),
            Self::Box { half_extents } => 8.0 * half_extents.x * half_extents.y * half_extents.z,
        }
    }

    fn is_valid(&self) -> bool {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        match *self {
            Self::Capsule {
                radius,
                half_height,
            } => positive(radius) && half_height.is_finite() && half_height >= 0.0,
            Self::Sphere { radius } => positive(radius),
            Self::Box { half_extents } => half_extents.to_array().into_iter().all(positive),
        }
    }
}

/// One rigid body of the rig.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigBody {
    /// The skeleton joint it drives, by palette index.
    pub joint: usize,
    /// Its shape.
    pub shape: BodyShape,
    /// In kilograms.
    pub mass: f64,
    /// The body's pose in its joint's rigid frame, in metres.
    pub offset: Transform,
}

/// What a physical joint lets through, measured from the rest pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RigJointKind {
    /// A ball and socket: the child's axis kept within `cone` radians of the
    /// parent's, and its twist about that axis between the two twist angles.
    Ball {
        /// The cone's half-angle, in `[0, π]`.
        cone: f64,
        /// The least twist, in `[-π, π]`.
        twist_lower: f64,
        /// The greatest twist, in `[-π, π]`, not below `twist_lower`.
        twist_upper: f64,
    },
    /// A hinge about the joint's axis, bending between the two angles.
    Hinge {
        /// In `[-π, π]`.
        lower: f64,
        /// In `[-π, π]`, not below `lower`.
        upper: f64,
    },
    /// Welded: the two bodies move as one.
    Fixed,
}

impl RigJointKind {
    /// The physics joint kind, resisting relative turning with up to
    /// `friction` newton-metres: a motor driving zero relative speed. A weld
    /// has no turning to resist.
    #[must_use]
    pub const fn joint_kind(&self, friction: f64) -> JointKind {
        match *self {
            Self::Ball {
                cone,
                twist_lower,
                twist_upper,
            } => JointKind::Spherical(
                SphericalJoint::ball()
                    .with_cone(cone)
                    .with_twist(twist_lower, twist_upper)
                    .with_motor(DVec3::ZERO, friction),
            ),
            Self::Hinge { lower, upper } => JointKind::Revolute(
                RevoluteJoint::hinge()
                    .with_limits(lower, upper)
                    .with_motor(0.0, friction),
            ),
            Self::Fixed => JointKind::Weld(WeldJoint::rigid()),
        }
    }

    fn is_valid(&self) -> bool {
        use core::f64::consts::PI;
        let angle = |value: f64| value.is_finite() && (-PI..=PI).contains(&value);
        match *self {
            Self::Ball {
                cone,
                twist_lower,
                twist_upper,
            } => {
                cone.is_finite()
                    && (0.0..=PI).contains(&cone)
                    && angle(twist_lower)
                    && angle(twist_upper)
                    && twist_lower <= twist_upper
            }
            Self::Hinge { lower, upper } => angle(lower) && angle(upper) && lower <= upper,
            Self::Fixed => true,
        }
    }
}

/// A physical joint between two of the rig's bodies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigJoint {
    /// The parent body, by index into [`RagdollRig::bodies`].
    pub parent: usize,
    /// The child body, likewise.
    pub child: usize,
    /// The joint's frame in the parent body's frame. Its `z` is the cone or
    /// hinge axis.
    pub frame_parent: Transform,
    /// The same frame in the child body's frame, as it stands at rest.
    pub frame_child: Transform,
    /// What it lets through.
    pub kind: RigJointKind,
    /// The most torque it resists turning with, in newton-metres: the
    /// stiffness of a dead body's joints, which lets a ragdoll come to rest
    /// rather than tremble.
    pub friction: f64,
}

/// Why a rig is not valid.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum RigError {
    /// A rig needs at least one body.
    #[error("a ragdoll rig has no bodies")]
    NoBodies,
    /// A body names a joint the skeleton does not have.
    #[error("rig body {body} names skeleton joint {joint}, which does not exist")]
    JointOutOfRange {
        /// The body.
        body: usize,
        /// The joint it named.
        joint: usize,
    },
    /// Two bodies drive one skeleton joint.
    #[error("skeleton joint {joint} carries more than one rig body")]
    DuplicateJoint {
        /// The joint.
        joint: usize,
    },
    /// A body's shape, mass or offset is not finite and positive where it
    /// must be.
    #[error("rig body {body} has an invalid shape, mass or offset")]
    InvalidBody {
        /// The body.
        body: usize,
    },
    /// A physical joint names a body the rig does not have, or one body
    /// twice.
    #[error("rig joint {joint} does not join two distinct rig bodies")]
    JointBodies {
        /// The joint.
        joint: usize,
    },
    /// A physical joint's frames or limits are out of range.
    #[error("rig joint {joint} has invalid frames or limits")]
    InvalidJoint {
        /// The joint.
        joint: usize,
    },
    /// An ignored pair names a body the rig does not have, or one body twice.
    #[error("ignored pair {pair} does not name two distinct rig bodies")]
    IgnoredPair {
        /// Its index.
        pair: usize,
    },
}

/// A ragdoll rig, checked against the skeleton it was made for.
///
/// Build one from an authored [`crate::RagdollDesc`], from
/// [`RagdollRig::generate`], or from parts with [`RagdollRig::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct RagdollRig {
    joint_count: usize,
    bodies: Vec<RigBody>,
    joints: Vec<RigJoint>,
    ignored: Vec<(usize, usize)>,
    substeps: u32,
}

impl RagdollRig {
    /// A rig for a skeleton of `joint_count` joints.
    ///
    /// `ignored` lists body pairs, by index, that never collide with each
    /// other; jointed pairs never collide anyway. `substeps` is the solver
    /// substeps the ragdoll's bodies ask for, or zero for the system's own
    /// (see [`crcbl_phys::PhysicsSystem::set_substeps`]).
    ///
    /// # Errors
    ///
    /// [`RigError`] names the first body, joint or pair at fault.
    pub fn new(
        joint_count: usize,
        bodies: Vec<RigBody>,
        joints: Vec<RigJoint>,
        ignored: Vec<(usize, usize)>,
        substeps: u32,
    ) -> Result<Self, RigError> {
        if bodies.is_empty() {
            return Err(RigError::NoBodies);
        }
        let mut carried = vec![false; joint_count];
        for (index, body) in bodies.iter().enumerate() {
            let slot = carried
                .get_mut(body.joint)
                .ok_or(RigError::JointOutOfRange {
                    body: index,
                    joint: body.joint,
                })?;
            if core::mem::replace(slot, true) {
                return Err(RigError::DuplicateJoint { joint: body.joint });
            }
            if !body.shape.is_valid()
                || !body.mass.is_finite()
                || body.mass <= 0.0
                || !is_rigid(&body.offset)
            {
                return Err(RigError::InvalidBody { body: index });
            }
        }
        let distinct = |a: usize, b: usize| a != b && a < bodies.len() && b < bodies.len();
        for (index, joint) in joints.iter().enumerate() {
            if !distinct(joint.parent, joint.child) {
                return Err(RigError::JointBodies { joint: index });
            }
            if !is_rigid(&joint.frame_parent)
                || !is_rigid(&joint.frame_child)
                || !joint.kind.is_valid()
                || !joint.friction.is_finite()
                || joint.friction < 0.0
            {
                return Err(RigError::InvalidJoint { joint: index });
            }
        }
        for (pair, &(a, b)) in ignored.iter().enumerate() {
            if !distinct(a, b) {
                return Err(RigError::IgnoredPair { pair });
            }
        }
        Ok(Self {
            joint_count,
            bodies,
            joints,
            ignored,
            substeps,
        })
    }

    /// How many joints the skeleton it was made for has.
    #[must_use]
    pub const fn joint_count(&self) -> usize {
        self.joint_count
    }

    /// The bodies, in the order a spawned ragdoll creates them.
    #[must_use]
    pub fn bodies(&self) -> &[RigBody] {
        &self.bodies
    }

    /// The physical joints.
    #[must_use]
    pub fn joints(&self) -> &[RigJoint] {
        &self.joints
    }

    /// The body pairs kept from colliding, besides the jointed ones.
    #[must_use]
    pub fn ignored(&self) -> &[(usize, usize)] {
        &self.ignored
    }

    /// The solver substeps the bodies ask for; zero for the system's.
    #[must_use]
    pub const fn substeps(&self) -> u32 {
        self.substeps
    }

    /// The rig's total mass, in kilograms.
    #[must_use]
    pub fn mass(&self) -> f64 {
        self.bodies.iter().map(|body| body.mass).sum()
    }
}

/// Whether `transform` is finite with a unit rotation.
pub(crate) fn is_rigid(transform: &Transform) -> bool {
    transform.position.is_finite()
        && transform.rotation.is_finite()
        && transform.rotation.is_normalized()
}

/// `a` then `b`: `b` given in `a`'s frame.
pub(crate) fn compose(a: &Transform, b: &Transform) -> Transform {
    Transform::new(
        a.position + a.rotation * b.position,
        (a.rotation * b.rotation).normalize(),
    )
}

/// The frame that undoes `transform`.
pub(crate) fn inverse(transform: &Transform) -> Transform {
    let rotation = transform.rotation.conjugate();
    Transform::new(-(rotation * transform.position), rotation)
}

/// The shortest rotation taking `from` onto the unit direction of `to`, or
/// `None` if `to` has no direction.
pub(crate) fn rotation_onto(from: DVec3, to: DVec3) -> Option<DQuat> {
    let to = to.try_normalize()?;
    Some(DQuat::from_rotation_arc(from, to))
}
