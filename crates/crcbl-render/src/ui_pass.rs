//! UI compositing pass: renders a [`DrawList`] on top of the target.
//!
//! ```text
//! UiRenderer ──begin_frame──▶ uploads vertex/index buffers from DrawList,
//!      │                     rasterising the glyphs its runs miss, and stages
//!      │                     what changed in the image atlas and glyph pages
//!      │
//!      └──add_passes──▶ [ui-images] ─▶ [ui-glyphs] ─▶ ui-composite ─▶ ui-overlay
//!                       a copy when either atlas changed, then two
//!                       alpha-blended passes onto the same target, after the
//!                       tonemap
//! ```
//!
//! The UI pass uses the same target as the tonemap pass, compositing on top
//! with alpha blending. The bitmap font's atlas is a static R8_UNORM texture
//! uploaded once at creation.
//!
//! # Glyph pages upload what changed, the same way
//!
//! [`DrawList::glyphs`] runs draw from the renderer's own [`GlyphAtlas`], which
//! [`begin_frame`](UiRenderer::begin_frame) starts a frame of and rasterises into
//! while it tessellates. Its pages are the layers of one `R8Unorm` `D2Array`
//! image bound at [`GLYPH_PAGES_BINDING`] — every page the atlas may open,
//! allocated at start-up, because a WebGPU texture cannot grow in place and one
//! image is one binding on every backend. What a frame rasterised is staged per
//! page as a dirty rectangle and copied by a `ui-glyphs` copy pass, exactly as
//! the image atlas's `ui-images` is.
//!
//! # The image atlas uploads what changed, inside the frame
//!
//! Beside the glyph atlas the pass binds a second page: the
//! [`ImageAtlas`] this renderer owns, `Rgba8UnormSrgb` and sampled through a
//! linear sampler, which is what [`DrawList::image`] and
//! [`DrawList::nine_slice`] draw from. The whole page is uploaded once at
//! creation, like the glyphs. An image registered after that — through
//! [`images_mut`](UiRenderer::images_mut) — marks a rectangle of the page
//! dirty, and the next [`begin_frame`](UiRenderer::begin_frame) stages just that
//! rectangle into a host-visible buffer. [`add_passes`](UiRenderer::add_passes)
//! then records the copy as a `ui-images` copy pass ahead of the draws, with the
//! page imported so the graph emits the transitions around it: a copy recorded
//! mid-frame outside [`RenderGraph`] would be a barrier outside the graph, which
//! is the one rule `crate`'s docs forbid. A frame with nothing dirty stages
//! nothing, imports nothing and adds no pass.
//!
//! # The menu is in the draw list
//!
//! A [`DrawList`] carries the game's HUD **and** the engine's overlays — the
//! menu, the debug panel, the console — and [`DrawList::begin_overlay`] marks
//! where one ends and the other begins. The menu's art is registered in this
//! renderer's image atlas at start-up ([`menu_skin`](UiRenderer::menu_skin)),
//! and [`Menu::render`](crcbl_ui::menu::Menu::render) pushes the scrim, the
//! frame and the buttons ahead of the labels, so the scrim dims the HUD and the
//! labels stay legible over the panel because that is the order they went in.
//!
//! It used to be a sprite pass of its own, sandwiched between the draw list's
//! two halves by this module, because this pass had no textured quad. The two
//! halves are still two passes, `ui-composite` and `ui-overlay`, drawn back to
//! back; nothing is between them any more.
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
//! `crcbl-webgpu`, on a WebGPU with no
//! [`PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS) at all — could not
//! create the module.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendState, BufferDesc,
    BufferHandle, BufferImageCopy, BufferUsage, ColorTargetState, ColorWrites, Device, Extent3d,
    FilterMode, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageAspect,
    ImageSubresourceLayers, ImageViewHandle, ImageViewType, IndexFormat, LoadOp, MemoryLocation,
    Offset3d, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState, QueueHandle, ResourceState,
    SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry, ShaderModuleDesc,
    ShaderStages, StoreOp, check_portable_storage_buffers,
};

use crcbl_shaders::{Stage, UI};
use crcbl_ui::draw_list::{DrawList, Vertex2d};
use crcbl_ui::font::atlas::{GLYPH_MAX_PAGES, GLYPH_PAGE_SIZE, GLYPH_RASTER_BUDGET, GlyphAtlas};
use crcbl_ui::image::{ImageAtlas, PAGE_SIZE, TexelRect};
use crcbl_ui::menu::MenuSkin;
use crcbl_ui::text::FontAtlas;

use core::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::counters::FrameCounters;
use crate::graph::{ImageId, ImportedImage, InitialClaim, RenderGraph};
use crate::texture::{UploadedTexture, stage_region, upload_texture, upload_texture_layers};

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

/// The binding number the image atlas occupies, after the constants.
pub const IMAGE_ATLAS_BINDING: u32 = 4;

/// The binding number the image atlas's linear sampler occupies.
pub const IMAGE_SAMPLER_BINDING: u32 = 5;

/// The binding number the glyph pages occupy — the last one. They are sampled
/// through the bitmap font's nearest sampler at binding 1.
pub const GLYPH_PAGES_BINDING: u32 = 6;

/// The image atlas page's format: sRGB-encoded, straight alpha, the sprite
/// pass's sheet format.
const IMAGE_FORMAT: Format = Format::Rgba8UnormSrgb;

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
/// `add_passes` inserts the draw passes into the graph.
#[derive(Debug)]
pub struct UiRenderer {
    // Pipeline state
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    bind_group_layout: BindGroupLayoutHandle,

    // Glyph atlas
    atlas: UploadedTexture,
    atlas_sampler: SamplerHandle,

    /// The pictures [`DrawList::image`] draws, and the page they are on.
    images: ImageAtlas,
    /// The shipped menu art, registered in [`Self::images`] at start-up.
    menu_skin: MenuSkin,
    image_page: UploadedTexture,
    image_sampler: SamplerHandle,
    /// The staged copy of a dirty rectangle, per frame in flight: a slot's
    /// buffer is released when its turn of the ring comes round again, which is
    /// when the frame that copied from it has finished.
    image_staging: Vec<Option<BufferHandle>>,
    /// This frame's copy, when [`begin_frame`](Self::begin_frame) staged one.
    image_upload: Option<ImageUpload>,
    /// Set by the copy pass's body once the copy is recorded. A frame that
    /// staged an upload and never recorded it puts the rectangle back on the
    /// atlas for the next frame to try again.
    image_recorded: Arc<AtomicBool>,

    /// The glyphs [`DrawList::glyphs`] runs draw, rasterised on demand.
    glyphs: GlyphAtlas,
    /// Its pages, one layer each.
    glyph_pages: UploadedTexture,
    /// The staged copies of dirty page rectangles, per frame in flight, freed
    /// on the slot's next turn as [`Self::image_staging`] is.
    glyph_staging: Vec<Vec<BufferHandle>>,
    /// This frame's page copies.
    glyph_uploads: Vec<GlyphUpload>,
    /// [`Self::image_recorded`]'s counterpart for the page copies.
    glyph_recorded: Arc<AtomicBool>,

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

    /// Where the overlay's indices start in [`Self::last_index_count`]'s range,
    /// per frame in flight — [`crcbl_ui::draw_list::Triangles::overlay`] as the
    /// upload left it. The one number that turns a frame's geometry into the
    /// two ranges [`UiRenderer::add_passes`] draws.
    last_overlay_index: Vec<usize>,

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

        // The image atlas: the menu's art registered, then the whole page, once,
        // at start-up — the same staging path as the glyphs, and legal here for
        // the same reason. What changes later is copied inside a frame; see the
        // module docs.
        let mut images = ImageAtlas::new();
        let menu_skin = crate::menu::menu_skin(&mut images).map_err(|error| {
            HalError::InvalidDescriptor(format!("ui image atlas: the menu art: {error}"))
        })?;
        let image_page = upload_texture(
            device,
            queue,
            "ui image atlas",
            IMAGE_FORMAT,
            PAGE_SIZE,
            PAGE_SIZE,
            images.pixels(),
        )?;
        rollback.textures.push(image_page);
        // The whole page is on the GPU now, so nothing is owed.
        images.take_dirty();
        // Linear, for the shader's sharp-bilinear bend — see `ui.slang`.
        let image_sampler = device.create_sampler(&SamplerDesc {
            label: Some("ui image atlas"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(image_sampler);

        // The glyph pages: every page the atlas may open, empty, once. What the
        // atlas rasterises later is copied inside a frame; see the module docs.
        let glyphs = GlyphAtlas::new(GLYPH_PAGE_SIZE, GLYPH_MAX_PAGES, GLYPH_RASTER_BUDGET);
        let empty_page = vec![0u8; GLYPH_PAGE_SIZE as usize * GLYPH_PAGE_SIZE as usize];
        let glyph_pages = upload_texture_layers(
            device,
            queue,
            "ui glyph pages",
            Format::R8Unorm,
            GLYPH_PAGE_SIZE,
            GLYPH_PAGE_SIZE,
            &[empty_page.as_slice(); GLYPH_MAX_PAGES],
        )?;
        rollback.textures.push(glyph_pages);

        // Bind group layout: atlas texture, sampler, vertex storage buffer, the
        // constants uniform buffer, the image atlas and its sampler, then the
        // glyph pages.
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
                    stride: size_of::<Vertex2d>() as u32,
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
            BindGroupLayoutEntry {
                binding: IMAGE_ATLAS_BINDING,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: IMAGE_SAMPLER_BINDING,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: GLYPH_PAGES_BINDING,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let layout_desc = BindGroupLayoutDesc {
            label: Some("ui pass"),
            entries: &layout_entries,
        };
        check_portable_storage_buffers(Some("ui"), &[&layout_desc])?;
        let bind_group_layout = device.create_bind_group_layout(&layout_desc)?;
        rollback.bind_group_layouts.push(bind_group_layout);

        // Per-frame bind groups (atlas/sampler are static, the rest rotate)
        let mut frame_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut vertex_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut index_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut constant_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut last_vertex_count = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut last_index_count = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut last_overlay_index = Vec::with_capacity(FRAMES_IN_FLIGHT);
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
                entries: &frame_entries(
                    FrameTextures {
                        glyphs: (atlas.view, atlas_sampler),
                        images: (image_page.view, image_sampler),
                        glyph_pages: glyph_pages.view,
                    },
                    vb,
                    cb,
                ),
                variable_count: None,
            })?;
            rollback.bind_groups.push(bg);
            vertex_buffers.push(vb);
            index_buffers.push(ib);
            frame_groups.push(bg);
            last_vertex_count.push(0);
            last_index_count.push(0);
            last_overlay_index.push(0);
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
            // does, and is how the browser used to lose its HUD.
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
            images,
            menu_skin,
            image_page,
            image_sampler,
            image_staging: vec![None; FRAMES_IN_FLIGHT],
            image_upload: None,
            image_recorded: Arc::new(AtomicBool::new(false)),
            glyphs,
            glyph_pages,
            glyph_staging: vec![Vec::new(); FRAMES_IN_FLIGHT],
            glyph_uploads: Vec::new(),
            glyph_recorded: Arc::new(AtomicBool::new(false)),
            frame_groups,
            vertex_buffers,
            index_buffers,
            frame: 0,
            constant_buffers,
            vertex_capacity,
            index_capacity,
            last_vertex_count,
            last_index_count,
            last_overlay_index,
            target_format,
            destroyed: false,
        })
    }

    /// The format the compositing pass renders into.
    #[must_use]
    pub const fn target_format(&self) -> Format {
        self.target_format
    }

    /// The shipped menu art, for [`Menu::render`](crcbl_ui::menu::Menu::render).
    #[must_use]
    pub const fn menu_skin(&self) -> &MenuSkin {
        &self.menu_skin
    }

    /// The image atlas [`DrawList::image`] and [`DrawList::nine_slice`] draw
    /// from.
    #[must_use]
    pub const fn images(&self) -> &ImageAtlas {
        &self.images
    }

    /// The image atlas, to register pictures into.
    ///
    /// A registration marks the texels it wrote dirty, and the next
    /// [`begin_frame`](Self::begin_frame) uploads that rectangle and nothing
    /// else — see the module docs. The [`AtlasImage`](crcbl_ui::AtlasImage) it
    /// returns is valid in a draw list from that frame on.
    pub const fn images_mut(&mut self) -> &mut ImageAtlas {
        &mut self.images
    }

    /// The glyph atlas [`DrawList::glyphs`] runs are rasterised into: what it
    /// holds, and what it did in the last [`begin_frame`](Self::begin_frame).
    #[must_use]
    pub const fn glyphs(&self) -> &GlyphAtlas {
        &self.glyphs
    }

    /// Uploads the draw list's triangulated geometry and advances the ring.
    ///
    /// Call once per frame before `add_passes`. If the draw list is empty, the
    /// frame stores zero geometry and the passes draw nothing.
    ///
    /// **One expansion, both halves.** The list is tessellated once and the
    /// index where [`DrawList::begin_overlay`] cut it comes back with the
    /// geometry, so the two passes below share one upload and cannot disagree
    /// about where the HUD ends.
    ///
    /// **One glyph-atlas frame per call.** The tessellation rasterises every
    /// glyph a run misses, up to the atlas's budget, and what it put on a page
    /// is staged after it; a glyph past the budget is left out of this frame
    /// and drawn by a later one.
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
        self.stage_images(device, idx)?;

        self.glyphs.begin_frame();
        let crcbl_ui::draw_list::Triangles {
            vertices,
            indices,
            overlay,
        } = draw_list.to_triangles_split(Some(atlas), Some(&mut self.glyphs), scale);
        self.stage_glyphs(device, idx)?;

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
        self.last_overlay_index[idx] = overlay;

        // Only a new vertex buffer needs a new bind group; the atlas and the
        // sampler never change, so a steady-state frame writes no descriptors.
        if vertex_buffer_replaced {
            let entries = frame_entries(
                FrameTextures {
                    glyphs: (self.atlas.view, self.atlas_sampler),
                    images: (self.image_page.view, self.image_sampler),
                    glyph_pages: self.glyph_pages.view,
                },
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

    /// Stages whatever of the image atlas changed since the last upload, for
    /// this frame's `ui-images` copy pass.
    ///
    /// Also where last frame's staging is settled: an upload that was staged
    /// and never recorded — a graph built and dropped, or `add_passes` never
    /// called — hands its rectangle back to the atlas so it is not lost, and
    /// this slot's buffer from a whole ring ago is released.
    fn stage_images(&mut self, device: &dyn Device, idx: usize) -> Result<(), HalError> {
        if let Some(upload) = self.image_upload.take()
            && !self.image_recorded.swap(false, Ordering::Relaxed)
        {
            self.images.mark_dirty(upload.rect);
        }
        if let Some(stale) = self.image_staging[idx].take() {
            device.destroy_buffer(stale);
        }
        let Some(rect) = self.images.take_dirty() else {
            return Ok(());
        };
        let staged = stage_region(
            device,
            "ui image atlas staging",
            IMAGE_FORMAT,
            (rect.width, rect.height),
            &self.images.region(rect),
        );
        let (buffer, row_texels) = match staged {
            Ok(staged) => staged,
            Err(error) => {
                // Not lost: the next frame stages it again.
                self.images.mark_dirty(rect);
                return Err(error);
            }
        };
        self.image_staging[idx] = Some(buffer);
        self.image_recorded.store(false, Ordering::Relaxed);
        self.image_upload = Some(ImageUpload {
            staging: buffer,
            rect,
            row_texels,
        });
        Ok(())
    }

    /// Stages every glyph page rectangle this frame's tessellation dirtied, for
    /// the `ui-glyphs` copy pass — and settles last frame's, as
    /// [`stage_images`](Self::stage_images) does for the image atlas.
    fn stage_glyphs(&mut self, device: &dyn Device, idx: usize) -> Result<(), HalError> {
        if !self.glyph_recorded.swap(false, Ordering::Relaxed) {
            for upload in self.glyph_uploads.drain(..) {
                self.glyphs.mark_dirty(upload.page as usize, upload.rect);
            }
        }
        self.glyph_uploads.clear();
        for stale in self.glyph_staging[idx].drain(..) {
            device.destroy_buffer(stale);
        }
        for page in 0..self.glyphs.page_count() {
            let Some(rect) = self.glyphs.take_dirty(page) else {
                continue;
            };
            let staged = stage_region(
                device,
                "ui glyph page staging",
                Format::R8Unorm,
                (rect.width, rect.height),
                &self.glyphs.region(page, rect),
            );
            let (staging, row_texels) = match staged {
                Ok(staged) => staged,
                Err(error) => {
                    // Not lost: this rectangle and every one staged before it
                    // this frame go back on the atlas for the next frame.
                    self.glyphs.mark_dirty(page, rect);
                    for upload in self.glyph_uploads.drain(..) {
                        self.glyphs.mark_dirty(upload.page as usize, upload.rect);
                    }
                    return Err(error);
                }
            };
            self.glyph_staging[idx].push(staging);
            self.glyph_uploads.push(GlyphUpload {
                staging,
                page: page as u32,
                rect,
                row_texels,
            });
        }
        Ok(())
    }

    /// The most passes [`add_passes`](Self::add_passes) adds to a frame.
    ///
    /// Four: the image atlas's `ui-images` copy on a frame that registered a
    /// picture, the glyph pages' `ui-glyphs` copy on a frame that rasterised a
    /// glyph, then the HUD half and the overlay half, either of which a frame
    /// can leave empty — a frame with no menu and no console draws only the
    /// first, and a frame with an empty draw list draws neither.
    ///
    /// The most rather than the count. What a caller sizing
    /// [`PassTimers`](crate::timing::PassTimers) adds up — see
    /// [`MAX_TIMED_PASSES`](crate::timing::MAX_TIMED_PASSES).
    pub const MAX_PASSES: u32 = 4;

    /// This frame's two index ranges: the HUD half, then the overlay half.
    ///
    /// **The one place the split is computed**, so
    /// [`counters`](Self::counters) and [`add_passes`](Self::add_passes) cannot
    /// disagree about which halves a frame draws — the pair the counters' docs
    /// promise. Both are empty on a frame with nothing uploaded, and the second
    /// is empty on a frame whose draw list was never cut.
    fn segments(&self) -> (Range<u32>, Range<u32>) {
        let idx = self.frame;
        if self.last_vertex_count[idx] == 0 {
            return (0..0, 0..0);
        }
        let total = self.last_index_count[idx] as u32;
        // Clamped rather than trusted: the two ranges must partition the index
        // buffer, and a cut past its end would otherwise hand the draw call a
        // range the buffer does not hold.
        let split = (self.last_overlay_index[idx] as u32).min(total);
        (0..split, split..total)
    }

    /// What the last [`begin_frame`](Self::begin_frame) left this pass to draw.
    ///
    /// **Off `segments`, the same two index ranges
    /// [`add_passes`](Self::add_passes) branches on**, so a half this reports as
    /// drawn is a half that gets a pass and the two cannot disagree. One draw of
    /// one instance per non-empty half: a frame with no overlay on it reports
    /// one, a paused frame reports two, and an empty draw list reports nothing.
    ///
    /// The triangles are the index counts of the halves actually drawn —
    /// `draw_list.to_triangles_split` produced them and the pipeline's
    /// `PrimitiveState::default()` is a triangle list, so the count is a
    /// division and not an estimate.
    #[must_use]
    pub fn counters(&self) -> FrameCounters {
        let (below, above) = self.segments();
        let draws = u64::from(!below.is_empty()) + u64::from(!above.is_empty());
        if draws == 0 {
            return FrameCounters::default();
        }
        FrameCounters {
            draws,
            instances: draws,
            drawn: Some(draws),
            triangles: Some((below.len() + above.len()) as u64 / 3),
            // No cluster geometry and no readback: a known zero and no second
            // lag to declare — see [`crate::counters`].
            clusters: Some(0),
            cull_frame: None,
        }
    }

    /// Adds the UI's passes to `graph`, drawing on top of `target`.
    ///
    /// In order: `ui-images` when [`begin_frame`](Self::begin_frame) staged a
    /// changed rectangle of the image atlas, `ui-glyphs` when it staged glyph
    /// page rectangles, then `ui-composite` for the commands below
    /// [`DrawList::begin_overlay`]'s cut — the game's HUD and GUI — then
    /// `ui-overlay` for the commands above it — the menu, the debug panel and
    /// the console.
    ///
    /// A half with no triangles in it adds **no pass at all** rather than an
    /// empty one, the rule this pass has always had — so an unpaused frame with
    /// the console closed still records exactly one pass, named as it always
    /// was.
    ///
    /// Each pass reads nothing except its own vertex buffer; both blend onto
    /// the target using alpha blending. Call after the tonemap pass.
    ///
    /// `extent` is the target's size in pixels, which the shader divides by to
    /// reach NDC. It is *not* used to set the viewport or the scissor: the
    /// graph already sets both from the pass's own render area
    /// ([`CompiledGraph::execute`](crate::graph::CompiledGraph::execute)), and a
    /// body that set them again could only disagree with it.
    ///
    /// `extent` is also the *only* source of the viewport constants, which is
    /// why the uniform buffer is written in the pass body rather than in
    /// [`begin_frame`](Self::begin_frame): a second extent taken a second time
    /// is a second thing that can disagree.
    pub fn add_passes<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        target: ImageId,
        extent: (u32, u32),
    ) {
        let (below, above) = self.segments();
        let pages = [self.add_image_upload(graph), self.add_glyph_upload(graph)];
        self.add_segment(graph, target, extent, "ui-composite", below, pages);
        self.add_segment(graph, target, extent, "ui-overlay", above, pages);
    }

    /// Adds the `ui-glyphs` copy of this frame's staged page rectangles, and
    /// returns the page image as the graph knows it — or does nothing and
    /// returns `None` on a frame that staged nothing.
    ///
    /// Imported tracked in [`ResourceState::ShaderRead`] and back, for
    /// [`add_image_upload`](Self::add_image_upload)'s reason. One copy per
    /// rectangle, each into its page's layer.
    fn add_glyph_upload<'a>(&'a self, graph: &mut RenderGraph<'a>) -> Option<ImageId> {
        if self.glyph_uploads.is_empty() {
            return None;
        }
        let pages = graph.import_image(
            "ui glyph pages",
            ImportedImage {
                image: self.glyph_pages.image,
                view: self.glyph_pages.view,
                format: Format::R8Unorm,
                extent: (GLYPH_PAGE_SIZE, GLYPH_PAGE_SIZE),
                initial: ResourceState::ShaderRead,
                claim: InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        );
        let uploads = self.glyph_uploads.clone();
        let recorded = Arc::clone(&self.glyph_recorded);
        graph
            .add_copy_pass("ui-glyphs")
            .use_image(pages, ResourceState::TransferDst)
            .execute(move |ctx| {
                let image = ctx.image(pages);
                for upload in &uploads {
                    ctx.encoder().copy_buffer_to_image(&BufferImageCopy {
                        buffer: upload.staging,
                        buffer_offset: 0,
                        buffer_row_length: upload.row_texels,
                        buffer_image_height: upload.rect.height,
                        image,
                        image_subresource: ImageSubresourceLayers {
                            aspect: ImageAspect::COLOR,
                            mip: 0,
                            base_layer: upload.page,
                            layer_count: 1,
                        },
                        image_offset: Offset3d {
                            x: upload.rect.x as i32,
                            y: upload.rect.y as i32,
                            z: 0,
                        },
                        image_extent: Extent3d::d2(upload.rect.width, upload.rect.height),
                    });
                }
                recorded.store(true, Ordering::Relaxed);
            });
        Some(pages)
    }

    /// Adds the `ui-images` copy of this frame's staged rectangle, and returns
    /// the page as the graph knows it — or does nothing and returns `None` on a
    /// frame that staged nothing.
    ///
    /// The page is imported **tracked**, in [`ResourceState::ShaderRead`] and
    /// back to it: that is where the start-up upload left it and where every
    /// frame's graph leaves it, so the claim is one the pool's ledger can check.
    fn add_image_upload<'a>(&'a self, graph: &mut RenderGraph<'a>) -> Option<ImageId> {
        let upload = self.image_upload?;
        let page = graph.import_image(
            "ui image atlas",
            ImportedImage {
                image: self.image_page.image,
                view: self.image_page.view,
                format: IMAGE_FORMAT,
                extent: (PAGE_SIZE, PAGE_SIZE),
                initial: ResourceState::ShaderRead,
                claim: InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        );
        let recorded = Arc::clone(&self.image_recorded);
        graph
            .add_copy_pass("ui-images")
            .use_image(page, ResourceState::TransferDst)
            .execute(move |ctx| {
                let image = ctx.image(page);
                ctx.encoder().copy_buffer_to_image(&BufferImageCopy {
                    buffer: upload.staging,
                    buffer_offset: 0,
                    buffer_row_length: upload.row_texels,
                    buffer_image_height: upload.rect.height,
                    image,
                    image_subresource: ImageSubresourceLayers {
                        aspect: ImageAspect::COLOR,
                        mip: 0,
                        base_layer: 0,
                        layer_count: 1,
                    },
                    image_offset: Offset3d {
                        x: upload.rect.x as i32,
                        y: upload.rect.y as i32,
                        z: 0,
                    },
                    image_extent: Extent3d::d2(upload.rect.width, upload.rect.height),
                });
                recorded.store(true, Ordering::Relaxed);
            });
        Some(page)
    }

    /// Adds one half of the draw list as one render pass, or nothing if the
    /// half is empty.
    ///
    /// Both halves index the *same* vertex and index buffers — the shader reads
    /// `vertices[SV_VertexID]` and the index values are absolute — so a half is
    /// a range of the frame's one upload rather than a second one.
    ///
    /// `pages` are the image atlas and the glyph pages, each when this frame's
    /// graph imported it for an upload, and the pass then declares that it
    /// samples it, so the graph returns it from the copy's `TransferDst` before
    /// the draw reads it.
    fn add_segment<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        target: ImageId,
        extent: (u32, u32),
        label: &'static str,
        segment: Range<u32>,
        pages: [Option<ImageId>; 2],
    ) {
        if segment.is_empty() {
            return; // nothing to draw
        }

        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let bg = self.frame_groups[self.frame];
        let index_buffer = self.index_buffers[self.frame];
        let constants = self.constant_buffers[self.frame];

        let mut pass = graph
            .add_render_pass(label)
            // Draw on top of the tonemapped target with alpha blending.
            .color(target, LoadOp::Load, StoreOp::Store, Default::default());
        for page in pages.into_iter().flatten() {
            pass = pass.read_image(page);
        }
        pass.execute(move |ctx| {
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
                crcbl_core::log::error!("graph: ui constants write failed: {error}");
                return;
            }
            let encoder = ctx.encoder();
            encoder.bind_graphics_pipeline(pipeline);
            encoder.bind_group(0, bg, &[], pipeline_layout);
            encoder.bind_index_buffer(index_buffer, 0, IndexFormat::Uint32);
            encoder.draw_indexed(segment.clone(), 0, 0..1);
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
        for staging in self.image_staging.drain(..).flatten() {
            device.destroy_buffer(staging);
        }
        for staging in self.glyph_staging.drain(..).flatten() {
            device.destroy_buffer(staging);
        }
        self.glyph_pages.destroy(device);
        device.destroy_sampler(self.atlas_sampler);
        self.atlas.destroy(device);
        device.destroy_sampler(self.image_sampler);
        self.image_page.destroy(device);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.bind_group_layout);
    }
}

impl Drop for UiRenderer {
    fn drop(&mut self) {
        if !self.destroyed {
            crcbl_core::log::warn!(
                "UiRenderer dropped without calling destroy() — GPU resources leaked"
            );
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

/// One frame's staged image-atlas copy: where the bytes are and where they go.
#[derive(Clone, Copy, Debug)]
struct ImageUpload {
    staging: BufferHandle,
    rect: TexelRect,
    /// The staged rows' pitch, in texels.
    row_texels: u32,
}

/// One frame's staged glyph page copy.
#[derive(Clone, Copy, Debug)]
struct GlyphUpload {
    staging: BufferHandle,
    /// The page, which is the layer copied into.
    page: u32,
    rect: TexelRect,
    /// The staged rows' pitch, in texels.
    row_texels: u32,
}

/// The textures a frame's bind group names: the two atlases each as its view
/// and sampler, and the glyph pages, which share the bitmap font's sampler.
///
/// One argument rather than loose handles, so the pairs cannot be crossed at
/// the call site — a glyph view under the image sampler still binds.
#[derive(Clone, Copy)]
struct FrameTextures {
    glyphs: (ImageViewHandle, SamplerHandle),
    images: (ImageViewHandle, SamplerHandle),
    glyph_pages: ImageViewHandle,
}

/// One frame's bind-group entries.
///
/// One function rather than the two copies `new` and `begin_frame` used to
/// carry: they build the *same* group, and a binding added to one and forgotten
/// in the other is a bind group that stops matching its layout the first time
/// the vertex ring grows.
fn frame_entries(
    textures: FrameTextures,
    vertices: BufferHandle,
    constants: BufferHandle,
) -> [BindGroupEntry; 7] {
    [
        BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: BindingResource::ImageView(textures.glyphs.0),
        },
        BindGroupEntry {
            binding: 1,
            array_index: 0,
            resource: BindingResource::Sampler(textures.glyphs.1),
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
        BindGroupEntry {
            binding: IMAGE_ATLAS_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(textures.images.0),
        },
        BindGroupEntry {
            binding: IMAGE_SAMPLER_BINDING,
            array_index: 0,
            resource: BindingResource::Sampler(textures.images.1),
        },
        BindGroupEntry {
            binding: GLYPH_PAGES_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(textures.glyph_pages),
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
                (ResourceState::Undefined, ResourceState::TransferDst),
                (ResourceState::TransferDst, ResourceState::ShaderRead),
                (ResourceState::Undefined, ResourceState::TransferDst),
                (ResourceState::TransferDst, ResourceState::ShaderRead),
            ],
            "the two atlases and the glyph pages are the only barriers the UI renderer's \
             construction records"
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

            let (vertices, indices) = dl.to_triangles(Some(&atlas), None, 1.0);
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

    /// **The counters are the one draw this pass records and the triangles it
    /// wrote the indices for**, against two lists whose triangle counts differ.
    ///
    /// One rectangle is two triangles; a rectangle and a string are more, and
    /// the number is the index list's own length rather than a per-glyph
    /// estimate — so a counter that guessed, or that reported the vertex count,
    /// fails on both. An empty list records no pass at all and the counters say
    /// zero *and know it*, which is the value `indirect` is not.
    #[test]
    fn the_counters_are_the_one_draw_and_the_indices_it_covers() {
        let (device, queue) = open();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();

        let mut one_rect = DrawList::new();
        one_rect.rect(
            glam::Vec2::ZERO,
            glam::Vec2::new(10.0, 10.0),
            [1.0, 1.0, 1.0, 1.0],
        );
        renderer
            .begin_frame(device.as_ref(), &one_rect, &atlas, 1.0)
            .expect("upload");
        let counters = renderer.counters();
        assert_eq!(counters.draws, 1, "one `draw_indexed` for the whole list");
        assert_eq!(counters.instances, 1);
        assert_eq!(counters.drawn, Some(1));
        assert_eq!(counters.triangles, Some(2), "a quad is two triangles");

        let mut with_text = one_rect.clone();
        with_text.text(
            glam::Vec2::new(4.0, 4.0),
            "counters",
            [1.0, 1.0, 1.0, 1.0],
            14.0,
        );
        renderer
            .begin_frame(device.as_ref(), &with_text, &atlas, 1.0)
            .expect("upload");
        let richer = renderer.counters();
        assert_eq!(
            richer.draws, 1,
            "still one draw, however much is in the list"
        );
        // The index list the pass actually built, so this is the pass's own
        // arithmetic and not a second count of the glyphs.
        let (_, indices) = with_text.to_triangles(Some(&atlas), None, 1.0);
        assert_eq!(richer.triangles, Some(indices.len() as u64 / 3));
        assert!(
            richer.triangles > counters.triangles,
            "a longer list must move the counter: {:?} against {:?}",
            richer.triangles,
            counters.triangles,
        );

        renderer
            .begin_frame(device.as_ref(), &DrawList::new(), &atlas, 1.0)
            .expect("upload");
        assert_eq!(
            renderer.counters(),
            crate::counters::FrameCounters::default()
        );

        renderer.destroy(device.as_ref());
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

    // -----------------------------------------------------------------------
    // A paused frame
    // -----------------------------------------------------------------------

    /// A pause menu, laid out for `extent`, as `apps/*/src/menu.rs` builds one.
    fn pause_menu() -> crcbl_ui::menu::Menu {
        use crcbl_ui::menu::{Menu, MenuItem};
        Menu::new(
            "PAUSED",
            vec![
                MenuItem::new(1, "RESUME", "ESC"),
                MenuItem::new(2, "QUIT", ""),
            ],
        )
    }

    /// A draw list shaped like a paused frame, as the engine's loop builds one: a
    /// HUD bar, the cut, then the whole menu — its art and its text — drawn with
    /// the renderer's own skin.
    fn paused_list(ui: &UiRenderer, extent: (u32, u32), atlas: &FontAtlas) -> DrawList {
        let mut list = DrawList::new();
        list.rect(
            glam::Vec2::new(0.0, 0.0),
            glam::Vec2::new(64.0, 8.0),
            [1.0; 4],
        );
        list.begin_overlay();
        let panel = pause_menu();
        panel.render(&mut list, &panel.layout(extent, atlas), ui.menu_skin());
        list
    }

    /// **A paused frame is the two halves of one upload drawn back to back, the
    /// menu's art in the second** — and the two halves partition the index
    /// buffer.
    ///
    /// The pass labels come out of the compiled graph in execution order, and
    /// the index ranges come out of the recorded draw calls — so a swap of the
    /// two `add_segment` calls fails the labels, and a range that overlapped or
    /// left a gap fails the arithmetic. The ranges are asserted against each
    /// other and against the frame's own index count rather than against
    /// literals, which is what keeps the test about the partition instead of
    /// about the glyph layout.
    #[test]
    fn a_paused_frame_draws_both_halves_of_one_upload_with_the_menu_art_above_the_cut() {
        use crate::graph::{CompiledPass, RenderGraph};
        use crate::transient::{TransientImageDesc, TransientPool};
        use crcbl_hal::null::{Command, Event};
        use crcbl_hal::{CommandEncoderDesc, Format as HalFormat, ImageUsage};

        const EXTENT: (u32, u32) = (128, 96);

        let (recorder, device, queue) = open_recorded();
        let mut pool = TransientPool::new();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

        let atlas = FontAtlas::built_in();
        let list = paused_list(&ui, EXTENT, &atlas);
        ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
            .expect("upload");

        let total = ui.last_index_count[ui.frame] as u32;
        let split = ui.last_overlay_index[ui.frame] as u32;
        assert!(
            split > 0 && split < total,
            "the fixture must put geometry on both sides of the cut: {split} of {total}"
        );
        // The menu's frame is textured quads in the overlay half, off the
        // renderer's own atlas: every image vertex is above the cut.
        let triangles = list.to_triangles_split(Some(&atlas), None, 1.0);
        let image_vertices: Vec<u32> = triangles.indices[..]
            .iter()
            .copied()
            .filter(|&index| {
                triangles.vertices[index as usize].primitive()
                    == Some(crcbl_ui::draw_list::Primitive::Image)
            })
            .collect();
        assert!(!image_vertices.is_empty(), "the menu drew no art");
        assert!(
            triangles.indices[..split as usize]
                .iter()
                .all(|&index| !image_vertices.contains(&index)),
            "menu art landed below the cut"
        );

        // The uploads are start-up and per-frame CPU work; what is under test is
        // the passes the frame records.
        recorder.clear();

        let mut graph = RenderGraph::new(queue);
        let target = graph.create_image(
            "target",
            TransientImageDesc::new(
                EXTENT,
                HalFormat::Bgra8UnormSrgb,
                ImageUsage::COLOR_ATTACHMENT,
            ),
        );
        ui.add_passes(&mut graph, target, EXTENT);
        let compiled = graph.compile(&pool).expect("a legal frame");

        let labels: Vec<&str> = compiled.passes().iter().map(CompiledPass::label).collect();
        assert_eq!(
            labels,
            ["ui-composite", "ui-overlay"],
            "the HUD half, then the overlay half, and nothing between them"
        );

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("paused frame"),
            queue,
        });
        compiled
            .execute(device.as_ref(), &mut pool, encoder.as_mut(), None)
            .expect("the graph executed");

        // The recorder sees a command stream only once the encoder is finished
        // — the shape `tests/ui_pass_stream.rs` reads it in.
        let commands = encoder.finish().expect("recording succeeded");

        let drawn: Vec<std::ops::Range<u32>> = recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::DrawIndexed { indices, .. } => Some(indices),
                _ => None,
            })
            .collect();
        assert_eq!(
            drawn,
            vec![0..split, split..total],
            "the two halves must partition the frame's one index buffer"
        );

        // Both halves are drawn out of the *same* upload — the assertion that
        // separates a split pass from a second tessellation.
        let writes = recorder
            .events()
            .into_iter()
            .filter(|event| matches!(event, Event::BufferWritten { .. }))
            .count();
        assert_eq!(
            writes, 2,
            "one viewport-constants write per pass and no second geometry \
             upload: the tessellation happened before the frame"
        );

        let counters = ui.counters();
        assert_eq!(counters.draws, 2, "one draw per half");
        assert_eq!(counters.drawn, Some(2));
        assert_eq!(
            counters.triangles,
            Some(u64::from(total) / 3),
            "and every triangle in the buffer is in one half or the other"
        );

        device.destroy_command_buffer(commands);
        ui.destroy(device.as_ref());
        pool.destroy(device.as_ref());
    }

    /// A frame with nothing above the cut records **one** pass, named as it
    /// always was — the unpaused case every sample spends its life in, and the
    /// reason every existing `contains("ui-composite")` assertion still holds.
    #[test]
    fn an_uncut_draw_list_is_still_one_ui_composite_pass() {
        use crate::graph::{CompiledPass, RenderGraph};
        use crate::transient::{TransientImageDesc, TransientPool};
        use crcbl_hal::{Format as HalFormat, ImageUsage};

        const EXTENT: (u32, u32) = (128, 96);

        let (device, queue) = open();
        let mut pool = TransientPool::new();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        list.text(glam::Vec2::new(4.0, 4.0), "SCORE", [1.0; 4], 14.0);
        ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
            .expect("upload");

        let mut graph = RenderGraph::new(queue);
        let target = graph.create_image(
            "target",
            TransientImageDesc::new(
                EXTENT,
                HalFormat::Bgra8UnormSrgb,
                ImageUsage::COLOR_ATTACHMENT,
            ),
        );
        ui.add_passes(&mut graph, target, EXTENT);
        let compiled = graph.compile(&pool).expect("a legal frame");

        let labels: Vec<&str> = compiled.passes().iter().map(CompiledPass::label).collect();
        assert_eq!(labels, ["ui-composite"]);
        assert_eq!(ui.counters().draws, 1, "one half, one draw");

        drop(compiled);
        ui.destroy(device.as_ref());
        pool.destroy(device.as_ref());
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

    // -----------------------------------------------------------------------
    // The image atlas
    // -----------------------------------------------------------------------

    /// The copy-buffer-to-image commands in `commands`, in order.
    fn image_copies(commands: &[crcbl_hal::null::Command]) -> Vec<crcbl_hal::BufferImageCopy> {
        commands
            .iter()
            .filter_map(|command| match command {
                crcbl_hal::null::Command::CopyBufferToImage(copy) => Some(*copy),
                _ => None,
            })
            .collect()
    }

    /// Records one frame of `ui` through a real graph onto a fresh target and
    /// returns the pass labels in execution order.
    ///
    /// The recorder's command stream is filled once the encoder is finished,
    /// which this does before returning.
    fn record_frame(
        ui: &mut UiRenderer,
        device: &dyn Device,
        queue: QueueHandle,
        pool: &mut crate::transient::TransientPool,
        list: &DrawList,
    ) -> Vec<String> {
        use crate::graph::{CompiledPass, RenderGraph};
        use crate::transient::TransientImageDesc;
        use crcbl_hal::{CommandEncoderDesc, ImageUsage};

        const EXTENT: (u32, u32) = (64, 48);
        ui.begin_frame(device, list, &FontAtlas::built_in(), 1.0)
            .expect("upload");
        let mut graph = RenderGraph::new(queue);
        let target = graph.create_image(
            "target",
            TransientImageDesc::new(EXTENT, Format::Bgra8UnormSrgb, ImageUsage::COLOR_ATTACHMENT),
        );
        ui.add_passes(&mut graph, target, EXTENT);
        let compiled = graph.compile(pool).expect("a legal frame");
        let labels = compiled
            .passes()
            .iter()
            .map(|pass| CompiledPass::label(pass).to_owned())
            .collect();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("ui frame"),
            queue,
        });
        compiled
            .execute(device, pool, encoder.as_mut(), None)
            .expect("the graph executed");
        let commands = encoder.finish().expect("recording succeeded");
        device.destroy_command_buffer(commands);
        labels
    }

    /// A list with one quad on it, so the draw passes have something to do.
    fn one_rect() -> DrawList {
        let mut list = DrawList::new();
        list.rect(glam::Vec2::ZERO, glam::Vec2::splat(8.0), [1.0; 4]);
        list
    }

    /// **The page goes up whole at start-up, as four bytes a texel**, beside the
    /// glyph atlas's one.
    #[test]
    fn the_image_atlas_is_an_rgba_page_uploaded_whole_at_start_up() {
        let (recorder, device, queue) = open_recorded();
        let renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

        let page = PAGE_SIZE as usize;
        let writes: Vec<usize> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                crcbl_hal::null::Event::BufferWritten { len, .. } => Some(len),
                _ => None,
            })
            .collect();
        assert!(
            writes.contains(&(page * page * 4)),
            "no staging write the size of an RGBA page in {writes:?}"
        );
        let copies = image_copies(&recorder.commands());
        assert!(
            copies
                .iter()
                .any(|copy| copy.image_extent == Extent3d::d2(PAGE_SIZE, PAGE_SIZE)),
            "no copy of the whole page in {copies:?}"
        );
        assert_eq!(renderer.images().dirty(), None, "the page owes nothing");
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A frame with nothing registered copies nothing**: no staging, no pass,
    /// no import — the steady state every frame after start-up is in.
    #[test]
    fn a_frame_with_no_new_image_uploads_nothing() {
        let (recorder, device, queue) = open_recorded();
        let mut pool = crate::transient::TransientPool::new();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        recorder.clear();

        let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
        assert_eq!(labels, ["ui-composite"]);
        assert!(image_copies(&recorder.commands()).is_empty());

        ui.destroy(device.as_ref());
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **An image registered after start-up is copied once, as exactly the
    /// rectangle it dirtied, inside the graph** — ahead of the draws, into the
    /// page the graph moved to `TransferDst` and back — and the frame after it
    /// copies nothing.
    #[test]
    fn a_registered_image_is_copied_once_as_its_own_rectangle_inside_the_graph() {
        use crcbl_hal::null::Command;

        let (recorder, device, queue) = open_recorded();
        let mut pool = crate::transient::TransientPool::new();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let image = ui
            .images_mut()
            .register(5, 3, &[200; 5 * 3 * 4])
            .expect("fits an empty page");
        let dirty = ui
            .images()
            .dirty()
            .expect("the registration dirtied the page");
        recorder.clear();

        let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
        assert_eq!(
            labels,
            ["ui-images", "ui-composite"],
            "the copy comes before the draw that samples it"
        );
        let commands = recorder.commands();
        let copies = image_copies(&commands);
        assert_eq!(copies.len(), 1, "{copies:?}");
        let copy = copies[0];
        assert_eq!(copy.image, ui.image_page.image);
        assert_eq!(
            (copy.image_offset.x, copy.image_offset.y),
            (dirty.x as i32, dirty.y as i32)
        );
        assert_eq!(copy.image_extent, Extent3d::d2(dirty.width, dirty.height));
        assert!(
            dirty.x <= image.x
                && dirty.y <= image.y
                && image.x + image.width <= dirty.x + dirty.width
                && image.y + image.height <= dirty.y + dirty.height,
            "the copied rectangle {dirty:?} does not cover the image {image:?}"
        );
        // The graph, not the renderer, moved the page out of `ShaderRead` for
        // the copy and back before the draw.
        let page_transitions: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Barrier { images, .. } => Some(images.clone()),
                _ => None,
            })
            .flatten()
            .filter(|barrier| barrier.image == ui.image_page.image)
            .map(|barrier| (barrier.from, barrier.to))
            .collect();
        assert_eq!(
            page_transitions,
            [
                (ResourceState::ShaderRead, ResourceState::TransferDst),
                (ResourceState::TransferDst, ResourceState::ShaderRead),
            ]
        );
        // And back *before* the draw that samples it, not at the end of the
        // frame: a draw reading a page still in `TransferDst` is the hazard the
        // pass's `read_image` declaration exists to rule out.
        let returned = commands
            .iter()
            .position(|command| match command {
                Command::Barrier { images, .. } => images.iter().any(|barrier| {
                    barrier.image == ui.image_page.image && barrier.to == ResourceState::ShaderRead
                }),
                _ => false,
            })
            .expect("the page is returned to ShaderRead");
        let drawn = commands
            .iter()
            .position(|command| matches!(command, Command::DrawIndexed { .. }))
            .expect("the rect is drawn");
        assert!(
            returned < drawn,
            "the page went back to ShaderRead at command {returned}, after the draw at {drawn}"
        );

        recorder.clear();
        let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
        assert_eq!(labels, ["ui-composite"], "uploaded once, not every frame");
        assert!(image_copies(&recorder.commands()).is_empty());

        ui.destroy(device.as_ref());
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A staged upload whose frame never recorded it is not lost: the next
    /// frame stages the same rectangle again.
    #[test]
    fn an_upload_that_was_never_recorded_is_staged_again() {
        let (device, queue) = open();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        ui.images_mut()
            .register(4, 4, &[9; 4 * 4 * 4])
            .expect("fits");
        let atlas = FontAtlas::built_in();

        ui.begin_frame(device.as_ref(), &one_rect(), &atlas, 1.0)
            .expect("upload");
        let first = ui.image_upload.expect("staged").rect;
        // No graph this frame.
        ui.begin_frame(device.as_ref(), &one_rect(), &atlas, 1.0)
            .expect("upload");
        let again = ui.image_upload.expect("staged again").rect;
        assert_eq!(first, again);

        ui.destroy(device.as_ref());
    }

    // -----------------------------------------------------------------------
    // The glyph pages
    // -----------------------------------------------------------------------

    /// A list drawing `text` in the committed font at 20px from (4, 4).
    fn glyph_run(text: &str) -> DrawList {
        use crcbl_ui::font::Font;
        use crcbl_ui::font::layout::TextLayout;

        let font = Font::sans();
        let layout = TextLayout::new(font, text, 20.0, 24.0, None);
        let mut list = DrawList::new();
        list.glyphs(
            glam::Vec2::splat(4.0),
            font,
            20.0,
            [1.0; 4],
            layout.glyphs(),
        );
        list
    }

    /// **Every page the atlas may open goes up empty at start-up**, one R8 copy
    /// per layer of one image.
    #[test]
    fn the_glyph_pages_are_every_layer_uploaded_empty_at_start_up() {
        let (recorder, device, queue) = open_recorded();
        let renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let page = GLYPH_PAGE_SIZE as usize;
        let layers: Vec<u32> = image_copies(&recorder.commands())
            .into_iter()
            .filter(|copy| copy.image == renderer.glyph_pages.image)
            .map(|copy| {
                assert_eq!(
                    copy.image_extent,
                    Extent3d::d2(GLYPH_PAGE_SIZE, GLYPH_PAGE_SIZE)
                );
                assert_eq!(copy.image_subresource.layer_count, 1);
                copy.image_subresource.base_layer
            })
            .collect();
        assert_eq!(layers, (0..GLYPH_MAX_PAGES as u32).collect::<Vec<_>>());
        let writes: Vec<usize> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                crcbl_hal::null::Event::BufferWritten { len, .. } => Some(len),
                _ => None,
            })
            .collect();
        assert!(
            writes.contains(&(page * page * GLYPH_MAX_PAGES)),
            "no staging write of every page at one byte a texel in {writes:?}"
        );
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A frame that rasterises glyphs copies each dirty page rectangle into
    /// its layer, in a `ui-glyphs` pass ahead of the draw** — covering every
    /// glyph the frame drew — and the frame after it, drawing the same text,
    /// rasterises and copies nothing.
    #[test]
    fn rasterised_glyphs_are_copied_into_their_page_once_ahead_of_the_draw() {
        use crcbl_hal::null::Command;

        let (recorder, device, queue) = open_recorded();
        let mut pool = crate::transient::TransientPool::new();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        recorder.clear();

        let list = glyph_run("Kerned AVATAR");
        let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &list);
        assert_eq!(labels, ["ui-glyphs", "ui-composite"]);
        assert!(ui.glyphs().stats().rasterized > 0);
        let commands = recorder.commands();
        let copies: Vec<_> = image_copies(&commands)
            .into_iter()
            .filter(|copy| copy.image == ui.glyph_pages.image)
            .collect();
        assert_eq!(copies.len(), 1, "one page, one rectangle: {copies:?}");
        let copy = copies[0];
        assert_eq!(copy.image_subresource.base_layer, 0);

        // Every quad the frame drew samples inside the copied rectangle.
        let (vertices, _) = list.to_triangles(None, None, 1.0);
        assert!(vertices.is_empty(), "the run needs the glyph atlas to draw");
        let triangles = ui.last_index_count[ui.frame] / 6;
        assert!(triangles >= 11, "{triangles} glyph quads");
        let page = GLYPH_PAGE_SIZE as f32;
        let mut atlas =
            crcbl_ui::font::atlas::GlyphAtlas::new(GLYPH_PAGE_SIZE, GLYPH_MAX_PAGES, 1000);
        atlas.begin_frame();
        let (vertices, _) = list.to_triangles(None, Some(&mut atlas), 1.0);
        for vertex in vertices {
            let texel = vertex.uv * page;
            assert!(
                texel.x >= copy.image_offset.x as f32
                    && texel.y >= copy.image_offset.y as f32
                    && texel.x <= (copy.image_offset.x as u32 + copy.image_extent.width) as f32
                    && texel.y <= (copy.image_offset.y as u32 + copy.image_extent.height) as f32,
                "a glyph samples {texel} outside the copied {copy:?}"
            );
        }
        // Back to `ShaderRead` before the draw that samples the pages.
        let returned = commands
            .iter()
            .position(|command| match command {
                Command::Barrier { images, .. } => images.iter().any(|barrier| {
                    barrier.image == ui.glyph_pages.image && barrier.to == ResourceState::ShaderRead
                }),
                _ => false,
            })
            .expect("the pages are returned to ShaderRead");
        let drawn = commands
            .iter()
            .position(|command| matches!(command, Command::DrawIndexed { .. }))
            .expect("the glyphs are drawn");
        assert!(returned < drawn);

        recorder.clear();
        let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &list);
        assert_eq!(
            labels,
            ["ui-composite"],
            "cached glyphs were uploaded again"
        );
        assert_eq!(ui.glyphs().stats().rasterized, 0);
        assert!(image_copies(&recorder.commands()).is_empty());

        ui.destroy(device.as_ref());
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A staged page copy whose frame never recorded it is staged again.
    #[test]
    fn a_glyph_upload_that_was_never_recorded_is_staged_again() {
        let (device, queue) = open();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        let atlas = FontAtlas::built_in();
        let list = glyph_run("again");
        ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
            .expect("upload");
        let first: Vec<TexelRect> = ui.glyph_uploads.iter().map(|upload| upload.rect).collect();
        assert_eq!(first.len(), 1);
        // No graph this frame; the glyphs are cached, so only the retry stages.
        ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
            .expect("upload");
        let again: Vec<TexelRect> = ui.glyph_uploads.iter().map(|upload| upload.rect).collect();
        assert_eq!(first, again);
        ui.destroy(device.as_ref());
    }

    /// Every glyph staging buffer is given back, by the ring or by `destroy`.
    #[test]
    fn glyph_uploads_leak_nothing() {
        let (recorder, device, queue) = open_recorded();
        let before = recorder.total_live_objects();
        let mut pool = crate::transient::TransientPool::new();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        for (round, text) in ["abc", "def", "ghi", "jkl"].into_iter().enumerate() {
            let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &glyph_run(text));
            assert_eq!(labels[0], "ui-glyphs", "round {round}");
        }
        ui.destroy(device.as_ref());
        pool.destroy(device.as_ref());
        assert_eq!(recorder.total_live_objects(), before);
        recorder.assert_valid();
    }

    /// Every staging buffer an upload made is given back — by the ring on its
    /// next turn, or by `destroy` for the ones still in flight.
    #[test]
    fn image_uploads_leak_nothing() {
        let (recorder, device, queue) = open_recorded();
        let before = recorder.total_live_objects();
        let mut pool = crate::transient::TransientPool::new();
        let mut ui =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        for round in 0..(FRAMES_IN_FLIGHT * 2) {
            ui.images_mut()
                .register(3, 3, &[round as u8; 3 * 3 * 4])
                .expect("fits");
            let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
            assert_eq!(labels[0], "ui-images", "round {round}");
        }
        ui.destroy(device.as_ref());
        pool.destroy(device.as_ref());
        assert_eq!(recorder.total_live_objects(), before);
        recorder.assert_valid();
    }
}
