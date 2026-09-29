//! What a [`DrawList::texture`](crcbl_ui::DrawList::texture) rectangle needs
//! from the UI pass: the caller's pairing of each [`TextureId`] with a graph
//! image, the split of a half's indices into one draw per bind, and the set-1
//! bind groups those draws name.

use core::ops::Range;
use std::sync::{Mutex, PoisonError};

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutHandle, BindingResource, Device,
    ImageViewHandle,
};
use crcbl_ui::draw_list::TextureRun;
use crcbl_ui::image::TextureId;

use crate::graph::ImageId;

/// One texture a frame's draw list names, as the render graph knows it — what
/// [`UiRenderer::add_passes_with_textures`](super::UiRenderer::add_passes_with_textures)
/// takes.
///
/// `image` is any image in the same graph that a fragment shader can sample
/// as filterable float: a secondary view's target, the target a primary camera
/// drew into, an imported texture. The UI pass declares that it reads it, so
/// the graph puts the barrier between it and the passes that drew it, and the
/// caller declares nothing. Those passes are added first: the graph runs passes
/// in the order they were declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiTexture {
    /// The name the draw list used.
    pub id: TextureId,
    /// The image it stands for this frame.
    pub image: ImageId,
}

/// One draw of a half: an index range, and the texture its set 1 binds — or
/// `None` for the transparent placeholder every range but a texture run binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Draw {
    pub(super) indices: Range<u32>,
    pub(super) texture: Option<TextureId>,
}

/// Splits `segment` into the draws that record it: every [`TextureRun`] inside
/// it is a draw of its own, and each stretch between two runs is one draw.
///
/// Runs never straddle the overlay cut (see
/// [`Triangles::textures`](crcbl_ui::draw_list::Triangles::textures)), so a
/// run is either inside a half or outside it; one reaching past `segment`
/// anyway is clamped to it rather than drawn out of range.
pub(super) fn draws(segment: Range<u32>, runs: &[TextureRun]) -> Vec<Draw> {
    let mut draws = Vec::new();
    let mut cursor = segment.start;
    for run in runs {
        let start = run.indices.start.max(segment.start);
        let end = run.indices.end.min(segment.end);
        if start >= end {
            continue;
        }
        if cursor < start {
            draws.push(Draw {
                indices: cursor..start,
                texture: None,
            });
        }
        draws.push(Draw {
            indices: start..end,
            texture: Some(run.texture),
        });
        cursor = end;
    }
    if cursor < segment.end {
        draws.push(Draw {
            indices: cursor..segment.end,
            texture: None,
        });
    }
    draws
}

/// The image `id` stands for, the first pairing in `textures` that names it.
pub(super) fn image_of(textures: &[UiTexture], id: TextureId) -> Option<ImageId> {
    textures
        .iter()
        .find(|texture| texture.id == id)
        .map(|texture| texture.image)
}

/// One set-1 group, and whether a frame recorded through its slot used it.
#[derive(Debug)]
struct Cached {
    view: ImageViewHandle,
    group: BindGroupHandle,
    used: bool,
}

/// The set-1 groups naming the textures frames sampled, per frame in flight.
///
/// **Kept rather than rebuilt**: a view that samples the same target every
/// frame writes one descriptor when the target is (re)made, not one a frame.
/// A group is released on its slot's next turn after a frame that did not use
/// it, which is the same rotation that keeps the vertex ring safe: by then the
/// last frame that bound it has finished.
///
/// Behind a [`Mutex`] because the pass bodies that fill it borrow the renderer
/// shared — [`UiRenderer::add_passes`](super::UiRenderer::add_passes) takes
/// `&self`, as every caller has always called it.
#[derive(Debug)]
pub(super) struct TextureGroups {
    slots: Mutex<Vec<Vec<Cached>>>,
}

impl TextureGroups {
    pub(super) fn new(frames: usize) -> Self {
        Self {
            slots: Mutex::new((0..frames).map(|_| Vec::new()).collect()),
        }
    }

    /// Begins `slot`'s turn: releases every group the slot's last frame did not
    /// use, and marks the rest unused until this frame uses them.
    pub(super) fn retire(&mut self, device: &dyn Device, slot: usize) {
        let slots = self.slots.get_mut().unwrap_or_else(PoisonError::into_inner);
        slots[slot].retain_mut(|cached| {
            if !cached.used {
                device.destroy_bind_group(cached.group);
            }
            let kept = cached.used;
            cached.used = false;
            kept
        });
    }

    /// The group binding `view` for a frame in `slot`, built on first use.
    ///
    /// [`None`] when the device refused it, logged: the draw is skipped, as
    /// [`crate::bind_group_cache`]'s groups skip theirs.
    pub(super) fn group(
        &self,
        device: &dyn Device,
        layout: BindGroupLayoutHandle,
        slot: usize,
        view: ImageViewHandle,
    ) -> Option<BindGroupHandle> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let groups = &mut slots[slot];
        if let Some(cached) = groups.iter_mut().find(|cached| cached.view == view) {
            cached.used = true;
            return Some(cached.group);
        }
        match device.create_bind_group(&BindGroupDesc {
            label: Some("ui texture"),
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::ImageView(view),
            }],
            variable_count: None,
        }) {
            Ok(group) => {
                groups.push(Cached {
                    view,
                    group,
                    used: true,
                });
                Some(group)
            }
            Err(error) => {
                crcbl_core::log::error!("graph: ui texture bind group failed: {error}");
                None
            }
        }
    }

    /// The view each held group names, across every slot.
    #[cfg(test)]
    pub(super) fn views(&self) -> Vec<ImageViewHandle> {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .flatten()
            .map(|cached| cached.view)
            .collect()
    }

    /// Releases every group. The device must be idle.
    pub(super) fn destroy(&mut self, device: &dyn Device) {
        let slots = self.slots.get_mut().unwrap_or_else(PoisonError::into_inner);
        for cached in slots.iter_mut().flat_map(|slot| slot.drain(..)) {
            device.destroy_bind_group(cached.group);
        }
    }
}
