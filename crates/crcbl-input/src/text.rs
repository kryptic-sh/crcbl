//! The engine's reserved `text` context: the keys a text field takes while it
//! is being typed into, so that they reach neither the UI's navigation nor the
//! game.
//!
//! **One action, bound to every key that types or edits**: the letters, the
//! digits, the punctuation row and the ISO and JIS extra keys, Space, the
//! numpad's digits and operators, Backspace and Delete, Home and End, and Left
//! and Right. Nothing reads [`TYPE`]'s value — the characters arrive as the
//! shell's `TextCommit` with the layout applied, and the edits come from the
//! key events themselves — it exists so the context **owns** those keys, and
//! the context stack's rule that the topmost active context binding an input
//! consumes it does the rest.
//!
//! **Pushed over [`ui::CONTEXT`](crate::ui::CONTEXT) while a text field is
//! engaged**, and popped when it is not. While it is pushed W, A, S, D and the
//! arrows are no `ui_move`, Space is no `ui_accept`, and a game's binding on
//! any of them hears nothing. Enter, Escape and Tab are not bound, so they
//! still fall through to `ui_accept`, `ui_back` and `ui_next` — which is how a
//! field is committed, cancelled or left. The Up and Down arrows are not bound
//! either: a single-line field has no use for them, and an engaged widget
//! already takes navigation steps instead of focus.
//!
//! **A key held when the context is pushed is withheld from it until
//! released**, as for every push: the Space that engaged a field is not then
//! also typed into it by this context, and a W held since before the field
//! engaged goes up for the game and stays silent here.

use super::{ActionDecl, ActionKind, ActionMap, ActionMapError, Binding};
use crcbl_core::input::KeyCode;

/// The reserved context's name.
pub const CONTEXT: &str = "text";
/// The one action: a key a text field takes. See the module docs.
pub const TYPE: &str = "text_type";

/// Every key [`CONTEXT`] takes.
pub const KEYS: &[KeyCode] = &[
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Backquote,
    KeyCode::Minus,
    KeyCode::Equal,
    KeyCode::BracketLeft,
    KeyCode::BracketRight,
    KeyCode::Backslash,
    KeyCode::Semicolon,
    KeyCode::Quote,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::Slash,
    KeyCode::IntlBackslash,
    KeyCode::IntlRo,
    KeyCode::IntlYen,
    KeyCode::Space,
    KeyCode::Backspace,
    KeyCode::Delete,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
    KeyCode::Numpad0,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
    KeyCode::NumpadDivide,
    KeyCode::NumpadMultiply,
    KeyCode::NumpadSubtract,
    KeyCode::NumpadAdd,
    KeyCode::NumpadDecimal,
];

/// Declare the reserved `text` context into `map`, off the stack.
///
/// # Errors
/// [`ActionMapError::DuplicateName`] if `map` already declares [`TYPE`].
pub fn declare(map: &mut ActionMap) -> Result<(), ActionMapError> {
    map.try_declare_in(
        CONTEXT,
        ActionDecl {
            name: TYPE.to_owned(),
            kind: ActionKind::Button,
            bindings: KEYS.iter().copied().map(Binding::Key).collect(),
        },
    )
}

/// Puts [`CONTEXT`] on `map`'s stack while `editing`, and takes it off when
/// not: what a caller runs once a frame with
/// `Ui::text_editing`'s answer. Does nothing when the stack already agrees.
///
/// # Errors
/// [`ActionMapError::UnknownContext`] if [`declare`] never ran on `map`, and
/// [`ActionMapError::ContextNotOnTop`] if `editing` is false while another
/// context was pushed over this one — an owner popping out of order, which the
/// stack refuses rather than hides.
pub fn sync(map: &mut ActionMap, editing: bool) -> Result<(), ActionMapError> {
    match (editing, map.is_context_active(CONTEXT)) {
        (true, false) => map.push_context(CONTEXT),
        (false, true) => map.pop_context(CONTEXT),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui;

    const TICK: f32 = 1.0 / 60.0;

    /// A game that walks on WASD and jumps on Space, with both reserved
    /// contexts declared beside it and `ui` pushed: a menu with a text field.
    fn menu() -> ActionMap {
        let mut map = ActionMap::new();
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
        map.declare(ActionDecl {
            name: "jump".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::Space)],
        });
        ui::declare(&mut map).expect("nothing reserved is taken");
        declare(&mut map).expect("nothing reserved is taken");
        map.push_context(ui::CONTEXT).expect("declared");
        map
    }

    /// Whether any reserved `ui` action or game action reads anything.
    fn anything_but_typing(map: &ActionMap) -> Vec<&'static str> {
        let mut heard = Vec::new();
        if map.cardinal(ui::MOVE).is_some() {
            heard.push(ui::MOVE);
        }
        for name in [ui::ACCEPT, ui::BACK, ui::NEXT, ui::PREV, "jump"] {
            if map.button_held(name) {
                heard.push(name);
            }
        }
        if map.axis2("walk") != (0.0, 0.0) {
            heard.push("walk");
        }
        heard
    }

    /// The keys a text field must take, found without reading [`KEYS`]: every
    /// letter and digit, and every key the `ui` context's `ui_move` and
    /// `ui_accept` bind except Enter and the up and down arrows, which a
    /// single-line field leaves to navigation.
    fn keys_a_field_must_take(map: &ActionMap) -> Vec<KeyCode> {
        let mut keys: Vec<KeyCode> = KeyCode::ALL
            .iter()
            .copied()
            .filter(|key| key.as_str().starts_with("Key") || key.as_str().starts_with("Digit"))
            .collect();
        for action in [ui::MOVE, ui::ACCEPT] {
            for binding in map.bindings(action).expect("declared") {
                binding.visit_keys(|key| {
                    if ![KeyCode::Enter, KeyCode::ArrowUp, KeyCode::ArrowDown].contains(&key) {
                        keys.push(key);
                    }
                });
            }
        }
        keys
    }

    /// **Every key the context takes reaches neither `ui_*` nor the game**
    /// while it is pushed — each pressed alone, so one key's route cannot hide
    /// another's — and the same keys do reach them once it is popped. The keys
    /// walked are [`KEYS`] and, found independently of it, every letter and
    /// digit and every typing key the `ui` context binds, so a key missing
    /// from the list is a key this walks and finds leaking.
    #[test]
    fn typing_keys_reach_neither_the_ui_nor_the_game() {
        let mut map = menu();
        let must = keys_a_field_must_take(&map);
        assert!(
            must.contains(&KeyCode::KeyW) && must.contains(&KeyCode::Space),
            "the independent list lost the keys the claim is about: {must:?}"
        );
        sync(&mut map, true).expect("declared");
        assert_eq!(
            map.active_contexts().collect::<Vec<_>>(),
            ["gameplay", "ui", "text"]
        );
        for &key in KEYS.iter().chain(&must) {
            map.begin_tick(TICK);
            map.key_event(key, true);
            assert!(map.button_held(TYPE), "{key} is not the text context's");
            assert_eq!(anything_but_typing(&map), [] as [&str; 0], "{key} leaked");
            map.key_event(key, false);
        }

        sync(&mut map, false).expect("on top");
        assert_eq!(
            map.active_contexts().collect::<Vec<_>>(),
            ["gameplay", "ui"]
        );
        let mut reached = Vec::new();
        for key in [KeyCode::KeyW, KeyCode::ArrowLeft, KeyCode::Space] {
            map.begin_tick(TICK);
            map.key_event(key, true);
            reached.extend(anything_but_typing(&map));
            map.key_event(key, false);
        }
        assert_eq!(
            reached,
            [ui::MOVE, ui::MOVE, ui::ACCEPT],
            "popped, the keys did not go back to the ui"
        );
    }

    /// **Enter, Escape and Tab still commit, cancel and leave** while the
    /// context is pushed: it binds none of them.
    #[test]
    fn enter_escape_and_tab_still_reach_the_ui() {
        let mut map = menu();
        sync(&mut map, true).expect("declared");
        for (key, action) in [
            (KeyCode::Enter, ui::ACCEPT),
            (KeyCode::Escape, ui::BACK),
            (KeyCode::Tab, ui::NEXT),
        ] {
            map.begin_tick(TICK);
            map.key_event(key, true);
            assert!(map.button_held(action), "{key} did not reach {action}");
            assert!(!map.button_held(TYPE));
            map.key_event(key, false);
        }
    }

    /// **The Space that engaged a field does not type into it**: held across
    /// the push, it is withheld from the new context until it comes up, and
    /// the next press is the context's.
    #[test]
    fn the_key_that_engaged_the_field_is_withheld_until_released() {
        let mut map = menu();
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(map.just_pressed(ui::ACCEPT));
        sync(&mut map, true).expect("declared");
        map.begin_tick(TICK);
        assert!(
            !map.button_held(TYPE),
            "the engaging Space reached the field"
        );
        assert!(!map.button_held(ui::ACCEPT) && !map.button_held("jump"));
        map.key_event(KeyCode::Space, false);
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(map.just_pressed(TYPE), "a fresh Space is not the field's");
    }

    /// Sync is idempotent, refuses to pop out of order, and needs the
    /// declaration.
    #[test]
    fn sync_agrees_with_the_stack_and_refuses_out_of_order() {
        let mut map = menu();
        sync(&mut map, false).expect("already off");
        sync(&mut map, true).expect("declared");
        sync(&mut map, true).expect("already on");
        map.declare_in(
            "modal",
            ActionDecl {
                name: "close".to_owned(),
                kind: ActionKind::Button,
                bindings: vec![],
            },
        );
        map.push_context("modal").expect("declared");
        assert_eq!(
            sync(&mut map, false),
            Err(ActionMapError::ContextNotOnTop(CONTEXT.to_owned()))
        );

        let mut bare = ActionMap::new();
        assert_eq!(
            sync(&mut bare, true),
            Err(ActionMapError::UnknownContext(CONTEXT.to_owned()))
        );
    }
}
