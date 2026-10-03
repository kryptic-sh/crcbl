//! The history beside a scene, `.crcbl-history`, through the loop: what the
//! `crcbl scene` CLI wrote is undone with Ctrl+Z, what Save writes is undone
//! from the CLI, a refused history is said and left alone, and a drag is one
//! entry on disk.
//!
//! The CLI's side is the [`Document`] calls `crcbl scene` makes —
//! `Document::open_with_history`, the field paste its `move` is, and
//! `Document::save_with_history` — since this crate cannot run that binary:
//! the CLI depends on the editor, not the other way round.

use super::*;

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::document::origin_tests::tree;
use crate::document::{HISTORY, HistoryError};

use super::files::{chord, type_and_enter};

/// The frames each editor here may run.
const FRAMES: u64 = 256;

/// Towers' build plots.
const PLOTS: &str = "plots";

/// Where `crcbl scene move` puts the plot here, as typed on its command line.
const MOVED_TO: [&str; 3] = ["11.5", "0.0", "-2.25"];

/// A copy of towers' committed field in a fresh directory under `base`.
fn towers_field(base: &Path) -> PathBuf {
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../towers/assets/scenes/field.scn");
    let dir = base.join("field.scn");
    for (key, bytes) in tree(&committed) {
        let path = dir.join(&key);
        std::fs::create_dir_all(path.parent().expect("under the copy")).expect("writable");
        std::fs::write(path, bytes).expect("writable");
    }
    dir
}

/// The scene's own files in `dir`, byte for byte — every file but the
/// history.
fn scene_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = tree(dir);
    files.remove(HISTORY);
    files
}

/// The scene at `dir` opened as `crcbl scene` opens it, with its history.
fn cli(dir: &Path) -> Document {
    Document::open_with_history(dir, crate::scene::vocabulary())
        .expect("the CLI reads the history the last save wrote")
}

/// `crcbl scene move <DIR> <plot> 11.5 0.0 -2.25`: the first plot's position
/// pasted as text, three writes as one entry, saved with the history.
fn cli_move_a_plot(dir: &Path) {
    let mut document = cli(dir);
    let plot = document.entities_in(PLOTS)[0];
    let system = document
        .placing_system(plot)
        .expect("a plot is a thing in space");
    let fields: Vec<(String, &str)> = MOVED_TO
        .iter()
        .enumerate()
        .map(|(axis, value)| (format!("{}.{axis}", crcbl::registry::POSITION), *value))
        .collect();
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(path, value)| (path.as_str(), *value))
        .collect();
    document
        .paste_fields(plot, &system, &fields)
        .expect("a plot moves");
    document.save_with_history().expect("saved");
}

/// An editor started on the scene directory `scene`, as `editor <SCENE_DIR>`
/// starts.
fn editor_on(scene: &Path) -> Editor<HeadlessShell> {
    let mut options = options(FRAMES);
    options.scene = Some(scene.to_path_buf());
    Editor::with_shell(Box::new(HeadlessShell::new()), &options)
        .expect("the null backend opens the scene")
}

/// **A towers plot moved from the CLI is undone in the editor, and the
/// editor's save is undone and redone from the CLI** — the files on disk
/// byte for byte at every step, and the dirty marker right at each.
#[test]
fn a_plot_moved_from_the_cli_undoes_in_the_editor_and_its_save_from_the_cli() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let field = scene_files(&dir);

    cli_move_a_plot(&dir);
    let moved = scene_files(&dir);
    assert_ne!(moved, field, "the CLI's move changed nothing");

    let mut editor = editor_on(&dir);
    assert_eq!(
        (
            editor.document().log().position(),
            editor.document().log().len()
        ),
        (1, 1),
        "the editor did not read the CLI's history"
    );
    assert!(!editor.document().is_dirty(), "opened with history, dirty");
    assert!(!editor.document().title().starts_with('*'));

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert!(
        editor.document().is_dirty(),
        "an undo past the save is clean"
    );
    assert!(editor.document().title().starts_with('*'));
    let undone: BTreeMap<String, Vec<u8>> = editor
        .document_mut()
        .files()
        .expect("every entity has an id")
        .into_iter()
        .map(|(key, text)| (key, text.into_bytes()))
        .collect();
    assert_eq!(undone, field, "the undo did not put the plot back");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyY);
    assert!(!editor.document().is_dirty(), "a redo to the save is dirty");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert!(!editor.document().is_dirty(), "the save left it dirty");
    assert_eq!(editor.panels.status(), ("Saved", Tone::Info));
    assert_eq!(scene_files(&dir), field, "the save is not the field");
    editor.finish(ExitReason::FrameBudget).expect("teardown");

    let mut second = cli(&dir);
    assert_eq!(
        (second.log().position(), second.log().len()),
        (0, 1),
        "the editor's save wrote another history than its log"
    );
    assert!(
        !second.undo().expect("nothing is applied"),
        "undid past the start"
    );
    assert!(second.redo().expect("the CLI's move redoes"));
    second.save_with_history().expect("saved");
    assert_eq!(scene_files(&dir), moved, "the redo is not the CLI's move");
}

/// **A refused history is said, opens as an empty log, and is left alone**
/// until the next save replaces it with one the CLI reads back.
#[test]
fn a_refused_history_is_said_and_left_alone_until_a_save_replaces_it() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    cli_move_a_plot(&dir);
    let path = dir.join(HISTORY);
    let mut tampered = std::fs::read(&path).expect("a history");
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    std::fs::write(&path, &tampered).expect("written");

    let mut editor = headless(FRAMES);
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyO);
    type_and_enter(&mut editor, &dir.display().to_string());
    assert_eq!(
        editor.document().origin(),
        Some(dir.as_path()),
        "not opened"
    );
    assert!(
        editor.document().log().is_empty(),
        "a refused history replayed"
    );
    assert!(!editor.document().is_dirty());
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains(HISTORY), "{text}");
    assert!(
        text.contains(&HistoryError::Checksum.to_string()),
        "the status does not say why: {text}"
    );
    assert!(text.contains("next save replaces it"), "{text}");
    assert_eq!(std::fs::read(&path).expect("still there"), tampered);

    let plot = editor.document_mut().entities_in(PLOTS)[0];
    editor.document_mut().select(Some(plot));
    editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
    assert_eq!(
        cli(&dir).log().len(),
        1,
        "the save did not replace the refused history"
    );
}

/// **A refused history is said when the editor starts on the scene, too.**
#[test]
fn a_refused_history_is_said_when_the_editor_starts_on_the_scene() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    cli_move_a_plot(&dir);
    std::fs::write(dir.join(HISTORY), b"not a history").expect("written");

    let editor = editor_on(&dir);
    assert!(editor.document().log().is_empty());
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.starts_with("Opened"), "{text}");
    assert!(text.contains(HISTORY), "{text}");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A gizmo drag saved is one entry on disk**, whose undo from the CLI puts
/// the block back where the drag found it.
#[test]
fn a_gizmo_drag_saved_is_one_entry_on_disk() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = base.path().join("greybox.scn");
    let mut editor = headless(FRAMES);
    editor
        .document_mut()
        .save_as(&dir)
        .expect("a fresh directory");
    let before = scene_files(&dir);
    let id = SceneEntityId(2);
    editor.document_mut().select(Some(id));
    editor.frame().expect("a frame");

    let (from, to) = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    drag(&mut editor, (from + to) * 0.5, to + (to - from) * 0.5);
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    let dragged = scene_files(&dir);
    assert_ne!(dragged, before, "the drag moved nothing");
    editor.finish(ExitReason::FrameBudget).expect("teardown");

    let mut read = cli(&dir);
    assert_eq!(read.log().len(), 1, "a drag is not one entry on disk");
    assert!(read.undo().expect("the drag's inverse applies"));
    read.save_with_history().expect("saved");
    assert_eq!(scene_files(&dir), before, "the undo is not where it began");
}

/// **A save whose history will not be written says so**: the scene lands and
/// the document is clean, and the status line warns that the history beside
/// it was not written — here because a directory stands where it goes.
#[test]
fn a_save_whose_history_will_not_write_says_so() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = base.path().join("greybox.scn");
    let mut editor = headless(FRAMES);
    editor
        .document_mut()
        .save_as(&dir)
        .expect("a fresh directory");
    std::fs::create_dir(dir.join(HISTORY)).expect("in the way");
    editor.document_mut().select(Some(SceneEntityId(0)));
    editor.act(&Action::Nudge { axis: 1, sign: 1.0 });
    let nudged: BTreeMap<String, Vec<u8>> = editor
        .document_mut()
        .files()
        .expect("every entity has an id")
        .into_iter()
        .map(|(key, text)| (key, text.into_bytes()))
        .collect();

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert!(!editor.document().is_dirty(), "the scene did not land");
    assert_eq!(scene_files(&dir), nudged);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.starts_with("Saved, but"), "{text}");
    assert!(text.contains("was not written"), "{text}");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
