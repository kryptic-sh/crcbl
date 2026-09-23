//! Joints: the rest of rung 5 of `docs/plan/36-contact-solver.md` — L3's
//! constraints, solved by the same Soft Step as the contacts.
//!
//! A [`Joint`] ties two bodies together at a frame fixed in each: an anchor
//! point and an orientation. Its [`JointKind`] says which of the six relative
//! motions it takes away, which it bounds with limits, and which a motor or a
//! spring drives:
//!
//! | Kind                        | Holds                                    | Limits, motor, spring                         |
//! | --------------------------- | ---------------------------------------- | --------------------------------------------- |
//! | [`DistanceJoint`]           | the anchors' distance (rigid, or a rope) | minimum and maximum length, a motor, a spring |
//! | [`RevoluteJoint`] (hinge)   | the anchors together, the frames' z-axes | the angle about z, a motor, a spring          |
//! | [`PrismaticJoint`] (slider) | the orientation, and B on A's x-axis     | the travel along x, a motor, a spring         |
//! | [`WeldJoint`] (fixed)       | everything, softly if asked              | —                                             |
//! | [`SphericalJoint`] (ball)   | the anchors together                     | a cone about A's z-axis, twist, a motor       |
//!
//! These are Box3D's joints (github.com/erincatto/box3d, commit `9e5a4cde`),
//! the three-dimensional sibling of Box2D v3's, whose solvers the contact
//! solver's own Soft Step already transcribes; each solver's source is named
//! in `crates/crcbl-phys/src/contact/joint/`.
//!
//! # What a joint does to the rest of the system
//!
//! - **It joins an island.** Two dynamic bodies joined by a joint are one
//!   island, which sleeps and wakes whole; a joint to a static body joins
//!   nothing, as a contact with one does not.
//! - **A new joint wakes both its bodies**, and so does changing one through
//!   [`crate::PhysicsSystem::set_joint`] or taking one away.
//! - **It is warm-started**: every impulse it carries seeds the next tick.
//! - **Its bodies do not collide** unless [`Joint::collide_connected`] says
//!   so, as Box2D's default is: a ragdoll's limbs overlap at every joint.
//! - **It breaks** when the force or torque it carries reaches
//!   [`Joint::force_threshold`] or [`Joint::torque_threshold`]: it is taken
//!   out at the end of that step, and reported as a [`JointBreak`].
//!
//! Joints exist in a system made with
//! [`crate::PhysicsSystem::with_contacts`]: they are constraints in its
//! solver.

use crcbl_ecs::Entity;
use glam::DVec3;

use crate::components::Transform;

/// A joint added to a system: see [`crate::PhysicsSystem::add_joint`].
///
/// Generational, so an id kept after its joint was removed or broke names
/// nothing rather than whatever took its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JointId(pub(crate) crcbl_core::Handle<crate::contact::joint::JointRecord>);

impl JointId {
    /// The id as one integer, for hashing and replication.
    #[must_use]
    pub const fn to_bits(self) -> u64 {
        self.0.to_bits()
    }
}

/// A spring's stiffness and damping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// How stiff, as the frequency of the oscillation it would make, in hertz.
    /// Zero is no spring at all.
    pub hertz: f64,
    /// Its damping ratio: 1 is critical.
    pub damping_ratio: f64,
}

/// A joint between two bodies.
///
/// The frames are each body's own: `frame_a` is where the joint sits on
/// `body_a`, in `body_a`'s frame, relative to its position (its centre of
/// mass), and likewise `frame_b`. [`Joint::at`] builds both from one frame
/// in the world. A body may be dynamic, kinematic or static — an entity with
/// only a transform is a fixed anchor — but a joint neither of whose bodies
/// is dynamic is never solved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joint {
    /// The first body.
    pub body_a: Entity,
    /// The second body.
    pub body_b: Entity,
    /// The joint's frame on `body_a`, in its frame.
    pub frame_a: Transform,
    /// The joint's frame on `body_b`, in its frame.
    pub frame_b: Transform,
    /// What it holds, and how.
    pub kind: JointKind,
    /// Whether the two bodies still collide with each other. Off by default.
    pub collide_connected: bool,
    /// The force, in newtons, at which it breaks. Infinite by default.
    pub force_threshold: f64,
    /// The torque, in newton-metres, at which it breaks. Infinite by default.
    pub torque_threshold: f64,
    /// How stiff its rigid parts are, as a spring's frequency in hertz: decision
    /// 1's 60 Hz by default, Box3D's `constraintHertz`. Capped at a quarter of
    /// the substep rate, as a contact's is, and raised in proportion for a
    /// group with extra substeps: see [`crate::PhysicsSystem::set_substeps`].
    pub constraint_hertz: f64,
    /// Their damping ratio: decision 1's 2 by default.
    pub constraint_damping_ratio: f64,
}

impl Joint {
    /// Decision 1's joint stiffness, in hertz, and Box3D's default.
    pub const CONSTRAINT_HERTZ: f64 = 60.0;
    /// Decision 1's joint damping ratio, and Box3D's default.
    pub const CONSTRAINT_DAMPING_RATIO: f64 = 2.0;

    /// A joint of `kind` between `body_a` and `body_b` at the frames given in
    /// each body's own frame, unbreakable, its bodies not colliding.
    #[must_use]
    pub const fn new(
        body_a: Entity,
        body_b: Entity,
        frame_a: Transform,
        frame_b: Transform,
        kind: JointKind,
    ) -> Self {
        Self {
            body_a,
            body_b,
            frame_a,
            frame_b,
            kind,
            collide_connected: false,
            force_threshold: f64::INFINITY,
            torque_threshold: f64::INFINITY,
            constraint_hertz: Self::CONSTRAINT_HERTZ,
            constraint_damping_ratio: Self::CONSTRAINT_DAMPING_RATIO,
        }
    }

    /// [`new`](Self::new) with both frames taken from one frame in the world,
    /// `frame`, for bodies standing at `at_a` and `at_b`: the joint holds
    /// them as they stand.
    #[must_use]
    pub fn at(
        body_a: Entity,
        at_a: &Transform,
        body_b: Entity,
        at_b: &Transform,
        frame: Transform,
        kind: JointKind,
    ) -> Self {
        Self::new(
            body_a,
            body_b,
            local_frame(at_a, frame),
            local_frame(at_b, frame),
            kind,
        )
    }

    /// This joint, breaking at `force` newtons or `torque` newton-metres.
    #[must_use]
    pub const fn breaking_at(self, force: f64, torque: f64) -> Self {
        Self {
            force_threshold: force,
            torque_threshold: torque,
            ..self
        }
    }

    /// This joint, its bodies colliding with each other or not.
    #[must_use]
    pub const fn colliding(self, collide_connected: bool) -> Self {
        Self {
            collide_connected,
            ..self
        }
    }

    /// Why this joint cannot be added, if it cannot: a frame or a number
    /// that is not finite, a threshold below zero, or a limit whose lower end
    /// is above its upper.
    pub(crate) fn invalid(&self) -> Option<&'static str> {
        let finite = |t: &Transform| {
            t.position.is_finite() && t.rotation.is_finite() && t.rotation.length_squared() > 0.0
        };
        if !finite(&self.frame_a) || !finite(&self.frame_b) {
            return Some("a joint's frames are finite, with non-zero rotations");
        }
        if self.force_threshold.is_nan()
            || self.torque_threshold.is_nan()
            || self.force_threshold < 0.0
            || self.torque_threshold < 0.0
        {
            return Some("a joint's thresholds are zero or more");
        }
        if !self.constraint_hertz.is_finite()
            || self.constraint_hertz < 0.0
            || !self.constraint_damping_ratio.is_finite()
            || self.constraint_damping_ratio < 0.0
        {
            return Some("a joint's constraint stiffness and damping are finite and not negative");
        }
        self.kind.invalid()
    }
}

/// `world` in the frame of a body standing at `body`.
fn local_frame(body: &Transform, world: Transform) -> Transform {
    let inverse = body.rotation.conjugate();
    Transform::new(
        inverse * (world.position - body.position),
        (inverse * world.rotation).normalize(),
    )
}

/// Which joint, and its settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JointKind {
    /// Holds the anchors' distance: a rod, a rope or a spring.
    Distance(DistanceJoint),
    /// A hinge about the frames' z-axes.
    Revolute(RevoluteJoint),
    /// A slider along frame A's x-axis.
    Prismatic(PrismaticJoint),
    /// Welds the frames together.
    Weld(WeldJoint),
    /// A ball and socket, with a cone and twist limit for ragdoll shoulders
    /// and hips.
    Spherical(SphericalJoint),
}

impl JointKind {
    fn invalid(&self) -> Option<&'static str> {
        let finite = |values: &[f64]| values.iter().all(|v| v.is_finite());
        let spring = |s: &Spring| s.hertz >= 0.0 && s.damping_ratio >= 0.0;
        match self {
            Self::Distance(d) => {
                if !finite(&[d.length, d.min_length, d.motor_speed]) || d.max_length.is_nan() {
                    return Some("a distance joint's lengths are finite");
                }
                if d.length < 0.0 || d.min_length < 0.0 || d.min_length > d.max_length {
                    return Some("a distance joint's lengths are not negative, min at most max");
                }
                (!spring(&d.spring) || d.max_motor_force < 0.0)
                    .then_some("a distance joint's spring and motor are not negative")
            }
            Self::Revolute(r) => {
                if !finite(&[r.lower_angle, r.upper_angle, r.target_angle, r.motor_speed]) {
                    return Some("a revolute joint's angles are finite");
                }
                if r.lower_angle > r.upper_angle
                    || r.lower_angle < -core::f64::consts::PI
                    || r.upper_angle > core::f64::consts::PI
                {
                    return Some("a revolute joint's limits lie in [-π, π], lower first");
                }
                (!spring(&r.spring) || r.max_motor_torque < 0.0)
                    .then_some("a revolute joint's spring and motor are not negative")
            }
            Self::Prismatic(p) => {
                if !finite(&[
                    p.lower_translation,
                    p.upper_translation,
                    p.target_translation,
                    p.motor_speed,
                ]) {
                    return Some("a prismatic joint's translations are finite");
                }
                if p.lower_translation > p.upper_translation {
                    return Some("a prismatic joint's lower limit is at most its upper");
                }
                (!spring(&p.spring) || p.max_motor_force < 0.0)
                    .then_some("a prismatic joint's spring and motor are not negative")
            }
            Self::Weld(w) => (!spring(&w.linear) || !spring(&w.angular))
                .then_some("a weld joint's springs are not negative"),
            Self::Spherical(s) => {
                if !finite(&[s.cone_angle, s.lower_twist_angle, s.upper_twist_angle])
                    || !s.motor_velocity.is_finite()
                {
                    return Some("a spherical joint's angles are finite");
                }
                if s.cone_angle < 0.0
                    || s.cone_angle > core::f64::consts::PI
                    || s.lower_twist_angle > s.upper_twist_angle
                    || s.lower_twist_angle < -core::f64::consts::PI
                    || s.upper_twist_angle > core::f64::consts::PI
                {
                    return Some(
                        "a spherical joint's cone lies in [0, π] and its twist in [-π, π], lower first",
                    );
                }
                (s.max_motor_torque < 0.0).then_some("a spherical joint's motor is not negative")
            }
        }
    }
}

/// A distance joint: Box3D's `b3DistanceJointDef`.
///
/// **Rigid** unless its spring is enabled: then the length is held only by
/// the spring, and by the limits if they are on — which is a rope, with a
/// spring of zero hertz and a limit from zero to its length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceJoint {
    /// The rest length, in metres.
    pub length: f64,
    /// Whether the length is held by the spring (and the limits) rather than
    /// rigidly.
    pub enable_spring: bool,
    /// The spring, while it is enabled.
    pub spring: Spring,
    /// Whether the length is kept between the limits.
    pub enable_limit: bool,
    /// The shortest it may be, in metres.
    pub min_length: f64,
    /// The longest it may be, in metres; infinite for no bound.
    pub max_length: f64,
    /// Whether the motor drives the length.
    pub enable_motor: bool,
    /// The most force the motor applies, in newtons.
    pub max_motor_force: f64,
    /// The speed the motor drives the length at, in m/s.
    pub motor_speed: f64,
}

impl DistanceJoint {
    /// A rigid rod of `length`.
    #[must_use]
    pub const fn rigid(length: f64) -> Self {
        Self {
            length,
            enable_spring: false,
            spring: Spring {
                hertz: 0.0,
                damping_ratio: 0.0,
            },
            enable_limit: false,
            min_length: 0.0,
            max_length: f64::INFINITY,
            enable_motor: false,
            max_motor_force: 0.0,
            motor_speed: 0.0,
        }
    }

    /// A rope of `length`: it goes slack when shorter and holds at its
    /// length — Box2D's rope, a spring of no stiffness with a limit.
    #[must_use]
    pub const fn rope(length: f64) -> Self {
        Self {
            enable_spring: true,
            enable_limit: true,
            min_length: 0.0,
            max_length: length,
            ..Self::rigid(length)
        }
    }

    /// A spring of `length` at `hertz` and `damping_ratio`.
    #[must_use]
    pub const fn spring(length: f64, hertz: f64, damping_ratio: f64) -> Self {
        Self {
            enable_spring: true,
            spring: Spring {
                hertz,
                damping_ratio,
            },
            ..Self::rigid(length)
        }
    }

    /// This joint, kept between `min` and `max` metres.
    #[must_use]
    pub const fn with_limits(self, min: f64, max: f64) -> Self {
        Self {
            enable_limit: true,
            min_length: min,
            max_length: max,
            ..self
        }
    }

    /// This joint, its length driven at `speed` m/s by up to `max_force`
    /// newtons.
    #[must_use]
    pub const fn with_motor(self, speed: f64, max_force: f64) -> Self {
        Self {
            enable_motor: true,
            motor_speed: speed,
            max_motor_force: max_force,
            ..self
        }
    }
}

/// A revolute joint, a hinge about the frames' z-axes: Box3D's
/// `b3RevoluteJointDef`.
///
/// Its angle is frame B's turn about frame A's z-axis, zero where the frames
/// agree, in `[-π, π]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RevoluteJoint {
    /// Whether the spring pulls the angle towards `target_angle`.
    pub enable_spring: bool,
    /// The spring.
    pub spring: Spring,
    /// The angle the spring pulls towards, in radians.
    pub target_angle: f64,
    /// Whether the angle is kept between the limits.
    pub enable_limit: bool,
    /// The least angle, in radians, at least −π.
    pub lower_angle: f64,
    /// The greatest angle, in radians, at most π.
    pub upper_angle: f64,
    /// Whether the motor drives the angle.
    pub enable_motor: bool,
    /// The most torque the motor applies, in newton-metres.
    pub max_motor_torque: f64,
    /// The speed it drives at, in rad/s.
    pub motor_speed: f64,
}

impl RevoluteJoint {
    /// A free hinge.
    #[must_use]
    pub const fn hinge() -> Self {
        Self {
            enable_spring: false,
            spring: Spring {
                hertz: 0.0,
                damping_ratio: 0.0,
            },
            target_angle: 0.0,
            enable_limit: false,
            lower_angle: 0.0,
            upper_angle: 0.0,
            enable_motor: false,
            max_motor_torque: 0.0,
            motor_speed: 0.0,
        }
    }

    /// This hinge, kept between `lower` and `upper` radians.
    #[must_use]
    pub const fn with_limits(self, lower: f64, upper: f64) -> Self {
        Self {
            enable_limit: true,
            lower_angle: lower,
            upper_angle: upper,
            ..self
        }
    }

    /// This hinge, driven at `speed` rad/s by up to `max_torque`.
    #[must_use]
    pub const fn with_motor(self, speed: f64, max_torque: f64) -> Self {
        Self {
            enable_motor: true,
            motor_speed: speed,
            max_motor_torque: max_torque,
            ..self
        }
    }

    /// This hinge, pulled towards `target` radians by `spring`.
    #[must_use]
    pub const fn with_spring(self, target: f64, spring: Spring) -> Self {
        Self {
            enable_spring: true,
            spring,
            target_angle: target,
            ..self
        }
    }
}

/// A prismatic joint, a slider along frame A's x-axis with the bodies'
/// orientation held: Box3D's `b3PrismaticJointDef`.
///
/// Its translation is how far frame B's origin is from frame A's along that
/// axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrismaticJoint {
    /// Whether the spring pulls the translation towards `target_translation`.
    pub enable_spring: bool,
    /// The spring.
    pub spring: Spring,
    /// The translation the spring pulls towards, in metres.
    pub target_translation: f64,
    /// Whether the translation is kept between the limits.
    pub enable_limit: bool,
    /// The least translation, in metres.
    pub lower_translation: f64,
    /// The greatest translation, in metres.
    pub upper_translation: f64,
    /// Whether the motor drives the translation.
    pub enable_motor: bool,
    /// The most force the motor applies, in newtons.
    pub max_motor_force: f64,
    /// The speed it drives at, in m/s.
    pub motor_speed: f64,
}

impl PrismaticJoint {
    /// A free slider.
    #[must_use]
    pub const fn slider() -> Self {
        Self {
            enable_spring: false,
            spring: Spring {
                hertz: 0.0,
                damping_ratio: 0.0,
            },
            target_translation: 0.0,
            enable_limit: false,
            lower_translation: 0.0,
            upper_translation: 0.0,
            enable_motor: false,
            max_motor_force: 0.0,
            motor_speed: 0.0,
        }
    }

    /// This slider, kept between `lower` and `upper` metres.
    #[must_use]
    pub const fn with_limits(self, lower: f64, upper: f64) -> Self {
        Self {
            enable_limit: true,
            lower_translation: lower,
            upper_translation: upper,
            ..self
        }
    }

    /// This slider, driven at `speed` m/s by up to `max_force` newtons.
    #[must_use]
    pub const fn with_motor(self, speed: f64, max_force: f64) -> Self {
        Self {
            enable_motor: true,
            motor_speed: speed,
            max_motor_force: max_force,
            ..self
        }
    }

    /// This slider, pulled towards `target` metres by `spring`.
    #[must_use]
    pub const fn with_spring(self, target: f64, spring: Spring) -> Self {
        Self {
            enable_spring: true,
            spring,
            target_translation: target,
            ..self
        }
    }
}

/// A weld joint, holding the frames together: Box3D's `b3WeldJointDef`.
///
/// A spring of zero hertz is rigid — the joint's own constraint stiffness —
/// and any other makes that half of the weld a spring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeldJoint {
    /// How the frames' origins are held together.
    pub linear: Spring,
    /// How their orientations are.
    pub angular: Spring,
}

impl WeldJoint {
    /// A rigid weld.
    #[must_use]
    pub const fn rigid() -> Self {
        let rigid = Spring {
            hertz: 0.0,
            damping_ratio: 0.0,
        };
        Self {
            linear: rigid,
            angular: rigid,
        }
    }
}

/// A spherical joint, a ball and socket: Box3D's `b3SphericalJointDef`
/// without its spring.
///
/// The **cone** bounds how far frame B's z-axis tips from frame A's; the
/// **twist** bounds frame B's turn about its own z-axis. Together they are
/// a ragdoll's shoulder or hip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SphericalJoint {
    /// Whether the cone limit is on.
    pub enable_cone_limit: bool,
    /// The cone's half-angle, in radians, in `[0, π]`.
    pub cone_angle: f64,
    /// Whether the twist limit is on.
    pub enable_twist_limit: bool,
    /// The least twist, in radians.
    pub lower_twist_angle: f64,
    /// The greatest twist, in radians.
    pub upper_twist_angle: f64,
    /// Whether the motor drives the relative angular velocity.
    pub enable_motor: bool,
    /// The most torque the motor applies, in newton-metres.
    pub max_motor_torque: f64,
    /// The relative angular velocity it drives towards, in rad/s, in the
    /// world.
    pub motor_velocity: DVec3,
}

impl SphericalJoint {
    /// A free ball and socket.
    #[must_use]
    pub const fn ball() -> Self {
        Self {
            enable_cone_limit: false,
            cone_angle: 0.0,
            enable_twist_limit: false,
            lower_twist_angle: 0.0,
            upper_twist_angle: 0.0,
            enable_motor: false,
            max_motor_torque: 0.0,
            motor_velocity: DVec3::ZERO,
        }
    }

    /// This joint, its frame B's z-axis kept within `angle` of frame A's.
    #[must_use]
    pub const fn with_cone(self, angle: f64) -> Self {
        Self {
            enable_cone_limit: true,
            cone_angle: angle,
            ..self
        }
    }

    /// This joint, its twist kept between `lower` and `upper` radians.
    #[must_use]
    pub const fn with_twist(self, lower: f64, upper: f64) -> Self {
        Self {
            enable_twist_limit: true,
            lower_twist_angle: lower,
            upper_twist_angle: upper,
            ..self
        }
    }

    /// This joint, driving the relative angular velocity towards `velocity`
    /// with up to `max_torque`: a friction joint, at zero velocity.
    #[must_use]
    pub const fn with_motor(self, velocity: DVec3, max_torque: f64) -> Self {
        Self {
            enable_motor: true,
            motor_velocity: velocity,
            max_motor_torque: max_torque,
            ..self
        }
    }
}

/// A joint that broke: the force or torque it carried reached its
/// threshold. Raised by the step it broke in, which took it out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointBreak {
    /// The joint, which no longer exists.
    pub joint: JointId,
    /// Its first body.
    pub body_a: Entity,
    /// Its second body.
    pub body_b: Entity,
    /// What it held when it broke.
    pub joint_def: Joint,
    /// The force it carried in the substep it broke, in newtons.
    pub force: f64,
    /// The torque it carried, in newton-metres.
    pub torque: f64,
}

/// Why [`crate::PhysicsSystem::add_joint`] refused a joint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointError {
    /// An entity that is not registered with the system.
    UnknownBody(Entity),
    /// Both ends are one body.
    SameBody,
    /// A setting out of range, and which.
    Invalid(&'static str),
}

impl core::fmt::Display for JointError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownBody(entity) => write!(f, "{entity:?} is not registered"),
            Self::SameBody => f.write_str("a joint joins two different bodies"),
            Self::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for JointError {}

/// How far a joint is from holding, as
/// [`crate::PhysicsSystem::joint_drift`] measures it: how far its positional
/// rows are broken, and by what angle its rotational ones are.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JointDrift {
    /// In metres.
    pub linear: f64,
    /// In radians.
    pub angular: f64,
}
