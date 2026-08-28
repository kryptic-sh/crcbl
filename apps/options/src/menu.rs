//! The one screen this sample has, and the taper its faders use.
//!
//! ```text
//!   every frame ──▶ Settings
//! ```
//!
//! # The panel is never dismissed
//!
//! Every other sample's panel appears on `ESC` and goes away again, because
//! behind it there is a game the player came for. Here the panel *is* what the
//! player came for: [`crate::app::Screen`]'s `menu_kind` answers
//! [`MenuKind::Settings`] on every frame, paused or not, and there is no state
//! in which this sample draws nothing. So the title is `SETTINGS` and there is
//! no `RESUME` row — a button that dismissed the screen would leave a window
//! with nothing in it.
//!
//! `FULLSCREEN` and `DEBUG PANEL` are still here, for
//! [`crate::menu`]'s reason in `apps/viewer`: they are bound to `F11` and `F3`,
//! and a panel is the only way to reach either with a pointer.
//!
//! # A fader is not linear in amplitude
//!
//! `[engine.audio]` stores a **linear gain** in `[0, 1]`, and a groove laid out
//! linearly in that number spends its top half in a range a listener can barely
//! separate: half amplitude is about six decibels down, which most people call
//! "slightly quieter", so three quarters of the perceived range is crammed into
//! the bottom of the fader. Every mixing desk answers this with a taper, and the
//! square law [`gain_at`] uses is the cheap one — position squared, so the
//! middle of the groove is a quarter of the amplitude, roughly the "half as
//! loud" a player is reaching for. [`handle_at`] is the same map backwards.

use crcbl::audio::mixer::Bus;
use crcbl::engine::{DEBUG_OVERLAY_ID, FIRST_GAME_ID, FULLSCREEN_ID};
use crcbl::ui::menu::{Menu, MenuItem, MenuSet, Slider};

/// The id of `bus`'s fader, numbered in [`Bus::ALL`]'s order from the first id
/// the engine leaves to a game.
///
/// A slider fires nothing — see [`crate::app::Screen`]'s `menu_action` — so
/// these ids never reach [`crcbl::engine::MenuAction::from_id`]. They are read
/// back off the set instead, which is what [`bus_of`] is for.
#[must_use]
pub const fn fader_id(bus: Bus) -> crcbl::ui::WidgetId {
    FIRST_GAME_ID + bus.index() as crcbl::ui::WidgetId
}

/// The bus a fader id names, or `None` for an id that is not a fader.
#[must_use]
pub fn bus_of(id: crcbl::ui::WidgetId) -> Option<Bus> {
    Bus::ALL.into_iter().find(|bus| fader_id(*bus) == id)
}

/// The id of the button that writes the file.
pub const SAVE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + Bus::ALL.len() as crcbl::ui::WidgetId;

/// The id of the button that puts every bus back to unity.
pub const RESET_ID: crcbl::ui::WidgetId = SAVE_ID + 1;

/// What this sample's own two buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Write the edited settings back to wherever they were read from.
    Save,
    /// Put every bus back to unity — which is what an absent key means.
    Reset,
}

/// Which menu a frame shows. There is only one, and it is always on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuKind {
    /// The settings screen.
    #[default]
    Settings,
}

/// This sample's menus, keyed by the state each belongs to.
pub type Menus = MenuSet<MenuKind>;

/// The gain a handle `position` of the way along the groove names.
///
/// See the [module docs](self) for why this is not the identity.
#[must_use]
pub fn gain_at(position: f32) -> f32 {
    // Through the widget's own clamp rather than one of this module's, so the
    // number the screen writes is the one the handle will hold on the frame
    // after — and so a `NaN` lands at nought instead of being handed straight
    // back by `f32::clamp`.
    let position = Slider::new(position).position();
    position * position
}

/// Where the handle sits for `gain` — [`gain_at`] the other way round.
#[must_use]
pub fn handle_at(gain: f32) -> f32 {
    Slider::new(gain).position().sqrt()
}

/// How a gain is written beside its groove.
#[must_use]
pub fn percent(gain: f32) -> String {
    format!("{:.0}%", gain * 100.0)
}

/// The whole hint on a bus's row: its gain, and a mark when the bus carries
/// nothing to hear.
///
/// `docs/plan/sample/20-options.md`'s exit criteria want a control with no
/// implementation to say so. Two of the six buses have no content — see
/// [`crate::audio`] — and their faders write a key that nothing in this process
/// reads back as sound, which without the mark is indistinguishable from broken
/// audio.
#[must_use]
pub fn fader_hint(gain: f32, audible: bool) -> String {
    if audible {
        percent(gain)
    } else {
        format!("{} {SILENT_MARK}", percent(gain))
    }
}

/// What [`fader_hint`] writes after the gain of a bus with nothing on it.
pub const SILENT_MARK: &str = "(silent)";

/// The label a bus wears on its row.
#[must_use]
pub const fn label(bus: Bus) -> &'static str {
    match bus {
        Bus::Master => "MASTER",
        Bus::Music => "MUSIC",
        Bus::Sfx => "EFFECTS",
        Bus::Ui => "INTERFACE",
        Bus::Voice => "VOICE",
        Bus::Ambience => "AMBIENCE",
    }
}

/// The screen, with every fader placed from `gains` — one per bus, in
/// [`Bus::ALL`]'s order.
///
/// Built from the gains the run opened with rather than from a default, for
/// `apps/viewer`'s reason: a panel whose handles started in the middle of the
/// groove would be lying about the player's file on its first frame.
#[must_use]
pub fn menus(gains: &[(Bus, f32); Bus::ALL.len()]) -> Menus {
    let mut items = vec![
        MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
        MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
    ];
    items.extend(gains.iter().map(|(bus, gain)| {
        MenuItem::slider(
            fader_id(*bus),
            label(*bus),
            percent(*gain),
            handle_at(*gain),
        )
    }));
    items.push(MenuItem::new(SAVE_ID, "SAVE", ""));
    items.push(MenuItem::new(RESET_ID, "RESET", ""));
    MenuSet::new(
        MenuKind::Settings,
        vec![(MenuKind::Settings, Menu::new("SETTINGS", items))],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bus has a row, no two rows share an id, and nothing on the screen
    /// claims an id the loop owns.
    #[test]
    fn every_bus_has_a_fader_of_its_own_and_no_row_claims_a_reserved_id() {
        let mut menus = menus(&Bus::ALL.map(|bus| (bus, 1.0)));
        menus.show(MenuKind::Settings);
        let menu = menus.current().expect("the screen is always showing");

        for bus in Bus::ALL {
            assert_eq!(
                bus_of(fader_id(bus)),
                Some(bus),
                "{bus:?}'s id must name it back",
            );
            assert!(
                menu.items().iter().any(|item| item.id == fader_id(bus)),
                "{bus:?} has no row",
            );
        }

        let mut ids: Vec<_> = menu.items().iter().map(|item| item.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two rows share an id");

        for item in menu.items() {
            let reserved = item.id < FIRST_GAME_ID;
            let engine_owned = item.id == FULLSCREEN_ID || item.id == DEBUG_OVERLAY_ID;
            assert_eq!(
                reserved, engine_owned,
                "{} claims {}, which is not its to claim",
                item.label, item.id,
            );
        }
    }

    /// The two conversions are each other's inverse across the groove, which is
    /// what stops a fader drifting every time the screen reconciles it.
    #[test]
    fn the_taper_round_trips_and_puts_the_middle_of_the_groove_a_quarter_up() {
        for step in 0_u8..=20 {
            let position = f32::from(step) / 20.0;
            let back = handle_at(gain_at(position));
            assert!(
                (back - position).abs() < 1e-5,
                "a handle at {position} came back at {back}",
            );
        }
        assert!((gain_at(0.5) - 0.25).abs() < 1e-6);
        assert!((gain_at(1.0) - 1.0).abs() < 1e-6);
        assert!((gain_at(0.0)).abs() < 1e-6);
    }

    /// A `NaN` from a broken settings file lands at an end of the groove rather
    /// than making every later comparison false.
    #[test]
    fn a_handle_that_is_not_a_number_lands_at_an_end_of_the_groove() {
        assert_eq!(gain_at(f32::NAN), 0.0);
        assert_eq!(handle_at(f32::NAN), 0.0);
        assert_eq!(gain_at(2.0), 1.0);
        assert_eq!(handle_at(-1.0), 0.0);
    }

    /// The number beside the groove is the gain, not the handle: a fader at
    /// half its travel is a quarter of the amplitude and has to say so.
    #[test]
    fn the_value_beside_a_groove_is_the_gain() {
        assert_eq!(percent(gain_at(0.5)), "25%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(0.0), "0%");
    }
}
