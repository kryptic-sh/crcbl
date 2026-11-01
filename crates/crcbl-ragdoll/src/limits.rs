//! A joint's limit settings beyond its hard range, shared by the rig and its
//! file: the soft zones inside the hard limits, and a ball's neutral turn.
//!
//! Every angle is in radians and measured from the joint's **neutral** —
//! the relative turn at which it meets no resistance, rest unless a joint
//! names another. A soft zone's strength is `crcbl_phys`'s soft-zone
//! [`Spring`]: free inside the soft bounds, resistance growing with the depth
//! squared past one, up to `hertz` at the hard limit.

use core::f64::consts::PI;

use crcbl_phys::{Spring, rotation_from_scaled_axis};
use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};

/// A soft zone on one axis: free between `lower` and `upper`, resisted from
/// each out to the hard limit on its side.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoftRange {
    /// Where the zone below begins, at least the hard lower limit.
    pub lower: f64,
    /// Where the zone above begins, at most the hard upper limit, not below
    /// `lower`.
    pub upper: f64,
    /// The zone's stiffness at the hard limit, as a frequency in hertz; zero
    /// is no resistance.
    pub hertz: f64,
    /// Its damping ratio: 1 is critical.
    pub damping_ratio: f64,
}

/// A ball's soft swing: free inside the ellipse of half-angles `x` about the
/// joint frame's x-axis and `y` about its y-axis, resisted out to the hard
/// swing limit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoftSwing {
    /// The half-angle about x, at most the hard one; zero only with `y`.
    pub x: f64,
    /// The half-angle about y, at most the hard one; zero only with `x`.
    pub y: f64,
    /// As [`SoftRange::hertz`].
    pub hertz: f64,
    /// As [`SoftRange::damping_ratio`].
    pub damping_ratio: f64,
}

/// A ball's neutral turn of the child from rest, in the joint's frame: a
/// swing whose rotation vector is `(swing_x, swing_y, 0)`, after a twist of
/// `twist` about the frame's z-axis — `crcbl_phys`'s swing-twist split. For a
/// hip whose frame's z runs down the thigh, a swing about x is flexion and
/// one about y abduction.
///
/// **The swing limit stays the parent's**: the neutral swing tips the
/// parent's frame, carrying the ellipse with it, and the neutral twist turns
/// the child's frame, so a neutral twist never turns the ellipse's axes —
/// flexion stays about the parent's x however the limb is rotated inwards.
/// See [`crate::RigJoint::physics_frames`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SwingTwist {
    /// The swing about the frame's x-axis.
    pub swing_x: f64,
    /// The swing about the frame's y-axis.
    pub swing_y: f64,
    /// The twist about the frame's z-axis.
    pub twist: f64,
}

impl SwingTwist {
    /// No turn: the neutral is the rest pose.
    pub const REST: Self = Self {
        swing_x: 0.0,
        swing_y: 0.0,
        twist: 0.0,
    };

    /// The swing part of the turn, built without the platform's
    /// trigonometry.
    #[must_use]
    pub fn swing(&self) -> DQuat {
        rotation_from_scaled_axis(DVec3::new(self.swing_x, self.swing_y, 0.0))
    }

    /// The twist part, about z, likewise. The whole turn is
    /// `swing() * twist()`.
    #[must_use]
    pub fn twist(&self) -> DQuat {
        rotation_from_scaled_axis(DVec3::Z * self.twist)
    }

    pub(crate) fn is_valid(&self) -> bool {
        let swing = (self.swing_x * self.swing_x + self.swing_y * self.swing_y).sqrt();
        swing.is_finite() && swing <= PI && angle(self.twist)
    }
}

impl SoftRange {
    /// The zone's strength as the physics joint takes it.
    #[must_use]
    pub const fn zone(&self) -> Spring {
        spring(self.hertz, self.damping_ratio)
    }

    /// Whether the zone lies inside `[lower, upper]`, lower first, with a
    /// strength the physics joint takes.
    pub(crate) fn is_valid_within(&self, lower: f64, upper: f64) -> bool {
        self.lower.is_finite()
            && self.upper.is_finite()
            && lower <= self.lower
            && self.lower <= self.upper
            && self.upper <= upper
            && is_strength(self.hertz, self.damping_ratio)
    }
}

impl SoftSwing {
    /// As [`SoftRange::zone`].
    #[must_use]
    pub const fn zone(&self) -> Spring {
        spring(self.hertz, self.damping_ratio)
    }

    /// Whether the soft ellipse lies inside the hard one of half-angles `x`
    /// and `y`, with no lone zero half-angle and a strength the physics joint
    /// takes.
    pub(crate) fn is_valid_within(&self, x: f64, y: f64) -> bool {
        (0.0..=x).contains(&self.x)
            && (0.0..=y).contains(&self.y)
            && both_or_neither_zero(self.x, self.y)
            && is_strength(self.hertz, self.damping_ratio)
    }
}

const fn spring(hertz: f64, damping_ratio: f64) -> Spring {
    Spring {
        hertz,
        damping_ratio,
    }
}

fn is_strength(hertz: f64, damping_ratio: f64) -> bool {
    hertz.is_finite() && hertz >= 0.0 && damping_ratio.is_finite() && damping_ratio >= 0.0
}

/// Whether `value` is a finite angle in `[-π, π]`.
pub(crate) fn angle(value: f64) -> bool {
    value.is_finite() && (-PI..=PI).contains(&value)
}

/// Whether two swing half-angles are both zero or neither: one zero makes
/// the physics joint's ellipse a segment, which it refuses.
pub(crate) fn both_or_neither_zero(x: f64, y: f64) -> bool {
    (x == 0.0) == (y == 0.0)
}
