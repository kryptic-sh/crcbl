//! `docs/plan/18-render-features.md`'s antialiasing row, cheap rung: the FXAA
//! pass that resolves the tonemapped frame into the target.
//!
//! # It is the only pass in this crate that reads what another pass presented
//!
//! Every other full-screen pass here reads a transient and writes a transient.
//! This one reads the image the tonemap wrote and writes the image the UI is
//! composited onto, which is why switching it on changes the *shape* of the
//! frame rather than adding a pass to it: with antialiasing off the tonemap
//! writes the caller's target directly, and with it on the tonemap writes a
//! transient at the target's own format and this pass resolves that into the
//! target. [`crate::forward`] owns that choice, because it owns both ends.
//!
//! The ground grid moves with the tonemap and not with this pass, deliberately.
//! It is display-space chrome drawn after the operator — [`crate::forward`]
//! argues that at length — and drawing it into the intermediate keeps it after
//! the operator while also putting it *in front of* the edge filter, which is
//! what a grid wants: a grid is a field of thin high-contrast lines, and thin
//! high-contrast lines are the thing antialiasing exists for. The UI is the
//! opposite case and stays behind: it composites onto the resolved target at
//! native resolution, so its glyphs are never filtered.
//!
//! # One pass, one sampler, and the sampler is not the tonemap's
//!
//! `fxaa.slang` samples at fractional offsets by construction — the whole filter
//! is a bilinear fetch placed off-centre by an amount it computed — so this pass
//! creates a **linear** sampler of its own where the tonemap deliberately keeps
//! a nearest one. Sharing the tonemap's would round every offset back to the
//! texel it started at and the pass would be an expensive blit.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, FilterMode, Format, GraphicsPipelineHandle,
    HalError, ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderStages,
    StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::{FXAA, fxaa};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;

/// Vertices in the over-sized full-screen triangle `fxaa.slang` generates from
/// `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// Everything the resolve owns.
///
/// Built once by [`Fxaa::new`] and released by [`Fxaa::destroy`], which is the
/// shape every other resource group in this crate has — see [`crate::ssao`].
#[derive(Debug)]
pub(crate) struct Fxaa {
    /// `[frame]`: `fxaa.slang`'s uniform block. One per frame in flight for the
    /// frame uniforms' reason exactly — the previous frame may still be reading
    /// last frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    sampler: SamplerHandle,
    /// `[frame]`: the group, cached against the source view.
    ///
    /// **One per frame in flight**, because this group names [`Fxaa::uniforms`]
    /// as well as the transient — and that is a ring. A single cache keyed on
    /// the view alone would hand the even frames' block to the odd frames.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

impl Fxaa {
    /// Passes [`Fxaa::add_pass`] adds to a frame.
    pub(crate) const PASSES: u32 = 1;

    /// Builds the pipeline, the sampler and the uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated, on
    /// [`crate::ssao::Ssao::new`]'s terms exactly: it is [`crate::forward`]'s,
    /// because the tonemap pass is a caller of the same shape.
    ///
    /// `target_format` is the format this pass writes, which is the caller's
    /// target — and, because the tonemap now writes an intermediate of the same
    /// description, also the format it reads.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the
    /// caller holds a rollback, and this is stored in it whole.
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
            label: Some("fxaa"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("fxaa"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("fxaa"),
            bind_group_layouts: &[layout],
            push_constants: None,
        })?;
        let pipeline = build_fullscreen(
            device,
            "fxaa",
            &FXAA,
            pipeline_layout,
            &[ColorTargetState::opaque(target_format)],
        )?;

        // **Linear**, unlike the tonemap's — see the module docs.
        let sampler = device.create_sampler(&SamplerDesc {
            label: Some("fxaa source"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("fxaa params"),
                size: fxaa::PARAMS_SIZE as u64,
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

    /// Writes this frame's block.
    ///
    /// The extent is the frame's, and it is the *only* field the shader cannot
    /// derive — see [`crcbl_shaders::fxaa::FxaaParams::for_extent`], whose
    /// default leaves it zero precisely so an unwritten block draws a tell
    /// rather than something that nearly works.
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
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        let params = fxaa::FxaaParams::for_extent(extent.0, extent.1);
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `fxaa` pass, reading `source` and writing `target`.
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
            .add_render_pass("fxaa")
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
                let Some(group) =
                    cached_group(cached, device, &[(0, view)], "fxaa source", layout, entries)
                else {
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
