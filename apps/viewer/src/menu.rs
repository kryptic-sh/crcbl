//! The viewer's one panel, and every button on it is the loop's.
//!
//! ```text
//!   ESC pressed ──▶ Menu
//!   otherwise ─────▶ none
//! ```
//!
//! # A viewer has nothing to pause, and the panel is not about pausing
//!
//! [`crcbl::engine::Loop`] calls the state "paused" because for a game that is
//! what stopping the clock means. This sample steps no simulation at all — it is
//! `docs/plan/sample/05-viewer.md`'s sanctioned exception to rule 2, see
//! [`crate`] — so the flag stops nothing here and the panel is the only thing it
//! does.
//!
//! That is worth having on its own terms: fullscreen and the debug panel are
//! bound to `F11` and `F3`, and a panel is the **only** way to reach either with
//! a pointer. A machine with no keyboard could otherwise open a model and then
//! do neither. So the title is `MENU` and the first item is `BACK` rather than
//! the `PAUSED`/`RESUME` every game uses: the keys are the same in every sample
//! and the words are not lies about this one.
//!
//! # And no item of the viewer's own
//!
//! Milestone 2's mesh, material and texture listings are panels this sample does
//! not have yet, and the exposure slider is a renderer control it does not reach
//! for; `docs/backlog.md` carries both. Until one of them lands there is nothing
//! for a viewer button to do, which is why [`crate::app::Viewer`]'s `MenuAction`
//! is [`core::convert::Infallible`] — uninhabited rather than an empty enum
//! waiting to be filled in.

use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuKind {
    /// Looking at the model: no panel at all.
    #[default]
    None,
    /// `ESC` has been pressed.
    Menu,
}

impl MenuKind {
    /// The menu this frame shows.
    ///
    /// `paused` is the loop's flag; see the [module docs](self) for why the
    /// word does not fit here.
    #[must_use]
    pub const fn of(paused: bool) -> Self {
        if paused { Self::Menu } else { Self::None }
    }
}

/// The viewer's menus, keyed by the state each belongs to.
pub type Menus = MenuSet<MenuKind>;

/// The one menu, with nothing shown.
///
/// [`MenuKind::None`] has no entry, which is how an ordinary frame is told to
/// draw nothing.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(
        MenuKind::None,
        vec![(
            MenuKind::Menu,
            Menu::new(
                "MENU",
                vec![
                    MenuItem::new(RESUME_ID, "BACK", "ESC"),
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

    /// `ESC` is the only thing that puts a panel on screen here.
    #[test]
    fn the_panel_is_shown_exactly_while_the_loop_calls_itself_paused() {
        assert_eq!(MenuKind::of(true), MenuKind::Menu);
        assert_eq!(MenuKind::of(false), MenuKind::None);

        let mut menus = menus();
        assert!(!menus.is_showing(), "an ordinary frame draws no panel");
        menus.show(MenuKind::Menu);
        let menu = menus.current().expect("the menu kind has a menu");
        assert_eq!(menu.title, "MENU");
        assert_eq!(
            menu.items().iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![RESUME_ID, FULLSCREEN_ID, DEBUG_OVERLAY_ID],
            "every button on this panel is one the loop owns",
        );
    }

    /// Nothing on this panel is numbered in the game's range, so
    /// [`crcbl::engine::MenuAction::from_id`] never asks the viewer about an id
    /// — which is what makes an uninhabited `MenuAction` sound rather than a
    /// hole.
    #[test]
    fn no_item_claims_an_id_the_viewer_would_have_to_answer_for() {
        let mut menus = menus();
        menus.show(MenuKind::Menu);
        for item in menus.current().expect("the menu").items() {
            assert!(
                item.id < crcbl::engine::FIRST_GAME_ID,
                "{} claims {}, which the viewer would have to name",
                item.label,
                item.id,
            );
            assert_eq!(
                crcbl::engine::MenuAction::from_id(item.id, crate::app::Viewer::menu_action),
                Some(match item.id {
                    RESUME_ID => crcbl::engine::MenuAction::Resume,
                    FULLSCREEN_ID => crcbl::engine::MenuAction::Fullscreen,
                    _ => crcbl::engine::MenuAction::DebugOverlay,
                }),
            );
        }
    }
}
