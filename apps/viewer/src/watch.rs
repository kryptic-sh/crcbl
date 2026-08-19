//! Noticing that the document on disk has been written again.
//!
//! `docs/plan/sample/05-viewer.md` V-F4: re-export from Blender and the viewer
//! picks it up. This is the *noticing* half — [`crate::app`] owns what happens
//! next.
//!
//! # Polled, not subscribed
//!
//! There is no filesystem-notification dependency here. A watch would be the
//! better mechanism for a directory of thousands of files; this is one path, and
//! the whole of what a poll costs is a `stat` four times a second. Against that,
//! an API-level watch is a per-platform surface — `inotify`, `FSEvents`,
//! `ReadDirectoryChangesW` — and every one of them reports a re-export as a
//! *burst* of events that has to be debounced back into one anyway. The debounce
//! below is the part that actually matters, and a poll needs nothing else.
//!
//! # Settling, because an exporter's file is not written all at once
//!
//! Blender writes a `.glb` progressively, so a poll that landed mid-write would
//! read a truncated document and report a parse error for a file that is about
//! to be perfectly good. So a stamp has to be seen **twice running** — a quarter
//! of a second unchanged — before it is offered for loading.
//!
//! # What this cannot see
//!
//! Two things, and both are the same trade. A re-export that lands on the same
//! length *and* the same modification time is missed; a rewrite that lands the
//! same **bytes** with a new modification time is offered, and rebuilds a scene
//! that draws identically.
//!
//! **The window for the first is wider on Windows than the file system
//! suggests.** ext4 and APFS stamp to the nanosecond and NTFS stores 100 ns, but
//! the clock Windows stamps a write from advances on the system timer tick —
//! about 15.6 ms by default — so two writes inside one tick share a timestamp.
//! Not theoretical: a test here wrote a file three times in a row and CI caught
//! the second and third landing on the same stamp, which is why that test no
//! longer depends on one moving. An export takes orders of magnitude longer than
//! a tick, so the case that matters is unaffected.
//!
//! The alternative is hashing the file's contents on every poll, which is
//! megabytes of reading four times a second to catch a case nothing produces.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How long between two looks at the file, in seconds.
///
/// Also the settle time, since a stamp must survive one interval to be offered:
/// a quarter of a second is under the threshold a re-export feels instant at,
/// and long enough that a `stat` is not on any budget.
const POLL_SECONDS: f64 = 0.25;

/// What one look at the file saw.
///
/// The pair every polled watcher in the ecosystem compares, and no more: a
/// length alone misses an edit that keeps the size, and a modification time
/// alone misses a filesystem that does not move it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    /// `None` where the filesystem does not report one, which leaves `len` to
    /// carry the comparison on its own.
    modified: Option<SystemTime>,
    len: u64,
}

/// One path, and what the viewer has already loaded from it.
#[derive(Debug)]
pub struct Watch {
    path: PathBuf,
    /// The stamp the document on screen was offered from.
    ///
    /// Set when a reload is *offered*, not when it succeeds — a document that
    /// failed to parse would parse the same way again, so retrying the identical
    /// bytes is a warning a second time and nothing else. The next real change
    /// is what tries again.
    loaded: Option<Stamp>,
    /// A stamp seen once and waiting to be seen again unchanged.
    settling: Option<Stamp>,
    /// Seconds since the last look.
    since_poll: f64,
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
            loaded: stamp(path),
            settling: None,
            since_poll: 0.0,
        }
    }

    /// The path being watched.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Advances the timer by `dt` seconds and reports whether the file should be
    /// read again.
    ///
    /// True at most once per change, and never for a file that is still being
    /// written — see the [module docs](self). A path that does not exist reports
    /// false rather than an error: a document deleted out from under the viewer
    /// leaves the frame it already has, which is more useful than a blank
    /// window, and the file coming back is an ordinary change.
    pub fn poll(&mut self, dt: f64) -> bool {
        self.since_poll += dt;
        if self.since_poll < POLL_SECONDS {
            return false;
        }
        self.since_poll = 0.0;

        let Some(seen) = stamp(&self.path) else {
            // Nothing to settle on, and nothing to offer.
            self.settling = None;
            return false;
        };
        if Some(seen) == self.loaded {
            // Including the case where a write was reverted while settling.
            self.settling = None;
            return false;
        }
        if self.settling == Some(seen) {
            self.loaded = Some(seen);
            self.settling = None;
            return true;
        }
        self.settling = Some(seen);
        false
    }
}

/// What `path` looks like now, or `None` if it cannot be read at all.
fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enough polls to cross [`POLL_SECONDS`] once, at a plausible tick.
    const ONE_INTERVAL: f64 = POLL_SECONDS;

    /// A file, and the directory that has to outlive it.
    fn file(contents: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("panel.glb");
        std::fs::write(&path, contents).expect("the file");
        (dir, path)
    }

    /// Rewrites the file with a stamp the watch can tell from the last one.
    ///
    /// The length is what carries it: a modification time is what a filesystem
    /// chooses, and a test that leaned on one would be asserting about the host.
    fn rewrite(path: &Path, contents: &[u8]) {
        std::fs::write(path, contents).expect("the file");
    }

    /// **A file nobody touched is never offered.** The first poll after opening
    /// must not rebuild the scene the viewer just built.
    #[test]
    fn an_untouched_file_is_never_offered() {
        let (_dir, path) = file(b"one");
        let mut watch = Watch::new(&path);
        for _ in 0..10 {
            assert!(!watch.poll(ONE_INTERVAL), "an untouched file was offered");
        }
    }

    /// **A change is offered once, after it has settled — and once only.**
    #[test]
    fn a_settled_change_is_offered_exactly_once() {
        let (_dir, path) = file(b"one");
        let mut watch = Watch::new(&path);
        rewrite(&path, b"two words");

        assert!(
            !watch.poll(ONE_INTERVAL),
            "the first sighting was offered without settling",
        );
        assert!(
            watch.poll(ONE_INTERVAL),
            "the settled change was not offered"
        );
        for _ in 0..10 {
            assert!(
                !watch.poll(ONE_INTERVAL),
                "the same change was offered twice"
            );
        }
    }

    /// **A file still being written is never offered.** This is the case the
    /// settle exists for: an exporter's document grows across several polls, and
    /// a load in the middle of that is a parse error about a file that is fine.
    #[test]
    fn a_file_still_growing_is_never_offered() {
        let (_dir, path) = file(b"");
        let mut watch = Watch::new(&path);
        let mut contents = Vec::new();
        for _ in 0..8 {
            contents.extend_from_slice(b"more");
            rewrite(&path, &contents);
            assert!(
                !watch.poll(ONE_INTERVAL),
                "a document still being written was offered",
            );
        }
        // And the moment it stops, it is.
        assert!(
            watch.poll(ONE_INTERVAL),
            "the finished write was not offered"
        );
    }

    /// **The interval is honoured.** Polling faster than [`POLL_SECONDS`] does
    /// not `stat` the file more often — which is what stops a change being
    /// offered on the frame after it lands, before it has settled.
    #[test]
    fn nothing_is_offered_before_the_interval_has_passed() {
        let (_dir, path) = file(b"one");
        let mut watch = Watch::new(&path);
        rewrite(&path, b"two words");
        // A tenth of the interval, twice: below one look, so the change has not
        // even been seen yet, let alone settled.
        assert!(!watch.poll(POLL_SECONDS / 10.0));
        assert!(!watch.poll(POLL_SECONDS / 10.0));
        // Crossing it is the first look, which only starts the settle.
        assert!(!watch.poll(POLL_SECONDS));
        assert!(watch.poll(POLL_SECONDS));
    }

    /// **A document deleted out from under the viewer offers nothing**, and the
    /// file coming back is an ordinary change.
    #[test]
    fn a_missing_file_offers_nothing_and_its_return_is_a_change() {
        let (_dir, path) = file(b"one");
        let mut watch = Watch::new(&path);
        std::fs::remove_file(&path).expect("the file goes");
        for _ in 0..4 {
            assert!(!watch.poll(ONE_INTERVAL), "a missing file was offered");
        }
        rewrite(&path, b"two words");
        assert!(!watch.poll(ONE_INTERVAL));
        assert!(
            watch.poll(ONE_INTERVAL),
            "the file coming back was not offered"
        );
    }

    /// **Going back to bytes the viewer has already drawn is still a change**,
    /// and this records that rather than wishing otherwise.
    ///
    /// A [`Watch`] keeps one stamp and no history, so it cannot know that a
    /// document is the one it opened with. The cost is a rebuild of a scene
    /// that draws identically, which nobody can see; avoiding it means hashing
    /// the file's contents on every poll, which is the trade the [module
    /// docs](self) decline. Asserted so that a future content check is a
    /// deliberate change to this line and not a silent one.
    ///
    /// **The two documents differ in length, deliberately.** An earlier version
    /// of this test rewrote the same three bytes and leaned on the modification
    /// time to tell the writes apart; on Windows all three landed inside one
    /// 15.6 ms timer tick and shared a timestamp, so the watch correctly saw no
    /// change and the assertion — not the code — was wrong. A length is the
    /// half of a stamp no clock granularity can flatten.
    #[test]
    fn returning_to_a_document_already_drawn_is_still_a_change() {
        let (_dir, path) = file(b"one");
        let mut watch = Watch::new(&path);

        rewrite(&path, b"two words");
        assert!(!watch.poll(ONE_INTERVAL), "the first sighting was offered");
        assert!(
            watch.poll(ONE_INTERVAL),
            "the settled change was not offered"
        );

        // Back to the bytes the watch was built on, which it has no way to
        // recognise as such.
        rewrite(&path, b"one");
        assert!(
            !watch.poll(ONE_INTERVAL),
            "the revert was offered unsettled"
        );
        assert!(
            watch.poll(ONE_INTERVAL),
            "a document the watch cannot see through was not offered",
        );
    }
}
