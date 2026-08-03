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
//! The art, the layout and the keyboard model are `crcbl::ui::menu` and
//! `crcbl::render::menu` — shared with breakout, flappy, asteroids and the
//! sandbox, because a window frame is not something a game should own. What is
//! here is the part that genuinely is this game's: **which menu a frame shows,
//! and what happens when a button is fired.**
//!
//! **The container is no longer written here.** `Menus` was the fifth hand-rolled
//! copy of the same struct, after `apps/breakout/src/menu.rs`,
//! `apps/flappy/src/menu.rs`, `apps/sandbox/src/menu.rs` and
//! `apps/asteroids/src/menu.rs`; it is now
//! [`crcbl::ui::menu::MenuSet`], keyed by this game's [`MenuKind`]. What stays
//! per-game is what was always genuinely per-game: the [`MenuAction`] enum and
//! its `WidgetId` discriminants, the [`MenuKind::of`] precedence rule, the
//! titles and the labels.
//!
//! # There is a start menu, and it was argued against before it was built
//!
//! The first cut of this game had none: the other samples open on a *board*
//! worth looking at and this one's is empty at `t = 0`, so a start screen here
//! is a blank arena with a prompt on it. The user played it and asked for the
//! screen; `game::GameState`'s docs carry the whole of the reversal. What it
//! costs is the odd shape of the button — `PLAY` and `TRY AGAIN` are the same
//! [`HordeAction::Restart`], because the simulation has one edge for "begin a
//! run" and the death screen's button lands on the start screen rather than in
//! play.
//!
//! # The level-up menu is the one that is rebuilt
//!
//! Every other menu in every other sample is built once at start-up, because its
//! buttons never change. This one's are three upgrades drawn from a seed, so
//! [`LevelUpOffer::refresh`] rebuilds it when the offer changes — once per level-up,
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

use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::game::{GameState, RenderState, UPGRADE_CHOICES, Upgrade};

/// What only horde's menus do.
///
/// Resume, fullscreen and the debug panel are the *loop's* — they are the menu
/// equivalents of its three reserved keys — and live in
/// [`crcbl::engine::MenuAction`]. These two are what is left, and they are
/// actions rather than keys because a button that "presses Space" would be a
/// menu re-entering its own input path.
///
/// Both are nonetheless delivered to the simulation *as* keys, in
/// `crate::app::Horde`'s `HostedGame::apply`: starting a run and taking an
/// upgrade are the simulation's business and the simulation is driven by its
/// action map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HordeAction {
    /// Begin a run — the one edge `run_tick` reads for both `PLAY` on the start
    /// screen and `TRY AGAIN` on the death screen.
    Restart,
    /// Take the `n`-th upgrade of the current offer, zero-based.
    Choose(usize),
}

/// The [`WidgetId`] carrying [`HordeAction::Restart`].
///
/// Numbered from [`crcbl::engine::FIRST_GAME_ID`], not from one: everything
/// below that is the loop's, and a button that claimed
/// [`crcbl::engine::RESUME_ID`] would un-pause instead of starting a run.
pub const RESTART_ID: WidgetId = crcbl::engine::FIRST_GAME_ID;

/// The first of the three ids [`HordeAction::Choose`] uses.
///
/// A block of its own above [`RESTART_ID`], so a second non-choice action can
/// be added without moving the choices — which is the same reason the old
/// hand-numbered set started them at 10.
pub const FIRST_CHOOSE_ID: WidgetId = crcbl::engine::FIRST_GAME_ID + 8;

/// The [`WidgetId`] carrying the `index`-th choice.
///
/// # Panics
///
/// If `index` is past [`UPGRADE_CHOICES`], because that is a menu built with
/// more slots than the id block reserves and the symptom would be two buttons
/// sharing an id.
#[must_use]
pub fn choose_id(index: usize) -> WidgetId {
    assert!(index < UPGRADE_CHOICES, "no id for upgrade slot {index}");
    FIRST_CHOOSE_ID + index as WidgetId
}

/// The half of the id mapping that is horde's, for
/// [`crcbl::engine::MenuAction::from_id`]. Never asked about a reserved id.
#[must_use]
pub fn action_from_id(id: WidgetId) -> Option<HordeAction> {
    if id == RESTART_ID {
        return Some(HordeAction::Restart);
    }
    let index = id.checked_sub(FIRST_CHOOSE_ID)? as usize;
    (index < UPGRADE_CHOICES).then_some(HordeAction::Choose(index))
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

/// Horde's menus, keyed by the state each belongs to.
///
/// The container is [`crcbl::ui::menu::MenuSet`] — shared with every other
/// sample, because holding a handful of panels and switching between them
/// without carrying a half-finished click across is not something a game should
/// own. [`MenuKind::None`] has no entry in it, which is how a playing frame is
/// told to draw nothing.
pub type Menus = MenuSet<MenuKind>;

/// The four menus, with nothing shown.
///
/// The level-up panel is a placeholder, replaced by [`LevelUpOffer::refresh`]
/// before it is ever shown. Built here rather than left out so `current` stays
/// a total function and a menu is never missing from a frame that maps to it.
#[must_use]
pub fn menus() -> Menus {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    MenuSet::new(
        MenuKind::None,
        vec![
            // `SPACE`, which is what breakout, flappy and asteroids print on
            // theirs — a player moving between the demos presses one key. `R` is
            // bound to the same action and still works; the death screen prints
            // that one, because it always has.
            (
                MenuKind::Start,
                Menu::new(
                    "HORDE",
                    vec![
                        item(RESTART_ID, "PLAY", "SPACE"),
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
                MenuKind::LevelUp,
                offer_menu(1, &[Upgrade::ALL[0], Upgrade::ALL[1], Upgrade::ALL[2]]),
            ),
            (
                MenuKind::GameOver,
                Menu::new(
                    "YOU DIED",
                    vec![
                        item(RESTART_ID, "TRY AGAIN", "R"),
                        item(FULLSCREEN_ID, "FULLSCREEN", "F11"),
                    ],
                ),
            ),
        ],
    )
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
                    choose_id(index),
                    upgrade.label(),
                    // One-based, because it is the digit key the player
                    // presses and `ACTION_CHOOSE` binds.
                    &(index + 1).to_string(),
                )
            })
            .collect(),
    )
}

/// Remembers which offer the level-up panel was built from, so a frame with an
/// unchanged offer rebuilds nothing.
///
/// Every other menu in every sample is built once at start-up, because its
/// buttons never change. This one's are three upgrades drawn from a seed, so it
/// has to be rebuilt — once per level-up, and **not** once per frame, because
/// [`MenuSet::replace`] drops the selection and the pointer's capture with it
/// and a panel rebuilt sixty times a second could never be clicked.
///
/// It is a type of its own rather than a field of [`Menus`] because the set is
/// the engine's and knows nothing about upgrades: what is stale is horde's
/// question, and this is horde's answer to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LevelUpOffer {
    /// The offer the panel was last built from, or `None` before the first.
    built_from: Option<(u32, [Upgrade; UPGRADE_CHOICES])>,
}

impl LevelUpOffer {
    /// Rebuilds the level-up panel if the offer has changed.
    ///
    /// Takes what [`RenderState`] carries — `None` on every frame the screen is
    /// not up — so the loop has one call rather than a condition.
    pub fn refresh(
        &mut self,
        menus: &mut Menus,
        level: u32,
        offer: Option<[Upgrade; UPGRADE_CHOICES]>,
    ) {
        let Some(offer) = offer else {
            return;
        };
        if self.built_from == Some((level, offer)) {
            return;
        }
        self.built_from = Some((level, offer));
        menus.replace(MenuKind::LevelUp, offer_menu(level, &offer));
    }
}
// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Horde's whole menu vocabulary: the loop's three, plus [`HordeAction`].
    ///
    /// Only the tests name it. `app.rs` never sees a whole `MenuAction` — the
    /// loop keeps its own three and hands `Horde::apply` the [`HordeAction`] it
    /// could not answer itself.
    type MenuAction = crcbl::engine::MenuAction<HordeAction>;
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
            .and_then(|id| MenuAction::from_id(id, action_from_id))
    }

    /// The action a frame of pointer input fired, if any.
    fn point(menus: &mut Menus, extent: (u32, u32), pointer: PointerInput) -> Option<MenuAction> {
        menus
            .point(extent, &FontAtlas::built_in(), pointer)
            .and_then(|id| MenuAction::from_id(id, action_from_id))
    }

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
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 3, Some(OFFER));
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
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 2, Some(OFFER));
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
                    MenuAction::from_id(item.id, action_from_id)
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
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 4, Some(OFFER));
        menus.show(MenuKind::LevelUp);
        let menu = menus.current().expect("a menu");
        assert_eq!(menu.title, "LEVEL 4");
        assert_eq!(menu.items().len(), UPGRADE_CHOICES);
        for (index, upgrade) in OFFER.iter().enumerate() {
            assert_eq!(menu.items()[index].label, upgrade.label());
            assert_eq!(menu.items()[index].hint, (index + 1).to_string());
            assert_eq!(
                MenuAction::from_id(menu.items()[index].id, action_from_id),
                Some(MenuAction::Game(HordeAction::Choose(index))),
            );
        }

        // A second offer replaces it rather than appending to it.
        let next = [Upgrade::Vitality, Upgrade::RapidFire, Upgrade::LongBarrel];
        offer.refresh(&mut menus, 5, Some(next));
        let menu = menus.current().expect("a menu");
        assert_eq!(menu.title, "LEVEL 5");
        assert_eq!(menu.items().len(), UPGRADE_CHOICES);
        assert_eq!(menu.items()[0].label, Upgrade::Vitality.label());
    }

    /// **An unchanged offer rebuilds nothing**, which is what makes the
    /// level-up screen usable at all.
    ///
    /// `draw_menu` calls `refresh` every frame, and
    /// [`MenuSet::replace`](crcbl::ui::menu::MenuSet::replace) drops the
    /// selection and the pointer's capture — so a guard that did not hold would
    /// throw the player's highlight away sixty times a second and no upgrade
    /// could ever be taken. The `None` case is the same call on the frames the
    /// screen is not up, which is most of them.
    #[test]
    fn an_unchanged_offer_leaves_the_panel_and_its_selection_alone() {
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 3, Some(OFFER));
        menus.show(MenuKind::LevelUp);
        menus.select_next();
        assert_eq!(
            activate(&mut menus),
            Some(MenuAction::Game(HordeAction::Choose(1)))
        );

        // The frames after it: the same offer, then the frames where the screen
        // is not up at all and `RenderState::offer` is `None`.
        for _ in 0..3 {
            offer.refresh(&mut menus, 3, Some(OFFER));
        }
        offer.refresh(&mut menus, 3, None);
        assert_eq!(
            activate(&mut menus),
            Some(MenuAction::Game(HordeAction::Choose(1))),
            "a redundant refresh threw the selection away",
        );
        assert_eq!(
            menus.current().expect("a menu").title,
            "LEVEL 3",
            "a `None` offer replaced the panel",
        );
    }

    /// **Keyboard activation works**, on every menu, and reports the action the
    /// selected button carries.
    #[test]
    fn the_keyboard_selects_and_activates() {
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 1, Some(OFFER));
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

        menus.show(MenuKind::LevelUp);
        assert_eq!(
            activate(&mut menus),
            Some(MenuAction::Game(HordeAction::Choose(0)))
        );
        menus.select_next();
        menus.select_next();
        assert_eq!(
            activate(&mut menus),
            Some(MenuAction::Game(HordeAction::Choose(2)))
        );

        menus.show(MenuKind::None);
        assert_eq!(activate(&mut menus), None, "there is no menu to activate");
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = menus();
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
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 2, Some(OFFER));
        menus.show(MenuKind::LevelUp);

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
        assert_eq!(
            point(&mut menus, extent, up),
            Some(MenuAction::Game(HordeAction::Choose(1))),
            "the pointer took the wrong upgrade",
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
    /// started on the pause menu cannot land on the level-up menu's button in
    /// the same place.
    #[test]
    fn switching_menus_drops_the_press() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 1, Some(OFFER));
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

        menus.show(MenuKind::LevelUp);
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

    /// A rebuilt level-up panel drops the capture too — the press was on a
    /// button that no longer says what it said.
    #[test]
    fn a_new_offer_drops_the_press() {
        let atlas = FontAtlas::built_in();
        let extent = (960, 720);
        let mut menus = menus();
        let mut offer = LevelUpOffer::default();
        offer.refresh(&mut menus, 1, Some(OFFER));
        menus.show(MenuKind::LevelUp);
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

        offer.refresh(
            &mut menus,
            2,
            Some([Upgrade::Vitality, Upgrade::Magnet, Upgrade::RapidFire]),
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
            "a release took an upgrade the player never pressed",
        );
    }
}
