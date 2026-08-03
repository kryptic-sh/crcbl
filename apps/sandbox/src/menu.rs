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
//! the keyboard model are `crcbl::ui::menu` and `crcbl::render::menu`, the same
//! three keys navigate it, and the same [`MenuAction`] shape turns a button into
//! an effect. `docs/plan/sample/00-samples-overview.md`'s rule 4 is why the
//! sandbox has a debug overlay at all, and it is the same reason it has this: a
//! sample that cannot show the engine's menu is a finding about the menu.
//!
//! # The container is the engine's, keyed by a `bool`
//!
//! [`crcbl::ui::menu::MenuSet`] holds a game's menus keyed by whatever type
//! names its states. The other samples key theirs by a `MenuKind` enum because
//! they have several panels; this one has exactly one, so its key is `bool` and
//! `false` is the state with no menu in the set — which is what makes every
//! method on the set a no-op while the sandbox is running.

use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

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

/// The sandbox's menus, keyed by whether it is paused.
pub type Menus = MenuSet<bool>;

/// The pause menu, not shown.
///
/// `false` — the running sandbox — has no entry, which is how the set is told
/// that a running frame draws no menu.
#[must_use]
pub fn menus() -> Menus {
    use MenuAction::{DebugOverlay, Fullscreen, Resume};
    MenuSet::new(
        false,
        vec![(
            true,
            Menu::new(
                "PAUSED",
                vec![
                    MenuItem::new(Resume.id(), "RESUME", "ESC"),
                    MenuItem::new(Fullscreen.id(), "FULLSCREEN", "F11"),
                    MenuItem::new(DebugOverlay.id(), "DEBUG PANEL", "F3"),
                ],
            ),
        )],
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

    /// **A running sandbox shows no menu, and a paused one shows exactly the
    /// pause menu.** The sandbox's whole state machine, and the reason it has no
    /// start or game-over panel.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = menus();
        assert!(menus.current().is_none());
        assert!(!menus.is_showing());
        assert_eq!(activate(&mut menus), None, "nothing to fire");

        menus.show(true);
        assert_eq!(menus.current().expect("the pause menu").title, "PAUSED");

        menus.show(false);
        assert!(menus.current().is_none());
    }

    /// Every button carries an action the loop can act on, no two carry the same
    /// one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = menus();
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
        let mut menus = menus();
        menus.show(true);
        assert_eq!(activate(&mut menus), Some(MenuAction::Resume));
        menus.select_next();
        assert_eq!(activate(&mut menus), Some(MenuAction::Fullscreen));
        menus.select_next();
        assert_eq!(activate(&mut menus), Some(MenuAction::DebugOverlay));
        menus.select_next();
        assert_eq!(activate(&mut menus), Some(MenuAction::Resume), "it wraps");
        menus.select_previous();
        assert_eq!(activate(&mut menus), Some(MenuAction::DebugOverlay));
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = menus();
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
        let mut menus = menus();
        menus.show(true);

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
        menus.show(true);
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

        menus.show(false);
        menus.show(true);
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
