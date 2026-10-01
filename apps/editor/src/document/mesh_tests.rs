//! A scene's meshes in the document: boxed by their assets wherever a row
//! enters the world, a missing asset a placeholder and a named problem, and a
//! mesh picked by the box it is drawn as.

use super::*;

use crcbl::scene_mesh::{MESHES, MIN_HALF_EXTENT, PLACEHOLDER_HALF_EXTENT};
use crcbl_scene::gltf_fixture::{BIN_CHUNK_BUFFER, glb, triangle_bin, triangle_json};

use crate::command::SystemRow;

/// The fixture triangle's key in [`assets`]: vertices `(0,0,0)`, `(1,0,0)`
/// and `(0,1,0)` under a node at `(10,0,0)` holding one at `(0,5,0)`.
pub(crate) const TRIANGLE: &str = "props/triangle.glb";

/// The triangle's box in its own frame, through both nodes.
pub(crate) const TRIANGLE_MIN: DVec3 = DVec3::new(10.0, 5.0, 0.0);
pub(crate) const TRIANGLE_MAX: DVec3 = DVec3::new(11.0, 6.0, 0.0);

/// A key [`assets`] does not hold.
pub(crate) const GONE: &str = "props/gone.glb";

/// The triangle mesh in [`props`].
pub(crate) const TRIANGLE_MESH: SceneEntityId = SceneEntityId(1);
/// The mesh of a missing asset in [`props`].
pub(crate) const MISSING_MESH: SceneEntityId = SceneEntityId(2);

/// An asset root holding the triangle, and a file that is not a model.
pub(crate) fn assets() -> MemorySource {
    let mut source = MemorySource::new();
    source
        .insert(
            Path::new(TRIANGLE),
            glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin())),
        )
        .expect("a legal key");
    source
        .insert(Path::new("props/notes.txt"), b"not a model".to_vec())
        .expect("a legal key");
    source
}

/// A scene of blocks and meshes: the greybox ground slab (#0), the triangle
/// standing at the origin (#1) and a mesh of a missing asset (#2) — its
/// meshes read from [`assets`].
pub(crate) fn props() -> Document {
    let mut source = MemorySource::new();
    for (key, text) in [
        (
            "scene.ron",
            "Scene(\n    format: 0,\n    name: \"props\",\n    systems: [\n        \"blocks\",\n        \
             \"meshes\",\n    ],\n)"
                .to_owned(),
        ),
        (
            "env.ron",
            "Env(\n    camera: Camera(\n        position: (0.0, 6.0, 14.0),\n        look_at: (0.0, \
             1.0, 0.0),\n    ),\n    ambient: (0.05, 0.05, 0.06),\n)"
                .to_owned(),
        ),
        (
            "sys/blocks.ron",
            "Chunk(\n    system: \"blocks\",\n    entities: [\n        (0, Block(\n            \
             position: (0.0, -0.5, 0.0),\n            half_extents: (8.0, 0.5, 8.0),\n        \
             )),\n    ],\n)"
                .to_owned(),
        ),
        (
            "sys/meshes.ron",
            format!(
                "Chunk(\n    system: \"meshes\",\n    entities: [\n        (1, Mesh(\n            \
                 asset: {TRIANGLE:?},\n            position: (0.0, 0.0, 0.0),\n        )),\n        \
                 (2, Mesh(\n            asset: {GONE:?},\n            position: (-4.0, 0.5, \
                 0.0),\n        )),\n    ],\n)"
            ),
        ),
    ] {
        source
            .insert(
                Path::new(&format!("props.scn/{key}")),
                text.into_bytes(),
            )
            .expect("a nested scene key is a legal asset key");
    }
    let mut document = Document::open(&source, Path::new("props.scn"), crate::scene::vocabulary())
        .expect("blocks and meshes are a scene");
    document.set_assets(Box::new(assets()));
    document
}

/// `(min, max)` as the render-space bounds [`Document::bounds`] answers.
fn render_box(min: DVec3, max: DVec3) -> (Vec3, Vec3) {
    (narrow(min), narrow(max))
}

/// **A mesh is boxed by its asset once the document has a source**, a flat
/// one given [`MIN_HALF_EXTENT`] across its plane, and a mesh of a missing
/// asset is the placeholder cube with a problem naming it.
#[test]
fn a_mesh_is_boxed_by_its_asset_and_a_missing_one_by_the_placeholder() {
    let mut document = props();
    let depth = DVec3::new(0.0, 0.0, MIN_HALF_EXTENT);
    assert_eq!(
        document.bounds(TRIANGLE_MESH),
        Some(render_box(TRIANGLE_MIN - depth, TRIANGLE_MAX + depth)),
    );
    let centre = DVec3::new(-4.0, 0.5, 0.0);
    let half = DVec3::splat(PLACEHOLDER_HALF_EXTENT);
    assert_eq!(
        document.bounds(MISSING_MESH),
        Some(render_box(centre - half, centre + half))
    );
    let problems = document.mesh_problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("entity #2: ") && problems[0].contains(GONE),
        "{problems:?}"
    );
    assert_eq!(document.problems().expect("it saves"), problems);
    assert_eq!(
        document.mesh_assets().into_iter().collect::<Vec<_>>(),
        [TRIANGLE],
        "a missing asset is not one to draw",
    );
}

/// **A document opened with no assets boxes every mesh as the placeholder**,
/// and naming the source measures them.
#[test]
fn a_document_without_assets_boxes_its_meshes_as_placeholders_until_given_some() {
    let mut document = props();
    document.set_assets(Box::new(MemorySource::new()));
    let half = DVec3::splat(PLACEHOLDER_HALF_EXTENT);
    assert_eq!(
        document.bounds(TRIANGLE_MESH),
        Some(render_box(-half, half))
    );
    assert_eq!(document.mesh_problems().len(), 2);
    let measures = document.measures();
    document.set_assets(Box::new(assets()));
    assert!(document.measures() > measures, "measuring moved no box");
    assert_eq!(document.mesh_problems().len(), 1);
}

/// **A mesh picks by the box it is drawn as**, measured — a ray through the
/// triangle's box picks it, where the placeholder at its origin is not.
#[test]
fn a_mesh_picks_by_its_measured_box() {
    let mut document = props();
    let through_triangle = Ray::new(DVec3::new(10.5, 5.5, 20.0), DVec3::NEG_Z);
    assert_eq!(document.pick(&through_triangle), Some(TRIANGLE_MESH));
    let through_origin = Ray::new(DVec3::new(0.0, 0.25, 20.0), DVec3::NEG_Z);
    assert_eq!(
        document.pick(&through_origin),
        None,
        "the triangle still picks as the placeholder at its origin"
    );
}

/// **A spawned mesh is measured as it arrives, and its undo and redo too** —
/// the path a drop from the asset browser takes.
#[test]
fn a_spawned_mesh_is_measured_and_its_undo_takes_it_back() {
    let mut document = props();
    let id = document.ids.next_id();
    let row = crcbl::scene::scn::row_text(
        MESHES,
        &crcbl::scene_mesh::Mesh::new(TRIANGLE, [2.0, 0.0, 0.0]),
    )
    .expect("a mesh serialises");
    document
        .apply(EditCommand::Spawn {
            entity: id,
            rows: vec![SystemRow {
                system: MESHES.to_owned(),
                row,
            }],
            name: None,
        })
        .expect("the scene lists meshes");
    let shift = DVec3::new(2.0, 0.0, 0.0);
    let depth = DVec3::new(0.0, 0.0, MIN_HALF_EXTENT);
    let measured = Some(render_box(
        TRIANGLE_MIN + shift - depth,
        TRIANGLE_MAX + shift + depth,
    ));
    assert_eq!(document.bounds(id), measured);
    document.undo().expect("the spawn undoes");
    assert_eq!(document.bounds(id), None);
    document.redo().expect("and redoes");
    assert_eq!(
        document.bounds(id),
        measured,
        "the redone mesh is a placeholder"
    );
}

/// **Retyping a mesh's asset in a panel measures the new one**, and a key the
/// source does not hold is the placeholder and a problem — never a panic.
#[test]
fn a_retyped_asset_is_measured_or_reported() {
    let mut document = props();
    document
        .apply(EditCommand::SetProperty {
            entity: MISSING_MESH,
            system: MESHES.to_owned(),
            path: "asset".to_owned(),
            value: Value::Text(TRIANGLE.to_owned()),
        })
        .expect("the asset is text");
    assert!(document.mesh_problems().is_empty());
    let shift = DVec3::new(-4.0, 0.5, 0.0);
    let depth = DVec3::new(0.0, 0.0, MIN_HALF_EXTENT);
    assert_eq!(
        document.bounds(MISSING_MESH),
        Some(render_box(
            TRIANGLE_MIN + shift - depth,
            TRIANGLE_MAX + shift + depth
        )),
    );
    document.undo().expect("the retype undoes");
    assert_eq!(document.mesh_problems().len(), 1);
}

/// **Play's restore measures the meshes it loads**, so a played scene draws
/// and picks its meshes by their assets, and so does the scene stop puts back.
#[test]
fn play_and_stop_keep_the_meshes_measured() {
    let mut document = props();
    let measured = document.bounds(TRIANGLE_MESH);
    document.play().expect("nothing here refuses play");
    assert_eq!(document.bounds(TRIANGLE_MESH), measured);
    document.stop().expect("the snapshot reloads");
    assert_eq!(document.bounds(TRIANGLE_MESH), measured);
}

/// **A mesh dropped into a scene with no meshes lists the system with the
/// spawn**, as one entry: one undo takes the mesh and the manifest entry back
/// and the files are what they were; the redo brings both.
#[test]
fn a_mesh_spawned_into_a_scene_without_meshes_lists_them_as_one_undo() {
    let mut document = Document::built_in().expect("the compiled-in scene");
    document.set_assets(Box::new(assets()));
    let before = document.files().expect("saves");
    assert!(!before["scene.ron"].contains(MESHES));

    let id = document
        .spawn_mesh(TRIANGLE, DVec3::new(1.0, 0.0, 2.0))
        .expect("the vocabulary has meshes");
    assert_eq!(document.log().position(), 1, "the drop was not one entry");
    let files = document.files().expect("saves");
    assert!(
        files["scene.ron"].contains(MESHES),
        "{}",
        files["scene.ron"]
    );
    assert!(files["sys/meshes.ron"].contains(TRIANGLE));
    let (min, max) = document.bounds(id).expect("placed");
    assert_eq!(min.y, 0.0, "the mesh does not stand on the point");
    assert_eq!(((min.x + max.x) * 0.5, (min.z + max.z) * 0.5), (1.0, 2.0));

    document.undo().expect("the drop undoes");
    assert_eq!(document.files().expect("saves"), before);
    document.redo().expect("and redoes");
    assert_eq!(document.files().expect("saves"), files);
}

/// **A key no mesh may name is refused before anything is spawned**, and an
/// asset the source does not hold is spawned as the placeholder with its
/// problem — never refused, never a panic.
#[test]
fn a_drop_of_a_bad_key_is_refused_and_of_a_missing_asset_is_a_placeholder() {
    let mut document = props();
    let before = document.files().expect("saves");
    for asset in ["", "../escape.glb", "props/notes.txt"] {
        assert!(
            matches!(
                document.spawn_mesh(asset, DVec3::ZERO),
                Err(EditError::Asset(_))
            ),
            "`{asset}` was not refused as a key",
        );
    }
    assert_eq!(document.files().expect("saves"), before);
    assert_eq!(document.log().position(), 0);

    let id = document
        .spawn_mesh("props/elsewhere.glb", DVec3::new(2.0, 0.0, 0.0))
        .expect("a missing asset is a placeholder, not a refusal");
    let half = PLACEHOLDER_HALF_EXTENT;
    assert_eq!(
        document.bounds(id),
        Some(render_box(
            DVec3::new(2.0 - half, 0.0, -half),
            DVec3::new(2.0 + half, 2.0 * half, half)
        )),
    );
    assert!(
        document
            .mesh_problems()
            .iter()
            .any(|problem| problem.starts_with(&format!("entity #{id}: "))),
        "{:?}",
        document.mesh_problems(),
    );
}

/// **A drop is refused in play mode**, before anything is spawned.
#[test]
fn a_drop_in_play_mode_is_refused() {
    let mut document = props();
    document.play().expect("nothing here refuses play");
    assert!(matches!(
        document.spawn_mesh(TRIANGLE, DVec3::ZERO),
        Err(EditError::Playing)
    ));
}

/// **A system holding rows is not unlisted, and one listed is not listed
/// again**: the refusals that keep a save from dropping rows.
#[test]
fn listing_and_unlisting_refuse_what_would_lose_rows() {
    let mut document = props();
    assert!(matches!(
        document.apply(EditCommand::UnlistSystem {
            system: MESHES.to_owned()
        }),
        Err(EditError::Populated(system)) if system == MESHES
    ));
    assert!(matches!(
        document.apply(EditCommand::ListSystem {
            system: MESHES.to_owned()
        }),
        Err(EditError::Listed(system)) if system == MESHES
    ));
    assert!(matches!(
        document.apply(EditCommand::ListSystem {
            system: "nonsense".to_owned()
        }),
        Err(EditError::NoSystem(_))
    ));
    assert_eq!(document.log().position(), 0);
}
