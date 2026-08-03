//! Flappy's menus: which one belongs to which state, and what its buttons do.
//!
//! ```text
//!   paused ──────────────────────────────────────────────► Paused
//!   Dead ────────────────────────────────────────────────► Dead
//!   WaitingToStart ──────────────────────────────────────► Start
//!   Playing ─────────────────────────────────────────────► none
//! ```
//!
//! The art, the layout and the keyboard model are `crcbl::ui::menu` and
//! `crcbl::render::menu` — shared with breakout and the sandbox, because a window
//! frame is not something a game should own. What is here is the part that
//! genuinely is flappy's: **which menu a frame of this game shows, and what
//! happens when a button is fired.**
//!
//! **The container is no longer written here.** `Menus` was a hand-rolled copy
//! of the same struct in every sample; it is now
//! [`crcbl::ui::menu::MenuSet`], keyed by this game's [`MenuKind`]. What stays
//! per-game is what was always genuinely per-game: the [`Flap`] action and its
//! id, the [`MenuKind::of`] precedence rule, the titles and the labels. Resume,
//! fullscreen and the debug panel were declared five times too — they are the
//! menu equivalents of the loop's three reserved keys — and they are
//! [`crcbl::engine::MenuAction`]'s now.
//!
//! # There is no win menu, and that is not an omission
//!
//! Flappy is endless. There is no state to reach and no score to beat but the
//! player's own, which is what `RenderState::best` is for and what the HUD
//! already says. A `YOU WIN` panel would be a screen the game can never show.
//!
//! # What the menu takes from the keyboard, and the one thing it shadows
//!
//! Up, Down and Enter, and only while a menu is on screen. Two of the three are
//! free — `game.rs` binds Space, ArrowUp and R — and **ArrowUp is not**: it is
//! the *second* binding of the flap action, beside Space.
//!
//! So while a panel is up, ArrowUp moves the selection instead of flapping. That
//! is deliberate rather than overlooked:
//!
//! * Space is the primary binding, is what the HUD has always told the player to
//!   press, and is printed on the button that does the same thing. It is not
//!   shadowed, on any menu, ever.
//! * The three keys are the same three in every sample, which is the same rule
//!   F3, Escape and F11 already follow. A menu navigated with different keys in
//!   each game is three menus.
//!
//! `docs/backlog.md` records the shadowing so it is a decision on the record
//! rather than a surprise. Nothing else changes: every key still does what it
//! did the moment the panel is dismissed.

use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::game::{GameState, RenderState};

/// What only flappy's menus do.
///
/// Resume, fullscreen and the debug panel are the *loop's* — they are the menu
/// equivalents of its three reserved keys — and live in
/// [`crcbl::engine::MenuAction`]. This is the fourth, and it is an action
/// rather than a key because a button that "presses Space" would be a menu
/// re-entering its own input path.
///
/// Flapping is nonetheless delivered to the simulation *as* a key, in
/// [`crate::app::Flappy::apply`]: starting and restarting a run is the
/// simulation's business and the simulation is driven by its action map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flap {
    /// Beat a wing — which starts a waiting run and restarts a finished one.
    Wing,
}

/// The [`WidgetId`] carrying [`Flap::Wing`].
///
/// Numbered from [`crcbl::engine::FIRST_GAME_ID`], not from one: everything
/// below that is the loop's, and a button that claimed
/// [`crcbl::engine::RESUME_ID`] would un-pause instead of flapping.
pub const FLAP_ID: WidgetId = crcbl::engine::FIRST_GAME_ID;

/// The half of the id mapping that is flappy's, for
/// [`crcbl::engine::MenuAction::from_id`]. Never asked about a reserved id.
#[must_use]
pub const fn flap_from_id(id: WidgetId) -> Option<Flap> {
    if id == FLAP_ID {
        Some(Flap::Wing)
    } else {
        None
    }
}

/// An item on `id`, labelled and with its key printed beside it.
fn item(id: WidgetId, label: &str, hint: &str) -> MenuItem {
    MenuItem::new(id, label, hint)
}

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MenuKind {
    /// The run is in the air: no menu at all.
    #[default]
    None,
    /// The bird is parked, waiting for the first flap.
    Start,
    /// The loop has stopped advancing the simulation.
    Paused,
    /// The run ended.
    Dead,
}

impl MenuKind {
    /// The menu this frame shows.
    ///
    /// **Pause wins over everything**, for the same reason
    /// `HudStrings::refresh` lets it win over the simulation's own state: the
    /// server is still flying, and a player who alt-tabbed away wants to be told
    /// the game is stopped rather than what the bird was doing.
    ///
    /// Unlike breakout there is no "fresh game" test to make: `WaitingToStart`
    /// is reached only at the very start and by a restart, never mid-run, so
    /// every visit to it is a moment the player is being asked to begin.
    #[must_use]
    pub const fn of(paused: bool, render: &RenderState) -> Self {
        if paused {
            return Self::Paused;
        }
        match render.state {
            Some(GameState::Dead) => Self::Dead,
            Some(GameState::WaitingToStart) | None => Self::Start,
            Some(GameState::Playing) => Self::None,
        }
    }
}

/// Flappy's menus, keyed by the state each belongs to.
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
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    MenuSet::new(
        MenuKind::None,
        vec![
            (
                MenuKind::Start,
                Menu::new(
                    "FLAPPY",
                    vec![
                        item(FLAP_ID, "FLY", "SPACE"),
                        item(FULLSCREEN_ID, "FULLSCREEN", "F11"),
                        item(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
                    ],
                ),
            ),
            (
                MenuKind::Paused,
                Menu::new(
                    "PAUSED",
                    vec![
                        item(RESUME_ID, "RESUME", "ESC"),
                        item(FULLSCREEN_ID, "FULLSCREEN", "F11"),
                        item(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
                    ],
                ),
            ),
            (
                MenuKind::Dead,
                Menu::new(
                    "GAME OVER",
                    vec![
                        item(FLAP_ID, "TRY AGAIN", "SPACE"),
                        item(FULLSCREEN_ID, "FULLSCREEN", "F11"),
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

    /// Flappy's whole menu vocabulary: the loop's three, plus [`Flap`].
    ///
    /// Only the tests name it. `app.rs` never sees a whole `MenuAction` — the
    /// loop keeps its own three and hands `Flappy::apply` the [`Flap`] it could
    /// not answer itself.
    type MenuAction = crcbl::engine::MenuAction<Flap>;
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
            .and_then(|id| MenuAction::from_id(id, flap_from_id))
    }

    /// The action a frame of pointer input fired, if any.
    fn point(menus: &mut Menus, extent: (u32, u32), pointer: PointerInput) -> Option<MenuAction> {
        menus
            .point(extent, &FontAtlas::built_in(), pointer)
            .and_then(|id| MenuAction::from_id(id, flap_from_id))
    }

    fn render(state: Option<GameState>) -> RenderState {
        RenderState {
            state,
            ..RenderState::default()
        }
    }

    /// **A paused game shows the pause menu and nothing else.** The assertion
    /// the slice was asked for: pause wins over every simulation state,
    /// including the one that would otherwise have drawn the start menu.
    #[test]
    fn a_paused_game_shows_the_pause_menu_and_nothing_else() {
        for state in [
            None,
            Some(GameState::WaitingToStart),
            Some(GameState::Playing),
            Some(GameState::Dead),
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
        for kind in [MenuKind::Start, MenuKind::Paused, MenuKind::Dead] {
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

    /// Every other state maps to the menu that belongs to it, and a run in the
    /// air gets none.
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
            MenuKind::of(false, &render(Some(GameState::Dead))),
            MenuKind::Dead,
        );
    }

    /// Every button carries an action the loop can act on, no two in one menu
    /// carry the same one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = menus();
        for kind in [MenuKind::Start, MenuKind::Paused, MenuKind::Dead] {
            menus.show(kind);
            let menu = menus.current().expect("a menu");
            let actions: Vec<MenuAction> = menu
                .items()
                .iter()
                .map(|item| {
                    MenuAction::from_id(item.id, flap_from_id)
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

    /// **The primary flap binding is never shadowed.** Every button whose action
    /// is a flap prints `SPACE`, which is the key `game.rs` lists first and the
    /// one the menu does not take — see this module's header.
    #[test]
    fn every_flap_button_prints_the_key_the_menu_does_not_take() {
        let mut menus = menus();
        for kind in [MenuKind::Start, MenuKind::Dead] {
            menus.show(kind);
            for item in menus.current().expect("a menu").items() {
                if MenuAction::from_id(item.id, flap_from_id) == Some(MenuAction::Game(Flap::Wing))
                {
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
        menus.show(MenuKind::Dead);

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
        assert_eq!(
            point(&mut menus, extent, up),
            Some(MenuAction::Game(Flap::Wing))
        );

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

        // **Slot 1, not slot 0.** `UiState::interact` fires on release only
        // when the capture names the id the cursor is over, and these two menus
        // agree on no id at slot 0 — one opens with `RESUME`, the other with
        // this game's own action — so a press there could not have fired
        // whatever the container did with the capture. Written that way the
        // test passed with `MenuSet::show`'s `self.ui.clear()` deleted
        // outright. `FULLSCREEN_ID` sits at slot 1 in both, which is what gives
        // the release something to fire.
        const SHARED: usize = 1;
        let shared_id = menus.current().expect("a menu").items()[SHARED].id;
        let paused = menus.current().expect("a menu").layout(extent, &atlas);
        let over = (paused.items()[SHARED].min + paused.items()[SHARED].max) * 0.5;
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
            menus.current().expect("a menu").state(SHARED),
            ButtonState::Pressed,
        );

        menus.show(MenuKind::Start);
        assert_eq!(
            menus.current().expect("a menu").items()[SHARED].id,
            shared_id,
            "the two menus stopped sharing a button, so this test guards nothing",
        );
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Hovered,
            "the new menu inherited a press nobody is making",
        );
        let start = menus.current().expect("a menu").layout(extent, &atlas);
        let over = (start.items()[SHARED].min + start.items()[SHARED].max) * 0.5;
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
