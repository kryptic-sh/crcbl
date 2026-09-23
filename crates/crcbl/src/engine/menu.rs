//! The pause panel, which is the loop's menu rather than a game's.
//!
//! ```text
//!   ┌───────────────────────────┐
//!   │          PAUSED           │
//!   │  RESUME              ESC  │
//!   │  FULLSCREEN          F11  │
//!   │  DEBUG PANEL          F3  │
//!   └───────────────────────────┘
//! ```
//!
//! # Why this is not each sample's own
//!
//! Every row on it is the menu equivalent of one of [`PAUSE_KEY`](super::PAUSE_KEY),
//! [`FULLSCREEN_KEY`](super::FULLSCREEN_KEY) and
//! [`DEBUG_OVERLAY_KEY`](super::DEBUG_OVERLAY_KEY) — keys the loop acts on itself
//! and never hands a game. So the ids, the labels, the shortcuts and the order
//! are all the loop's, and a sample writing them out is writing down a fact
//! about its host. Seven of them did, character for character, and
//! `crates/crcbl-cli/templates/main.rs.tmpl` wrote it an eighth time into every
//! project `crcbl new` scaffolds.
//!
//! What stays with the sample is the state the panel belongs to — its
//! `MenuKind`, and the `paused` it maps from — because that is the only part
//! that differs between a demo with one menu and a demo with four.
//!
//! # Here rather than in `crcbl_ui::menu`
//!
//! [`Menu`] and [`MenuSet`] are the toolkit's: layout, selection and
//! activation, with no notion of what any button means. *Which* buttons a pause
//! panel has is [`MenuAction`](super::MenuAction)'s, and
//! [`RESUME_ID`] and its two neighbours live beside it — the
//! toolkit cannot see them, and giving it a "resume" would be the layer
//! boundary this crate's split exists to hold.

use crcbl_input::{ActionDecl, ActionKind, ActionMap, Binding, text, ui};
use crcbl_ui::menu::{Menu, MenuItem, MenuSet};

use super::{
    DEBUG_OVERLAY_ID, FULLSCREEN_ID, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, MENU_LEFT_KEY,
    MENU_RIGHT_KEY, MENU_UP_KEY, PAUSE_BUTTON, RESUME_ID,
};

/// The panel's heading.
///
/// Read back by every sample's browser row, which finds the panel by this
/// string.
pub const PAUSE_TITLE: &str = "PAUSED";

/// The three rows the loop owns, in the order they are drawn.
///
/// Separate from [`pause_menu`] for the sample that has a row of its own to put
/// among them: `apps/towers` inserts `RESTART` after `RESUME` rather than
/// appending it, so it needs the items before they become a [`Menu`].
#[must_use]
pub fn pause_items() -> Vec<MenuItem> {
    vec![
        MenuItem::new(RESUME_ID, "RESUME", "ESC"),
        MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
        MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
    ]
}

/// The pause panel: [`PAUSE_TITLE`] over [`pause_items`].
#[must_use]
pub fn pause_menu() -> Menu {
    Menu::new(PAUSE_TITLE, pause_items())
}

/// A [`MenuSet`] holding [`pause_menu`] and nothing else.
///
/// `none` is the state that draws no menu at all and `paused` the one that
/// draws the panel — the two halves of a sample's `MenuKind`, or `false` and
/// `true` for a game keyed on the flag directly. `none` gets no entry, which is
/// how the set is told a running frame draws nothing.
#[must_use]
pub fn pause_only<K: Copy + Eq>(none: K, paused: K) -> MenuSet<K> {
    MenuSet::new(none, vec![(paused, pause_menu())])
}

/// The map the loop drives its menus and its console from: the two reserved
/// contexts and nothing else, both off the stack until something has input.
///
/// # The engine's map, not the game's
///
/// A game's map is inside its simulation — breakout, flappy, asteroids and
/// horde keep theirs in `game.rs` and replay [`HostedGame::key_event`] into it
/// at the start of a tick — and several games have none
/// ([`HostedGame::actions`] is optional). A paused frame runs no tick, which
/// is exactly when the pause panel has input, and [`crate::nav::nav_input`]
/// wants a map ticked once per frame. So the loop owns this one and feeds it
/// every key the pump sees; the game's map never hears of the `ui` context.
///
/// # Narrowed to the keys the menus have always taken
///
/// [`ui::declare`]'s keyboard column also binds W, A, S and D, Space, Tab,
/// Shift+Tab and Escape. **Pushed, those would stop reaching the game under
/// every panel**: breakout, flappy, asteroids and horde start a run with Space
/// — their gameplay binding, which their start panels print as the `PLAY` row's
/// hint — horde walks with WASD, and Escape is [`PAUSE_KEY`](super::PAUSE_KEY),
/// which the loop folds before any menu sees a key. Taking them would change
/// what those samples' keys do, so the reserved actions are rebound to what
/// the menus claimed before the context existed: [`MENU_UP_KEY`],
/// [`MENU_DOWN_KEY`], [`MENU_LEFT_KEY`] and [`MENU_RIGHT_KEY`] as
/// [`ui::MOVE`], [`MENU_ACTIVATE_KEY`] as [`ui::ACCEPT`], and nothing for
/// [`ui::NEXT`], [`ui::PREV`] and [`ui::BACK`].
///
/// **Only the keys are narrowed.** The pad column of [`ui::declare`] stays as
/// it is, so the left stick moves, South accepts and East backs out: the loop
/// withholds no pad event from the game (see
/// [`HostedGame::gamepad_event`](super::HostedGame::gamepad_event)), so there
/// is no game binding for the narrowing to protect.
///
/// # The pad's pause
///
/// [`PAUSE_BUTTON`] is bound here too, to an action of
/// the loop's own in the base context: [`PAUSE_KEY`](super::PAUSE_KEY)'s twin,
/// which has to work with no panel up, so it cannot live in `ui`. Nothing in
/// `ui` binds it, so a pushed context leaves it where it is.
///
/// # The `text` context rides on the same map
///
/// [`text::declare`] puts the reserved `text` context here too, off the stack,
/// and [`Loop`](crate::engine::Loop) pushes it over `ui` while the debug
/// console's field is engaged. That is what stops a letter typed at the console
/// from also being [`ui::MOVE`] or [`ui::ACCEPT`] for a panel underneath it: the
/// context stack is the disambiguator, rather than the loop withholding keys
/// from the map by hand.
///
/// [`HostedGame::key_event`]: super::HostedGame::key_event
/// [`HostedGame::actions`]: super::HostedGame::actions
///
/// # Panics
///
/// Never on a fresh map: nothing is declared before the reserved contexts.
#[must_use]
pub fn menu_actions() -> ActionMap {
    let mut actions = ActionMap::new();
    ui::declare(&mut actions).expect("a fresh map has no names to clash with");
    text::declare(&mut actions).expect("a fresh map has no names to clash with");
    let rebinds = [
        (
            ui::MOVE,
            vec![Binding::Wasd {
                up: MENU_UP_KEY,
                down: MENU_DOWN_KEY,
                left: MENU_LEFT_KEY,
                right: MENU_RIGHT_KEY,
            }],
        ),
        (ui::ACCEPT, vec![Binding::Key(MENU_ACTIVATE_KEY)]),
        (ui::NEXT, Vec::new()),
        (ui::PREV, Vec::new()),
        (ui::BACK, Vec::new()),
    ];
    for (name, keys) in rebinds {
        let pads = actions
            .bindings(name)
            .unwrap_or_default()
            .iter()
            .filter(|binding| reads_gamepad(binding))
            .cloned();
        let bindings = keys.into_iter().chain(pads).collect();
        actions
            .rebind(name, bindings)
            .expect("ui::declare declared every reserved action");
    }
    actions.declare(ActionDecl {
        name: PAUSE_ACTION.to_owned(),
        kind: ActionKind::Button,
        bindings: vec![Binding::PadButton(PAUSE_BUTTON)],
    });
    actions
}

/// The loop's own action on [`PAUSE_BUTTON`], in the base context — see
/// [`menu_actions`].
pub(super) const PAUSE_ACTION: &str = "engine_pause";

/// Whether `binding` reads a pad: what [`menu_actions`] keeps when it narrows
/// the keys.
const fn reads_gamepad(binding: &Binding) -> bool {
    matches!(
        binding,
        Binding::PadButton(_) | Binding::PadStick { .. } | Binding::PadTrigger { .. }
    )
}

/// Whether `actions`' `ui` context binds `key` at all: a key a menu may take.
pub(super) fn menu_binds(actions: &ActionMap, key: crcbl_core::input::KeyCode) -> bool {
    [ui::MOVE, ui::NEXT, ui::PREV, ui::ACCEPT, ui::BACK]
        .iter()
        .filter_map(|name| actions.bindings(name))
        .flatten()
        .any(|binding| binding.owns_key(key))
}

/// Whether `key` is one of [`ui::MOVE`]'s horizontal keys: what a menu takes
/// only over a value row.
pub(super) fn moves_sideways(actions: &ActionMap, key: crcbl_core::input::KeyCode) -> bool {
    actions.bindings(ui::MOVE).is_some_and(|bindings| {
        bindings.iter().any(|binding| {
            matches!(binding, Binding::Wasd { left, right, .. } if *left == key || *right == key)
        })
    })
}

/// Lets go of every key `actions`' `ui` context binds, for a window that lost
/// focus: no platform sends those releases, and a key the map still held
/// would be withheld from the next panel, or read as held down for good.
pub(super) fn release_menu_keys(actions: &mut ActionMap) {
    let mut keys = Vec::new();
    for name in [ui::MOVE, ui::NEXT, ui::PREV, ui::ACCEPT, ui::BACK] {
        for binding in actions.bindings(name).unwrap_or_default() {
            binding.visit_keys(|key| keys.push(key));
        }
    }
    for key in keys {
        actions.key_event(key, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FIRST_GAME_ID, MenuAction};

    /// **Every button on the panel is one the loop owns**, in the order it
    /// draws them.
    ///
    /// The order is as load-bearing as the set: a sample's browser row drives
    /// this panel with the arrow keys and counts rows to reach one.
    #[test]
    fn the_panel_is_the_three_buttons_the_loop_owns() {
        let menu = pause_menu();
        assert_eq!(menu.title, PAUSE_TITLE);
        assert_eq!(
            menu.items()
                .iter()
                .map(|item| (item.id, item.label.as_str(), item.hint.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (RESUME_ID, "RESUME", "ESC"),
                (FULLSCREEN_ID, "FULLSCREEN", "F11"),
                (DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
            ],
        );
    }

    /// Nothing on it is numbered in a game's range, so
    /// [`MenuAction::from_id`] never has to ask a game about one — which is why
    /// a sample whose whole menu is this panel can declare its `MenuAction` as
    /// [`Infallible`](core::convert::Infallible).
    #[test]
    fn no_item_claims_an_id_a_game_would_have_to_answer_for() {
        for item in pause_menu().items() {
            assert!(
                item.id < FIRST_GAME_ID,
                "{} claims {}, which the game would have to name",
                item.label,
                item.id,
            );
            assert_eq!(
                MenuAction::from_id(item.id, |_| None::<core::convert::Infallible>),
                Some(match item.id {
                    RESUME_ID => MenuAction::Resume,
                    FULLSCREEN_ID => MenuAction::Fullscreen,
                    _ => MenuAction::DebugOverlay,
                }),
            );
        }
    }

    /// The set draws nothing until it is shown the paused state, which is what
    /// makes a running frame's menu absent rather than empty.
    #[test]
    fn a_pause_only_set_draws_nothing_until_the_paused_state_is_shown() {
        let mut menus = pause_only(false, true);
        assert!(!menus.is_showing(), "a running frame draws no menu");
        menus.show(true);
        assert_eq!(
            menus.current().expect("the paused state has a menu").title,
            PAUSE_TITLE,
        );
    }
}
