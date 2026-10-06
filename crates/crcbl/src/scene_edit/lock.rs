//! A scene directory shared with other programs: the lock an editing
//! program holds on it, and what a save does when something wrote the scene
//! without taking that lock.
//!
//! # The lock (decided 2026-10-04, for the long term)
//!
//! * **A file in the scene's own directory**, [`SCENE_LOCK`], held under an
//!   exclusive [`std::fs::File::try_lock`] for as long as a program edits the
//!   scene — the recovery autosave's in-use marker's mechanism
//!   (`scene_edit::recovery`'s `copies` module docs say why a lock and not a
//!   process id). The operating system releases it when its process ends,
//!   however it ends, so a crashed holder's file is an unlocked file and
//!   blocks nobody. Hidden and ignored by git as [`HISTORY`] is: the scene
//!   loader never reads it, because it reads only what the manifest names,
//!   and a save never removes it, because a save removes only files the
//!   document owns.
//! * **Never removed when it is let go.** Removing a lock file on release
//!   races the next program to lock it: on Linux and macOS the next one may
//!   have opened the old file and lock a name nobody else will find, and on
//!   Windows a file another process has open is not removed at once. An
//!   unlocked file costs nothing.
//! * **The editor holds it from open until it lets the scene go**: a new
//!   scene, another scene opened, a save-as moving the document, or the
//!   editor closing. A document holds a lock only on its own directory
//!   ([`Document::open_locked`], [`Document::lock_origin`]), so whatever puts
//!   another directory in its place lets the old lock go with it. The
//!   `crcbl scene` CLI takes it for the length of one edit run.
//! * **A lock already held is refused, never waited on** ([`lock_scene`],
//!   [`EditError::Locked`]): an editor can hold a scene for hours, and a
//!   terminal waiting that long has hung, not queued.
//! * **The holder is named where the platform allows it.** The file holds
//!   the holder's program name and process id, written once it is locked; on
//!   Linux and macOS a refused program reads that line, and on Windows, which
//!   refuses a read of a locked file, the refusal says only that another
//!   program holds it. Written for a person reading it, never parsed for a
//!   decision.
//! * **Only a scene directory is locked**: a directory holding no
//!   `scene.ron` is refused before any file is made in it, so a mistyped
//!   path leaves nothing behind.
//!
//! The lock is `flock` on Linux and macOS and `LockFileEx` on Windows
//! ([`std::fs::File::try_lock`]'s own docs). Verified on Windows; the macOS
//! side is the same standard-library calls, compiled but not run, and the
//! Linux side was not built.
//!
//! # A scene changed behind the document's back (decided 2026-10-04)
//!
//! The lock binds only the programs that take it. An older build, a person's
//! text editor or a checkout writes the scene without asking, so a document
//! also remembers what its directory held when it last read or wrote it — a
//! SHA-256 of each file it owns there, by key — and **a save into that
//! directory refuses when the files are no longer those**
//! ([`EditError::ChangedOnDisk`]), writing nothing. The check reads the document's own files again at save
//! time, which is a handful of small files. The caller decides what next: the
//! editor asks whether to overwrite them or to reload the scene from disk,
//! and [`Document::accept_changes_on_disk`] is how it says to overwrite.
//!
//! **One digest per file, not one over them all** (decided 2026-10-06), so a
//! chunk reloaded from disk is taken in alone ([`Document::reload_chunk`]):
//! another file changed at the same moment still makes the next save ask,
//! and a chunk file holding what the document itself last wrote there is
//! told apart from one another program changed — a watch sees the
//! document's own save as a change like any other.
//!
//! # What neither touches
//!
//! Recovery copies and the autosave: [`Document::write_recovery`] writes a
//! directory of its own under the recovery base, which takes no lock and is
//! not the document's directory, so neither the lock nor the check stands in
//! its way, and an editor's autosave runs while it holds its scene's lock.
//!
//! [`HISTORY`]: super::HISTORY

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use crate::registry::Registry;

use crate::shaders::sha256::sha256;

use super::history::DIGEST_BYTES;
use super::{Document, EditError, ownership};

/// The lock file's name in the scene directory.
pub const SCENE_LOCK: &str = ".crcbl-lock";

/// The file that makes a directory a scene, which a lock is taken only
/// beside — see the module docs.
const HEADER: &str = "scene.ron";

/// A program's hold on a scene directory: its [`SCENE_LOCK`], open and
/// locked until this is dropped — see the module docs.
#[derive(Debug)]
pub struct SceneLock {
    /// The lock file; the lock goes when it closes.
    _file: File,
    /// The scene directory it locks.
    dir: PathBuf,
}

impl SceneLock {
    /// The scene directory this lock holds.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Locks the scene directory `dir` for this process until the [`SceneLock`]
/// handed back is dropped, writing this program's name and process id into
/// it — see the module docs.
///
/// # Errors
///
/// [`EditError::Locked`] while another holder has it, naming that holder
/// where the platform lets its line be read; [`EditError::Lock`] for a
/// directory holding no `scene.ron`, or a lock file that would not be made,
/// locked or written. Nothing in the scene is read or written either way.
pub fn lock_scene(dir: impl Into<PathBuf>) -> Result<SceneLock, EditError> {
    let dir = dir.into();
    if !dir.join(HEADER).is_file() {
        return Err(EditError::Lock {
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("it holds no `{HEADER}`, so it is not a scene"),
            ),
            dir,
        });
    }
    // Not truncated on open: the holder's line is what a refused program
    // reads, and truncating before the lock is held would wipe it.
    let opened = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(SCENE_LOCK));
    let mut file = match opened {
        Ok(file) => file,
        Err(source) => return Err(EditError::Lock { dir, source }),
    };
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let holder = holder_of(&mut file);
            return Err(EditError::Locked { dir, holder });
        }
        Err(TryLockError::Error(source)) => return Err(EditError::Lock { dir, source }),
    }
    let written = file
        .set_len(0)
        .and_then(|()| writeln!(file, "{}", this_holder()));
    match written {
        Ok(()) => Ok(SceneLock { _file: file, dir }),
        Err(source) => Err(EditError::Lock { dir, source }),
    }
}

/// How this process names itself in a lock it holds: its program's name and
/// its process id.
fn this_holder() -> String {
    let program = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "a crcbl program".to_owned());
    format!("{program} (process {})", std::process::id())
}

/// The holder's line in a lock file another process holds, or [`None`] where
/// it cannot be read: Windows refuses a read of a locked file, which is the
/// case this exists for and no failure of the refusal it goes into. An empty
/// file — read between the holder's lock and its write — names nobody.
fn holder_of(file: &mut File) -> Option<String> {
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    let line = text.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

impl Document {
    /// [`open_with_history_or_fresh`](Self::open_with_history_or_fresh) of
    /// the directory `lock` holds, the document then holding `lock` until it
    /// lets that directory go — what the editor opens a scene with; see the
    /// module docs.
    ///
    /// # Errors
    ///
    /// As [`open_with_history_or_fresh`](Self::open_with_history_or_fresh);
    /// the lock is let go with the refusal.
    pub fn open_locked(lock: SceneLock, registry: Registry) -> Result<Self, EditError> {
        let mut document = Self::open_with_history_or_fresh(lock.dir.clone(), registry)?;
        document.lock = Some(lock);
        Ok(document)
    }

    /// Locks the document's own directory for it, until it lets that
    /// directory go — what the editor does once a save-as has moved the
    /// document. A document holding its directory's lock already keeps it.
    ///
    /// # Errors
    ///
    /// [`EditError::NoOrigin`] for a document with no directory, and as
    /// [`lock_scene`]; the document then holds no lock.
    pub fn lock_origin(&mut self) -> Result<(), EditError> {
        let origin = self.origin.clone().ok_or(EditError::NoOrigin)?;
        if self.holds_lock_on(&origin) {
            return Ok(());
        }
        self.lock = None;
        self.lock = Some(lock_scene(origin)?);
        Ok(())
    }

    /// Whether this document holds the lock on the scene directory `dir`.
    #[must_use]
    pub fn holds_lock_on(&self, dir: &Path) -> bool {
        self.lock
            .as_ref()
            .is_some_and(|lock| ownership::same_dir(&lock.dir, dir))
    }

    /// Hands this document's lock to `next` when `next` is the same scene
    /// directory read again and holds no lock of its own — an editor opening
    /// the scene it already holds, or reloading it from disk, cannot lock it
    /// a second time, since a lock refuses every other holder, its own
    /// process too. Anything else leaves both as they were.
    pub fn hand_lock_to(&mut self, next: &mut Self) {
        let same = next
            .origin
            .as_deref()
            .is_some_and(|origin| self.holds_lock_on(origin));
        if same && next.lock.is_none() {
            next.lock = self.lock.take();
        }
    }

    /// Takes the document's directory as it stands now as what the document
    /// last read, so the next save into it overwrites whatever another
    /// program wrote there since — see the module docs. A document with no
    /// directory has nothing to take.
    pub fn accept_changes_on_disk(&mut self) {
        self.record_disk();
    }

    /// Remembers what the document's own files in its directory hold now —
    /// see the module docs. [`None`] for a document with no directory.
    pub(super) fn record_disk(&mut self) {
        self.on_disk = self
            .origin
            .as_deref()
            .map(|dir| disk_digests(dir, &self.owned));
    }

    /// Remembers `bytes` as what the file `key` of the document's directory
    /// holds, and no other file — a chunk reloaded from it, taken in alone
    /// (see the module docs). A document with no directory remembers nothing.
    pub(super) fn record_file(&mut self, key: &str, bytes: &[u8]) {
        if let Some(on_disk) = &mut self.on_disk {
            on_disk.insert(key.to_owned(), sha256(bytes));
        }
    }

    /// Whether `bytes` are what the file `key` of the document's directory
    /// held when the document last read or wrote it — its own writing, or a
    /// file nobody has changed since.
    pub(super) fn last_read(&self, key: &str, bytes: &[u8]) -> bool {
        self.on_disk
            .as_ref()
            .and_then(|on_disk| on_disk.get(key))
            .is_some_and(|recorded| *recorded == sha256(bytes))
    }

    /// [`EditError::ChangedOnDisk`] when the document's own files in `dir`,
    /// its directory, no longer hold what it last read or wrote there — see
    /// the module docs.
    pub(super) fn refuse_changed_on_disk(&self, dir: &Path) -> Result<(), EditError> {
        match &self.on_disk {
            Some(recorded) if disk_digests(dir, &self.owned) != *recorded => {
                Err(EditError::ChangedOnDisk(dir.to_path_buf()))
            }
            _ => Ok(()),
        }
    }
}

/// What the document remembers of its directory: a SHA-256 of each file's
/// bytes, by key.
pub(super) type DiskDigests = BTreeMap<String, [u8; DIGEST_BYTES]>;

/// The SHA-256 of each file `keys` names in `dir`, as its bytes stand on
/// disk. A file that is not there, or will not be read, is left out, so it
/// differs from the file that was read; one unreadable at both ends compares
/// the same, and the save that follows meets the reason itself when it
/// writes.
fn disk_digests(dir: &Path, keys: &BTreeSet<String>) -> DiskDigests {
    keys.iter()
        .filter_map(|key| {
            std::fs::read(dir.join(key))
                .ok()
                .map(|bytes| (key.clone(), sha256(&bytes)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory holding a scene header, which is all a lock asks of it.
    fn scene_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(dir.path().join(HEADER), "a header").expect("written");
        dir
    }

    /// **A held lock refuses every other holder, this process too, and is
    /// free once dropped** — and names its holder where the platform reads
    /// a locked file.
    #[test]
    fn a_held_lock_refuses_another_until_it_is_dropped() {
        let dir = scene_dir();
        let held = lock_scene(dir.path()).expect("nobody holds it");
        assert_eq!(held.dir(), dir.path());
        match lock_scene(dir.path()) {
            Err(EditError::Locked {
                dir: refused,
                holder,
            }) => {
                assert_eq!(refused, dir.path());
                let pid = format!("process {}", std::process::id());
                if cfg!(windows) {
                    assert_eq!(holder, None, "Windows read a locked file");
                } else {
                    assert!(
                        holder.as_deref().is_some_and(|line| line.contains(&pid)),
                        "{holder:?}"
                    );
                }
            }
            other => panic!("a held lock was not refused: {other:?}"),
        }
        drop(held);
        lock_scene(dir.path()).expect("a dropped lock is free");
    }

    /// **A lock file nobody holds is free**, as a crashed holder's is left by
    /// the operating system: the next program locks it and writes itself in.
    #[test]
    fn a_crashed_holders_lock_file_blocks_nobody() {
        let dir = scene_dir();
        std::fs::write(dir.path().join(SCENE_LOCK), "editor (process 4242)\n").expect("written");
        let held = lock_scene(dir.path()).expect("an unheld file is free");
        drop(held);
        let line = std::fs::read_to_string(dir.path().join(SCENE_LOCK)).expect("readable");
        assert_eq!(line.trim(), this_holder(), "the old holder's line was kept");
    }

    /// **Only a scene directory is locked**: one holding no header is
    /// refused, and no lock file is made in it.
    #[test]
    fn a_directory_that_is_no_scene_is_not_locked() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        assert!(matches!(
            lock_scene(dir.path()),
            Err(EditError::Lock { .. })
        ));
        assert!(
            !dir.path().join(SCENE_LOCK).exists(),
            "a lock file was left"
        );
    }
}
