//! The confirm flow through the loop: the pause menu's `FULLSCREEN`, the
//! fullscreen key and a settings row's ask, each applied live, held on the
//! prompt and kept or reverted — on the engine's own fixture and a headless
//! shell, with no window on any screen.

use super::*;

use crcbl_console::Value;
use crcbl_store::MemoryStorage;

use crate::settings::confirm::REVERT_AFTER;
use crate::settings::{DISPLAY_MODE_KEY, PRESENT_MODE_KEY, VIDEO_NAMESPACE};

fn display_key() -> String {
    format!("{VIDEO_NAMESPACE}.{DISPLAY_MODE_KEY}")
}

fn present_key() -> String {
    format!("{VIDEO_NAMESPACE}.{PRESENT_MODE_KEY}")
}

/// A settings file the loop keeps changes into, for the life of the test
/// binary: a [`SettingsSource`] the loop holds is `'static`, and a leaked
/// memory store is the one that touches no disk.
fn kept_into(engine: &mut Hosted) -> &'static MemoryStorage {
    let storage: &'static MemoryStorage = Box::leak(Box::new(MemoryStorage::new()));
    engine.settings_source = SettingsSource::Source(storage);
    storage
}

/// What the loop's own settings stack says the display mode is.
fn stacked_display_mode(engine: &mut Hosted) -> Option<DisplayMode> {
    crate::settings::display_mode(&engine.console.host_mut().stack())
}

/// Whether the window's standing request is for borderless.
fn asked_borderless(engine: &mut Hosted) -> bool {
    let window = engine.window;
    engine
        .shell_mut()
        .window_state(window)
        .expect("the window is live")
        .requested_mode
        .is_borderless()
}

fn frames(engine: &mut Hosted, count: usize) {
    for _ in 0..count {
        engine.frame().expect("a headless frame");
    }
}

/// **The pause menu's `FULLSCREEN` applies at once, waits on the prompt, and
/// KEEP writes `display_mode` to the stack and the file** — the persistence the
/// button never had, through the same flow a settings row takes.
#[test]
fn the_pause_menus_fullscreen_is_kept_into_display_mode() {
    let mut engine = hosted(None);
    let storage = kept_into(&mut engine);
    frames(&mut engine, 1);

    tap(&mut engine, PAUSE_KEY);
    frames(&mut engine, 1);
    tap(&mut engine, MENU_DOWN_KEY);
    tap(&mut engine, MENU_ACTIVATE_KEY);
    frames(&mut engine, 1);

    assert!(
        asked_borderless(&mut engine),
        "FULLSCREEN did not ask the window system for borderless",
    );
    assert!(engine.confirm.is_showing(), "no prompt asked to keep it");
    assert_eq!(
        stacked_display_mode(&mut engine),
        None,
        "the trial reached the stack before the player kept it",
    );

    // A player reads the prompt over the frames the headless window system
    // takes to answer — what is kept is what landed, so KEEP before the answer
    // would keep the windowed mode the window was still in.
    frames(&mut engine, 8);
    assert!(
        engine.display_mode().is_borderless(),
        "the window never answered"
    );
    // KEEP is the highlighted button.
    tap(&mut engine, MENU_ACTIVATE_KEY);
    frames(&mut engine, 1);
    assert!(!engine.confirm.is_showing(), "KEEP left the prompt up");
    assert_eq!(
        stacked_display_mode(&mut engine),
        Some(DisplayMode::Borderless { monitor: None }),
    );
    let file = SettingsStack::from_storage(storage);
    assert_eq!(
        crate::settings::display_mode(&file),
        Some(DisplayMode::Borderless { monitor: None }),
        "the kept mode is not in the settings file",
    );
    assert!(engine.is_paused(), "answering the prompt resumed the game");
}

/// **The prompt takes the keys while it is up**: the ENTER that keeps the
/// change does not also press the game's own panel underneath.
#[test]
fn the_prompt_takes_the_keys_from_the_panel_under_it() {
    let mut engine = hosted(None);
    frames(&mut engine, 1);
    assert_eq!(engine.menu_kind(), FakeMenu::Start, "PLAY is on screen");

    tap(&mut engine, FULLSCREEN_KEY);
    frames(&mut engine, 1);
    assert!(engine.confirm.is_showing());
    tap(&mut engine, MENU_ACTIVATE_KEY);
    frames(&mut engine, 1);

    assert!(
        !engine.confirm.is_showing(),
        "ENTER did not answer the prompt"
    );
    assert!(
        !engine.game().served,
        "ENTER pressed PLAY under the prompt as well",
    );
}

/// **Not kept in time, the fullscreen key's change reverts and writes
/// nothing**, and the countdown is spent in frame time: each frame takes
/// exactly the frame's step off it.
#[test]
fn an_unkept_fullscreen_reverts_on_frame_time_and_writes_nothing() {
    let mut engine = hosted(None);
    let storage = kept_into(&mut engine);
    let step = MAX_FRAME_STEP;
    engine.set_frame_step(step);
    frames(&mut engine, 2);

    tap(&mut engine, FULLSCREEN_KEY);
    frames(&mut engine, 1);
    let started = engine.confirm.pending().expect("held").remaining();
    frames(&mut engine, 1);
    assert_eq!(
        engine.confirm.pending().expect("still held").remaining(),
        started - step,
        "a frame took something other than its own step off the countdown",
    );

    let left = engine.confirm.pending().expect("held").remaining();
    let whole_frames = left.as_nanos().div_ceil(step.as_nanos());
    frames(
        &mut engine,
        usize::try_from(whole_frames).expect("a countdown's frames") - 1,
    );
    assert!(engine.confirm.is_showing(), "reverted a frame early");
    frames(&mut engine, 1);
    assert!(
        !engine.confirm.is_showing(),
        "still waiting after the countdown"
    );

    assert!(
        !asked_borderless(&mut engine),
        "the window was not asked back to windowed",
    );
    assert!(
        !engine.console.host_mut().stack().contains(&display_key()),
        "a revert wrote the stack",
    );
    assert!(
        storage.read(std::path::Path::new(SETTINGS_FILE)).is_err(),
        "a revert wrote the file",
    );
    assert!(started <= REVERT_AFTER);
}

/// **A settings row's ask reaches the swapchain, and REVERT puts the old
/// pacing back** — and writes nothing.
#[test]
fn a_rows_present_mode_is_reverted_from_the_prompt() {
    let mut engine = hosted(None);
    frames(&mut engine, 1);
    engine.game_mut().pending_change = Some((present_key(), Value::Enum("off")));
    frames(&mut engine, 1);
    assert_eq!(
        engine.gpu().pacing,
        Pacing::Off,
        "the swapchain was not told"
    );
    assert!(engine.confirm.is_showing());

    tap(&mut engine, MENU_DOWN_KEY);
    tap(&mut engine, MENU_ACTIVATE_KEY);
    frames(&mut engine, 1);
    assert!(!engine.confirm.is_showing(), "REVERT left the prompt up");
    assert_eq!(
        engine.gpu().pacing,
        Pacing::Auto,
        "the old pacing is not back"
    );
    assert_eq!(engine.gpu().pacings_asked, [Pacing::Off, Pacing::Auto]);
    assert!(
        !engine.console.host_mut().stack().contains(&present_key()),
        "a revert wrote the stack",
    );
}

/// **A second fullscreen press while the first waits goes back, and leaves
/// nothing waiting** — pressing the key again is the player's undo.
#[test]
fn a_second_fullscreen_press_while_waiting_leaves_nothing_waiting() {
    let mut engine = hosted(None);
    frames(&mut engine, 1);
    tap(&mut engine, FULLSCREEN_KEY);
    // Long enough for the headless window system to answer the request, so
    // the second press reads the window as borderless.
    frames(&mut engine, 8);
    assert!(
        engine.display_mode().is_borderless(),
        "the window never answered"
    );
    assert!(engine.confirm.is_showing());

    tap(&mut engine, FULLSCREEN_KEY);
    frames(&mut engine, 8);
    assert!(
        !engine.confirm.is_showing(),
        "the undo left a change waiting"
    );
    assert_eq!(engine.display_mode(), DisplayMode::Windowed);
    assert_eq!(stacked_display_mode(&mut engine), None);
}

/// **What landed is what the prompt shows and what KEEP writes**: the window
/// system's answer arrives frames after the request, and replaces what the
/// request left behind.
#[test]
fn the_prompt_shows_and_keeps_what_the_window_system_did() {
    let mut engine = hosted(None);
    frames(&mut engine, 1);
    let window = engine.window;
    let windowed = engine.extent();
    tap(&mut engine, FULLSCREEN_KEY);
    frames(&mut engine, 1);
    // The answer: a refusal, which is what a tiling window manager sends.
    engine
        .shell_mut()
        .resize(
            window,
            crcbl_shell::PhysicalSize::new(windowed.0, windowed.1),
        )
        .expect("the window is live");
    frames(&mut engine, 4);

    let held = engine.confirm.pending().expect("held");
    assert_eq!(held.asked(), &Value::Enum("borderless"));
    assert_eq!(
        held.landed(),
        &Value::Enum("windowed"),
        "the prompt shows the request rather than what the window is in",
    );
    let lines = confirm::prompt_lines(held);
    assert_eq!(lines[0].text, "display mode: windowed");

    tap(&mut engine, MENU_ACTIVATE_KEY);
    frames(&mut engine, 1);
    assert_eq!(
        stacked_display_mode(&mut engine),
        Some(DisplayMode::Windowed)
    );
}

/// **A file that says borderless opens the window borderless**, and one that
/// says windowed leaves a `--fullscreen` window alone.
#[test]
fn the_players_display_mode_is_put_into_force_before_the_first_frame() {
    let over = |toml: &str, desc: &crcbl_shell::WindowDesc<'_>| {
        let storage = MemoryStorage::new();
        storage
            .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
            .expect("memory takes every write");
        let game = FakeGame {
            settings: Some(crate::settings::SharedSettings::new(
                SettingsStack::from_storage(&storage),
            )),
            ..FakeGame::default()
        };
        hosted_with(desc, None, game)
    };

    let mut engine = over(
        "[engine.video]\ndisplay_mode = \"borderless\"\n",
        &crcbl_shell::WindowDesc::default(),
    );
    assert!(
        asked_borderless(&mut engine),
        "the file's borderless did not reach the window",
    );

    let mut engine = over(
        "[engine.video]\ndisplay_mode = \"windowed\"\n",
        &crcbl_shell::WindowDesc {
            mode: DisplayMode::Borderless { monitor: None },
            ..crcbl_shell::WindowDesc::default()
        },
    );
    assert!(
        asked_borderless(&mut engine),
        "the file outranked the command line's --fullscreen",
    );
}

/// **A player's `present_mode` replaces `Auto` when a context opens, and does
/// not replace a pacing the caller named.**
#[test]
fn the_players_present_mode_replaces_auto_and_not_a_named_pacing() {
    let storage = MemoryStorage::new();
    storage
        .write(
            std::path::Path::new(SETTINGS_FILE),
            b"[engine.video]\npresent_mode = \"off\"\n",
        )
        .expect("memory takes every write");
    let open = |pacing| {
        GpuContext::open_offscreen(
            (64, 64),
            &GpuContextDesc {
                backend: Some(GpuBackend::Null),
                pacing,
                settings: SettingsSource::Source(&storage),
                ..GpuContextDesc::default()
            },
        )
        .expect("the null backend opens everywhere")
    };

    let auto = open(Pacing::Auto);
    assert_eq!(auto.pacing(), Pacing::Off, "the file did not replace Auto");
    auto.destroy().expect("teardown");

    let named = open(Pacing::Vsync);
    assert_eq!(
        named.pacing(),
        Pacing::Vsync,
        "the file outranked `--pacing vsync`"
    );
    named.destroy().expect("teardown");
}
