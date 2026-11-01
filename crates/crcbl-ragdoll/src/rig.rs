//! The validated rig: which joints carry bodies, their shapes and masses, the
//! physical joints between them, and the pairs kept from colliding.
//!
//! Lengths are metres and masses kilograms. A body is placed in its skeleton
//! joint's *rigid frame* — the joint's world rotation and origin with the
//! skeleton's scale taken out — so a rig authored in metres fits a skeleton
//! imported at centimetre scale. Joint frames are in each body's own frame, as
//! [`crcbl_phys::Joint::new`] takes them, and were fixed in the skeleton's
//! rest pose. A joint's limits are measured from its neutral, which is the
//! rest pose unless the joint names another, whatever pose the ragdoll is
//! spawned in: [`RigJoint::physics_frames`] turns the frames by the neutral,
//! so the physics joint's zero is the neutral.

use crcbl_phys::{
    ColliderComponent, JointKind, RevoluteJoint, SphericalJoint, Transform, WeldJoint,
    rotation_from_scaled_axis,
};
use glam::{DQuat, DVec3};

use crate::limits::{SoftRange, SoftSwing, SwingTwist, angle, both_or_neither_zero};

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
            // Multiplied out rather than `powi(3)`, whose precision is the
            // target's to choose: the volume shares out a generated rig's
            // mass, so it must be the same bits everywhere.
            Self::Sphere { radius } => 4.0 / 3.0 * PI * (radius * radius * radius),
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

/// What a physical joint lets through, measured from its neutral: the
/// child's turn from rest at which it meets no resistance, rest itself unless
/// a neutral is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RigJointKind {
    /// A ball and socket: the child's axis kept inside an elliptical cone
    /// about the parent's, and its twist about that axis between the two
    /// twist angles. The half-angles `swing_x` and `swing_y` are about the
    /// x- and y-axes of the joint's frame **on the parent's side**,
    /// [`RigJoint::frame_parent`], tipped by the neutral swing: the ellipse
    /// belongs to the parent and is not turned by the child's twist.
    Ball {
        /// The swing's half-angle about x, in `[0, π]`; zero only with
        /// `swing_y` zero too.
        swing_x: f64,
        /// The swing's half-angle about y, in `[0, π]`; zero only with
        /// `swing_x` zero too.
        swing_y: f64,
        /// The least twist, in `[-π, π]`.
        twist_lower: f64,
        /// The greatest twist, in `[-π, π]`, not below `twist_lower`.
        twist_upper: f64,
        /// The neutral turn the limits are measured from. A pose outside the
        /// limits as measured from it snaps inside on the first steps; see
        /// [`RigJointKind::rest_is_within_limits`].
        neutral: SwingTwist,
        /// Resistance past a softer ellipse inside the swing limit.
        soft_swing: Option<SoftSwing>,
        /// Resistance past soft bounds inside the twist limit.
        soft_twist: Option<SoftRange>,
    },
    /// A hinge about the joint's axis, bending between the two angles.
    Hinge {
        /// In `[-π, π]`.
        lower: f64,
        /// In `[-π, π]`, not below `lower`.
        upper: f64,
        /// The neutral bend from rest about the axis, in `[-π, π]`, which
        /// the limits are measured from. A pose outside the limits as
        /// measured from it snaps inside on the first steps; see
        /// [`RigJointKind::rest_is_within_limits`].
        neutral: f64,
        /// Resistance past soft bounds inside the limits.
        soft: Option<SoftRange>,
    },
    /// Welded: the two bodies move as one.
    Fixed,
}

impl RigJointKind {
    /// The physics joint kind, resisting relative turning with up to
    /// `friction` newton-metres: a motor driving zero relative speed. A weld
    /// has no turning to resist. Its zero is the neutral only once the frames
    /// say so: spawn it with [`RigJoint::physics_frames`].
    #[must_use]
    pub const fn joint_kind(&self, friction: f64) -> JointKind {
        match *self {
            Self::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral: _,
                soft_swing,
                soft_twist,
            } => {
                let mut ball = SphericalJoint::ball()
                    .with_swing(swing_x, swing_y)
                    .with_twist(twist_lower, twist_upper)
                    .with_motor(DVec3::ZERO, friction);
                if let Some(soft) = soft_swing {
                    ball = ball.with_soft_swing(soft.x, soft.y, soft.zone());
                }
                if let Some(soft) = soft_twist {
                    ball = ball.with_soft_twist(soft.lower, soft.upper, soft.zone());
                }
                JointKind::Spherical(ball)
            }
            Self::Hinge {
                lower,
                upper,
                neutral: _,
                soft,
            } => {
                let mut hinge = RevoluteJoint::hinge()
                    .with_limits(lower, upper)
                    .with_motor(0.0, friction);
                if let Some(soft) = soft {
                    hinge = hinge.with_soft_limits(soft.lower, soft.upper, soft.zone());
                }
                JointKind::Revolute(hinge)
            }
            Self::Fixed => JointKind::Weld(WeldJoint::rigid()),
        }
    }

    /// How the neutral turns the joint's frame on each side, parent's then
    /// child's: a ball's neutral swing on the parent's side, so its swing
    /// ellipse turns with it and no further, and the inverse of its neutral
    /// twist on the child's; a hinge's bend on the parent's side. The
    /// identity on both for a weld and for a neutral of rest.
    pub(crate) fn neutral_turns(&self) -> (DQuat, DQuat) {
        match *self {
            Self::Ball { neutral, .. } => (neutral.swing(), neutral.twist().conjugate()),
            Self::Hinge { neutral, .. } => (
                rotation_from_scaled_axis(DVec3::Z * neutral),
                DQuat::IDENTITY,
            ),
            Self::Fixed => (DQuat::IDENTITY, DQuat::IDENTITY),
        }
    }

    /// Whether the rest pose lies inside the hard limits as measured from
    /// the neutral. A rig spawned at a pose outside them is pushed inside on
    /// its first steps — a snap — so a neutral farther from rest than its
    /// limits reach snaps any ragdoll spawned near rest; an editor warns of
    /// it. Always for a weld.
    ///
    /// Inclusive, to within a billionth: a limit drawn through rest holds.
    #[must_use]
    pub fn rest_is_within_limits(&self) -> bool {
        /// How far past a limit, as a sine of half the angle or a fraction of
        /// the ellipse, rest may lie and still count as inside.
        const SLACK: f64 = 1e-9;
        match *self {
            Self::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                ..
            } => {
                // Rest against the physics joint's frames: the parent's turned
                // by the neutral swing, the child's by the inverse twist.
                let rest = (neutral.twist() * neutral.swing()).conjugate();
                let across = (rest.z * rest.z + rest.w * rest.w).sqrt();
                let (x, y) = if across > 0.0 {
                    (
                        (rest.w * rest.x - rest.z * rest.y) / across,
                        (rest.w * rest.y + rest.z * rest.x) / across,
                    )
                } else {
                    (rest.x, rest.y)
                };
                let within_swing = if swing_x == 0.0 {
                    x.abs() <= SLACK && y.abs() <= SLACK
                } else {
                    let (u, v) = (x / half_sine(swing_x), y / half_sine(swing_y));
                    u * u + v * v <= 1.0 + SLACK
                };
                // The twist's half-angle sine, its quaternion taken with a
                // non-negative real part, against those of the limits: the
                // sine rises with the half-angle across the whole range.
                let sign = if rest.w < 0.0 { -1.0 } else { 1.0 };
                let twist = if across > 0.0 {
                    sign * rest.z / across
                } else {
                    0.0
                };
                within_swing
                    && half_sine(twist_lower) - SLACK <= twist
                    && twist <= half_sine(twist_upper) + SLACK
            }
            Self::Hinge {
                lower,
                upper,
                neutral,
                ..
            } => (lower - SLACK..=upper + SLACK).contains(&-neutral),
            Self::Fixed => true,
        }
    }

    fn is_valid(&self) -> bool {
        use core::f64::consts::PI;
        match *self {
            Self::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                soft_swing,
                soft_twist,
            } => {
                let half_turn = |value: f64| (0.0..=PI).contains(&value);
                half_turn(swing_x)
                    && half_turn(swing_y)
                    && both_or_neither_zero(swing_x, swing_y)
                    && angle(twist_lower)
                    && angle(twist_upper)
                    && twist_lower <= twist_upper
                    && neutral.is_valid()
                    && soft_swing.is_none_or(|soft| soft.is_valid_within(swing_x, swing_y))
                    && soft_twist.is_none_or(|soft| soft.is_valid_within(twist_lower, twist_upper))
            }
            Self::Hinge {
                lower,
                upper,
                neutral,
                soft,
            } => {
                angle(lower)
                    && angle(upper)
                    && lower <= upper
                    && angle(neutral)
                    && soft.is_none_or(|soft| soft.is_valid_within(lower, upper))
            }
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
    /// The joint's frame in the parent body's frame, as it stands at rest.
    /// Its `z` is the swing's or the hinge's axis; the physics joint takes it
    /// turned by the neutral ([`RigJoint::physics_frames`]). Its `x` and `y`
    /// are the axes a ball's swing half-angles are about.
    pub frame_parent: Transform,
    /// The same frame in the child body's frame, as it stands at rest; the
    /// physics joint takes it turned back by a ball's neutral twist.
    pub frame_child: Transform,
    /// What it lets through.
    pub kind: RigJointKind,
    /// The most torque it resists turning with, in newton-metres: the
    /// stiffness of a dead body's joints, which lets a ragdoll come to rest
    /// rather than tremble.
    pub friction: f64,
}

impl RigJoint {
    /// The frames the physics joint takes, parent's then child's, turned so
    /// the physics joint's zero — which its limits are measured from — is the
    /// neutral. A hinge's parent frame turns by its neutral bend. A ball's
    /// parent frame turns by its neutral **swing** alone, and its child frame
    /// back by its neutral **twist**: the swing ellipse stays on the
    /// parent's axes, tipped with the swing, and a neutral twist leaves it
    /// where it was. A side the neutral does not turn is exactly as stored,
    /// so a joint whose neutral is rest spawns to the same bits as before.
    #[must_use]
    pub fn physics_frames(&self) -> (Transform, Transform) {
        let (parent, child) = self.kind.neutral_turns();
        let turned = |frame: Transform, turn: DQuat| {
            if turn == DQuat::IDENTITY {
                return frame;
            }
            Transform::new(frame.position, (frame.rotation * turn).normalize())
        };
        (
            turned(self.frame_parent, parent),
            turned(self.frame_child, child),
        )
    }
}

/// The sine of half `angle`, built without the platform's trigonometry: the
/// `z` part of the turn by `angle` about `z`.
fn half_sine(angle: f64) -> f64 {
    rotation_from_scaled_axis(DVec3::Z * angle).z
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

/// The shortest rotation taking the unit vector `from` onto the unit
/// direction of `to`, or `None` if `to` has no direction.
///
/// `glam`'s `DQuat::from_rotation_arc`, except for a half turn: glam builds
/// that with the platform's sine and cosine of a quarter turn, a cosine that
/// is not exactly zero and is rounded as each C runtime rounds it, so a body
/// fitted to a bone pointing straight down — every leg of a T-pose — could
/// stand turned by different bits on different targets. The half turn about
/// an axis is exactly that axis with a zero real part.
pub(crate) fn rotation_onto(from: DVec3, to: DVec3) -> Option<DQuat> {
    let to = to.try_normalize()?;
    // glam's own threshold for the half turn, so every other pair takes its
    // arithmetic unchanged.
    const ONE_MINUS_EPS: f64 = 1.0 - 2.0 * f64::EPSILON;
    if from.dot(to) < -ONE_MINUS_EPS {
        let axis = from.any_orthonormal_vector();
        return Some(DQuat::from_xyzw(axis.x, axis.y, axis.z, 0.0));
    }
    Some(DQuat::from_rotation_arc(from, to))
}

/// The joint frame's turn in its child bone's frame: its `z` along `axis`,
/// its `x` along `x_axis` made square to it, or with no `x_axis`
/// [`rotation_onto`]'s shortest turn from `z`. `None` if `axis` has no
/// direction, or `x_axis` none across it.
pub(crate) fn joint_rotation(axis: DVec3, x_axis: Option<DVec3>) -> Option<DQuat> {
    let Some(x_axis) = x_axis else {
        return rotation_onto(DVec3::Z, axis);
    };
    let z = axis.try_normalize()?;
    let x = (x_axis - z * z.dot(x_axis)).try_normalize()?;
    let y = z.cross(x);
    Some(DQuat::from_mat3(&glam::DMat3::from_cols(x, y, z)).normalize())
}

/// How far, as one less the cosine, a frame's `x` may stand from the one
/// [`rotation_onto`] gives its `z` and still count as that roll: rounding,
/// not an authored turn, about a millionth of a radian.
const ROLL_TOLERANCE: f64 = 1e-12;

/// Whether a joint frame whose `z` is `axis` and `x` is `x` has the roll
/// [`joint_rotation`] gives `axis` alone, to within [`ROLL_TOLERANCE`].
pub(crate) fn has_default_roll(axis: DVec3, x: DVec3) -> bool {
    rotation_onto(DVec3::Z, axis)
        .is_some_and(|rotation| (rotation * DVec3::X).dot(x) >= 1.0 - ROLL_TOLERANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A half turn is built without the platform's trigonometry: its real
    /// part is exactly zero, and it still takes `from` onto `to`.
    #[test]
    fn a_half_turn_onto_a_bone_is_exact() {
        let turn = rotation_onto(DVec3::Y, DVec3::new(0.0, -0.42, 0.0)).expect("a direction");
        assert_eq!(turn.w, 0.0);
        assert!((turn * DVec3::Y - DVec3::NEG_Y).length() < 1e-15);
        let arc = rotation_onto(DVec3::Y, DVec3::new(0.3, 0.4, 0.0)).expect("a direction");
        assert_eq!(
            arc,
            DQuat::from_rotation_arc(DVec3::Y, DVec3::new(0.6, 0.8, 0.0))
        );
    }

    /// **A joint frame's `x` is what the ragdoll file's docs say**: the
    /// bone's `x` for an axis of `z`, `-z` for an axis of `x`, `-x` for an
    /// axis of exactly `-z`; and a given `swing_x_axis` is taken square to the
    /// axis.
    #[test]
    fn a_joint_frames_x_is_the_documented_one() {
        let x_of = |axis: DVec3, x_axis: Option<DVec3>| {
            joint_rotation(axis, x_axis).expect("a frame") * DVec3::X
        };
        let near = |a: DVec3, b: DVec3| a.distance(b) < 1e-15;
        assert!(near(x_of(DVec3::Z, None), DVec3::X));
        assert!(near(x_of(DVec3::X, None), DVec3::NEG_Z));
        assert_eq!(x_of(DVec3::NEG_Z, None), DVec3::NEG_X);
        let tilted = x_of(DVec3::X, Some(DVec3::new(0.5, 0.0, 2.0)));
        assert!(near(tilted, DVec3::Z), "{tilted}");
        assert!(joint_rotation(DVec3::X, Some(DVec3::X * 3.0)).is_none());
        assert!(has_default_roll(DVec3::X, DVec3::NEG_Z));
        assert!(!has_default_roll(DVec3::X, DVec3::Z));
    }
}
