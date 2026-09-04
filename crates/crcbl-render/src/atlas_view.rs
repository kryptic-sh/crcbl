//! `docs/plan/sample/18-sundial.md`'s atlas viewer: the one full-screen pass
//! that draws the shadow atlas over the finished frame.
//!
//! ```text
//! shadow ──▶ shadow-atlas ──▶ … the frame …
//!            (D32Float)          │
//!                                ▼
//!                   tonemap ──▶ display ──▶ shadow-atlas-view (replaces it)
//! ```
//!
//! A module of its own rather than more of [`crate::forward`], on
//! [`crate::contact_shadows`]'s terms exactly: what lives here is one pipeline,
//! one cache and one pass, and none of it is reachable from the frame except
//! through [`AtlasView::add_pass`].
//!
//! # It is a pass where every other debug view is a shader branch
//!
//! [`crate::forward`]'s other replacing views ride in one lane of the frame
//! block and are branches in `mesh.slang`, because each is a function of the
//! fragment being shaded. This one is not: the atlas is a single image the whole
//! frame shares, and a pixel of this picture is an atlas texel rather than a
//! piece of geometry. Drawn from the colour pass it could only have appeared
//! where geometry covered, so what a reviewer saw of the atlas would depend on
//! where they were standing.
//!
//! # It draws in display space, after the tonemap, and that is the decision
//!
//! [`crate::grid`]'s placement argued the same thing for the ground grid and the
//! argument is stronger here: this picture is a *readout*, and a readout whose
//! greys move with the scene's exposure is one nobody can compare across two
//! frames. Drawn into the HDR scene colour it would be exposed and tonemapped
//! like geometry, so the grey standing for a given stored depth would depend on
//! how bright the rest of the room happened to be. Drawn here, a depth maps to a
//! value and stays there.
//!
//! It also costs the frame nothing it was not already paying: the pass runs only
//! when [`crate::DebugView::ShadowAtlas`] resolved, and a frame that resolved
//! any other view records no pass at all — see
//! [`ForwardRenderer::debug_view`](crate::ForwardRenderer::debug_view), which is
//! where that order lives.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, Format, GraphicsPipelineHandle, HalError,
    ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::{ATLAS_VIEW, atlas_view};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;

/// Vertices in the over-sized full-screen triangle `atlas_view.slang` generates
/// from `SV_VertexID`. No geometry is bound anywhere.
///
/// `pub(crate)` where every other full-screen pass in this crate keeps its copy
/// private, because [`crate::forward`]'s
/// `the_atlas_viewer_records_its_pass_only_when_it_is_the_view_showing` reads
/// the draw back out of the recorded stream and compares it against this — a
/// literal there would be the same number written twice.
pub(crate) const FULLSCREEN_VERTICES: u32 = 3;

/// The label the pass is recorded under, and therefore the row
/// [`crate::PassStats`] reports it as.
pub(crate) const LABEL: &str = "shadow-atlas-view";

/// Everything the viewer owns.
///
/// Built once by [`AtlasView::new`] and released by [`AtlasView::destroy`],
/// which is the shape every other resource group in this crate has — see
/// [`crate::contact_shadows::ContactShadows`].
#[derive(Debug)]
pub(crate) struct AtlasView {
    /// `[frame]`: `atlas_view.slang`'s uniform block. One per frame in flight
    /// for the frame uniforms' reason exactly — the previous frame may still be
    /// reading last frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the group, cached against the atlas's view.
    ///
    /// **One per frame in flight**, for [`crate::contact_shadows`]'s reason:
    /// this group names [`AtlasView::uniforms`] as well as the image, and a
    /// single cache keyed on the view alone would hand the even frames' block to
    /// the odd frames. The atlas is a renderer-owned image rather than a
    /// transient, so the view itself does not move — what the cache is really
    /// doing here is building each group once.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

impl AtlasView {
    /// Passes [`AtlasView::add_pass`] adds to a frame that records it.
    ///
    /// One, and a count rather than a ceiling: there is no chain behind it and
    /// nothing whose length depends on the extent. `crate::forward`'s
    /// `RENDER_PASSES` takes it as its term all the same.
    pub(crate) const PASSES: u32 = 1;

    /// Builds the pipeline and the uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated, on
    /// [`crate::contact_shadows::ContactShadows::new`]'s terms: it is
    /// [`crate::forward`]'s.
    ///
    /// `target_format` is the format this pass writes, which is the caller's
    /// target — the picture it replaces is the one the tonemap put there.
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
        // **No sampler in the layout**, which is what reading by `Load` buys —
        // see the binding comments in `shaders/atlas_view.slang`.
        let entries = [
            // A uniform buffer rather than a push constant, for the reason every
            // full-screen pass in this crate gives: WebGPU has no push
            // constants, and one Slang entry point cannot read both a
            // push-constant block and a bound one, so a range here would fork
            // the shader.
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
                    // **`Depth`, and it is the seam's half of `DepthTexture2D`**
                    // — `crate::contact_shadows::ContactShadows::new`'s binding
                    // argues it in full. WebGPU will only bind a `D32Float` view
                    // through a depth sample type, and the WGSL artifact agrees.
                    // There is no comparison sampler beside it because nothing
                    // here compares: this binding is fetched, not sampled.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let desc = BindGroupLayoutDesc {
            label: Some("shadow atlas view"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("shadow atlas view"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("shadow atlas view"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;

        let targets = [ColorTargetState::opaque(target_format)];
        let pipeline = build_fullscreen(
            device,
            "shadow atlas view",
            &ATLAS_VIEW,
            pipeline_layout,
            &targets,
        )?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("shadow atlas view params"),
                size: atlas_view::PARAMS_SIZE as u64,
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
        })
    }

    /// Writes `frame`'s block: where the atlas is letterboxed into `extent`, and
    /// where each slot's map is inside it.
    ///
    /// `rects` is the frame block's own `shadow_atlas_rect`, handed over rather
    /// than derived a second time — a viewer drawing a tile grid the sampling
    /// side does not have would be a diagnostic that invents its own evidence.
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
        atlas: (u32, u32),
        rects: [[f32; 4]; crcbl_shaders::mesh::SHADOW_ATLAS_TILES],
    ) -> Result<(), HalError> {
        let params = atlas_view::AtlasViewParams::letterboxed(extent, atlas, rects);
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `shadow-atlas-view` pass, reading `atlas` and writing over
    /// `target`.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_pass<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        atlas: ImageId,
        target: ImageId,
    ) {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let uniforms = self.uniforms[frame];
        let cached = &mut self.groups[frame];

        graph
            .add_render_pass(LABEL)
            // `DontCare`, not `Load`: this view *replaces* the picture, so the
            // full-screen triangle writes every pixel of the target and loading
            // what the tonemap put there is pure bandwidth.
            .color(
                target,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            // The shadow pass left this in whatever the frame's samplers wanted.
            // Declaring the read is what gives the graph an edge from that write
            // to this one; without it this pass would read the atlas through no
            // barrier at all.
            .read_image(atlas)
            .execute(move |ctx| {
                let view = ctx.image_view(atlas);
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
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(1, view)],
                    "shadow atlas view",
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
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
    }
}
