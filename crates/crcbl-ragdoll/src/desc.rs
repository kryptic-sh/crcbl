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
//!             kind: Ball(
//!                 swing_x: 1.2,
//!                 swing_y: 0.8,
//!                 twist_lower: -0.4,
//!                 twist_upper: 0.4,
//!                 neutral: (swing_x: 0.6),
//!                 soft_swing: (x: 0.9, y: 0.5, hertz: 4.0, damping_ratio: 0.7),
//!             ),
//!             axis: (0.0, -1.0, 0.0),
//!         ),
//!     ],
//!     ignore: [],
//!     fold: ["ik_foot_l"],
//!     substeps: 8,
//! )
//! ```
//!
//! A body's `centre` and `axis` are in its bone's rigid frame, in metres:
//! the capsule's or box's `y` lies along `axis`. A joint sits at its child
//! bone's origin, moved by `at` in that bone's frame, and its `axis` — the
//! swing's or the hinge's, its frame's `z` — is in the same frame.
//!
//! **A joint frame's `x`**, which a ball's `swing_x` is about and its `y` its
//! `swing_y`, is `swing_x_axis` made square to `axis` when the file gives
//! one. Without it the frame is the shortest turn taking the bone's `z` onto
//! `axis`, so `x` is the bone's `x` tipped along with it: the bone's `x`
//! itself for an `axis` of `z`, and `(0, 0, -1)` for an `axis` of `x`. An
//! `axis` of exactly `-z` has no shortest turn, and takes the half turn
//! about the bone's `y`, so `x` is `-x`. Limits
//! are measured from the joint's `neutral`, the rest pose if it names none
//! ([`JointKindDesc`]). `ignore` names bone pairs whose bodies never
//! collide; pairs overlapping at rest are added unless
//! `ignore_rest_overlaps` is `false`. `fold` names bones that carry no body
//! and move rigidly with their parents.

use std::collections::HashMap;

use crcbl_anim::{Pose, Skeleton};
use crcbl_phys::Transform;
use glam::{DVec3, Mat4};
use serde::{Deserialize, Serialize};

use crate::frames::{body_worlds, overlapping_pairs, rigid_frames};
use crate::limits::{SoftRange, SoftSwing, SwingTwist};
use crate::rig::{
    BodyShape, RagdollRig, RigBody, RigError, RigJoint, RigJointKind, compose, has_default_roll,
    inverse, joint_rotation, rotation_onto,
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

/// What a joint in the file lets through, in radians, measured from the
/// joint's neutral — rest unless it names another.
///
/// Older files gave a ball one `cone` half-angle; one still reads, as both
/// `swing_x` and `swing_y`, and a file is written with the two. The optional
/// fields are written bare, `soft: (lower: -0.2, ...)`, and left out when
/// absent.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "JointKindFile")]
pub enum JointKindDesc {
    /// A ball and socket with an elliptical swing and a twist range.
    Ball {
        /// The swing's half-angle about the joint frame's x-axis.
        swing_x: f64,
        /// The swing's half-angle about its y-axis.
        swing_y: f64,
        /// The least twist.
        twist_lower: f64,
        /// The greatest twist.
        twist_upper: f64,
        /// The child's neutral turn from rest. Rest if left out.
        #[serde(skip_serializing_if = "is_default")]
        neutral: SwingTwist,
        /// Resistance past a softer swing ellipse. None if left out.
        #[serde(skip_serializing_if = "Option::is_none", with = "bare")]
        soft_swing: Option<SoftSwing>,
        /// Resistance past soft twist bounds. None if left out.
        #[serde(skip_serializing_if = "Option::is_none", with = "bare")]
        soft_twist: Option<SoftRange>,
    },
    /// A hinge bending between two angles.
    Hinge {
        /// The least angle.
        lower: f64,
        /// The greatest angle.
        upper: f64,
        /// The child's neutral bend from rest about the axis. Rest if left
        /// out.
        #[serde(skip_serializing_if = "is_default")]
        neutral: f64,
        /// Resistance past soft bounds. None if left out.
        #[serde(skip_serializing_if = "Option::is_none", with = "bare")]
        soft: Option<SoftRange>,
    },
    /// Welded.
    Fixed,
}

/// [`JointKindDesc`] as the file may spell it: a ball's swing as one `cone`
/// or as its two half-angles.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum JointKindFile {
    Ball {
        #[serde(default, with = "bare")]
        cone: Option<f64>,
        #[serde(default, with = "bare")]
        swing_x: Option<f64>,
        #[serde(default, with = "bare")]
        swing_y: Option<f64>,
        twist_lower: f64,
        twist_upper: f64,
        #[serde(default)]
        neutral: SwingTwist,
        #[serde(default, with = "bare")]
        soft_swing: Option<SoftSwing>,
        #[serde(default, with = "bare")]
        soft_twist: Option<SoftRange>,
    },
    Hinge {
        lower: f64,
        upper: f64,
        #[serde(default)]
        neutral: f64,
        #[serde(default, with = "bare")]
        soft: Option<SoftRange>,
    },
    Fixed,
}

impl TryFrom<JointKindFile> for JointKindDesc {
    type Error = &'static str;

    fn try_from(file: JointKindFile) -> Result<Self, Self::Error> {
        Ok(match file {
            JointKindFile::Ball {
                cone,
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                soft_swing,
                soft_twist,
            } => {
                let (swing_x, swing_y) = match (cone, swing_x, swing_y) {
                    (Some(cone), None, None) => (cone, cone),
                    (None, Some(x), Some(y)) => (x, y),
                    (Some(_), _, _) => {
                        return Err(
                            "a ball's swing is a `cone` or `swing_x` and `swing_y`, not both",
                        );
                    }
                    (None, _, _) => {
                        return Err(
                            "a ball's swing needs both `swing_x` and `swing_y`, or a `cone`",
                        );
                    }
                };
                Self::Ball {
                    swing_x,
                    swing_y,
                    twist_lower,
                    twist_upper,
                    neutral,
                    soft_swing,
                    soft_twist,
                }
            }
            JointKindFile::Hinge {
                lower,
                upper,
                neutral,
                soft,
            } => Self::Hinge {
                lower,
                upper,
                neutral,
                soft,
            },
            JointKindFile::Fixed => Self::Fixed,
        })
    }
}

/// An optional field written bare — `soft: (lower: ...)` rather than RON's
/// `Some(...)` — and left out for `None`, which `skip_serializing_if` and
/// `default` on the field see to.
mod bare {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer, T: Serialize>(
        value: &Option<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => value.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<Option<T>, D::Error> {
        T::deserialize(deserializer).map(Some)
    }
}

/// Whether a field holds its default, which the file leaves out.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
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
    /// The swing's or the hinge's axis in the child bone's rigid frame: the
    /// joint frame's `z`.
    pub axis: (f64, f64, f64),
    /// The direction of the joint frame's `x` in the child bone's rigid
    /// frame — the axis a ball's `swing_x` half-angle and neutral `swing_x`
    /// are about — made square to `axis` by removing its part along it. Its
    /// turn about the axis otherwise follows from `axis` alone, as the module
    /// docs say. Left out when it would add nothing.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "bare")]
    pub swing_x_axis: Option<(f64, f64, f64)>,
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
    /// Bones folded into their parents: each carries no body and moves
    /// rigidly with its parent bone, keeping the turn it had from it at the
    /// handoff — an IK helper, an eye, or a finger not worth simulating. A
    /// folded bone may not carry a body. None if left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fold: Vec<String>,
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
    /// A bone the file folds into its parent also carries a body.
    #[error("bone {0:?} is folded into its parent and carries a body")]
    FoldedBody(String),
    /// A bone the file folds has no ancestor carrying a body to follow: a
    /// root, or a bone under bodiless ones only.
    #[error("bone {0:?} is folded, and no bone above it carries a body to follow")]
    FoldedWithoutParent(String),
    /// An axis has no direction, or a joint's `swing_x_axis` none across
    /// its axis.
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
            let rotation =
                joint_rotation(DVec3::from(desc.axis), desc.swing_x_axis.map(DVec3::from))
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
        let bodied = |joint: usize| bodies.iter().any(|body| body.joint == joint);
        for name in &self.fold {
            let joint = bone(name)?;
            if bodied(joint) {
                return Err(DescError::FoldedBody(name.clone()));
            }
            // A bone follows its nearest ancestor with a body; with none, the
            // read-back would hold it where the handoff left it.
            let mut ancestor = skeleton.joints()[joint].parent;
            while let Some(above) = ancestor.filter(|&above| !bodied(above)) {
                ancestor = skeleton.joints()[above].parent;
            }
            if ancestor.is_none() {
                return Err(DescError::FoldedWithoutParent(name.clone()));
            }
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

impl RagdollRig {
    /// This rig as a file again: bodies and joints by the bone names in
    /// `names` (one per skeleton joint), what an editor saves as the starting
    /// point of an authored rig.
    ///
    /// Every pair the rig ignores is listed and `ignore_rest_overlaps` is off,
    /// so the file resolves to the same pairs. **Two things do not survive**:
    /// the file gives a body an axis and not a whole rotation, so a box turned
    /// about its own axis comes back unturned — capsules and spheres, which a
    /// turn about the axis does not change, come back as they were — and the
    /// file lists no folded bones, which a rig keeps no record of: a bone
    /// with no body moves with its parent whether it was folded or not.
    ///
    /// # Errors
    ///
    /// [`DescError::NameCount`] for a names list not one per joint, and
    /// [`DescError::AmbiguousBone`] for a body on a bone whose name another
    /// joint shares, which the file could not name apart.
    pub fn to_desc(&self, names: &[&str]) -> Result<RagdollDesc, DescError> {
        if names.len() != self.joint_count() {
            return Err(DescError::NameCount {
                names: names.len(),
                joints: self.joint_count(),
            });
        }
        let name_of = |joint: usize| {
            let name = names[joint];
            if names.iter().filter(|&&other| other == name).count() > 1 {
                return Err(DescError::AmbiguousBone(name.to_owned()));
            }
            Ok(name.to_owned())
        };
        let triple = |v: DVec3| (v.x, v.y, v.z);
        let bodies = self
            .bodies()
            .iter()
            .map(|body| {
                Ok(BodyDesc {
                    bone: name_of(body.joint)?,
                    shape: ShapeDesc::of(body.shape),
                    mass: body.mass,
                    centre: triple(body.offset.position),
                    axis: triple(body.offset.rotation * DVec3::Y),
                })
            })
            .collect::<Result<Vec<_>, DescError>>()?;
        let joints = self
            .joints()
            .iter()
            .map(|joint| {
                let child = &self.bodies()[joint.child];
                // The joint in its child bone's frame: the child body's offset
                // from that frame, then the joint's frame on the body.
                let local = compose(&child.offset, &joint.frame_child);
                let axis = local.rotation * DVec3::Z;
                let x = local.rotation * DVec3::X;
                Ok(JointDesc {
                    parent: name_of(self.bodies()[joint.parent].joint)?,
                    child: name_of(child.joint)?,
                    kind: JointKindDesc::of(joint.kind),
                    axis: triple(axis),
                    swing_x_axis: (!has_default_roll(axis, x)).then(|| triple(x)),
                    at: triple(local.position),
                    friction: joint.friction,
                })
            })
            .collect::<Result<Vec<_>, DescError>>()?;
        let ignore = self
            .ignored()
            .iter()
            .map(|&(a, b)| {
                Ok((
                    name_of(self.bodies()[a].joint)?,
                    name_of(self.bodies()[b].joint)?,
                ))
            })
            .collect::<Result<Vec<_>, DescError>>()?;
        Ok(RagdollDesc {
            bodies,
            joints,
            ignore,
            ignore_rest_overlaps: false,
            fold: Vec::new(),
            substeps: self.substeps(),
        })
    }
}

impl RagdollDesc {
    /// The file's text: pretty RON with `\n` line ends on every platform, so
    /// a file saved on one machine diffs cleanly on another.
    ///
    /// # Panics
    ///
    /// Never for a desc built by this crate: every field is a number, a string
    /// or a list of them, which RON always writes.
    #[must_use]
    pub fn to_ron(&self) -> String {
        let config = ron::ser::PrettyConfig::new().new_line("\n".to_owned());
        ron::ser::to_string_pretty(self, config).expect("a desc is plain data RON always writes")
    }
}

impl ShapeDesc {
    const fn of(shape: BodyShape) -> Self {
        match shape {
            BodyShape::Capsule {
                radius,
                half_height,
            } => Self::Capsule {
                radius,
                half_height,
            },
            BodyShape::Sphere { radius } => Self::Sphere { radius },
            BodyShape::Box { half_extents } => Self::Box {
                half_extents: (half_extents.x, half_extents.y, half_extents.z),
            },
        }
    }

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

impl From<JointKindDesc> for RigJointKind {
    /// The kind a file's joint resolves to: every field as the file has it.
    fn from(desc: JointKindDesc) -> Self {
        desc.kind()
    }
}

impl JointKindDesc {
    const fn of(kind: RigJointKind) -> Self {
        match kind {
            RigJointKind::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                soft_swing,
                soft_twist,
            } => Self::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                soft_swing,
                soft_twist,
            },
            RigJointKind::Hinge {
                lower,
                upper,
                neutral,
                soft,
            } => Self::Hinge {
                lower,
                upper,
                neutral,
                soft,
            },
            RigJointKind::Fixed => Self::Fixed,
        }
    }

    const fn kind(self) -> RigJointKind {
        match self {
            Self::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                soft_swing,
                soft_twist,
            } => RigJointKind::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                soft_swing,
                soft_twist,
            },
            Self::Hinge {
                lower,
                upper,
                neutral,
                soft,
            } => RigJointKind::Hinge {
                lower,
                upper,
                neutral,
                soft,
            },
            Self::Fixed => RigJointKind::Fixed,
        }
    }
}
