//! The instances the document's entities are drawn as, and the last
//! descriptions sent to the renderer.
//!
//! Two kinds of entity are drawn: the scene's, by the id it is saved under,
//! and what a playing module spawned
//! ([`Document::spawned`]), by its entity — it has no id, and is drawn and
//! nothing else.
//!
//! **A mesh is drawn as its asset, everything else as a greybox cube** scaled
//! to its own box. A [`crcbl::scene_mesh::Mesh`] whose asset is measured and on
//! the renderer's [`Shelf`] is one instance per part of its glTF, each put
//! where the row's `position` puts the asset's origin; one whose asset is
//! missing, or not yet on the shelf, is the cube of its box — the placeholder
//! the document picks it by.

use std::collections::HashSet;

use crcbl::ecs::Entity;
use crcbl::greybox::{GREYBOX_CUBE, GREYBOX_GREY};
use crcbl::math::{Mat4, Quat, Vec3};
use crcbl::render::instance_pool::InstancePoolError;
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{ForwardRenderer, InstanceHandle};
use crcbl::scene::scn::SceneEntityId;

use super::meshes::Shelf;
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

/// A placed entity, the instances it is drawn as, and the description last
/// published for each: one for a cube, one per part for a mesh.
#[derive(Debug)]
pub(super) struct PlacedInstance {
    pub(super) drawn: Drawn,
    handles: Vec<InstanceHandle>,
    descs: Vec<InstanceDesc>,
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
        shelf: &Shelf,
    ) -> Result<Self, InstancePoolError> {
        let mut placed = Self {
            instances: Vec::new(),
            membership: document.membership(),
        };
        placed.reconcile(renderer, document, shelf)?;
        Ok(placed)
    }

    /// Brings the instances into line with the entities `document` draws when
    /// one has entered or left it, then publishes every changed description
    /// before the renderer settles motion history.
    ///
    /// # Errors
    ///
    /// The renderer's pool refusing an instance for an entity that arrived, or
    /// for a mesh drawn as more parts than before.
    pub(super) fn update(
        &mut self,
        renderer: &mut ForwardRenderer,
        document: &mut Document,
        shelf: &Shelf,
    ) -> Result<(), InstancePoolError> {
        if document.membership() != self.membership {
            self.reconcile(renderer, document, shelf)?;
            self.membership = document.membership();
        }
        publish(&mut self.instances, renderer, document, shelf)
    }

    /// Removes the instance of every entity the document no longer draws and
    /// adds one for every entity it draws that has none: the scene's, then
    /// what play spawned.
    ///
    /// A removal clears the handle's live bit, so a deleted entity stops being
    /// drawn rather than standing where it was. An entity with no bounds is one
    /// the document does not draw: it gets no instance, as at start-up, and
    /// loses the one it had when the component placing it was detached.
    fn reconcile(
        &mut self,
        renderer: &mut ForwardRenderer,
        document: &mut Document,
        shelf: &Shelf,
    ) -> Result<(), InstancePoolError> {
        let scene = document
            .outline()
            .into_iter()
            .flat_map(|(_, ids)| ids)
            .map(Drawn::Scene);
        let spawned = document.spawned().into_iter().map(Drawn::Spawned);
        let listed: Vec<Drawn> = scene.chain(spawned).collect();
        let wanted: Vec<(Drawn, Vec<InstanceDesc>)> = listed
            .into_iter()
            .map(|drawn| (drawn, instances_of(document, shelf, drawn)))
            .filter(|(_, descs)| !descs.is_empty())
            .collect();
        let held: HashSet<Drawn> = wanted.iter().map(|(drawn, _)| *drawn).collect();
        self.instances.retain(|instance| {
            let keep = held.contains(&instance.drawn);
            if !keep {
                for &handle in &instance.handles {
                    renderer.remove_instance(handle);
                }
            }
            keep
        });
        let placed: HashSet<Drawn> = self.instances.iter().map(|each| each.drawn).collect();
        for (drawn, descs) in wanted
            .into_iter()
            .filter(|(drawn, _)| !placed.contains(drawn))
        {
            let handles = add_all(renderer, &descs)?;
            self.instances.push(PlacedInstance {
                drawn,
                handles,
                descs,
            });
        }
        Ok(())
    }
}

/// One instance per description, in order.
fn add_all(
    renderer: &mut ForwardRenderer,
    descs: &[InstanceDesc],
) -> Result<Vec<InstanceHandle>, InstancePoolError> {
    descs
        .iter()
        .map(|desc| renderer.add_instance(desc))
        .collect()
}

/// Publishes changed descriptions before the renderer settles motion history.
///
/// An entity drawn as as many instances as before has each changed one set in
/// place; one drawn as a different number — a mesh whose asset reached the
/// shelf, or went missing — has its instances replaced.
///
/// # Errors
///
/// The renderer's pool refusing a replacement instance.
fn publish(
    placed: &mut [PlacedInstance],
    renderer: &mut ForwardRenderer,
    document: &mut Document,
    shelf: &Shelf,
) -> Result<(), InstancePoolError> {
    for instance in placed {
        let descs = instances_of(document, shelf, instance.drawn);
        // An entity with no box is one reconcile takes out when the membership
        // moves; until then it stays as it was last published.
        if descs.is_empty() || descs == instance.descs {
            continue;
        }
        if descs.len() == instance.handles.len() {
            for ((handle, desc), old) in instance.handles.iter().zip(&descs).zip(&instance.descs) {
                if desc != old {
                    renderer.set_instance(*handle, desc);
                }
            }
        } else {
            for &handle in &instance.handles {
                renderer.remove_instance(handle);
            }
            instance.handles = add_all(renderer, &descs)?;
        }
        instance.descs = descs;
    }
    Ok(())
}

/// How one entity is drawn — see the [module docs](self): its mesh's parts,
/// or the unit cube scaled to its own box, or nothing for an entity with no
/// box.
fn instances_of(document: &mut Document, shelf: &Shelf, drawn: Drawn) -> Vec<InstanceDesc> {
    if let Drawn::Scene(id) = drawn
        && let Some(mesh) = document.mesh(id)
        && mesh.local_bounds().is_some()
        && let Some(parts) = shelf.parts(&mesh.asset)
    {
        let origin = Mat4::from_translation(narrow(mesh.position));
        return parts
            .iter()
            .map(|part| InstanceDesc {
                mesh: part.mesh,
                material: part.material,
                transform: origin * part.transform,
            })
            .collect();
    }
    let bounds = match drawn {
        Drawn::Scene(id) => document.bounds(id),
        Drawn::Spawned(entity) => document.spawned_bounds(entity),
    };
    let Some((min, max)) = bounds else {
        return Vec::new();
    };
    vec![InstanceDesc {
        mesh: GREYBOX_CUBE,
        material: GREYBOX_GREY,
        transform: Mat4::from_scale_rotation_translation(
            max - min,
            Quat::IDENTITY,
            (min + max) * 0.5,
        ),
    }]
}

/// Render space's `f32`, from a row's `f64` position: the lossy direction,
/// which is the one a picture is drawn in.
#[allow(clippy::cast_possible_truncation)]
fn narrow(position: [f64; 3]) -> Vec3 {
    Vec3::new(position[0] as f32, position[1] as f32, position[2] as f32)
}

#[cfg(test)]
mod tests;
