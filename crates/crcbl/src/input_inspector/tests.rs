//! The input section, drawn: every row it promises reaches the debug panel's
//! draw list, with the values the map holds.

use super::*;
use crate::input::{
    ActionDecl, ActionKind, GamepadEvent, GamepadId, GamepadSnapshot, PadKind, TracedInput,
};
use crcbl_core::input::{KeyCode, PointerButton};
use crcbl_ui::draw_list::{DrawCommand, DrawList};
use crcbl_ui::{DebugPanel, FontAtlas};

/// `jump` on Space and South in gameplay, `walk` on WASD, `accept` on Space
/// in a `menu` context, and `close` on Escape in a modal `inventory`; tracing.
fn map() -> ActionMap {
    let mut map = ActionMap::new();
    map.declare(ActionDecl {
        name: "jump".to_owned(),
        kind: ActionKind::Button,
        bindings: vec![
            Binding::Key(KeyCode::Space),
            Binding::PadButton(PadButton::South),
        ],
    });
    map.declare(ActionDecl {
        name: "walk".to_owned(),
        kind: ActionKind::Axis2,
        bindings: vec![Binding::Wasd {
            up: KeyCode::KeyW,
            down: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
        }],
    });
    map.declare_in(
        "menu",
        ActionDecl {
            name: "accept".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::Space)],
        },
    );
    map.declare_in(
        "inventory",
        ActionDecl {
            name: "close".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::Escape)],
        },
    );
    map.set_tracing(true);
    map
}

/// Every text the panel drew, in draw order: the title, then each row's
/// label and value.
fn drawn(map: &ActionMap) -> Vec<String> {
    let mut panel = DebugPanel::new();
    panel.set_visible(true);
    panel.add(&InputInspector::new(map));
    let mut dl = DrawList::new();
    panel.render(
        &mut dl,
        glam::Vec2::new(1920.0, 1080.0),
        &FontAtlas::built_in(),
    );
    dl.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The value drawn right after `label`, panicking with the whole panel if
/// the label was never drawn.
fn value<'a>(drawn: &'a [String], label: &str) -> &'a str {
    drawn
        .iter()
        .position(|text| text == label)
        .and_then(|at| drawn.get(at + 1))
        .unwrap_or_else(|| panic!("no {label:?} row: {drawn:?}"))
}

/// **Every promised row reaches the draw list**: the pads, the raw keys and
/// mouse, the context stack top first, each action's value, the last device
/// and the trace newest first.
#[test]
fn the_panel_draws_every_input_row() {
    let mut map = map();
    map.gamepad_event(&GamepadEvent::Connected {
        id: GamepadId(5),
        kind: PadKind::PlayStation,
    });
    map.key_event(KeyCode::KeyQ, true);
    map.key_event(KeyCode::KeyW, true);
    map.mouse_button(PointerButton::Right, true);
    map.pointer_position(0.5, -0.25);
    map.push_context("menu").expect("declared");
    map.key_event(KeyCode::Space, true);
    map.trace_claimed(TracedInput::Key(KeyCode::Escape), "the loop: pause");
    map.set_enabled("jump", false);

    let drawn = drawn(&map);
    assert_eq!(drawn.first().map(String::as_str), Some(INPUT_SECTION));
    assert_eq!(value(&drawn, LAST_DEVICE_ROW), "Keyboard");
    assert_eq!(
        value(&drawn, "pad 5"),
        "PlayStation: none L (0.00, 0.00) R (0.00, 0.00) LT 0.00 RT 0.00"
    );
    assert_eq!(value(&drawn, "keys"), "KeyQ KeyW Space", "in press order");
    assert_eq!(value(&drawn, "mouse"), "Mouse:Right at (0.50, -0.25)");
    assert_eq!(value(&drawn, CONTEXTS_ROW), "global > menu > gameplay");
    assert_eq!(value(&drawn, "jump"), "disabled");
    assert_eq!(value(&drawn, "walk"), "(0.00, 1.00)");
    assert_eq!(value(&drawn, "accept"), "pressed");
    assert_eq!(value(&drawn, "close"), "idle: inventory is off the stack");

    // The trace, newest first: the claimed Escape, then Space eaten by the
    // menu, then W walking and Q reaching nothing.
    let trace: Vec<(&str, &str)> = ["Escape", "Space", "Mouse:Right", "KeyW", "KeyQ"]
        .into_iter()
        .map(|input| (input, value(&drawn, input)))
        .collect();
    assert_eq!(
        trace,
        [
            ("Escape", "claimed by the loop: pause"),
            ("Space", "menu: accept (Space)"),
            ("Mouse:Right", "unbound"),
            ("KeyW", "gameplay: walk (Wasd:KeyW,KeyS,KeyA,KeyD)"),
            ("KeyQ", "unbound"),
        ]
    );
    let at = |label: &str| drawn.iter().position(|text| text == label);
    assert!(
        at("Escape") < at("Space") && at("Space") < at("KeyQ"),
        "newest first: {drawn:?}"
    );
}

/// **A modal context is marked, a pad that spoke names its family, and a
/// held pad shows its raw values** — and with tracing off the trace row says
/// so instead of drawing nothing.
#[test]
fn the_panel_marks_modal_contexts_pads_and_an_idle_trace() {
    let mut map = map();
    map.push_context_modal("inventory").expect("declared");
    let mut snapshot = GamepadSnapshot::neutral(PadKind::Xbox);
    snapshot.buttons.insert(PadButton::South);
    snapshot.axes[crate::input::PadAxis::LeftX as usize] = 0.25;
    snapshot.axes[crate::input::PadAxis::RightTrigger as usize] = 1.0;
    map.gamepad_event(&GamepadEvent::State {
        id: GamepadId(2),
        snapshot,
    });
    map.set_tracing(false);

    let drawn = drawn(&map);
    assert_eq!(
        value(&drawn, CONTEXTS_ROW),
        "global > inventory (modal) > gameplay"
    );
    assert_eq!(value(&drawn, LAST_DEVICE_ROW), "Gamepad (Xbox)");
    assert_eq!(
        value(&drawn, "pad 2"),
        "Xbox: Pad:South L (0.25, 0.00) R (0.00, 0.00) LT 0.00 RT 1.00"
    );
    assert_eq!(value(&drawn, TRACE_ROW), "off");

    let mut quiet = ActionMap::new();
    quiet.set_tracing(true);
    let drawn = self::drawn(&quiet);
    assert_eq!(value(&drawn, TRACE_ROW), "nothing pressed yet");
    assert_eq!(value(&drawn, "pads"), "none");
    assert_eq!(value(&drawn, LAST_DEVICE_ROW), "none yet");
    assert_eq!(value(&drawn, "keys"), "none");
}
