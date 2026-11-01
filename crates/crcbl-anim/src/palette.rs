//! The joint palette: a posed skeleton composed down its hierarchy and folded
//! against the bind pose, which is what a skinning shader consumes.

use std::fmt;

use glam::Mat4;

use crate::{Pose, Skeleton, Trs};

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
/// The buffers are allocated once by [`new`](Self::new) and refilled by
/// [`compute`](Self::compute), which walks the joints once, forwards. That
/// single pass is what [`Skeleton`]'s parent-before-child invariant buys: each
/// joint's parent has already been written when the joint is reached.
///
/// The walk recomposes only what changed. The palette keeps the local
/// transforms it last composed, and a joint is recomposed when its local
/// differs from the kept one in any bit, or its parent was recomposed on this
/// walk — the joints that changed and everything that hangs below them. The
/// rest keep the matrices they hold, which are what recomposing them would
/// write again: a joint's matrices are a function of its local, its parent's
/// global and its inverse bind, and none of those moved. So the result is the
/// full composition bit for bit, whatever the pose did since the last call,
/// and a rig that recomputes after each of a dozen small edits to one pose
/// pays for the joints each edit moved rather than for the skeleton a dozen
/// times. Being handed a different skeleton than last time, or computing for
/// the first time, recomposes everything.
#[derive(Clone)]
pub struct Palette {
    globals: Vec<Mat4>,
    skinning: Vec<Mat4>,
    /// The local transforms `globals` and `skinning` were composed from.
    composed: Vec<Trs>,
    /// Which joints this walk recomposed, so their children follow. Scratch:
    /// read only within one [`compute`](Self::compute).
    recomposed: Vec<bool>,
    /// The [`Skeleton::id`] the buffers were composed for, or `None` before
    /// the first compute.
    skeleton: Option<u64>,
}

impl PartialEq for Palette {
    /// Equal when the matrices are: what was kept to compose them is not part
    /// of what a palette *is*.
    fn eq(&self, other: &Self) -> bool {
        self.globals == other.globals && self.skinning == other.skinning
    }
}

impl fmt::Debug for Palette {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Palette")
            .field("globals", &self.globals)
            .field("skinning", &self.skinning)
            .finish_non_exhaustive()
    }
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
            composed: vec![Trs::IDENTITY; skeleton.len()],
            recomposed: vec![false; skeleton.len()],
            skeleton: None,
        }
    }

    /// Composes `pose` down `skeleton` and folds in the inverse bind matrices,
    /// recomposing only the joints whose local transform changed since the
    /// last call and their descendants — see the type docs for why that is the
    /// full composition bit for bit.
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
        let everything = self.skeleton != Some(skeleton.id());
        let joints = skeleton.joints().iter().zip(pose.locals());
        for (index, ((joint, local), composed)) in joints.zip(&mut self.composed).enumerate() {
            // The parent precedes the child, so `recomposed[parent]` and
            // `globals[parent]` are already this walk's rather than the last.
            let parent_moved = joint.parent.is_some_and(|parent| self.recomposed[parent]);
            let moved = everything || parent_moved || !same_bits(local, composed);
            self.recomposed[index] = moved;
            if !moved {
                continue;
            }
            *composed = *local;
            let local = local.to_mat4();
            let global = match joint.parent {
                Some(parent) => self.globals[parent] * local,
                None => local,
            };
            self.globals[index] = global;
            self.skinning[index] = global * joint.inverse_bind;
        }
        self.skeleton = Some(skeleton.id());
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

/// Whether two local transforms are the same in every bit — `-0.0` apart from
/// `0.0`, and a `NaN` the same as itself — which is the sameness that makes
/// composing them give the same bits.
#[inline]
fn same_bits(a: &Trs, b: &Trs) -> bool {
    let bits = |trs: &Trs| {
        let [tx, ty, tz] = trs.translation.to_array();
        let [rx, ry, rz, rw] = trs.rotation.to_array();
        let [sx, sy, sz] = trs.scale.to_array();
        [tx, ty, tz, rx, ry, rz, rw, sx, sy, sz].map(f32::to_bits)
    };
    // One accumulated difference rather than a comparison per component: a
    // branch-free form the compiler keeps in registers, where `==` on the
    // arrays became a call to `bcmp` through two stack copies.
    let difference = bits(a)
        .iter()
        .zip(bits(b))
        .fold(0, |difference, (a, b)| difference | (a ^ b));
    difference == 0
}
