//! The authored rig: a hand-edited RON file naming bones, resolved against a
//! skeleton's joint names into a [`RagdollRig`].
//!
//! ```ron
//! (
//!     bodies: [
//!         (bone: "pelvis", shape: Sphere(radius: 0.12), mass: 12.0),
//!         (
//!             bone: "thigh_l",
//!             shape: Capsule(radius: 0.07, half_height: 0.15),
//!             mass: 8.0,
//!             centre: (0.0, -0.22, 0.0),
//!             axis: (0.0, 1.0, 0.0),
//!         ),
//!     ],
//!     joints: [
//!         (
//!             parent: "pelvis",
//!             child: "thigh_l",
//!             kind: Ball(cone: 1.2, twist_lower: -0.4, twist_upper: 0.4),
//!             axis: (0.0, -1.0, 0.0),
//!         ),
//!     ],
//!     ignore: [],
//!     substeps: 8,
//! )
//! ```
//!
//! A body's `centre` and `axis` are in its bone's rigid frame, in metres:
//! the capsule's or box's `y` lies along `axis`. A joint sits at its child
//! bone's origin, moved by `at` in that bone's frame, and its `axis` — the
//! cone's or the hinge's — is in the same frame. Limits are measured from
//! the rest pose. `ignore` names bone pairs whose bodies never collide;
//! pairs overlapping at rest are added unless `ignore_rest_overlaps` is
//! `false`.

use std::collections::HashMap;

use crcbl_anim::{Pose, Skeleton};
use crcbl_phys::Transform;
use glam::{DVec3, Mat4};
use serde::{Deserialize, Serialize};

use crate::frames::{body_worlds, overlapping_pairs, rigid_frames};
use crate::rig::{
    BodyShape, RagdollRig, RigBody, RigError, RigJoint, RigJointKind, compose, inverse,
    rotation_onto,
};

/// A body's shape in the file.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ShapeDesc {
    /// A capsule along the body's axis.
    Capsule {
        /// In metres.
        radius: f64,
        /// Half the cylinder's length, in metres.
        half_height: f64,
    },
    /// A sphere.
    Sphere {
        /// In metres.
        radius: f64,
    },
    /// A box, its `y` half-extent along the body's axis.
    Box {
        /// In metres.
        half_extents: (f64, f64, f64),
    },
}

/// One body in the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyDesc {
    /// The bone it drives.
    pub bone: String,
    /// Its shape.
    pub shape: ShapeDesc,
    /// In kilograms.
    pub mass: f64,
    /// Its centre in the bone's rigid frame, in metres. The bone's origin if
    /// left out.
    #[serde(default)]
    pub centre: (f64, f64, f64),
    /// The direction of its shape's `y` in the bone's frame. The bone's own
    /// `y` if left out.
    #[serde(default = "up")]
    pub axis: (f64, f64, f64),
}

/// What a joint in the file lets through, measured from rest.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum JointKindDesc {
    /// A ball and socket with a cone and a twist range, in radians.
    Ball {
        /// The cone's half-angle.
        cone: f64,
        /// The least twist.
        twist_lower: f64,
        /// The greatest twist.
        twist_upper: f64,
    },
    /// A hinge bending between two angles, in radians.
    Hinge {
        /// The least angle.
        lower: f64,
        /// The greatest angle.
        upper: f64,
    },
    /// Welded.
    Fixed,
}

/// One joint in the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JointDesc {
    /// The parent body's bone.
    pub parent: String,
    /// The child body's bone, at whose origin the joint sits.
    pub child: String,
    /// What it lets through.
    pub kind: JointKindDesc,
    /// The cone or hinge axis in the child bone's rigid frame.
    pub axis: (f64, f64, f64),
    /// Where it sits in the child bone's frame, in metres. The bone's origin
    /// if left out.
    #[serde(default)]
    pub at: (f64, f64, f64),
    /// The most torque it resists turning with, in newton-metres. None if
    /// left out.
    #[serde(default)]
    pub friction: f64,
}

/// An authored ragdoll rig, as read from its file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RagdollDesc {
    /// The bodies, one per bone at most.
    pub bodies: Vec<BodyDesc>,
    /// The joints between them.
    #[serde(default)]
    pub joints: Vec<JointDesc>,
    /// Bone pairs whose bodies never collide.
    #[serde(default)]
    pub ignore: Vec<(String, String)>,
    /// Whether pairs overlapping at rest are ignored as well. On if left out.
    #[serde(default = "yes")]
    pub ignore_rest_overlaps: bool,
    /// The solver substeps the bodies ask for; zero, or left out, for the
    /// system's.
    #[serde(default)]
    pub substeps: u32,
}

const fn up() -> (f64, f64, f64) {
    (0.0, 1.0, 0.0)
}

const fn yes() -> bool {
    true
}

/// Why a file is not a [`RagdollRig`] for a skeleton.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DescError {
    /// The text is not RON of this shape.
    #[error("ragdoll file, line {line}, column {column}: {message}")]
    Parse {
        /// Where ron stopped, 1-based.
        line: usize,
        /// See `line`.
        column: usize,
        /// What ron said.
        message: String,
    },
    /// The names list is not one name per skeleton joint.
    #[error("{names} bone names given for a skeleton of {joints} joints")]
    NameCount {
        /// How many names.
        names: usize,
        /// How many joints.
        joints: usize,
    },
    /// A bone the file names is not in the skeleton.
    #[error("no bone named {0:?}")]
    UnknownBone(String),
    /// A bone the file names is in the skeleton more than once.
    #[error("more than one bone is named {0:?}")]
    AmbiguousBone(String),
    /// A joint or pair names a bone with no body.
    #[error("bone {0:?} carries no body")]
    NoBody(String),
    /// An axis has no direction.
    #[error("an axis for bone {0:?} has no direction")]
    ZeroAxis(String),
    /// The skeleton's rest pose leaves a joint with no usable frame.
    #[error("skeleton joint {joint} has no usable rest frame")]
    InvalidFrame {
        /// The joint.
        joint: usize,
    },
    /// The resolved rig was refused.
    #[error(transparent)]
    Rig(#[from] RigError),
}

impl RagdollDesc {
    /// Reads a rig file.
    ///
    /// # Errors
    ///
    /// [`DescError::Parse`] for text that is not RON of this shape.
    pub fn from_ron(text: &str) -> Result<Self, DescError> {
        ron::from_str(text).map_err(|error| DescError::Parse {
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        })
    }

    /// Resolves the file against `skeleton`, whose joint `i` is named
    /// `names[i]`, at the scale `model` gives it — the scale it will be
    /// spawned at.
    ///
    /// # Errors
    ///
    /// [`DescError`] names the first bone, axis or setting at fault.
    pub fn resolve(
        &self,
        skeleton: &Skeleton,
        names: &[&str],
        model: Mat4,
    ) -> Result<RagdollRig, DescError> {
        if names.len() != skeleton.len() {
            return Err(DescError::NameCount {
                names: names.len(),
                joints: skeleton.len(),
            });
        }
        let mut by_name: HashMap<&str, Option<usize>> = HashMap::new();
        for (index, name) in names.iter().enumerate() {
            by_name
                .entry(name)
                .and_modify(|slot| *slot = None)
                .or_insert(Some(index));
        }
        let bone = |name: &str| match by_name.get(name) {
            Some(Some(index)) => Ok(*index),
            Some(None) => Err(DescError::AmbiguousBone(name.to_owned())),
            None => Err(DescError::UnknownBone(name.to_owned())),
        };
        let frames = rigid_frames(skeleton, &Pose::new(skeleton), model)
            .map_err(|joint| DescError::InvalidFrame { joint })?;

        let mut bodies = Vec::with_capacity(self.bodies.len());
        for desc in &self.bodies {
            let rotation = rotation_onto(DVec3::Y, DVec3::from(desc.axis))
                .ok_or_else(|| DescError::ZeroAxis(desc.bone.clone()))?;
            bodies.push(RigBody {
                joint: bone(&desc.bone)?,
                shape: desc.shape.shape(),
                mass: desc.mass,
                offset: Transform::new(DVec3::from(desc.centre), rotation),
            });
        }
        let body_of = |name: &str| {
            let joint = bone(name)?;
            bodies
                .iter()
                .position(|body| body.joint == joint)
                .ok_or_else(|| DescError::NoBody(name.to_owned()))
        };
        let worlds = body_worlds(&bodies, &frames);
        let mut joints = Vec::with_capacity(self.joints.len());
        for desc in &self.joints {
            let (parent, child) = (body_of(&desc.parent)?, body_of(&desc.child)?);
            let rotation = rotation_onto(DVec3::Z, DVec3::from(desc.axis))
                .ok_or_else(|| DescError::ZeroAxis(desc.child.clone()))?;
            let local = Transform::new(DVec3::from(desc.at), rotation);
            let frame = compose(&frames[bodies[child].joint], &local);
            joints.push(RigJoint {
                parent,
                child,
                frame_parent: compose(&inverse(&worlds[parent]), &frame),
                frame_child: compose(&inverse(&worlds[child]), &frame),
                kind: desc.kind.kind(),
                friction: desc.friction,
            });
        }
        let mut ignored = Vec::with_capacity(self.ignore.len());
        for (a, b) in &self.ignore {
            ignored.push((body_of(a)?, body_of(b)?));
        }
        // Validated before the overlap test, which places the shapes.
        let rig = RagdollRig::new(skeleton.len(), bodies, joints, ignored, self.substeps)?;
        if !self.ignore_rest_overlaps {
            return Ok(rig);
        }
        let mut ignored = rig.ignored().to_vec();
        for pair in overlapping_pairs(rig.bodies(), rig.joints(), &worlds) {
            let listed = ignored
                .iter()
                .any(|&(a, b)| (a, b) == pair || (b, a) == pair);
            if !listed {
                ignored.push(pair);
            }
        }
        Ok(RagdollRig::new(
            skeleton.len(),
            rig.bodies().to_vec(),
            rig.joints().to_vec(),
            ignored,
            self.substeps,
        )?)
    }
}

impl ShapeDesc {
    fn shape(self) -> BodyShape {
        match self {
            Self::Capsule {
                radius,
                half_height,
            } => BodyShape::Capsule {
                radius,
                half_height,
            },
            Self::Sphere { radius } => BodyShape::Sphere { radius },
            Self::Box {
                half_extents: (x, y, z),
            } => BodyShape::Box {
                half_extents: DVec3::new(x, y, z),
            },
        }
    }
}

impl JointKindDesc {
    const fn kind(self) -> RigJointKind {
        match self {
            Self::Ball {
                cone,
                twist_lower,
                twist_upper,
            } => RigJointKind::Ball {
                cone,
                twist_lower,
                twist_upper,
            },
            Self::Hinge { lower, upper } => RigJointKind::Hinge { lower, upper },
            Self::Fixed => RigJointKind::Fixed,
        }
    }
}
