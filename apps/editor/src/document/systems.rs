//! Which systems an entity is in: read, and changed through
//! [`EditCommand::Attach`] and [`EditCommand::Detach`].
//!
//! A scene may hold one entity in several systems (`crcbl::scene::scn`'s
//! _One entity, several systems_), each holding one component of it. An
//! attach gives the entity a component in one more of the scene's systems and
//! a detach takes one away, each the other's inverse through the same log as
//! every other edit, and refused in play mode by the same check.
//!
//! # Every registered system, listing it when it must
//!
//! [`attachable`](Document::attachable) offers every system the vocabulary
//! registers that does not hold the entity: the manifest's first, in its
//! order, then the rest in name order. A save writes the manifest's chunks and
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
//! which [`crcbl::registry::Registry::register`] requires of every registered
//! component — see that method for why the game chooses it rather than this
//! tool.

use crcbl::scene::scn::{SceneEntityId, SystemChunk};

use super::{Document, EditError, sync_colliders};
use crate::command::EditCommand;

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
    /// [`crcbl::registry::Registry::placing_system`] says which one that is.
    ///
    /// [`None`] for an id this document does not hold, and for an entity none
    /// of whose components is a thing in space.
    #[must_use]
    pub fn placing_system(&mut self, id: SceneEntityId) -> Option<String> {
        let entity = self.ids.entity(id)?;
        self.registry.placing_system(&mut self.world, entity)
    }

    /// The systems `id` could be given a component in: every registered one
    /// less those already holding it — the manifest's in its order, then the
    /// rest in name order. See the module docs for what attaching to one the
    /// manifest does not list does.
    #[must_use]
    pub fn attachable(&mut self, id: SceneEntityId) -> Vec<String> {
        if self.ids.entity(id).is_none() {
            return Vec::new();
        }
        let held = self.systems_of(id);
        let listed = self.scene.systems();
        let unlisted = self
            .registry
            .systems()
            .filter(|system| !listed.iter().any(|each| each == system));
        listed
            .iter()
            .map(String::as_str)
            .chain(unlisted)
            .filter(|system| !held.iter().any(|each| each == system))
            .map(str::to_owned)
            .collect()
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

    /// Adds `system` to the manifest, and hands back the unlisting that undoes
    /// it. The outline gains its group, so the membership moves.
    pub(super) fn list_system(&mut self, system: &str) -> Result<EditCommand, EditError> {
        if !self.registry.contains(system) {
            return Err(EditError::NoSystem(system.to_owned()));
        }
        if !self.scene.list_system(system) {
            return Err(EditError::Listed(system.to_owned()));
        }
        self.membership += 1;
        Ok(EditCommand::UnlistSystem {
            system: system.to_owned(),
        })
    }

    /// Takes `system`, which must hold no entity, out of the manifest, and
    /// hands back the listing that undoes it.
    pub(super) fn unlist_system(&mut self, system: &str) -> Result<EditCommand, EditError> {
        self.listed_codec(system)?;
        if !self
            .registry
            .entities(&mut self.world, &self.ids, system)
            .is_empty()
        {
            return Err(EditError::Populated(system.to_owned()));
        }
        self.scene.unlist_system(system);
        self.membership += 1;
        Ok(EditCommand::ListSystem {
            system: system.to_owned(),
        })
    }

    /// What follows `entity` entering or leaving a system: its collider rebuilt
    /// from whatever places it now, and the membership moved so the outline and
    /// the picture are read again.
    fn joined(&mut self, entity: crcbl::ecs::Entity) {
        sync_colliders(&self.registry, &mut self.world, [entity]);
        self.membership += 1;
    }
}
