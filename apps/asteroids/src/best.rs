//! Where asteroids's best score is kept.
//!
//! | Target | Where |
//! | --- | --- |
//! | native, windowed | `~/.config/asteroids/best.bin` |
//! | native, `--headless` | nowhere, so a CI run leaves no trace |
//! | `wasm32` | the Origin Private File System |
//!
//! # This file used to be two hundred lines
//!
//! It held a `Backing` enum, three platform arms, the little-endian encode, the
//! corrupt-file case and the headless rule — and so did the same file in the
//! other three samples, line for line, under different type names. All of it is
//! [`crcbl::store::record::Record`] now.
//!
//! What is left is what genuinely is this game's, and it is what the engine
//! could not have guessed: **which application directory, which file name, and
//! — in the browser — which store**. The OPFS handle is installed by this
//! sample's own shim before the restore pass runs, and nothing in `crcbl-store`
//! can reach for it.

use crcbl::store::record::{Backing, Record};

/// The application directory. `~/.config/asteroids/` on Linux.
///
/// Native only: the browser has no directory to name, and its store arrives
/// from the shim already open.
#[cfg(not(target_arch = "wasm32"))]
const APP: &str = "asteroids";

/// The file inside it.
const BEST_FILE: &str = "best.bin";

/// Opens the best score, or an in-memory one for a headless run.
#[must_use]
pub fn open(headless: bool) -> Record {
    let backing = if headless {
        Backing::None
    } else {
        platform_backing()
    };
    Record::open(backing, BEST_FILE)
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
            crcbl::log::warn!("best: no OPFS store installed; scores will not persist");
            Backing::None
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The headless rule is this module's own — `Record` writes to whatever
    /// backing it is handed, and choosing `None` here is what stops the test
    /// suite writing into a developer's real config directory.
    #[test]
    fn a_headless_run_keeps_its_best_in_memory_and_writes_nothing() {
        let mut best = open(true);
        assert_eq!(best.get(), 0);
        assert!(best.raise(500), "it still tracks the best in memory");
        assert_eq!(best.get(), 500);

        // Nothing was written, so a second headless open starts over.
        assert_eq!(open(true).get(), 0, "a headless run left a file behind");
    }
}
