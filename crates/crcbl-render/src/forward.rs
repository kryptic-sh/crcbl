//! The frame milestones 3, 4 and 5 draw: a lit mesh into an HDR target, then a
//! tonemap into whatever the caller is presenting to.
//!
//! ```text
//! ┌ forward ────────────────────────────┐   ┌ tonemap ─────────────────┐
//! │ scene-color  Rgba16Float  Clear     │──▶│ reads scene-color        │
//! │ scene-depth  D32Float     Clear 0.0 │   │ writes the target        │
//! │ reads camera UBO + cube SSBO + IBO  │   │ full-screen triangle     │
//! └─────────────────────────────────────┘   └──────────────────────────┘
//! ```
//!
//! Two passes and one graph. Everything that makes them ordered, everything that
//! transitions `scene-color` from a colour attachment into a sampled texture,
//! and everything that returns the target to [`ResourceState::Present`] is
//! computed by [`crate::graph`] — **there is not one hand-written barrier in
//! this file, or anywhere above the seam.**
//!
//! # Scope
//!
//! `docs/plan/02-vulkan-backend.md`'s ladder rungs 3–5, and nothing beyond:
//!
//! * **3** — depth-tested spinning mesh, perspective camera in a uniform buffer.
//! * **4** — one directional light, Lambert + Blinn (in `mesh.slang`).
//! * **5** — orthographic mode, which is [`Camera::projection`] and *nothing
//!   else*: no second pipeline, no branch in this file, no shader permutation.
//!
//! Explicitly not here: geometry pools, instance deltas, GPU culling, indirect
//! draw count (all P7), bindless at scale (P3), shadows and real post (P7),
//! materials (topic 37), asset loading (P9). The mesh is a constant in
//! `crcbl-shaders` because rung 3 says "hardcoded cube/sphere".
//!
//! # Uniforms are a ring, and they have to be
//!
//! The camera spins, so the uniform block is rewritten every frame — and the
//! previous frame may still be reading it. One buffer would be a
//! read-after-write hazard *across* submissions, which is precisely what
//! `CRCBL_VK_SYNC_VALIDATION=1` exists to find and precisely what
//! `docs/plan/02-vulkan-backend.md` calls this stage's headline risk. So there
//! is one uniform buffer and one bind group per frame in flight, and
//! [`ForwardRenderer::begin_frame`] rotates them.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ColorTargetState, CommandEncoderDesc, CullMode, DepthStencilState, Device,
    FilterMode, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageViewHandle,
    IndexFormat, LoadOp, MemoryLocation, MultisampleState, PipelineLayoutDesc,
    PipelineLayoutHandle, PrimitiveState, QueueHandle, ResourceState, SamplerAddressMode,
    SamplerDesc, SamplerHandle, ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp, SubmitInfo,
};
use crcbl_shaders::{MESH, Stage, TONEMAP, mesh};
use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, DirectionalLight};
use crate::graph::{ImageId, ImportedImage, RenderGraph};
use crate::transient::TransientImageDesc;

/// The clear behind the mesh, in **linear** light.
///
/// The scene target is `Rgba16Float`, so this is a linear value and the tonemap
/// pass plus the swapchain's sRGB encode are what turn it into pixels. Writing a
/// display-referred colour here would come out visibly paler, which is the
/// classic HDR-pipeline mistake and worth a sentence rather than a surprise.
pub const SCENE_CLEAR: [f32; 4] = [0.012, 0.016, 0.030, 1.0];

/// How many frames of uniform buffers to keep. Matches the frame loop's
/// frames-in-flight.
pub const FRAMES_IN_FLIGHT: usize = 2;

/// Everything the forward frame owns, created once.
#[derive(Debug)]
pub struct ForwardRenderer {
    // Geometry, uploaded once at startup.
    vertices: BufferHandle,
    indices: BufferHandle,
    index_count: u32,

    // Per-frame uniforms, one set per frame in flight.
    uniforms: Vec<BufferHandle>,
    mesh_groups: Vec<BindGroupHandle>,
    frame: usize,

    mesh_layout: BindGroupLayoutHandle,
    mesh_pipeline_layout: PipelineLayoutHandle,
    mesh_pipeline: GraphicsPipelineHandle,

    tonemap_layout: BindGroupLayoutHandle,
    tonemap_pipeline_layout: PipelineLayoutHandle,
    tonemap_pipeline: GraphicsPipelineHandle,
    sampler: SamplerHandle,
    /// Rebuilt only when the scene target's view changes, which is only on a
    /// resize. The graph hands the view to the pass body; caching against it is
    /// what keeps a steady-state frame free of descriptor writes.
    tonemap_group: Option<(ImageViewHandle, BindGroupHandle)>,

    /// The format the tonemap pipeline was built for. A swapchain format change
    /// needs a new pipeline, which is why it is remembered rather than assumed.
    target_format: Format,
}

/// What a partly-built [`ForwardRenderer`] has to give back.
///
/// `new` creates a dozen objects with `?` between them, and the seam's
/// `destroy_*` is explicit — so a failure half way through used to leak
/// everything created before it. The recorder's leak assertions cover the happy
/// path; this covers the other one.
#[derive(Default)]
struct Rollback {
    buffers: Vec<BufferHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    pipelines: Vec<GraphicsPipelineHandle>,
    samplers: Vec<SamplerHandle>,
}

impl Rollback {
    /// Releases everything, in the same dependency order as
    /// [`ForwardRenderer::destroy`].
    fn run(self, device: &dyn Device) {
        for handle in self.samplers {
            device.destroy_sampler(handle);
        }
        for handle in self.pipelines {
            device.destroy_graphics_pipeline(handle);
        }
        for handle in self.pipeline_layouts {
            device.destroy_pipeline_layout(handle);
        }
        for handle in self.bind_groups {
            device.destroy_bind_group(handle);
        }
        for handle in self.bind_group_layouts {
            device.destroy_bind_group_layout(handle);
        }
        for handle in self.buffers {
            device.destroy_buffer(handle);
        }
    }
}

impl ForwardRenderer {
    /// Builds both pipelines and uploads the cube.
    ///
    /// `target_format` must be the format the tonemap pass will render into —
    /// dynamic rendering checks pipeline and attachment formats against each
    /// other at pass-begin time, not at creation.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. A backend that cannot build a pipeline
    /// says so here rather than drawing nothing later — and a failure part-way
    /// through releases everything already created, so a caller that retries or
    /// exits leaves nothing behind.
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

    /// The body of [`ForwardRenderer::new`], recording what it has created into
    /// `rollback` as it goes.
    fn build(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        let vertices = upload(
            device,
            queue,
            "cube vertices",
            BufferUsage::STORAGE,
            &mesh::cube_vertex_bytes(),
        )?;
        rollback.buffers.push(vertices);
        let indices = upload(
            device,
            queue,
            "cube indices",
            BufferUsage::INDEX,
            &mesh::cube_index_bytes(),
        )?;
        rollback.buffers.push(indices);

        // --- the mesh pass ---
        //
        // No `BindingFlags` anywhere: this layout is legal on **both** tiers,
        // because a lit mesh is not a Tier A feature. The bindless shape — a
        // trailing `VARIABLE_COUNT | PARTIALLY_BOUND | UPDATE_AFTER_BIND`
        // texture array — is topic 03's, at P3.
        let mesh_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                kind: BindingKind::StorageBuffer {
                    // The shader says `StructuredBuffer`, not
                    // `RWStructuredBuffer`, so this is the truth rather than a
                    // hint — and it is what lets the graph merge read-after-read.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let mesh_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("mesh frame"),
            entries: &mesh_entries,
        })?;
        rollback.bind_group_layouts.push(mesh_layout);

        let mut uniforms = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut mesh_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for index in 0..FRAMES_IN_FLIGHT {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("mesh frame uniforms"),
                size: mesh::FRAME_UNIFORMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(buffer);
            let entries = [
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(buffer),
                },
                BindGroupEntry {
                    binding: 1,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(vertices),
                },
            ];
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("mesh frame"),
                layout: mesh_layout,
                entries: &entries,
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            uniforms.push(buffer);
            mesh_groups.push(group);
            let _ = index;
        }

        let mesh_set_layouts = [mesh_layout];
        let mesh_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("mesh"),
            bind_group_layouts: &mesh_set_layouts,
            // The camera goes in the uniform buffer above rather than here:
            // `Features::PUSH_CONSTANTS` is absent on Tier B and the seam is
            // explicit that the substitute is a data-layout decision. So this
            // file needs no tier branch at all — it has been on Tier B's shape
            // since P1, and `crate::ui_pass` is where the split actually bites.
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(mesh_pipeline_layout);

        // Entry points resolved before the module exists: a manifest that
        // disagreed with the SPIR-V would otherwise fail inside the descriptor
        // literal, with the module already created and nothing holding it.
        let mesh_vertex = entry(&MESH, Stage::Vertex)?;
        let mesh_fragment = entry(&MESH, Stage::Fragment)?;
        let mesh_module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("mesh.slang"),
            spirv: MESH.spirv(),
            wgsl: MESH.wgsl(),
            msl: MESH.msl(),
        })?;
        let mesh_targets = [ColorTargetState::opaque(Format::Rgba16Float)];
        let mesh_pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("forward mesh"),
            layout: mesh_pipeline_layout,
            vertex: ShaderEntry {
                module: mesh_module,
                entry_point: mesh_vertex,
            },
            fragment: Some(ShaderEntry {
                module: mesh_module,
                entry_point: mesh_fragment,
            }),
            primitive: PrimitiveState {
                // Back-face culling is on from the first mesh. The cube's
                // winding is asserted by `crcbl-shaders`' own tests, so a face
                // that vanished would be a *test* failure rather than a
                // debugging session — and a mesh drawn without culling would let
                // a winding mistake survive into P7's geometry pool.
                cull_mode: CullMode::Back,
                ..PrimitiveState::default()
            },
            // Milestone 3's depth test, and the seam's default is already
            // reversed-Z: `Greater` against `D32Float`, writes on. The clear
            // value that agrees with it comes from the graph
            // (`PassBuilder::clear_depth`), and the projection matrix that
            // agrees with *both* comes from `crate::camera`.
            depth_stencil: Some(DepthStencilState::default()),
            multisample: MultisampleState::default(),
            color_targets: &mesh_targets,
        });
        device.destroy_shader_module(mesh_module);
        let mesh_pipeline = mesh_pipeline?;
        rollback.pipelines.push(mesh_pipeline);

        // --- the tonemap pass ---
        let tonemap_entries = [
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
        ];
        let tonemap_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("tonemap scene"),
            entries: &tonemap_entries,
        })?;
        rollback.bind_group_layouts.push(tonemap_layout);
        let tonemap_set_layouts = [tonemap_layout];
        let tonemap_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("tonemap"),
            bind_group_layouts: &tonemap_set_layouts,
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(tonemap_pipeline_layout);
        let tonemap_vertex = entry(&TONEMAP, Stage::Vertex)?;
        let tonemap_fragment = entry(&TONEMAP, Stage::Fragment)?;
        let tonemap_module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("tonemap.slang"),
            spirv: TONEMAP.spirv(),
            wgsl: TONEMAP.wgsl(),
            msl: TONEMAP.msl(),
        })?;
        let tonemap_targets = [ColorTargetState::opaque(target_format)];
        let tonemap_pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("tonemap"),
            layout: tonemap_pipeline_layout,
            vertex: ShaderEntry {
                module: tonemap_module,
                entry_point: tonemap_vertex,
            },
            fragment: Some(ShaderEntry {
                module: tonemap_module,
                entry_point: tonemap_fragment,
            }),
            // The full-screen triangle is deliberately oversized, so two of its
            // vertices are outside the viewport and its winding is not worth
            // reasoning about. `CullMode::None` is the honest setting.
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            color_targets: &tonemap_targets,
        });
        device.destroy_shader_module(tonemap_module);
        let tonemap_pipeline = tonemap_pipeline?;
        rollback.pipelines.push(tonemap_pipeline);

        // Nearest, not linear: see `tonemap.slang` on why a 1:1 blit must not
        // leave a golden image depending on two rasterisers agreeing about
        // texel-centre arithmetic.
        let sampler = device.create_sampler(&SamplerDesc {
            label: Some("tonemap scene"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(sampler);

        Ok(Self {
            vertices,
            indices,
            index_count: u32::try_from(mesh::CUBE_INDEX_COUNT).expect("a cube is small"),
            uniforms,
            mesh_groups,
            frame: 0,
            mesh_layout,
            mesh_pipeline_layout,
            mesh_pipeline,
            tonemap_layout,
            tonemap_pipeline_layout,
            tonemap_pipeline,
            sampler,
            tonemap_group: None,
            target_format,
        })
    }

    /// Rotates to the next frame's uniform buffer and writes this frame's
    /// camera and light into it.
    ///
    /// Must be called once per frame, before [`ForwardRenderer::add_passes`].
    /// Separate from `add_passes` because the write happens on the CPU and the
    /// passes are recorded on the GPU timeline: fusing them would hide the ring
    /// rotation inside a function whose name says nothing about it.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the uniform buffer could not be written.
    pub fn begin_frame(
        &mut self,
        device: &dyn Device,
        camera: &Camera,
        light: &DirectionalLight,
        model: Mat4,
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        self.frame = (self.frame + 1) % self.uniforms.len();

        // A minimised window reports a zero extent in *either* dimension, and
        // `Projection::matrix` asserts a finite positive aspect. Guarding only
        // the height left `extent.0 == 0` producing `0.0`, which trips that
        // assert and takes the frame loop down with it.
        let aspect = if extent.0 == 0 || extent.1 == 0 {
            1.0
        } else {
            extent.0 as f32 / extent.1 as f32
        };
        let direction = light.direction.normalize_or_zero();
        let uniforms = mesh::FrameUniforms {
            view_proj: camera.view_projection(aspect).to_cols_array(),
            model: model.to_cols_array(),
            camera_position: camera.eye.extend(1.0).to_array(),
            light_direction: direction.extend(0.0).to_array(),
            light_color: light.color.extend(0.0).to_array(),
            ambient: light.ambient.extend(0.0).to_array(),
        };
        device.write_buffer(self.uniforms[self.frame], 0, &uniforms.to_bytes())
    }

    /// Adds the forward and tonemap passes to `graph`, rendering into `target`,
    /// and returns the HDR scene target they went through.
    ///
    /// `target` is normally the imported swapchain image. Everything else — the
    /// HDR scene colour, the depth buffer, and every barrier between them — is
    /// the graph's.
    ///
    /// The returned [`ImageId`] is the `Rgba16Float` scene colour. A caller that
    /// wants to add a pass of its own after the tonemap — a debug overlay, or a
    /// readback proving the HDR range is real — declares a read of it and the
    /// graph works out the transition, exactly as it does for the tonemap.
    pub fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        target: ImageId,
        extent: (u32, u32),
    ) -> ImageId {
        let scene_color =
            graph.create_image("scene-color", TransientImageDesc::scene_color(extent));
        let scene_depth =
            graph.create_image("scene-depth", TransientImageDesc::scene_depth(extent));

        let group = self.mesh_groups[self.frame];
        let pipeline = self.mesh_pipeline;
        let layout = self.mesh_pipeline_layout;
        let vertices = self.vertices;
        let indices = self.indices;
        let index_count = self.index_count;

        graph
            .add_render_pass("forward")
            .clear_color(scene_color, SCENE_CLEAR)
            .clear_depth(scene_depth)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], layout);
                encoder.bind_index_buffer(indices, 0, IndexFormat::Uint32);
                encoder.draw_indexed(0..index_count, 0, 0..1);
                let _ = vertices;
            });

        // The tonemap group names a *graph-owned* view, so it can only be built
        // once the graph has realised one. It is cached against the view handle
        // and therefore rebuilt only on a resize.
        let sampler = self.sampler;
        let layout = self.tonemap_layout;
        let pipeline_layout = self.tonemap_pipeline_layout;
        let tonemap_pipeline = self.tonemap_pipeline;
        let cached = &mut self.tonemap_group;

        graph
            .add_render_pass("tonemap")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target, so loading or clearing it is pure bandwidth.
            .color(
                target,
                LoadOp::DontCare,
                StoreOp::Store,
                crcbl_hal::ClearValue::default(),
            )
            .read_image(scene_color)
            .execute(move |ctx| {
                let view = ctx.image_view(scene_color);
                let device = ctx.device();
                let group = match cached {
                    Some((cached_view, group)) if *cached_view == view => *group,
                    other => {
                        if let Some((_, stale)) = other.take() {
                            device.destroy_bind_group(stale);
                        }
                        let entries = [
                            BindGroupEntry {
                                binding: 0,
                                array_index: 0,
                                resource: BindingResource::ImageView(view),
                            },
                            BindGroupEntry {
                                binding: 1,
                                array_index: 0,
                                resource: BindingResource::Sampler(sampler),
                            },
                        ];
                        match device.create_bind_group(&BindGroupDesc {
                            label: Some("tonemap scene"),
                            layout,
                            entries: &entries,
                            variable_count: None,
                        }) {
                            Ok(group) => {
                                *other = Some((view, group));
                                group
                            }
                            Err(error) => {
                                // Recording a pass that draws nothing is better
                                // than aborting a frame: the window goes black,
                                // the log says why, and the next frame retries.
                                log::error!("graph: tonemap bind group failed: {error}");
                                return;
                            }
                        }
                    }
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(tonemap_pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                // Three vertices, no geometry bound, no vertex buffer anywhere.
                encoder.draw(0..3, 0..1);
            });

        scene_color
    }

    /// The model matrix for a cube spinning at `seconds` into the run.
    ///
    /// Two axes at incommensurable rates, so the animation never repeats and
    /// every face comes into view — which is what makes "the cube is spinning"
    /// observable rather than a claim about a matrix.
    #[must_use]
    pub fn spin(seconds: f32) -> Mat4 {
        Mat4::from_quat(
            Quat::from_axis_angle(Vec3::Y, seconds * 0.9)
                * Quat::from_axis_angle(Vec3::X, seconds * 0.55),
        )
    }

    /// The format the tonemap pass renders into.
    #[must_use]
    pub const fn target_format(&self) -> Format {
        self.target_format
    }

    /// Describes the imported swapchain image the frame ends in.
    ///
    /// A helper rather than something every caller writes, because getting
    /// `initial`/`final_state` wrong is exactly the hand-written barrier this
    /// slice removed: an acquired image's contents are undefined, and the
    /// compositor may only be handed one in [`ResourceState::Present`].
    #[must_use]
    pub fn present_target(
        image: crcbl_hal::ImageHandle,
        view: ImageViewHandle,
        format: Format,
        extent: (u32, u32),
    ) -> ImportedImage {
        ImportedImage {
            image,
            view,
            format,
            extent,
            initial: ResourceState::Undefined,
            final_state: ResourceState::Present,
        }
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        if let Some((_, group)) = self.tonemap_group {
            device.destroy_bind_group(group);
        }
        device.destroy_sampler(self.sampler);
        device.destroy_graphics_pipeline(self.tonemap_pipeline);
        device.destroy_pipeline_layout(self.tonemap_pipeline_layout);
        device.destroy_bind_group_layout(self.tonemap_layout);

        device.destroy_graphics_pipeline(self.mesh_pipeline);
        device.destroy_pipeline_layout(self.mesh_pipeline_layout);
        for group in self.mesh_groups {
            device.destroy_bind_group(group);
        }
        device.destroy_bind_group_layout(self.mesh_layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        device.destroy_buffer(self.indices);
        device.destroy_buffer(self.vertices);
    }
}

/// The entry point for `stage`, or an error naming the shader.
///
/// Unreachable in practice — `crcbl-shaders`' own tests assert both modules
/// expose both stages — but the alternative is an `expect` in engine code.
fn entry(shader: &crcbl_shaders::Shader, stage: Stage) -> Result<&'static str, HalError> {
    shader.entry_point(stage).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "{}.slang exposes no unambiguous {stage:?} entry point; the committed SPIR-V and its \
             manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix",
            shader.name()
        ))
    })
}

/// Uploads `bytes` into a fresh device-local buffer through a staging copy.
///
/// The real upload path — staging buffer, copy, barrier into the state the
/// shader reads it in — rather than a host-visible buffer written directly.
/// `docs/plan/03-gpu-driven-rendering.md` §3.1's upload path is a staging ring
/// with timeline tracking, and doing the shape once at startup means P7 changes
/// *when* this happens rather than *what* happens.
///
/// The barrier here is the one exception to "no manual barriers outside the
/// graph", and it is not really one: this is a **startup** submission with no
/// graph in the room, before any frame. Every barrier in a *frame* is the
/// graph's.
///
/// Every object created here is released on every path out, failing ones
/// included: a `?` that walked away from the staging buffer would leak one per
/// failed startup and leave the recorder's leak assertions passing.
fn upload(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    usage: BufferUsage,
    bytes: &[u8],
) -> Result<BufferHandle, HalError> {
    let size = bytes.len() as u64;
    let staging = device.create_buffer(&BufferDesc {
        label: Some("upload staging"),
        size,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    })?;
    let target = upload_into_target(device, queue, label, usage, bytes, staging);
    device.destroy_buffer(staging);
    target
}

/// The half of [`upload`] that owns the destination buffer.
fn upload_into_target(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    usage: BufferUsage,
    bytes: &[u8],
    staging: BufferHandle,
) -> Result<BufferHandle, HalError> {
    let size = bytes.len() as u64;
    device.write_buffer(staging, 0, bytes)?;

    let target = device.create_buffer(&BufferDesc {
        label: Some(label),
        size,
        usage: usage | BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::DeviceLocal,
    })?;
    match record_upload(device, queue, usage, size, staging, target) {
        Ok(()) => Ok(target),
        Err(error) => {
            device.destroy_buffer(target);
            Err(error)
        }
    }
}

/// Records, submits and drains the staging copy.
fn record_upload(
    device: &dyn Device,
    queue: QueueHandle,
    usage: BufferUsage,
    size: u64,
    staging: BufferHandle,
    target: BufferHandle,
) -> Result<(), HalError> {
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("startup upload"),
        queue,
    });
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: staging,
        src_offset: 0,
        dst: target,
        dst_offset: 0,
        size,
    });
    encoder.pipeline_barrier(&crcbl_hal::Barriers {
        buffers: &[crcbl_hal::BufferBarrier::new(
            target,
            ResourceState::TransferDst,
            if usage.contains(BufferUsage::INDEX) {
                ResourceState::IndexBuffer
            } else {
                ResourceState::ShaderRead
            },
        )],
        ..crcbl_hal::Barriers::default()
    });
    let commands = encoder.finish()?;

    // The seam sanctions `wait_idle` as "a shutdown and test primitive".
    // Startup is neither, but it is also not a frame, and the staging buffer
    // cannot be freed until the copy has run.
    let submitted = device
        .submit(queue, &SubmitInfo::new(&[commands]))
        .and_then(|()| device.wait_idle());
    device.destroy_command_buffer(commands);
    submitted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance, QueueKind};

    fn open() -> (Recorder, Box<dyn Device>, QueueHandle) {
        let recorder = Recorder::new();
        let instance = NullInstance::tier_a().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (recorder, device, queue)
    }

    /// The whole renderer builds and tears down against a backend with no
    /// driver, which is what makes it testable on every CI leg.
    #[test]
    fn the_forward_renderer_builds_and_leaks_nothing() {
        let (recorder, device, queue) = open();
        let before = recorder.total_live_objects();
        let renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
            .expect("the null backend accepts every descriptor");
        assert_eq!(renderer.target_format(), Format::Rgba8UnormSrgb);
        assert!(recorder.total_live_objects() > before);
        renderer.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "every object created must be destroyed"
        );
        recorder.assert_valid();
    }

    /// The uniform ring really rotates, or a spinning cube is a
    /// read-after-write hazard across submissions.
    #[test]
    fn the_uniform_ring_rotates() {
        let (_, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let camera = Camera::default();
        let light = DirectionalLight::default();

        let mut seen = Vec::new();
        for frame in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(
                    device.as_ref(),
                    &camera,
                    &light,
                    ForwardRenderer::spin(frame as f32),
                    (64, 48),
                )
                .expect("write");
            seen.push(renderer.uniforms[renderer.frame]);
        }
        assert_ne!(
            seen[0], seen[1],
            "consecutive frames must not share a buffer"
        );
        assert_eq!(seen[0], seen[FRAMES_IN_FLIGHT], "and the ring must wrap");
        renderer.destroy(device.as_ref());
    }

    /// The spin is a rotation: no scale, no shear, so the shader's
    /// "the 3×3 is its own inverse-transpose" assumption holds.
    #[test]
    fn the_spin_matrix_stays_rigid() {
        for seconds in [0.0f32, 0.5, 3.25, 100.0] {
            let model = ForwardRenderer::spin(seconds);
            let basis = glam::Mat3::from_mat4(model);
            let product = basis * basis.transpose();
            for (row, expected) in [
                (product.x_axis, Vec3::X),
                (product.y_axis, Vec3::Y),
                (product.z_axis, Vec3::Z),
            ] {
                assert!(
                    (row - expected).length() < 1e-5,
                    "the spin at {seconds}s is not orthonormal: {product:?}"
                );
            }
            assert!((model.determinant() - 1.0).abs() < 1e-5, "at {seconds}s");
        }
        // And it actually moves.
        assert_ne!(ForwardRenderer::spin(0.0), ForwardRenderer::spin(1.0));
    }

    /// The swapchain import says what a swapchain image actually needs, which is
    /// the knowledge the hand-written barriers used to carry.
    #[test]
    fn the_present_target_starts_undefined_and_ends_presentable() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let image = pool.insert(0).cast();
        let view = pool.insert(0).cast();
        let imported =
            ForwardRenderer::present_target(image, view, Format::Bgra8UnormSrgb, (1280, 720));
        assert_eq!(imported.initial, ResourceState::Undefined);
        assert_eq!(imported.final_state, ResourceState::Present);
        assert_eq!(imported.extent, (1280, 720));
    }

    /// The scene target is `Rgba16Float`, and the depth target is the
    /// reversed-Z format. Both are locked decisions and both are cheap to pin.
    #[test]
    fn the_scene_targets_are_hdr_and_reversed_z() {
        let color = TransientImageDesc::scene_color((64, 48));
        assert_eq!(color.format, Format::Rgba16Float);
        assert!(color.format.is_hdr_capable());
        assert!(color.usage.contains(crcbl_hal::ImageUsage::SAMPLED));

        let depth = TransientImageDesc::scene_depth((64, 48));
        assert_eq!(depth.format, Format::D32Float);
        assert_eq!(
            DepthStencilState::default().depth_compare,
            crcbl_hal::CompareOp::Greater
        );
    }
}
