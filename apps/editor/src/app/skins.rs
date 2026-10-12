//! The skinned geometry the viewport draws: a posed region for every part a
//! shelved asset's rig deforms, and the pass that fills them.
//!
//! **One region per part, not per entity.** In the editor every entity of an
//! asset stands in the same pose — its rest pose — so one region holds the
//! posed vertices and every entity's instance draws it under its own
//! transform. That keeps the count fixed when the shelf is built, so spawning
//! or deleting an entity never rebuilds the renderer; a later slice that poses
//! an entity on its own (a ragdoll dropped in play) reserves regions of its
//! own. `docs/backlog.md`'s editor entry has the slices.
//!
//! **A palette per range.** The editor merges many assets into one renderer,
//! so unlike `apps/viewer`, whose ranges all share one skeleton, each range
//! carries its own asset's rest palette.
//!
//! The pass is built against the renderer's vertex pool and must go with it:
//! a rebuild replaces both, and [`Skins::destroy`] runs beside
//! `ForwardRenderer::destroy`.

use std::collections::BTreeMap;

use crcbl::anim::{Palette, Pose};
use crcbl::engine::GpuError;
use crcbl::hal::{Device, HalError};
use crcbl::math::Mat4;
use crcbl::render::forward::FRAMES_IN_FLIGHT;
use crcbl::render::{
    ForwardRenderer, SkinRange, SkinnedMesh, Skinning, SkinningDesc, SkinningError,
};
use crcbl::shaders::skinning::SkinBinding;

use super::meshes::Shelf;

/// One posed region: the part it draws, its bindings and its palette.
#[derive(Debug)]
struct Draw {
    mesh: SkinnedMesh,
    bindings: Vec<SkinBinding>,
    /// The asset's rest palette, its skin's joints in palette order.
    palette: Vec<Mat4>,
    /// Skeleton space to the asset's own frame.
    transform: Mat4,
}

/// The posed regions and their pass — see the [module docs](self).
#[derive(Debug, Default)]
pub(super) struct Skins {
    skinning: Option<Skinning>,
    draws: Vec<Draw>,
    /// Which draw an (asset, part index) is posed in.
    by_part: BTreeMap<(String, usize), usize>,
}

impl Skins {
    /// Reserves a region for every part `shelf`'s rigs deform, at its asset's
    /// rest pose, and builds the pass that fills them. Empty — no pass at
    /// all — for a shelf with no rig, which draws exactly as before.
    ///
    /// # Errors
    ///
    /// [`GpuError`] for a pool with no room for a region or a pass that could
    /// not be built. Every reservation is given back first, so the renderer is
    /// left as it was found.
    pub(super) fn build(
        device: &dyn Device,
        renderer: &mut ForwardRenderer,
        shelf: &Shelf,
    ) -> Result<Self, GpuError> {
        let mut skins = Self::default();
        for asset in shelf.assets() {
            let (Some(rig), Some(parts)) = (shelf.rig(&asset), shelf.parts(&asset)) else {
                continue;
            };
            let mut palette = Palette::new(&rig.skeleton);
            palette.compute(&rig.skeleton, &Pose::new(&rig.skeleton));
            for deformed in &rig.skinned.instances {
                let Some(part) = parts.get(deformed.instance) else {
                    continue;
                };
                match renderer.reserve_skinned(part.mesh) {
                    Ok(mesh) => {
                        skins
                            .by_part
                            .insert((asset.clone(), deformed.instance), skins.draws.len());
                        skins.draws.push(Draw {
                            mesh,
                            bindings: deformed.bindings.clone(),
                            palette: palette.matrices().to_vec(),
                            transform: deformed.transform,
                        });
                    }
                    Err(error) => {
                        skins.release(renderer);
                        return Err(GpuError::pools("the editor's skinned meshes", &error));
                    }
                }
            }
        }
        if skins.draws.is_empty() {
            return Ok(skins);
        }
        let count = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
        let built = Skinning::new(
            device,
            &SkinningDesc {
                label: Some("editor skinning"),
                frames: FRAMES_IN_FLIGHT,
                ranges: count(skins.draws.len()),
                // Each range its own asset's palette — see the module docs.
                joints: count(skins.draws.iter().map(|draw| draw.palette.len()).sum()),
                bindings: count(skins.draws.iter().map(|draw| draw.bindings.len()).sum()),
                // The renderer's pool and no other: a pass built against a
                // different buffer writes vertices no draw of it can reach.
                vertices: renderer.vertex_buffer(),
                attribute_base: renderer.attribute_base(),
            },
        );
        match built {
            Ok(skinning) => {
                skins.skinning = Some(skinning);
                Ok(skins)
            }
            Err(error) => {
                skins.release(renderer);
                Err(GpuError::Hal(hal_error(error)))
            }
        }
    }

    /// The draw `asset`'s part `part` is posed in, if a rig deforms it.
    pub(super) fn draw_of(&self, asset: &str, part: usize) -> Option<usize> {
        self.by_part.get(&(asset.to_owned(), part)).copied()
    }

    /// Draw `draw`'s region.
    pub(super) fn mesh(&self, draw: usize) -> &SkinnedMesh {
        &self.draws[draw].mesh
    }

    /// Where draw `draw`'s skeleton hangs in its asset's frame.
    pub(super) fn transform(&self, draw: usize) -> Mat4 {
        self.draws[draw].transform
    }

    /// The pass, for the frame's skinned passes, or [`None`] when nothing is
    /// skinned.
    pub(super) fn skinning(&self) -> Option<&Skinning> {
        self.skinning.as_ref()
    }

    /// The pass and this frame's ranges, one per region, or [`None`] when
    /// nothing is skinned and the frame takes the plain entry points.
    pub(super) fn frame(&mut self) -> Option<(&mut Skinning, Vec<SkinRange<'_>>)> {
        let skinning = self.skinning.as_mut()?;
        let ranges = self
            .draws
            .iter()
            .map(|draw| draw.mesh.skin_range(&draw.palette, &draw.bindings))
            .collect();
        Some((skinning, ranges))
    }

    /// Releases the pass. The regions go with the renderer they were reserved
    /// in, which is destroyed beside this.
    pub(super) fn destroy(self, device: &dyn Device) {
        if let Some(skinning) = self.skinning {
            skinning.destroy(device);
        }
    }

    /// Gives every reserved region back to `renderer`.
    fn release(self, renderer: &mut ForwardRenderer) {
        for draw in self.draws {
            renderer.release_skinned(draw.mesh);
        }
    }
}

/// A skinning error as the device error it is, or a backend one carrying its
/// message.
pub(super) fn hal_error(error: SkinningError) -> HalError {
    match error {
        SkinningError::Hal(hal) => hal,
        other => HalError::Backend(other.to_string()),
    }
}
