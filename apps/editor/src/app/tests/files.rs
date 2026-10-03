//! A new scene and save-as through the loop: Ctrl+N, Ctrl+S and
//! Ctrl+Shift+S, the toolbar's buttons, and a directory typed on the save-as
//! line as a window system delivers the keys.

use super::*;

use crate::document::origin_tests::tree;

/// Holds `modifiers`, taps `key`, and lets go — a frame each, through the
/// shell, the way a person presses a chord.
pub(super) fn chord(editor: &mut Editor<HeadlessShell>, modifiers: Modifiers, key: KeyCode) {
    let window = editor.window;
    let held: Vec<KeyCode> = [
        (Modifiers::CTRL, KeyCode::ControlLeft),
        (Modifiers::SHIFT, KeyCode::ShiftLeft),
    ]
    .into_iter()
    .filter(|(bit, _)| modifiers.contains(*bit))
    .map(|(_, key)| key)
    .collect();
    editor.shell_mut().set_modifiers(modifiers);
    for modifier in &held {
        editor
            .shell_mut()
            .key_press(window, *modifier)
            .expect("live");
    }
    editor.frame().expect("a frame");
    tap(editor, key);
    editor.shell_mut().set_modifiers(Modifiers::empty());
    for modifier in &held {
        editor
            .shell_mut()
            .key_release(window, *modifier)
            .expect("live");
    }
    editor.frame().expect("a frame");
}

/// Types `text` into whatever input is engaged and presses Enter — a frame
/// for the input to engage first, the way the line asks for it.
pub(super) fn type_and_enter(editor: &mut Editor<HeadlessShell>, text: &str) {
    editor.frame().expect("a frame");
    assert!(
        editor.panels.text_editing(),
        "no input is engaged, so what is typed now goes nowhere"
    );
    let window = editor.window;
    editor
        .shell_mut()
        .commit_text(window, text)
        .expect("the headless shell takes text");
    editor.frame().expect("a frame");
    tap(editor, KeyCode::Enter);
}

/// **Ctrl+N puts an empty scene in place, frames it and draws nothing**, and
/// the status line names the scene whose unsaved edits it dropped — and only
/// when there were some.
#[test]
fn ctrl_n_starts_an_empty_scene_and_says_what_it_dropped() {
    let mut editor = headless(64);
    editor.document_mut().select(Some(SceneEntityId(2)));
    editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    editor.frame().expect("a frame");
    assert!(!editor.instances.instances.is_empty());

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert_eq!(editor.document().entity_count(), 0);
    assert_eq!(editor.document().origin(), None);
    assert!(editor.document().log().is_empty());
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(
        text.contains("unsaved edits to `greybox` were dropped"),
        "{text}"
    );
    assert!(
        editor.instances.instances.is_empty(),
        "the old scene is still drawn"
    );

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert_eq!(
        editor.panels.status(),
        (crate::app::files::NEW_SCENE, Tone::Info)
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A new scene is refused in play mode**, on the status line, and the
/// played scene goes on.
#[test]
fn a_new_scene_in_play_mode_is_refused() {
    let mut editor = headless(16);
    let count = editor.document().entity_count();
    editor.act(&Action::PlayStop);
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("play mode"), "{text}");
    assert_eq!(editor.document().entity_count(), count);
    assert_eq!(editor.document().play_state(), PlayState::Playing);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The toolbar's file buttons are Ctrl+N's and Ctrl+Shift+S's**: one puts
/// a new scene in place, the other opens the save-as line.
#[test]
fn the_toolbar_starts_a_new_scene_and_asks_for_a_directory() {
    let mut editor = headless(32);
    editor.frame().expect("a frame");
    let [new, save_as] = editor.panels.file_buttons();
    let at = centre(&editor, new);
    click(&mut editor, at);
    assert_eq!(editor.document().entity_count(), 0, "New made nothing new");

    let at = centre(&editor, save_as);
    click(&mut editor, at);
    assert_eq!(editor.panels.saving_as(), Some(""));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Ctrl+S on a scene with no directory asks for one**, as save-as does,
/// rather than refusing — and writes nothing until one is given.
#[test]
fn ctrl_s_with_no_origin_opens_save_as() {
    let mut editor = headless(16);
    editor.document_mut().select(Some(SceneEntityId(0)));
    editor.act(&Action::Nudge { axis: 1, sign: 1.0 });
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert_eq!(editor.panels.saving_as(), Some(""), "no line was opened");
    editor.frame().expect("a frame");
    assert!(editor.panels.text_editing(), "the line is not taking keys");
    assert!(editor.document().is_dirty(), "something was saved");
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Info, "{text}");
    assert!(text.starts_with("Save as"), "{text}");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A directory typed on the save-as line is where the scene goes, and
/// where Ctrl+S goes after it**: Ctrl+Shift+S, the path, Enter, the files
/// there; a nudge and Ctrl+S, the files there again.
#[test]
fn a_typed_directory_is_saved_into_and_later_saves_follow() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let target = dir.path().join("greybox.scn");
    let mut editor = headless(64);

    chord(
        &mut editor,
        Modifiers::CTRL | Modifiers::SHIFT,
        KeyCode::KeyS,
    );
    type_and_enter(&mut editor, &target.display().to_string());
    assert_eq!(editor.panels.saving_as(), None, "the line stayed open");
    assert_eq!(editor.document().origin(), Some(target.as_path()));
    let (text, _) = editor.panels.status();
    assert!(text.starts_with("Saved as"), "{text}");
    let written = editor.document_mut().files().expect("ids");
    assert_eq!(tree(&target).len(), written.len());

    editor.document_mut().select(Some(SceneEntityId(0)));
    editor.act(&Action::Nudge { axis: 1, sign: 1.0 });
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert!(!editor.document().is_dirty(), "Ctrl+S did not save");
    let saved = editor.document_mut().files().expect("ids");
    assert_ne!(saved, written, "the nudge changed nothing");
    assert_eq!(
        std::fs::read_to_string(target.join("sys").join("blocks.ron")).expect("written"),
        saved["sys/blocks.ron"],
        "the later save went somewhere else",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A directory holding another scene is refused on the status line and
/// asked for again**, holding what was typed — and nothing is written into
/// it.
#[test]
fn an_occupied_directory_is_refused_and_asked_again() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    Document::built_in()
        .expect("the compiled-in scene")
        .save_to(dir.path())
        .expect("a fresh directory");
    let there = tree(dir.path());
    let typed = dir.path().display().to_string();
    let mut editor = headless(64);
    editor.document_mut().select(Some(SceneEntityId(0)));
    editor.act(&Action::Nudge { axis: 1, sign: 1.0 });

    chord(
        &mut editor,
        Modifiers::CTRL | Modifiers::SHIFT,
        KeyCode::KeyS,
    );
    type_and_enter(&mut editor, &typed);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("already holds"), "{text}");
    assert_eq!(editor.panels.saving_as(), Some(typed.as_str()));
    assert_eq!(editor.document().origin(), None);
    assert!(editor.document().is_dirty());
    assert_eq!(tree(dir.path()), there, "it wrote into the other scene");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A toolbar click that ends a save-as being typed saves the scene it was
/// typed for**: the click commits the line, and the scene saved is the one
/// being edited — not the new, empty one the click then puts in its place.
#[test]
fn a_toolbar_click_that_commits_a_save_as_saves_the_scene_it_was_typed_for() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let target = dir.path().join("greybox.scn");
    let mut editor = headless(64);
    let greybox = editor.document_mut().files().expect("ids");

    chord(
        &mut editor,
        Modifiers::CTRL | Modifiers::SHIFT,
        KeyCode::KeyS,
    );
    editor.frame().expect("a frame");
    let window = editor.window;
    editor
        .shell_mut()
        .commit_text(window, &target.display().to_string())
        .expect("the headless shell takes text");
    editor.frame().expect("a frame");
    let [new, _] = editor.panels.file_buttons();
    let at = centre(&editor, new);
    click(&mut editor, at);

    assert_eq!(editor.document().entity_count(), 0, "New made nothing new");
    assert_eq!(
        std::fs::read_to_string(target.join("sys").join("blocks.ron")).expect("saved"),
        greybox["sys/blocks.ron"],
        "the scene saved is not the one the path was typed for",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
