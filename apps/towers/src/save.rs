//! Save and resume between waves: what a run keeps, where, and how it is read
//! back.
//!
//! | Target | Where |
//! | --- | --- |
//! | native, windowed or `--host` | the data directory's `towers/`, [`PLAYER_FILE`] |
//! | native, `--serve` | the same directory, [`SERVER_FILE`] |
//! | native, `--headless` | nowhere, so a CI run leaves no trace |
//! | `wasm32` | the Origin Private File System, [`PLAYER_FILE`] |
//!
//! ```text
//!   Stage ──▶ Stage::checkpoint ──▶ Checkpoint ──▶ encode ──▶ SectorSave
//!                                                                │
//!                                  SaveWriter ◀──────────────────┘
//!                                      │
//!                                      ▼
//!                                Vault::store ──▶ StorageSource::write
//!
//!   StorageSource::read ──▶ SaveReader ──▶ decode ──▶ Checkpoint ──▶ Stage::restore
//! ```
//!
//! # Between waves, and only then
//!
//! A save is taken in the **build phase**: no wave releasing, in a run that
//! is still being played. `crate::game`'s `Stage::checkpoint` refuses
//! anything else with a [`NotSaved`] the player is shown — a save mid-wave
//! is the one the slice rules out. What the last wave sent may still be on
//! the field, because the table measures the build phase from a wave's last
//! release rather than from its last creep — on the committed field the
//! next wave is usually on its way before the last one is gone — so a save
//! carries the creeps, the bolts in the air and the bursts still drawn, and
//! a resumed stage puts each creep's sphere back in a fresh physics world.
//!
//! **The same run, tick for tick.** A resumed stage hashes as the stage that
//! saved, and played on with the same commands it hashes alike on every tick
//! after — `crate::game::checkpoint`'s tests hold both through the next
//! wave. That is why every clock, position and reload is written as the bit
//! pattern of the float it is, not rounded: a reload a tick out fires a tick
//! out, and a hash compares bits. It is also why the stage reads its physics
//! queries in the field's order rather than the order the world answers in
//! (`Stage::splash`, [`crate::tower::acquire`]): the resumed world is a
//! fresh one, whose answer order is not the original's.
//!
//! **A finished run is not saved.** It plays itself again after
//! [`crate::game::RESTART_S`], so a save of it would resume into a result
//! screen and then a fresh field; refusing it keeps the last between-waves
//! save as what *Continue* offers.
//!
//! # The container is `crcbl-store`'s, unchanged
//!
//! [`SaveWriter`] and [`SaveReader`] own the magic, the container version,
//! the SHA-256 over everything before it and the atomic write, and
//! [`SaveBacking`] owns where saves live — the shape `apps/shard` writes, one
//! [`SectorSave`] at [`SectorId::ZERO`]. The container's header carries the
//! stage's tick count and its simulated clock, which are
//! [`SaveHeader::tick`] and [`SaveHeader::playtime_secs`] exactly, so the
//! payload does not carry them a second time.
//!
//! # The payload's bytes
//!
//! Little-endian throughout, each float as its `f64` bits, each count a
//! `u32` written before what it counts — the `payload` module writes and reads
//! them:
//!
//! | Offset | Size | Field |
//! | --- | --- | --- |
//! | 0 | 4 | magic, `TWRS` |
//! | 4 | 2 | the payload's version, `u16` |
//! | 6 | 32 | the map's [`Map::fingerprint`] |
//! | 38 | 8 | how many runs the stage has played, `u64` |
//! | 46 | 4 | gold, `u32` |
//! | 50 | 4 | lives, `u32` |
//! | 54 | 8 | kills, `u64` |
//! | 62 | 8 | leaks, `u64` |
//! | 70 | 8 | shots fired, `u64` — what every bolt's id is numbered from |
//! | 78 | 8 | towers built, `u64` |
//! | 86 | 1 | how many kinds follow, `u8` |
//! | 87 | 8 each | towers built of each kind, `u64`, in [`crate::tower::ALL`]'s order |
//! | then | 8 | towers stepped up, `u64` |
//! | then | 8 | commands refused, `u64` |
//! | then | 4 | waves started, `u32` |
//! | then | 8 | when the next wave is due |
//! | then | 4 + 19 each | the towers: plot, kind and tier (`u8` each), when it may fire again, when it last did |
//! | then | 4 + 21 each | the creeps: kind (`u8`), how far along the path, health (`u32`), the hold it walks at |
//! | then | 4 + 72 each | the bolts: id (`u64`), where, heading, target (`u32`, the creep's place in the list, or `u32::MAX` for one gone), damage (`u32`), burst radius |
//! | then | 4 + 48 each | the bursts: id (`u64`), where, radius, when raised |
//!
//! Every list is in the stage's own order, which is the order the state hash
//! reads it in.
//!
//! **What is not written, and why.** The outcome is always
//! [`Outcome::Playing`](crate::wave::Outcome::Playing) and the end time
//! unset, because a finished run is not saved. How much of the last wave went
//! out is always all of it, because nothing is releasing. A creep's centre
//! and heading are where its distance along the path puts it, read off the
//! path again on resume. A second copy of any of them would be a field that
//! could disagree with the rule.
//!
//! # Everything read back is a stranger's
//!
//! A file on disk is untrusted, so [`decode`] refuses — by name, as a
//! [`SaveError`] — anything a run between waves could not have written: a
//! foreign magic, another payload version, **another map**, a clock that is
//! not a finite number, a wave past the table, lives that do not add up with
//! the leaks, kills, leaks and creeps on the field that do not add up to the
//! creeps the started waves released, more gold than the table can ever have
//! paid, a tower on a plot the map lacks or on a plot twice, a kind or tier
//! byte no tower or creep has, a reload, a shot or a burst the clock could not
//! have reached, counters that disagree with the towers they count, a creep
//! off the path, dead or held by no tower's hold, a bolt or burst no tower
//! fires, a point off the field, or a byte too many or too few. Every count
//! is held to its bound and to the bytes left before anything is reserved for
//! it. There is no migration seam — `docs/backlog.md` owes `crcbl-store` one
//! — so a version bump orphans older saves, which are refused by name.

use std::path::Path;

use crcbl::core::TickId;
use crcbl::net::types::SectorId;
use crcbl::store::save::{SaveBacking, SaveHeader, SaveReader, SaveWriter, SectorSave};
use crcbl::store::{StorageError, StorageSource};

use crcbl::math::DVec3;

use crate::map::Map;
use crate::tower::{self, Kind, Tier};

mod payload;

pub use crate::game::NotSaved;
pub use payload::decode;
use payload::encode;

// ---------------------------------------------------------------------------
// Where
// ---------------------------------------------------------------------------

/// The application directory: `~/.local/share/towers/` on Linux. A browser
/// ignores it — the origin is already the namespace.
const APP: &str = "towers";

/// The run a player saves — solo, or the host of a LAN session — and the one
/// the lobby's *Continue* and `--resume` read.
///
/// One slot, which the autosave at a wave's end, the save key and a close
/// all write: a second slot would make *Continue* choose between two runs, and
/// a save between waves is small enough to take at every wave. One component
/// and no directory, because the browser shim restores OPFS entries by name
/// off the root (`restoreOpfs` in `web/engine/storage.js`), and named for this
/// sample because every demo on the site shares that root.
pub const PLAYER_FILE: &str = "towers-run.crb";

/// The run a dedicated server saves: its own file, so a server and a player
/// on one machine never write over each other's run.
pub const SERVER_FILE: &str = "towers-server.crb";

/// Where this run's saves go, if anywhere: a [`SaveBacking`] and the file in
/// it.
#[derive(Debug)]
pub struct Vault {
    backing: SaveBacking,
    file: &'static str,
}

impl Vault {
    /// A player's saves: [`PLAYER_FILE`] where this platform keeps saves, or
    /// nowhere for a headless run, which must leave no trace.
    #[must_use]
    pub fn player(headless: bool) -> Self {
        Self {
            backing: if headless {
                SaveBacking::None
            } else {
                SaveBacking::platform(APP)
            },
            file: PLAYER_FILE,
        }
    }

    /// A dedicated server's saves: [`SERVER_FILE`] in the platform's data
    /// directory. A server has no window to be headless about, and its saves
    /// are what it keeps for the players who come back.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn server() -> Self {
        Self {
            backing: SaveBacking::platform(APP),
            file: SERVER_FILE,
        }
    }

    /// Saves kept nowhere.
    #[must_use]
    pub const fn nowhere() -> Self {
        Self {
            backing: SaveBacking::None,
            file: PLAYER_FILE,
        }
    }

    /// `file` in the directory `root` — for the tests, which keep their saves
    /// in a scratch directory and never in a real data directory.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn at(root: std::path::PathBuf, file: &'static str) -> Self {
        Self {
            backing: SaveBacking::Native(crcbl::store::NativeStorage::at(root)),
            file,
        }
    }

    /// Where the saves go, in the words a debug panel uses.
    #[must_use]
    pub const fn where_it_goes(&self) -> &'static str {
        self.backing.label()
    }

    /// The run saved here, read against `map` — `None` when there is none,
    /// which is the ordinary first run, and always for saves kept nowhere.
    ///
    /// # Errors
    ///
    /// A [`SaveError`] naming why a save that is there will not be resumed:
    /// a file the container refuses (a checksum that does not match, a
    /// truncated file), or a payload [`decode`] refuses.
    pub fn load(&self, map: &Map) -> Result<Option<Checkpoint>, SaveError> {
        let Some(source) = self.backing.source() else {
            return Ok(None);
        };
        match SaveReader::open(source, Path::new(self.file)) {
            Ok(reader) => decode(reader.data(), map).map(Some),
            Err(StorageError::NotFound(_)) => Ok(None),
            Err(error) => Err(SaveError::Unreadable(error)),
        }
    }

    /// Writes `checkpoint` here, over whatever run was saved before.
    ///
    /// **In a browser a written save is not yet on the disk**: the write is
    /// queued and the page's shim performs it on a later frame —
    /// `crcbl-store`'s `web` module carries which half of the atomic-write
    /// guarantee survives a browser.
    ///
    /// # Errors
    ///
    /// [`SaveError::Nowhere`] for saves kept nowhere, and
    /// [`SaveError::Unwritten`] for a write the backend refused.
    pub fn store(&self, checkpoint: &Checkpoint) -> Result<(), SaveError> {
        let Some(source) = self.backing.source() else {
            return Err(SaveError::Nowhere);
        };
        write(source, Path::new(self.file), checkpoint)
    }
}

/// Writes `checkpoint` to `path` in `source`, in the container.
fn write(
    source: &dyn StorageSource,
    path: &Path,
    checkpoint: &Checkpoint,
) -> Result<(), SaveError> {
    let mut writer = SaveWriter::new(SaveHeader {
        tick: TickId::from_raw(checkpoint.ticks),
        playtime_secs: checkpoint.elapsed,
    });
    writer.add_sector(SectorSave {
        sector_id: SectorId::ZERO,
        snapshot_data: encode(checkpoint),
    });
    writer.write(source, path).map_err(SaveError::Unwritten)
}

// ---------------------------------------------------------------------------
// What
// ---------------------------------------------------------------------------

/// The stage between two waves: everything a resumed run needs to be the run
/// that saved. Taken by `Stage::checkpoint` ([`crate::Game::checkpoint`]),
/// read back by [`decode`], and put back by [`crate::Game::restore`].
///
/// Only those three make one, so a `Checkpoint` is always either the stage's
/// own or one [`decode`] has held to every rule a run between waves keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct Checkpoint {
    /// The map it was played on — [`Map::fingerprint`].
    pub(crate) map: [u8; 32],
    pub(crate) runs: u64,
    /// The stage's tick count and simulated clock, in seconds.
    pub(crate) ticks: u64,
    pub(crate) elapsed: f64,
    pub(crate) gold: u32,
    pub(crate) lives: u32,
    pub(crate) kills: u64,
    pub(crate) leaks: u64,
    pub(crate) shots: u64,
    pub(crate) built: u64,
    pub(crate) built_by_kind: [u64; tower::KINDS],
    pub(crate) upgrades: u64,
    pub(crate) refused: u64,
    /// How many waves have been started, and when the next is due.
    pub(crate) wave: usize,
    pub(crate) due_at: f64,
    /// Every tower, creep, bolt and burst, each list in the stage's order.
    pub(crate) towers: Vec<SavedTower>,
    pub(crate) creeps: Vec<SavedCreep>,
    pub(crate) bolts: Vec<SavedBolt>,
    pub(crate) bursts: Vec<SavedBurst>,
}

impl Checkpoint {
    /// How many waves had been started, out of [`crate::wave::WAVES`].
    #[must_use]
    pub const fn wave(&self) -> usize {
        self.wave
    }

    /// What the team had left.
    #[must_use]
    pub const fn lives(&self) -> u32 {
        self.lives
    }

    /// What the team had in the purse.
    #[must_use]
    pub const fn gold(&self) -> u32 {
        self.gold
    }
}

/// One tower as a save keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SavedTower {
    pub(crate) plot: usize,
    pub(crate) kind: Kind,
    pub(crate) tier: Tier,
    pub(crate) ready_at: f64,
    pub(crate) fired_at: f64,
}

/// One creep as a save keeps it: its distance along the path is the whole of
/// where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SavedCreep {
    pub(crate) kind: crate::creep::Kind,
    pub(crate) along: f64,
    pub(crate) health: u32,
    /// The fraction of its speed the last tick's holds left it at.
    pub(crate) slow: f64,
}

/// One bolt in the air as a save keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SavedBolt {
    pub(crate) id: u64,
    pub(crate) at: DVec3,
    pub(crate) heading: DVec3,
    /// The creep it was fired at, by its place in the stage's list, or
    /// `None` for one that has left the field.
    pub(crate) target: Option<usize>,
    pub(crate) damage: u32,
    pub(crate) burst_m: f64,
}

/// One burst still drawn, as a save keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SavedBurst {
    pub(crate) id: u64,
    pub(crate) at: DVec3,
    pub(crate) radius_m: f64,
    pub(crate) raised_at: f64,
}

/// Why a save was not read back or not written.
#[derive(Debug)]
pub enum SaveError {
    /// This run keeps its saves nowhere — a headless run, or a platform that
    /// gave no place.
    Nowhere,
    /// The backend refused the write.
    Unwritten(StorageError),
    /// The container refused the file: its checksum does not match, it is cut
    /// short, it is not a save, or it is another container version.
    Unreadable(StorageError),
    /// A towers save is one sector at [`SectorId::ZERO`], and this is not.
    NotOneSector,
    /// The payload stops partway through a field.
    Truncated,
    /// The payload does not start with towers' magic.
    NotTowers,
    /// The payload is another version of the format.
    Version {
        /// The version the file says it is.
        found: u16,
    },
    /// The save was played on another map.
    OtherMap,
    /// Bytes follow the last tower.
    TrailingBytes {
        /// How many.
        count: usize,
    },
    /// A value no run between waves could have written, named.
    Invalid(String),
    /// A joiner has no stage to resume into: only the host's run resumes.
    NoStage,
    /// There is no save to resume.
    NoSave,
    /// The session is being recorded to this file, and a recording is
    /// re-simulated from a fresh run: a run loaded under it is one the
    /// recording could not reproduce.
    Recording(std::path::PathBuf),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nowhere => write!(f, "this run keeps its saves nowhere"),
            Self::Unwritten(error) => write!(f, "the save was not written: {error}"),
            Self::Unreadable(error) => write!(f, "the save cannot be read: {error}"),
            Self::NotOneSector => write!(f, "the save is not one sector, as a towers save is"),
            Self::Truncated => write!(f, "the save stops partway through a field"),
            Self::NotTowers => write!(f, "the save is not a towers save"),
            Self::Version { found } => write!(
                f,
                "the save is format version {found}, and this build reads version {}",
                payload::PAYLOAD_VERSION
            ),
            Self::OtherMap => write!(f, "the save was played on another map"),
            Self::TrailingBytes { count } => {
                write!(f, "{count} byte(s) follow the save's last tower")
            }
            Self::Invalid(what) => write!(f, "the save's {what}"),
            Self::NoStage => write!(f, "a joiner has no stage to resume into"),
            Self::NoSave => write!(f, "there is no save to resume"),
            Self::Recording(path) => write!(
                f,
                "the session is being recorded to {}, and a recording re-simulates from a \
                 fresh run",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unwritten(error) | Self::Unreadable(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
