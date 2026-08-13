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
    PipelineLayoutHandle, SampleType, ShaderStages, StoreOp,
};
use crcbl_shaders::{SSR, SSR_BLUR, ssr};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;

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
        frames: usize,
        build_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
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
        ];
        let layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("ssr"),
            entries: &entries,
        })?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ssr"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;

        // **The same shape as the march, with the raw reflection where the
        // reflectivity attachment was.** The blur reads the roughness ramp out
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
        let blur_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("ssr blur"),
            entries: &blur_entries,
        })?;
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
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        images: SsrImages,
    ) {
        let SsrImages {
            depth,
            color,
            reflectivity,
            reflection,
            composited,
        } = images;
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let uniforms = self.uniforms[frame];
        // Split so the two closures below borrow different halves of `self`, on
        // `Ssao::add_passes`' terms: one `&mut self` shared between them is what
        // the borrow checker refuses, and it would be refusing something
        // genuinely wrong — a pass body may run at any point after it is
        // declared.
        let (groups, blur_groups) = (&mut self.groups, &mut self.blur_groups);
        let cached = &mut groups[frame];
        let blur_cached = &mut blur_groups[frame];

        graph
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
            .execute(move |ctx| {
                let color_view = ctx.image_view(color);
                let depth_view = ctx.image_view(depth);
                let material_view = ctx.image_view(reflectivity);
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
                        resource: BindingResource::ImageView(material_view),
                    },
                ];
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(1, depth_view), (2, color_view), (3, material_view)],
                    "ssr",
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
    }
}
