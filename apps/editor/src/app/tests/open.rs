//! Open through the loop: Ctrl+O and the toolbar's button, a scene directory
//! typed on the path line, what the editor holds once it is in place, and
//! what is refused.

use super::*;

use std::collections::BTreeMap;
use std::path::PathBuf;

use crcbl::math::DVec3;

use super::files::{chord, type_and_enter};
use crate::document::mesh_tests::TRIANGLE;
use crate::document::origin_tests::{game, tree};

/// A scene of one mesh of the fixture triangle, saved at `levels/one.scn`
/// under `game` — whose asset root is the game's own, so the mesh is
/// measured once it is opened from there. Hands back the directory.
fn mesh_scene(game: &Path) -> PathBuf {
    let dir = game.join("levels").join("one.scn");
    let mut document = crate::scene::built_in_document().expect("the compiled-in scene");
    document.new_scene().expect("editing");
    document
        .spawn_mesh(TRIANGLE, DVec3::ZERO)
        .expect("a mesh key");
    document.save_as(&dir).expect("a fresh directory");
    dir
}

/// Makes the editor's scene dirty, with something selected.
fn dirty(editor: &mut Editor<HeadlessShell>) {
    editor.document_mut().select(Some(SceneEntityId(0)));
    editor.act(&Action::Nudge { axis: 1, sign: 1.0 });
    assert!(editor.document().is_dirty());
}

/// **Ctrl+O and a typed scene directory put that scene in place**: its
/// entities, its directory, its game's assets measured and listed and on the
/// renderer's shelf, nothing selected and nothing to undo.
#[test]
fn ctrl_o_opens_a_typed_scene_directory() {
    let game = game();
    let dir = mesh_scene(game.path());
    // Nothing edited: an untouched document's counts are where a freshly
    // opened one's are, so the renderer's rebuild cannot ride on them moving.
    let mut editor = headless(64);
    editor.document_mut().select(Some(SceneEntityId(0)));
    assert!(!editor.panels.listed_assets().contains(&TRIANGLE));

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyO);
    assert_eq!(editor.panels.opening(), Some(""), "no line was opened");
    type_and_enter(&mut editor, &dir.display().to_string());

    assert_eq!(
        editor.panels.unsaved(),
        None,
        "a clean scene was asked about"
    );
    assert_eq!(editor.document().origin(), Some(dir.as_path()));
    assert_eq!(editor.document().name(), "one");
    assert_eq!(editor.document().entity_count(), 1);
    assert_eq!(
        editor.document().selection(),
        [],
        "the old selection stayed"
    );
    assert!(
        editor.document().mesh_problems().is_empty(),
        "not measured from the game's root: {:?}",
        editor.document().mesh_problems()
    );
    assert!(
        editor.panels.listed_assets().contains(&TRIANGLE),
        "the browser lists {:?}",
        editor.panels.listed_assets()
    );
    editor.frame().expect("a frame");
    assert!(
        editor.shelf.assets().contains(TRIANGLE),
        "the renderer was not rebuilt from the opened scene's assets"
    );
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Info, "{text}");
    assert!(text.starts_with("Opened"), "{text}");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **An asset root `--assets` named stays named** for a scene opened in the
/// run: its meshes are read from there, not from the scene's game.
#[test]
fn a_named_asset_root_stays_for_an_opened_scene() {
    let game = game();
    let dir = mesh_scene(game.path());
    let elsewhere = tempfile::tempdir().expect("a temporary directory");
    let mut options = options(64);
    options.assets = Some(elsewhere.path().to_path_buf());
    let mut editor = Editor::with_shell(Box::new(HeadlessShell::new()), &options)
        .expect("the null backend runs everywhere");

    editor.act(&Action::Open);
    type_and_enter(&mut editor, &dir.display().to_string());
    assert_eq!(editor.document().origin(), Some(dir.as_path()));
    assert!(
        !editor.document().mesh_problems().is_empty(),
        "the mesh was measured from the scene's game, not the named root"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A dirty scene is asked about before an open**, once the typed scene has
/// been read: the bar names the directory, the scene stays until Discard,
/// and Discard opens it with a history of its own.
#[test]
fn a_dirty_open_asks_and_discard_opens() {
    let game = game();
    let dir = mesh_scene(game.path());
    let mut editor = headless(64);
    dirty(&mut editor);

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyO);
    type_and_enter(&mut editor, &dir.display().to_string());
    let asked = editor.panels.unsaved().expect("the open asked nothing");
    assert!(asked.contains(&dir.display().to_string()), "{asked}");
    assert_eq!(editor.document().origin(), None, "opened before the answer");
    assert!(editor.document().is_dirty());

    tap(&mut editor, KeyCode::KeyD);
    assert_eq!(editor.panels.unsaved(), None);
    assert_eq!(editor.document().origin(), Some(dir.as_path()));
    assert!(!editor.document().is_dirty());
    assert!(editor.document().log().is_empty(), "the old history stayed");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A directory that is not a scene is refused by name**, the line opening
/// again holding what was typed — and nothing is asked and nothing changes.
#[test]
fn a_directory_that_is_not_a_scene_is_refused_by_name() {
    let empty = tempfile::tempdir().expect("a temporary directory");
    let typed = empty.path().display().to_string();
    let mut editor = headless(64);
    dirty(&mut editor);
    let before = editor.document_mut().files().expect("ids");

    editor.act(&Action::Open);
    type_and_enter(&mut editor, &typed);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains(&typed), "{text}");
    assert!(text.contains("not a scene"), "{text}");
    assert_eq!(editor.panels.opening(), Some(typed.as_str()));
    assert_eq!(editor.panels.unsaved(), None, "a refused open asked");
    assert_eq!(editor.document_mut().files().expect("ids"), before);
    assert!(editor.document().is_dirty());
    assert_eq!(tree(empty.path()), BTreeMap::new(), "something was written");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Open is refused in play mode**, on the status line, and opens no line.
#[test]
fn open_is_refused_in_play_mode() {
    let mut editor = headless(16);
    editor.act(&Action::PlayStop);
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyO);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("play mode"), "{text}");
    assert_eq!(editor.panels.opening(), None);
    assert_eq!(editor.document().play_state(), PlayState::Playing);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The toolbar's Open is Ctrl+O's**: it opens the path line for a scene.
#[test]
fn the_toolbar_asks_for_a_scene_to_open() {
    let mut editor = headless(32);
    editor.frame().expect("a frame");
    let [_, open, _] = editor.panels.file_buttons();
    let at = centre(&editor, open);
    click(&mut editor, at);
    assert_eq!(editor.panels.opening(), Some(""));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **An opened scene is drawn from its own game's files**, even where its
/// meshes name the keys the old scene's did: the renderer is rebuilt from the
/// new asset root rather than kept because it already holds those keys. Here
/// the new game has no file under the key, so a renderer read from it holds
/// none, where the old one still held the old game's triangle.
#[test]
fn an_opened_scene_is_drawn_from_its_own_game() {
    let first = game();
    let first_scene = mesh_scene(first.path());
    let second = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(second.path().join("Cargo.toml"), "[package]\n").expect("writable");
    let second_scene = mesh_scene(second.path());
    let mut options = options(64);
    options.scene = Some(first_scene);
    let mut editor = Editor::with_shell(Box::new(HeadlessShell::new()), &options)
        .expect("the null backend runs everywhere");
    editor.frame().expect("a frame");
    assert!(
        editor.shelf.assets().contains(TRIANGLE),
        "the first game's mesh"
    );

    editor.act(&Action::Open);
    let holding = format!(
        "{}{}",
        first.path().join("levels").display(),
        std::path::MAIN_SEPARATOR
    );
    assert_eq!(
        editor.panels.opening(),
        Some(holding.as_str()),
        "the line does not start where the scene is"
    );
    editor
        .panels
        .begin_open(&editor.document, String::new())
        .expect("editing");
    type_and_enter(&mut editor, &second_scene.display().to_string());
    assert_eq!(editor.document().origin(), Some(second_scene.as_path()));
    editor.frame().expect("a frame");
    assert!(
        !editor.shelf.assets().contains(TRIANGLE),
        "the renderer still draws the first game's file under the key"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
