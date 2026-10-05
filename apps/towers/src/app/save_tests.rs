//! Saving and resuming through the running game, headless: the save key, the
//! autosave at a wave's end, the save on close, `--resume` and the lobby's
//! *Continue* — each with its saves in a scratch directory, never a real data
//! directory.

use crcbl::core::input::KeyCode;
use crcbl::engine::{ExitReason, Flow};
use crcbl::shell::HeadlessShell;

use crcbl::save::SaveTrigger;

use super::tests::{
    frames, headless, headless_with, in_a_lobby, joined, lobby_row, loopback_host, scripted, tap,
};
use super::{Loop, Options, resume};
use crate::game::GameError;
use crate::map::Map;
use crate::save::{PLAYER_FILE, SaveError, Vault};

/// A headless run, with its saves in `dir` rather than nowhere.
fn saving_in(dir: &std::path::Path) -> Loop<HeadlessShell> {
    let mut engine = scripted(&headless(4000));
    engine.game_mut().vault = Vault::at(dir.to_path_buf(), PLAYER_FILE);
    engine
}

/// The line on the page, if one is up.
fn notice(engine: &Loop<HeadlessShell>) -> Option<String> {
    engine.game().notice.as_ref().map(|(line, _)| line.clone())
}

/// **`S` saves the run between waves and is refused while a wave is coming
/// in**, each said on the page. The first build phase saves, and the file is
/// there; the wave sent at once and `S` pressed while it releases is refused
/// by name, and writes nothing.
#[test]
fn s_saves_between_waves_and_is_refused_while_a_wave_comes_in() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let file = dir.path().join(PLAYER_FILE);
    let mut engine = saving_in(dir.path());
    frames(&mut engine, 4);

    tap(&mut engine, KeyCode::KeyS);
    assert_eq!(
        notice(&engine).as_deref(),
        Some("SAVED: WAVE 0/10"),
        "the build phase did not save"
    );
    assert!(file.is_file(), "the save is not in the file");
    assert_eq!(
        engine.game().desk().last_trigger(),
        Some(SaveTrigger::Input),
        "the key's save did not go through the one save path"
    );
    std::fs::remove_file(&file).expect("the save this test wrote");

    tap(&mut engine, KeyCode::KeyN);
    assert!(
        engine.game().game().stats().next_wave_in.is_none(),
        "the wave did not start"
    );
    tap(&mut engine, KeyCode::KeyS);
    assert_eq!(
        notice(&engine).as_deref(),
        Some("NOT SAVED: A WAVE IS COMING IN")
    );
    assert!(!file.exists(), "a save was written while a wave came in");
}

/// **A wave's end is saved by the running game**, with nothing pressed: no
/// file while the first wave is still to come or coming in, and the run at
/// its end once it has.
#[test]
fn a_waves_end_is_autosaved_by_the_running_game() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let file = dir.path().join(PLAYER_FILE);
    let mut engine = saving_in(dir.path());
    let mut frame = 0;
    while !file.exists() {
        let stats = engine.game().game().stats();
        assert!(
            stats.wave == 0 || stats.next_wave_in.is_none(),
            "the first wave ended at frame {frame} and nothing was saved"
        );
        frames(&mut engine, 1);
        frame += 1;
        assert!(frame < 3000, "no autosave in {frame} frames");
    }
    let saved = Vault::at(dir.path().to_path_buf(), PLAYER_FILE)
        .load(&Map::built_in())
        .expect("the autosave this run wrote")
        .expect("a save is there");
    assert_eq!(saved.wave(), 1, "the autosave is not the first wave's end");
    assert_eq!(
        engine.game().desk().last_trigger(),
        Some(SaveTrigger::Autosave),
        "the autosave did not go through the one save path"
    );
}

/// **The debug console's `save` reaches the one save path, slot and all**: a
/// `save slot2` run at boot writes the slot's file beside the player's — and
/// not the player's — the save resumes, and the desk records the console as
/// what asked.
#[test]
fn the_consoles_save_writes_the_named_slot_through_the_one_path() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut engine = scripted(&headless_with(4000, |common| {
        common.exec = vec!["save slot2".to_owned()];
    }));
    engine.game_mut().vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    frames(&mut engine, 1);

    assert!(
        !dir.path().join(PLAYER_FILE).exists(),
        "the slot's save went to the player's own file"
    );
    let slot = Vault::at(dir.path().to_path_buf(), "towers-run-slot2.crb")
        .load(&Map::built_in())
        .expect("the slot's save reads back");
    assert_eq!(slot.map(|saved| saved.wave()), Some(0));
    assert_eq!(
        engine.game().desk().last_trigger(),
        Some(SaveTrigger::Console)
    );
}

/// **`--resume` opens on the saved run, and refuses to start without one** —
/// none at all, or one played on another map — by name.
#[test]
fn resume_opens_on_the_saved_run_and_refuses_without_one() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    let mut game = crate::Game::new(crate::DEFAULT_TICK_HZ, &Map::built_in())
        .expect("a solo game always starts");
    assert!(matches!(
        resume(&mut game, &vault),
        Err(GameError::Resume(SaveError::NoSave))
    ));

    let saved = crate::save::tests::a_first_waves_end();
    vault
        .store(&saved)
        .expect("the scratch directory is writable");
    resume(&mut game, &vault).expect("the run this test saved");
    let stats = game.stats();
    assert_eq!(
        (stats.wave, stats.gold, stats.lives),
        (saved.wave(), saved.gold(), saved.lives())
    );

    let plot = |label: &str, z: f64| crate::scene::Plot {
        label: label.to_string(),
        position: [0.0, 0.0, z],
    };
    let other = Map::new(
        vec![
            crcbl::math::DVec3::new(-10.0, 0.0, 0.0),
            crcbl::math::DVec3::new(10.0, 0.0, 0.0),
        ],
        vec![plot("north", -3.0), plot("south", 3.0)],
    )
    .expect("a straight lane with a plot either side is a map");
    let mut elsewhere =
        crate::Game::new(crate::DEFAULT_TICK_HZ, &other).expect("a solo game always starts");
    assert!(matches!(
        resume(&mut elsewhere, &vault),
        Err(GameError::Resume(SaveError::OtherMap))
    ));
}

/// **The lobby's *Continue* is its first row, and Enter on it plays the saved
/// run** in place of the fresh one under the lobby, which never ticked.
#[test]
fn continue_from_the_lobby_plays_the_saved_run() {
    let saved = crate::save::tests::a_first_waves_end();
    let mut engine = in_a_lobby(None);
    let lobby = engine
        .game_mut()
        .lobby
        .take()
        .expect("the loop opened on a lobby");
    engine.game_mut().lobby = Some(lobby.offering(Ok(Some(saved.clone()))));
    frames(&mut engine, 4);
    assert_eq!(lobby_row(&engine), Some(crate::menu::CONTINUE_ID));

    tap(&mut engine, KeyCode::Enter);
    assert!(!engine.game().in_the_lobby(), "continue left the lobby up");
    let stats = engine.game().game().stats();
    assert_eq!(
        (stats.wave, stats.gold, stats.lives),
        (saved.wave(), saved.gold(), saved.lives()),
        "the run continued is not the saved one"
    );
}

/// Asks the window to close, as its title-bar button would, runs the frame
/// that reads it and tears the run down — the whole way a player's close
/// goes, `HostedGame::exiting` included.
fn close(mut engine: Loop<HeadlessShell>) {
    let window = engine.window();
    engine
        .shell_mut()
        .request_close(window)
        .expect("the window is live");
    assert_eq!(
        engine.frame().expect("a frame"),
        Flow::Stop(ExitReason::CloseRequested)
    );
    engine.finish(ExitReason::CloseRequested).expect("teardown");
}

/// The files in `dir`, by name.
fn files_in(dir: &std::path::Path) -> Vec<std::ffi::OsString> {
    std::fs::read_dir(dir)
        .expect("the scratch directory")
        .map(|entry| entry.expect("an entry").file_name())
        .collect()
}

/// **A close in the build phase saves the run, and the save resumes to the
/// same stage**: a tower bought before the first wave, the window closed with
/// nothing saved before it, and the file it leaves resumed onto a fresh game
/// hashes as the stage that closed.
#[test]
fn a_close_in_the_build_phase_saves_a_run_that_resumes_to_the_same_stage() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut engine = saving_in(dir.path());
    frames(&mut engine, 4);
    tap(&mut engine, KeyCode::KeyB);
    frames(&mut engine, 30);
    let stats = engine.game().game().stats();
    assert_eq!(stats.towers, 1, "the tower was not built");
    assert!(stats.next_wave_in.is_some(), "the run left the build phase");
    assert!(
        files_in(dir.path()).is_empty(),
        "a save came before the close"
    );
    let closing = engine
        .game()
        .game()
        .stage_fingerprint()
        .expect("solo has a stage");

    close(engine);
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    let mut resumed = crate::Game::new(crate::DEFAULT_TICK_HZ, &Map::built_in())
        .expect("a solo game always starts");
    resume(&mut resumed, &vault).expect("the close saved the run");
    assert_eq!(
        resumed.stage_fingerprint(),
        Some(closing),
        "the run the close saved is not the run that closed"
    );
}

/// **A close while a wave comes in keeps the save before it**, byte for byte:
/// a save a wave's end could have written is there, the next wave is sent,
/// and the window closed while it releases writes nothing over it.
#[test]
fn a_close_mid_wave_leaves_the_last_save_untouched() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let file = dir.path().join(PLAYER_FILE);
    Vault::at(dir.path().to_path_buf(), PLAYER_FILE)
        .store(&crate::save::tests::a_first_waves_end())
        .expect("the scratch directory is writable");
    let before = std::fs::read(&file).expect("the save this test wrote");

    let mut engine = saving_in(dir.path());
    frames(&mut engine, 4);
    tap(&mut engine, KeyCode::KeyN);
    assert!(
        engine.game().game().stats().next_wave_in.is_none(),
        "the wave did not start"
    );
    close(engine);
    assert_eq!(
        std::fs::read(&file).expect("the save is still there"),
        before,
        "a close mid-wave wrote over the last save"
    );
}

/// **A run stopped by its frame budget writes nothing**, build phase or not:
/// it was told when to stop, and a scripted run must never write over a
/// player's run.
#[test]
fn a_run_its_budget_stops_saves_nothing() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut engine = saving_in(dir.path());
    frames(&mut engine, 4);
    assert!(engine.game().game().stats().next_wave_in.is_some());
    engine.finish(ExitReason::FrameBudget).expect("teardown");
    assert!(files_in(dir.path()).is_empty(), "the budget's stop saved");
}

/// **A close in the lobby writes nothing**: the fresh run under it never
/// ticked, and saving it would put a new run over the one *Continue* offers.
#[test]
fn a_close_in_the_lobby_saves_nothing() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut engine = in_a_lobby(None);
    engine.game_mut().vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    frames(&mut engine, 4);
    assert!(engine.game().in_the_lobby());
    close(engine);
    assert!(files_in(dir.path()).is_empty(), "the lobby's close saved");
}

/// **A joiner's close writes nothing**, while its join waits for the host's
/// map — over the idle solo run under the joining panel — and once it plays
/// the host's run, which only the host saves.
#[test]
fn a_joiners_close_saves_nothing() {
    use crate::lan::tests::{FRAME, MAX_FRAMES, PAUSE};

    let (mut host, address) = loopback_host();
    let joiner = || {
        scripted(&Options {
            lan: crcbl::lan::LanMode::Join(address),
            ..headless(4000)
        })
    };

    let waiting_dir = tempfile::tempdir().expect("a scratch directory");
    let mut waiting = joiner();
    waiting.game_mut().vault = Vault::at(waiting_dir.path().to_path_buf(), PLAYER_FILE);
    frames(&mut waiting, 1);
    assert!(waiting.game().is_joining(), "the join is not waiting");
    close(waiting);
    assert!(
        files_in(waiting_dir.path()).is_empty(),
        "a close while joining saved the run under the panel"
    );

    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut engine = joiner();
    engine.game_mut().vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    for _ in 0..MAX_FRAMES {
        if joined(&engine).is_some() {
            break;
        }
        host.tick();
        host.frame(FRAME);
        frames(&mut engine, 1);
        std::thread::sleep(PAUSE);
    }
    assert_eq!(joined(&engine), Some(address), "the join never played");
    assert!(
        host.checkpoint().is_ok(),
        "the host's run cannot be saved now, so the joiner's close proves nothing"
    );
    close(engine);
    assert!(files_in(dir.path()).is_empty(), "a joiner's close saved");
}
