//! `docs/plan/08-editor.md`'s first exit criterion, end to end through the
//! loop: create a scene from empty, place a mesh from the asset browser,
//! transform it with a gizmo, edit a property, save, reopen, play and stop —
//! every step a key, a click, a drag or typed text delivered through the
//! headless shell, and no file written by anything but the editor's own
//! commands.

use super::*;

use std::collections::BTreeMap;
use std::path::PathBuf;

use crcbl::scene_mesh::MESHES;
use crcbl::scene_physics::BODIES;

use super::assets::{pixel_of, row_of};
use super::clipboard::leaf_field;
use super::files::{chord, type_and_enter};
use crate::document::mesh_tests::TRIANGLE;
use crate::document::origin_tests::{game, tree};

/// The frames each editor here may run: enough for every step's clicks and
/// drags, and for the fall.
const FRAMES: u64 = 2_000;

/// A frame of play: one tick at the world's default rate, on a manual clock
/// so the fall is the same on every machine.
const FRAME_TIME: Duration = Duration::from_micros(16_667);

/// How many frames the scene plays before it is stopped.
const PLAYED_FRAMES: usize = 30;

/// A body's rows in the inspector: its kind, then its mass.
const MASS_ROW: usize = 1;

/// The headless editor over `scene` — or the compiled-in scene — reading
/// assets from `assets` when it is named, on a manual clock.
fn editor_over(scene: Option<PathBuf>, assets: Option<PathBuf>) -> Editor<HeadlessShell> {
    let mut options = options(FRAMES);
    options.scene = scene;
    options.assets = assets;
    let mut editor = Editor::with_shell(Box::new(HeadlessShell::new()), &options)
        .expect("the null backend runs everywhere");
    editor.clock_source = Clock::manual(FRAME_TIME);
    editor
}

/// The document's files as the bytes [`tree`] reads off a disk, each key
/// under `prefix`.
fn on_disk_as(editor: &mut Editor<HeadlessShell>, prefix: &str) -> BTreeMap<String, Vec<u8>> {
    editor
        .document_mut()
        .files()
        .expect("every entity has an id")
        .into_iter()
        .map(|(key, text)| (format!("{prefix}{key}"), text.into_bytes()))
        .collect()
}

/// The one entity a scene holds.
fn only_entity(editor: &mut Editor<HeadlessShell>) -> SceneEntityId {
    let ids: Vec<SceneEntityId> = editor
        .document_mut()
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect();
    assert_eq!(ids.len(), 1, "the scene holds {ids:?}");
    ids[0]
}

/// The mesh's `position`, as the gizmo writes it.
fn position(editor: &mut Editor<HeadlessShell>, id: SceneEntityId) -> [f64; 3] {
    [0, 1, 2].map(|axis| {
        match editor
            .document_mut()
            .read(id, MESHES, &format!("position.{axis}"))
        {
            Ok(Value::Float(value)) => value,
            other => panic!("position.{axis} of {id} reads {other:?}"),
        }
    })
}

/// The body's mass.
fn mass(editor: &mut Editor<HeadlessShell>, id: SceneEntityId) -> f64 {
    match editor.document_mut().read(id, BODIES, "mass") {
        Ok(Value::Float(mass)) => mass,
        other => panic!("the mass of {id} reads {other:?}"),
    }
}

/// **Empty scene to play and stop without a text editor.** The steps, each
/// asserted as it lands:
///
/// 1. Ctrl+N on the compiled-in scene: nothing in it, no system listed, no
///    directory — and the browser lists the game's mesh.
/// 2. The mesh dragged from its browser row onto the ground in the viewport:
///    one mesh of it, measured from the asset (no placeholder), `meshes`
///    listed, selected.
/// 3. The translate gizmo's X arrow dragged: the mesh moves along X alone.
/// 4. The inspector's add button for `bodies` clicked, and the body's mass
///    dragged: a body on the mesh, `bodies` listed, the mass changed.
/// 5. The toolbar's Save as, a directory under the game typed, Enter: the
///    scene's files there and nowhere else, the document's own.
/// 6. A fresh editor opened on that directory: the same files, the mesh
///    measured from the game's root, the move and the mass as they were left.
/// 7. F5: the body falls.
/// 8. F5 again: the scene is back to the saved files byte for byte.
///
/// The game's folder is compared whole after every step: nothing is
/// written until the save-as, the save-as writes the scene's files and
/// nothing else, and play and stop write nothing at all.
#[test]
fn empty_scene_to_play_and_stop_without_a_text_editor() {
    let game = game();
    let fixture = tree(game.path());
    let scene = game.path().join("levels").join("first.scn");
    let mut editor = editor_over(None, Some(game.path().to_path_buf()));
    editor.frame().expect("a frame");

    // 1. A scene from empty.
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert_eq!(editor.document().entity_count(), 0, "1: not empty");
    assert!(
        editor.document_mut().outline().is_empty(),
        "1: a system is listed"
    );
    assert_eq!(editor.document().origin(), None, "1: it has a directory");
    assert!(
        editor.panels.listed_assets().contains(&TRIANGLE),
        "1: the browser does not list the game's mesh: {:?}",
        editor.panels.listed_assets(),
    );
    assert_eq!(tree(game.path()), fixture, "1: something was written");

    // 2. A mesh placed from the asset browser.
    let eye = editor.camera.camera().eye;
    assert!(eye.y > 0.0, "2: the camera is not above the ground: {eye}");
    let grab = row_of(&editor, TRIANGLE);
    let release = pixel_of(&editor, Vec3::ZERO);
    drag(&mut editor, grab, release);
    let id = only_entity(&mut editor);
    assert_eq!(editor.document().primary(), Some(id), "2: not selected");
    assert_eq!(
        editor
            .document_mut()
            .mesh(id)
            .map(|mesh| mesh.asset.clone()),
        Some(TRIANGLE.to_owned()),
        "2: not a mesh of the dragged asset",
    );
    assert!(
        editor.document().mesh_problems().is_empty(),
        "2: drawn as a placeholder: {:?}",
        editor.document().mesh_problems(),
    );
    assert_eq!(editor.document_mut().systems_of(id), [MESHES]);
    assert_eq!(tree(game.path()), fixture, "2: something was written");

    // 3. Moved with the gizmo.
    editor.frame().expect("a frame");
    let placed = position(&mut editor, id);
    let (from, to) = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    drag(&mut editor, (from + to) * 0.5, to + (to - from) * 0.5);
    let moved = position(&mut editor, id);
    assert!(
        moved[0] > placed[0] + 0.1,
        "3: not moved along X: {placed:?} to {moved:?}"
    );
    assert_eq!(
        [moved[1], moved[2]],
        [placed[1], placed[2]],
        "3: moved off X"
    );
    assert_eq!(tree(game.path()), fixture, "3: something was written");

    // 4. A property edited in the inspector.
    let add = editor
        .panels
        .add_buttons()
        .into_iter()
        .find_map(|(system, key)| (system == BODIES).then_some(key))
        .expect("4: the inspector offers no body");
    let at = centre(&editor, add);
    click(&mut editor, at);
    assert_eq!(
        editor.document_mut().systems_of(id),
        [MESHES, BODIES],
        "4: the add button attached no body",
    );
    let was = mass(&mut editor, id);
    let section = editor
        .panels
        .section_systems()
        .iter()
        .position(|system| system == BODIES)
        .expect("4: the inspector drew no body section");
    let field = leaf_field(&editor, section, MASS_ROW);
    let field = Vec2::new(field.x as f32, field.y as f32);
    drag(&mut editor, field, field + Vec2::new(60.0, 0.0));
    let edited = mass(&mut editor, id);
    assert!(
        edited > was,
        "4: the mass drag did not raise it: {was} to {edited}"
    );
    assert_eq!(tree(game.path()), fixture, "4: something was written");

    // 5. Saved, into a directory typed on the save-as line.
    let [_, save_as] = editor.panels.file_buttons();
    let at = centre(&editor, save_as);
    click(&mut editor, at);
    type_and_enter(&mut editor, &scene.display().to_string());
    assert_eq!(
        editor.document().origin(),
        Some(scene.as_path()),
        "5: not its own"
    );
    assert!(!editor.document().is_dirty(), "5: still dirty");
    let saved = on_disk_as(&mut editor, "levels/first.scn/");
    let mut expected = fixture.clone();
    expected.extend(saved.clone());
    assert_eq!(
        tree(game.path()),
        expected,
        "5: the folder is not the fixture and the scene's files"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");

    // 6. Reopened, in a fresh editor, from the directory alone.
    let mut editor = editor_over(Some(scene.clone()), None);
    editor.frame().expect("a frame");
    assert_eq!(
        on_disk_as(&mut editor, "levels/first.scn/"),
        saved,
        "6: not what was saved"
    );
    let id = only_entity(&mut editor);
    assert!(
        editor.document().mesh_problems().is_empty(),
        "6: the mesh is not measured from the game's root: {:?}",
        editor.document().mesh_problems(),
    );
    assert_eq!(position(&mut editor, id), moved, "6: the move was lost");
    assert_eq!(mass(&mut editor, id), edited, "6: the mass was lost");

    // 7. Played: the body falls.
    tap(&mut editor, KeyCode::F5);
    assert_eq!(
        editor.document().play_state(),
        PlayState::Playing,
        "7: not playing"
    );
    let start = position(&mut editor, id)[1];
    for _ in 0..PLAYED_FRAMES {
        editor.frame().expect("a frame");
    }
    let fallen = position(&mut editor, id)[1];
    assert!(
        fallen < start - 0.5,
        "7: the body did not fall: {start} to {fallen}"
    );
    assert_eq!(tree(game.path()), expected, "7: play wrote something");

    // 8. Stopped: the saved scene, byte for byte.
    tap(&mut editor, KeyCode::F5);
    assert_eq!(
        editor.document().play_state(),
        PlayState::Editing,
        "8: still playing"
    );
    assert_eq!(
        on_disk_as(&mut editor, "levels/first.scn/"),
        saved,
        "8: not restored"
    );
    assert_eq!(position(&mut editor, id), moved, "8: the body is not back");
    assert_eq!(tree(game.path()), expected, "8: stop wrote something");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
