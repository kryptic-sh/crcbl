//! UI compositing pass: renders a [`DrawList`] on top of the target.
//!
//! ```text
//! UiRenderer ──begin_frame──▶ uploads vertex/index buffers from DrawList
//!      │
//!      └──add_pass──▶ inserts an alpha-blended render pass into the graph
//!                       after the tonemap, drawing the UI on top of the target
//! ```
//!
//! The UI pass uses the same target as the tonemap pass, compositing on top
//! with alpha blending. The glyph atlas is a static R8_UNORM texture uploaded
//! once at creation.
//!
//! # Per-pass constants are a uniform buffer, on every tier
//!
//! `ui.slang` needs one thing from the CPU each pass — the framebuffer size it
//! divides by to reach NDC — and it takes it from a uniform buffer bound at
//! [`CONSTANTS_BINDING`] of the frame's existing bind group. One per frame in
//! flight, written on the CPU while the pass body records; the pass records no
//! `push_constants` at all.
//!
//! **That is a deliberate refusal of a tier split.** A push constant would
//! deliver the same eight bytes with one indirection fewer, and **WebGPU has no
//! push constants at all** — so it would have made this pass the smallest
//! possible instance of `docs/plan/03-gpu-driven-rendering.md`'s Tier A / Tier B
//! axis, chosen from
//! [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS). It did,
//! until 2026-08: there was a `ConstantDelivery` enum here, two branches through
//! every layout and buffer this file creates, and — because one Slang entry
//! point reads either a `[[vk::push_constant]]` block or a `[[vk::binding]]`ed
//! one, never both — a whole second shader source, `ui_tier_b.slang`, kept in
//! step with `ui.slang` by a comment. `sprite.slang` had already declined the same trade for the same
//! reason; `crates/crcbl-shaders/shaders/ui.slang`'s header carries the
//! argument. What is left is one source, one artifact per target, and one path
//! through this file.
//!
//! **Not** a dynamic offset, even though the seam has `dynamic_offsets` on
//! [`bind_group`](crcbl_hal::CommandEncoder::bind_group) and
//! [`BindingKind::UniformBuffer`] documents it as *the* portable substitute for
//! a push constant. A dynamic offset buys one buffer sliced per draw; this pass
//! has exactly one draw per frame and already owns one bind group per frame in
//! flight, so the offset would always be zero and the only thing it would add is
//! a way to get it wrong. P7's per-bucket constants are where that vocabulary
//! earns its keep, and the layout here does not foreclose it — `dynamic: false`
//! becomes `dynamic: true` and the offset joins the `bind_group` call.
//!
//! `wgsl/ui.wgsl` declares `@binding(3) @group(0) var<uniform> constants_0`,
//! which is what makes it WGSL a browser will accept: the push-constant form
//! lowered to a module-scope `var<uniform>` with no `@group`/`@binding` at all,
//! which `naga` rejects outright, so the only backend that ingests WGSL —
//! `crcbl-wgpu`, which deliberately never reports
//! [`PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS) on any target — could
//! not create the module.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendState, BufferDesc,
    BufferHandle, BufferUsage, ColorTargetState, ColorWrites, Device, FilterMode, Format,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageViewHandle, ImageViewType,
    IndexFormat, LoadOp, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState,
    QueueHandle, SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry,
    ShaderModuleDesc, ShaderStages, StoreOp,
};

use crcbl_shaders::{Stage, UI};
use crcbl_ui::draw_list::{DrawList, Vertex2d};
use crcbl_ui::text::FontAtlas;

use crate::graph::{ImageId, RenderGraph};
use crate::texture::{UploadedTexture, upload_texture};

/// The constant block matching `ui.slang`'s `UiConstants`.
///
/// `viewport` is the framebuffer size in pixels (width, height). The shader
/// divides position by viewport and maps to NDC.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct UiConstants {
    viewport: [f32; 2],
}

/// The binding number the constants buffer occupies, after the atlas (0), its
/// sampler (1) and the vertex storage buffer (2).
pub const CONSTANTS_BINDING: u32 = 3;

/// Bytes reserved for one frame's constants buffer.
///
/// Sixteen rather than the eight [`UiConstants`] occupies: WGSL rounds a
/// `uniform` struct's size up to a multiple of 16, so a binding sized to the
/// Rust struct is one naga validation error rather than a saving. The trailing
/// bytes are never written and never read.
const CONSTANTS_UNIFORM_SIZE: u64 = 16;

/// How many frames of vertex/index buffers to keep in flight.
const FRAMES_IN_FLIGHT: usize = 2;

/// Starting size of each ring buffer, in bytes.
const INITIAL_RING_BYTES: u64 = 1024;

/// The size a ring buffer grows to when `needed` bytes no longer fit.
///
/// Doubling rather than fitting exactly, so a UI that grows a few vertices per
/// frame reallocates a handful of times rather than every frame.
fn grown(needed: u64) -> u64 {
    needed
        .max(INITIAL_RING_BYTES)
        .next_power_of_two()
        .next_multiple_of(256)
}

/// The UI compositing renderer.
///
/// Created once; `begin_frame` uploads the current frame's geometry, and
/// `add_pass` inserts the draw pass into the graph.
#[derive(Debug)]
pub struct UiRenderer {
    // Pipeline state
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    bind_group_layout: BindGroupLayoutHandle,

    // Glyph atlas
    atlas: UploadedTexture,
    atlas_sampler: SamplerHandle,

    // Per-frame bind groups (each contains atlas+sampler+vertex_buffer+constants)
    frame_groups: Vec<BindGroupHandle>,
    vertex_buffers: Vec<BufferHandle>,
    index_buffers: Vec<BufferHandle>,
    frame: usize,

    /// One constants buffer per frame in flight, rotating with the geometry
    /// rings so a frame still in flight cannot have its viewport overwritten.
    constant_buffers: Vec<BufferHandle>,

    /// How many **bytes** each ring buffer holds. Compared against the bytes a
    /// frame needs; the counts below are *elements* and comparing the two is
    /// what used to make a steady-state frame destroy and recreate both
    /// buffers and the bind group every time.
    vertex_capacity: Vec<u64>,
    index_capacity: Vec<u64>,

    // Element counts, for the draw call and for "is there anything to draw".
    last_vertex_count: Vec<usize>,
    last_index_count: Vec<usize>,

    /// The format the pipeline was built for. Dynamic rendering checks the
    /// pipeline's colour-target format against the attachment at pass-begin, so
    /// a swapchain in a different format needs a different pipeline.
    target_format: Format,

    destroyed: bool,
}

impl UiRenderer {
    /// Creates the UI pipeline, glyph atlas, and per-frame geometry buffers.
    ///
    /// `target_format` must be the format of the image the pass composites
    /// onto — normally the swapchain's, which is `Bgra8UnormSrgb` on most
    /// desktop platforms. Under dynamic rendering the pipeline's colour-target
    /// format is checked against the attachment at pass-begin time rather than
    /// at creation, so a pipeline built for the wrong one fails the frame, not
    /// the constructor. [`ForwardRenderer::new`](crate::ForwardRenderer::new)
    /// takes it for the same reason.
    ///
    /// The atlas is uploaded immediately via a staging copy.
    ///
    /// This pass asks for no device feature of its own — its constants are a
    /// uniform buffer, which every target has — so a browser and a native
    /// Vulkan device build the same pipeline from the same artifact.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. A failure part-way through releases
    /// everything already created, so a caller that retries or exits leaves
    /// nothing behind.
    pub fn new(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
    ) -> Result<Self, HalError> {
        let mut rollback = Rollback::default();
        match Self::build(device, queue, target_format, &mut rollback) {
            Ok(renderer) => Ok(renderer),
            Err(error) => {
                rollback.run(device);
                Err(error)
            }
        }
    }

    /// The body of [`UiRenderer::new`], recording what it has created into
    /// `rollback` as it goes.
    fn build(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        let atlas = FontAtlas::built_in();
        let (atlas_w, atlas_h, atlas_pixels) = atlas.glyph_bitmap();

        // Upload glyph atlas texture via staging.
        let atlas = upload_texture(
            device,
            queue,
            "ui glyph atlas",
            Format::R8Unorm,
            atlas_w,
            atlas_h,
            &atlas_pixels,
        )?;
        rollback.textures.push(atlas);

        let atlas_sampler = device.create_sampler(&SamplerDesc {
            label: Some("ui glyph atlas"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(atlas_sampler);

        // Bind group layout: atlas texture, sampler, vertex storage buffer and
        // the constants uniform buffer.
        let layout_entries = [
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
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX,
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: CONSTANTS_BINDING,
                visibility: ShaderStages::VERTEX,
                // `dynamic: false`: one buffer per frame in flight, bound whole.
                // See this module's docs on why a dynamic offset would only ever
                // be zero here.
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("ui pass"),
            entries: &layout_entries,
        })?;
        rollback.bind_group_layouts.push(bind_group_layout);

        // Per-frame bind groups (atlas/sampler are static, the rest rotate)
        let mut frame_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut vertex_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut index_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut constant_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut last_vertex_count = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut last_index_count = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut vertex_capacity = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut index_capacity = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for _ in 0..FRAMES_IN_FLIGHT {
            let vb = device.create_buffer(&BufferDesc {
                label: Some("ui vertices"),
                size: INITIAL_RING_BYTES,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(vb);
            let ib = device.create_buffer(&BufferDesc {
                label: Some("ui indices"),
                size: INITIAL_RING_BYTES,
                usage: BufferUsage::INDEX,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(ib);
            let cb = device.create_buffer(&BufferDesc {
                label: Some("ui constants"),
                size: CONSTANTS_UNIFORM_SIZE,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(cb);
            constant_buffers.push(cb);
            let bg = device.create_bind_group(&BindGroupDesc {
                label: Some("ui frame"),
                layout: bind_group_layout,
                entries: &frame_entries(atlas.view, atlas_sampler, vb, cb),
                variable_count: None,
            })?;
            rollback.bind_groups.push(bg);
            vertex_buffers.push(vb);
            index_buffers.push(ib);
            frame_groups.push(bg);
            last_vertex_count.push(0);
            last_index_count.push(0);
            vertex_capacity.push(INITIAL_RING_BYTES);
            index_capacity.push(INITIAL_RING_BYTES);
        }

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ui"),
            bind_group_layouts: &set_layouts,
            // No range at all: the block arrives through the bind group, and a
            // backend without push constants must refuse a range rather than
            // drop the writes silently — which is exactly what the null backend
            // and `crcbl-wgpu` do, and is how the browser used to lose its HUD.
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(pipeline_layout);

        // Entry points resolved before the module exists: a manifest that
        // disagreed with the artifact would otherwise fail inside the descriptor
        // literal, with the module already created and nothing holding it.
        let vertex_entry = entry(&UI, Stage::Vertex)?;
        let fragment_entry = entry(&UI, Stage::Fragment)?;
        let ui_module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("ui.slang"),
            spirv: UI.spirv(),
            wgsl: UI.wgsl(),
            msl: UI.msl(),
            // One container per entry point, both of them, in one module; see
            // `crcbl_render::forward`.
            dxil: &UI.dxil_containers(),
        })?;
        let ui_targets = [ColorTargetState {
            format: target_format,
            blend: Some(BlendState::alpha()),
            write_mask: ColorWrites::ALL,
        }];
        let ui_pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("ui compositing"),
            layout: pipeline_layout,
            vertex: ShaderEntry {
                module: ui_module,
                entry_point: vertex_entry,
            },
            fragment: Some(ShaderEntry {
                module: ui_module,
                entry_point: fragment_entry,
            }),
            primitive: PrimitiveState::default(), // TriangleList, no culling
            depth_stencil: None,
            multisample: Default::default(),
            color_targets: &ui_targets,
        });
        device.destroy_shader_module(ui_module);
        let pipeline = ui_pipeline?;
        rollback.pipelines.push(pipeline);

        Ok(Self {
            pipeline_layout,
            pipeline,
            bind_group_layout,
            atlas,
            atlas_sampler,
            frame_groups,
            vertex_buffers,
            index_buffers,
            frame: 0,
            constant_buffers,
            vertex_capacity,
            index_capacity,
            last_vertex_count,
            last_index_count,
            target_format,
            destroyed: false,
        })
    }

    /// The format the compositing pass renders into.
    #[must_use]
    pub const fn target_format(&self) -> Format {
        self.target_format
    }

    /// Uploads the draw list's triangulated geometry and advances the ring.
    ///
    /// Call once per frame before `add_pass`. If the draw list is empty, the
    /// frame stores zero geometry and the pass will draw nothing.
    ///
    /// # Errors
    ///
    /// [`HalError`] if buffer upload failed.
    pub fn begin_frame(
        &mut self,
        device: &dyn Device,
        draw_list: &DrawList,
        atlas: &FontAtlas,
        scale: f32,
    ) -> Result<(), HalError> {
        self.frame = (self.frame + 1) % FRAMES_IN_FLIGHT;
        let idx = self.frame;

        let (vertices, indices) = draw_list.to_triangles(Some(atlas), scale);

        // Grow the ring buffers only when this frame genuinely needs more room.
        // Both sides of every comparison here are **bytes**.
        let vb_needed = (vertices.len() * std::mem::size_of::<Vertex2d>()) as u64;
        let ib_needed = (indices.len() * std::mem::size_of::<u32>()) as u64;

        // The vertex buffer is named by the frame's bind group, so replacing it
        // is the only thing that makes the group stale.
        let mut vertex_buffer_replaced = false;
        if vb_needed > self.vertex_capacity[idx] {
            let size = grown(vb_needed);
            // Create before destroying: a creation that fails must not leave a
            // destroyed handle in the struct for `destroy` to hand back to the
            // device a second time. `TransientPool::image` has the same shape.
            let fresh = device.create_buffer(&BufferDesc {
                label: Some("ui vertices"),
                size,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            device.destroy_buffer(std::mem::replace(&mut self.vertex_buffers[idx], fresh));
            self.vertex_capacity[idx] = size;
            vertex_buffer_replaced = true;
        }
        if ib_needed > self.index_capacity[idx] {
            let size = grown(ib_needed);
            let fresh = device.create_buffer(&BufferDesc {
                label: Some("ui indices"),
                size,
                usage: BufferUsage::INDEX,
                memory: MemoryLocation::HostUpload,
            })?;
            device.destroy_buffer(std::mem::replace(&mut self.index_buffers[idx], fresh));
            self.index_capacity[idx] = size;
        }
        // Upload vertex data
        if !vertices.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&vertices);
            device.write_buffer(self.vertex_buffers[idx], 0, bytes)?;
        }

        // Upload index data
        if !indices.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&indices);
            device.write_buffer(self.index_buffers[idx], 0, bytes)?;
        }

        // The counts describe the bytes just written: committing them only after
        // the uploads succeeded keeps a failed write from drawing new counts over
        // stale indices (a Vulkan OOB index read).
        self.last_vertex_count[idx] = vertices.len();
        self.last_index_count[idx] = indices.len();

        // Only a new vertex buffer needs a new bind group; the atlas and the
        // sampler never change, so a steady-state frame writes no descriptors.
        if vertex_buffer_replaced {
            let entries = frame_entries(
                self.atlas.view,
                self.atlas_sampler,
                self.vertex_buffers[idx],
                self.constant_buffers[idx],
            );
            let fresh = device.create_bind_group(&BindGroupDesc {
                label: Some("ui frame"),
                layout: self.bind_group_layout,
                entries: &entries,
                variable_count: None,
            })?;
            device.destroy_bind_group(std::mem::replace(&mut self.frame_groups[idx], fresh));
        }

        Ok(())
    }

    /// Adds the UI compositing pass to `graph`, drawing on top of `target`.
    ///
    /// The pass reads nothing except its own vertex buffer; it blends onto the
    /// target using alpha blending. Call after the tonemap pass.
    ///
    /// `extent` is the target's size in pixels, which the shader divides by to
    /// reach NDC. It is *not* used to set the viewport or the scissor: the
    /// graph already sets both from the pass's own render area
    /// ([`CompiledGraph::execute`](crate::graph::CompiledGraph::execute)), and a
    /// body that set them again could only disagree with it.
    ///
    /// `extent` is also the *only* source of the viewport constants, which is
    /// why the uniform buffer is written here rather than in
    /// [`begin_frame`](Self::begin_frame): a second extent taken a second time
    /// is a second thing that can disagree.
    pub fn add_pass<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        target: ImageId,
        extent: (u32, u32),
    ) {
        if self.last_vertex_count[self.frame] == 0 || self.last_index_count[self.frame] == 0 {
            return; // nothing to draw
        }

        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let bg = self.frame_groups[self.frame];
        let index_buffer = self.index_buffers[self.frame];
        let index_count = self.last_index_count[self.frame] as u32;
        let constants = self.constant_buffers[self.frame];

        graph
            .add_render_pass("ui-composite")
            // Draw on top of the tonemapped target with alpha blending.
            .color(target, LoadOp::Load, StoreOp::Store, Default::default())
            .execute(move |ctx| {
                let block = UiConstants {
                    viewport: [extent.0 as f32, extent.1 as f32],
                };
                let bytes: &[u8] = bytemuck::bytes_of(&block);
                // A host-visible write, not a command: it lands before this
                // frame is submitted, and the buffer is one of
                // `FRAMES_IN_FLIGHT` — the same rotation that makes the vertex
                // ring safe makes this safe.
                if let Err(error) = ctx.device().write_buffer(constants, 0, bytes) {
                    // Recording a pass that draws nothing beats aborting the
                    // frame, as in the tonemap's bind-group path: the HUD
                    // vanishes for a frame, the log says why, the next frame
                    // retries.
                    log::error!("graph: ui constants write failed: {error}");
                    return;
                }
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, bg, &[], pipeline_layout);
                encoder.bind_index_buffer(index_buffer, 0, IndexFormat::Uint32);
                encoder.draw_indexed(0..index_count, 0, 0..1);
            });
    }

    /// Destroys all GPU resources.
    ///
    /// The device must be idle.
    pub fn destroy(mut self, device: &dyn Device) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        for bg in self.frame_groups.drain(..) {
            device.destroy_bind_group(bg);
        }
        for vb in self.vertex_buffers.drain(..) {
            device.destroy_buffer(vb);
        }
        for ib in self.index_buffers.drain(..) {
            device.destroy_buffer(ib);
        }
        for cb in self.constant_buffers.drain(..) {
            device.destroy_buffer(cb);
        }
        device.destroy_sampler(self.atlas_sampler);
        self.atlas.destroy(device);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.bind_group_layout);
    }
}

impl Drop for UiRenderer {
    fn drop(&mut self) {
        if !self.destroyed {
            log::warn!("UiRenderer dropped without calling destroy() — GPU resources leaked");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// What a partly-built [`UiRenderer`] has to give back.
///
/// `build` creates a dozen objects with `?` between them and the seam's
/// `destroy_*` is explicit, so a failure half way through would otherwise leak
/// everything created before it — a glyph atlas and two rings per failed
/// start-up. [`crate::forward`] carries the same shape for the same reason.
#[derive(Default)]
struct Rollback {
    buffers: Vec<BufferHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    pipelines: Vec<GraphicsPipelineHandle>,
    samplers: Vec<SamplerHandle>,
    textures: Vec<UploadedTexture>,
}

impl Rollback {
    /// Releases everything, in the same dependency order as
    /// [`UiRenderer::destroy`].
    fn run(self, device: &dyn Device) {
        for handle in self.bind_groups {
            device.destroy_bind_group(handle);
        }
        for handle in self.buffers {
            device.destroy_buffer(handle);
        }
        for handle in self.samplers {
            device.destroy_sampler(handle);
        }
        for texture in self.textures {
            texture.destroy(device);
        }
        for handle in self.pipelines {
            device.destroy_graphics_pipeline(handle);
        }
        for handle in self.pipeline_layouts {
            device.destroy_pipeline_layout(handle);
        }
        for handle in self.bind_group_layouts {
            device.destroy_bind_group_layout(handle);
        }
    }
}

/// One frame's bind-group entries.
///
/// One function rather than the two copies `new` and `begin_frame` used to
/// carry: they build the *same* group, and a binding added to one and forgotten
/// in the other is a bind group that stops matching its layout the first time
/// the vertex ring grows.
fn frame_entries(
    atlas_view: ImageViewHandle,
    atlas_sampler: SamplerHandle,
    vertices: BufferHandle,
    constants: BufferHandle,
) -> [BindGroupEntry; 4] {
    [
        BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: BindingResource::ImageView(atlas_view),
        },
        BindGroupEntry {
            binding: 1,
            array_index: 0,
            resource: BindingResource::Sampler(atlas_sampler),
        },
        BindGroupEntry {
            binding: 2,
            array_index: 0,
            resource: BindingResource::whole_buffer(vertices),
        },
        BindGroupEntry {
            binding: CONSTANTS_BINDING,
            array_index: 0,
            resource: BindingResource::whole_buffer(constants),
        },
    ]
}

fn entry(shader: &crcbl_shaders::Shader, stage: Stage) -> Result<&'static str, HalError> {
    shader.entry_point(stage).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "{}.slang exposes no unambiguous {stage:?} entry point",
            shader.name()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::NullInstance;
    use crcbl_hal::{DeviceDesc, Features, Instance, QueueKind};

    fn open() -> (Box<dyn Device>, QueueHandle) {
        let instance = NullInstance::gpu_driven();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::PUSH_CONSTANTS,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    /// [`open`] with a recorder attached, for the tests whose claim is about
    /// what the renderer did rather than what it returned.
    fn open_recorded() -> (crcbl_hal::null::Recorder, Box<dyn Device>, QueueHandle) {
        let recorder = crcbl_hal::null::Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::PUSH_CONSTANTS,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (recorder, device, queue)
    }

    /// Bytes written into the current frame's vertex and index rings, read off
    /// the recorded stream.
    ///
    /// Zero for a ring nothing was written to, which is the case an `Ok` from
    /// [`UiRenderer::begin_frame`] cannot tell apart from a full upload.
    fn uploaded(recorder: &crcbl_hal::null::Recorder, renderer: &UiRenderer) -> (usize, usize) {
        use crcbl_hal::null::Event;

        let vertices = renderer.vertex_buffers[renderer.frame];
        let indices = renderer.index_buffers[renderer.frame];
        let mut written = (0, 0);
        for event in recorder.events() {
            if let Event::BufferWritten {
                buffer,
                offset,
                len,
            } = event
            {
                assert_eq!(offset, 0, "a ring is written from its start");
                if buffer == vertices {
                    written.0 += len;
                } else if buffer == indices {
                    written.1 += len;
                }
            }
        }
        written
    }

    /// Everything [`UiRenderer::new`] created, [`UiRenderer::destroy`] hands
    /// back.
    ///
    /// The recorder is what makes the second half of that a claim: without it
    /// a leaked sampler, pipeline layout or bind-group layout is invisible with
    /// no GPU, and this test asserted nothing at all.
    #[test]
    fn ui_renderer_builds_and_leaks_nothing() {
        let (recorder, device, queue) = open_recorded();
        let before = recorder.total_live_objects();
        let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the null backend accepts everything");
        assert!(
            recorder.total_live_objects() > before,
            "a renderer that created nothing would also leak nothing"
        );
        renderer.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "destroy must give back every object new took"
        );
        recorder.assert_valid();
    }

    /// The glyph atlas upload moved to [`crate::texture`] and must not have
    /// changed on the way: one byte per texel, the atlas's own extent, and
    /// `Undefined → TransferDst → ShaderRead` around the copy.
    ///
    /// The image's *format* is not observable through the recorder — it logs a
    /// kind and a label, not the descriptor — so the staging write's length
    /// stands in for it: `R8Unorm` writes `width * height`, and the same call
    /// with `Rgba8Unorm` would write four times that.
    ///
    /// The atlas is 768 texels wide, which is already a multiple of Tier A's
    /// 4-byte copy alignment, so *this* upload pads nothing and the numbers are
    /// spelled out rather than recomputed. The padding itself is exercised in
    /// [`crate::texture`]'s own tests, against Tier B's 256-byte alignment.
    #[test]
    fn the_glyph_atlas_is_still_an_r8_upload_at_the_same_pitch() {
        use crcbl_hal::null::{Command, Event};
        use crcbl_hal::{Extent3d, Offset3d, ResourceState};

        let recorder = crcbl_hal::null::Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::PUSH_CONSTANTS,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");

        let (atlas_w, atlas_h, atlas_pixels) = FontAtlas::built_in().glyph_bitmap();
        assert_eq!((atlas_w, atlas_h), (768, 13));
        assert_eq!(atlas_pixels.len(), 768 * 13);
        assert_eq!(
            768 % device.caps().limits.optimal_buffer_copy_offset_alignment,
            0,
            "the pitch below is the unpadded width only because the row is already aligned"
        );

        let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the null backend accepts everything");

        let written = recorder
            .events()
            .into_iter()
            .find_map(|event| match event {
                Event::BufferWritten { len, .. } => Some(len),
                _ => None,
            })
            .expect("the atlas staging buffer is written before any frame buffer");
        assert_eq!(
            written,
            768 * 13,
            "one byte per texel: the same call with an Rgba8Unorm atlas would write four times this"
        );

        let commands = recorder.commands();
        let copy = commands
            .iter()
            .find_map(|command| match command {
                Command::CopyBufferToImage(copy) => Some(*copy),
                _ => None,
            })
            .expect("the atlas is uploaded with one buffer-to-image copy");
        assert_eq!(
            copy.buffer_row_length, 768,
            "R8 is one byte per texel, so the texel pitch equals the byte pitch"
        );
        assert_eq!(copy.buffer_image_height, atlas_h);
        assert_eq!(copy.image_extent, Extent3d::d2(atlas_w, atlas_h));
        assert_eq!(copy.image_offset, Offset3d { x: 0, y: 0, z: 0 });

        let transitions: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Barrier { images, .. } => Some(images.clone()),
                _ => None,
            })
            .flatten()
            .map(|barrier| (barrier.from, barrier.to))
            .collect();
        assert_eq!(
            transitions,
            [
                (ResourceState::Undefined, ResourceState::TransferDst),
                (ResourceState::TransferDst, ResourceState::ShaderRead),
            ],
            "the atlas is the only barrier the UI renderer's construction records"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// An empty draw list writes no bytes and leaves the frame with nothing to
    /// draw.
    ///
    /// The `Ok` this used to assert comes back from an upload of any size,
    /// including one that wrote the *previous* frame's geometry again and left
    /// its counts in place for the draw call to read.
    #[test]
    fn an_empty_draw_list_uploads_no_bytes_and_leaves_the_counts_at_zero() {
        let (recorder, device, queue) = open_recorded();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();
        // The atlas upload is `new`'s, not this frame's.
        recorder.clear();

        let dl = DrawList::new();
        renderer
            .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
            .expect("empty draw list upload should succeed");

        assert_eq!(uploaded(&recorder, &renderer), (0, 0));
        assert_eq!(renderer.last_vertex_count[renderer.frame], 0);
        assert_eq!(renderer.last_index_count[renderer.frame], 0);
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Both primitives really reach the rings, and they are not the same
    /// geometry.
    ///
    /// Two tests here once asserted that `begin_frame` returned `Ok`, which a
    /// `begin_frame` that returned before touching a buffer does too. The
    /// observable is the byte count the recorder saw, derived from the
    /// tessellation rather than spelled out — a literal would rot the first
    /// time [`Vertex2d`] grew a field. And the two cases must disagree: two
    /// primitives asserting one number is one case written twice.
    #[test]
    fn a_rect_and_a_line_of_text_each_upload_exactly_the_geometry_they_tessellate_to() {
        let atlas = FontAtlas::built_in();

        let mut rect = DrawList::new();
        rect.rect(
            glam::Vec2::new(10.0, 20.0),
            glam::Vec2::new(110.0, 120.0),
            [1.0, 0.0, 0.0, 1.0],
        );
        let mut text = DrawList::new();
        text.text(
            glam::Vec2::new(10.0, 10.0),
            "Hello",
            [1.0, 1.0, 1.0, 1.0],
            14.0,
        );

        let mut sizes = Vec::new();
        for (what, dl) in [("a rect", &rect), ("a line of text", &text)] {
            let (recorder, device, queue) = open_recorded();
            let mut renderer =
                UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
            recorder.clear();

            renderer
                .begin_frame(device.as_ref(), dl, &atlas, 1.0)
                .expect("upload should succeed");

            let (vertices, indices) = dl.to_triangles(Some(&atlas), 1.0);
            let expected = (
                vertices.len() * std::mem::size_of::<Vertex2d>(),
                indices.len() * std::mem::size_of::<u32>(),
            );
            assert!(
                expected.0 > 0 && expected.1 > 0,
                "{what} tessellates to nothing, so this case asserts nothing"
            );
            assert_eq!(uploaded(&recorder, &renderer), expected, "{what}");
            assert_eq!(renderer.last_vertex_count[renderer.frame], vertices.len());
            assert_eq!(renderer.last_index_count[renderer.frame], indices.len());

            sizes.push(expected);
            renderer.destroy(device.as_ref());
            recorder.assert_valid();
        }

        assert_eq!(sizes.len(), 2, "both primitives were measured");
        assert_ne!(
            sizes[0], sizes[1],
            "one quad and five glyphs are not the same geometry"
        );
    }

    /// A UI that has not changed must not churn the GPU: the byte counts and
    /// the element counts used to be compared against each other, so both ring
    /// buffers *and* the frame bind group were destroyed and recreated every
    /// single frame in steady state.
    #[test]
    fn a_steady_state_frame_recreates_nothing() {
        let recorder = crcbl_hal::null::Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::PUSH_CONSTANTS,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.text(
            glam::Vec2::new(10.0, 10.0),
            "steady",
            [1.0, 1.0, 1.0, 1.0],
            14.0,
        );

        // Two frames to fill both slots of the ring, then measure.
        for _ in 0..FRAMES_IN_FLIGHT {
            renderer
                .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
                .expect("upload");
        }
        let buffers = renderer.vertex_buffers.clone();
        let groups = renderer.frame_groups.clone();
        let settled = recorder.total_live_objects();

        for _ in 0..8 {
            renderer
                .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
                .expect("upload");
        }
        assert_eq!(
            recorder.total_live_objects(),
            settled,
            "an unchanged draw list must not allocate"
        );
        assert_eq!(
            renderer.vertex_buffers, buffers,
            "the vertex ring must be reused, not reallocated"
        );
        assert_eq!(
            renderer.frame_groups, groups,
            "the frame bind group only changes when its vertex buffer does"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// The ring still grows when a frame genuinely needs more room, and the old
    /// buffer is released rather than leaked.
    #[test]
    fn a_bigger_draw_list_grows_the_ring_once() {
        let (device, queue) = open();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();

        let mut small = DrawList::new();
        small.rect(
            glam::Vec2::ZERO,
            glam::Vec2::new(10.0, 10.0),
            [1.0, 1.0, 1.0, 1.0],
        );
        renderer
            .begin_frame(device.as_ref(), &small, &atlas, 1.0)
            .expect("upload");
        let before = renderer.vertex_capacity[renderer.frame];

        let mut big = DrawList::new();
        for index in 0..512 {
            let x = index as f32;
            big.rect(
                glam::Vec2::new(x, 0.0),
                glam::Vec2::new(x + 1.0, 1.0),
                [1.0, 1.0, 1.0, 1.0],
            );
        }
        // Two frames so the same ring slot comes round again.
        for _ in 0..FRAMES_IN_FLIGHT {
            renderer
                .begin_frame(device.as_ref(), &big, &atlas, 1.0)
                .expect("upload");
        }
        assert!(
            renderer.vertex_capacity[renderer.frame] > before,
            "the ring must grow when the frame no longer fits"
        );
        renderer.destroy(device.as_ref());
    }

    /// A device that reports no [`Features::PUSH_CONSTANTS`] — what a browser
    /// is, and what this pass used to need a second shader artifact for.
    fn open_portable() -> (Box<dyn Device>, QueueHandle) {
        let instance = NullInstance::portable();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::COMPUTE,
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the tier B null adapter opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    /// **Both devices build the same renderer**, each with one constants buffer
    /// per frame in flight. A device that reports push constants gets no
    /// different treatment from one that does not, which is the whole of what
    /// deleting `ConstantDelivery` was for: the two used to differ in the
    /// pipeline layout, the bind-group layout, the buffers allocated, the
    /// commands recorded *and* the shader artifact resolved.
    #[test]
    fn push_constants_or_not_the_renderer_is_the_same() {
        for (device, queue) in [open(), open_portable()] {
            let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
                .expect("neither device is refused");
            assert_eq!(
                renderer.constant_buffers.len(),
                FRAMES_IN_FLIGHT,
                "one constants buffer per frame in flight, or two frames share one"
            );
            renderer.destroy(device.as_ref());
        }
    }

    /// The renderer builds, uploads and tears down with no GPU and no leak on a
    /// device with no push constants — the path that was once an early
    /// `return Err`, and then a second shader artifact.
    #[test]
    fn the_portable_renderer_leaks_nothing() {
        let recorder = crcbl_hal::null::Recorder::new();
        let instance = NullInstance::portable().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::COMPUTE,
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the portable null adapter opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let before = recorder.total_live_objects();

        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.text(glam::Vec2::new(4.0, 4.0), "score", [1.0; 4], 14.0);
        for _ in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
                .expect("upload");
        }
        renderer.destroy(device.as_ref());
        assert_eq!(recorder.total_live_objects(), before);
        recorder.assert_valid();
    }

    /// A frame that grows its ring rebuilds a bind group that still names the
    /// constants buffer — the entry `new` and `begin_frame` used to spell twice,
    /// and could therefore spell differently.
    #[test]
    fn growing_the_ring_keeps_the_constants_bound() {
        let (device, queue) = open_portable();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();

        let mut big = DrawList::new();
        for index in 0..512 {
            let x = index as f32;
            big.rect(
                glam::Vec2::new(x, 0.0),
                glam::Vec2::new(x + 1.0, 1.0),
                [1.0; 4],
            );
        }
        for _ in 0..FRAMES_IN_FLIGHT {
            renderer
                .begin_frame(device.as_ref(), &big, &atlas, 1.0)
                .expect("upload");
        }
        assert!(renderer.vertex_capacity[renderer.frame] > INITIAL_RING_BYTES);
        // A bind group that had stopped matching its layout would have been
        // refused by the null backend's descriptor check, not merely wrong.
        assert_eq!(
            renderer.constant_buffers.len(),
            FRAMES_IN_FLIGHT,
            "the rebuilt group still names a constants buffer per frame"
        );
        renderer.destroy(device.as_ref());
    }
}
