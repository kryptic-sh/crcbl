//! Quarry's menu: the pause panel, and the rows of its own.
//!
//! Smaller than a game's, for `apps/lumen/src/menu.rs`'s reason: this is a
//! fixture, not a game — there is no run to start, no score to lose and nothing
//! to win, so a `GAME OVER` panel would be a screen it could never show.
//!
//! The rows it does have are the things a reviewer of a *geometry* fixture keeps
//! reaching for. The camera, because the face is looked at from three places —
//! wherever the reviewer flew to, the pose `tests/golden/` was blessed from, and
//! the slow run down the face that shows detail arriving — and getting back to
//! any of them should be a keypress. The two overlays, because
//! `docs/plan/sample/14-quarry.md`'s "one mesh spanning several levels across
//! its own surface" is a claim nobody can see in a shaded frame: the tint is
//! what makes it a claim anyone can check, and holding it against the shaded
//! picture is the comparison. And the freeze, because a cut looked at from the
//! eye that chose it is a cut nobody can find a fault in — that is what a
//! screen-space error budget promises.
//!
//! `docs/plan/sample/00-samples-overview.md` rule 4 is why there is a panel at
//! all, and it is the same reason there is this: a sample that cannot show the
//! engine's menu is a finding about the menu.

use crcbl::engine::FIRST_GAME_ID;
use crcbl::render::DebugView;
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
    /// The same dolly, run down the face and back — [`crate::app::dolly_at`].
    ///
    /// `docs/plan/sample/14-quarry.md`'s Proves section asks that "a slow dolly
    /// past the switch distance shows no boundary popping, on every path".
    /// `tests/device/dolly.rs` asserts that headlessly, frame by frame on one
    /// renderer; this is the same run made watchable, and it is what the
    /// browser page opens on.
    Dolly,
    /// [`crcbl::render::Flyer`], starting at that same pose.
    Free,
}

impl CameraMode {
    /// Parses `fixed` / `dolly` / `free`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "fixed" | "golden" => Some(Self::Fixed),
            "dolly" | "animated" => Some(Self::Dolly),
            "free" | "fly" | "free-fly" => Some(Self::Free),
            _ => None,
        }
    }

    /// The next one round: the held pose, the moving one, then the reviewer's.
    ///
    /// A cycle rather than a swap, and in that order because it is the order a
    /// reviewer wants them: the frame the goldens were blessed from, the same
    /// frame moving on its own, and only then one they have to fly themselves.
    /// It also makes each step continuous — [`Self::Dolly`] starts at the pose
    /// [`Self::Fixed`] holds, and [`crate::camera::flyer`] starts there too.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Fixed => Self::Dolly,
            Self::Dolly => Self::Free,
            Self::Free => Self::Fixed,
        }
    }

    /// What the panel and the summary call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fixed => "FIXED",
            Self::Dolly => "DOLLY",
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
    /// Flip [`ForwardRenderer::set_heatmap`](crcbl::render::ForwardRenderer::set_heatmap):
    /// each cluster shaded by the projected error the selection judged it on.
    ///
    /// **Switching one overlay on switches the other off** — see
    /// [`toggled_to`], which is where the fixture's exclusivity lives. Two rows
    /// that could both read `ON` while one picture is drawn would be a panel
    /// that lies about the frame under it.
    ToggleHeatmap,
    /// Pin the LOD selection at the camera's current position, or let it follow
    /// the camera again —
    /// [`ForwardRenderer::set_frozen_selection_eye`](crcbl::render::ForwardRenderer::set_frozen_selection_eye).
    ///
    /// **Not one of the overlays, and it must not become one.** The three views
    /// are one picture chosen between, so pressing one row replaces the others;
    /// this changes what the *selection* answers and leaves the picture's choice
    /// alone. Freezing the cut and then reading it off the LOD tint is the whole
    /// point of having both, so [`toggled_to`] does not know about this row.
    ToggleFreeze,
}

/// The view a row leaves in force: `on` if it was not already showing, and the
/// shaded picture if it was.
///
/// **The whole of this fixture's exclusivity.** The renderer resolves a
/// precedence over three independent switches — a debug view has to survive a
/// caller setting two of them — but a *panel* has rows, and a row that says
/// `LOD VIEW: ON` while the heatmap is drawn is a row nobody can act on. So the
/// fixture holds one view rather than a flag per overlay, and pressing a row is
/// this function.
#[must_use]
pub const fn toggled_to(view: DebugView, on: DebugView) -> DebugView {
    if matches!(
        (view, on),
        (DebugView::LodTint, DebugView::LodTint) | (DebugView::Heatmap, DebugView::Heatmap)
    ) {
        DebugView::Shaded
    } else {
        on
    }
}

/// The id carrying [`QuarryAction::ToggleCamera`]. The first id a game may use,
/// per [`FIRST_GAME_ID`].
pub const CAMERA_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The id carrying [`QuarryAction::ToggleLodView`].
pub const LOD_VIEW_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 1;

/// The id carrying [`QuarryAction::ToggleHeatmap`].
pub const HEATMAP_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 2;

/// The id carrying [`QuarryAction::ToggleFreeze`].
pub const FREEZE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 3;

/// The key that pins and unpins the selection without opening the panel.
///
/// **`F`, and it was free**: `crcbl::render::Flyer` claims `WSAD`, `Space`,
/// `Shift` and the four arrows, and the loop reserves `Escape`, `F3`, `F11` and
/// `Enter`. Reachable with the left hand while the right one is flying, which is
/// the gesture this feature is: pin the cut where you are standing, then fly
/// off and look at it.
pub const FREEZE_KEY: crcbl::core::input::KeyCode = crcbl::core::input::KeyCode::KeyF;

/// How the freeze row spells [`FREEZE_KEY`] in its shortcut column.
///
/// Beside the key rather than inline in the row, so
/// `the_freeze_row_names_the_key_that_fires_it` can hold the two together — a
/// panel offering a key that does nothing is worse than one offering none.
const FREEZE_KEY_HINT: &str = "F";

/// The action a widget id names, or `None` for an id this sample's menus do not
/// use.
#[must_use]
pub const fn action_for(id: crcbl::ui::WidgetId) -> Option<QuarryAction> {
    if id == CAMERA_ID {
        Some(QuarryAction::ToggleCamera)
    } else if id == LOD_VIEW_ID {
        Some(QuarryAction::ToggleLodView)
    } else if id == HEATMAP_ID {
        Some(QuarryAction::ToggleHeatmap)
    } else if id == FREEZE_ID {
        Some(QuarryAction::ToggleFreeze)
    } else {
        None
    }
}

/// The pause panel: the loop's own three rows, then the camera in force, one
/// row per overlay, and whether the LOD selection is pinned.
///
/// Each overlay row says whether *it* is the view being drawn, so at most one of
/// them reads `ON` — see [`toggled_to`]. The freeze row is outside that
/// exclusivity and can read `ON` beside any of them, which is
/// [`QuarryAction::ToggleFreeze`]'s whole point; the *position* it is frozen at
/// is a debug-panel row rather than a menu label, because a label wide enough
/// for three coordinates is a label nothing else on the panel can line up with.
#[must_use]
pub fn pause_menu(camera: CameraMode, view: DebugView, frozen: bool) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    let state = |row: DebugView| if view == row { "ON" } else { "OFF" };
    Menu::new(
        "PAUSED",
        vec![
            MenuItem::new(RESUME_ID, "RESUME", "ESC"),
            MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
            MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
            MenuItem::new(CAMERA_ID, format!("CAMERA: {}", camera.label()), "ENTER"),
            MenuItem::new(
                LOD_VIEW_ID,
                format!("LOD VIEW: {}", state(DebugView::LodTint)),
                "ENTER",
            ),
            MenuItem::new(
                HEATMAP_ID,
                format!("HEATMAP: {}", state(DebugView::Heatmap)),
                "ENTER",
            ),
            MenuItem::new(
                FREEZE_ID,
                format!("FREEZE SELECTION: {}", if frozen { "ON" } else { "OFF" }),
                FREEZE_KEY_HINT,
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
        vec![(
            true,
            pause_menu(CameraMode::default(), DebugView::default(), false),
        )],
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
            MenuAction::Game(QuarryAction::ToggleHeatmap),
            MenuAction::Game(QuarryAction::ToggleFreeze),
        ] {
            assert!(actions.contains(&expected), "no row fires {expected:?}");
        }
    }

    /// **The freeze row prints the key that does the same thing**, and the two
    /// are one statement rather than two.
    ///
    /// A shortcut column is a promise. `FREEZE_KEY` and [`FREEZE_KEY_HINT`] sit
    /// beside each other and would still drift — a key moved to `KeyG` with the
    /// label left at `F` compiles, runs, and tells every reviewer to press a
    /// key that does nothing. `KeyCode`'s own debug spelling is what ties them.
    #[test]
    fn the_freeze_row_names_the_key_that_fires_it() {
        assert_eq!(
            format!("{FREEZE_KEY:?}"),
            format!("Key{FREEZE_KEY_HINT}"),
            "the freeze row offers a key the fixture does not listen for",
        );
        let rows = pause_menu(CameraMode::default(), DebugView::default(), false);
        let row = rows
            .items()
            .iter()
            .find(|item| item.id == FREEZE_ID)
            .expect("the freeze row is on the panel");
        assert_eq!(row.hint, FREEZE_KEY_HINT);
    }

    /// **Each row labels the value it is set to**, so a reviewer reads the
    /// state off the panel rather than guessing it from the picture.
    ///
    /// One arm per row and every value of each, because a label wired to the
    /// wrong field reads correctly for exactly one of the six combinations.
    #[test]
    fn each_row_labels_the_value_it_is_set_to() {
        for (camera, camera_row) in [
            (CameraMode::Fixed, "CAMERA: FIXED"),
            (CameraMode::Dolly, "CAMERA: DOLLY"),
            (CameraMode::Free, "CAMERA: FREE"),
        ] {
            for (view, lod_row, heat_row) in [
                (DebugView::Shaded, "LOD VIEW: OFF", "HEATMAP: OFF"),
                (DebugView::LodTint, "LOD VIEW: ON", "HEATMAP: OFF"),
                (DebugView::Heatmap, "LOD VIEW: OFF", "HEATMAP: ON"),
            ] {
                for (frozen, freeze_row) in [
                    (false, "FREEZE SELECTION: OFF"),
                    (true, "FREEZE SELECTION: ON"),
                ] {
                    let rows = labels(&pause_menu(camera, view, frozen));
                    assert!(rows.contains(&camera_row.to_string()), "{rows:?}");
                    assert!(rows.contains(&lod_row.to_string()), "{rows:?}");
                    assert!(rows.contains(&heat_row.to_string()), "{rows:?}");
                    assert!(rows.contains(&freeze_row.to_string()), "{rows:?}");
                }
            }
        }
    }

    /// **Freezing is orthogonal to the overlays**, which the panel has to show
    /// rather than merely permit.
    ///
    /// The combination a reviewer actually wants is a frozen cut *and* the LOD
    /// tint, so the freeze row has to be able to read `ON` beside an overlay row
    /// reading `ON`. Folding freezing into [`toggled_to`]'s exclusivity — the
    /// obvious way to add a fourth row to a panel that already has three — would
    /// make the two mutually exclusive and the feature unusable, and every
    /// single-row assertion above would still pass.
    #[test]
    fn the_freeze_row_reads_on_beside_a_live_overlay() {
        for view in [DebugView::Shaded, DebugView::LodTint, DebugView::Heatmap] {
            let rows = labels(&pause_menu(CameraMode::default(), view, true));
            assert!(
                rows.contains(&"FREEZE SELECTION: ON".to_string()),
                "the freeze row went off when {view:?} was drawn: {rows:?}",
            );
        }
        assert!(
            labels(&pause_menu(CameraMode::default(), DebugView::LodTint, true))
                .contains(&"LOD VIEW: ON".to_string()),
            "and the tint stayed on while the selection was frozen",
        );
    }

    /// **The two overlay rows are mutually exclusive**, and each one is its own
    /// off switch.
    ///
    /// Every start × every row, so the case a naive toggle gets wrong is
    /// covered: pressing HEATMAP while the tint is showing must *replace* it
    /// rather than leave both set, and pressing a row that is already on must
    /// return the shaded picture rather than doing nothing.
    #[test]
    fn each_overlay_row_replaces_the_other_and_switches_itself_off() {
        for start in [DebugView::Shaded, DebugView::LodTint, DebugView::Heatmap] {
            assert_eq!(
                toggled_to(start, DebugView::LodTint),
                if start == DebugView::LodTint {
                    DebugView::Shaded
                } else {
                    DebugView::LodTint
                },
                "LOD VIEW pressed from {start:?}"
            );
            assert_eq!(
                toggled_to(start, DebugView::Heatmap),
                if start == DebugView::Heatmap {
                    DebugView::Shaded
                } else {
                    DebugView::Heatmap
                },
                "HEATMAP pressed from {start:?}"
            );
            // And pressing one row twice comes back to where it started, which
            // is what a reviewer flicking a row expects.
            for row in [DebugView::LodTint, DebugView::Heatmap] {
                assert_eq!(
                    toggled_to(toggled_to(start, row), row),
                    if start == row {
                        start
                    } else {
                        DebugView::Shaded
                    },
                    "{row:?} pressed twice from {start:?}"
                );
            }
        }
    }

    /// Every spelling of every mode round-trips, and nothing else parses.
    #[test]
    fn the_camera_modes_parse_by_name() {
        assert_eq!(CameraMode::from_name("fixed"), Some(CameraMode::Fixed));
        assert_eq!(CameraMode::from_name("golden"), Some(CameraMode::Fixed));
        assert_eq!(CameraMode::from_name("dolly"), Some(CameraMode::Dolly));
        assert_eq!(CameraMode::from_name("animated"), Some(CameraMode::Dolly));
        assert_eq!(CameraMode::from_name("free"), Some(CameraMode::Free));
        assert_eq!(CameraMode::from_name("fly"), Some(CameraMode::Free));
        assert_eq!(CameraMode::from_name("free-fly"), Some(CameraMode::Free));
        assert_eq!(CameraMode::from_name("sideways"), None);
    }

    /// **The row cycles through all three and comes back**, in the order
    /// [`CameraMode::toggled`] documents.
    ///
    /// Written out as a walk rather than as three equalities: what a reviewer
    /// presses is one button, and the claim is that pressing it three times
    /// returns them to where they started rather than that any single step is
    /// right. A cycle that visited two of the three would land back on the
    /// default in two presses and pass every assertion about a single step.
    #[test]
    fn the_camera_row_cycles_through_every_mode() {
        let mut seen = vec![CameraMode::default()];
        for _ in 0..2 {
            seen.push(seen.last().expect("seeded above").toggled());
        }
        assert_eq!(
            seen,
            vec![CameraMode::Fixed, CameraMode::Dolly, CameraMode::Free],
        );
        assert_eq!(
            seen.last().expect("seeded above").toggled(),
            CameraMode::default(),
            "a third press must come back round to the goldens' pose",
        );
    }
}
