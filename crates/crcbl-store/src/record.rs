//! One number, kept between sessions, wherever the platform keeps such things.
//!
//! ```text
//!  native, windowed  ──▶ ~/.config/<app>/<file>   write_atomic
//!  native, headless  ──▶ nowhere                  in memory, no trace
//!  wasm32            ──▶ the Origin Private File System
//! ```
//!
//! # Why this is the engine's and not a game's
//!
//! The rest of this crate hands out a [`crate::StorageSource`],
//! an atomic write and a
//! platform-standard root, and stops there — so a game wanting a high score
//! wrote the platform arms, the encode, the corrupt-file case and the
//! headless-writes-nowhere rule itself. Four samples did, and the bodies
//! matched line for line while the *names* agreed about nothing: `HighScore` in
//! `high_score.bin`, `Best` in `best.bin`, and — in `apps/horde` — a `Best`
//! whose number is not a score at all but a run length in whole seconds.
//!
//! Four copies proved the split: the **policy** differs per game and the
//! **plumbing** never does. This is the plumbing.
//!
//! # What stays the game's
//!
//! What the number *means*, and when it is worth keeping. [`Record::raise`]
//! implements "keep it if it is larger", which is what all four wanted, but a
//! game that measures a best lap wants the smaller one and writes
//! [`Record::set`] behind its own comparison. Horde's truncation of a float
//! run-time to whole seconds happens before it gets here, and has to: a best
//! that displayed as `2:13` and compared as `133.4187` would be a record the
//! player could beat without the display changing.

// Only the browser arms use these: on native the reads and writes go to
// `std::fs` and `crate::write_atomic`, which take the paths directly.
#[cfg(target_arch = "wasm32")]
use std::path::Path;

#[cfg(target_arch = "wasm32")]
use crate::StorageSource;

/// Where a [`Record`] is kept.
///
/// Public because a browser build has to supply its own store — the shim
/// installs the OPFS handle and the engine has no way to reach for it — and
/// because "nowhere" is a state a caller chooses rather than a failure.
#[derive(Debug)]
pub enum Backing {
    /// Kept in memory only, leaving no trace. Headless runs use this, so a CI
    /// run cannot write into a developer's real config directory.
    None,
    /// A directory on a real filesystem.
    #[cfg(not(target_arch = "wasm32"))]
    Native(std::path::PathBuf),
    /// A browser storage source, typically the Origin Private File System.
    ///
    /// Boxed rather than generic so [`Record`] stays one type: a game holds it
    /// in a struct field, and a type parameter that only ever has one value
    /// would spread through every signature above it.
    #[cfg(target_arch = "wasm32")]
    Browser(std::rc::Rc<dyn StorageSource>),
}

impl Backing {
    /// The platform-standard config directory for `app_name`, or
    /// [`Backing::None`] if the platform will not name one.
    ///
    /// Native only. A browser build constructs `Backing::Browser` from the
    /// store its shim restored into.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn config(app_name: &str) -> Self {
        match crate::NativeStorage::config(app_name) {
            Ok(store) => Self::Native(store.root().to_path_buf()),
            Err(error) => {
                log::warn!("record: no config dir ({error}); values will not persist");
                Self::None
            }
        }
    }
}

/// A single `u32`, loaded at construction and written when it changes.
///
/// The value is held in memory and answered from there, so reading it is free
/// and a failed write costs the player nothing this session.
#[derive(Debug)]
pub struct Record {
    backing: Backing,
    file: String,
    value: u32,
}

impl Record {
    /// Opens `file` under `backing`, reading the stored value or starting at 0.
    ///
    /// Absence is the ordinary first-run case and is silent — no file, no
    /// store, a browser whose restore has not finished. A file of the **wrong
    /// length** is corruption and is logged, because that one is a bug
    /// somewhere rather than a new player.
    #[must_use]
    pub fn open(backing: Backing, file: &str) -> Self {
        let value = Self::read(&backing, file).unwrap_or(0);
        Self {
            backing,
            file: file.to_owned(),
            value,
        }
    }

    fn read(backing: &Backing, file: &str) -> Option<u32> {
        let data = match backing {
            Backing::None => return None,
            #[cfg(not(target_arch = "wasm32"))]
            Backing::Native(root) => match std::fs::read(root.join(file)) {
                Ok(data) => data,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
                Err(error) => {
                    log::warn!("record: read error ({error})");
                    return None;
                }
            },
            #[cfg(target_arch = "wasm32")]
            Backing::Browser(store) => match store.read(Path::new(file)) {
                Ok(data) => data,
                Err(error) => {
                    log::info!("record: no previous value ({error})");
                    return None;
                }
            },
        };

        match <[u8; 4]>::try_from(data.as_slice()) {
            Ok(bytes) => Some(u32::from_le_bytes(bytes)),
            Err(_) => {
                log::warn!("record: corrupt file ({} bytes)", data.len());
                None
            }
        }
    }

    /// The value.
    #[must_use]
    pub const fn get(&self) -> u32 {
        self.value
    }

    /// Records `value` if it beats what is stored. Reports whether it did.
    ///
    /// "Beats" is strictly greater, so re-recording the same number does not
    /// write — which is what keeps a game that calls this every frame from
    /// writing every frame.
    pub fn raise(&mut self, value: u32) -> bool {
        if value <= self.value {
            return false;
        }
        self.set(value);
        true
    }

    /// Records `value` unconditionally.
    ///
    /// For the game whose "better" is not "larger" — a best lap time, a lowest
    /// stroke count — which compares for itself and then writes.
    pub fn set(&mut self, value: u32) {
        self.value = value;
        self.save();
    }

    /// Writes the current value out, if there is anywhere to write it.
    ///
    /// The browser arm returns as soon as the write is *queued*: OPFS has no
    /// synchronous path to the disk, and the shim performs it later on
    /// `visibilitychange` and `beforeunload`, so a player who closes the tab on
    /// a new record still keeps it.
    fn save(&self) {
        let bytes = self.value.to_le_bytes();
        match &self.backing {
            Backing::None => {}
            #[cfg(not(target_arch = "wasm32"))]
            Backing::Native(root) => {
                if let Err(error) = crate::write_atomic(&root.join(&self.file), &bytes) {
                    log::warn!("record: save failed ({error})");
                }
            }
            #[cfg(target_arch = "wasm32")]
            Backing::Browser(store) => {
                if let Err(error) = store.write(Path::new(&self.file), &bytes) {
                    log::warn!("record: save failed ({error})");
                }
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// A directory of this test's own, named for the test, so two running at
    /// once cannot see each other's files.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("crcbl-record-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir is writable");
        dir
    }

    #[test]
    fn a_raised_value_survives_a_reopen() {
        let dir = scratch("survives");
        let mut record = Record::open(Backing::Native(dir.clone()), "best.bin");
        assert_eq!(record.get(), 0, "nothing stored yet");
        assert!(record.raise(42), "42 beats nothing");

        let reopened = Record::open(Backing::Native(dir.clone()), "best.bin");
        assert_eq!(reopened.get(), 42, "the value did not reach the disk");
        std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
    }

    /// The write is what `raise` is for, so the check is that a *losing* value
    /// does not reach the file — not merely that the in-memory number is
    /// unchanged, which a no-op `raise` would also satisfy.
    #[test]
    fn a_value_that_does_not_beat_the_record_is_not_written() {
        let dir = scratch("not-written");
        let mut record = Record::open(Backing::Native(dir.clone()), "best.bin");
        assert!(record.raise(10));

        assert!(!record.raise(10), "equal is not better");
        assert!(!record.raise(3), "smaller is not better");
        assert_eq!(record.get(), 10);
        assert_eq!(
            Record::open(Backing::Native(dir.clone()), "best.bin").get(),
            10,
            "a losing value reached the file"
        );
        std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
    }

    /// `set` is the escape hatch for a game whose better is smaller, so it has
    /// to write a value `raise` would have refused.
    #[test]
    fn set_writes_a_value_raise_would_have_refused() {
        let dir = scratch("set");
        let mut record = Record::open(Backing::Native(dir.clone()), "best.bin");
        record.set(100);
        record.set(7);
        assert_eq!(
            Record::open(Backing::Native(dir.clone()), "best.bin").get(),
            7,
            "set did not overwrite with the smaller value"
        );
        std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
    }

    /// A headless run leaves nothing behind — the property that lets CI run the
    /// samples without writing into whoever's config directory.
    #[test]
    fn a_headless_record_keeps_its_value_and_writes_nothing() {
        let dir = scratch("headless");
        let mut record = Record::open(Backing::None, "best.bin");
        assert!(record.raise(5));
        assert_eq!(record.get(), 5, "it still answers in memory");
        assert!(
            std::fs::read_dir(&dir)
                .expect("the scratch directory exists")
                .next()
                .is_none(),
            "a headless record wrote a file"
        );
        std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
    }

    /// Wrong length is corruption and reads as absent rather than as a
    /// truncated number — a 3-byte file interpreted as a `u32` would be a
    /// silently wrong record.
    #[test]
    fn a_wrong_length_file_reads_as_absent() {
        let dir = scratch("corrupt");
        std::fs::write(dir.join("best.bin"), [1, 2, 3]).expect("the scratch dir is writable");
        assert_eq!(
            Record::open(Backing::Native(dir.clone()), "best.bin").get(),
            0,
            "three bytes were read as a value"
        );
        std::fs::remove_dir_all(&dir).expect("the scratch directory is this test's");
    }
}
