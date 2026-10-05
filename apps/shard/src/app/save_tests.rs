//! The saves through the running game and headless — the autosave's cadence
//! and the save on close: each test keeps its saves in a scratch directory of
//! its own, never a real data directory.

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

/// How many autosave periods the cadence test plays through: more than one,
/// so a write that came once and never again is red.
const PERIODS: u64 = 3;

/// **The autosave writes once a period of ticks, and never before the
/// first**: a character walking through [`PERIODS`] periods on the headless
/// run's manual clock leaves nothing on the disk until the first period's
/// last tick, and after every frame the accepted-write counter is the
/// periods played — each write landing on the tick that ends its period, and
/// the file then loading back as the stage on that tick.
#[test]
fn the_autosave_writes_once_a_period_and_never_before_the_first() {
    let dir = scratch("autosave-cadence");
    let mut engine = saving_in(&dir);
    let period = crate::save::save_ticks(crate::game::DEFAULT_TICK_HZ);
    let window = engine.window();
    engine
        .shell_mut()
        .key_press(window, KeyCode::KeyW)
        .expect("the window is live");
    let mut written = Vec::new();
    while engine.game().stats.ticks < PERIODS * period {
        let saves = engine.game().saves();
        // The run's frame budget bounds the loop: a clock that stopped
        // ticking ends the run, which fails here rather than spinning.
        assert_eq!(
            engine.frame().expect("a frame"),
            Flow::Continue,
            "the run stopped short of {PERIODS} periods"
        );
        let ticks = engine.game().stats.ticks;
        assert_eq!(
            engine.game().saves(),
            ticks / period,
            "{ticks} ticks into a {period}-tick period"
        );
        if ticks < period {
            assert!(files_in(&dir).is_empty(), "a save came at tick {ticks}");
        }
        if engine.game().saves() > saves {
            let snapshot = engine.game().game().snapshot();
            let loaded = Vault::at(dir.clone()).load();
            assert_eq!(
                loaded.as_ref(),
                Some(&snapshot),
                "the save written by tick {ticks} is not the stage on it"
            );
            written.push(snapshot);
        }
    }
    let ticks: Vec<u64> = written.iter().map(|character| character.tick).collect();
    let periods: Vec<u64> = (1..=PERIODS).map(|n| n * period).collect();
    assert_eq!(
        ticks, periods,
        "the writes did not land on the periods' ends"
    );
    assert_ne!(
        written[0].centre, written[1].centre,
        "the walk went nowhere, so a first save left in place would pass for a second"
    );
    assert_eq!(
        engine.game().desk.last_trigger(),
        Some(crcbl::save::SaveTrigger::Autosave),
        "the autosave did not go through the one save path"
    );
    let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
    assert_eq!(
        summary.saves, PERIODS,
        "the summary lost count of the writes"
    );
    std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
}

/// **A save the game refuses reaches the console that asked**: a `save`
/// run at boot, before this session's first tick, is refused by shard's one
/// save path, the console prints why, and nothing is written or counted.
#[test]
fn a_console_save_the_game_refuses_is_printed_and_writes_nothing() {
    let dir = scratch("console-refused");
    let mut options = headless(4_000);
    options.common.exec = vec!["save".to_owned()];
    let logs = crcbl::core::log::capture();
    let mut engine = scripted(&options);
    engine.game_mut().vault = Vault::at(dir.clone());
    frames(&mut engine, 1);

    let printed: Vec<String> = logs
        .records()
        .into_iter()
        .map(|record| record.message)
        .collect();
    assert!(
        printed
            .iter()
            .any(|line| line == "not saved: nothing has been played this session yet"),
        "the refusal never reached the console: {printed:?}"
    );
    assert!(files_in(&dir).is_empty(), "a refused save wrote a file");
    assert_eq!(engine.game().saves(), 0, "a refused save was counted");
    std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
}
