//! Tide's menu: the pause panel, and the rows of its own.
//!
//! Smaller than a game's, for `apps/sundial/src/menu.rs`' reason: this is a
//! fixture, not a game, so there is no run to start and nothing to win. What it
//! has is the three knobs milestone 1 asks to be legible — which scene, which
//! medium and which camera — each a row `ENTER` moves on, and a reset.
//!
//! `docs/plan/sample/00-samples-overview.md` rule 4 is why there is a panel at
//! all; the debug overlay's `water` section carries the same readings for a run
//! nobody has paused.

use crcbl::engine::FIRST_GAME_ID;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::knobs::Knobs;

/// Which camera the frame is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// The pose [`crate::scene::fixed_camera`] names, held still.
    ///
    /// The default, on sundial's ground: a run whose first frame is the golden's
    /// frame can be compared to the reference without anybody having to stand in
    /// the right place first.
    #[default]
    Fixed,
    /// [`crate::scene::orbit_camera`], turning round the pool on the fixed step.
    Orbit,
    /// [`crcbl::render::Flyer`], starting at the fixed pose.
    Free,
}

impl CameraMode {
    /// Every mode, in the order `C` walks them.
    pub const ALL: [Self; 3] = [Self::Fixed, Self::Orbit, Self::Free];

    /// Parses `fixed` / `orbit` / `free`, and the lower-case of
    /// [`CameraMode::label`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "fixed" | "golden" => Some(Self::Fixed),
            "orbit" => Some(Self::Orbit),
            "free" | "fly" | "free-fly" => Some(Self::Free),
            _ => None,
        }
    }

    /// The next one, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Fixed => Self::Orbit,
            Self::Orbit => Self::Free,
            Self::Free => Self::Fixed,
        }
    }

    /// What the panel, the heartbeat and the page call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fixed => "FIXED",
            Self::Orbit => "ORBIT",
            Self::Free => "FREE",
        }
    }
}

/// The actions this sample's menus have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TideAction {
    /// Move on to the next camera.
    CycleCamera,
    /// Move on to the next of the four scenes.
    CycleScene,
    /// Move on to the next medium preset.
    CycleMedium,
    /// Put every knob back where a run opens.
    Reset,
}

/// The camera's row. The first id a game may use, per [`FIRST_GAME_ID`].
pub const CAMERA_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The scene's row.
pub const SCENE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 1;

/// The medium's row.
pub const MEDIUM_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 2;

/// The row that puts everything back.
pub const RESET_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 3;

/// Every row `ENTER` fires, with the action it carries and the word it prints —
/// one table, on sundial's `PRESSED_ROWS`' argument.
pub(crate) const PRESSED_ROWS: [(crcbl::ui::WidgetId, TideAction, &str); 4] = [
    (CAMERA_ID, TideAction::CycleCamera, "CAMERA"),
    (SCENE_ID, TideAction::CycleScene, "SCENE"),
    (MEDIUM_ID, TideAction::CycleMedium, "MEDIUM"),
    (RESET_ID, TideAction::Reset, "RESET"),
];

/// The action a widget id names, or `None` for an id this sample's menus do not
/// use.
#[must_use]
pub fn action_for(id: crcbl::ui::WidgetId) -> Option<TideAction> {
    PRESSED_ROWS
        .iter()
        .find(|(row, _, _)| *row == id)
        .map(|&(_, action, _)| action)
}

/// The pause panel: the camera, the scene, the medium and a reset.
#[must_use]
pub fn pause_menu(knobs: Knobs) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    let items = vec![
        MenuItem::new(RESUME_ID, "RESUME", "ESC"),
        MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
        MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
        MenuItem::new(
            CAMERA_ID,
            format!("CAMERA: {}", knobs.camera.label()),
            "ENTER",
        ),
        MenuItem::new(
            SCENE_ID,
            format!("SCENE: {}", knobs.scene.label().to_uppercase()),
            "ENTER",
        ),
        MenuItem::new(
            MEDIUM_ID,
            format!("MEDIUM: {}", knobs.medium.label().to_uppercase()),
            "ENTER",
        ),
        MenuItem::new(RESET_ID, "RESET", "ENTER"),
    ];
    Menu::new("PAUSED", items)
}

/// Tide's menus, keyed by whether it is paused.
pub type Menus = MenuSet<bool>;

/// The pause menu, not shown; `crate::app`'s `menu_kind` rebuilds it with the
/// knobs in force before the first pause draws it.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(false, vec![(true, pause_menu(crate::knobs::read()))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medium::Preset;
    use crate::scene::Scene;

    /// Tide's whole menu vocabulary: the loop's three, plus its own rows.
    type MenuAction = crcbl::engine::MenuAction<TideAction>;

    /// A running fixture shows no menu, and a paused one shows the pause menu.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = menus();
        assert!(menus.current().is_none());
        menus.show(true);
        assert_eq!(menus.current().expect("the pause menu").title, "PAUSED");
        menus.show(false);
        assert!(menus.current().is_none());
    }

    /// **Every row carries an action the loop can act on, no two carry the same
    /// one, and each prints the knob it moves.**
    #[test]
    fn every_row_names_an_action_and_prints_its_knob() {
        let knobs = Knobs {
            scene: Scene::Valley,
            medium: Preset::Swamp,
            camera: CameraMode::Orbit,
        };
        let menu = pause_menu(knobs);
        let mut actions: Vec<MenuAction> = Vec::new();
        for item in menu.items() {
            let action = MenuAction::from_id(item.id, action_for)
                .unwrap_or_else(|| panic!("{} names no action", item.label));
            assert!(
                !actions.contains(&action),
                "the menu carries {action:?} twice"
            );
            actions.push(action);
            assert!(!item.hint.is_empty(), "{} has no key", item.label);
        }
        for (id, action, name) in PRESSED_ROWS {
            assert_eq!(action_for(id), Some(action), "the {name} row");
            assert!(
                actions.contains(&MenuAction::Game(action)),
                "no row fires {action:?}"
            );
        }
        let labels: Vec<String> = menu.items().iter().map(|item| item.label.clone()).collect();
        for row in ["CAMERA: ORBIT", "SCENE: VALLEY", "MEDIUM: SWAMP"] {
            assert!(
                labels.iter().any(|label| label == row),
                "no {row} in {labels:?}"
            );
        }
    }

    /// The camera names parse, and the modes are a cycle that comes back round.
    #[test]
    fn the_camera_modes_parse_and_cycle() {
        for mode in CameraMode::ALL {
            assert_eq!(
                CameraMode::from_name(&mode.label().to_lowercase()),
                Some(mode)
            );
        }
        assert_eq!(CameraMode::from_name("sideways"), None);
        let mut mode = CameraMode::default();
        for _ in 0..CameraMode::ALL.len() {
            mode = mode.next();
        }
        assert_eq!(mode, CameraMode::default(), "the cycle must wrap");
    }
}
