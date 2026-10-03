//! What a recovery copy records of where its scene lived: a file beside the
//! scene's own, [`SIDECAR`], naming the directory the scene was opened from
//! or last saved to and the asset root its meshes were read from.
//!
//! **The scene loader never reads it.** `Scene::load` reads `scene.ron`,
//! `env.ron`, a `sys/<name>.ron` per system the manifest lists and
//! `names.ron` when the header declares it — nothing else in the directory —
//! and no scene writes a key called [`SIDECAR`], so the file is never one of
//! the scene's.
//!
//! **What it holds is untrusted.** A copy sits in a temporary directory
//! anyone could have written into, and a directory it names may have been
//! removed or replaced since. So [`read`] keeps a path only when it is
//! absolute and a directory now, and passes over anything else with a note
//! saying what; and nothing is ever written or removed through what it
//! names — the asset root is only read from, and the scene's old directory
//! is only offered as the save-as line's text, which a person commits or
//! not, through the same checks as anything typed there.
//!
//! # The format
//!
//! Plain text, a line per thing recorded, each `<key>=<path>`:
//!
//! ```text
//! origin=/home/me/game/assets/scenes/field.scn
//! assets=/home/me/game
//! ```
//!
//! Either line may be missing, and a copy with neither has no sidecar at
//! all. A path that is not text, or holds a control character such as the
//! line break the format splits on, is not recorded.

use std::path::{Path, PathBuf};

/// What the sidecar is called, beside the copy's `scene.ron`.
pub const SIDECAR: &str = "origin.txt";

/// The key of the line naming the scene's directory.
const ORIGIN_KEY: &str = "origin";

/// The key of the line naming its asset root.
const ASSETS_KEY: &str = "assets";

/// Where a scene lived, as a copy records it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Home {
    /// The directory the scene was opened from or last saved to.
    pub(super) origin: Option<PathBuf>,
    /// The asset root its meshes were read from.
    pub(super) assets: Option<PathBuf>,
}

/// The sidecar's text for `home`, or [`None`] when nothing in it can be
/// recorded — see the module docs. Each path is made absolute first, so it
/// names the same directory whatever directory the next run starts in.
pub(super) fn text(home: &Home) -> Option<String> {
    let lines: String = [(ORIGIN_KEY, &home.origin), (ASSETS_KEY, &home.assets)]
        .into_iter()
        .filter_map(|(key, path)| {
            let path = std::path::absolute(path.as_deref()?).ok()?;
            let path = path.to_str()?;
            (!path.chars().any(char::is_control)).then(|| format!("{key}={path}\n"))
        })
        .collect();
    (!lines.is_empty()).then_some(lines)
}

/// The sidecar in the copy at `dir`, checked — see the module docs: what it
/// names that is still a directory, and a note for each thing passed over.
/// A copy with no sidecar is where nothing was recorded, and says nothing.
pub(super) fn read(dir: &Path) -> (Home, Vec<String>) {
    let mut home = Home::default();
    let mut notes = Vec::new();
    let text = match std::fs::read_to_string(dir.join(SIDECAR)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (home, notes),
        Err(error) => {
            notes.push(format!(
                "the copy's `{SIDECAR}` would not be read ({error}), so where the scene lived \
                 is not known"
            ));
            return (home, notes);
        }
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let recorded = line.split_once('=').and_then(|(key, path)| match key {
            ORIGIN_KEY => Some((&mut home.origin, "where the scene lived", path)),
            ASSETS_KEY => Some((&mut home.assets, "its asset root", path)),
            _ => None,
        });
        let Some((slot, what, path)) = recorded else {
            notes.push(format!(
                "a line the copy's `{SIDECAR}` holds is not one it writes"
            ));
            continue;
        };
        let path = Path::new(path);
        if path.is_absolute() && path.is_dir() {
            *slot = Some(path.to_path_buf());
        } else {
            notes.push(format!(
                "`{}`, recorded as {what}, is not a directory now, so it was passed over",
                path.display()
            ));
        }
    }
    (home, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **What is written is read back**, each path made absolute; a home
    /// with nothing recordable writes no sidecar at all.
    #[test]
    fn a_home_round_trips_through_its_text() {
        let scenes = tempfile::tempdir().expect("a temporary directory");
        let origin = scenes.path().join("one.scn");
        std::fs::create_dir(&origin).expect("a fresh directory");
        let home = Home {
            origin: Some(origin.clone()),
            assets: Some(scenes.path().to_path_buf()),
        };
        let written = text(&home).expect("both are text");
        assert_eq!(
            written,
            format!(
                "origin={}\nassets={}\n",
                origin.display(),
                scenes.path().display()
            )
        );

        let copy = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(copy.path().join(SIDECAR), &written).expect("written");
        assert_eq!(read(copy.path()), (home, Vec::new()));

        assert_eq!(text(&Home::default()), None);
        let unwritable = Home {
            origin: Some(PathBuf::from("a\nline")),
            assets: None,
        };
        assert_eq!(text(&unwritable), None, "a line break was recorded");
    }

    /// **Nothing a sidecar names is trusted**: a directory gone since, a file
    /// where a directory was, a relative path and a line it never writes are
    /// each passed over with a note — and a missing sidecar says nothing.
    #[test]
    fn a_stale_sidecar_is_passed_over_with_a_note() {
        let scenes = tempfile::tempdir().expect("a temporary directory");
        let gone = scenes.path().join("gone.scn");
        let file = scenes.path().join("file");
        std::fs::write(&file, "not a directory").expect("written");
        let copy = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(read(copy.path()), (Home::default(), Vec::new()));

        std::fs::write(
            copy.path().join(SIDECAR),
            format!(
                "origin={}\nassets={}\norigin=relative\nsomething else\n",
                gone.display(),
                file.display()
            ),
        )
        .expect("written");
        let (home, notes) = read(copy.path());
        assert_eq!(home, Home::default(), "a stale path was kept");
        assert_eq!(notes.len(), 4, "{notes:?}");
        assert!(notes[0].contains("gone.scn"), "{notes:?}");
        assert!(notes[1].contains("asset root"), "{notes:?}");
        assert!(!gone.exists(), "reading made the directory");
    }
}
