//! `docs/plan/43-render-standards.md` §8's second half: the pass that **draws**
//! the gradient sky, where §8's first half only lit and reflected with it.
//!
//! [`crcbl_shaders::sky`] projects a [`crate::Sky`] into an L1 probe for the
//! forward pass's ambient term and hands the raw gradient to `ssr.slang` for a
//! reflection that hit nothing. Both are the sky seen *by a surface*. Neither
//! puts a texel on the screen, so until this pass existed a frame lit by a
//! bright sky still had [`crate::SCENE_CLEAR`] behind it — one flat colour with
//! no relationship to the sky in front of it.
//!
//! # The hardware decides which pixels are background
//!
//! `sky.slang` emits its full-screen triangle at
//! [`crcbl_hal::depth::CLEAR`] — the reversed-Z far plane — and the pipeline
//! tests [`crcbl_hal::DepthStencilState::equal_depth_read_only`] against the depth the
//! forward pass stored, with writes off. A pixel geometry covered holds a depth
//! strictly greater than zero and fails; a pixel nothing covered holds exactly
//! the clear value and passes. So this pass binds no depth texture, samples
//! nothing, and has no `discard` in it: the same silicon that rejected the
//! hidden fragments rejects these.
//!
//! The colour attachment is therefore **loaded**, not cleared or discarded —
//! this pass writes the background alone and has to leave every shaded pixel
//! exactly as the forward pass left it.
//!
//! # It is a caller's opt-in and not a [`RenderEffects`](crate::RenderEffects)
//! bit
//!
//! A frame whose sky is [`crate::Sky::NONE`] adds no pass at all, on
//! [`crate::grid`]'s terms: no pipeline bound, no block written, and the
//! background is the clear colour every frame drawn before this module existed
//! had. That is what makes the off position bit-identical, and it is also the
//! honest reading — a black gradient drawn over the clear colour would be a
//! *change* to those frames rather than an absence.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle, BufferUsage, ClearValue,
    ColorTargetState, Device, Format, GraphicsPipelineHandle, HalError, LoadOp, MemoryLocation,
    PipelineLayoutDesc, PipelineLayoutHandle, ShaderStages, StoreOp,
    check_portable_storage_buffers,
};
use crcbl_shaders::{SKY, sky};

use crate::graph::{ImageId, RenderGraph};

/// Vertices in the over-sized full-screen triangle `sky.slang` generates from
/// `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// Everything the sky pass owns.
///
/// The shape every other full-screen pass in this crate has — see
/// [`crate::fxaa`] — with one simplification: the bind group names a buffer and
/// nothing else, so it is built once per frame slot rather than cached against
/// a graph transient's view.
#[derive(Debug)]
pub(crate) struct SkyPass {
    /// `[frame]`: `sky.slang`'s uniform block, one per frame in flight for the
    /// frame uniforms' reason — the previous frame may still be reading last
    /// frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    /// `[frame]`: the group naming [`SkyPass::uniforms`] at the same index.
    groups: Vec<BindGroupHandle>,
    layout: crcbl_hal::BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
}

impl SkyPass {
    /// Passes [`SkyPass::add_pass`] adds to a frame that has a sky.
    pub(crate) const PASSES: u64 = 1;

    /// Builds the pipeline, the uniform ring and the groups that name it.
    ///
    /// `build_tested_fullscreen` is handed in rather than duplicated, on
    /// [`crate::fxaa::Fxaa::new`]'s terms: it is [`crate::forward`]'s, because
    /// the module lookup and the destroy-before-unwrap are that file's.
    ///
    /// `color_format` is the scene target's and `depth_format` the prepass's.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the
    /// caller holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        color_format: Format,
        depth_format: Format,
        build_tested_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
            Format,
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        // A uniform buffer rather than a push constant, for the reason every
        // post pass in this crate gives: WebGPU has no push constants, and one
        // Slang entry point cannot read both a push-constant block and a bound
        // one, so a range here would fork the shader.
        let entries = [BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::UniformBuffer { dynamic: false },
            count: 1,
            flags: BindingFlags::empty(),
        }];
        let desc = BindGroupLayoutDesc {
            label: Some("sky"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("sky"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("sky"),
            bind_group_layouts: &[layout],
            push_constants: None,
        })?;
        let pipeline = build_tested_fullscreen(
            device,
            "sky",
            &SKY,
            pipeline_layout,
            &[ColorTargetState::opaque(color_format)],
            depth_format,
        )?;

        let mut uniforms = Vec::with_capacity(frames);
        let mut groups = Vec::with_capacity(frames);
        for _ in 0..frames {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("sky params"),
                size: sky::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            uniforms.push(buffer);
            groups.push(device.create_bind_group(&BindGroupDesc {
                label: Some("sky"),
                layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(buffer),
                }],
                variable_count: None,
            })?);
        }

        Ok(Self {
            uniforms,
            groups,
            layout,
            pipeline_layout,
            pipeline,
        })
    }

    /// Writes this frame's block: the two inverses that turn a pixel into a
    /// world-space ray, and the gradient that ray is evaluated against.
    ///
    /// The matrices are the ones [`crate::ssr`]'s block already carries, and
    /// they arrive already inverted for that reason — one inversion per frame
    /// feeds both passes.
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
        inv_proj: [f32; 16],
        inv_view: [f32; 16],
        gradient: &crcbl_shaders::sky::SkyGradient,
    ) -> Result<(), HalError> {
        let params = sky::SkyParams {
            inv_proj,
            inv_view,
            sky: gradient.rows(),
        };
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `sky` pass, drawing into `color` where `depth` is still the far
    /// plane.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_pass<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        color: ImageId,
        depth: ImageId,
    ) {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let group = self.groups[frame];

        graph
            .add_render_pass("sky")
            // **Loaded**, where every other full-screen pass in this crate
            // takes `DontCare`: those write every pixel of their target and
            // this one writes the background alone.
            .color(color, LoadOp::Load, StoreOp::Store, ClearValue::default())
            // Tested and not written — see the module docs. The graph is what
            // moves the attachment out of whatever state the forward pass left
            // it in and back for the passes that sample it afterwards.
            .depth_read(depth)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for group in self.groups {
            device.destroy_bind_group(group);
        }
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The vertex stage emits its triangle at the value the depth attachment is
    /// cleared to, which is the whole of what makes the read-only
    /// `GreaterOrEqual` test select the pixels no geometry covered.
    ///
    /// Both halves are asserted here because this is the one crate that can see
    /// both: `crcbl-shaders` has no dependencies and cannot name
    /// [`crcbl_hal::depth::CLEAR`], and `crcbl-hal` has never heard of the
    /// shader. A triangle emitted anywhere else draws over the frame or over
    /// nothing at all, and neither is an error on any backend.
    #[test]
    fn the_triangle_sits_exactly_at_the_far_plane() {
        let source = include_str!("../../crcbl-shaders/shaders/sky.slang");
        let emitted = source
            .split_once(
                "output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0),",
            )
            .expect("sky.slang emits its triangle from `uv`")
            .1
            .split_once(',')
            .expect("the position has a `z` and a `w`")
            .0
            .trim()
            .parse::<f32>()
            .expect("the `z` is a literal");
        assert_eq!(
            emitted,
            crcbl_hal::depth::CLEAR,
            "sky.slang emits its triangle at {emitted}, which is no longer the reversed-Z far \
             plane the depth test compares it against"
        );
    }

    /// [`super::SkyPass::PASSES`] is what [`crate::forward`] adds to the frame's
    /// recorded full-screen count, so it has to be the number of
    /// `add_render_pass` calls in [`super::SkyPass::add_pass`].
    #[test]
    fn the_declared_pass_count_is_the_one_the_body_adds() {
        let source = include_str!("sky_pass.rs");
        let body = source
            .split_once("pub(crate) fn add_pass<'a>(")
            .expect("this file declares `add_pass`")
            .1
            .split_once("\n    }")
            .expect("the function has a body")
            .0;
        let added = body.matches("add_render_pass(").count() as u64;
        assert_eq!(added, super::SkyPass::PASSES);
    }
}
