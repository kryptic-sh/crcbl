//! Best-score persistence — one `u32`, wherever the platform keeps such things.
//!
//! | Target | Where | How |
//! | --- | --- | --- |
//! | native, windowed | `~/.config/asteroids/best.bin` | [`crcbl_store::write_atomic`] |
//! | native, `--headless` | nowhere | in memory, so a CI run leaves no trace |
//! | `wasm32` | the Origin Private File System | `crcbl_store::web::OpfsStorage` |
//!
//! # This is the *third* copy of this file
//!
//! S1B finding 4 in `docs/plan/ROADMAP.md` says `crcbl-store` has no "one
//! number, kept between sessions": it gives a `StorageSource` and an atomic
//! write and leaves the platform arms, the encode, the corrupt-file case and the
//! headless-means-nowhere rule to be written out again. Two games with nothing
//! else in common made that a gap rather than a preference; a third confirms it
//! and adds one thing the second could not.
//!
//! **The two existing copies do not agree on what any of it is called.**
//! `apps/breakout/src/high_score.rs` holds `HighScore` and writes
//! `high_score.bin`; `apps/flappy/src/best.rs` holds `Best` and writes
//! `best.bin`. The bodies match line for line — the same `Backing` enum, the
//! same three arms, the same `<[u8; 4]>::try_from`, the same "queued, not
//! written" note on the OPFS arm — and the module, the type and the file on disk
//! are all named differently, so a reader of either has no way to know which is
//! "the" pattern. This copy follows flappy's, because it is the more recent, and
//! that is the whole of the reason: with nothing shared, the pattern is whichever
//! copy you happened to open. Owed by P10, where the settings UI wants the same
//! thing a fourth time.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

const BEST_FILE: &str = "best.bin";

/// Where the score is kept, or [`Backing::None`] for a run that keeps it
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

/// The best score this player has reached.
#[derive(Debug)]
pub struct Best {
    backing: Backing,
    value: u32,
}

impl Best {
    /// Loads the best score. Headless runs are in-memory only.
    #[must_use]
    pub fn load(headless: bool) -> Self {
        let backing = if headless {
            Backing::None
        } else {
            Self::platform_backing()
        };
        let value = Self::read(&backing).unwrap_or(0);
        Self { backing, value }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn platform_backing() -> Backing {
        match crcbl_store::NativeStorage::config("asteroids") {
            Ok(store) => Backing::Native(store.root().to_path_buf()),
            Err(_) => {
                log::warn!("best: no config dir; scores will not persist");
                Backing::None
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn platform_backing() -> Backing {
        match crate::web::opfs_store() {
            Some(store) => Backing::Opfs(store),
            None => {
                log::warn!("best: no OPFS store installed; scores will not persist");
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
        let value = Self::read(&backing).unwrap_or(0);
        Self { backing, value }
    }

    /// The best score so far.
    #[must_use]
    pub const fn get(&self) -> u32 {
        self.value
    }

    /// Records `score` if it beats the best. Reports whether it did.
    pub fn update(&mut self, score: u32) -> bool {
        if score <= self.value {
            return false;
        }
        self.value = score;
        self.save();
        log::info!("best: new best = {}", self.value);
        true
    }

    /// Writes the current value out, if there is anywhere to write it.
    ///
    /// The browser arm returns as soon as the record is *queued*: OPFS has no
    /// synchronous path to the disk and the shim performs the write later, on
    /// `visibilitychange` and `beforeunload`, so a player who closes the tab on
    /// a new best still keeps it.
    fn save(&self) {
        match &self.backing {
            Backing::None => {}
            #[cfg(not(target_arch = "wasm32"))]
            Backing::Native(root) => {
                let path = root.join(BEST_FILE);
                if let Err(e) = crcbl_store::write_atomic(&path, &self.value.to_le_bytes()) {
                    log::warn!("best: save failed ({e})");
                }
            }
            #[cfg(target_arch = "wasm32")]
            Backing::Opfs(store) => {
                use crcbl_store::StorageSource as _;
                if let Err(e) =
                    store.write(std::path::Path::new(BEST_FILE), &self.value.to_le_bytes())
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

    /// A best score survives the process that set it.
    #[test]
    fn a_best_score_is_written_and_read_back() {
        let dir = scratch("crcbl_asteroids_best");

        let mut best = Best::with_root(dir.clone());
        assert_eq!(best.get(), 0);
        assert!(best.update(150));
        assert!(!best.update(90), "a worse run must not overwrite the best");
        assert!(!best.update(150), "and neither must an equal one");
        assert!(best.update(1_020));

        assert_eq!(
            Best::with_root(dir.clone()).get(),
            1_020,
            "the next session did not see the best"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is not four bytes is corruption, not a score.
    #[test]
    fn a_wrong_length_file_reads_as_no_score() {
        let dir = scratch("crcbl_asteroids_best_corrupt");
        std::fs::write(dir.join(BEST_FILE), [1, 2, 3]).expect("write");
        assert_eq!(Best::with_root(dir.clone()).get(), 0);
        // And a longer file is corruption too, not a value with a tail: a
        // `try_from` on the first four bytes would read one and be wrong.
        std::fs::write(dir.join(BEST_FILE), [1, 2, 3, 4, 5]).expect("write");
        assert_eq!(Best::with_root(dir.clone()).get(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A headless run keeps the score in memory and touches nothing.
    #[test]
    fn a_headless_run_persists_nothing() {
        let mut best = Best::load(true);
        assert!(matches!(best.backing, Backing::None));
        assert!(best.update(7));
        assert_eq!(best.get(), 7);
    }
}
