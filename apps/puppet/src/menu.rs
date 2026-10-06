//! Puppet's menus: the pause panel, and the controls overlay opened from it.
//!
//! ```text
//!   running ─▶ none
//!   paused ──▶ Paused ── CONTROLS ──▶ Controls ── a row ──▶ listening
//!                 ▲                      │  ▲                  │ a clash
//!                 └──────── BACK ────────┘  └──── Conflict ◀───┘
//! ```
//!
//! # The pause panel is the loop's, plus one row
//!
//! Resume, fullscreen and the debug panel are the menu equivalents of the
//! engine's three reserved keys and live in [`crcbl::engine::MenuAction`].
//! Puppet adds `CONTROLS` below them, the one row that is this sample's: it
//! opens the overlay where a player rebinds run and jump.
//!
//! # The overlay is the engine's rebind flow
//!
//! The overlay and its clash panel are [`crcbl::rebind`]'s, built from
//! [`crate::bindings::ROWS`] at [`crate::bindings::IDS`] — the same flow, the
//! same rows and the same clash rule as `apps/options`' `CONTROLS` page, so
//! the two cannot drift. [`PuppetAction`] is what their rows fire.

use crcbl::engine::pause_items;
use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::bindings::{CONTROLS_ID, IDS, ROWS};

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuKind {
    /// The character is being walked: no menu at all.
    #[default]
    None,
    /// The loop has stopped ticking.
    Paused,
    /// The controls overlay, over the paused frame.
    Controls,
    /// The clash panel, over the overlay, while the player chooses.
    Conflict,
}

/// What a row of puppet's own fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PuppetAction {
    /// Open the controls overlay.
    Controls,
    /// Go back from the overlay to the pause panel.
    Back,
    /// Listen for the input to bind to the given entry of
    /// [`crate::bindings::ROWS`].
    Rebind(usize),
    /// Put every action back on its default bindings.
    ResetControls,
    /// Take the clashing input from the other action.
    Swap,
    /// Leave both clashing actions as they were.
    Cancel,
}

impl PuppetAction {
    /// The action a widget id names, or `None` for an id puppet's menus do
    /// not use.
    #[must_use]
    pub fn of(id: WidgetId) -> Option<Self> {
        match id {
            CONTROLS_ID => Some(Self::Controls),
            id if id == IDS.back() => Some(Self::Back),
            id if id == IDS.reset() => Some(Self::ResetControls),
            id if id == IDS.swap() => Some(Self::Swap),
            id if id == IDS.cancel() => Some(Self::Cancel),
            id => IDS.action_of(id, ROWS.len()).map(Self::Rebind),
        }
    }
}

/// Puppet's menus, keyed by the state each belongs to.
pub type Menus = MenuSet<MenuKind>;

/// The pause panel with `CONTROLS` under the loop's rows, the overlay and its
/// clash panel, with nothing shown while the demo runs.
#[must_use]
pub fn menus() -> Menus {
    let mut pause = pause_items();
    pause.push(MenuItem::new(CONTROLS_ID, "CONTROLS", ""));
    let (controls, conflict) = crcbl::rebind::menus(IDS, &ROWS);
    MenuSet::new(
        MenuKind::None,
        vec![
            (
                MenuKind::Paused,
                Menu::new(crcbl::engine::PAUSE_TITLE, pause),
            ),
            (MenuKind::Controls, controls),
            (MenuKind::Conflict, conflict),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A running frame draws nothing, and each paused state has its panel.
    ///
    /// **The loop's rows on the pause panel are not asserted here.** They are
    /// [`crcbl::engine::pause_items`]', and the claims about them are beside
    /// it in [`crcbl::engine::menu`].
    #[test]
    fn each_paused_state_has_its_panel_and_a_running_frame_none() {
        let mut menus = menus();
        assert!(!menus.is_showing(), "a running frame draws no menu");
        for kind in [MenuKind::Paused, MenuKind::Controls, MenuKind::Conflict] {
            menus.show(kind);
            assert!(menus.is_showing(), "{kind:?} has no panel");
        }
        let pause = menus.get_mut(MenuKind::Paused).expect("the pause panel");
        assert!(
            pause.items().iter().any(|item| item.id == CONTROLS_ID),
            "the pause panel has no way to the controls overlay",
        );
    }

    /// Every row puppet adds fires its own action, and an id past the last row
    /// fires none.
    #[test]
    fn every_row_fires_its_own_action() {
        assert_eq!(PuppetAction::of(CONTROLS_ID), Some(PuppetAction::Controls));
        assert_eq!(PuppetAction::of(IDS.back()), Some(PuppetAction::Back));
        assert_eq!(
            PuppetAction::of(IDS.reset()),
            Some(PuppetAction::ResetControls)
        );
        assert_eq!(PuppetAction::of(IDS.swap()), Some(PuppetAction::Swap));
        assert_eq!(PuppetAction::of(IDS.cancel()), Some(PuppetAction::Cancel));
        for index in 0..ROWS.len() {
            assert_eq!(
                PuppetAction::of(IDS.action(index)),
                Some(PuppetAction::Rebind(index))
            );
        }
        assert_eq!(PuppetAction::of(IDS.action(ROWS.len())), None);
    }
}
