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
//! # Per-pass constants are a tier decision
//!
//! `ui.slang` needs one thing from the CPU each pass — the framebuffer size it
//! divides by to reach NDC — and **WebGPU has no push constants at all**. That
//! is the Tier A / Tier B split `docs/plan/03-gpu-driven-rendering.md`
//! describes, arriving at its smallest possible scale: the same renderer, two
//! ways of delivering the same eight bytes, chosen from
//! [`Features::PUSH_CONSTANTS`].
//!
//! * **Tier A** ([`ConstantDelivery::PushConstants`]) — a
//!   [`PushConstantRange`] on the pipeline layout and one
//!   [`push_constants`](crcbl_hal::CommandEncoder::push_constants) per pass.
//!   Nothing about this path changed when Tier B arrived, deliberately: it is
//!   the cheaper one and it is what native wants.
//! * **Tier B** ([`ConstantDelivery::UniformBuffer`]) — one small uniform
//!   buffer per frame in flight, bound at binding 3 of the frame's existing
//!   bind group. The pass records no `push_constants` at all; the block is
//!   written on the CPU while the pass body records.
//!
//! **Not** a dynamic offset, even though the seam has `dynamic_offsets` on
//! [`bind_group`](crcbl_hal::CommandEncoder::bind_group) and
//! [`BindingKind::UniformBuffer`] documents it as *the* Tier B substitute. A
//! dynamic offset buys one buffer sliced per draw; this pass has exactly one
//! draw per frame and already owns one bind group per frame in flight, so the
//! offset would always be zero and the only thing it would add is a way to get
//! it wrong. P7's per-bucket constants are where that vocabulary earns its
//! keep, and the layout here does not foreclose it — `dynamic: false` becomes
//! `dynamic: true` and the offset joins the `bind_group` call.
//!
//! # Open: the Tier B shader artifact does not exist yet
//!
//! The two paths need two *shader* spellings of the same block, and one Slang
//! source cannot carry both: an entry point either reads a `[[vk::push_constant]]`
//! block or a `[[vk::binding]]`ed constant buffer, and whichever it reads is
//! what the pipeline layout must provide. So the tier is a **permutation axis**
//! — `03-gpu-driven-rendering.md`'s own words — and the permutation is a second
//! artifact named `ui_tier_b`.
//!
//! That artifact is **not committed**, because `slangc` was not available on the
//! machine that wrote this and the committed artifacts are verified byte-for-byte
//! against `crates/crcbl-shaders/spirv/manifest.txt`; hand-written ones would be
//! a lie the manifest could not catch. Until it lands, the Tier B path resolves
//! back to `ui`'s artifacts and logs a warning naming this gap — a Tier B
//! *device* will then fail at shader-module or pipeline creation rather than
//! draw a mis-scaled UI.
//!
//! Note that this is not a regression the Tier B path introduced: `ui.wgsl` as
//! committed today declares `var<uniform> constants_0` with **no** `@group` or
//! `@binding`, which is invalid WGSL and which `naga` rejects, so the UI pass has
//! never had a working browser shader.
//!
//! The follow-up is mechanical and needs only a machine with the pinned
//! `slangc`. Add `crates/crcbl-shaders/shaders/ui_tier_b.slang`, which is
//! `ui.slang` with exactly one declaration changed:
//!
//! ```text
//! // ui.slang (Tier A)                 ui_tier_b.slang (Tier B)
//! [[vk::push_constant]]                [[vk::binding(3, 0)]]
//! ConstantBuffer<UiConstants>          ConstantBuffer<UiConstants>
//!     constants;                           constants;
//! ```
//!
//! then run `crates/crcbl-shaders/tools/compile-shaders.sh` and commit
//! `spirv/ui_tier_b.spv`, `wgsl/ui_tier_b.wgsl` and the regenerated
//! `spirv/manifest.txt`. Nothing in this file changes: the shader is resolved by
//! name out of `crcbl_shaders::ALL`, so the moment the artifact exists the
//! warning stops and the Tier B pipeline is built from it. `ui.slang` itself is
//! untouched, so `spirv/ui.spv` stays byte-identical and Tier A cannot move.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendState, BufferDesc,
    BufferHandle, BufferImageCopy, BufferUsage, ColorTargetState, ColorWrites, CommandEncoderDesc,
    Device, Extent3d, Features, FilterMode, Format, GraphicsPipelineDesc, GraphicsPipelineHandle,
    HalError, ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageType,
    ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, IndexFormat, LoadOp, MemoryLocation,
    Offset3d, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState, PushConstantRange,
    QueueHandle, ResourceState, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry,
    ShaderModuleDesc, ShaderStages, StoreOp, SubmitInfo,
};

use crcbl_shaders::{Stage, UI};
use crcbl_ui::draw_list::{DrawList, Vertex2d};
use crcbl_ui::text::FontAtlas;

use crate::graph::{ImageId, RenderGraph};

/// The constant block matching `ui.slang`'s `UiConstants`.
///
/// `viewport` is the framebuffer size in pixels (width, height). The shader
/// divides position by viewport and maps to NDC.
///
/// The same eight bytes go down both delivery paths: a push-constant write on
/// Tier A, a uniform-buffer write on Tier B. One type rather than two, so the
/// two paths cannot disagree about the layout the shader reads.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct UiConstants {
    viewport: [f32; 2],
}

/// How the UI pass delivers `ui.slang`'s `UiConstants` block to the shader.
///
/// Chosen from the device's capabilities by
/// [`ConstantDelivery::for_device`], never by the caller: a game asking for
/// push constants on a browser would be asking for something that does not
/// exist. See this module's docs for the full reasoning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstantDelivery {
    /// **Tier A.** A [`PushConstantRange`] on the pipeline layout, written with
    /// [`push_constants`](crcbl_hal::CommandEncoder::push_constants) once per
    /// pass. Requires [`Features::PUSH_CONSTANTS`].
    PushConstants,
    /// **Tier B.** A uniform buffer per frame in flight, bound at
    /// [`CONSTANTS_BINDING`] of the frame's bind group. What WebGPU — which has
    /// no push constants at all — gets instead.
    UniformBuffer,
}

impl ConstantDelivery {
    /// What `device` can actually do.
    #[must_use]
    pub fn for_device(device: &dyn Device) -> Self {
        if device.caps().features.contains(Features::PUSH_CONSTANTS) {
            Self::PushConstants
        } else {
            Self::UniformBuffer
        }
    }

    /// The name of the `crcbl-shaders` artifact that spells the constants this
    /// way.
    ///
    /// The tier is a permutation axis, and the permutation is a whole artifact:
    /// see this module's docs on why one Slang entry point cannot carry both.
    #[must_use]
    pub const fn shader_name(self) -> &'static str {
        match self {
            Self::PushConstants => "ui",
            Self::UniformBuffer => "ui_tier_b",
        }
    }
}

/// The binding number the Tier B constants buffer occupies, after the atlas
/// (0), its sampler (1) and the vertex storage buffer (2).
pub const CONSTANTS_BINDING: u32 = 3;

/// Bytes reserved for one frame's Tier B constants buffer.
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
    atlas_image: ImageHandle,
    atlas_view: ImageViewHandle,
    atlas_sampler: SamplerHandle,

    // Per-frame bind groups (each contains atlas+sampler+vertex_buffer, plus
    // the constants buffer on Tier B)
    frame_groups: Vec<BindGroupHandle>,
    vertex_buffers: Vec<BufferHandle>,
    index_buffers: Vec<BufferHandle>,
    frame: usize,

    /// How this renderer hands the shader its viewport size.
    delivery: ConstantDelivery,

    /// One constants buffer per frame in flight on
    /// [`ConstantDelivery::UniformBuffer`], and **empty** on
    /// [`ConstantDelivery::PushConstants`] — so the Tier A path allocates
    /// nothing for a thing it does not use, and `constants_buffer` below
    /// returns `None` without a second flag to keep in step with this one.
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
    /// A device without [`Features::PUSH_CONSTANTS`] is **not** an error: it
    /// gets [`ConstantDelivery::UniformBuffer`] instead, which is the whole
    /// point of the tier split and the only reason a browser can run this pass
    /// at all. Ask [`constant_delivery`](Self::constant_delivery) which one it
    /// got.
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
        let delivery = ConstantDelivery::for_device(device);
        let atlas = FontAtlas::built_in();
        let (atlas_w, atlas_h, atlas_pixels) = atlas.glyph_bitmap();

        // Upload glyph atlas texture via staging.
        let (atlas_image, atlas_view) =
            upload_texture_r8(device, queue, atlas_w, atlas_h, &atlas_pixels)?;
        rollback.images.push(atlas_image);
        rollback.image_views.push(atlas_view);

        let atlas_sampler = device.create_sampler(&SamplerDesc {
            label: Some("ui glyph atlas"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(atlas_sampler);

        // Bind group layout: atlas texture, sampler, vertex storage buffer —
        // and, on Tier B only, the constants uniform buffer.
        let layout_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage,
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::Sampler,
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
        let declared = match delivery {
            ConstantDelivery::PushConstants => 3,
            ConstantDelivery::UniformBuffer => 4,
        };
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("ui pass"),
            entries: &layout_entries[..declared],
        })?;
        rollback.bind_group_layouts.push(bind_group_layout);

        // Per-frame bind groups (atlas/sampler are static, the rest rotate)
        let mut frame_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut vertex_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut index_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut constant_buffers = Vec::new();
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
            let cb = match delivery {
                ConstantDelivery::PushConstants => None,
                ConstantDelivery::UniformBuffer => {
                    let cb = device.create_buffer(&BufferDesc {
                        label: Some("ui constants"),
                        size: CONSTANTS_UNIFORM_SIZE,
                        usage: BufferUsage::UNIFORM,
                        memory: MemoryLocation::HostUpload,
                    })?;
                    rollback.buffers.push(cb);
                    constant_buffers.push(cb);
                    Some(cb)
                }
            };
            let (entries, used) = frame_entries(atlas_view, atlas_sampler, vb, cb);
            let bg = device.create_bind_group(&BindGroupDesc {
                label: Some("ui frame"),
                layout: bind_group_layout,
                entries: &entries[..used],
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
            // The block is a push-constant range only where push constants
            // exist. A backend without them must refuse this range rather than
            // drop the writes silently, which is exactly what the null backend
            // and `crcbl-wgpu` do — so asking for it unconditionally is how the
            // browser used to lose its HUD.
            push_constants: match delivery {
                ConstantDelivery::PushConstants => Some(PushConstantRange {
                    stages: ShaderStages::VERTEX,
                    offset: 0,
                    size: std::mem::size_of::<UiConstants>() as u32,
                }),
                ConstantDelivery::UniformBuffer => None,
            },
        })?;
        rollback.pipeline_layouts.push(pipeline_layout);

        // Entry points resolved before the module exists: a manifest that
        // disagreed with the artifact would otherwise fail inside the descriptor
        // literal, with the module already created and nothing holding it.
        let shader = ui_shader(delivery);
        let vertex_entry = entry(shader, Stage::Vertex)?;
        let fragment_entry = entry(shader, Stage::Fragment)?;
        let module_label = format!("{}.slang", shader.name());
        let ui_module = device.create_shader_module(&ShaderModuleDesc {
            label: Some(&module_label),
            spirv: shader.spirv(),
            wgsl: shader.wgsl(),
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
            atlas_image,
            atlas_view,
            atlas_sampler,
            frame_groups,
            vertex_buffers,
            index_buffers,
            frame: 0,
            delivery,
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

    /// How this renderer hands `ui.slang` its viewport size.
    ///
    /// Decided from the device at construction and fixed for the renderer's
    /// life. Exposed because it is the observable half of the tier split: a
    /// test — or a debug overlay — can ask which path a device took without
    /// re-deriving it from [`DeviceCaps`](crcbl_hal::DeviceCaps).
    #[must_use]
    pub const fn constant_delivery(&self) -> ConstantDelivery {
        self.delivery
    }

    /// This frame's constants buffer, on the tier that has one.
    fn constants_buffer(&self) -> Option<BufferHandle> {
        self.constant_buffers.get(self.frame).copied()
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
        self.last_vertex_count[idx] = vertices.len();
        self.last_index_count[idx] = indices.len();

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

        // Only a new vertex buffer needs a new bind group; the atlas and the
        // sampler never change, so a steady-state frame writes no descriptors.
        if vertex_buffer_replaced {
            let (entries, used) = frame_entries(
                self.atlas_view,
                self.atlas_sampler,
                self.vertex_buffers[idx],
                self.constant_buffers.get(idx).copied(),
            );
            let fresh = device.create_bind_group(&BindGroupDesc {
                label: Some("ui frame"),
                layout: self.bind_group_layout,
                entries: &entries[..used],
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
    /// `extent` is also the *only* source of the viewport constants on either
    /// tier, which is why the Tier B uniform buffer is written here rather than
    /// in [`begin_frame`](Self::begin_frame): a second extent taken a second
    /// time is a second thing that can disagree.
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
        // `None` on Tier A, and the *only* thing that distinguishes the two
        // paths in the body below.
        let constants = self.constants_buffer();

        graph
            .add_render_pass("ui-composite")
            // Draw on top of the tonemapped target with alpha blending.
            .color(target, LoadOp::Load, StoreOp::Store, Default::default())
            .execute(move |ctx| {
                let block = UiConstants {
                    viewport: [extent.0 as f32, extent.1 as f32],
                };
                let bytes: &[u8] = bytemuck::bytes_of(&block);
                if let Some(buffer) = constants {
                    // A host-visible write, not a command: it lands before this
                    // frame is submitted, and the buffer is one of
                    // `FRAMES_IN_FLIGHT` — the same rotation that makes the
                    // vertex ring safe makes this safe.
                    if let Err(error) = ctx.device().write_buffer(buffer, 0, bytes) {
                        // Recording a pass that draws nothing beats aborting the
                        // frame, as in the tonemap's bind-group path: the HUD
                        // vanishes for a frame, the log says why, the next frame
                        // retries.
                        log::error!("graph: ui constants write failed: {error}");
                        return;
                    }
                }
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, bg, &[], pipeline_layout);
                if constants.is_none() {
                    // Tier A: the viewport goes down the push-constant block.
                    encoder.push_constants(ShaderStages::VERTEX, 0, bytes, pipeline_layout);
                }
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
        // Empty on Tier A, so this is a no-op there rather than a branch.
        for cb in self.constant_buffers.drain(..) {
            device.destroy_buffer(cb);
        }
        device.destroy_sampler(self.atlas_sampler);
        device.destroy_image_view(self.atlas_view);
        device.destroy_image(self.atlas_image);
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
    image_views: Vec<ImageViewHandle>,
    images: Vec<ImageHandle>,
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
        for handle in self.image_views {
            device.destroy_image_view(handle);
        }
        for handle in self.images {
            device.destroy_image(handle);
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

/// One frame's bind-group entries, and how many of them this tier declares.
///
/// One function rather than the two copies `new` and `begin_frame` used to
/// carry: they build the *same* group, and a Tier B binding added to one and
/// forgotten in the other is a bind group that stops matching its layout the
/// first time the vertex ring grows.
///
/// The fourth entry is written unconditionally and sliced off on Tier A —
/// [`BindGroupEntry`] has no "absent" resource to spell, and a `Vec` here would
/// allocate on a path that runs whenever the ring grows.
fn frame_entries(
    atlas_view: ImageViewHandle,
    atlas_sampler: SamplerHandle,
    vertices: BufferHandle,
    constants: Option<BufferHandle>,
) -> ([BindGroupEntry; 4], usize) {
    let mut entries = [
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
            // Placeholder, replaced below when there is a constants buffer and
            // never reached when there is not.
            resource: BindingResource::whole_buffer(vertices),
        },
    ];
    match constants {
        Some(buffer) => {
            entries[3].resource = BindingResource::whole_buffer(buffer);
            (entries, 4)
        }
        None => (entries, 3),
    }
}

/// The shader artifact that spells the constants the way `delivery` delivers
/// them.
///
/// Resolved **by name** out of [`crcbl_shaders::ALL`] rather than by naming a
/// static, so the Tier B permutation can land as an artifact alone: the moment
/// `crates/crcbl-shaders/shaders/ui_tier_b.slang` and its compiled outputs are
/// committed, this finds them with no change here. See this module's docs for
/// the exact follow-up.
///
/// Until then the Tier B path falls back to `ui`, whose SPIR-V takes a push
/// constant and whose WGSL declares an unbound uniform. That is a pipeline a
/// real Tier B device will refuse — loudly, at module or pipeline creation —
/// which is the honest outcome, and it is what keeps a `NullBackend` test able
/// to exercise the Rust half of the path today.
fn ui_shader(delivery: ConstantDelivery) -> &'static crcbl_shaders::Shader {
    let wanted = delivery.shader_name();
    if let Some(shader) = crcbl_shaders::ALL
        .iter()
        .copied()
        .find(|shader| shader.name() == wanted)
    {
        return shader;
    }
    log::warn!(
        "ui pass: this device has no push constants and `{wanted}` is not among the compiled \
         shaders, so the pass falls back to `ui`, which delivers its viewport through a push \
         constant. Add crates/crcbl-shaders/shaders/{wanted}.slang and run \
         crates/crcbl-shaders/tools/compile-shaders.sh — see crcbl_render::ui_pass's docs."
    );
    &UI
}

/// Uploads an R8_UNORM texture via a staging buffer copy.
///
/// Every object this creates is released on every path out, including the
/// failing ones: a `?` that dropped the staging buffer on the floor would leak
/// one per failed startup, and the recorder's leak assertions would only notice
/// once something actually failed.
fn upload_texture_r8(
    device: &dyn Device,
    queue: QueueHandle,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(ImageHandle, ImageViewHandle), HalError> {
    // Rows are padded to the device's copy alignment rather than packed
    // tightly. Vulkan takes either; WebGPU requires a 256-byte row pitch and
    // says so through this limit, so padding here is what makes one upload path
    // work on both instead of one that only ever ran on the backend it was
    // written against. `R8Unorm` is one byte per texel, so bytes and texels are
    // the same number throughout.
    let alignment = device
        .caps()
        .limits
        .optimal_buffer_copy_offset_alignment
        .max(1);
    let row_pitch = u64::from(width).next_multiple_of(alignment);
    let size = row_pitch * u64::from(height);
    let mut padded = vec![0u8; usize::try_from(size).unwrap_or(usize::MAX)];
    for row in 0..height as usize {
        let src = row * width as usize;
        let dst = row * row_pitch as usize;
        padded[dst..dst + width as usize].copy_from_slice(&pixels[src..src + width as usize]);
    }

    let staging = device.create_buffer(&BufferDesc {
        label: Some("ui atlas staging"),
        size,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    })?;
    let row_texels = u32::try_from(row_pitch).unwrap_or(u32::MAX);
    let outcome = upload_atlas_image(device, queue, width, height, row_texels, staging, &padded);
    device.destroy_buffer(staging);
    outcome
}

/// The half of [`upload_texture_r8`] that owns the image and the view.
fn upload_atlas_image(
    device: &dyn Device,
    queue: QueueHandle,
    width: u32,
    height: u32,
    row_texels: u32,
    staging: BufferHandle,
    pixels: &[u8],
) -> Result<(ImageHandle, ImageViewHandle), HalError> {
    device.write_buffer(staging, 0, pixels)?;

    let image = device.create_image(&ImageDesc {
        label: Some("ui glyph atlas"),
        image_type: ImageType::D2,
        format: Format::R8Unorm,
        extent: Extent3d::d2(width, height),
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
        memory: MemoryLocation::DeviceLocal,
    })?;

    let view = match device.create_image_view(&ImageViewDesc {
        label: Some("ui glyph atlas"),
        image,
        view_type: ImageViewType::D2,
        format: Format::R8Unorm,
        range: ImageSubresourceRange {
            aspect: crcbl_hal::ImageAspect::COLOR,
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        },
    }) {
        Ok(view) => view,
        Err(error) => {
            device.destroy_image(image);
            return Err(error);
        }
    };

    match record_atlas_upload(device, queue, image, staging, width, height, row_texels) {
        Ok(()) => Ok((image, view)),
        Err(error) => {
            device.destroy_image_view(view);
            device.destroy_image(image);
            Err(error)
        }
    }
}

/// Records, submits and drains the staging copy.
fn record_atlas_upload(
    device: &dyn Device,
    queue: QueueHandle,
    image: ImageHandle,
    staging: BufferHandle,
    width: u32,
    height: u32,
    row_texels: u32,
) -> Result<(), HalError> {
    let range = ImageSubresourceRange {
        aspect: crcbl_hal::ImageAspect::COLOR,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    };

    // Upload via staging copy with barriers
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ui atlas upload"),
        queue,
    });

    // Transition image: Undefined → TransferDst
    encoder.pipeline_barrier(&crcbl_hal::Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            image,
            range,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Default::default()
    });

    encoder.copy_buffer_to_image(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: row_texels,
        buffer_image_height: height,
        image,
        image_subresource: ImageSubresourceLayers {
            aspect: crcbl_hal::ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(width, height),
    });

    // Transition image: TransferDst → ShaderRead
    encoder.pipeline_barrier(&crcbl_hal::Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            image,
            range,
            ResourceState::TransferDst,
            ResourceState::ShaderRead,
        )],
        ..Default::default()
    });

    let commands = encoder.finish()?;
    let submitted = device
        .submit(queue, &SubmitInfo::new(&[commands]))
        .and_then(|()| device.wait_idle());
    device.destroy_command_buffer(commands);
    submitted
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
        let instance = NullInstance::tier_a();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::TIER_A,
                optional_features: Features::PUSH_CONSTANTS,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    #[test]
    fn ui_renderer_builds_and_leaks_nothing() {
        let (device, queue) = open();
        let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the null backend accepts everything");
        renderer.destroy(device.as_ref());
    }

    #[test]
    fn begin_frame_with_empty_draw_list_is_harmless() {
        let (device, queue) = open();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();
        let dl = DrawList::new();
        renderer
            .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
            .expect("empty draw list upload should succeed");
        renderer.destroy(device.as_ref());
    }

    #[test]
    fn begin_frame_with_rect_uploads_geometry() {
        let (device, queue) = open();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.rect(
            glam::Vec2::new(10.0, 20.0),
            glam::Vec2::new(110.0, 120.0),
            [1.0, 0.0, 0.0, 1.0],
        );
        renderer
            .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
            .expect("rect upload should succeed");
        renderer.destroy(device.as_ref());
    }

    #[test]
    fn begin_frame_with_text_uploads_geometry() {
        let (device, queue) = open();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();
        let mut dl = DrawList::new();
        dl.text(
            glam::Vec2::new(10.0, 10.0),
            "Hello",
            [1.0, 1.0, 1.0, 1.0],
            14.0,
        );
        renderer
            .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
            .expect("text upload should succeed");
        renderer.destroy(device.as_ref());
    }

    /// A UI that has not changed must not churn the GPU: the byte counts and
    /// the element counts used to be compared against each other, so both ring
    /// buffers *and* the frame bind group were destroyed and recreated every
    /// single frame in steady state.
    #[test]
    fn a_steady_state_frame_recreates_nothing() {
        let recorder = crcbl_hal::null::Recorder::new();
        let instance = NullInstance::tier_a().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::TIER_A,
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

    fn open_tier_b() -> (Box<dyn Device>, QueueHandle) {
        let instance = NullInstance::tier_b();
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

    /// **A device without push constants is no longer refused.** WebGPU has
    /// none at all, and the UI pass is where breakout's score, lives and game
    /// state are drawn — so the tier without them gets the uniform-buffer path
    /// rather than an error.
    #[test]
    fn a_device_without_push_constants_gets_the_uniform_path() {
        let (device, queue) = open_tier_b();
        let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the tier B path exists precisely so this succeeds");
        assert_eq!(
            renderer.constant_delivery(),
            ConstantDelivery::UniformBuffer
        );
        assert_eq!(
            renderer.constant_buffers.len(),
            FRAMES_IN_FLIGHT,
            "one constants buffer per frame in flight, or two frames share one"
        );
        renderer.destroy(device.as_ref());
    }

    /// And a device *with* them still takes the cheaper path, with nothing
    /// allocated for the buffer it does not use.
    #[test]
    fn a_device_with_push_constants_keeps_the_push_path() {
        let (device, queue) = open();
        let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the null backend accepts everything");
        assert_eq!(
            renderer.constant_delivery(),
            ConstantDelivery::PushConstants
        );
        assert!(
            renderer.constant_buffers.is_empty(),
            "Tier A must not allocate a uniform buffer it never binds"
        );
        renderer.destroy(device.as_ref());
    }

    /// The Tier B renderer builds, uploads and tears down with no GPU and no
    /// leak — the same sweep the Tier A one gets, on the path that used to be
    /// an early `return Err`.
    #[test]
    fn the_tier_b_renderer_leaks_nothing() {
        let recorder = crcbl_hal::null::Recorder::new();
        let instance = NullInstance::tier_b().with_recorder(recorder.clone());
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

    /// A Tier B frame that grows its ring rebuilds a bind group that still
    /// names the constants buffer — the entry `new` and `begin_frame` used to
    /// spell twice, and could therefore spell differently.
    #[test]
    fn growing_the_tier_b_ring_keeps_the_constants_bound() {
        let (device, queue) = open_tier_b();
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
        assert!(renderer.constants_buffer().is_some());
        renderer.destroy(device.as_ref());
    }

    /// Both tiers ask for a shader by name, and Tier A's name resolves to the
    /// artifact that is actually committed. Tier B's does not yet — which is
    /// the documented gap, and this is what will notice when it closes.
    #[test]
    fn the_push_path_resolves_the_committed_ui_artifact() {
        assert_eq!(ConstantDelivery::PushConstants.shader_name(), "ui");
        assert_eq!(
            ui_shader(ConstantDelivery::PushConstants).name(),
            "ui",
            "the Tier A pass must never silently fall back to another artifact"
        );
        assert_eq!(ConstantDelivery::UniformBuffer.shader_name(), "ui_tier_b");
    }
}
