//! The scene lock and a scene changed on disk, through the loop: a scene
//! another editor holds is refused, the lock is held from open until the
//! scene is let go, a crashed holder's file blocks nobody, and a Save over
//! files another program wrote asks whether to overwrite or reload.
//!
//! The `crcbl scene` CLI's side is [`lock_scene`], the call it makes before
//! an edit run, since this crate cannot run that binary;
//! `crates/crcbl-cli/tests/scene.rs` runs it against a held scene.

use super::*;

use std::collections::BTreeMap;

use crate::document::origin_tests::tree;
use crate::document::{HISTORY, SCENE_LOCK, lock_scene};
use crate::panel::Asking;

use super::files::{chord, type_and_enter};
use super::unsaved::{close_pending, press, request_close};

/// The frames each editor here may run.
pub(super) const FRAMES: u64 = 256;

/// Plot 4, `entry`, in towers' field: what the editor relabels here.
pub(super) const ENTRY: SceneEntityId = SceneEntityId(4);

/// Plot 6 in towers' field: what another program relabels here.
pub(super) const FAR_PLOT: SceneEntityId = SceneEntityId(6);

/// An editor started on the scene directory `scene`, as `editor <SCENE_DIR>`
/// starts, or why it did not start.
pub(super) fn start_on(scene: &Path) -> Result<Editor<HeadlessShell>, EditorError> {
    let mut options = options(FRAMES);
    options.scene = Some(scene.to_path_buf());
    Editor::with_shell(Box::new(HeadlessShell::new()), &options)
}

/// Whether a program asking for the scene's lock now — as a `crcbl scene`
/// edit run does — is refused because something holds it.
fn held(dir: &Path) -> bool {
    match lock_scene(dir) {
        Ok(_) => false,
        Err(EditError::Locked { .. }) => true,
        Err(other) => panic!("the lock would not be asked for: {other}"),
    }
}

/// Opens `dir` through Ctrl+O and the path line, typing what the line does
/// not already hold — it opens holding the directory the scene being edited
/// is in, and after a refused open, what was typed.
fn ctrl_o(editor: &mut Editor<HeadlessShell>, dir: &Path) {
    chord(editor, Modifiers::CTRL, KeyCode::KeyO);
    let full = dir.display().to_string();
    let holding = editor.panels.opening().expect("Ctrl+O opened no line");
    let rest = full
        .strip_prefix(holding)
        .expect("the line holds the start of the directory")
        .to_owned();
    if rest.is_empty() {
        editor.frame().expect("a frame");
        tap(editor, KeyCode::Enter);
    } else {
        type_and_enter(editor, &rest);
    }
}

/// The scene's own files in `dir`, byte for byte — every file but the
/// history.
pub(super) fn scene_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = tree(dir);
    files.remove(HISTORY);
    files
}

/// `document`'s files as bytes, keyed as [`scene_files`] keys them.
pub(super) fn files_of(document: &mut Document) -> BTreeMap<String, Vec<u8>> {
    document
        .files()
        .expect("every entity has an id")
        .into_iter()
        .map(|(key, text)| (key, text.into_bytes()))
        .collect()
}

/// Another program's edit, made without the lock — an older build, or a
/// text editor: the scene opened from `dir`, a plot relabelled, saved.
/// Hands back the files it wrote.
pub(super) fn edited_behind(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut other = Document::open_dir(dir, crate::scene::vocabulary()).expect("the field opens");
    other.apply(relabel(FAR_PLOT, "far")).expect("a label");
    other.save().expect("saved");
    scene_files(dir)
}

/// A plot's label set to `text`.
pub(super) fn relabel(entity: SceneEntityId, text: &str) -> EditCommand {
    EditCommand::SetProperty {
        entity,
        system: "plots".to_owned(),
        path: "label".to_owned(),
        value: Value::Text(text.to_owned()),
    }
}

/// An editor on a copy of towers' field with the entry plot relabelled, and
/// the field changed on disk behind it since; and the files on disk.
pub(super) fn editing_a_changed_field(
    dir: &Path,
) -> (Editor<HeadlessShell>, BTreeMap<String, Vec<u8>>) {
    let mut editor = start_on(dir).expect("the field opens");
    editor
        .document_mut()
        .apply(relabel(ENTRY, "gate"))
        .expect("a label");
    assert!(editor.document().is_dirty());
    let theirs = edited_behind(dir);
    (editor, theirs)
}

/// **A second editor cannot take a scene the first holds** — `editor
/// <SCENE_DIR>` does not start, and Ctrl+O is refused on the status line
/// with the scene being edited kept — and takes it once the first closes.
#[test]
fn a_second_editor_is_refused_a_held_scene_until_the_first_closes() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let first = start_on(&dir).expect("the field opens");

    let Err(refused) = start_on(&dir) else {
        panic!("a second editor started on a held scene");
    };
    assert!(
        refused.to_string().contains(SCENE_LOCK),
        "the refusal does not name the lock: {refused}"
    );

    let mut second = headless(FRAMES);
    ctrl_o(&mut second, &dir);
    let (text, tone) = second.panels.status();
    assert!(text.contains("being edited"), "{text}");
    assert_eq!(tone, Tone::Warning);
    assert_eq!(second.document().origin(), None, "the held scene opened");
    assert_eq!(second.document().name(), "greybox", "the scene was lost");

    first.finish(ExitReason::FrameBudget).expect("teardown");
    ctrl_o(&mut second, &dir);
    assert_eq!(second.document().origin(), Some(dir.as_path()));
    assert!(held(&dir), "the second editor opened it unlocked");
    second.finish(ExitReason::FrameBudget).expect("teardown");
    assert!(!held(&dir), "the lock outlived the editor");
}

/// **The editor holds its scene until it lets it go**: through Ctrl+O on
/// the same directory, which reads it again under the lock it has; not past
/// a new scene; and a save-as takes the new directory's lock.
#[test]
fn the_editor_holds_its_scene_until_it_lets_it_go() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let mut editor = start_on(&dir).expect("the field opens");
    assert!(held(&dir), "an open scene is not locked");

    ctrl_o(&mut editor, &dir);
    assert_eq!(editor.document().origin(), Some(dir.as_path()));
    assert_eq!(
        editor.panels.status().1,
        Tone::Info,
        "{:?}",
        editor.panels.status()
    );
    assert!(held(&dir), "reading the scene again let it go");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert_eq!(editor.document().origin(), None);
    assert!(!held(&dir), "a new scene kept the old one locked");

    let saved = base.path().join("saved.scn");
    chord(
        &mut editor,
        Modifiers::CTRL | Modifiers::SHIFT,
        KeyCode::KeyS,
    );
    type_and_enter(&mut editor, &saved.display().to_string());
    assert_eq!(editor.document().origin(), Some(saved.as_path()));
    assert!(held(&saved), "a save-as left its directory unlocked");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A lock file a crashed editor left blocks nobody**: a file nobody holds,
/// as the operating system leaves one when its process ends, is taken over.
#[test]
fn a_crashed_holders_lock_file_does_not_stop_the_editor() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    std::fs::write(dir.join(SCENE_LOCK), "editor (process 4242)\n").expect("written");

    let editor = start_on(&dir).expect("an unheld lock file blocks nobody");
    assert!(held(&dir), "the editor did not take the lock");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A Save over a scene changed on disk asks, and writes nothing until it
/// is answered**: Cancel keeps both sides as they were, and Overwrite writes
/// this editor's scene over the other program's.
#[test]
fn a_save_over_a_scene_changed_on_disk_asks_and_overwrite_writes() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, theirs) = editing_a_changed_field(&dir);

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::ChangedOnDisk));
    let asked = editor.panels.unsaved().expect("the bar is up");
    assert!(asked.contains("changed on disk"), "{asked}");
    assert_eq!(scene_files(&dir), theirs, "the save wrote before asking");

    tap(&mut editor, KeyCode::Escape);
    assert_eq!(editor.panels.unsaved(), None, "Cancel left the bar up");
    assert_eq!(scene_files(&dir), theirs, "Cancel wrote");
    assert!(editor.document().is_dirty(), "Cancel dropped the edit");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    tap(&mut editor, KeyCode::Enter);
    assert_eq!(editor.panels.unsaved(), None, "Overwrite left the bar up");
    assert!(!editor.document().is_dirty(), "Overwrite did not save");
    let ours = files_of(editor.document_mut());
    assert_eq!(scene_files(&dir), ours, "Overwrite is not this scene");
    assert_ne!(ours, theirs);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Reload takes the other program's scene and drops the edits here**,
/// still holding the scene's lock.
#[test]
fn reload_reads_the_changed_scene_back() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, theirs) = editing_a_changed_field(&dir);

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::ChangedOnDisk));
    tap(&mut editor, KeyCode::KeyD);
    assert_eq!(editor.panels.unsaved(), None, "Reload left the bar up");
    assert!(!editor.document().is_dirty(), "the reloaded scene is dirty");
    assert_eq!(
        files_of(editor.document_mut()),
        theirs,
        "not the disk's scene"
    );
    assert_eq!(scene_files(&dir), theirs, "Reload wrote");
    assert!(held(&dir), "Reload let the scene go");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Reload goes through the chunk reload and is one entry that Ctrl+Z
/// takes back** (decided 2026-10-06): the history keeps the edit beneath
/// it, and one undo brings the edit back, the scene dirty again.
#[test]
fn reload_is_one_entry_that_undo_takes_back() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, _) = editing_a_changed_field(&dir);
    let ours = files_of(editor.document_mut());

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    tap(&mut editor, KeyCode::KeyD);
    assert_eq!(
        editor.document().log().len(),
        2,
        "the reload is not one entry over the edit"
    );
    assert!(!editor.document().is_dirty());
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    assert_eq!(
        files_of(editor.document_mut()),
        ours,
        "undo did not bring the edit back"
    );
    assert!(editor.document().is_dirty());
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A header changed on disk is read back whole**: the scene another
/// program renamed is opened again in place, its history starting over,
/// since a revert does not take a header in.
#[test]
fn reload_over_a_changed_header_reads_the_scene_again_whole() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, _) = editing_a_changed_field(&dir);
    let header = dir.join("scene.ron");
    let text = std::fs::read_to_string(&header).expect("a header");
    let renamed = text.replacen("name: \"", "name: \"renamed ", 1);
    assert_ne!(renamed, text, "the header names no scene");
    std::fs::write(&header, renamed).expect("written");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    tap(&mut editor, KeyCode::KeyD);
    assert_eq!(editor.panels.unsaved(), None, "Reload left the bar up");
    assert!(editor.document().name().starts_with("renamed "));
    assert!(editor.document().log().is_empty(), "the history went on");
    assert!(!editor.document().is_dirty());
    assert!(held(&dir), "Reload let the scene go");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// Commits a save-as onto the scene's own directory: Ctrl+Shift+S, and Enter
/// on the line, which opens holding that directory.
fn save_as_onto_its_own(editor: &mut Editor<HeadlessShell>, dir: &Path) {
    chord(editor, Modifiers::CTRL | Modifiers::SHIFT, KeyCode::KeyS);
    let holding = dir.display().to_string();
    assert_eq!(editor.panels.saving_as(), Some(holding.as_str()));
    editor.frame().expect("a frame");
    tap(editor, KeyCode::Enter);
}

/// **A save-as onto the scene's own directory, changed on disk, asks as
/// Ctrl+S does**, writing nothing until it is answered: Cancel keeps the
/// edits and leaves the line closed, and Overwrite writes this scene over
/// the other program's and says it saved as.
#[test]
fn a_save_as_onto_its_own_changed_directory_asks_and_overwrite_writes() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, theirs) = editing_a_changed_field(&dir);

    save_as_onto_its_own(&mut editor, &dir);
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::ChangedOnDisk));
    assert_eq!(editor.panels.saving_as(), None, "the line stayed up");
    assert_eq!(scene_files(&dir), theirs, "the save-as wrote before asking");

    tap(&mut editor, KeyCode::Escape);
    assert_eq!(editor.panels.unsaved(), None, "Cancel left the bar up");
    assert_eq!(editor.panels.saving_as(), None, "Cancel reopened the line");
    assert_eq!(scene_files(&dir), theirs, "Cancel wrote");
    assert!(editor.document().is_dirty(), "Cancel dropped the edit");

    save_as_onto_its_own(&mut editor, &dir);
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::ChangedOnDisk));
    tap(&mut editor, KeyCode::Enter);
    assert_eq!(editor.panels.unsaved(), None, "Overwrite left the bar up");
    assert!(!editor.document().is_dirty(), "Overwrite did not save");
    let ours = files_of(editor.document_mut());
    assert_eq!(scene_files(&dir), ours, "Overwrite is not this scene");
    assert_ne!(ours, theirs);
    let (text, tone) = editor.panels.status();
    assert!(text.starts_with("Saved as"), "{text}");
    assert_eq!(tone, Tone::Info, "{text}");
    assert!(held(&dir), "Overwrite let the scene go");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Reload on a save-as's question takes the other program's scene** and
/// drops the edits here, deliberately, still holding the scene's lock.
#[test]
fn reload_on_a_save_as_reads_the_changed_scene_back() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, theirs) = editing_a_changed_field(&dir);

    save_as_onto_its_own(&mut editor, &dir);
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::ChangedOnDisk));
    tap(&mut editor, KeyCode::KeyD);
    assert_eq!(editor.panels.unsaved(), None, "Reload left the bar up");
    assert!(!editor.document().is_dirty(), "the reloaded scene is dirty");
    assert_eq!(
        files_of(editor.document_mut()),
        theirs,
        "not the disk's scene"
    );
    assert_eq!(scene_files(&dir), theirs, "Reload wrote");
    assert!(held(&dir), "Reload let the scene go");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// An editor on a changed field, as [`editing_a_changed_field`] makes one,
/// whose window was asked to close and whose unsaved bar was answered Save:
/// the second question up, the close still held; and the files on disk.
fn closing_over_a_changed_field(dir: &Path) -> (Editor<HeadlessShell>, BTreeMap<String, Vec<u8>>) {
    let (mut editor, theirs) = editing_a_changed_field(dir);
    assert_eq!(request_close(&mut editor), Flow::Continue, "closed unasked");
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::Unsaved));
    tap(&mut editor, KeyCode::Enter);
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::ChangedOnDisk));
    assert!(close_pending(&mut editor), "the close was answered");
    assert_eq!(
        scene_files(dir),
        theirs,
        "the bar's Save wrote before asking"
    );
    (editor, theirs)
}

/// **The bar's Save on a close, meeting a changed scene, asks again, and
/// Overwrite saves over it and closes.**
#[test]
fn a_close_saved_over_a_changed_scene_asks_and_overwrite_closes() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, theirs) = closing_over_a_changed_field(&dir);

    assert_eq!(
        press(&mut editor, KeyCode::Enter),
        Flow::Stop(ExitReason::CloseRequested),
        "Overwrite did not close"
    );
    assert!(!editor.document().is_dirty(), "Overwrite did not save");
    let ours = files_of(editor.document_mut());
    assert_eq!(scene_files(&dir), ours, "Overwrite is not this scene");
    assert_ne!(ours, theirs);
    editor.finish(ExitReason::CloseRequested).expect("teardown");
}

/// **Reload on a close's second question drops the edits and closes**,
/// writing nothing: the scene in the editor is the disk's when it goes.
#[test]
fn a_close_reloaded_over_a_changed_scene_drops_the_edits_and_closes() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, theirs) = closing_over_a_changed_field(&dir);

    assert_eq!(
        press(&mut editor, KeyCode::KeyD),
        Flow::Stop(ExitReason::CloseRequested),
        "Reload did not close"
    );
    assert!(!editor.document().is_dirty(), "the edits were not dropped");
    assert_eq!(
        files_of(editor.document_mut()),
        theirs,
        "not the disk's scene"
    );
    assert_eq!(scene_files(&dir), theirs, "Reload wrote");
    editor.finish(ExitReason::CloseRequested).expect("teardown");
}

/// **Cancel on a close's second question keeps the edits and the window**,
/// answering the close request "keep" so the window holds nothing; a later
/// close asks again from the start.
#[test]
fn a_close_cancelled_over_a_changed_scene_keeps_the_window_and_edits() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, theirs) = closing_over_a_changed_field(&dir);
    assert_eq!(request_close(&mut editor), Flow::Continue);
    assert_eq!(
        editor.panels.unsaved_asking(),
        Some(Asking::ChangedOnDisk),
        "a repeated close request asked the first question again"
    );

    assert_eq!(press(&mut editor, KeyCode::Escape), Flow::Continue);
    assert!(!close_pending(&mut editor), "the close is still held");
    assert_eq!(editor.panels.unsaved(), None, "Cancel left the bar up");
    assert!(editor.document().is_dirty(), "Cancel dropped the edit");
    assert_eq!(scene_files(&dir), theirs, "Cancel wrote");

    assert_eq!(request_close(&mut editor), Flow::Continue);
    assert_eq!(editor.panels.unsaved_asking(), Some(Asking::Unsaved));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A reload refused on a close's second question keeps the edits and the
/// window**, as Cancel does: the files another program left no longer read
/// as a scene, so nothing replaces the edits, and the close is answered
/// "keep" rather than left held.
#[test]
fn a_close_whose_reload_is_refused_keeps_the_window_and_edits() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(base.path());
    let (mut editor, _) = closing_over_a_changed_field(&dir);
    std::fs::write(dir.join("scene.ron"), "not a scene").expect("writable");

    assert_eq!(press(&mut editor, KeyCode::KeyD), Flow::Continue);
    assert!(!close_pending(&mut editor), "the close is still held");
    assert_eq!(editor.panels.unsaved(), None, "the bar is still up");
    assert!(
        editor.document().is_dirty(),
        "the refused reload lost the edit"
    );
    assert_eq!(editor.document().origin(), Some(dir.as_path()));
    assert_eq!(editor.panels.status().1, Tone::Warning);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The bar's Save on an open, meeting a changed scene, asks again** and
/// goes on as answered: Overwrite saves over it and opens the other scene,
/// Reload drops the edits and opens it, writing nothing, and Cancel keeps
/// the scene, its edits and its lock, opening nothing.
#[test]
fn an_open_saved_over_a_changed_scene_asks_and_goes_on_as_answered() {
    for answer in [KeyCode::Enter, KeyCode::KeyD, KeyCode::Escape] {
        let base = tempfile::tempdir().expect("a temporary directory");
        let dir = towers_field(base.path());
        let other = towers_field(&base.path().join("other"));
        let (mut editor, theirs) = editing_a_changed_field(&dir);
        let ours = files_of(editor.document_mut());

        ctrl_o(&mut editor, &other);
        assert_eq!(editor.panels.unsaved_asking(), Some(Asking::Unsaved));
        tap(&mut editor, KeyCode::Enter);
        assert_eq!(
            editor.panels.unsaved_asking(),
            Some(Asking::ChangedOnDisk),
            "{answer:?}"
        );
        assert_eq!(scene_files(&dir), theirs, "the bar's Save wrote first");

        tap(&mut editor, answer);
        assert_eq!(editor.panels.unsaved(), None, "{answer:?} left the bar up");
        let cancelled = answer == KeyCode::Escape;
        let (opened, written) = match answer {
            KeyCode::Enter => (&other, &ours),
            KeyCode::KeyD => (&other, &theirs),
            _ => (&dir, &theirs),
        };
        assert_eq!(
            editor.document().origin(),
            Some(opened.as_path()),
            "{answer:?}"
        );
        assert_eq!(&scene_files(&dir), written, "{answer:?}");
        assert_eq!(held(&dir), cancelled, "{answer:?}");
        assert_eq!(held(&other), !cancelled, "{answer:?}");
        assert_eq!(editor.document().is_dirty(), cancelled, "{answer:?}");
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
