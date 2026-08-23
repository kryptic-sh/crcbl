//! `docs/plan/18-render-features.md`'s bloom: the threshold-free down/upsample
//! chain between the reflection composite and the tonemap.
//!
//! ```text
//! scene ──▶ bloom-down-1 ──▶ bloom-down-2 ──▶ … ──▶ bloom-down-N
//!   │            ▲                ▲                      │
//!   │            └── bloom-up-1 ◀─┴── bloom-up-… ◀────────┘   (additive blend)
//!   │                  │
//!   └──────────────▶ bloom-composite ──▶ bloom-color ──▶ tonemap
//! ```
//!
//! A module of its own rather than more of [`crate::forward`], on
//! [`crate::ssao`]'s terms exactly: what lives here is three pipelines, three
//! caches, a sampler and the chain, and none of it is reachable from the
//! geometry passes except through [`Bloom::add_passes`].
//!
//! # The chain is N single-mip images, not one image with N mips
//!
//! Every article about bloom describes one image with a mip chain, and this is
//! not that. Two independent reasons, either of which decides it:
//!
//! * **The render graph cannot attach a mip.**
//!   [`PassBuilder::color`](crate::graph::PassBuilder::color) passes `range:
//!   None`, a realised transient holds exactly one view, and a pass's render
//!   area is the description's own extent — so "mip 3 of this image, at a
//!   quarter the width" is not something a pass can say without surgery on the
//!   graph.
//! * **WebGPU requires a render-attachment view to be exactly one mip level**,
//!   so per-mip views would be mandatory there whatever this crate did.
//!
//! N distinct descriptions at N distinct extents are N distinct physical images
//! to the pool, each with a correct render area, and `read_image`/`color` work
//! unchanged. What it costs is that the pool cannot alias two levels of one
//! chain, which it could not do for a mip chain either.
//!
//! # How long the chain is
//!
//! Topic 18 asks for five or six mips. That is a statement about a 1080p-ish
//! frame rather than a constant: halving a 97×61 offscreen six times reaches
//! zero, and a zero-extent image is not a resource any backend will create. So
//! the count is **derived from the extent** — halve while both axes stay at
//! [`MIN_MIP_EXTENT`] or above, and stop at [`MAX_MIPS`] — which gives six at
//! 1080p and 1440p, four at the golden suite's 256×192, and three at its awkward
//! 97×61.
//!
//! **Halving floors.** An odd extent loses its last half-texel column to the
//! rounding, and that is deliberate rather than tolerated: every tap offset in
//! the chain is a whole number of *source* texels scaled by the source's own
//! `1 / extent`, so a step that reduces 97 to 48 samples a footprint 2/97 wider
//! than the exact one and nothing else moves. Rounding up instead would leave
//! the last destination texel reading past the source's edge, where clamping
//! duplicates the border into the halo.
//!
//! A target too small for even one level — under `2 ·`[`MIN_MIP_EXTENT`] on
//! either axis — gets **no chain and no passes at all**, and the caller
//! tonemaps the scene colour unchanged. That is the same off-switch the effect
//! toggle uses, arrived at from the other side.
//!
//! # The off-switch
//!
//! A frame that does not add these passes hands the scene colour on and is
//! bit-identical, needing no placeholder because nothing upstream reads the
//! result — [`crate::ssr`]'s off-switch exactly.
//!
//! There is no device fact to gate on. Every backend has a full-screen draw, a
//! sampled `Rgba16Float`, and additive blending on it: `Rgba16Float` carries
//! `COLOR_ATTACHMENT_BLEND` in Vulkan's mandatory format table, and is
//! `blendable` in WebGPU, Metal and D3D12 alike.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendFactor, BlendOp,
    BlendState, BufferDesc, BufferHandle, BufferUsage, ClearValue, ColorTargetState, ColorWrites,
    Device, FilterMode, Format, GraphicsPipelineHandle, HalError, ImageViewHandle, ImageViewType,
    LoadOp, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, SampleType,
    SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderStages, StoreOp,
    check_portable_storage_buffers,
};
use crcbl_shaders::{BLOOM_COMPOSITE, BLOOM_DOWN, BLOOM_UP, bloom};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;

/// Vertices in the over-sized full-screen triangle every `bloom_*.slang`
/// generates from `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// One slot of a [`cached_group`] cache: the group, and every view it was built
/// against.
///
/// A name rather than the shape written out, because the chain keeps a slot per
/// level per frame in flight and two levels of [`Vec`] around it is a type
/// `clippy::type_complexity` refuses — reasonably, since the shape says nothing
/// the name does not.
type GroupCache = Option<(Vec<ImageViewHandle>, BindGroupHandle)>;

/// The longest chain, in levels below the scene target.
///
/// Topic 18's "5–6 mips", capped at the top of that range. A seventh level of a
/// 4K frame would be sixteen texels across and contribute a term that is
/// indistinguishable from a constant added to the whole image, for another pair
/// of passes.
pub(crate) const MAX_MIPS: u32 = 6;

/// The smallest a level of the chain may be on either axis, in texels.
///
/// `bloom_down.slang` reaches two source texels out and `bloom_up.slang` one, so
/// a level below this is a level where the filters' footprints are most of the
/// image and every tap is clamped against its border. Eight also keeps the
/// smallest level of a 16:9 frame from collapsing to a single row.
pub(crate) const MIN_MIP_EXTENT: u32 = 8;

/// The three colour targets the chain writes: `Rgba16Float`, the scene target's
/// format.
///
/// Not a narrower one, and for the reason `crate::ssr` gives about its own
/// march: the chain carries the scene's bright end, which is the part of it that
/// blooms, and an eight-bit level would clip exactly that before the tonemap saw
/// it.
const CHAIN_FORMAT: Format = Format::Rgba16Float;

/// How many levels the chain has at `extent`, and therefore how many downsample
/// passes it records.
///
/// Zero for a target too small to halve once — see the module docs.
pub(crate) fn mips_for(extent: (u32, u32)) -> u32 {
    let (mut width, mut height) = extent;
    let mut mips = 0;
    while mips < MAX_MIPS {
        let (next_width, next_height) = (width / 2, height / 2);
        if next_width < MIN_MIP_EXTENT || next_height < MIN_MIP_EXTENT {
            break;
        }
        width = next_width;
        height = next_height;
        mips += 1;
    }
    mips
}

/// The extent of level `mip` of a chain over `extent`, where level 0 is the
/// scene target itself.
///
/// # Panics
///
/// If `mip` is past the chain [`mips_for`] allows, which is a caller that built
/// a different chain than the one it is asking about.
pub(crate) fn mip_extent(extent: (u32, u32), mip: u32) -> (u32, u32) {
    assert!(
        mip <= mips_for(extent),
        "level {mip} is past the {} this chain has at {extent:?}",
        mips_for(extent)
    );
    let (mut width, mut height) = extent;
    for _ in 0..mip {
        width /= 2;
        height /= 2;
    }
    (width, height)
}

/// Everything the chain owns.
///
/// Built once by [`Bloom::new`] and released by [`Bloom::destroy`], which is the
/// shape every other resource group in this crate has — see [`crate::ssao`].
#[derive(Debug)]
pub(crate) struct Bloom {
    /// **Linear** min/mag/mip, clamped to the edge, and its own object rather
    /// than [`crate::forward`]'s.
    ///
    /// That one is `Nearest` on purpose — `tonemap.slang` says why a 1:1 blit
    /// must not depend on texel-centre arithmetic — and every tap in this chain
    /// is a genuine magnification or minification between two extents, where
    /// nearest would alias the pyramid into blocks. The composite's read of the
    /// *scene* is the one 1:1 fetch here, and `bloom_composite.slang` does it
    /// with a `Load` and no sampler at all.
    sampler: SamplerHandle,
    /// One source image, one sampler, one block — shared by the downsample and
    /// the upsample, which differ in their pipeline and in nothing else.
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    down_pipeline: GraphicsPipelineHandle,
    /// The tent, with **additive blending**: the destination keeps what the
    /// downsample wrote and this pass adds its level on top. See
    /// `bloom_up.slang`.
    up_pipeline: GraphicsPipelineHandle,
    composite_layout: BindGroupLayoutHandle,
    composite_pipeline_layout: PipelineLayoutHandle,
    composite_pipeline: GraphicsPipelineHandle,
    /// `[frame * MAX_PASSES + step]`: one [`bloom::BloomParams`] row per step of
    /// the chain, per frame in flight.
    ///
    /// **A buffer per step and not one block for the pass**, because every step
    /// reads a different extent — and a ring on top of that for the frame
    /// uniforms' reason exactly: the previous frame may still be reading last
    /// frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    /// `[frame][mip - 1]`: the group the pass writing level `mip` binds.
    ///
    /// Three caches rather than one indexed by step, because a step's *index*
    /// changes role with the extent — step 4 is a downsample on a long chain and
    /// an upsample on a short one — and a cache keyed on the view alone would
    /// then be able to hand a group built against one layout to a pass expecting
    /// the other.
    down_groups: Vec<Vec<GroupCache>>,
    /// `[frame][mip - 1]`: the group the pass adding into level `mip` binds.
    up_groups: Vec<Vec<GroupCache>>,
    /// `[frame]`: the composite's group, cached against the scene view and mip
    /// 1's together.
    composite_groups: Vec<GroupCache>,
}

impl Bloom {
    /// The most passes [`Bloom::add_passes`] can add to a frame: a downsample
    /// per level, an upsample per level but the last, and the composite.
    ///
    /// A **ceiling**, unlike [`crate::ssao::Ssao::PASSES`], because the chain's
    /// length is a function of the extent and [`ForwardRenderer::MAX_PASSES`] is
    /// a constant a caller sizes query sets with before any extent is known.
    /// [`Bloom::passes_for`] is what a frame actually records.
    ///
    /// [`ForwardRenderer::MAX_PASSES`]: crate::ForwardRenderer::MAX_PASSES
    pub(crate) const MAX_PASSES: u32 = 2 * MAX_MIPS;

    /// How many passes a frame drawing the chain at `extent` records.
    ///
    /// `2 · mips`: the downsamples, one fewer upsample, and the composite. Zero
    /// where the target is too small for a level at all.
    pub(crate) fn passes_for(extent: (u32, u32)) -> u32 {
        match mips_for(extent) {
            0 => 0,
            mips => 2 * mips,
        }
    }

    /// Builds the three pipelines, the sampler and the uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated, on
    /// [`Ssao::new`](crate::ssao::Ssao::new)'s terms: it is [`crate::forward`]'s,
    /// because the shape it carries — a triangle out of `SV_VertexID`, no depth
    /// state, one colour target — is documented there.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the
    /// caller holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        build_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        // **Linear, unlike every other sampler this crate creates.** See the
        // field it is stored in.
        let sampler = device.create_sampler(&SamplerDesc {
            label: Some("bloom chain"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Linear,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;

        let entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // `Float`: the chain is `Rgba16Float` colour throughout and
                    // the WGSL artifacts declare `texture_2d<f32>` for it.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let desc = BindGroupLayoutDesc {
            label: Some("bloom chain"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("bloom chain"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("bloom chain"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;

        // The same shape with the scene target in front of it: the composite is
        // the last upsample and the add in one pass, so it names both images.
        let composite_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let composite_desc = BindGroupLayoutDesc {
            label: Some("bloom composite"),
            entries: &composite_entries,
        };
        check_portable_storage_buffers(Some("bloom composite"), &[&composite_desc])?;
        let composite_layout = device.create_bind_group_layout(&composite_desc)?;
        let composite_set_layouts = [composite_layout];
        let composite_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("bloom composite"),
            bind_group_layouts: &composite_set_layouts,
            push_constants: None,
        })?;

        let opaque = [ColorTargetState::opaque(CHAIN_FORMAT)];
        // **`One`/`One` on colour, `Zero`/`One` on alpha**: the upsample adds its
        // level to what the downsample already wrote, and leaves the alpha
        // alone. An attachment a fragment stage both sampled and wrote would be
        // undefined on every backend here, so the add has to be the blender's.
        let additive = [ColorTargetState {
            format: CHAIN_FORMAT,
            blend: Some(BlendState {
                color_src: BlendFactor::One,
                color_dst: BlendFactor::One,
                color_op: BlendOp::Add,
                alpha_src: BlendFactor::Zero,
                alpha_dst: BlendFactor::One,
                alpha_op: BlendOp::Add,
            }),
            write_mask: ColorWrites::ALL,
        }];

        let down_pipeline =
            build_fullscreen(device, "bloom down", &BLOOM_DOWN, pipeline_layout, &opaque)?;
        let up_pipeline =
            build_fullscreen(device, "bloom up", &BLOOM_UP, pipeline_layout, &additive)?;
        let composite_pipeline = build_fullscreen(
            device,
            "bloom composite",
            &BLOOM_COMPOSITE,
            composite_pipeline_layout,
            &opaque,
        )?;

        let mut uniforms = Vec::with_capacity(frames * Self::MAX_PASSES as usize);
        for _ in 0..frames * Self::MAX_PASSES as usize {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("bloom params"),
                size: bloom::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?);
        }

        Ok(Self {
            sampler,
            layout,
            pipeline_layout,
            down_pipeline,
            up_pipeline,
            composite_layout,
            composite_pipeline_layout,
            composite_pipeline,
            uniforms,
            down_groups: vec![vec![None; MAX_MIPS as usize]; frames],
            up_groups: vec![vec![None; MAX_MIPS as usize]; frames],
            composite_groups: vec![None; frames],
        })
    }

    /// Writes `frame`'s block for every step of the chain at `extent`.
    ///
    /// Written here rather than in [`Bloom::add_passes`] for every other block's
    /// reason: a pass body runs at execute time, and the buffer it reads has to
    /// have been written before the frame was submitted.
    ///
    /// **Unconditional on the effect toggle**, exactly as the occlusion and
    /// reflection blocks are: a chain of at most twelve sixteen-byte rows is
    /// cheaper to write than to decide about, and a block written for a pass
    /// that was not added is read by nobody.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any of the mapped writes.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn begin_frame(
        &self,
        device: &dyn Device,
        frame: usize,
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        let mips = mips_for(extent);
        for step in 0..Self::passes_for(extent) {
            // Which image this step reads, and what it needs from the block.
            let (source, karis, strength) = if step < mips {
                // Downsample: level `step + 1` out of level `step`. The Karis
                // average is the first one's alone — see `bloom_down.slang`.
                let karis = if step == 0 {
                    bloom::KARIS_ON
                } else {
                    bloom::KARIS_OFF
                };
                (step, karis, 0.0)
            } else if step < 2 * mips - 1 {
                // Upsample: level `mips - (step - mips) - 1` out of the level
                // below it. Walking the pyramid back down, smallest first.
                (2 * mips - step, bloom::KARIS_OFF, 0.0)
            } else {
                // The composite, which reads level 1 and is the only step that
                // carries the scalar.
                (1, bloom::KARIS_OFF, bloom::DEFAULT_STRENGTH)
            };
            let (width, height) = mip_extent(extent, source);
            let params = bloom::BloomParams {
                inv_source: [1.0 / width as f32, 1.0 / height as f32],
                karis,
                strength,
            };
            device.write_buffer(
                self.uniforms[frame * Self::MAX_PASSES as usize + step as usize],
                0,
                &params.to_bytes(),
            )?;
        }
        Ok(())
    }

    /// Adds the whole chain: `bloom-down-1` … `bloom-down-N`, `bloom-up-N-1` …
    /// `bloom-up-1`, then `bloom-composite`.
    ///
    /// `scene` is what the tonemap would otherwise have read, `mips` is the
    /// chain's levels in order — `mips[0]` is level 1, at half `scene`'s extent
    /// — and `composited` is what the caller must go on to tonemap. Every image
    /// is the caller's so it can declare the read on the pass that consumes the
    /// last one; a pass declares its own accesses and this one cannot declare
    /// the tonemap's.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with, or if `mips` is not the
    /// chain [`mips_for`] describes for `extent` — which is a caller that
    /// created a different set of images than the one the blocks were written
    /// for.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        extent: (u32, u32),
        scene: ImageId,
        mips: &[ImageId],
        composited: ImageId,
    ) {
        let count = mips_for(extent);
        assert_eq!(
            mips.len(),
            count as usize,
            "the caller created {} chain images for an extent whose chain is {count} levels",
            mips.len()
        );
        assert!(count > 0, "a chain with no levels records no passes");

        let base = frame * Self::MAX_PASSES as usize;
        let layout = self.layout;
        let pipeline_layout = self.pipeline_layout;
        let down_pipeline = self.down_pipeline;
        let up_pipeline = self.up_pipeline;
        let sampler = self.sampler;
        // Split so each closure below borrows a different piece of `self`; one
        // `&mut self` shared between them is what the borrow checker refuses,
        // and it would be refusing something genuinely wrong — a pass body may
        // run at any point after it is declared.
        let mut down_cache = self.down_groups[frame].iter_mut();
        let mut up_cache = self.up_groups[frame].iter_mut();
        let composite_cache = &mut self.composite_groups[frame];

        // --- down: level 0 (the scene) into level 1, then each level into the
        // next. `DontCare` on every target, because the full-screen triangle
        // writes every pixel of it.
        for level in 1..=count {
            let source = if level == 1 {
                scene
            } else {
                mips[level as usize - 2]
            };
            let target = mips[level as usize - 1];
            let uniforms = self.uniforms[base + level as usize - 1];
            let cached = down_cache
                .next()
                .unwrap_or_else(|| unreachable!("MAX_MIPS slots and at most MAX_MIPS levels"));
            graph
                .add_render_pass(format!("bloom-down-{level}"))
                .color(
                    target,
                    LoadOp::DontCare,
                    StoreOp::Store,
                    ClearValue::default(),
                )
                .read_image(source)
                .execute(move |ctx| {
                    let view = ctx.image_view(source);
                    let device = ctx.device();
                    let entries = chain_entries(view, sampler, uniforms);
                    let Some(group) =
                        cached_group(cached, device, &[(0, view)], "bloom down", layout, entries)
                    else {
                        return;
                    };
                    let encoder = ctx.encoder();
                    encoder.bind_graphics_pipeline(down_pipeline);
                    encoder.bind_group(0, group, &[], pipeline_layout);
                    encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
                });
        }

        // --- up: the smallest level into the one above it, and so on down to
        // level 1, which ends up holding the sum of the whole pyramid.
        //
        // `LoadOp::Load` and an additive pipeline, so what the downsample wrote
        // into the target survives and this adds to it. `StoreOp::Store`
        // because the next step up reads it.
        for level in (1..count).rev() {
            let source = mips[level as usize];
            let target = mips[level as usize - 1];
            let uniforms = self.uniforms[base + (2 * count - 1 - level) as usize];
            let cached = up_cache
                .next()
                .unwrap_or_else(|| unreachable!("MAX_MIPS slots and at most MAX_MIPS-1 steps"));
            graph
                .add_render_pass(format!("bloom-up-{level}"))
                .color(target, LoadOp::Load, StoreOp::Store, ClearValue::default())
                .read_image(source)
                .execute(move |ctx| {
                    let view = ctx.image_view(source);
                    let device = ctx.device();
                    let entries = chain_entries(view, sampler, uniforms);
                    let Some(group) =
                        cached_group(cached, device, &[(0, view)], "bloom up", layout, entries)
                    else {
                        return;
                    };
                    let encoder = ctx.encoder();
                    encoder.bind_graphics_pipeline(up_pipeline);
                    encoder.bind_group(0, group, &[], pipeline_layout);
                    encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
                });
        }

        // --- composite: the last tent and the add, into an image of its own.
        let composite_layout = self.composite_layout;
        let composite_pipeline_layout = self.composite_pipeline_layout;
        let composite_pipeline = self.composite_pipeline;
        let level_one = mips[0];
        let uniforms = self.uniforms[base + (2 * count - 1) as usize];
        graph
            .add_render_pass("bloom-composite")
            .color(
                composited,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .read_image(scene)
            .read_image(level_one)
            .execute(move |ctx| {
                let scene_view = ctx.image_view(scene);
                let bloom_view = ctx.image_view(level_one);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        // Overwritten by `cached_group` with the realised view,
                        // as is the level below; written here so the list is a
                        // complete description of the layout rather than a list
                        // with two holes in it.
                        resource: BindingResource::ImageView(scene_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::ImageView(bloom_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::Sampler(sampler),
                    },
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                ];
                let Some(group) = cached_group(
                    composite_cache,
                    device,
                    &[(0, scene_view), (1, bloom_view)],
                    "bloom composite",
                    composite_layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(composite_pipeline);
                encoder.bind_group(0, group, &[], composite_pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self
            .down_groups
            .into_iter()
            .chain(self.up_groups)
            .flatten()
            .flatten()
            .chain(self.composite_groups.into_iter().flatten())
        {
            device.destroy_bind_group(cached.1);
        }
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        device.destroy_graphics_pipeline(self.composite_pipeline);
        device.destroy_pipeline_layout(self.composite_pipeline_layout);
        device.destroy_bind_group_layout(self.composite_layout);
        device.destroy_graphics_pipeline(self.up_pipeline);
        device.destroy_graphics_pipeline(self.down_pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
        device.destroy_sampler(self.sampler);
    }
}

/// The down and up passes' group, as a complete description of their shared
/// layout.
///
/// `view` is overwritten by [`cached_group`] with the realised one; it is
/// written here so the list is a complete description rather than a list with a
/// hole in it.
fn chain_entries(
    view: ImageViewHandle,
    sampler: SamplerHandle,
    uniforms: BufferHandle,
) -> Vec<BindGroupEntry> {
    vec![
        BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: BindingResource::ImageView(view),
        },
        BindGroupEntry {
            binding: 1,
            array_index: 0,
            resource: BindingResource::Sampler(sampler),
        },
        BindGroupEntry {
            binding: 2,
            array_index: 0,
            resource: BindingResource::whole_buffer(uniforms),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The chain stops before a level degenerates, and never runs longer than
    /// topic 18 asks for.**
    ///
    /// The two ends of the rule, and the extents that decide them are the ones
    /// this engine actually renders at: the golden suite's two sizes, a 16×16
    /// null-backend frame, and a 1080p window. A chain that halved one step too
    /// far would ask the pool for a zero-extent image — which is not a resource
    /// any backend creates — and the failure would be a device error naming a
    /// descriptor rather than a bloom that was one level too long.
    #[test]
    fn the_chain_stops_before_a_level_falls_under_the_filter_footprint() {
        for (extent, expected) in [
            ((1920, 1080), MAX_MIPS),
            ((1280, 720), MAX_MIPS),
            ((256, 192), 4),
            ((97, 61), 2),
            ((64, 48), 2),
            ((16, 16), 1),
            // A target under `2 · MIN_MIP_EXTENT` on one axis has no chain at
            // all, and the caller tonemaps the scene colour unchanged.
            ((256, 15), 0),
            ((1, 1), 0),
        ] {
            assert_eq!(mips_for(extent), expected, "{extent:?}");
            for mip in 1..=mips_for(extent) {
                let (width, height) = mip_extent(extent, mip);
                assert!(
                    width >= MIN_MIP_EXTENT && height >= MIN_MIP_EXTENT,
                    "{extent:?} level {mip} is {width}x{height}, under the filter's own footprint"
                );
            }
        }
    }

    /// **`MAX_PASSES` is the ceiling and it lands on the longest chain.**
    ///
    /// It is what [`crate::ForwardRenderer::MAX_PASSES`] is built out of, so
    /// both halves matter: a ceiling under the longest chain would stop timing
    /// the tail of every large frame, and one over it would buy query sets
    /// nothing writes.
    #[test]
    fn the_pass_ceiling_is_the_longest_chain_this_module_will_build() {
        assert_eq!(Bloom::passes_for((1920, 1080)), Bloom::MAX_PASSES);
        assert_eq!(Bloom::passes_for((256, 192)), 8);
        assert_eq!(Bloom::passes_for((16, 16)), 2);
        assert_eq!(
            Bloom::passes_for((1, 1)),
            0,
            "a target with no chain records nothing, rather than a composite with \
             nothing to composite"
        );
        for extent in [(1920, 1080), (256, 192), (97, 61), (16, 16), (1, 1)] {
            assert!(
                Bloom::passes_for(extent) <= Bloom::MAX_PASSES,
                "{extent:?} records {} passes, past the ceiling of {}",
                Bloom::passes_for(extent),
                Bloom::MAX_PASSES
            );
        }
    }

    /// **Halving floors, and every level is exactly half the one above it.**
    ///
    /// The rounding rule stated as an assertion rather than as prose in the
    /// module docs, because the tap offsets in all three shaders are scaled by
    /// the *source's* texel size and a chain that rounded up somewhere would
    /// sample past the source's edge at the far border.
    #[test]
    fn every_level_is_the_floor_of_half_the_level_above_it() {
        for extent in [(1920, 1080), (256, 192), (97, 61), (16, 16)] {
            assert_eq!(mip_extent(extent, 0), extent, "level 0 is the scene itself");
            for mip in 1..=mips_for(extent) {
                let above = mip_extent(extent, mip - 1);
                assert_eq!(
                    mip_extent(extent, mip),
                    (above.0 / 2, above.1 / 2),
                    "{extent:?} level {mip}"
                );
            }
        }
    }
}
