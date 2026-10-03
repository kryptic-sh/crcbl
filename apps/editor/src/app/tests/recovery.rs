//! Recovery copies through the loop: offered back at start-up, opened,
//! deleted or put away from the bar, the directory pruned, and the autosave
//! written into it on its timer.

use super::*;

use std::collections::BTreeMap;

use super::files::chord;
use crate::app::recovery::{AUTOSAVE_KEY, AUTOSAVE_SECONDS, Autosave, OFFERED, now_millis};
use crate::document::origin_tests::tree;
use crate::document::play_tests::drifting_document;
use crate::document::{KEEP_NEWEST, MAX_AGE, list_copies};
use crate::scene::BLOCKS;

/// A day, in milliseconds.
const DAY_MS: u128 = 24 * 60 * 60 * 1000;

/// The interval the autosave tests run at, and the step their clock takes a
/// frame: ten frames an interval.
const INTERVAL: Duration = Duration::from_secs(10);
const STEP: Duration = Duration::from_secs(1);

/// A headless editor keeping its recovery copies in `base`.
fn recovering(base: &Path, frames: u64) -> Editor<HeadlessShell> {
    let mut options = options(frames);
    options.recovery = Some(base.to_path_buf());
    Editor::with_shell(Box::new(HeadlessShell::new()), &options)
        .expect("the null backend runs everywhere")
}

/// The compiled-in scene with its first block moved to `x`, written as a
/// recovery copy under `base` stamped `stamp` — handing back the copy and
/// the files it holds.
fn copy_in(base: &Path, stamp: u128, x: f64) -> (PathBuf, BTreeMap<String, String>) {
    let mut document = Document::built_in().expect("the compiled-in scene");
    document
        .apply(EditCommand::SetProperty {
            entity: SceneEntityId(0),
            system: BLOCKS.to_owned(),
            path: "position.0".to_owned(),
            value: Value::Float(x),
        })
        .expect("a block moves");
    let files = document.files().expect("ids");
    let dir = document.write_recovery(base, stamp).expect("a fresh name");
    (dir, files)
}

/// Clicks `key` on the panels as the last frame laid them out.
fn click_key(editor: &mut Editor<HeadlessShell>, key: NodeKey) {
    let at = centre(editor, key);
    click(editor, at);
}

/// Every copy under `base`, newest first.
fn copies(base: &Path) -> Vec<PathBuf> {
    list_copies(base)
        .expect("readable")
        .into_iter()
        .map(|copy| copy.dir)
        .collect()
}

/// Moves the editor's first block, making the scene dirty, or dirtier.
fn edit(editor: &mut Editor<HeadlessShell>) {
    editor.document_mut().select(Some(SceneEntityId(0)));
    editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    assert!(editor.document().is_dirty());
}

/// Puts the editor on a clock stepping [`STEP`] a frame from zero, and an
/// autosave every [`INTERVAL`].
fn on_test_clock(editor: &mut Editor<HeadlessShell>) {
    editor.clock_source = Clock::manual(STEP);
    editor.elapsed = Duration::ZERO;
    editor.autosave = Autosave::new(INTERVAL);
}

/// Runs `count` frames.
fn frames(editor: &mut Editor<HeadlessShell>, count: u32) {
    for _ in 0..count {
        assert_eq!(editor.frame().expect("a frame"), Flow::Continue);
    }
}

/// How many frames of [`STEP`] make an [`INTERVAL`].
fn interval_frames() -> u32 {
    u32::try_from(INTERVAL.as_secs() / STEP.as_secs()).expect("small")
}

/// **Start-up offers the newest copies**: the bar lists [`OFFERED`] of
/// them, newest first, by name and age — and with no copies, or on a
/// headless run that named no recovery directory, it offers nothing.
#[test]
fn start_up_offers_the_newest_copies() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let now = now_millis();
    for index in 0..=OFFERED {
        copy_in(
            base.path(),
            now - u128::try_from(index).expect("small") * 1000,
            1.0,
        );
    }
    let editor = recovering(base.path(), 16);
    let (heading, rows) = editor.panels.recovery().expect("nothing was offered");
    assert!(
        heading.contains(&format!("{} recovery copies", OFFERED)),
        "{heading}"
    );
    assert_eq!(rows.len(), OFFERED, "{rows:?}");
    assert!(rows[0].starts_with("`greybox`,"), "{rows:?}");
    assert!(rows[0].ends_with("under a minute ago"), "{rows:?}");
    assert_eq!(
        editor.offered[0].dir,
        copies(base.path())[0],
        "not newest first"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");

    let empty = tempfile::tempdir().expect("a temporary directory");
    let editor = recovering(empty.path(), 16);
    assert_eq!(editor.panels.recovery(), None, "an empty directory offered");
    editor.finish(ExitReason::FrameBudget).expect("teardown");

    let editor = headless(16);
    assert_eq!(editor.recovery, None, "a headless run keeps a directory");
    assert_eq!(editor.panels.recovery(), None);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Open copy opens it unowned, and Save asks for a directory**: the scene
/// in place is the copy's, with no origin and dirty, the bar is gone, Ctrl+S
/// opens the save-as line — and the copy is left as it was.
#[test]
fn open_copy_opens_it_unowned_and_save_routes_to_save_as() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (older, _) = copy_in(base.path(), now_millis() - 2000, 3.0);
    let (newer, files) = copy_in(base.path(), now_millis() - 1000, 7.0);
    let mut editor = recovering(base.path(), 64);
    editor.frame().expect("a frame");
    let (rows, _) = editor.panels.recovery_buttons();
    click_key(&mut editor, rows[0][0]);

    assert_eq!(editor.panels.recovery(), None, "the bar stayed up");
    assert_eq!(editor.document_mut().files().expect("ids"), files);
    assert_eq!(editor.document().origin(), None, "the copy became its home");
    assert!(
        editor.document().is_dirty(),
        "a recovered scene opened clean"
    );
    let (text, _) = editor.panels.status();
    assert!(text.contains("recovery copy"), "{text}");

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert_eq!(
        editor.panels.saving_as(),
        Some(""),
        "Ctrl+S did not ask for a directory"
    );
    assert_eq!(tree(&newer).len(), files.len(), "the copy was touched");
    assert!(older.exists(), "the other copy went");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Delete removes only that copy**: its directory goes, the others and
/// what is not a copy stay, and the bar lists the rest.
#[test]
fn delete_removes_only_that_copy() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let now = now_millis();
    let (oldest, _) = copy_in(base.path(), now - 3000, 1.0);
    let (middle, _) = copy_in(base.path(), now - 2000, 2.0);
    let (newest, _) = copy_in(base.path(), now - 1000, 3.0);
    let notes = base.path().join("notes");
    std::fs::create_dir(&notes).expect("a fresh base");
    let mut editor = recovering(base.path(), 64);
    editor.frame().expect("a frame");
    let (rows, _) = editor.panels.recovery_buttons();
    click_key(&mut editor, rows[1][1]);

    assert!(!middle.exists(), "the copy is still there");
    for kept in [&oldest, &newest, &notes] {
        assert!(kept.exists(), "{} went", kept.display());
    }
    let (_, rows) = editor.panels.recovery().expect("the bar went down");
    assert_eq!(rows.len(), 2, "{rows:?}");
    let offered: Vec<_> = editor.offered.iter().map(|copy| copy.dir.clone()).collect();
    assert_eq!(offered, [newest, oldest]);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Later puts the offer away**, removing nothing.
#[test]
fn later_puts_the_offer_away() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (copy, _) = copy_in(base.path(), now_millis(), 1.0);
    let mut editor = recovering(base.path(), 64);
    editor.frame().expect("a frame");
    let (_, later) = editor.panels.recovery_buttons();
    click_key(&mut editor, later);

    assert_eq!(editor.panels.recovery(), None, "the bar stayed up");
    assert!(copy.exists(), "Later removed the copy");
    let (text, _) = editor.panels.status();
    assert!(text.contains("next start"), "{text}");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A path outside the recovery directory is refused**: a Delete of a row
/// that names one removes nothing and says why.
#[test]
fn a_delete_outside_the_recovery_directory_is_refused() {
    let base = tempfile::tempdir().expect("a temporary directory");
    copy_in(base.path(), now_millis(), 1.0);
    let elsewhere = tempfile::tempdir().expect("a temporary directory");
    let (outside, _) = copy_in(elsewhere.path(), now_millis(), 1.0);
    let mut editor = recovering(base.path(), 64);
    editor.offered[0].dir = outside.clone();
    editor.frame().expect("a frame");
    let (rows, _) = editor.panels.recovery_buttons();
    click_key(&mut editor, rows[0][1]);

    assert!(outside.exists(), "a path outside the directory was removed");
    let (text, tone) = editor.panels.status();
    assert!(text.contains("not a recovery copy"), "{text}");
    assert_eq!(tone, Tone::Warning);
    assert!(editor.panels.recovery().is_some(), "the bar went down");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Start-up prunes old and excess copies, by path, and never the scene it
/// opened**: a copy past [`MAX_AGE`] goes, the oldest of more than
/// [`KEEP_NEWEST`] fresh ones goes, and an old copy named on the command
/// line stays — as does what is not a copy.
#[test]
fn start_up_prunes_old_and_excess_copies_but_not_the_one_opened() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let now = now_millis();
    let fresh: Vec<PathBuf> = (0..=KEEP_NEWEST)
        .map(|index| {
            copy_in(
                base.path(),
                now - u128::try_from(index).expect("small") * 1000,
                1.0,
            )
            .0
        })
        .collect();
    let max_age = MAX_AGE.as_millis();
    let (old, _) = copy_in(base.path(), now - max_age - DAY_MS, 1.0);
    let (opened, _) = copy_in(base.path(), now - max_age - 2 * DAY_MS, 1.0);
    let notes = base.path().join("notes");
    std::fs::create_dir(&notes).expect("a fresh base");

    let mut options = options(16);
    options.recovery = Some(base.path().to_path_buf());
    options.scene = Some(opened.clone());
    let editor = Editor::with_shell(Box::new(HeadlessShell::new()), &options)
        .expect("a copy opens as a scene");

    assert!(!old.exists(), "the old copy was left");
    assert!(!fresh[KEEP_NEWEST].exists(), "the excess copy was left");
    for kept in fresh[..KEEP_NEWEST].iter().chain([&opened, &notes]) {
        assert!(kept.exists(), "{} was pruned", kept.display());
    }
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **An autosave is written after the interval, only while dirty**: none
/// while the scene is clean however long it stays so, none before a whole
/// interval of unsaved edits, one at it — and not again while nothing
/// changed. A later change replaces the session's copy rather than adding
/// one, and another session's copy is never touched.
#[test]
fn an_autosave_is_written_after_the_interval_only_when_dirty() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (other, other_files) = copy_in(base.path(), now_millis() - DAY_MS, 1.0);
    let mut editor = recovering(base.path(), 256);
    on_test_clock(&mut editor);

    frames(&mut editor, interval_frames() * 3);
    assert_eq!(
        copies(base.path()),
        std::slice::from_ref(&other),
        "a clean scene was saved"
    );

    edit(&mut editor);
    frames(&mut editor, interval_frames() - 1);
    assert_eq!(
        copies(base.path()),
        std::slice::from_ref(&other),
        "saved before the interval"
    );
    frames(&mut editor, 1);
    let first = editor
        .autosave
        .slot
        .clone()
        .expect("no autosave at the interval");
    let edited = editor.document_mut().files().expect("ids");
    assert_eq!(
        tree(&first)["sys/blocks.ron"],
        edited["sys/blocks.ron"].as_bytes()
    );
    assert_eq!(copies(base.path()), [first.clone(), other.clone()]);

    frames(&mut editor, interval_frames() * 2);
    assert_eq!(
        editor.autosave.slot.as_ref(),
        Some(&first),
        "an unchanged scene was written again"
    );

    edit(&mut editor);
    frames(&mut editor, interval_frames());
    let second = editor.autosave.slot.clone().expect("an autosave");
    assert_ne!(second, first, "the change was not autosaved");
    assert!(!first.exists(), "the session's old autosave was left");
    let edited = editor.document_mut().files().expect("ids");
    assert_eq!(
        tree(&second)["sys/blocks.ron"],
        edited["sys/blocks.ron"].as_bytes()
    );
    assert_eq!(copies(base.path()), [second, other.clone()]);
    assert_eq!(
        tree(&other)["sys/blocks.ron"],
        other_files["sys/blocks.ron"].as_bytes(),
        "another session's copy was touched"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **An autosave in play is the authored scene**, not the played state.
#[test]
fn an_autosave_in_play_is_the_authored_scene() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let mut editor = recovering(base.path(), 256);
    editor.document = drifting_document();
    on_test_clock(&mut editor);
    edit(&mut editor);
    let authored = editor.document_mut().files().expect("ids");
    editor.act(&Action::PlayStop);
    assert_eq!(editor.document().play_state(), PlayState::Playing);

    frames(&mut editor, interval_frames());
    assert_ne!(
        editor.document_mut().files().expect("ids"),
        authored,
        "nothing moved in play, so this cannot tell the two apart"
    );
    let slot = editor.autosave.slot.clone().expect("no autosave in play");
    let written = tree(&slot);
    assert_eq!(written.len(), authored.len());
    for (key, text) in &authored {
        assert_eq!(written[key], text.as_bytes(), "`{key}` is the played state");
    }
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A clean save removes the session's autosave, and so does a discard** —
/// and neither touches another session's copy.
#[test]
fn a_clean_save_and_a_discard_remove_the_sessions_autosave() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (other, _) = copy_in(base.path(), now_millis() - DAY_MS, 1.0);
    let scenes = tempfile::tempdir().expect("a temporary directory");
    let mut editor = recovering(base.path(), 256);
    editor
        .document_mut()
        .save_as(scenes.path().join("greybox.scn"))
        .expect("a fresh directory");
    on_test_clock(&mut editor);

    edit(&mut editor);
    frames(&mut editor, interval_frames());
    let slot = editor.autosave.slot.clone().expect("an autosave");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyS);
    assert!(!editor.document().is_dirty(), "Ctrl+S did not save");
    assert!(!slot.exists(), "a clean save left the autosave");
    assert_eq!(editor.autosave.slot, None);

    edit(&mut editor);
    frames(&mut editor, interval_frames());
    let slot = editor.autosave.slot.clone().expect("an autosave");
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert!(editor.panels.unsaved().is_some(), "Ctrl+N asked nothing");
    tap(&mut editor, KeyCode::KeyD);
    assert_eq!(
        editor.document().entity_count(),
        0,
        "Discard made nothing new"
    );
    assert!(!slot.exists(), "a discard left the autosave");
    assert_eq!(copies(base.path()), [other], "another session's copy went");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The help text states what the constants hold**, so a change to one is
/// a change to the other. Its words, read with every run of whitespace as one
/// space, so a rewrapped paragraph still matches.
#[test]
fn the_help_text_states_the_recovery_constants() {
    let usage = crate::args::USAGE
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(MAX_AGE, Duration::from_secs(14 * 24 * 60 * 60));
    assert!(usage.contains("older than two weeks"), "{usage}");
    assert_eq!(KEEP_NEWEST, 20);
    assert!(usage.contains("past the newest twenty"), "{usage}");
    assert!((AUTOSAVE_SECONDS - 60.0).abs() < f64::EPSILON);
    assert!(usage.contains("(settings.toml, default 60)"), "{usage}");
    assert!(usage.contains(AUTOSAVE_KEY), "{usage}");
}

/// **A recovery copy written as the window is taken away supersedes the
/// session's autosave**, which goes.
#[test]
fn a_window_taken_away_replaces_the_autosave_with_a_recovery_copy() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let mut editor = recovering(base.path(), 256);
    on_test_clock(&mut editor);
    edit(&mut editor);
    frames(&mut editor, interval_frames());
    let slot = editor.autosave.slot.clone().expect("an autosave");
    edit(&mut editor);

    let window = editor.window;
    editor
        .shell_mut()
        .destroy_window(window)
        .expect("the window is open");
    assert_eq!(
        editor.frame().expect("a frame"),
        Flow::Stop(ExitReason::WindowDestroyed)
    );
    assert!(!slot.exists(), "the autosave was left beside the copy");
    let left = copies(base.path());
    assert_eq!(left.len(), 1, "{left:?}");
    let edited = editor.document_mut().files().expect("ids");
    assert_eq!(
        tree(&left[0])["sys/blocks.ron"],
        edited["sys/blocks.ron"].as_bytes()
    );
    editor
        .finish(ExitReason::WindowDestroyed)
        .expect("teardown");
}

/// **Open copy is refused in play mode**, as an open is: the playing scene
/// stays, and the offer with it.
#[test]
fn open_copy_is_refused_in_play() {
    let base = tempfile::tempdir().expect("a temporary directory");
    copy_in(base.path(), now_millis(), 9.0);
    let mut editor = recovering(base.path(), 64);
    let files = editor.document_mut().files().expect("ids");
    editor.act(&Action::PlayStop);
    assert_eq!(editor.document().play_state(), PlayState::Playing);
    editor.frame().expect("a frame");
    let (rows, _) = editor.panels.recovery_buttons();
    click_key(&mut editor, rows[0][0]);

    assert_eq!(editor.document().play_state(), PlayState::Playing);
    let (text, _) = editor.panels.status();
    assert!(text.contains("play mode"), "{text}");
    assert!(editor.panels.recovery().is_some(), "the offer went");
    editor.act(&Action::PlayStop);
    assert_eq!(editor.document_mut().files().expect("ids"), files);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A scene opened while copies are offered keeps the offer up**: the
/// panels are built afresh for it, and the bar with them.
#[test]
fn an_open_keeps_the_offer_up() {
    let base = tempfile::tempdir().expect("a temporary directory");
    copy_in(base.path(), now_millis(), 1.0);
    let scenes = tempfile::tempdir().expect("a temporary directory");
    let scene = scenes.path().join("kept.scn");
    Document::built_in()
        .expect("the compiled-in scene")
        .save_to(&scene)
        .expect("a fresh directory");
    let mut editor = recovering(base.path(), 64);
    assert!(editor.panels.recovery().is_some());

    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyO);
    super::files::type_and_enter(&mut editor, &scene.display().to_string());
    assert_eq!(
        editor.document().origin(),
        Some(scene.as_path()),
        "nothing opened"
    );
    assert!(
        editor.panels.recovery().is_some(),
        "the open dropped the offer"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Discard on a close removes the session's autosave** before the run
/// ends, with no frame after it to notice the scene went.
#[test]
fn a_discarded_close_removes_the_sessions_autosave() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let mut editor = recovering(base.path(), 256);
    on_test_clock(&mut editor);
    edit(&mut editor);
    frames(&mut editor, interval_frames());
    let slot = editor.autosave.slot.clone().expect("an autosave");

    let window = editor.window;
    editor.shell_mut().request_close(window).expect("live");
    assert_eq!(editor.frame().expect("a frame"), Flow::Continue);
    assert!(editor.panels.unsaved().is_some(), "the close asked nothing");
    editor
        .shell_mut()
        .key_press(window, KeyCode::KeyD)
        .expect("live");
    assert_eq!(
        editor.frame().expect("a frame"),
        Flow::Stop(ExitReason::CloseRequested)
    );
    assert!(!slot.exists(), "a discarded close left the autosave");
    editor.finish(ExitReason::CloseRequested).expect("teardown");
}
