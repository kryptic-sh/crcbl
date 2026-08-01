//! High-score persistence — one `u32`, wherever the platform keeps such things.
//!
//! | Target | Where | How |
//! | --- | --- | --- |
//! | native, windowed | `~/.config/breakout/high_score.bin` | [`crcbl_store::write_atomic`] |
//! | native, `--headless` | nowhere | in memory, so a CI run leaves no trace |
//! | `wasm32` | the Origin Private File System | [`crcbl_store::web::OpfsStorage`] |
//!
//! # Why the browser needed its own arm
//!
//! This file used to name [`crcbl_store::NativeStorage`] unconditionally. That
//! *compiles* for `wasm32-unknown-unknown` — `dirs` has a wasm build — and then
//! answers `None` for every path it is asked for, so the browser build reached
//! run time with persistence that could only ever fail, silently, once per
//! game over. P5.7 built [`OpfsStorage`](crcbl_store::web::OpfsStorage) for
//! exactly this and left the wiring here.
//!
//! The OPFS store is created and installed by [`crate::web`] before the shim's
//! restore pass runs, so by the time [`HighScore::load`] asks for it the value
//! is already resident and the read is a map lookup. A page whose shim never
//! restored answers [`StorageError::Pending`](crcbl_store::StorageError::Pending)
//! instead, which is treated as "no previous save" for reading — and *not* as a
//! reason to skip writing, because the write path is what makes the next
//! session's read succeed.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

const HIGH_SCORE_FILE: &str = "high_score.bin";

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

/// Manages a single persistent high score value.
#[derive(Debug)]
pub struct HighScore {
    backing: Backing,
    value: u32,
}

impl HighScore {
    /// Load the high score. Headless runs are in-memory only.
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
        match crcbl_store::NativeStorage::config("breakout") {
            Ok(store) => Backing::Native(store.root().to_path_buf()),
            Err(_) => {
                log::warn!("high_score: no config dir; scores will not persist");
                Backing::None
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn platform_backing() -> Backing {
        match crate::web::opfs_store() {
            Some(store) => Backing::Opfs(store),
            None => {
                log::warn!("high_score: no OPFS store installed; scores will not persist");
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
            Backing::Native(root) => match std::fs::read(root.join(HIGH_SCORE_FILE)) {
                Ok(data) => data,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    log::info!("high_score: no previous save");
                    return None;
                }
                Err(e) => {
                    log::warn!("high_score: read error ({e})");
                    return None;
                }
            },
            #[cfg(target_arch = "wasm32")]
            Backing::Opfs(store) => {
                use crcbl_store::StorageSource as _;
                match store.read(std::path::Path::new(HIGH_SCORE_FILE)) {
                    Ok(data) => data,
                    Err(e) => {
                        log::info!("high_score: no previous save ({e})");
                        return None;
                    }
                }
            }
        };
        match <[u8; 4]>::try_from(data.as_slice()) {
            Ok(bytes) => Some(u32::from_le_bytes(bytes)),
            Err(_) => {
                log::warn!("high_score: corrupt file ({} bytes)", data.len());
                None
            }
        }
    }

    /// Load with a specific root directory (for testing).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn with_root(root: std::path::PathBuf) -> Self {
        let backing = Backing::Native(root);
        let value = Self::read(&backing).unwrap_or(0);
        Self { backing, value }
    }

    /// Current best score.
    pub fn get(&self) -> u32 {
        self.value
    }

    /// Update the high score if `score` exceeds it. Returns `true` on record.
    pub fn update(&mut self, score: u32) -> bool {
        if score <= self.value {
            return false;
        }
        self.value = score;
        self.save();
        log::info!("high_score: new best = {}", self.value);
        true
    }

    /// Writes the current value out, if there is anywhere to write it.
    ///
    /// The browser arm returns as soon as the record is *queued*: OPFS has no
    /// synchronous path to the disk and the shim performs the write later.
    /// `OpfsStats::queued + in_flight == 0` is how a caller learns it landed;
    /// the shim drains on `visibilitychange` and `beforeunload` so a player who
    /// closes the tab on a new best still keeps it.
    fn save(&self) {
        match &self.backing {
            Backing::None => {}
            #[cfg(not(target_arch = "wasm32"))]
            Backing::Native(root) => {
                let path = root.join(HIGH_SCORE_FILE);
                if let Err(e) = crcbl_store::write_atomic(&path, &self.value.to_le_bytes()) {
                    log::warn!("high_score: save failed ({e})");
                }
            }
            #[cfg(target_arch = "wasm32")]
            Backing::Opfs(store) => {
                use crcbl_store::StorageSource as _;
                let path = std::path::Path::new(HIGH_SCORE_FILE);
                if let Err(e) = store.write(path, &self.value.to_le_bytes()) {
                    log::warn!("high_score: save failed ({e})");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_score_persists_and_reloads() {
        let dir = std::env::temp_dir().join("crcbl_breakout_test_hs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        // First session: score 50, verify it saves.
        let mut hs = HighScore::with_root(dir.clone());
        assert_eq!(hs.get(), 0);
        assert!(hs.update(50));
        assert_eq!(hs.get(), 50);

        // Same score should not update.
        assert!(!hs.update(40));
        assert_eq!(hs.get(), 50);

        // Higher score should update.
        assert!(hs.update(100));
        assert_eq!(hs.get(), 100);

        // Second session: reload from disk, verify value persisted.
        let hs2 = HighScore::with_root(dir.clone());
        assert_eq!(hs2.get(), 100);

        // Clean up.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is not four bytes is corruption, not a score.
    ///
    /// The old decoder matched `data.len() == 4` and then `unwrap`ped a slice
    /// conversion that could not fail; this one converts first, so the two can
    /// never disagree about what "four bytes" means.
    #[test]
    fn a_wrong_length_file_reads_as_no_score() {
        let dir = std::env::temp_dir().join("crcbl_breakout_test_hs_corrupt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        std::fs::write(dir.join(HIGH_SCORE_FILE), [1, 2, 3]).expect("write");

        assert_eq!(HighScore::with_root(dir.clone()).get(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A headless run keeps the score in memory and touches nothing.
    #[test]
    fn a_headless_run_persists_nothing() {
        let mut hs = HighScore::load(true);
        assert!(matches!(hs.backing, Backing::None));
        assert!(hs.update(7));
        assert_eq!(hs.get(), 7);
    }
}
