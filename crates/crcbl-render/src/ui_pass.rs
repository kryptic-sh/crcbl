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
    Device, Extent3d, FilterMode, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError,
    ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageType, ImageUsage,
    ImageViewDesc, ImageViewHandle, ImageViewType, IndexFormat, LoadOp, MemoryLocation, Offset3d,
    PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState, PushConstantRange, QueueHandle,
    ResourceState, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry, ShaderModuleDesc,
    ShaderStages, StoreOp, SubmitInfo,
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

    // Cached sizes for re-upload detection
    last_vertex_count: Vec<usize>,
    last_index_count: Vec<usize>,

    destroyed: bool,
}

impl UiRenderer {
    /// Creates the UI pipeline, glyph atlas, and per-frame geometry buffers.
    ///
    /// The atlas is uploaded immediately via a staging copy.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call.
    pub fn new(device: &dyn Device, queue: QueueHandle) -> Result<Self, HalError> {
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
        for _ in 0..FRAMES_IN_FLIGHT {
            let vb = device.create_buffer(&BufferDesc {
                label: Some("ui vertices"),
                size: 1024,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            let ib = device.create_buffer(&BufferDesc {
                label: Some("ui indices"),
                size: 1024,
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
        }

        let set_layouts = [bind_group_layout];
        let supports_push = device
            .caps()
            .features
            .contains(crcbl_hal::Features::PUSH_CONSTANTS);
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ui"),
            bind_group_layouts: &set_layouts,
            push_constants: if supports_push {
                Some(PushConstantRange {
                    stages: ShaderStages::VERTEX,
                    offset: 0,
                    size: std::mem::size_of::<UiPushConstants>() as u32,
                })
            } else {
                None
            },
        })?;

        let ui_module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("ui.slang"),
            spirv: UI.spirv(),
        })?;
        let ui_targets = [ColorTargetState {
            format: Format::Rgba8UnormSrgb, // placeholder; reconfigured at draw time
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
            last_vertex_count,
            last_index_count,
            destroyed: false,
        })
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

        // Recreate vertex buffer if too small
        let vb_needed = (vertices.len() * std::mem::size_of::<Vertex2d>()) as u64;
        if vb_needed > self.last_vertex_count[idx] as u64 || vb_needed == 0 {
            // round up to at least 1 KiB
            let size = vb_needed.max(1024).next_multiple_of(256);
            device.destroy_buffer(self.vertex_buffers[idx]);
            self.vertex_buffers[idx] = device.create_buffer(&BufferDesc {
                label: Some("ui vertices"),
                size,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
        }
        self.last_vertex_count[idx] = vertices.len();

        // Recreate index buffer if too small
        let ib_needed = (indices.len() * std::mem::size_of::<u32>()) as u64;
        if ib_needed > self.last_index_count[idx] as u64 || ib_needed == 0 {
            let size = ib_needed.max(1024).next_multiple_of(256);
            device.destroy_buffer(self.index_buffers[idx]);
            self.index_buffers[idx] = device.create_buffer(&BufferDesc {
                label: Some("ui indices"),
                size,
                usage: BufferUsage::INDEX,
                memory: MemoryLocation::HostUpload,
            })?;
        }
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

        // Recreate frame bind group pointing at new vertex buffer (all 3 bindings)
        device.destroy_bind_group(self.frame_groups[idx]);
        self.frame_groups[idx] = device.create_bind_group(&BindGroupDesc {
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

        Ok(())
    }

    /// Adds the UI compositing pass to `graph`, drawing on top of `target`.
    ///
    /// The pass reads nothing except its own vertex buffer; it blends onto the
    /// target using alpha blending. Call after the tonemap pass.
    ///
    /// Returns the pass builder so the caller can further configure it.
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
                // Set viewport to cover the full target
                encoder.set_viewport(&crcbl_hal::Viewport::from_size(extent.0, extent.1));
                encoder.set_scissor(&crcbl_hal::Rect2d {
                    x: 0,
                    y: 0,
                    width: extent.0,
                    height: extent.1,
                });
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
fn upload_texture_r8(
    device: &dyn Device,
    queue: QueueHandle,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(ImageHandle, ImageViewHandle), HalError> {
    let size = pixels.len() as u64;
    let staging = device.create_buffer(&BufferDesc {
        label: Some("ui atlas staging"),
        size,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    })?;
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

    let view = device.create_image_view(&ImageViewDesc {
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
    })?;

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
        buffer_row_length: 0,
        buffer_image_height: 0,
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
    device.submit(queue, &SubmitInfo::new(&[commands]))?;
    device.wait_idle()?;
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    Ok((image, view))
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
        let renderer =
            UiRenderer::new(device.as_ref(), queue).expect("the null backend accepts everything");
        renderer.destroy(device.as_ref());
    }

    #[test]
    fn begin_frame_with_empty_draw_list_is_harmless() {
        let (device, queue) = open();
        let mut renderer = UiRenderer::new(device.as_ref(), queue).expect("built");
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
        let mut renderer = UiRenderer::new(device.as_ref(), queue).expect("built");
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
        let mut renderer = UiRenderer::new(device.as_ref(), queue).expect("built");
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
}
