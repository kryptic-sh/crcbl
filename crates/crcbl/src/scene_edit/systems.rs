//! Which systems an entity is in: read, and changed through
//! [`EditCommand::Attach`] and [`EditCommand::Detach`].
//!
//! A scene may hold one entity in several systems (`crate::scene::scn`'s
//! _One entity, several systems_), each holding one component of it. An
//! attach gives the entity a component in one more of the scene's systems and
//! a detach takes one away, each the other's inverse through the same log as
//! every other edit, and refused in play mode by the same check.
//!
//! # Every registered system, listing it when it must
//!
//! [`attachable`](Document::attachable) offers every system the vocabulary
//! registers that does not hold the entity, in the groups
//! [`attachable_groups`](Document::attachable_groups) heads them with: the
//! manifest's first, in its order, then the rest by the game that registered
//! them ([`Registry::group`](crate::registry::Registry::group)) — so towers'
//! `waypoints` is offered on a breakout scene under "towers", not unremarked
//! beside the scene's own. A save writes the manifest's chunks and
//! no others, so a component attached in a system the manifest does not list
//! would be dropped by the next one — attaching there is one
//! [`EditCommand::Batch`] of the [`EditCommand::ListSystem`] that adds it at
//! the manifest's end and the [`EditCommand::Attach`], as a drop from the
//! asset browser lists `meshes` ([`Document::spawn_mesh`]), so one undo takes
//! the row and the manifest entry back and the files are what they were.
//!
//! **Detaching the last entity of a system leaves it listed.** A manifest
//! entry with no rows is harmless — its chunk is saved empty and loads as
//! nothing — and taking it out is [`EditCommand::UnlistSystem`], an act of its
//! own. Unlisting as a side effect would make a detach's inverse sometimes a
//! batch and sometimes not, and a system a person listed by attaching, then
//! emptied to rearrange, would vanish from the outline under them.
//!
//! # A new component's value
//!
//! [`attach`](Document::attach) starts the component at its type's `Default`,
//! which [`crate::registry::Registry::register`] requires of every registered
//! component — see that method for why the game chooses it rather than this
//! tool.
//!
//! # A new entity in one system
//!
//! [`add_entity`](Document::add_entity) puts a new entity in the scene
//! holding one component, at its type's `Default`, in any registered system —
//! what the inspector offers while nothing is selected, under the headings
//! [`addable_groups`](Document::addable_groups) gives. Before it, the only
//! entity a scene from empty could gain was a mesh dropped from the asset
//! browser, so a scene of a game's own components — towers' path and plots —
//! had to start as a mesh, take its components and lose the mesh, and the
//! emptied `meshes` system stayed in the manifest, with no button to take it
//! out. It is one [`EditCommand::Spawn`], batched after the listing as a drop
//! is, so one undo takes the entity and the manifest entry back.

use std::collections::BTreeMap;

use crate::scene::scn::{SceneEntityId, SystemChunk};

use super::{Document, EditError, sync_colliders};
use crate::scene::edit::{EditCommand, SystemRow};

/// The heading of the systems the scene's manifest lists, first in the add
/// list.
pub const IN_SCENE: &str = "In this scene";

/// The heading of systems registered outside any
/// [`Registry::group`](crate::registry::Registry::group), last in the add
/// list.
pub const UNGROUPED: &str = "Other";

/// One heading of the add list, and the systems under it in the order they
/// are offered — see [`Document::attachable_groups`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemGroup {
    /// [`IN_SCENE`], a game's group label, or [`UNGROUPED`].
    pub label: String,
    /// The systems under it, never empty.
    pub systems: Vec<String>,
}

impl Document {
    /// The systems holding `id`, in the order the scene's manifest lists them —
    /// the order the inspector draws their sections in. Empty for an id this
    /// document does not hold.
    #[must_use]
    pub fn systems_of(&mut self, id: SceneEntityId) -> Vec<String> {
        let Some(entity) = self.ids.entity(id) else {
            return Vec::new();
        };
        let held = self.registry.systems_of(&mut self.world, entity);
        self.scene
            .systems()
            .iter()
            .filter(|system| held.contains(system))
            .cloned()
            .collect()
    }

    /// The system whose component places `id` — what a gizmo or an arrow key
    /// moves, so that what moves is what the picture and the pick read.
    /// [`crate::registry::Registry::placing_system`] says which one that is.
    ///
    /// [`None`] for an id this document does not hold, and for an entity none
    /// of whose components is a thing in space.
    #[must_use]
    pub fn placing_system(&mut self, id: SceneEntityId) -> Option<String> {
        let entity = self.ids.entity(id)?;
        self.registry.placing_system(&mut self.world, entity)
    }

    /// The systems `id` could be given a component in: every registered one
    /// less those already holding it, in the order
    /// [`attachable_groups`](Self::attachable_groups) offers them. See the
    /// module docs for what attaching to one the manifest does not list does.
    #[must_use]
    pub fn attachable(&mut self, id: SceneEntityId) -> Vec<String> {
        self.attachable_groups(id)
            .into_iter()
            .flat_map(|group| group.systems)
            .collect()
    }

    /// [`attachable`](Self::attachable), under the headings the inspector's
    /// add list draws: [`IN_SCENE`] for the manifest's systems, in its order;
    /// then each game's group by its label, its systems in name order; then
    /// [`UNGROUPED`]. A heading with nothing left to offer is left out.
    #[must_use]
    pub fn attachable_groups(&mut self, id: SceneEntityId) -> Vec<SystemGroup> {
        if self.ids.entity(id).is_none() {
            return Vec::new();
        }
        let held = self.systems_of(id);
        self.groups(|system| !held.iter().any(|each| each == system))
    }

    /// Every registered system, under the headings
    /// [`attachable_groups`](Self::attachable_groups) draws — the systems
    /// [`add_entity`](Self::add_entity) can put a new entity in, which the
    /// inspector offers while nothing is selected.
    #[must_use]
    pub fn addable_groups(&self) -> Vec<SystemGroup> {
        self.groups(|_| true)
    }

    /// The registered systems `offered` keeps, under the headings
    /// [`attachable_groups`](Self::attachable_groups) describes.
    fn groups(&self, offered: impl Fn(&str) -> bool) -> Vec<SystemGroup> {
        let listed = self.scene.systems();
        let mut groups = vec![SystemGroup {
            label: IN_SCENE.to_owned(),
            systems: listed
                .iter()
                .filter(|system| offered(system))
                .cloned()
                .collect(),
        }];
        // `None` sorts first in a map, so the ungrouped are taken out and put
        // last by hand.
        let mut games: BTreeMap<Option<&str>, Vec<String>> = BTreeMap::new();
        for system in self.registry.systems() {
            if offered(system) && !listed.iter().any(|each| each == system) {
                games
                    .entry(self.registry.group_of(system))
                    .or_default()
                    .push(system.to_owned());
            }
        }
        let ungrouped = games.remove(&None);
        groups.extend(games.into_iter().map(|(label, systems)| SystemGroup {
            label: label.unwrap_or(UNGROUPED).to_owned(),
            systems,
        }));
        groups.extend(ungrouped.map(|systems| SystemGroup {
            label: UNGROUPED.to_owned(),
            systems,
        }));
        groups.retain(|group| !group.systems.is_empty());
        groups
    }

    /// Gives `id` a component in `system`, at the component type's `Default`,
    /// as one [`EditCommand::Attach`] — batched after the
    /// [`EditCommand::ListSystem`] that adds `system` to the manifest when it
    /// is not there yet, so one undo takes both back.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::NoSystem`] for a
    /// system the vocabulary does not register, [`EditError::NoEntity`] for an
    /// id this document does not hold, [`EditError::Attached`] for a system
    /// that already holds it, and [`EditError::Scene`] if the default would not
    /// serialise. Nothing is attached, listed or recorded when it refuses.
    pub fn attach(&mut self, id: SceneEntityId, system: &str) -> Result<(), EditError> {
        self.refuse_in_play()?;
        if !self.registry.contains(system) {
            return Err(EditError::NoSystem(system.to_owned()));
        }
        let row = self.registry.default_row(system)?;
        let attach = EditCommand::Attach {
            entity: id,
            system: system.to_owned(),
            row,
        };
        self.apply(self.listing_first(system, attach))
    }

    /// Puts a new entity in the scene holding one component, in `system`, at
    /// the component type's `Default`, and hands back its id — the next one
    /// the document hands out. One [`EditCommand::Spawn`], batched after the
    /// [`EditCommand::ListSystem`] that adds `system` to the manifest when it
    /// is not there yet, so one undo takes both back. See the module docs.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::NoSystem`] for a
    /// system the vocabulary does not register, and [`EditError::Scene`] if
    /// the default would not serialise. Nothing is spawned, listed or
    /// recorded when it refuses.
    pub fn add_entity(&mut self, system: &str) -> Result<SceneEntityId, EditError> {
        self.refuse_in_play()?;
        if !self.registry.contains(system) {
            return Err(EditError::NoSystem(system.to_owned()));
        }
        let row = self.registry.default_row(system)?;
        let id = self.ids.next_id();
        let spawn = EditCommand::Spawn {
            entity: id,
            rows: vec![SystemRow {
                system: system.to_owned(),
                row,
            }],
            name: None,
        };
        self.apply(self.listing_first(system, spawn))?;
        Ok(id)
    }

    /// Takes `id`'s component out of `system`, as one [`EditCommand::Detach`]
    /// whose undo attaches the same row back.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::NoEntity`] for an id
    /// this document does not hold, [`EditError::NoSystem`] for a system the
    /// manifest does not list, [`EditError::NotAttached`] for one that does not
    /// hold it, and [`EditError::NoComponent`] for its last system —
    /// [`delete`](Self::delete) is how an entity goes. Nothing is detached or
    /// recorded when it refuses.
    pub fn detach(&mut self, id: SceneEntityId, system: &str) -> Result<(), EditError> {
        self.apply(EditCommand::Detach {
            entity: id,
            system: system.to_owned(),
        })
    }

    /// Attaches `row` to `id` in `system`, and hands back the detach that
    /// undoes it — the body of [`EditCommand::Attach`].
    pub(super) fn attach_row(
        &mut self,
        id: SceneEntityId,
        system: &str,
        row: &str,
    ) -> Result<EditCommand, EditError> {
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        let codec = self.listed_codec(system)?;
        if codec.row(&mut self.world, entity)?.is_some() {
            return Err(EditError::Attached {
                entity: id,
                system: system.to_owned(),
            });
        }
        codec.attach_row(&mut self.world, entity, row)?;
        self.joined(entity);
        Ok(EditCommand::Detach {
            entity: id,
            system: system.to_owned(),
        })
    }

    /// Takes `id`'s component out of `system`, and hands back the attach of the
    /// row it removed — the body of [`EditCommand::Detach`].
    pub(super) fn detach_row(
        &mut self,
        id: SceneEntityId,
        system: &str,
    ) -> Result<EditCommand, EditError> {
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        let codec = self.listed_codec(system)?;
        let held = self.systems_of(id);
        if !held.iter().any(|each| each == system) {
            return Err(EditError::NotAttached {
                entity: id,
                system: system.to_owned(),
            });
        }
        if held.len() == 1 {
            return Err(EditError::NoComponent(id));
        }
        let row =
            codec
                .detach_row(&mut self.world, entity)?
                .ok_or_else(|| EditError::NotAttached {
                    entity: id,
                    system: system.to_owned(),
                })?;
        self.joined(entity);
        Ok(EditCommand::Attach {
            entity: id,
            system: system.to_owned(),
            row,
        })
    }

    /// `command`, which puts a row in `system`, as it must be applied: alone
    /// while the manifest lists `system`, and otherwise batched after the
    /// [`EditCommand::ListSystem`] that adds it — the shape an attach and a
    /// drop share, so one undo takes the row and the listing back together.
    pub(super) fn listing_first(&self, system: &str, command: EditCommand) -> EditCommand {
        if self.scene.systems().iter().any(|each| each == system) {
            return command;
        }
        EditCommand::Batch(vec![
            EditCommand::ListSystem {
                system: system.to_owned(),
                at: self.scene.systems().len(),
            },
            command,
        ])
    }

    /// The codec of `system`, if the scene's manifest lists it and the
    /// vocabulary can read it — the one check a spawn, an attach and a detach
    /// share.
    pub(super) fn listed_codec(&self, system: &str) -> Result<Box<dyn SystemChunk>, EditError> {
        self.scene
            .systems()
            .iter()
            .any(|listed| listed == system)
            .then(|| self.registry.codec(system))
            .flatten()
            .ok_or_else(|| EditError::NoSystem(system.to_owned()))
    }

    /// Adds `system` to the manifest at position `at`, and hands back the
    /// unlisting that undoes it. The outline gains its group, so the
    /// membership moves.
    pub(super) fn list_system(
        &mut self,
        system: &str,
        at: usize,
    ) -> Result<EditCommand, EditError> {
        if !self.registry.contains(system) {
            return Err(EditError::NoSystem(system.to_owned()));
        }
        if self.scene.systems().iter().any(|listed| listed == system) {
            return Err(EditError::Listed(system.to_owned()));
        }
        let len = self.scene.systems().len();
        if at > len {
            return Err(EditError::PastManifest {
                system: system.to_owned(),
                at,
                len,
            });
        }
        self.scene.list_system_at(at, system);
        self.membership += 1;
        Ok(EditCommand::UnlistSystem {
            system: system.to_owned(),
        })
    }

    /// Takes `system`, which must hold no entity, out of the manifest, and
    /// hands back the listing that undoes it — at the place it held, so the
    /// manifest's order and the `scene.ron` it saves come back too.
    pub(super) fn unlist_system(&mut self, system: &str) -> Result<EditCommand, EditError> {
        self.listed_codec(system)?;
        if !self
            .registry
            .entities(&mut self.world, &self.ids, system)
            .is_empty()
        {
            return Err(EditError::Populated(system.to_owned()));
        }
        let at = self
            .scene
            .systems()
            .iter()
            .position(|listed| listed == system)
            .ok_or_else(|| EditError::NoSystem(system.to_owned()))?;
        self.scene.unlist_system(system);
        self.membership += 1;
        Ok(EditCommand::ListSystem {
            system: system.to_owned(),
            at,
        })
    }

    /// What follows `entity` entering or leaving a system: its collider rebuilt
    /// from whatever places it now, and the membership moved so the outline and
    /// the picture are read again.
    fn joined(&mut self, entity: crate::ecs::Entity) {
        sync_colliders(&self.registry, &mut self.world, [entity]);
        self.membership += 1;
    }
}
