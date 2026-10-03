//! The dirty marker against the entry a save was taken at — decided
//! 2026-10-03, `UndoLog`'s _The saved state is an entry_: a save is
//! identified by its entry, not the log's position, so an edit that drops
//! that entry is dirty until the next save.

use super::*;

/// The block the tests move.
const BLOCK: SceneEntityId = SceneEntityId(1);

/// The compiled-in scene and a directory to save it into.
fn saving() -> (tempfile::TempDir, Document) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let document = crate::scene::built_in_document().expect("the compiled-in scene is a scene");
    (dir, document)
}

/// [`BLOCK`]'s `position.{axis}` set to `to`.
fn set(axis: usize, to: f64) -> EditCommand {
    EditCommand::SetProperty {
        entity: BLOCK,
        system: crate::scene::BLOCKS.to_owned(),
        path: format!("position.{axis}"),
        value: Value::Float(to),
    }
}

/// Two edits and a save on the second, then an undo: the log at 1 with the
/// saved entry above it to redo.
fn saved_at_two_undone_to_one(dir: &Path, document: &mut Document) {
    for to in [1.0, 2.0] {
        document.apply(set(0, to)).expect("a block has an x");
    }
    document.save_to(dir).expect("a fresh directory");
    assert!(!document.is_dirty());
    assert!(document.undo().expect("two entries"));
    assert!(document.is_dirty(), "one below the save");
}

/// **The regression: save at 2, undo to 1, another edit — dirty**, though
/// the log is at 2 again. The entry the save names is gone, so no walk
/// reaches a clean state until the next save.
#[test]
fn an_edit_replacing_the_saved_entry_is_dirty_until_the_next_save() {
    let (dir, mut document) = saving();
    saved_at_two_undone_to_one(dir.path(), &mut document);

    document.apply(set(1, 5.0)).expect("a block has a y");
    assert_eq!(document.log().position(), 2);
    assert!(
        document.is_dirty(),
        "an edit replacing the saved entry reads clean"
    );
    assert!(document.title().starts_with('*'), "{}", document.title());
    while document.undo().expect("live entities") {
        assert!(document.is_dirty(), "a state below the edit reads clean");
    }
    while document.redo().expect("live entities") {
        assert!(document.is_dirty(), "a state above the bottom reads clean");
    }

    let again = tempfile::tempdir().expect("a temporary directory");
    document.save_to(again.path()).expect("a fresh directory");
    assert!(!document.is_dirty(), "the next save left it dirty");
}

/// **Save, undo, redo — clean**: the saved entry is still in the log, and
/// standing on it again is the saved state.
#[test]
fn a_redo_back_onto_the_saved_entry_is_clean() {
    let (dir, mut document) = saving();
    saved_at_two_undone_to_one(dir.path(), &mut document);

    assert!(document.redo().expect("the saved entry"));
    assert!(!document.is_dirty(), "the saved entry redone reads dirty");
}

/// **A drag back to its start keeps the saved entry to redo** — the entries
/// its first write truncated are held aside and put back when it nets to
/// nothing (`UndoLog::record_in`), the saved one among them, so a redo onto
/// it is clean.
#[test]
fn a_drag_back_to_its_start_keeps_the_saved_entry_to_redo() {
    let (dir, mut document) = saving();
    saved_at_two_undone_to_one(dir.path(), &mut document);

    let drag = document.begin_gesture();
    for to in [7.0, 1.0] {
        document
            .apply_in(set(0, to), drag)
            .expect("a block has an x");
    }
    assert_eq!(
        (document.log().position(), document.log().len()),
        (1, 2),
        "the drag back to its start moved the log or lost the redo"
    );
    assert!(document.is_dirty());
    assert!(document.redo().expect("the saved entry, put back"));
    assert!(!document.is_dirty(), "the saved entry put back reads dirty");
}
