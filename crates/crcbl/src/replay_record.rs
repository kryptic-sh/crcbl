//! Recording a live host's session to a `.crpl` file a fresh host
//! re-simulates.
//!
//! Here, in the umbrella, because it is the one crate that names both halves:
//! `crcbl-server`'s [`Host`], which keeps what its module was handed, and
//! `crcbl-store`'s [`ReplayStream`], which writes the file. Neither depends
//! on the other, so the conversion between the host's records and the file's
//! — [`recorded_peer_tick`] and [`tick_inputs`] — lives here once, for the
//! recorder and for whatever re-simulates its files ([`resimulate`]).
//!
//! # What a recording holds
//!
//! A [`Recorder`] pulls from its host after every
//! [`Host::update`]: the `Flags::SIM` sets applied since the last pull
//! ([`Host::take_sim_record`]), what the module was handed of its peers
//! ([`Host::take_peer_input_record`]), and [`hash_world`] at the tick the
//! host stands at, if it moved — and the state hash of the tick it started
//! on, so a host built unlike the recorded one is told so before it runs a
//! tick. An update that ran several ticks to catch up is hashed at its last;
//! the file allows a recorder to hash only some ticks, and a re-simulation
//! compares the ones it has.
//!
//! It records no output entries — the messages a viewer plays back — so its
//! file is one a re-simulation checks and nothing plays.
//!
//! # Drained, not accumulated
//!
//! Both of the host's records are **taken**, not read, so a host that is
//! recorded holds only what happened since the last update rather than every
//! frame of the session. The recorder holds the sets and the hashes — the
//! sets are rare, a hash is a tick and a value — and streams the peer track,
//! which grows by every peer's frames every tick, to a spool file beside the
//! recording ([`SPOOL_SUFFIX`]), from which [`Recorder::finish`] copies it
//! into the file and then removes it.
//!
//! # Where a recording starts
//!
//! A re-simulation runs a host built like the recorded one — the same world,
//! module and registry — from the tick the recording started, starting from
//! no peers. So a recording is started on a host that has run no tick, or on
//! a host whose state at that tick the re-simulating side can rebuild: the
//! peers already in session open the record as joining
//! ([`Host::record_peer_inputs`]), and the sets applied before it are part of
//! that state, so they are not in the file. A game that changes its world
//! from [`Host::events`] outside its module diverges on re-simulation, since
//! nothing replays those handlers.
//!
//! # Files
//!
//! A recording never overwrites: [`Recorder::start`] refuses a path that
//! exists, by name, and creates the file and its spool so that a file made
//! between the check and the create is refused too. **Until it finishes the
//! file is empty**, and a recorder that is dropped unfinished, or a process
//! killed mid-session, leaves it empty with the spool beside it — the reader
//! refuses an empty file as too short. A [`crate::lan::LanHost`] finishes
//! its recorder when it is dropped, which is a window closing or a panic
//! unwinding through its owner.
//!
//! # Native only
//!
//! It writes through `std::fs`, which a browser does not have.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter};
use std::iter::Peekable;
use std::path::{Path, PathBuf};

use crate::args::Consumed;
use crate::core::TickId;
use crate::net::ConsoleSet;
use crate::server::sim_hash::hash_world;
use crate::server::{Host, PeerFrames, PeerId, ResimError, RosterChange, TickInputs};
use crate::store::StorageError;
use crate::store::replay::{
    FileTransport, RecordedPeerFrames, RecordedPeerTick, RecordedRosterChange, ReplayStream,
    RosterChangeKind,
};

/// What the spool beside a recording is named: the recording's path with this
/// appended.
pub const SPOOL_SUFFIX: &str = ".spool";

/// Why a recording did not start, or did not finish whole.
#[derive(Debug)]
#[non_exhaustive]
pub enum RecordError {
    /// Something already exists at the path: a recording never overwrites.
    Exists(PathBuf),
    /// A file could not be created, written or removed.
    Io {
        /// The file.
        path: PathBuf,
        /// What the system said.
        error: io::Error,
    },
    /// The host is already recording, to this file.
    Recording(PathBuf),
    /// The store refused what was recorded — a rule of the input section, or
    /// the spool's write — while recording to `path`.
    Store {
        /// The recording.
        path: PathBuf,
        /// What the store said.
        error: StorageError,
    },
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exists(path) => write!(
                f,
                "refusing to record to {}: it exists, and a recording never overwrites",
                path.display()
            ),
            Self::Recording(path) => {
                write!(f, "the host is already recording, to {}", path.display())
            }
            Self::Io { path, error } => write!(f, "recording to {}: {error}", path.display()),
            Self::Store { path, error } => write!(f, "recording to {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for RecordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Exists(_) | Self::Recording(_) => None,
            Self::Io { error, .. } => Some(error),
            Self::Store { error, .. } => Some(error),
        }
    }
}

/// What a finished recording holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordSummary {
    /// The file.
    pub path: PathBuf,
    /// The first tick hashed — the one the recording started on — and the
    /// last.
    pub ticks: (TickId, TickId),
    /// How many ticks were hashed.
    pub state_hashes: usize,
    /// How many `Flags::SIM` sets were recorded.
    pub sim_sets: usize,
    /// How many ticks of the peer track were recorded: those with a roster
    /// change or a peer handed anything.
    pub peer_ticks: usize,
}

impl std::fmt::Display for RecordSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "recorded ticks {} to {} to {}: {} state hashes, {} sets, {} ticks of peer input",
            self.ticks.0.get(),
            self.ticks.1.get(),
            self.path.display(),
            self.state_hashes,
            self.sim_sets,
            self.peer_ticks,
        )
    }
}

/// A host's session, being recorded to a file: see the [module docs](self).
#[derive(Debug)]
pub struct Recorder {
    path: PathBuf,
    spool_path: PathBuf,
    /// The recording, created empty and written when it finishes.
    file: File,
    stream: ReplayStream<File>,
    /// The last tick hashed, once one is.
    hashed: Option<TickId>,
}

impl Recorder {
    /// Starts recording `host`, which ticks at `tick_hz`, to a new file at
    /// `path`: creates it and its spool, starts the host's input record, and
    /// hashes the tick it stands at. Sets the host applied before now are
    /// taken and left out, being part of the state the recording starts
    /// from; so is anything an input record the host already kept holds.
    ///
    /// # Errors
    ///
    /// [`RecordError::Exists`] when something exists at `path` or at its
    /// spool, and [`RecordError::Io`] when either could not be created —
    /// leaving nothing behind.
    pub fn start(path: &Path, host: &mut Host, tick_hz: u32) -> Result<Self, RecordError> {
        let spool_path = spool_path(path);
        let file = create_new(path, false)?;
        let spool = match create_new(&spool_path, true) {
            Ok(spool) => spool,
            Err(error) => {
                drop(file);
                remove_created(path);
                return Err(error);
            }
        };
        host.record_peer_inputs();
        host.take_peer_input_record();
        host.take_sim_record();
        let mut recorder = Self {
            path: path.to_path_buf(),
            spool_path,
            file,
            stream: ReplayStream::new(tick_hz, spool),
            hashed: None,
        };
        // Nothing is recorded yet, so this hashes the tick the host stands at
        // and takes nothing else.
        if let Err(error) = recorder.record(host) {
            let Self {
                path,
                spool_path,
                file,
                stream,
                ..
            } = recorder;
            drop((file, stream));
            remove_created(&path);
            remove_created(&spool_path);
            return Err(error);
        }
        Ok(recorder)
    }

    /// The file being recorded to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Takes what `host` did since the last call — its applied sets and the
    /// ticks of its input record — into the recording, and hashes the tick it
    /// stands at if it moved. Call it after every [`Host::update`].
    ///
    /// # Errors
    ///
    /// [`RecordError::Store`] when the store refused an entry or the spool
    /// refused a write. What `host` handed over after the refused entry is
    /// not in the recording; what came before it still is.
    pub fn record(&mut self, host: &mut Host) -> Result<(), RecordError> {
        for applied in host.take_sim_record() {
            let set = ConsoleSet {
                name: applied.set.name().to_owned(),
                value: applied.set.value_text(),
            };
            self.stream
                .push_sim_set(applied.tick, set)
                .map_err(|error| store(&self.path, error))?;
        }
        for entry in host.take_peer_input_record() {
            self.stream
                .push_peer_tick(&recorded_peer_tick(&entry))
                .map_err(|error| store(&self.path, error))?;
        }
        let tick = host.tick_id();
        if self.hashed.is_none_or(|hashed| tick > hashed) {
            self.stream
                .push_state_hash(tick, hash_world(host.world(), tick))
                .map_err(|error| store(&self.path, error))?;
            self.hashed = Some(tick);
        }
        Ok(())
    }

    /// Takes `host`'s last ticks ([`record`](Self::record)), writes the file
    /// and removes the spool.
    ///
    /// # Errors
    ///
    /// As [`record`](Self::record) for the last ticks — the file is still
    /// written, with what came before them — and [`RecordError::Store`] or
    /// [`RecordError::Io`] when the file could not be written whole, which
    /// leaves it cut short, or the spool could not be removed.
    pub fn finish(mut self, host: &mut Host) -> Result<RecordSummary, RecordError> {
        let last = self.record(host);
        let summary = self.close()?;
        last.map(|()| summary)
    }

    /// Writes the file from what is recorded, and removes the spool.
    fn close(self) -> Result<RecordSummary, RecordError> {
        let summary = RecordSummary {
            path: self.path.clone(),
            ticks: self
                .stream
                .hashed_ticks()
                .unwrap_or((TickId::ZERO, TickId::ZERO)),
            state_hashes: self.stream.state_hash_count(),
            sim_sets: self.stream.sim_set_count(),
            peer_ticks: self.stream.peer_tick_count(),
        };
        let mut out = BufWriter::new(self.file);
        let spool = self
            .stream
            .finish(&mut out)
            .map_err(|error| store(&self.path, error))?;
        let file = out
            .into_inner()
            .map_err(|error| Self::io(&self.path, error.into_error()))?;
        file.sync_all()
            .map_err(|error| Self::io(&self.path, error))?;
        // Closed first, so no handle to the spool outlives its name.
        drop(spool);
        std::fs::remove_file(&self.spool_path)
            .map_err(|error| Self::io(&self.spool_path, error))?;
        Ok(summary)
    }

    fn io(path: &Path, error: io::Error) -> RecordError {
        RecordError::Io {
            path: path.to_path_buf(),
            error,
        }
    }
}

/// The spool beside a recording at `path`.
fn spool_path(path: &Path) -> PathBuf {
    let mut spool = path.as_os_str().to_owned();
    spool.push(SPOOL_SUFFIX);
    PathBuf::from(spool)
}

/// Removes the empty file this module created at `path` while starting a
/// recording that then failed, logging a removal that fails: the start's own
/// error is the one to report.
fn remove_created(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        crate::log::warn!("record: {} was left behind: {error}", path.display());
    }
}

/// Creates a file at `path` that did not exist — refusing one that does, by
/// name — for writing, and for reading too when `read`.
fn create_new(path: &Path, read: bool) -> Result<File, RecordError> {
    OpenOptions::new()
        .read(read)
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| match error.kind() {
            io::ErrorKind::AlreadyExists => RecordError::Exists(path.to_path_buf()),
            _ => RecordError::Io {
                path: path.to_path_buf(),
                error,
            },
        })
}

fn store(path: &Path, error: StorageError) -> RecordError {
    RecordError::Store {
        path: path.to_path_buf(),
        error,
    }
}

/// Refuses `path` if something exists there, by name — what a command line
/// checks while it still has an exit code to refuse with. [`Recorder::start`]
/// refuses it again when it creates the file, which is the check that holds.
///
/// # Errors
///
/// [`RecordError::Exists`] when it does.
pub fn refuse_existing(path: &Path) -> Result<(), RecordError> {
    // `symlink_metadata`, so a dangling link is refused too: creating the
    // file would follow it.
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(RecordError::Exists(path.to_path_buf()));
    }
    Ok(())
}

/// Claims `arg` if it is `--record <FILE>`, taking its value from `rest` into
/// `record`.
///
/// A second `--record` is [`Consumed::Bad`], and so is a missing value or a
/// path something already exists at ([`refuse_existing`]). Which session it
/// records — a host's, never a joiner's — is the sample's to check.
pub fn consume<I: Iterator<Item = String>>(
    record: &mut Option<PathBuf>,
    arg: &str,
    rest: &mut Peekable<I>,
) -> Consumed {
    if arg != "--record" {
        return Consumed::No;
    }
    let Some(value) = rest.next() else {
        return Consumed::Bad("--record needs a file".to_string());
    };
    if record.is_some() {
        return Consumed::Bad("--record was given twice".to_string());
    }
    let path = PathBuf::from(value);
    if let Err(error) = refuse_existing(&path) {
        return Consumed::Bad(format!("--record: {error}"));
    }
    *record = Some(path);
    Consumed::Yes
}

/// A host's input record of one tick, as a file's peer track carries it.
pub fn recorded_peer_tick(entry: &TickInputs) -> RecordedPeerTick {
    RecordedPeerTick {
        tick: entry.tick,
        roster: entry
            .roster
            .iter()
            .map(|change| RecordedRosterChange {
                kind: match change {
                    RosterChange::Joined(_) => RosterChangeKind::Joined,
                    RosterChange::Lost(_) => RosterChangeKind::Lost,
                    RosterChange::Resumed(_) => RosterChangeKind::Resumed,
                    RosterChange::Left(_) => RosterChangeKind::Left,
                    RosterChange::Ended(_) => RosterChangeKind::Ended,
                },
                peer: change.peer().get(),
            })
            .collect(),
        peers: entry
            .peers
            .iter()
            .map(|frames| RecordedPeerFrames {
                peer: frames.peer.get(),
                dropped: frames.dropped,
                frames: frames.frames.clone(),
            })
            .collect(),
    }
}

/// A file's peer track entry, as [`Host::resimulate`] takes it.
pub fn tick_inputs(entry: &RecordedPeerTick) -> TickInputs {
    TickInputs {
        tick: entry.tick,
        roster: entry
            .roster
            .iter()
            .map(|change| {
                let peer = PeerId::from_raw(change.peer);
                match change.kind {
                    RosterChangeKind::Joined => RosterChange::Joined(peer),
                    RosterChangeKind::Lost => RosterChange::Lost(peer),
                    RosterChangeKind::Resumed => RosterChange::Resumed(peer),
                    RosterChangeKind::Left => RosterChange::Left(peer),
                    RosterChangeKind::Ended => RosterChange::Ended(peer),
                }
            })
            .collect(),
        peers: entry
            .peers
            .iter()
            .map(|frames| PeerFrames {
                peer: PeerId::from_raw(frames.peer),
                frames: frames.frames.clone(),
                dropped: frames.dropped,
            })
            .collect(),
    }
}

/// Re-simulates `file` on `host` — built like the recorded one, at the tick
/// the recording started — through [`Host::resimulate`]: its sets, its state
/// hashes and its peer track. Answers the tick it stopped at.
///
/// # Errors
///
/// As [`Host::resimulate`]: an input the host refuses before any tick runs,
/// or the first hash it does not reproduce.
pub fn resimulate(host: &mut Host, file: &FileTransport) -> Result<TickId, ResimError> {
    host.resimulate(
        file.sim_sets()
            .iter()
            .map(|recorded| (recorded.tick, recorded.set.clone())),
        file.state_hashes()
            .iter()
            .map(|recorded| (recorded.tick, recorded.hash)),
        file.peer_ticks().iter().map(tick_inputs),
    )
}

#[cfg(test)]
mod tests;
