//! The joint palette: a posed skeleton composed down its hierarchy and folded
//! against the bind pose, which is what a skinning shader consumes.

use glam::Mat4;

use crate::{Pose, Skeleton};

/// One skinning matrix per joint, in palette order, and the global transforms
/// they were built from.
///
/// # What a palette matrix is
///
/// For each joint, its global transform times its inverse bind matrix. Two
/// sentences of the specification are what that is assembled from. §3.7.3.2, on
/// composing the hierarchy:
///
/// > The global transformation matrix of a node is the product of the global
/// > transformation matrix of its parent node and its own local transformation
/// > matrix. When the node has no parent node, its global transformation matrix
/// > is identical to its local transformation matrix.
///
/// and §3.7.4.3, on which side the bind matrix goes:
///
/// > To apply skinning, a transformation matrix is computed for each joint.
/// > Then, the per-vertex transformation matrices are computed as weighted
/// > linear sums of the joint transformation matrices. Note that per-joint
/// > inverse bind matrices (when present) **MUST** be applied before the base
/// > node transforms.
///
/// "Before" is *to the right*, because a matrix acts on the vertex to its
/// right: the inverse bind takes the vertex out of mesh space into the joint's
/// bind-time local space, and the global transform then puts it where the joint
/// is now. Reversing the two is the classic wrong palette — it compiles, and it
/// explodes the mesh.
///
/// # What is deliberately not in it
///
/// The skinned mesh node's own transform, and its inverse. §3.7.4.2:
///
/// > Only the joint transforms are applied to the skinned mesh; the transform
/// > of the skinned mesh node **MUST** be ignored.
///
/// So a palette needs no mesh node at all, and this type never sees one. An
/// instance transform for the *character* — where it stands in the world — is
/// the renderer's, applied outside skinning like any other object's.
///
/// # Cost
///
/// Both buffers are allocated once by [`new`](Self::new) and refilled by
/// [`compute`](Self::compute), which walks the joints once, forwards. That
/// single pass is what [`Skeleton`]'s parent-before-child invariant buys: each
/// joint's parent has already been written when the joint is reached.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    globals: Vec<Mat4>,
    skinning: Vec<Mat4>,
}

impl Palette {
    /// A palette sized for this skeleton, filled with identities.
    ///
    /// The contents are meaningless until [`compute`](Self::compute) runs;
    /// identity is chosen over the bind pose only because it costs nothing and
    /// is obviously not a pose.
    #[must_use]
    pub fn new(skeleton: &Skeleton) -> Self {
        Self {
            globals: vec![Mat4::IDENTITY; skeleton.len()],
            skinning: vec![Mat4::IDENTITY; skeleton.len()],
        }
    }

    /// Composes `pose` down `skeleton` and folds in the inverse bind matrices.
    ///
    /// # Panics
    ///
    /// If `pose` or this palette was not built for a skeleton of this size —
    /// the palette index would name a different bone in each, and there is no
    /// answer to give.
    pub fn compute(&mut self, skeleton: &Skeleton, pose: &Pose) {
        assert_eq!(
            pose.len(),
            skeleton.len(),
            "this pose was built for a skeleton with a different joint count"
        );
        assert_eq!(
            self.skinning.len(),
            skeleton.len(),
            "this palette was built for a skeleton with a different joint count"
        );
        for (index, joint) in skeleton.joints().iter().enumerate() {
            let local = pose.locals()[index].to_mat4();
            // The parent precedes the child, so `globals[parent]` is already
            // this frame's value rather than the last one's.
            let global = match joint.parent {
                Some(parent) => self.globals[parent] * local,
                None => local,
            };
            self.globals[index] = global;
            self.skinning[index] = global * joint.inverse_bind;
        }
    }

    /// The skinning matrices, in palette order — mesh space to posed space.
    ///
    /// This is the array a skinning shader reads.
    #[inline]
    #[must_use]
    pub fn matrices(&self) -> &[Mat4] {
        &self.skinning
    }

    /// Each joint's global transform, in palette order — where the bone *is*,
    /// with no bind matrix folded in.
    ///
    /// Kept beside the skinning matrices because [`compute`](Self::compute)
    /// needs them anyway and the two consumers that want them — a socket
    /// attaching a prop to a hand joint, and the skeleton debug overlay — would
    /// otherwise have to recompose the hierarchy to get them back.
    #[inline]
    #[must_use]
    pub fn globals(&self) -> &[Mat4] {
        &self.globals
    }

    /// How many joints this palette covers.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.skinning.len()
    }

    /// Whether this palette covers no joints.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skinning.is_empty()
    }
}
