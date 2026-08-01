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

/// Push constant block matching `ui.slang`'s `UiConstants`.
///
/// `viewport` is the framebuffer size in pixels (width, height). The shader
/// divides position by viewport and maps to NDC.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct UiPushConstants {
    viewport: [f32; 2],
}

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

    // Per-frame bind groups (each contains atlas+sampler+vertex_buffer)
    frame_groups: Vec<BindGroupHandle>,
    vertex_buffers: Vec<BufferHandle>,
    index_buffers: Vec<BufferHandle>,
    frame: usize,

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
    /// # Errors
    ///
    /// [`HalError::Unsupported`] on a device without
    /// [`Features::PUSH_CONSTANTS`] — `ui.slang` receives the viewport size as
    /// a push constant and has no other binding to receive it through, so a
    /// device that cannot deliver one has to be told rather than shown a UI
    /// scaled by whatever was left in the block. Otherwise [`HalError`] from
    /// any seam call.
    pub fn new(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
    ) -> Result<Self, HalError> {
        if !device.caps().features.contains(Features::PUSH_CONSTANTS) {
            return Err(HalError::Unsupported {
                backend: device.backend(),
                what: "the UI pass: ui.slang takes its viewport size as a push constant and this \
                       device has no push constants",
            });
        }
        let atlas = FontAtlas::built_in();
        let (atlas_w, atlas_h, atlas_pixels) = atlas.glyph_bitmap();

        // Upload glyph atlas texture via staging.
        let (atlas_image, atlas_view) =
            upload_texture_r8(device, queue, atlas_w, atlas_h, &atlas_pixels)?;

        let atlas_sampler = device.create_sampler(&SamplerDesc {
            label: Some("ui glyph atlas"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;

        // Bind group layout: atlas texture, sampler, vertex storage buffer
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("ui pass"),
            entries: &[
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
            ],
        })?;

        // Per-frame bind groups (all three bindings, atlas/sampler are static)
        let mut frame_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut vertex_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut index_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
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
            let ib = device.create_buffer(&BufferDesc {
                label: Some("ui indices"),
                size: INITIAL_RING_BYTES,
                usage: BufferUsage::INDEX,
                memory: MemoryLocation::HostUpload,
            })?;
            let bg = device.create_bind_group(&BindGroupDesc {
                label: Some("ui frame"),
                layout: bind_group_layout,
                entries: &[
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
                        resource: BindingResource::whole_buffer(vb),
                    },
                ],
                variable_count: None,
            })?;
            vertex_buffers.push(vb);
            index_buffers.push(ib);
            frame_groups.push(bg);
            last_vertex_count.push(0);
            last_index_count.push(0);
            vertex_capacity.push(INITIAL_RING_BYTES);
            index_capacity.push(INITIAL_RING_BYTES);
        }

        let set_layouts = [bind_group_layout];
        // Unconditional: `new` refused a device without push constants above,
        // so the layout the pass records against always has the block.
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ui"),
            bind_group_layouts: &set_layouts,
            push_constants: Some(PushConstantRange {
                stages: ShaderStages::VERTEX,
                offset: 0,
                size: std::mem::size_of::<UiPushConstants>() as u32,
            }),
        })?;

        let ui_module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("ui.slang"),
            spirv: UI.spirv(),
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
                entry_point: entry(&UI, Stage::Vertex)?,
            },
            fragment: Some(ShaderEntry {
                module: ui_module,
                entry_point: entry(&UI, Stage::Fragment)?,
            }),
            primitive: PrimitiveState::default(), // TriangleList, no culling
            depth_stencil: None,
            multisample: Default::default(),
            color_targets: &ui_targets,
        });
        device.destroy_shader_module(ui_module);
        let pipeline = ui_pipeline?;

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
            let fresh = device.create_bind_group(&BindGroupDesc {
                label: Some("ui frame"),
                layout: self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(self.atlas_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::Sampler(self.atlas_sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(self.vertex_buffers[idx]),
                    },
                ],
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

        graph
            .add_render_pass("ui-composite")
            // Draw on top of the tonemapped target with alpha blending.
            .color(target, LoadOp::Load, StoreOp::Store, Default::default())
            .execute(move |ctx| {
                let push = UiPushConstants {
                    viewport: [extent.0 as f32, extent.1 as f32],
                };
                let push_bytes: &[u8] = bytemuck::bytes_of(&push);
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, bg, &[], pipeline_layout);
                // Write push constants for the viewport
                encoder.push_constants(ShaderStages::VERTEX, 0, push_bytes, pipeline_layout);
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

    /// The UI shader has nowhere but a push constant to learn the viewport
    /// from, so a device without them is refused rather than shown a UI scaled
    /// by whatever was left in the block.
    #[test]
    fn a_device_without_push_constants_is_refused() {
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
        let error = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect_err("no push constants, no UI pass");
        assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");
    }
}
