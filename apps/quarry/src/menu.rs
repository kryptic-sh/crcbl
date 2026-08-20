//! Quarry's menu: the pause panel, and the two rows of its own.
//!
//! Smaller than a game's, for `apps/lumen/src/menu.rs`'s reason: this is a
//! fixture, not a game — there is no run to start, no score to lose and nothing
//! to win, so a `GAME OVER` panel would be a screen it could never show.
//!
//! The rows it does have are the two things a reviewer of a *geometry* fixture
//! keeps reaching for. The camera, because the face is looked at from two
//! places — wherever the reviewer flew to, and the pose
//! `tests/golden/` was blessed from — and getting back to the second should be
//! a keypress. And the LOD view, because
//! `docs/plan/sample/14-quarry.md`'s "one mesh spanning several levels across
//! its own surface" is a claim nobody can see in a shaded frame: the tint is
//! what makes it a claim anyone can check, and holding it against the shaded
//! picture is the comparison.
//!
//! `docs/plan/sample/00-samples-overview.md` rule 4 is why there is a panel at
//! all, and it is the same reason there is this: a sample that cannot show the
//! engine's menu is a finding about the menu.

use crcbl::engine::FIRST_GAME_ID;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

/// Which camera the frame is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// The dolly's start pose, held still — [`crate::camera::dolly`].
    ///
    /// The default, and deliberately: a run whose first frame is the goldens'
    /// framing is one whose screenshot can be compared to the checked-in
    /// references without anybody having to fly to the right place first.
    #[default]
    Fixed,
    /// [`crcbl::render::Flyer`], starting at that same pose.
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

/// The actions this sample's menus have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuarryAction {
    /// Swap the camera, and put the free one back at the dolly's start pose
    /// when switching away from it.
    ToggleCamera,
    /// Flip [`ForwardRenderer::set_lod_view`](crcbl::render::ForwardRenderer::set_lod_view):
    /// each cluster tinted by the DAG level it came from, instead of shaded.
    ToggleLodView,
}

/// The id carrying [`QuarryAction::ToggleCamera`]. The first id a game may use,
/// per [`FIRST_GAME_ID`].
pub const CAMERA_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The id carrying [`QuarryAction::ToggleLodView`].
pub const LOD_VIEW_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 1;

/// The action a widget id names, or `None` for an id this sample's menus do not
/// use.
#[must_use]
pub const fn action_for(id: crcbl::ui::WidgetId) -> Option<QuarryAction> {
    if id == CAMERA_ID {
        Some(QuarryAction::ToggleCamera)
    } else if id == LOD_VIEW_ID {
        Some(QuarryAction::ToggleLodView)
    } else {
        None
    }
}

/// The pause panel: the loop's own three rows, then the camera in force and
/// whether the LOD tint is on.
#[must_use]
pub fn pause_menu(camera: CameraMode, lod_view: bool) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    Menu::new(
        "PAUSED",
        vec![
            MenuItem::new(RESUME_ID, "RESUME", "ESC"),
            MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
            MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
            MenuItem::new(CAMERA_ID, format!("CAMERA: {}", camera.label()), "ENTER"),
            MenuItem::new(
                LOD_VIEW_ID,
                format!("LOD VIEW: {}", if lod_view { "ON" } else { "OFF" }),
                "ENTER",
            ),
        ],
    )
}

/// Quarry's menus, keyed by whether it is paused.
pub type Menus = MenuSet<bool>;

/// The pause menu, not shown.
///
/// `false` — the running fixture — has no entry, which is how the set is told
/// that a running frame draws no menu.
///
/// Built from the defaults, because the values in force live on the renderer
/// and this is called before there is one. `crate::app`'s `menu_kind` replaces
/// the panel with them before the first pause draws it.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(
        false,
        vec![(true, pause_menu(CameraMode::default(), false))],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quarry's whole menu vocabulary: the loop's three, plus its own rows.
    type MenuAction = crcbl::engine::MenuAction<QuarryAction>;

    /// Every row's label, in the order the panel draws them.
    fn labels(menu: &Menu) -> Vec<String> {
        menu.items().iter().map(|item| item.label.clone()).collect()
    }

    /// A running fixture shows no menu, and a paused one shows exactly the
    /// pause menu.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = menus();
        assert!(menus.current().is_none());

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
        for expected in [
            MenuAction::Game(QuarryAction::ToggleCamera),
            MenuAction::Game(QuarryAction::ToggleLodView),
        ] {
            assert!(actions.contains(&expected), "no row fires {expected:?}");
        }
    }

    /// **Each row labels the value it is set to**, so a reviewer reads the
    /// state off the panel rather than guessing it from the picture.
    ///
    /// One arm per row and both values of each, because a label wired to the
    /// wrong field reads correctly for exactly one of the four combinations.
    #[test]
    fn each_row_labels_the_value_it_is_set_to() {
        for (camera, camera_row) in [
            (CameraMode::Fixed, "CAMERA: FIXED"),
            (CameraMode::Free, "CAMERA: FREE"),
        ] {
            for (lod_view, lod_row) in [(false, "LOD VIEW: OFF"), (true, "LOD VIEW: ON")] {
                let rows = labels(&pause_menu(camera, lod_view));
                assert!(rows.contains(&camera_row.to_string()), "{rows:?}");
                assert!(rows.contains(&lod_row.to_string()), "{rows:?}");
            }
        }
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
