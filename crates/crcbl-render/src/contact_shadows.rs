//! `docs/plan/45-shadows.md`'s screen-space contact shadows: the one full-screen
//! pass between the depth prepass and the forward pass.
//!
//! ```text
//! depth-prepass ──▶ scene-depth ──▶ contact-shadows ──▶ contact
//!                   │                                   (R8Unorm)
//!                   └───────────────────────────────────────┘
//!                                                       forward
//!                                                (× the sun's visibility)
//! ```
//!
//! A module of its own rather than more of [`crate::forward`], on
//! [`crate::ssao`]'s terms exactly: what lives here is one pipeline, one cache
//! and one pass, and none of it is reachable from the geometry passes except
//! through [`ContactShadows::add_passes`].
//!
//! # What the atlas cannot do, and this can
//!
//! A cascade texel is a finite patch of the world, and `crate::shadow`'s biases
//! are denominated in exactly that patch — so the bias that stops acne on a
//! sunlit floor is also what detaches the shadow from the foot standing on it.
//! `shaders/contact_shadows.slang` marches the depth prepass instead, at frame
//! resolution, where that contact is a few pixels wide. It is a screen-space
//! term, so it writes nothing into a tile and stacks with every rung the atlas
//! ladder has above it.
//!
//! # There is no blur, unlike the occlusion pair
//!
//! `crate::ssao`'s blur is not optional because a binary depth comparison
//! resolves differently on two drivers and cliffs a pixel by an eighth, and the
//! blur divides that by sixteen. A march has no such denominator — the first
//! crossing *is* the answer — so a blur here would soften the contact this pass
//! exists to draw without buying the determinism that argument was about.
//! `shaders/contact_shadows.slang` takes the other route the reflection march
//! takes: no jitter of any kind, and every weight continuous and reaching zero
//! exactly where the decision is fragile.
//!
//! # The off-switch is data rather than a branch
//!
//! A frame that does not add this pass leaves `ForwardRenderer`'s white
//! placeholder bound, and `mesh.slang` multiplies the sun's visibility by 1.0.
//! There is no device fact to gate on — every backend has a full-screen draw, a
//! sampled `D32Float` and an `R8Unorm` target.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, Format, GraphicsPipelineHandle, HalError,
    ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::{CONTACT_SHADOWS, contact_shadows};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;

/// Vertices in the over-sized full-screen triangle
/// `shaders/contact_shadows.slang` generates from `SV_VertexID`. No geometry is
/// bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// The label the pass is recorded under, and therefore the row
/// [`crate::PassStats`] reports it as.
const LABEL: &str = "contact-shadows";

/// Everything the march owns.
///
/// Built once by [`ContactShadows::new`] and released by
/// [`ContactShadows::destroy`], which is the shape every other resource group in
/// this crate has — see [`crate::ssao::Ssao`].
#[derive(Debug)]
pub(crate) struct ContactShadows {
    /// `[frame]`: `contact_shadows.slang`'s uniform block. One per frame in
    /// flight for the frame uniforms' reason exactly — the previous frame may
    /// still be reading last frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the march's group, cached against the depth view.
    ///
    /// **One per frame in flight**, for [`crate::ssao::Ssao`]'s reason: this
    /// group names [`ContactShadows::uniforms`] as well as the depth transient,
    /// and a single cache keyed on the view alone would hand the even frames'
    /// block to the odd frames.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

impl ContactShadows {
    /// Passes [`ContactShadows::add_passes`] adds to a frame that records it.
    ///
    /// One, and a count rather than a ceiling: there is no blur behind it and no
    /// chain whose length depends on the extent, so the frame that records this
    /// effect records exactly this many passes. `crate::forward`'s
    /// `RENDER_PASSES` takes it as its term all the same.
    pub(crate) const PASSES: u32 = 1;

    /// Builds the pipeline and the uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated, on
    /// [`crate::ssao::Ssao::new`]'s terms: it is [`crate::forward`]'s, and the
    /// shape it carries — a triangle out of `SV_VertexID`, no depth state, one
    /// colour target — is documented there.
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
        // **No sampler in the layout**, which is the whole of what reading by
        // `Load` buys — see the binding comments in
        // `shaders/contact_shadows.slang`.
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
                    // **`Depth`, and it is the seam's half of `DepthTexture2D`**
                    // — `crate::ssao::Ssao::new`'s binding argues it in full.
                    // WebGPU will only bind a `D32Float` view through a depth
                    // sample type, and the WGSL artifact agrees. There is no
                    // comparison sampler beside it because nothing here
                    // compares: this binding is fetched, not sampled.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let desc = BindGroupLayoutDesc {
            label: Some("contact shadows depth"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("contact shadows"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("contact shadows"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;

        let targets = [ColorTargetState::opaque(Format::R8Unorm)];
        let pipeline = build_fullscreen(
            device,
            "contact shadows",
            &CONTACT_SHADOWS,
            pipeline_layout,
            &targets,
        )?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("contact shadow params"),
                size: contact_shadows::PARAMS_SIZE as u64,
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
        params: contact_shadows::ContactShadowParams,
    ) -> Result<(), HalError> {
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `contact-shadows` pass and returns the image the forward pass
    /// should bind.
    ///
    /// `depth` must be the prepass's stored depth and `mask` is where the march
    /// writes. The image is the caller's so it can declare the read on the pass
    /// that consumes it: a pass declares its own accesses and this one cannot
    /// declare the forward pass's.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        depth: ImageId,
        mask: ImageId,
    ) -> ImageId {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let uniforms = self.uniforms[frame];
        let cached = &mut self.groups[frame];

        graph
            .add_render_pass(LABEL)
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target, so loading or clearing it is pure bandwidth.
            .color(
                mask,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            // The prepass left this in `DepthStencilWrite`. Declaring the read
            // is what moves it to a shader-readable layout, and without it every
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
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(1, view)],
                    "contact shadows depth",
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

        mask
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
