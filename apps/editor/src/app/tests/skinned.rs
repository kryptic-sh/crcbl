//! A rigged asset in the viewport: drawn through the skinning pass from one
//! posed region its every entity shares, at the rest pose.

use std::path::Path;

use super::*;

use crcbl::assets::MemorySource;
use crcbl::math::DVec3;
use crcbl_scene::gltf_fixture::{
    BIN_CHUNK_BUFFER, glb, rigged_bin, rigged_json, triangle_bin, triangle_json,
};

use crate::app::instances::Desc;
use crate::document::mesh_tests::{TRIANGLE, props};

const RIG: &str = "props/rig.glb";

/// The props' assets with a rig beside the triangle.
fn assets() -> MemorySource {
    let mut source = MemorySource::new();
    for (key, bytes) in [
        (
            RIG,
            glb(&rigged_json(BIN_CHUNK_BUFFER), Some(&rigged_bin())),
        ),
        (
            TRIANGLE,
            glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin())),
        ),
    ] {
        source.insert(Path::new(key), bytes).expect("a legal key");
    }
    source
}

/// The descriptions the entity spawned as `id` is drawn with.
fn descs_of(editor: &Editor<HeadlessShell>, id: SceneEntityId) -> Vec<Desc> {
    editor
        .instances
        .instances
        .iter()
        .find(|placed| placed.drawn == instances::Drawn::Scene(id))
        .expect("the entity is placed")
        .descs()
        .to_vec()
}

/// **A rigged mesh is drawn from its posed region, and a second one shares
/// it.** Spawning the rig rebuilds the renderer around it once, with a skinning
/// pass; both entities' instances name the one region, each under its own
/// placement, and frames run through the skinned entry points. The triangle
/// beside them stays a plain instance.
#[test]
fn a_rigged_mesh_draws_from_one_shared_region() {
    let mut editor = headless(64);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    editor.frame().expect("a frame");
    assert!(editor.skins.skinning().is_none(), "nothing rigged yet");

    let first = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    editor
        .frame()
        .expect("a frame through the skinned entry points");
    assert!(editor.skins.skinning().is_some(), "the rig built a pass");
    let draw = editor
        .skins
        .draw_of(RIG, 0)
        .expect("the rig's part is posed");

    let second = editor
        .document
        .spawn_mesh(RIG, DVec3::new(-2.0, 0.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    assert_eq!(editor.skins.draw_of(RIG, 0), Some(draw), "no second region");

    let skinned = |id| match descs_of(&editor, id).as_slice() {
        [
            Desc::Skinned {
                draw: named,
                transform,
                ..
            },
        ] => {
            assert_eq!(*named, draw, "both share the region");
            *transform
        }
        other => panic!("a rigged mesh drawn as {other:?}"),
    };
    let (a, b) = (skinned(first), skinned(second));
    assert!(
        (a.w_axis.x - b.w_axis.x - 4.0).abs() < 1e-5,
        "each under its own placement: {a} vs {b}"
    );
    let triangle = editor
        .document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .find(|&id| {
            editor
                .document
                .mesh(id)
                .is_some_and(|mesh| mesh.asset == TRIANGLE)
        })
        .expect("the props' triangle");
    assert!(matches!(
        descs_of(&editor, triangle).as_slice(),
        [Desc::Plain(_)]
    ));
    // Each frame begun through the skinned entry point flips the pass's half;
    // a frame that took the plain one would leave it where it was.
    let parity = |editor: &Editor<HeadlessShell>| editor.skins.skinning().expect("a pass").parity();
    let before = parity(&editor);
    editor.frame().expect("another skinned frame");
    assert_ne!(
        parity(&editor),
        before,
        "the frame skipped the skinning pass"
    );
}

/// **Selecting a rigged mesh shows its skeleton**, and selecting the triangle
/// or nothing shows none.
#[test]
fn a_selected_rigged_mesh_shows_its_bones() {
    let mut editor = headless(64);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    let rig = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    editor.document.select(None);
    assert!(selected_bones(&mut editor.document, &editor.shelf).is_empty());
    editor.document.select(Some(rig));
    let bones = selected_bones(&mut editor.document, &editor.shelf);
    assert_eq!(bones.len(), 1, "the rig's one bone: {bones:?}");
    assert!(
        (bones[0].1.y - bones[0].0.y - 1.0).abs() < 1e-5,
        "a unit long, upwards"
    );
}
