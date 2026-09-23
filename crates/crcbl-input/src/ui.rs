//! The engine's reserved `ui` context: the navigation actions every UI screen
//! is driven by, as `docs/plan/07-ui-debug.md`'s "Navigation = reserved UI
//! actions" table defines them.
//!
//! | Action     | Kind    | Keyboard                      | Repeat         |
//! | ---------- | ------- | ----------------------------- | -------------- |
//! | [`MOVE`]   | `Axis2` | the arrows, and W, A, S and D | [`Repeat::UI`] |
//! | [`NEXT`]   | Button  | Tab                           | [`Repeat::UI`] |
//! | [`PREV`]   | Button  | Shift+Tab                     | [`Repeat::UI`] |
//! | [`ACCEPT`] | Button  | Enter, Space                  | none           |
//! | [`BACK`]   | Button  | Escape                        | none           |
//!
//! **Rebindable like every action** — they are ordinary actions in an ordinary
//! context, so [`ActionMap::rebind`] moves them.
//!
//! **Pushed only while a UI has input.** Declaring the context leaves it off
//! the stack; whoever owns a UI's input pushes [`CONTEXT`] while that UI has
//! it and pops it after, and while it is pushed the arrows, WASD, Tab, Enter,
//! Space and Escape are the UI's and not the game's beneath — the context stack
//! is the disambiguator, not a list of special cases.
//!
//! **The plan's gamepad column is not declared yet.** [`Binding::PadButton`]
//! and [`Binding::PadStick`] can name the dpad, the left stick, the shoulders,
//! South and East now, but the engine loop feeds no pad into the map this
//! context is declared in, so a pad row here would be a binding nothing can
//! press. It arrives with the loop's pad pump.

use super::{ActionDecl, ActionKind, ActionMap, ActionMapError, Binding, Modifier, Repeat};
use crcbl_core::input::KeyCode;

/// The reserved context's name.
pub const CONTEXT: &str = "ui";
/// Spatial focus movement: an `Axis2`, +Y up.
pub const MOVE: &str = "ui_move";
/// Forward in tree order.
pub const NEXT: &str = "ui_next";
/// Backward in tree order.
pub const PREV: &str = "ui_prev";
/// Fire a button, engage a widget, or commit the engaged one.
pub const ACCEPT: &str = "ui_accept";
/// Cancel the engaged widget, or close the screen.
pub const BACK: &str = "ui_back";

/// The reserved actions with their default bindings.
fn declarations() -> [ActionDecl; 5] {
    let button = |name: &str, bindings: Vec<Binding>| ActionDecl {
        name: name.to_owned(),
        kind: ActionKind::Button,
        bindings,
    };
    [
        ActionDecl {
            name: MOVE.to_owned(),
            kind: ActionKind::Axis2,
            bindings: vec![
                Binding::Wasd {
                    up: KeyCode::ArrowUp,
                    down: KeyCode::ArrowDown,
                    left: KeyCode::ArrowLeft,
                    right: KeyCode::ArrowRight,
                },
                Binding::Wasd {
                    up: KeyCode::KeyW,
                    down: KeyCode::KeyS,
                    left: KeyCode::KeyA,
                    right: KeyCode::KeyD,
                },
            ],
        },
        button(NEXT, vec![Binding::Key(KeyCode::Tab)]),
        button(
            PREV,
            vec![Binding::Chord {
                modifier: Modifier::Shift,
                key: KeyCode::Tab,
            }],
        ),
        button(
            ACCEPT,
            vec![Binding::Key(KeyCode::Enter), Binding::Key(KeyCode::Space)],
        ),
        button(BACK, vec![Binding::Key(KeyCode::Escape)]),
    ]
}

/// Declare the reserved `ui` context into `map`, off the stack, with
/// [`Repeat::UI`] on the three navigation actions.
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
    for name in [MOVE, NEXT, PREV] {
        map.set_repeat(name, Some(Repeat::UI))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cardinal, GAMEPLAY_CONTEXT};

    const TICK: f32 = 1.0 / 60.0;

    fn declared() -> ActionMap {
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
        declare(&mut map).expect("nothing reserved is taken");
        map
    }

    /// Declared off the stack, in its own context, and inert until pushed:
    /// the game keeps WASD and Space.
    #[test]
    fn the_ui_context_is_declared_off_the_stack() {
        let mut map = declared();
        for name in [MOVE, NEXT, PREV, ACCEPT, BACK] {
            assert_eq!(map.context_of(name), Some(CONTEXT), "{name}");
        }
        assert_eq!(
            map.active_contexts().collect::<Vec<_>>(),
            [GAMEPLAY_CONTEXT]
        );
        map.key_event(KeyCode::KeyW, true);
        map.key_event(KeyCode::Space, true);
        assert_eq!(map.axis2("walk"), (0.0, 1.0));
        assert!(map.button_held("jump"));
        assert_eq!(map.cardinal(MOVE), None);
        assert!(!map.button_held(ACCEPT));
    }

    /// **Pushed, it takes the table's keys from the game**, both movement
    /// sets drive one `ui_move`, and Tab and Shift+Tab are distinct.
    #[test]
    fn pushed_it_drives_the_reserved_actions_from_the_table() {
        let mut map = declared();
        map.push_context(CONTEXT).expect("declared");

        map.begin_tick(TICK);
        map.key_event(KeyCode::ArrowLeft, true);
        assert_eq!(map.cardinal(MOVE), Some(Cardinal::Left));
        map.key_event(KeyCode::ArrowLeft, false);
        map.key_event(KeyCode::KeyS, true);
        assert_eq!(map.cardinal(MOVE), Some(Cardinal::Down));
        assert_eq!(map.axis2("walk"), (0.0, 0.0), "S is the UI's now");
        map.key_event(KeyCode::KeyS, false);

        for key in [KeyCode::Enter, KeyCode::Space] {
            map.begin_tick(TICK);
            map.key_event(key, true);
            assert!(map.just_pressed(ACCEPT), "{key}");
            assert!(!map.button_held("jump"));
            map.key_event(key, false);
        }

        map.begin_tick(TICK);
        map.key_event(KeyCode::Escape, true);
        assert!(map.just_pressed(BACK));

        map.begin_tick(TICK);
        map.key_event(KeyCode::Tab, true);
        assert!(map.repeated(NEXT) && !map.button_held(PREV));
        map.key_event(KeyCode::Tab, false);
        map.begin_tick(TICK);
        map.key_event(KeyCode::ShiftRight, true);
        map.key_event(KeyCode::Tab, true);
        assert!(map.repeated(PREV), "Shift+Tab is ui_prev");
        assert!(!map.button_held(NEXT), "and not ui_next as well");
    }

    /// The reserved actions rebind like any other.
    #[test]
    fn the_reserved_actions_rebind() {
        let mut map = declared();
        map.push_context(CONTEXT).expect("declared");
        map.rebind(ACCEPT, vec![Binding::Key(KeyCode::KeyE)])
            .expect("declared");
        map.key_event(KeyCode::KeyE, true);
        map.key_event(KeyCode::Space, true);
        assert!(map.button_held(ACCEPT));
        assert!(
            map.button_held("jump"),
            "Space is no longer the UI's, so it falls through"
        );
    }

    /// A clash is refused whole, before anything is declared.
    #[test]
    fn a_clash_declares_nothing() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: BACK.to_owned(),
            kind: ActionKind::Button,
            bindings: vec![],
        });
        assert_eq!(
            declare(&mut map),
            Err(ActionMapError::DuplicateName(BACK.to_owned())),
        );
        assert_eq!(map.action_names().collect::<Vec<_>>(), [BACK]);
    }
}
