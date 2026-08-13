//! Lumen's menu: the pause panel, and one row of its own.
//!
//! Smaller than a game's, for `apps/sandbox/src/menu.rs`'s reason: this is a
//! fixture, not a game — there is no run to start, no score to lose and nothing
//! to win, so a `GAME OVER` panel would be a screen it could never show.
//!
//! The row it does have is the camera. A lighting fixture is looked at from two
//! places — wherever the reviewer walked to, and the pose the golden was taken
//! from — and getting back to the second is a thing a reviewer wants to do
//! constantly. `docs/plan/sample/00-samples-overview.md` rule 4 is why there is
//! a panel at all, and it is the same reason there is this: a sample that cannot
//! show the engine's menu is a finding about the menu.

use crcbl::engine::FIRST_GAME_ID;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

/// Which camera the frame is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// The pose `crate::room::fixed_camera` names, held still.
    ///
    /// The default, and deliberately: a run whose first frame is the golden's
    /// frame is one whose screenshot can be compared to the checked-in
    /// reference without anybody having to stand in the right place first.
    #[default]
    Fixed,
    /// [`crate::camera::Flyer`], starting at that same pose.
    Free,
}

impl CameraMode {
    /// Parses `fixed` / `free`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "fixed" | "golden" => Some(Self::Fixed),
            "free" | "fly" | "free-fly" => Some(Self::Free),
            _ => None,
        }
    }

    /// The other one.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Fixed => Self::Free,
            Self::Free => Self::Fixed,
        }
    }

    /// What the panel and the summary call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fixed => "FIXED",
            Self::Free => "FREE",
        }
    }
}

/// The one action this sample's menus have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LumenAction {
    /// Swap the camera, and put the free one back at the golden pose when
    /// switching away from it.
    ToggleCamera,
}

/// The id carrying [`LumenAction::ToggleCamera`]. The first id a game may use,
/// per [`FIRST_GAME_ID`].
pub const CAMERA_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The action a widget id names, or `None` for an id this game's menus do not
/// use.
#[must_use]
pub const fn action_for(id: crcbl::ui::WidgetId) -> Option<LumenAction> {
    match id {
        CAMERA_ID => Some(LumenAction::ToggleCamera),
        _ => None,
    }
}

/// The pause panel, its camera row labelled with the mode in force.
#[must_use]
pub fn pause_menu(camera: CameraMode) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    Menu::new(
        "PAUSED",
        vec![
            MenuItem::new(RESUME_ID, "RESUME", "ESC"),
            MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
            MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
            MenuItem::new(CAMERA_ID, format!("CAMERA: {}", camera.label()), "ENTER"),
        ],
    )
}

/// Lumen's menus, keyed by whether it is paused.
pub type Menus = MenuSet<bool>;

/// The pause menu, not shown.
///
/// `false` — the running fixture — has no entry, which is how the set is told
/// that a running frame draws no menu.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(false, vec![(true, pause_menu(CameraMode::default()))])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lumen's whole menu vocabulary: the loop's three, plus its own row.
    type MenuAction = crcbl::engine::MenuAction<LumenAction>;

    /// The action the highlighted button carries, which is what the loop reads.
    fn activate(menus: &mut Menus) -> Option<MenuAction> {
        menus
            .activate()
            .and_then(|id| MenuAction::from_id(id, action_for))
    }

    /// A running fixture shows no menu, and a paused one shows exactly the
    /// pause menu.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = menus();
        assert!(menus.current().is_none());
        assert_eq!(activate(&mut menus), None, "nothing to fire");

        menus.show(true);
        assert_eq!(menus.current().expect("the pause menu").title, "PAUSED");

        menus.show(false);
        assert!(menus.current().is_none());
    }

    /// Every button carries an action the loop can act on, no two carry the
    /// same one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = menus();
        menus.show(true);
        let menu = menus.current().expect("the pause menu");
        let actions: Vec<MenuAction> = menu
            .items()
            .iter()
            .map(|item| {
                MenuAction::from_id(item.id, action_for)
                    .unwrap_or_else(|| panic!("{} names no action", item.label))
            })
            .collect();
        for (index, action) in actions.iter().enumerate() {
            assert!(
                !actions[..index].contains(action),
                "the menu carries {action:?} twice",
            );
        }
        for item in menu.items() {
            assert!(!item.hint.is_empty(), "{} has no key", item.label);
        }
        assert_eq!(
            actions.last(),
            Some(&MenuAction::Game(LumenAction::ToggleCamera)),
        );
    }

    /// The camera row labels the mode it is set to, so a reviewer can read it
    /// off the panel rather than guessing from the picture.
    #[test]
    fn the_camera_row_labels_the_mode_it_is_set_to() {
        let labels = |mode| {
            pause_menu(mode)
                .items()
                .iter()
                .map(|item| item.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(CameraMode::Fixed).last().unwrap(), "CAMERA: FIXED");
        assert_eq!(labels(CameraMode::Free).last().unwrap(), "CAMERA: FREE");
    }

    /// The two spellings of each mode round-trip, and nothing else parses.
    #[test]
    fn the_camera_modes_parse_by_name() {
        assert_eq!(CameraMode::from_name("fixed"), Some(CameraMode::Fixed));
        assert_eq!(CameraMode::from_name("free"), Some(CameraMode::Free));
        assert_eq!(CameraMode::from_name("sideways"), None);
        assert_eq!(CameraMode::default().toggled(), CameraMode::Free);
        assert_eq!(CameraMode::default().toggled().toggled(), CameraMode::Fixed);
    }
}
