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
use crcbl::engine::{DEBUG_OVERLAY_ID, FIRST_GAME_ID, FULLSCREEN_ID, FrameLimit};
use crcbl::render::DEFAULT_ANISOTROPY;
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

/// The id of the button that puts every setting back to its default.
pub const RESET_ID: crcbl::ui::WidgetId = SAVE_ID + 1;

/// The id of the row that steps `[engine.video] frame_limit`.
pub const FRAME_CAP_ID: crcbl::ui::WidgetId = RESET_ID + 1;

/// The ceilings the frame-cap row steps through, **in ascending order**.
///
/// A ladder rather than a groove because the value is a rate a display has and
/// not a continuum: a slider over `u32` would let a player ask for 143 fps on a
/// 144 Hz panel, which is a worse setting than either neighbour and one nobody
/// means to pick.
///
/// The widget set has a button and a slider and no third thing, so the row is a
/// button that steps forward — see [`next_frame_cap`]. Adding a cycler widget
/// to `crcbl-ui` is the alternative and is its own change; `docs/backlog.md`
/// carries it.
///
/// [`FrameLimit::unlimited`] sits at the end because that is where it sits in
/// the ordering, not because it is a special case — see [`is_above`].
pub const FRAME_CAPS: [FrameLimit; 7] = [
    FrameLimit::fps(30),
    FrameLimit::fps(60),
    FrameLimit::fps(72),
    FrameLimit::fps(120),
    FrameLimit::fps(144),
    FrameLimit::fps(240),
    FrameLimit::unlimited(),
];

/// Whether `limit` is a higher ceiling than `than`.
///
/// Asked of [`FrameLimit::clamped_to`] rather than of the rates, because the
/// ordering is that method's: holding `limit` under `than` yields `than`
/// exactly when `than` is the lower of the two. Comparing
/// [`FrameLimit::rate`]s directly is the thing that gets it backwards —
/// unlimited is spelled zero and is the *largest* ceiling there is.
#[must_use]
pub fn is_above(limit: FrameLimit, than: FrameLimit) -> bool {
    limit.clamped_to(than) == than && limit != than
}

/// The next ceiling up from `current`, wrapping round to the lowest.
///
/// **The first rung strictly above `current`**, so a file holding a rate that
/// is not on the ladder at all — a player who typed `frame_limit = 90` — steps
/// to the next rung up rather than to somewhere arbitrary.
#[must_use]
pub fn next_frame_cap(current: FrameLimit) -> FrameLimit {
    FRAME_CAPS
        .into_iter()
        .find(|rung| is_above(*rung, current))
        .unwrap_or(FRAME_CAPS[0])
}

/// How a ceiling is written beside its row.
#[must_use]
pub fn frame_cap_label(limit: FrameLimit) -> String {
    match limit.rate() {
        0 => "unlimited".to_string(),
        fps => format!("{fps} fps"),
    }
}

/// The id of the row that steps `[engine.video] anisotropic_filtering`.
pub const ANISOTROPY_ID: crcbl::ui::WidgetId = FRAME_CAP_ID + 1;

/// The anisotropies the row steps through, **in ascending order**.
///
/// A ladder for [`FRAME_CAPS`]' reason: the value is a count of taps a sampler
/// takes along a footprint, and hardware steps it in powers of two, so a groove
/// over the range would let a player ask for `6` and get `8`'s cost with a
/// number that says otherwise. Its top is
/// [`MAX_ANISOTROPIC_FILTERING`](crcbl::settings::MAX_ANISOTROPIC_FILTERING) —
/// the desktop ceiling the key reads up to — and a device whose own ceiling is lower clamps below the screen's back,
/// which the renderer does and this row cannot see.
///
/// The row is a button that steps forward, like the cap's; the cycler this
/// would rather be is `docs/backlog.md`'s entry.
pub const ANISOTROPIES: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

/// The next anisotropy up from `current`, wrapping round to the lowest.
///
/// **The first rung strictly above `current`**, on [`next_frame_cap`]'s terms:
/// the key's reader accepts any number in range, so a file holding a
/// hand-written `6` steps to `8` rather than somewhere arbitrary. A `NaN` sits
/// above nothing and wraps to the bottom.
#[must_use]
pub fn next_anisotropy(current: f32) -> f32 {
    ANISOTROPIES
        .into_iter()
        .find(|rung| *rung > current)
        .unwrap_or(ANISOTROPIES[0])
}

/// How an anisotropy is written beside its row: `off` at one, since one tap is
/// no anisotropy at all, and the count with an `x` above it.
///
/// An ASCII `x` rather than `×`, because the menu's atlas holds the printable
/// ASCII range and draws anything else as `.notdef`.
#[must_use]
pub fn anisotropy_label(anisotropy: f32) -> String {
    if anisotropy <= 1.0 {
        "off".to_string()
    } else {
        format!("{anisotropy}x")
    }
}

/// What this sample's own rows do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Write the edited settings back to wherever they were read from.
    Save,
    /// Put every setting back to its default — which is what an absent key
    /// means: every bus at unity, no frame ceiling and the engine's own
    /// anisotropy.
    Reset,
    /// Step the frame ceiling to the next rung of [`FRAME_CAPS`].
    CycleFrameCap,
    /// Step the page's anisotropy to the next rung of [`ANISOTROPIES`].
    CycleAnisotropy,
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

/// What a row writes after a value this run is not running under.
///
/// The loop takes its frame limit when it is built, so a cap chosen here is
/// one the **next** start will use; and this sample draws no page, so there is
/// nothing here for an anisotropy to reach until a renderer opens over the key.
/// The faders apply as they move, which is exactly why these two rows have to
/// say that they do not.
pub const NEXT_START_MARK: &str = "(next start)";

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
        // Beside `FULLSCREEN`, because both are `[engine.video]` and a player
        // looking for one is looking for the other. Its hint is written every
        // frame by `Screen::menu_kind`, like a fader's.
        MenuItem::new(
            FRAME_CAP_ID,
            "FRAME CAP",
            frame_cap_label(FrameLimit::unlimited()),
        ),
        // The other `[engine.video]` key, beside the first, and written every
        // frame the same way.
        MenuItem::new(
            ANISOTROPY_ID,
            "ANISOTROPY",
            anisotropy_label(DEFAULT_ANISOTROPY),
        ),
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
    use crcbl::settings::MAX_ANISOTROPIC_FILTERING;

    /// **The ladder is ascending, which everything else here assumes.**
    ///
    /// [`next_frame_cap`] wraps to `FRAME_CAPS[0]` when nothing is above the
    /// current rung, so a table written out of order would wrap to whatever
    /// happened to be first and step downwards from there. The order is also
    /// the order a player sees, and a settings list that jumps about is a
    /// different bug with the same cause.
    #[test]
    fn the_frame_cap_ladder_climbs() {
        for pair in FRAME_CAPS.windows(2) {
            assert!(
                is_above(pair[1], pair[0]),
                "{:?} does not sit above {:?}",
                pair[1],
                pair[0],
            );
        }
        assert_eq!(
            *FRAME_CAPS.last().expect("the ladder has rungs"),
            FrameLimit::unlimited(),
            "no ceiling is the highest ceiling, so it is the last rung",
        );
    }

    /// **Unlimited is the largest ceiling, not the smallest.**
    ///
    /// It is spelled zero, so every comparison that reaches for the rate gets
    /// this backwards — which is the whole reason [`is_above`] asks
    /// [`FrameLimit::clamped_to`] instead.
    #[test]
    fn no_ceiling_sits_above_every_ceiling() {
        for rung in FRAME_CAPS {
            if rung == FrameLimit::unlimited() {
                continue;
            }
            assert!(is_above(FrameLimit::unlimited(), rung), "{rung:?}");
            assert!(!is_above(rung, FrameLimit::unlimited()), "{rung:?}");
            assert!(!is_above(rung, rung), "a rung is not above itself");
        }
    }

    /// Every rung steps to the next, and the last steps back to the first.
    #[test]
    fn the_ladder_steps_up_and_wraps_at_the_top() {
        for (index, rung) in FRAME_CAPS.into_iter().enumerate() {
            let expected = FRAME_CAPS[(index + 1) % FRAME_CAPS.len()];
            assert_eq!(
                next_frame_cap(rung),
                expected,
                "{rung:?} stepped somewhere other than {expected:?}",
            );
        }
    }

    /// **A rate that is not on the ladder steps to the next rung above it**,
    /// rather than to an arbitrary place.
    ///
    /// A player who typed `frame_limit = 90` into the file by hand is the case:
    /// the screen has to do something with it, and stepping down or jumping to
    /// the end would silently take their setting further from what they asked
    /// for than one press should.
    #[test]
    fn a_rate_between_rungs_steps_up_to_the_next_one() {
        for rung in FRAME_CAPS {
            let Some(rate) = rung.rate().checked_sub(1).filter(|rate| *rate > 0) else {
                // Unlimited has no rate below it to sit under, and 30 minus one
                // is below the whole ladder, which the next case covers.
                continue;
            };
            assert_eq!(
                next_frame_cap(FrameLimit::fps(rate)),
                rung,
                "{rate} fps did not step up to {rung:?}",
            );
        }
        assert_eq!(
            next_frame_cap(FrameLimit::fps(1)),
            FRAME_CAPS[0],
            "a rate under the whole ladder steps onto its bottom rung",
        );
    }

    /// The row's text names the rate, and says so in words when there is none.
    #[test]
    fn a_ceiling_is_written_as_a_rate_and_no_ceiling_is_written_as_words() {
        assert_eq!(frame_cap_label(FrameLimit::fps(144)), "144 fps");
        assert_eq!(frame_cap_label(FrameLimit::unlimited()), "unlimited");
        assert_eq!(
            frame_cap_label(FrameLimit::fps(0)),
            frame_cap_label(FrameLimit::unlimited()),
            "zero is how the file spells no ceiling",
        );
    }

    /// **The anisotropy ladder is ascending and spans the key's whole range**:
    /// its bottom is the one tap that is no anisotropy, its top is the ceiling
    /// the key reads up to, and the engine's default is a rung — so `RESET`
    /// lands on a value the row could have stepped to.
    #[test]
    fn the_anisotropy_ladder_climbs_from_off_to_the_keys_ceiling() {
        for pair in ANISOTROPIES.windows(2) {
            assert!(
                pair[1] > pair[0],
                "{} does not sit above {}",
                pair[1],
                pair[0]
            );
        }
        assert_eq!(ANISOTROPIES[0], 1.0, "the bottom rung is off");
        assert_eq!(
            *ANISOTROPIES.last().expect("the ladder has rungs"),
            MAX_ANISOTROPIC_FILTERING,
            "the top rung is the most the key reads",
        );
        assert!(
            ANISOTROPIES.contains(&DEFAULT_ANISOTROPY),
            "the engine's default {DEFAULT_ANISOTROPY} is not a rung",
        );
    }

    /// Every rung steps to the next, the last wraps to the first, and a value
    /// between rungs — a hand-written `6` — steps up to the rung above it.
    #[test]
    fn the_anisotropy_ladder_steps_up_wraps_and_lifts_a_value_between_rungs() {
        for (index, rung) in ANISOTROPIES.into_iter().enumerate() {
            let expected = ANISOTROPIES[(index + 1) % ANISOTROPIES.len()];
            assert_eq!(
                next_anisotropy(rung),
                expected,
                "{rung} stepped somewhere other than {expected}",
            );
        }
        assert_eq!(next_anisotropy(6.0), 8.0);
        assert_eq!(next_anisotropy(0.5), ANISOTROPIES[0]);
        assert_eq!(
            next_anisotropy(f32::NAN),
            ANISOTROPIES[0],
            "a number above nothing wraps to the bottom",
        );
    }

    /// One tap is written as `off`; a count is written with its `x`, and a
    /// value the file spelled between rungs is written as it stands.
    #[test]
    fn an_anisotropy_is_written_as_a_count_and_one_is_written_as_off() {
        assert_eq!(anisotropy_label(1.0), "off");
        assert_eq!(anisotropy_label(8.0), "8x");
        assert_eq!(anisotropy_label(16.0), "16x");
        assert_eq!(anisotropy_label(6.0), "6x");
        assert!(
            anisotropy_label(DEFAULT_ANISOTROPY)
                .bytes()
                .all(|byte| byte.is_ascii_graphic()),
            "the label has to be in the atlas",
        );
    }

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
