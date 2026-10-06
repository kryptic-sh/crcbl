//! The directory an opened scene came from: its chunk files watched, and a
//! changed one reloaded into the world — only its own system's rows.
//!
//! The watch is the engine's ([`crcbl::assets::watch`]) and the reload is the
//! scene format's ([`crcbl::scene::scn::reload`]): a file is offered once it
//! has stopped changing, its rows are compared with the system's, and only
//! what differs is applied. So editing `sys/sun.ron` relights the cube and
//! leaves its spin — entity, id and the seconds it has turned through —
//! exactly where it was.
//!
//! # Watched in every run that names a directory (decided 2026-10-06)
//!
//! Not behind a flag of its own: naming a scene directory on the sandbox's
//! command line is the development loop, and the watch costs a `stat` per
//! chunk per [`crcbl::assets::watch::POLL_INTERVAL`] and changes nothing
//! until a file does — which is also why a headless run with `--scene` stays
//! a reproducible one. Only the chunk files are watched: a change to
//! `scene.ron` or `env.ron` waits for a restart, since the sandbox reads
//! neither past the load.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crcbl::assets::DirSource;
use crcbl::assets::watch::PolledWatch;
use crcbl::ecs::World;
use crcbl::scene::scn::{self, ChunkDiff, IdMap, ScnError, SystemChunk, chunk_of};

use super::{SPIN, SUN, Spin, Sun};

/// What became of one chunk file that changed on disk.
#[derive(Debug)]
pub struct Reload {
    /// The system whose chunk it is.
    pub system: String,
    /// What the reload changed, or why the file was refused — in which case
    /// the system holds what the last good read left.
    pub outcome: Result<ChunkDiff, ScnError>,
}

/// An opened scene's directory, what was read from it, and its watch.
#[derive(Debug)]
pub(super) struct Disk {
    dir: PathBuf,
    source: DirSource,
    scene: scn::Scene,
    ids: IdMap,
    watch: PolledWatch,
    /// Wall-clock time since the scene was opened: the sum of every `dt`
    /// [`Disk::poll`] was handed, and the clock the watch reads.
    clock: Duration,
}

/// The codecs of the sandbox's two systems.
pub(super) fn codecs() -> Vec<Box<dyn SystemChunk>> {
    vec![chunk_of::<Spin>(SPIN), chunk_of::<Sun>(SUN)]
}

impl Disk {
    /// Loads the scene at `dir` into `world`, whose systems are registered
    /// already, and starts watching its chunk files.
    pub(super) fn open(dir: &Path, world: &mut World) -> Result<Self, ScnError> {
        // Rooted at the scene directory and read with an empty prefix:
        // `DirSource` refuses an absolute key, so the root is how a caller
        // says where the scene is.
        let source = DirSource::at(dir.to_path_buf());
        let (scene, ids) = scn::Scene::load(&source, Path::new(""), &codecs(), world)?;
        let watch = PolledWatch::new(
            scene.systems().iter().map(|system| chunk_path(dir, system)),
            Duration::ZERO,
        );
        Ok(Self {
            dir: dir.to_path_buf(),
            source,
            scene,
            ids,
            watch,
            clock: Duration::ZERO,
        })
    }

    /// Advances the clock by `dt`, and reloads into `world` each chunk the
    /// watch offers.
    pub(super) fn poll(&mut self, world: &mut World, dt: Duration) -> Vec<Reload> {
        self.clock += dt;
        let mut reloads = Vec::new();
        for path in self.watch.poll(self.clock) {
            let Some(system) = self
                .scene
                .systems()
                .iter()
                .find(|system| chunk_path(&self.dir, system) == path)
                .cloned()
            else {
                continue;
            };
            let outcome = self.scene.reload_chunk(
                &self.source,
                Path::new(""),
                &system,
                &codecs(),
                world,
                &mut self.ids,
            );
            reloads.push(Reload { system, outcome });
        }
        reloads
    }
}

/// Where `system`'s chunk file is in the scene directory `dir`.
fn chunk_path(dir: &Path, system: &str) -> PathBuf {
    dir.join("sys").join(format!("{system}.ron"))
}

#[cfg(test)]
mod tests;
