//! The run's launch layers: the game's defaults and the command line's
//! `--set` overrides, held for the process.
//!
//! # Why process-wide
//!
//! A run opens its settings in more places than any one value is handed to:
//! [`GpuContext::open`](crate::engine::GpuContext::open) reads
//! `[engine.video]`, the loop's console reads the whole file, and a game's
//! audio start-up reads `[engine.audio]` — each through
//! [`SettingsSource::open`], from a source and an app name and nothing else.
//! The command line is the process's, as `std::env::args` is, and a game's
//! defaults are its binary's, so both are installed here once, by
//! [`crate::args::run_front_end`], and every [`SettingsSource::open`] layers
//! them. A caller holding the layers itself — a tool that parsed its own
//! [`Common`](crate::args::Common) — passes them to
//! [`SettingsSource::open_with`] instead, and no global is involved.

use std::sync::{PoisonError, RwLock};

use crcbl_store::settings::LaunchLayers;

#[cfg(doc)]
use crate::engine::SettingsSource;

/// What [`install`] put here, or `None` before anything did.
static INSTALLED: RwLock<Option<LaunchLayers>> = RwLock::new(None);

/// The prefix of every key the engine's catalogue is the authority on.
const ENGINE_PREFIX: &str = "engine.";

/// Makes `layers` the ones every later [`SettingsSource::open`] in this
/// process layers around the player's file, replacing any installed before.
pub fn install(layers: LaunchLayers) {
    *INSTALLED.write().unwrap_or_else(PoisonError::into_inner) = Some(layers);
}

/// The installed launch layers, or none at all before [`install`].
#[must_use]
pub fn installed() -> LaunchLayers {
    INSTALLED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// Every override in `layers` whose key nothing defines, in key order.
///
/// A key under `engine.` is the engine's, so one [`super::catalogued`] does
/// not name is a typo. Any other key is the game's, and only a game that
/// supplied defaults has said which keys it has: for one that did, a key its
/// defaults do not hold is unknown too, and for one that did not, nothing
/// here can tell. Unknown is a warning and never a refusal — the settings
/// rules in `docs/notes/simulation.md` say unknown keys warn and never crash,
/// and a key a newer build reads is one an older build does not know.
#[must_use]
pub fn unknown_overrides(layers: &LaunchLayers) -> Vec<String> {
    layers
        .overridden_keys()
        .into_iter()
        .filter(|key| {
            if key.starts_with(ENGINE_PREFIX) {
                super::catalogued(key).is_none()
            } else {
                layers.has_game_defaults() && !layers.game_defines(key)
            }
        })
        .collect()
}

/// Holds the process's launch layers for one test and puts "none installed"
/// back when it drops.
#[cfg(test)]
pub(crate) struct LaunchTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for LaunchTestGuard {
    fn drop(&mut self) {
        *INSTALLED.write().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// Serialises the tests that install launch layers, which are process state.
#[cfg(test)]
pub(crate) fn launch_test_guard() -> LaunchTestGuard {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LaunchTestGuard {
        _lock: LOCK.lock().unwrap_or_else(PoisonError::into_inner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers(game: Option<&str>, sets: &[&str]) -> LaunchLayers {
        let mut layers = LaunchLayers::new();
        if let Some(game) = game {
            layers = layers.with_game_defaults(game).expect("a test's own TOML");
        }
        for arg in sets {
            layers.set(arg).expect("a well-formed override");
        }
        layers
    }

    /// **A mistyped engine key is unknown; a catalogued one is not.**
    #[test]
    fn an_engine_key_the_catalogue_does_not_name_is_unknown() {
        let layers = layers(
            None,
            &["engine.video.shadow=false", "engine.video.shadows=false"],
        );
        assert_eq!(unknown_overrides(&layers), ["engine.video.shadow"]);
    }

    /// **A game key is judged against the game's defaults, and only when it
    /// has some.**
    #[test]
    fn a_game_key_is_unknown_only_against_defaults_that_omit_it() {
        let sets = ["editor.snap.grid=0.5", "editor.snap.gird=0.5"];
        assert_eq!(
            unknown_overrides(&layers(Some("[editor.snap]\ngrid = 0.25"), &sets)),
            ["editor.snap.gird"]
        );
        assert!(
            unknown_overrides(&layers(None, &sets)).is_empty(),
            "a game with no defaults has not said which keys are its own"
        );
    }
}
