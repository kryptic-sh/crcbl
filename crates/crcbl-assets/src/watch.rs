//! Noticing that files on disk have been written again: the engine's polled
//! watch, stage 6's task 5 watcher.
//!
//! [`PolledWatch`] holds a set of paths and what each looked like when it was
//! last offered. [`PolledWatch::poll`] looks at them at most once per
//! [`POLL_INTERVAL`] of the caller's clock and hands back the ones whose
//! modification time or length changed and then held still for [`SETTLE`].
//! What happens next — reading the file, reloading what came from it — is the
//! caller's: this module decodes nothing, like the rest of the crate.
//!
//! # Polled, not subscribed (decided 2026-10-06)
//!
//! There is no filesystem-notification dependency, and `notify` stays out
//! until a directory of thousands of files needs one, which is the owner's
//! call. A poll is a `stat` per path per interval, the same on every platform
//! the engine targets and deterministic under a test's clock; an API-level
//! watch is a per-platform surface — `inotify`, `FSEvents`,
//! `ReadDirectoryChangesW` — and every one of them reports a re-export as a
//! **burst** of events that has to be debounced back into one anyway. The
//! settle below is the part that matters, and a poll needs nothing else. The
//! viewer's document watch and `crcbl_ui`'s stylesheet reload showed a poll is
//! enough for the paths a person edits by hand.
//!
//! # Settling, because a file is not written all at once
//!
//! An exporter writes progressively, and a text editor may truncate before it
//! writes, so a look that lands mid-write sees a file that is about to be
//! perfectly good and would read as a parse error. A changed stamp is offered
//! only once it has stayed the same for [`SETTLE`] — and a write burst, whose
//! every look sees a new stamp, restarts that wait each time.
//!
//! # What a stamp cannot see
//!
//! A rewrite that lands on the same length **and** the same modification time
//! is missed, and one that lands the same bytes with a new time is offered.
//! The first window is wider on Windows than the file system suggests: the
//! clock it stamps a write from advances on the system timer tick, so two
//! writes inside one tick share a time. An edit takes orders of magnitude
//! longer than a tick, so the case that matters is unaffected; hashing every
//! file on every look is the alternative, and it buys only that corner.
//!
//! # Native only
//!
//! A browser has no filesystem to look at, so the module is absent on
//! `wasm32` rather than present and silent: a caller that wants a watch there
//! fails to compile, which is the loud way to say there is none.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The shortest time between two looks at the watched files, on the clock
/// [`PolledWatch::poll`] is given.
///
/// Under the threshold an edit feels instant at, and long enough that a `stat`
/// per path is on nobody's budget. The interval is also why the watch keeps a
/// clock at all: a caller polling once a frame would otherwise `stat` hundreds
/// of times a second for files a person saves about once a minute.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long a changed stamp must stay unchanged before it is offered.
///
/// Two [`POLL_INTERVAL`]s, so a change is offered by the third look that sees
/// it, half a second to three quarters after the write. Longer than one
/// interval so that a writer pausing between its chunks for less than that —
/// an exporter flushing, a text editor's truncate and then write — is still
/// one write, and short enough that a save still shows at once. (The viewer's
/// watch, before it moved here, offered at the second look.)
pub const SETTLE: Duration = Duration::from_millis(500);

/// What one look at a file saw: the pair every polled watcher compares, and
/// no more. A length alone misses an edit that keeps the size; a modification
/// time alone misses a filesystem that does not move it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    /// `None` where the filesystem reports none, which leaves `len` to carry
    /// the comparison on its own.
    modified: Option<SystemTime>,
    len: u64,
}

/// One watched path.
#[derive(Debug)]
struct Watched {
    path: PathBuf,
    /// The stamp last offered — or seen when the path was added.
    ///
    /// Set when a change is *offered*, not when the caller's read of it
    /// succeeds: bytes that failed to parse would fail the same way again, so
    /// the next real change is what tries again.
    offered: Option<Stamp>,
    /// A changed stamp, and when it was first seen, waiting to hold still for
    /// [`SETTLE`].
    settling: Option<(Stamp, Duration)>,
}

/// A set of paths, polled for changes — see the [module docs](self).
#[derive(Debug)]
pub struct PolledWatch {
    watched: Vec<Watched>,
    /// When the files were last looked at, on the caller's clock.
    looked: Duration,
}

impl PolledWatch {
    /// Watches every path of `paths`, treating what each holds now as already
    /// offered, with `now` as the time of that first look.
    ///
    /// Which is what the caller has just done: it read the files before it
    /// asked for a watch, so a watch that offered them again on its first poll
    /// would reload for nothing. A path that does not exist yet is watched all
    /// the same, and its arrival is an ordinary change.
    #[must_use]
    pub fn new(paths: impl IntoIterator<Item = PathBuf>, now: Duration) -> Self {
        Self {
            watched: paths
                .into_iter()
                .map(|path| Watched {
                    offered: stamp(&path),
                    settling: None,
                    path,
                })
                .collect(),
            looked: now,
        }
    }

    /// The watched paths, in the order they were given.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.watched.iter().map(|watched| watched.path.as_path())
    }

    /// Looks at the files if [`POLL_INTERVAL`] has passed since the last look,
    /// and hands back each path whose change has settled, in watch order.
    ///
    /// `now` is the caller's clock — any monotonic one, so long as every call
    /// reads the same clock [`new`](Self::new) was given. **Each change is
    /// handed back once**, and never while it is still being written: see the
    /// module docs. A path that has gone offers nothing — whatever was read
    /// from it is more useful than nothing — and its return is an ordinary
    /// change.
    pub fn poll(&mut self, now: Duration) -> Vec<PathBuf> {
        if now.saturating_sub(self.looked) < POLL_INTERVAL {
            return Vec::new();
        }
        self.looked = now;
        let mut settled = Vec::new();
        for watched in &mut self.watched {
            let Some(seen) = stamp(&watched.path) else {
                // Nothing to settle on, and nothing to offer.
                watched.settling = None;
                continue;
            };
            if Some(seen) == watched.offered {
                // Including a write reverted while it settled.
                watched.settling = None;
                continue;
            }
            match watched.settling {
                Some((settling, since)) if settling == seen => {
                    if now.saturating_sub(since) >= SETTLE {
                        watched.offered = Some(seen);
                        watched.settling = None;
                        settled.push(watched.path.clone());
                    }
                }
                // A new stamp, or one still moving: the wait starts again.
                _ => watched.settling = Some((seen, now)),
            }
        }
        settled
    }
}

/// What `path` looks like now, or `None` if it cannot be looked at at all.
fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    /// One write of a progressive export, sized so every chunk moves the
    /// length — the half of a stamp no clock granularity can flatten, for the
    /// reason the module docs give.
    const CHUNK: [u8; 4096] = [0; 4096];

    /// A file holding `contents`, and the directory that has to outlive it.
    fn file(contents: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("chunk.ron");
        std::fs::write(&path, contents).expect("the file");
        (dir, path)
    }

    /// A watch over `path` and a clock that steps one interval per look.
    struct Rig {
        watch: PolledWatch,
        now: Duration,
    }

    impl Rig {
        fn over(paths: &[&Path]) -> Self {
            Self {
                watch: PolledWatch::new(
                    paths.iter().map(|path| path.to_path_buf()),
                    Duration::ZERO,
                ),
                now: Duration::ZERO,
            }
        }

        /// One interval on, and a look.
        fn look(&mut self) -> Vec<PathBuf> {
            self.now += POLL_INTERVAL;
            self.watch.poll(self.now)
        }

        /// The looks a change needs after the one that first saw it: each
        /// but the last falls inside the settle period and offers nothing —
        /// asserted — and the last one's offer is handed back.
        fn settle(&mut self) -> Vec<PathBuf> {
            for _ in 1..looks_to_settle() {
                assert!(self.look().is_empty(), "offered inside the settle period");
            }
            self.look()
        }
    }

    /// How many looks, an interval apart, after the first sighting a change
    /// takes to settle.
    fn looks_to_settle() -> u128 {
        SETTLE.as_nanos().div_ceil(POLL_INTERVAL.as_nanos())
    }

    /// **A file nobody touched is never offered**, the first look included:
    /// the caller read it before it asked for the watch.
    #[test]
    fn an_untouched_file_is_never_offered() {
        let (_dir, path) = file(b"one");
        let mut rig = Rig::over(&[&path]);
        for _ in 0..10 {
            assert!(rig.look().is_empty(), "an untouched file was offered");
        }
    }

    /// **A change is offered once, after it has settled, and once only.**
    #[test]
    fn a_settled_change_is_offered_exactly_once() {
        let (_dir, path) = file(b"one");
        let mut rig = Rig::over(&[&path]);
        std::fs::write(&path, b"two words").expect("the edit");

        assert!(rig.look().is_empty(), "offered on first sight");
        assert_eq!(
            rig.settle(),
            std::slice::from_ref(&path),
            "the settled change"
        );
        for _ in 0..10 {
            assert!(rig.look().is_empty(), "the same change was offered twice");
        }
    }

    /// **A write burst offers nothing until it stops**: every look in the
    /// middle of it sees a new stamp, and each one starts the wait again.
    #[test]
    fn a_write_burst_offers_nothing_until_it_stops() {
        let (_dir, path) = file(b"");
        let mut rig = Rig::over(&[&path]);
        let mut writing = std::fs::File::create(&path).expect("the writer opens the file");
        for _ in 0..8 {
            writing.write_all(&CHUNK).expect("a chunk of the write");
            assert!(
                rig.look().is_empty(),
                "a file still being written was offered"
            );
        }
        // Closing moves neither the length nor the time, so what makes the
        // file offerable is the settle period passing unchanged, and nothing
        // else.
        drop(writing);
        assert_eq!(
            rig.settle(),
            std::slice::from_ref(&path),
            "the finished write"
        );
        assert!(rig.look().is_empty());
    }

    /// **The settle period is a time, not a count of looks**: a look that
    /// sees the stamp unchanged before [`SETTLE`] has passed since the first
    /// sight offers nothing, and the first look at or past it offers.
    #[test]
    fn a_change_is_not_offered_before_the_settle_period_has_passed() {
        let (_dir, path) = file(b"one");
        let mut watch = PolledWatch::new([path.clone()], Duration::ZERO);
        std::fs::write(&path, b"two words").expect("the edit");

        // The first look sees the change, and starts its wait.
        assert!(watch.poll(POLL_INTERVAL).is_empty());
        // The next look, just short of the settle period after it: the stamp
        // has held still, and not for long enough.
        let short = POLL_INTERVAL + SETTLE - Duration::from_millis(1);
        assert!(
            short - POLL_INTERVAL >= POLL_INTERVAL,
            "that poll is no look"
        );
        assert!(watch.poll(short).is_empty(), "offered before it settled");
        // At it, an interval later: offered.
        assert_eq!(watch.poll(short + POLL_INTERVAL), [path]);
    }

    /// **Polling faster than the interval does not look more often**, which
    /// is what keeps a once-a-frame caller from `stat`ing every frame.
    #[test]
    fn nothing_is_looked_at_before_the_interval_has_passed() {
        let (_dir, path) = file(b"one");
        let mut watch = PolledWatch::new([path.clone()], Duration::ZERO);
        std::fs::write(&path, b"two words").expect("the edit");
        for tenth in 1..10 {
            assert!(watch.poll(POLL_INTERVAL / 10 * tenth).is_empty());
        }
        assert!(watch.poll(POLL_INTERVAL).is_empty(), "the first look");
        // A second write, then many polls inside the next interval: none of
        // them looks, so the next look is the first to see the new stamp,
        // and the wait starts again from there.
        std::fs::write(&path, b"two words, longer").expect("the second write");
        for tenth in 1..10 {
            assert!(
                watch
                    .poll(POLL_INTERVAL + POLL_INTERVAL / 10 * tenth)
                    .is_empty()
            );
        }
        let second_sight = POLL_INTERVAL * 2;
        assert!(watch.poll(second_sight).is_empty());
        assert!(
            watch.poll(POLL_INTERVAL + SETTLE).is_empty(),
            "settled from the first sight, not the second"
        );
        // A poll a fifth of an interval later is no look either — a look
        // there would find the second write settled, had any of the polls
        // between the first two looks seen it.
        assert!(
            watch
                .poll(POLL_INTERVAL + SETTLE + POLL_INTERVAL / 5)
                .is_empty(),
            "looked inside an interval"
        );
        assert_eq!(watch.poll(second_sight + SETTLE), [path]);
    }

    /// **Of several paths, only the one written is offered.**
    #[test]
    fn only_the_path_that_changed_is_offered() {
        let (dir, first) = file(b"one");
        let second = dir.path().join("other.ron");
        std::fs::write(&second, b"other").expect("the other file");
        let mut rig = Rig::over(&[&first, &second]);
        assert_eq!(rig.watch.paths().collect::<Vec<_>>(), [&first, &second]);

        std::fs::write(&second, b"other, edited").expect("the edit");
        assert!(rig.look().is_empty());
        assert_eq!(rig.settle(), [second]);
    }

    /// **A file written beside the path and renamed onto it is offered** —
    /// the pattern an exporter or an editor that will not leave a truncated
    /// file uses. `stat` follows the path, so the rename swaps a new file
    /// under it and the next look sees that file's stamp.
    #[test]
    fn a_temporary_file_renamed_onto_the_path_is_offered() {
        let (dir, path) = file(b"one");
        let mut rig = Rig::over(&[&path]);
        let scratch = dir.path().join("chunk.ron.tmp");
        let mut writing = std::fs::File::create(&scratch).expect("the temporary file");
        for _ in 0..4 {
            writing.write_all(&CHUNK).expect("a chunk");
            assert!(
                rig.look().is_empty(),
                "offered while the write was beside it"
            );
        }
        drop(writing);
        std::fs::rename(&scratch, &path).expect("the rename into place");
        assert!(rig.look().is_empty(), "the rename was offered unsettled");
        assert_eq!(rig.settle(), [path]);
    }

    /// **A file deleted offers nothing, and its return is a change.**
    #[test]
    fn a_missing_file_offers_nothing_and_its_return_is_a_change() {
        let (_dir, path) = file(b"one");
        let mut rig = Rig::over(&[&path]);
        std::fs::remove_file(&path).expect("the file goes");
        for _ in 0..4 {
            assert!(rig.look().is_empty(), "a missing file was offered");
        }
        std::fs::write(&path, b"two words").expect("the file comes back");
        assert!(rig.look().is_empty());
        assert_eq!(rig.settle(), [path]);
    }

    /// **Going back to bytes already offered is still a change**, and this
    /// records that rather than wishing otherwise: the watch keeps one stamp
    /// and no history, so it cannot know a file is one it saw before. The cost
    /// is a reload that changes nothing; avoiding it means hashing contents on
    /// every look, the trade the module docs decline. Asserted so that a
    /// future content check is a deliberate change to this line.
    ///
    /// The two contents differ in length so the stamps differ on every host:
    /// a modification time inside one Windows timer tick would not.
    #[test]
    fn returning_to_bytes_already_offered_is_still_a_change() {
        let (_dir, path) = file(b"one");
        let mut rig = Rig::over(&[&path]);
        std::fs::write(&path, b"two words").expect("the edit");
        assert!(rig.look().is_empty());
        assert_eq!(rig.settle(), std::slice::from_ref(&path));

        std::fs::write(&path, b"one").expect("back to the first bytes");
        assert!(rig.look().is_empty(), "the return was offered unsettled");
        assert_eq!(
            rig.settle(),
            [path],
            "a return the watch cannot see through"
        );
    }

    /// **A write reverted while it settles is no change.** The watch keeps
    /// the stamp it last offered, so a burst that ends where it began offers
    /// nothing.
    #[test]
    fn a_write_reverted_before_it_settles_is_never_offered() {
        let (_dir, path) = file(b"one");
        let mut rig = Rig::over(&[&path]);
        std::fs::write(&path, b"two words").expect("the edit");
        assert!(rig.look().is_empty());
        // Back to the very file the watch was built on, stamp and all — set
        // explicitly, since a rewrite's time is the host's to choose.
        let first = rig.watch.watched[0].offered.expect("stamped at the start");
        std::fs::write(&path, b"one").expect("the revert");
        let file = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("the file");
        if let Some(modified) = first.modified {
            file.set_modified(modified).expect("the time put back");
        }
        drop(file);
        for _ in 0..4 {
            assert!(rig.look().is_empty(), "a reverted write was offered");
        }
    }
}
