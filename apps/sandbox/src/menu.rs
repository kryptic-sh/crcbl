//! The sandbox's menu: the pause panel, and nothing else.
//!
//! **Smaller than breakout's and flappy's on purpose.** The sandbox is a
//! milestone harness, not a game: there is no run to start, no score to lose and
//! nothing to win, so a `GAME OVER` panel would be a screen it could never show
//! and a `START` panel would be a door with no room behind it. What it does have
//! is a pause — the loop declining to advance a spinning cube — and that is the
//! one state a menu belongs to.
//!
//! Everything else is shared: the window frame, the button skin, the layout and
//! the keyboard model are `crcbl_ui::menu` and `crcbl_render::menu`, the same
//! three keys navigate it, and the same [`MenuAction`] shape turns a button into
//! an effect. `docs/plan/sample/00-samples-overview.md`'s rule 4 is why the
//! sandbox has a debug overlay at all, and it is the same reason it has this: a
//! sample that cannot show the engine's menu is a finding about the menu.

use crcbl::ui::menu::{Menu, MenuItem};
use crcbl::ui::{PointerInput, UiState, WidgetId};

/// What firing a menu button asks the loop to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Un-pause.
    Resume,
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
            Self::Fullscreen => 3,
            Self::DebugOverlay => 4,
        }
    }

    /// The action an id names, or `None` for an id from another menu system.
    ///
    /// The gap at 2 is deliberate: breakout's and flappy's `Launch`/`Flap` sit
    /// there, and leaving the number free is what keeps the three samples' ids
    /// readable side by side.
    #[must_use]
    pub const fn from_id(id: WidgetId) -> Option<Self> {
        match id {
            1 => Some(Self::Resume),
            3 => Some(Self::Fullscreen),
            4 => Some(Self::DebugOverlay),
            _ => None,
        }
    }
}

/// The sandbox's one menu, and the pointer's capture.
#[derive(Debug)]
pub struct Menus {
    paused: Menu,
    shown: bool,
    ui: UiState,
}

impl Default for Menus {
    fn default() -> Self {
        Self::new()
    }
}

impl Menus {
    /// The pause menu, not shown.
    #[must_use]
    pub fn new() -> Self {
        use MenuAction::{DebugOverlay, Fullscreen, Resume};
        Self {
            paused: Menu::new(
                "PAUSED",
                vec![
                    MenuItem::new(Resume.id(), "RESUME", "ESC"),
                    MenuItem::new(Fullscreen.id(), "FULLSCREEN", "F11"),
                    MenuItem::new(DebugOverlay.id(), "DEBUG PANEL", "F3"),
                ],
            ),
            shown: false,
            ui: UiState::new(),
        }
    }

    /// Shows the menu, or takes it away.
    ///
    /// A change drops the hover and the held key: a menu re-shown with a stale
    /// press on it draws a button nobody is touching, and a capture left in
    /// [`UiState`] would credit the next click to a widget that was on screen
    /// two pauses ago.
    pub fn show(&mut self, shown: bool) {
        if shown == self.shown {
            return;
        }
        self.paused.clear_input();
        self.ui.clear();
        self.shown = shown;
    }

    /// Whether a menu is on screen.
    #[must_use]
    pub const fn is_showing(&self) -> bool {
        self.shown
    }

    /// The menu being shown, or `None`.
    #[must_use]
    pub const fn current(&self) -> Option<&Menu> {
        if self.shown { Some(&self.paused) } else { None }
    }

    /// Moves the selection down, if there is a menu.
    pub fn select_next(&mut self) {
        if self.shown {
            self.paused.select_next();
        }
    }

    /// Moves the selection up, if there is a menu.
    pub fn select_previous(&mut self) {
        if self.shown {
            self.paused.select_previous();
        }
    }

    /// Holds the highlighted button down, or lets it up.
    pub const fn press(&mut self, down: bool) {
        if self.shown {
            self.paused.press(down);
        }
    }

    /// Fires the highlighted button.
    pub fn activate(&mut self) -> Option<MenuAction> {
        if !self.shown {
            return None;
        }
        self.paused.activate().and_then(MenuAction::from_id)
    }

    /// Runs one frame of pointer input against the menu on screen.
    pub fn point(
        &mut self,
        extent: (u32, u32),
        atlas: &crcbl::ui::text::FontAtlas,
        pointer: PointerInput,
    ) -> Option<MenuAction> {
        if !self.shown {
            return None;
        }
        let layout = self.paused.layout(extent, atlas);
        self.paused
            .point(&layout, &mut self.ui, pointer)
            .and_then(MenuAction::from_id)
    }
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::math::Vec2;
    use crcbl::ui::ButtonState;
    use crcbl::ui::text::FontAtlas;

    /// **A running sandbox shows no menu, and a paused one shows exactly the
    /// pause menu.** The sandbox's whole state machine, and the reason it has no
    /// start or game-over panel.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = Menus::new();
        assert!(menus.current().is_none());
        assert_eq!(menus.activate(), None, "nothing to fire");

        menus.show(true);
        assert_eq!(menus.current().expect("the pause menu").title, "PAUSED");

        menus.show(false);
        assert!(menus.current().is_none());
    }

    /// Every button carries an action the loop can act on, no two carry the same
    /// one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = Menus::new();
        menus.show(true);
        let menu = menus.current().expect("the pause menu");
        let actions: Vec<MenuAction> = menu
            .items()
            .iter()
            .map(|item| {
                MenuAction::from_id(item.id)
                    .unwrap_or_else(|| panic!("{} names no action", item.label))
            })
            .collect();
        assert_eq!(actions.len(), 3);
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

    /// **Keyboard activation works**, and reports the action the selected button
    /// carries.
    #[test]
    fn the_keyboard_selects_and_activates() {
        let mut menus = Menus::new();
        menus.show(true);
        assert_eq!(menus.activate(), Some(MenuAction::Resume));
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::Fullscreen));
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::DebugOverlay));
        menus.select_next();
        assert_eq!(menus.activate(), Some(MenuAction::Resume), "it wraps");
        menus.select_previous();
        assert_eq!(menus.activate(), Some(MenuAction::DebugOverlay));
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = Menus::new();
        menus.show(true);
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
        let mut menus = Menus::new();
        menus.show(true);

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
            Some(MenuAction::Fullscreen),
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

    /// Taking the menu away drops the press capture, so a click that started on
    /// it cannot land after it comes back.
    #[test]
    fn hiding_the_menu_drops_the_press() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = Menus::new();
        menus.show(true);
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

        menus.show(false);
        menus.show(true);
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Hovered,
            "the menu came back with a press nobody is making",
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
            "a release fired a button whose press was on a menu that was gone",
        );
    }
}
