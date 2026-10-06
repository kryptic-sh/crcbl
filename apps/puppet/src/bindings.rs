//! Puppet's controls: the action map it is played with on a keyboard and on a
//! pad, what that map asks the simulation for, the prompt line that names the
//! device the player is holding, and the rows a player may rebind.
//!
//! # One map, two devices, no branch
//!
//! Every action is bound on both devices at once, so picking up a pad mid-walk
//! needs nothing from this sample: `move` is one [`ActionKind::Axis2`] that
//! `WASD`, the arrows, the left stick and the d-pad all sum into, and `run` and
//! `jump` each carry a key and a pad button. [`controls`] reads the same
//! actions whichever device moved them, so the state machine on the server is
//! driven identically from either — it never learns which one it was.
//!
//! # The prompt follows the last device
//!
//! [`prompt`] names each action by [`ActionMap::hint`]: the binding on the
//! device that last spoke, labelled in [`crcbl::input::DefaultLabels`]' words.
//! [`crate::app::Puppet`] rebuilds it when
//! [`ActionMap::last_device_changed`] rises, and after a rebind. What counts
//! as the device speaking is the input layer's rule, so a pad stick drifting
//! inside [`PAD_ACTIVITY_THRESHOLD`](crcbl::input::PAD_ACTIVITY_THRESHOLD)
//! never takes the prompt from the keyboard. Text, not glyph artwork: what a
//! button looks like is a game's art direction, and this sample has none —
//! `crates/crcbl-input/src/hint.rs` says why the engine ships no images.
//!
//! # What a player may rebind
//!
//! [`ROWS`]: run and jump, through [`crcbl::rebind`] — the flow
//! `apps/options`' `CONTROLS` page runs on too. Not `move`: a capture takes one
//! key or one button, and the walk is a four-key composite and a stick, which a
//! single press cannot replace without leaving three directions unbound. Not
//! the camera keys either, which have no pad binding to keep beside a new key.

use crcbl::core::input::KeyCode;
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding, PadButton, Stick};
use crcbl::rebind::{RebindIds, RebindRow, Rebinder};
use crcbl::store::profile::ProfileStore;
use crcbl::ui::WidgetId;

use crate::game::Controls;

/// Walk, away from the camera and around it: `WASD`, the arrows, the left
/// stick and the d-pad, summed into one 2-D axis.
pub const ACTION_MOVE: &str = "move";
/// Run rather than walk, for as long as it is held. Either shift key, or
/// [`RUN_PAD_BUTTON`].
pub const ACTION_RUN: &str = "run";
/// Jump: Space, or the pad's south face button.
pub const ACTION_JUMP: &str = "jump";
/// Swing the camera about the character, anticlockwise and clockwise.
const ACTION_CAMERA_LEFT: &str = "camera-left";
/// See [`ACTION_CAMERA_LEFT`].
const ACTION_CAMERA_RIGHT: &str = "camera-right";
/// Raise and lower the camera's elevation.
const ACTION_CAMERA_UP: &str = "camera-up";
/// See [`ACTION_CAMERA_UP`].
const ACTION_CAMERA_DOWN: &str = "camera-down";

/// The pad button run is held on: the right bumper, which a thumb on the left
/// stick and a finger on the shoulder can hold together for the whole of a
/// run. A stick click, where `apps/options` puts its sprint, is a press a
/// thumb steering the same stick cannot keep down.
pub const RUN_PAD_BUTTON: PadButton = PadButton::RightShoulder;

/// The left stick's own dead zone, as a fraction of its throw: inside it the
/// stick adds nothing to `move`, so a resting stick's drift is not summed with
/// a key held beside it.
const STICK_DEAD_ZONE: f32 = 0.2;

/// How far `move` has to be pushed before it asks for a direction — applied to
/// the sum, after [`STICK_DEAD_ZONE`], so a pad stick walks once it is past
/// both. A key is always all the way over.
const MOVE_DEAD_ZONE: f32 = 0.25;

/// The keyboard and the pad this sample is walked with.
///
/// Declared in one place so the bindings and the read-out below cannot name
/// different actions: a typo in either is an action that resolves to nothing,
/// and [`ActionMap`] answers `false` for an action nobody declared rather than
/// complaining.
#[must_use]
pub fn action_map() -> ActionMap {
    let mut map = ActionMap::new();
    map.declare(ActionDecl {
        name: ACTION_MOVE.into(),
        kind: ActionKind::Axis2,
        bindings: vec![
            Binding::Wasd {
                up: KeyCode::KeyW,
                down: KeyCode::KeyS,
                left: KeyCode::KeyA,
                right: KeyCode::KeyD,
            },
            Binding::Wasd {
                up: KeyCode::ArrowUp,
                down: KeyCode::ArrowDown,
                left: KeyCode::ArrowLeft,
                right: KeyCode::ArrowRight,
            },
            Binding::PadStick {
                stick: Stick::Left,
                deadzone: STICK_DEAD_ZONE,
            },
            Binding::PadDpad,
        ],
    });
    for (name, bindings) in [
        (
            ACTION_RUN,
            vec![
                Binding::Key(KeyCode::ShiftLeft),
                Binding::Key(KeyCode::ShiftRight),
                Binding::PadButton(RUN_PAD_BUTTON),
            ],
        ),
        (
            ACTION_JUMP,
            vec![
                Binding::Key(KeyCode::Space),
                Binding::PadButton(PadButton::South),
            ],
        ),
        (ACTION_CAMERA_LEFT, vec![Binding::Key(KeyCode::KeyQ)]),
        (ACTION_CAMERA_RIGHT, vec![Binding::Key(KeyCode::KeyE)]),
        (ACTION_CAMERA_UP, vec![Binding::Key(KeyCode::KeyR)]),
        (ACTION_CAMERA_DOWN, vec![Binding::Key(KeyCode::KeyF)]),
    ] {
        map.declare(ActionDecl {
            name: name.into(),
            kind: ActionKind::Button,
            bindings,
        });
    }
    map
}

/// What the player is asking the **simulation** for on the tick `actions` has
/// just begun, at the yaw the view is currently at.
///
/// The walk is `move` split into the four directions the wire carries, by
/// [`crcbl::input::eight_way`]: the character walks at one speed (or runs at
/// one), so a stick's magnitude has nowhere to go and a stick and a key held
/// the same way ask for the same thing. Walking and running read the **held**
/// state: they happen for as long as an input is down. The jump reads the held
/// state *or* this tick's press edge, so a tap that went down and up between
/// two ticks still reaches the server, which takes the jump on the edge it
/// sees tick to tick — holding the button does not jump again. The camera
/// actions are deliberately absent — they are read on the frame's clock,
/// because the camera is not part of what the server owns.
#[must_use]
pub fn controls(actions: &ActionMap, yaw: f32) -> Controls {
    let (x, y) = actions.axis2(ACTION_MOVE);
    let (forward, back, left, right) = crcbl::input::eight_way(x, y, MOVE_DEAD_ZONE);
    Controls {
        forward,
        back,
        left,
        right,
        run: actions.button_held(ACTION_RUN),
        jump: actions.button_held(ACTION_JUMP) || actions.just_pressed(ACTION_JUMP),
        yaw,
    }
}

/// How far the camera should turn this frame, given what is held down and how
/// long the frame was: `(yaw, pitch)` in radians.
#[must_use]
pub fn camera_turn(actions: &ActionMap, seconds: f32) -> (f32, f32) {
    let axis = |positive: &str, negative: &str| {
        f32::from(i8::from(actions.button_held(positive)) - i8::from(actions.button_held(negative)))
    };
    (
        axis(ACTION_CAMERA_RIGHT, ACTION_CAMERA_LEFT) * crate::camera::TURN_RATE * seconds,
        axis(ACTION_CAMERA_UP, ACTION_CAMERA_DOWN) * crate::camera::TURN_RATE * seconds,
    )
}

/// The prompt line's entries, in the order they are printed: the actions each
/// names, and what they do.
const PROMPTS: [(&[&str], &str); 5] = [
    (&[ACTION_MOVE], "walk"),
    (&[ACTION_RUN], "run"),
    (&[ACTION_JUMP], "jump"),
    (
        &[ACTION_CAMERA_LEFT, ACTION_CAMERA_RIGHT],
        "turn the camera",
    ),
    (&[ACTION_CAMERA_UP, ACTION_CAMERA_DOWN], "tilt it"),
];

/// What goes between two of the prompt line's entries.
const PROMPT_GAP: &str = "   ";

/// The control prompt for the device the player last used — `WASD walk   Shift
/// run   Space jump …` on the keyboard, `Left stick walk   RB run   A jump …`
/// once an Xbox pad speaks. See the [module docs](self).
///
/// Each entry names its actions' hints, each label once: the camera's two
/// keys are `Q/E`, and an entry whose actions share one label — a stick bound
/// both ways — prints it once. An action with nothing on the last device falls
/// back as [`ActionMap::hint`] does, so the camera keys still read `Q/E` while
/// the player is on the pad: those keys are the only camera there is.
#[must_use]
pub fn prompt(actions: &ActionMap) -> String {
    PROMPTS
        .iter()
        .map(|(names, verb)| {
            let mut labels: Vec<String> = Vec::new();
            for hint in names.iter().filter_map(|name| actions.hint(name)) {
                if !labels.contains(&hint.label) {
                    labels.push(hint.label);
                }
            }
            format!("{} {verb}", labels.join("/"))
        })
        .collect::<Vec<_>>()
        .join(PROMPT_GAP)
}

/// The actions the controls overlay rebinds, in the order it lists them.
pub const ROWS: [RebindRow; 2] = [
    RebindRow {
        name: ACTION_RUN,
        label: "RUN",
    },
    RebindRow {
        name: ACTION_JUMP,
        label: "JUMP",
    },
];

/// The pause panel's row that opens the controls overlay: the first id the
/// loop leaves to a game.
pub const CONTROLS_ID: WidgetId = crcbl::engine::FIRST_GAME_ID;

/// The overlay's and its clash panel's ids, just after [`CONTROLS_ID`].
pub const IDS: RebindIds = RebindIds::starting_at(CONTROLS_ID + 1);

/// [`action_map`] with the player's rebinds in `store` on top, offering
/// [`ROWS`].
#[must_use]
pub fn open(store: ProfileStore) -> Rebinder {
    Rebinder::open(action_map(), ROWS.to_vec(), store)
}

#[cfg(test)]
mod tests;
