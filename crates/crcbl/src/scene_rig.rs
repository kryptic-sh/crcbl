//! A glTF skin as an animation skeleton: the index bookkeeping between
//! `crcbl-scene`'s importer and `crcbl-anim`'s runtime.
//!
//! `crcbl-anim` does not depend on the glTF parser, so a browser build that
//! only plays cooked clips never links one; its docs say the conversion
//! "belongs to whoever holds both". The umbrella holds both behind its
//! `scene` feature, and `apps/viewer` and `apps/editor` both convert skins, so
//! the conversion lives here rather than twice.
//!
//! **A joint's palette index is its place in [`SkinOrder`]**: the skin's
//! `joints` array renumbered so every parent comes before its children, which
//! [`Skeleton::new`] requires and a glTF file does not promise — Blender
//! exports skins in whatever order it holds the bones. Three things index the
//! skin's own order and so go through the same [`SkinOrder`]: the skin's
//! `inverseBindMatrices` ([`skeleton_of`] reads them through it), a skinned
//! vertex's `JOINTS_0` ([`skinned_of`] remaps the bindings through it), and a
//! channel or a node, which names a glTF *node* that [`SkinOrder::joint_of`]
//! maps to a palette index. A skin already listed parents first renumbers to
//! itself.

use crate::anim::{Joint, Skeleton, SkeletonError, Trs};
use crate::math::Mat4;
use crate::scene::gltf_render::{RenderScene, Skip};
use crate::scene::{GltfNode, GltfScene, GltfSkin};
use crate::shaders::skinning::SkinBinding;

/// A skin's joints in palette order: the skin's `joints` array with every
/// parent moved before its children, and the map between the two numberings.
///
/// **Stable**: a joint moves only to follow an ancestor that came after it, so
/// a skin already listed parents first keeps its own order
/// ([`is_identity`](Self::is_identity)), and a reordered one keeps its
/// siblings in the order the file listed them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinOrder {
    /// Palette index → the node that joint is.
    nodes: Vec<usize>,
    /// Palette index → the joint's index in the skin's `joints` array.
    skin_index: Vec<usize>,
    /// The skin's `joints` index → palette index.
    palette: Vec<usize>,
}

impl SkinOrder {
    /// The palette order of a skin whose `joints` array is `joints`, under the
    /// node hierarchy `parents` ([`parents_of`]).
    ///
    /// Each joint is placed after the chain of its unplaced joint ancestors,
    /// root-most first. A chain that loops back on itself — a `children` graph
    /// with a cycle, which nothing before here rules out — is placed in the
    /// order it was climbed, so [`Skeleton::new`] refuses it by name rather
    /// than this looping.
    #[must_use]
    pub fn of(joints: &[usize], parents: &[Option<usize>]) -> Self {
        let mut palette = vec![usize::MAX; joints.len()];
        let mut skin_index = Vec::with_capacity(joints.len());
        let mut chain = Vec::new();
        for start in 0..joints.len() {
            let mut at = Some(start);
            while let Some(index) = at {
                if palette[index] != usize::MAX || chain.contains(&index) {
                    break;
                }
                chain.push(index);
                at = parents[joints[index]].and_then(|parent| joint_of(joints, parent));
            }
            for &index in chain.iter().rev() {
                palette[index] = skin_index.len();
                skin_index.push(index);
            }
            chain.clear();
        }
        Self {
            nodes: skin_index.iter().map(|&index| joints[index]).collect(),
            skin_index,
            palette,
        }
    }

    /// The node each palette index is, in palette order: the list a channel's
    /// node is looked up in, and what a joint's name is read from.
    #[must_use]
    pub fn nodes(&self) -> &[usize] {
        &self.nodes
    }

    /// The palette index of the joint at `skin_index` in the skin's `joints`
    /// array — where a `JOINTS_0` value or an inverse bind matrix of the file
    /// lands — or [`None`] past the end of it.
    #[must_use]
    pub fn palette_of(&self, skin_index: usize) -> Option<usize> {
        self.palette.get(skin_index).copied()
    }

    /// The palette index of `node`, or [`None`] for a node that is not a joint
    /// of this skin: where a channel targeting `node` drives.
    #[must_use]
    pub fn joint_of(&self, node: usize) -> Option<usize> {
        joint_of(&self.nodes, node)
    }

    /// Whether the palette order is the skin's own, as it is for a skin
    /// listed parents first.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.skin_index
            .iter()
            .enumerate()
            .all(|(palette, &index)| palette == index)
    }

    /// `binding` with its joints renumbered into palette order. A joint index
    /// past the skin's end is left as it is, as the file gave it: what the
    /// skinning pass does with one is its own refusal, not a renumbering.
    #[must_use]
    pub fn binding(&self, binding: SkinBinding) -> SkinBinding {
        SkinBinding {
            joints: binding.joints.map(|joint| {
                usize::try_from(joint)
                    .ok()
                    .and_then(|joint| self.palette_of(joint))
                    .and_then(|palette| u32::try_from(palette).ok())
                    .unwrap_or(joint)
            }),
            ..binding
        }
    }
}

/// Builds the skeleton a skin describes, in palette order ([`SkinOrder`]), and
/// returns that order beside it.
///
/// The hierarchy is not in the skin — a joint is a node like any other, so
/// where it sits is [`GltfNode::local_transform`] and what hangs off it is
/// [`GltfNode::children`] — which is why this takes the node array as well.
///
/// A joint whose parent node is not itself a joint of *this* skin is a root
/// here, which is what [`Joint::parent`]'s docs describe: glTF's joints share a
/// common root node and that node need not be a joint.
///
/// **The order is what keeps the rest of the file meaningful**: a caller that
/// loads `JOINTS_0` or a clip itself renumbers through it —
/// [`SkinOrder::binding`], [`SkinOrder::joint_of`] — as [`skinned_of`] does.
///
/// # Errors
///
/// Whatever [`Skeleton::new`] refused the joints with: after the reordering,
/// only a joint hierarchy with a cycle in it.
pub fn skeleton_of(
    skin: &GltfSkin,
    nodes: &[GltfNode],
) -> Result<(Skeleton, SkinOrder), SkeletonError> {
    let parents = parents_of(nodes);
    let order = SkinOrder::of(skin.joints(), &parents);
    let built = order
        .nodes
        .iter()
        .zip(&order.skin_index)
        .map(|(&node, &index)| Joint {
            parent: parents[node].and_then(|parent| order.joint_of(parent)),
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
    Ok((Skeleton::new(built)?, order))
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

/// What a renderer needs to draw a document's skinned geometry: the instances
/// its first skin deforms, and how large a palette poses them.
///
/// **`instances` being empty is the ordinary case** and is what a document with
/// no skin, and a document whose skinned nodes were all refused, both produce —
/// the frame then draws exactly what it drew before skinning existed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skinned {
    /// Matrices one instance's palette holds: the joint count of the first
    /// skin, and zero when there is none.
    ///
    /// The skinning pass is sized from this times [`instances`](Self::instances)'
    /// length, because every one of them is posed from the same skeleton.
    pub joints: u32,
    /// One entry per instance a skin deforms, in [`RenderScene::instances`]
    /// order.
    pub instances: Vec<SkinnedInstance>,
}

/// One instance of the document that a skin deforms.
///
/// It **replaces** a row of [`RenderScene::instances`] rather than joining it:
/// the row is drawn through the skinning seam instead of through
/// [`ForwardRenderer::add_instance`](crate::render::ForwardRenderer::add_instance),
/// and a caller that placed both would draw the mesh twice — once deformed and
/// once in its bind pose, in the same frame, at two different places.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinnedInstance {
    /// Which of [`RenderScene::instances`] this replaces.
    ///
    /// Its mesh and its material are read from that row rather than copied
    /// here, so there is no second answer to drift from it. Its **transform**
    /// is not — see [`transform`](Self::transform), which is the one field a
    /// skinned draw cannot take from the row it replaces.
    pub instance: usize,
    /// **Skeleton space to world**, which is where the drawing node's own
    /// transform is deliberately absent.
    ///
    /// glTF is explicit that "the transform of the skinned mesh node MUST be
    /// ignored": a skinned vertex is placed by its joints, which carry their own
    /// placement. What a palette does *not* carry is where the skeleton itself
    /// hangs — `crcbl::anim::Palette` composes a root joint's global from its
    /// own local transform and stops there — so this is the world transform of
    /// the node those roots hang from, and it is the identity for a rig whose
    /// root joint is a root of the scene.
    pub transform: Mat4,
    /// One [`SkinBinding`] per emitted vertex of the row's mesh, in the same
    /// order:
    /// [`MeshOrigin::bindings`](crate::scene::gltf_render::MeshOrigin::bindings),
    /// with their joints renumbered into palette order ([`SkinOrder`]).
    ///
    /// Owned rather than borrowed from the [`RenderScene`], because the frame
    /// uploads them every frame and the document they came from is dropped once
    /// the renderer is built.
    pub bindings: Vec<SkinBinding>,
}

/// Adds what the skinning seam reserves on top of the description's own pools.
///
/// **Two mesh-table entries and two vertex regions per skinned instance**, which
/// is what `ForwardRenderer::reserve_skinned` documents it takes and what
/// `crcbl::render::SkinnedRegion` needs to hold a frame's pose beside the one
/// before it. `crcbl_scene::gltf_render` sizes both pools to exactly what the
/// description holds, so without this every document with a rig fails to
/// reserve at load — on the *second* instance of the pair, with the pool
/// reporting a number the caller has no way to connect to a skin.
///
/// Saturating rather than wrapping: a document that overflows a `u32` of
/// vertices has already been refused by the conversion, and a capacity that
/// wrapped to a small number would be a pool failure blamed on the wrong thing.
pub fn reserve_for_skinning(capacities: &mut crate::render::Capacities, skinned: &Skinned) {
    for instance in &skinned.instances {
        capacities.meshes = capacities.meshes.saturating_add(2);
        let vertices = u32::try_from(instance.bindings.len()).unwrap_or(u32::MAX);
        capacities.vertices = capacities
            .vertices
            .saturating_add(vertices.saturating_mul(2));
    }
}

/// Which of `render`'s instances the document's **first** skin deforms.
///
/// The first, because the palette is the one thing a skinned draw cannot be
/// given twice; a node
/// wearing any other skin is reported and drawn in its bind pose. Everything
/// else this refuses is reported the same way, because a document whose rigged
/// mesh silently draws undeformed is the failure a person looking at it needs explained.
///
/// # How an instance is paired with the node that placed it
///
/// Neither [`InstanceDesc`](crate::render::scene::InstanceDesc) nor
/// [`MeshOrigin`](crate::scene::gltf_render::MeshOrigin) names a node, so the
/// walk `crcbl_scene::gltf_render::place_instances` did is repeated here:
/// placements in document order, and within each, that mesh's converted
/// primitives in primitive order. [`RenderScene::origins`] is in
/// `(mesh, primitive)` order, so the rows one placement emits are the run of
/// origins naming its mesh.
///
/// **The reconstruction checks itself rather than being trusted.** Every
/// instance carries the row it draws, so the row this walk predicts is compared
/// against the row the instance actually names; a mismatch means the
/// conversion's expansion is no longer the one described above, and this skins
/// nothing at all and says so. That is what keeps it from being a silent
/// assumption about another crate's loop.
pub fn skinned_of(scene: &GltfScene, render: &RenderScene, skips: &mut Vec<Skip>) -> Skinned {
    let Some(skin) = scene.skins().first() else {
        return Skinned::default();
    };
    let parents = parents_of(scene.nodes());
    let Some(to_world) = skeleton_to_world(scene, &parents, skin.joints(), skips) else {
        return Skinned::default();
    };
    let order = SkinOrder::of(skin.joints(), &parents);

    // The converted rows each glTF mesh produced, in primitive order — the
    // half of `place_instances`' walk that lives in `origins`.
    let mut rows_of: Vec<Vec<usize>> = vec![Vec::new(); scene.meshes().len()];
    for (row, origin) in render.origins.iter().enumerate() {
        rows_of[origin.mesh].push(row);
    }

    let mut instances = Vec::new();
    let mut cursor = 0usize;
    for placement in scene.instances() {
        let node = placement.node();
        let at = || match scene.nodes()[node].name() {
            Some(name) => format!("node {node} {name:?}"),
            None => format!("node {node}"),
        };
        for &row in &rows_of[placement.mesh()] {
            let Some(desc) = render.instances.get(cursor) else {
                skips.push(Skip {
                    feature: "skin",
                    at: at(),
                    why: "the conversion ran out of instances while pairing them with the nodes \
                          that placed them, so it cannot tell which of them a skin deforms \
                          and skins none of them"
                        .to_owned(),
                });
                return Skinned::default();
            };
            if desc.mesh != row {
                skips.push(Skip {
                    feature: "skin",
                    at: at(),
                    why: format!(
                        "this conversion expected instance {cursor} to draw mesh row {row} and it \
                         draws row {}, so the walk that pairs an instance with the node that \
                         placed it no longer matches the conversion; nothing in this document \
                         is skinned",
                        desc.mesh,
                    ),
                });
                return Skinned::default();
            }
            cursor += 1;

            let Some(worn) = scene.nodes()[node].skin() else {
                continue;
            };
            if worn != 0 {
                skips.push(Skip {
                    feature: "skin",
                    at: at(),
                    why: format!(
                        "it wears skin {worn} and this conversion poses the first skin only, so it \
                         is drawn in its bind pose"
                    ),
                });
                continue;
            }
            let bindings = &render.origins[row].bindings;
            if bindings.is_empty() {
                skips.push(Skip {
                    feature: "JOINTS_0",
                    at: at(),
                    why: format!(
                        "it wears a skin and the primitive it draws (mesh {} primitive {}) \
                         carries no joint bindings, so there is nothing to deform and it is \
                         drawn in its bind pose",
                        render.origins[row].mesh, render.origins[row].primitive,
                    ),
                });
                continue;
            }
            instances.push(SkinnedInstance {
                // `cursor` was advanced past this instance above, so the row
                // this describes is the one before it.
                instance: cursor - 1,
                transform: to_world,
                bindings: bindings
                    .iter()
                    .map(|&binding| order.binding(binding))
                    .collect(),
            });
        }
    }

    Skinned {
        joints: u32::try_from(skin.joints().len()).unwrap_or(u32::MAX),
        instances,
    }
}

/// Where the skeleton hangs, in world space:
/// [`SkinnedInstance::transform`].
///
/// `crcbl::anim::Palette` composes a root joint's global transform from that
/// joint's own local transform and no further, so its matrices are in the space
/// of the node the root joints are children of. This is that node's world
/// transform, which is the same matrix for every joint of the skeleton and so
/// belongs on the instance rather than folded into the palette every frame.
///
/// `None` refuses the whole document's skinning, and is a hierarchy this cannot
/// answer for: a `children` cycle, which nothing between a file and here rules
/// out — `crcbl_scene::gltf_check`'s `check_nodes` checks that a child index
/// exists, not that the graph is a forest.
///
/// A skin whose root joints hang from **different** nodes gets the first one's
/// answer and a report, because one instance carries one transform. Rigs like
/// that are rare and the alternative is refusing to draw a document over a
/// second root nobody notices.
fn skeleton_to_world(
    scene: &GltfScene,
    parents: &[Option<usize>],
    joints: &[usize],
    skips: &mut Vec<Skip>,
) -> Option<Mat4> {
    // A root of the skeleton is a joint whose parent node is not itself a joint
    // of this skin, which is exactly what [`skeleton_of`] calls a
    // root when it builds the hierarchy the palette is composed down.
    let roots = joints
        .iter()
        .filter(|&&joint| !parents[joint].is_some_and(|parent| joints.contains(&parent)));
    let mut hangs_from = None;
    for &root in roots {
        let parent = parents[root];
        match hangs_from {
            None => hangs_from = Some(parent),
            Some(first) if first != parent => {
                skips.push(Skip {
                    feature: "skin",
                    at: format!("skin 0 root joint node {root}"),
                    why: "this skin's root joints hang from different nodes and a skinned \
                          instance carries one transform, so every one of them is placed by \
                          the first root's"
                        .to_owned(),
                });
            }
            Some(_) => {}
        }
    }
    let Some(Some(parent)) = hangs_from else {
        // Either the skin has no joints at all, or its roots are children of
        // nothing — the scene's own space, which is the identity.
        return Some(Mat4::IDENTITY);
    };
    match world_transform(scene.nodes(), parents, parent) {
        Some(transform) => Some(transform),
        None => {
            skips.push(Skip {
                feature: "skin",
                at: format!("node {parent}"),
                why: "the skeleton hangs from it and its ancestors form a cycle, so there is \
                      no world transform to place the skeleton with and this document is drawn \
                      in its bind pose"
                    .to_owned(),
            });
            None
        }
    }
}

/// `node`'s world transform: its own local transform with every ancestor's
/// applied outside it.
///
/// `None` for a `children` graph that is not a forest — see
/// [`skeleton_to_world`]. The climb is bounded by the node count rather than
/// trusted to terminate, because an acyclic chain visits each node at most once
/// and this is the one application a person points at a file nobody curated.
fn world_transform(nodes: &[GltfNode], parents: &[Option<usize>], node: usize) -> Option<Mat4> {
    let mut transform = Mat4::IDENTITY;
    let mut at = Some(node);
    for _ in 0..=nodes.len() {
        let Some(index) = at else {
            return Some(transform);
        };
        transform = nodes[index].local_transform() * transform;
        at = parents[index];
    }
    None
}

#[cfg(test)]
mod tests;
