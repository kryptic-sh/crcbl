//! The frame milestones 3, 4 and 5 draw: a lit mesh into an HDR target, then a
//! tonemap into whatever the caller is presenting to.
//!
//! ```text
//! ┌ cull ───────────┐ ┌ draw-args ──────┐ ┌ forward ──────────┐ ┌ tonemap ──────┐
//! │ instances ─────▶│ │ visible ───────▶│ │ scene-color Clear │ │ reads it      │
//! │ visible list    │ │ per-bucket runs │ │ scene-depth Clear │ │ writes the    │
//! │ + a GPU count   │ │ + indirect args │ │ one indirect call │ │ target        │
//! │                 │ │ + a draw count  │ │ per bucket        │ │               │
//! └─────────────────┘ └─────────────────┘ └───────────────────┘ └───────────────┘
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
//! Explicitly not here: bindless at scale (P3), shadows and real post (P7),
//! material authoring (topic 37), asset loading (P9). The mesh is a constant in
//! `crcbl-shaders` because rung 3 says "hardcoded cube/sphere".
//!
//! What *is* here since 2026-08 is [`crate::material_table`]: §3.2's material
//! table, holding **both** halves of "texture indices + factors", with
//! [`mesh::GpuInstance::material`] selecting a row. Three rows are resident and
//! two of them exist to be seen: [`ForwardRenderer::set_tinted_pyramid`]'s
//! differs from the default row in its factor alone, and
//! [`ForwardRenderer::set_textured_pyramid`]'s in its base-colour page layer
//! alone.
//!
//! The page is the texture side, and it is one `D2Array` image rather than an
//! array of descriptors — `BindingModel::ArrayPages` rather than `Bindless`.
//! That is the whole of the binding-model decision §3.2 leaves open, taken
//! here because a layer index needs nothing of a device where a descriptor
//! array needs `Features::DESCRIPTOR_INDEXING`, which `crcbl-mtl` withdraws. So
//! there is one lookup, one layout and one artifact rather than a permutation,
//! and a device that reports the feature runs the same declaration. What
//! bindless buys later is capacity, not a second path — see `PAGE_EXTENT` and
//! the layout entry for binding 7.
//!
//! What *was* on that list until 2026-08 is GPU culling and the indirect draw
//! count, and both are here now: [`crate::draw_gen`] runs `cull.slang` and
//! `draw_gen.slang` in front of this pass, and the draws below come out of the
//! arguments they wrote. See that module for what a bucket is and why the
//! arguments are per bucket rather than per surviving instance.
//!
//! # The CPU records a fixed number of draws
//!
//! One per bucket, whatever the scene holds — `docs/plan/03-gpu-driven-rendering.md`'s
//! headline goal, "10 objects and 10,000 objects record roughly the same
//! commands". Adding an object is an instance in the pool and nothing else;
//! removing one is [`InstancePool::remove`] and nothing else. Neither changes a
//! single command this file records, because what varies — how many instances a
//! bucket draws, and which — is written by a compute pass into a buffer the
//! draw reads.
//!
//! Which *call* those draws are is the one branch: [`GeometryPath::IndirectCount`]
//! takes the count from GPU memory too, and [`GeometryPath::IndirectPerBatch`] —
//! Metal, whose API has multi-draw-indirect and no GPU-side count — reads each
//! bucket's one argument structure unconditionally and leans on its instance
//! count being zero.
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
//! # A second resident, and how it finds its own vertices
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
//! the real ones arrive as data. The base vertex comes from
//! [`MeshPool`]'s mesh table, indexed by the drawn instance's
//! [`mesh::GpuInstance::mesh`] — so it is resolved per instance, by the GPU,
//! and one draw could cover instances of different meshes. The instance comes
//! from the bucket's run of survivors, walked with `SV_InstanceID`, which counts
//! from zero on every target *because* the first instance is zero. All a draw
//! says for itself is where its run starts, in a [`mesh::DrawConstants`] block
//! reached through a dynamic offset: one block per bucket, written once at
//! build.
//!
//! That is what makes the pool's *second* resident,
//! [`mesh::pyramid_vertices`], draw its own geometry rather than the cube's.
//!
//! The pyramid is in the pools from `new` but has no *instance* until a caller
//! asks for one with [`ForwardRenderer::set_pyramid`] — an instance in the pool
//! is an object in the scene now that culling decides what draws. The frame the
//! samples draw is still the cube alone, which is why their golden images did
//! not move either.
//!
//! # A third resident, and how it finds its own clusters
//!
//! The same argument one layer down. A mesh that fits **one** cluster cannot
//! tell a per-cluster offset that works from one that is lost, and the cube and
//! the pyramid fit one each — 24 and 16 vertices against a bound of 64. So the
//! pool's third resident is [`mesh::open_box_vertices`], five clusters of one
//! flat face each, four of them at a `Meshlet::vertex_offset` that is not zero.
//! It is [`crate::cluster_pool`]'s counterpart to what the pyramid is for
//! [`crate::mesh_pool`], and it is what §3.5's per-cluster culling will need
//! before a surviving-cluster count can mean anything.
//!
//! It arrives on [`ForwardRenderer::set_open_box`]' terms exactly: resident from
//! `new`, in the scene only when a caller asks, so no golden moved for it
//! either.
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
    BufferUsage, ColorTargetState, CullMode, DepthStencilState, Device, DrawIndirect,
    DrawIndirectCount, FilterMode, Format, GeometryPath, GraphicsPipelineDesc,
    GraphicsPipelineHandle, HalError, ImageViewHandle, ImageViewType, IndexFormat, LoadOp,
    MemoryLocation, MeshPipelineDesc, MultisampleState, PipelineLayoutDesc, PipelineLayoutHandle,
    PrimitiveState, QueueHandle, ResourceState, SamplerAddressMode, SamplerDesc, SamplerHandle,
    ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp,
};
use crcbl_shaders::{MESH, MESH_CLUSTER, Stage, TONEMAP, mesh};
use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, DirectionalLight};
use crate::cluster_pool::ClusterPool;
use crate::cull::Frustum;
use crate::draw_gen::{DrawGen, DrawGenDesc};
use crate::graph::{ImageId, ImportedImage, RenderGraph};
use crate::instance_pool::{InstanceHandle, InstancePool, InstancePoolDesc};
use crate::material_table::{MaterialTable, MaterialTableDesc};
use crate::mesh_pool::{MeshPool, MeshPoolDesc, MeshPoolError};
use crate::texture::{UploadedTexture, upload_texture_layers};
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
/// A handful of meshes are resident today, and the pool exists so that stops
/// being the interesting number. It is device-local memory reserved at start-up
/// and never grown — [`crate::mesh_pool`] says why — so it is sized for the
/// scene P7 puts in it rather than for the meshes P1 draws.
const POOL_VERTEX_CAPACITY: u32 = 64 * 1024;

/// Indices the geometry pool holds. Four per vertex is the usual ratio for
/// indexed triangle soup, rounded up.
const POOL_INDEX_CAPACITY: u32 = 256 * 1024;

/// Meshes the geometry pool can hold at once, which is the length of its mesh
/// table.
///
/// Distinct meshes, not instances of them: §3.2's instance array is what a
/// scene's object count sizes, and a mesh id names one entry here however many
/// instances carry it. Sized on the same terms as the pools it indexes —
/// reserved at start-up, never grown — against a scene with far more objects
/// than distinct geometry.
const POOL_MESH_CAPACITY: u32 = 1024;

/// Instances the instance pool holds, per frame in flight.
///
/// A handful are resident, for the same reason the geometry pools hold a
/// handful of meshes. Sized against topic 03's exit criterion — "sandbox scene:
/// 10k+ instanced meshes" — rather than against them, because the buffers are
/// reserved at start-up and never grown.
const POOL_INSTANCE_CAPACITY: u32 = 16 * 1024;

/// Materials the material table holds.
///
/// A handful are resident, on the same terms as the meshes: the buffer is
/// reserved at start-up and never grown, so it is sized against a scene rather
/// than against what P1 draws. Distinct materials, not instances of them — a
/// material id names one row however many instances carry it, which is the
/// property that makes the table worth having.
const POOL_MATERIAL_CAPACITY: u32 = 1024;

/// The second material's base colour, and the whole of what makes it visible.
///
/// A factor per channel with no two alike, so multiplying by it moves every
/// colour it touches: a tint that left a channel at `1.0` would leave the
/// pyramid's white base looking like a lighting difference, and one that left
/// them equal would be a brightness change a shading bug could also produce.
/// See [`ForwardRenderer::set_tinted_pyramid`], which is the pair this exists
/// for.
const PYRAMID_TINT: [f32; 4] = [0.15, 0.45, 1.0, 1.0];

/// The base-colour page's extent, in texels — square, and **two**.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's
/// [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) page is one image with a
/// layer per material texture, and two texels a side is the smallest extent in
/// which a layer can be something other than a flat colour. Small on purpose,
/// and not only because this is demo content:
///
/// * A flat layer would make the golden below pass with **no UV at all**. Four
///   texels is what makes the texture coordinate observable, because a mesh
///   whose UVs never varied would shade each face in one texel's colour.
/// * Every texel boundary is a hard edge — the sampler has no mips and filters
///   nearest, see [`ForwardRenderer::build`] — and a hard edge is where two
///   rasterisers can land on opposite sides of an interpolated UV. Four texels
///   put **one** boundary across a face in each axis, at `0.5`, which is as far
///   from a vertex as an edge can be. A denser checker would put a row of
///   disagreeable pixels through every face for no more evidence.
const PAGE_EXTENT: u32 = 2;

/// Bytes in one layer of the page: `PAGE_EXTENT²` RGBA texels.
const PAGE_LAYER_BYTES: usize = (PAGE_EXTENT * PAGE_EXTENT) as usize * 4;

/// The page layer a material naming no texture samples, which
/// [`mesh::GpuMaterial::UNTINTED`] is written against.
///
/// **Layer 0 is white**, which is the convention that type's docs state and
/// [`UNTEXTURED_TEXELS`] keeps: an `Rgba8UnormSrgb` texel of `0xFF` decodes to
/// exactly `1.0`, so a material that names this layer is shaded by the same
/// product it was before there was a page at all.
const UNTEXTURED_LAYER: u32 = 0;

/// The layer [`ForwardRenderer::set_textured_pyramid`] shades with.
const CHECKER_LAYER: u32 = 1;

/// Layer [`UNTEXTURED_LAYER`]: opaque white, in every texel.
///
/// sRGB-encoded like the layer beside it — the image is `Rgba8UnormSrgb` — and
/// `0xFF` is the one value the encoding leaves alone, so the sampler returns
/// exactly `1.0`.
const UNTEXTURED_TEXELS: [u8; PAGE_LAYER_BYTES] = [0xFF; PAGE_LAYER_BYTES];

/// Layer [`CHECKER_LAYER`]: four **distinct** greys, one per texel.
///
/// Distinct rather than a two-value checker, for the reason `crcbl-vk`'s sprite
/// suite records about its sheets: a two-value checker is symmetric under both a
/// flipped U and a flipped V, so either mistake would produce the same picture.
/// No two of these are equal, so any flip is a different frame.
///
/// Grey rather than coloured because the colour axis is already spoken for:
/// [`PYRAMID_TINT`] is what proves the *factor* column, and a texture that also
/// changed hue would make the two columns' evidence look alike.
const CHECKER_TEXELS: [u8; PAGE_LAYER_BYTES] = [
    0xFF, 0xFF, 0xFF, 0xFF, // (0, 0)
    0xB0, 0xB0, 0xB0, 0xFF, // (1, 0)
    0x70, 0x70, 0x70, 0xFF, // (0, 1)
    0x30, 0x30, 0x30, 0xFF, // (1, 1)
];

/// The page, layer by layer: element `n` is layer `n`.
///
/// The one place the page's length lives, so [`UNTEXTURED_LAYER`] and
/// [`CHECKER_LAYER`] can be checked against it rather than against a number
/// written twice.
const PAGE_LAYERS: [&[u8; PAGE_LAYER_BYTES]; 2] = [&UNTEXTURED_TEXELS, &CHECKER_TEXELS];

/// Buckets in the draw table, which is how many indirect calls the forward pass
/// records — **whatever the scene holds**.
///
/// One per resident mesh, because an argument structure's index range is per
/// draw and instances of different meshes cannot share one. See
/// [`crate::draw_gen`] on what a bucket is and what a longer key would buy.
const BUCKET_COUNT: u32 = 3;

/// The cube's bucket, the pyramid's, and the open box's. Named rather than
/// written as `0`, `1` and `2` where the bucket table is filled in.
const CUBE_BUCKET: usize = 0;
const PYRAMID_BUCKET: usize = 1;
const OPEN_BOX_BUCKET: usize = 2;

/// Which indirect call the forward pass records per bucket.
///
/// Derived from [`GeometryPath`] at build and stored, because the answer cannot
/// change while a device is open and re-deriving it inside the pass body would
/// be a capability query per draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmitTail {
    /// [`GeometryPath::MeshShader`]: one `draw_mesh_tasks` per bucket, of a
    /// **mesh** pipeline — no vertex stage, no index buffer and no indirect
    /// arguments anywhere in the call. §3.5's primary geometry path; see
    /// `shaders/mesh_cluster.slang`.
    Mesh,
    /// [`GeometryPath::IndirectCount`]: the draw count comes from GPU memory
    /// too, so the CPU never learns whether a bucket drew anything.
    Count,
    /// [`GeometryPath::IndirectPerBatch`]: one `draw_indexed_indirect` per
    /// bucket with a count of one. An empty bucket's argument structure carries
    /// an instance count of zero and draws nothing, which is what makes this the
    /// same picture rather than an approximation of it.
    PerBatch,
}

impl EmitTail {
    /// What `caps` selects.
    ///
    /// One value per [`GeometryPath`] since 2026-08: the mesh-shader path used
    /// to degrade to an indirect tail and log that it had, because there was no
    /// mesh pipeline to select.
    const fn from_caps(caps: &crcbl_hal::DeviceCaps) -> Self {
        match caps.geometry_path() {
            GeometryPath::MeshShader => Self::Mesh,
            GeometryPath::IndirectCount => Self::Count,
            GeometryPath::IndirectPerBatch => Self::PerBatch,
        }
    }

    /// Whether this tail draws through a mesh pipeline, which decides the bind
    /// group layout, the pipeline kind and the constant block — everything the
    /// two shapes of this pass differ in.
    const fn is_mesh(self) -> bool {
        matches!(self, Self::Mesh)
    }
}

/// Everything the forward frame owns, created once.
#[derive(Debug)]
pub struct ForwardRenderer {
    // Geometry: the global pools, and the meshes resident in them. Each
    // range is resolved once, at build, and `MeshPool::mesh` only hands one out
    // for a mesh whose upload has completed — so there is no way to reach a
    // draw below with a mesh the GPU has not received.
    pool: MeshPool,
    /// The dynamic offset of each bucket's [`mesh::DrawConstants`] block, in
    /// the order [`CUBE_BUCKET`] and [`PYRAMID_BUCKET`] name.
    ///
    /// **The whole of what the CPU still says per draw.** Everything else a
    /// draw needs — how many instances, which indices, which vertices — is in
    /// the arguments the GPU wrote or in a table it resolves them through.
    bucket_constants: [u32; BUCKET_COUNT as usize],

    // The instance array, and the objects' places in it. `begin_frame` rewrites
    // the cube's transform and the pool uploads only what changed.
    instances: InstancePool,
    cube_instance: InstanceHandle,
    /// The pyramid's instance while a caller is asking for one.
    ///
    /// `None` is how it is *hidden*: an instance that is in the pool is in the
    /// scene, because the frame no longer records a draw per object and the
    /// cull pass is what decides what is drawn. See
    /// [`ForwardRenderer::set_pyramid`].
    pyramid_instance: Option<InstanceHandle>,
    /// A second instance of the pyramid mesh, shaded through the table's other
    /// row. See [`ForwardRenderer::set_tinted_pyramid`], which is what puts it
    /// in the scene and what it is for.
    tinted_pyramid_instance: Option<InstanceHandle>,
    /// A third, shaded through a row that differs from the first's in nothing
    /// but its page layer. See [`ForwardRenderer::set_textured_pyramid`].
    textured_pyramid_instance: Option<InstanceHandle>,
    /// The open box's instance while a caller is asking for one, on the
    /// pyramid's terms exactly. See [`ForwardRenderer::set_open_box`].
    open_box_instance: Option<InstanceHandle>,
    /// The mesh ids those instances carry. Kept because every write of an
    /// instance writes the whole record, and an instance that lost its mesh id
    /// would resolve to entry 0 — which is a mesh, and the wrong one.
    cube_mesh: u32,
    pyramid_mesh: u32,
    open_box_mesh: u32,

    /// §3.2's material table, and the two rows in it.
    ///
    /// One buffer shared by every frame's bind group, not a ring — see
    /// [`crate::material_table`], which is where that decision lives.
    materials: MaterialTable,
    /// The row every instance carries unless something asks for another, whose
    /// factors are all `1.0`. Kept for the reason the mesh ids above are: a
    /// `set` writes the whole record, and an instance that lost its material id
    /// would name row 0 by accident — which is a material, and only *happens*
    /// to be this one.
    untinted_material: u32,
    /// The row [`PYRAMID_TINT`] went into.
    tinted_material: u32,
    /// The row that is [`mesh::GpuMaterial::UNTINTED`] with
    /// [`CHECKER_LAYER`] in place of [`UNTEXTURED_LAYER`] — the same factor as
    /// [`ForwardRenderer::untinted_material`] and a different texture, which is
    /// what makes the texture column observable on its own.
    textured_material: u32,
    /// §3.2's texture side: one `D2Array` image whose layers the material rows
    /// index. One page, bound once, for every material — see the module docs on
    /// why this is [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) and not
    /// a bindless descriptor array.
    base_color_page: UploadedTexture,
    /// The sampler the page is read through.
    base_color_sampler: SamplerHandle,

    /// The cull and draw-argument passes, and the indirect arguments they
    /// produce.
    draws: DrawGen,
    /// Which call the forward pass records — the device's [`GeometryPath`],
    /// resolved once.
    emit: EmitTail,
    /// §3.5's clusters, and **only on [`EmitTail::Mesh`]**: the two indirect
    /// tails draw the same geometry out of the index pool and read none of
    /// this. `None` is therefore the ordinary state on most devices rather than
    /// a failure, and it is also what makes a fall-through impossible — see
    /// [`ForwardRenderer::geometry_path`].
    clusters: Option<ClusterPool>,
    /// How many clusters each bucket's mesh has, which is the x extent of its
    /// dispatch. Zero on the indirect tails, where nothing dispatches.
    bucket_clusters: [u32; BUCKET_COUNT as usize],

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
    /// Uploaded images, which own a view each and so cannot be rolled back as
    /// one list of handles.
    textures: Vec<UploadedTexture>,
    /// The geometry pool, which owns two buffers, a semaphore and anything
    /// still staged — so it cannot be rolled back as a list of handles.
    pool: Option<MeshPool>,
    /// The instance pool, which owns one buffer per frame in flight.
    instances: Option<InstancePool>,
    /// The material table, which owns one buffer.
    materials: Option<MaterialTable>,
    /// The cull and draw-argument passes, which own two pipelines and a ring of
    /// buffers each — and which clean themselves up on their own failure path,
    /// so this only carries one that was built.
    draws: Option<DrawGen>,
    /// The cluster buffers, which own three buffers and are built on the
    /// mesh-shader path alone.
    clusters: Option<ClusterPool>,
}

impl Rollback {
    /// Releases everything, in the same dependency order as
    /// [`ForwardRenderer::destroy`].
    fn run(self, device: &dyn Device) {
        for texture in self.textures {
            texture.destroy(device);
        }
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
        if let Some(clusters) = self.clusters {
            clusters.destroy(device);
        }
        if let Some(draws) = self.draws {
            draws.destroy(device);
        }
        if let Some(table) = self.materials {
            table.destroy(device);
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
        // Resolved before anything is created, because it decides the *shape*
        // of what is created — the bind group layout, which pipeline kind the
        // pass builds, and what a bucket's constant block says. A device on the
        // mesh path never builds the raster pipeline and a device on either
        // indirect tail never builds the mesh one, which is what makes "the
        // frame came out of the mesh stage" a fact about the object graph
        // rather than a claim about a branch.
        let emit = EmitTail::from_caps(&device.caps());

        let (pool, cube_mesh, pyramid_mesh, open_box_mesh) = Self::build_geometry(device, queue)?;
        // The handles are `Copy`, so they can be read out before the pool
        // becomes the rollback's — which it must be before the first `?` below,
        // or a failed pipeline would leak two device-local buffers. The index
        // pool is not named here at all: it is bound at draw time rather than
        // descriptor-written, so only the vertex pool and the mesh table reach a
        // bind group.
        let vertices = pool.vertex_buffer();
        let mesh_table = pool.table_buffer();
        rollback.pool = Some(pool);

        // §3.2's texture side, before the table that indexes it: a material row
        // is written with a layer number, so the layer has to exist to be named.
        //
        // **`Rgba8UnormSrgb`, and that is the colour-space decision.** The
        // texels above are sRGB-encoded, which is what glTF defines a
        // base-colour texture to be, so the format is what makes the sampler
        // decode them — and `mesh.slang` then multiplies a linear texel by a
        // linear `base_color` and lights in linear, exactly as it did before
        // there was a texture. Taking the decode off the format would mean
        // multiplying an encoded value by a linear one, which is
        // `crate::sprite_pass`'s "darkens every half-transparent edge" defect
        // in a different place.
        let page_layers = PAGE_LAYERS.map(|texels| texels.as_slice());
        let base_color_page = upload_texture_layers(
            device,
            queue,
            "material base colour",
            Format::Rgba8UnormSrgb,
            PAGE_EXTENT,
            PAGE_EXTENT,
            &page_layers,
        )?;
        rollback.textures.push(base_color_page);

        // Nearest and clamped, like the tonemap's sampler below and for a
        // reason of its own: the page has one mip level, because §3.2 makes mip
        // generation a compute pass of its own, and filtering a page with no
        // mips buys a shimmer rather than a smoother picture. A second sampler
        // object rather than sharing that one, so a capture names each for what
        // it filters.
        let base_color_sampler = device.create_sampler(&SamplerDesc {
            label: Some("material base colour"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(base_color_sampler);

        // The material table, before the instances: an instance is written with
        // the material id it carries, so the row has to exist to be named.
        let (materials, untinted_material, tinted_material, textured_material) =
            Self::build_materials(device)?;
        let material_buffer = materials.buffer();
        rollback.materials = Some(materials);

        let (instances, cube_instance) =
            Self::build_instances(device, cube_mesh, untinted_material)?;
        // Same handle-then-hand-over dance as the geometry pool above: the
        // buffers are `Copy` and are read out before the pool becomes the
        // rollback's, which it must be before the first `?` below — after which
        // nothing needs the pool again until `begin_frame`.
        let instance_buffers: Vec<BufferHandle> = instances.buffers().to_vec();
        rollback.instances = Some(instances);

        // The two compute passes, and the buffers the draws come out of. Built
        // here rather than beside the pipelines below because the mesh bind
        // group names one of its buffers, so it has to exist first.
        //
        // The bucket table, and the one place its order is decided.
        let mut bucket_meshes = [0u32; BUCKET_COUNT as usize];
        bucket_meshes[CUBE_BUCKET] = cube_mesh;
        bucket_meshes[PYRAMID_BUCKET] = pyramid_mesh;
        bucket_meshes[OPEN_BOX_BUCKET] = open_box_mesh;
        let draws = DrawGen::new(
            device,
            &DrawGenDesc {
                label: Some("forward"),
                instances: &instance_buffers,
                mesh_table,
                bucket_meshes: &bucket_meshes,
                instance_capacity: POOL_INSTANCE_CAPACITY,
            },
        )?;
        let runs: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.runs(frame))
            .collect();
        let args: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.args(frame))
            .collect();
        rollback.draws = Some(draws);

        // §3.5's clusters, on the path that reads them and on no other. The
        // records are `crcbl-shaders`' — cooked, because the builder is
        // `crcbl-scene`'s and the renderer must not depend on that crate — and
        // they arrive in bucket-mesh order so a bucket's range is its own
        // index. See `crate::cluster_pool`.
        let mut bucket_clusters = [0u32; BUCKET_COUNT as usize];
        if emit.is_mesh() {
            let mut cooked: [crcbl_shaders::meshlet::MeshClusters; BUCKET_COUNT as usize] =
                Default::default();
            cooked[CUBE_BUCKET] = crcbl_shaders::meshlet::cube_clusters();
            cooked[PYRAMID_BUCKET] = crcbl_shaders::meshlet::pyramid_clusters();
            cooked[OPEN_BOX_BUCKET] = crcbl_shaders::meshlet::open_box_clusters();
            let clusters = ClusterPool::new(device, "forward", &cooked)?;
            for (bucket, count) in bucket_clusters.iter_mut().enumerate() {
                *count = clusters
                    .range(bucket)
                    .unwrap_or_else(|| unreachable!("one cluster range per bucket, in order"))
                    .count;
            }
            rollback.clusters = Some(clusters);
        }

        // --- the mesh pass ---
        //
        // No `BindingFlags` anywhere: this layout is legal on **both** tiers,
        // because a lit mesh is not a Tier A feature. The bindless shape — a
        // trailing `VARIABLE_COUNT | PARTIALLY_BOUND | UPDATE_AFTER_BIND`
        // texture array — is topic 03's, at P3.
        // **Which stage pulls the geometry**, and the only thing the two shapes
        // of this layout disagree about for bindings 0 to 8. `ShaderStages::MESH`
        // is outside `GRAPHICS` and `ALL`, so it is named rather than implied,
        // and every backend refuses a layout carrying it on a device without
        // `Features::MESH_SHADER` — which is exactly the device that never
        // reaches this arm.
        let geometry = if emit.is_mesh() {
            ShaderStages::MESH
        } else {
            ShaderStages::VERTEX
        };
        let mut mesh_entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: geometry.union(ShaderStages::FRAGMENT),
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: geometry,
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
                visibility: geometry,
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
                visibility: geometry,
                // `dynamic: true`, and it is the whole mechanism: it is what
                // lets a draw say where its run of instances starts without
                // `draw_indexed`'s own base instance, which the module docs and
                // `mesh.slang`'s header explain the targets disagree about.
                // Same shape, for the same reason, as `crate::sprite_pass`'s.
                kind: BindingKind::UniformBuffer { dynamic: true },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // The mesh table. `StructuredBuffer` again — the vertex
                    // stage looks its mesh up and writes nothing — and one
                    // buffer shared by every frame's group, because unlike the
                    // instance array nothing rewrites it between frames.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 5,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // The per-bucket runs of surviving instances. Read-only
                    // here and written by the draw-argument pass, which is a
                    // different bind group and a different pass — the graph is
                    // what orders the two.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 6,
                // **Both stages, and the union is not belt-and-braces.** The
                // fragment stage is where `mesh.slang` now reads the table, so
                // it plainly needs it — a pipeline layout that leaves it out is
                // refused outright by wgpu and reported as
                // `VUID-VkGraphicsPipelineCreateInfo-layout-07988` by Vulkan.
                // The vertex half has to stay because Slang's Metal backend
                // materialises every global in every entry point: `vertexMain`
                // in `msl/mesh.metal` still takes `materials [[buffer(6)]]`
                // whether it reads it or not, so dropping VERTEX would break
                // Metal alone, on a runner this team cannot debug on.
                visibility: geometry.union(ShaderStages::FRAGMENT),
                kind: BindingKind::StorageBuffer {
                    // The material table. One buffer in every frame's group,
                    // like the mesh table above and unlike the instance ring:
                    // a material is written when it is created, not per frame.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 7,
                // Both stages, for binding 6's reason exactly: the fragment
                // stage samples it, and Slang's Metal backend materialises
                // every global in every entry point — `vertexMain` in
                // `msl/mesh.metal` takes the texture whether it reads it or
                // not, so a layout without VERTEX would break Metal alone.
                visibility: geometry.union(ShaderStages::FRAGMENT),
                // **`D2Array`, and the layout is where wgpu wants to hear it.**
                // The other three backends read the dimension off the bound
                // view and never consult this; WebGPU compares the two at
                // pipeline creation and refuses the pair, which is what a
                // layout claiming `D2` over an array view got — "expects
                // dimension = D2, but given a view with dimension = D2Array".
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                },
                // **`count: 1`, and this is the binding-model decision.** One
                // `D2Array` image whose *layers* the material rows index is
                // `BindingModel::ArrayPages`; an array of `count` descriptors
                // indexed per fragment would be `Bindless` and would need
                // `Features::DESCRIPTOR_INDEXING`, which `crcbl-mtl` withdraws.
                // So this layout is legal on every device, exactly as the six
                // buffers above are, and no `BindingFlags` appear anywhere in
                // this file yet.
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 8,
                visibility: geometry.union(ShaderStages::FRAGMENT),
                kind: BindingKind::Sampler,
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        if emit.is_mesh() {
            // §3.5's four extra reads, and only on the path that has them.
            // Adding them unconditionally would make every device declare four
            // buffers `mesh.slang` does not name — and WebGPU, which checks a
            // bind group against its layout entry for entry, would then need
            // four buffers created for a path that never reads them.
            //
            // `read_only: true` on all four, which is the truth rather than a
            // hint: the mesh stage indexes them and writes nothing, and it is
            // what lets the graph merge read-after-read.
            let cluster_read = BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            };
            mesh_entries.extend((9..=12).map(|binding| BindGroupLayoutEntry {
                binding,
                // **Not the fragment stage.** These four are the geometry
                // stage's alone, and `mesh_cluster.slang` has no fragment entry
                // point of its own to materialise them into — its fragment
                // stage is `mesh.slang`'s, which names bindings 0 to 8 and
                // nothing above them.
                visibility: geometry,
                kind: cluster_read,
                count: 1,
                flags: BindingFlags::empty(),
            }));
        }
        let mesh_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("mesh frame"),
            entries: &mesh_entries,
        })?;
        rollback.bind_group_layouts.push(mesh_layout);

        // A dynamic offset must be a multiple of the device's alignment, and one
        // block has to fit inside one stride. Read from the device rather than
        // assumed, exactly as `crate::sprite_pass` does: 256 on WebGPU, 64 on a
        // typical desktop Vulkan driver.
        //
        // **Both blocks are sixteen bytes**, so the stride is one number rather
        // than one per path: `mesh::DrawConstants` is a `uint` and three of
        // padding, and `meshlet::ClusterDrawConstants` is four `uint`s that say
        // something. Which of the two a bucket's block holds is decided below.
        let alignment = device.caps().limits.min_uniform_buffer_offset_alignment;
        const _: () = assert!(
            mesh::DRAW_CONSTANTS_SIZE == crcbl_shaders::meshlet::CLUSTER_DRAW_CONSTANTS_SIZE
        );
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
            size: u64::from(draw_stride) * u64::from(BUCKET_COUNT),
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        })?;
        rollback.buffers.push(draw_constants);
        // Written here and never again: where a bucket's run of instances
        // starts is fixed by the bucket table, and the table is fixed at build.
        // What varies per frame is how much of the run is filled, and the GPU
        // writes that into the bucket's indirect arguments.
        let draws = rollback
            .draws
            .as_ref()
            .unwrap_or_else(|| unreachable!("draw generation was placed in the rollback above"));
        let mut bucket_constants = [0u32; BUCKET_COUNT as usize];
        for (bucket, offset) in bucket_constants.iter_mut().enumerate() {
            let index = bucket;
            let bucket =
                u32::try_from(bucket).unwrap_or_else(|_| unreachable!("a table of a few buckets"));
            *offset = bucket * draw_stride;
            let base = draws.bucket_base(bucket);
            // The mesh path's block says three more things — where this
            // bucket's mesh's clusters are, how many it has, and which element
            // of the indirect arguments holds its instance count — because a
            // dispatch carries none of them and a `draw_indexed_indirect` does
            // not need them. All three are fixed when the bucket table is,
            // exactly like `base`.
            let block = if emit.is_mesh() {
                crcbl_shaders::meshlet::ClusterDrawConstants {
                    base,
                    cluster_base: rollback
                        .clusters
                        .as_ref()
                        .and_then(|clusters| clusters.range(index))
                        .unwrap_or_else(|| unreachable!("the mesh path built a cluster pool"))
                        .base,
                    cluster_count: bucket_clusters[index],
                    bucket,
                }
                .to_bytes()
            } else {
                mesh::DrawConstants { base }.to_bytes()
            };
            device.write_buffer(draw_constants, u64::from(*offset), &block)?;
        }

        let mut uniforms = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut mesh_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for (frame, &slot_instances) in instance_buffers.iter().enumerate() {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("mesh frame uniforms"),
                size: mesh::FRAME_UNIFORMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(buffer);
            let mut entries = vec![
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
                BindGroupEntry {
                    binding: 4,
                    array_index: 0,
                    // The same table in every frame's group, unlike the
                    // instance array above it: the pool writes an entry when a
                    // mesh is uploaded or freed, neither of which happens
                    // between frames here.
                    resource: BindingResource::whole_buffer(mesh_table),
                },
                BindGroupEntry {
                    binding: 5,
                    array_index: 0,
                    // This frame's runs, for the reason the instance ring
                    // above gives: the draw-argument pass rewrites them every
                    // frame and the previous frame may still be drawing from
                    // the other slot.
                    resource: BindingResource::whole_buffer(runs[frame]),
                },
                BindGroupEntry {
                    binding: 6,
                    array_index: 0,
                    // The same table in every frame's group, on the mesh
                    // table's terms rather than the instance array's.
                    resource: BindingResource::whole_buffer(material_buffer),
                },
                BindGroupEntry {
                    binding: 7,
                    array_index: 0,
                    // **One entry, `array_index: 0`**, because the page is one
                    // image and the layer is chosen in the shader. A bindless
                    // array would be one entry per texture at ascending array
                    // indices, which is the write path `BindGroupEntry`'s own
                    // docs describe and the one this pass does not take.
                    resource: BindingResource::ImageView(base_color_page.view),
                },
                BindGroupEntry {
                    binding: 8,
                    array_index: 0,
                    resource: BindingResource::Sampler(base_color_sampler),
                },
            ];
            if let Some(clusters) = rollback.clusters.as_ref() {
                entries.extend([
                    BindGroupEntry {
                        binding: 9,
                        array_index: 0,
                        // The same three buffers in every frame's group, on the
                        // mesh table's terms: clusters are written when the
                        // pool is built and never again.
                        resource: BindingResource::whole_buffer(clusters.clusters()),
                    },
                    BindGroupEntry {
                        binding: 10,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(clusters.vertices()),
                    },
                    BindGroupEntry {
                        binding: 11,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(clusters.corners()),
                    },
                    BindGroupEntry {
                        binding: 12,
                        array_index: 0,
                        // **This frame's arguments, read as data.** The mesh
                        // path records no indirect call at all; what it wants
                        // out of this buffer is the one word only the GPU knows
                        // — how many instances survived into each bucket — and
                        // it is this frame's slot for the ring's reason.
                        resource: BindingResource::whole_buffer(args[frame]),
                    },
                ]);
            }
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
            // **Both containers, and still one module.** A DXIL container holds
            // exactly one entry point, so this is a list where its three
            // neighbours are single artifacts; the backend that consumes it
            // picks the container named by the stage it is building. Nothing
            // here branches on a backend, and the three that ignore DXIL see
            // the same module they always did.
            dxil: &MESH.dxil_containers(),
        })?;
        let mesh_targets = [ColorTargetState::opaque(Format::Rgba16Float)];
        // Shared by both pipeline shapes, because the only thing that differs
        // is which stage produces the geometry.
        //
        // Back-face culling is on from the first mesh. The cube's winding is
        // asserted by `crcbl-shaders`' own tests, so a face that vanished would
        // be a *test* failure rather than a debugging session — and a mesh
        // drawn without culling would let a winding mistake survive into P7's
        // geometry pool. The mesh stage emits its corner triples in the index
        // buffer's own order, so the winding it produces is the same one.
        let primitive = PrimitiveState {
            cull_mode: CullMode::Back,
            ..PrimitiveState::default()
        };
        // Milestone 3's depth test, and the seam's default is already
        // reversed-Z: `Greater` against `D32Float`, writes on. The clear value
        // that agrees with it comes from the graph (`PassBuilder::clear_depth`),
        // and the projection matrix that agrees with *both* comes from
        // `crate::camera`.
        let depth_stencil = Some(DepthStencilState::default());
        let (mesh_pipeline, cluster_module) = if emit.is_mesh() {
            // **The mesh stage's module is `mesh_cluster.slang`; the fragment
            // stage's is still `mesh.slang`'s.** A pipeline takes a module per
            // stage, so the shading — Lambert, Blinn, the material row, the
            // page sample — is the same code both paths run rather than a copy
            // that agrees today. `mesh_cluster.slang`'s header carries the
            // argument, and its `VertexOutput` is what the two agree through.
            let cluster_entry = entry(&MESH_CLUSTER, Stage::Mesh)?;
            let cluster_module = device.create_shader_module(&ShaderModuleDesc {
                label: Some("mesh_cluster.slang"),
                spirv: MESH_CLUSTER.spirv(),
                // `None`, and it is the whole reason this is a second file:
                // WGSL cannot express a mesh stage at all.
                wgsl: MESH_CLUSTER.wgsl(),
                msl: MESH_CLUSTER.msl(),
                dxil: &MESH_CLUSTER.dxil_containers(),
            })?;
            let pipeline = device.create_mesh_pipeline(&MeshPipelineDesc {
                label: Some("forward mesh cluster"),
                layout: mesh_pipeline_layout,
                // **No amplification stage.** Per-cluster culling is what one
                // would be for and that is §3.5's own later slice, so a task
                // stage here would be a stage that dispatched its mesh stage
                // and did nothing else — and `Features::TASK_SHADER` is a
                // separate capability this path would then need.
                task: None,
                mesh: ShaderEntry {
                    module: cluster_module,
                    entry_point: cluster_entry,
                },
                fragment: Some(ShaderEntry {
                    module: mesh_module,
                    entry_point: mesh_fragment,
                }),
                primitive,
                depth_stencil,
                multisample: MultisampleState::default(),
                color_targets: &mesh_targets,
            });
            (pipeline, Some(cluster_module))
        } else {
            let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
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
                primitive,
                depth_stencil,
                multisample: MultisampleState::default(),
                color_targets: &mesh_targets,
            });
            (pipeline, None)
        };
        device.destroy_shader_module(mesh_module);
        if let Some(module) = cluster_module {
            device.destroy_shader_module(module);
        }
        let mesh_pipeline = mesh_pipeline?;
        rollback.pipelines.push(mesh_pipeline);

        // --- the tonemap pass ---
        let tonemap_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                },
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
            // A container per entry point, for the reason the mesh module above
            // gives.
            dxil: &TONEMAP.dxil_containers(),
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
            bucket_constants,
            instances: rollback
                .instances
                .take()
                .unwrap_or_else(|| unreachable!("the pool was placed in the rollback above")),
            cube_instance,
            pyramid_instance: None,
            tinted_pyramid_instance: None,
            textured_pyramid_instance: None,
            open_box_instance: None,
            cube_mesh,
            pyramid_mesh,
            open_box_mesh,
            materials: rollback
                .materials
                .take()
                .unwrap_or_else(|| unreachable!("the table was placed in the rollback above")),
            untinted_material,
            tinted_material,
            textured_material,
            base_color_page,
            base_color_sampler,
            draws: rollback.draws.take().unwrap_or_else(|| {
                unreachable!("draw generation was placed in the rollback above")
            }),
            emit,
            clusters: rollback.clusters.take(),
            bucket_clusters,
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

    /// Creates the geometry pool and makes every resident mesh resident in it.
    ///
    /// Separate from [`ForwardRenderer::build`] because it is **self-cleaning**:
    /// the pool is not the rollback's until this has returned, so a failure
    /// between creating it and flushing the cube releases it here.
    fn build_geometry(
        device: &dyn Device,
        queue: QueueHandle,
    ) -> Result<(MeshPool, u32, u32, u32), HalError> {
        let mut pool = MeshPool::new(
            device,
            &MeshPoolDesc {
                label: Some("forward geometry"),
                vertex_capacity: POOL_VERTEX_CAPACITY,
                index_capacity: POOL_INDEX_CAPACITY,
                mesh_capacity: POOL_MESH_CAPACITY,
            },
        )?;
        match Self::residents(device, queue, &mut pool) {
            Ok((cube, pyramid, open_box)) => Ok((pool, cube, pyramid, open_box)),
            Err(error) => {
                pool.destroy(device);
                Err(error.into())
            }
        }
    }

    /// Creates the material table and fills its two rows, returning it and the
    /// ids of both.
    ///
    /// Self-cleaning for the same reason [`ForwardRenderer::build_geometry`] is:
    /// the table is not the rollback's until this has returned.
    ///
    /// The untinted row is created **first**, so it is row 0 — which is what
    /// [`mesh::GpuInstance::default`] names, and therefore what an instance
    /// written without a material id would shade with. Nothing here relies on
    /// that: every instance below is written with an id read out of this table.
    /// It is a defence in depth, not the contract, because a caller assembling
    /// a `GpuInstance` by hand is one who has not read either.
    fn build_materials(device: &dyn Device) -> Result<(MaterialTable, u32, u32, u32), HalError> {
        let mut materials = MaterialTable::new(
            device,
            &MaterialTableDesc {
                label: Some("forward"),
                capacity: POOL_MATERIAL_CAPACITY,
            },
        )?;
        match Self::material_rows(device, &mut materials) {
            Ok((untinted, tinted, textured)) => Ok((materials, untinted, tinted, textured)),
            Err(error) => {
                materials.destroy(device);
                Err(error)
            }
        }
    }

    /// Fills the three rows and returns the ids an instance carries.
    ///
    /// Split out of [`ForwardRenderer::build_materials`] only so the table can
    /// be released on a failure without the borrow that filling it takes, which
    /// is the same shape [`ForwardRenderer::residents`] has.
    ///
    /// **The three rows are one row and two single-column edits of it**, which
    /// is what makes each column's evidence its own. The tinted row differs
    /// from the untinted one in its factor and nothing else; the textured row
    /// differs from it in its page layer and nothing else. A row that changed
    /// both would be a frame in which neither column could be told from the
    /// other.
    fn material_rows(
        device: &dyn Device,
        materials: &mut MaterialTable,
    ) -> Result<(u32, u32, u32), HalError> {
        // The layer is named rather than left to `UNTINTED`'s own zero: this
        // module owns the page, so it is the one that has to say which layer is
        // the white one, and the two agreeing is a fact worth being able to see
        // at the call site.
        let untinted = materials.insert(
            device,
            &mesh::GpuMaterial {
                base_color_texture: UNTEXTURED_LAYER,
                ..mesh::GpuMaterial::UNTINTED
            },
        )?;
        let tinted = materials.insert(
            device,
            &mesh::GpuMaterial {
                base_color: PYRAMID_TINT,
                ..mesh::GpuMaterial::UNTINTED
            },
        )?;
        let textured = materials.insert(
            device,
            &mesh::GpuMaterial {
                base_color_texture: CHECKER_LAYER,
                ..mesh::GpuMaterial::UNTINTED
            },
        )?;
        // A table this size cannot have refused any of the handles, so all
        // three resolve — but the ids are asked for rather than assumed,
        // because the number an instance carries is this one and nothing else
        // knows it.
        match (
            materials.index(untinted),
            materials.index(tinted),
            materials.index(textured),
        ) {
            (Some(untinted), Some(tinted), Some(textured)) => Ok((untinted, tinted, textured)),
            _ => Err(HalError::Backend(
                "a material inserted into an empty table did not resolve".to_string(),
            )),
        }
    }

    /// Creates the instance pool and puts the cube in it.
    ///
    /// Self-cleaning for the same reason [`ForwardRenderer::build_geometry`] is:
    /// the pool is not the rollback's until this has returned.
    ///
    /// **The cube only.** An instance in the pool is an object in the scene now
    /// that the cull pass decides what draws, so the pyramid arrives when a
    /// caller asks for it with [`ForwardRenderer::set_pyramid`] and not before —
    /// which is what keeps the frame every sample draws the cube alone.
    ///
    /// The transform is left at [`Mat4::IDENTITY`], and **the instance carries
    /// its own mesh id from here on**: that is what the vertex stage resolves
    /// its geometry through, so an instance written without one would draw the
    /// mesh at table entry 0. [`ForwardRenderer::begin_frame`] rewrites the
    /// transform before the first draw and writes the id back with it.
    ///
    /// It carries its **material** id for the same reason it carries its mesh
    /// id: a row nobody wrote shades black, so an instance that named one by
    /// omission would be an invisible object rather than an untinted one.
    ///
    /// The sector id stays zero because nothing reads it yet — see
    /// [`crcbl_shaders::mesh::GpuInstance`], which is the field that is still
    /// reserved.
    ///
    /// The instance's index is not asserted to be anything in particular. It
    /// used to be — the cube had to land at 0, because a draw could address no
    /// other one — and nothing on the CPU names an instance index at all now
    /// that the cull pass writes the list a draw walks.
    fn build_instances(
        device: &dyn Device,
        cube_mesh: u32,
        material: u32,
    ) -> Result<(InstancePool, InstanceHandle), HalError> {
        let mut instances = InstancePool::new(
            device,
            &InstancePoolDesc {
                label: Some("forward instances"),
                capacity: POOL_INSTANCE_CAPACITY,
                frames_in_flight: FRAMES_IN_FLIGHT,
            },
        )?;
        match instances.insert(&mesh::GpuInstance {
            transform: Mat4::IDENTITY.to_cols_array(),
            mesh: cube_mesh,
            material,
            ..mesh::GpuInstance::default()
        }) {
            Ok(cube) => Ok((instances, cube)),
            Err(error) => {
                instances.destroy(device);
                Err(error.into())
            }
        }
    }

    /// Uploads every resident mesh and returns their mesh ids — **only** once
    /// the transfers have completed.
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
    ///
    /// The open box is third and is here for the same shape of reason one layer
    /// down: it is the only resident that is **more than one cluster**, so it is
    /// the only one whose mesh-shader dispatch has a cluster at a non-zero
    /// `Meshlet::vertex_offset` *within* one mesh. See
    /// [`crcbl_shaders::meshlet::open_box_clusters`].
    fn residents(
        device: &dyn Device,
        queue: QueueHandle,
        pool: &mut MeshPool,
    ) -> Result<(u32, u32, u32), MeshPoolError> {
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
        let open_box = pool.upload(
            device,
            queue,
            "open box",
            &mesh::open_box_vertex_bytes(),
            &mesh::open_box_indices(),
        )?;
        pool.flush(device)?;
        // The table index is the only number that leaves here — where the
        // geometry actually is reaches the GPU through the mesh table, and the
        // draw arguments are built from that table by a shader. `MeshPool::mesh`
        // is still asked, because it is the call that refuses a mesh whose
        // upload has not completed, and an id for a mesh that is not there yet
        // would be a draw of nothing.
        let resolve = |handle| match (pool.mesh(handle), pool.table_index(handle)) {
            (Some(_), Some(id)) => Ok(id),
            _ => Err(MeshPoolError::NotResident { handle }),
        };
        Ok((resolve(cube)?, resolve(pyramid)?, resolve(open_box)?))
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
    /// It also writes this frame's **cull parameters** — the camera's frustum,
    /// extracted from the very same view-projection the uniform block carries,
    /// so the pass that decides what is on screen and the pass that draws it
    /// cannot be looking at two different cameras — and zeroes the counters the
    /// two compute passes only ever add to. See [`crate::draw_gen`] on why those
    /// zeroes come from the host.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the uniform buffer, the instance buffer or any of the
    /// draw-generation buffers could not be written.
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
                // A `set` writes the whole record, so the mesh and material ids
                // are written with it or the cube resolves to entry 0 of each
                // by accident — one of which is a mesh and the other a colour.
                mesh: self.cube_mesh,
                material: self.untinted_material,
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
        // One matrix, used twice. Recomputing it for the frustum below would be
        // two chances to pass a different aspect ratio, and the failure that
        // produces — geometry culled against a camera the frame does not draw
        // with — is invisible until something at the edge of the screen
        // disappears.
        let view_projection = camera.view_projection(aspect);
        let uniforms = mesh::FrameUniforms {
            view_proj: view_projection.to_cols_array(),
            camera_position: camera.eye.extend(1.0).to_array(),
            light_direction: direction.extend(0.0).to_array(),
            light_color: light.color.extend(0.0).to_array(),
            ambient: light.ambient.extend(0.0).to_array(),
        };
        device.write_buffer(self.uniforms[self.frame], 0, &uniforms.to_bytes())?;

        self.draws.begin_frame(
            device,
            self.frame,
            &Frustum::from_view_projection(view_projection),
            // Every element the pool has ever handed out, not its live count: a
            // removed instance leaves a hole and the live ones above it still
            // have to be tested. `InstancePool::slot_count` carries the
            // difference.
            self.instances.slot_count(),
        )
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
    /// **`None` removes the instance rather than skipping a draw**, because the
    /// frame no longer records a draw per object: the pass draws whatever the
    /// cull pass kept, and an instance left in the pool at the origin would be
    /// kept. [`InstancePool::remove`] clears the record's live bit, which is
    /// what `cull.slang` asks before it reads anything else — so hiding the
    /// pyramid and culling it off screen take the same path out of the frame.
    ///
    /// Takes effect at the next [`ForwardRenderer::begin_frame`], which is what
    /// uploads the change.
    pub fn set_pyramid(&mut self, model: Option<Mat4>) {
        let instance = model.map(|model| mesh::GpuInstance {
            transform: model.to_cols_array(),
            // Without this the pyramid's instance names table entry 0, which is
            // the cube — and it would draw a second cube here rather than
            // nothing, which is the failure the whole second resident exists to
            // make visible.
            mesh: self.pyramid_mesh,
            material: self.untinted_material,
            ..mesh::GpuInstance::default()
        });
        place(
            &mut self.instances,
            &mut self.pyramid_instance,
            instance.as_ref(),
            "the pyramid",
        );
    }

    /// Puts a **second instance of the pyramid mesh** in the frame at `model`,
    /// shaded through a material of its own, or takes it back out with `None`.
    ///
    /// **This is what makes `GpuInstance::material` observable**, and it is why
    /// it is here at all. The two pyramids carry the same mesh id, the same
    /// orientation and the same size; the *only* field they differ in is the
    /// material, so a frame in which they are the same colour is a frame where
    /// the id indexed nothing. That is a claim no single-material scene can
    /// make — a tint applied to the one pyramid would pass just as well if the
    /// shader keyed the colour off the mesh id.
    ///
    /// [`ForwardRenderer::set_pyramid`]'s sibling in every other respect: off
    /// by default, `None` removes the instance rather than skipping a draw, and
    /// the change takes effect at the next [`ForwardRenderer::begin_frame`].
    /// See that method for why each of those is what it is.
    pub fn set_tinted_pyramid(&mut self, model: Option<Mat4>) {
        let instance = model.map(|model| mesh::GpuInstance {
            transform: model.to_cols_array(),
            mesh: self.pyramid_mesh,
            material: self.tinted_material,
            ..mesh::GpuInstance::default()
        });
        place(
            &mut self.instances,
            &mut self.tinted_pyramid_instance,
            instance.as_ref(),
            "the tinted pyramid",
        );
    }

    /// Puts a **third instance of the pyramid mesh** in the frame at `model`,
    /// shaded through a material whose only difference from
    /// [`ForwardRenderer::set_pyramid`]'s is its base-colour page layer, or
    /// takes it back out with `None`.
    ///
    /// **This is what makes `GpuMaterial::base_color_texture` observable**, and
    /// it is why it is here at all. It is [`ForwardRenderer::set_tinted_pyramid`]'s
    /// argument moved one column along: that pair's two rows differ in their
    /// factor and this pair's differ in their texture, so each column has a pair
    /// of its own and neither can be mistaken for the other. A frame in which
    /// this pyramid and the plain one are the same picture is a frame where the
    /// layer index indexed nothing — and because the layer is four unequal
    /// texels rather than a flat colour, it is also a frame that fails if the
    /// texture coordinate never reached the fragment stage.
    ///
    /// [`ForwardRenderer::set_pyramid`]'s sibling in every other respect: off
    /// by default, `None` removes the instance rather than skipping a draw, and
    /// the change takes effect at the next [`ForwardRenderer::begin_frame`].
    pub fn set_textured_pyramid(&mut self, model: Option<Mat4>) {
        let instance = model.map(|model| mesh::GpuInstance {
            transform: model.to_cols_array(),
            mesh: self.pyramid_mesh,
            material: self.textured_material,
            ..mesh::GpuInstance::default()
        });
        place(
            &mut self.instances,
            &mut self.textured_pyramid_instance,
            instance.as_ref(),
            "the textured pyramid",
        );
    }

    /// Puts the pool's third mesh — the open box — in the frame at `model`, or
    /// takes it back out with `None`.
    ///
    /// **This is what makes a cluster at a non-zero `vertex_offset` observable
    /// in a rendered frame**, and it is why the mesh is here at all. The cube
    /// and the pyramid are one cluster each, so on
    /// [`GeometryPath::MeshShader`] every workgroup they draw reads cluster
    /// zero of its mesh and a run that starts at zero; the open box is five
    /// clusters, one per face, and four of them are not. A mesh stage that
    /// dropped the offset draws the first face five times over — the same
    /// triangle count, the same buffer sizes, a different picture.
    ///
    /// It is also the mesh §3.5's per-cluster culling needs: with one cluster
    /// per mesh, rejecting a cluster and rejecting the whole mesh are the same
    /// act and no count can tell them apart.
    ///
    /// [`ForwardRenderer::set_pyramid`]'s sibling in every other respect: off
    /// by default, `None` removes the instance rather than skipping a draw, and
    /// the change takes effect at the next [`ForwardRenderer::begin_frame`].
    pub fn set_open_box(&mut self, model: Option<Mat4>) {
        let instance = model.map(|model| mesh::GpuInstance {
            transform: model.to_cols_array(),
            mesh: self.open_box_mesh,
            material: self.untinted_material,
            ..mesh::GpuInstance::default()
        });
        place(
            &mut self.instances,
            &mut self.open_box_instance,
            instance.as_ref(),
            "the open box",
        );
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
        // The cull dispatch and the draw-argument dispatch, before anything
        // draws. Every barrier between them and the pass below — including the
        // one into `IndirectArgument` — is the graph's, computed from what each
        // pass declares.
        let generated = self
            .draws
            .add_passes(graph, self.frame, self.instances.slot_count());

        let scene_color =
            graph.create_image("scene-color", TransientImageDesc::scene_color(extent));
        let scene_depth =
            graph.create_image("scene-depth", TransientImageDesc::scene_depth(extent));

        let group = self.mesh_groups[self.frame];
        let pipeline = self.mesh_pipeline;
        let layout = self.mesh_pipeline_layout;
        let indices = self.pool.index_buffer();
        let emit = self.emit;
        let stride = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u32;
        // **The mesh path's dispatch bound, and it is a bound rather than an
        // answer.** `draw_mesh_tasks` takes its group counts as arguments —
        // the seam has no indirect form of it — so the y extent is every slot
        // the instance pool has ever handed out, and the mesh stage reads the
        // bucket's real survivor count out of the arguments and emits nothing
        // past it. Same number `begin_frame` gives the cull dispatch, for the
        // same reason: a removed instance leaves a hole and the live ones above
        // it are still in the array.
        let instance_bound = self.instances.slot_count();
        let clusters = self.bucket_clusters;
        // One call per bucket, always — the number the CPU records does not
        // depend on what is in the scene, which is the whole of what §3.3 asks
        // for. An empty bucket's arguments carry an instance count of zero.
        let calls: Vec<(u32, u64, u64)> = self
            .bucket_constants
            .iter()
            .enumerate()
            .map(|(bucket, constant_offset)| {
                let bucket = u32::try_from(bucket)
                    .unwrap_or_else(|_| unreachable!("a fixed table of a few buckets"));
                (
                    *constant_offset,
                    self.draws.args_offset(bucket),
                    self.draws.count_offset(bucket),
                )
            })
            .collect();

        let pass = graph
            .add_render_pass("forward")
            .clear_color(scene_color, SCENE_CLEAR)
            .clear_depth(scene_depth)
            // The buffers the draws come out of. Declaring them is what makes
            // the graph transition them out of the compute pass's
            // `ShaderReadWrite` — the seam calls that the single most important
            // barrier in a GPU-driven frame, and its absence produces
            // "sometimes nothing draws".
            .read_buffer(generated.runs_id);
        let pass = if emit.is_mesh() {
            // **The same arguments, read as data rather than executed.** The
            // mesh path issues no indirect call, so the buffer it needs the
            // barrier for is a shader read — and the per-bucket draw *counts*
            // are not read at all, because nothing here is a draw whose count
            // could come from memory.
            pass.read_buffer(generated.args_id)
        } else {
            pass.use_buffer(generated.args_id, ResourceState::IndirectArgument)
                .use_buffer(generated.counts_id, ResourceState::IndirectArgument)
        };
        pass.execute(move |ctx| {
            let encoder = ctx.encoder();
            encoder.bind_graphics_pipeline(pipeline);
            if !emit.is_mesh() {
                // The index pool is bound whole, at offset zero, for every mesh
                // in it: the mesh's place is the draw's first index and its
                // table entry, not a buffer offset. That is what makes one bind
                // enough for the scene P7 puts in here.
                //
                // A mesh pipeline has no index buffer at all — the corner
                // triples come out of the cluster records — so binding one
                // would be a bind no stage could read.
                encoder.bind_index_buffer(indices, 0, IndexFormat::Uint32);
            }
            for (bucket, (constant_offset, args_offset, count_offset)) in
                calls.into_iter().enumerate()
            {
                // The block written at build for this bucket: where its run
                // of surviving instances starts. `SV_InstanceID` walks the
                // run from there, each entry names an instance, the
                // instance names its mesh, and the mesh table says where
                // that mesh's vertices start — none of which the draw call
                // carries. The mesh path's block says the same and three
                // things more; see `meshlet::ClusterDrawConstants`.
                encoder.bind_group(0, group, &[constant_offset], layout);
                match emit {
                    EmitTail::Mesh => {
                        // One workgroup per (cluster, instance slot). Both
                        // extents are upper bounds the CPU knows without
                        // looking at the scene, and a group past either one
                        // emits no vertices — so the picture is the culled
                        // set, exactly as it is on the two tails below.
                        //
                        // A pool with no slots is a dispatch of no workgroups,
                        // which Metal rejects outright rather than treating as
                        // a no-op, so it is not recorded at all.
                        if instance_bound > 0 && clusters[bucket] > 0 {
                            encoder.draw_mesh_tasks(clusters[bucket], instance_bound, 1);
                        }
                    }
                    EmitTail::Count => {
                        encoder.draw_indexed_indirect_count(&DrawIndirectCount {
                            args: generated.args,
                            args_offset,
                            count_buffer: generated.counts,
                            count_offset,
                            // One argument structure per bucket, so this is
                            // the ceiling rather than a guess: the count in
                            // the buffer is zero or one and the GPU decides
                            // which.
                            max_draw_count: 1,
                            stride,
                        });
                    }
                    EmitTail::PerBatch => {
                        encoder.draw_indexed_indirect(&DrawIndirect {
                            args: generated.args,
                            offset: args_offset,
                            // Read the bucket's one structure unconditionally
                            // — a device without a GPU-side count cannot ask
                            // whether there is anything in it, and an
                            // instance count of zero draws nothing anyway.
                            // That is why the two paths are the same picture
                            // and not an approximation of each other.
                            draw_count: 1,
                            stride,
                        });
                    }
                }
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

    /// The cull and draw-argument passes, and the buffers they produce.
    ///
    /// What a caller reads the culling statistics out of — topic 03 §3.6's
    /// delayed readback ring is [`DrawGen::visible_count`] — and what
    /// `crcbl-vk`'s end-to-end suite copies back to check the generated
    /// arguments against the draws this pass used to record itself.
    #[must_use]
    pub const fn draws(&self) -> &DrawGen {
        &self.draws
    }

    /// Which [`GeometryPath`] this renderer was **built for** — not what the
    /// device reports, but what it actually built.
    ///
    /// The two are the same by construction and that is the point of asking the
    /// renderer rather than the device: [`GeometryPath::MeshShader`] here means
    /// `build` created a mesh pipeline out of `mesh_cluster.slang` and its
    /// cluster buffers, and created **no** raster pipeline for the pass to fall
    /// back to. So a frame this renderer drew came out of a mesh stage, and a
    /// silent degradation is not a state it has — which is the claim a golden
    /// image on its own could never make, because a fall-through draws the same
    /// picture.
    #[must_use]
    pub const fn geometry_path(&self) -> GeometryPath {
        match self.emit {
            EmitTail::Mesh => GeometryPath::MeshShader,
            EmitTail::Count => GeometryPath::IndirectCount,
            EmitTail::PerBatch => GeometryPath::IndirectPerBatch,
        }
    }

    /// Which frame-in-flight slot the last [`ForwardRenderer::begin_frame`]
    /// rotated to, which is the slot every per-frame buffer is indexed by.
    #[must_use]
    pub const fn frame(&self) -> usize {
        self.frame
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
        self.base_color_page.destroy(device);
        device.destroy_sampler(self.base_color_sampler);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        device.destroy_buffer(self.draw_constants);
        if let Some(clusters) = self.clusters {
            clusters.destroy(device);
        }
        self.draws.destroy(device);
        self.materials.destroy(device);
        self.instances.destroy(device);
        self.pool.destroy(device);
    }
}

/// Puts `instance` in `slot`, or takes whatever is there back out when it is
/// `None`.
///
/// The body [`ForwardRenderer::set_pyramid`],
/// [`ForwardRenderer::set_tinted_pyramid`] and
/// [`ForwardRenderer::set_textured_pyramid`] share: each holds an optional
/// handle into the same pool and each means the same three things by it —
/// insert when there is nothing there, rewrite when there is, and remove on
/// `None`.
///
/// A free function rather than a method because it takes the pool and the slot
/// as separate borrows, which a `&mut self` method could not: both are fields
/// of the same renderer.
///
/// `what` names the object in the one message this can produce. A pool with
/// room for thousands is full only if a caller filled it, and the failure is
/// logged rather than propagated: neither signature says anything about a pool,
/// and a frame that draws one fewer object is better than a frame loop that
/// stops.
fn place(
    instances: &mut InstancePool,
    slot: &mut Option<InstanceHandle>,
    instance: Option<&mesh::GpuInstance>,
    what: &str,
) {
    let Some(instance) = instance else {
        if let Some(handle) = slot.take() {
            instances.remove(handle);
        }
        return;
    };
    match *slot {
        Some(handle) => {
            instances.set(handle, instance);
        }
        None => match instances.insert(instance) {
            Ok(handle) => *slot = Some(handle),
            Err(error) => log::error!("forward: {what} has no instance slot: {error}"),
        },
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
    use crcbl_hal::{DeviceDesc, Features, Instance, QueueKind};

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

    /// The property the whole arrangement exists for: the pool's second
    /// resident is **not** at base vertex zero, so a frame containing it is a
    /// frame that fails if the base is lost between the pool and the shader.
    ///
    /// Read out of the **mesh table** rather than out of a renderer field,
    /// because the table is where a generated draw's index range comes from:
    /// `draw_gen.slang` builds every argument structure out of these bytes and
    /// nothing on the CPU is consulted.
    ///
    /// Without this the pass is back where it started — one mesh, base 0, and a
    /// base-vertex bug that no picture can show.
    #[test]
    fn the_second_mesh_lands_past_the_first() {
        let (recorder, device, queue) = open();
        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let cube = mesh_entry(&recorder, &renderer, renderer.cube_mesh);
        let pyramid = mesh_entry(&recorder, &renderer, renderer.pyramid_mesh);

        assert_eq!(cube.base_vertex, 0, "the cube is first");
        assert_eq!(
            pyramid.base_vertex as usize,
            crcbl_shaders::mesh::CUBE_VERTEX_COUNT,
            "the pyramid must start where the cube ends, or the pools are not \
             suballocating at all"
        );
        assert_eq!(
            pyramid.index_count as usize,
            crcbl_shaders::mesh::PYRAMID_INDEX_COUNT,
            "and the entry carries the whole range, which is what the draw-argument \
             pass builds its arguments out of"
        );
        // And the two buckets read different constant blocks, or both would walk
        // the same run of instances.
        assert_ne!(
            renderer.bucket_constants[CUBE_BUCKET], renderer.bucket_constants[PYRAMID_BUCKET],
            "each bucket needs a block of its own"
        );
        renderer.destroy(device.as_ref());
    }

    /// **Each bucket's block points at that bucket's own run**, and the runs do
    /// not overlap.
    ///
    /// This is what replaced "the block names an instance": a draw covers
    /// however many instances survived culling, so what it can say for itself is
    /// only where its slice of the survivors begins. Two buckets sharing a base
    /// would draw each other's objects — with the *right* geometry, because the
    /// index range comes from the arguments, which is exactly the kind of
    /// plausible wrong picture a golden image struggles with.
    ///
    /// Read out of the bytes that reached the device rather than out of the
    /// renderer's own fields, because the failure is a block written wrong and
    /// the field would still be right.
    #[test]
    fn each_bucket_walks_its_own_run_of_survivors() {
        let (recorder, device, queue) = open();
        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let blocks = recorder
            .buffer_bytes(renderer.draw_constants)
            .expect("the blocks are live");
        let base_at = |offset: u32| {
            let at = offset as usize;
            u32::from_le_bytes(blocks[at..at + 4].try_into().expect("four bytes"))
        };

        let cube = base_at(renderer.bucket_constants[CUBE_BUCKET]);
        let pyramid = base_at(renderer.bucket_constants[PYRAMID_BUCKET]);
        assert_eq!(cube, 0, "the first bucket's run starts at the beginning");
        assert_eq!(
            pyramid,
            renderer.draws.visible_capacity(),
            "and the second's starts a whole run later — the stride is the \
             capacity, so a bucket that filled up still cannot reach the next"
        );
        renderer.destroy(device.as_ref());
    }

    /// **The two pyramids differ in exactly one field, and it is the material
    /// id — which names a row holding a different colour.**
    ///
    /// The whole of what §3.2's table buys, stated as the two things that have
    /// to be true at once. Placed at the *same* transform, so "differ only in
    /// material" is literal rather than nearly: same mesh id, same matrix, same
    /// flags, and a `GpuInstance` comparison that would fail on any other
    /// field. And the ids they carry name rows the device actually holds —
    /// asserted on the bytes in the buffer, because a table the renderer agrees
    /// with itself about is not a table a shader can read.
    ///
    /// The two rows are asserted to differ, or the pair would be evidence of
    /// nothing: two instances naming two rows of the same colour draw the same
    /// picture whether or not either id was ever used.
    #[test]
    fn the_two_pyramids_differ_only_in_their_material() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let at = Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0));
        renderer.set_pyramid(Some(at));
        renderer.set_tinted_pyramid(Some(at));

        let instance = |handle: Option<InstanceHandle>| {
            renderer
                .instances
                .get(handle.expect("the instance was inserted"))
                .expect("and it is live")
        };
        let plain = instance(renderer.pyramid_instance);
        let tinted = instance(renderer.tinted_pyramid_instance);
        assert_ne!(
            plain.material, tinted.material,
            "the two pyramids must not share a material row"
        );
        assert_eq!(
            mesh::GpuInstance {
                material: tinted.material,
                ..plain
            },
            tinted,
            "the two instances must differ in the material id and in nothing else"
        );

        let bytes = recorder
            .buffer_bytes(renderer.materials.buffer())
            .expect("the table is live");
        let row = |index: u32| {
            let at = index as usize * crcbl_shaders::mesh::MATERIAL_STRIDE;
            mesh::GpuMaterial::from_bytes(
                bytes[at..at + crcbl_shaders::mesh::MATERIAL_STRIDE]
                    .try_into()
                    .expect("one row"),
            )
        };
        assert_eq!(
            row(plain.material),
            mesh::GpuMaterial::UNTINTED,
            "the plain pyramid's row must be the factor that changes nothing"
        );
        assert_eq!(
            row(tinted.material),
            mesh::GpuMaterial {
                base_color: PYRAMID_TINT,
                ..mesh::GpuMaterial::UNTINTED
            },
            "the tinted pyramid's row must be the tint"
        );
        assert_ne!(
            row(plain.material),
            row(tinted.material),
            "two rows holding the same colour would make the pair prove nothing"
        );
        // And the tint is the *only* thing that differs between them: a row that
        // also changed its page layer would make this pair evidence about two
        // columns at once, which is what the third pyramid exists to avoid.
        assert_eq!(
            row(plain.material).base_color_texture,
            row(tinted.material).base_color_texture,
            "the factor pair must sample the same page layer"
        );

        // And the second pyramid leaves the frame the way the first does: the
        // instance is removed, not a draw skipped.
        renderer.set_tinted_pyramid(None);
        assert!(
            renderer.tinted_pyramid_instance.is_none(),
            "the instance must be given back, or an object nobody asked for stays in the scene"
        );

        renderer.destroy(device.as_ref());
    }

    /// **The same claim one column along: the plain and textured pyramids
    /// differ in exactly one field, and the rows it names differ in exactly one
    /// column — the base-colour page layer.**
    ///
    /// [`the_two_pyramids_differ_only_in_their_material`]'s argument for §3.2's
    /// *texture indices* rather than its *factors*. The factors of the two rows
    /// are asserted **equal** here, which is the half that makes the pair
    /// evidence about the texture at all: a textured row that also carried a
    /// tint would draw a different picture for a reason the frame could not
    /// distinguish from the tinted pyramid's.
    ///
    /// The layer numbers are asserted against the page the renderer actually
    /// uploaded, so a row naming a layer that does not exist is caught here
    /// rather than as an out-of-range sample on a device.
    #[test]
    fn the_textured_pyramid_differs_only_in_its_page_layer() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let at = Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0));
        renderer.set_pyramid(Some(at));
        renderer.set_textured_pyramid(Some(at));

        let instance = |handle: Option<InstanceHandle>| {
            renderer
                .instances
                .get(handle.expect("the instance was inserted"))
                .expect("and it is live")
        };
        let plain = instance(renderer.pyramid_instance);
        let textured = instance(renderer.textured_pyramid_instance);
        assert_ne!(
            plain.material, textured.material,
            "the two pyramids must not share a material row"
        );
        assert_eq!(
            mesh::GpuInstance {
                material: textured.material,
                ..plain
            },
            textured,
            "the two instances must differ in the material id and in nothing else"
        );

        let bytes = recorder
            .buffer_bytes(renderer.materials.buffer())
            .expect("the table is live");
        let row = |index: u32| {
            let at = index as usize * crcbl_shaders::mesh::MATERIAL_STRIDE;
            mesh::GpuMaterial::from_bytes(
                bytes[at..at + crcbl_shaders::mesh::MATERIAL_STRIDE]
                    .try_into()
                    .expect("one row"),
            )
        };
        assert_eq!(
            row(plain.material).base_color,
            row(textured.material).base_color,
            "the texture pair must share a factor, or the pair is evidence about both columns"
        );
        assert_eq!(row(plain.material).base_color_texture, UNTEXTURED_LAYER);
        assert_eq!(row(textured.material).base_color_texture, CHECKER_LAYER);
        assert_ne!(
            row(plain.material),
            row(textured.material),
            "two rows naming the same layer would make the pair prove nothing"
        );
        // The layers those numbers name are layers the page has, and they hold
        // different texels — a page whose two layers were the same image would
        // pass every assertion above and draw one picture.
        assert_ne!(UNTEXTURED_TEXELS, CHECKER_TEXELS);
        for layer in [UNTEXTURED_LAYER, CHECKER_LAYER] {
            assert!(
                (layer as usize) < PAGE_LAYERS.len(),
                "layer {layer} is past the end of a {}-layer page, which is an out-of-range                  sample nothing below the seam would report",
                PAGE_LAYERS.len()
            );
        }

        renderer.set_textured_pyramid(None);
        assert!(
            renderer.textured_pyramid_instance.is_none(),
            "the instance must be given back, or an object nobody asked for stays in the scene"
        );

        renderer.destroy(device.as_ref());
    }

    /// **The frame records two compute dispatches and exactly one indirect draw
    /// per bucket — whatever is in the scene.**
    ///
    /// The property topic 03 opens with, checked on the recorded command stream:
    /// adding the pyramid adds an instance and changes no command. A pass that
    /// had kept a draw per object would record one command here and two there,
    /// which is what this used to assert and what the indirect path replaced.
    ///
    /// It also pins the arguments each call names: bucket `n` reads the `n`-th
    /// argument structure and the `n`-th count, at the stride every API fixed.
    /// Two calls pointing at one structure is a frame that draws one object
    /// twice.
    #[test]
    fn the_frame_records_one_indirect_call_per_bucket_whatever_the_scene_holds() {
        use crcbl_hal::null::Command;

        for pyramid in [None, Some(Mat4::from_translation(Vec3::X))] {
            let (recorder, device, queue) = open();
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("built");
            renderer.set_pyramid(pyramid);
            let frame = frame(device.as_ref(), &mut renderer, queue);

            let stride = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u32;
            let expected: Vec<(u32, u64, u64)> = (0..BUCKET_COUNT)
                .map(|bucket| {
                    (
                        renderer.bucket_constants[bucket as usize],
                        u64::from(bucket) * u64::from(stride),
                        u64::from(bucket) * 4,
                    )
                })
                .collect();

            // The dynamic offset last bound before each draw, paired with the
            // offsets that draw read its arguments and its count from.
            let mut offset = None;
            let mut seen: Vec<(u32, u64, u64)> = Vec::new();
            let mut dispatches = 0;
            for command in recorder.commands() {
                match command {
                    Command::BindGroup {
                        slot: 0,
                        dynamic_offsets,
                        ..
                    } => offset = dynamic_offsets.first().copied(),
                    Command::Dispatch { .. } => dispatches += 1,
                    Command::DrawIndexedIndirectCount(draw) => {
                        assert_eq!(draw.stride, stride, "sizeof(VkDrawIndexedIndirectCommand)");
                        assert_eq!(
                            draw.max_draw_count, 1,
                            "one argument structure per bucket, so one is the ceiling"
                        );
                        seen.push((
                            offset.expect("a draw is preceded by its constant block"),
                            draw.args_offset,
                            draw.count_offset,
                        ));
                    }
                    Command::DrawIndexed { .. } => {
                        panic!("the forward pass must not record a CPU-counted draw any more")
                    }
                    _ => {}
                }
            }
            assert_eq!(
                dispatches, 3,
                "the clearing pass, the cull pass and the draw-argument pass, in front of \
                 the draws"
            );
            assert_eq!(
                seen, expected,
                "with pyramid = {pyramid:?}, the pass must record one indirect call per \
                 bucket, each pointing at its own arguments, its own count and its own \
                 block"
            );

            frame.finish(device.as_ref(), renderer);
        }
    }

    /// **The two `GeometryPath` arms record different calls and nothing else
    /// differs.**
    ///
    /// A device with a GPU-side count takes it from GPU memory; one without —
    /// Metal, whose API has multi-draw-indirect and no count buffer — reads each
    /// bucket's one argument structure unconditionally. Same buckets, same
    /// arguments, same block: the tail is the only thing that moves, which is
    /// what `docs/plan/03-gpu-driven-rendering.md` means by "the lesser path is
    /// a constraint on data layout, not a separate renderer".
    ///
    /// Both presets are run here rather than only the one this machine has,
    /// because the arm a device never selects is the arm nothing tests.
    #[test]
    fn each_geometry_path_records_its_own_indirect_call() {
        use crcbl_hal::null::Command;

        for (preset, path, counted) in [
            (
                NullInstance::gpu_driven(),
                GeometryPath::IndirectCount,
                true,
            ),
            (
                NullInstance::portable(),
                GeometryPath::IndirectPerBatch,
                false,
            ),
        ] {
            let recorder = Recorder::new();
            let instance = preset.with_recorder(recorder.clone());
            let adapter = instance.adapters().remove(0);
            assert_eq!(
                adapter.caps.geometry_path(),
                path,
                "the preset must select the path this arm is about"
            );
            // `DeviceDesc::for_adapter` requires a timeline semaphore, which the
            // portable preset does not have; the forward pass does not need one
            // either, so this asks for what it uses. The optional set matters:
            // a device is opened with the features it *asked* for, and the path
            // is derived from those rather than from the adapter's.
            let device = instance
                .create_device(&DeviceDesc {
                    label: None,
                    adapter: adapter.id,
                    required_features: Features::COMPUTE,
                    optional_features: Features::GPU_DRIVEN,
                    compatible_surface: None,
                })
                .expect("the null backend always opens");
            assert_eq!(
                device.caps().geometry_path(),
                path,
                "and the opened device still selects it"
            );
            let queue = device.queue(QueueKind::Graphics).expect("always present");
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("built");
            let frame = frame(device.as_ref(), &mut renderer, queue);

            let mut counted_calls = 0;
            let mut per_batch_calls = 0;
            for command in recorder.commands() {
                match command {
                    Command::DrawIndexedIndirectCount(_) => counted_calls += 1,
                    Command::DrawIndexedIndirect(draw) => {
                        assert_eq!(
                            draw.draw_count, 1,
                            "one argument structure per bucket, read unconditionally"
                        );
                        per_batch_calls += 1;
                    }
                    _ => {}
                }
            }
            let (expected_counted, expected_per_batch) = if counted {
                (BUCKET_COUNT, 0)
            } else {
                (0, BUCKET_COUNT)
            };
            assert_eq!(
                (counted_calls, per_batch_calls),
                (expected_counted, expected_per_batch),
                "{path:?} must record one call of its own kind per bucket and none of the other"
            );

            frame.finish(device.as_ref(), renderer);
        }
    }

    /// **A device selecting [`GeometryPath::MeshShader`] draws through a mesh
    /// pipeline, and records no indirect draw and no index-buffer bind at all.**
    ///
    /// The three halves are one claim. That it dispatched says the path is
    /// wired; that it recorded *no* `DrawIndexedIndirect` of either kind says it
    /// did not quietly fall through to a tail that draws the same picture; and
    /// that it bound no index buffer says the geometry really came out of the
    /// cluster records rather than out of the index pool. Until 2026-08 this
    /// device degraded and logged that it had, which is exactly the shape —
    /// "not implemented" arriving as a passing frame — the three together rule
    /// out.
    ///
    /// The dispatch extents are checked too: x is the bucket's mesh's cluster
    /// count, y the instance pool's slot count. A dispatch of `(1, 1, 1)` would
    /// draw one cluster of one instance and look perfectly healthy on a scene
    /// with one of each.
    #[test]
    fn a_mesh_shader_device_draws_through_a_mesh_pipeline() {
        use crcbl_hal::null::Command;

        let recorder = Recorder::new();
        let caps = crcbl_hal::DeviceCaps {
            features: Features::GPU_DRIVEN | Features::MESH_SHADER,
            limits: crcbl_hal::Limits::desktop(),
        };
        let instance = NullInstance::new(caps).with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        assert_eq!(
            adapter.caps.geometry_path(),
            GeometryPath::MeshShader,
            "a device with mesh shaders selects them, which is the case this is about"
        );
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::COMPUTE,
                // Asked for, because a device reports what it enabled: leaving
                // mesh shaders out of the optional set would open a device that
                // selects an indirect path outright and test nothing.
                optional_features: Features::GPU_DRIVEN | Features::MESH_SHADER,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        assert_eq!(
            device.caps().geometry_path(),
            GeometryPath::MeshShader,
            "and the opened device still selects it"
        );
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        assert_eq!(
            renderer.geometry_path(),
            GeometryPath::MeshShader,
            "the renderer must have *built* the mesh path, not merely been offered it"
        );
        let slots = renderer.instances.slot_count();
        let clusters = renderer.bucket_clusters;
        let frame = frame(device.as_ref(), &mut renderer, queue);

        let mut dispatched = Vec::new();
        let mut index_binds = 0;
        for command in recorder.commands() {
            match command {
                Command::DrawMeshTasks { x, y, z } => dispatched.push((x, y, z)),
                Command::DrawIndexedIndirectCount(_) | Command::DrawIndexedIndirect(_) => {
                    panic!(
                        "the mesh path recorded an indirect draw, which is the silent \
                         fall-through this test exists to rule out"
                    )
                }
                Command::BindIndexBuffer { .. } => index_binds += 1,
                _ => {}
            }
        }
        assert_eq!(
            dispatched,
            vec![
                (clusters[CUBE_BUCKET], slots, 1),
                (clusters[PYRAMID_BUCKET], slots, 1),
                (clusters[OPEN_BOX_BUCKET], slots, 1)
            ],
            "one dispatch per bucket, sized to that bucket's mesh's clusters and to \
             every instance slot"
        );
        assert!(
            clusters.iter().all(|&count| count > 0),
            "a bucket with no clusters dispatches nothing, so this comparison would \
             pass against an empty pool: {clusters:?}"
        );
        // **The x extents are not all the same number**, which is what makes
        // the comparison above a statement about each bucket's own mesh. Two
        // single-cluster meshes would let a dispatch hard-coded to one cluster
        // pass it; the open box is five, one per face.
        assert_eq!(
            clusters[OPEN_BOX_BUCKET] as usize,
            crcbl_shaders::mesh::OPEN_BOX_FACES.len(),
            "the open box dispatches one workgroup per face, and that is the only \
             resident whose cluster count is not one: {clusters:?}"
        );
        assert_eq!(
            index_binds, 0,
            "a mesh pipeline has no index buffer; binding one would mean the geometry \
             came from the index pool after all"
        );

        frame.finish(device.as_ref(), renderer);
    }

    /// **The cull pass is told the camera the frame is drawn with**, and how
    /// many array elements to walk.
    ///
    /// Both halves fail silently: a frustum from a different aspect ratio culls
    /// geometry that is on screen, and an instance count short of the pool's
    /// high-water mark stops testing the instances above a freed slot. Neither
    /// is visible in a picture until something disappears from an edge.
    #[test]
    fn the_cull_pass_gets_this_frame_s_camera_and_every_occupied_slot() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        renderer.set_pyramid(Some(Mat4::from_translation(Vec3::X)));

        let camera = Camera {
            eye: Vec3::new(1.6, 1.2, 2.2),
            ..Camera::default()
        };
        let extent = (256u32, 192u32);
        renderer
            .begin_frame(
                device.as_ref(),
                &camera,
                &DirectionalLight::default(),
                Mat4::IDENTITY,
                extent,
            )
            .expect("write");

        let aspect = extent.0 as f32 / extent.1 as f32;
        let expected = crate::cull::Frustum::from_view_projection(camera.view_projection(aspect));
        let written = recorder
            .buffer_bytes(renderer.draws.cull_params(renderer.frame))
            .expect("this frame's cull parameters are live");
        assert_eq!(
            &written[..crcbl_shaders::cull::PARAMS_SIZE],
            &crcbl_shaders::cull::Params {
                planes: expected.planes.map(|plane| plane.to_array()),
                instance_count: 2,
                capacity: POOL_INSTANCE_CAPACITY,
            }
            .to_bytes()[..],
            "the cull block must carry this frame's own frustum, the pool's whole \
             occupied range, and the visible list's capacity"
        );

        // And the high-water mark is what is written, not the live count: remove
        // the cube and the pyramid above it still has to be tested.
        let cube = renderer.cube_instance;
        renderer.instances.remove(cube);
        renderer
            .begin_frame(
                device.as_ref(),
                &camera,
                &DirectionalLight::default(),
                Mat4::IDENTITY,
                extent,
            )
            .expect("write");
        let written = recorder
            .buffer_bytes(renderer.draws.cull_params(renderer.frame))
            .expect("live");
        assert_eq!(
            u32::from_le_bytes(written[96..100].try_into().expect("four bytes")),
            2,
            "one live instance, two occupied slots — a walk of one would never \
             reach the pyramid"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Every counter both compute passes add to is zeroed by a pass of its
    /// own, before either of them runs.**
    ///
    /// The two shaders only ever increment, so a counter carrying the previous
    /// frame's total makes a bucket's argument structure claim more instances
    /// than its run holds — which draws whatever the run's stale tail happens to
    /// name.
    ///
    /// # This once read the bytes back, and no longer can
    ///
    /// It used to poison all three counters with `0xAB` and assert `begin_frame`
    /// had written zeroes over them — the poison being the whole test, because
    /// the null backend runs no shader and a counter nothing wrote reads as the
    /// zero it was created with. The counters are device-local now, so nothing
    /// on the host writes them and [`Recorder::buffer_bytes`] holds nothing for
    /// them: the zero is a dispatch inside the frame, and a backend that runs no
    /// shader cannot observe it at all.
    ///
    /// So the poison moved to where a shader actually runs — `crcbl-vk`'s
    /// `draw_gen` end-to-end fills all three with a sentinel and reads back the
    /// generated arguments — and what is checkable *here* is the schedule: that
    /// the frame contains the clearing pass, that it writes exactly those three
    /// buffers, and that the graph orders the two accumulating passes after it.
    /// Each of the three counts below goes red if a buffer is dropped from the
    /// clearing pass, and the pass list goes red if the pass leaves the frame.
    #[test]
    fn every_frame_starts_from_zeroed_counters() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        let imported = swapchain_image(device.as_ref());
        // Twice round the ring, because a slot cleared only on its first use is
        // exactly the failure this is about.
        for round in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(
                    device.as_ref(),
                    &Camera::default(),
                    &DirectionalLight::default(),
                    Mat4::IDENTITY,
                    (64, 48),
                )
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, target, (64, 48));
            let pool = crate::TransientPool::new();
            let compiled = graph.compile(&pool).expect("a legal frame");

            let clear = &compiled.passes()[0];
            assert_eq!(
                clear.label(),
                "clear-counters",
                "round {round}: the zero has to be written before anything adds to it"
            );
            let barriers = &clear.barriers().buffers;
            assert_eq!(
                barriers.len(),
                3,
                "round {round}: the survivor count, the arguments and the draw counts, \
                 and nothing else: {barriers:?}"
            );
            assert!(
                barriers
                    .iter()
                    .all(|barrier| barrier.to == ResourceState::ShaderReadWrite),
                "round {round}: every one of them is written by this pass: {barriers:?}"
            );
            // The two the frame leaves as indirect arguments are the arguments
            // and the counts; the survivor count rests in a shader read. That
            // split is what names the three without a handle to compare.
            assert_eq!(
                barriers
                    .iter()
                    .filter(|barrier| barrier.from == ResourceState::IndirectArgument)
                    .count(),
                2,
                "round {round}: out of the state a draw read them in: {barriers:?}"
            );

            let after = |label: &str| {
                compiled
                    .passes()
                    .iter()
                    .find(|pass| pass.label() == label)
                    .unwrap_or_else(|| panic!("round {round}: no `{label}` pass"))
                    .barriers()
                    .buffers
                    .iter()
                    .filter(|barrier| {
                        barrier.from == ResourceState::ShaderReadWrite
                            && barrier.to == ResourceState::ShaderReadWrite
                    })
                    .count()
            };
            assert_eq!(
                after("cull"),
                1,
                "round {round}: the survivor count reaches the cull pass's atomic behind a \
                 barrier on the write that zeroed it"
            );
            assert_eq!(
                after("draw-args"),
                2,
                "round {round}: and so do the arguments and the draw counts"
            );

            // The compiled graph borrows the renderer's pass bodies, so it has
            // to go before the next frame borrows the renderer again.
            drop(compiled);
        }
        renderer.destroy(device.as_ref());
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// **The graph transitions the generated arguments into
    /// [`ResourceState::IndirectArgument`] before the draws read them**, and the
    /// runs into a shader read.
    ///
    /// The seam calls this the single most important barrier in a GPU-driven
    /// frame and says its absence produces "sometimes nothing draws" — which is
    /// exactly the failure a golden image on one driver would not catch. It is
    /// asserted on the compiled graph, which is pure and needs no device.
    #[test]
    fn the_graph_barriers_the_arguments_into_place_before_the_draws() {
        let (recorder, device, queue) = open();
        let mut renderer =
            ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb).expect("built");
        renderer
            .begin_frame(
                device.as_ref(),
                &Camera::default(),
                &DirectionalLight::default(),
                Mat4::IDENTITY,
                (64, 48),
            )
            .expect("write");

        let imported = swapchain_image(device.as_ref());
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        renderer.add_passes(&mut graph, target, (64, 48));
        let pool = crate::TransientPool::new();
        let compiled = graph.compile(&pool).expect("a legal frame");

        let passes: Vec<String> = compiled
            .passes()
            .iter()
            .map(|pass| pass.label().to_string())
            .collect();
        assert_eq!(
            passes,
            ["clear-counters", "cull", "draw-args", "forward", "tonemap"],
            "the three compute passes come first, and in that order"
        );

        let forward = compiled
            .passes()
            .iter()
            .find(|pass| pass.label() == "forward")
            .expect("the pass list above");
        let into_indirect = forward
            .barriers()
            .buffers
            .iter()
            .filter(|barrier| barrier.to == ResourceState::IndirectArgument)
            .count();
        assert_eq!(
            into_indirect,
            2,
            "the arguments and the count, out of the compute pass's writes and into \
             the state a draw reads them in: {:?}",
            forward.barriers().buffers
        );
        assert!(
            forward.barriers().buffers.iter().any(|barrier| {
                barrier.from == ResourceState::ShaderReadWrite
                    && barrier.to == ResourceState::ShaderRead
            }),
            "and the runs, which the vertex stage reads: {:?}",
            forward.barriers().buffers
        );

        let draw_args = compiled
            .passes()
            .iter()
            .find(|pass| pass.label() == "draw-args")
            .expect("the pass list above");
        assert!(
            draw_args.barriers().buffers.iter().any(|barrier| {
                barrier.from == ResourceState::ShaderReadWrite
                    && barrier.to == ResourceState::ShaderRead
            }),
            "and the visible list is ordered after the cull pass wrote it: {:?}",
            draw_args.barriers().buffers
        );

        // The compiled graph borrows the renderer's pass bodies, so it has to go
        // before the renderer does.
        drop(compiled);
        renderer.destroy(device.as_ref());
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.assert_valid();
    }

    /// One mesh-table entry, decoded from the bytes that reached the device.
    fn mesh_entry(
        recorder: &Recorder,
        renderer: &ForwardRenderer,
        id: u32,
    ) -> crcbl_shaders::mesh::GpuMesh {
        let table = recorder
            .buffer_bytes(renderer.pool.table_buffer())
            .expect("the table is live");
        let at = id as usize * crcbl_shaders::mesh::MESH_ENTRY_STRIDE;
        crcbl_shaders::mesh::GpuMesh::from_bytes(
            table[at..at + crcbl_shaders::mesh::MESH_ENTRY_STRIDE]
                .try_into()
                .expect("one whole entry"),
        )
    }

    /// One frame recorded end to end: `begin_frame`, the graph, and an encoder
    /// the commands land in.
    ///
    /// Returned rather than cleaned up here because the caller reads the
    /// recorder afterwards, and everything below must outlive that.
    struct Frame {
        imported: ImportedImage,
        pool: crate::TransientPool,
        commands: crcbl_hal::CommandBufferHandle,
    }

    impl Frame {
        /// Releases the frame, the renderer and the image it drew into.
        fn finish(self, device: &dyn Device, renderer: ForwardRenderer) {
            device.destroy_command_buffer(self.commands);
            renderer.destroy(device);
            let mut pool = self.pool;
            pool.destroy(device);
            device.destroy_image_view(self.imported.view);
            device.destroy_image(self.imported.image);
        }
    }

    fn frame(device: &dyn Device, renderer: &mut ForwardRenderer, queue: QueueHandle) -> Frame {
        renderer
            .begin_frame(
                device,
                &Camera::default(),
                &DirectionalLight::default(),
                Mat4::IDENTITY,
                (64, 48),
            )
            .expect("write");
        let imported = swapchain_image(device);
        let mut graph = crate::RenderGraph::new(queue);
        let target = graph.import_image("target", imported);
        renderer.add_passes(&mut graph, target, (64, 48));
        let mut pool = crate::TransientPool::new();
        let compiled = graph.compile(&pool).expect("a legal frame");
        let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("forward frame"),
            queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), None)
            .expect("the graph executed");
        let commands = encoder.finish().expect("recording succeeded");
        Frame {
            imported,
            pool,
            commands,
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
