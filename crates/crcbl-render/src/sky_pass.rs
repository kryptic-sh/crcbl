//! `docs/plan/43-render-standards.md` §8's second half: the pass that **draws**
//! the sky, where §8's first half only lit and reflected with it.
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
//! # Two skies, one pass and one pipeline
//!
//! [`crate::Sky`] is the gradient and [`crate::Atmosphere`] is Hillaire's, and
//! `sky.slang` picks between them on one lane of its block. The atmosphere is
//! **not evaluated on the device at all**: the scattering integral is marched
//! on the host into a [`crcbl_shaders::atmosphere::SkyView`], which arrives
//! here as a storage buffer, and the fragment stage reads one bilinear tap out
//! of it. So the two skies cost the same four loads and a blend, and no
//! transcendental reaches a colour on either arm.
//!
//! **A storage buffer rather than a sampled image**, which is the one design
//! choice in this file worth stating: the LUT is rebuilt whenever the sun
//! moves, and an image would have to be uploaded through a staging copy and a
//! device idle to change, where a mapped buffer is a `write_buffer` into the
//! ring this pass already keeps. It also settles the filter the way
//! [`crate::forward`] settles the `DFG` table's — spelled out in the shader
//! rather than asked of a sampler, because a hardware filter's weights are
//! fixed-function arithmetic four rasterisers compute independently.
//!
//! The buffer is bound on **every** frame, holding zeroes on the frames whose
//! sky is a gradient or which have none: a binding that came and went with the
//! arm would be two pipeline layouts and two pipelines.
//!
//! # It is a caller's opt-in and not a [`RenderEffects`](crate::RenderEffects)
//! bit
//!
//! A frame with neither a sky nor an atmosphere adds no pass at all, on
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
use crcbl_shaders::{SKY, atmosphere, sky};

use crate::graph::{ImageId, RenderGraph};

/// Vertices in the over-sized full-screen triangle `sky.slang` generates from
/// `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// The binding `sky.slang` reads the sky-view LUT through.
///
/// **Appended past the block, never inserted**, on [`crate::forward`]'s binding
/// constants' reason: `crcbl-mtl` numbers a resource by counting the
/// same-table entries of the layout list and Slang numbers a stage's arguments
/// by declaration order, so the two agree only while both ascend.
/// `msl/sky.metal` puts it at `buffer(1)`, the next free index of that table.
const SKY_VIEW_BINDING: u32 = 1;

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
    /// `[frame]`: `sky.slang`'s sky-view LUT, one per frame in flight for
    /// [`SkyPass::uniforms`]' reason exactly.
    ///
    /// Written whole on every frame that has an atmosphere and left holding
    /// zeroes otherwise — `crcbl_shaders::atmosphere::SKY_VIEW_BUFFER_BYTES` is
    /// under a hundred kilobytes, which is a memory copy rather than a cost.
    luts: Vec<BufferHandle>,
    /// `[frame]`: the group naming [`SkyPass::uniforms`] and [`SkyPass::luts`]
    /// at the same index.
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
        let entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: SKY_VIEW_BINDING,
                // **`VERTEX` beside `FRAGMENT`**, on [`crate::forward`]'s
                // `SPECULAR_DFG_BINDING`'s reason: Slang's Metal backend
                // materialises every global into every entry point, so
                // `msl/sky.metal`'s `vertexMain` takes `sky_view [[buffer(1)]]`
                // whether it reads it or not.
                visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
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
        let mut luts = Vec::with_capacity(frames);
        let mut groups = Vec::with_capacity(frames);
        for _ in 0..frames {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("sky params"),
                size: sky::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            uniforms.push(buffer);
            let lut = device.create_buffer(&BufferDesc {
                label: Some("sky view lut"),
                size: atmosphere::SKY_VIEW_BUFFER_BYTES as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            // Zeroed once here rather than left as whatever the allocation
            // held. Nothing reads it on a gradient frame — the shader branches
            // before the first load — but a buffer whose contents depend on
            // what was in that memory is one a debugger and a validation layer
            // both have something to say about.
            device.write_buffer(lut, 0, &vec![0u8; atmosphere::SKY_VIEW_BUFFER_BYTES])?;
            luts.push(lut);
            groups.push(device.create_bind_group(&BindGroupDesc {
                label: Some("sky"),
                layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(buffer),
                    },
                    BindGroupEntry {
                        binding: SKY_VIEW_BINDING,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(lut),
                    },
                ],
                variable_count: None,
            })?);
        }

        Ok(Self {
            uniforms,
            luts,
            groups,
            layout,
            pipeline_layout,
            pipeline,
        })
    }

    /// Writes this frame's block: the two inverses that turn a pixel into a
    /// world-space ray, the gradient that ray is evaluated against, and — when
    /// the frame has one — the sky-view LUT and the sun it was built around.
    ///
    /// The matrices are the ones [`crate::ssr`]'s block already carries, and
    /// they arrive already inverted for that reason — one inversion per frame
    /// feeds both passes.
    ///
    /// `sky_view` is `None` on a gradient frame, and then the block's sun row
    /// is [`sky::ATMOSPHERE_OFF`] and the LUT buffer is left alone — so a frame
    /// blessed before an atmosphere existed writes exactly the bytes it used
    /// to.
    ///
    /// **The LUT is written on every atmosphere frame** rather than only when
    /// it changes, which is a memory copy of
    /// [`atmosphere::SKY_VIEW_BUFFER_BYTES`] against a per-frame slot ring
    /// whose slots would otherwise have to be tracked for staleness
    /// individually. The march that produced it is orders of magnitude dearer
    /// and is what [`crate::forward`] caches.
    ///
    /// # Errors
    ///
    /// [`HalError`] from either mapped write.
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
        sky_view: Option<&atmosphere::SkyView>,
    ) -> Result<(), HalError> {
        let params = sky::SkyParams {
            inv_proj,
            inv_view,
            sky: gradient.rows(),
            atmosphere: match sky_view {
                Some(view) => {
                    let sun = view.sun_direction();
                    [sun[0], sun[1], sun[2], sky::ATMOSPHERE_ON]
                }
                None => [0.0, 0.0, 0.0, sky::ATMOSPHERE_OFF],
            },
        };
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())?;
        if let Some(view) = sky_view {
            device.write_buffer(self.luts[frame], 0, &view.rows())?;
        }
        Ok(())
    }

    /// The sky-view LUT this frame's slot holds — the buffer
    /// [`SkyPass::begin_frame`] writes [`atmosphere::SkyView::rows`] into.
    ///
    /// **Handed to [`crate::ssr`] rather than copied for it.** A mirror
    /// reflects the sky the background draws, so the reflection pass reads this
    /// very buffer; a second ring would be a second
    /// [`atmosphere::SKY_VIEW_BUFFER_BYTES`] write on every atmosphere frame
    /// and one more place for the two skies to disagree. Nothing on the device
    /// writes it — both passes only read, and the write is a host upload — so
    /// there is no barrier for the graph to be told about and neither pass
    /// declares it.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn lut(&self, frame: usize) -> BufferHandle {
        self.luts[frame]
    }

    /// The sky-view LUT buffers, one per frame in flight, in slot order.
    ///
    /// Test-only, on `crate::ssao::Ssao::blocks`' terms: the reference backend
    /// can be asked what bytes a buffer holds, and the LUT that reaches the
    /// device is the only place `crate::forward`'s striped march is visible —
    /// `a_moving_sun_is_marched_a_stripe_per_frame` is what reads it.
    #[cfg(test)]
    pub(crate) fn luts(&self) -> &[BufferHandle] {
        &self.luts
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
        for buffer in self.uniforms.into_iter().chain(self.luts) {
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
