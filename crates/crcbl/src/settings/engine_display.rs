//! The two `[engine.video]` keys that are properties of a window and its
//! swapchain rather than of a frame: `display_mode` and `present_mode`.
//!
//! # Why a file of their own
//!
//! Every key in `engine_video` reaches a renderer, the loop's clock or the
//! UI's scale, and none of them can stop a player seeing the screen. These two
//! can: a display mode a driver handles badly, or a present mode that hangs a
//! compositor, leaves a player looking at nothing with no way to reach the row
//! that would undo it. So they are the catalogue's
//! [`confirm`](super::CatalogueKey::confirm) keys, applied through
//! [`super::confirm`]'s pending change rather than written outright, and their
//! live seams are a window and a swapchain where every other key's is a
//! renderer or a mixer.
//!
//! # Both keys replace; neither clamps
//!
//! The display catalogue (`docs/notes/backends.md`) once read
//! `display_mode = "borderless"` as a **ceiling** under rule 1 — a game that
//! opened windowed stayed windowed. That reading left the one round trip a
//! player expects, the fullscreen toggle remembered between runs, with nothing
//! to say it in, and "windowed" and "borderless" are not a less and a more: a
//! borderless window is not a costlier frame, it is a different place for the
//! same one. So both keys **replace** the game's opening choice, as
//! [`antialiasing`](super::antialiasing) replaces the resolve slot — decided
//! 2026-10-05 and recorded in `docs/backlog.md`. What still outranks the file
//! is the command line: `--fullscreen` and a `--pacing` other than `auto` are
//! the person starting the run saying what they want for it.

use crcbl_shell::DisplayMode;
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

use super::engine_video::VIDEO_NAMESPACE;
use crate::engine::Pacing;

/// The `[engine.video]` key that says how the window sits on the desktop.
pub const DISPLAY_MODE_KEY: &str = "display_mode";

/// The words [`display_mode`] reads, in the order a settings row steps them.
///
/// Two, because [`DisplayMode`] has two variants and the engine never
/// modesets — there is no exclusive fullscreen to name.
pub const DISPLAY_MODE_NAMES: [&str; 2] = ["windowed", "borderless"];

/// The `[engine.video]` key that says how presented frames are paced.
pub const PRESENT_MODE_KEY: &str = "present_mode";

/// The words [`present_mode`] reads: [`Pacing::name`] of each of
/// [`Pacing::ALL`], so the key, `--pacing` and the console agree about every
/// word without a second list to drift.
pub const PRESENT_MODE_NAMES: [&str; Pacing::ALL.len()] = {
    let mut names = [""; Pacing::ALL.len()];
    let mut i = 0;
    while i < Pacing::ALL.len() {
        names[i] = Pacing::ALL[i].name();
        i += 1;
    }
    names
};

/// The word `[engine.video] display_mode` holds for `mode`.
///
/// The monitor a borderless window covers is not part of the word: that is the
/// `monitor` key's, and a borderless answer that names one is still
/// borderless.
#[must_use]
pub const fn display_mode_name(mode: DisplayMode) -> &'static str {
    if mode.is_borderless() {
        DISPLAY_MODE_NAMES[1]
    } else {
        DISPLAY_MODE_NAMES[0]
    }
}

/// The mode `name` spells, or `None` for a word that is not one of
/// [`DISPLAY_MODE_NAMES`].
#[must_use]
pub fn display_mode_from_name(name: &str) -> Option<DisplayMode> {
    if name == DISPLAY_MODE_NAMES[0] {
        Some(DisplayMode::Windowed)
    } else if name == DISPLAY_MODE_NAMES[1] {
        Some(DisplayMode::Borderless { monitor: None })
    } else {
        None
    }
}

/// How the player wants the window to sit, or `None` for a player who has not
/// said.
///
/// A word that is not one of [`DISPLAY_MODE_NAMES`] — or a value that is not a
/// word — is not said either, and warns once naming the key, on
/// [`antialiasing`](super::antialiasing)'s terms. An absent key is the
/// ordinary case and does not warn.
#[must_use]
pub fn display_mode(stack: &SettingsStack) -> Option<DisplayMode> {
    let dotted = format!("{VIDEO_NAMESPACE}.{DISPLAY_MODE_KEY}");
    let mode = stack
        .get::<String>(&dotted)
        .and_then(|name| display_mode_from_name(&name));
    if mode.is_none() && stack.contains(&dotted) {
        crcbl_core::log::warn!(
            "settings: `{dotted}` names no display mode, so it does nothing; \
             the window opens the way the game asked for it"
        );
    }
    mode
}

/// How the player wants frames paced, or `None` for a player who has not
/// said — [`display_mode`]'s rules, over [`PRESENT_MODE_NAMES`].
#[must_use]
pub fn present_mode(stack: &SettingsStack) -> Option<Pacing> {
    let dotted = format!("{VIDEO_NAMESPACE}.{PRESENT_MODE_KEY}");
    // `from_name` alone would also take " VSYNC ", which `--pacing` forgives
    // and a file the console wrote never holds; the key's domain is the exact
    // words, as every other enum key's is.
    let pacing = stack
        .get::<String>(&dotted)
        .filter(|name| PRESENT_MODE_NAMES.contains(&name.as_str()))
        .and_then(|name| Pacing::from_name(&name));
    if pacing.is_none() && stack.contains(&dotted) {
        crcbl_core::log::warn!(
            "settings: `{dotted}` names no present mode, so it does nothing; \
             frames are paced the way the game asked for"
        );
    }
    pacing
}

/// Write `[engine.video] display_mode` as the word [`display_mode`] reads back.
///
/// # Errors
///
/// [`SettingsStack::set`]'s: no user layer in the stack, or an ancestor of the
/// key already holding a scalar in a hand-edited file.
pub fn set_display_mode(stack: &mut SettingsStack, mode: DisplayMode) -> Result<(), StorageError> {
    stack.set(
        &format!("{VIDEO_NAMESPACE}.{DISPLAY_MODE_KEY}"),
        &display_mode_name(mode),
    )
}

/// Write `[engine.video] present_mode` as the word [`present_mode`] reads back.
///
/// # Errors
///
/// [`set_display_mode`]'s.
pub fn set_present_mode(stack: &mut SettingsStack, pacing: Pacing) -> Result<(), StorageError> {
    stack.set(
        &format!("{VIDEO_NAMESPACE}.{PRESENT_MODE_KEY}"),
        &pacing.name(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::tests::{round_trip, stack_from};

    /// **Each mode and each pacing survives the file**, written by its writer
    /// and read back by its reader through the real loader.
    #[test]
    fn every_display_and_present_mode_round_trips_through_the_file() {
        for mode in [
            DisplayMode::Windowed,
            DisplayMode::Borderless { monitor: None },
        ] {
            let (reloaded, written) = round_trip(|stack| {
                set_display_mode(stack, mode).expect("a fresh user layer accepts every key");
            });
            assert_eq!(display_mode(&reloaded), Some(mode), "{written}");
        }
        for pacing in Pacing::ALL {
            let (reloaded, written) = round_trip(|stack| {
                set_present_mode(stack, pacing).expect("a fresh user layer accepts every key");
            });
            assert_eq!(present_mode(&reloaded), Some(pacing), "{written}");
            assert_eq!(Pacing::from_name(pacing.name()), Some(pacing));
        }
    }

    /// **An absent key and a word outside the domain both say nothing**, so a
    /// file the player broke opens the window the game's way rather than a way
    /// nobody asked for.
    #[test]
    fn an_absent_or_unknown_word_says_nothing() {
        assert_eq!(display_mode(&stack_from("")), None);
        assert_eq!(present_mode(&stack_from("")), None);
        let broken = stack_from("[engine.video]\ndisplay_mode = \"exclusive\"\npresent_mode = 3\n");
        assert_eq!(display_mode(&broken), None);
        assert_eq!(present_mode(&broken), None);
        assert_eq!(
            present_mode(&stack_from("[engine.video]\npresent_mode = \" VSYNC \"\n")),
            None,
            "the file's domain is the exact words, not `--pacing`'s forgiving parse",
        );
    }

    /// The monitor a borderless answer names is not part of the word.
    #[test]
    fn a_borderless_answer_on_a_named_monitor_is_still_borderless() {
        assert_eq!(
            display_mode_name(DisplayMode::Borderless {
                monitor: Some(crcbl_shell::MonitorId(3)),
            }),
            "borderless",
        );
        assert_eq!(display_mode_name(DisplayMode::Windowed), "windowed");
    }
}
