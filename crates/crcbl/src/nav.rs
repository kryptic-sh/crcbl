//! The join between `crcbl-input`'s reserved `ui` context and `crcbl-ui`'s
//! tree: a frame's [`NavInput`], built from an [`ActionMap`].
//!
//! Here, in the umbrella, because it is the one crate that names both: the
//! tree reads no device and no action map, so `crcbl-ui` gains no dependency,
//! and `crcbl-input` knows nothing of a tree.
//!
//! # How a caller drives it
//!
//! A map carrying [`input::ui::declare`](crate::input::ui::declare)'s context
//! is fed raw events like any other, has
//! [`input::ui::CONTEXT`](crate::input::ui::CONTEXT) pushed while the UI has
//! input, and begins a tick **once per UI frame** — [`NavInput`] is one frame's
//! edges, and a map ticked at the simulation's rate would hand a frame that ran
//! no tick nothing and a frame that ran two only the second's edges. Then
//! [`nav_input`] is what the frame passes to
//! [`Ui::begin_frame_with`](crate::ui::tree::Ui::begin_frame_with).
//!
//! **Nothing in the engine loop calls this yet.** [`Loop`](crate::engine::Loop)
//! hosts no tree: its menus are `crcbl_ui::menu`'s, which read raw keys through
//! `MenuPump`, and a game's map is the game's. The tree reaches a frame only in
//! the screenshot scenes, and `screenshot::ui_focus`'s tests are what hold its
//! scripted pad to what the keyboard produces through this.

use crate::input::ui::{ACCEPT, BACK, MOVE, NEXT, PREV};
use crate::input::{ActionMap, Cardinal, Device};
use crate::ui::tree::{Direction, InputMode, NavInput};

/// The frame's navigation input from `actions`' reserved `ui` actions.
///
/// A direction and the tree-order steps are their actions' repeat pulses, so a
/// held arrow steps once, then again after the delay and on every interval; a
/// 2-D move is its [`Cardinal`]. Accept and back are press edges. The mode is
/// [`input_mode`] of the last device. A map with the `ui` context off the stack
/// has idle `ui` actions and yields no press, whatever is held.
#[must_use]
pub fn nav_input(actions: &ActionMap) -> NavInput {
    let direction = if actions.repeated(MOVE) {
        actions.cardinal(MOVE).map(direction)
    } else {
        None
    };
    NavInput {
        mode: input_mode(actions.last_device()),
        direction,
        next: actions.repeated(NEXT),
        prev: actions.repeated(PREV),
        accept: actions.just_pressed(ACCEPT),
        back: actions.just_pressed(BACK),
    }
}

/// The tree's mixed-input mode for the device that last spoke.
///
/// A keyboard or a pad navigates and shows focus; a pointer, a finger on an
/// on-screen control, or nothing yet shows hover — [`InputMode::Pointer`] is
/// also [`NavInput::default`]'s, so a run nobody has touched looks as it did
/// before this existed.
#[must_use]
pub const fn input_mode(device: Option<Device>) -> InputMode {
    match device {
        Some(Device::Keyboard | Device::Gamepad) => InputMode::Navigation,
        Some(Device::Pointer | Device::Touch) | None => InputMode::Pointer,
    }
}

/// `crcbl-input`'s +Y-up cardinal as the tree's on-screen direction.
const fn direction(cardinal: Cardinal) -> Direction {
    match cardinal {
        Cardinal::Up => Direction::Up,
        Cardinal::Down => Direction::Down,
        Cardinal::Left => Direction::Left,
        Cardinal::Right => Direction::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::input::KeyCode;
    use crate::input::{ActionDecl, ActionKind, Binding, REPEAT_DELAY, REPEAT_INTERVAL, ui};

    const FRAME: f32 = 1.0 / 60.0;

    /// A game's map with the reserved context declared beside it.
    fn map() -> ActionMap {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "jump".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::Space)],
        });
        ui::declare(&mut map).expect("no clash");
        map
    }

    /// One frame: begin the tick, feed the keys, read the frame's input.
    fn frame(map: &mut ActionMap, dt: f32, keys: &[(KeyCode, bool)]) -> NavInput {
        map.begin_tick(dt);
        for &(key, pressed) in keys {
            map.key_event(key, pressed);
        }
        nav_input(map)
    }

    /// **Scripted keys become the tree's vocabulary**: each reserved action
    /// lands in its field, Shift+Tab is `prev` and not `next` as well, and the
    /// keyboard puts the tree in navigation mode.
    #[test]
    fn scripted_keys_become_nav_input() {
        let mut map = map();
        map.push_context(ui::CONTEXT).expect("declared");

        assert_eq!(nav_input(&map), NavInput::default(), "nothing spoke yet");
        assert_eq!(
            frame(&mut map, FRAME, &[(KeyCode::ArrowRight, true)]),
            NavInput::toward(Direction::Right),
        );
        assert_eq!(
            frame(
                &mut map,
                FRAME,
                &[(KeyCode::ArrowRight, false), (KeyCode::KeyW, true)]
            ),
            NavInput::toward(Direction::Up),
            "+Y up in the action map is up on screen",
        );
        assert_eq!(
            frame(
                &mut map,
                FRAME,
                &[(KeyCode::KeyW, false), (KeyCode::Tab, true)]
            ),
            NavInput::NEXT,
        );
        assert_eq!(
            frame(
                &mut map,
                FRAME,
                &[
                    (KeyCode::Tab, false),
                    (KeyCode::ShiftLeft, true),
                    (KeyCode::Tab, true)
                ]
            ),
            NavInput::PREV,
        );
        assert_eq!(
            frame(
                &mut map,
                FRAME,
                &[
                    (KeyCode::Tab, false),
                    (KeyCode::ShiftLeft, false),
                    (KeyCode::Space, true)
                ]
            ),
            NavInput::ACCEPT,
        );
        assert_eq!(
            frame(&mut map, FRAME, &[]),
            NavInput::NAVIGATION,
            "accept is the press, so a held Space does not accept every frame",
        );
        assert_eq!(
            frame(
                &mut map,
                FRAME,
                &[(KeyCode::Space, false), (KeyCode::Escape, true)]
            ),
            NavInput::BACK,
        );
        assert_eq!(
            frame(&mut map, FRAME, &[]),
            NavInput::NAVIGATION,
            "nor a held Escape back out every frame",
        );
        assert_eq!(
            frame(&mut map, FRAME, &[(KeyCode::Escape, false)]),
            NavInput::NAVIGATION,
            "a release frame presses nothing and the keyboard is still the last device",
        );
        assert!(
            !map.button_held("jump"),
            "Space went to the UI and never to the game"
        );
    }

    /// **A held arrow steps, waits the delay, then steps every interval** —
    /// the repeat reaching the tree, not only the action map.
    #[test]
    fn a_held_direction_repeats_into_the_frames() {
        let mut map = map();
        map.push_context(ui::CONTEXT).expect("declared");
        let down = NavInput::toward(Direction::Down);

        assert_eq!(frame(&mut map, FRAME, &[(KeyCode::ArrowDown, true)]), down);
        assert_eq!(
            frame(&mut map, FRAME, &[]),
            NavInput::NAVIGATION,
            "held is not a step"
        );
        assert_eq!(
            frame(&mut map, REPEAT_DELAY, &[]),
            down,
            "the delay elapsed"
        );
        assert_eq!(
            frame(&mut map, REPEAT_INTERVAL / 2.0, &[]),
            NavInput::NAVIGATION
        );
        assert_eq!(
            frame(&mut map, REPEAT_INTERVAL, &[]),
            down,
            "an interval later"
        );
    }

    /// **Off the stack, the UI hears nothing** and the game keeps its keys.
    #[test]
    fn with_the_ui_context_popped_nothing_navigates() {
        let mut map = map();
        let nav = frame(
            &mut map,
            FRAME,
            &[(KeyCode::ArrowDown, true), (KeyCode::Space, true)],
        );
        assert_eq!(
            nav,
            NavInput::NAVIGATION,
            "the mode follows the keyboard, the presses do not"
        );
        assert!(map.just_pressed("jump"));
    }

    /// The pointer and a finger show hover; the keyboard and a pad show focus.
    #[test]
    fn the_mode_follows_the_last_device() {
        assert_eq!(input_mode(None), InputMode::Pointer);
        assert_eq!(input_mode(Some(Device::Pointer)), InputMode::Pointer);
        assert_eq!(input_mode(Some(Device::Touch)), InputMode::Pointer);
        assert_eq!(input_mode(Some(Device::Keyboard)), InputMode::Navigation);
        assert_eq!(input_mode(Some(Device::Gamepad)), InputMode::Navigation);

        let mut map = map();
        map.key_event(KeyCode::KeyQ, true);
        map.mouse_motion(4.0, 0.0);
        assert_eq!(
            nav_input(&map).mode,
            InputMode::Pointer,
            "the mouse moved last"
        );
    }
}
