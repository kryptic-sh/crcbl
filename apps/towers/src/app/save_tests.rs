//! Saving and resuming through the running game, headless: the save key, the
//! autosave at a wave's end, `--resume` and the lobby's *Continue* — each
//! with its saves in a scratch directory, never a real data directory.

use crcbl::core::input::KeyCode;
use crcbl::shell::HeadlessShell;

use super::tests::{frames, headless, in_a_lobby, lobby_row, scripted, tap};
use super::{Loop, resume};
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
