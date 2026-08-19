//! The viewer's one panel, and every button on it is the loop's.
//!
//! ```text
//!   ESC pressed ──▶ Menu
//!   otherwise ─────▶ none
//! ```
//!
//! # A viewer has nothing to pause, and the panel is not about pausing
//!
//! [`crcbl::engine::Loop`] calls the state "paused" because for a game that is
//! what stopping the clock means. This sample steps no simulation at all — it is
//! `docs/plan/sample/05-viewer.md`'s sanctioned exception to rule 2, see
//! [`crate`] — so the flag stops nothing here and the panel is the only thing it
//! does.
//!
//! That is worth having on its own terms: fullscreen and the debug panel are
//! bound to `F11` and `F3`, and a panel is the **only** way to reach either with
//! a pointer. A machine with no keyboard could otherwise open a model and then
//! do neither. So the title is `MENU` and the first item is `BACK` rather than
//! the `PAUSED`/`RESUME` every game uses: the keys are the same in every sample
//! and the words are not lies about this one.
//!
//! # One item of the viewer's own, and it is not a button
//!
//! Milestone 2's listing panel has landed — [`crate::listing`] — and it is
//! bound to a key rather than to a button here, because it is a read-only view
//! of the document and a menu that has to be dismissed to see what it toggled
//! is the wrong shape for one.
//!
//! The exposure is the exception, and it is a **slider**: `-` and `=` already
//! step it, so what a panel adds is the thing keys cannot do — reaching a value
//! directly, and seeing where in the range you are. [`EXPOSURE_ID`] names it.
//!
//! It is still not a button. A [`crcbl::ui::MenuItemKind::Slider`] fires
//! nothing from either the commit key or a click, so no id from this panel ever
//! reaches [`crcbl::engine::MenuAction::from_id`] — which is what keeps
//! [`crate::app::Viewer`]'s `MenuAction` [`core::convert::Infallible`],
//! uninhabited rather than an empty enum waiting to be filled in. The viewer
//! reads the handle out of the set in
//! [`HostedGame::menu_kind`](crcbl::engine::HostedGame::menu_kind) instead.
//!
//! # The handle is logarithmic, because exposure is a ratio
//!
//! [`crcbl::render::EXPOSURE_MIN`] to [`crcbl::render::EXPOSURE_MAX`] is five
//! stops either side of one. Laid out linearly, the whole bottom half of the
//! range — every value under one — would live in the first one-and-a-half
//! percent of the groove and be unreachable with a mouse. In stops it is even,
//! and the middle of the groove is the middle of the range.

use crcbl::engine::{DEBUG_OVERLAY_ID, FIRST_GAME_ID, FULLSCREEN_ID, RESUME_ID};
use crcbl::render::{EXPOSURE_MAX, EXPOSURE_MIN};
use crcbl::ui::menu::{Menu, MenuItem, MenuSet, Slider};

/// The exposure slider's id.
///
/// The first id the engine leaves to the game — the viewer has exactly one row
/// of its own, and this is it.
pub const EXPOSURE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// Which menu a frame shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuKind {
    /// Looking at the model: no panel at all.
    #[default]
    None,
    /// `ESC` has been pressed.
    Menu,
}

impl MenuKind {
    /// The menu this frame shows.
    ///
    /// `paused` is the loop's flag; see the [module docs](self) for why the
    /// word does not fit here.
    #[must_use]
    pub const fn of(paused: bool) -> Self {
        if paused { Self::Menu } else { Self::None }
    }
}

/// The viewer's menus, keyed by the state each belongs to.
pub type Menus = MenuSet<MenuKind>;

/// The one menu, with nothing shown.
///
/// [`MenuKind::None`] has no entry, which is how an ordinary frame is told to
/// draw nothing.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(
        MenuKind::None,
        vec![(
            MenuKind::Menu,
            Menu::new(
                "MENU",
                vec![
                    MenuItem::new(RESUME_ID, "BACK", "ESC"),
                    MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
                    MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
                    // The handle is placed from the renderer's own default
                    // rather than from the middle of the groove: `crate::app`
                    // reads the exposure off the renderer for the same reason,
                    // and a panel that opened with the handle somewhere the
                    // frame is not drawn at would be lying on its first frame.
                    MenuItem::slider(
                        EXPOSURE_ID,
                        "EXPOSURE",
                        crate::listing::exposure_value(crcbl::shaders::tonemap::DEFAULT_EXPOSURE),
                        handle_at(crcbl::shaders::tonemap::DEFAULT_EXPOSURE),
                    ),
                ],
            ),
        )],
    )
}

/// Where the handle sits for `exposure`: `0.0` at
/// [`crcbl::render::EXPOSURE_MIN`], `1.0` at [`crcbl::render::EXPOSURE_MAX`],
/// and evenly spaced in **stops** between them — see the [module docs](self).
#[must_use]
pub fn handle_at(exposure: f32) -> f32 {
    // Through the widget's own clamp rather than one of this module's, so the
    // number `crate::app` compares for exact equality against the handle is the
    // number the handle will hold. It also lands a `NaN` at nought, which
    // `f32::clamp` would hand straight back — and a `NaN` there is never equal
    // to anything, so every frame would read as a fresh drag.
    Slider::new((exposure.log2() - EXPOSURE_MIN.log2()) / STOPS).position()
}

/// The exposure a handle at `position` names — [`handle_at`] the other way
/// round.
#[must_use]
pub fn exposure_at(position: f32) -> f32 {
    (EXPOSURE_MIN.log2() + position * STOPS).exp2()
}

/// How many stops the groove spans.
const STOPS: f32 = 10.0;

/// The range really is [`STOPS`] stops wide, so the two conversions above are
/// not carrying a number of their own.
const _: () = assert!(EXPOSURE_MIN * 1024.0 == EXPOSURE_MAX);

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::engine::HostedGame as _;
    use crcbl::ui::MenuItemKind;

    /// `ESC` is the only thing that puts a panel on screen here.
    #[test]
    fn the_panel_is_shown_exactly_while_the_loop_calls_itself_paused() {
        assert_eq!(MenuKind::of(true), MenuKind::Menu);
        assert_eq!(MenuKind::of(false), MenuKind::None);

        let mut menus = menus();
        assert!(!menus.is_showing(), "an ordinary frame draws no panel");
        menus.show(MenuKind::Menu);
        let menu = menus.current().expect("the menu kind has a menu");
        assert_eq!(menu.title, "MENU");
        assert_eq!(
            menu.items().iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![RESUME_ID, FULLSCREEN_ID, DEBUG_OVERLAY_ID, EXPOSURE_ID],
            "the panel is the loop's three buttons and the viewer's slider",
        );
    }

    /// **No *button* on this panel is numbered in the game's range**, so
    /// [`crcbl::engine::MenuAction::from_id`] never asks the viewer about an id
    /// — which is what makes an uninhabited `MenuAction` sound rather than a
    /// hole.
    ///
    /// The slider does claim a game id, and the reason that is not the same
    /// thing is the next test: a slider reports no id at all, so nothing that
    /// reaches `from_id` was ever numbered here.
    #[test]
    fn no_button_claims_an_id_the_viewer_would_have_to_answer_for() {
        let mut menus = menus();
        menus.show(MenuKind::Menu);
        for item in menus.current().expect("the menu").items() {
            if matches!(item.kind, MenuItemKind::Slider(_)) {
                continue;
            }
            assert!(
                item.id < FIRST_GAME_ID,
                "{} claims {}, which the viewer would have to name",
                item.label,
                item.id,
            );
            assert_eq!(
                crcbl::engine::MenuAction::from_id(item.id, crate::app::Viewer::menu_action),
                Some(match item.id {
                    RESUME_ID => crcbl::engine::MenuAction::Resume,
                    FULLSCREEN_ID => crcbl::engine::MenuAction::Fullscreen,
                    _ => crcbl::engine::MenuAction::DebugOverlay,
                }),
            );
        }
    }

    /// **The slider fires nothing**, from the commit key or from a release over
    /// it — which is what keeps [`crate::app::Viewer`]'s `MenuAction`
    /// uninhabited while the panel carries a row numbered in the game's range.
    ///
    /// The engine only asks `menu_action` about ids it was handed, so an id
    /// this never reports is one the viewer never has to answer for.
    #[test]
    fn the_slider_reports_no_id_for_the_loop_to_route() {
        let mut menus = menus();
        menus.show(MenuKind::Menu);
        let menu = menus.current_mut().expect("the menu");
        menu.select_id(EXPOSURE_ID);
        assert_eq!(menu.selected_item().map(|item| item.id), Some(EXPOSURE_ID));
        assert_eq!(menu.activate(), None, "the slider fired");
    }

    /// **Stops, not multipliers.** The handle is even in stops, so the middle
    /// of the groove is one — the value the renderer starts at — and each half
    /// of the groove holds five stops.
    #[test]
    fn the_handle_is_even_in_stops() {
        assert_eq!(handle_at(EXPOSURE_MIN), 0.0);
        assert_eq!(handle_at(EXPOSURE_MAX), 1.0);
        assert!((handle_at(1.0) - 0.5).abs() <= 1e-6, "{}", handle_at(1.0));
        assert!(
            (handle_at(2.0) - 0.6).abs() <= 1e-6,
            "one stop up is a tenth of the groove, not {}",
            handle_at(2.0),
        );
    }

    /// The two directions are each other's inverse across the whole range, so a
    /// handle the viewer mirrors from the renderer and then reads back does not
    /// creep.
    #[test]
    fn a_position_and_an_exposure_round_trip() {
        for step in 0..=20 {
            let position = step as f32 / 20.0;
            let back = handle_at(exposure_at(position));
            assert!(
                (back - position).abs() <= 1e-5,
                "{position} came back as {back}",
            );
        }
        for exposure in [
            EXPOSURE_MIN,
            0.5,
            1.0,
            crate::app::exposure_step(),
            EXPOSURE_MAX,
        ] {
            let back = exposure_at(handle_at(exposure));
            assert!(
                (back - exposure).abs() <= exposure * 1e-5,
                "{exposure} came back as {back}",
            );
        }
    }

    /// Anything outside the renderer's range lands on the end of the groove
    /// rather than off it — the handle is drawn from this number.
    #[test]
    fn a_value_outside_the_range_lands_on_an_end() {
        assert_eq!(handle_at(EXPOSURE_MAX * 4.0), 1.0);
        assert_eq!(handle_at(EXPOSURE_MIN / 4.0), 0.0);
        assert_eq!(handle_at(0.0), 0.0);
        assert_eq!(handle_at(f32::INFINITY), 1.0);
        assert_eq!(handle_at(f32::NAN), 0.0, "a NaN handle is drawn nowhere");
    }
}
