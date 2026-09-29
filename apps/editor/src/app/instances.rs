//! Greybox instances and the last document descriptions sent to the renderer.

use std::collections::HashSet;

use crcbl::greybox::{GREYBOX_CUBE, GREYBOX_GREY};
use crcbl::math::{Mat4, Quat};
use crcbl::render::instance_pool::InstancePoolError;
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{ForwardRenderer, InstanceHandle};
use crcbl::scene::scn::SceneEntityId;

use crate::document::Document;

/// A placed entity and the description last published for its handle.
#[derive(Debug)]
pub(super) struct PlacedInstance {
    id: SceneEntityId,
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
    /// One instance per entity `document` holds, in its own order.
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

    /// Brings the instances into line with the entities `document` holds when
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

    /// Removes the instance of every entity the document no longer holds and
    /// adds one for every entity it holds that has none.
    ///
    /// A removal clears the handle's live bit, so a deleted entity stops being
    /// drawn rather than standing where it was; an entity with no bounds gets no
    /// instance, as at start-up.
    fn reconcile(
        &mut self,
        renderer: &mut ForwardRenderer,
        document: &mut Document,
    ) -> Result<(), InstancePoolError> {
        let ids: Vec<SceneEntityId> = document
            .outline()
            .into_iter()
            .flat_map(|(_, ids)| ids)
            .collect();
        let held: HashSet<SceneEntityId> = ids.iter().copied().collect();
        self.instances.retain(|instance| {
            let keep = held.contains(&instance.id);
            if !keep {
                renderer.remove_instance(instance.handle);
            }
            keep
        });
        let drawn: HashSet<SceneEntityId> = self.instances.iter().map(|each| each.id).collect();
        for id in ids.into_iter().filter(|id| !drawn.contains(id)) {
            let Some(desc) = instance_of(document, id) else {
                continue;
            };
            self.instances.push(PlacedInstance {
                id,
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
        let Some(desc) = instance_of(document, instance.id) else {
            continue;
        };
        if desc != instance.desc {
            renderer.set_instance(instance.handle, &desc);
            instance.desc = desc;
        }
    }
}

/// How one entity is drawn: the unit cube, scaled to its own extents.
fn instance_of(document: &mut Document, id: SceneEntityId) -> Option<InstanceDesc> {
    let (min, max) = document.bounds(id)?;
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
