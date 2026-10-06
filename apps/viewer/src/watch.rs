//! Noticing that the document on disk has been written again.
//!
//! The re-export loop (V-F4): re-export from Blender and the viewer
//! picks it up. This is the *noticing* half — [`crate::app`] owns what happens
//! next.
//!
//! The poll, the settle and what a stamp cannot see are the engine's
//! [`crcbl::assets::watch`], whose module docs make the argument for polling
//! rather than subscribing; this module is the viewer's one path on it, and
//! the clock the viewer drives it with. It began here and moved to the engine
//! when the sandbox's scene reload needed the same watch (2026-10-06).
//!
//! **Native only**, as the engine's watch is: a browser has no file to look
//! at, so a page's viewer has no watch rather than one that never fires.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crcbl::assets::watch::PolledWatch;

/// One path, and the clock its watch is polled on.
#[derive(Debug)]
pub struct Watch {
    path: PathBuf,
    inner: PolledWatch,
    /// Wall-clock time since the watch was made: the sum of every `dt`
    /// [`Watch::poll`] was handed, which is the clock the engine's watch reads.
    clock: Duration,
}

impl Watch {
    /// Watches `path`, treating whatever is there now as already loaded.
    ///
    /// Which is what the caller has just done: [`crate::app::with_shell`] loads
    /// the document before it opens a window, so a watch that offered the same
    /// file again on its first poll would rebuild the scene for nothing.
    #[must_use]
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            inner: PolledWatch::new([path.to_path_buf()], Duration::ZERO),
            clock: Duration::ZERO,
        }
    }

    /// The path being watched.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Advances the clock by `dt` seconds and reports whether the file should
    /// be read again.
    ///
    /// **`dt` is wall-clock seconds, and it has to be.** Stepped on the
    /// simulation instead — which is where this used to be driven from — the
    /// clock stops the moment the pause panel is up, and an exporter writing
    /// the file goes unnoticed for as long as the panel stays open. See
    /// `Viewer::poll_for_re_export` in [`crate::app`].
    ///
    /// True at most once per change, and never for a file that is still being
    /// written — see [`crcbl::assets::watch`]. A path that does not exist
    /// reports false rather than an error: a document deleted out from under
    /// the viewer leaves the frame it already has, which is more useful than a
    /// blank window, and the file coming back is an ordinary change.
    pub fn poll(&mut self, dt: f64) -> bool {
        self.clock += Duration::from_secs_f64(dt);
        !self.inner.poll(self.clock).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crcbl::assets::watch::{POLL_INTERVAL, SETTLE};

    use super::*;

    /// One step big enough to cross the engine's interval, so a poll looks.
    fn one_interval() -> f64 {
        POLL_INTERVAL.as_secs_f64()
    }

    /// A file, and the directory that has to outlive it.
    fn file(contents: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("panel.glb");
        std::fs::write(&path, contents).expect("the file");
        (dir, path)
    }

    /// **A file nobody touched is never offered.** The first poll after opening
    /// must not rebuild the scene the viewer just built.
    #[test]
    fn an_untouched_file_is_never_offered() {
        let (_dir, path) = file(b"one");
        let mut watch = Watch::new(&path);
        for _ in 0..10 {
            assert!(!watch.poll(one_interval()), "an untouched file was offered");
        }
    }

    /// **A re-export is offered once, after it has settled, on the clock the
    /// viewer's `dt`s add up to** — the viewer's half of the engine's watch:
    /// that the seconds it is handed reach the watch as its clock.
    ///
    /// The length of the two documents differs, deliberately: on Windows two
    /// writes inside one timer tick share a modification time, and a length is
    /// the half of a stamp no clock granularity can flatten.
    #[test]
    fn a_re_export_is_offered_once_after_it_settles() {
        let (_dir, path) = file(b"one");
        let mut watch = Watch::new(&path);
        std::fs::write(&path, b"two words").expect("the re-export");

        // A tenth of the interval, twice: below one look, so the change has not
        // even been seen yet, let alone settled.
        assert!(!watch.poll(one_interval() / 10.0));
        assert!(!watch.poll(one_interval() / 10.0));
        // Crossing it is the first look, which only starts the settle.
        assert!(!watch.poll(one_interval()), "offered before it settled");
        assert!(
            watch.poll(SETTLE.as_secs_f64()),
            "the settled re-export was not offered"
        );
        for _ in 0..10 {
            assert!(
                !watch.poll(one_interval()),
                "the same re-export was offered twice"
            );
        }
    }
}
