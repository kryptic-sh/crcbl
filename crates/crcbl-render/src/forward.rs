//! The frame milestones 3, 4 and 5 draw: a lit mesh into an HDR target, then a
//! tonemap into whatever the caller is presenting to.
//!
//! ```text
//! ┌ forward ────────────────────────────┐   ┌ tonemap ─────────────────┐
//! │ scene-color  Rgba16Float  Clear     │──▶│ reads scene-color        │
//! │ scene-depth  D32Float     Clear 0.0 │   │ writes the target        │
//! │ reads camera UBO + vertex/instance  │   │ full-screen triangle     │
//! │ SSBOs + IBO                         │   │                          │
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
//! Explicitly not here: GPU culling, indirect draw count (both P7), bindless at
//! scale (P3), shadows and real post (P7), materials (topic 37), asset loading
//! (P9). The mesh is a constant in `crcbl-shaders` because rung 3 says
//! "hardcoded cube/sphere".
//!
//! What *is* here, since 2026-08, is [`crate::mesh_pool`]: the cube is the first
//! resident of the global vertex and index pools rather than owning two buffers
//! of its own, so it is `{base_vertex, base_index, index_count}` like everything
//! P7 will draw. Same geometry, different residence.
//!
//! And since the same month, [`crate::instance_pool`]: the cube's model matrix
//! is a `GpuInstance` in a storage buffer rather than a field of the frame's
//! uniform block, uploaded by delta. It holds the same matrix `begin_frame` used
//! to write into the uniform, which is why the golden images did not move.
//!
//! # A second resident, and the block that makes it draw
//!
//! A pool whose only mesh is at base vertex 0 cannot tell a base vertex that
//! works from one that is silently cancelled out, and `mesh.slang`'s header
//! shows the four targets disagreeing about exactly that: the SPIR-V subtracts
//! `BaseVertex` back out of `SV_VertexID`, so the pool's base — passed through
//! `draw_indexed`'s own argument — reached the shader as zero, while WGSL and
//! MSL kept it. The same disagreement covers `SV_InstanceID`, which is what
//! [`crate::sprite_pass`] paid for first.
//!
//! So **every draw this pass records passes zero for both of its bases**, and
//! the real ones arrive as a [`mesh::DrawConstants`] block reached through a
//! dynamic offset — one block per draw, written once at build. That is the one
//! formulation no target's lowering can change the meaning of, and it is what
//! makes the pool's *second* resident, [`mesh::pyramid_vertices`], draw its own
//! geometry rather than the cube's.
//!
//! The pyramid is in the pools from `new` but is drawn only when a caller asks
//! for it with [`ForwardRenderer::set_pyramid`]. The frame the samples draw is
//! still the cube alone, which is why their golden images did not move either.
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
    BufferUsage, ColorTargetState, CullMode, DepthStencilState, Device, FilterMode, Format,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageViewHandle, IndexFormat, LoadOp,
    MemoryLocation, MultisampleState, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState,
    QueueHandle, ResourceState, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry,
    ShaderModuleDesc, ShaderStages, StoreOp,
};
use crcbl_shaders::{MESH, Stage, TONEMAP, mesh};
use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, DirectionalLight};
use crate::graph::{ImageId, ImportedImage, RenderGraph};
use crate::instance_pool::{InstanceHandle, InstancePool, InstancePoolDesc};
use crate::mesh_pool::{MeshPool, MeshPoolDesc, MeshPoolError, MeshRange};
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

/// Vertices the geometry pool holds.
///
/// Two meshes are resident today, and the pool exists so that stops being the
/// interesting number. It is device-local memory reserved at start-up and never
/// grown — [`crate::mesh_pool`] says why — so it is sized for the scene P7 puts
/// in it rather than for the meshes P1 draws.
const POOL_VERTEX_CAPACITY: u32 = 64 * 1024;

/// Indices the geometry pool holds. Four per vertex is the usual ratio for
/// indexed triangle soup, rounded up.
const POOL_INDEX_CAPACITY: u32 = 256 * 1024;

/// Instances the instance pool holds, per frame in flight.
///
/// Two are resident, for the same reason the geometry pools hold two meshes.
/// Sized against topic 03's exit criterion — "sandbox scene: 10k+ instanced
/// meshes" — rather than against them, because the buffers are reserved at
/// start-up and never grown.
const POOL_INSTANCE_CAPACITY: u32 = 16 * 1024;

/// How many [`mesh::DrawConstants`] blocks the pass keeps: one per draw it can
/// record, which is one per resident mesh.
///
/// A fixed number rather than a growing ring because the pass records a fixed
/// list of draws — §3.3's indirect path is what makes a frame's draw count a
/// runtime quantity, and it replaces this block with a GPU-side mesh table
/// rather than growing it.
const DRAW_BLOCKS: u64 = 2;

/// One draw call: which mesh, and where the block naming its bases sits.
///
/// Both halves are fixed once the mesh is resident and the instance is
/// allocated, so the constant block is written at build and never again.
#[derive(Clone, Copy, Debug)]
struct Draw {
    /// Where the mesh lives in the pools.
    range: MeshRange,
    /// The dynamic offset of this draw's [`mesh::DrawConstants`] block.
    constant_offset: u32,
}

/// Everything the forward frame owns, created once.
#[derive(Debug)]
pub struct ForwardRenderer {
    // Geometry: the global pools, and the two meshes resident in them. Each
    // range is resolved once, at build, and `MeshPool::mesh` only hands one out
    // for a mesh whose upload has completed — so there is no way to reach a
    // draw below with a mesh the GPU has not received.
    pool: MeshPool,
    cube: Draw,
    pyramid: Draw,
    /// Whether this frame draws the pyramid, from
    /// [`ForwardRenderer::set_pyramid`]. `false` until a caller asks for it, so
    /// the frame the samples draw is the cube alone.
    pyramid_visible: bool,

    // The instance array, and the two objects' places in it. `begin_frame`
    // rewrites the cube's transform and the pool uploads only what changed.
    instances: InstancePool,
    cube_instance: InstanceHandle,
    pyramid_instance: InstanceHandle,

    // Per-frame uniforms, one set per frame in flight.
    uniforms: Vec<BufferHandle>,
    /// One [`mesh::DrawConstants`] block per draw, a device dynamic-offset
    /// alignment apart. Shared by every frame's bind group rather than ringed,
    /// because nothing rewrites it after `build`: a mesh's base vertex and an
    /// object's instance index are decided when the pools allocate them.
    draw_constants: BufferHandle,
    mesh_groups: Vec<BindGroupHandle>,
    /// Which frame-in-flight slot is current. Set from
    /// [`InstancePool::begin_frame`]'s return rather than counted here, so the
    /// uniform ring and the instance ring cannot drift apart.
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
#[derive(Debug, Default)]
struct Rollback {
    buffers: Vec<BufferHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    pipelines: Vec<GraphicsPipelineHandle>,
    samplers: Vec<SamplerHandle>,
    /// The geometry pool, which owns two buffers, a semaphore and anything
    /// still staged — so it cannot be rolled back as a list of handles.
    pool: Option<MeshPool>,
    /// The instance pool, which owns one buffer per frame in flight.
    instances: Option<InstancePool>,
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
        if let Some(pool) = self.instances {
            pool.destroy(device);
        }
        if let Some(pool) = self.pool {
            pool.destroy(device);
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
        let (pool, cube, pyramid) = Self::build_geometry(device, queue)?;
        // The handle is `Copy`, so it can be read out before the pool becomes
        // the rollback's — which it must be before the first `?` below, or a
        // failed pipeline would leak two device-local buffers. The index pool is
        // not named here at all: it is bound at draw time rather than
        // descriptor-written, and only the vertex pool reaches a bind group.
        let vertices = pool.vertex_buffer();
        rollback.pool = Some(pool);

        let (instances, cube_instance, pyramid_instance) = Self::build_instances(device)?;
        // Same handle-then-hand-over dance as the geometry pool above: the
        // buffers are `Copy` and are read out before the pool becomes the
        // rollback's, which it must be before the first `?` below. The two
        // instance *indices* travel the same way, into the constant blocks
        // below — after which nothing needs the pool again until `begin_frame`.
        let instance_buffers: Vec<BufferHandle> = instances.buffers().to_vec();
        let (cube_instance_index, pyramid_instance_index) = match (
            instances.index(cube_instance),
            instances.index(pyramid_instance),
        ) {
            (Some(cube), Some(pyramid)) => (cube, pyramid),
            _ => unreachable!("both instances were just inserted into a fresh pool"),
        };
        rollback.instances = Some(instances);

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
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX,
                kind: BindingKind::StorageBuffer {
                    // `StructuredBuffer` again: the vertex stage reads its
                    // instance and writes nothing. The *host* writes this one
                    // every frame, which is a mapped write rather than a shader
                    // one and does not make it read-write here.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::VERTEX,
                // `dynamic: true`, and it is the whole mechanism: it is what
                // lets a draw say which mesh and which instance it is without
                // `draw_indexed`'s own bases, which the module docs and
                // `mesh.slang`'s header explain the targets disagree about.
                // Same shape, for the same reason, as `crate::sprite_pass`'s.
                kind: BindingKind::UniformBuffer { dynamic: true },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let mesh_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("mesh frame"),
            entries: &mesh_entries,
        })?;
        rollback.bind_group_layouts.push(mesh_layout);

        // A dynamic offset must be a multiple of the device's alignment, and one
        // block has to fit inside one stride. Read from the device rather than
        // assumed, exactly as `crate::sprite_pass` does: 256 on WebGPU, 64 on a
        // typical desktop Vulkan driver.
        let alignment = device.caps().limits.min_uniform_buffer_offset_alignment;
        let draw_stride = u32::try_from(
            (mesh::DRAW_CONSTANTS_SIZE as u64).next_multiple_of(alignment),
        )
        .map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "min_uniform_buffer_offset_alignment is {alignment}, which no dynamic \
                         offset can express"
            ))
        })?;
        let draw_constants = device.create_buffer(&BufferDesc {
            label: Some("mesh draw constants"),
            size: u64::from(draw_stride) * DRAW_BLOCKS,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        })?;
        rollback.buffers.push(draw_constants);
        // Written here and never again: a mesh's base vertex and an object's
        // instance index are decided by the pools, and neither pool moves what
        // it has handed out.
        let draw_of = |slot: u32, range: MeshRange, instance: u32| -> Result<Draw, HalError> {
            let constant_offset = slot * draw_stride;
            device.write_buffer(
                draw_constants,
                u64::from(constant_offset),
                &mesh::DrawConstants {
                    base_vertex: range.base_vertex,
                    base_instance: instance,
                }
                .to_bytes(),
            )?;
            Ok(Draw {
                range,
                constant_offset,
            })
        };
        let cube = draw_of(0, cube, cube_instance_index)?;
        let pyramid = draw_of(1, pyramid, pyramid_instance_index)?;

        let mut uniforms = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut mesh_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for &slot_instances in &instance_buffers {
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
                BindGroupEntry {
                    binding: 2,
                    array_index: 0,
                    // **This frame's slot of the instance ring, not a shared
                    // buffer.** Binding one buffer here for every group would
                    // undo the ring and reintroduce the cross-submission
                    // read-after-write hazard it exists to prevent.
                    resource: BindingResource::whole_buffer(slot_instances),
                },
                BindGroupEntry {
                    binding: 3,
                    array_index: 0,
                    // **One block, not the whole buffer.** The binding is
                    // dynamic, so the bind's offset is added on top of this one
                    // and both Vulkan and WebGPU require `offset + dynamic +
                    // size` to stay inside the buffer — bound whole, the very
                    // first non-zero dynamic offset would be out of range.
                    resource: BindingResource::Buffer {
                        buffer: draw_constants,
                        offset: 0,
                        size: mesh::DRAW_CONSTANTS_SIZE as u64,
                    },
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
            // **`None` is the truthful answer for a two-stage module, not an
            // omission.** A DXIL container holds exactly one entry point, so
            // `crcbl_shaders::Shader::dxil` takes an entry-point name and there
            // is no container that is "the DXIL for mesh.slang". Reaching D3D12
            // means one module per stage here, which is a change to this pass's
            // handle bookkeeping rather than to the descriptor —
            // `docs/backlog.md` carries it.
            dxil: None,
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
            // `None` for the reason the mesh module above gives: one DXIL
            // container, one entry point, and this module has two.
            dxil: None,
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
            pool: rollback
                .pool
                .take()
                .unwrap_or_else(|| unreachable!("the pool was placed in the rollback above")),
            cube,
            pyramid,
            pyramid_visible: false,
            instances: rollback
                .instances
                .take()
                .unwrap_or_else(|| unreachable!("the pool was placed in the rollback above")),
            cube_instance,
            pyramid_instance,
            uniforms,
            draw_constants,
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

    /// Creates the geometry pool and makes the cube resident in it.
    ///
    /// Separate from [`ForwardRenderer::build`] because it is **self-cleaning**:
    /// the pool is not the rollback's until this has returned, so a failure
    /// between creating it and flushing the cube releases it here.
    fn build_geometry(
        device: &dyn Device,
        queue: QueueHandle,
    ) -> Result<(MeshPool, MeshRange, MeshRange), HalError> {
        let mut pool = MeshPool::new(
            device,
            &MeshPoolDesc {
                label: Some("forward geometry"),
                vertex_capacity: POOL_VERTEX_CAPACITY,
                index_capacity: POOL_INDEX_CAPACITY,
            },
        )?;
        match Self::residents(device, queue, &mut pool) {
            Ok((cube, pyramid)) => Ok((pool, cube, pyramid)),
            Err(error) => {
                pool.destroy(device);
                Err(error.into())
            }
        }
    }

    /// Creates the instance pool and puts both objects in it.
    ///
    /// Self-cleaning for the same reason [`ForwardRenderer::build_geometry`] is:
    /// the pool is not the rollback's until this has returned.
    ///
    /// Both transforms are left at [`Mat4::IDENTITY`] and every id at zero.
    /// [`ForwardRenderer::begin_frame`] rewrites the cube's before the first
    /// draw and [`ForwardRenderer::set_pyramid`] the pyramid's; the ids stay
    /// zero because nothing reads any of them yet — see
    /// [`crcbl_shaders::mesh::GpuInstance`] on which fields are reserved.
    ///
    /// Neither instance's index is asserted to be anything in particular. It
    /// used to be — the cube had to land at 0, because a draw could address no
    /// other one — and the [`mesh::DrawConstants`] block is what replaced that
    /// constraint with a number the draw carries.
    fn build_instances(
        device: &dyn Device,
    ) -> Result<(InstancePool, InstanceHandle, InstanceHandle), HalError> {
        let mut instances = InstancePool::new(
            device,
            &InstancePoolDesc {
                label: Some("forward instances"),
                capacity: POOL_INSTANCE_CAPACITY,
                frames_in_flight: FRAMES_IN_FLIGHT,
            },
        )?;
        let mut insert = || {
            instances.insert(&mesh::GpuInstance {
                transform: Mat4::IDENTITY.to_cols_array(),
                ..mesh::GpuInstance::default()
            })
        };
        match (insert(), insert()) {
            (Ok(cube), Ok(pyramid)) => Ok((instances, cube, pyramid)),
            (Err(error), _) | (Ok(_), Err(error)) => {
                instances.destroy(device);
                Err(error.into())
            }
        }
    }

    /// Uploads both meshes and returns their ranges — **only** once the
    /// transfers have completed.
    ///
    /// The calls are §3.1's upload path in order: the copies are recorded and
    /// submitted against timeline values, [`MeshPool::flush`] is what makes those
    /// values pass, and [`MeshPool::mesh`] is what refuses to hand out a range
    /// before they have.
    ///
    /// The pyramid is uploaded second, so it is the pool's first resident at a
    /// non-zero base vertex — which is the whole reason it is here, and what the
    /// module docs call the one thing that can tell a working base vertex from a
    /// cancelled one.
    fn residents(
        device: &dyn Device,
        queue: QueueHandle,
        pool: &mut MeshPool,
    ) -> Result<(MeshRange, MeshRange), MeshPoolError> {
        let cube = pool.upload(
            device,
            queue,
            "cube",
            &mesh::cube_vertex_bytes(),
            &mesh::cube_indices(),
        )?;
        let pyramid = pool.upload(
            device,
            queue,
            "pyramid",
            &mesh::pyramid_vertex_bytes(),
            &mesh::pyramid_indices(),
        )?;
        pool.flush(device)?;
        let resolve = |handle| {
            pool.mesh(handle)
                .ok_or(MeshPoolError::NotResident { handle })
        };
        Ok((resolve(cube)?, resolve(pyramid)?))
    }

    /// Rotates to the next frame's uniform buffer and instance buffer, writes
    /// this frame's camera and light into the first, and uploads whatever
    /// changed into the second.
    ///
    /// Must be called once per frame, before [`ForwardRenderer::add_passes`].
    /// Separate from `add_passes` because the write happens on the CPU and the
    /// passes are recorded on the GPU timeline: fusing them would hide the ring
    /// rotation inside a function whose name says nothing about it.
    ///
    /// `model` is the cube's transform, and it is written into the instance
    /// pool rather than into the uniform block. A frame that passes the same
    /// matrix as the last one still marks the instance dirty — see
    /// [`InstancePool::set`], which explains why it does not compare — so the
    /// sandbox's spinning cube uploads 80 bytes a frame and a still one uploads
    /// 80 bytes a frame too. What delta upload buys is that a *second* instance
    /// that did not move costs nothing, which is the property §3.2 is about.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the uniform buffer or the instance buffer could not be
    /// written.
    pub fn begin_frame(
        &mut self,
        device: &dyn Device,
        camera: &Camera,
        light: &DirectionalLight,
        model: Mat4,
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        self.instances.set(
            self.cube_instance,
            &mesh::GpuInstance {
                transform: model.to_cols_array(),
                ..mesh::GpuInstance::default()
            },
        );
        // The instance pool owns the ring, so its slot is the frame index the
        // uniform buffer and the bind group below are picked with.
        self.frame = self.instances.begin_frame(device)?;

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
            camera_position: camera.eye.extend(1.0).to_array(),
            light_direction: direction.extend(0.0).to_array(),
            light_color: light.color.extend(0.0).to_array(),
            ambient: light.ambient.extend(0.0).to_array(),
        };
        device.write_buffer(self.uniforms[self.frame], 0, &uniforms.to_bytes())
    }

    /// Puts the pool's second mesh in the frame at `model`, or takes it back out
    /// with `None`.
    ///
    /// **This is what makes a non-zero `MeshRange::base_vertex` observable**, and
    /// it is why it is here at all: the pyramid is uploaded after the cube, so
    /// it is the pool's first resident that is not at base 0, and a frame
    /// containing it is a frame that fails if the base is lost on the way to the
    /// shader. See the module docs for the disagreement that made losing it the
    /// default.
    ///
    /// Off by default, so the frame every sample draws is the cube alone.
    ///
    /// Takes effect at the next [`ForwardRenderer::begin_frame`], which is what
    /// uploads the instance the transform is written into.
    pub fn set_pyramid(&mut self, model: Option<Mat4>) {
        self.pyramid_visible = model.is_some();
        if let Some(model) = model {
            self.instances.set(
                self.pyramid_instance,
                &mesh::GpuInstance {
                    transform: model.to_cols_array(),
                    ..mesh::GpuInstance::default()
                },
            );
        }
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
        let indices = self.pool.index_buffer();
        let mut draws = vec![self.cube];
        if self.pyramid_visible {
            draws.push(self.pyramid);
        }

        graph
            .add_render_pass("forward")
            .clear_color(scene_color, SCENE_CLEAR)
            .clear_depth(scene_depth)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                // The index pool is bound whole, at offset zero, for every mesh
                // in it: the mesh's place is the draw's first index and its
                // constant block, not a buffer offset. That is what makes one
                // bind enough for the scene P7 puts in here.
                encoder.bind_index_buffer(indices, 0, IndexFormat::Uint32);
                for draw in &draws {
                    // The block written at build for this draw, and the only
                    // thing that tells the shader which mesh's vertices and
                    // which instance it is looking at.
                    encoder.bind_group(0, group, &[draw.constant_offset], layout);
                    let first = draw.range.base_index;
                    encoder.draw_indexed(
                        first..first + draw.range.index_count,
                        // **Zero, not the mesh's base vertex.** Every target
                        // folds this into `SV_VertexID` differently — the
                        // module docs and `mesh.slang`'s header measure all
                        // four — and zero is the only value they agree on. The
                        // real base is in the block bound above.
                        0,
                        // One instance and no base instance, for exactly the
                        // same reason: `SV_InstanceID` is 0 on every target,
                        // and the block says which instance that means.
                        0..1,
                    );
                }
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
        device.destroy_buffer(self.draw_constants);
        self.instances.destroy(device);
        self.pool.destroy(device);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance, QueueKind};

    fn open() -> (Recorder, Box<dyn Device>, QueueHandle) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
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

    /// The cube really is an instance. `begin_frame` puts the model matrix in
    /// the instance pool instead of the uniform block, and in the steady state
    /// it reaches the device as one instance-sized write per frame — the
    /// pyramid, which is in the array and did not move, costs nothing.
    #[test]
    fn only_the_instance_that_moved_is_uploaded() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let instance_buffers = renderer.instances.buffers().to_vec();

        // Both instances are inserted at build and are dirty in *every* slot, so
        // the first frames upload both however well delta upload works. The ring
        // has to be walked right through before an upload is evidence about the
        // delta rather than about initialisation.
        let frame = |renderer: &mut ForwardRenderer, model| {
            renderer
                .begin_frame(
                    device.as_ref(),
                    &Camera::default(),
                    &DirectionalLight::default(),
                    model,
                    (64, 48),
                )
                .expect("write");
        };
        for _ in 0..FRAMES_IN_FLIGHT {
            frame(&mut renderer, Mat4::IDENTITY);
        }

        let before = recorder.events().len();
        let model = ForwardRenderer::spin(1.25);
        frame(&mut renderer, model);
        assert_eq!(
            renderer
                .instances
                .get(renderer.cube_instance)
                .expect("the cube is live")
                .transform,
            model.to_cols_array(),
            "the model matrix must land in the instance, not the uniform block"
        );

        let instance_writes: Vec<(u64, usize)> = recorder
            .events()
            .into_iter()
            .skip(before)
            .filter_map(|event| match event {
                crcbl_hal::null::Event::BufferWritten {
                    buffer,
                    offset,
                    len,
                } if instance_buffers.contains(&buffer) => Some((offset, len)),
                _ => None,
            })
            .collect();
        let cube_at = u64::from(
            renderer
                .instances
                .index(renderer.cube_instance)
                .expect("the cube is live"),
        ) * crcbl_shaders::mesh::INSTANCE_STRIDE as u64;
        assert_eq!(
            instance_writes,
            [(cube_at, crcbl_shaders::mesh::INSTANCE_STRIDE)],
            "a steady-state frame must upload exactly the one instance that changed"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// The property the whole draw-constants mechanism exists for: the pool's
    /// second resident is **not** at base vertex zero, so a frame containing it
    /// is a frame that fails if the base is lost between the pool and the
    /// shader.
    ///
    /// Without this the pass is back where it started — one mesh, base 0, and a
    /// base-vertex bug that no picture can show.
    #[test]
    fn the_second_mesh_lands_past_the_first() {
        let (_, device, queue) = open();
        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        assert_eq!(renderer.cube.range.base_vertex, 0, "the cube is first");
        assert_eq!(
            renderer.pyramid.range.base_vertex as usize,
            crcbl_shaders::mesh::CUBE_VERTEX_COUNT,
            "the pyramid must start where the cube ends, or the pools are not \
             suballocating at all"
        );
        // And the two draws read different constant blocks, or both would be
        // told the same base.
        assert_ne!(
            renderer.cube.constant_offset, renderer.pyramid.constant_offset,
            "each draw needs a block of its own"
        );
        renderer.destroy(device.as_ref());
    }

    /// Every draw the pass records passes **zero** for its base vertex and its
    /// base instance, and says which mesh it is through the dynamic offset
    /// instead.
    ///
    /// This is the assertion that would have caught the bug: the base vertex
    /// used to travel through `draw_indexed`, where the SPIR-V's `OpISub`
    /// subtracted it straight back out. It is checked on the recorded commands
    /// rather than argued from the source, because "the shader adds it back" is
    /// only true while nothing puts it in the draw as well.
    #[test]
    fn every_draw_passes_zero_for_both_of_its_bases() {
        use crcbl_hal::null::Command;

        for pyramid in [None, Some(Mat4::from_translation(Vec3::X))] {
            let (recorder, device, queue) = open();
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("built");
            renderer.set_pyramid(pyramid);
            renderer
                .begin_frame(
                    device.as_ref(),
                    &Camera::default(),
                    &DirectionalLight::default(),
                    Mat4::IDENTITY,
                    (64, 48),
                )
                .expect("write");

            let expected: Vec<(u32, i32)> = if pyramid.is_some() {
                vec![
                    (renderer.cube.constant_offset, 0),
                    (renderer.pyramid.constant_offset, 0),
                ]
            } else {
                vec![(renderer.cube.constant_offset, 0)]
            };

            let imported = swapchain_image(device.as_ref());
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, target, (64, 48));
            let mut pool = crate::TransientPool::new();
            let compiled = graph.compile(&pool).expect("a legal frame");
            let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("forward draws"),
                queue,
            });
            compiled
                .execute(device.as_ref(), &mut pool, encoder.as_mut(), None)
                .expect("the graph executed");
            let commands = encoder.finish().expect("recording succeeded");

            // The dynamic offset last bound before each draw, paired with the
            // base vertex that draw asked for.
            let mut offset = None;
            let mut seen: Vec<(u32, i32)> = Vec::new();
            for command in recorder.commands() {
                match command {
                    Command::BindGroup {
                        slot: 0,
                        dynamic_offsets,
                        ..
                    } => offset = dynamic_offsets.first().copied(),
                    Command::DrawIndexed {
                        base_vertex,
                        instances,
                        ..
                    } => {
                        assert_eq!(instances, 0..1, "and no base instance either");
                        seen.push((
                            offset.expect("a draw is preceded by its constant block"),
                            base_vertex,
                        ));
                    }
                    _ => {}
                }
            }
            assert_eq!(
                seen, expected,
                "with pyramid = {pyramid:?}, the pass must record one draw per visible \
                 mesh, each at base vertex 0 and each pointing at its own block"
            );

            device.destroy_command_buffer(commands);
            renderer.destroy(device.as_ref());
            pool.destroy(device.as_ref());
            device.destroy_image_view(imported.view);
            device.destroy_image(imported.image);
        }
    }

    /// A stand-in for the acquired swapchain image the frame normally ends in.
    fn swapchain_image(device: &dyn Device) -> ImportedImage {
        let format = Format::Rgba8UnormSrgb;
        let image = device
            .create_image(&crcbl_hal::ImageDesc {
                label: Some("fake swapchain image"),
                image_type: crcbl_hal::ImageType::D2,
                extent: crcbl_hal::Extent3d::d2(64, 48),
                format,
                mip_levels: 1,
                samples: 1,
                usage: crcbl_hal::ImageUsage::COLOR_ATTACHMENT | crcbl_hal::ImageUsage::PRESENT,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("an image");
        let view = device
            .create_image_view(&crcbl_hal::ImageViewDesc {
                label: Some("fake swapchain view"),
                image,
                view_type: crcbl_hal::ImageViewType::D2,
                format,
                range: crcbl_hal::ImageSubresourceRange::all(format),
            })
            .expect("a view");
        ForwardRenderer::present_target(image, view, format, (64, 48))
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
