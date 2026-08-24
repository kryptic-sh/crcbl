//! The joint hierarchy a clip is sampled onto and a palette is computed from.

use std::fmt;

use glam::Mat4;

use crate::Trs;

/// One joint: where it hangs, where it rests, and how it undoes the bind pose.
///
/// The index of a joint in [`Skeleton::joints`] is its *palette index*, and
/// that index is an identity the rest of the pipeline holds: a skinned vertex's
/// `JOINTS_n` attribute indexes the skin's joint array and nothing else. §3.7.4
/// of the specification:
///
/// > The `JOINTS_n` attribute data contains the indices of the joints from the
/// > corresponding `skin.joints` array that affect the vertex.
///
/// So a joint's number is not this crate's to choose. See
/// [`Skeleton::new`] for what follows from that.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joint {
    /// The palette index of this joint's parent, or `None` for a root.
    ///
    /// A skeleton may have more than one root: glTF requires the joints to
    /// share a *common root* node, but that node need not itself be a joint, in
    /// which case its several joint children are each parentless here.
    pub parent: Option<usize>,
    /// World → joint-local at bind time, the skin's `inverseBindMatrices`
    /// entry for this joint.
    ///
    /// [`Mat4::IDENTITY`] is the specification's meaning for a skin that
    /// declares none: the matrices were pre-applied to the vertices.
    pub inverse_bind: Mat4,
    /// The joint node's own local transform with no channel driving it.
    ///
    /// This is the joint's *rest* pose, not its bind pose — the two coincide in
    /// most exports and are different things. It is what a joint keeps when a
    /// clip leaves it alone, and what a partially-driven joint keeps the
    /// components of: a channel that carries only rotations must not flatten
    /// the joint's rest translation to zero, or the skeleton collapses onto its
    /// roots.
    pub rest: Trs,
}

/// Why a joint array could not become a [`Skeleton`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkeletonError {
    /// A joint named a parent that is not a joint of this skeleton.
    ParentOutOfRange {
        /// The palette index of the joint that named it.
        joint: usize,
        /// The parent index it named.
        parent: usize,
    },
    /// A joint named a parent that comes after it in palette order.
    ParentAfterChild {
        /// The palette index of the joint that named it.
        joint: usize,
        /// The parent index it named.
        parent: usize,
    },
    /// A joint is its own parent.
    SelfParent {
        /// The palette index of the joint.
        joint: usize,
    },
}

impl fmt::Display for SkeletonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ParentOutOfRange { joint, parent } => {
                write!(
                    f,
                    "joint {joint} names parent {parent}, which is not a joint"
                )
            }
            Self::ParentAfterChild { joint, parent } => write!(
                f,
                "joint {joint} names parent {parent}, which comes after it in palette order"
            ),
            Self::SelfParent { joint } => write!(f, "joint {joint} is its own parent"),
        }
    }
}

impl std::error::Error for SkeletonError {}

/// Joints in palette order, each with its parent and its inverse bind matrix.
///
/// # Parents come first, and that is checked rather than fixed
///
/// [`new`](Self::new) refuses a joint whose parent has a higher palette index.
/// The invariant is what lets [`Palette::compute`](crate::Palette::compute) walk
/// the array once, forwards, reading each parent's global transform out of the
/// same buffer it is filling — the single pass the per-frame cost budget is
/// written against.
///
/// **Enforced, not sorted into.** A topological sort would renumber the joints,
/// and a joint's number is the one thing about it this crate does not own: it is
/// what a mesh's `JOINTS_n` attribute and the skin's `inverseBindMatrices` order
/// already agree on (see [`Joint`]). Sorting here would silently invalidate
/// every skinned vertex the caller holds, and this crate never sees those
/// vertices, so it could not fix them. Refusing names the problem in the one
/// place that *can* fix it — the converter, which holds the joint array, the
/// bind matrices and the mesh together.
///
/// In practice the check passes: glTF's `skin.joints` is conventionally written
/// in hierarchy order, and the sample assets `docs/plan/17-animation.md` names
/// as the acceptance set are. A document that is not is a real remap job, and it
/// is better to be told so than to be given a quietly wrong palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    joints: Vec<Joint>,
}

impl Skeleton {
    /// Takes joints in palette order.
    ///
    /// # Errors
    ///
    /// [`SkeletonError`] if any joint names a parent that does not exist, is
    /// itself, or comes after it — see the type docs for why the last of those
    /// is refused rather than repaired.
    ///
    /// An empty skeleton is allowed. It poses nothing and its palette is empty,
    /// which is what a skin with no joints means; there is no arithmetic that
    /// could go wrong for it.
    pub fn new(joints: Vec<Joint>) -> Result<Self, SkeletonError> {
        for (index, joint) in joints.iter().enumerate() {
            let Some(parent) = joint.parent else {
                continue;
            };
            if parent == index {
                return Err(SkeletonError::SelfParent { joint: index });
            }
            if parent >= joints.len() {
                return Err(SkeletonError::ParentOutOfRange {
                    joint: index,
                    parent,
                });
            }
            if parent > index {
                return Err(SkeletonError::ParentAfterChild {
                    joint: index,
                    parent,
                });
            }
        }
        Ok(Self { joints })
    }

    /// The joints, in palette order.
    #[inline]
    #[must_use]
    pub fn joints(&self) -> &[Joint] {
        &self.joints
    }

    /// How many joints there are, which is also the palette's length.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.joints.len()
    }

    /// Whether this skeleton has no joints at all.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.joints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Joint, Skeleton, SkeletonError};
    use crate::Trs;
    use glam::Mat4;

    fn joint(parent: Option<usize>) -> Joint {
        Joint {
            parent,
            inverse_bind: Mat4::IDENTITY,
            rest: Trs::IDENTITY,
        }
    }

    #[test]
    fn accepts_a_chain_in_hierarchy_order() {
        let skeleton = Skeleton::new(vec![joint(None), joint(Some(0)), joint(Some(1))])
            .expect("a chain whose parents precede their children is well ordered");
        assert_eq!(skeleton.len(), 3);
        assert!(!skeleton.is_empty());
    }

    #[test]
    fn accepts_several_roots() {
        let skeleton = Skeleton::new(vec![joint(None), joint(None), joint(Some(1))])
            .expect("a common root that is not itself a joint leaves several parentless joints");
        assert_eq!(skeleton.len(), 3);
    }

    #[test]
    fn accepts_no_joints() {
        let skeleton = Skeleton::new(Vec::new()).expect("an empty skeleton poses nothing");
        assert!(skeleton.is_empty());
    }

    #[test]
    fn refuses_a_parent_after_its_child() {
        let error = Skeleton::new(vec![joint(Some(1)), joint(None)])
            .expect_err("joint 0 names joint 1, which the single forward pass has not reached");
        assert_eq!(
            error,
            SkeletonError::ParentAfterChild {
                joint: 0,
                parent: 1
            }
        );
    }

    #[test]
    fn refuses_a_parent_that_is_not_a_joint() {
        let error =
            Skeleton::new(vec![joint(None), joint(Some(7))]).expect_err("joint 7 does not exist");
        assert_eq!(
            error,
            SkeletonError::ParentOutOfRange {
                joint: 1,
                parent: 7
            }
        );
    }

    #[test]
    fn refuses_a_joint_that_parents_itself() {
        let error = Skeleton::new(vec![joint(None), joint(Some(1))])
            .expect_err("a self-parent is a one-joint cycle");
        assert_eq!(error, SkeletonError::SelfParent { joint: 1 });
    }

    #[test]
    fn errors_say_which_joint() {
        let error = SkeletonError::ParentAfterChild {
            joint: 3,
            parent: 9,
        };
        assert_eq!(
            error.to_string(),
            "joint 3 names parent 9, which comes after it in palette order"
        );
    }
}
