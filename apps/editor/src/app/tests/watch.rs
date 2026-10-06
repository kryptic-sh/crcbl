//! The scene's chunk files watched through the loop: a chunk another
//! program changed reloads into a clean scene, is asked about over unsaved
//! edits — each answer doing what it says — and waits out play mode.
//! `watch`'s module docs hold the decisions.

use super::*;

use crcbl::assets::watch::{POLL_INTERVAL, SETTLE};
use crcbl::engine::HEADLESS_FRAME_STEP;

use crate::document::PlayState;
use crate::panel::Asking;

use super::files::chord;
use super::lock::{ENTRY, FAR_PLOT, edited_behind, editing_a_changed_field, files_of, relabel};
use super::play::towers_editor;

/// The frames after a write by which the watch has offered it, at the
/// headless clock's step: a look a [`POLL_INTERVAL`] at the latest, then
/// [`SETTLE`], then the look that offers it — with a margin.
fn settle_frames() -> usize {
    let latest = POLL_INTERVAL * 2 + SETTLE;
    2 * (latest.as_nanos() / HEADLESS_FRAME_STEP.as_nanos()) as usize
}

/// The frames each editor here may run: room for every wait a test makes.
const BUDGET: u64 = 2048;

/// `editor`, with room for [`BUDGET`] frames.
fn roomy(mut editor: Editor<HeadlessShell>) -> Editor<HeadlessShell> {
    editor.budget = FrameBudget::new(Some(BUDGET));
    editor
}

/// One frame, which must not be the budget's last.
fn step(editor: &mut Editor<HeadlessShell>) {
    assert_eq!(
        editor.frame().expect("a frame"),
        Flow::Continue,
        "the run stopped"
    );
}

/// Runs frames until `done` holds, or panics naming `what` once the watch
/// would have offered any change.
fn until(
    editor: &mut Editor<HeadlessShell>,
    what: &str,
    done: impl Fn(&Editor<HeadlessShell>) -> bool,
) {
    for _ in 0..settle_frames() {
        step(editor);
        if done(editor) {
            return;
        }
    }
    panic!("never happened: {what}");
}

/// Runs frames until a change the watch would offer has been offered.
fn settle(editor: &mut Editor<HeadlessShell>) {
    for _ in 0..settle_frames() {
        step(editor);
    }
}

/// An editor started on the scene directory `dir`, with room to wait.
fn start_on(dir: &Path) -> Editor<HeadlessShell> {
    roomy(super::lock::start_on(dir).expect("the field opens"))
}

/// A plot's label, as the document holds it.
fn label(editor: &mut Editor<HeadlessShell>, plot: SceneEntityId) -> Value {
    editor
        .document_mut()
        .read(plot, "plots", "label")
        .expect("a plot has a label")
}

/// **A clean scene reloads a chunk another program changed and stays
/// clean**, as one entry the status line names; Ctrl+Z puts the scene back,
/// dirty.
#[test]
fn a_clean_scene_reloads_a_chunk_changed_on_disk() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let mut editor = start_on(&dir);
    let before = label(&mut editor, FAR_PLOT);
    let theirs = edited_behind(&dir);

    until(&mut editor, "the change reloaded", |editor| {
        !editor.document().log().is_empty()
    });
    assert_eq!(label(&mut editor, FAR_PLOT), Value::Text("far".to_owned()));
    assert_eq!(files_of(editor.document_mut()), theirs);
    assert_eq!(editor.document().log().len(), 1, "a reload is one entry");
    assert!(
        !editor.document().is_dirty(),
        "a reload left the scene dirty"
    );
    let (text, tone) = editor.panels.status();
    assert!(text.contains("sys/plots.ron"), "{text}");
    assert_eq!(tone, Tone::Info);

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert_eq!(label(&mut editor, FAR_PLOT), before);
    assert!(editor.document().is_dirty());
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Unsaved edits ask, and Keep mine keeps them — Enter and Escape alike**:
/// the edit stays, the other program's change is not taken in, and the next
/// save asks whether to write over it.
#[test]
fn keep_mine_leaves_the_edits_and_the_next_save_asks() {
    for key in [KeyCode::Enter, KeyCode::Escape] {
        let base = tempfile::tempdir().expect("a temporary directory");
        let dir = towers_field(base.path());
        let (editor, _) = editing_a_changed_field(&dir);
        let mut editor = roomy(editor);
        until(&mut editor, "the bar asks", |editor| {
            editor.panels.unsaved_asking() == Some(Asking::ChunkChanged)
        });
        let asked = editor.panels.unsaved().expect("the bar is up");
        assert!(asked.contains("sys/plots.ron"), "{asked}");
        let before = files_of(editor.document_mut());

        tap(&mut editor, key);
        assert_eq!(editor.panels.unsaved(), None, "{key:?} left the bar up");
        assert_eq!(files_of(editor.document_mut()), before, "{key:?}");
        assert_eq!(label(&mut editor, ENTRY), Value::Text("gate".to_owned()));
        assert!(editor.document().is_dirty());
        settle(&mut editor);
        assert_eq!(editor.panels.unsaved(), None, "{key:?} was asked again");

        chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
        assert_eq!(
            editor.panels.unsaved_asking(),
            Some(Asking::ChangedOnDisk),
            "{key:?}: the save did not ask"
        );
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }
}

/// **Reload from disk takes the file's rows over the edits as one entry on
/// top**: the scene is the other program's, still dirty, and Ctrl+Z brings
/// the edit back.
#[test]
fn reload_from_disk_takes_the_files_rows_over_the_edits() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (editor, theirs) = editing_a_changed_field(&dir);
    let mut editor = roomy(editor);
    until(&mut editor, "the bar asks", |editor| {
        editor.panels.unsaved_asking() == Some(Asking::ChunkChanged)
    });

    tap(&mut editor, KeyCode::KeyD);
    assert_eq!(editor.panels.unsaved(), None, "D left the bar up");
    assert_eq!(files_of(editor.document_mut()), theirs, "not the disk's");
    assert_eq!(editor.document().log().len(), 2, "the reload is not on top");
    assert!(editor.document().is_dirty(), "the edit beneath was dropped");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert_eq!(label(&mut editor, ENTRY), Value::Text("gate".to_owned()));
    assert_eq!(label(&mut editor, FAR_PLOT), {
        let mut committed = start_document(&base);
        committed.read(FAR_PLOT, "plots", "label").expect("a label")
    });
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// Towers' committed field, opened fresh under `base` beside the copy the
/// test edits — what the field held before anyone changed it.
fn start_document(base: &tempfile::TempDir) -> Document {
    let fresh = towers_field(&base.path().join("committed"));
    Document::open_dir(fresh, crate::scene::vocabulary()).expect("the field opens")
}

/// **A change made while the scene plays waits for play to stop**: the
/// played world is not reloaded into, the status line says when the change
/// comes in, and it reloads in the frame play stops.
#[test]
fn a_change_during_play_reloads_once_play_stops() {
    let (base, mut editor) = towers_editor(BUDGET, 1);
    let dir = base.path().join("field.scn");
    editor.act(&Action::PlayStop);
    assert_eq!(editor.document().play_state(), PlayState::Playing);
    edited_behind(&dir);

    until(&mut editor, "the status says it waits", |editor| {
        editor.panels.status().0.contains("when play stops")
    });
    settle(&mut editor);
    assert!(
        editor.document().log().is_empty(),
        "the played scene reloaded"
    );

    editor.act(&Action::PlayStop);
    step(&mut editor);
    assert_eq!(editor.document().play_state(), PlayState::Editing);
    assert_eq!(editor.document().log().len(), 1, "the change never came in");
    assert_eq!(label(&mut editor, FAR_PLOT), Value::Text("far".to_owned()));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The editor's own save is no change**: after Ctrl+S and an edit made
/// since, the watch asks nothing and reloads nothing.
#[test]
fn the_editors_own_save_asks_nothing() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let mut editor = start_on(&dir);
    editor
        .document_mut()
        .apply(relabel(ENTRY, "gate"))
        .expect("a label");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert!(!editor.document().is_dirty(), "Ctrl+S did not save");
    editor
        .document_mut()
        .apply(relabel(ENTRY, "post"))
        .expect("a label");

    settle(&mut editor);
    assert_eq!(editor.panels.unsaved(), None, "the editor's own save asked");
    assert_eq!(editor.document().log().len(), 2);
    assert_eq!(label(&mut editor, ENTRY), Value::Text("post".to_owned()));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
