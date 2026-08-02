//! Horde's menus: which one belongs to which state, and what its buttons do.
//!
//! ```text
//!   paused ──────────────────────────────────────────────► Paused
//!   Dead ────────────────────────────────────────────────► GameOver
//!   LevelUp ─────────────────────────────────────────────► LevelUp
//!   WaitingToStart ──────────────────────────────────────► Start
//!   Playing ─────────────────────────────────────────────► none
//! ```
//!
//! The art, the layout and the keyboard model are `crcbl_ui::menu` and
//! `crcbl_render::menu` — shared with breakout, flappy, asteroids and the
//! sandbox, because a window frame is not something a game should own. What is
//! here is the part that genuinely is this game's: **which menu a frame shows,
//! and what happens when a button is fired.**
//!
//! **This is the fifth copy of this file**, after `apps/breakout/src/menu.rs`,
//! `apps/flappy/src/menu.rs`, `apps/sandbox/src/menu.rs` and
//! `apps/asteroids/src/menu.rs`. The `MenuAction` enum, its `WidgetId`
//! discriminants, the `MenuKind::of` precedence rule and the whole of `Menus`
//! are the same five times over; only the titles, the labels and the state
//! mapping differ. `docs/backlog.md` carries it.
//!
//! # There is a start menu, and it was argued against before it was built
//!
//! The first cut of this game had none: the other samples open on a *board*
//! worth looking at and this one's is empty at `t = 0`, so a start screen here
//! is a blank arena with a prompt on it. The user played it and asked for the
//! screen; `game::GameState`'s docs carry the whole of the reversal. What it
//! costs is the odd shape of the button — `PLAY` and `TRY AGAIN` are the same
//! [`MenuAction::Restart`], because the simulation has one edge for "begin a
//! run" and the death screen's button lands on the start screen rather than in
//! play.
//!
//! # The level-up menu is the one that is rebuilt
//!
//! Every other menu in every other sample is built once at start-up, because its
//! buttons never change. This one's are three upgrades drawn from a seed, so
//! [`Menus::set_offer`] rebuilds it when the offer changes — once per level-up,
//! never per frame, and guarded by comparing the offer it was last built from.
//!
//! # What the menu takes from the keyboard, and the one thing it shadows
//!
//! Up, Down and Enter, and only while a menu is on screen. Two of the three are
//! free — `game.rs` binds the arrows, WASD, Space, R and the digits — and
//! **ArrowUp is not**: it is the second binding of the "up" movement action,
//! beside `KeyW`.
//!
//! So while a panel is up, ArrowUp moves the selection instead of walking north.
//! That is the same trade flappy and asteroids made and for the same two
//! reasons: `KeyW` still walks and is shadowed on no menu, so nothing becomes
//! unreachable; and the three keys are the same three in every sample, which is
//! the rule F3, Escape and F11 already follow.
//!
//! It costs less here than in either: a menu is on screen only when the game is
//! **not** being played — `MenuKind::None` is every unpaused `Playing` frame —
//! and the level-up screen freezes the field anyway, so there is nothing to walk
//! away from while it is up.

use crcbl::ui::menu::{Menu, MenuItem};
use crcbl::ui::{PointerInput, UiState, WidgetId};

use crate::game::{GameState, RenderState, UPGRADE_CHOICES, Upgrade};

/// What firing a menu button asks the loop to do.
///
/// An action rather than a key: a button that "presses Escape" would be a menu
/// re-entering its own input path, and the loop would have to tell a synthesised
/// key from a real one. The loop matches on this and does the thing directly —
/// except [`MenuAction::Restart`] and the three [`MenuAction::Choose`]s, which
/// really are keys, because restarting a run and taking an upgrade are the
/// *simulation's* business and the simulation is driven by its action map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Un-pause.
    Resume,
    /// Begin a run — the one edge `run_tick` reads for both `PLAY` on the start
    /// screen and `TRY AGAIN` on the death screen.
    Restart,
    /// Take the `n`-th upgrade of the current offer, zero-based.
    Choose(usize),
    /// Toggle borderless fullscreen.
    Fullscreen,
    /// Toggle the debug panel.
    DebugOverlay,
}

impl MenuAction {
    /// The [`WidgetId`] this action is carried by.
    ///
    /// Written out rather than derived, because a `WidgetId` that changed when a
    /// variant was inserted would silently re-point every button. The choices
    /// take a block of three from 10, so a fourth non-choice action can be added
    /// without moving them.
    const fn id(self) -> WidgetId {
        match self {
            Self::Resume => 1,
            Self::Restart => 2,
            Self::Fullscreen => 3,
            Self::DebugOverlay => 4,
            Self::Choose(index) => 10 + index as WidgetId,
        }
    }

    /// The action an id names, or `None` for an id from another menu system.
    #[must_use]
    pub const fn from_id(id: WidgetId) -> Option<Self> {
        match id {
            1 => Some(Self::Resume),
            2 => Some(Self::Restart),
            3 => Some(Self::Fullscreen),
            4 => Some(Self::DebugOverlay),
            10 => Some(Self::Choose(0)),
            11 => Some(Self::Choose(1)),
            12 => Some(Self::Choose(2)),
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
    /// The arena is empty and the run has not begun.
    Start,
    /// The loop has stopped advancing the simulation.
    Paused,
    /// The simulation has stopped itself, waiting for one of three.
    LevelUp,
    /// Out of hit points.
    GameOver,
}

impl MenuKind {
    /// The menu this frame shows.
    ///
    /// **Pause wins over everything**, for the same reason `HudStrings::refresh`
    /// lets it win over the simulation's own state: the server is still running,
    /// and a player whose window lost focus wants to be told the game is stopped
    /// rather than what the horde was doing. A level-up that opened while the
    /// window was in the background is still there when it comes back.
    #[must_use]
    pub const fn of(paused: bool, render: &RenderState) -> Self {
        if paused {
            return Self::Paused;
        }
        match render.state {
            Some(GameState::Dead) => Self::GameOver,
            Some(GameState::LevelUp) => Self::LevelUp,
            // `None` is a frame drawn before the first `render_state`, which is
            // the title screen and not a run in progress — the same mapping
            // asteroids makes.
            Some(GameState::WaitingToStart) | None => Self::Start,
            Some(GameState::Playing) => Self::None,
        }
    }
}

/// Horde's four menus, the one being shown, and the pointer's capture.
#[derive(Debug)]
pub struct Menus {
    start: Menu,
    paused: Menu,
    level_up: Menu,
    game_over: Menu,
    /// The offer `level_up` was last built from, so a frame with an unchanged
    /// offer rebuilds nothing.
    built_from: Option<(u32, [Upgrade; UPGRADE_CHOICES])>,
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
        use MenuAction::{DebugOverlay, Fullscreen, Restart, Resume};
        Self {
            // `SPACE`, which is what breakout, flappy and asteroids print on
            // theirs — a player moving between the demos presses one key. `R` is
            // bound to the same action and still works; the death screen prints
            // that one, because it always has.
            start: Menu::new(
                "HORDE",
                vec![
                    item(Restart, "PLAY", "SPACE"),
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
            // Placeholder buttons, replaced by `set_offer` before this is ever
            // shown. Built here rather than as an `Option` so `current` stays a
            // total function and a menu is never missing from a frame that maps
            // to it.
            level_up: Self::offer_menu(1, &[Upgrade::ALL[0], Upgrade::ALL[1], Upgrade::ALL[2]]),
            game_over: Menu::new(
                "YOU DIED",
                vec![
                    item(Restart, "TRY AGAIN", "R"),
                    item(Fullscreen, "FULLSCREEN", "F11"),
                ],
            ),
            built_from: None,
            shown: MenuKind::None,
            ui: UiState::new(),
        }
    }

    /// The level-up panel for one offer.
    fn offer_menu(level: u32, offer: &[Upgrade; UPGRADE_CHOICES]) -> Menu {
        Menu::new(
            format!("LEVEL {level}"),
            offer
                .iter()
                .enumerate()
                .map(|(index, upgrade)| {
                    item(
                        MenuAction::Choose(index),
                        upgrade.label(),
                        // One-based, because it is the digit key the player
                        // presses and `ACTION_CHOOSE` binds.
                        &(index + 1).to_string(),
                    )
                })
                .collect(),
        )
    }

    /// Rebuilds the level-up panel if the offer has changed.
    ///
    /// Takes what [`RenderState`] carries — `None` on every frame the screen is
    /// not up — so the loop has one call rather than a condition.
    pub fn set_offer(&mut self, level: u32, offer: Option<[Upgrade; UPGRADE_CHOICES]>) {
        let Some(offer) = offer else {
            return;
        };
        if self.built_from == Some((level, offer)) {
            return;
        }
        self.built_from = Some((level, offer));
        self.level_up = Self::offer_menu(level, &offer);
        // A rebuilt menu is a different menu, so the capture and the selection
        // that belonged to the old one must not survive into it — the same
        // argument `show` makes for switching between two.
        self.ui.clear();
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
            MenuKind::LevelUp => Some(&self.level_up),
            MenuKind::GameOver => Some(&self.game_over),
        }
    }

    /// The menu being shown, mutably.
    pub const fn current_mut(&mut self) -> Option<&mut Menu> {
        match self.shown {
            MenuKind::None => None,
            MenuKind::Start => Some(&mut self.start),
            MenuKind::Paused => Some(&mut self.paused),
            MenuKind::LevelUp => Some(&mut self.level_up),
            MenuKind::GameOver => Some(&mut self.game_over),
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
            MenuKind::LevelUp => &mut self.level_up,
            MenuKind::GameOver => &mut self.game_over,
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
    use glam::Vec2;

    fn render(state: Option<GameState>) -> RenderState {
        RenderState {
            state,
            ..RenderState::default()
        }
    }

    const OFFER: [Upgrade; UPGRADE_CHOICES] =
        [Upgrade::Magnet, Upgrade::HeavyBolts, Upgrade::SwiftBoots];

    /// **A paused game shows the pause menu and nothing else.** Pause wins over
    /// every simulation state, including the two that would otherwise have drawn
    /// a panel of their own.
    #[test]
    fn a_paused_game_shows_the_pause_menu_and_nothing_else() {
        for state in [
            None,
            Some(GameState::WaitingToStart),
            Some(GameState::Playing),
            Some(GameState::LevelUp),
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
        let mut menus = Menus::new();
        menus.set_offer(3, Some(OFFER));
        let mut titles = Vec::new();
        for kind in [
            MenuKind::Start,
            MenuKind::Paused,
            MenuKind::LevelUp,
            MenuKind::GameOver,
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

    /// Every other state maps to the menu that belongs to it, and a game being
    /// played gets none.
    #[test]
    fn each_state_shows_the_menu_that_belongs_to_it() {
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::Playing))),
            MenuKind::None,
        );
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::WaitingToStart))),
            MenuKind::Start,
        );
        assert_eq!(MenuKind::of(false, &render(None)), MenuKind::Start);
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::LevelUp))),
            MenuKind::LevelUp,
        );
        assert_eq!(
            MenuKind::of(false, &render(Some(GameState::Dead))),
            MenuKind::GameOver,
        );
    }

    /// Every button carries an action the loop can act on, no two in one menu
    /// carry the same one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = Menus::new();
        menus.set_offer(2, Some(OFFER));
        for kind in [
            MenuKind::Start,
            MenuKind::Paused,
            MenuKind::LevelUp,
            MenuKind::GameOver,
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
            for item in menu.items() {
                assert!(!item.hint.is_empty(), "{kind:?}: {} has no key", item.label);
            }
        }
    }

    /// **The level-up menu is exactly the offer**, in order, titled with the
    /// level, and each button prints the digit that takes it.
    #[test]
    fn the_level_up_menu_is_the_offer_it_was_given() {
        let mut menus = Menus::new();
        menus.set_offer(4, Some(OFFER));
        menus.show(MenuKind::LevelUp);
        let menu = menus.current().expect("a menu");
        assert_eq!(menu.title, "LEVEL 4");
        assert_eq!(menu.items().len(), UPGRADE_CHOICES);
        for (index, upgrade) in OFFER.iter().enumerate() {
            assert_eq!(menu.items()[index].label, upgrade.label());
            assert_eq!(menu.items()[index].hint, (index + 1).to_string());
            assert_eq!(
                MenuAction::from_id(menu.items()[index].id),
                Some(MenuAction::Choose(index)),
            );
        }

        // A second offer replaces it rather than appending to it.
        let next = [Upgrade::Vitality, Upgrade::RapidFire, Upgrade::LongBarrel];
        menus.set_offer(5, Some(next));
        let menu = menus.current().expect("a menu");
        assert_eq!(menu.title, "LEVEL 5");
        assert_eq!(menu.items().len(), UPGRADE_CHOICES);
        assert_eq!(menu.items()[0].label, Upgrade::Vitality.label());
    }

    /// **Keyboard activation works**, on every menu, and reports the action the
    /// selected button carries.
    #[test]
    fn the_keyboard_selects_and_activates() {
        let mut menus = Menus::new();
        menus.set_offer(1, Some(OFFER));
        menus.show(MenuKind::Paused);
        assert_eq!(menus.activate(), Some(MenuAction::Resume));
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::Fullscreen));
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::DebugOverlay));
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::Resume), "it wraps");
        menus.select_previous();
        assert_eq!(menus.activate(), Some(MenuAction::DebugOverlay));

        menus.show(MenuKind::LevelUp);
        assert_eq!(menus.activate(), Some(MenuAction::Choose(0)));
        menus.select_next();
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::Choose(2)));

        menus.show(MenuKind::None);
        assert_eq!(menus.activate(), None, "there is no menu to activate");
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = Menus::new();
        menus.show(MenuKind::Paused);
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
        let mut menus = Menus::new();
        menus.set_offer(2, Some(OFFER));
        menus.show(MenuKind::LevelUp);

        let layout = menus.current().expect("a menu").layout(extent, &atlas);
        let target = layout.items()[1];
        let over = (target.min + target.max) * 0.5;

        let down = PointerInput {
            pos: over,
            down: true,
            released: false,
        };
        assert_eq!(menus.point(extent, &atlas, down), None);
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
        assert_eq!(
            menus.point(extent, &atlas, up),
            Some(MenuAction::Choose(1)),
            "the pointer took the wrong upgrade",
        );

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
    /// started on the pause menu cannot land on the level-up menu's button in
    /// the same place.
    #[test]
    fn switching_menus_drops_the_press() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = Menus::new();
        menus.set_offer(1, Some(OFFER));
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
            ButtonState::Pressed,
        );

        menus.show(MenuKind::LevelUp);
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

    /// A rebuilt level-up panel drops the capture too — the press was on a
    /// button that no longer says what it said.
    #[test]
    fn a_new_offer_drops_the_press() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = Menus::new();
        menus.set_offer(1, Some(OFFER));
        menus.show(MenuKind::LevelUp);
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

        menus.set_offer(
            2,
            Some([Upgrade::Vitality, Upgrade::Magnet, Upgrade::RapidFire]),
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
            "a release took an upgrade the player never pressed",
        );
    }
}
