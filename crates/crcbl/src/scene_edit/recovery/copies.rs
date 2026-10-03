//! The copies in a recovery directory: listed newest first, pruned at
//! start-up, and removed one at a time.
//!
//! **A copy is only what [`Document::write_recovery`] makes**: a directory
//! directly under the base, not a link, named `<millis>-<scene name>` — the
//! stamp all digits and the name nothing but what `dir_name` keeps, with the
//! `-1`, `-2` a taken name gets. Anything else under the base — a file, a
//! link, a directory a person made — is not one: it is never listed and
//! never removed.
//!
//! **Nothing is removed by a pattern.** Every removal is of one path this
//! module listed or was handed, and [`remove_copy`] checks that path again —
//! its parent is the base, its name is a copy's, it is a directory — before
//! it removes anything, so a path from anywhere else is refused by name
//! ([`EditError::NotACopy`]) rather than deleted.
//!
//! # A live session's autosave is in use (decided 2026-10-03)
//!
//! Every editor sharing a recovery directory sees every other one's autosave
//! there, so each marks its slot ([`mark_in_use`]): a file beside the copy,
//! named for it with [`IN_USE_SUFFIX`], which the session holds an exclusive
//! lock on ([`std::fs::File::lock`]) for as long as the slot is its own.
//! Listing, [`remove_copy`] and pruning all pass over a copy whose marker is
//! locked, so one editor never offers, deletes or prunes another's live
//! autosave.
//!
//! **A lock, not a process id, says the session is alive.** The standard
//! library cannot ask whether a process is running, and a recorded id can be
//! reused by an unrelated process after a crash; a lock is released by the
//! operating system when its process ends, however it ends. So a crashed
//! session's marker is an unlocked file, its slot an ordinary copy — offered,
//! deletable and pruned — and removing the copy removes the marker with it.
//! The marker still holds the process id, for a person reading it once it is
//! released (Windows refuses a read of a locked file).
//!
//! The lock is `flock` on Linux and macOS and `LockFileEx` on Windows
//! ([`std::fs::File::try_lock`]'s own docs), both released when their
//! process ends. Verified on Windows; the Linux and macOS sides are the same
//! standard-library calls, compiled for both targets but not run here.
//! **What cannot be told is in use**: a marker that is there but would not
//! open, or whose lock state the platform will not say, keeps its copy out
//! of the listing and every removal, so nothing live is ever deleted.
//!
//! [`Document::write_recovery`]: crate::scene_edit::Document::write_recovery

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::scene_edit::{EditError, ownership};

/// How old a copy is let get before start-up removes it: two weeks, long
/// enough to come back to a scene after a holiday, short enough that the
/// directory does not grow for ever.
pub const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// How many copies start-up keeps, newest first, whatever their age: an
/// editor that crashes in a loop leaves this many and no more.
pub const KEEP_NEWEST: usize = 20;

/// What a copy's marker is called: the copy's directory name with this
/// after it — never a copy's name itself, since a copy's name holds no `.`.
pub const IN_USE_SUFFIX: &str = ".in-use";

/// A live session's hold on its autosave slot: the slot's marker, locked
/// until this is dropped — see the module docs.
#[derive(Debug)]
pub struct InUse {
    /// The marker, open and locked; the lock goes when the file closes.
    _marker: File,
}

/// One copy in a recovery directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryCopy {
    /// The copy's directory.
    pub dir: PathBuf,
    /// When it was written: milliseconds since the Unix epoch, read from its
    /// name.
    pub stamp: u128,
    /// The scene's name as the directory spells it, with any counter a taken
    /// name was given.
    pub name: String,
}

/// What a prune removed, and what it could not.
#[derive(Debug, Default)]
pub struct Pruned {
    /// Each copy removed.
    pub removed: Vec<PathBuf>,
    /// Each copy that would not go, and why.
    pub failed: Vec<(PathBuf, EditError)>,
}

/// Every copy under `base`, newest first — none when `base` is not there,
/// and none another session marks in use (see the module docs).
///
/// # Errors
///
/// [`EditError::Recovery`] if `base` is there and would not be read.
pub fn list_copies(base: &Path) -> Result<Vec<RecoveryCopy>, EditError> {
    let entries = match std::fs::read_dir(base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(EditError::Recovery {
                dir: base.to_path_buf(),
                source,
            });
        }
    };
    let mut copies = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| EditError::Recovery {
            dir: base.to_path_buf(),
            source,
        })?;
        let file_name = entry.file_name();
        let Some((stamp, name)) = file_name.to_str().and_then(parse) else {
            continue;
        };
        // The entry's own type, which does not follow a link: a link named
        // like a copy is not one.
        let file_type = entry.file_type().map_err(|source| EditError::Recovery {
            dir: entry.path(),
            source,
        })?;
        if file_type.is_dir() && !in_use(&entry.path()) {
            copies.push(RecoveryCopy {
                dir: entry.path(),
                stamp,
                name: name.to_owned(),
            });
        }
    }
    copies.sort_by(|a, b| b.stamp.cmp(&a.stamp).then_with(|| b.dir.cmp(&a.dir)));
    Ok(copies)
}

/// Removes the copy at `dir`, which must be a copy directly under `base` —
/// see the module docs — and its marker, if it has one. A copy already gone
/// is removed.
///
/// # Errors
///
/// [`EditError::NotACopy`] for a path that is not a copy under `base`, and
/// [`EditError::CopyInUse`] for a live session's autosave, each removing
/// nothing; [`EditError::RemoveCopy`] if the filesystem would not remove the
/// copy or its marker.
pub fn remove_copy(base: &Path, dir: &Path) -> Result<(), EditError> {
    check_copy(base, dir)?;
    if in_use(dir) {
        return Err(EditError::CopyInUse(dir.to_path_buf()));
    }
    match std::fs::symlink_metadata(dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(EditError::RemoveCopy {
                dir: dir.to_path_buf(),
                source,
            });
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(EditError::NotACopy(dir.to_path_buf()));
        }
        Ok(_) => std::fs::remove_dir_all(dir).map_err(|source| EditError::RemoveCopy {
            dir: dir.to_path_buf(),
            source,
        })?,
    }
    // After the copy, so a copy that would not go keeps its marker: an
    // unlocked marker is inert, and goes with the copy next time.
    let marker = marker(dir);
    match std::fs::remove_file(&marker) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(EditError::RemoveCopy {
            dir: marker,
            source: error,
        }),
        _ => Ok(()),
    }
}

/// Marks the copy at `dir`, which must be a copy directly under `base`, as
/// this session's live autosave until the [`InUse`] handed back is dropped —
/// see the module docs.
///
/// # Errors
///
/// [`EditError::NotACopy`] for a path that is not a copy under `base`;
/// [`EditError::Recovery`] if the marker would not be made, locked or
/// written. A marker left unlocked by a failure is a crashed session's, and
/// goes with its copy.
pub fn mark_in_use(base: &Path, dir: &Path) -> Result<InUse, EditError> {
    check_copy(base, dir)?;
    let path = marker(dir);
    let failed = |source| EditError::Recovery {
        dir: path.clone(),
        source,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(failed)?;
    // Blocking rather than trying: another editor's check holds the lock for
    // the moment it takes to read it, and this one waits that out.
    file.lock().map_err(failed)?;
    writeln!(file, "{}", std::process::id()).map_err(failed)?;
    Ok(InUse { _marker: file })
}

/// Removes every copy under `base` older than [`MAX_AGE`] at `now`
/// (milliseconds since the Unix epoch), and every one past the newest
/// [`KEEP_NEWEST`] — except the copies `keep` names, which a person has open.
/// A live session's autosave is not listed, so it is neither removed nor
/// counted among the newest.
///
/// A copy stamped after `now`, from a clock set back since, is not old.
///
/// # Errors
///
/// As [`list_copies`]. A copy that would not go is in [`Pruned::failed`]
/// and the rest are still pruned.
pub fn prune_copies(base: &Path, now: u128, keep: &[PathBuf]) -> Result<Pruned, EditError> {
    let mut pruned = Pruned::default();
    for (index, copy) in list_copies(base)?.into_iter().enumerate() {
        let old = now.saturating_sub(copy.stamp) > MAX_AGE.as_millis();
        let excess = index >= KEEP_NEWEST;
        let kept = keep.iter().any(|open| ownership::same_dir(open, &copy.dir));
        if !(old || excess) || kept {
            continue;
        }
        match remove_copy(base, &copy.dir) {
            Ok(()) => pruned.removed.push(copy.dir),
            Err(error) => pruned.failed.push((copy.dir, error)),
        }
    }
    Ok(pruned)
}

/// [`EditError::NotACopy`] unless `dir` is named as a copy is and its parent
/// is `base` — what [`remove_copy`] and [`mark_in_use`] check before they
/// touch anything.
fn check_copy(base: &Path, dir: &Path) -> Result<(), EditError> {
    let named = dir
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(parse)
        .is_some();
    let under_base = dir
        .parent()
        .is_some_and(|parent| ownership::same_dir(parent, base));
    if named && under_base {
        Ok(())
    } else {
        Err(EditError::NotACopy(dir.to_path_buf()))
    }
}

/// The marker beside the copy at `dir`: its name with [`IN_USE_SUFFIX`].
fn marker(dir: &Path) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_default().to_os_string();
    name.push(IN_USE_SUFFIX);
    dir.with_file_name(name)
}

/// Whether a live session holds the copy at `dir` as its autosave — see the
/// module docs. A marker that is there and cannot be checked counts as held.
fn in_use(dir: &Path) -> bool {
    // Open for writing too: an exclusive lock on a file open only for reading
    // is left unspecified by `File::try_lock`'s docs.
    let file = match OpenOptions::new().read(true).write(true).open(marker(dir)) {
        Ok(file) => file,
        Err(error) => return error.kind() != std::io::ErrorKind::NotFound,
    };
    // Taken here, the lock is released as `file` drops at the end of this
    // function: nobody else held it.
    match file.try_lock() {
        Ok(()) => false,
        Err(TryLockError::WouldBlock | TryLockError::Error(_)) => true,
    }
}

/// A copy's stamp and name, from its directory name — or [`None`] for a
/// name [`Document::write_recovery`] never makes.
///
/// [`Document::write_recovery`]: crate::scene_edit::Document::write_recovery
fn parse(dir_name: &str) -> Option<(u128, &str)> {
    let (stamp, name) = dir_name.split_once('-')?;
    let kept = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if stamp.is_empty() || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if name.is_empty() || !name.chars().all(kept) {
        return None;
    }
    Some((stamp.parse().ok()?, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A day, in milliseconds.
    const DAY_MS: u128 = 24 * 60 * 60 * 1000;

    /// Makes a copy-shaped directory `name` under `base`, holding one file,
    /// and hands it back.
    fn copy_at(base: &Path, name: &str) -> PathBuf {
        let dir = base.join(name);
        std::fs::create_dir_all(dir.join("sys")).expect("a fresh base");
        std::fs::write(dir.join("scene.ron"), name).expect("written");
        dir
    }

    /// **Only copies are listed, newest first**: a file, a directory a
    /// person named and a name with no stamp are passed over.
    #[test]
    fn only_copies_are_listed_newest_first() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let older = copy_at(base.path(), "100-greybox");
        let newer = copy_at(base.path(), "200-field-1");
        copy_at(base.path(), "notes");
        copy_at(base.path(), "x-greybox");
        copy_at(base.path(), "300-");
        copy_at(base.path(), "400-a b");
        std::fs::write(base.path().join("500-file"), "a file").expect("written");

        let copies = list_copies(base.path()).expect("readable");
        assert_eq!(
            copies,
            [
                RecoveryCopy {
                    dir: newer,
                    stamp: 200,
                    name: "field-1".to_owned()
                },
                RecoveryCopy {
                    dir: older,
                    stamp: 100,
                    name: "greybox".to_owned()
                },
            ]
        );
        assert_eq!(
            list_copies(&base.path().join("absent")).expect("nothing to read"),
            []
        );
    }

    /// **A copy is removed by its path, and nothing beside it is**; a path
    /// that is not a copy directly under the base is refused and left.
    #[test]
    fn a_copy_is_removed_and_anything_else_refused() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let gone = copy_at(base.path(), "1-greybox");
        let stays = copy_at(base.path(), "2-greybox");
        remove_copy(base.path(), &gone).expect("a copy");
        assert!(!gone.exists(), "the copy is still there");
        assert!(stays.join("scene.ron").exists(), "its neighbour went");
        remove_copy(base.path(), &gone).expect("gone already is removed");

        let elsewhere = tempfile::tempdir().expect("a temporary directory");
        let outside = copy_at(elsewhere.path(), "3-greybox");
        let notes = copy_at(base.path(), "notes");
        let nested = copy_at(&stays, "4-greybox");
        for refused in [
            outside.clone(),
            notes.clone(),
            nested.clone(),
            base.path().to_path_buf(),
            stays.join(".."),
        ] {
            assert!(
                matches!(
                    remove_copy(base.path(), &refused),
                    Err(EditError::NotACopy(path)) if path == refused
                ),
                "{} was not refused",
                refused.display()
            );
        }
        for left in [&outside, &notes, &nested, &stays] {
            assert!(left.join("scene.ron").exists(), "{}", left.display());
        }
    }

    /// **A prune removes a copy past [`MAX_AGE`] and keeps a younger one**,
    /// by path — and keeps an old copy `keep` names, which is open, and
    /// whatever under the base is not a copy.
    #[test]
    fn a_prune_removes_old_copies_and_keeps_the_open_one() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let now = 100 * DAY_MS;
        let old = copy_at(base.path(), &format!("{}-old", now - 15 * DAY_MS));
        let young = copy_at(base.path(), &format!("{}-young", now - 13 * DAY_MS));
        let open = copy_at(base.path(), &format!("{}-open", now - 30 * DAY_MS));
        let ahead = copy_at(base.path(), &format!("{}-ahead", now + DAY_MS));
        let notes = copy_at(base.path(), "notes");

        let pruned = prune_copies(base.path(), now, std::slice::from_ref(&open)).expect("readable");
        assert!(pruned.failed.is_empty(), "{:?}", pruned.failed);
        assert_eq!(pruned.removed, std::slice::from_ref(&old));
        assert!(!old.exists(), "the old copy was left");
        for kept in [&young, &open, &ahead, &notes] {
            assert!(kept.exists(), "{} was pruned", kept.display());
        }
    }

    /// **A prune keeps the newest [`KEEP_NEWEST`] copies and removes the
    /// rest**, however young — but not one `keep` names.
    #[test]
    fn a_prune_removes_copies_past_the_newest_and_keeps_the_open_one() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let now = 100 * DAY_MS;
        let copies: Vec<PathBuf> = (0..KEEP_NEWEST + 3)
            .map(|index| {
                let stamp = now - u128::try_from(index).expect("small") * 1000;
                copy_at(base.path(), &format!("{stamp}-scene"))
            })
            .collect();
        let open = copies[KEEP_NEWEST + 1].clone();

        let pruned = prune_copies(base.path(), now, std::slice::from_ref(&open)).expect("readable");
        assert!(pruned.failed.is_empty(), "{:?}", pruned.failed);
        assert_eq!(
            pruned.removed,
            [copies[KEEP_NEWEST].clone(), copies[KEEP_NEWEST + 2].clone()]
        );
        for (index, copy) in copies.iter().enumerate() {
            let removed = index == KEEP_NEWEST || index == KEEP_NEWEST + 2;
            assert_eq!(copy.exists(), !removed, "{}", copy.display());
        }
    }

    /// **A live session's autosave is passed over by everything that
    /// removes**: not listed, Delete refused by name, and not pruned however
    /// old — and once the session lets go, it is an ordinary copy, listed,
    /// pruned and its marker gone with it.
    #[test]
    fn a_live_autosave_is_not_listed_removed_or_pruned() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let now = 100 * DAY_MS;
        let live = copy_at(base.path(), &format!("{}-live", now - 30 * DAY_MS));
        let young = copy_at(base.path(), &format!("{}-young", now - DAY_MS));
        let held = mark_in_use(base.path(), &live).expect("a copy");

        let listed: Vec<PathBuf> = list_copies(base.path())
            .expect("readable")
            .into_iter()
            .map(|copy| copy.dir)
            .collect();
        assert_eq!(listed, std::slice::from_ref(&young), "the live slot listed");
        assert!(
            matches!(
                remove_copy(base.path(), &live),
                Err(EditError::CopyInUse(path)) if path == live
            ),
            "a live slot was not refused"
        );
        let pruned = prune_copies(base.path(), now, &[]).expect("readable");
        assert!(pruned.removed.is_empty(), "{:?}", pruned.removed);
        assert!(pruned.failed.is_empty(), "{:?}", pruned.failed);
        assert!(live.join("scene.ron").exists(), "the live slot went");

        drop(held);
        assert_eq!(list_copies(base.path()).expect("readable").len(), 2);
        let pruned = prune_copies(base.path(), now, &[]).expect("readable");
        assert_eq!(pruned.removed, std::slice::from_ref(&live));
        assert!(!live.exists(), "the released slot was left");
        assert!(!marker(&live).exists(), "its marker was left");
        assert!(young.exists(), "a young copy was pruned");
    }

    /// **A crashed session's marker is an ordinary copy's**: a marker file
    /// nobody holds, as the operating system leaves one, keeps nothing out
    /// of the listing, and Delete removes it with its copy.
    #[test]
    fn a_crashed_sessions_marker_makes_an_ordinary_copy() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let copy = copy_at(base.path(), "1-greybox");
        std::fs::write(
            marker(&copy),
            "4242
",
        )
        .expect("written");

        assert_eq!(list_copies(base.path()).expect("readable").len(), 1);
        remove_copy(base.path(), &copy).expect("an ordinary copy");
        assert!(!copy.exists(), "the copy was left");
        assert!(!marker(&copy).exists(), "its marker was left");
    }

    /// **Only a copy is marked**: a path that is not a copy directly under
    /// the base is refused, and no marker is made beside it.
    #[test]
    fn only_a_copy_is_marked_in_use() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let notes = copy_at(base.path(), "notes");
        let elsewhere = tempfile::tempdir().expect("a temporary directory");
        let outside = copy_at(elsewhere.path(), "1-greybox");
        for refused in [&notes, &outside] {
            assert!(
                matches!(
                    mark_in_use(base.path(), refused),
                    Err(EditError::NotACopy(path)) if path == *refused
                ),
                "{} was marked",
                refused.display()
            );
            assert!(!marker(refused).exists(), "{}", refused.display());
        }
    }

    /// **A marker that cannot be checked keeps its copy**: one that would
    /// not open — here a directory where the file should be — counts as a
    /// live session's, so the copy is neither listed nor removed.
    #[test]
    fn a_marker_that_cannot_be_checked_keeps_its_copy() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let copy = copy_at(base.path(), "1-greybox");
        std::fs::create_dir(marker(&copy)).expect("a fresh base");

        assert_eq!(list_copies(base.path()).expect("readable"), []);
        assert!(matches!(
            remove_copy(base.path(), &copy),
            Err(EditError::CopyInUse(_))
        ));
        assert!(copy.join("scene.ron").exists(), "the copy went");
    }
}
