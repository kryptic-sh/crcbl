//! The save on close, through the running game and headless: each test keeps
//! its saves in a scratch directory of its own, never a real data directory.

use std::path::{Path, PathBuf};

use crcbl::core::input::KeyCode;
use crcbl::engine::{ExitReason, Flow};
use crcbl::shell::HeadlessShell;

use super::tests::{frames, headless, scripted};
use super::{Loop, Summary};
use crate::save::{SAVE_FILE, Vault};

/// A fresh, empty scratch directory for `test`, named for the process as well,
/// so two checkouts running this suite at once never share one.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("crcbl-shard-{test}-{}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!(
            "the scratch directory {} would not clear: {error}",
            dir.display()
        ),
    }
    std::fs::create_dir_all(&dir).expect("the scratch directory is writable");
    dir
}

/// The files in `dir`, by name.
fn files_in(dir: &Path) -> Vec<std::ffi::OsString> {
    std::fs::read_dir(dir)
        .expect("the scratch directory")
        .map(|entry| entry.expect("an entry").file_name())
        .collect()
}

/// A headless run whose saves go to `dir`: the run itself opens nowhere, as
/// every headless run does, and the test hands it the directory afterwards.
fn saving_in(dir: &Path) -> Loop<HeadlessShell> {
    let mut engine = scripted(&headless(4_000));
    engine.game_mut().vault = Vault::at(dir.to_path_buf());
    engine
}

/// Asks the window to close, as its title-bar button would, runs the frame
/// that reads it and tears the run down — the whole way a player's close
/// goes, `HostedGame::exiting` included.
fn close(mut engine: Loop<HeadlessShell>) -> Summary {
    let window = engine.window();
    engine
        .shell_mut()
        .request_close(window)
        .expect("the window is live");
    assert_eq!(
        engine.frame().expect("a frame"),
        Flow::Stop(ExitReason::CloseRequested)
    );
    engine.finish(ExitReason::CloseRequested).expect("teardown")
}

/// **A close writes the character, and the save loads back as the stage that
/// closed**: a walk off the spawn short of the first autosave, so nothing was
/// on the disk before the close, and the file it leaves read back field for
/// field as the snapshot taken as the window went.
#[test]
fn a_close_saves_a_character_that_loads_back_as_the_stage_that_closed() {
    let dir = scratch("close-saves");
    let mut engine = saving_in(&dir);
    let window = engine.window();
    engine
        .shell_mut()
        .key_press(window, KeyCode::KeyW)
        .expect("the window is live");
    frames(&mut engine, 30);
    let ticks = engine.game().game().stats().ticks;
    assert!(
        ticks < crate::save::save_ticks(crate::game::DEFAULT_TICK_HZ),
        "{ticks} ticks reached an autosave, so a file proves nothing about the close"
    );
    assert!(files_in(&dir).is_empty(), "a save came before the close");
    let closing = engine.game().game().snapshot();
    let fresh = crate::Game::new(
        crate::game::DEFAULT_TICK_HZ,
        crate::loot::DEFAULT_SEED,
        None,
    )
    .expect("a fresh zone always starts")
    .snapshot();
    assert_ne!(
        closing.centre, fresh.centre,
        "the walk went nowhere, so a fresh zone would pass for the one that closed"
    );

    let summary = close(engine);
    assert_eq!(
        Vault::at(dir.clone()).load(),
        Some(closing),
        "the character the close saved is not the one that closed"
    );
    assert_eq!(summary.saves, 1, "the close's write was not counted");
    std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
}

/// **A run stopped by its frame budget writes nothing**, though it has
/// ticked and so would be saveable: it was told when to stop, and a scripted
/// run must never write over a player's character.
#[test]
fn a_run_its_budget_stops_saves_nothing() {
    let dir = scratch("budget-saves-nothing");
    let mut engine = saving_in(&dir);
    frames(&mut engine, 4);
    assert!(
        engine.game().game().stats().ticks > 0,
        "no tick ran, so the stop proves nothing about the budget"
    );
    assert!(files_in(&dir).is_empty(), "a save came before the stop");
    let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
    assert!(files_in(&dir).is_empty(), "the budget's stop saved");
    assert_eq!(summary.saves, 0);
    std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
}

/// **A close before the first tick keeps the last save, byte for byte**: a
/// character a previous session left is on the disk, and a session closed
/// before it played anything writes its untouched zone over nothing.
#[test]
fn a_close_before_the_first_tick_leaves_the_last_save_untouched() {
    let dir = scratch("close-before-a-tick");
    assert!(
        Vault::at(dir.clone()).store(&crate::save::tests::walked()),
        "the scratch directory is writable"
    );
    let file = dir.join(SAVE_FILE);
    let before = std::fs::read(&file).expect("the save this test wrote");

    let engine = saving_in(&dir);
    assert_eq!(engine.game().stats.ticks, 0, "a tick ran before the close");
    let summary = close(engine);
    assert_eq!(
        std::fs::read(&file).expect("the save is still there"),
        before,
        "a close before the first tick wrote over the last save"
    );
    assert_eq!(summary.saves, 0);
    std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
}
