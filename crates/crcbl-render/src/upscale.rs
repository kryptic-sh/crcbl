//! `docs/plan/43-render-standards.md`'s render-scale row: the spatial upscale
//! that carries an internal render target to the caller's own extent.
//!
//! # It is the second pass in this crate that changes the frame's shape
//!
//! [`crate::fxaa`] is the first, and the argument is the same one: with render
//! scale at `1.0` the post chain writes the caller's target directly and this
//! pass does not exist, and below `1.0` every internal target in the frame is
//! smaller than the target and this pass is what reaches it. [`crate::forward`]
//! owns the choice, because it owns both ends.
//!
//! The two compose in one order and only one. The resolve runs at the internal
//! extent, before this pass, so FXAA filters the edges the renderer actually
//! drew; resolving after the upscale would be filtering an interpolation of
//! them. The UI goes the other way and composites onto the target after this
//! pass at native resolution, which is the whole reason a render-scale knob is
//! usable at all — text stays sharp while the 3D frame is cheap.
//!
//! # One pass, one sampler, and why it is linear
//!
//! `upscale.slang` computes its own sixteen weights, so a nearest sampler would
//! be the defensible choice. It is the wrong one at the border: every tap is
//! addressed by UV and the ring outside the source is what `ClampToEdge`
//! decides, and a linear sampler reaching a clamped edge returns the edge texel
//! exactly, which is what the filter wants there.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, FilterMode, Format, GraphicsPipelineHandle,
    HalError, ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderStages,
    StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::{UPSCALE, upscale};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;

/// Vertices in the over-sized full-screen triangle `upscale.slang` generates
/// from `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// Everything the upscale owns.
///
/// Built once by [`Upscale::new`] and released by [`Upscale::destroy`], which is
/// the shape every other resource group in this crate has — see [`crate::fxaa`].
#[derive(Debug)]
pub(crate) struct Upscale {
    /// `[frame]`: `upscale.slang`'s uniform block, one per frame in flight for
    /// the frame uniforms' reason exactly — the previous frame may still be
    /// reading last frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    sampler: SamplerHandle,
    /// `[frame]`: the group, cached against the source view. One per frame in
    /// flight, because it names [`Upscale::uniforms`] as well as the transient.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

impl Upscale {
    /// Passes [`Upscale::add_pass`] adds to a frame.
    pub(crate) const PASSES: u32 = 1;

    /// Builds the pipeline, the sampler and the uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated, on
    /// [`crate::fxaa::Fxaa::new`]'s terms exactly: it is [`crate::forward`]'s.
    ///
    /// `target_format` is the format this pass writes — the caller's target —
    /// and, because what it reads is a transient of that same description, also
    /// the format it reads.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**: the caller holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        target_format: Format,
        build_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        let entries = [
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
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            // A uniform buffer rather than a push constant, for the reason every
            // post pass in this crate gives: WebGPU has no push constants, and
            // one Slang entry point cannot read both a push-constant block and a
            // bound one, so a range here would fork the shader.
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let desc = BindGroupLayoutDesc {
            label: Some("upscale"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("upscale"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("upscale"),
            bind_group_layouts: &[layout],
            push_constants: None,
        })?;
        let pipeline = build_fullscreen(
            device,
            "upscale",
            &UPSCALE,
            pipeline_layout,
            &[ColorTargetState::opaque(target_format)],
        )?;

        // **Linear, and only the border depends on it** — see the module docs.
        let sampler = device.create_sampler(&SamplerDesc {
            label: Some("upscale source"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("upscale params"),
                size: upscale::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?);
        }

        Ok(Self {
            uniforms,
            layout,
            pipeline_layout,
            pipeline,
            sampler,
            groups: (0..frames).map(|_| None).collect(),
        })
    }

    /// Writes this frame's block, where `source` is the **internal** render
    /// extent and not the caller's target extent.
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
        source: (u32, u32),
    ) -> Result<(), HalError> {
        let params = upscale::UpscaleParams::for_extent(source.0, source.1);
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `upscale` pass, reading `source` and writing `target`.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_pass<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        source: ImageId,
        target: ImageId,
    ) {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let sampler = self.sampler;
        let uniforms = self.uniforms[frame];
        let cached = &mut self.groups[frame];

        graph
            .add_render_pass("upscale")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target, so loading or clearing it is pure bandwidth.
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
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        // Overwritten by `cached_group` with the realised view;
                        // written here so the list is a complete description of
                        // the layout rather than a list with a hole in it.
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
                ];
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(0, view)],
                    "upscale source",
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
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self.groups.into_iter().flatten() {
            device.destroy_bind_group(cached.1);
        }
        device.destroy_sampler(self.sampler);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
    }
}
