//! A glTF skin as an animation skeleton: the index bookkeeping between
//! `crcbl-scene`'s importer and `crcbl-anim`'s runtime.
//!
//! `crcbl-anim` does not depend on the glTF parser, so a browser build that
//! only plays cooked clips never links one; its docs say the conversion
//! "belongs to whoever holds both". The umbrella holds both behind its
//! `scene` feature, and `apps/viewer` and `apps/editor` both convert skins, so
//! the conversion lives here rather than twice.
//!
//! **A joint's palette index is its position in the skin's `joints` array**,
//! and that number is not this code's to choose: a skinned vertex's
//! `JOINTS_0` attribute indexes that same array, and the skin's
//! `inverseBindMatrices` are in that same order. So [`skeleton_of`] walks
//! `skin.joints()` in order and renumbers nothing. A channel or a node names a
//! glTF *node*, which [`joint_of`] maps to a palette index.

use crate::anim::{Joint, Skeleton, SkeletonError, Trs};
use crate::math::Mat4;
use crate::scene::{GltfNode, GltfSkin};

/// Builds the skeleton a skin describes, in the skin's own joint order.
///
/// The hierarchy is not in the skin — a joint is a node like any other, so
/// where it sits is [`GltfNode::local_transform`] and what hangs off it is
/// [`GltfNode::children`] — which is why this takes the node array as well.
///
/// A joint whose parent node is not itself a joint of *this* skin is a root
/// here, which is what [`Joint::parent`]'s docs describe: glTF's joints share a
/// common root node and that node need not be a joint.
///
/// # Errors
///
/// Whatever [`Skeleton::new`] refused the joints with — a skeleton whose
/// joints are not parent-first cannot be renumbered without breaking every
/// skinned vertex, so the refusal is reported rather than repaired.
pub fn skeleton_of(skin: &GltfSkin, nodes: &[GltfNode]) -> Result<Skeleton, SkeletonError> {
    let joints = skin.joints();
    let parents = parents_of(nodes);
    let built = joints
        .iter()
        .enumerate()
        .map(|(index, &node)| Joint {
            parent: parents[node].and_then(|parent| joint_of(joints, parent)),
            // Exactly as long as `joints` by the importer's own contract; the
            // identity fallback is the specification's meaning for a skin that
            // declares no matrices at all, so it is also the only answer that
            // is not an invention if the two arrays ever disagreed.
            inverse_bind: skin
                .inverse_binds()
                .get(index)
                .copied()
                .unwrap_or(Mat4::IDENTITY),
            // The *rest* pose, which is the joint node's own transform with no
            // channel driving it — not the bind pose, which is the inverse
            // matrix above. A joint no channel touches keeps this, and so does
            // the translation of a joint only a rotation channel drives.
            rest: Trs::from_mat4(nodes[node].local_transform()),
        })
        .collect();
    Skeleton::new(built)
}

/// Which node is each node's parent, or `None` for a node nothing declares as a
/// child.
///
/// Built by walking the children lists, because that is the only direction glTF
/// stores the tree in. Every index is valid: `crcbl-scene`'s `gltf_check`
/// refuses a document whose node names a child that does not exist, before
/// [`import_gltf`](crate::scene::import_gltf) builds anything.
///
pub fn parents_of(nodes: &[GltfNode]) -> Vec<Option<usize>> {
    let mut parents = vec![None; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        for &child in node.children() {
            parents[child] = Some(index);
        }
    }
    parents
}

/// Which palette index of this skin `node` is, or `None` for a node the skin
/// has not got.
///
/// A scan rather than a map: a skin's joint list is tens of entries, this is
/// asked once per joint and once per channel at load, and a `HashMap` built for
/// it would cost more to fill than the scans it saves.
pub fn joint_of(joints: &[usize], node: usize) -> Option<usize> {
    joints.iter().position(|&joint| joint == node)
}
