//! Towers' menus: the pause panel, and — natively — the lobby.
//!
//! ```text
//!   paused ─────────────────▶ Paused
//!   the lobby is open ──────▶ Lobby     (native only)
//!   --join / --browse wait ─▶ Joining   (native only)
//!   running ────────────────▶ none
//! ```
//!
//! Pause wins over the lobby, as it wins over everything in every sample: a
//! player who pressed Escape in the lobby is shown the panel Escape opens,
//! and resuming goes back to the lobby. The lobby's rows are built at run
//! time from what the LAN answers — `crate::lobby` has them — so [`menus`]
//! holds only the pause panel, and `crate::app` puts the lobby in the set the
//! first frame it is open.
//!
//! # One row on it is this game's, and the rest belong to the loop
//!
//! Resume, fullscreen and the debug panel are the menu equivalents of the
//! engine's three reserved keys and live in [`crcbl::engine::MenuAction`].
//! `RESTART` is towers' own, so it takes an id from
//! [`FIRST_GAME_ID`] upward and
//! [`crate::app::Towers`] answers for it — which is the shape a game with a
//! menu action of its own has, and the thing `apps/breach`'s uninhabited
//! `MenuAction` could not demonstrate.
//!
//! **It is the same restart the `R` key sends**, and it crosses the wire the
//! same way: the menu sets a flag, the next tick seals it into a command and
//! the server throws the run away. A menu that reached into the stage would be
//! a client mutating server state, which rule 2 has no exemption for.

use crcbl::engine::{FIRST_GAME_ID, PAUSE_TITLE, pause_items};
use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

/// The pause panel's own row.
pub const RESTART_ID: WidgetId = FIRST_GAME_ID;

/// The lobby's solo row.
#[cfg(not(target_arch = "wasm32"))]
pub const SOLO_ID: WidgetId = FIRST_GAME_ID + 1;

/// The lobby's host row.
#[cfg(not(target_arch = "wasm32"))]
pub const HOST_ID: WidgetId = FIRST_GAME_ID + 2;

/// The lobby's connect row, which joins the address typed into it.
#[cfg(not(target_arch = "wasm32"))]
pub const CONNECT_ID: WidgetId = FIRST_GAME_ID + 3;

/// The lobby's first listed host; the rest follow it, one id a row, up to the
/// most hosts a browser lists
/// ([`DEFAULT_MAX_HOSTS`](crcbl::net::udp::discovery::DEFAULT_MAX_HOSTS)).
#[cfg(not(target_arch = "wasm32"))]
pub const FIRST_LISTED_ID: WidgetId = FIRST_GAME_ID + 4;

/// Where `RESTART` sits among [`crcbl::engine::pause_items`]' three rows:
/// directly under `RESUME`.
const RESTART_ROW: usize = 1;

/// What only towers' menus do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Throw the run away and start again.
    Restart,
    /// Start what a lobby row asks for.
    #[cfg(not(target_arch = "wasm32"))]
    Lobby(crate::lobby::Pick),
}

impl MenuAction {
    /// The action a widget id of this game's names, or `None` for one it does
    /// not use.
    #[must_use]
    pub fn from_id(id: WidgetId) -> Option<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::lobby::Pick;
            use crcbl::net::udp::discovery::DEFAULT_MAX_HOSTS;

            let pick = match id {
                SOLO_ID => Some(Pick::Solo),
                HOST_ID => Some(Pick::Host),
                CONNECT_ID => Some(Pick::Connect),
                _ => id
                    .checked_sub(FIRST_LISTED_ID)
                    .and_then(|row| usize::try_from(row).ok())
                    .filter(|&row| row < DEFAULT_MAX_HOSTS)
                    .map(Pick::Listed),
            };
            if let Some(pick) = pick {
                return Some(Self::Lobby(pick));
            }
        }
        (id == RESTART_ID).then_some(Self::Restart)
    }
}

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuKind {
    /// The field is being played: no menu at all.
    #[default]
    None,
    /// The loop has stopped ticking.
    Paused,
    /// The lobby: solo, host, the LAN's hosts and a direct connect.
    #[cfg(not(target_arch = "wasm32"))]
    Lobby,
    /// A join the command line asked for, waiting for the host's map — or
    /// saying why it never came. No rows: there is nothing to pick.
    #[cfg(not(target_arch = "wasm32"))]
    Joining,
}

impl MenuKind {
    /// The menu this frame shows, with no lobby open.
    #[must_use]
    pub const fn of(paused: bool) -> Self {
        if paused { Self::Paused } else { Self::None }
    }

    /// The menu this frame shows with the lobby open: pause still wins.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub const fn in_the_lobby(paused: bool) -> Self {
        if paused { Self::Paused } else { Self::Lobby }
    }

    /// The menu this frame shows while a command-line join waits: pause
    /// still wins.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub const fn joining(paused: bool) -> Self {
        if paused { Self::Paused } else { Self::Joining }
    }
}

/// Towers' menus, keyed by the state each belongs to.
pub type Menus = MenuSet<MenuKind>;

/// The one menu, with nothing shown while the field is being played.
#[must_use]
pub fn menus() -> Menus {
    // Inserted rather than appended: `RESTART` belongs directly under `RESUME`,
    // which is where a player who just lost reaches for it, and the arrow keys
    // walk the rows in this order.
    let mut items = pause_items();
    items.insert(RESTART_ROW, MenuItem::new(RESTART_ID, "RESTART", "R"));
    MenuSet::new(
        MenuKind::None,
        vec![(MenuKind::Paused, Menu::new(PAUSE_TITLE, items))],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, HostedGame as _, RESUME_ID};

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
            vec![RESUME_ID, RESTART_ID, FULLSCREEN_ID, DEBUG_OVERLAY_ID],
            "RESTART sits under RESUME, and the loop's three rows keep their order",
        );
    }

    /// **Every id on the menu resolves to exactly one action**, and the one
    /// this game claims is the only one it answers for.
    ///
    /// The second half is what an id collision would break: a game claiming a
    /// number the loop already owns would shadow `RESUME`, and a paused demo
    /// nobody can resume is a demo nobody can leave.
    #[test]
    fn the_game_answers_for_its_own_id_and_no_others() {
        // Constant on both sides, so the compiler is what refuses an id the
        // loop already owns.
        const { assert!(RESTART_ID >= FIRST_GAME_ID, "RESTART claims a reserved id") };
        assert_eq!(
            crate::app::Towers::menu_action(RESTART_ID),
            Some(MenuAction::Restart),
        );
        for id in [RESUME_ID, FULLSCREEN_ID, DEBUG_OVERLAY_ID] {
            assert_eq!(
                crate::app::Towers::menu_action(id),
                None,
                "the game claimed {id}, which the loop owns",
            );
            assert!(
                crcbl::engine::MenuAction::from_id(id, crate::app::Towers::menu_action).is_some(),
                "{id} resolves to nothing at all",
            );
        }
    }
}
