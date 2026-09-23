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
//! [`DrawList::nine_slice`] draw from. The page is built with the renderer,
//! like the glyphs, through [`upload_cleared_texture`]: zeroed on the GPU, with
//! only the menu art's rectangle staged from the host. An image registered
//! after that — through
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
use crate::texture::{
    ClearedTextureDesc, TexturePatch, UploadedTexture, stage_region, upload_cleared_texture,
    upload_texture,
};

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

        // The image atlas: the menu's art registered, then the page created once,
        // at start-up — zeroed on the GPU with only the registered rectangle
        // staged from the host, and legal here for the same reason as the glyphs.
        // What changes later is copied inside a frame; see the module docs.
        let mut images = ImageAtlas::new();
        let menu_skin = crate::menu::menu_skin(&mut images).map_err(|error| {
            HalError::InvalidDescriptor(format!("ui image atlas: the menu art: {error}"))
        })?;
        // Taken rather than read: the rectangle is on the GPU once the page is,
        // so nothing is owed.
        let registered = images.take_dirty().map(|rect| (rect, images.region(rect)));
        let patches: Vec<TexturePatch<'_>> = registered
            .iter()
            .map(|(rect, pixels)| TexturePatch {
                layer: 0,
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                pixels,
            })
            .collect();
        let image_page = upload_cleared_texture(
            device,
            queue,
            &ClearedTextureDesc {
                label: "ui image atlas",
                format: IMAGE_FORMAT,
                width: PAGE_SIZE,
                height: PAGE_SIZE,
                layers: 1,
                view_type: ImageViewType::D2,
                patches: &patches,
            },
        )?;
        rollback.textures.push(image_page);
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

        // The glyph pages: every page the atlas may open, empty, once — zeroed on
        // the GPU rather than sent from the host. What the atlas rasterises later
        // is copied inside a frame; see the module docs.
        let glyphs = GlyphAtlas::new(GLYPH_PAGE_SIZE, GLYPH_MAX_PAGES, GLYPH_RASTER_BUDGET);
        let glyph_pages = upload_cleared_texture(
            device,
            queue,
            &ClearedTextureDesc {
                label: "ui glyph pages",
                format: Format::R8Unorm,
                width: GLYPH_PAGE_SIZE,
                height: GLYPH_PAGE_SIZE,
                layers: GLYPH_MAX_PAGES as u32,
                view_type: ImageViewType::D2Array,
                patches: &[],
            },
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

// `Instance::create_device` is native-only: see the `crcbl_hal::device` module docs.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
