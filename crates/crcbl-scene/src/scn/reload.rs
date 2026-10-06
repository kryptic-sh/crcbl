//! Reloading one chunk file: `sys/<name>.ron` read again, compared with the
//! rows its system holds, and only the difference applied.
//!
//! Stage 6's per-chunk reload, the scene half of task 5: **a changed chunk
//! reloads only its own system**, so editing `sys/sun.ron` leaves every other
//! system's entities, their ids and their state exactly as they were.
//!
//! # Row by row, not system by system (decided 2026-10-06)
//!
//! The design asked for a changed chunk to tear down and re-instantiate its
//! system's entities. An entity may hold rows in several systems — the
//! [module docs](super) say why — so tearing down an entity of this system
//! would take its other systems' rows with it. The reload works on rows
//! instead, and [`diff_chunk`] says which: a row the file spells that the
//! system does not hold is [added](RowChange::Added), one whose text differs
//! is [changed](RowChange::Changed), and one the system holds that the file no
//! longer spells is [removed](RowChange::Removed). A row whose text is the
//! same is not touched at all, so its entity keeps whatever runtime state it
//! has in other systems and its id. An entity left in no system by a removal
//! goes with its id and its name, as a delete takes them.
//!
//! **Ids are the file's.** A row the file keeps under the same id stays filed
//! under it ([`IdMap::restore`] for a new one), so a reload that keeps an id
//! keeps the entity it names wherever it already exists.
//!
//! # A chunk that will not read keeps the last good state
//!
//! The whole file is read and checked before anything changes: a parse error
//! (a refused row included), another system's chunk or an id spelled twice
//! is the refusal [`Scene::reload_chunk`] returns, and the world is left as
//! the last good read made it. A write caught half-finished is usually that
//! refusal, and the watch offers the finished file a poll later.
//!
//! # Two appliers of one difference
//!
//! [`Scene::reload_chunk`] applies a [`ChunkDiff`] straight to a world — what
//! a game that loaded a scene into its own world, like `apps/sandbox`, wants.
//! An editor's document applies the same difference as edit commands instead,
//! so the reload is an entry of its history and every client of an edit server
//! hears of it (`crcbl::scene_edit`'s reload); this crate cannot apply those,
//! so it computes the difference once and each side applies it its own way.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crcbl_assets::{AssetSource, StorageError};
use crcbl_ecs::World;

use super::{IdMap, Scene, SceneEntityId, ScnError, SystemChunk, codec_named, join_key, read_text};

/// One row a reload changes — see the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowChange {
    /// The file spells `id`, and the system does not hold it.
    Added {
        /// The id the file files the row under.
        id: SceneEntityId,
        /// The row, as [`SystemChunk::row`] spells one.
        row: String,
    },
    /// The system holds `id` with a row other than the one the file spells.
    Changed {
        /// Whose.
        id: SceneEntityId,
        /// The row the file spells now.
        row: String,
    },
    /// The system holds `id`, and the file no longer spells it.
    Removed {
        /// Whose.
        id: SceneEntityId,
    },
}

impl RowChange {
    /// The id the change is about.
    #[must_use]
    pub const fn id(&self) -> SceneEntityId {
        match self {
            Self::Added { id, .. } | Self::Changed { id, .. } | Self::Removed { id } => *id,
        }
    }
}

/// What a chunk file holds that its system does not: every [`RowChange`], in
/// id order. Empty for a file that says what the system already holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkDiff {
    system: String,
    changes: Vec<RowChange>,
}

impl ChunkDiff {
    /// The system whose chunk this is.
    #[must_use]
    pub fn system(&self) -> &str {
        &self.system
    }

    /// Every row the file changes, in id order.
    #[must_use]
    pub fn changes(&self) -> &[RowChange] {
        &self.changes
    }

    /// Whether the file changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// What `text`, the chunk file of `codec`'s system read from `key`, holds that
/// the system in `world` does not — see the [module docs](self). Nothing is
/// changed.
///
/// The system's rows are the ones whose entity `ids` files: an entity spawned
/// into the system with no scene id is no row of the file's, and is left
/// alone.
///
/// # Errors
///
/// As [`SystemChunk::rows`] for the text, and [`ScnError::NoSystem`] if
/// `world` holds no such system.
pub fn diff_chunk(
    codec: &dyn SystemChunk,
    world: &mut World,
    ids: &IdMap,
    key: &str,
    text: &str,
) -> Result<ChunkDiff, ScnError> {
    let mut wanted: BTreeMap<SceneEntityId, String> = codec.rows(key, text)?.into_iter().collect();
    let mut changes = Vec::new();
    for (id, entity) in ids.iter() {
        let held = codec.row(world, entity)?;
        match (held, wanted.remove(&id)) {
            (Some(held), Some(row)) if held != row => {
                changes.push(RowChange::Changed { id, row });
            }
            (Some(_), None) => changes.push(RowChange::Removed { id }),
            (None, Some(row)) => changes.push(RowChange::Added { id, row }),
            (Some(_), Some(_)) | (None, None) => {}
        }
    }
    // What is left names no entity the scene holds.
    changes.extend(
        wanted
            .into_iter()
            .map(|(id, row)| RowChange::Added { id, row }),
    );
    changes.sort_by_key(RowChange::id);
    Ok(ChunkDiff {
        system: codec.name().to_owned(),
        changes,
    })
}

/// The key of `system`'s chunk file in the scene directory `dir`, and its
/// text read through `source` — what a reload reads, spelled as
/// [`Scene::load`] spells it.
///
/// # Errors
///
/// [`ScnError::Read`] for a `dir` that is not text, a key that will not read,
/// or bytes that are not UTF-8.
pub fn chunk_text(
    source: &dyn AssetSource,
    dir: &Path,
    system: &str,
) -> Result<(String, String), ScnError> {
    let prefix = dir.to_str().ok_or_else(|| ScnError::Read {
        key: dir.display().to_string(),
        source: StorageError::InvalidPath(dir.to_path_buf()),
    })?;
    let key = join_key(prefix, &format!("sys/{system}.ron"));
    let text = read_text(source, &key)?;
    Ok((key, text))
}

impl Scene {
    /// Reads `dir/sys/<system>.ron` through `source` again and applies what it
    /// changes to `world` and `ids`, handing back what that was — see the
    /// [module docs](self). Only `system`'s rows are touched; an entity a
    /// removal leaves in none of the manifest's systems is despawned, and its
    /// id and its name go with it.
    ///
    /// `chunks` are the codecs the scene was loaded with: `system`'s reads the
    /// file, and the rest say whether a removed row's entity is still in
    /// another system.
    ///
    /// # Errors
    ///
    /// [`ScnError::Unlisted`] for a system the manifest does not list, and
    /// otherwise as [`Scene::load`] for one chunk: a key that will not read,
    /// text that is not the system's chunk, or a manifest system with no codec
    /// or no system in `world`. **Nothing changes when it refuses**, so the
    /// world keeps the last good read.
    pub fn reload_chunk(
        &mut self,
        source: &dyn AssetSource,
        dir: &Path,
        system: &str,
        chunks: &[Box<dyn SystemChunk>],
        world: &mut World,
        ids: &mut IdMap,
    ) -> Result<ChunkDiff, ScnError> {
        if !self.systems.iter().any(|listed| listed == system) {
            return Err(ScnError::Unlisted {
                system: system.to_owned(),
            });
        }
        let codec = codec_named(chunks, system)?;
        // Every other manifest system's codec, found before anything changes,
        // so a missing one refuses with the world untouched.
        let others = self
            .systems
            .iter()
            .filter(|listed| *listed != system)
            .map(|listed| codec_named(chunks, listed))
            .collect::<Result<Vec<_>, _>>()?;
        let (key, text) = chunk_text(source, dir, system)?;
        let diff = diff_chunk(codec, world, ids, &key, &text)?;

        // Which removed rows leave their entity in no system, asked before
        // anything changes: the question reads every other system, and a
        // refusal half way through would leave half a reload.
        let mut gone = BTreeSet::new();
        for change in &diff.changes {
            if let RowChange::Removed { id } = change {
                let entity = ids.entity(*id).expect("a removed row's id is filed");
                let mut elsewhere = false;
                for other in &others {
                    elsewhere |= other.row(world, entity)?.is_some();
                }
                if !elsewhere {
                    gone.insert(*id);
                }
            }
        }

        // Every row was read and checked by `diff_chunk`, and a system missing
        // from the world refuses the first change before it changes anything,
        // so what is left cannot refuse part way: a refused attach takes back
        // the entity it spawned, which is the only change made before it.
        for change in &diff.changes {
            match change {
                RowChange::Added { id, row } => {
                    let held = ids.entity(*id);
                    let entity = held.unwrap_or_else(|| {
                        let entity = world.spawn();
                        assert!(
                            ids.restore(*id, entity),
                            "a fresh entity under an id the map does not hold is filed",
                        );
                        entity
                    });
                    if let Err(error) = codec.attach_row(world, entity, row) {
                        if held.is_none() {
                            ids.remove(*id);
                            world.despawn(entity);
                            world.sweep();
                        }
                        return Err(error);
                    }
                }
                // `System::attach` replaces a row the entity already has, in
                // place.
                RowChange::Changed { id, row } => {
                    let entity = ids.entity(*id).expect("a changed row's id is filed");
                    codec.attach_row(world, entity, row)?;
                }
                RowChange::Removed { id } => {
                    let entity = ids.entity(*id).expect("a removed row's id is filed");
                    codec.detach_row(world, entity)?;
                    if gone.contains(id) {
                        world.despawn(entity);
                        ids.remove(*id);
                        self.entity_names.remove(id);
                    }
                }
            }
        }
        if !gone.is_empty() {
            // Now rather than at the end of a tick: a despawned entity stays in
            // every system until a sweep, and the caller may save or draw
            // before it next ticks.
            world.sweep();
        }
        Ok(diff)
    }
}

#[cfg(test)]
mod tests;
