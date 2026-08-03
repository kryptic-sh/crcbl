//! Asteroids' menus: which one belongs to which state, and what its buttons do.
//!
//! ```text
//!   paused ──────────────────────────────────────────────► Paused
//!   GameOver ────────────────────────────────────────────► GameOver
//!   WaitingToStart ──────────────────────────────────────► Start
//!   Playing ─────────────────────────────────────────────► none
//! ```
//!
//! The art, the layout and the keyboard model are `crcbl::ui::menu` and
//! `crcbl::render::menu` — shared with breakout, flappy and the sandbox, because
//! a window frame is not something a game should own. What is here is the part
//! that genuinely is this game's: **which menu a frame shows, and what happens
//! when a button is fired.**
//!
//! **The container is no longer written here.** `Menus` was a hand-rolled copy
//! of the same struct in every sample; it is now
//! [`crcbl::ui::menu::MenuSet`], keyed by this game's [`MenuKind`]. What stays
//! per-game is what was always genuinely per-game: the [`MenuAction`] enum and
//! its `WidgetId` discriminants, the [`MenuKind::of`] precedence rule, the
//! titles and the labels.
//!
//! # There is no win menu, and that is not an omission
//!
//! The waves never stop and the count is capped rather than terminal
//! (`game::MAX_WAVE_ROCKS`), so there is no state to reach. A `YOU WIN` panel
//! would be a screen the game can never show.
//!
//! # What the menu takes from the keyboard, and the one thing it shadows
//!
//! Up, Down and Enter, and only while a menu is on screen. Two of the three are
//! free — `game.rs` binds the arrows, WASD, Space and R — and **ArrowUp is
//! not**: it is the second binding of the thrust action, beside `KeyW`.
//!
//! So while a panel is up, ArrowUp moves the selection instead of thrusting.
//! That is the same trade flappy made and for the same two reasons:
//!
//! * Thrust has a second binding, `KeyW`, which is not shadowed on any menu.
//!   Nothing the player can do becomes unreachable.
//! * The three keys are the same three in every sample, which is the rule F3,
//!   Escape and F11 already follow. A menu navigated with different keys in each
//!   game is three menus.
//!
//! It matters less here than it did in flappy, because a menu is only on screen
//! when the game is *not* being flown: `MenuKind::None` is every `Playing`
//! frame that is not paused, and thrusting while paused does nothing anyway.

use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::game::{GameState, RenderState};

/// What firing a menu button asks the loop to do.
///
/// An action rather than a key: a button that "presses Space" would be a menu
/// re-entering its own input path, and the loop would have to tell a synthesised
/// key from a real one. The loop matches on this and does the thing directly —
/// except [`MenuAction::Fire`], which really is a key, because starting and
/// restarting a game is the *simulation's* business and the simulation is driven
/// by its action map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Un-pause.
    Resume,
    /// Pull the trigger — which starts a waiting game and restarts a finished
    /// one, exactly as `run_tick` reads it.
    Fire,
    /// Toggle borderless fullscreen.
    Fullscreen,
    /// Toggle the debug panel.
    DebugOverlay,
}

impl MenuAction {
    /// The [`WidgetId`] this action is carried by.
    ///
    /// The discriminant, written out rather than derived, because a `WidgetId`
    /// that changed when a variant was inserted would silently re-point every
    /// button.
    const fn id(self) -> WidgetId {
        match self {
            Self::Resume => 1,
            Self::Fire => 2,
            Self::Fullscreen => 3,
            Self::DebugOverlay => 4,
        }
    }

    /// The action an id names, or `None` for an id from another menu system.
    #[must_use]
    pub const fn from_id(id: WidgetId) -> Option<Self> {
        match id {
            1 => Some(Self::Resume),
            2 => Some(Self::Fire),
            3 => Some(Self::Fullscreen),
            4 => Some(Self::DebugOverlay),
            _ => None,
        }
    }
}

/// An item for `action`, labelled and with its key printed beside it.
fn item(action: MenuAction, label: &str, hint: &str) -> MenuItem {
    MenuItem::new(action.id(), label, hint)
}

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MenuKind {
    /// The game is being played: no menu at all.
    #[default]
    None,
    /// The rocks are drifting and the ship has not fired yet.
    Start,
    /// The loop has stopped advancing the simulation.
    Paused,
    /// Out of lives.
    GameOver,
}

impl MenuKind {
    /// The menu this frame shows.
    ///
    /// **Pause wins over everything**, for the same reason
    /// `HudStrings::refresh` lets it win over the simulation's own state: the
    /// server is still running, and a player whose window lost focus wants to be
    /// told the game is stopped rather than what the ship was doing.
    #[must_use]
    pub const fn of(paused: bool, render: &RenderState) -> Self {
        if paused {
            return Self::Paused;
        }
        match render.state {
            Some(GameState::GameOver) => Self::GameOver,
            Some(GameState::WaitingToStart) | None => Self::Start,
            Some(GameState::Playing) => Self::None,
        }
    }
}

/// Asteroids' menus, keyed by the state each belongs to.
///
/// The container is [`crcbl::ui::menu::MenuSet`] — shared with every other
/// sample, because holding a handful of panels and switching between them
/// without carrying a half-finished click across is not something a game should
/// own. [`MenuKind::None`] has no entry in it, which is how a flying frame is
/// told to draw nothing.
pub type Menus = MenuSet<MenuKind>;

/// The three menus, with nothing shown.
#[must_use]
pub fn menus() -> Menus {
    use MenuAction::{DebugOverlay, Fire, Fullscreen, Resume};
    MenuSet::new(
        MenuKind::None,
        vec![
            (
                MenuKind::Start,
                Menu::new(
                    "ASTEROIDS",
                    vec![
                        item(Fire, "FLY", "SPACE"),
                        item(Fullscreen, "FULLSCREEN", "F11"),
                        item(DebugOverlay, "DEBUG PANEL", "F3"),
                    ],
                ),
            ),
            (
                MenuKind::Paused,
                Menu::new(
                    "PAUSED",
                    vec![
                        item(Resume, "RESUME", "ESC"),
                        item(Fullscreen, "FULLSCREEN", "F11"),
                        item(DebugOverlay, "DEBUG PANEL", "F3"),
                    ],
                ),
            ),
            (
                MenuKind::GameOver,
                Menu::new(
                    "GAME OVER",
                    vec![
                        item(Fire, "TRY AGAIN", "SPACE"),
                        item(Fullscreen, "FULLSCREEN", "F11"),
                    ],
                ),
            ),
        ],
    )
}
// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::math::Vec2;
    use crcbl::ui::text::FontAtlas;
    use crcbl::ui::{ButtonState, PointerInput};

    /// The action the highlighted button carries, which is what the loop reads.
    ///
    /// The set deals in [`WidgetId`] — it is shared with every other sample and
    /// has no idea what an id means here — so this is the one translation, in
    /// one place, the way `app.rs` does it.
    fn activate(menus: &mut Menus) -> Option<MenuAction> {
        menus.activate().and_then(MenuAction::from_id)
    }

    /// The action a frame of pointer input fired, if any.
    fn point(menus: &mut Menus, extent: (u32, u32), pointer: PointerInput) -> Option<MenuAction> {
        menus
            .point(extent, &FontAtlas::built_in(), pointer)
            .and_then(MenuAction::from_id)
    }

    fn render(state: Option<GameState>) -> RenderState {
        RenderState {
            state,
            ..RenderState::default()
        }
    }

    /// **A paused game shows the pause menu and nothing else.** Pause wins over
    /// every simulation state, including the two that would otherwise have drawn
    /// a panel of their own.
    #[test]
    fn a_paused_game_shows_the_pause_menu_and_nothing_else() {
        for state in [
            None,
            Some(GameState::WaitingToStart),
            Some(GameState::Playing),
            Some(GameState::GameOver),
        ] {
            assert_eq!(
                MenuKind::of(true, &render(state)),
                MenuKind::Paused,
                "paused over {state:?} did not show the pause menu",
            );
        }

        // And the container really draws one menu: `current` is a single
        // `Option`, so "and nothing else" is structural — but the titles must
        // differ, or two kinds could draw the same panel and nobody would know.
        let mut menus = menus();
        let mut titles = Vec::new();
        for kind in [MenuKind::Start, MenuKind::Paused, MenuKind::GameOver] {
            menus.show(kind);
            titles.push(
                menus
                    .current()
                    .expect("every kind but None has one")
                    .title
                    .clone(),
            );
        }
        titles.sort();
        titles.dedup();
        assert_eq!(titles.len(), 3, "two menus share a title");

        menus.show(MenuKind::None);
        assert!(menus.current().is_none(), "a flying frame draws no menu");
    }

    /// Every other state maps to the menu that belongs to it, and a game being
    /// played gets none.
    #[test]
    fn each_state_shows_the_menu_that_belongs_to_it() {
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::WaitingToStart))),
            MenuKind::Start,
        );
        assert_eq!(MenuKind::of(false, &render(None)), MenuKind::Start);
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::Playing))),
            MenuKind::None,
        );
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::GameOver))),
            MenuKind::GameOver,
        );
    }

    /// Every button carries an action the loop can act on, no two in one menu
    /// carry the same one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = menus();
        for kind in [MenuKind::Start, MenuKind::Paused, MenuKind::GameOver] {
            menus.show(kind);
            let menu = menus.current().expect("a menu");
            let actions: Vec<MenuAction> = menu
                .items()
                .iter()
                .map(|item| {
                    MenuAction::from_id(item.id)
                        .unwrap_or_else(|| panic!("{kind:?}: {} names no action", item.label))
                })
                .collect();
            assert!(!actions.is_empty(), "{kind:?} has no buttons");
            for (index, action) in actions.iter().enumerate() {
                assert!(
                    !actions[..index].contains(action),
                    "{kind:?} carries {action:?} twice",
                );
            }
            for item in menu.items() {
                assert!(!item.hint.is_empty(), "{kind:?}: {} has no key", item.label);
            }
        }
    }

    /// **The trigger's binding is never shadowed.** Every button whose action is
    /// a shot prints `SPACE`, which is the only key `game.rs` binds to fire and
    /// the one the menu does not take — see this module's header.
    #[test]
    fn every_fire_button_prints_the_key_the_menu_does_not_take() {
        let mut menus = menus();
        for kind in [MenuKind::Start, MenuKind::GameOver] {
            menus.show(kind);
            for item in menus.current().expect("a menu").items() {
                if MenuAction::from_id(item.id) == Some(MenuAction::Fire) {
                    assert_eq!(item.hint, "SPACE", "{kind:?}: {}", item.label);
                }
            }
        }
    }

    /// **Keyboard activation works**, on every menu, and reports the action the
    /// selected button carries.
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
        assert_eq!(activate(&mut menus), Some(MenuAction::Resume), "it wraps");
        menus.select_previous();
        assert_eq!(activate(&mut menus), Some(MenuAction::DebugOverlay));

        menus.show(MenuKind::None);
        assert_eq!(activate(&mut menus), None, "there is no menu to activate");
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = menus();
        menus.show(MenuKind::Start);
        menus.select_next();
        menus.press(true);
        let menu = menus.current().expect("a menu");
        assert_eq!(menu.state(1), ButtonState::Pressed);
        assert_eq!(menu.state(0), ButtonState::Idle);
        menus.press(false);
        assert_eq!(
            menus.current().expect("a menu").state(1),
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
        menus.show(MenuKind::GameOver);

        let layout = menus.current().expect("a menu").layout(extent, &atlas);
        let target = layout.items()[0];
        let over = (target.min + target.max) * 0.5;

        let down = PointerInput {
            pos: over,
            down: true,
            released: false,
        };
        assert_eq!(point(&mut menus, extent, down), None);
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Pressed,
            "the pointer's press did not reach the art",
        );
        let up = PointerInput {
            pos: over,
            down: false,
            released: true,
        };
        assert_eq!(point(&mut menus, extent, up), Some(MenuAction::Fire));

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

    /// Switching menus drops the previous one's press capture, so a click that
    /// started on the pause menu cannot land on the start menu's button in the
    /// same place.
    #[test]
    fn switching_menus_drops_the_press() {
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

        menus.show(MenuKind::Start);
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Hovered,
            "the new menu inherited a press nobody is making",
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
            "a release fired a button whose press was on another menu",
        );
    }
}
