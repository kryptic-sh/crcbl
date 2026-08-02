//! Best-run persistence — one `u32`, wherever the platform keeps such things.
//!
//! | Target | Where | How |
//! | --- | --- | --- |
//! | native, windowed | `~/.config/horde/best.bin` | [`crcbl_store::write_atomic`] |
//! | native, `--headless` | nowhere | in memory, so a CI run leaves no trace |
//! | `wasm32` | the Origin Private File System | `crcbl_store::web::OpfsStorage` |
//!
//! # The number is a *time*, and that is this sample's own choice
//!
//! The plan's exit criterion is "survive five minutes", so the run's score is
//! how long it lasted. It is stored in **whole seconds** because that is what
//! the HUD's `m:ss` clock shows — a best that read `2:13` and compared as
//! `133.4187` would be a number the player could beat without the display
//! changing. `Best::update` therefore truncates before it compares, and a run
//! that beats the record by a tenth of a second correctly does not.
//!
//! # This is the *fourth* copy of this file
//!
//! S1B finding 4 in `docs/plan/ROADMAP.md` says `crcbl-store` has no "one
//! number, kept between sessions": it gives a `StorageSource` and an atomic
//! write and leaves the platform arms, the encode, the corrupt-file case and the
//! headless-means-nowhere rule to be written out again. Two games with nothing
//! else in common made that a gap rather than a preference; the third confirmed
//! it; the fourth is where the *names* stopped agreeing about anything but the
//! bodies.
//!
//! `apps/breakout/src/high_score.rs` holds `HighScore` and writes
//! `high_score.bin`; `apps/flappy/src/best.rs` and `apps/asteroids/src/best.rs`
//! hold `Best` and write `best.bin`; this holds `Best`, writes `best.bin`, and
//! is the first whose stored number is not a score at all. The bodies still
//! match line for line — the same `Backing` enum, the same three arms, the same
//! `<[u8; 4]>::try_from`, the same "queued, not written" note on the OPFS arm —
//! so the one thing four copies prove is that the *policy* differs per game and
//! the *plumbing* never does, which is exactly the shape of the missing engine
//! API. Owed by P10, where the settings UI wants the same thing a fifth time.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

const BEST_FILE: &str = "best.bin";

/// Where the record is kept, or [`Backing::None`] for a run that keeps it
/// nowhere.
#[derive(Debug)]
enum Backing {
    /// Headless, or a platform that would not say where to write.
    None,
    /// A directory on a real filesystem.
    #[cfg(not(target_arch = "wasm32"))]
    Native(PathBuf),
    /// The page's Origin Private File System, drained by the JS shim.
    #[cfg(target_arch = "wasm32")]
    Opfs(std::rc::Rc<crcbl_store::web::OpfsStorage>),
}

/// The longest run this player has survived, in whole seconds.
#[derive(Debug)]
pub struct Best {
    backing: Backing,
    seconds: u32,
}

impl Best {
    /// Loads the record. Headless runs are in-memory only.
    #[must_use]
    pub fn load(headless: bool) -> Self {
        let backing = if headless {
            Backing::None
        } else {
            Self::platform_backing()
        };
        let seconds = Self::read(&backing).unwrap_or(0);
        Self { backing, seconds }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn platform_backing() -> Backing {
        match crcbl_store::NativeStorage::config("horde") {
            Ok(store) => Backing::Native(store.root().to_path_buf()),
            Err(_) => {
                log::warn!("best: no config dir; the record will not persist");
                Backing::None
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn platform_backing() -> Backing {
        match crate::web::opfs_store() {
            Some(store) => Backing::Opfs(store),
            None => {
                log::warn!("best: no OPFS store installed; the record will not persist");
                Backing::None
            }
        }
    }

    /// Decodes the stored value, or `None` when there is not one to decode.
    ///
    /// A wrong length is corruption and is reported; every other absence — no
    /// file, no store, a browser whose restore has not finished — is the
    /// ordinary first-run case.
    fn read(backing: &Backing) -> Option<u32> {
        let data = match backing {
            Backing::None => return None,
            #[cfg(not(target_arch = "wasm32"))]
            Backing::Native(root) => match std::fs::read(root.join(BEST_FILE)) {
                Ok(data) => data,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
                Err(e) => {
                    log::warn!("best: read error ({e})");
                    return None;
                }
            },
            #[cfg(target_arch = "wasm32")]
            Backing::Opfs(store) => {
                use crcbl_store::StorageSource as _;
                match store.read(std::path::Path::new(BEST_FILE)) {
                    Ok(data) => data,
                    Err(e) => {
                        log::info!("best: no previous save ({e})");
                        return None;
                    }
                }
            }
        };
        match <[u8; 4]>::try_from(data.as_slice()) {
            Ok(bytes) => Some(u32::from_le_bytes(bytes)),
            Err(_) => {
                log::warn!("best: corrupt file ({} bytes)", data.len());
                None
            }
        }
    }

    /// Loads with a specific root directory, for tests.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn with_root(root: PathBuf) -> Self {
        let backing = Backing::Native(root);
        let seconds = Self::read(&backing).unwrap_or(0);
        Self { backing, seconds }
    }

    /// The longest run so far, in whole seconds.
    #[must_use]
    pub const fn get(&self) -> u32 {
        self.seconds
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
        if seconds <= self.seconds {
            return false;
        }
        self.seconds = seconds;
        self.save();
        log::info!("best: new best run = {}s", self.seconds);
        true
    }

    /// Writes the current value out, if there is anywhere to write it.
    ///
    /// The browser arm returns as soon as the record is *queued*: OPFS has no
    /// synchronous path to the disk and the shim performs the write later, on
    /// `visibilitychange` and `beforeunload`, so a player who closes the tab on
    /// a new record still keeps it.
    fn save(&self) {
        match &self.backing {
            Backing::None => {}
            #[cfg(not(target_arch = "wasm32"))]
            Backing::Native(root) => {
                let path = root.join(BEST_FILE);
                if let Err(e) = crcbl_store::write_atomic(&path, &self.seconds.to_le_bytes()) {
                    log::warn!("best: save failed ({e})");
                }
            }
            #[cfg(target_arch = "wasm32")]
            Backing::Opfs(store) => {
                use crcbl_store::StorageSource as _;
                if let Err(e) =
                    store.write(std::path::Path::new(BEST_FILE), &self.seconds.to_le_bytes())
                {
                    log::warn!("best: save failed ({e})");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// A record survives the process that set it.
    #[test]
    fn a_best_run_is_written_and_read_back() {
        let dir = scratch("crcbl_horde_best");

        let mut best = Best::with_root(dir.clone());
        assert_eq!(best.get(), 0);
        assert!(best.update(150.0));
        assert!(
            !best.update(90.0),
            "a worse run must not overwrite the best"
        );
        assert!(!best.update(150.0), "and neither must an equal one");
        assert!(best.update(1_020.4));

        assert_eq!(
            Best::with_root(dir.clone()).get(),
            1_020,
            "the next session did not see the record"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Whole seconds, compared as whole seconds.**
    ///
    /// The one behaviour that is this sample's rather than the shared body's: a
    /// run that beats the record by a tenth of a second does not beat the
    /// number the HUD is showing, so it is not a record. A `update(f64)` that
    /// stored the raw value would report `true` here and change nothing on
    /// screen.
    #[test]
    fn a_tenth_of_a_second_is_not_a_new_record() {
        let dir = scratch("crcbl_horde_best_tenths");
        let mut best = Best::with_root(dir.clone());
        assert!(best.update(133.9));
        assert_eq!(best.get(), 133);
        assert!(!best.update(133.999), "a tenth of a second beat the record");
        assert!(best.update(134.0));
        assert_eq!(best.get(), 134);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nothing the simulation cannot produce gets stored as a record.
    ///
    /// An infinity is the one that matters: `f64::INFINITY as u32` saturates to
    /// `u32::MAX`, so a version that cast first would store a record of 136
    /// years and no run would ever beat it again.
    #[test]
    fn a_nonsense_elapsed_is_not_a_record() {
        let mut best = Best::load(true);
        for bad in [f64::NAN, -1.0, 0.0, f64::NEG_INFINITY, f64::INFINITY] {
            assert!(!best.update(bad), "{bad} was taken as a record");
        }
        assert_eq!(best.get(), 0);
        // …and a finite run still is, or the guard above is a mute button.
        assert!(best.update(12.0));
        assert_eq!(best.get(), 12);
    }

    /// A file that is not four bytes is corruption, not a record.
    #[test]
    fn a_wrong_length_file_reads_as_no_record() {
        let dir = scratch("crcbl_horde_best_corrupt");
        std::fs::write(dir.join(BEST_FILE), [1, 2, 3]).expect("write");
        assert_eq!(Best::with_root(dir.clone()).get(), 0);
        // And a longer file is corruption too, not a value with a tail: a
        // `try_from` on the first four bytes would read one and be wrong.
        std::fs::write(dir.join(BEST_FILE), [1, 2, 3, 4, 5]).expect("write");
        assert_eq!(Best::with_root(dir.clone()).get(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A headless run keeps the record in memory and touches nothing.
    #[test]
    fn a_headless_run_persists_nothing() {
        let mut best = Best::load(true);
        assert!(matches!(best.backing, Backing::None));
        assert!(best.update(7.0));
        assert_eq!(best.get(), 7);
    }
}
