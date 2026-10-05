//! A process killed while it writes a save leaves a whole save behind.
//!
//! [`write_atomic`](crate::write_atomic) promises that a crash during the write
//! leaves the previous file intact, and one after the rename a complete new
//! one. Each test here re-runs itself in a child copy of this test binary,
//! which writes a large save over an existing one through [`SaveWriter`] and
//! [`NativeStorage`] — the path a game takes — and stops at a [`KillPoint`]
//! inside `write_atomic`. The parent waits for the child to say it has
//! stopped there, kills it, and reads what is on disk.
//!
//! **The seam is test-only.** `write_atomic` calls [`reached`] under
//! `#[cfg(test)]`, so it exists only in this crate's own unit-test binary:
//! no build a game links has it, and in the test binary it does nothing
//! unless the [`KILL_AT`] variable this module sets on its children names
//! the point. That is why these are unit tests rather than integration tests
//! under `tests/`, whose copy of the crate is built without `cfg(test)` and
//! so has no point to stop at.
//!
//! **A kill is not a power cut.** The killed process's writes are already
//! in the operating system's cache, which outlives it, so what these tests
//! prove is the ordering — no torn file at the target, ever — and not the
//! `sync_all` calls, whose whole job is surviving the cache being lost.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crcbl_core::TickId;
use crcbl_net::types::SectorId;

use crate::save::{SaveHeader, SaveReader, SaveWriter, SectorSave};
use crate::{MemoryStorage, NativeStorage, StorageSource};

/// Set on a child to the [`KillPoint::name`] it stops at.
const KILL_AT: &str = "CRCBL_STORE_KILL_AT";

/// Set on a child to the directory holding the save it overwrites.
const CHILD_DIR: &str = "CRCBL_STORE_KILL_DIR";

/// How long a child gets to reach its point, and how long a stopped child
/// waits to be killed before it gives up and fails. Far past the fraction of
/// a second either takes, so only a hang reaches it — and a child whose
/// parent died still exits on its own rather than staying stopped forever.
const CHILD_DEADLINE: Duration = Duration::from_secs(60);

/// What a child prints, followed by the point's name, once it has stopped.
const READY: &str = "crcbl-store kill point reached:";

/// The save's name in the storage root.
const SLOT: &str = "slot.crb";

/// How many sectors each save holds.
const SECTORS: i64 = 16;

/// Each sector's snapshot size: with [`SECTORS`], a save of several
/// megabytes, so the new one cannot land in a single small write.
const SECTOR_BYTES: usize = 512 * 1024;

/// The tick of the save already on disk when the child starts.
const OLD_TICK: u64 = 1;

/// The tick of the save the child writes over it.
const NEW_TICK: u64 = 2;

/// Where in [`write_atomic`](crate::write_atomic) a child stops to be killed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KillPoint {
    /// The temporary file is written, synced and closed; the target has not
    /// been replaced.
    BeforeRename,
    /// The target has been replaced; the directory has not been synced again.
    AfterRename,
}

impl KillPoint {
    fn name(self) -> &'static str {
        match self {
            Self::BeforeRename => "before-rename",
            Self::AfterRename => "after-rename",
        }
    }
}

/// In a child told to stop at `point`: says so on stdout and blocks until
/// it is killed. Everywhere else, including every other unit test, returns
/// at once.
pub(crate) fn reached(point: KillPoint) {
    if env::var_os(KILL_AT).as_deref() != Some(OsStr::new(point.name())) {
        return;
    }
    {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{READY} {}", point.name())
            .and_then(|()| stdout.flush())
            .expect("telling the parent the child has stopped");
    }
    let stopped = Instant::now();
    while let Some(left) = CHILD_DEADLINE.checked_sub(stopped.elapsed()) {
        thread::park_timeout(left);
    }
    panic!("stopped at {point:?} and not killed within {CHILD_DEADLINE:?}");
}

/// A save at `tick` whose every snapshot byte depends on the tick, so the old
/// save and the new one differ throughout and a file mixing them is caught.
fn save_at(tick: u64) -> SaveWriter {
    let mut writer = SaveWriter::new(SaveHeader::new(TickId::from_raw(tick), 0.0));
    for x in 0..SECTORS {
        let snapshot_data = (0..SECTOR_BYTES)
            .map(|i| (i as u64 + x as u64) as u8 ^ tick as u8)
            .collect();
        writer.add_sector(SectorSave {
            sector_id: SectorId { x, y: 0, z: 0 },
            snapshot_data,
        });
    }
    writer
}

/// The exact bytes [`save_at`]`(tick)` puts in a file.
fn bytes_of(tick: u64) -> Vec<u8> {
    let memory = MemoryStorage::new();
    save_at(tick)
        .write(&memory, Path::new(SLOT))
        .expect("encoding a save in memory");
    memory.read(Path::new(SLOT)).expect("reading it back")
}

/// A child that is killed and reaped when this is dropped, so a parent
/// failing an assertion part-way leaves no process behind.
struct Reaped(Option<Child>);

impl Reaped {
    /// Kills the child and waits for it to exit.
    fn kill(&mut self) -> ExitStatus {
        let mut child = self.0.take().expect("the child is killed once");
        child.kill().expect("killing the child");
        child.wait().expect("reaping the child")
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // Only reached when the parent is already failing: the failure
            // being reported is that one, not whether this cleanup worked.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// In a child: writes the new save over the old one in [`CHILD_DIR`],
/// stopping at the point [`KILL_AT`] names. In the parent: puts the old save
/// in a temp directory, re-runs test `name` alone in a child stopping at
/// `point`, kills it there, and returns the directory for the test to read.
fn killed_at(name: &str, point: KillPoint) -> tempfile::TempDir {
    if let Some(dir) = env::var_os(CHILD_DIR) {
        let storage = NativeStorage::at(PathBuf::from(dir));
        let written = save_at(NEW_TICK).write(&storage, Path::new(SLOT));
        panic!("the child was to stop at {point:?} and its write returned {written:?}");
    }

    let dir = tempfile::tempdir().expect("a temp directory");
    let storage = NativeStorage::at(dir.path().to_path_buf());
    save_at(OLD_TICK)
        .write(&storage, Path::new(SLOT))
        .expect("writing the save the child overwrites");

    let mut spawned = Command::new(env::current_exe().expect("a test binary knows its own path"))
        .args([
            "--exact",
            &format!("kill_during_write::{name}"),
            "--nocapture",
        ])
        .env(CHILD_DIR, dir.path())
        .env(KILL_AT, point.name())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("re-running this test binary");

    // Read on threads of their own, so a child writing more than a pipe holds
    // cannot stall, and so the parent can wait on a line with a deadline.
    let (sender, received) = mpsc::channel();
    let stdout = spawned.stdout.take().expect("a piped stdout");
    let stdout = thread::spawn(move || {
        for text in BufReader::new(stdout).lines() {
            let Ok(text) = text else { break };
            if sender.send(text).is_err() {
                break;
            }
        }
    });
    let mut stderr = spawned.stderr.take().expect("a piped stderr");
    let stderr = thread::spawn(move || {
        let mut text = String::new();
        // A read cut short by the kill keeps what it had: the report is
        // diagnostics for a failure, never what a test asserts on.
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut child = Reaped(Some(spawned));

    let ready = format!("{READY} {}", point.name());
    let started = Instant::now();
    let mut seen = Vec::new();
    let stopped = loop {
        let left = CHILD_DEADLINE.saturating_sub(started.elapsed());
        match received.recv_timeout(left) {
            // `ends_with`: the test harness may have begun the line already.
            Ok(text) if text.ends_with(&ready) => break true,
            Ok(text) => seen.push(text),
            Err(_) => break false,
        }
    };
    let status = child.kill();
    stdout.join().expect("the stdout reader");
    let stderr = stderr.join().expect("the stderr reader");
    assert!(
        stopped,
        "the child never stopped at {point:?} (exit {status:?}) — has this test been renamed?\n\
         stdout:\n{}\nstderr:\n{stderr}",
        seen.join("\n")
    );
    assert!(!status.success(), "the child exited before it was killed");
    dir
}

/// The names in `dir` other than the save's own, sorted.
fn strays(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("listing the save directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != SLOT)
        .collect();
    names.sort();
    names
}

/// Asserts the save in `dir` opens, verifies its checksum, is at `tick` and
/// is byte for byte the save written at it.
fn assert_whole_save(dir: &Path, tick: u64) {
    let storage = NativeStorage::at(dir.to_path_buf());
    let reader = SaveReader::open(&storage, Path::new(SLOT))
        .unwrap_or_else(|error| panic!("the save left behind does not open: {error}"));
    assert!(reader.data().checksum_valid);
    assert_eq!(reader.data().header.tick, TickId::from_raw(tick));
    let on_disk = fs::read(dir.join(SLOT)).expect("reading the save");
    assert!(
        on_disk == bytes_of(tick),
        "the save is not exactly the one written at tick {tick} ({} bytes on disk)",
        on_disk.len()
    );
}

/// **Killed before the rename, the previous save is untouched**, and the
/// temporary file the kill strands holds the whole new save.
///
/// The store never cleans such a file up — no part of it looks for one — so
/// this also holds the next write and read to working beside it, and the
/// file to staying where the kill left it.
#[test]
#[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
fn a_save_killed_before_its_rename_leaves_the_previous_save() {
    let dir = killed_at(
        "a_save_killed_before_its_rename_leaves_the_previous_save",
        KillPoint::BeforeRename,
    );
    assert_whole_save(dir.path(), OLD_TICK);

    let found = strays(dir.path());
    let [stray] = found.as_slice() else {
        panic!("expected the one temporary file the kill strands, found {found:?}");
    };
    assert!(
        stray.starts_with(".slot.") && stray.ends_with(".crb.tmp"),
        "{stray}"
    );
    let stranded = fs::read(dir.path().join(stray)).expect("reading the temporary file");
    assert!(
        stranded == bytes_of(NEW_TICK),
        "the kill came before the temporary file was whole"
    );

    let storage = NativeStorage::at(dir.path().to_path_buf());
    save_at(NEW_TICK)
        .write(&storage, Path::new(SLOT))
        .expect("the next write beside a stranded temporary file");
    assert_whole_save(dir.path(), NEW_TICK);
    assert_eq!(strays(dir.path()), found);
}

/// **Killed after the rename, the new save is whole** at the target and
/// nothing is stranded beside it.
#[test]
#[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
fn a_save_killed_after_its_rename_leaves_the_new_save() {
    let dir = killed_at(
        "a_save_killed_after_its_rename_leaves_the_new_save",
        KillPoint::AfterRename,
    );
    assert_whole_save(dir.path(), NEW_TICK);
    assert_eq!(strays(dir.path()), Vec::<String>::new());
}
