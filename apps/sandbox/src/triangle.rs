//! Milestone 2: a triangle, pulled from a storage buffer.
//!
//! `docs/plan/02-vulkan-backend.md`'s ladder, rung 2: *"Triangle (vertex pulling
//! from a storage buffer — no vertex input state; this is the pattern stage 3
//! scales up)."*
//!
//! # There is no vertex buffer here, and that is the point
//!
//! [`GraphicsPipelineDesc`] has no vertex-input state and
//! [`CommandEncoder`](crcbl::hal::CommandEncoder) has no `bind_vertex_buffer`,
//! so the three vertices below live in a **storage buffer** that the vertex
//! shader indexes with `SV_VertexID`. The draw is
//! `encoder.draw(0..3, 0..1)` — three vertices, no geometry bound to the
//! pipeline at all.
//!
//! Stage 3 replaces this one buffer with the global vertex pool and this index
//! with `base_vertex + i` (`docs/plan/03-gpu-driven-rendering.md` §3.1). Nothing
//! about the pipeline or the recording changes, which is exactly why the plan
//! establishes the pattern at the first triangle rather than at the first mesh.
//!
//! # The upload goes through a staging buffer
//!
//! It would be shorter to make the storage buffer
//! [`MemoryLocation::HostUpload`] and write it directly, and it would work. It
//! would also be a different code path from the one everything after this uses:
//! §3.1's upload path is "staging ring buffer + transfer submits with timeline
//! semaphore tracking", and the device-local buffer is where geometry actually
//! belongs. So this does the real thing once, at startup — write a staging
//! buffer, copy it into a device-local one, barrier it into `ShaderRead` — and
//! throws the staging buffer away.
//!
//! # The vertices are not here
//!
//! They live in [`crcbl::shaders::triangle`], beside the `.slang` file whose
//! `struct Vertex` fixes their byte layout. `crcbl-vk`'s golden-image test
//! renders the same constant, which is what makes a blessed frame evidence
//! about *this* triangle rather than about a second one that happens to look
//! similar.

use crcbl::hal::{
    Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource,
    BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferUsage, ColorTargetState,
    CommandEncoderDesc, Device, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError,
    MemoryLocation, MultisampleState, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState,
    QueueHandle, ResourceState, ShaderEntry, ShaderModuleDesc, ShaderStages, SubmitInfo,
};
use crcbl::shaders::{Stage, TRIANGLE, triangle};

/// Everything the triangle needs, created once and destroyed once.
#[derive(Debug)]
pub struct Triangle {
    vertices: BufferHandle,
    bind_group_layout: BindGroupLayoutHandle,
    bind_group: BindGroupHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    vertex_count: u32,
}

impl Triangle {
    /// Builds the pipeline and uploads the geometry.
    ///
    /// `format` must be the colour target the pass will actually render to:
    /// dynamic rendering checks pipeline and attachment formats against each
    /// other at `vkCmdBeginRendering` time, not at creation.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any of the seam calls. A backend that cannot create a
    /// pipeline says so here rather than drawing nothing later.
    pub fn new(device: &dyn Device, queue: QueueHandle, format: Format) -> Result<Self, HalError> {
        // The geometry lives in `crcbl-shaders`, next to the `.slang` file that
        // declares its `std430` layout — see that module for why. The golden
        // image test renders the same constant, so a golden frame really is
        // evidence about what this draws.
        let vertices = upload(device, queue, &triangle::vertex_bytes())?;

        // One binding: the vertex stream. No `BindingFlags` at all, which is
        // what makes this layout legal on **both** tiers — vertex pulling is not
        // a Tier A feature, it is the engine's only geometry path, so the shape
        // that carries it has to work everywhere. A Tier A layout would look
        // identical with `VARIABLE_COUNT | PARTIALLY_BOUND | UPDATE_AFTER_BIND`
        // and `count: u32::MAX` on a trailing texture array; that is topic 03's,
        // at P3.
        let layout_entries = [BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            kind: BindingKind::StorageBuffer {
                // The shader declares `StructuredBuffer`, not
                // `RWStructuredBuffer`, so this is the truth and not a hint —
                // and it is what lets the render graph merge read-after-read.
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        }];
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("triangle vertices"),
            entries: &layout_entries,
        })?;

        let group_entries = [BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: BindingResource::whole_buffer(vertices),
        }];
        let bind_group = device.create_bind_group(&BindGroupDesc {
            label: Some("triangle vertices"),
            layout: bind_group_layout,
            entries: &group_entries,
        })?;

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("triangle"),
            bind_group_layouts: &set_layouts,
            // Nothing per-draw yet. P1.3's camera goes in a uniform buffer
            // rather than here, because `Features::PUSH_CONSTANTS` is absent on
            // Tier B and the seam is explicit that the substitute is a
            // data-layout decision, not a per-draw one.
            push_constants: None,
        })?;

        // One module, two entry points — which is why `ShaderEntry` names one
        // per stage instead of assuming `main`.
        let module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("triangle.slang"),
            spirv: TRIANGLE.spirv(),
        })?;
        let vertex_entry = TRIANGLE
            .entry_point(Stage::Vertex)
            .ok_or_else(|| shader_gap("vertex"))?;
        let fragment_entry = TRIANGLE
            .entry_point(Stage::Fragment)
            .ok_or_else(|| shader_gap("fragment"))?;

        let color_targets = [ColorTargetState::opaque(format)];
        let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("triangle"),
            layout: pipeline_layout,
            vertex: ShaderEntry {
                module,
                entry_point: vertex_entry,
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: fragment_entry,
            }),
            primitive: PrimitiveState::default(),
            // No depth attachment until milestone 3. The seam's default is
            // reversed-Z, and a pipeline that declared one here would have to
            // be given a depth attachment at `begin_render_pass` — which is the
            // next rung, not this one.
            depth_stencil: None,
            multisample: MultisampleState::default(),
            color_targets: &color_targets,
        });
        // Destroyed either way: the seam promises "pipelines built from it stay
        // valid", so keeping the module alive for the process would be a leak
        // with no reader. Doing it before the `?` means the failure path does
        // not leak it either.
        device.destroy_shader_module(module);
        let pipeline = pipeline?;

        Ok(Self {
            vertices,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
            #[allow(clippy::cast_possible_truncation)]
            vertex_count: triangle::VERTICES.len() as u32,
        })
    }

    /// Records the draw. Must be called inside a render pass.
    ///
    /// The order is load-bearing on Vulkan: `crcbl-vk` takes the pipeline
    /// layout that `bind_group` needs from the last pipeline bound, so the
    /// pipeline goes first. See `crcbl-vk`'s `command` module.
    pub fn draw(&self, encoder: &mut dyn crcbl::hal::CommandEncoder) {
        encoder.bind_graphics_pipeline(self.pipeline);
        encoder.bind_group(0, self.bind_group, &[]);
        // Three vertices and no geometry bound. The whole milestone, in one
        // line.
        encoder.draw(0..self.vertex_count, 0..1);
    }

    /// Releases everything, in dependency order.
    pub fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.vertices);
    }
}

/// Uploads `bytes` into a fresh device-local storage buffer.
///
/// Staging buffer → copy → barrier into [`ResourceState::ShaderRead`], then
/// wait and drop the staging buffer. Blocking is correct *here*: this runs once
/// at startup, before any frame, and the alternative — keeping the staging
/// buffer alive on a timeline — is the streaming path P1.3's graph owns.
fn upload(device: &dyn Device, queue: QueueHandle, bytes: &[u8]) -> Result<BufferHandle, HalError> {
    let size = bytes.len() as u64;
    let staging = device.create_buffer(&BufferDesc {
        label: Some("triangle staging"),
        size,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    })?;
    device.write_buffer(staging, 0, bytes)?;

    let vertices = device.create_buffer(&BufferDesc {
        label: Some("triangle vertices"),
        size,
        usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::DeviceLocal,
    })?;

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("triangle upload"),
        queue,
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: staging,
        src_offset: 0,
        dst: vertices,
        dst_offset: 0,
        size,
    });
    // Without this the vertex stage may read the buffer before the copy has
    // landed. Nothing about a single startup submit makes that impossible — it
    // is a hazard *within* one command buffer — and it is exactly the class of
    // bug the sync-validation layer exists to catch.
    encoder.pipeline_barrier(&Barriers {
        buffers: &[BufferBarrier {
            buffer: vertices,
            from: ResourceState::TransferDst,
            to: ResourceState::ShaderRead,
            queue_transfer: None,
        }],
        ..Barriers::default()
    });
    let commands = encoder.finish()?;

    device.submit(queue, &SubmitInfo::new(&[commands]))?;
    // The one blocking wait in the whole app, and the seam's docs sanction it:
    // `wait_idle` is "a shutdown and test primitive". Startup is neither, but it
    // is also not a frame, and the staging buffer cannot be freed until the copy
    // has run.
    device.wait_idle()?;
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    Ok(vertices)
}

/// The error for a shader that does not expose the entry point it must.
///
/// Unreachable in practice — `crcbl-shaders`' own tests assert the triangle has
/// both — but the alternative is an `expect` in application code, and a game
/// scaffolded from this should not learn that habit.
fn shader_gap(stage: &str) -> HalError {
    HalError::ShaderCompilation(format!(
        "triangle.slang exposes no {stage} entry point; the committed SPIR-V and its manifest \
         disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix"
    ))
}
