//! `docs/plan/18-render-features.md`'s Hi-Z pyramid: the depth reduction
//! `crate::ssr`'s march climbs instead of stepping at a fixed stride.
//!
//! ```text
//! depth-prepass ──▶ hiz-1 ──▶ hiz-2 ──▶ … ──▶ hiz-N
//!       │             │         │              │
//!       └─────────────┴─────────┴──────────────┴──▶ ssr   (level 0 … level N)
//! ```
//!
//! A module of its own rather than more of [`crate::forward`], on
//! `crate::ssao`'s terms exactly: what lives here is one pipeline, one cache per
//! level and the chain, and none of it is reachable from the geometry passes
//! except through the recorder this module keeps to itself.
//!
//! Only the *shape* of the pyramid is public — [`levels_for`] and
//! [`level_extent`] — because a caller budgeting passes or GPU timer slots has
//! to be able to ask how many levels a frame at a given size will have, and that
//! answer is not derivable from anything else it can see.
//!
//! # Each level is the nearest surface of the block below it
//!
//! Under reversed-Z nearer is larger, so the reduction is a `max` and the far
//! plane's zero loses every comparison — see `shaders/hiz.slang`, which carries
//! the whole of what a level means. What this module owns is the *shape*: how
//! many levels a frame has, how big each one is, and which image feeds which.
//!
//! # The chain is N single-level images, not one image with mips
//!
//! `crate::bloom`'s header gives the two independent reasons and both apply
//! here unchanged: the render graph cannot attach one mip of an image, and
//! WebGPU requires a render-attachment view to be exactly one level. So a level
//! is its own description at its own extent, the pool hands out a physical image
//! for each, and every pass gets a correct render area.
//!
//! # Depth images, not colour ones
//!
//! Every level is `D32Float`, written through `SV_Depth` with the comparison
//! set to [`CompareOp::Always`] and writes on. `shaders/hiz.slang`'s header
//! gives the reason in full — it keeps the pyramid the same texture type as the
//! prepass at level 0, so the march reads all six bindings through one
//! `DepthTexture2D` spelling rather than binding an unfilterable `R32Float`
//! beside a depth one.
//!
//! # The off-switch
//!
//! A frame that does not add these passes leaves `crate::ssr::Ssr`'s pyramid
//! bindings filled with the prepass itself and its level count at zero, and the
//! march never leaves level 0, which is the fixed-stride walk this replaced. So
//! the pyramid is an optimisation the reflection pass can be built without,
//! rather than a resource it requires.
//!
//! [`CompareOp::Always`]: crcbl_hal::CompareOp

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, ClearValue, Device, Format,
    GraphicsPipelineHandle, HalError, ImageViewHandle, ImageViewType, LoadOp, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::HIZ;

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;

/// The vertex count of the full-screen triangle every pass here draws.
const FULLSCREEN_VERTICES: u32 = 3;

/// The deepest level the pyramid is built to, and the number of pyramid
/// bindings `ssr.slang` declares.
///
/// Five, so the coarsest cell is 32 texels across. `ssr.slang`'s `MAX_STEPS`
/// buys three thousand texels of reach at that size, which is more than any
/// frame this engine renders is wide, and a sixth level would be another
/// binding, another pass and another image for reach nothing can use.
/// `ssr.slang`'s `MAX_HIZ_LEVEL` is the shader mirror.
pub(crate) const MAX_LEVELS: u32 = 5;

/// The smallest a level may be on either axis, in texels.
///
/// Eight, [`crate::bloom::MIN_MIP_EXTENT`]'s value, and for a related reason: a
/// level of four texels is a level where one cell is a quarter of the screen, so
/// a ray that climbs to it is a ray that has stopped asking a screen-space
/// question. It also keeps the smallest level of a 16:9 frame from collapsing to
/// a single row.
const MIN_LEVEL_EXTENT: u32 = 8;

/// How many levels the pyramid has at `extent`, and therefore how many passes it
/// records.
///
/// Zero for a target too small to halve once, which is a frame the march walks
/// at full resolution.
pub fn levels_for(extent: (u32, u32)) -> u32 {
    let (mut width, mut height) = extent;
    let mut levels = 0;
    while levels < MAX_LEVELS {
        let (next_width, next_height) = (width / 2, height / 2);
        if next_width < MIN_LEVEL_EXTENT || next_height < MIN_LEVEL_EXTENT {
            break;
        }
        width = next_width;
        height = next_height;
        levels += 1;
    }
    levels
}

/// The extent of level `level` of a pyramid over `extent`, where level 0 is the
/// prepass itself.
///
/// **Halving floors**, which is what `shaders/hiz.slang`'s odd-extent taps
/// exist to cover: a level whose source had an odd axis reaches one row or
/// column short, and the reduction takes a third tap along it rather than
/// letting the pyramid claim that strip is empty.
///
/// # Panics
///
/// If `level` is past the pyramid [`levels_for`] allows, which is a caller
/// asking about a chain other than the one it built.
pub fn level_extent(extent: (u32, u32), level: u32) -> (u32, u32) {
    assert!(
        level <= levels_for(extent),
        "level {level} is past the {} this pyramid has at {extent:?}",
        levels_for(extent)
    );
    let (mut width, mut height) = extent;
    for _ in 0..level {
        width /= 2;
        height /= 2;
    }
    (width, height)
}

/// One frame's reduction groups, one slot per level, each cached against the
/// source view it names.
///
/// A name of its own because the field below is a ring *of* these — one row per
/// frame in flight — and the nesting is what makes the type unreadable written
/// out.
type LevelGroups = Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>;

/// Everything the pyramid owns.
///
/// Built once by [`Hiz::new`] and released by [`Hiz::destroy`], which is the
/// shape every other resource group in this crate has — see [`crate::ssao`].
#[derive(Debug)]
pub(crate) struct Hiz {
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame][level]`: the reduction's group, cached against its source view.
    ///
    /// One row per frame in flight and one slot per level, because a level's
    /// source is the level above it and those are transients: the pool may hand
    /// the same description a different image from one frame to the next, and a
    /// single cache would then hold a view of an image this frame is not using.
    groups: Vec<LevelGroups>,
}

impl Hiz {
    /// Passes [`Hiz::add_passes`] adds to a frame, at most.
    pub(crate) const PASSES: u32 = MAX_LEVELS;

    /// Builds the reduction pipeline and the per-level caches.
    ///
    /// `build_depth_fullscreen` is handed in rather than duplicated, on
    /// [`crate::ssao::Ssao::new`]'s terms: it is [`crate::forward`]'s, because
    /// the shape it carries — a triangle out of `SV_VertexID`, no colour target
    /// and a depth attachment written with the comparison always passing — is
    /// documented there.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the
    /// caller holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        build_depth_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            Format,
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        // One binding and no sampler: the reduction fetches by integer texel,
        // which is what keeps `max` the same number on all four rasterisers —
        // see `shaders/hiz.slang`.
        let entries = [BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // `Depth`, and it is the seam's half of `DepthTexture2D` —
                // `crate::ssao`'s own depth binding carries the paragraph. The
                // source is the prepass for level 1 and a level of this pyramid
                // for every level after it, and both are `D32Float`.
                sample_type: SampleType::Depth,
            },
            count: 1,
            flags: BindingFlags::empty(),
        }];
        let desc = BindGroupLayoutDesc {
            label: Some("hiz source"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("hiz"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("hiz"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;
        let pipeline =
            build_depth_fullscreen(device, "hiz", &HIZ, pipeline_layout, Format::D32Float)?;

        Ok(Self {
            layout,
            pipeline_layout,
            pipeline,
            groups: vec![vec![None; MAX_LEVELS as usize]; frames],
        })
    }

    /// Records one reduction pass per level of `levels`.
    ///
    /// `depth` is the prepass, which is level 0 and the source of the first
    /// reduction; `levels` are the images the caller created for levels 1
    /// upwards, in order.
    ///
    /// # Panics
    ///
    /// If `levels` is not the chain [`levels_for`] describes at `extent`, which
    /// is a caller that created a different pyramid than the one it is asking
    /// for passes over.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        extent: (u32, u32),
        depth: ImageId,
        levels: &[ImageId],
    ) {
        let count = levels_for(extent);
        assert_eq!(
            levels.len(),
            count as usize,
            "the caller created {} pyramid images for an extent whose pyramid is {count} levels",
            levels.len()
        );

        let layout = self.layout;
        let pipeline_layout = self.pipeline_layout;
        let pipeline = self.pipeline;
        let mut cache = self.groups[frame].iter_mut();

        for level in 1..=count {
            let source = if level == 1 {
                depth
            } else {
                levels[level as usize - 2]
            };
            let target = levels[level as usize - 1];
            let cached = cache
                .next()
                .unwrap_or_else(|| unreachable!("MAX_LEVELS slots and at most MAX_LEVELS levels"));
            graph
                .add_render_pass(format!("hiz-{level}"))
                // `DontCare`, not `Clear`: the full-screen triangle writes every
                // texel of the target, so loading or clearing it is bandwidth
                // spent on values the pass is about to overwrite.
                .depth(
                    target,
                    LoadOp::DontCare,
                    StoreOp::Store,
                    ClearValue::default(),
                )
                // The prepass left level 0 in `DepthStencilWrite` and the pass
                // before this one left its own target there too. Declaring the
                // read is what moves each into a shader-readable layout.
                .read_image(source)
                .execute(move |ctx| {
                    let view = ctx.image_view(source);
                    let device = ctx.device();
                    let entries = vec![BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(view),
                    }];
                    let Some(group) =
                        cached_group(cached, device, &[(0, view)], "hiz", layout, entries)
                    else {
                        return;
                    };
                    let encoder = ctx.encoder();
                    encoder.bind_graphics_pipeline(pipeline);
                    encoder.bind_group(0, group, &[], pipeline_layout);
                    encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
                });
        }
    }

    /// Releases everything [`Hiz::new`] created.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self.groups.into_iter().flatten().flatten() {
            device.destroy_bind_group(cached.1);
        }
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
    }
}

/// The [`MAX_LEVELS`] slots `ssr.slang`'s pyramid bindings take, given the
/// levels this frame actually has.
///
/// **Every slot is always filled**, and a frame with fewer levels than bindings
/// repeats its deepest one into the slots above. The march never reads them —
/// `SsrParams::hiz_levels` bounds the level it may climb to — but a bind group
/// with a hole in it is not something any backend will create, and a frame with
/// no pyramid at all still has to name five images. That case takes `base`, the
/// prepass, which is level 0 and is named at every other slot of the group
/// already.
///
/// Generic because the caller fills the slots with [`ImageId`]s and the pass
/// body binds [`ImageViewHandle`]s, and repeating the deepest level is the same
/// arithmetic either side of that.
pub(crate) fn level_slots<T: Copy>(base: T, levels: &[T]) -> [T; MAX_LEVELS as usize] {
    let deepest = levels.last().copied().unwrap_or(base);
    let mut slots = [deepest; MAX_LEVELS as usize];
    for (slot, level) in slots.iter_mut().zip(levels) {
        *slot = *level;
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The pyramid stops before a level is smaller than a cell means
    /// anything**, and it stops at [`MAX_LEVELS`] however large the frame is.
    #[test]
    fn the_pyramid_stops_at_the_floor_and_at_the_ceiling() {
        // Under twice the floor on either axis: nothing to halve, and the march
        // walks the prepass.
        assert_eq!(levels_for((15, 192)), 0);
        assert_eq!(levels_for((256, 15)), 0);
        // The golden suite's two extents, which is what says the assertion above
        // is about frames this tree actually renders.
        assert_eq!(levels_for((256, 192)), 4);
        assert_eq!(levels_for((97, 61)), 2);
        // And a real window, where the ceiling is what stops it rather than the
        // floor: 1080 halves seven times before reaching eight.
        assert_eq!(levels_for((1920, 1080)), MAX_LEVELS);
    }

    /// Every level is the floor of half the level above it, which is the
    /// halving `shaders/hiz.slang`'s odd-extent taps are written against.
    #[test]
    fn every_level_is_the_floor_of_half_the_level_above_it() {
        let extent = (97, 61);
        assert_eq!(level_extent(extent, 0), extent);
        for level in 1..=levels_for(extent) {
            let (width, height) = level_extent(extent, level - 1);
            assert_eq!(level_extent(extent, level), (width / 2, height / 2));
        }
    }

    /// **A frame with fewer levels than bindings fills every slot**, which is
    /// what stops a bind group being built with a hole in it.
    #[test]
    fn the_binding_slots_are_all_filled_however_short_the_pyramid_is() {
        // Handles built from bits rather than from a device: this function is
        // arithmetic on a slice and has no seam in it.
        let view = |bits: u64| ImageViewHandle::from_bits(bits).expect("a non-zero generation");
        let depth = view(1 << 32);
        let one = view((1 << 32) | 1);
        let two = view((1 << 32) | 2);

        // No pyramid: every slot is the prepass, which the march never reads.
        assert_eq!(level_slots(depth, &[]), [depth; MAX_LEVELS as usize]);

        // Two levels: the two it has, and its deepest repeated above them.
        assert_eq!(level_slots(depth, &[one, two]), [one, two, two, two, two]);
    }
}
