use super::*;
use crcbl::input::{GamepadEvent, GamepadId, GamepadSnapshot, PadAxis, PadKind};

/// The keyboard prompt, as a run opens on.
const KEYBOARD_PROMPT: &str =
    "WASD walk   Shift run   Space jump   Q/E turn the camera   R/F tilt it";

/// The prompt once an Xbox pad has spoken. The camera keys stay: they are the
/// only camera there is.
const XBOX_PROMPT: &str = "Left stick walk   RB run   A jump   Q/E turn the camera   R/F tilt it";

/// One snapshot of an Xbox pad, edited from neutral.
fn xbox(map: &mut ActionMap, edit: impl FnOnce(&mut GamepadSnapshot)) {
    let mut snapshot = GamepadSnapshot::neutral(PadKind::Xbox);
    edit(&mut snapshot);
    map.gamepad_event(&GamepadEvent::State {
        id: GamepadId(1),
        snapshot,
    });
}

/// **The pad asks the simulation for exactly what the keys do.** Walking
/// forward at a run and jumping, once from `W`, Shift and Space and once from
/// the left stick, the right bumper and South: the two [`Controls`] are equal,
/// so the state machine on the server cannot tell them apart.
#[test]
fn a_pad_and_the_keys_ask_the_simulation_for_the_same_thing() {
    let mut keys = action_map();
    keys.begin_tick(0.0);
    for key in [KeyCode::KeyW, KeyCode::ShiftLeft, KeyCode::Space] {
        keys.key_event(key, true);
    }

    let mut pad = action_map();
    pad.begin_tick(0.0);
    xbox(&mut pad, |snapshot| {
        snapshot.axes[PadAxis::LeftY as usize] = 1.0;
        snapshot.buttons.insert(RUN_PAD_BUTTON);
        snapshot.buttons.insert(PadButton::South);
    });

    let from_keys = controls(&keys, 0.0);
    assert!(
        from_keys.forward && from_keys.run && from_keys.jump,
        "the keys asked for {from_keys:?}"
    );
    assert_eq!(controls(&pad, 0.0), from_keys);
}

/// **A stick pushed a little off its axis still walks straight**, and one
/// resting inside its dead zone walks nowhere — the eight-way split, through
/// the real map rather than through [`crcbl::input::eight_way`] alone.
#[test]
fn a_stick_walks_eight_ways_and_a_resting_one_walks_nowhere() {
    let mut map = action_map();
    xbox(&mut map, |snapshot| {
        snapshot.axes[PadAxis::LeftX as usize] = 0.2;
        snapshot.axes[PadAxis::LeftY as usize] = 0.9;
    });
    let walked = controls(&map, 0.0);
    assert!(
        walked.forward && !walked.left && !walked.right && !walked.back,
        "a stick pushed nearly straight up asked for {walked:?}"
    );

    xbox(&mut map, |snapshot| {
        snapshot.axes[PadAxis::LeftX as usize] = STICK_DEAD_ZONE * 0.9;
    });
    assert_eq!(controls(&map, 0.0), Controls::default(), "drift walked");
}

/// **The prompt names the device the player last used**: the keyboard's words
/// before anything spoke, the Xbox pad's once it did, and the keyboard's again
/// after a key.
#[test]
fn the_prompt_names_the_last_device() {
    let mut map = action_map();
    assert_eq!(prompt(&map), KEYBOARD_PROMPT);

    xbox(&mut map, |snapshot| {
        snapshot.buttons.insert(PadButton::South)
    });
    assert_eq!(prompt(&map), XBOX_PROMPT);

    map.key_event(KeyCode::KeyQ, true);
    assert_eq!(prompt(&map), KEYBOARD_PROMPT);
}
