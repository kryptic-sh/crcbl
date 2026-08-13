//! `docs/plan/18-render-features.md`'s screen-space ambient occlusion: the two
//! full-screen passes between the depth prepass and the forward pass.
//!
//! ```text
//! depth-prepass ──▶ scene-depth ──▶ ssao ──▶ ssao ──▶ ssao-blur ──▶ ssao-blurred
//!                                  (R8Unorm)                        (R8Unorm)
//!                                                                        │
//!                                                       forward ◀────────┘
//!                                                    (× frame.ambient)
//! ```
//!
//! A module of its own rather than more of [`crate::forward`], which is a file
//! this crate is trying to stop growing: what lives here is two pipelines, two
//! caches and the pass pair, and none of it is reachable from the geometry
//! passes except through [`Ssao::add_passes`].
//!
//! **The blur is not optional.** `shaders/ssao.slang` says why in full: its
//! rotation table makes the raw result carry a 4×4 tile as banding, and each of
//! its samples is a binary depth comparison that one driver may resolve
//! differently from another. The blur's footprint is exactly that tile, so it
//! removes the banding and divides an isolated flipped sample by sixteen.
//!
//! The off-switch is data rather than a branch: a frame that does not add these
//! passes leaves `ForwardRenderer`'s white placeholder bound, and `mesh.slang`
//! multiplies its ambient by 1.0. There is no device fact to gate on — every
//! backend has a full-screen draw, a sampled `D32Float` and an `R8Unorm` target.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, Format, GraphicsPipelineHandle, HalError,
    ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, ShaderStages, StoreOp,
};
use crcbl_shaders::{SSAO, SSAO_BLUR, ssao};

use crate::graph::{ImageId, RenderGraph};

/// Vertices in the over-sized full-screen triangle `ssao.slang` and
/// `ssao_blur.slang` generate from `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// Everything the occlusion pair owns.
///
/// Built once by [`Ssao::new`] and released by [`Ssao::destroy`], which is the
/// shape every other resource group in this crate has — see
/// [`crate::light_grid::LightGrid`].
#[derive(Debug)]
pub(crate) struct Ssao {
    /// `[frame]`: `ssao.slang`'s uniform block. One per frame in flight for the
    /// frame uniforms' reason exactly — the previous frame may still be reading
    /// last frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the occlusion group, cached against the depth view.
    ///
    /// **One per frame in flight**, because this group names [`Ssao::uniforms`]
    /// as well as the depth transient — and that is a ring. A single cache keyed
    /// on the view alone would hand the even frames' block to the odd frames.
    groups: Vec<Option<(ImageViewHandle, BindGroupHandle)>>,
    blur_layout: BindGroupLayoutHandle,
    blur_pipeline_layout: PipelineLayoutHandle,
    blur_pipeline: GraphicsPipelineHandle,
    /// The blur group, cached against the raw occlusion view. One rather than a
    /// ring: it names one image and nothing per-frame.
    blur_group: Option<(ImageViewHandle, BindGroupHandle)>,
}

impl Ssao {
    /// Passes [`Ssao::add_passes`] adds to a frame.
    pub(crate) const PASSES: u32 = 2;

    /// Builds both pipelines and the uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated: it is
    /// [`crate::forward`]'s, because the tonemap pass is the third caller and the
    /// shape it carries — a triangle out of `SV_VertexID`, no depth state, one
    /// colour target — is documented there.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the caller
    /// holds a rollback, and this is stored in it whole.
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
        // **No sampler in either layout**, which is the whole of what reading by
        // `Load` buys — see the binding comments in `shaders/ssao.slang`.
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
                    // `texture_depth_2d`, which `textureLoad` takes. There is no
                    // comparison sampler beside it because nothing here compares:
                    // this binding is fetched, not sampled.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("ssao depth"),
            entries: &entries,
        })?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ssao"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;

        let blur_entries = [BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // `Float`, unlike the pass above: the raw occlusion is an
                // ordinary `R8Unorm` colour image and the WGSL artifact declares
                // `texture_2d<f32>` for it.
                sample_type: SampleType::Float,
            },
            count: 1,
            flags: BindingFlags::empty(),
        }];
        let blur_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("ssao blur"),
            entries: &blur_entries,
        })?;
        let blur_set_layouts = [blur_layout];
        let blur_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ssao blur"),
            bind_group_layouts: &blur_set_layouts,
            push_constants: None,
        })?;

        let targets = [ColorTargetState::opaque(Format::R8Unorm)];
        let pipeline = build_fullscreen(device, "ssao", &SSAO, pipeline_layout, &targets)?;
        let blur_pipeline = build_fullscreen(
            device,
            "ssao blur",
            &SSAO_BLUR,
            blur_pipeline_layout,
            &targets,
        )?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("ssao params"),
                size: ssao::PARAMS_SIZE as u64,
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
            blur_group: None,
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
        params: ssao::SsaoParams,
    ) -> Result<(), HalError> {
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `ssao` and `ssao-blur` passes, in that order.
    ///
    /// `depth` must be the prepass's stored depth and `blurred` is what the
    /// forward pass reads. Both occlusion images are the caller's so it can
    /// declare the read on the pass that consumes the second one — a pass
    /// declares its own accesses and this one cannot declare the forward pass's.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        depth: ImageId,
        raw: ImageId,
        blurred: ImageId,
    ) {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let uniforms = self.uniforms[frame];
        // Split so the two closures below borrow different halves of `self`; one
        // `&mut self` shared between them is what the borrow checker refuses, and
        // it would be refusing something genuinely wrong — a pass body may run at
        // any point after it is declared.
        let (groups, blur_group) = (&mut self.groups, &mut self.blur_group);
        let cached = &mut groups[frame];

        graph
            .add_render_pass("ssao")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target, so loading or clearing it is pure bandwidth.
            .color(raw, LoadOp::DontCare, StoreOp::Store, ClearValue::default())
            // The prepass left this in `DepthStencilWrite`. Declaring the read is
            // what moves it to a shader-readable layout, and without it every
            // backend reads whatever the depth writes left behind.
            .read_image(depth)
            .execute(move |ctx| {
                let view = ctx.image_view(depth);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        // Overwritten by `cached_group` with the realised view;
                        // written here so the list is a complete description of
                        // the layout rather than a list with a hole in it.
                        resource: BindingResource::ImageView(view),
                    },
                ];
                let Some(group) =
                    cached_group(cached, device, view, "ssao depth", layout, entries, 1)
                else {
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
            .add_render_pass("ssao-blur")
            .color(
                blurred,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .read_image(raw)
            .execute(move |ctx| {
                let view = ctx.image_view(raw);
                let device = ctx.device();
                let entries = vec![BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::ImageView(view),
                }];
                let Some(group) = cached_group(
                    blur_group,
                    device,
                    view,
                    "ssao blur",
                    blur_layout,
                    entries,
                    0,
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
        for cached in self.groups.into_iter().chain([self.blur_group]).flatten() {
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

/// A bind group naming `view` at `binding`, built once and kept until the view
/// changes.
///
/// **The shape every group in this engine that names a graph transient has.** A
/// transient's view is realised by the graph and is therefore not known until a
/// pass body runs, so such a group cannot be built where the others are — and
/// rebuilding it every frame would be a descriptor write per frame for a handle
/// that only ever changes on a resize. `entries` is the group's complete
/// description with any value at `binding`; this replaces that one entry.
///
/// [`None`] means the group could not be created and the caller should record
/// nothing. Recording a pass that draws nothing is better than aborting a frame:
/// the window loses that pass's contribution, the log says why, and the next
/// frame retries.
///
/// It lives in this module because three of its four callers do; the fourth is
/// the tonemap pass in [`crate::forward`], which is the file this split exists to
/// stop growing.
///
/// # Panics
///
/// If `entries` has no entry at `binding` — which is a caller that built its list
/// against a different layout than the one it passed.
pub(crate) fn cached_group(
    cache: &mut Option<(ImageViewHandle, BindGroupHandle)>,
    device: &dyn Device,
    view: ImageViewHandle,
    label: &str,
    layout: BindGroupLayoutHandle,
    mut entries: Vec<BindGroupEntry>,
    binding: u32,
) -> Option<BindGroupHandle> {
    if let Some((cached, group)) = cache
        && *cached == view
    {
        return Some(*group);
    }
    if let Some((_, stale)) = cache.take() {
        device.destroy_bind_group(stale);
    }
    let slot = entries
        .iter_mut()
        .find(|entry| entry.binding == binding)
        .unwrap_or_else(|| panic!("{label}: the entry list has no binding {binding} to fill"));
    slot.resource = BindingResource::ImageView(view);
    match device.create_bind_group(&BindGroupDesc {
        label: Some(label),
        layout,
        entries: &entries,
        variable_count: None,
    }) {
        Ok(group) => {
            *cache = Some((view, group));
            Some(group)
        }
        Err(error) => {
            log::error!("graph: {label} bind group failed: {error}");
            None
        }
    }
}
