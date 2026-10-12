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

/// **Selecting a rigged mesh shows the ragdoll rig fitted to it**: the
/// fixture's one bone carries a capsule along it and its tip a sphere, both
/// standing where the mesh does.
#[test]
fn a_selected_rigged_mesh_shows_its_fitted_ragdoll() {
    let mut editor = headless(64);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    let rig = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    editor.document.select(Some(rig));
    let bodies = selected_ragdoll_bodies(&mut editor.document, &editor.shelf);
    let [(capsule, along), (sphere, tip)] = bodies.as_slice() else {
        panic!("a capsule and a sphere: {bodies:?}");
    };
    assert!(matches!(capsule, crcbl::ragdoll::BodyShape::Capsule { .. }));
    assert!(matches!(sphere, crcbl::ragdoll::BodyShape::Sphere { .. }));
    // Where the mesh stands is where its bone does: the capsule is centred
    // on the bone, and the sphere sits past its tip.
    let [(root, tip_joint)] = selected_bones(&mut editor.document, &editor.shelf)[..] else {
        panic!("one bone");
    };
    let middle = ((root + tip_joint) * 0.5).as_dvec3();
    assert!(
        along.position.distance(middle) < 1e-5,
        "{along:?} vs {middle}"
    );
    assert!(
        tip.position.y > 1.0,
        "the sphere sits past the tip: {tip:?}"
    );
    editor.document.select(None);
    assert!(selected_ragdoll_bodies(&mut editor.document, &editor.shelf).is_empty());
}

/// **A box body's corners are in the order `box_edges` joins**: corners whose
/// indices differ in one bit differ along exactly one of the box's own axes,
/// however the box is turned.
#[test]
fn box_corners_follow_the_edge_order() {
    let half = DVec3::new(0.5, 1.0, 2.0);
    let at = crcbl::phys::Transform::new(
        DVec3::new(1.0, 2.0, 3.0),
        crcbl::math::DQuat::from_rotation_y(0.7) * crcbl::math::DQuat::from_rotation_x(0.3),
    );
    let corners = box_corners(half, &at);
    for i in 0..8_usize {
        for bit in [1_usize, 2, 4] {
            let j = i ^ bit;
            let length = corners[i].distance(corners[j]);
            let want = match bit {
                1 => 2.0 * half.x,
                2 => 2.0 * half.y,
                _ => 2.0 * half.z,
            };
            assert!(
                (f64::from(length) - want).abs() < 1e-5,
                "corners {i} and {j}: {length} vs {want}"
            );
        }
    }
}

/// **G drops the selected rigged mesh as a ragdoll, and G stops it.** The
/// dropped entity draws from its preview region while the bodies fall; its
/// twin of the same asset stays on the shared rest-pose region; and after G
/// the dropped one is back on it too. Nothing in the scene changes.
#[test]
fn g_drops_a_selected_rigged_mesh_and_g_stops_it() {
    let mut editor = headless(256);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    let dropped = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 3.0, 0.0))
        .expect("spawned");
    let twin = editor
        .document
        .spawn_mesh(RIG, DVec3::new(-2.0, 3.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    let before = editor.document.files().expect("files");
    editor.document.select(Some(dropped));
    let shared = editor.skins.draw_of(RIG, 0).expect("a shared region");
    let preview = editor.skins.preview_of(RIG, 0).expect("a preview region");
    assert_ne!(shared, preview);

    tap(&mut editor, KeyCode::KeyG);
    let start = editor.dropping.as_ref().expect("dropping").heights();
    for _ in 0..30 {
        editor.frame().expect("a frame");
    }
    let fallen = editor.dropping.as_ref().expect("still dropping").heights();
    assert!(
        fallen.iter().zip(&start).all(|(now, then)| now < then),
        "the bodies did not fall: {start:?} to {fallen:?}"
    );
    let drawn_from = |editor: &Editor<HeadlessShell>, id| match descs_of(editor, id).as_slice() {
        [Desc::Skinned { draw, .. }] => *draw,
        other => panic!("a rigged mesh drawn as {other:?}"),
    };
    assert_eq!(drawn_from(&editor, dropped), preview);
    assert_eq!(drawn_from(&editor, twin), shared);
    assert_ne!(
        editor.skins.palette(preview),
        editor.skins.palette(shared),
        "the preview region still holds the rest pose"
    );

    tap(&mut editor, KeyCode::KeyG);
    assert!(editor.dropping.is_none());
    assert_eq!(drawn_from(&editor, dropped), shared);
    assert_eq!(
        editor.document.files().expect("files"),
        before,
        "the scene changed"
    );
}
