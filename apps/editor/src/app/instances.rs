//! Greybox instances and the last document descriptions sent to the renderer.

use crcbl::greybox::{GREYBOX_CUBE, GREYBOX_GREY};
use crcbl::math::{Mat4, Quat};
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

/// Publishes changed descriptions before the renderer settles motion history.
pub(super) fn update(
    placed: &mut [PlacedInstance],
    renderer: &mut ForwardRenderer,
    document: &mut Document,
) {
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

/// Places one instance per entity, in the document's own order.
pub(super) fn place(
    renderer: &mut ForwardRenderer,
    document: &mut Document,
) -> Result<Vec<PlacedInstance>, crcbl::render::instance_pool::InstancePoolError> {
    let ids: Vec<SceneEntityId> = document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect();
    let mut placed = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(desc) = instance_of(document, id) else {
            continue;
        };
        placed.push(PlacedInstance {
            id,
            handle: renderer.add_instance(&desc)?,
            desc,
        });
    }
    Ok(placed)
}

#[cfg(test)]
mod tests;
