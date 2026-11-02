//! The fallback rig: bodies fitted to a skeleton's bones, for a character
//! nobody has authored a rig for yet.
//!
//! In the rest pose, placed by the model transform:
//!
//! - a joint with **one** child at least [`GenerateSettings::min_length`]
//!   away carries a capsule from itself to that child, its radius
//!   [`GenerateSettings::radius_ratio`] of the bone's length (a sphere if the
//!   bone is too short for the caps);
//! - a joint with **several** children — a pelvis, a chest — carries a
//!   sphere at the middle of itself and them, its radius half their mean
//!   distance, however short they are, so the subtrees stay joined;
//! - a joint with **none** — a hand, a foot, a head with no end bone — carries
//!   a sphere just beyond itself, sized from its own bone;
//! - a joint closer than that to its parent, and a childless root, carries
//!   nothing and follows its parent.
//!
//! Every body hangs from the nearest ancestor that has one by a ball joint at
//! its own skeleton joint, its cone about the bone's direction, resisting
//! turning with [`GenerateSettings::joint_friction`]. Mass is shared out by
//! volume, then evened until no body is more than
//! [`GenerateSettings::max_mass_ratio`] times a body it is jointed to: a pelvis
//! sphere a hundredth the mass of the thighs hanging from it is what makes a
//! joint solver throw a ragdoll apart. Without
//! [`GenerateSettings::anatomy`] joint types are not inferred from names: an
//! elbow is a ball, and a hinge only in an authored rig. With it, the
//! [`Humanoid`] profile makes the joints it can name anatomical — knees and
//! elbows hinges, their limits and neutrals from anatomical ranges and NASA's
//! neutral posture — and folds its helper bones into their parents. Bodies
//! whose rest shapes overlap without a joint between them are kept from
//! colliding.

use crcbl_anim::{Pose, Skeleton};
use crcbl_phys::Transform;
use glam::{DVec3, Mat4};

use crate::anatomy::{Humanoid, Profile};
use crate::frames::{body_worlds, overlapping_pairs, rigid_frames};
use crate::limits::SwingTwist;
use crate::rig::{
    BodyShape, RagdollRig, RigBody, RigError, RigJoint, RigJointKind, compose, inverse,
    rotation_onto,
};

/// How [`RagdollRig::generate`] fits its bodies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GenerateSettings {
    /// The whole ragdoll's mass, in kilograms.
    pub total_mass: f64,
    /// A capsule's radius as a fraction of its bone's length.
    pub radius_ratio: f64,
    /// Bones shorter than this, in metres, carry no body of their own.
    pub min_length: f64,
    /// Every ball joint's cone half-angle, in radians.
    pub cone: f64,
    /// Every ball joint's twist either way, in radians.
    pub twist: f64,
    /// Every joint's friction, in newton-metres per kilogram of the lighter
    /// body it joins.
    pub joint_friction: f64,
    /// The most one body may weigh against a body it is jointed to.
    pub max_mass_ratio: f64,
    /// The solver substeps the bodies ask for; zero for the system's.
    pub substeps: u32,
    /// The anatomical profile for a humanoid's joints, if any: off, every
    /// joint is the ball `cone` and `twist` describe and every bone long
    /// enough carries a body.
    pub anatomy: Option<Humanoid>,
}

impl GenerateSettings {
    /// An adult's mass, slim limbs, and limits loose enough to fall in.
    ///
    /// Sixteen substeps, measured on 2026-10-12 over 24 head strikes of up to
    /// 40 N·s in three directions on the test humanoid: at eight, a third
    /// never came to rest in 40 s — a light limb held trembling at a joint
    /// limit by the heavier one hanging off it — at twelve one did, and at
    /// sixteen none, settling in half the time.
    pub const DEFAULT: Self = Self {
        total_mass: 80.0,
        radius_ratio: 0.2,
        min_length: 0.04,
        cone: 0.7,
        twist: 0.5,
        joint_friction: 0.1,
        max_mass_ratio: 4.0,
        substeps: 16,
        anatomy: None,
    };

    fn is_valid(&self) -> bool {
        use core::f64::consts::PI;
        let positive = |value: f64| value.is_finite() && value > 0.0;
        positive(self.total_mass)
            && positive(self.radius_ratio)
            && self.radius_ratio < 0.5
            && positive(self.min_length)
            && self.cone.is_finite()
            && (0.0..=PI).contains(&self.cone)
            && self.twist.is_finite()
            && (0.0..=PI).contains(&self.twist)
            && self.joint_friction.is_finite()
            && self.joint_friction >= 0.0
            && self.max_mass_ratio.is_finite()
            && self.max_mass_ratio >= 1.0
    }
}

impl Default for GenerateSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why [`RagdollRig::generate`] made no rig.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GenerateError {
    /// A setting is out of range.
    #[error("ragdoll generation settings are out of range")]
    InvalidSettings,
    /// The names given are not one per skeleton joint.
    #[error("{names} bone names for a skeleton of {joints} joints")]
    NameCount {
        /// How many names were given.
        names: usize,
        /// How many joints the skeleton has.
        joints: usize,
    },
    /// The model or a rest transform leaves a joint with no usable frame.
    #[error("skeleton joint {joint} has no usable rest frame")]
    InvalidFrame {
        /// The joint.
        joint: usize,
    },
    /// The result was refused; with valid settings this means no bone was
    /// long enough to carry a body.
    #[error(transparent)]
    Rig(#[from] RigError),
}

/// A fitted body before its mass is known: its shape, its world pose at
/// rest, and the direction its joint's cone is about.
struct Fitted {
    joint: usize,
    shape: BodyShape,
    world: Transform,
    axis: DVec3,
}

impl RagdollRig {
    /// Fits a rig to `skeleton`'s rest pose, placed at the scale `model`
    /// gives it: see the module docs for the rules. `names` are the joints'
    /// names in palette order, which only [`GenerateSettings::anatomy`]
    /// reads.
    ///
    /// # Errors
    ///
    /// [`GenerateError`] for settings out of range, names that are not one
    /// per joint, a rest pose with no usable frame, or a skeleton with no
    /// bone long enough for a body.
    pub fn generate(
        skeleton: &Skeleton,
        names: &[&str],
        model: Mat4,
        settings: &GenerateSettings,
    ) -> Result<Self, GenerateError> {
        if !settings.is_valid() {
            return Err(GenerateError::InvalidSettings);
        }
        if names.len() != skeleton.len() {
            return Err(GenerateError::NameCount {
                names: names.len(),
                joints: skeleton.len(),
            });
        }
        let frames = rigid_frames(skeleton, &Pose::new(skeleton), model)
            .map_err(|joint| GenerateError::InvalidFrame { joint })?;
        let joints = skeleton.joints();
        let at = |joint: usize| frames[joint].position;
        let mut profile = settings.anatomy.map(|humanoid| {
            let parents: Vec<Option<usize>> = joints.iter().map(|joint| joint.parent).collect();
            Profile::new(&humanoid, names, &parents, &at)
        });
        let folded = |joint: usize| profile.as_ref().is_some_and(|p| p.is_folded(joint));
        // Each bone is fitted against its nearest ancestor the profile
        // leaves standing: its parent, without one.
        let kept_parent = |joint: usize| {
            profile
                .as_ref()
                .map_or(joints[joint].parent, |p| p.kept_parent(joint))
        };
        let mut children = vec![Vec::new(); joints.len()];
        for index in (0..joints.len()).filter(|&joint| !folded(joint)) {
            if let Some(parent) = kept_parent(index) {
                children[parent].push(index);
            }
        }
        let fitted: Vec<Fitted> = (0..joints.len())
            .filter(|&joint| !folded(joint))
            .filter_map(|joint| {
                // A branching joint keeps every child however short: its body
                // is what joins the subtrees, and without one a coarse rig's
                // legs would hang from nothing.
                let reaching = |child: usize| {
                    let length = at(child).distance(at(joint));
                    length >= settings.min_length || (children[joint].len() > 1 && length > 0.0)
                };
                let long: Vec<usize> = children[joint]
                    .iter()
                    .copied()
                    .filter(|&child| reaching(child))
                    .collect();
                let from_parent = kept_parent(joint).map(|parent| at(joint) - at(parent));
                // A head whose face bones are folded is sized from them.
                let head = if long.is_empty() {
                    profile.as_ref().and_then(|p| p.head_sphere(joint, &at))
                } else {
                    None
                };
                if let Some((centre, radius)) = head {
                    return Some(Fitted {
                        joint,
                        shape: BodyShape::Sphere { radius },
                        world: Transform::from_position(centre),
                        axis: (centre - at(joint)) / radius,
                    });
                }
                fit(joint, &long, from_parent, &at, settings)
            })
            .collect();
        let mut body_of = vec![None; joints.len()];
        for (index, body) in fitted.iter().enumerate() {
            body_of[body.joint] = Some(index);
        }
        if let Some(profile) = &mut profile {
            profile.count_bodies(fitted.iter().map(|body| body.joint));
        }
        let mut rig_joints = Vec::new();
        for (child, body) in fitted.iter().enumerate() {
            let mut ancestor = joints[body.joint].parent;
            while let Some(joint) = ancestor {
                if body_of[joint].is_some() {
                    break;
                }
                ancestor = joints[joint].parent;
            }
            let Some(parent) = ancestor.and_then(|joint| body_of[joint]) else {
                continue;
            };
            let profiled = profile
                .as_ref()
                .and_then(|profile| profile.joint(body.joint, body.axis, &at));
            let (rotation, kind) = profiled.unwrap_or_else(|| {
                (
                    rotation_onto(DVec3::Z, body.axis).unwrap_or_default(),
                    RigJointKind::Ball {
                        swing_x: settings.cone,
                        swing_y: settings.cone,
                        twist_lower: -settings.twist,
                        twist_upper: settings.twist,
                        neutral: SwingTwist::REST,
                        soft_swing: None,
                        soft_twist: None,
                    },
                )
            });
            let frame = Transform::new(at(body.joint), rotation);
            rig_joints.push(RigJoint {
                parent,
                child,
                frame_parent: compose(&inverse(&fitted[parent].world), &frame),
                frame_child: compose(&inverse(&body.world), &frame),
                kind,
                friction: 0.0,
            });
        }
        let masses = share_mass(&fitted, &rig_joints, settings);
        for joint in &mut rig_joints {
            joint.friction =
                settings.joint_friction * masses[joint.parent].min(masses[joint.child]);
        }
        let bodies: Vec<RigBody> = fitted
            .iter()
            .zip(masses)
            .map(|(body, mass)| RigBody {
                joint: body.joint,
                shape: body.shape,
                mass,
                offset: compose(&inverse(&frames[body.joint]), &body.world),
            })
            .collect();
        let worlds = body_worlds(&bodies, &frames);
        let ignored = overlapping_pairs(&bodies, &rig_joints, &worlds);
        Ok(Self::new(
            joints.len(),
            bodies,
            rig_joints,
            ignored,
            settings.substeps,
        )?)
    }
}

/// The body for `joint`, whose children at least the minimum length away are
/// `long`, and which sits `from_parent` from its parent, if it has one.
fn fit(
    joint: usize,
    long: &[usize],
    from_parent: Option<DVec3>,
    at: &dyn Fn(usize) -> DVec3,
    settings: &GenerateSettings,
) -> Option<Fitted> {
    let origin = at(joint);
    let parent_axis = from_parent.and_then(DVec3::try_normalize);
    match long {
        [] => {
            let bone = from_parent?;
            let length = bone.length();
            if length < settings.min_length {
                return None;
            }
            let axis = parent_axis?;
            let radius = settings.radius_ratio * length;
            Some(Fitted {
                joint,
                shape: BodyShape::Sphere { radius },
                world: Transform::from_position(origin + axis * radius),
                axis,
            })
        }
        [child] => {
            let bone = at(*child) - origin;
            let length = bone.length();
            let radius = settings.radius_ratio * length;
            let half_height = 0.5 * length - radius;
            let rotation = rotation_onto(DVec3::Y, bone)?;
            let shape = if half_height > 0.0 {
                BodyShape::Capsule {
                    radius,
                    half_height,
                }
            } else {
                BodyShape::Sphere {
                    radius: 0.5 * length,
                }
            };
            Some(Fitted {
                joint,
                shape,
                world: Transform::new(origin + 0.5 * bone, rotation),
                axis: bone / length,
            })
        }
        several => {
            let count = several.len() as f64;
            let centre = (origin + several.iter().map(|&c| at(c)).sum::<DVec3>()) / (count + 1.0);
            let reach = several.iter().map(|&c| at(c).distance(origin)).sum::<f64>() / count;
            Some(Fitted {
                joint,
                shape: BodyShape::Sphere {
                    radius: 0.5 * reach,
                },
                world: Transform::from_position(centre),
                axis: parent_axis.unwrap_or(DVec3::Y),
            })
        }
    }
}

/// Each body's mass: by volume, every body then raised to at least
/// `1 / max_mass_ratio` of the heaviest body jointed to it, and the whole
/// scaled back to the total.
///
/// Raising a body only ever raises it towards a heavier neighbour, so each
/// pass leaves every mass at most the heaviest, and the passes stop once
/// nothing moves; the final uniform scale keeps every ratio.
fn share_mass(fitted: &[Fitted], joints: &[RigJoint], settings: &GenerateSettings) -> Vec<f64> {
    let mut masses: Vec<f64> = fitted.iter().map(|body| body.shape.volume()).collect();
    loop {
        let mut raised = false;
        for joint in joints {
            let (a, b) = (joint.parent, joint.child);
            let (light, heavy) = if masses[a] < masses[b] {
                (a, b)
            } else {
                (b, a)
            };
            let floor = masses[heavy] / settings.max_mass_ratio;
            if masses[light] < floor {
                masses[light] = floor;
                raised = true;
            }
        }
        if !raised {
            break;
        }
    }
    let total: f64 = masses.iter().sum();
    masses
        .iter()
        .map(|mass| settings.total_mass * mass / total)
        .collect()
}
