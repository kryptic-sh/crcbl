use std::borrow::Cow;

use crcbl_core::input::{KeyCode, PointerButton};

use super::*;
use crate::{
    ActionDecl, ActionKind, GamepadEvent, GamepadId, GamepadSnapshot, Modifier, PadButton,
    PointerAxis, Stick, Trigger,
};

const PAD: GamepadId = GamepadId(3);

/// A map with `jump` on Space, the left mouse button and South, in that order,
/// and `walk` on the keyboard and the pad only.
fn jump_and_walk() -> ActionMap {
    let mut map = ActionMap::new();
    map.declare(ActionDecl {
        name: "jump".to_owned(),
        kind: ActionKind::Button,
        bindings: vec![
            Binding::Key(KeyCode::Space),
            Binding::MouseButton(PointerButton::Left),
            Binding::PadButton(PadButton::South),
        ],
    });
    map.declare(ActionDecl {
        name: "walk".to_owned(),
        kind: ActionKind::Button,
        bindings: vec![
            Binding::Key(KeyCode::KeyW),
            Binding::PadButton(PadButton::North),
        ],
    });
    map
}

/// `PAD`, a pad of `kind`, pressing `button`.
fn press(map: &mut ActionMap, kind: PadKind, button: PadButton) {
    let mut snapshot = GamepadSnapshot::neutral(kind);
    snapshot.buttons.insert(button);
    map.gamepad_event(&GamepadEvent::State { id: PAD, snapshot });
    map.gamepad_event(&GamepadEvent::State {
        id: PAD,
        snapshot: GamepadSnapshot::neutral(kind),
    });
}

fn label(map: &ActionMap, action: &str) -> Option<String> {
    map.hint(action).map(|hint| hint.label)
}

/// **The hint is the binding on the device the player last used**, and it
/// switches as they do.
#[test]
fn the_hint_shows_the_binding_for_the_last_device() {
    let mut map = jump_and_walk();
    map.key_event(KeyCode::KeyQ, true);
    assert_eq!(
        map.hint("jump"),
        Some(Hint {
            device: Device::Keyboard,
            binding: Binding::Key(KeyCode::Space),
            pad: None,
            label: "Space".to_owned(),
        })
    );

    press(&mut map, PadKind::Xbox, PadButton::West);
    assert_eq!(
        map.hint("jump"),
        Some(Hint {
            device: Device::Gamepad,
            binding: Binding::PadButton(PadButton::South),
            pad: Some(PadKind::Xbox),
            label: "A".to_owned(),
        })
    );

    map.mouse_button(PointerButton::Right, true);
    assert_eq!(label(&map, "jump").as_deref(), Some("LMB"));
    map.key_event(KeyCode::KeyQ, true);
    assert_eq!(label(&map, "jump").as_deref(), Some("Space"));
}

/// **An action with nothing on the last device falls back through the
/// devices used before it, most recent first**, and to its first binding when
/// nothing it heard from is bound.
#[test]
fn an_unbound_device_falls_back_to_the_one_used_before_it() {
    let mut map = jump_and_walk();
    assert_eq!(
        label(&map, "walk").as_deref(),
        Some("W"),
        "before any device spoke, the first binding"
    );

    map.key_event(KeyCode::KeyQ, true);
    press(&mut map, PadKind::Xbox, PadButton::West);
    map.mouse_motion(4.0, 0.0);
    assert_eq!(map.last_device(), Some(Device::Pointer));
    assert_eq!(
        label(&map, "walk").as_deref(),
        Some("Y"),
        "the mouse binds nothing, and the pad spoke after the keyboard",
    );

    map.key_event(KeyCode::KeyQ, true);
    map.mouse_motion(4.0, 0.0);
    assert_eq!(
        label(&map, "walk").as_deref(),
        Some("W"),
        "the keyboard spoke after the pad this time",
    );

    let mut touched = jump_and_walk();
    touched.virtual_button("btn", true);
    assert_eq!(
        label(&touched, "walk").as_deref(),
        Some("W"),
        "nothing it heard from is bound: the first binding",
    );

    assert_eq!(map.hint("fly"), None, "no such action");
    map.declare(ActionDecl {
        name: "idle".to_owned(),
        kind: ActionKind::Button,
        bindings: Vec::new(),
    });
    assert_eq!(map.hint("idle"), None, "nothing to show");
}

/// **A pad button is named for the pad that last spoke**; before any did, for
/// the first connected pad; with none, positionally.
#[test]
fn a_pad_hint_is_named_for_the_pad_that_last_spoke() {
    let mut map = jump_and_walk();
    map.declare(ActionDecl {
        name: "pause".to_owned(),
        kind: ActionKind::Button,
        bindings: vec![Binding::PadButton(PadButton::South)],
    });
    map.key_event(KeyCode::KeyQ, true);
    assert_eq!(
        label(&map, "pause").as_deref(),
        Some("South"),
        "no pad at all"
    );

    map.gamepad_event(&GamepadEvent::Connected {
        id: GamepadId(9),
        kind: PadKind::Switch,
    });
    assert_eq!(
        label(&map, "pause").as_deref(),
        Some("B"),
        "a connected Switch pad"
    );
    assert_eq!(map.last_pad_kind(), None, "plugging in is not speaking");

    press(&mut map, PadKind::PlayStation, PadButton::West);
    assert_eq!(map.last_pad_kind(), Some(PadKind::PlayStation));
    assert_eq!(label(&map, "jump").as_deref(), Some("Cross"));
    assert_eq!(
        map.hint("jump").and_then(|hint| hint.pad),
        Some(PadKind::PlayStation)
    );

    map.key_event(KeyCode::KeyQ, true);
    assert_eq!(
        map.last_pad_kind(),
        Some(PadKind::PlayStation),
        "the keyboard speaking leaves the pad in the player's other hand",
    );
}

/// **Every positional button prints what its family prints**, as the table on
/// [`HintLabels::pad_button`] documents.
#[test]
fn pad_buttons_print_their_family_names() {
    use PadButton::*;
    #[rustfmt::skip]
    let table: [(PadButton, [&str; 5]); 15] = [
        //                Xbox    PlayStation  Switch  Steam Deck  Generic
        (South,          ["A",    "Cross",     "B",    "A",        "South"]),
        (East,           ["B",    "Circle",    "A",    "B",        "East"]),
        (West,           ["X",    "Square",    "Y",    "X",        "West"]),
        (North,          ["Y",    "Triangle",  "X",    "Y",        "North"]),
        (LeftShoulder,   ["LB",   "L1",        "L",    "L1",       "Left bumper"]),
        (RightShoulder,  ["RB",   "R1",        "R",    "R1",       "Right bumper"]),
        (LeftStick,      ["LS",   "L3",        "LS",   "L3",       "Left stick button"]),
        (RightStick,     ["RS",   "R3",        "RS",   "R3",       "Right stick button"]),
        (Start,          ["Menu", "Options",   "+",    "Menu",     "Start"]),
        (Select,         ["View", "Share",     "-",    "View",     "Select"]),
        (DpadUp,         ["D-pad up"; 5]),
        (DpadDown,       ["D-pad down"; 5]),
        (DpadLeft,       ["D-pad left"; 5]),
        (DpadRight,      ["D-pad right"; 5]),
        (Guide,          ["Xbox", "PS",        "Home", "Steam",    "Guide"]),
    ];
    let kinds = [
        PadKind::Xbox,
        PadKind::PlayStation,
        PadKind::Switch,
        PadKind::SteamDeck,
        PadKind::Generic,
    ];
    assert_eq!(
        table.map(|(button, _)| button),
        PadButton::ALL,
        "every button"
    );
    for (button, names) in table {
        for (kind, name) in kinds.into_iter().zip(names) {
            assert_eq!(
                DefaultLabels.pad_button(kind, button),
                name,
                "{kind:?} {button:?}"
            );
        }
    }

    #[rustfmt::skip]
    let triggers = [
        (PadKind::Xbox,        ["LT", "RT"]),
        (PadKind::PlayStation, ["L2", "R2"]),
        (PadKind::Switch,      ["ZL", "ZR"]),
        (PadKind::SteamDeck,   ["L2", "R2"]),
        (PadKind::Generic,     ["Left trigger", "Right trigger"]),
    ];
    for (kind, [left, right]) in triggers {
        assert_eq!(DefaultLabels.trigger(kind, Trigger::Left), left, "{kind:?}");
        assert_eq!(
            DefaultLabels.trigger(kind, Trigger::Right),
            right,
            "{kind:?}"
        );
    }
}

/// **Keys print their legend, mouse buttons their short names, and a
/// composite its parts** — joined as [`HintLabels::label`] documents.
#[test]
fn keys_mice_and_composites_print_their_labels() {
    let wasd = |up, down, left, right| Binding::Wasd {
        up,
        down,
        left,
        right,
    };
    let cases = [
        (Binding::Key(KeyCode::Space), "Space"),
        (Binding::Key(KeyCode::KeyW), "W"),
        (Binding::Key(KeyCode::Digit1), "1"),
        (Binding::Key(KeyCode::ArrowUp), "Up"),
        (Binding::Key(KeyCode::ShiftRight), "Shift"),
        (Binding::Key(KeyCode::ControlLeft), "Ctrl"),
        (Binding::Key(KeyCode::Escape), "Esc"),
        (Binding::Key(KeyCode::Slash), "/"),
        (Binding::Key(KeyCode::Numpad5), "Num 5"),
        (Binding::Key(KeyCode::NumpadAdd), "Num +"),
        (Binding::Key(KeyCode::F11), "F11"),
        (Binding::Key(KeyCode::PageUp), "PageUp"),
        (Binding::MouseButton(PointerButton::Left), "LMB"),
        (Binding::MouseButton(PointerButton::Right), "RMB"),
        (Binding::MouseButton(PointerButton::Middle), "MMB"),
        (Binding::MouseButton(PointerButton::Back), "Mouse 4"),
        (Binding::MouseButton(PointerButton::Forward), "Mouse 5"),
        (Binding::MouseButton(PointerButton::Other(9)), "Mouse 9"),
        (Binding::MouseMotion, "Mouse"),
        (Binding::MouseScroll, "Wheel"),
        (
            Binding::PointerPosition {
                axis: PointerAxis::X,
            },
            "Pointer",
        ),
        (Binding::Virtual("stick_move".to_owned()), "stick_move"),
        (
            Binding::Chord {
                modifier: Modifier::Shift,
                key: KeyCode::Tab,
            },
            "Shift+Tab",
        ),
        (
            Binding::ButtonChord {
                modifier: Modifier::Alt,
                button: PointerButton::Right,
            },
            "Alt+RMB",
        ),
        (
            Binding::ScrollChord {
                held: KeyCode::ControlLeft,
            },
            "Ctrl+Wheel",
        ),
        (
            Binding::KeyAxis {
                negative: KeyCode::KeyS,
                positive: KeyCode::KeyW,
            },
            "S/W",
        ),
        (
            wasd(KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD),
            "WASD",
        ),
        (
            wasd(
                KeyCode::ArrowUp,
                KeyCode::ArrowDown,
                KeyCode::ArrowLeft,
                KeyCode::ArrowRight,
            ),
            "Up/Left/Down/Right",
        ),
        (
            Binding::PadChord {
                modifier: PadButton::LeftShoulder,
                button: PadButton::South,
            },
            "LB+A",
        ),
        (Binding::PadDpad, "D-pad"),
        (
            Binding::PadStick {
                stick: Stick::Right,
                deadzone: 0.2,
            },
            "Right stick",
        ),
        (
            Binding::PadTrigger {
                trigger: Trigger::Left,
                threshold: 0.5,
            },
            "LT",
        ),
    ];
    for (binding, expected) in cases {
        assert_eq!(
            DefaultLabels.label(&binding, PadKind::Xbox),
            expected,
            "{binding}"
        );
    }
}

/// A game's table: circled letters for an Xbox pad's face buttons, and its own
/// word for Space.
struct GameLabels;

impl HintLabels for GameLabels {
    fn key(&self, key: KeyCode) -> Cow<'static, str> {
        match key {
            KeyCode::Space => "Spacebar".into(),
            other => DefaultLabels.key(other),
        }
    }

    fn pad_button(&self, kind: PadKind, button: PadButton) -> Cow<'static, str> {
        match (kind, button) {
            (PadKind::Xbox, PadButton::South) => "Ⓐ".into(),
            _ => DefaultLabels.pad_button(kind, button),
        }
    }
}

/// **A game's own table overrides the engine's, entry by entry**, composites
/// included, and leaves every entry it does not override alone.
#[test]
fn a_game_supplied_label_table_overrides_the_engines() {
    let mut map = jump_and_walk();
    map.key_event(KeyCode::KeyQ, true);
    assert_eq!(
        map.hint_with("jump", &GameLabels)
            .map(|hint| hint.label)
            .as_deref(),
        Some("Spacebar")
    );
    assert_eq!(
        map.hint_with("walk", &GameLabels)
            .map(|hint| hint.label)
            .as_deref(),
        Some("W"),
        "a key the game left alone"
    );

    press(&mut map, PadKind::Xbox, PadButton::West);
    let dynamic: &dyn HintLabels = &GameLabels;
    assert_eq!(
        map.hint_with("jump", dynamic)
            .map(|hint| hint.label)
            .as_deref(),
        Some("Ⓐ")
    );
    assert_eq!(
        GameLabels.label(
            &Binding::PadChord {
                modifier: PadButton::RightShoulder,
                button: PadButton::South,
            },
            PadKind::Xbox,
        ),
        "RB+Ⓐ",
    );
    assert_eq!(
        label(&map, "jump").as_deref(),
        Some("A"),
        "the engine's own"
    );
}
