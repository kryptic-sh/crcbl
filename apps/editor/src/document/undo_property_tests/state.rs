//! What the undo property test compares: a document's logical content, keyed
//! by the ids that survive an undo.

use std::collections::{BTreeMap, BTreeSet};

use crcbl::ecs::Entity;
use crcbl::phys::PhysicsSystem;
use crcbl::scene::scn::SceneEntityId;

use super::super::Document;

/// A document's logical content.
///
/// **Not [`crcbl::ecs::World::hash_state`] nor `crcbl_server`'s `hash_world`**:
/// both fold in each [`Entity`]'s bits, and an undone delete files a new
/// `Entity` under the old [`SceneEntityId`] — the same scene with a different
/// handle (decided 2026-09-30, `docs/plan/08-editor.md`). Everything here is
/// keyed by the id instead, and every float is printed through its shortest
/// round trip, so two states differing in any bit of any field differ here.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct State {
    /// [`Document::files`]: the manifest in its order, `names.ron`, the
    /// environment and every listed system's chunk — what a save writes,
    /// byte-identical for equal scenes.
    files: BTreeMap<String, String>,
    /// Every live entity the id map files, by id: what the files cannot show
    /// — a row in a system the manifest does not list, which the next save
    /// would drop — and the picking collider the edit rebuilt.
    held: BTreeMap<SceneEntityId, Held>,
    /// Live entities the id map files under no id: a delete that left its
    /// entity behind, or a refused spawn that did not take its back out.
    unfiled: usize,
}

/// One filed entity, as [`State::held`] keeps it.
#[derive(Clone, Debug, PartialEq)]
struct Held {
    /// Every registered system holding it — listed or not — and its row's
    /// text.
    rows: BTreeMap<String, String>,
    /// Its picking body's transform and its collider's world box, printed,
    /// or [`None`] for an entity that cannot be picked.
    collider: Option<String>,
}

impl State {
    /// `document`'s content now.
    pub(super) fn of(document: &mut Document) -> Self {
        let files = document.files().expect("every entity has an id");
        let entities: Vec<Entity> = document.world().entities().collect();
        let mut held = BTreeMap::new();
        let mut unfiled = 0;
        for entity in entities {
            let Some(id) = document.ids().id(entity) else {
                unfiled += 1;
                continue;
            };
            let mut rows = BTreeMap::new();
            let registry = document.registry().clone();
            for system in registry.systems_of(document.world_mut(), entity) {
                let row = registry
                    .codec(&system)
                    .expect("a registered system has a codec")
                    .row(document.world_mut(), entity)
                    .expect("a held component serialises")
                    .expect("the system holds it");
                rows.insert(system, row);
            }
            let collider = document
                .world_mut()
                .system_mut::<PhysicsSystem>()
                .and_then(|physics| {
                    let transform = physics.transform(entity)?;
                    let bounds = physics
                        .collider_of(entity)
                        .and_then(|collider| physics.world().aabb_of(collider));
                    Some(format!("{transform:?} {bounds:?}"))
                });
            held.insert(id, Held { rows, collider });
        }
        Self {
            files,
            held,
            unfiled,
        }
    }

    /// Whether the scene names any entity.
    pub(super) fn has_names(&self) -> bool {
        self.files.contains_key("names.ron")
    }

    /// Where `self` and `other` differ, each side printed — a failure's
    /// message, which a whole state would bury.
    pub(super) fn difference(&self, other: &Self) -> String {
        let mut out = Vec::new();
        let files: BTreeSet<&String> = self.files.keys().chain(other.files.keys()).collect();
        for key in files {
            let (now, expected) = (self.files.get(key), other.files.get(key));
            if now != expected {
                out.push(format!(
                    "file {key}:\n  now:      {now:?}\n  expected: {expected:?}"
                ));
            }
        }
        let ids: BTreeSet<&SceneEntityId> = self.held.keys().chain(other.held.keys()).collect();
        for id in ids {
            let (now, expected) = (self.held.get(id), other.held.get(id));
            if now != expected {
                out.push(format!(
                    "entity #{id}:\n  now:      {now:?}\n  expected: {expected:?}"
                ));
            }
        }
        if self.unfiled != other.unfiled {
            out.push(format!(
                "entities filed under no id: now {}, expected {}",
                self.unfiled, other.unfiled
            ));
        }
        out.join("\n")
    }
}
