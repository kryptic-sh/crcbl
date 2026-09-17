//! `docs/plan/03-gpu-driven-rendering.md` §3.3's occlusion cull: the
//! farthest-depth pyramid it reads, and the switches that turn it on.
//!
//! ```text
//!  frame N-1: … ──▶ early prepass ──▶ occlusion-hiz-1 ──▶ … ──▶ occlusion-hiz-n
//!                                                                     │ kept
//!  frame N:  cull (phase 1, reads N-1's pyramid through N-1's matrix) ◀─┘
//!              └─▶ draw args ──▶ early prepass ──▶ occlusion-hiz-1..n (N's)
//!                                                         │
//!            occlusion-late (phase 2, reads N's) ◀────────┘
//!              └─▶ draw-late-scatter ──▶ draw-late-finish ──▶ late prepass
//!                                                            ──▶ forward
//! ```
//!
//! # One pyramid, kept, and rewritten in place
//!
//! The first phase needs the previous frame's pyramid and the second needs this
//! frame's, so the levels are **images this module owns** rather than graph
//! transients — a transient's contents do not survive the frame. They are
//! imported every frame in the state the pool's ledger last saw them in, read by
//! the first phase, written by this frame's reduction and read again by the
//! second, and the graph orders all three out of those declarations. No ring:
//! frames reach the device in order, so frame N's first read runs after frame
//! N-1's last write whether or not N-1 is still in flight when N is recorded.
//!
//! **What the next frame reads is this frame's early depth**, before the late
//! prepass adds what the second phase rescued. That is less than the finished
//! frame holds, which is the safe direction: a pyramid missing an occluder hides
//! less. It saves a second reduction per frame.
//!
//! # Levels, and the memory they cost
//!
//! A level is a `D32Float` image at half the extent of the one above, from level
//! 1 down to [`OCCLUSION_MAX_LEVELS`] or a one-texel axis — see [`levels_for`].
//! Level 0 is the prepass itself, which is a transient the first phase cannot
//! reach, so both phases start at level 1. At 1920×1080 that is eight images
//! totalling about a third of the prepass's texels, 2.64 MiB — measured by
//! `the_pyramid_costs_a_third_of_the_prepass`.
//!
//! # A second chain, not a second channel
//!
//! [`crate::hiz`]'s chain holds the **nearest** depth for the reflection march,
//! and is built only on frames that reflect; this one holds the **farthest** —
//! see `shaders/hiz.slang`'s `farthestMain` — and is built on frames that cull.
//! A second channel would need a colour format and a second texture type for
//! one of the two readers, which `hiz.slang`'s header refuses for reasons that
//! still hold; a variant entry point of the same reduction is the whole change.
//!
//! [`OCCLUSION_MAX_LEVELS`]: crcbl_shaders::cull::OCCLUSION_MAX_LEVELS

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, ClearValue, Device, Format,
    GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageSubresourceRange, ImageType,
    ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, LoadOp, PipelineLayoutDesc,
    PipelineLayoutHandle, ResourceState, SampleType, ShaderStages, StoreOp,
    check_portable_storage_buffers,
};
use crcbl_shaders::HIZ;
use crcbl_shaders::cull::OCCLUSION_MAX_LEVELS;

use crate::graph::{ImageId, ImportedImage, InitialClaim, RenderGraph};
use crate::ssao::cached_group;
use crate::transient::TransientPool;

/// The vertex count of the full-screen triangle every reduction pass draws.
const FULLSCREEN_VERTICES: u32 = 3;

/// How many farthest-depth levels a prepass of `extent` has: halving from level
/// 1 while both axes keep at least one texel, up to
/// [`OCCLUSION_MAX_LEVELS`].
///
/// Zero for a one-texel axis, which is a target nothing can be hidden in.
#[must_use]
pub fn levels_for(extent: (u32, u32)) -> u32 {
    let mut levels = 0;
    while levels < OCCLUSION_MAX_LEVELS
        && extent.0 >> (levels + 1) >= 1
        && extent.1 >> (levels + 1) >= 1
    {
        levels += 1;
    }
    levels
}

/// The extent of level `level` over a prepass of `extent`: the floor of halving
/// `level` times, which `hiz.slang`'s odd-axis taps are written against.
#[must_use]
pub const fn level_extent(extent: (u32, u32), level: u32) -> (u32, u32) {
    (extent.0 >> level, extent.1 >> level)
}

crcbl_console::convar! {
    /// Cull instances hidden behind the frame's own depth, in two phases: off.
    ///
    /// **Pixel-identical either way** — the second phase is conservative — so
    /// what it changes is draws and time. Off by default until the price on
    /// every tier says otherwise; `docs/plan/03-gpu-driven-rendering.md` §3.3.
    pub static r_occlusion_cull: bool = false;
}

crcbl_console::convar! {
    /// Drop instances projecting under this many pixels: 0 keeps every size.
    ///
    /// **Changes pixels by design**, which is why it is off.
    pub static r_small_feature_px: f32 in 0.0 ..= 64.0 = 0.0;
}

/// The occlusion culls a caller asked for — see
/// [`ForwardRenderer::set_occlusion_culling`](crate::ForwardRenderer::set_occlusion_culling).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OcclusionCulling {
    /// The two-phase depth-pyramid cull. **Pixel-identical** to leaving it off:
    /// it removes draws and never a fragment that would have survived the
    /// forward pass's depth test.
    pub occlusion: bool,
    /// Drop an instance whose projected box's longer side is under this many
    /// pixels, or `None` to keep every size. **This one changes pixels** — a
    /// feature that small still covers the pixel centres it straddles — which is
    /// why it is off unless a caller names a threshold.
    pub small_feature_pixels: Option<f32>,
}

impl OcclusionCulling {
    /// Nothing culled beyond the frustum: every renderer's default.
    pub const OFF: Self = Self {
        occlusion: false,
        small_feature_pixels: None,
    };

    /// Whether either cull is asked for, which is what makes a frame dispatch
    /// the occlusion entry point at all.
    #[must_use]
    pub const fn any(self) -> bool {
        self.occlusion || self.small_feature_pixels.is_some()
    }
}

impl Default for OcclusionCulling {
    fn default() -> Self {
        Self::OFF
    }
}

/// One level image and its view.
#[derive(Clone, Copy, Debug)]
struct Level {
    image: ImageHandle,
    view: ImageViewHandle,
}

/// The pyramid's images at one extent, and the groups that name them.
#[derive(Debug)]
struct Chain {
    extent: (u32, u32),
    levels: Vec<Level>,
    /// The cull's set 1, naming every slot `cull.slang` declares — a chain
    /// shorter than [`OCCLUSION_MAX_LEVELS`] repeats its deepest level.
    cull_group: BindGroupHandle,
    /// `[level - 2]`: the reduction's group for levels 2 and down, whose sources
    /// are this chain's own images and so never change under it.
    reduce_groups: Vec<BindGroupHandle>,
}

/// The farthest-depth pyramid one view's occlusion cull reads — see the
/// [module docs](self).
#[derive(Debug)]
pub(crate) struct OcclusionPyramid {
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// The chain at the extent last asked for, or `None` before the first frame
    /// that culled.
    chain: Option<Chain>,
    /// `[frame]`: level 1's reduction group, cached against the prepass view —
    /// a transient, so per frame in flight on [`crate::hiz`]'s terms.
    first_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// Whether the chain holds a frame's depth at its own extent: set when a
    /// frame records the reduction, cleared when the chain is replaced.
    history: bool,
}

impl OcclusionPyramid {
    /// Reduction passes [`OcclusionPyramid::add_passes`] adds at most.
    pub(crate) const PASSES: u32 = OCCLUSION_MAX_LEVELS;

    /// Builds the reduction pipeline. The images wait for the first frame's
    /// extent — see [`OcclusionPyramid::prepare`].
    ///
    /// `build_depth_fullscreen` is [`crate::forward`]'s, handed in on
    /// [`crate::hiz::Hiz::new`]'s terms, with the entry points named: `hiz.slang`
    /// has two fragment stages.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. Nothing is released on the failing path,
    /// for the reason every builder in this crate gives.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        build_depth_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            &str,
            &str,
            PipelineLayoutHandle,
            Format,
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        let entries = [BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Depth,
            },
            count: 1,
            flags: BindingFlags::empty(),
        }];
        let desc = BindGroupLayoutDesc {
            label: Some("occlusion hiz source"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("occlusion hiz"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("occlusion hiz"),
            bind_group_layouts: &[layout],
            push_constants: None,
        })?;
        let pipeline = build_depth_fullscreen(
            device,
            "occlusion hiz",
            &HIZ,
            "vertexMain",
            "farthestMain",
            pipeline_layout,
            Format::D32Float,
        )?;
        Ok(Self {
            layout,
            pipeline_layout,
            pipeline,
            chain: None,
            first_groups: vec![None; frames],
            history: false,
        })
    }

    /// Makes the chain match `extent`, rebuilding it — and forgetting its
    /// history — when it does not. `cull_layout` is the camera generator's
    /// [`DrawGen::occlusion_layout`](crate::DrawGen::occlusion_layout).
    ///
    /// Called from a frame's `begin_frame`, which is where a failed allocation
    /// can still refuse the frame rather than leave it half recorded.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call; the chain is then absent, and a later
    /// frame tries again.
    pub(crate) fn prepare(
        &mut self,
        device: &dyn Device,
        extent: (u32, u32),
        cull_layout: BindGroupLayoutHandle,
    ) -> Result<(), HalError> {
        if self
            .chain
            .as_ref()
            .is_some_and(|chain| chain.extent == extent)
        {
            return Ok(());
        }
        if let Some(chain) = self.chain.take() {
            destroy_chain(device, chain);
        }
        self.history = false;
        let count = levels_for(extent);
        if count == 0 {
            return Ok(());
        }
        let mut levels: Vec<Level> = Vec::with_capacity(count as usize);
        let built = (|| -> Result<Chain, HalError> {
            for level in 1..=count {
                let (width, height) = level_extent(extent, level);
                let image = device.create_image(&ImageDesc {
                    label: Some(&format!("occlusion pyramid {level}")),
                    image_type: ImageType::D2,
                    format: Format::D32Float,
                    extent: crcbl_hal::Extent3d::d2(width, height),
                    mip_levels: 1,
                    samples: 1,
                    // `TRANSFER_SRC` so a test can read a level back and hold it
                    // against the CPU reduction.
                    usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT
                        .union(ImageUsage::SAMPLED)
                        .union(ImageUsage::TRANSFER_SRC),
                })?;
                let view = match device.create_image_view(&ImageViewDesc {
                    label: Some(&format!("occlusion pyramid {level}")),
                    image,
                    view_type: ImageViewType::D2,
                    format: Format::D32Float,
                    range: ImageSubresourceRange::all(Format::D32Float),
                }) {
                    Ok(view) => view,
                    Err(error) => {
                        device.destroy_image(image);
                        return Err(error);
                    }
                };
                levels.push(Level { image, view });
            }
            let deepest = levels[levels.len() - 1];
            let cull_entries: Vec<BindGroupEntry> = (0..OCCLUSION_MAX_LEVELS)
                .map(|binding| BindGroupEntry {
                    binding,
                    array_index: 0,
                    resource: BindingResource::ImageView(
                        levels.get(binding as usize).unwrap_or(&deepest).view,
                    ),
                })
                .collect();
            let cull_group = device.create_bind_group(&BindGroupDesc {
                label: Some("occlusion pyramid"),
                layout: cull_layout,
                entries: &cull_entries,
                variable_count: None,
            })?;
            let mut reduce_groups = Vec::with_capacity(levels.len().saturating_sub(1));
            for source in &levels[..levels.len() - 1] {
                match device.create_bind_group(&BindGroupDesc {
                    label: Some("occlusion hiz"),
                    layout: self.layout,
                    entries: &[BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(source.view),
                    }],
                    variable_count: None,
                }) {
                    Ok(group) => reduce_groups.push(group),
                    Err(error) => {
                        for group in reduce_groups {
                            device.destroy_bind_group(group);
                        }
                        device.destroy_bind_group(cull_group);
                        return Err(error);
                    }
                }
            }
            Ok(Chain {
                extent,
                levels: levels.clone(),
                cull_group,
                reduce_groups,
            })
        })();
        match built {
            Ok(chain) => {
                self.chain = Some(chain);
                Ok(())
            }
            Err(error) => {
                for level in levels {
                    device.destroy_image_view(level.view);
                    device.destroy_image(level.image);
                }
                Err(error)
            }
        }
    }

    /// Levels the chain has at the extent [`prepare`](Self::prepare) last built
    /// it for — zero with no chain.
    pub(crate) fn levels(&self) -> u32 {
        self.chain
            .as_ref()
            .map_or(0, |chain| u32::try_from(chain.levels.len()).unwrap_or(0))
    }

    /// Whether the chain holds the previous frame's depth at `extent` — and
    /// forgets it, so a frame that records no reduction leaves the next one
    /// without a history rather than with a stale one.
    pub(crate) fn take_history(&mut self, extent: (u32, u32)) -> bool {
        let history = self.history
            && self
                .chain
                .as_ref()
                .is_some_and(|chain| chain.extent == extent);
        self.history = false;
        history
    }

    /// The cull's set 1, or `None` with no chain.
    pub(crate) fn cull_group(&self) -> Option<BindGroupHandle> {
        self.chain.as_ref().map(|chain| chain.cull_group)
    }

    /// Imports every level into `graph`, in the state the pool's ledger last
    /// recorded, handed back in [`ResourceState::ShaderRead`] — which is what a
    /// frame that only reads them leaves them in too.
    pub(crate) fn import(&self, graph: &mut RenderGraph<'_>, pool: &TransientPool) -> Vec<ImageId> {
        let Some(chain) = &self.chain else {
            return Vec::new();
        };
        chain
            .levels
            .iter()
            .enumerate()
            .map(|(index, level)| {
                graph.import_image(
                    format!("occlusion-pyramid-{}", index + 1),
                    ImportedImage {
                        image: level.image,
                        view: level.view,
                        format: Format::D32Float,
                        extent: level_extent(chain.extent, index as u32 + 1),
                        initial: pool
                            .imported_image_use(level.image)
                            .unwrap_or(ResourceState::Undefined),
                        claim: InitialClaim::Tracked,
                        final_state: ResourceState::ShaderRead,
                    },
                )
            })
            .collect()
    }

    /// Records one reduction per level: level 1 from `depth` — the early
    /// prepass — and every level after from the one above.
    ///
    /// `levels` are [`import`](Self::import)'s ids for this frame. Marks the
    /// chain as holding a frame's depth, which is what the next frame's first
    /// phase asks before it trusts it.
    ///
    /// # Panics
    ///
    /// If `levels` is not the chain [`prepare`](Self::prepare) built.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        depth: ImageId,
        levels: &[ImageId],
    ) {
        let Some(chain) = &self.chain else {
            return;
        };
        assert_eq!(
            levels.len(),
            chain.levels.len(),
            "the caller imported a different pyramid than the one prepared"
        );
        self.history = true;
        let layout = self.layout;
        let pipeline_layout = self.pipeline_layout;
        let pipeline = self.pipeline;
        let first = &mut self.first_groups[frame];
        let mut reduce = chain.reduce_groups.iter().copied();
        let mut first = Some(first);
        for (index, &target) in levels.iter().enumerate() {
            let source = if index == 0 { depth } else { levels[index - 1] };
            let pass = graph
                .add_render_pass(format!("occlusion-hiz-{}", index + 1))
                // `DontCare`: the full-screen triangle writes every texel.
                .depth(
                    target,
                    LoadOp::DontCare,
                    StoreOp::Store,
                    ClearValue::default(),
                )
                .read_image(source);
            if index == 0 {
                let cached = first
                    .take()
                    .unwrap_or_else(|| unreachable!("level 1 is reduced once"));
                pass.execute(move |ctx| {
                    let view = ctx.image_view(source);
                    let device = ctx.device();
                    let entries = vec![BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(view),
                    }];
                    let Some(group) = cached_group(
                        cached,
                        device,
                        &[(0, view)],
                        "occlusion hiz",
                        layout,
                        entries,
                    ) else {
                        return;
                    };
                    let encoder = ctx.encoder();
                    encoder.bind_graphics_pipeline(pipeline);
                    encoder.bind_group(0, group, &[], pipeline_layout);
                    encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
                });
            } else {
                let group = reduce
                    .next()
                    .unwrap_or_else(|| unreachable!("a group per level below the first"));
                pass.execute(move |ctx| {
                    let encoder = ctx.encoder();
                    encoder.bind_graphics_pipeline(pipeline);
                    encoder.bind_group(0, group, &[], pipeline_layout);
                    encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
                });
            }
        }
    }

    /// The level images and their extents, level 1 first, for a test reading one
    /// back.
    pub(crate) fn level_images(&self) -> Vec<(ImageHandle, ImageViewHandle, (u32, u32))> {
        self.chain.as_ref().map_or_else(Vec::new, |chain| {
            chain
                .levels
                .iter()
                .zip(1..)
                .map(|(level, index)| (level.image, level.view, level_extent(chain.extent, index)))
                .collect()
        })
    }

    /// Releases everything [`OcclusionPyramid::new`] and
    /// [`prepare`](Self::prepare) created.
    pub(crate) fn destroy(self, device: &dyn Device) {
        if let Some(chain) = self.chain {
            destroy_chain(device, chain);
        }
        for cached in self.first_groups.into_iter().flatten() {
            device.destroy_bind_group(cached.1);
        }
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
    }
}

/// Releases one chain's groups, views and images.
fn destroy_chain(device: &dyn Device, chain: Chain) {
    device.destroy_bind_group(chain.cull_group);
    for group in chain.reduce_groups {
        device.destroy_bind_group(group);
    }
    for level in chain.levels {
        device.destroy_image_view(level.view);
        device.destroy_image(level.image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Levels halve until an axis would reach zero texels, or the binding
    /// count runs out** — the two floors `cull.slang`'s eight bindings and a
    /// one-row target set.
    #[test]
    fn the_pyramid_stops_at_one_texel_or_at_the_bindings() {
        assert_eq!(levels_for((1, 1)), 0, "nothing to halve");
        assert_eq!(
            levels_for((256, 1)),
            0,
            "a one-row target halves to nothing"
        );
        assert_eq!(levels_for((2, 2)), 1);
        // The golden suite's extents, which is what says the numbers are about
        // frames this tree renders: 256×192 halves seven times to 2×1.
        assert_eq!(levels_for((256, 192)), 7);
        assert_eq!(level_extent((256, 192), 7), (2, 1));
        assert_eq!(levels_for((97, 61)), 5);
        assert_eq!(level_extent((97, 61), 5), (3, 1));
        // A real window, where the binding count is what stops it.
        assert_eq!(levels_for((1920, 1080)), OCCLUSION_MAX_LEVELS);
        assert_eq!(level_extent((1920, 1080), OCCLUSION_MAX_LEVELS), (7, 4));
    }

    /// **The whole chain is about a third of the prepass**, which is the
    /// memory figure the module docs quote, at the extent they quote it for.
    #[test]
    fn the_pyramid_costs_a_third_of_the_prepass() {
        let extent = (1920, 1080);
        let prepass = u64::from(extent.0) * u64::from(extent.1);
        let pyramid: u64 = (1..=levels_for(extent))
            .map(|level| {
                let (width, height) = level_extent(extent, level);
                u64::from(width) * u64::from(height)
            })
            .sum();
        // Four bytes a `D32Float` texel.
        assert_eq!(pyramid * 4, 2_764_192, "2.64 MiB of level texels");
        assert!(
            pyramid * 3 <= prepass && pyramid * 3 + prepass / 100 >= prepass,
            "a third of the prepass's {prepass} texels, to within one per cent: {pyramid}"
        );
    }
}
