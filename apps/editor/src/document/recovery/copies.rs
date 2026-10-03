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
//! [`Document::write_recovery`]: crate::document::Document::write_recovery

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::document::{EditError, ownership};

/// How old a copy is let get before start-up removes it: two weeks, long
/// enough to come back to a scene after a holiday, short enough that the
/// directory does not grow for ever.
pub const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// How many copies start-up keeps, newest first, whatever their age: an
/// editor that crashes in a loop leaves this many and no more.
pub const KEEP_NEWEST: usize = 20;

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

/// Every copy under `base`, newest first — none when `base` is not there.
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
        if file_type.is_dir() {
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
/// see the module docs. A copy already gone is removed.
///
/// # Errors
///
/// [`EditError::NotACopy`] for a path that is not a copy under `base`,
/// removing nothing; [`EditError::RemoveCopy`] if the filesystem would not
/// remove it.
pub fn remove_copy(base: &Path, dir: &Path) -> Result<(), EditError> {
    let not_a_copy = || EditError::NotACopy(dir.to_path_buf());
    let named = dir
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(parse)
        .is_some();
    let under_base = dir
        .parent()
        .is_some_and(|parent| ownership::same_dir(parent, base));
    if !named || !under_base {
        return Err(not_a_copy());
    }
    match std::fs::symlink_metadata(dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(EditError::RemoveCopy {
                dir: dir.to_path_buf(),
                source,
            });
        }
        Ok(metadata) if !metadata.is_dir() => return Err(not_a_copy()),
        Ok(_) => {}
    }
    std::fs::remove_dir_all(dir).map_err(|source| EditError::RemoveCopy {
        dir: dir.to_path_buf(),
        source,
    })
}

/// Removes every copy under `base` older than [`MAX_AGE`] at `now`
/// (milliseconds since the Unix epoch), and every one past the newest
/// [`KEEP_NEWEST`] — except the copies `keep` names, which a person has open.
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

/// A copy's stamp and name, from its directory name — or [`None`] for a
/// name [`Document::write_recovery`] never makes.
///
/// [`Document::write_recovery`]: crate::document::Document::write_recovery
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
}
