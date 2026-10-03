//! The engine's reserved `list` context: the keys an open pop-up list takes —
//! a drop-down's list or a context menu — so that they reach neither the UI's
//! navigation nor the game.
//!
//! | Action        | Kind   | Keyboard                         | Repeat         |
//! | ------------- | ------ | -------------------------------- | -------------- |
//! | [`FIRST`]     | Button | Home                             | none           |
//! | [`LAST`]      | Button | End                              | none           |
//! | [`PAGE_UP`]   | Button | Page Up                          | [`Repeat::UI`] |
//! | [`PAGE_DOWN`] | Button | Page Down                        | [`Repeat::UI`] |
//! | [`TYPE`]      | Button | every key [`type_keys`] names    | none           |
//!
//! **Pushed over [`ui::CONTEXT`](crate::ui::CONTEXT) while a list is open**, as
//! [`text`]'s context is while a field is engaged, and popped
//! when it is not; the tree's `Ui::popup_list_open` says which. The two are
//! never pushed together: a list takes no keys while something is engaged.
//!
//! **[`TYPE`] owns the keys that type a character** — the letters, the digits,
//! the punctuation row, the ISO and JIS extra keys and the numpad's digits and
//! operators — for the list's typeahead. Nothing reads its value: the
//! characters arrive as the shell's `TextCommit` with the layout applied, as a
//! field's do. Owning them is what keeps W from being `ui_move` and a game's
//! binding on a letter from firing while a list is up.
//!
//! **What navigation needs stays navigation's**: the arrows, Tab, Enter,
//! Space and Escape are not bound here, so they fall through to `ui_move`,
//! `ui_next`, `ui_accept` and `ui_back` — and left and right still close and
//! open a context menu's submenus. Backspace and Delete are not bound either:
//! a list edits nothing.
//!
//! **No pad button is bound.** A pad has no Home, End or page keys every
//! platform agrees on, and nothing to type with.

use super::{ActionDecl, ActionKind, ActionMap, ActionMapError, Binding, Repeat, text};
use crcbl_core::input::KeyCode;

/// The reserved context's name.
pub const CONTEXT: &str = "list";
/// Go to the list's first item.
pub const FIRST: &str = "list_first";
/// Go to the list's last item.
pub const LAST: &str = "list_last";
/// Go a view's height up the list.
pub const PAGE_UP: &str = "list_page_up";
/// Go a view's height down the list.
pub const PAGE_DOWN: &str = "list_page_down";
/// A key that types a character into the list's typeahead. See the module
/// docs.
pub const TYPE: &str = "list_type";

/// Every reserved action, in the table's order.
pub const ACTIONS: [&str; 5] = [FIRST, LAST, PAGE_UP, PAGE_DOWN, TYPE];

/// The keys of [`text::KEYS`] a list leaves alone: what navigation needs, the
/// jump keys this context binds to actions of their own, and the keys that
/// edit text.
const NOT_TYPED: [KeyCode; 7] = [
    KeyCode::Space,
    KeyCode::Backspace,
    KeyCode::Delete,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
];

/// Every key [`TYPE`] takes: each of [`text::KEYS`] that types a character.
pub fn type_keys() -> impl Iterator<Item = KeyCode> {
    text::KEYS
        .iter()
        .copied()
        .filter(|key| !NOT_TYPED.contains(key))
}

/// The reserved actions with their default bindings.
fn declarations() -> [ActionDecl; ACTIONS.len()] {
    let button = |name: &str, bindings: Vec<Binding>| ActionDecl {
        name: name.to_owned(),
        kind: ActionKind::Button,
        bindings,
    };
    [
        button(FIRST, vec![Binding::Key(KeyCode::Home)]),
        button(LAST, vec![Binding::Key(KeyCode::End)]),
        button(PAGE_UP, vec![Binding::Key(KeyCode::PageUp)]),
        button(PAGE_DOWN, vec![Binding::Key(KeyCode::PageDown)]),
        button(TYPE, type_keys().map(Binding::Key).collect()),
    ]
}

/// Declare the reserved `list` context into `map`, off the stack, with
/// [`Repeat::UI`] on the two page actions.
///
/// # Errors
/// [`ActionMapError::DuplicateName`] if `map` already declares any of the
/// reserved names — checked before any is declared, so a refused call leaves
/// the map as it was.
pub fn declare(map: &mut ActionMap) -> Result<(), ActionMapError> {
    let decls = declarations();
    if let Some(taken) = decls.iter().find(|decl| map.bindings(&decl.name).is_some()) {
        return Err(ActionMapError::DuplicateName(taken.name.clone()));
    }
    for decl in decls {
        map.try_declare_in(CONTEXT, decl)?;
    }
    for name in [PAGE_UP, PAGE_DOWN] {
        map.set_repeat(name, Some(Repeat::UI))?;
    }
    Ok(())
}

/// Puts [`CONTEXT`] on `map`'s stack while `open`, and takes it off when not:
/// what a caller runs once a frame with `Ui::popup_list_open`'s answer. Does
/// nothing when the stack already agrees.
///
/// # Errors
/// [`ActionMapError::UnknownContext`] if [`declare`] never ran on `map`, and
/// [`ActionMapError::ContextNotOnTop`] if `open` is false while another
/// context was pushed over this one — an owner popping out of order, which the
/// stack refuses rather than hides.
pub fn sync(map: &mut ActionMap, open: bool) -> Result<(), ActionMapError> {
    match (open, map.is_context_active(CONTEXT)) {
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

    /// A game that walks on WASD, jumps on Space and lifts on Page Up, with
    /// the reserved `ui` and `list` contexts declared beside it and `ui`
    /// pushed: a menu with a drop-down.
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
        map.declare(ActionDecl {
            name: "lift".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::PageUp)],
        });
        ui::declare(&mut map).expect("nothing reserved is taken");
        declare(&mut map).expect("nothing reserved is taken");
        map.push_context(ui::CONTEXT).expect("declared");
        map
    }

    /// The actions outside this context that read anything.
    fn heard_elsewhere(map: &ActionMap) -> Vec<&'static str> {
        let mut heard = Vec::new();
        if map.cardinal(ui::MOVE).is_some() {
            heard.push(ui::MOVE);
        }
        for name in [ui::ACCEPT, ui::BACK, ui::NEXT, ui::PREV, "jump", "lift"] {
            if map.button_held(name) {
                heard.push(name);
            }
        }
        if map.axis2("walk") != (0.0, 0.0) {
            heard.push("walk");
        }
        heard
    }

    /// **Every letter and digit, and Home, End and the page keys, are the
    /// list's while it is pushed** — each pressed alone, reaching its own
    /// action and nothing of `ui` or the game — and they go back once it is
    /// popped. The letters and digits are found from [`KeyCode::ALL`], not
    /// from [`type_keys`], so one missing from it is one this finds leaking.
    #[test]
    fn typing_and_jump_keys_are_the_lists_while_it_is_pushed() {
        let mut map = menu();
        let typed: Vec<KeyCode> = KeyCode::ALL
            .iter()
            .copied()
            .filter(|key| key.as_str().starts_with("Key") || key.as_str().starts_with("Digit"))
            .collect();
        assert!(typed.contains(&KeyCode::KeyW), "the walk found no letters");
        sync(&mut map, true).expect("declared");
        assert_eq!(
            map.active_contexts().collect::<Vec<_>>(),
            ["gameplay", "ui", "list"]
        );
        let jumps = [
            (KeyCode::Home, FIRST),
            (KeyCode::End, LAST),
            (KeyCode::PageUp, PAGE_UP),
            (KeyCode::PageDown, PAGE_DOWN),
        ];
        let keys = typed
            .iter()
            .chain(&type_keys().collect::<Vec<_>>())
            .map(|&key| (key, TYPE))
            .chain(jumps)
            .collect::<Vec<_>>();
        for (key, action) in keys {
            map.begin_tick(TICK);
            map.key_event(key, true);
            assert!(map.just_pressed(action), "{key} is not {action}");
            assert_eq!(heard_elsewhere(&map), [] as [&str; 0], "{key} leaked");
            map.key_event(key, false);
        }

        sync(&mut map, false).expect("on top");
        let mut reached = Vec::new();
        for key in [KeyCode::KeyW, KeyCode::PageUp] {
            map.begin_tick(TICK);
            map.key_event(key, true);
            reached.extend(heard_elsewhere(&map));
            map.key_event(key, false);
        }
        assert_eq!(reached, [ui::MOVE, "lift"], "popped, the keys stayed");
    }

    /// **What navigation needs still reaches `ui`** while the list is
    /// pushed: the arrows, Tab, Enter, Space and Escape.
    #[test]
    fn navigation_keys_still_reach_the_ui() {
        let mut map = menu();
        sync(&mut map, true).expect("declared");
        for key in [
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
        ] {
            map.begin_tick(TICK);
            map.key_event(key, true);
            assert!(map.cardinal(ui::MOVE).is_some(), "{key} is not ui_move");
            map.key_event(key, false);
        }
        for (key, action) in [
            (KeyCode::Tab, ui::NEXT),
            (KeyCode::Enter, ui::ACCEPT),
            (KeyCode::Space, ui::ACCEPT),
            (KeyCode::Escape, ui::BACK),
        ] {
            map.begin_tick(TICK);
            map.key_event(key, true);
            assert!(map.button_held(action), "{key} did not reach {action}");
            assert!(!map.button_held(TYPE));
            map.key_event(key, false);
        }
    }

    /// **A held page key repeats**, as a held arrow walks a list.
    #[test]
    fn a_held_page_key_repeats() {
        let mut map = menu();
        sync(&mut map, true).expect("declared");
        map.begin_tick(TICK);
        map.key_event(KeyCode::PageDown, true);
        assert!(map.repeated(PAGE_DOWN), "the press did not step");
        map.begin_tick(crate::REPEAT_DELAY);
        assert!(map.repeated(PAGE_DOWN), "the hold did not repeat");
    }

    /// Sync is idempotent, refuses to pop out of order, and needs the
    /// declaration; a clash declares nothing.
    #[test]
    fn sync_agrees_with_the_stack_and_a_clash_declares_nothing() {
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
        assert_eq!(
            sync(&mut ActionMap::new(), true),
            Err(ActionMapError::UnknownContext(CONTEXT.to_owned()))
        );

        let mut taken = ActionMap::new();
        taken.declare(ActionDecl {
            name: PAGE_UP.to_owned(),
            kind: ActionKind::Button,
            bindings: vec![],
        });
        assert_eq!(
            declare(&mut taken),
            Err(ActionMapError::DuplicateName(PAGE_UP.to_owned()))
        );
        assert_eq!(taken.action_names().collect::<Vec<_>>(), [PAGE_UP]);
    }
}
