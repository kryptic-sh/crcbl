//! The mesh component through the scene format, its key's refusals, and the
//! box its asset is measured at.

use crcbl_assets::MemorySource;
use crcbl_reflect::{Value, get_path, set_path};
use crcbl_scene::scn::{IdMap, Scene, SceneEntityId, ScnError};

use super::*;

/// The meshes alone.
fn registry() -> Registry {
    let mut registry = Registry::new();
    register(&mut registry);
    registry
}

/// A scene listing the meshes, with `rows` as its chunk's entities.
fn scene(rows: &str) -> MemorySource {
    let mut source = MemorySource::new();
    for (key, text) in [
        (
            "scene.ron",
            "(format: 0, name: \"props\", systems: [\"meshes\"])".to_owned(),
        ),
        (
            "env.ron",
            "(camera: (position: (0.0, 2.0, 12.0), look_at: (0.0, 0.0, 0.0)), \
             ambient: (0.1, 0.1, 0.1))"
                .to_owned(),
        ),
        (
            "sys/meshes.ron",
            format!("(system: \"meshes\", entities: [{rows}])"),
        ),
    ] {
        source
            .insert(Path::new(key), text.into_bytes())
            .expect("a scene key is a legal asset key");
    }
    source
}

/// The scene in `source`, loaded through the meshes' codec.
fn load(source: &MemorySource) -> Result<(World, Scene, IdMap), ScnError> {
    let registry = registry();
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (scene, ids) = Scene::load(source, Path::new(""), &registry.codecs(), &mut world)?;
    Ok((world, scene, ids))
}

/// The mesh entity `id` names.
fn mesh(world: &mut World, ids: &IdMap, id: u32) -> Mesh {
    let entity = ids.entity(SceneEntityId(id)).expect("in the scene");
    mesh_of(world, entity).expect("a mesh").clone()
}

/// **A mesh round-trips through the scene format byte for byte**, with its
/// asset and its position, and the measured box is never written.
#[test]
fn a_mesh_round_trips_and_its_measured_box_is_not_written() {
    let rows = "(0, (asset: \"props/crate.glb\", position: (1.0, 0.0, -2.5)))";
    let (mut world, scene, ids) = load(&scene(rows)).expect("the scene loads");
    let entity = ids.entity(SceneEntityId(0)).expect("in the scene");
    assert_eq!(
        mesh(&mut world, &ids, 0),
        Mesh::new("props/crate.glb", [1.0, 0.0, -2.5])
    );

    meshes(&mut world)
        .and_then(|system| system.get_mut(entity))
        .expect("a mesh")
        .set_local_bounds(DVec3::ZERO, DVec3::ONE);
    let registry = registry();
    let written = scene
        .save(&mut world, &ids, &registry.codecs())
        .expect("the scene saves");
    assert_eq!(
        written["sys/meshes.ron"],
        "Chunk(\n    system: \"meshes\",\n    entities: [\n        (0, Mesh(\n            \
         asset: \"props/crate.glb\",\n            position: (1.0, 0.0, -2.5),\n        )),\n    \
         ],\n)",
        "the measured box leaked into the file",
    );
    let mut again = MemorySource::new();
    for (key, text) in &written {
        again
            .insert(Path::new(key), text.clone().into_bytes())
            .expect("a scene key");
    }
    let (mut reloaded, scene, ids) = load(&again).expect("what the writer wrote loads");
    assert_eq!(
        scene
            .save(&mut reloaded, &ids, &registry.codecs())
            .expect("saves"),
        written,
    );
}

/// **A key that is not one a mesh may name is refused on load, naming the
/// field and the file, and by the check** — each rule on its own.
#[test]
fn an_asset_key_a_mesh_may_not_name_is_refused_on_load_and_by_the_check() {
    let registry = registry();
    for (asset, says) in [
        ("/etc/crate.glb", "relative to the asset root"),
        ("C:/crate.glb", "relative to the asset root"),
        ("props/../../crate.glb", "`..`"),
        ("props/crate.png", "`.glb` or `.gltf`"),
        ("props/crate", "`.glb` or `.gltf`"),
        ("props/my crate.glb", "an asset key"),
        ("props//crate.glb", "an asset key"),
        ("./props/crate.glb", "spelled `props/crate.glb`"),
    ] {
        let source = scene(&format!(
            "(0, (asset: {asset:?}, position: (0.0, 0.0, 0.0)))"
        ));
        let error = load(&source)
            .err()
            .unwrap_or_else(|| panic!("`{asset}` loaded"));
        assert!(
            matches!(&error, ScnError::Parse { key, message, .. }
                if key == "sys/meshes.ron" && message.contains(says)),
            "`{asset}` was refused without saying {says} in its file: {error}",
        );
        let problems = registry.problems(&[MESHES.to_owned()], &source, Path::new(""));
        assert!(
            problems.len() == 1 && problems[0].contains(says),
            "the check did not report `{asset}`: {problems:?}",
        );
    }
    for asset in ["", "crate.glb", "props/Crate.GLTF", "a-b_c/d.e.glb"] {
        assert_eq!(check_asset(asset), Ok(()), "`{asset}` was refused");
    }
    assert!(
        registry
            .problems(
                &[MESHES.to_owned()],
                &scene("(0, (asset: \"crate.glb\", position: (0.0, 0.0, 0.0)))"),
                Path::new(""),
            )
            .is_empty(),
        "the check refused a key a mesh may name",
    );
}

/// A new mesh has no asset chosen, which is a key the loader takes back.
#[test]
fn a_new_mesh_has_no_asset_and_reloads() {
    let row = registry()
        .default_row(MESHES)
        .expect("meshes is registered");
    assert_eq!(row, "(asset:\"\",position:(0.0,0.0,0.0))");
    load(&scene(&format!("(0, {row})"))).expect("a new mesh's row loads");
}

/// The asset key, the position and the rotation are rows and the box is not:
/// the box is a fact about the asset, never edited.
#[test]
fn the_inspector_sees_the_asset_position_and_rotation_and_not_the_box() {
    let mesh = Mesh::new("props/crate.glb", [1.0, 2.0, 3.0]);
    let labels: Vec<&str> = mesh.fields().iter().map(|field| field.label).collect();
    assert_eq!(labels, ["Asset", "Position", "Rotation"]);
    assert_eq!(
        get_path(&mesh, "asset"),
        Ok(Value::Text("props/crate.glb".to_owned()))
    );
}

/// **An unmeasured mesh is the placeholder cube centred on its origin**, and
/// a measured one is its box offset by its position, a flat axis given
/// [`MIN_HALF_EXTENT`].
#[test]
fn a_meshs_placement_is_its_measured_box_or_the_placeholder() {
    let mut mesh = Mesh::new("props/sign.glb", [1.0, 2.0, 3.0]);
    assert_eq!(
        mesh.placement(),
        Some(OrientedBox::axis_aligned(
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::splat(PLACEHOLDER_HALF_EXTENT)
        )),
    );
    mesh.set_local_bounds(DVec3::new(-1.0, 0.0, 0.0), DVec3::new(1.0, 4.0, 0.0));
    assert_eq!(
        mesh.placement(),
        Some(OrientedBox::axis_aligned(
            DVec3::new(1.0, 4.0, 3.0),
            DVec3::new(1.0, 2.0, MIN_HALF_EXTENT)
        )),
    );

    // Retyping the key leaves the old asset's box behind.
    set_path(
        &mut mesh,
        "asset",
        &Value::Text("props/crate.glb".to_owned()),
    )
    .expect("the asset is text");
    assert_eq!(mesh.local_bounds(), None);
    assert_eq!(
        mesh.placement(),
        Some(OrientedBox::axis_aligned(
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::splat(PLACEHOLDER_HALF_EXTENT)
        )),
    );
}

/// **A mesh dropped on a point stands on it**: the bottom centre of its box,
/// or of the placeholder, is the point.
#[test]
fn a_mesh_standing_on_a_point_has_its_foot_there() {
    let point = DVec3::new(2.0, 1.0, -3.0);
    let local = (DVec3::new(10.0, 5.0, 0.0), DVec3::new(11.0, 6.0, 2.0));
    let mut mesh = Mesh::standing_on("a.glb", point, Some(local));
    mesh.set_local_bounds(local.0, local.1);
    let placed = mesh.placement().expect("placed");
    assert_eq!(
        placed.centre - DVec3::new(0.0, placed.half_extents.y, 0.0),
        point
    );

    let placeholder = Mesh::standing_on("missing.glb", point, None);
    let placed = placeholder.placement().expect("placed");
    assert_eq!(
        placed.centre - DVec3::new(0.0, placed.half_extents.y, 0.0),
        point
    );
}

#[cfg(feature = "scene")]
mod measured {
    use super::*;

    use crcbl_scene::gltf_fixture::{BIN_CHUNK_BUFFER, glb, triangle_bin, triangle_json};

    /// The fixture triangle: its vertices are `(0,0,0)`, `(1,0,0)` and
    /// `(0,1,0)` under a node at `(10,0,0)` holding one at `(0,5,0)`.
    const TRIANGLE: &str = "props/triangle.glb";

    /// The triangle's box in its own frame, through both nodes.
    const TRIANGLE_MIN: DVec3 = DVec3::new(10.0, 5.0, 0.0);
    const TRIANGLE_MAX: DVec3 = DVec3::new(11.0, 6.0, 0.0);

    /// An asset root holding the triangle.
    fn assets() -> MemorySource {
        let mut source = MemorySource::new();
        source
            .insert(
                Path::new(TRIANGLE),
                glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin())),
            )
            .expect("a legal key");
        source
    }

    /// **The box is the asset's, measured through its nodes**, and a resolved
    /// row is placed by it: the triangle is a flat thing, so its depth is
    /// [`MIN_HALF_EXTENT`].
    #[test]
    fn a_resolved_mesh_is_placed_by_its_assets_box() {
        let rows = format!("(0, (asset: {TRIANGLE:?}, position: (1.0, 0.0, 2.0)))");
        let (mut world, _, ids) = load(&scene(&rows)).expect("the scene loads");
        let mut library = MeshLibrary::new();
        assert_eq!(
            library.measure(&assets(), TRIANGLE),
            Ok((TRIANGLE_MIN, TRIANGLE_MAX))
        );

        let resolution = library.resolve(&mut world, &assets());
        let entity = ids.entity(SceneEntityId(0)).expect("in the scene");
        assert_eq!(resolution.moved, [entity]);
        assert_eq!(resolution.problems, []);
        assert_eq!(
            registry().placement(&mut world, entity),
            Some(OrientedBox::axis_aligned(
                DVec3::new(11.5, 5.5, 2.0),
                DVec3::new(0.5, 0.5, MIN_HALF_EXTENT)
            )),
        );
        assert_eq!(
            library.resolve(&mut world, &assets()),
            Resolution::default(),
            "a resolved row moved again",
        );
    }

    /// **A missing asset is the placeholder and a problem naming it**, and an
    /// asset that is not a glTF is the same with the importer's reason —
    /// neither panics.
    #[test]
    fn a_missing_or_broken_asset_is_a_placeholder_and_a_named_problem() {
        let rows = format!(
            "(0, (asset: \"props/gone.glb\", position: (0.0, 0.0, 0.0))), \
             (1, (asset: \"props/junk.glb\", position: (4.0, 0.0, 0.0))), \
             (2, (asset: {TRIANGLE:?}, position: (0.0, 0.0, 0.0))), \
             (3, (asset: \"\", position: (0.0, 0.0, 0.0)))"
        );
        let (mut world, _, ids) = load(&scene(&rows)).expect("the scene loads");
        let mut source = assets();
        source
            .insert(Path::new("props/junk.glb"), b"not a model".to_vec())
            .expect("a legal key");

        let resolution = MeshLibrary::new().resolve(&mut world, &source);
        let messages: Vec<String> = resolution
            .problems
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert!(
            messages[0].contains("`props/gone.glb` would not load"),
            "{messages:?}"
        );
        assert!(
            messages[1].contains("`props/junk.glb` would not load"),
            "{messages:?}"
        );
        assert!(messages[2].contains("no asset chosen"), "{messages:?}");
        assert_eq!(
            resolution.problems[0].entity,
            ids.entity(SceneEntityId(0)).expect("in the scene")
        );
        for (id, centre) in [(0, DVec3::ZERO), (1, DVec3::new(4.0, 0.0, 0.0))] {
            let entity = ids.entity(SceneEntityId(id)).expect("in the scene");
            assert_eq!(
                registry().placement(&mut world, entity),
                Some(OrientedBox::axis_aligned(
                    centre,
                    DVec3::splat(PLACEHOLDER_HALF_EXTENT)
                )),
                "#{id} is not the placeholder",
            );
        }
    }

    /// A retyped key is measured again on the next resolve, and the old box
    /// goes with the old key.
    #[test]
    fn a_retyped_asset_is_measured_again() {
        let rows = format!("(0, (asset: {TRIANGLE:?}, position: (0.0, 0.0, 0.0)))");
        let (mut world, _, ids) = load(&scene(&rows)).expect("the scene loads");
        let entity = ids.entity(SceneEntityId(0)).expect("in the scene");
        let mut library = MeshLibrary::new();
        library.resolve(&mut world, &assets());
        let row = meshes(&mut world)
            .and_then(|system| system.get_mut(entity))
            .expect("a mesh");
        row.asset = "props/gone.glb".to_owned();
        // Placed as the placeholder from the moment the key changed, which is
        // when a tool rebuilds what follows a placement — so the resolve moves
        // nothing more, and says why the box is gone.
        assert_eq!(
            row.placement(),
            Some(OrientedBox::axis_aligned(
                DVec3::ZERO,
                DVec3::splat(PLACEHOLDER_HALF_EXTENT)
            ))
        );
        let resolution = library.resolve(&mut world, &assets());
        assert_eq!(resolution.moved, []);
        assert_eq!(resolution.problems.len(), 1);
        assert_eq!(resolution.problems[0].asset, "props/gone.glb");

        // And back: the triangle's box, remembered, places it again.
        meshes(&mut world)
            .and_then(|system| system.get_mut(entity))
            .expect("a mesh")
            .asset = TRIANGLE.to_owned();
        assert_eq!(library.resolve(&mut world, &assets()).moved, [entity]);
        assert_eq!(
            mesh(&mut world, &ids, 0).local_bounds(),
            Some((TRIANGLE_MIN, TRIANGLE_MAX))
        );
    }
}

/// **A turned mesh round-trips with its rotation as four numbers**, and its
/// file reads back to the same row — where an unturned one writes none
/// (`a_mesh_round_trips_and_its_measured_box_is_not_written`).
#[test]
fn a_turned_mesh_round_trips_with_its_rotation() {
    let rows = "(0, (asset: \"a.glb\", position: (1.0, 0.0, 0.0), \
                rotation: (0.0, 0.6, 0.0, 0.8)))";
    let (mut world, scene, ids) = load(&scene(rows)).expect("the scene loads");
    let loaded = mesh(&mut world, &ids, 0);
    assert_eq!(loaded.rotation.to_array(), [0.0, 0.6, 0.0, 0.8]);
    let written = scene
        .save(&mut world, &ids, &registry().codecs())
        .expect("the scene saves");
    assert!(
        written["sys/meshes.ron"].contains("rotation: (0.0, 0.6, 0.0, 0.8),"),
        "{}",
        written["sys/meshes.ron"],
    );
    let mut again = MemorySource::new();
    for (key, text) in &written {
        again
            .insert(Path::new(key), text.clone().into_bytes())
            .expect("a scene key");
    }
    let (mut reloaded, _, ids) = load(&again).expect("what the writer wrote loads");
    assert_eq!(mesh(&mut reloaded, &ids, 0), loaded);
}

/// **A rotation that is not a unit quaternion is refused on load, naming the
/// file, and by the check** — a length off one and a number that is not
/// finite, each on its own.
#[test]
fn a_rotation_that_is_not_a_rotation_is_refused_on_load_and_by_the_check() {
    let registry = registry();
    for (rotation, says) in [
        ("(0.0, 0.0, 0.0, 2.0)", "unit quaternion"),
        ("(0.0, 0.0, 0.0, 0.0)", "unit quaternion"),
        ("(NaN, 0.0, 0.0, 1.0)", "finite"),
    ] {
        let source = scene(&format!(
            "(0, (asset: \"a.glb\", position: (0.0, 0.0, 0.0), rotation: {rotation}))"
        ));
        let error = load(&source)
            .err()
            .unwrap_or_else(|| panic!("{rotation} loaded"));
        assert!(
            matches!(&error, ScnError::Parse { key, message, .. }
                if key == "sys/meshes.ron" && message.contains(says)),
            "{rotation} was refused without saying {says} in its file: {error}",
        );
        let problems = registry.problems(&[MESHES.to_owned()], &source, Path::new(""));
        assert!(
            problems.len() == 1 && problems[0].contains(says),
            "the check did not report {rotation}: {problems:?}",
        );
    }
}

/// **A turned mesh's box swings about its origin**: a quarter turn about
/// `+Y` takes a box reaching out along the asset's own `+X` round to the
/// world's `-Z`, and turns the box with it.
#[test]
fn a_turned_meshs_box_swings_about_its_origin() {
    let mut mesh = Mesh::new("a.glb", [1.0, 0.0, 0.0]);
    mesh.rotation = Rotation::new(glam::DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2))
        .expect("a quarter turn is unit");
    mesh.set_local_bounds(DVec3::new(2.0, -1.0, -1.0), DVec3::new(4.0, 1.0, 1.0));
    let placed = mesh.placement().expect("placed");
    assert!(
        placed.centre.abs_diff_eq(DVec3::new(1.0, 0.0, -3.0), 1e-12),
        "the box stands at {}",
        placed.centre,
    );
    assert_eq!(placed.half_extents, DVec3::ONE);
    assert_eq!(placed.rotation, mesh.rotation.quat());
}
