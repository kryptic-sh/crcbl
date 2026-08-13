//! Reversed-Z, proved rather than asserted.
//!
//! `docs/plan/02-vulkan-backend.md` locks reversed-Z, which is the kind of
//! decision a comment can claim and nothing checks. The one test here renders
//! two overlapping quads — the near one drawn first, so draw order cannot carry
//! it — twice, through the same pipeline, the same `CompareOp::Greater` and the
//! same clear of 0.0, changing only the projection matrix. Under the engine's
//! projection the near quad wins and the frame is red; under a conventional
//! `0 at near, 1 at far` one the far quad has the larger depth value, passes
//! `Greater`, and overwrites it. Both outcomes are asserted, so the test fails
//! under standard-Z in the direction that names which convention is in force.
//!
//! It borrows `mesh`'s extent and shader but is not part of that module,
//! because it is not a picture of a scene: the conventional projection it
//! builds for the control is one nothing in the engine ever constructs, and it
//! exists only so the reversed-Z result has something to be different from.

use crate::harness::{Headless, POISON, poisoned};
use crate::mesh::MESH_EXTENT;
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Device, Extent3d,
    Format, ImageAspect, ImageSubresourceLayers, MemoryLocation, PresentInfo, ResourceState,
    SampleType, SubmitInfo,
};

/// Two overlapping quads, the **near one drawn first**, so the depth test is the
/// only thing deciding what is visible.
///
/// This is the fixture that makes reversed-Z a test result. `crcbl-render`'s
/// `camera` module proves the *maths* on the CPU — two surfaces a centimetre
/// apart at 300 m quantise to the same `f32` under a conventional projection and
/// to different ones under the engine's. This proves the *pipeline*: the same
/// geometry, the same shader, the same `CompareOp::Greater`, the same clear of
/// 0.0, and **only the projection matrix differs** between the two runs. One
/// produces a red square, the other a blue one.
///
/// Why the near quad is drawn first: with the far quad first, a broken depth
/// test would still leave the near one on top by draw order, and the test would
/// pass for the wrong reason.
struct DepthProbe {
    vertices: crcbl_hal::BufferHandle,
    indices: crcbl_hal::BufferHandle,
    uniforms: crcbl_hal::BufferHandle,
    /// One `GpuInstance` at identity. `mesh.slang` reads its transform out of
    /// this rather than out of the uniform block, and the probe's geometry is
    /// already in world space — so the instance is a constant and this buffer
    /// is written once, at construction.
    instances: crcbl_hal::BufferHandle,
    /// One `DrawConstants` block naming instance zero. The probe has one
    /// instance, so this is the identity — but `mesh.slang` reads the block
    /// unconditionally, and a set that did not bind it is a pipeline that draws
    /// nothing.
    draw_constants: crcbl_hal::BufferHandle,
    /// A one-entry mesh table, and it is the identity for the same reason: the
    /// probe's geometry starts at vertex 0, so entry 0 is all zeroes and the
    /// instance's mesh id is 0. What it proves is that the *path* is bound —
    /// the vertex stage resolves its base vertex through here on every draw in
    /// the engine, this one included.
    mesh_table: crcbl_hal::BufferHandle,
    materials: crcbl_hal::BufferHandle,
    lights: crcbl_hal::BufferHandle,
    light_grid: crcbl_hal::BufferHandle,
    /// A one-entry run of visible instances, holding the index 0.
    ///
    /// `mesh.slang` reads its instance out of a run rather than naming one,
    /// because the engine's own draws come out of `draw_gen.slang` — see
    /// `crcbl_render::draw_gen`. This probe records an ordinary `draw_indexed`
    /// of one instance, so `SV_InstanceID` is 0, the block's base is 0, and this
    /// entry is what sends it to instance 0.
    visible_instances: crcbl_hal::BufferHandle,
    /// A one-layer `D2Array` page of one white texel, and its view.
    ///
    /// `mesh.slang` samples `base_color_textures` unconditionally, so the probe
    /// has to bind *something*; white is the one thing that leaves the two
    /// quads the colours this test asserts, because the material row names
    /// layer 0 and the shader multiplies by what it finds there.
    base_color_page: crcbl_render::UploadedTexture,
    /// `mesh.slang`'s occlusion channel, one white texel — see the bind group.
    occlusion: crcbl_render::UploadedTexture,
    base_color_sampler: crcbl_hal::SamplerHandle,
    /// A 1×1 `D32Float` image standing in for topic 18's shadow atlas, and its
    /// **comparison** sampler.
    ///
    /// Bound for the same reason the white page above is bound: `mesh.slang`
    /// declares `shadow_atlas` and `shadow_sampler`, and a layout that leaves a
    /// declared binding out is refused at pipeline creation — or, as that page's
    /// comment records, is a `SIGSEGV` on lavapipe rather than a message.
    ///
    /// It is never sampled. The uniform block below sets every cascade's reach
    /// to zero, so every fragment takes `sun_visibility`'s "past the last split"
    /// path and is fully lit — which is what keeps this probe's depth answers
    /// the ones it recorded before shadows existed.
    shadow_atlas: crcbl_hal::ImageHandle,
    shadow_atlas_view: crcbl_hal::ImageViewHandle,
    shadow_sampler: crcbl_hal::SamplerHandle,
    /// What the last frame left [`DepthProbe::shadow_atlas`] in.
    ///
    /// The probe outlives its frames and this image is not a swapchain image, so
    /// nothing else orders one frame's use of it against the next: re-declaring
    /// [`ResourceState::Undefined`] every frame would give the incoming layout
    /// transition `srcStageMask = NONE` and leave it racing the previous frame's
    /// sampled read. `ForwardRenderer` carries the same field for the same
    /// reason — see `crcbl_render::ForwardRenderer`'s `shadow_imported`.
    shadow_imported: ResourceState,
    layout: crcbl_hal::BindGroupLayoutHandle,
    group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::GraphicsPipelineHandle,
}

/// Where the probe's camera sits, on the +Z axis looking at the origin.
const PROBE_EYE: f32 = 2.0;
/// The probe's near plane. The only number that controls depth precision.
const PROBE_NEAR: f32 = 0.1;
/// The probe's far plane — used **only** by the conventional control matrix; the
/// engine's own projection has none.
const PROBE_FAR: f32 = 100.0;
/// What the probe's render pass clears its colour target to.
///
/// Opaque black, so a pixel no fragment reached reads back as `[0, 0, 0, 255]`
/// — the value `vk e2e (lavapipe, windows)` has twice found at the centre. It
/// is named rather than written into the pass because the assertion below has
/// to say what that reading means.
const PROBE_CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

impl DepthProbe {
    /// The two quads, near-first, in `crcbl_shaders::mesh::MeshVertex` layout,
    /// with the box they fit in.
    ///
    /// The bounds are accumulated from the same loop that writes the vertices
    /// rather than restated as a literal beside it: the mesh table's entry is
    /// supposed to describe *this* geometry, and a hand-written pair of corners
    /// would go on describing the old geometry the day a quad moves.
    fn geometry() -> (Vec<u8>, Vec<u8>, [f32; 3], [f32; 3]) {
        // (z, half-extent, colour). The near quad is smaller, so a correct
        // frame is a red square inside a blue ring and a *wrong* one is a plain
        // blue rectangle — two visibly different pictures, not two shades.
        let quads = [
            (0.3f32, 0.25f32, [0.9f32, 0.05, 0.05]),
            (-0.3, 0.6, [0.05, 0.1, 0.9]),
        ];
        let mut vertices = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut bounds_min = [f32::INFINITY; 3];
        let mut bounds_max = [f32::NEG_INFINITY; 3];
        for (quad, (z, half, color)) in quads.iter().enumerate() {
            let base = u32::try_from(quad * 4).expect("two quads");
            for (x, y) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let position = [x * half, y * half, *z];
                for axis in 0..3 {
                    bounds_min[axis] = bounds_min[axis].min(position[axis]);
                    bounds_max[axis] = bounds_max[axis].max(position[axis]);
                }
                for value in [position[0], position[1], position[2], 1.0] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
                // Facing the camera, so both quads are lit identically and the
                // only difference between them is their albedo.
                for value in [0.0f32, 0.0, 1.0, 0.0] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
                for value in [color[0], color[1], color[2], 1.0] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
                // The fourth `float4` of `crcbl_shaders::mesh::MeshVertex`:
                // the base-colour UV, the corner's own position mapped to
                // `0..=1`. It selects nothing here — the probe's page has one
                // white layer — but it has to be *written*, because the stride
                // is 64 bytes and a vertex short of it would slide every later
                // vertex's position into the wrong lane.
                for value in [x * 0.5 + 0.5, y * 0.5 + 0.5, 0.0, 0.0] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let index_bytes = indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        (vertices, index_bytes, bounds_min, bounds_max)
    }

    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let (vertex_bytes, index_bytes, bounds_min, bounds_max) = Self::geometry();

        let upload = |label, usage, bytes: &[u8], state| {
            let size = bytes.len() as u64;
            let staging = device
                .create_buffer(&BufferDesc {
                    label: Some("probe staging"),
                    size,
                    usage: BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("a staging buffer");
            device.write_buffer(staging, 0, bytes).expect("write");
            let target = device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size,
                    usage: usage | BufferUsage::TRANSFER_DST,
                    memory: MemoryLocation::DeviceLocal,
                })
                .expect("a device-local buffer");
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("probe upload"),
                queue: headless.queue,
            });
            encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
                src: staging,
                src_offset: 0,
                dst: target,
                dst_offset: 0,
                size,
            });
            encoder.pipeline_barrier(&Barriers {
                buffers: &[crcbl_hal::BufferBarrier::new(
                    target,
                    ResourceState::TransferDst,
                    state,
                )],
                ..Barriers::default()
            });
            let commands = encoder.finish().expect("recorded");
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);
            device.destroy_buffer(staging);
            target
        };

        let vertices = upload(
            "probe vertices",
            BufferUsage::STORAGE,
            &vertex_bytes,
            ResourceState::ShaderRead,
        );
        let indices = upload(
            "probe indices",
            BufferUsage::INDEX,
            &index_bytes,
            ResourceState::IndexBuffer,
        );
        let uniforms = device
            .create_buffer(&BufferDesc {
                label: Some("probe uniforms"),
                size: crcbl_shaders::mesh::FRAME_UNIFORMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a uniform buffer");
        let instances = device
            .create_buffer(&BufferDesc {
                label: Some("probe instances"),
                size: crcbl_shaders::mesh::INSTANCE_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an instance buffer");
        device
            .write_buffer(
                instances,
                0,
                &crcbl_shaders::mesh::GpuInstance {
                    transform: glam::Mat4::IDENTITY.to_cols_array(),
                    ..crcbl_shaders::mesh::GpuInstance::default()
                }
                .to_bytes(),
            )
            .expect("write");

        let draw_constants = device
            .create_buffer(&BufferDesc {
                label: Some("probe draw constants"),
                size: crcbl_shaders::mesh::DRAW_CONSTANTS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a draw-constants buffer");
        device
            .write_buffer(
                draw_constants,
                0,
                &crcbl_shaders::mesh::DrawConstants::default().to_bytes(),
            )
            .expect("write");

        // One untinted row, so `instance.material == 0` multiplies the albedo
        // by 1.0 and this probe's depth answers stay the answers it recorded
        // before §3.2 existed.
        let materials = device
            .create_buffer(&BufferDesc {
                label: Some("probe materials"),
                size: crcbl_shaders::mesh::MATERIAL_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a material table");
        device
            .write_buffer(
                materials,
                0,
                &crcbl_shaders::mesh::GpuMaterial::UNTINTED.to_bytes(),
            )
            .expect("write");

        let mesh_table = device
            .create_buffer(&BufferDesc {
                label: Some("probe mesh table"),
                size: crcbl_shaders::mesh::MESH_ENTRY_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a mesh table");
        device
            .write_buffer(
                mesh_table,
                0,
                &crcbl_shaders::mesh::GpuMesh {
                    base_vertex: 0,
                    base_index: 0,
                    index_count: u32::try_from(index_bytes.len() / 4).expect("twelve indices"),
                    // Read by nothing on this path — `mesh.slang` does not look
                    // at the bounds and this probe records no cull pass — but
                    // written rather than defaulted, because an entry claiming a
                    // degenerate box at the origin is a lie about a mesh that is
                    // neither.
                    bounds_min,
                    bounds_max,
                }
                .to_bytes(),
            )
            .expect("write");

        let visible_instances = device
            .create_buffer(&BufferDesc {
                label: Some("probe visible instances"),
                size: 4,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a run of visible instances");
        device
            .write_buffer(visible_instances, 0, &0u32.to_le_bytes())
            .expect("write");

        // Topic 18's light list and froxel grid, written by the host rather than
        // by `light_cluster.slang`.
        //
        // This probe runs no compute pass at all — it is a question about depth,
        // and it draws through a pipeline of its own — so the grid is filled here
        // with the one answer the clustering pass would have produced for one
        // directional light: **every froxel holds it**, which is what "affects
        // every cluster" means. Bound read-only, so the device-local rule a
        // shader-written grid is under does not apply.
        let lights = device
            .create_buffer(&BufferDesc {
                label: Some("probe lights"),
                size: crcbl_shaders::light::LIGHT_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a light list");
        device
            .write_buffer(lights, 0, &probe_sun().to_bytes())
            .expect("write");

        let grid = PROBE_GRID;
        let mut grid_words =
            vec![0u32; (grid.froxels() * crcbl_shaders::light::CLUSTER_STRIDE) as usize];
        for froxel in 0..grid.froxels() {
            let base = (froxel * crcbl_shaders::light::CLUSTER_STRIDE) as usize;
            grid_words[base] = 1;
            grid_words[base + 1] = 0;
        }
        let grid_bytes: Vec<u8> = grid_words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        let light_grid = device
            .create_buffer(&BufferDesc {
                label: Some("probe light grid"),
                size: grid_bytes.len() as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a froxel grid");
        device
            .write_buffer(light_grid, 0, &grid_bytes)
            .expect("write");

        // §3.2's texture side, in its smallest honest form: one layer, one
        // texel, opaque white. `Rgba8UnormSrgb` for `crcbl_render::forward`'s
        // reason — the format is the sRGB decode, and `0xFF` decodes to exactly
        // 1.0 — so the sample multiplies the albedo by one and this probe's
        // depth answers stay the answers it recorded before there was a page.
        let base_color_page = crcbl_render::upload_texture_layers(
            device,
            headless.queue,
            "probe base colour",
            crcbl_hal::Format::Rgba8UnormSrgb,
            1,
            1,
            &[&[0xFF, 0xFF, 0xFF, 0xFF]],
        )
        .expect("a one-layer page");
        let base_color_sampler = device
            .create_sampler(&crcbl_hal::SamplerDesc {
                label: Some("probe base colour"),
                mag_filter: crcbl_hal::FilterMode::Nearest,
                min_filter: crcbl_hal::FilterMode::Nearest,
                mip_filter: crcbl_hal::FilterMode::Nearest,
                address_mode: [crcbl_hal::SamplerAddressMode::ClampToEdge; 3],
                ..crcbl_hal::SamplerDesc::default()
            })
            .expect("a sampler");

        let shadow_atlas = device
            .create_image(&crcbl_hal::ImageDesc {
                label: Some("probe shadow atlas"),
                image_type: crcbl_hal::ImageType::D2,
                extent: Extent3d::d2(1, 1),
                format: Format::D32Float,
                mip_levels: 1,
                samples: 1,
                usage: crcbl_hal::ImageUsage::DEPTH_STENCIL_ATTACHMENT
                    .union(crcbl_hal::ImageUsage::SAMPLED),
            })
            .expect("a depth image");
        let shadow_atlas_view = device
            .create_image_view(&crcbl_hal::ImageViewDesc {
                label: Some("probe shadow atlas"),
                image: shadow_atlas,
                view_type: crcbl_hal::ImageViewType::D2,
                format: Format::D32Float,
                range: crcbl_hal::ImageSubresourceRange::all(Format::D32Float),
            })
            .expect("a depth view");
        let shadow_sampler = device
            .create_sampler(&crcbl_hal::SamplerDesc {
                label: Some("probe shadow comparison"),
                // `Some` is what makes it a comparison sampler, and the layout
                // below says the same thing in the vocabulary WebGPU needs.
                compare: Some(crcbl_hal::CompareOp::Greater),
                ..crcbl_hal::SamplerDesc::default()
            })
            .expect("a comparison sampler");

        // `mesh.slang`'s occlusion channel, bound white so the probe's ambient
        // term is unscaled — `crcbl_render::forward`'s placeholder, by hand,
        // because this file builds its own layout out of the same shader.
        let occlusion = crcbl_render::upload_texture(
            device,
            headless.queue,
            "probe ssao placeholder",
            Format::R8Unorm,
            1,
            1,
            &[0xFF],
        )
        .expect("a one-texel white image");

        let entries = [
            crcbl_hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 1,
                visibility: crcbl_hal::ShaderStages::VERTEX,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 2,
                visibility: crcbl_hal::ShaderStages::VERTEX,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 3,
                visibility: crcbl_hal::ShaderStages::VERTEX,
                // Not dynamic, unlike `crcbl_render::ForwardRenderer`'s: the
                // probe records one draw, so there is nothing for an offset to
                // select between.
                kind: crcbl_hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 4,
                visibility: crcbl_hal::ShaderStages::VERTEX,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 5,
                visibility: crcbl_hal::ShaderStages::VERTEX,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            // The material table, §3.2's factors half. This probe shades
            // nothing per material, but a pipeline layout that does not cover
            // a binding the module declares is refused outright, so the entry
            // has to exist even though one untinted row is all it points at.
            crcbl_hal::BindGroupLayoutEntry {
                binding: 6,
                // Both stages: `mesh.slang` reads the table in the fragment
                // stage, and Slang's Metal backend still hands it to the vertex
                // entry point whether it reads it or not. A layout that covers
                // only one is refused at pipeline creation.
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            // The base-colour page and its sampler. **A layout that leaves
            // these out is not a validation message here — it is a
            // `SIGSEGV`**: lavapipe takes an undeclared descriptor at face
            // value and dereferences whatever the set happens to hold, so a
            // missing entry crashes the runner instead of naming itself.
            crcbl_hal::BindGroupLayoutEntry {
                binding: 7,
                // Both stages, for binding 6's reason: Slang's Metal backend
                // materialises every global in every entry point, so the vertex
                // half has to stay even though only `fragmentMain` samples it.
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                // `D2Array`, matching the view above and the shader's
                // `Texture2DArray`. Vulkan reads the dimension off the view and
                // ignores this, but the seam is one declaration for every
                // backend and WebGPU refuses a layout that disagrees.
                kind: crcbl_hal::BindingKind::SampledImage {
                    view_type: crcbl_hal::ImageViewType::D2Array,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 8,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::Sampler { comparison: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 15,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::SampledImage {
                    view_type: crcbl_hal::ImageViewType::D2,
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 16,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::Sampler { comparison: true },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 20,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 21,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 22,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::SampledImage {
                    view_type: crcbl_hal::ImageViewType::D2,
                    sample_type: crcbl_hal::SampleType::Float,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
        ];
        let layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("probe"),
                entries: &entries,
            })
            .expect("a layout");
        let group_entries = [
            crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(uniforms),
            },
            crcbl_hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(vertices),
            },
            crcbl_hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(instances),
            },
            crcbl_hal::BindGroupEntry {
                binding: 3,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(draw_constants),
            },
            crcbl_hal::BindGroupEntry {
                binding: 4,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(mesh_table),
            },
            crcbl_hal::BindGroupEntry {
                binding: 5,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(visible_instances),
            },
            crcbl_hal::BindGroupEntry {
                binding: 6,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(materials),
            },
            crcbl_hal::BindGroupEntry {
                binding: 7,
                array_index: 0,
                resource: crcbl_hal::BindingResource::ImageView(base_color_page.view),
            },
            crcbl_hal::BindGroupEntry {
                binding: 8,
                array_index: 0,
                resource: crcbl_hal::BindingResource::Sampler(base_color_sampler),
            },
            crcbl_hal::BindGroupEntry {
                binding: 15,
                array_index: 0,
                resource: crcbl_hal::BindingResource::ImageView(shadow_atlas_view),
            },
            crcbl_hal::BindGroupEntry {
                binding: 16,
                array_index: 0,
                resource: crcbl_hal::BindingResource::Sampler(shadow_sampler),
            },
            crcbl_hal::BindGroupEntry {
                binding: 20,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(lights),
            },
            crcbl_hal::BindGroupEntry {
                binding: 21,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(light_grid),
            },
            crcbl_hal::BindGroupEntry {
                binding: 22,
                array_index: 0,
                resource: crcbl_hal::BindingResource::ImageView(occlusion.view),
            },
        ];
        let group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("probe"),
                layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");
        let set_layouts = [layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("probe"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl_hal::ShaderModuleDesc {
                label: Some("mesh.slang"),
                spirv: crcbl_shaders::MESH.spirv(),
                wgsl: crcbl_shaders::MESH.wgsl(),
                msl: crcbl_shaders::MESH.msl(),
                dxil: &[],
            })
            .expect("the committed SPIR-V is accepted");
        let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];
        let pipeline = device.create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
            label: Some("depth probe"),
            layout: pipeline_layout,
            vertex: crcbl_hal::ShaderEntry {
                module,
                entry_point: "vertexMain",
            },
            fragment: Some(crcbl_hal::ShaderEntry {
                module,
                entry_point: "fragmentMain",
            }),
            primitive: crcbl_hal::PrimitiveState {
                // No culling: the point is the depth test, and a winding
                // mistake would otherwise delete a quad and look like one.
                cull_mode: crcbl_hal::CullMode::None,
                ..crcbl_hal::PrimitiveState::default()
            },
            // The seam's default, unchanged: `Greater` on `D32Float` with
            // writes on. **This is what the two projections are tested
            // against, and it is not adjusted between runs.**
            depth_stencil: Some(crcbl_hal::DepthStencilState::default()),
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        });
        device.destroy_shader_module(module);

        Self {
            lights,
            light_grid,
            vertices,
            indices,
            uniforms,
            instances,
            draw_constants,
            mesh_table,
            materials,
            visible_instances,
            base_color_page,
            occlusion,
            base_color_sampler,
            shadow_atlas,
            shadow_atlas_view,
            shadow_sampler,
            // Nothing has touched it yet, so the first frame's graph is what
            // gives it a layout.
            shadow_imported: ResourceState::Undefined,
            layout,
            group,
            pipeline_layout,
            pipeline: pipeline.expect("a graphics pipeline"),
        }
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.group);
        device.destroy_bind_group_layout(self.layout);
        device.destroy_sampler(self.shadow_sampler);
        device.destroy_image_view(self.shadow_atlas_view);
        device.destroy_image(self.shadow_atlas);
        device.destroy_sampler(self.base_color_sampler);
        self.base_color_page.destroy(device);
        self.occlusion.destroy(device);
        device.destroy_buffer(self.visible_instances);
        device.destroy_buffer(self.mesh_table);
        device.destroy_buffer(self.light_grid);
        device.destroy_buffer(self.lights);
        device.destroy_buffer(self.materials);
        device.destroy_buffer(self.draw_constants);
        device.destroy_buffer(self.instances);
        device.destroy_buffer(self.uniforms);
        device.destroy_buffer(self.indices);
        device.destroy_buffer(self.vertices);
    }
}

/// The froxel grid this probe's own frame runs with.
///
/// One slice, because a slice index would need a view depth and this probe is a
/// question about depth rather than a user of it — and one light in every froxel
/// makes the slice count irrelevant to the picture either way. The tiles are the
/// ordinary ones for this extent, so `mesh.slang`'s `froxel_of` divides its pixel
/// down exactly as it does in a real frame.
const PROBE_GRID: crcbl_render::Grid = crcbl_render::Grid {
    x: 4,
    y: 3,
    slices: 1,
    tile_pixels: 64,
};

const _: () = assert!(
    PROBE_GRID.x * PROBE_GRID.tile_pixels >= MESH_EXTENT.0
        && PROBE_GRID.y * PROBE_GRID.tile_pixels >= MESH_EXTENT.1,
    "the grid must cover the probe's frame, or a fragment past it reads a froxel \
     built for somewhere else"
);

/// The one light in this probe's list: straight at the quads, so both are lit
/// identically and the only difference between them is their albedo.
///
/// The same direction and colour this probe carried in the frame block before
/// `docs/plan/18-render-features.md`'s light list existed, so its depth answers
/// are the answers it recorded then.
fn probe_sun() -> crcbl_shaders::light::GpuLight {
    crcbl_shaders::light::GpuLight {
        position: [0.0; 4],
        color: [0.8, 0.8, 0.8, 0.0],
        direction: [0.0, 0.0, 1.0, 0.0],
        kind: crcbl_shaders::light::KIND_DIRECTIONAL,
        cos_inner: 0.0,
        shadow_tile: crcbl_shaders::light::NO_SHADOW_TILE,
        pad1: 0,
    }
}

/// Renders the probe with `view_proj` and reads the frame back.
fn render_probe(
    headless: &Headless,
    probe: &mut DepthProbe,
    pool: &mut crcbl_render::TransientPool,
    view_proj: glam::Mat4,
) -> crcbl_golden::Image {
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;

    let uniforms = crcbl_shaders::mesh::FrameUniforms {
        view_proj: view_proj.to_cols_array(),
        camera_position: [0.0, 0.0, PROBE_EYE, 1.0],
        ambient: [0.2, 0.2, 0.2, 0.0],
        // This probe binds no shadow atlas and draws through a pipeline of its
        // own, so the cascades are never sampled. Identity matrices and a zero
        // reach say that plainly: a fragment whose eye distance is past every
        // split takes the shader's "outside the cascade" path, which is fully
        // lit — the same picture the probe asserted before shadows existed.
        shadow_view_proj: [glam::Mat4::IDENTITY.to_cols_array();
            crcbl_shaders::mesh::SHADOW_CASCADES],
        cascade_far: [0.0; 4],
        shadow_params: [0.0; 4],
        // The grid the host filled above, so the fragment stage looks itself up
        // in the froxel that really holds the one light.
        cluster_grid: PROBE_GRID.to_frame_block(),
        // No shadowed light either, and the row above says so: `probe_sun`
        // carries `NO_SHADOW_TILE`, so nothing in this frame reads these.
        light_view_proj: [glam::Mat4::IDENTITY.to_cols_array();
            crcbl_shaders::mesh::SHADOW_LIGHT_TILES],
    };
    device
        .write_buffer(probe.uniforms, 0, &uniforms.to_bytes())
        .expect("write");

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    let bytes = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("probe readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("probe frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = crcbl_render::RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                final_state: ResourceState::TransferSrc,
            },
        );
        let depth = graph.create_image(
            "probe-depth",
            crcbl_render::TransientImageDesc::scene_depth(MESH_EXTENT),
        );
        // Declared so the graph gives the placeholder atlas a shader-read
        // layout before the set is bound. Nothing samples it — see the field —
        // but a descriptor pointing at an image in no layout at all is a
        // validation error whether or not the shader reads it.
        //
        // **`initial` is what the last frame left it in, not `Undefined`.** The
        // probe owns this image across frames and no semaphore orders one frame's
        // draw against the next frame's transition, so a second `Undefined` is a
        // barrier with no source scope sitting after a fragment shader read of
        // the same image: `SYNC-HAZARD-WRITE-AFTER-READ`. See the field.
        let shadow = graph.import_image(
            "probe-shadow-atlas",
            crcbl_render::ImportedImage {
                image: probe.shadow_atlas,
                view: probe.shadow_atlas_view,
                format: Format::D32Float,
                extent: (1, 1),
                initial: std::mem::replace(&mut probe.shadow_imported, ResourceState::ShaderRead),
                final_state: ResourceState::ShaderRead,
            },
        );
        graph
            .add_render_pass("probe")
            .clear_color(target, PROBE_CLEAR)
            // The reversed-Z clear: `depth::CLEAR` = 0.0, so any geometry beats
            // the empty buffer under `Greater`.
            .clear_depth(depth)
            .read_image(shadow)
            .execute(|ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(probe.pipeline);
                encoder.bind_group(0, probe.group, &[], probe.pipeline_layout);
                encoder.bind_index_buffer(probe.indices, 0, crcbl_hal::IndexFormat::Uint32);
                encoder.draw_indexed(0..12, 0, 0..1);
            });
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("executed");

    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });
    let commands = encoder.finish().expect("recorded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");

    let mut pixels = poisoned(bytes as usize);
    headless.readback(staging, bytes, &mut pixels);
    // The poison, cashed in. Left to the pixel assertions below it is only
    // advisory — every channel of `POISON` is above the threshold the first of
    // them uses, so an uncopied frame sails past it and fails the *second* one
    // with a message about the projection matrix. Asked here it is a diagnosis:
    // this fires when and only when no copy reached the destination.
    assert!(
        pixels.iter().any(|&byte| byte != POISON),
        "all {bytes} bytes of the readback are still {POISON:#04x} — this is the \
         harness's own fill, so no copy reached the destination and there is no \
         frame here to read. Look at the submission and the readback, not at \
         what was drawn."
    );
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &pixels, order).expect("one image")
}

/// **Reversed-Z, on the GPU, discriminated against the alternative.**
///
/// `docs/plan/02-vulkan-backend.md` locks reversed-Z, and it is the kind of
/// decision a comment can claim and nothing checks. This renders the *same*
/// geometry through the *same* pipeline with the *same* `Greater` compare op and
/// the *same* clear of 0.0, twice, changing one thing: the projection matrix.
///
/// * With the engine's reversed-Z projection, the near quad wins → **red**.
/// * With a conventional `0 at near, 1 at far` projection, the far quad has the
///   larger depth value, passes `Greater`, and overwrites it → **blue**.
///
/// So this test would fail under standard-Z, in the direction that says which
/// convention is in force — which is the point.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn reversed_z_puts_the_nearer_surface_in_front_and_standard_z_would_not() {
    let headless = Headless::open_for_mesh();
    let mut probe = DepthProbe::new(&headless);
    let mut pool = crcbl_render::TransientPool::new();

    #[allow(clippy::cast_precision_loss)]
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    let view = glam::camera::rh::view::look_at_mat4(
        glam::Vec3::new(0.0, 0.0, PROBE_EYE),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
    );
    let fov = core::f32::consts::FRAC_PI_4;

    // The engine's own projection, straight out of `crcbl-render`.
    let reversed = crcbl_render::Projection::Perspective {
        fov_y: fov,
        near: PROBE_NEAR,
    }
    .matrix(aspect)
        * view;
    // The control: conventional depth, 0 at the near plane and 1 at the far one.
    // `crcbl-render` deliberately has no constructor for this, which is why the
    // suite reaches for glam directly.
    let standard =
        glam::camera::rh::proj::directx::perspective(fov, aspect, PROBE_NEAR, PROBE_FAR) * view;

    let centre = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2);

    let reversed_frame = render_probe(&headless, &mut probe, &mut pool, reversed);
    let pixel = reversed_frame.pixel(centre.0, centre.1).expect("inside");
    // **Neither quad drawing is its own diagnosis, and it must not read as the
    // projection one.** The two want looking at in completely different places.
    // This message said only "if it is blue…" once, and a black centre on a
    // slow software rasteriser sent a reader hunting through the projection
    // matrix for an hour; it then said "nothing drew", which is a narrower
    // claim than the pixel supports. What the pixel supports is below.
    assert!(
        pixel[0] > 100 || pixel[2] > 100,
        "neither quad reached the centre: got {pixel:?}, which is neither the \
         near quad's red nor the far quad's blue. Do not look at the \
         projection; read what the centre holds instead.\n\
         \x20 * If it is this pass's own clear ({PROBE_CLEAR:?} encoded to \
         unorm bytes), the copy landed and carried a cleared pixel — so either \
         the draw produced no fragment there or the pass never executed, and \
         this pixel alone does not separate the two.\n\
         \x20 * If any channel is {poison:#04x} it is the readback \
         destination's own fill, untouched: no copy reached it and the bytes \
         are this harness's, not a frame's.",
        poison = POISON,
    );
    assert!(
        pixel[0] > pixel[2] && pixel[0] > 100,
        "under reversed-Z the *near* quad must win the depth test, so the centre \
         must be red; got {pixel:?}. It is the far quad's blue, so the projection \
         matrix is not reversed and every depth comparison in the engine is \
         inverted."
    );

    // And the far quad really is there, around the edge of the near one — so
    // "red at the centre" is a depth test rather than the blue quad having
    // failed to draw at all.
    // Between the two quads' silhouettes. At this camera the near quad reaches
    // 34 pixels from the centre and the far one 60, so 48 is comfortably inside
    // the blue ring and comfortably outside the red square — worth deriving
    // rather than guessing, because a sample point that lands on neither reads
    // as a depth-test failure.
    let ring = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2 + 48);
    let pixel = reversed_frame.pixel(ring.0, ring.1).expect("inside");
    assert!(
        pixel[2] > pixel[0] && pixel[2] > 100,
        "the far quad is larger, so it must be visible around the near one; got \
         {pixel:?} at {ring:?}"
    );

    let standard_frame = render_probe(&headless, &mut probe, &mut pool, standard);
    let pixel = standard_frame.pixel(centre.0, centre.1).expect("inside");
    assert!(
        pixel[2] > pixel[0] && pixel[2] > 100,
        "the control is only meaningful if a conventional projection really does \
         invert the outcome under the engine's `Greater` test; it gave {pixel:?}, \
         which is not blue. Re-derive the quad depths rather than relaxing this."
    );

    eprintln!(
        "vk e2e: reversed-Z centre {:?}, conventional-Z centre {:?} — the same \
         pipeline, the same compare op, only the projection differs",
        reversed_frame.pixel(centre.0, centre.1).expect("inside"),
        standard_frame.pixel(centre.0, centre.1).expect("inside"),
    );

    probe.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
