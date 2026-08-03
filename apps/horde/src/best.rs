//! Where horde's best run is kept.
//!
//! | Target | Where |
//! | --- | --- |
//! | native, windowed | `~/.config/horde/best.bin` |
//! | native, `--headless` | nowhere, so a CI run leaves no trace |
//! | `wasm32` | the Origin Private File System |
//!
//! # The number is a *time*, and that is this sample's own choice
//!
//! The plan's exit criterion is "survive five minutes", so the run's score is
//! how long it lasted. It is stored in **whole seconds** because that is what
//! the HUD's `m:ss` clock shows — a best that read `2:13` and compared as
//! `133.4187` would be a number the player could beat without the display
//! changing. [`Best::update`] therefore truncates before it compares, and a run
//! that beats the record by a tenth of a second correctly does not.
//!
//! **That rule is why this sample still has a type where the other three do
//! not.** The plumbing — the platform arms, the encode, the corrupt-file case,
//! the headless rule — is [`crcbl::store::record::Record`], the same as
//! breakout's, flappy's and asteroids'. What is wrapped around it here is the
//! policy the engine has no business knowing: that the stored `u32` is a
//! duration, that the caller measures it as an `f64`, and that the conversion
//! happens on the near side of the comparison.

use crcbl::store::record::{Backing, Record};

/// The application directory. `~/.config/horde/` on Linux.
///
/// Native only: the browser has no directory to name, and its store arrives
/// from the shim already open.
#[cfg(not(target_arch = "wasm32"))]
const APP: &str = "horde";

/// The file inside it.
const BEST_FILE: &str = "best.bin";

/// The longest run this player has survived, in whole seconds.
#[derive(Debug)]
pub struct Best {
    record: Record,
}

impl Best {
    /// Loads the best run. Headless runs are in-memory only.
    #[must_use]
    pub fn load(headless: bool) -> Self {
        let backing = if headless {
            Backing::None
        } else {
            platform_backing()
        };
        Self {
            record: Record::open(backing, BEST_FILE),
        }
    }

    /// The longest run so far, in whole seconds.
    #[must_use]
    pub fn get(&self) -> u32 {
        self.record.get()
    }

    /// Records a run of `elapsed` simulated seconds if it beats the record.
    /// Reports whether it did.
    ///
    /// Truncated to whole seconds **before** the comparison, for the reason the
    /// module header gives. A negative or non-finite `elapsed` cannot come from
    /// the simulation and is refused outright rather than cast: `as u32` on a
    /// NaN is zero and on an infinity is `u32::MAX`, and the second of those
    /// would write a record no run can ever beat.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn update(&mut self, elapsed: f64) -> bool {
        if !elapsed.is_finite() || elapsed <= 0.0 {
            return false;
        }
        // Saturating, as every float-to-integer cast in Rust is since 1.45, so
        // a run longer than 136 years reports 136 years rather than wrapping.
        let seconds = elapsed as u32;
        if self.record.raise(seconds) {
            crcbl::log::info!("best: new best run = {}s", self.get());
            return true;
        }
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn platform_backing() -> Backing {
    Backing::config(APP)
}

#[cfg(target_arch = "wasm32")]
fn platform_backing() -> Backing {
    match crate::web::opfs_store() {
        Some(store) => Backing::Browser(store),
        None => {
            crcbl::log::warn!("best: no OPFS store installed; runs will not persist");
            Backing::None
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The truncation is this module's whole reason for existing, so it is what
    /// is asserted: a run that beats the record by less than a second must not
    /// count, because the clock the player reads would not have changed.
    #[test]
    fn a_run_that_does_not_gain_a_whole_second_is_not_a_new_best() {
        let mut best = Best::load(true);
        assert!(best.update(130.9), "the first run is always a best");
        assert_eq!(best.get(), 130, "the stored value is whole seconds");

        assert!(!best.update(130.99), "a tenth of a second is not a second");
        assert!(!best.update(130.0), "and neither is nothing");
        assert!(best.update(131.0), "a whole second is");
        assert_eq!(best.get(), 131);
    }

    /// A cast is what makes these dangerous: `f64::NAN as u32` is 0 and
    /// `f64::INFINITY as u32` is `u32::MAX`, and writing the latter would set a
    /// record no run could ever beat.
    #[test]
    fn a_run_that_is_not_a_real_duration_is_refused_rather_than_cast() {
        let mut best = Best::load(true);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
            assert!(!best.update(bad), "{bad} was accepted as a run length");
        }
        assert_eq!(best.get(), 0, "one of them reached the record");
    }

    /// The headless rule is this module's own — `Record` writes to whatever
    /// backing it is handed, and choosing `None` here is what stops the test
    /// suite writing into a developer's real config directory.
    #[test]
    fn a_headless_run_keeps_its_best_in_memory_and_writes_nothing() {
        let mut best = Best::load(true);
        assert!(best.update(42.0));
        assert_eq!(best.get(), 42);
        assert_eq!(
            Best::load(true).get(),
            0,
            "a headless run left a file behind"
        );
    }
}
