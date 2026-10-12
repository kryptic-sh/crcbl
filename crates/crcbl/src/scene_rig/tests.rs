//! A skin listed child first: what a Blender export gives, renumbered.

use std::path::Path;

use crcbl_assets::MemorySource;
use crcbl_scene::gltf_fixture::{BIN_CHUNK_BUFFER, glb, rigged_bin, rigged_json};

use super::*;

/// The fixture rig imported and converted, with its skin's `joints` array
/// replaced by `joints`.
fn rig(joints: &str) -> (GltfScene, RenderScene) {
    let json = rigged_json(BIN_CHUNK_BUFFER);
    assert_eq!(
        json.matches(r#""joints": [1, 2]"#).count(),
        1,
        "the fixture moved"
    );
    let json = json.replace(r#""joints": [1, 2]"#, &format!(r#""joints": {joints}"#));
    let mut source = MemorySource::new();
    source
        .insert(Path::new("rig.glb"), glb(&json, Some(&rigged_bin())))
        .expect("a legal key");
    let imported =
        crate::scene::import_gltf(&source, Path::new("rig.glb")).expect("the rig imports");
    let render = crate::scene::gltf_render::build_render_scene(&imported, Path::new("rig.glb"));
    (imported, render)
}

/// **A parent-first skin renumbers to itself**: the order every rig that
/// worked before this keeps.
#[test]
fn a_parent_first_skin_keeps_its_order() {
    let (scene, _) = rig("[1, 2]");
    let (skeleton, order) = skeleton_of(&scene.skins()[0], scene.nodes()).expect("a skeleton");
    assert!(order.is_identity());
    assert_eq!(order.nodes(), [1, 2]);
    assert_eq!(skeleton.joints()[1].parent, Some(0));
}

/// **A skin listed child first is renumbered parents first, and everything
/// that indexed the skin's order follows it**: the inverse bind matrices, the
/// vertices' `JOINTS_0`, and a channel's node.
#[test]
fn a_child_first_skin_is_renumbered_with_its_bindings() {
    let (parent_first, parent_render) = rig("[1, 2]");
    let (child_first, child_render) = rig("[2, 1]");
    let skin = &child_first.skins()[0];

    let (skeleton, order) =
        skeleton_of(skin, child_first.nodes()).expect("renumbered, not refused");
    assert!(!order.is_identity());
    assert_eq!(order.nodes(), [1, 2], "the root first");
    assert_eq!(
        (order.palette_of(0), order.palette_of(1)),
        (Some(1), Some(0))
    );
    assert_eq!(order.palette_of(2), None);
    assert_eq!(
        order.joint_of(2),
        Some(1),
        "a channel on the tip drives palette 1"
    );
    assert_eq!(order.joint_of(0), None, "the mesh node is no joint");
    assert_eq!(
        skeleton
            .joints()
            .iter()
            .map(|joint| joint.parent)
            .collect::<Vec<_>>(),
        [None, Some(0)]
    );
    // The file's second matrix belongs to its second joint, the root, which
    // is palette 0 now.
    assert_eq!(skeleton.joints()[0].inverse_bind, skin.inverse_binds()[1]);
    assert_eq!(skeleton.joints()[1].inverse_bind, skin.inverse_binds()[0]);

    // The same bytes under the swapped list name the other joint, so the
    // renumbered bindings are the parent-first ones with 0 and 1 swapped.
    let bindings = |scene: &GltfScene, render: &RenderScene| {
        let mut skips = Vec::new();
        let skinned = skinned_of(scene, render, &mut skips);
        assert!(skips.is_empty(), "{skips:?}");
        skinned.instances[0].bindings.clone()
    };
    let swapped: Vec<SkinBinding> = bindings(&parent_first, &parent_render)
        .into_iter()
        .map(|binding| SkinBinding {
            joints: binding.joints.map(|joint| match joint {
                0 => 1,
                1 => 0,
                other => other,
            }),
            ..binding
        })
        .collect();
    assert_eq!(bindings(&child_first, &child_render), swapped);
    assert!(
        swapped
            .iter()
            .any(|binding| binding.joints.contains(&1) || binding.joints.contains(&0)),
        "the fixture binds a joint, so the swap is seen"
    );
}

/// **A joint moves only to follow its ancestors**: a skin of a chain listed
/// leaf first comes out root first, and siblings keep the file's order.
#[test]
fn the_order_is_stable() {
    // Nodes: 0 root, 1 and 2 children of 0, 3 a child of 2.
    let parents = [None, Some(0), Some(0), Some(2)];
    let order = SkinOrder::of(&[3, 1, 0, 2], &parents);
    assert_eq!(order.nodes(), [0, 2, 3, 1]);
    assert!(SkinOrder::of(&[0, 1, 2, 3], &parents).is_identity());
    // A cycle is placed, not looped on.
    let cycle = SkinOrder::of(&[0, 1], &[Some(1), Some(0)]);
    assert_eq!(cycle.nodes().len(), 2);
}
