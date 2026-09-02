//! `mesh.slang` driven through a pipeline this file builds by hand, so a
//! question about the *pipeline* can be asked without a scene around it.
//!
//! Three questions are asked here, and they share one fixture because they share
//! one pipeline.
//!
//! **Reversed-Z, proved rather than asserted.** `docs/plan/02-vulkan-backend.md`
//! locks it, which is the kind of decision a comment can claim and nothing
//! checks. Two overlapping quads — the near one drawn first, so draw order
//! cannot carry it — are rendered twice, through the same pipeline, the same
//! `CompareOp::Greater` and the same clear of 0.0, changing only the projection
//! matrix. Under the engine's projection the near quad wins and the frame is
//! red; under a conventional `0 at near, 1 at far` one the far quad has the
//! larger depth value, passes `Greater`, and overwrites it. Both outcomes are
//! asserted, so the test fails under standard-Z in the direction that names
//! which convention is in force.
//!
//! **The reflectivity attachment carries the row it was told to.**
//! `docs/plan/18-render-features.md`'s screen-space reflections read `F0` and a
//! roughness out of a second colour target the forward pass writes, and nothing
//! in a rendered picture shows what is in it — a wrong channel, a wrong row or a
//! target that was never written all produce exactly the frame the goldens
//! already hold. So this file reads that attachment back and says what it found.
//!
//! **A 1×1 occlusion image reads as its one texel, everywhere.** That is what
//! `crcbl::render::forward`'s ambient-occlusion off-switch is, and an unclamped
//! `Load` at `SV_Position.xy` does not do it — the fetch lands outside a
//! one-texel image at every pixel but the origin and yields zero, which reads as
//! total occlusion. The probe already binds exactly that placeholder, so the
//! question is asked here by darkening the light list until ambient is the whole
//! of the frame's colour.
//!
//! It borrows the fixture's extent and the engine's shader but draws no scene:
//! none of the three is a picture of one. The conventional projection
//! built for the control is one nothing in the engine ever constructs, the
//! reflectivity frame is deliberately shaded through a row that makes the colour
//! target nearly black, and the occlusion frame has no direct light in it at all.

use crate::harness::{Headless, POISON, poisoned};
use crate::mesh_scene::MESH_EXTENT;
use crcbl::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Device, Extent3d,
    Format, ImageAspect, ImageSubresourceLayers, MemoryLocation, PresentInfo, ResourceState,
    SampleType, SubmitInfo,
};
use crcbl::shaders::dfg;
use crcbl::shaders::ltc;
use crcbl::shaders::mesh;

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
    vertices: crcbl::hal::BufferHandle,
    indices: crcbl::hal::BufferHandle,
    uniforms: crcbl::hal::BufferHandle,
    /// One `GpuInstance` at identity. `mesh.slang` reads its transform out of
    /// this rather than out of the uniform block, and the probe's geometry is
    /// already in world space — so the transform is a constant.
    ///
    /// Written per frame rather than at construction, because its
    /// [`material`](crcbl::shaders::mesh::GpuInstance::material) is the one field
    /// that varies: it is how a frame says which row of [`PROBE_MATERIALS`] its
    /// fragments shade through, and the reflectivity assertion exists to check
    /// that the row named here is the row that arrives.
    instances: crcbl::hal::BufferHandle,
    /// One `DrawConstants` block naming instance zero. The probe has one
    /// instance, so this is the identity — but `mesh.slang` reads the block
    /// unconditionally, and a set that did not bind it is a pipeline that draws
    /// nothing.
    draw_constants: crcbl::hal::BufferHandle,
    /// A one-entry mesh table, and it is the identity for the same reason: the
    /// probe's geometry starts at vertex 0, so entry 0 is all zeroes and the
    /// instance's mesh id is 0. What it proves is that the *path* is bound —
    /// the vertex stage resolves its base vertex through here on every draw in
    /// the engine, this one included.
    mesh_table: crcbl::hal::BufferHandle,
    materials: crcbl::hal::BufferHandle,
    lights: crcbl::hal::BufferHandle,
    light_grid: crcbl::hal::BufferHandle,
    /// A one-row irradiance probe table, left **zeroed**.
    ///
    /// `mesh.slang` reads it unconditionally and clamps every fetch into it, so
    /// a probe binding nothing would be a pipeline that fails to create. Zeroed
    /// because the frame block's volume is the default one: the grid evaluates
    /// to exactly zero and this probe's ambient answers stay the answers it
    /// recorded before there was a grid at all.
    probes: crcbl::hal::BufferHandle,
    /// A one-entry run of visible instances, holding the index 0.
    ///
    /// `mesh.slang` reads its instance out of a run rather than naming one,
    /// because the engine's own draws come out of `draw_gen.slang` — see
    /// `crcbl::render::draw_gen`. This probe records an ordinary `draw_indexed`
    /// of one instance, so `SV_InstanceID` is 0, the block's base is 0, and this
    /// entry is what sends it to instance 0.
    visible_instances: crcbl::hal::BufferHandle,
    /// A one-layer `D2Array` page of one white texel, and its view.
    ///
    /// `mesh.slang` samples `base_color_textures` unconditionally, so the probe
    /// has to bind *something*; white is the one thing that leaves the two
    /// quads the colours this test asserts, because the material row names
    /// layer 0 and the shader multiplies by what it finds there.
    base_color_page: crcbl::render::UploadedTexture,
    /// `mesh.slang`'s occlusion channel, one white texel — `crcbl::render::forward`'s
    /// own placeholder, and the subject of this file's third question.
    ///
    /// **Deliberately smaller than the frame**, which is the whole point: the
    /// shader has to clamp its fetch to reach this texel at all, and every frame
    /// this file renders is drawn through it.
    occlusion: crcbl::render::UploadedTexture,
    /// `mesh.slang`'s split-sum `DFG` table, the cooked one the engine binds.
    ///
    /// Bound because the shader declares it and a layout that leaves a declared
    /// binding out is refused at pipeline creation; the *real* table rather than
    /// a stand-in because the probe's frames are compared against what the
    /// renderer draws, and a different table is a different lobe.
    specular_dfg: crcbl::render::UploadedTexture,
    /// `mesh.slang`'s linearly transformed cosine table, on the table above it's
    /// terms exactly — the real cooked one, bound because the module declares
    /// it. No frame this probe draws holds an area light, so nothing reads it.
    ltc_table: crcbl::render::UploadedTexture,
    /// `mesh.slang`'s per-probe visibility maps, as the renderer's own
    /// off-switch binds them: one layer of one texel holding
    /// `crcbl::shaders::probe_visibility::FAR`, so every probe reads as
    /// unobstructed and the irradiance this probe measures is the unweighted
    /// trilinear blend.
    probe_visibility: crcbl::render::UploadedTexture,
    base_color_sampler: crcbl::hal::SamplerHandle,
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
    shadow_atlas: crcbl::hal::ImageHandle,
    shadow_atlas_view: crcbl::hal::ImageViewHandle,
    shadow_sampler: crcbl::hal::SamplerHandle,
    layout: crcbl::hal::BindGroupLayoutHandle,
    group: crcbl::hal::BindGroupHandle,
    pipeline_layout: crcbl::hal::PipelineLayoutHandle,
    pipeline: crcbl::hal::GraphicsPipelineHandle,
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

/// The near quad's vertex colour: the red the reversed-Z assertion looks for at
/// the centre, and **the albedo the reflectivity assertion derives its `F0`
/// from** — a conductor's `F0` is its base colour, so the value expected at the
/// centre is this multiplied by the material row's factor rather than a triple
/// written down beside it.
const NEAR_QUAD_COLOR: [f32; 3] = [0.9, 0.05, 0.05];

/// The far quad's vertex colour: the blue that must still be visible around the
/// near quad's edge.
const FAR_QUAD_COLOR: [f32; 3] = [0.05, 0.1, 0.9];

/// The ambient term every frame this file renders carries.
///
/// Named rather than written into the uniforms, because the occlusion assertion
/// multiplies by it: with the light list darkened, `mesh.slang`'s whole output is
/// `diffuse_albedo * ambient * occluded`, and that assertion has to be derived
/// from the value the frame really carried rather than from a literal beside it.
const PROBE_AMBIENT: [f32; 4] = [0.2, 0.2, 0.2, 0.0];

/// The probe's material table. Row [`PROBE_PLAIN_ROW`] is what the reversed-Z
/// frames shade through and row [`PROBE_REFLECTIVE_ROW`] is what the
/// reflectivity frame does.
///
/// **Two rows, because one row cannot fail a wrong-row bug.** The reflectivity
/// assertion reads the attachment a fragment wrote after being told to use the
/// second row; a shader that resolved the *first* one — or a host that never
/// wrote the instance's index — would produce a perfectly plausible triple, and
/// the only thing that separates the two answers is that the rows differ. They
/// differ in both fields and by a wide margin, which is what lets the assertion
/// be a tolerance around one value rather than a preference between two close
/// ones.
const PROBE_MATERIALS: [crcbl::shaders::mesh::GpuMaterial; 2] = [
    // The row this probe has always shaded through. Its `F0` is the flat
    // dielectric 0.04 and its roughness is 0.5, so it is far from the row below
    // in every channel of the attachment.
    crcbl::shaders::mesh::GpuMaterial::UNTINTED,
    crcbl::shaders::mesh::GpuMaterial {
        // **Not `[1.0; 4]`, and the blue factor is the reason.** `F0` here is
        // the quad's own albedo, and the near quad's green and blue are equal —
        // two equal channels are two a swizzle could swap unseen. Scaling one of
        // them makes all three of the expected triple distinct.
        base_color: [1.0, 1.0, 0.4, 1.0],
        base_color_texture: 0,
        // **A conductor, so `F0` is coloured.** A dielectric's `F0` is grey
        // whatever its albedo, and a grey triple cannot fail a channel swap.
        metallic: 1.0,
        // Far from the row above's 0.5, and far from every channel of the `F0`
        // beside it — so a wrong row fails on the alpha and a swap of the `rgb`
        // and `a` halves fails on all four.
        roughness: 0.25,
        // Authored UV, like the untextured row above: this probe scene samples
        // no page, so physical tiling has nothing to tile.
        tiling: crcbl::shaders::mesh::GpuMaterial::TILING_AUTHORED,
        tile_metres: crcbl::shaders::mesh::GpuMaterial::UNTINTED.tile_metres,
        // **Emitting nothing, and that matters here.** Emission is added to the
        // lit colour and is not part of `F0`, so a row that emitted would leave
        // the reflectivity attachment alone and change only the frame this probe
        // does not read — which would make a difference this test cannot see. It
        // is the row above's value, so the two rows still differ only where the
        // assertion looks.
        emissive: [0.0; 3],
        // **No page on any of the material rows either**, for the reason the
        // tiling comment gives: this probe's scene carries the one white layer
        // and the one neutral normal layer `PageDesc::opaque_white` describes,
        // so a row naming any other layer would name one that does not exist.
        normal_texture: crcbl::shaders::mesh::GpuMaterial::NO_PAGE,
        normal_scale: crcbl::shaders::mesh::GpuMaterial::UNTINTED.normal_scale,
        metallic_roughness_occlusion_texture: crcbl::shaders::mesh::GpuMaterial::NO_PAGE,
        emissive_texture: crcbl::shaders::mesh::GpuMaterial::NO_PAGE,
        alpha_cutoff: crcbl::shaders::mesh::GpuMaterial::UNTINTED.alpha_cutoff,
        flags: crcbl::shaders::mesh::GpuMaterial::UNTINTED.flags,
    },
];

/// The row of [`PROBE_MATERIALS`] the reversed-Z frames shade through, and the
/// one the reflectivity assertion must **not** find.
const PROBE_PLAIN_ROW: usize = 0;

/// The row of [`PROBE_MATERIALS`] the reflectivity frame shades through.
const PROBE_REFLECTIVE_ROW: usize = 1;

/// How far a channel of the reflectivity attachment may sit from the value its
/// material row implies, as a fraction of full scale.
///
/// Two counts of the `Rgba8Unorm` target it was written through. One is the
/// quantisation itself; the second is the last bit of a multiply of three floats
/// that four rasterisers are not obliged to round identically. Both together are
/// an order of magnitude below what separates the two rows — the assertion names
/// the distance it is really discriminating.
const REFLECTIVITY_TOLERANCE: f32 = 2.0 / 255.0;

/// How far a channel of the ambient-only frame may sit from the value the
/// fixture implies, as a fraction of full scale.
///
/// Wider than [`REFLECTIVITY_TOLERANCE`] because that attachment is linear and
/// this one is not: the colour target is `Rgba8UnormSrgb`, so the hardware
/// applies a transfer function whose slope is steep down where the green and blue
/// channels sit, and a last-bit disagreement in linear light arrives here
/// magnified. It is still far below what the assertion discriminates, which is a
/// channel at zero.
const OCCLUSION_TOLERANCE: f32 = 3.0 / 255.0;

/// The range this probe's coordinates are quantised against: the unit square,
/// which is what `x * 0.5 + 0.5` covers for corners at `±1`.
const PROBE_UV_RANGE: crcbl::shaders::vertex::UvRange = crcbl::shaders::vertex::UvRange {
    scale: [1.0, 1.0],
    offset: [0.0, 0.0],
};

/// Vertices in this probe's geometry: two quads of four corners.
const PROBE_VERTEX_COUNT: usize = 8;

/// Where its attribute region begins, as a word index — the stand-in for
/// `MeshPool::attribute_base`, since this probe binds no pool.
const PROBE_ATTRIBUTE_BASE: u32 =
    (PROBE_VERTEX_COUNT * mesh::POSITION_STRIDE / size_of::<u32>()) as u32;

impl DepthProbe {
    /// The two quads, near-first, in `crcbl::shaders::mesh::MeshVertex` layout,
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
            (0.3f32, 0.25f32, NEAR_QUAD_COLOR),
            (-0.3, 0.6, FAR_QUAD_COLOR),
        ];
        let mut records = Vec::new();
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
                records.push(crcbl::shaders::mesh::MeshVertex::from_normal(
                    position,
                    // Facing the camera, so both quads are lit identically and
                    // the only difference between them is their albedo.
                    [0.0, 0.0, 1.0],
                    [color[0], color[1], color[2], 1.0],
                    // The corner's own position mapped to `0..=1`. It selects
                    // nothing here — the probe's page has one white layer.
                    [x * 0.5 + 0.5, y * 0.5 + 0.5],
                    &PROBE_UV_RANGE,
                ));
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        // **The pool's own arrangement, by hand**: every position, then every
        // attribute. This probe binds no `MeshPool`, so it is what stands in
        // for one — and `PROBE_ATTRIBUTE_BASE` is the boundary the frame block
        // has to carry, exactly as `MeshPool::attribute_base` would.
        let mut vertices = Vec::with_capacity(records.len() * mesh::VERTEX_STRIDE);
        for record in &records {
            vertices.extend_from_slice(&record.position_bytes());
        }
        for record in &records {
            vertices.extend_from_slice(&record.attribute_bytes());
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
            encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
                src: staging,
                src_offset: 0,
                dst: target,
                dst_offset: 0,
                size,
            });
            encoder.pipeline_barrier(&Barriers {
                buffers: &[crcbl::hal::BufferBarrier::new(
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
                size: crcbl::shaders::mesh::FRAME_UNIFORMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a uniform buffer");
        let instances = device
            .create_buffer(&BufferDesc {
                label: Some("probe instances"),
                size: crcbl::shaders::mesh::INSTANCE_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an instance buffer");

        let draw_constants = device
            .create_buffer(&BufferDesc {
                label: Some("probe draw constants"),
                size: crcbl::shaders::mesh::DRAW_CONSTANTS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a draw-constants buffer");
        device
            .write_buffer(
                draw_constants,
                0,
                &crcbl::shaders::mesh::DrawConstants::default().to_bytes(),
            )
            .expect("write");

        // [`PROBE_MATERIALS`], both rows. Row `PROBE_PLAIN_ROW` is the untinted
        // one, so a frame naming it multiplies the albedo by 1.0 and this
        // probe's depth answers stay the answers it recorded before §3.2
        // existed; the reflectivity frame names the other.
        let materials = device
            .create_buffer(&BufferDesc {
                label: Some("probe materials"),
                size: (PROBE_MATERIALS.len() * crcbl::shaders::mesh::MATERIAL_STRIDE) as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a material table");
        for (row, material) in PROBE_MATERIALS.iter().enumerate() {
            device
                .write_buffer(
                    materials,
                    (row * crcbl::shaders::mesh::MATERIAL_STRIDE) as u64,
                    &material.to_bytes(),
                )
                .expect("write");
        }

        let mesh_table = device
            .create_buffer(&BufferDesc {
                label: Some("probe mesh table"),
                size: crcbl::shaders::mesh::MESH_ENTRY_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a mesh table");
        device
            .write_buffer(
                mesh_table,
                0,
                &crcbl::shaders::mesh::GpuMesh {
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
                    uv_range: PROBE_UV_RANGE,
                    // The quads are `MeshVertex::from_normal`'s, so their frame
                    // is the arbitrary stand-in and this probe declines the
                    // authored-tangent claim.
                    flags: 0,
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
                size: crcbl::shaders::light::LIGHT_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a light list");
        device
            .write_buffer(lights, 0, &probe_sun().to_bytes())
            .expect("write");

        let grid = PROBE_GRID;
        let mut grid_words =
            vec![0u32; (grid.froxels() * crcbl::shaders::light::CLUSTER_STRIDE) as usize];
        for froxel in 0..grid.froxels() {
            let base = (froxel * crcbl::shaders::light::CLUSTER_STRIDE) as usize;
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

        // The irradiance probe table, one cleared row — see the field.
        let probes = device
            .create_buffer(&BufferDesc {
                label: Some("probe irradiance probes"),
                size: crcbl::shaders::probe::PROBE_STRIDE as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a probe table");
        device
            .write_buffer(probes, 0, &crcbl::shaders::probe::GpuProbe::ZERO.to_bytes())
            .expect("write");

        // §3.2's texture side, in its smallest honest form: one layer, one
        // texel, opaque white. `Rgba8UnormSrgb` for `crcbl::render::forward`'s
        // reason — the format is the sRGB decode, and `0xFF` decodes to exactly
        // 1.0 — so the sample multiplies the albedo by one and this probe's
        // depth answers stay the answers it recorded before there was a page.
        let base_color_page = crcbl::render::upload_texture_layers(
            device,
            headless.queue,
            "probe base colour",
            crcbl::hal::Format::Rgba8UnormSrgb,
            1,
            1,
            &[&[0xFF, 0xFF, 0xFF, 0xFF]],
        )
        .expect("a one-layer page");
        let base_color_sampler = device
            .create_sampler(&crcbl::hal::SamplerDesc {
                label: Some("probe base colour"),
                mag_filter: crcbl::hal::FilterMode::Nearest,
                min_filter: crcbl::hal::FilterMode::Nearest,
                mip_filter: crcbl::hal::FilterMode::Nearest,
                address_mode: [crcbl::hal::SamplerAddressMode::ClampToEdge; 3],
                ..crcbl::hal::SamplerDesc::default()
            })
            .expect("a sampler");

        let shadow_atlas = device
            .create_image(&crcbl::hal::ImageDesc {
                label: Some("probe shadow atlas"),
                image_type: crcbl::hal::ImageType::D2,
                extent: Extent3d::d2(1, 1),
                format: Format::D32Float,
                mip_levels: 1,
                samples: 1,
                usage: crcbl::hal::ImageUsage::DEPTH_STENCIL_ATTACHMENT
                    .union(crcbl::hal::ImageUsage::SAMPLED),
            })
            .expect("a depth image");
        let shadow_atlas_view = device
            .create_image_view(&crcbl::hal::ImageViewDesc {
                label: Some("probe shadow atlas"),
                image: shadow_atlas,
                view_type: crcbl::hal::ImageViewType::D2,
                format: Format::D32Float,
                range: crcbl::hal::ImageSubresourceRange::all(Format::D32Float),
            })
            .expect("a depth view");
        let shadow_sampler = device
            .create_sampler(&crcbl::hal::SamplerDesc {
                label: Some("probe shadow comparison"),
                // `Some` is what makes it a comparison sampler, and the layout
                // below says the same thing in the vocabulary WebGPU needs.
                compare: Some(crcbl::hal::CompareOp::Greater),
                ..crcbl::hal::SamplerDesc::default()
            })
            .expect("a comparison sampler");

        // `mesh.slang`'s occlusion channel, bound white so the probe's ambient
        // term is unscaled and holding the bent direction's zero sentinel so it
        // is unsteered — `crcbl::render::forward`'s placeholder, by hand,
        // because this file builds its own layout out of the same shader. One
        // texel against a frame of `MESH_EXTENT`, so the ambient term is unscaled
        // only if the shader clamps its fetch; see the field, and the test that
        // asks.
        //
        // `crcbl::shaders::ssao::BENT_NORMAL_NONE` rather than a byte spelled
        // here: `mesh.slang`'s `bent_normal_at` reads any other value as a
        // direction and would sample this probe's ambient along a world axis.
        let occlusion = crcbl::render::upload_texture(
            device,
            headless.queue,
            "probe ssao placeholder",
            Format::Rgba8Unorm,
            1,
            1,
            &[
                0xFF,
                crcbl::shaders::ssao::BENT_NORMAL_NONE,
                crcbl::shaders::ssao::BENT_NORMAL_NONE,
                crcbl::shaders::ssao::BENT_NORMAL_NONE,
            ],
        )
        .expect("a one-texel white image");

        // `mesh.slang`'s split-sum `DFG` table, the real one rather than a
        // stand-in: it is what the engine binds and it is cooked into the
        // binary, so a probe that fabricated its own would be shading with a
        // different lobe than the renderer it exists to speak for.
        let specular_dfg = crcbl::render::upload_texture(
            device,
            headless.queue,
            "probe dfg table",
            Format::Rgba8Unorm,
            dfg::DFG_SIZE as u32,
            dfg::DFG_SIZE as u32,
            &dfg::pair_texels(),
        )
        .expect("the cooked DFG table uploads");

        // And the area lights' fit beside it, for the same reason: the module
        // declares it, so the layout has to cover it whether a frame here reads
        // it or not.
        let ltc_table = crcbl::render::upload_texture(
            device,
            headless.queue,
            "probe ltc table",
            Format::Rgba16Float,
            ltc::LTC_SIZE as u32,
            ltc::LTC_SIZE as u32,
            &ltc::texels(),
        )
        .expect("the cooked LTC table uploads");

        // And `mesh.slang`'s per-probe visibility maps, bound as the renderer's
        // own off-switch binds them: one layer of one texel holding
        // `crcbl::shaders::probe_visibility::FAR`, which no surface is further
        // away than, so `probe_weight` answers one for every probe and the
        // irradiance this probe measures is the unweighted trilinear blend.
        // `upload_texture_layers` rather than `upload_texture` because the
        // module declares a `Texture2DArray`, and WebGPU compares the view type
        // against the layout's at pipeline creation.
        let probe_visibility = {
            let far = crcbl::shaders::probe_visibility::FAR;
            let mut texel = [0u8; crcbl::shaders::probe_visibility::TEXEL_BYTES];
            texel[..4].copy_from_slice(&far.to_le_bytes());
            texel[4..].copy_from_slice(&(far * far).to_le_bytes());
            crcbl::render::upload_texture_layers(
                device,
                headless.queue,
                "probe visibility placeholder",
                Format::Rg32Float,
                1,
                1,
                &[&texel],
            )
            .expect("a one-texel visibility placeholder")
        };

        let entries = [
            crcbl::hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 1,
                visibility: crcbl::hal::ShaderStages::VERTEX,
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 2,
                visibility: crcbl::hal::ShaderStages::VERTEX,
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 3,
                visibility: crcbl::hal::ShaderStages::VERTEX,
                // Not dynamic, unlike `crcbl::render::ForwardRenderer`'s: the
                // probe records one draw, so there is nothing for an offset to
                // select between.
                kind: crcbl::hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 4,
                visibility: crcbl::hal::ShaderStages::VERTEX,
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 5,
                visibility: crcbl::hal::ShaderStages::VERTEX,
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            // The material table, §3.2's factors half. This probe shades
            // nothing per material, but a pipeline layout that does not cover
            // a binding the module declares is refused outright, so the entry
            // has to exist even though one untinted row is all it points at.
            crcbl::hal::BindGroupLayoutEntry {
                binding: 6,
                // Both stages: `mesh.slang` reads the table in the fragment
                // stage, and Slang's Metal backend still hands it to the vertex
                // entry point whether it reads it or not. A layout that covers
                // only one is refused at pipeline creation.
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            // The base-colour page and its sampler. **A layout that leaves
            // these out is not a validation message here — it is a
            // `SIGSEGV`**: lavapipe takes an undeclared descriptor at face
            // value and dereferences whatever the set happens to hold, so a
            // missing entry crashes the runner instead of naming itself.
            crcbl::hal::BindGroupLayoutEntry {
                binding: 7,
                // Both stages, for binding 6's reason: Slang's Metal backend
                // materialises every global in every entry point, so the vertex
                // half has to stay even though only `fragmentMain` samples it.
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                // `D2Array`, matching the view above and the shader's
                // `Texture2DArray`. Vulkan reads the dimension off the view and
                // ignores this, but the seam is one declaration for every
                // backend and WebGPU refuses a layout that disagrees.
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2Array,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 8,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::Sampler { comparison: false },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 15,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2,
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 16,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::Sampler { comparison: true },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 20,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 21,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 22,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2,
                    sample_type: crcbl::hal::SampleType::Float,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 23,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 25,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2,
                    sample_type: crcbl::hal::SampleType::Float,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            // The normal page, on binding 7's terms exactly: this probe's one
            // material row names no normal map, so `shading_normal_of` returns
            // before it samples — but the module *declares* the global, and a
            // pipeline layout that does not cover a declared descriptor is
            // refused. It shares the base-colour sampler at binding 8, so there
            // is no second sampler entry to add.
            crcbl::hal::BindGroupLayoutEntry {
                binding: 26,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2Array,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            // The area lights' table, on the two rows above's terms: declared by
            // the module, read by no frame here, and a layout that left it out
            // would be refused at pipeline creation.
            crcbl::hal::BindGroupLayoutEntry {
                binding: 27,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            // The contact-shadow channel, on the two rows above's terms.
            // `docs/plan/45-shadows.md`'s march writes it and
            // `mesh.slang` multiplies the directional term by it; this probe
            // never runs that pass, but the module declares the global and a
            // layout that leaves a declared descriptor uncovered is refused —
            // on lavapipe and WARP it is a **segmentation fault** rather than a
            // refusal, which is how it was found.
            crcbl::hal::BindGroupLayoutEntry {
                binding: 28,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            // `docs/plan/50-irradiance-probes.md`'s per-probe visibility maps,
            // on the row above's terms and with two differences that matter:
            // the module declares a `Texture2DArray`, so this entry has to as
            // well — WebGPU refuses a pipeline whose layout claims a dimension
            // the bound view does not have — and the image is `Rg32Float`,
            // which is unfilterable there, so the slot must say so too.
            crcbl::hal::BindGroupLayoutEntry {
                binding: 29,
                visibility: crcbl::hal::ShaderStages::VERTEX
                    .union(crcbl::hal::ShaderStages::FRAGMENT),
                kind: crcbl::hal::BindingKind::SampledImage {
                    view_type: crcbl::hal::ImageViewType::D2Array,
                    sample_type: SampleType::UnfilterableFloat,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
        ];

        // **This table is a hand-written copy of `mesh.slang`'s, and this is
        // what holds the two together.** A layout that leaves a descriptor the
        // module declares uncovered is supposed to be refused at pipeline
        // creation — the entries above say so twice — but on lavapipe and WARP
        // it is a `SIGSEGV` instead, naming nothing. That is how binding 28
        // arrived: the contact-shadow channel landed in `mesh.slang`, this
        // array did not gain it, and three tests in this file crashed on two CI
        // runners while every gate on a developer's machine stayed green.
        //
        // `crcbl_shaders::mesh::DECLARED_BINDINGS` is parsed from the shader by
        // a test in that crate, so the pair fails loudly and in order: there
        // first, under a plain `cargo test`, and here if only this copy is
        // stale.
        let mut covered: Vec<u32> = entries.iter().map(|entry| entry.binding).collect();
        covered.sort_unstable();
        assert_eq!(
            covered,
            crcbl::shaders::mesh::DECLARED_BINDINGS,
            "the probe's layout and mesh.slang's declared bindings disagree; a descriptor the \
             module declares and this table omits is a segfault on a software adapter"
        );

        let layout = device
            .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
                label: Some("probe"),
                entries: &entries,
            })
            .expect("a layout");
        let group_entries = [
            crcbl::hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(uniforms),
            },
            crcbl::hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(vertices),
            },
            crcbl::hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(instances),
            },
            crcbl::hal::BindGroupEntry {
                binding: 3,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(draw_constants),
            },
            crcbl::hal::BindGroupEntry {
                binding: 4,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(mesh_table),
            },
            crcbl::hal::BindGroupEntry {
                binding: 5,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(visible_instances),
            },
            crcbl::hal::BindGroupEntry {
                binding: 6,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(materials),
            },
            crcbl::hal::BindGroupEntry {
                binding: 7,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(base_color_page.view),
            },
            crcbl::hal::BindGroupEntry {
                binding: 8,
                array_index: 0,
                resource: crcbl::hal::BindingResource::Sampler(base_color_sampler),
            },
            crcbl::hal::BindGroupEntry {
                binding: 15,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(shadow_atlas_view),
            },
            crcbl::hal::BindGroupEntry {
                binding: 16,
                array_index: 0,
                resource: crcbl::hal::BindingResource::Sampler(shadow_sampler),
            },
            crcbl::hal::BindGroupEntry {
                binding: 20,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(lights),
            },
            crcbl::hal::BindGroupEntry {
                binding: 21,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(light_grid),
            },
            crcbl::hal::BindGroupEntry {
                binding: 22,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(occlusion.view),
            },
            crcbl::hal::BindGroupEntry {
                binding: 23,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(probes),
            },
            crcbl::hal::BindGroupEntry {
                binding: 25,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(specular_dfg.view),
            },
            // The base-colour page's own view again. A descriptor has to point
            // at something the backend can validate, and nothing here samples
            // it — every material row this probe draws with is `NO_PAGE`, which
            // the shader tests before it reads a texel.
            crcbl::hal::BindGroupEntry {
                binding: 26,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(base_color_page.view),
            },
            crcbl::hal::BindGroupEntry {
                binding: 27,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(ltc_table.view),
            },
            // The white texel again, which is what the renderer's own
            // off-switch binds here: the contact channel is a visibility, so
            // one everywhere is "nothing is in the way" and the sun term this
            // probe measures is left exactly as the shading computed it.
            crcbl::hal::BindGroupEntry {
                binding: 28,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(occlusion.view),
            },
            // The one-texel visibility placeholder, which is what makes this
            // probe's probe term the unweighted trilinear blend the Rust mirror
            // predicts.
            crcbl::hal::BindGroupEntry {
                binding: 29,
                array_index: 0,
                resource: crcbl::hal::BindingResource::ImageView(probe_visibility.view),
            },
        ];
        let group = device
            .create_bind_group(&crcbl::hal::BindGroupDesc {
                label: Some("probe"),
                layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");
        let set_layouts = [layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
                label: Some("probe"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl::hal::ShaderModuleDesc {
                label: Some("mesh.slang"),
                spirv: crcbl::shaders::MESH.spirv(),
                wgsl: crcbl::shaders::MESH.wgsl(),
                msl: crcbl::shaders::MESH.msl(),
                // **DXIL too, or this suite is Vulkan-only again.** A backend
                // that compiles only DXIL refuses a module handed the other
                // three, which is what made this test fail on WARP the first
                // time CI ran it agnostically.
                dxil: &crcbl::shaders::MESH.dxil_containers(),
            })
            .expect("the committed SPIR-V is accepted");
        // **One per `SV_Target` `fragmentMain` writes.** A fragment stage
        // writing a location a pipeline has no attachment for is a validation
        // error under WebGPU's rules and, on Vulkan, a warning at best — so a
        // hand-built pipeline short of a target would be this suite passing
        // while the real forward pass gained an output. Each format is the one
        // the matching `crcbl::render::TransientImageDesc` names —
        // `reflectivity` and `motion` — and the pass below attaches images of
        // exactly those descriptions.
        let color_targets = [
            crcbl::hal::ColorTargetState::opaque(headless.format),
            crcbl::hal::ColorTargetState::opaque(Format::Rgba8Unorm),
            crcbl::hal::ColorTargetState::opaque(Format::Rg16Float),
        ];
        let pipeline = device.create_graphics_pipeline(&crcbl::hal::GraphicsPipelineDesc {
            label: Some("depth probe"),
            layout: pipeline_layout,
            vertex: crcbl::hal::ShaderEntry {
                module,
                entry_point: "vertexMain",
            },
            fragment: Some(crcbl::hal::ShaderEntry {
                module,
                entry_point: "fragmentMain",
            }),
            primitive: crcbl::hal::PrimitiveState {
                // No culling: the point is the depth test, and a winding
                // mistake would otherwise delete a quad and look like one.
                cull_mode: crcbl::hal::CullMode::None,
                ..crcbl::hal::PrimitiveState::default()
            },
            // The seam's default, unchanged: `Greater` on `D32Float` with
            // writes on. **This is what the two projections are tested
            // against, and it is not adjusted between runs.**
            depth_stencil: Some(crcbl::hal::DepthStencilState::default()),
            multisample: crcbl::hal::MultisampleState::default(),
            color_targets: &color_targets,
        });
        device.destroy_shader_module(module);

        Self {
            lights,
            light_grid,
            probes,
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
            specular_dfg,
            ltc_table,
            probe_visibility,
            base_color_sampler,
            shadow_atlas,
            shadow_atlas_view,
            shadow_sampler,
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
        self.specular_dfg.destroy(device);
        self.ltc_table.destroy(device);
        self.probe_visibility.destroy(device);
        device.destroy_buffer(self.visible_instances);
        device.destroy_buffer(self.mesh_table);
        device.destroy_buffer(self.probes);
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
const PROBE_GRID: crcbl::render::Grid = crcbl::render::Grid {
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
fn probe_sun() -> crcbl::shaders::light::GpuLight {
    crcbl::shaders::light::GpuLight {
        position: [0.0; 4],
        color: [0.8, 0.8, 0.8, 0.0],
        direction: [0.0, 0.0, 1.0, 0.0],
        tangent: [0.0; 4],
        kind: crcbl::shaders::light::KIND_DIRECTIONAL,
        cos_inner: 0.0,
        shadow_tile: crcbl::shaders::light::NO_SHADOW_TILE,
        flags: 0,
    }
}

/// How many pixels of `frame` differ from the pixel at its top-left corner.
///
/// The corner stands in for "what this frame looks like where nothing drew":
/// neither quad reaches it, so it is the pass's clear whenever the pass ran at
/// all. Comparing against it rather than against an encoded [`PROBE_CLEAR`]
/// keeps this honest about the target's format — the clear is authored in
/// floats and read back through whichever transfer function the swapchain
/// carries, and a diagnostic that guessed at that encoding could report a fully
/// cleared frame as fully drawn.
fn pixels_unlike_the_corner(frame: &crcbl_golden::Image) -> usize {
    let Some(corner) = frame.pixel(0, 0) else {
        return 0;
    };
    frame
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| *pixel != corner)
        .count()
}

/// `value` in linear light, encoded the way this probe's `Rgba8UnormSrgb`
/// swapchain encodes what a fragment wrote into it.
///
/// IEC 61966-2-1's transfer function, which the Vulkan specification's sRGB
/// conversion is. It is here because the occlusion assertion below compares a
/// *derived* colour with a readback byte, and the value `mesh.slang` returns is
/// not the value that lands in the buffer — the other assertions in this file
/// read either a linear attachment or a channel ordering, and needed no such
/// thing.
fn srgb_encode(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// Both of a probe frame's colour attachments, read back.
struct ProbeFrame {
    /// Attachment 0: the swapchain image the two quads were drawn into.
    color: crcbl_golden::Image,
    /// Attachment 1: `Rgba8Unorm`, `rgb` each fragment's `F0` and `a` its
    /// roughness.
    ///
    /// Carried as an [`Image`](crcbl_golden::Image) for its `pixel` accessor and
    /// nothing else — these bytes are material data rather than a picture, and
    /// no golden holds them.
    reflectivity: crcbl_golden::Image,
}

/// Renders the probe with `view_proj`, shading through row `material` of
/// [`PROBE_MATERIALS`], and reads both of its colour attachments back.
fn render_probe(
    headless: &Headless,
    probe: &mut DepthProbe,
    pool: &mut crcbl::render::TransientPool,
    view_proj: crcbl::math::Mat4,
    material: usize,
) -> ProbeFrame {
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;

    // The one field of the instance that varies between this file's frames.
    // Written here rather than at construction so the row a frame shades through
    // is the frame's own decision — see the field.
    device
        .write_buffer(
            probe.instances,
            0,
            &crcbl::shaders::mesh::GpuInstance {
                transform: crcbl::math::Mat4::IDENTITY.to_cols_array(),
                material: u32::try_from(material).expect("a table of a few rows"),
                ..crcbl::shaders::mesh::GpuInstance::default()
            }
            .to_bytes(),
        )
        .expect("write");

    let uniforms = crcbl::shaders::mesh::FrameUniforms {
        view_proj: view_proj.to_cols_array(),
        camera_position: [0.0, 0.0, PROBE_EYE, 1.0],
        ambient: PROBE_AMBIENT,
        // This probe binds no shadow atlas and draws through a pipeline of its
        // own, so the cascades are never sampled. Identity matrices and a zero
        // reach say that plainly: a fragment whose eye distance is past every
        // split takes the shader's "outside the cascade" path, which is fully
        // lit — the same picture the probe asserted before shadows existed.
        shadow_view_proj: [crcbl::math::Mat4::IDENTITY.to_cols_array();
            crcbl::shaders::mesh::SHADOW_CASCADES],
        cascade_far: [0.0; 4],
        shadow_params: [0.0; 4],
        // The grid the host filled above, so the fragment stage looks itself up
        // in the froxel that really holds the one light.
        cluster_grid: PROBE_GRID.to_frame_block(),
        // No shadowed light either, and the row above says so: `probe_sun`
        // carries `NO_SHADOW_TILE`, so nothing in this frame reads these.
        light_view_proj: [crcbl::math::Mat4::IDENTITY.to_cols_array();
            crcbl::shaders::mesh::SHADOW_LIGHT_TILES],
        // No irradiance grid: this probe's own pipeline binds a single zeroed
        // probe row, and the default volume is what makes the fragment stage
        // add exactly nothing to `PROBE_AMBIENT` — see
        // `crcbl::shaders::probe`.
        probes: crcbl::shaders::probe::ProbeVolume::default(),
        // No LOD selection either: this probe draws through its own pipeline
        // with `mesh.slang`'s fragment stage, which reads none of these — the
        // screen-error heatmap is the mesh path's, and it is a debug view this
        // frame never switches on.
        lod_params: [0.0; 4],
        // And no fog: a zero density is what makes `mesh.slang`'s composite the
        // identity, so this probe reads the radiance the shading produced
        // rather than one an atmosphere has been over. The colour is
        // unobservable while the density is zero and is written as such.
        fog_params: [0.0; 4],
        fog_color: [0.0; 4],
        // And no sky, for the same reason the probe volume above is empty:
        // three zero rows add exactly nothing to `PROBE_AMBIENT`.
        sky_sh_r: [0.0; 4],
        sky_sh_g: [0.0; 4],
        sky_sh_b: [0.0; 4],
        // The frame this probe draws is the only one there is, so the previous
        // camera is this one: every fragment's motion vector is exactly zero,
        // which is what the two quads standing still deserve. The attachment it
        // lands in is read by nothing here — this file's claims are about the
        // depth test and the reflectivity channel — and a probe whose motion
        // target held a reprojection through some other matrix would be a
        // difference between two frames that are one frame.
        previous_view_proj: view_proj.to_cols_array(),
        // Where this probe's own attribute region begins — see
        // `PROBE_ATTRIBUTE_BASE`, which is what stands in for the pool's.
        vertex_pool: [PROBE_ATTRIBUTE_BASE, 0, 0, 0],
        // And no atlas rectangles, on the cascade matrices' terms exactly: the
        // rows this frame carries name no tile, so nothing looks a rectangle
        // up. A zero rectangle is what `crcbl::render::shadow` writes for a
        // slot no map was rendered into, and this whole frame is that.
        shadow_atlas_rect: [[0.0; 4]; crcbl::shaders::mesh::SHADOW_ATLAS_TILES],
    };
    device
        .write_buffer(probe.uniforms, 0, &uniforms.to_bytes())
        .expect("write");

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    // Four bytes a pixel for both attachments: the swapchain's format and
    // `Rgba8Unorm` are the same size, so one figure covers both readbacks.
    let bytes = u64::from(width) * u64::from(height) * 4;
    let readback = |label| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: bytes,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer")
    };
    let staging = readback("probe readback");
    let reflectivity_staging = readback("probe reflectivity readback");

    // Where the graph's realised reflectivity handle lands, so the copy below
    // can name it. A transient has no handle until the pool has given it one, so
    // there is nothing to write down before the frame runs. `Cell` rather than a
    // channel: the pass body runs synchronously inside `execute`, on this
    // thread — `mesh`'s HDR probe reads its target back the same way.
    let reflectivity_handle: std::cell::Cell<Option<crcbl::hal::ImageHandle>> =
        std::cell::Cell::new(None);

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("probe frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = crcbl::render::RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl::render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                claim: crcbl::render::InitialClaim::Acquired,
                final_state: ResourceState::TransferSrc,
            },
        );
        let depth = graph.create_image(
            "probe-depth",
            crcbl::render::TransientImageDesc::scene_depth(MESH_EXTENT),
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
        // the same image: `SYNC-HAZARD-WRITE-AFTER-READ`.
        //
        // Read out of the pool rather than remembered here, on
        // `crcbl::render::ForwardRenderer`'s terms: the ledger records what a
        // graph *executed*, so a frame that was described and then refused
        // cannot leave a probe-side field claiming a transition that never
        // happened. `None` is the first frame, and `Undefined` is what an image
        // no barrier has moved is in.
        let shadow = graph.import_image(
            "probe-shadow-atlas",
            crcbl::render::ImportedImage {
                image: probe.shadow_atlas,
                view: probe.shadow_atlas_view,
                format: Format::D32Float,
                extent: (1, 1),
                initial: pool
                    .imported_image_use(probe.shadow_atlas)
                    .unwrap_or(ResourceState::Undefined),
                // The probe owns this image across calls and no semaphore sits
                // between them, so the declaration above is one the graph can
                // and does check — this is the site whose second `Undefined`
                // was the hazard.
                claim: crcbl::render::InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        );
        // `mesh.slang`'s second output, described exactly as the forward pass
        // describes its own — so what this file's pipeline is validated against
        // is the real transient rather than a lookalike declared here.
        let reflectivity = graph.create_image(
            "probe-reflectivity",
            crcbl::render::TransientImageDesc::reflectivity(MESH_EXTENT),
        );
        // `mesh.slang`'s third output, on the second's terms exactly: the
        // pipeline above names a target for it, so the pass has to attach one.
        // Nothing here reads it back — the block above says why every texel of
        // it is zero — but an attachment the pipeline declares and the pass does
        // not provide is the validation error this file exists to not have.
        let motion = graph.create_image(
            "probe-motion",
            crcbl::render::TransientImageDesc::motion(MESH_EXTENT),
        );
        graph
            .add_render_pass("probe")
            .clear_color(target, PROBE_CLEAR)
            // **Cleared to `NO_REFLECTION`, and the corner assertion is what
            // reads it.** A pixel no fragment covered has no material: no `F0`
            // and fully rough is the quadruple that says so, as it is in
            // `crcbl::render::forward`.
            .clear_color(reflectivity, crcbl::shaders::ssr::NO_REFLECTION)
            // Zero, exactly as `crcbl::render::forward` clears its own: a pixel
            // no fragment covered has no motion.
            .clear_color(motion, [0.0; 4])
            // The reversed-Z clear: `depth::CLEAR` = 0.0, so any geometry beats
            // the empty buffer under `Greater`.
            .clear_depth(depth)
            .read_image(shadow)
            .execute(|ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(probe.pipeline);
                encoder.bind_group(0, probe.group, &[], probe.pipeline_layout);
                encoder.bind_index_buffer(probe.indices, 0, crcbl::hal::IndexFormat::Uint32);
                encoder.draw_indexed(0..12, 0, 0..1);
            });
        // One declaration, and the graph works out that the reflectivity target
        // has to move from a colour attachment to a copy source. There is not
        // one hand-written barrier in this file either.
        let sink = &reflectivity_handle;
        graph
            .add_compute_pass("reflectivity probe")
            .use_image(reflectivity, ResourceState::TransferSrc)
            .execute(move |ctx| sink.set(Some(ctx.image(reflectivity))));
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("executed");

    let layers = ImageSubresourceLayers {
        aspect: ImageAspect::COLOR,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    // Both copies sit outside every pass and need no barrier of their own: the
    // graph left the swapchain image in `TransferSrc` because the import asked
    // for it, and the reflectivity target for the same reason.
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: layers,
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: reflectivity_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: reflectivity_handle
            .get()
            .expect("the reflectivity probe pass ran"),
        image_subresource: layers,
        image_offset: crcbl::hal::Offset3d::default(),
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
    let mut reflectivity = poisoned(bytes as usize);
    headless.readback(reflectivity_staging, bytes, &mut reflectivity);
    assert!(
        reflectivity.iter().any(|&byte| byte != POISON),
        "all {bytes} bytes of the reflectivity readback are still {POISON:#04x} — \
         no copy reached it, so there is nothing here to say what the second \
         attachment holds. Look at the copy and the transient's usage flags."
    );
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    device.destroy_buffer(reflectivity_staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    ProbeFrame {
        color: crcbl_golden::Image::from_readback(width, height, &pixels, order)
            .expect("one image"),
        // `Rgba` whatever the swapchain carries: this attachment's format is
        // `TransientImageDesc::reflectivity`'s, not the presentation surface's.
        reflectivity: crcbl_golden::Image::from_readback(
            width,
            height,
            &reflectivity,
            crcbl_golden::ChannelOrder::Rgba,
        )
        .expect("one image"),
    }
}

/// The two projections this file renders through: the engine's own reversed-Z
/// matrix, and the conventional control the reversed-Z test discriminates
/// against.
///
/// Built together and from one view and one field of view, because the whole
/// force of that test is that **only the projection differs** — two call sites
/// each composing their own would be two places for that to stop being true.
/// The centre of the near quad is where both tests sample, and it is the same
/// pixel in both because the camera is.
fn probe_projections() -> (crcbl::math::Mat4, crcbl::math::Mat4) {
    #[allow(clippy::cast_precision_loss)]
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    let view = crcbl::math::camera::rh::view::look_at_mat4(
        crcbl::math::Vec3::new(0.0, 0.0, PROBE_EYE),
        crcbl::math::Vec3::ZERO,
        crcbl::math::Vec3::Y,
    );
    let fov = core::f32::consts::FRAC_PI_4;

    // The engine's own projection, straight out of `crcbl-render`.
    let reversed = crcbl::render::Projection::Perspective {
        fov_y: fov,
        near: PROBE_NEAR,
    }
    .matrix(aspect)
        * view;
    // The control: conventional depth, 0 at the near plane and 1 at the far one.
    // `crcbl-render` deliberately has no constructor for this, which is why the
    // suite reaches for glam directly.
    let standard =
        crcbl::math::camera::rh::proj::directx::perspective(fov, aspect, PROBE_NEAR, PROBE_FAR)
            * view;
    (reversed, standard)
}

/// The pixel both tests sample: the centre of the frame, which under the
/// engine's projection is inside the near quad.
const PROBE_CENTRE: (u32, u32) = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2);

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
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn reversed_z_puts_the_nearer_surface_in_front_and_standard_z_would_not() {
    let headless = Headless::open_for_mesh();
    let mut probe = DepthProbe::new(&headless);
    let mut pool = crcbl::render::TransientPool::new();

    let (reversed, standard) = probe_projections();
    let centre = PROBE_CENTRE;

    let reversed = render_probe(&headless, &mut probe, &mut pool, reversed, PROBE_PLAIN_ROW);
    let reversed_frame = reversed.color;
    let pixel = reversed_frame.pixel(centre.0, centre.1).expect("inside");
    // **What the second attachment holds at the same pixel**, which separates
    // the two readings the colour cannot. `vk e2e (lavapipe, windows)` has
    // failed here seven times with the centre at the pass's clear and a
    // frame-wide count of exactly the far quad's footprint less the near one's,
    // so the near quad wins the depth test and then shades to the clear. Either
    // the fragment stage ran and only the lit term came out wrong, or no
    // fragment ran there at all — and reflectivity says which, because
    // `mesh.slang` writes it from the material row unconditionally, in the same
    // invocation, with none of the lighting between. A row's `F0` here and a
    // black centre means the shading; a cleared reflectivity means no
    // invocation, which would contradict the depth test the count implies.
    let reflectivity = reversed
        .reflectivity
        .pixel(centre.0, centre.1)
        .expect("inside");
    // **What the rest of the frame holds, because one pixel cannot say.** A
    // centre holding the clear is either "no fragment survived anywhere" or
    // "fragments landed, but not here", and those want looking at in different
    // places — the first at the depth clear and the depth state, the second at
    // the projection and the vertex data. The corner is the reference because
    // neither quad reaches it: the near one spans 34 pixels from the centre and
    // the far one 60.
    let drawn = pixels_unlike_the_corner(&reversed_frame);
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
         are this harness's, not a frame's.\n\
         \x20 * The reflectivity attachment holds {reflectivity:?} at this \
         same pixel. `mesh.slang` writes it from the material row in the same \
         invocation, with none of the lighting between — so a row's own `F0` \
         here says the fragment stage ran and only the lit term is wrong, and \
         a cleared value says no invocation reached this pixel at all.\n\
         \x20 * {drawn} pixel(s) of this frame differ from its corner. Zero \
         means no fragment survived anywhere, so look at the depth clear and \
         the depth state rather than at where the geometry went; a non-zero \
         count means fragments landed and missed the centre, which is the \
         projection and the vertex data.",
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

    let standard_frame =
        render_probe(&headless, &mut probe, &mut pool, standard, PROBE_PLAIN_ROW).color;
    let pixel = standard_frame.pixel(centre.0, centre.1).expect("inside");
    assert!(
        pixel[2] > pixel[0] && pixel[2] > 100,
        "the control is only meaningful if a conventional projection really does \
         invert the outcome under the engine's `Greater` test; it gave {pixel:?}, \
         which is not blue. Re-derive the quad depths rather than relaxing this."
    );

    eprintln!(
        "crcbl forward e2e: reversed-Z centre {:?}, conventional-Z centre {:?} — the same \
         pipeline, the same compare op, only the projection differs",
        reversed_frame.pixel(centre.0, centre.1).expect("inside"),
        standard_frame.pixel(centre.0, centre.1).expect("inside"),
    );

    probe.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **The second colour attachment carries the material row the instance named.**
///
/// `docs/plan/18-render-features.md`'s screen-space reflections read `F0` and
/// roughness out of a target the forward pass writes beside its colour, and
/// **nothing in a rendered picture shows what is in it**: a wrong channel, a
/// wrong row, or a target the fragment stage never wrote all leave the frame
/// exactly as every golden already has it. So this reads the attachment back and
/// says what it holds.
///
/// The frame is shaded through [`PROBE_REFLECTIVE_ROW`] — a conductor, so its
/// `F0` is its albedo and therefore coloured, with all three channels distinct.
/// Three things are asserted, each failing a different mistake:
///
/// * The centre carries that row's `F0` and roughness. A swap of the `rgb`
///   and `a` halves fails all four channels; so does a target left at its clear.
/// * It is not the neighbouring row's answer: that row is a dielectric, whose
///   `F0` is grey whatever its albedo, and its roughness differs by design.
/// * A corner no fragment covered is `NO_REFLECTION` — no `F0`, fully rough:
///   the clear the design asks for, so a later march cannot start from a pixel
///   that has no material, nor read a mirror in it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_reflectivity_target_carries_the_bound_material_row_and_no_reflection_where_nothing_drew() {
    let headless = Headless::open_for_mesh();
    let mut probe = DepthProbe::new(&headless);
    let mut pool = crcbl::render::TransientPool::new();

    let (reversed, _) = probe_projections();
    let frame = render_probe(
        &headless,
        &mut probe,
        &mut pool,
        reversed,
        PROBE_REFLECTIVE_ROW,
    )
    .reflectivity;

    let row = PROBE_MATERIALS[PROBE_REFLECTIVE_ROW];
    let plain = PROBE_MATERIALS[PROBE_PLAIN_ROW];
    // **Derived from the fixture rather than written down.** `metallic` is 1.0,
    // factor times the page's white texel — and alpha is the lobe's roughness,
    // as `mesh.slang` rounds it for the store. A hand-written quadruple here
    // would go on describing the old material the day either constant moves.
    let expected = [
        NEAR_QUAD_COLOR[0] * row.base_color[0],
        NEAR_QUAD_COLOR[1] * row.base_color[1],
        NEAR_QUAD_COLOR[2] * row.base_color[2],
        crcbl::shaders::ssr::stored_roughness(row.roughness),
    ];

    let centre = PROBE_CENTRE;
    let pixel = frame.pixel(centre.0, centre.1).expect("inside");
    for (channel, want) in expected.iter().enumerate() {
        let got = f32::from(pixel[channel]) / 255.0;
        assert!(
            (got - want).abs() <= REFLECTIVITY_TOLERANCE,
            "channel {channel} of the reflectivity attachment at {centre:?} is \
             {got}, and the bound material row says {want}. The whole pixel is \
             {pixel:?}.\n\
             \x20 * All four channels wrong, with `rgb` holding what `a` should: \
             the two halves of `mesh.slang`'s `FragmentOutput.reflectivity` are \
             swapped.\n\
             \x20 * `rgb` zero and `a` full: no fragment wrote here, or the \
             pipeline's second target and the pass's second attachment disagree \
             — the pass would have cleared it and nothing else would have \
             touched it."
        );
    }

    // **And demonstrably not the row beside it**, checked as its own statement so
    // a wrong row names itself instead of arriving as "channel 0 was off by
    // 0.86". A dielectric's `F0` is the same number in all three channels
    // whatever its base colour, which is exactly what this must not be.
    assert!(
        pixel[0] != pixel[1] || pixel[1] != pixel[2],
        "the reflectivity at {centre:?} is the grey {pixel:?}, but the bound row \
         is a conductor and its `F0` is its albedo. A grey triple is what row \
         {PROBE_PLAIN_ROW} produces, so either the fragment stage resolved that \
         row or it ignored `metallic`."
    );
    let roughness = f32::from(pixel[3]) / 255.0;
    let plain_roughness = crcbl::shaders::ssr::stored_roughness(plain.roughness);
    assert!(
        (roughness - expected[3]).abs() < (roughness - plain_roughness).abs(),
        "the reflectivity alpha at {centre:?} is {roughness}, which is nearer row \
         {PROBE_PLAIN_ROW}'s roughness of {plain_roughness} than the bound row \
         {PROBE_REFLECTIVE_ROW}'s {}. The two rows differ by design; reading the wrong one is \
         what this separation exists to catch.",
        expected[3],
    );

    // The clear, which is the half of this slice a drawn pixel cannot show.
    // Neither quad reaches the corner — the near one spans 34 pixels from the
    // centre and the far one 60 — so this is the attachment's load op and
    // nothing else.
    let corner = frame.pixel(0, 0).expect("inside");
    let no_reflection = crcbl::shaders::ssr::NO_REFLECTION.map(|channel| (channel * 255.0) as u8);
    assert_eq!(
        corner, no_reflection,
        "a pixel no geometry covered must hold `NO_REFLECTION` — no `F0`, fully \
         rough: it has no material, and `docs/plan/18-render-features.md` asks \
         for the clear that says so rather than one a later march would read as \
         a reflective surface, which a zero alpha now is. Got {corner:?}."
    );

    eprintln!(
        "crcbl forward e2e: reflectivity at {centre:?} is {pixel:?} — row \
         {PROBE_REFLECTIVE_ROW}'s F0 {:?} and roughness {}",
        [expected[0], expected[1], expected[2]],
        expected[3],
    );

    probe.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **A fragment reading a 1×1 occlusion image gets that image's one texel.**
///
/// This is the property `crcbl::render::forward`'s AO off-switch rests on, and it
/// is the one that was not true: `mesh.slang` fetches the occlusion channel with
/// a `Load` at `SV_Position.xy`, and a `Load` outside a texture's extent yields
/// **zero** rather than the nearest texel. Bound to a frame larger than one
/// pixel, an unclamped fetch therefore reads the renderer's white placeholder as
/// *total* occlusion everywhere but the origin — which is not an error anywhere,
/// on any backend, and arrives only as a frame that lost its ambient term.
///
/// So the frame is shaded with **nothing but ambient**: the light list is
/// darkened, which zeroes `direct` and `gloss` exactly, and `mesh.slang`'s whole
/// output becomes `diffuse_albedo * ambient * occluded`. The occlusion factor is
/// then the only unknown left in a pixel, and the expected colour is derived from
/// the fixture rather than written down beside it.
///
/// Two assertions, in the order that separates the two ways this can be black:
///
/// * The reflectivity attachment at the same pixel is not the pass's clear.
///   `mesh.slang` writes it from the material row in the same invocation with
///   none of the lighting between, so this is the fragment stage having run —
///   without it, a frame where no geometry drew reads exactly like a frame that
///   read `0.0` out of the placeholder.
/// * The colour is the ambient term. An unclamped `Load` fails this at every
///   channel with a pixel of zeroes.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_fragment_reads_the_one_texel_occlusion_placeholder_as_no_occlusion() {
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    let mut probe = DepthProbe::new(&headless);
    let mut pool = crcbl::render::TransientPool::new();

    // The one light darkened rather than removed. The froxel grid this fixture
    // fills by hand lists light zero, so an empty list would change what the
    // cluster lookup finds as well as what it contributes — and a colour of zero
    // is what makes `direct` and `gloss` exactly zero rather than merely small.
    device
        .write_buffer(
            probe.lights,
            0,
            &crcbl::shaders::light::GpuLight {
                color: [0.0; 4],
                ..probe_sun()
            }
            .to_bytes(),
        )
        .expect("write");

    let (reversed, _) = probe_projections();
    let frame = render_probe(&headless, &mut probe, &mut pool, reversed, PROBE_PLAIN_ROW);
    let centre = PROBE_CENTRE;

    let reflectivity = frame
        .reflectivity
        .pixel(centre.0, centre.1)
        .expect("inside");
    let no_reflection = crcbl::shaders::ssr::NO_REFLECTION.map(|channel| (channel * 255.0) as u8);
    assert_ne!(
        reflectivity, no_reflection,
        "no fragment wrote the second attachment at {centre:?}, so nothing was \
         shaded there and the colour below cannot say anything about the \
         occlusion read. Look at the geometry, the depth state and the \
         projection — not at `mesh.slang`'s occlusion fetch."
    );
    // The row is `UNTINTED`, whose roughness sits on `ROUGHNESS_CUTOFF`; the
    // shader's rounding must land it on the level above, on every backend, so
    // the ramp `ssr.slang` derives is exactly zero and no march starts.
    let stored = crcbl::shaders::ssr::stored_roughness(PROBE_MATERIALS[PROBE_PLAIN_ROW].roughness);
    assert!(stored >= crcbl::shaders::ssr::ROUGHNESS_CUTOFF);
    assert_eq!(
        f32::from(reflectivity[3]),
        stored * 255.0,
        "the cutoff material's reflectivity alpha at {centre:?} must reload from \
         the Rgba8Unorm attachment as the level `mesh.slang` rounded it to, so \
         SSR derives a zero ramp and cannot start a march; got {reflectivity:?}"
    );

    // What `mesh.slang` computes when ambient is the whole of the light: the
    // near quad's vertex colour through the bound row, times the frame's ambient,
    // times the occlusion the fragment read. Derived from the fixture — a triple
    // written down here would go on describing the old quad the day any of the
    // three constants moves — and encoded, because the target is an sRGB format
    // and the shader's output is linear.
    let row = PROBE_MATERIALS[PROBE_PLAIN_ROW];
    let mut expected = [0.0f32; 3];
    for (channel, want) in expected.iter_mut().enumerate() {
        // `1.0 - metallic` is the shader's own diffuse albedo, spelled out
        // because a conductor has no ambient term at all and this row's being a
        // dielectric is what makes the assertion possible.
        *want = srgb_encode(
            NEAR_QUAD_COLOR[channel]
                * row.base_color[channel]
                * (1.0 - row.metallic)
                * PROBE_AMBIENT[channel],
        );
    }

    let pixel = frame.color.pixel(centre.0, centre.1).expect("inside");
    for (channel, want) in expected.iter().enumerate() {
        let got = f32::from(pixel[channel]) / 255.0;
        assert!(
            (got - want).abs() <= OCCLUSION_TOLERANCE,
            "channel {channel} of the ambient-only frame at {centre:?} is {got}, \
             and the fixture says {want}. The whole pixel is {pixel:?}, and the \
             reflectivity attachment says a fragment ran here.\n\
             \x20 * Every channel at zero: the occlusion fetch returned `0.0`. \
             The bound image is one texel and the frame is {extent:?}, so this \
             is `mesh.slang`'s `Load` reading outside the texture — the \
             coordinate is not being clamped against `GetDimensions`.\n\
             \x20 * One channel off: the ambient term reached the frame and \
             something else in the fixture moved. Re-derive from \
             `NEAR_QUAD_COLOR`, the bound material row and `PROBE_AMBIENT`.",
            extent = MESH_EXTENT,
        );
    }

    eprintln!(
        "crcbl forward e2e: a 1×1 occlusion image bound to a {MESH_EXTENT:?} frame reads as \
         no occlusion — centre {pixel:?}, ambient-only expectation {expected:?}"
    );

    probe.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
