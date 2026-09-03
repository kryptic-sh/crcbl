//! `docs/plan/18-render-features.md`'s screen-space reflections: the two
//! full-screen passes between the forward pass and the tonemap.
//!
//! ```text
//! forward ──▶ scene-color ─────┐                  ┌─────────────┐
//!         └─▶ reflectivity ────┼──▶ ssr ──▶ reflection ──▶ ssr-blur ──▶ tonemap
//! prepass ──▶ scene-depth ─────┴──────────────────┘  (scene-reflected)
//! ```
//!
//! A module of its own rather than more of [`crate::forward`], on
//! [`crate::ssao`]'s terms exactly: what lives here is two pipelines, two caches
//! and the pass pair, and none of it is reachable from the geometry passes
//! except through [`Ssr::add_passes`].
//!
//! **The blur is the composite, and the march is not.** `ssr.slang` writes the
//! reflection by itself into an `Rgba16Float` transient; `ssr_blur.slang`
//! filters that image and writes the sum of it and the scene colour into a
//! second one, which [`ForwardRenderer::add_passes`] returns in place of the
//! scene colour. Had the march composited there would be no way to filter the
//! reflection without filtering the whole frame with it. Two passes, no blend
//! state, and no image that is both read and written.
//!
//! That is also the off-switch: a frame that does not add these passes hands the
//! scene colour on and is bit-identical, needing no placeholder because nothing
//! upstream reads the result.
//!
//! There is no device fact to gate on. Every backend has a full-screen draw, a
//! sampled `D32Float` and a sampled `Rgba8Unorm`, and the reflectivity
//! attachment is written whether or not these passes exist.
//!
//! [`ForwardRenderer::add_passes`]: crate::forward::ForwardRenderer::add_passes

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, Format, GraphicsPipelineHandle, HalError,
    ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, QueueHandle, SampleType, ShaderStages, StoreOp,
    check_portable_storage_buffers,
};
use crcbl_shaders::{SSR, SSR_BLUR, dfg, sky_prefilter, ssr};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;
use crate::texture::{UploadedTexture, upload_texture};

/// The binding `ssr.slang` reads [`sky_prefilter`]'s table through — after the
/// pyramid's slots, so Metal's declaration-order argument numbering stays
/// aligned with this layout.
const SKY_PREFILTER_BINDING: u32 = 5 + crate::hiz::MAX_LEVELS;

/// [`sky_prefilter::PREFILTER_SIZE`] as an image extent.
const SKY_PREFILTER_SIZE: u32 = sky_prefilter::PREFILTER_SIZE as u32;

/// The binding `ssr.slang` reads [`dfg`]'s pair through, beside the sky table.
const DFG_BINDING: u32 = SKY_PREFILTER_BINDING + 1;

/// [`dfg::DFG_SIZE`] as an image extent.
const DFG_SIZE: u32 = dfg::DFG_SIZE as u32;

/// The binding `ssr.slang` reads `docs/plan/50-irradiance-probes.md`'s per-probe
/// visibility maps through — the image [`crate::forward`] binds to `mesh.slang`,
/// so that the reflection's probe fallback is weighed by the same Chebyshev
/// bound the diffuse gather is and stops reading a probe through a wall.
///
/// **Appended past [`DFG_BINDING`], never inserted**, for the reason
/// [`crate::forward`]'s own binding constants give: `crcbl-mtl` numbers a
/// resource by counting the same-table entries of the layout list and Slang
/// numbers a stage's arguments by declaration order, so the two agree only while
/// both ascend. `msl/ssr.metal` puts it at `texture(10)`, the next free index of
/// that table.
const PROBE_VISIBILITY_BINDING: u32 = DFG_BINDING + 1;

/// Vertices in the over-sized full-screen triangle `ssr.slang` and
/// `ssr_blur.slang` generate from `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// The graph transients [`Ssr::add_passes`] reads and writes.
///
/// **One struct rather than five positional arguments** — which is what
/// `clippy::too_many_arguments` objects to and also what makes five values of
/// one type impossible to swap at the call site. `crate::texture`'s
/// `UploadArgs` is the same shape for the same reason.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SsrImages {
    /// The depth prepass's stored depth, marched by the first pass and weighed
    /// by the second.
    pub(crate) depth: ImageId,
    /// The forward pass's scene target: where a hit's colour comes from, and
    /// what the blur adds the filtered reflection to.
    pub(crate) color: ImageId,
    /// The forward pass's reflectivity attachment. Read by the march alone —
    /// the blur takes the roughness ramp out of [`SsrImages::reflection`]'s
    /// alpha instead.
    pub(crate) reflectivity: ImageId,
    /// The march's output: the reflection by itself, with its weight in the
    /// alpha.
    pub(crate) reflection: ImageId,
    /// The blur's output, and the image the caller must go on to tonemap.
    pub(crate) composited: ImageId,
    /// Levels 1 upwards of the Hi-Z pyramid the march climbs, one slot per
    /// binding and **every slot filled** — a frame with a shorter pyramid than
    /// bindings repeats its deepest level, and one with no pyramid at all
    /// repeats [`SsrImages::depth`]. See [`crate::hiz::level_slots`], which is
    /// what the caller fills this with, and `SsrParams::hiz_levels`, which is
    /// what stops the march reading a slot that is a repeat.
    pub(crate) pyramid: [ImageId; crate::hiz::MAX_LEVELS as usize],
}

/// Everything the reflection pair owns.
///
/// Built once by [`Ssr::new`] and released by [`Ssr::destroy`], which is the
/// shape every other resource group in this crate has — see [`crate::ssao`].
#[derive(Debug)]
pub(crate) struct Ssr {
    /// `[frame]`: `ssr.slang`'s uniform block. One per frame in flight for the
    /// frame uniforms' reason exactly — the previous frame may still be reading
    /// last frame's while this one is written.
    ///
    /// **A block of its own rather than the occlusion pair's**, which carries
    /// the same two matrices and a radius and a bias besides. Sharing the buffer
    /// would make a pass that is meant to stand alone depend on that pair having
    /// been built, for 128 bytes per frame in flight.
    uniforms: Vec<BufferHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the group, cached against all three views together.
    ///
    /// **One per frame in flight**, because this group names [`Ssr::uniforms`]
    /// as well as three graph transients — and that is a ring. A single cache
    /// keyed on the views alone would hand the even frames' block to the odd
    /// frames.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    blur_layout: BindGroupLayoutHandle,
    blur_pipeline_layout: PipelineLayoutHandle,
    blur_pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the blur group, cached against its own three views together.
    ///
    /// **A ring for [`Ssr::groups`]' reason exactly**: `ssr_blur.slang` weights
    /// its kernel on view-space depth, so it binds the same [`Ssr::uniforms`]
    /// block the march does — and a single cache keyed on the views alone would
    /// hand the even frames' block to the odd frames.
    blur_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// `crcbl_shaders::sky_prefilter`'s table as the `Rgba8Unorm` image
    /// `ssr.slang` reads the sky through at a surface's roughness. Uploaded
    /// once here rather than by [`crate::forward`], because this pass is its
    /// only reader — the DFG table beside it in the forward pass has a reader
    /// there.
    sky_prefilter: UploadedTexture,
    /// `crcbl_shaders::dfg`'s pair as the `Rgba8Unorm` image `ssr.slang`
    /// scales its environment by — `f0 · scale + bias`, the split-sum's second
    /// half. The same table [`crate::forward`] uploads summed to one channel
    /// for energy compensation; this pass wants both channels, so it carries
    /// its own image of them.
    dfg: UploadedTexture,
}

impl Ssr {
    /// Passes [`Ssr::add_passes`] adds to a frame.
    pub(crate) const PASSES: u32 = 2;

    /// Builds both pipelines and the uniform ring.
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
        queue: QueueHandle,
        frames: usize,
        build_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        // The committed table, fixed point in four bytes a texel — see
        // `sky_prefilter::PREFILTER_TEXEL_BYTES`. Not baked here, for the DFG
        // table's reason in `crate::forward`: the integrator is a Monte Carlo
        // sum and four backends would bake four tables.
        let sky_prefilter = upload_texture(
            device,
            queue,
            "sky prefilter table",
            Format::Rgba8Unorm,
            SKY_PREFILTER_SIZE,
            SKY_PREFILTER_SIZE,
            &sky_prefilter::texels(),
        )?;
        let dfg_pair = upload_texture(
            device,
            queue,
            "dfg pair table",
            Format::Rgba8Unorm,
            DFG_SIZE,
            DFG_SIZE,
            &dfg::pair_texels(),
        )?;

        // **No sampler**, which is the whole of what reading by `Load` buys —
        // and it buys more here than in the occlusion pass, because what a tap
        // fetches is a colour rather than a fraction a blur will average. See
        // the binding comments in `shaders/ssr.slang`.
        let entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // **`Depth`, and it is the seam's half of `DepthTexture2D`.**
                    // WebGPU will only bind a `D32Float` view through a depth
                    // sample type, and the WGSL artifact agrees — it declares
                    // `texture_depth_2d`, which `textureLoad` takes. The
                    // paragraph on `crate::ssao`'s own depth binding is this
                    // one's too: it is the same image, read the same way.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // The `Rgba16Float` scene target: an ordinary colour image,
                    // and the WGSL artifact declares `texture_2d<f32>` for it.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // The `Rgba8Unorm` reflectivity attachment, on the scene
                    // colour's terms: `SAMPLED` is on
                    // `TransientImageDesc::reflectivity` because *this* pass is
                    // the thing that reads it.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        // Levels 1 upwards of the depth pyramid, one binding each rather than
        // one array binding: `ssr.slang` reads them through a `switch` on the
        // level, which is what keeps the whole march inside a texture type WGSL
        // will index without a binding array — see that shader's `hiz_at`.
        // Level 0 is binding 1 above, which is the prepass itself.
        let mut entries = entries.to_vec();
        entries.push(BindGroupLayoutEntry {
            binding: SKY_PREFILTER_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // An ordinary colour image the shader `Load`s and filters
                // itself; no sampler, like every other read in the pass.
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        entries.push(BindGroupLayoutEntry {
            binding: DFG_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // The per-probe visibility maps, at the highest binding of the set — see
        // [`PROBE_VISIBILITY_BINDING`] on why the number rather than the list
        // position is what has to ascend.
        //
        // `D2Array` and `UnfilterableFloat` are both *declared* rather than
        // merely bound, which is [`crate::forward`]'s note on the same image:
        // WebGPU compares the dimension and the sample type a layout entry
        // claims against the view handed to it, the image is `Rg32Float`, and
        // `Rg32Float` is unfilterable without the `float32-filterable` feature.
        // Reading it only with `Load` does not lift that — the check is on the
        // layout against the view's format, not on how the shader gets at it.
        entries.push(BindGroupLayoutEntry {
            binding: PROBE_VISIBILITY_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2Array,
                sample_type: SampleType::UnfilterableFloat,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        entries.extend(
            (0..crate::hiz::MAX_LEVELS).map(|level| BindGroupLayoutEntry {
                binding: 5 + level,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // `Depth`, and the paragraph on binding 1 is this one's too:
                    // every level is `D32Float`, which is the point of the pyramid
                    // being depth images rather than `R32Float` ones.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            }),
        );
        let desc = BindGroupLayoutDesc {
            label: Some("ssr"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("ssr"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ssr"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;

        // **The same shape as the march, with the raw reflection where the
        // reflectivity attachment was.** The blur reads the sharpness ramp out
        // of that image's alpha rather than reading the attachment a second
        // time, which is why this list is not one binding longer — see
        // `shaders/ssr_blur.slang`.
        let blur_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // `Depth`, and the paragraph on the march's own depth
                    // binding is this one's too: it is the same image, read
                    // through the same `texture_depth_2d`.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // The `Rgba16Float` scene target, which this pass adds the
                    // filtered reflection to.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // The march's own output, on the scene colour's terms: an
                    // ordinary `Rgba16Float` image read by `Load`.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let blur_desc = BindGroupLayoutDesc {
            label: Some("ssr blur"),
            entries: &blur_entries,
        };
        check_portable_storage_buffers(Some("ssr blur"), &[&blur_desc])?;
        let blur_layout = device.create_bind_group_layout(&blur_desc)?;
        let blur_set_layouts = [blur_layout];
        let blur_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ssr blur"),
            bind_group_layouts: &blur_set_layouts,
            push_constants: None,
        })?;

        // The same format the forward pass wrote, for both: the blur's output is
        // that image plus a term, so a narrower target would tonemap a truncated
        // scene and the reflection would be blamed for it — and the march's is a
        // reflection out of that same image, which an eight-bit target would
        // clip to the tonemap's range before the tonemap ever saw it.
        let targets = [ColorTargetState::opaque(Format::Rgba16Float)];
        let pipeline = build_fullscreen(device, "ssr", &SSR, pipeline_layout, &targets)?;
        let blur_pipeline = build_fullscreen(
            device,
            "ssr blur",
            &SSR_BLUR,
            blur_pipeline_layout,
            &targets,
        )?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("ssr params"),
                size: ssr::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?);
        }

        Ok(Self {
            uniforms,
            layout,
            pipeline_layout,
            pipeline,
            groups: vec![None; frames],
            blur_layout,
            blur_pipeline_layout,
            blur_pipeline,
            blur_groups: vec![None; frames],
            sky_prefilter,
            dfg: dfg_pair,
        })
    }

    /// Writes `frame`'s uniform block.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the mapped write.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn begin_frame(
        &self,
        device: &dyn Device,
        frame: usize,
        params: ssr::SsrParams,
    ) -> Result<(), HalError> {
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `ssr` and `ssr-blur` passes, in that order.
    ///
    /// Both reflection images are the caller's so it can declare the read on the
    /// pass that consumes the second one — a pass declares its own accesses and
    /// this one cannot declare the tonemap's.
    /// [`SsrImages::composited`] is what the caller must go on to tonemap and
    /// hand back: the reflection is *in* it, and the scene colour it was added
    /// to is not the finished picture any more.
    ///
    /// `probe_visibility` is the caller's own — the captured maps when it has
    /// them and the console switch is on, and its one-texel placeholder
    /// otherwise, which every probe keeps all of its weight through. It is not a
    /// graph transient, so it is not realised here; it does change between
    /// frames when the switch moves, which is why it joins the cache key below.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        images: SsrImages,
        probes: BufferHandle,
        probe_id: crate::graph::BufferId,
        probe_visibility: ImageViewHandle,
    ) {
        let SsrImages {
            depth,
            color,
            reflectivity,
            reflection,
            composited,
            pyramid,
        } = images;
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let uniforms = self.uniforms[frame];
        let sky_prefilter_view = self.sky_prefilter.view;
        let dfg_view = self.dfg.view;
        // Split so the two closures below borrow different halves of `self`, on
        // `Ssao::add_passes`' terms: one `&mut self` shared between them is what
        // the borrow checker refuses, and it would be refusing something
        // genuinely wrong — a pass body may run at any point after it is
        // declared.
        let (groups, blur_groups) = (&mut self.groups, &mut self.blur_groups);
        let cached = &mut groups[frame];
        let blur_cached = &mut blur_groups[frame];

        // The pyramid slots, deduplicated against the depth prepass and against
        // each other: a short pyramid repeats its deepest level into the slots
        // above, and declaring one image's read twice would have the graph emit
        // a barrier for a transition that has already happened.
        let mut pyramid_reads: Vec<ImageId> = Vec::with_capacity(pyramid.len());
        for level in pyramid {
            if level != depth && !pyramid_reads.contains(&level) {
                pyramid_reads.push(level);
            }
        }

        let mut pass = graph
            .add_render_pass("ssr")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target — a pixel with no reflection writes zero — so
            // loading or clearing it is pure bandwidth.
            .color(
                reflection,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            // The forward pass left both of these in `ColorAttachment` and the
            // prepass left the depth in `DepthStencilWrite`. Declaring the reads
            // is what moves each into a shader-readable layout, and without them
            // every backend reads whatever the last writer left behind.
            .read_image(color)
            .read_image(depth)
            .read_image(reflectivity)
            .read_buffer(probe_id);
        for level in pyramid_reads {
            // Each level was this pass's own reduction's depth attachment a
            // moment ago, so this declaration is its barrier into a
            // shader-readable layout.
            pass = pass.read_image(level);
        }
        pass.execute(move |ctx| {
            let color_view = ctx.image_view(color);
            let depth_view = ctx.image_view(depth);
            let material_view = ctx.image_view(reflectivity);
            let device = ctx.device();
            let mut entries = vec![
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(uniforms),
                },
                // The three below are overwritten by `cached_group` with the
                // realised views; written here so the list is a complete
                // description of the layout rather than a list with holes in
                // it.
                BindGroupEntry {
                    binding: 1,
                    array_index: 0,
                    resource: BindingResource::ImageView(depth_view),
                },
                BindGroupEntry {
                    binding: 2,
                    array_index: 0,
                    resource: BindingResource::ImageView(color_view),
                },
                BindGroupEntry {
                    binding: 3,
                    array_index: 0,
                    resource: BindingResource::ImageView(material_view),
                },
                BindGroupEntry {
                    binding: 4,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(probes),
                },
                // Static, like the uniforms and the probes: not part of the
                // cache key below, which holds the views that change.
                BindGroupEntry {
                    binding: SKY_PREFILTER_BINDING,
                    array_index: 0,
                    resource: BindingResource::ImageView(sky_prefilter_view),
                },
                BindGroupEntry {
                    binding: DFG_BINDING,
                    array_index: 0,
                    resource: BindingResource::ImageView(dfg_view),
                },
                // Overwritten by `cached_group` with the same view, which is
                // what puts it in the key: the two tables above never move and
                // this one does, whenever a capture lands or the console switch
                // is toggled.
                BindGroupEntry {
                    binding: PROBE_VISIBILITY_BINDING,
                    array_index: 0,
                    resource: BindingResource::ImageView(probe_visibility),
                },
            ];
            let pyramid_views = pyramid.map(|level| ctx.image_view(level));
            entries.extend(pyramid_views.iter().enumerate().map(|(level, view)| {
                BindGroupEntry {
                    binding: 5 + u32::try_from(level)
                        .unwrap_or_else(|_| unreachable!("a pyramid of a few levels")),
                    array_index: 0,
                    resource: BindingResource::ImageView(*view),
                }
            }));
            let mut key = vec![
                (1, depth_view),
                (2, color_view),
                (3, material_view),
                (PROBE_VISIBILITY_BINDING, probe_visibility),
            ];
            key.extend(pyramid_views.iter().enumerate().map(|(level, view)| {
                (
                    5 + u32::try_from(level)
                        .unwrap_or_else(|_| unreachable!("a pyramid of a few levels")),
                    *view,
                )
            }));
            let Some(group) = cached_group(cached, device, &key, "ssr", layout, entries) else {
                return;
            };
            let encoder = ctx.encoder();
            encoder.bind_graphics_pipeline(pipeline);
            encoder.bind_group(0, group, &[], pipeline_layout);
            encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
        });

        let blur_pipeline = self.blur_pipeline;
        let blur_pipeline_layout = self.blur_pipeline_layout;
        let blur_layout = self.blur_layout;
        graph
            .add_render_pass("ssr-blur")
            .color(
                composited,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            // **The march's own output**, which it wrote as a colour attachment
            // a moment ago, so this declaration is the barrier into a
            // shader-readable layout.
            .read_image(reflection)
            // The scene colour and the depth again: the pass above declared its
            // own reads, and a barrier the graph was never told about is one it
            // does not insert.
            .read_image(color)
            .read_image(depth)
            .execute(move |ctx| {
                let color_view = ctx.image_view(color);
                let depth_view = ctx.image_view(depth);
                let reflection_view = ctx.image_view(reflection);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                    // The three below are overwritten by `cached_group` with the
                    // realised views; written here so the list is a complete
                    // description of the layout rather than a list with holes in
                    // it.
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::ImageView(depth_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::ImageView(color_view),
                    },
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::ImageView(reflection_view),
                    },
                ];
                let Some(group) = cached_group(
                    blur_cached,
                    device,
                    &[(1, depth_view), (2, color_view), (3, reflection_view)],
                    "ssr blur",
                    blur_layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(blur_pipeline);
                encoder.bind_group(0, group, &[], blur_pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self.groups.into_iter().chain(self.blur_groups).flatten() {
            device.destroy_bind_group(cached.1);
        }
        device.destroy_graphics_pipeline(self.blur_pipeline);
        device.destroy_pipeline_layout(self.blur_pipeline_layout);
        device.destroy_bind_group_layout(self.blur_layout);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        self.sky_prefilter.destroy(device);
        self.dfg.destroy(device);
    }
}
