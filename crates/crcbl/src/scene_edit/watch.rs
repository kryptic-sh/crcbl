//! A document's chunk files watched on disk: what the editor and
//! `crcbl edit --serve` poll to learn which chunks to reload
//! ([`Document::reload_chunk`]).
//!
//! The engine's watch ([`crate::assets::watch`]) does the looking and the
//! settling; this keeps it pointed at the document's own directory and
//! manifest, and holds each chunk that changed until its caller can reload
//! it — after a drag, once play stops, once a question is answered.
//!
//! # It follows the document (decided 2026-10-06)
//!
//! * **The watched files are the document's**, read at every poll: a scene
//!   opened in its place, a save-as moving it, a system listed or unlisted,
//!   or a joined copy with no directory each change what is watched. A watch
//!   started afresh takes the files as they stand as already seen, as
//!   [`PolledWatch::new`] does — so a chunk changed on disk in the moment the
//!   manifest changes is seen only at its next change.
//! * **A changed chunk stays due until it is taken**, and a document moved to
//!   another directory drops what was due in the old one; a system the
//!   manifest no longer lists drops out too.
//! * **Only the chunks.** The header, `names.ron` and `env.ron` are not
//!   watched: reloading them in place is the header reload the backlog
//!   defers.
//!
//! # Native only
//!
//! As the engine's watch: a browser has no filesystem to look at, so this is
//! absent on `wasm32` rather than present and silent.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::assets::watch::PolledWatch;

use super::Document;

/// A document's chunk files, polled for changes — see the `scene_edit::watch`
/// module docs.
#[derive(Debug)]
pub struct ChunkWatch {
    /// The directory watched, or [`None`] for a document with none.
    dir: Option<PathBuf>,
    /// The manifest the watch was started for, in its order.
    systems: Vec<String>,
    watch: PolledWatch,
    /// Systems whose chunk changed and settled, not yet taken, in the order
    /// they settled.
    due: Vec<String>,
}

impl ChunkWatch {
    /// Watches `document`'s chunk files as they stand, with `now` as the time
    /// of that first look on the caller's clock — see
    /// [`PolledWatch::new`].
    #[must_use]
    pub fn new(document: &Document, now: Duration) -> Self {
        let dir = document.origin().map(Path::to_path_buf);
        let systems = document.manifest().to_vec();
        let watch = PolledWatch::new(chunk_paths(dir.as_deref(), &systems), now);
        Self {
            dir,
            systems,
            watch,
            due: Vec::new(),
        }
    }

    /// Looks at the chunk files of `document` — once per
    /// [`POLL_INTERVAL`](crate::assets::watch::POLL_INTERVAL) of the clock
    /// `now` reads, the one [`new`](Self::new) was given — and hands back the
    /// systems whose chunk changed and settled in this look, which are due
    /// from now until [`take_due`](Self::take_due).
    ///
    /// A document whose directory or manifest is not the one watched is
    /// watched afresh first, and that poll offers nothing — see the module
    /// docs.
    pub fn poll(&mut self, document: &Document, now: Duration) -> Vec<String> {
        let moved = document.origin() != self.dir.as_deref();
        if moved || document.manifest() != self.systems {
            let due = std::mem::take(&mut self.due);
            *self = Self::new(document, now);
            if !moved {
                self.due = due
                    .into_iter()
                    .filter(|system| self.systems.contains(system))
                    .collect();
            }
            return Vec::new();
        }
        let mut settled = Vec::new();
        for path in self.watch.poll(now) {
            let Some(system) = self
                .systems
                .iter()
                .find(|system| chunk_path(self.dir.as_deref(), system).as_ref() == Some(&path))
            else {
                continue;
            };
            if !self.due.contains(system) {
                self.due.push(system.clone());
            }
            settled.push(system.clone());
        }
        settled
    }

    /// Hands back every system whose chunk changed and has not been taken,
    /// in the order they settled, and forgets them.
    pub fn take_due(&mut self) -> Vec<String> {
        std::mem::take(&mut self.due)
    }
}

/// Where each of `systems`' chunk files is in the scene directory `dir` —
/// none without one.
fn chunk_paths(dir: Option<&Path>, systems: &[String]) -> Vec<PathBuf> {
    systems
        .iter()
        .filter_map(|system| chunk_path(dir, system))
        .collect()
}

/// Where `system`'s chunk file is in the scene directory `dir`, spelled as
/// [`crate::scene::scn::chunk_text`] reads it.
fn chunk_path(dir: Option<&Path>, system: &str) -> Option<PathBuf> {
    Some(dir?.join("sys").join(format!("{system}.ron")))
}
