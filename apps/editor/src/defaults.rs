//! The editor's default settings, and the rule its numeric keys are read by.
//!
//! `defaults.toml`, beside this file, is the one place the editor's default
//! snap steps and autosave interval are written. It reaches the settings
//! stack as the game-defaults layer — [`crate::args::parse`] declares it on
//! the shared [`Common`](crcbl::args::Common) — so it sits under the person's
//! `settings.toml` and a `--set`, and a key they never changed stays out of
//! their file.

use crcbl::store::settings::{LaunchLayers, SettingsStack, StorageSettingsFile};

/// The editor's default settings, a TOML document.
pub const SETTINGS_DEFAULTS: &str = include_str!("defaults.toml");

/// A stack holding the editor's defaults and nothing else: what every key
/// reads with no settings file and no command line.
///
/// # Panics
///
/// If `defaults.toml` is not TOML, which
/// `the_defaults_are_toml_and_hold_every_key_the_editor_reads` refuses.
#[must_use]
pub fn stack() -> SettingsStack {
    let layers = LaunchLayers::new()
        .with_game_defaults(SETTINGS_DEFAULTS)
        .expect("`defaults.toml` is TOML");
    SettingsStack::layered(&layers, StorageSettingsFile::empty())
}

/// The positive, finite number of `unit` that `key` holds, from the highest
/// layer holding one.
///
/// A value that is not one — `0`, `-1`, `"fine"` — is logged and passed over
/// rather than refused, and the layer beneath answers, down to
/// `defaults.toml`: a settings file is a thing people edit, and a step of zero
/// would divide by it.
///
/// # Panics
///
/// If no layer holds a usable value, which is a stack built without the
/// editor's defaults under it — [`stack`] and every stack
/// [`crate::app::Editor`] opens have them.
#[must_use]
pub fn positive(stack: &SettingsStack, key: &str, unit: &str) -> f64 {
    let (value, from) = stack
        .find::<f64>(key, |value| value.is_finite() && *value > 0.0)
        .unwrap_or_else(|| panic!("the editor's defaults hold {key}, and this stack has none"));
    if stack.layer_of(key) != Some(from) {
        crcbl::log::warn!("editor: {key} is not a positive number of {unit}; using {value} {unit}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::gizmo::{ANGLE_KEY, GRID_KEY, SCALE_KEY};

    /// **The compiled-in defaults parse, and hold every key the editor reads
    /// through [`positive`]** — the one claim the panics above rest on.
    #[test]
    fn the_defaults_are_toml_and_hold_every_key_the_editor_reads() {
        let stack = stack();
        // The autosave interval's default is held by the recovery tests, which
        // can see its key: `the_help_text_states_the_recovery_constants`.
        for key in [GRID_KEY, SCALE_KEY, ANGLE_KEY] {
            assert!(
                stack.get::<f64>(key).is_some_and(|value| value > 0.0),
                "{key} has no positive default"
            );
        }
    }

    /// **A value the editor cannot use is passed over to the default
    /// beneath, and the pass is logged by key.**
    #[test]
    fn an_unusable_value_falls_to_the_default_and_says_so() {
        let mut stack = stack();
        let default = positive(&stack, GRID_KEY, "metres");
        stack
            .set(GRID_KEY, &0.0)
            .expect("the user layer is writable");

        let logs = crcbl::core::log::capture();
        assert_eq!(positive(&stack, GRID_KEY, "metres"), default);
        let records = logs.records();
        assert!(
            records
                .iter()
                .any(|record| record.message.contains(GRID_KEY)),
            "{records:?}"
        );
    }
}
