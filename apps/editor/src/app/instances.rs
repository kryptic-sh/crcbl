//! Greybox instance placement and the document descriptions it draws.

use crcbl::greybox::{GREYBOX_CUBE, GREYBOX_GREY};
use crcbl::math::{Mat4, Quat};
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{ForwardRenderer, InstanceHandle};
use crcbl::scene::scn::SceneEntityId;

use crate::document::Document;

/// How one entity is drawn: the unit cube, scaled to its own extents.
pub(super) fn instance_of(document: &mut Document, id: SceneEntityId) -> Option<InstanceDesc> {
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
) -> Result<Vec<(SceneEntityId, InstanceHandle)>, crcbl::render::instance_pool::InstancePoolError> {
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
        placed.push((id, renderer.add_instance(&desc)?));
    }
    Ok(placed)
}
