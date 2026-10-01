//! Greybox instances and the last document descriptions sent to the renderer.
//!
//! Two kinds of entity are drawn: the scene's, by the id it is saved under,
//! and what a playing module spawned
//! ([`Document::spawned`]), by its entity — it has no id, and is drawn and
//! nothing else.

use std::collections::HashSet;

use crcbl::ecs::Entity;
use crcbl::greybox::{GREYBOX_CUBE, GREYBOX_GREY};
use crcbl::math::{Mat4, Quat};
use crcbl::render::instance_pool::InstancePoolError;
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{ForwardRenderer, InstanceHandle};
use crcbl::scene::scn::SceneEntityId;

use crate::document::Document;

/// Which entity an instance draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Drawn {
    /// One of the scene's, by the id it is saved under — what survives a
    /// reload.
    Scene(SceneEntityId),
    /// One a playing module spawned, which has no id; it is gone, and its
    /// instance with it, when play stops.
    Spawned(Entity),
}

/// A placed entity and the description last published for its handle.
#[derive(Debug)]
pub(super) struct PlacedInstance {
    pub(super) drawn: Drawn,
    handle: InstanceHandle,
    desc: InstanceDesc,
}

/// The instances drawn for a document, and the [`Document::membership`] they
/// were last brought into line with.
#[derive(Debug)]
pub(super) struct Placed {
    pub(super) instances: Vec<PlacedInstance>,
    membership: u64,
}

impl Placed {
    /// One instance per entity `document` draws, in its own order.
    ///
    /// # Errors
    ///
    /// The renderer's pool refusing an instance.
    pub(super) fn place(
        renderer: &mut ForwardRenderer,
        document: &mut Document,
    ) -> Result<Self, InstancePoolError> {
        let mut placed = Self {
            instances: Vec::new(),
            membership: document.membership(),
        };
        placed.reconcile(renderer, document)?;
        Ok(placed)
    }

    /// Brings the instances into line with the entities `document` draws when
    /// one has entered or left it, then publishes every changed description
    /// before the renderer settles motion history.
    ///
    /// # Errors
    ///
    /// The renderer's pool refusing an instance for an entity that arrived.
    pub(super) fn update(
        &mut self,
        renderer: &mut ForwardRenderer,
        document: &mut Document,
    ) -> Result<(), InstancePoolError> {
        if document.membership() != self.membership {
            self.reconcile(renderer, document)?;
            self.membership = document.membership();
        }
        publish(&mut self.instances, renderer, document);
        Ok(())
    }

    /// Removes the instance of every entity the document no longer draws and
    /// adds one for every entity it draws that has none: the scene's, then
    /// what play spawned.
    ///
    /// A removal clears the handle's live bit, so a deleted entity stops being
    /// drawn rather than standing where it was; an entity with no bounds gets no
    /// instance, as at start-up.
    fn reconcile(
        &mut self,
        renderer: &mut ForwardRenderer,
        document: &mut Document,
    ) -> Result<(), InstancePoolError> {
        let scene = document
            .outline()
            .into_iter()
            .flat_map(|(_, ids)| ids)
            .map(Drawn::Scene);
        let spawned = document.spawned().into_iter().map(Drawn::Spawned);
        let wanted: Vec<Drawn> = scene.chain(spawned).collect();
        let held: HashSet<Drawn> = wanted.iter().copied().collect();
        self.instances.retain(|instance| {
            let keep = held.contains(&instance.drawn);
            if !keep {
                renderer.remove_instance(instance.handle);
            }
            keep
        });
        let placed: HashSet<Drawn> = self.instances.iter().map(|each| each.drawn).collect();
        for drawn in wanted.into_iter().filter(|drawn| !placed.contains(drawn)) {
            let Some(desc) = instance_of(document, drawn) else {
                continue;
            };
            self.instances.push(PlacedInstance {
                drawn,
                handle: renderer.add_instance(&desc)?,
                desc,
            });
        }
        Ok(())
    }
}

/// Publishes changed descriptions before the renderer settles motion history.
fn publish(placed: &mut [PlacedInstance], renderer: &mut ForwardRenderer, document: &mut Document) {
    for instance in placed {
        let Some(desc) = instance_of(document, instance.drawn) else {
            continue;
        };
        if desc != instance.desc {
            renderer.set_instance(instance.handle, &desc);
            instance.desc = desc;
        }
    }
}

/// How one entity is drawn: the unit cube, scaled to its own extents.
fn instance_of(document: &mut Document, drawn: Drawn) -> Option<InstanceDesc> {
    let (min, max) = match drawn {
        Drawn::Scene(id) => document.bounds(id)?,
        Drawn::Spawned(entity) => document.spawned_bounds(entity)?,
    };
    Some(InstanceDesc {
        mesh: GREYBOX_CUBE,
        material: GREYBOX_GREY,
        transform: Mat4::from_scale_rotation_translation(
            max - min,
            Quat::IDENTITY,
            (min + max) * 0.5,
        ),
    })
}

#[cfg(test)]
mod tests;
