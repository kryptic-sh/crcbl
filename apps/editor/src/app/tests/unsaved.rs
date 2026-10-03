//! The unsaved bar through the loop: what asks, each answer, the window's
//! close request held open while it asks, and the recovery copy written when
//! the window goes without asking.

use super::*;

use std::path::PathBuf;

use super::files::{chord, type_and_enter};
use crate::document::origin_tests::tree;

/// Makes the editor's scene dirty, with something selected.
fn dirty(editor: &mut Editor<HeadlessShell>) {
    editor.document_mut().select(Some(SceneEntityId(0)));
    editor.act(&Action::Nudge { axis: 1, sign: 1.0 });
    assert!(editor.document().is_dirty());
}

/// A headless editor whose scene is saved in a directory of its own under
/// `dir`, and is dirty after that.
fn dirty_with_origin(dir: &Path) -> Editor<HeadlessShell> {
    let mut editor = headless(256);
    editor
        .document_mut()
        .save_as(dir.join("greybox.scn"))
        .expect("a fresh directory");
    dirty(&mut editor);
    editor
}

/// Asks the window to close, as its title-bar button would, and runs the
/// frame that reads it.
fn request_close(editor: &mut Editor<HeadlessShell>) -> Flow {
    let window = editor.window;
    editor.shell_mut().request_close(window).expect("live");
    editor.frame().expect("a frame")
}

/// Presses `key` and runs the frame that reads it, handing back its flow —
/// the frame that may close the window, after which no other frame runs.
fn press(editor: &mut Editor<HeadlessShell>, key: KeyCode) -> Flow {
    let window = editor.window;
    editor.shell_mut().key_press(window, key).expect("live");
    editor.frame().expect("a frame")
}

/// Whether the window's close request is still outstanding.
fn close_pending(editor: &mut Editor<HeadlessShell>) -> bool {
    let window = editor.window;
    editor
        .shell_mut()
        .window_state(window)
        .expect("the window is open")
        .close_pending
}

/// **A dirty new scene asks, and Cancel keeps everything**: the scene, its
/// edits and its history are as they were, and the bar is gone.
#[test]
fn cancel_keeps_the_scene_and_its_edits() {
    let mut editor = headless(64);
    dirty(&mut editor);
    let files = editor.document_mut().files().expect("ids");
    let position = editor.document().log().position();

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    let asked = editor.panels.unsaved().expect("Ctrl+N asked nothing");
    assert!(asked.contains("`greybox`"), "{asked}");
    assert!(asked.contains("new scene"), "{asked}");
    tap(&mut editor, KeyCode::Escape);

    assert_eq!(editor.panels.unsaved(), None, "Cancel left the bar up");
    assert_eq!(editor.document_mut().files().expect("ids"), files);
    assert!(editor.document().is_dirty());
    assert_eq!(editor.document().log().position(), position);
    let (text, _) = editor.panels.status();
    assert!(text.starts_with("Cancelled"), "{text}");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **While the bar asks, nothing else is done**: an arrow does not nudge,
/// Ctrl+Z does not undo, F does not frame and a click in the viewport picks
/// nothing — and the bar's own buttons are what answer it.
#[test]
fn nothing_else_is_done_while_the_bar_asks() {
    let mut editor = headless(64);
    dirty(&mut editor);
    // A step, which the middle of the view is not: a pick there changes it.
    editor.document_mut().select(Some(SceneEntityId(2)));
    let position = editor.document().log().position();
    let files = editor.document_mut().files().expect("ids");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert!(editor.panels.unsaved().is_some());

    tap(&mut editor, KeyCode::ArrowRight);
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyZ);
    let (min, max) = editor.panels.viewport_pixels();
    let middle = (min + max) * 0.5;
    click(
        &mut editor,
        PhysicalPoint {
            x: f64::from(middle.x),
            y: f64::from(middle.y),
        },
    );
    assert_eq!(editor.document().log().position(), position, "an edit ran");
    assert_eq!(editor.document_mut().files().expect("ids"), files);
    assert_eq!(
        editor.document().primary(),
        Some(SceneEntityId(2)),
        "the viewport picked"
    );
    assert!(editor.panels.unsaved().is_some(), "the bar went down");

    let [_, discard, _] = editor.panels.unsaved_buttons();
    let at = centre(&editor, discard);
    click(&mut editor, at);
    assert_eq!(editor.panels.unsaved(), None);
    assert_eq!(
        editor.document().entity_count(),
        0,
        "Discard made nothing new"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Save saves, then goes on**: the scene's directory holds the edited
/// files, and the new scene is in place.
#[test]
fn save_saves_then_goes_on() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut editor = dirty_with_origin(dir.path());
    let edited = editor.document_mut().files().expect("ids");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    tap(&mut editor, KeyCode::Enter);
    assert_eq!(editor.panels.unsaved(), None);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("greybox.scn/sys/blocks.ron")).expect("saved"),
        edited["sys/blocks.ron"],
        "Save did not write the edits",
    );
    assert_eq!(editor.document().entity_count(), 0, "Save did not go on");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Save with no directory asks for one, and goes on once it is saved**:
/// nothing changes while the line is up, and the typed directory holds the
/// scene before the new one is put in place.
#[test]
fn save_with_no_directory_saves_as_then_goes_on() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let target = dir.path().join("kept.scn");
    let mut editor = headless(64);
    dirty(&mut editor);
    let edited = editor.document_mut().files().expect("ids");
    let count = editor.document().entity_count();

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    tap(&mut editor, KeyCode::Enter);
    assert_eq!(
        editor.panels.saving_as(),
        Some(""),
        "no directory was asked"
    );
    assert_eq!(
        editor.document().entity_count(),
        count,
        "it went on unsaved"
    );
    type_and_enter(&mut editor, &target.display().to_string());

    assert_eq!(
        std::fs::read_to_string(target.join("sys/blocks.ron")).expect("saved"),
        edited["sys/blocks.ron"],
    );
    assert_eq!(
        editor.document().entity_count(),
        0,
        "the save-as did not go on"
    );
    assert_eq!(
        editor.document().origin(),
        None,
        "the new scene kept the old directory"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Cancelling the save-as a Save asked for does nothing**: the scene and
/// its edits stay, and no new scene comes.
#[test]
fn cancelling_the_save_as_a_save_asked_for_does_nothing() {
    let mut editor = headless(64);
    dirty(&mut editor);
    let files = editor.document_mut().files().expect("ids");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    tap(&mut editor, KeyCode::Enter);
    editor.frame().expect("a frame");
    assert!(editor.panels.text_editing());
    tap(&mut editor, KeyCode::Escape);
    assert_eq!(editor.panels.saving_as(), None);
    // A later save-as is not the one the bar asked for: it goes on with
    // nothing.
    chord(
        &mut editor,
        Modifiers::CTRL | Modifiers::SHIFT,
        KeyCode::KeyS,
    );
    let dir = tempfile::tempdir().expect("a temporary directory");
    type_and_enter(
        &mut editor,
        &dir.path().join("later.scn").display().to_string(),
    );
    assert_eq!(editor.document_mut().files().expect("ids"), files);
    assert_ne!(editor.document().entity_count(), 0, "a new scene came");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A dirty scene holds a close request open while the bar asks**: the
/// window stays and the request stays outstanding; Cancel answers it "keep",
/// and a second request asked again is closed by Discard.
#[test]
fn a_dirty_close_is_held_open_until_answered() {
    let mut editor = headless(64);
    dirty(&mut editor);

    assert_eq!(request_close(&mut editor), Flow::Continue, "closed unasked");
    let asked = editor.panels.unsaved().expect("the close asked nothing");
    assert!(asked.contains("window closes"), "{asked}");
    assert!(close_pending(&mut editor), "the request was answered");
    editor.frame().expect("a frame");
    assert!(close_pending(&mut editor), "the request was answered");

    tap(&mut editor, KeyCode::Escape);
    assert!(
        !close_pending(&mut editor),
        "Cancel left the request outstanding"
    );
    assert!(editor.document().is_dirty());

    assert_eq!(request_close(&mut editor), Flow::Continue);
    assert!(
        editor.panels.unsaved().is_some(),
        "the second close asked nothing"
    );
    assert_eq!(
        press(&mut editor, KeyCode::KeyD),
        Flow::Stop(ExitReason::CloseRequested)
    );
    editor.finish(ExitReason::CloseRequested).expect("teardown");
}

/// **A clean scene closes at once**, asking nothing.
#[test]
fn a_clean_close_closes_at_once() {
    let mut editor = headless(16);
    assert_eq!(
        request_close(&mut editor),
        Flow::Stop(ExitReason::CloseRequested)
    );
    editor.finish(ExitReason::CloseRequested).expect("teardown");
}

/// **Save on a close saves, then closes** — and in play mode stops play
/// first, so it is the authored scene that is saved.
#[test]
fn save_on_a_close_saves_then_closes_even_in_play() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut editor = dirty_with_origin(dir.path());
    let edited = editor.document_mut().files().expect("ids");
    editor.act(&Action::PlayStop);
    assert_eq!(editor.document().play_state(), PlayState::Playing);

    assert_eq!(request_close(&mut editor), Flow::Continue);
    assert_eq!(
        press(&mut editor, KeyCode::Enter),
        Flow::Stop(ExitReason::CloseRequested)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("greybox.scn/sys/blocks.ron")).expect("saved"),
        edited["sys/blocks.ron"],
    );
    editor.finish(ExitReason::CloseRequested).expect("teardown");
}

/// **A window taken away without a request leaves a recovery copy** of a
/// dirty scene — the scene's files, in a directory of their own under the
/// recovery base — and none of a clean one.
#[test]
fn a_window_taken_away_leaves_a_recovery_copy_of_a_dirty_scene() {
    for dirty_scene in [true, false] {
        let base = tempfile::tempdir().expect("a temporary directory");
        let mut editor = headless(16);
        editor.recovery = Some(base.path().to_path_buf());
        if dirty_scene {
            dirty(&mut editor);
        }
        let files = editor.document_mut().files().expect("ids");
        let window = editor.window;
        editor
            .shell_mut()
            .destroy_window(window)
            .expect("the window is open");
        assert_eq!(
            editor.frame().expect("a frame"),
            Flow::Stop(ExitReason::WindowDestroyed)
        );
        let copies: Vec<_> = std::fs::read_dir(base.path())
            .map(|entries| {
                entries
                    .map(|entry| entry.expect("an entry").path())
                    .collect()
            })
            .unwrap_or_default();
        if dirty_scene {
            assert_eq!(copies.len(), 1, "{copies:?}");
            let written = tree(&copies[0]);
            assert_eq!(written.len(), files.len());
            assert_eq!(
                written["sys/blocks.ron"],
                files["sys/blocks.ron"].as_bytes(),
                "the copy is not the edited scene"
            );
        } else {
            assert_eq!(copies, Vec::<PathBuf>::new(), "a clean scene was copied");
        }
        editor
            .finish(ExitReason::WindowDestroyed)
            .expect("teardown");
    }
}
