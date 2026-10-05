//! A player's profile: what they made their own that is not a setting — their
//! key binds.
//!
//! ```text
//!  native, windowed  ──▶ ~/.config/<app>/profile.toml   write_atomic
//!  native, headless  ──▶ nowhere                        in memory, no trace
//!  wasm32            ──▶ the Origin Private File System
//! ```
//!
//! # Where it is kept, and in what
//!
//! Beside the settings file, under the same [`Backing`] rules a
//! [`Record`](crate::record::Record) follows — the config directory natively,
//! the shim's OPFS store in a browser, nowhere when headless — because those
//! rules are a fact about the platform and the run, not about what is kept.
//!
//! **TOML, the settings file's format**, so a player who opens their config
//! directory meets one grammar rather than two. Topic 14 had named RON for
//! profiles; nothing in this crate parses RON, and the reason it gave — that
//! binds are structured data — is met by a TOML table of arrays.
//!
//! ```toml
//! version = 1
//!
//! [binds]
//! jump = ["KeyJ", "Pad:South"]
//! crouch = []
//! ```
//!
//! # What the binds are
//!
//! **Diffs over the game's defaults**, by action name, each binding in the
//! input layer's stable text form (`crcbl_input`'s `binding_text`). An action
//! missing from the table is on its defaults, so an action a game adds or a
//! default it changes reaches every player who never rebound it; an empty
//! array is an action the player left with nothing. This crate keeps the text
//! and never parses it: what a binding *is*, and which actions a game declares,
//! is the input layer's to say.
//!
//! # A file that cannot be read
//!
//! **Refused by name, and the run goes on with the defaults.**
//! [`ProfileStore::load`] says why — not TOML, no version, a version newer
//! than this build reads — and [`ProfileStore::load_or_default`] logs that
//! and hands back an empty profile. Losing a player's binds for one session is
//! recoverable; a game that would not start over a damaged file is not. The
//! file is left as it was: opening is a read, and only the next save replaces
//! it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::StorageError;
use crate::record::Backing;

/// The file a profile is kept in, beside the settings file.
pub const PROFILE_FILE: &str = "profile.toml";

/// The format version this build writes, and the newest it reads.
pub const PROFILE_VERSION: u32 = 1;

/// The key the version is kept under, read before anything else in the file
/// so a newer layout is refused by its number rather than by whatever its
/// first unfamiliar field fails on.
const VERSION_KEY: &str = "version";

/// One player's profile — see the [module docs](self).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    /// Action name to binding texts, for every action the player rebound.
    binds: BTreeMap<String, Vec<String>>,
}

/// The file's layout: the version, then each part of the profile.
#[derive(Serialize, Deserialize)]
struct ProfileFile {
    version: u32,
    #[serde(default)]
    binds: BTreeMap<String, Vec<String>>,
}

impl Profile {
    /// Every rebound action and its binding texts, by action name.
    pub fn binds(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.binds
            .iter()
            .map(|(action, bindings)| (action.as_str(), bindings.as_slice()))
    }

    /// Replaces the binds of every action `declares` names with `overrides`:
    /// each listed action takes its listed texts, and a declared action not
    /// listed is dropped from the table, which puts it back on its defaults.
    ///
    /// **An entry for an action `declares` does not name is kept.** It is an
    /// action this build does not have — a newer build's, or a mode this one
    /// left out — and the player's choice for it is theirs to get back when
    /// they run a build that has it. Dropping it here would make every save
    /// from an older build erase it.
    pub fn set_binds(
        &mut self,
        declares: impl Fn(&str) -> bool,
        overrides: impl IntoIterator<Item = (String, Vec<String>)>,
    ) {
        self.binds.retain(|action, _| !declares(action));
        self.binds.extend(overrides);
    }

    /// The profile `text` spells, or why it spells none.
    fn from_toml(text: &str) -> Result<Self, Refusal> {
        let table: toml::Table =
            toml::from_str(text).map_err(|error| Refusal::Corrupt(error.message().to_owned()))?;
        let version = match table.get(VERSION_KEY) {
            Some(toml::Value::Integer(version)) => *version,
            Some(other) => {
                return Err(Refusal::Corrupt(format!(
                    "`{VERSION_KEY}` is a {}, not a number",
                    other.type_str()
                )));
            }
            None => return Err(Refusal::Corrupt(format!("no `{VERSION_KEY}`"))),
        };
        if version > i64::from(PROFILE_VERSION) {
            return Err(Refusal::Newer(version));
        }
        if version < 1 {
            return Err(Refusal::Corrupt(format!(
                "`{VERSION_KEY} = {version}` names no format"
            )));
        }
        let file: ProfileFile =
            toml::from_str(text).map_err(|error| Refusal::Corrupt(error.message().to_owned()))?;
        Ok(Self { binds: file.binds })
    }

    /// This profile as its file's text, at [`PROFILE_VERSION`].
    fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&ProfileFile {
            version: PROFILE_VERSION,
            binds: self.binds.clone(),
        })
    }
}

/// Why a file's text is not a profile, before the file's name is attached.
enum Refusal {
    Corrupt(String),
    Newer(i64),
}

/// Why a profile could not be read or written. Each names the file.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// The file is there and is not a profile: not UTF-8, not TOML, or not the
    /// layout its version names.
    #[error("{file} is not a profile: {reason}")]
    Corrupt {
        /// The file, as [`ProfileStore::file`] names it.
        file: String,
        /// What was wrong with it.
        reason: String,
    },
    /// The file is a profile from a newer build, whose layout this one cannot
    /// know.
    #[error("{file} is profile version {found}, and this build reads up to {PROFILE_VERSION}")]
    Newer {
        /// The file, as [`ProfileStore::file`] names it.
        file: String,
        /// The version it carries.
        found: i64,
    },
    /// The profile could not be put into its file's text.
    #[error("{file} could not be written as TOML: {reason}")]
    Encode {
        /// The file, as [`ProfileStore::file`] names it.
        file: String,
        /// What the serialiser said.
        reason: String,
    },
    /// The storage refused the read or the write.
    #[error("{file}: {error}")]
    Storage {
        /// The file, as [`ProfileStore::file`] names it.
        file: String,
        /// What the storage said.
        error: StorageError,
    },
}

/// Where a [`Profile`] is read from and written to.
///
/// Holds no profile itself: a game keeps the [`Profile`] it loaded and hands
/// it back to [`ProfileStore::save`], so what is in memory and what is on disk
/// are two values the game can compare rather than one that hides the
/// difference.
#[derive(Debug)]
pub struct ProfileStore {
    backing: Backing,
    file: String,
}

impl ProfileStore {
    /// The profile kept in `file` under `backing`.
    #[must_use]
    pub fn open(backing: Backing, file: &str) -> Self {
        Self {
            backing,
            file: file.to_owned(),
        }
    }

    /// [`PROFILE_FILE`] for `app`, or nowhere when `headless`.
    ///
    /// [`Record::for_app`](crate::record::Record::for_app)'s rule, for its
    /// reason: a run with no window must leave nothing behind, so a CI job
    /// cannot write into whoever's config directory it ran as.
    #[must_use]
    pub fn for_app(app: &str, headless: bool) -> Self {
        let backing = if headless {
            Backing::None
        } else {
            Backing::platform(app)
        };
        Self::open(backing, PROFILE_FILE)
    }

    /// The file this store reads and writes, as its errors name it.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The stored profile, or an empty one when there is none yet — no file,
    /// or a store with nowhere to keep one.
    ///
    /// # Errors
    ///
    /// [`ProfileError::Corrupt`] or [`ProfileError::Newer`] for a file that is
    /// there and is not a profile this build reads, and
    /// [`ProfileError::Storage`] for a read the storage refused.
    pub fn load(&self) -> Result<Profile, ProfileError> {
        let bytes = match self.backing.read(&self.file) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(Profile::default()),
            Err(error) => {
                return Err(ProfileError::Storage {
                    file: self.file.clone(),
                    error,
                });
            }
        };
        let text = std::str::from_utf8(&bytes).map_err(|error| ProfileError::Corrupt {
            file: self.file.clone(),
            reason: format!("not UTF-8: invalid byte at offset {}", error.valid_up_to()),
        })?;
        Profile::from_toml(text).map_err(|refusal| match refusal {
            Refusal::Corrupt(reason) => ProfileError::Corrupt {
                file: self.file.clone(),
                reason,
            },
            Refusal::Newer(found) => ProfileError::Newer {
                file: self.file.clone(),
                found,
            },
        })
    }

    /// [`ProfileStore::load`], with a refusal logged as a warning and the
    /// defaults in its place — see the [module docs](self) for why a damaged
    /// file costs the session its binds and nothing else.
    #[must_use]
    pub fn load_or_default(&self) -> Profile {
        self.load().unwrap_or_else(|error| {
            crcbl_core::log::warn!("profile: {error}; using the defaults");
            Profile::default()
        })
    }

    /// Writes `profile` out, and answers whether there was anywhere to write
    /// it — `false` from a headless run, which writes nothing.
    ///
    /// # Errors
    ///
    /// [`ProfileError::Storage`] for a write the storage refused, and
    /// [`ProfileError::Encode`] for a profile the serialiser would not spell.
    pub fn save(&self, profile: &Profile) -> Result<bool, ProfileError> {
        let text = profile.to_toml().map_err(|error| ProfileError::Encode {
            file: self.file.clone(),
            reason: error.to_string(),
        })?;
        self.backing
            .write(&self.file, text.as_bytes())
            .map_err(|error| ProfileError::Storage {
                file: self.file.clone(),
                error,
            })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> ProfileStore {
        ProfileStore::open(Backing::Native(dir.path().to_path_buf()), PROFILE_FILE)
    }

    fn written(dir: &tempfile::TempDir, text: &str) {
        std::fs::write(dir.path().join(PROFILE_FILE), text).expect("the temp dir is writable");
    }

    fn binds(entries: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        entries
            .iter()
            .map(|(action, texts)| {
                (
                    (*action).to_owned(),
                    texts.iter().map(|text| (*text).to_owned()).collect(),
                )
            })
            .collect()
    }

    fn profile(entries: &[(&str, &[&str])]) -> Profile {
        let mut profile = Profile::default();
        profile.set_binds(|_| true, binds(entries));
        profile
    }

    /// **A saved profile reads back as itself**, an unbound action included —
    /// an empty array is a choice, not an absence.
    #[test]
    fn a_saved_profile_reads_back_as_itself() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let saved = profile(&[("jump", &["KeyJ", "Pad:South"]), ("crouch", &[])]);
        assert!(store(&dir).save(&saved).expect("a writable dir"));

        let read = store(&dir).load().expect("the file just written");
        assert_eq!(read, saved);
        let text = std::fs::read_to_string(dir.path().join(PROFILE_FILE)).expect("the file");
        assert!(text.contains("version = 1"), "no version in {text:?}");
    }

    /// No file is a new player, not a fault.
    #[test]
    fn no_file_is_an_empty_profile() {
        let dir = tempfile::tempdir().expect("a temp dir");
        assert_eq!(store(&dir).load().expect("absence"), Profile::default());
    }

    /// **A file that is not a profile is refused by name, and the defaults
    /// stand in for it** — and the file is left for the player to look at.
    #[test]
    fn a_corrupt_file_is_refused_by_name_and_the_defaults_stand_in() {
        let dir = tempfile::tempdir().expect("a temp dir");
        for text in [
            "version = 1\n[binds\njump = [",
            "[binds]\njump = [\"KeyJ\"]\n",
            "version = \"one\"\n",
            "version = 0\n",
            "version = 1\n[binds]\njump = \"KeyJ\"\n",
        ] {
            written(&dir, text);
            let error = store(&dir).load().expect_err(text);
            assert!(
                matches!(error, ProfileError::Corrupt { .. }),
                "{text:?} gave {error:?}"
            );
            assert!(
                error.to_string().starts_with(PROFILE_FILE),
                "the refusal does not name the file: {error}"
            );
            assert_eq!(
                store(&dir).load_or_default(),
                Profile::default(),
                "{text:?}"
            );
        }
        std::fs::write(dir.path().join(PROFILE_FILE), [0xff, 0xfe]).expect("writable");
        assert!(matches!(
            store(&dir).load(),
            Err(ProfileError::Corrupt { .. })
        ));
        assert!(
            dir.path().join(PROFILE_FILE).exists(),
            "a refused file was removed"
        );
    }

    /// A newer build's profile is refused by its version, before its layout is
    /// read at all.
    #[test]
    fn a_newer_profile_is_refused_by_its_version() {
        let dir = tempfile::tempdir().expect("a temp dir");
        written(&dir, "version = 2\n[binds]\njump = 7\n");
        let error = store(&dir).load().expect_err("version 2");
        assert!(
            matches!(error, ProfileError::Newer { found: 2, .. }),
            "{error:?}"
        );
        assert!(error.to_string().starts_with(PROFILE_FILE), "{error}");
    }

    /// **The binds of an action this build does not declare survive a save**;
    /// a declared action is replaced, and one back on its defaults is dropped.
    #[test]
    fn a_save_keeps_the_binds_of_actions_this_build_does_not_declare() {
        let mut loaded = profile(&[
            ("jump", &["KeyJ"]),
            ("crouch", &["KeyX"]),
            ("glide", &["KeyG"]),
        ]);
        let declared = |action: &str| action == "jump" || action == "crouch";
        loaded.set_binds(declared, binds(&[("jump", &["KeyK"])]));
        assert_eq!(
            loaded,
            profile(&[("jump", &["KeyK"]), ("glide", &["KeyG"])])
        );
    }

    /// **A headless store writes nothing and reads nothing**, which is the
    /// property that lets CI run a game without writing into whoever's config
    /// directory it runs as.
    #[test]
    fn a_headless_store_writes_nothing() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let headless = ProfileStore::open(Backing::None, PROFILE_FILE);
        let saved = profile(&[("jump", &["KeyJ"])]);
        assert!(
            !headless.save(&saved).expect("nowhere is not a failure"),
            "a headless store said it wrote"
        );
        assert_eq!(headless.load().expect("nothing"), Profile::default());
        assert!(
            std::fs::read_dir(dir.path())
                .expect("the temp dir")
                .next()
                .is_none(),
            "a headless store wrote a file"
        );
        let for_run = ProfileStore::for_app("crcbl-profile-test", true);
        assert!(!for_run.save(&saved).expect("headless"));
        assert_eq!(
            ProfileStore::for_app("crcbl-profile-test", true)
                .load()
                .expect("headless"),
            Profile::default(),
            "a headless run left a profile behind"
        );
    }
}
