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
//! The art, the layout and the keyboard model are `crcbl_ui::menu` and
//! `crcbl_render::menu` — shared with flappy and the sandbox, because a window
//! frame is not something a game should own. What is here is the part that
//! genuinely is breakout's: **which menu a frame of this game shows, and what
//! happens when a button is fired.**
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

use crcbl::ui::menu::{Menu, MenuItem};
use crcbl::ui::{PointerInput, UiState, WidgetId};

use crate::game::{BRICK_COUNT, GameState, RenderState};

/// What firing a menu button asks the loop to do.
///
/// An action rather than a key: a button that "presses Space" would be a menu
/// re-entering its own input path, and the loop would have to tell a synthesised
/// key from a real one. The loop matches on this and does the thing directly —
/// except [`MenuAction::Launch`], which really is a key, because launching the
/// ball is the *simulation's* business and the simulation is driven by its
/// action map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Un-pause.
    Resume,
    /// Serve the ball, or start the next game — `game.rs`'s launch action.
    Launch,
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
            Self::Launch => 2,
            Self::Fullscreen => 3,
            Self::DebugOverlay => 4,
        }
    }

    /// The action an id names, or `None` for an id from another menu system.
    #[must_use]
    pub const fn from_id(id: WidgetId) -> Option<Self> {
        match id {
            1 => Some(Self::Resume),
            2 => Some(Self::Launch),
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
    /// every life would be three panels a game rather than one at the start. A
    /// full grid and a score of zero is what "fresh" means, and both are needed:
    /// a score can be zero several bricks in.
    #[must_use]
    pub fn of(paused: bool, render: &RenderState) -> Self {
        if paused {
            return Self::Paused;
        }
        match render.state {
            Some(GameState::Won) => Self::Won,
            Some(GameState::Lost) => Self::Lost,
            Some(GameState::WaitingForLaunch) | None
                if render.score == 0 && render.bricks.len() == BRICK_COUNT =>
            {
                Self::Start
            }
            _ => Self::None,
        }
    }
}

/// Breakout's four menus, the one being shown, and the pointer's capture.
#[derive(Debug)]
pub struct Menus {
    start: Menu,
    paused: Menu,
    won: Menu,
    lost: Menu,
    shown: MenuKind,
    ui: UiState,
}

impl Default for Menus {
    fn default() -> Self {
        Self::new()
    }
}

impl Menus {
    /// The four menus, with nothing shown.
    #[must_use]
    pub fn new() -> Self {
        use MenuAction::{DebugOverlay, Fullscreen, Launch, Resume};
        Self {
            start: Menu::new(
                "BREAKOUT",
                vec![
                    item(Launch, "PLAY", "SPACE"),
                    item(Fullscreen, "FULLSCREEN", "F11"),
                    item(DebugOverlay, "DEBUG PANEL", "F3"),
                ],
            ),
            paused: Menu::new(
                "PAUSED",
                vec![
                    item(Resume, "RESUME", "ESC"),
                    item(Fullscreen, "FULLSCREEN", "F11"),
                    item(DebugOverlay, "DEBUG PANEL", "F3"),
                ],
            ),
            won: Menu::new(
                "YOU WIN",
                vec![
                    item(Launch, "PLAY AGAIN", "SPACE"),
                    item(Fullscreen, "FULLSCREEN", "F11"),
                ],
            ),
            lost: Menu::new(
                "GAME OVER",
                vec![
                    item(Launch, "PLAY AGAIN", "SPACE"),
                    item(Fullscreen, "FULLSCREEN", "F11"),
                ],
            ),
            shown: MenuKind::None,
            ui: UiState::new(),
        }
    }

    /// Switches to the menu this frame shows.
    ///
    /// A change drops the previous menu's hover and held key: a menu re-shown
    /// with a stale press on it draws a button nobody is touching, and a capture
    /// left in [`UiState`] would credit the next click to a widget that is no
    /// longer on screen.
    pub fn show(&mut self, kind: MenuKind) {
        if kind == self.shown {
            return;
        }
        if let Some(menu) = self.current_mut() {
            menu.clear_input();
        }
        self.ui.clear();
        self.shown = kind;
    }

    /// Which menu is being shown.
    #[must_use]
    pub const fn kind(&self) -> MenuKind {
        self.shown
    }

    /// The menu being shown, or `None` on a frame with no menu on it.
    #[must_use]
    pub const fn current(&self) -> Option<&Menu> {
        match self.shown {
            MenuKind::None => None,
            MenuKind::Start => Some(&self.start),
            MenuKind::Paused => Some(&self.paused),
            MenuKind::Won => Some(&self.won),
            MenuKind::Lost => Some(&self.lost),
        }
    }

    /// The menu being shown, mutably.
    pub const fn current_mut(&mut self) -> Option<&mut Menu> {
        match self.shown {
            MenuKind::None => None,
            MenuKind::Start => Some(&mut self.start),
            MenuKind::Paused => Some(&mut self.paused),
            MenuKind::Won => Some(&mut self.won),
            MenuKind::Lost => Some(&mut self.lost),
        }
    }

    /// Moves the selection down, if there is a menu.
    pub fn select_next(&mut self) {
        if let Some(menu) = self.current_mut() {
            menu.select_next();
        }
    }

    /// Moves the selection up, if there is a menu.
    pub fn select_previous(&mut self) {
        if let Some(menu) = self.current_mut() {
            menu.select_previous();
        }
    }

    /// Holds the highlighted button down, or lets it up.
    pub fn press(&mut self, down: bool) {
        if let Some(menu) = self.current_mut() {
            menu.press(down);
        }
    }

    /// Fires the highlighted button.
    pub fn activate(&mut self) -> Option<MenuAction> {
        self.current_mut()
            .and_then(Menu::activate)
            .and_then(MenuAction::from_id)
    }

    /// Runs one frame of pointer input against the menu on screen.
    ///
    /// The layout is recomputed here rather than kept, because it depends on the
    /// framebuffer's size and on the menu's own contents and both can change
    /// between frames — and a hit test against last frame's rectangles is how a
    /// resized window gets buttons that are not where they are drawn.
    pub fn point(
        &mut self,
        extent: (u32, u32),
        atlas: &crcbl::ui::text::FontAtlas,
        pointer: PointerInput,
    ) -> Option<MenuAction> {
        // The menu and the capture are borrowed as **separate fields**: a
        // `current_mut()` here would borrow the whole struct and `self.ui` with
        // it, which is the one place this container's shape shows through.
        let menu = match self.shown {
            MenuKind::None => return None,
            MenuKind::Start => &mut self.start,
            MenuKind::Paused => &mut self.paused,
            MenuKind::Won => &mut self.won,
            MenuKind::Lost => &mut self.lost,
        };
        let layout = menu.layout(extent, atlas);
        menu.point(&layout, &mut self.ui, pointer)
            .and_then(MenuAction::from_id)
    }
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::ui::ButtonState;
    use crcbl::ui::text::FontAtlas;
    use glam::{DVec3, Vec2};

    /// A render state for a game that is `state`, with `broken` bricks gone.
    fn render(state: Option<GameState>, score: u32, broken: usize) -> RenderState {
        RenderState {
            state,
            score,
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
        let mut menus = Menus::new();
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
    }

    /// Every button carries an action the loop can act on, and no two buttons in
    /// one menu carry the same one.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = Menus::new();
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
        let mut menus = Menus::new();
        menus.show(MenuKind::Paused);
        assert_eq!(menus.activate(), Some(MenuAction::Resume));

        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::Fullscreen));
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::DebugOverlay));
        // Wraps.
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::Resume));
        menus.select_previous();
        assert_eq!(menus.activate(), Some(MenuAction::DebugOverlay));

        // A frame with no menu has nothing to activate — the loop's Enter key
        // must not fire the menu it is not showing.
        menus.show(MenuKind::None);
        assert_eq!(menus.activate(), None);
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = Menus::new();
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
        let mut menus = Menus::new();
        menus.show(MenuKind::Paused);

        let layout = menus.current().expect("a menu").layout(extent, &atlas);
        let target = layout.items()[2];
        let over = (target.min + target.max) * 0.5;

        let down = PointerInput {
            pos: over,
            down: true,
            released: false,
        };
        assert_eq!(menus.point(extent, &atlas, down), None);
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
            menus.point(extent, &atlas, up),
            Some(MenuAction::DebugOverlay),
        );

        // A click in the corner of the screen fires nothing.
        let corner = PointerInput {
            pos: Vec2::new(3.0, 3.0),
            down: true,
            released: false,
        };
        assert_eq!(menus.point(extent, &atlas, corner), None);
        let corner_up = PointerInput {
            pos: Vec2::new(3.0, 3.0),
            down: false,
            released: true,
        };
        assert_eq!(menus.point(extent, &atlas, corner_up), None);
    }

    /// Switching menus drops the previous one's press capture, so a click that
    /// started on the pause menu cannot land on the start menu's button in the
    /// same place.
    #[test]
    fn switching_menus_drops_the_press() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = Menus::new();
        menus.show(MenuKind::Paused);
        let layout = menus.current().expect("a menu").layout(extent, &atlas);
        let over = (layout.items()[0].min + layout.items()[0].max) * 0.5;
        menus.point(
            extent,
            &atlas,
            PointerInput {
                pos: over,
                down: true,
                released: false,
            },
        );
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Pressed
        );

        menus.show(MenuKind::Start);
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Hovered,
            "the new menu inherited a press nobody is making",
        );
        assert_eq!(
            menus.point(
                extent,
                &atlas,
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
