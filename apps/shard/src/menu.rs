//! Shard's one menu: the pause panel.
//!
//! ```text
//!   paused ──▶ Paused
//!   running ─▶ none
//! ```
//!
//! # Every button on it belongs to the loop
//!
//! Resume, fullscreen and the debug panel are the menu equivalents of the
//! engine's three reserved keys and live in [`crcbl::engine::MenuAction`], so
//! this sample declares no action of its own — [`crate::app::Shard`]'s
//! `menu_action` answers `None` for every id it is asked about, and its
//! `MenuAction` type is [`core::convert::Infallible`] because there is genuinely
//! no value it could ever be handed.
//!
//! There is nothing else to put on it yet. Walking, turning the camera,
//! striking and putting the torches out are keys rather than rows, and the save
//! is not a row either — [`crate::save`] writes on a cadence the simulation
//! owns, so there is no button to press and nothing for a player to remember.
//! An inventory and a character sheet are the rows this menu will eventually
//! want, and both belong to a later slice of
//! `docs/plan/sample/15-shard.md`'s milestone 1.

use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuKind {
    /// The zone is being walked: no menu at all.
    #[default]
    None,
    /// The loop has stopped ticking.
    Paused,
}

impl MenuKind {
    /// The menu this frame shows.
    #[must_use]
    pub const fn of(paused: bool) -> Self {
        if paused { Self::Paused } else { Self::None }
    }
}

/// Shard's menus, keyed by the state each belongs to.
pub type Menus = MenuSet<MenuKind>;

/// The one menu, with nothing shown while the demo runs.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(
        MenuKind::None,
        vec![(
            MenuKind::Paused,
            Menu::new(
                "PAUSED",
                vec![
                    MenuItem::new(RESUME_ID, "RESUME", "ESC"),
                    MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
                    MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
                ],
            ),
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::engine::HostedGame as _;

    /// Pause is the only thing that puts a panel on screen here, and it always
    /// does — this sample has no other state a menu could belong to.
    #[test]
    fn the_pause_menu_is_shown_exactly_while_the_loop_is_paused() {
        assert_eq!(MenuKind::of(true), MenuKind::Paused);
        assert_eq!(MenuKind::of(false), MenuKind::None);

        let mut menus = menus();
        assert!(!menus.is_showing(), "a running frame draws no menu");
        menus.show(MenuKind::Paused);
        let menu = menus.current().expect("the paused kind has a menu");
        assert_eq!(menu.title, "PAUSED");
        assert_eq!(
            menu.items().iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![RESUME_ID, FULLSCREEN_ID, DEBUG_OVERLAY_ID],
            "every button on this menu is one the loop owns",
        );
    }

    /// Nothing on this menu is numbered in the game's range, so
    /// [`crcbl::engine::MenuAction::from_id`] never asks shard about an id.
    #[test]
    fn no_item_claims_an_id_the_game_would_have_to_answer_for() {
        let mut menus = menus();
        menus.show(MenuKind::Paused);
        for item in menus.current().expect("the paused menu").items() {
            assert!(
                item.id < crcbl::engine::FIRST_GAME_ID,
                "{} claims {}, which the game would have to name",
                item.label,
                item.id,
            );
            assert_eq!(
                crcbl::engine::MenuAction::from_id(item.id, crate::app::Shard::menu_action),
                Some(match item.id {
                    RESUME_ID => crcbl::engine::MenuAction::Resume,
                    FULLSCREEN_ID => crcbl::engine::MenuAction::Fullscreen,
                    _ => crcbl::engine::MenuAction::DebugOverlay,
                }),
            );
        }
    }
}
