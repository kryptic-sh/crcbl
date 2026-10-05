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
//! The platform arms came in last, as [`Backing::platform`]: each game had also
//! written out "config directory natively, the shim's OPFS store in a browser",
//! which is a fact about the platform and not about any game.
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

use crate::StorageError;
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
                crcbl_core::log::warn!("store: no config dir ({error}); values will not persist");
                Self::None
            }
        }
    }

    /// Where this platform keeps a small persistent value, whatever it is.
    ///
    /// One rule about the platform, in one place: the config directory
    /// natively, the installed OPFS store in a browser, and [`Backing::None`]
    /// when neither can be had — which is the ordinary first-run and
    /// no-shim case, not a failure the caller has to handle.
    ///
    /// `app_name` names the directory and so means nothing in a browser, where
    /// the origin already is the namespace. It stays in the signature rather
    /// than splitting this into two per-platform functions, because a signature
    /// per platform pushes the `#[cfg]` back out to every game that calls it —
    /// which is precisely the four hand-written copies this replaces.
    #[must_use]
    pub fn platform(app_name: &str) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self::config(app_name)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = app_name;
            match crate::web::opfs::installed() {
                Some(store) => Self::Browser(store),
                None => {
                    crcbl_core::log::warn!(
                        "store: no OPFS store installed; values will not persist"
                    );
                    Self::None
                }
            }
        }
    }

    /// The bytes of `file` under this backing, or `None` when there are none
    /// to read: no file yet, which is the ordinary first run, or
    /// [`Backing::None`], which never holds anything.
    ///
    /// Shared by every kind of value kept here — a [`Record`]'s number and a
    /// [`Profile`](crate::profile::Profile)'s file — so the platform arms are
    /// written once and the two cannot come to disagree about what absence is.
    ///
    /// # Errors
    ///
    /// Whatever the backend reports for a read that is neither a hit nor a
    /// missing file.
    pub(crate) fn read(&self, file: &str) -> Result<Option<Vec<u8>>, StorageError> {
        match self {
            Self::None => Ok(None),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(root) => {
                let path = root.join(file);
                match std::fs::read(&path) {
                    Ok(data) => Ok(Some(data)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(StorageError::from_io(&path, error)),
                }
            }
            #[cfg(target_arch = "wasm32")]
            Self::Browser(store) => match store.read(Path::new(file)) {
                Ok(data) => Ok(Some(data)),
                Err(StorageError::NotFound(_)) => Ok(None),
                Err(error) => Err(error),
            },
        }
    }

    /// Writes `bytes` to `file` under this backing: through [`crate::write_atomic`]
    /// natively, into the store in a browser, and nowhere for
    /// [`Backing::None`]. Answers whether anything was written.
    ///
    /// The browser arm returns as soon as the write is *queued*: OPFS has no
    /// synchronous path to the disk, and the shim performs it later on
    /// `visibilitychange` and `beforeunload`, so a player who closes the tab
    /// straight after still keeps it.
    ///
    /// # Errors
    ///
    /// Whatever the backend reports for a write it refused.
    pub(crate) fn write(&self, file: &str, bytes: &[u8]) -> Result<bool, StorageError> {
        match self {
            Self::None => Ok(false),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(root) => crate::write_atomic(&root.join(file), bytes).map(|()| true),
            #[cfg(target_arch = "wasm32")]
            Self::Browser(store) => store.write(Path::new(file), bytes).map(|()| true),
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
        let data = match backing.read(file) {
            Ok(Some(data)) => data,
            Ok(None) => return None,
            Err(error) => {
                crcbl_core::log::warn!("record: read error ({error})");
                return None;
            }
        };

        match <[u8; 4]>::try_from(data.as_slice()) {
            Ok(bytes) => Some(u32::from_le_bytes(bytes)),
            Err(_) => {
                crcbl_core::log::warn!("record: corrupt file ({} bytes)", data.len());
                None
            }
        }
    }

    /// Opens `file` for `app`, or an in-memory record when `headless`.
    ///
    /// The headless rule, which every sample that keeps a number wanted and
    /// each of them wrote out: a run with no window must leave nothing behind,
    /// so a CI job cannot write into whoever's config directory it happened to
    /// run as. It is stated here rather than in a game because it is a fact
    /// about the *run*, not about the number — `--headless` is the engine's
    /// flag, and the four samples that had it spelled the same three lines
    /// four times.
    ///
    /// What stays the game's is `app` and `file`: which directory the value
    /// belongs to and what it is called are the two things the engine could not
    /// have guessed.
    #[must_use]
    pub fn for_app(app: &str, file: &str, headless: bool) -> Self {
        let backing = if headless {
            Backing::None
        } else {
            Backing::platform(app)
        };
        Self::open(backing, file)
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
        if let Err(error) = self.backing.write(&self.file, &self.value.to_le_bytes()) {
            crcbl_core::log::warn!("record: save failed ({error})");
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

    /// **A headless run picks `Backing::None`**, which is the whole of what
    /// `for_app` decides — and the assertion is that nothing reached a file,
    /// not merely that the number came back, because an in-memory value is
    /// what a run writing into the real config directory would also answer.
    ///
    /// The windowed half is deliberately not asserted here: it would name
    /// whoever's config directory the suite is running as and write into it,
    /// which is the thing this rule exists to prevent.
    #[test]
    fn a_headless_run_keeps_its_value_in_memory_and_writes_nothing() {
        let mut record = Record::for_app("crcbl-record-test", "best.bin", true);
        assert_eq!(record.get(), 0);
        assert!(record.raise(500), "it still tracks the value in memory");
        assert_eq!(record.get(), 500);

        // Nothing was written, so a second headless open starts over.
        assert_eq!(
            Record::for_app("crcbl-record-test", "best.bin", true).get(),
            0,
            "a headless run left a file behind"
        );
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
