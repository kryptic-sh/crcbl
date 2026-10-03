//! The sandbox's menus: the pause panel, and — natively — the lobby.
//!
//! **Smaller than breakout's and flappy's on purpose.** The sandbox is a
//! milestone harness, not a game: there is no run to start, no score to lose and
//! nothing to win, so a `GAME OVER` panel would be a screen it could never show
//! and a `START` panel would be a door with no room behind it. What it does have
//! is a pause — the loop declining to advance a spinning cube — and that is the
//! one state a menu belongs to.
//!
//! Everything else is shared: the window frame, the button skin, the layout and
//! the keyboard model are `crcbl::ui::menu` and `crcbl::render::menu`, the same
//! three keys navigate it, and [`crcbl::engine::MenuAction`] turns a button into
//! an effect. `docs/plan/sample/00-samples-overview.md`'s rule 4 is why the
//! sandbox has a debug overlay at all, and it is the same reason it has this: a
//! sample that cannot show the engine's menu is a finding about the menu.
//!
//! # The lobby is a door to a session, not to a run
//!
//! A native sandbox whose command line chose no session opens on a LAN lobby
//! (`crate::lobby`): offline, host, the hosts it hears, and an address to
//! connect to. Its rows are built at run time from what the LAN answers, so
//! [`menus`] holds only the pause panel and `crate::app` puts the lobby in
//! the set the first frame it is open. Pause wins over it, as everywhere.
//!
//! # The container is the engine's, keyed by [`MenuKind`]
//!
//! [`crcbl::ui::menu::MenuSet`] holds a game's menus keyed by whatever type
//! names its states. [`MenuKind::Running`] is the state with no menu in the
//! set — which is what makes every method on the set a no-op while the
//! sandbox is running.

use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crcbl::engine::{FIRST_GAME_ID, FrameLimit, Pacing};
#[cfg(not(target_arch = "wasm32"))]
use crcbl::lan::lobby::LobbyPick;

/// What only the sandbox's menus do: the pause menu's two settings rows,
/// and — natively — the lobby's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxAction {
    /// Cycle the display pacing: Auto → Vsync → Adaptive → Off.
    CyclePacing,
    /// Cycle the frame limit up the ladder in `crate::app::next_limit`,
    /// wrapping at "unlimited".
    CycleLimit,
    /// Leave the lobby with no session: the sandbox as it always ran.
    #[cfg(not(target_arch = "wasm32"))]
    Offline,
    /// Start what a lobby row asks for.
    #[cfg(not(target_arch = "wasm32"))]
    Lobby(LobbyPick),
}

/// The id carrying [`SandboxAction::CyclePacing`]. The first id a game may
/// use, per [`FIRST_GAME_ID`].
pub const PACING_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The id carrying [`SandboxAction::CycleLimit`].
pub const LIMIT_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 1;

/// The lobby's offline row.
#[cfg(not(target_arch = "wasm32"))]
pub const OFFLINE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 2;

/// The lobby's host row.
#[cfg(not(target_arch = "wasm32"))]
pub const HOST_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 3;

/// The lobby's connect row, which joins the address typed into it.
#[cfg(not(target_arch = "wasm32"))]
pub const CONNECT_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 4;

/// The lobby's first listed host; the rest follow it, one id a row — see
/// [`crcbl::lan::lobby::listed_row`].
#[cfg(not(target_arch = "wasm32"))]
pub const FIRST_LISTED_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 5;

/// The action a widget id names, or `None` for an id this game's menus do
/// not use.
#[must_use]
pub fn action_for(id: crcbl::ui::WidgetId) -> Option<SandboxAction> {
    match id {
        PACING_ID => Some(SandboxAction::CyclePacing),
        LIMIT_ID => Some(SandboxAction::CycleLimit),
        #[cfg(not(target_arch = "wasm32"))]
        OFFLINE_ID => Some(SandboxAction::Offline),
        #[cfg(not(target_arch = "wasm32"))]
        HOST_ID => Some(SandboxAction::Lobby(LobbyPick::Host)),
        #[cfg(not(target_arch = "wasm32"))]
        CONNECT_ID => Some(SandboxAction::Lobby(LobbyPick::Connect)),
        #[cfg(not(target_arch = "wasm32"))]
        _ => crcbl::lan::lobby::listed_row(id, FIRST_LISTED_ID)
            .map(|row| SandboxAction::Lobby(LobbyPick::Listed(row))),
        #[cfg(target_arch = "wasm32")]
        _ => None,
    }
}

/// The pause panel, its two settings rows labelled with the values in force.
///
/// Rebuilt by the game when either value changes; the initial set is built
/// from the defaults so a run that never touches them draws the right labels
/// with no rebuild.
#[must_use]
pub fn pause_menu(pacing: Pacing, limit: FrameLimit) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    Menu::new(
        "PAUSED",
        vec![
            MenuItem::new(RESUME_ID, "RESUME", "ESC"),
            MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
            MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
            MenuItem::new(PACING_ID, pacing_label(pacing), "ENTER"),
            MenuItem::new(LIMIT_ID, limit_label(limit), "ENTER"),
        ],
    )
}

/// The label of the pacing row, naming the value it is set to.
fn pacing_label(pacing: Pacing) -> String {
    let name = match pacing {
        Pacing::Auto => "AUTO",
        Pacing::Vsync => "VSYNC",
        Pacing::Adaptive => "ADAPTIVE",
        Pacing::Off => "OFF",
    };
    format!("PACING: {name}")
}

/// The label of the fps row, naming the value it is set to.
fn limit_label(limit: FrameLimit) -> String {
    if limit.rate() == 0 {
        "FPS: UNLIMITED".to_string()
    } else {
        format!("FPS: {}", limit.rate())
    }
}

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuKind {
    /// The cube is spinning: no menu at all.
    #[default]
    Running,
    /// The loop has stopped ticking.
    Paused,
    /// The lobby: offline, host, the LAN's hosts and a direct connect.
    #[cfg(not(target_arch = "wasm32"))]
    Lobby,
}

/// The sandbox's menus, keyed by [`MenuKind`].
pub type Menus = MenuSet<MenuKind>;

/// The pause menu, not shown.
///
/// [`MenuKind::Running`] has no entry, which is how the set is told that a
/// running frame draws no menu.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(
        MenuKind::Running,
        vec![(
            MenuKind::Paused,
            pause_menu(Pacing::default(), FrameLimit::default()),
        )],
    )
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The sandbox's whole menu vocabulary: the loop's three, plus the two
    /// settings rows of its own.
    type MenuAction = crcbl::engine::MenuAction<SandboxAction>;

    use crcbl::math::Vec2;
    use crcbl::ui::text::FontAtlas;
    use crcbl::ui::{ButtonState, PointerInput};

    /// The action the highlighted button carries, which is what the loop reads.
    ///
    /// The set deals in [`WidgetId`] — it is shared with every other sample and
    /// has no idea what an id means here — so this is the one translation, in
    /// one place, the way `app.rs` does it.
    fn activate(menus: &mut Menus) -> Option<MenuAction> {
        menus
            .activate()
            .and_then(|id| MenuAction::from_id(id, action_for))
    }

    /// The action a frame of pointer input fired, if any.
    fn point(menus: &mut Menus, extent: (u32, u32), pointer: PointerInput) -> Option<MenuAction> {
        menus
            .point(extent, &FontAtlas::built_in(), pointer)
            .and_then(|id| MenuAction::from_id(id, action_for))
    }

    /// **A running sandbox shows no menu, and a paused one shows exactly the
    /// pause menu.** The sandbox's whole state machine, and the reason it has no
    /// start or game-over panel.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = menus();
        assert!(menus.current().is_none());
        assert!(!menus.is_showing());
        assert_eq!(activate(&mut menus), None, "nothing to fire");

        menus.show(MenuKind::Paused);
        assert_eq!(menus.current().expect("the pause menu").title, "PAUSED");

        menus.show(MenuKind::Running);
        assert!(menus.current().is_none());
    }

    /// Every button carries an action the loop can act on, no two carry the same
    /// one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = menus();
        menus.show(MenuKind::Paused);
        let menu = menus.current().expect("the pause menu");
        let actions: Vec<MenuAction> = menu
            .items()
            .iter()
            .map(|item| {
                MenuAction::from_id(item.id, action_for)
                    .unwrap_or_else(|| panic!("{} names no action", item.label))
            })
            .collect();
        assert_eq!(actions.len(), 5);
        for (index, action) in actions.iter().enumerate() {
            assert!(
                !actions[..index].contains(action),
                "the menu carries {action:?} twice",
            );
        }
        for item in menu.items() {
            assert!(!item.hint.is_empty(), "{} has no key", item.label);
        }
    }

    /// **Each lobby row's id names its action**, a listed host's naming its
    /// row, and none of them is one the pause menu's rows or the loop own.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn each_lobby_row_names_its_action() {
        assert_eq!(action_for(OFFLINE_ID), Some(SandboxAction::Offline));
        assert_eq!(
            action_for(HOST_ID),
            Some(SandboxAction::Lobby(LobbyPick::Host))
        );
        assert_eq!(
            action_for(CONNECT_ID),
            Some(SandboxAction::Lobby(LobbyPick::Connect))
        );
        assert_eq!(
            action_for(FIRST_LISTED_ID + 2),
            Some(SandboxAction::Lobby(LobbyPick::Listed(2)))
        );
        assert_eq!(action_for(PACING_ID), Some(SandboxAction::CyclePacing));
        assert_eq!(action_for(LIMIT_ID), Some(SandboxAction::CycleLimit));
        assert_eq!(action_for(crcbl::engine::RESUME_ID), None);
    }

    /// **Keyboard activation works**, and reports the action the selected button
    /// carries.
    #[test]
    fn the_keyboard_selects_and_activates() {
        let mut menus = menus();
        menus.show(MenuKind::Paused);
        assert_eq!(activate(&mut menus), Some(MenuAction::Resume));
        menus.select_next();
        assert_eq!(activate(&mut menus), Some(MenuAction::Fullscreen));
        menus.select_next();
        assert_eq!(activate(&mut menus), Some(MenuAction::DebugOverlay));
        menus.select_next();
        assert_eq!(
            activate(&mut menus),
            Some(MenuAction::Game(SandboxAction::CyclePacing)),
        );
        menus.select_next();
        assert_eq!(
            activate(&mut menus),
            Some(MenuAction::Game(SandboxAction::CycleLimit)),
        );
        menus.select_next();
        assert_eq!(activate(&mut menus), Some(MenuAction::Resume), "it wraps");
        menus.select_previous();
        assert_eq!(
            activate(&mut menus),
            Some(MenuAction::Game(SandboxAction::CycleLimit)),
            "previous from the top wraps to the last row",
        );
    }

    /// The settings rows label themselves with the values they are set to,
    /// so a player can read the current pacing and cap off the panel.
    #[test]
    fn the_settings_rows_label_the_values_they_are_set_to() {
        let menu = pause_menu(Pacing::default(), FrameLimit::default());
        let labels: Vec<&str> = menu
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "RESUME",
                "FULLSCREEN",
                "DEBUG PANEL",
                "PACING: AUTO",
                "FPS: 1000",
            ],
        );

        // And a changed value shows up in the label.
        let menu = pause_menu(Pacing::Off, FrameLimit::unlimited());
        let labels: Vec<&str> = menu
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels[3], "PACING: OFF");
        assert_eq!(labels[4], "FPS: UNLIMITED");
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = menus();
        menus.show(MenuKind::Paused);
        menus.select_next();
        menus.press(true);
        let menu = menus.current().expect("the pause menu");
        assert_eq!(menu.state(1), ButtonState::Pressed);
        assert_eq!(menu.state(0), ButtonState::Idle);
        menus.press(false);
        assert_eq!(
            menus.current().expect("the pause menu").state(1),
            ButtonState::Hovered,
        );
    }

    /// **The pointer activates too**, through the same actions, and a click over
    /// nothing fires nothing.
    #[test]
    fn the_pointer_clicks_a_button() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = menus();
        menus.show(MenuKind::Paused);

        let layout = menus.current().expect("a menu").layout(extent, &atlas);
        let target = layout.items()[1];
        let over = (target.min + target.max) * 0.5;

        let down = PointerInput {
            pos: over,
            down: true,
            released: false,
        };
        assert_eq!(point(&mut menus, extent, down), None);
        assert_eq!(
            menus.current().expect("a menu").state(1),
            ButtonState::Pressed,
            "the pointer's press did not reach the art",
        );
        let up = PointerInput {
            pos: over,
            down: false,
            released: true,
        };
        assert_eq!(point(&mut menus, extent, up), Some(MenuAction::Fullscreen));

        let corner = PointerInput {
            pos: Vec2::new(3.0, 3.0),
            down: true,
            released: false,
        };
        assert_eq!(point(&mut menus, extent, corner), None);
        let corner_up = PointerInput {
            pos: Vec2::new(3.0, 3.0),
            down: false,
            released: true,
        };
        assert_eq!(point(&mut menus, extent, corner_up), None);
    }

    /// Taking the menu away drops the press capture, so a click that started on
    /// it cannot land after it comes back.
    #[test]
    fn hiding_the_menu_drops_the_press() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = menus();
        menus.show(MenuKind::Paused);
        let layout = menus.current().expect("a menu").layout(extent, &atlas);
        let over = (layout.items()[0].min + layout.items()[0].max) * 0.5;
        point(
            &mut menus,
            extent,
            PointerInput {
                pos: over,
                down: true,
                released: false,
            },
        );
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Pressed,
        );

        menus.show(MenuKind::Running);
        menus.show(MenuKind::Paused);
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Hovered,
            "the menu came back with a press nobody is making",
        );
        assert_eq!(
            point(
                &mut menus,
                extent,
                PointerInput {
                    pos: over,
                    down: false,
                    released: true,
                },
            ),
            None,
            "a release fired a button whose press was on a menu that was gone",
        );
    }
}
