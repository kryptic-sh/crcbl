//! Breakout's menus: which one belongs to which state, and what its buttons do.
//!
//! ```text
//!   paused ──────────────────────────────────────────────► Paused
//!   Won ─────────────────────────────────────────────────► Won
//!   Lost ────────────────────────────────────────────────► Lost
//!   WaitingForLaunch, nothing broken yet ────────────────► Start
//!   WaitingForLaunch mid-game, Playing ──────────────────► none
//! ```
//!
//! The art, the layout and the keyboard model are `crcbl::ui::menu` and
//! `crcbl::render::menu` — shared with flappy and the sandbox, because a window
//! frame is not something a game should own. What is here is the part that
//! genuinely is breakout's: **which menu a frame of this game shows, and what
//! happens when a button is fired.**
//!
//! **Neither the container nor most of the vocabulary is written here.**
//! `Menus` was a hand-rolled copy of the same struct in every sample; it is now
//! [`crcbl::ui::menu::MenuSet`], keyed by this game's [`MenuKind`]. Resume,
//! fullscreen and the debug panel were declared five times as well — they are
//! the menu equivalents of the loop's three reserved keys — and they are
//! [`crcbl::engine::MenuAction`]'s, on [`crcbl::engine::RESUME_ID`] and its two
//! neighbours. What stays per-game is what was always genuinely per-game: the
//! [`Launch`] action and its id, the [`MenuKind::of`] precedence rule, the
//! titles and the labels.
//!
//! # A menu never takes a key the game already had
//!
//! Every existing key still does exactly what it did: Space launches, Escape
//! pauses, F11 goes fullscreen, F3 shows the panel — and each of those is
//! printed on the button that does the same thing, so the menu documents the
//! keyboard rather than replacing it. What the menu adds is Up, Down and Enter,
//! three keys `game.rs`'s action map does not bind, and it takes those **only
//! while a menu is on screen**.
//!
//! That is the whole answer to "does a keyboard-driven game regress when its
//! menus grow buttons". It does not: a player who never learns the menu is
//! playing the same game, and a player who does gets a second way in that also
//! works with a mouse.
//!
//! # There is one menu per state and never two
//!
//! [`MenuKind::of`] is a total function of `(paused, RenderState)` and returns
//! one variant. That is what
//! [`tests::a_paused_game_shows_the_pause_menu_and_nothing_else`] pins: a paused
//! frame that also drew the start menu would be two panels stacked on the same
//! centre.

use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::game::{BRICK_COUNT, GameState, RenderState, STARTING_LIVES};

/// What only breakout's menus do.
///
/// Resume, fullscreen and the debug panel are the *loop's* — they are the menu
/// equivalents of its three reserved keys — and live in
/// [`crcbl::engine::MenuAction`]. This is the fourth, and it is an action
/// rather than a key because a button that "presses Space" would be a menu
/// re-entering its own input path.
///
/// Serving is nonetheless delivered to the simulation *as* a key, in
/// [`crate::app::Breakout::apply`]: launching the ball is the simulation's
/// business and the simulation is driven by its action map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Serve the ball, or start the next game — `game.rs`'s launch action.
    Ball,
}

/// The [`WidgetId`] carrying [`Launch::Ball`].
///
/// Numbered from [`crcbl::engine::FIRST_GAME_ID`], not from one: everything
/// below that is the loop's, and a button that claimed
/// [`crcbl::engine::RESUME_ID`] would un-pause instead of serving.
pub const LAUNCH_ID: WidgetId = crcbl::engine::FIRST_GAME_ID;

/// The half of the id mapping that is breakout's, for
/// [`crcbl::engine::MenuAction::from_id`]. Never asked about a reserved id.
#[must_use]
pub const fn launch_from_id(id: WidgetId) -> Option<Launch> {
    if id == LAUNCH_ID {
        Some(Launch::Ball)
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
    /// The game is being played: no menu at all.
    #[default]
    None,
    /// A fresh game, waiting to be served.
    Start,
    /// The loop has stopped advancing the simulation.
    Paused,
    /// Every brick is gone.
    Won,
    /// No lives left.
    Lost,
}

impl MenuKind {
    /// The menu this frame shows.
    ///
    /// **Pause wins over everything**, for the same reason
    /// `HudStrings::refresh` lets it win over the simulation's own state: the
    /// server is still where it was, and a player who alt-tabbed away wants to
    /// be told the game is stopped rather than what the ball was doing.
    ///
    /// The start menu is for a **fresh** game only — `WaitingForLaunch` is also
    /// where a player who has just lost a life waits, and a modal panel between
    /// every life would be three panels a game rather than one at the start.
    /// Full lives, a full grid and a score of zero is what "fresh" means, and
    /// all three are needed: a score can be zero several bricks in, and the
    /// first life can be lost at score zero with the grid still full — which is
    /// exactly the corner a "full grid and a score of zero" check lets through.
    #[must_use]
    pub fn of(paused: bool, render: &RenderState) -> Self {
        if paused {
            return Self::Paused;
        }
        match render.state {
            Some(GameState::Won) => Self::Won,
            Some(GameState::Lost) => Self::Lost,
            Some(GameState::WaitingForLaunch) | None
                if render.score == 0
                    && render.lives == STARTING_LIVES
                    && render.bricks.len() == BRICK_COUNT =>
            {
                Self::Start
            }
            _ => Self::None,
        }
    }
}

/// Breakout's menus, keyed by the state each belongs to.
///
/// The container is [`crcbl::ui::menu::MenuSet`] — shared with every other
/// sample, because holding a handful of panels and switching between them
/// without carrying a half-finished click across is not something a game should
/// own. [`MenuKind::None`] has no entry in it, which is how a playing frame is
/// told to draw nothing.
pub type Menus = MenuSet<MenuKind>;

/// The four menus, with nothing shown.
#[must_use]
pub fn menus() -> Menus {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    MenuSet::new(
        MenuKind::None,
        vec![
            (
                MenuKind::Start,
                Menu::new(
                    "BREAKOUT",
                    vec![
                        item(LAUNCH_ID, "PLAY", "SPACE"),
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
                MenuKind::Won,
                Menu::new(
                    "YOU WIN",
                    vec![
                        item(LAUNCH_ID, "PLAY AGAIN", "SPACE"),
                        item(FULLSCREEN_ID, "FULLSCREEN", "F11"),
                    ],
                ),
            ),
            (
                MenuKind::Lost,
                Menu::new(
                    "GAME OVER",
                    vec![
                        item(LAUNCH_ID, "PLAY AGAIN", "SPACE"),
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

    /// Breakout's whole menu vocabulary: the loop's three, plus [`Launch`].
    ///
    /// Only the tests name it. `app.rs` never sees a whole `MenuAction` — the
    /// loop keeps its own three and hands `Breakout::apply` the [`Launch`] it
    /// could not answer itself.
    type MenuAction = crcbl::engine::MenuAction<Launch>;
    use crcbl::math::{DVec3, Vec2};
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
            .and_then(|id| MenuAction::from_id(id, launch_from_id))
    }

    /// The action a frame of pointer input fired, if any.
    fn point(menus: &mut Menus, extent: (u32, u32), pointer: PointerInput) -> Option<MenuAction> {
        menus
            .point(extent, &FontAtlas::built_in(), pointer)
            .and_then(|id| MenuAction::from_id(id, launch_from_id))
    }

    /// A render state for a game that is `state`, with `broken` bricks gone.
    ///
    /// A fresh game: full lives. The corner cases say when that is not true.
    fn render(state: Option<GameState>, score: u32, broken: usize) -> RenderState {
        RenderState {
            state,
            score,
            lives: STARTING_LIVES,
            bricks: vec![DVec3::ZERO; BRICK_COUNT - broken],
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
            Some(GameState::WaitingForLaunch),
            Some(GameState::Playing),
            Some(GameState::Won),
            Some(GameState::Lost),
        ] {
            assert_eq!(
                MenuKind::of(true, &render(state, 0, 0)),
                MenuKind::Paused,
                "paused over {state:?} did not show the pause menu",
            );
        }

        // And the container really draws one menu: `current` is a single
        // `Option`, so "and nothing else" is structural — but the titles must
        // differ, or two kinds could draw the same panel and nobody would know.
        let mut menus = menus();
        let mut titles = Vec::new();
        for kind in [
            MenuKind::Start,
            MenuKind::Paused,
            MenuKind::Won,
            MenuKind::Lost,
        ] {
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
        assert_eq!(titles.len(), 4, "two menus share a title");

        menus.show(MenuKind::None);
        assert!(menus.current().is_none(), "a playing frame draws no menu");
    }

    /// Every other state maps to the menu that belongs to it, and a game in
    /// progress gets none.
    #[test]
    fn each_state_shows_the_menu_that_belongs_to_it() {
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::WaitingForLaunch), 0, 0)),
            MenuKind::Start,
        );
        assert_eq!(MenuKind::of(false, &render(None, 0, 0)), MenuKind::Start);
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::Playing), 0, 0)),
            MenuKind::None,
        );
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::Won), 400, 40)),
            MenuKind::Won,
        );
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::Lost), 120, 12)),
            MenuKind::Lost,
        );
    }

    /// **A life lost mid-game does not put a panel over the court.**
    ///
    /// `WaitingForLaunch` is where a player waits after losing a life as well as
    /// at the start, and a modal between every life would be three panels a game.
    /// Both halves of "fresh" are checked, because either alone lets one through.
    #[test]
    fn waiting_to_serve_mid_game_shows_no_menu() {
        // Bricks broken, score still zero — reachable, because a brick that is
        // hit on the tick a life is lost scores nothing yet.
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::WaitingForLaunch), 0, 3)),
            MenuKind::None,
        );
        // Score on the board, grid somehow full.
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::WaitingForLaunch), 70, 0)),
            MenuKind::None,
        );
        // **The corner the old "fresh" test missed**: score 0, grid full, but
        // one life already lost — exactly the state the first life lost at
        // score 0 leaves behind. This is the bug: a full grid at score zero
        // used to mean "never started" even when it meant "one life down".
        assert_eq!(
            MenuKind::of(
                false,
                &RenderState {
                    lives: STARTING_LIVES - 1,
                    ..render(Some(GameState::WaitingForLaunch), 0, 0)
                },
            ),
            MenuKind::None,
        );
    }

    /// Every button carries an action the loop can act on, and no two buttons in
    /// one menu carry the same one.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = menus();
        for kind in [
            MenuKind::Start,
            MenuKind::Paused,
            MenuKind::Won,
            MenuKind::Lost,
        ] {
            menus.show(kind);
            let menu = menus.current().expect("a menu");
            let actions: Vec<MenuAction> = menu
                .items()
                .iter()
                .map(|item| {
                    MenuAction::from_id(item.id, launch_from_id)
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
            // And each button prints the key that does the same thing, so the
            // keyboard is documented rather than replaced.
            for item in menu.items() {
                assert!(!item.hint.is_empty(), "{kind:?}: {} has no key", item.label);
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
        // Wraps.
        menus.select_next();
        assert_eq!(activate(&mut menus), Some(MenuAction::Resume));
        menus.select_previous();
        assert_eq!(activate(&mut menus), Some(MenuAction::DebugOverlay));

        // A frame with no menu has nothing to activate — the loop's Enter key
        // must not fire the menu it is not showing.
        menus.show(MenuKind::None);
        assert_eq!(activate(&mut menus), None);
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
            ButtonState::Hovered
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
        let target = layout.items()[2];
        let over = (target.min + target.max) * 0.5;

        let down = PointerInput {
            pos: over,
            down: true,
            released: false,
        };
        assert_eq!(point(&mut menus, extent, down), None);
        assert_eq!(
            menus.current().expect("a menu").state(2),
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
            Some(MenuAction::DebugOverlay),
        );

        // A click in the corner of the screen fires nothing.
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
