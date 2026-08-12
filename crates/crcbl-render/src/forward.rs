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
    GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageSubresourceRange, ImageType,
    ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, IndexFormat, LoadOp, MemoryLocation,
    MeshPipelineDesc, MultisampleState, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState,
    QueueHandle, Rect2d, ResourceState, SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle,
    ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp, Viewport,
};
use crcbl_shaders::{MESH, MESH_CLUSTER, Stage, TONEMAP, level_select, mesh};
use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, DirectionalLight};
use crate::cluster_pool::{ClusterPool, ClusterRange, PooledMesh};
use crate::cull::Frustum;
use crate::draw_gen::{DrawGen, DrawGenDesc};
use crate::graph::{BufferId, ImageId, ImportedBuffer, ImportedImage, RenderGraph};
use crate::instance_pool::{InstanceHandle, InstancePool, InstancePoolDesc};
use crate::material_table::{MaterialTable, MaterialTableDesc};
use crate::mesh_pool::{MeshPool, MeshPoolDesc, MeshPoolError};
use crate::shadow::{self, Cascades};
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

/// The bind-group slot the sun's shadow atlas is read through.
///
/// **15, not 9**, and the gap is `mesh_cluster.slang`'s: bindings 9 to 14 belong
/// to the mesh-shader path, and `mesh.slang`'s `fragmentMain` — which declares
/// this one — is the fragment stage of *that* pipeline too. A binding number is
/// a property of the shader source, so the two files have to agree even where
/// only one of them uses the slot.
const SHADOW_ATLAS_BINDING: u32 = 15;

/// The bind-group slot the shadow atlas's **comparison** sampler is bound to.
const SHADOW_SAMPLER_BINDING: u32 = 16;

/// The cube's bucket, the pyramid's and the open box's. Named rather than
/// written as numbers where the bucket table is filled in.
///
/// One bucket per resident mesh, because an argument structure's index range is
/// per draw and instances of different meshes cannot share one. See
/// [`crate::draw_gen`] on what a bucket is and what a longer key would buy. How
/// many the table holds altogether is [`ForwardRenderer::bucket_count`], and it
/// is not the same number on every geometry path — see [`DUNES_BUCKET`].
const CUBE_BUCKET: usize = 0;
const PYRAMID_BUCKET: usize = 1;
const OPEN_BOX_BUCKET: usize = 2;

/// The dunes patch's first bucket, and **how many it has depends on the
/// geometry path** — which is the one place in this renderer the two differ in
/// shape rather than in the call they record.
///
/// On [`EmitTail::Mesh`] it is a single bucket for the whole DAG: a bucket is a
/// mesh's run of clusters, and the point of `docs/plan/25-lod.md`'s per-cluster
/// selection is that one dispatch covers several levels at once, so the run is
/// every level's clusters end to end and the bucket's mesh id is level 0's.
///
/// On the two indirect tails it is one bucket **per level**, because there the
/// same plan takes a uniform cut and a level is drawn as an ordinary index
/// range: a DAG's levels are separate mesh table entries, `draw_gen.slang`
/// selects one per instance, and a bucket is what an indexed draw of one index
/// range *is*. See [`ForwardRenderer::dunes_level_buckets`].
const DUNES_BUCKET: usize = 3;

/// The pixel budget `docs/plan/25-lod.md`'s descent compares a group's projected
/// error against, unless a caller sets another with
/// [`ForwardRenderer::set_lod_error_budget`].
///
/// One pixel, because that is what the metric is *for*: a level's error is how
/// far its simplification may have moved the surface, so a budget of a pixel is
/// the point at which a level change stops being something a viewer can see.
/// The plan's "correct thresholds make pops sub-pixel by definition" is this
/// number.
const LOD_ERROR_BUDGET: f32 = 1.0;

/// Bytes one bucket's draw-constant block occupies, whichever of the two blocks
/// it holds.
///
/// The larger of [`mesh::DRAW_CONSTANTS_SIZE`] and
/// [`CLUSTER_DRAW_CONSTANTS_SIZE`](crcbl_shaders::meshlet::CLUSTER_DRAW_CONSTANTS_SIZE),
/// because which one a bucket holds is the geometry path's decision and the
/// buffer, its dynamic stride and the range the bind group names are all fixed
/// before that decision reaches them.
///
/// **The bound range is the half that bites.** A range sized for the smaller
/// block leaves the larger one's tail outside it, and a uniform read past the
/// bound range is not a fault — it is a zero. Sized at sixteen bytes, the mesh
/// path's `group_stride` read back as zero and every instance descended against
/// instance zero's LOD state.
const DRAW_CONSTANTS_BLOCK: u64 =
    if mesh::DRAW_CONSTANTS_SIZE > crcbl_shaders::meshlet::CLUSTER_DRAW_CONSTANTS_SIZE {
        mesh::DRAW_CONSTANTS_SIZE as u64
    } else {
        crcbl_shaders::meshlet::CLUSTER_DRAW_CONSTANTS_SIZE as u64
    };

/// How far below the budget an already-expanded group is held before it
/// collapses again — `docs/plan/25-lod.md`'s "switch-up and switch-down differ",
/// as a fraction of [`LOD_ERROR_BUDGET`].
///
/// A ratio and not an offset because the band has to scale with the budget: a
/// fixed number of pixels is most of a one-pixel budget and none of a fifty-pixel
/// one. A fifth of the budget is a deadband a camera has to move decisively out
/// of, and one a camera drifting along a boundary never leaves — which is the
/// flicker the plan is about. `crcbl_shaders::cluster_select::LodBudgets` is the
/// pair this produces, and a ratio of one is that type's `sharp`: no band, and
/// the setting that shows the band is what stops the flicker.
const LOD_HOLD_RATIO: f32 = 0.8;

/// How much larger than the camera's the pixel budget a shadow cascade selects
/// under is — `docs/plan/25-lod.md`'s "**Shadow LOD bias**: shadow-pass culling
/// selects +1/+2 coarser levels — casters are cheap where it never shows".
///
/// # A budget factor, because a level is not a parameter of the rule
///
/// The descent compares a group's projected error to a pixel budget; it never
/// names a level. So "one or two levels coarser" is an *intent*, and the budget
/// that produces it is the setting — and the two do not convert, because how far
/// apart two levels' errors are is a property of the mesh. The committed dunes
/// DAG is the demonstration: the step from one level's error to the next is
/// several times larger in the middle of that hierarchy than at its base, and
/// across its top levels there is no step at all — they report one error and are
/// never separately selected by any budget. A bias expressed in levels would
/// mean something different at each of those. `cook-clusters` prints the per
/// level errors it built.
///
/// Four, then, is two doublings of the budget: enough that a caster is drawn
/// coarser than the same object is in the colour pass, and small enough to stay
/// inside what a shadow hides. What hides it is that a shadow edge is filtered —
/// the atlas is read through a comparison sampler with linear filtering, so an
/// edge arrives already spread over neighbouring texels — and that the atlas
/// quantises every caster to [`shadow::TILE`] texels per cascade before anything
/// samples it.
///
/// # Why one number, applied to the whole pass
///
/// **A cut is a cover only while expansion is monotone up the DAG**, and this
/// scaling is what keeps it so. Multiplying both budgets by one positive
/// constant leaves the rule in
/// [`crcbl_shaders::cluster_select`]'s form with different constants in it, and
/// that module's induction turns on the projected error being monotone up the
/// DAG rather than on what the constants are — so the shadow pass's cut is a
/// cover for exactly the reason the camera's is. A bias that varied by cluster,
/// by group, by level or by cascade would not be: two groups on one branch would
/// be judged against different budgets, and a child expanding under an
/// unexpanded parent is a hole.
///
/// Being larger than one also makes the shadow pass's expanded set a **subset**
/// of the colour pass's, since the two now select from one eye at one scale: a
/// group over `k` times the budget is over the budget. So the shadow cut is
/// never finer than the camera's anywhere, which is the property
/// `crcbl-vk`'s `the_shadow_cascades_select_coarser_than_the_camera` reads back
/// and asserts.
pub const SHADOW_LOD_BIAS: f32 = 4.0;

/// The budget an **orthographic** camera selects under: one no group satisfies,
/// so every group expands and the base level is drawn whole.
///
/// The metric divides a projected error by the distance to the group's sphere,
/// and an orthographic projection has no such falloff — see
/// [`Projection::pixels_per_unit`], which is where the trade is written down.
/// Drawing the finest level is the conservative answer and the honest one; a
/// distance term invented for a projection that has none would be neither.
///
/// [`Projection::pixels_per_unit`]: crate::camera::Projection::pixels_per_unit
const LOD_BUDGET_NONE: f32 = f32::NEG_INFINITY;

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
    /// the order [`CUBE_BUCKET`] and [`PYRAMID_BUCKET`] name. Its length is the
    /// bucket count, which [`DUNES_BUCKET`] says is a property of the path.
    ///
    /// **The whole of what the CPU still says per draw.** Everything else a
    /// draw needs — how many instances, which indices, which vertices — is in
    /// the arguments the GPU wrote or in a table it resolves them through.
    bucket_constants: Vec<u32>,

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
    /// The dunes patch's instance, likewise. See
    /// [`ForwardRenderer::set_dunes`], which is also where the one condition
    /// this object has and the others do not is written down.
    dunes_instance: Option<InstanceHandle>,
    /// The mesh ids those instances carry. Kept because every write of an
    /// instance writes the whole record, and an instance that lost its mesh id
    /// would resolve to entry 0 — which is a mesh, and the wrong one.
    cube_mesh: u32,
    pyramid_mesh: u32,
    open_box_mesh: u32,
    /// Level 0 of the dunes DAG. The coarser levels are resident too and no
    /// instance names one — a cluster reaches its own level's vertices through
    /// [`crcbl_shaders::cluster_select::ClusterSelect::vertex_base`].
    dunes_mesh: u32,

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
    /// Whether the mesh pipeline was built with §3.5's amplification stage in
    /// front of it — see [`ForwardRenderer::culls_clusters`], which is where
    /// what it means is written down.
    culls_clusters: bool,
    /// Where the dunes DAG's clusters are in [`ForwardRenderer::clusters`] —
    /// every level of it, as one run. `None` off the mesh path, where there is
    /// no cluster pool at all.
    dunes_clusters: Option<ClusterRange>,
    /// The bucket each level of the dunes DAG draws through, finest first, and
    /// empty on the mesh path. See
    /// [`ForwardRenderer::dunes_level_buckets`].
    dunes_level_buckets: Vec<u32>,
    /// One buffer per frame in flight holding the cut the descent chose, or
    /// empty where there is no amplification stage to choose one. See
    /// [`ForwardRenderer::cluster_selection`].
    cluster_selection: Vec<BufferHandle>,
    /// The pixel budget `docs/plan/25-lod.md`'s descent compares a group's
    /// projected error against. [`LOD_ERROR_BUDGET`] until
    /// [`ForwardRenderer::set_lod_error_budget`] says otherwise.
    lod_error_budget: f32,
    /// How far below that an expanded group is held before it collapses again.
    /// [`LOD_HOLD_RATIO`] until
    /// [`ForwardRenderer::set_lod_hold_ratio`] says otherwise.
    lod_hold_ratio: f32,
    /// What [`begin_frame`](ForwardRenderer::begin_frame) last wrote into
    /// [`mesh::FrameUniforms::lod_params`], kept so a reader can compute the
    /// same cut host-side without re-deriving it from the camera.
    ///
    /// Pixels per unit, the budget a group starts expanding over, and the budget
    /// it is held down to — `docs/plan/25-lod.md`'s hysteresis, and
    /// [`LOD_HOLD_RATIO`] is what puts the third below the second.
    lod_params: [f32; 3],
    /// The same three numbers the shadow cascades selected under, which is
    /// [`ForwardRenderer::lod_params`] with both budgets scaled by
    /// [`SHADOW_LOD_BIAS`] and the same pixels-per-unit.
    ///
    /// Kept beside the camera's for its reason: a reader comparing the two cuts
    /// takes both pairs from here rather than re-deriving the bias, which would
    /// be comparing two copies of one arithmetic instead of two selections.
    shadow_lod_params: [f32; 3],

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

    /// Topic 18's sun cascades: one `D32Float` image holding
    /// [`shadow::CASCADES`] square tiles side by side.
    ///
    /// Owned here rather than created by the graph per frame, because its
    /// extent is a **quality setting and not a window size** — nothing about it
    /// changes on a resize, so a transient would be re-created for no reason
    /// and could not be read back by a test that does not own the graph.
    shadow_atlas: ImageHandle,
    shadow_atlas_view: ImageViewHandle,
    /// A 1×1 depth image that exists so the depth-only pipeline's bind group can
    /// fill [`SHADOW_ATLAS_BINDING`] without naming the atlas it is writing.
    ///
    /// Both halves of that sentence are forced:
    ///
    /// * **The slot has to be filled.** Slang's Metal backend materialises every
    ///   global into every entry point — `msl/mesh.metal`'s `vertexMain` takes
    ///   `shadow_atlas [[texture(1)]]` whether it reads it or not — so a
    ///   depth-only pipeline sharing this layout still declares the argument. A
    ///   layout with the slot removed would break Metal alone, on a runner this
    ///   team cannot debug on, which is the trap binding 6's comment already
    ///   records.
    /// * **It cannot be the atlas.** WebGPU refuses a texture that is a render
    ///   attachment and a bind-group resource in the same pass, and the shadow
    ///   pass is exactly that pass.
    ///
    /// Nothing samples it: the shadow pipeline has no fragment stage. It is
    /// brought into the graph beside the atlas so its layout transition is the
    /// graph's like every other, rather than a hand-written barrier.
    shadow_placeholder: ImageHandle,
    shadow_placeholder_view: ImageViewHandle,
    /// The **comparison** sampler the atlas is read through — hardware PCF.
    shadow_sampler: SamplerHandle,
    /// The depth-only pipeline the cascades are rendered with: the same geometry
    /// stage as [`ForwardRenderer::mesh_pipeline`] and no fragment stage at all.
    shadow_pipeline: GraphicsPipelineHandle,
    /// One cull and draw-argument pass **per cascade**, which is topic 18's
    /// "one cull dispatch per cascade against the same instance/geometry pools".
    ///
    /// A whole [`DrawGen`] rather than just its cull half: the shadow pass emits
    /// the same indirect call the colour pass does, so it needs the same
    /// arguments, and there is no "cull only" constructor to reach for. The cost
    /// is that each cascade duplicates the clear and draw-argument pipelines.
    shadow_draws: Vec<DrawGen>,
    /// `[frame][cascade]`: a copy of the frame block whose `view_proj` **is**
    /// that cascade's matrix, so the depth-only pipeline runs the unmodified
    /// vertex and mesh stages rather than a second transform path.
    shadow_uniforms: Vec<Vec<BufferHandle>>,
    /// `[frame][cascade]`: the mesh layout again, reading that cascade's
    /// survivors and that cascade's uniforms.
    shadow_groups: Vec<Vec<BindGroupHandle>>,
    /// `[frame][cascade]`: the cut that cascade's amplification stage chose,
    /// one word per resident cluster — and empty where there is no such stage.
    ///
    /// **A buffer per cascade rather than the camera's**, unlike every other
    /// resource these passes share: the colour pass is recorded last and writes
    /// [`ForwardRenderer::cluster_selection`] over whatever a cascade left in it,
    /// so a cascade writing there leaves nothing behind to read. Without one of
    /// these the shadow pass's descent is unobservable — which is the state
    /// [`SHADOW_LOD_BIAS`] arrived in, and it is what a bias nothing can measure
    /// would have stayed in.
    shadow_selection: Vec<Vec<BufferHandle>>,
    /// What the last frame left [`ForwardRenderer::shadow_atlas`] and its
    /// placeholder in.
    ///
    /// [`ResourceState::Undefined`] until the first frame has declared them,
    /// which is the honest answer for an image nothing has written — importing
    /// them as `ShaderRead` from the start would skip the one barrier that gives
    /// them a layout at all.
    shadow_imported: ResourceState,

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
    /// The shadow atlas and its placeholder, which are created here rather than
    /// uploaded — so unlike [`Rollback::textures`] they are a plain handle each,
    /// with their views beside them.
    images: Vec<ImageHandle>,
    image_views: Vec<ImageViewHandle>,
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
    /// One more of the same per shadow cascade — a `Vec` rather than an
    /// `Option`, because a failure part way through the cascades has to release
    /// the ones already built.
    shadow_draws: Vec<DrawGen>,
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
        for handle in self.image_views {
            device.destroy_image_view(handle);
        }
        for handle in self.images {
            device.destroy_image(handle);
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
        for draws in self.shadow_draws {
            draws.destroy(device);
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

/// What every bind group of the mesh layout names identically.
///
/// Split from [`MeshGroup`] rather than folded into it because the split *is*
/// the claim: a resource here cannot differ between the colour pass's group and
/// a cascade's, and one there must be looked at.
struct SharedBindings<'a> {
    vertices: BufferHandle,
    draw_constants: BufferHandle,
    mesh_table: BufferHandle,
    materials: BufferHandle,
    page: ImageViewHandle,
    page_sampler: SamplerHandle,
    /// `Some` on [`GeometryPath::MeshShader`] and on no other path, which is
    /// what decides whether bindings 9 to 12 and 17 exist at all.
    clusters: Option<&'a ClusterPool>,
    culls_clusters: bool,
    shadow_sampler: SamplerHandle,
}

/// The half of a mesh-layout bind group that differs between the colour pass and
/// each shadow cascade.
///
/// One description for both, so the two cannot drift: a cascade's group is the
/// colour pass's with six resources swapped, and swapping the wrong five of them
/// is a shadow map rendered from the camera — which looks like a shadow map.
struct MeshGroup {
    /// Binding 0. The colour pass's frame block, or a cascade's copy whose
    /// `view_proj` is that cascade's matrix.
    uniforms: BufferHandle,
    /// Binding 2. This frame's slot of the instance ring.
    instances: BufferHandle,
    /// Binding 5. Whose survivors this group draws.
    runs: BufferHandle,
    /// Binding 12, on the mesh path: the same survivors' indirect arguments,
    /// read as data.
    args: BufferHandle,
    /// Binding 13, where there is an amplification stage: the frustum it culls
    /// clusters against.
    cull_params: BufferHandle,
    /// Binding 14, likewise: what it counts survivors into.
    cull_stats: BufferHandle,
    /// Binding 18, likewise: the cut the descent chose, one word per resident
    /// cluster. `None` exactly where there is no amplification stage, which is
    /// where the layout has no binding 18 either.
    ///
    /// **A buffer per pass**, not one shared: the colour pass is recorded last,
    /// so a cascade writing the camera's would leave nothing of its own to read.
    /// See [`ForwardRenderer::shadow_selection`].
    cluster_selection: Option<BufferHandle>,
    /// Binding 19, likewise: `docs/plan/25-lod.md`'s hysteresis state, which the
    /// draw-argument pass wrote this frame and this one only reads.
    ///
    /// **A buffer per pass** as well, because the colour pass and a cascade
    /// judge the same groups under different budgets — see
    /// [`CascadeBuffers::group_state`].
    group_state: Option<BufferHandle>,
    /// Binding [`SHADOW_ATLAS_BINDING`]. The atlas for the pass that reads it,
    /// and the placeholder for the pass that writes it — see
    /// [`ForwardRenderer::shadow_placeholder`], which is where that is argued.
    shadow_map: ImageViewHandle,
}

impl MeshGroup {
    fn entries(&self, shared: &SharedBindings<'_>) -> Vec<BindGroupEntry> {
        let mut entries = vec![
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(self.uniforms),
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::whole_buffer(shared.vertices),
            },
            BindGroupEntry {
                binding: 2,
                array_index: 0,
                // **This frame's slot of the instance ring, not a shared
                // buffer.** Binding one buffer here for every group would undo
                // the ring and reintroduce the cross-submission
                // read-after-write hazard it exists to prevent.
                resource: BindingResource::whole_buffer(self.instances),
            },
            BindGroupEntry {
                binding: 3,
                array_index: 0,
                // **One block, not the whole buffer.** The binding is dynamic,
                // so the bind's offset is added on top of this one and both
                // Vulkan and WebGPU require `offset + dynamic + size` to stay
                // inside the buffer — bound whole, the very first non-zero
                // dynamic offset would be out of range.
                resource: BindingResource::Buffer {
                    buffer: shared.draw_constants,
                    offset: 0,
                    size: DRAW_CONSTANTS_BLOCK,
                },
            },
            BindGroupEntry {
                binding: 4,
                array_index: 0,
                // The same table in every group, unlike the instance array
                // above it: the pool writes an entry when a mesh is uploaded or
                // freed, neither of which happens between frames here.
                resource: BindingResource::whole_buffer(shared.mesh_table),
            },
            BindGroupEntry {
                binding: 5,
                array_index: 0,
                // Whose cull this group draws from. This is the binding a
                // cascade's group differs in most consequentially: it is what
                // makes the shadow pass draw what the *light* can see rather
                // than what the camera can.
                resource: BindingResource::whole_buffer(self.runs),
            },
            BindGroupEntry {
                binding: 6,
                array_index: 0,
                resource: BindingResource::whole_buffer(shared.materials),
            },
            BindGroupEntry {
                binding: 7,
                array_index: 0,
                // **One entry, `array_index: 0`**, because the page is one
                // image and the layer is chosen in the shader. A bindless array
                // would be one entry per texture at ascending array indices,
                // which is the write path `BindGroupEntry`'s own docs describe
                // and the one this pass does not take.
                resource: BindingResource::ImageView(shared.page),
            },
            BindGroupEntry {
                binding: 8,
                array_index: 0,
                resource: BindingResource::Sampler(shared.page_sampler),
            },
        ];
        if let Some(clusters) = shared.clusters {
            entries.extend([
                BindGroupEntry {
                    binding: 9,
                    array_index: 0,
                    // The same three buffers in every group, on the mesh table's
                    // terms: clusters are written when the pool is built and
                    // never again.
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
                    // **Arguments read as data.** The mesh path records no
                    // indirect draw at all; what it wants out of this buffer is
                    // the one word only the GPU knows — how many instances
                    // survived into each bucket.
                    resource: BindingResource::whole_buffer(self.args),
                },
            ]);
        }
        if shared.culls_clusters {
            entries.extend([
                BindGroupEntry {
                    binding: 13,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(self.cull_params),
                },
                BindGroupEntry {
                    binding: 14,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(self.cull_stats),
                },
            ]);
        }
        entries.push(BindGroupEntry {
            binding: SHADOW_ATLAS_BINDING,
            array_index: 0,
            resource: BindingResource::ImageView(self.shadow_map),
        });
        entries.push(BindGroupEntry {
            binding: SHADOW_SAMPLER_BINDING,
            array_index: 0,
            resource: BindingResource::Sampler(shared.shadow_sampler),
        });
        // **After the shadow pair, because 15 and 16 are taken.** Those two are
        // `mesh.slang`'s, declared by the fragment stage of this very pipeline,
        // so `mesh_cluster.slang`'s own additions start at 17 — and the list
        // stays ascending, which is the order `crcbl-mtl` counts a Metal
        // argument table in.
        if let Some(clusters) = shared.clusters {
            entries.push(BindGroupEntry {
                binding: 17,
                array_index: 0,
                // Bound with the geometry rather than with the cull, because
                // `vertex_base` *is* geometry: it is which level of a DAG a
                // cluster's vertices live in, and both mesh entry points resolve
                // a vertex through it.
                resource: BindingResource::whole_buffer(clusters.selection()),
            });
        }
        if let Some(selection) = self.cluster_selection {
            entries.push(BindGroupEntry {
                binding: 18,
                array_index: 0,
                resource: BindingResource::whole_buffer(selection),
            });
        }
        if let Some(state) = self.group_state {
            entries.push(BindGroupEntry {
                binding: 19,
                array_index: 0,
                resource: BindingResource::whole_buffer(state),
            });
        }
        entries
    }
}

/// Every resident mesh's table id, and what a DAG needs on top of one.
struct Residents {
    cube: u32,
    pyramid: u32,
    open_box: u32,
    /// The dunes patch's DAG, level by level, as mesh table ids — finest first.
    ///
    /// Every level is its own vertex range and so its own table entry. Level 0's
    /// id is the one the *instance* carries and the one the cull pass reads a
    /// bounding box out of; the coarser ones are named by the bucket table on a
    /// path that takes a uniform cut, and by nothing at all on the mesh path,
    /// where a cluster reaches its own level through
    /// [`dunes_vertex_bases`](Self::dunes_vertex_bases) instead.
    dunes_levels: Vec<u32>,
    /// Per level, how far that level's vertices start past level 0's.
    ///
    /// What [`crcbl_shaders::cluster_select::ClusterSelect::vertex_base`] holds,
    /// and the whole of what makes a DAG drawable from one instance: the mesh
    /// stage adds this to the instance's own `base_vertex`, so a cut spanning
    /// three levels reads three different vertex ranges out of one pool.
    dunes_vertex_bases: Vec<u32>,
}

/// One cascade's per-frame buffers, read out of its [`DrawGen`] before that
/// generator is handed to the rollback.
struct CascadeBuffers {
    runs: Vec<BufferHandle>,
    args: Vec<BufferHandle>,
    cull_params: Vec<BufferHandle>,
    cull_stats: Vec<BufferHandle>,
    /// This cascade's own hysteresis state, and not the camera's.
    ///
    /// A cascade selects from the camera's eye at the camera's scale, like the
    /// colour pass, but under budgets [`SHADOW_LOD_BIAS`] times as large — so it
    /// reaches a different answer for the same group, and an answer carried
    /// between frames needs somewhere of its own to be carried. Sharing the
    /// camera's buffer would be two rules writing one history and each undoing
    /// the other's band every frame.
    ///
    /// One per cascade rather than one for the shadow pass, even though every
    /// cascade now selects identically: each cascade is its own [`DrawGen`], and
    /// one buffer between them would be several dispatches writing one element
    /// with nothing ordering them.
    group_state: BufferHandle,
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
        // **A second capability, asked separately.** `Features::TASK_SHADER` is
        // not implied by `MESH_SHADER`, so §3.5's per-cluster cull is an
        // amplification stage this renderer builds where the device has one and
        // leaves out where it does not — and a device without it keeps drawing
        // through `mesh_cluster.slang`'s un-amplified `meshMain`, which culls
        // nothing and emits every cluster. Same picture, more work.
        let culls_clusters = emit.is_mesh()
            && device
                .caps()
                .features
                .contains(crcbl_hal::Features::TASK_SHADER);

        let (
            pool,
            Residents {
                cube: cube_mesh,
                pyramid: pyramid_mesh,
                open_box: open_box_mesh,
                dunes_levels,
                dunes_vertex_bases,
            },
        ) = Self::build_geometry(device, queue)?;
        // The id the dunes *instance* carries. Level 0 whatever the path,
        // because it is the entry the cull pass reads a bounding box out of and
        // the coarser levels approximate the same surface inside the same box.
        let dunes_mesh = dunes_levels[0];
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
        // The bucket table, and the one place its order is decided. The dunes
        // patch is one bucket where an amplification stage picks its cut per
        // cluster and one per level where the cull pass picks a uniform one —
        // see [`DUNES_BUCKET`], which is where that difference is written down.
        let dunes_buckets = if emit.is_mesh() {
            1
        } else {
            dunes_levels.len()
        };
        let mut bucket_meshes = vec![0u32; DUNES_BUCKET + dunes_buckets];
        bucket_meshes[CUBE_BUCKET] = cube_mesh;
        bucket_meshes[PYRAMID_BUCKET] = pyramid_mesh;
        bucket_meshes[OPEN_BOX_BUCKET] = open_box_mesh;
        for (level, &mesh) in dunes_levels.iter().take(dunes_buckets).enumerate() {
            bucket_meshes[DUNES_BUCKET + level] = mesh;
        }
        let bucket_count = u32::try_from(bucket_meshes.len())
            .unwrap_or_else(|_| unreachable!("a table of a few buckets"));

        // `docs/plan/25-lod.md`'s selection tables, and the one thing that
        // decides whether `draw_gen.slang` takes a uniform cut at all.
        //
        // Every mesh id gets an entry, because the shader indexes this with
        // `GpuInstance::mesh` and an instance can name any of them; the default
        // is `MeshLevels::FLAT` pointing at a level run that is the mesh itself,
        // so a mesh with no hierarchy resolves back to itself with no branch.
        // **The dunes patch keeps that default on the mesh path**, which is how
        // a device already descending the DAG per cluster avoids a second,
        // coarser cut on top of it — the suppression is data rather than a
        // branch in the shader.
        let table_len = usize::try_from(
            [cube_mesh, pyramid_mesh, open_box_mesh]
                .into_iter()
                .chain(dunes_levels.iter().copied())
                .max()
                .unwrap_or_else(|| unreachable!("at least the cube is resident"))
                + 1,
        )
        .unwrap_or_else(|_| unreachable!("a table of a few meshes"));
        let mut level_meshes: Vec<u32> = (0..table_len)
            .map(|id| u32::try_from(id).unwrap_or_else(|_| unreachable!("bounded by table_len")))
            .collect();
        let mut mesh_levels: Vec<level_select::MeshLevels> = level_meshes
            .iter()
            .map(|&id| level_select::MeshLevels {
                first_level: id,
                ..level_select::MeshLevels::FLAT
            })
            .collect();
        let dag = crcbl_shaders::cluster_dag::dunes_dag();
        let level_groups: Vec<level_select::LevelGroup> = dag.level_groups();
        let dunes_first_group = 0u32;
        mesh_levels[dunes_mesh as usize] = level_select::MeshLevels {
            first_group: dunes_first_group,
            group_count: u32::try_from(level_groups.len())
                .unwrap_or_else(|_| unreachable!("a DAG of a few dozen groups")),
            // **The mesh path suppresses the uniform cut with a top level of
            // zero, not with a group count of zero**, and the difference is the
            // hysteresis state: `draw_gen.slang` judges every group it is given
            // whatever level it answers, and the amplification stage reads those
            // answers. A record naming no groups would leave the state
            // untouched and every cluster of the patch collapsed. A top level of
            // zero makes the level loop's minimum unreachable, so the instance
            // routes to the mesh it already names — which is level 0.
            first_level: if emit.is_mesh() {
                dunes_mesh
            } else {
                u32::try_from(level_meshes.len())
                    .unwrap_or_else(|_| unreachable!("a table of a few meshes"))
            },
            top_level: if emit.is_mesh() {
                0
            } else {
                u32::try_from(dunes_levels.len() - 1)
                    .unwrap_or_else(|_| unreachable!("a DAG of a few levels"))
            },
        };
        if !emit.is_mesh() {
            level_meshes.extend_from_slice(&dunes_levels);
        }

        // §3.5's clusters, on the path that reads them and on no other. The
        // records are `crcbl-shaders`' — cooked, because the builder is
        // `crcbl-scene`'s and the renderer must not depend on that crate — and
        // they arrive in bucket-mesh order so a bucket's range is its own
        // index. See `crate::cluster_pool`.
        //
        // Before the draw generation below rather than after it, because the
        // per-bucket cluster counts are the x extent of each bucket's mesh
        // dispatch and that pass writes the argument structure carrying them.
        let mut bucket_clusters = vec![0u32; bucket_meshes.len()];
        let mut bucket_cluster_bases = vec![0u32; bucket_meshes.len()];
        let mut dunes_clusters: Option<ClusterRange> = None;
        if emit.is_mesh() {
            // Three flat meshes and one DAG. The flat ones are one pool entry
            // each and their clusters carry `ClusterSelect::ALWAYS`, so the
            // descent draws them from every camera; the DAG is **one entry per
            // level**, laid end to end, and one bucket covers all of them.
            let selection = dag.selection_records(&dunes_vertex_bases, dunes_first_group);
            let mut cooked = vec![
                PooledMesh::without_lod(crcbl_shaders::meshlet::cube_clusters()),
                PooledMesh::without_lod(crcbl_shaders::meshlet::pyramid_clusters()),
                PooledMesh::without_lod(crcbl_shaders::meshlet::open_box_clusters()),
            ];
            for (level, records) in dag.levels.iter().zip(selection) {
                cooked.push(PooledMesh {
                    clusters: level.clusters.clone(),
                    selection: records,
                });
            }

            let clusters = ClusterPool::new(device, "forward", &cooked)?;
            let range = |entry: usize| {
                clusters
                    .range(entry)
                    .unwrap_or_else(|| unreachable!("one range per entry, in order"))
            };
            for bucket in [CUBE_BUCKET, PYRAMID_BUCKET, OPEN_BOX_BUCKET] {
                bucket_clusters[bucket] = range(bucket).count;
                bucket_cluster_bases[bucket] = range(bucket).base;
            }
            // **The DAG's levels are one run, not one per bucket.** `concatenate`
            // lays the entries down in the order it was given them, so the
            // levels are contiguous: the bucket starts where level 0 does and
            // reaches to the end of the last level. That is what lets one
            // dispatch cover a cut spanning several of them.
            bucket_cluster_bases[DUNES_BUCKET] = range(DUNES_BUCKET).base;
            bucket_clusters[DUNES_BUCKET] = (DUNES_BUCKET..cooked.len())
                .map(|entry| range(entry).count)
                .sum();
            dunes_clusters = Some(ClusterRange {
                base: bucket_cluster_bases[DUNES_BUCKET],
                count: bucket_clusters[DUNES_BUCKET],
            });

            rollback.clusters = Some(clusters);
        }

        let draws = DrawGen::new(
            device,
            queue,
            &DrawGenDesc {
                label: Some("forward"),
                instances: &instance_buffers,
                mesh_table,
                bucket_meshes: &bucket_meshes,
                bucket_clusters: &bucket_clusters,
                mesh_levels: &mesh_levels,
                level_groups: &level_groups,
                level_meshes: &level_meshes,
                instance_capacity: POOL_INSTANCE_CAPACITY,
            },
        )?;
        let runs: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.runs(frame))
            .collect();
        let args: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.args(frame))
            .collect();
        // What the amplification stage reads and writes, and nothing else does:
        // this frame's frustum, and the culling statistics its surviving
        // clusters are counted into.
        let cull_params: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.cull_params(frame))
            .collect();
        let cull_stats: Vec<BufferHandle> = (0..instance_buffers.len())
            .map(|frame| draws.visible_count(frame))
            .collect();
        rollback.draws = Some(draws);

        // `docs/plan/25-lod.md`'s observable: one word per resident cluster,
        // holding the cut the descent chose. Empty where there is no
        // amplification stage, which is the same condition binding 18 exists
        // under — and the two cannot disagree, because this vector is what
        // decides whether the entry is written.
        //
        // **One buffer per frame in flight**, on `cull_stats`' terms exactly: a
        // frame still in flight is a frame still writing, and one buffer shared
        // across the ring would have the next frame's dispatch overwriting what
        // this one recorded. `TRANSFER_SRC` because reading it is the point.
        let mut cluster_selection: Vec<BufferHandle> = Vec::new();
        // And one ring per shadow cascade, indexed `[cascade][frame]` — see
        // `ForwardRenderer::shadow_selection` for why a cascade cannot share the
        // buffer above.
        let mut cascade_selection: Vec<Vec<BufferHandle>> = Vec::new();
        if culls_clusters {
            let count = rollback
                .clusters
                .as_ref()
                .unwrap_or_else(|| unreachable!("an amplification stage implies a cluster pool"))
                .count();
            let mut ring = |label: &str| -> Result<Vec<BufferHandle>, HalError> {
                let mut buffers = Vec::with_capacity(instance_buffers.len());
                for frame in 0..instance_buffers.len() {
                    let buffer = device.create_buffer(&BufferDesc {
                        label: Some(&format!("{label} {frame}")),
                        size: u64::from(count) * 4,
                        usage: BufferUsage::STORAGE.union(BufferUsage::TRANSFER_SRC),
                        memory: MemoryLocation::DeviceLocal,
                    })?;
                    rollback.buffers.push(buffer);
                    buffers.push(buffer);
                }
                Ok(buffers)
            };
            cluster_selection = ring("cluster selection")?;
            for cascade in 0..shadow::CASCADES {
                cascade_selection.push(ring(&format!("shadow selection {cascade}"))?);
            }
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
        //
        // **`TASK` joins it when there is an amplification stage**, and the
        // union is deliberately over every binding rather than the eight
        // `taskMain` actually reads. Slang's Metal backend materialises every
        // global in every entry point of a module — the argument bindings 6 to
        // 8 already carry — so a layout that named only the task stage's own
        // reads would be a layout `msl/mesh_cluster.metal` disagrees with, on a
        // runner this team cannot debug on.
        let geometry = match (emit.is_mesh(), culls_clusters) {
            (true, true) => ShaderStages::MESH.union(ShaderStages::TASK),
            (true, false) => ShaderStages::MESH,
            (false, _) => ShaderStages::VERTEX,
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
                    sample_type: SampleType::Float,
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
                kind: BindingKind::Sampler { comparison: false },
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
        if culls_clusters {
            // §3.5's per-cluster cull needs two things `meshMain` does not, and
            // they exist only where an amplification stage does: the frustum,
            // and somewhere to count what survived.
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 13,
                visibility: geometry,
                // **The same block the instance cull reads**, not a copy: one
                // camera, one frustum, and no way for the two rejections to
                // disagree about which. `dynamic: false` because there is one
                // block per frame rather than one per bucket.
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            });
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 14,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // **The one writable binding this pass has.** The stage adds
                    // to a counter, so `read_only: false` is the truth rather
                    // than a hint, and it is what makes the graph order this
                    // write after the draw-argument pass's read of the same
                    // buffer.
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
        }
        // Topic 18's cascades, read by the fragment stage and declared last —
        // above `mesh_cluster.slang`'s range rather than inside it, for the
        // reason `SHADOW_ATLAS_BINDING` gives.
        //
        // Both carry `geometry` in their visibility beside `FRAGMENT`, for
        // binding 7's reason exactly: Slang's Metal backend materialises every
        // global into every entry point, and `msl/mesh.metal`'s `vertexMain`
        // takes `shadow_atlas [[texture(1)]]` and `shadow_sampler
        // [[sampler(1)]]` whether it reads them or not.
        mesh_entries.push(BindGroupLayoutEntry {
            binding: SHADOW_ATLAS_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // **`Depth`, and this is the field the seam grew for this
                // pass.** WebGPU will only bind a `D32Float` view through a
                // depth sample type, and its shader side agrees — the WGSL
                // artifact declares `texture_depth_2d`. The other three
                // backends take the interpretation off the view's format and
                // never read this.
                sample_type: SampleType::Depth,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        mesh_entries.push(BindGroupLayoutEntry {
            binding: SHADOW_SAMPLER_BINDING,
            visibility: geometry.union(ShaderStages::FRAGMENT),
            // The sampler-side twin of the line above: `sampler_comparison` in
            // the WGSL, `SamplerComparisonState` in the HLSL, and a
            // `compareEnable` on the Vulkan sampler object. The layout has to
            // say so on WebGPU and cannot on the other three.
            kind: BindingKind::Sampler { comparison: true },
            count: 1,
            flags: BindingFlags::empty(),
        });
        // `docs/plan/25-lod.md`'s two, **after the shadow pair rather than
        // beside their own kin**: 15 and 16 are taken by the fragment stage of
        // this very pipeline, so `mesh_cluster.slang` resumes at 17. Ascending
        // order matters here and not only for readability — `crcbl-mtl` gives a
        // resource the next index in its Metal argument table by counting the
        // same-table entries of this list.
        if emit.is_mesh() {
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 17,
                visibility: geometry,
                // Read-only and read by **both** mesh entry points, unlike the
                // pair below: a cluster's `vertex_base` is which level of a DAG
                // its geometry lives in, which an un-amplified stage needs just
                // as much as an amplified one.
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
        }
        if culls_clusters {
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 18,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // The second writable binding: the amplification stage
                    // records the cut it chose, one word per resident cluster.
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
            mesh_entries.push(BindGroupLayoutEntry {
                binding: 19,
                visibility: geometry,
                kind: BindingKind::StorageBuffer {
                    // `docs/plan/25-lod.md`'s hysteresis state, and read-only
                    // here: the draw-argument pass is its only writer, which is
                    // what lets a stage with one workgroup per cluster use a
                    // decision that has to survive a frame.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            });
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
        // One stride for both blocks, sized for the larger — see
        // [`DRAW_CONSTANTS_BLOCK`]. Which of the two a bucket's block holds is
        // decided below.
        let alignment = device.caps().limits.min_uniform_buffer_offset_alignment;
        let draw_stride =
            u32::try_from(DRAW_CONSTANTS_BLOCK.next_multiple_of(alignment)).map_err(|_| {
                HalError::InvalidDescriptor(format!(
                    "min_uniform_buffer_offset_alignment is {alignment}, which no dynamic \
                         offset can express"
                ))
            })?;
        let draw_constants = device.create_buffer(&BufferDesc {
            label: Some("mesh draw constants"),
            size: u64::from(draw_stride) * u64::from(bucket_count),
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
        let mut bucket_constants = vec![0u32; bucket_meshes.len()];
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
                    cluster_base: bucket_cluster_bases[index],
                    cluster_count: bucket_clusters[index],
                    bucket,
                    // The same number the draw-argument pass indexes the state
                    // with, taken from the object that owns the buffer rather
                    // than recomputed from the group table — the two indexing it
                    // differently is a cluster reading another instance's
                    // decision.
                    group_stride: draws.group_stride(),
                }
                .to_bytes()
                .to_vec()
            } else {
                // **The bucket's mesh, which is not always the drawn instance's.**
                // A uniform cut selects one of a DAG's levels and each level is
                // a mesh table entry of its own, so this is what says which
                // geometry the draw's index range belongs to; the instance goes
                // on naming level 0. See `mesh::DrawConstants::mesh`.
                mesh::DrawConstants {
                    base,
                    mesh: bucket_meshes[index],
                }
                .to_bytes()
                .to_vec()
            };
            device.write_buffer(draw_constants, u64::from(*offset), &block)?;
        }

        // --- the shadow map's own resources ---
        //
        // Before the bind groups, because every one of them names the atlas or
        // the placeholder standing in for it.
        let (atlas_width, atlas_height) = shadow::atlas_extent();
        let shadow_atlas = device.create_image(&ImageDesc {
            label: Some("shadow atlas"),
            image_type: ImageType::D2,
            format: Format::D32Float,
            extent: crcbl_hal::Extent3d::d2(atlas_width, atlas_height),
            mip_levels: 1,
            samples: 1,
            // `TRANSFER_SRC` so a test can copy the cascades back and check
            // that something was written into them. A shadow map that stayed at
            // its clear value produces a frame that looks entirely plausible —
            // everything is lit — so the readback is the only thing that can
            // tell a working pass from one that drew nothing.
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT
                .union(ImageUsage::SAMPLED)
                .union(ImageUsage::TRANSFER_SRC),
            memory: MemoryLocation::DeviceLocal,
        })?;
        rollback.images.push(shadow_atlas);
        let shadow_atlas_view = device.create_image_view(&ImageViewDesc {
            label: Some("shadow atlas"),
            image: shadow_atlas,
            view_type: ImageViewType::D2,
            format: Format::D32Float,
            range: ImageSubresourceRange::all(Format::D32Float),
        })?;
        rollback.image_views.push(shadow_atlas_view);

        let shadow_placeholder = device.create_image(&ImageDesc {
            label: Some("shadow placeholder"),
            image_type: ImageType::D2,
            format: Format::D32Float,
            extent: crcbl_hal::Extent3d::d2(1, 1),
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT.union(ImageUsage::SAMPLED),
            memory: MemoryLocation::DeviceLocal,
        })?;
        rollback.images.push(shadow_placeholder);
        let shadow_placeholder_view = device.create_image_view(&ImageViewDesc {
            label: Some("shadow placeholder"),
            image: shadow_placeholder,
            view_type: ImageViewType::D2,
            format: Format::D32Float,
            range: ImageSubresourceRange::all(Format::D32Float),
        })?;
        rollback.image_views.push(shadow_placeholder_view);

        // **A comparison sampler, and that is the PCF.** Each
        // `SampleCmpLevelZero` returns the filtered fraction of four texels that
        // passed the test, so the shader's 3×3 kernel is nine hardware-bilinear
        // comparisons rather than nine raw fetches it averages itself.
        //
        // `Greater` for the engine's reversed-Z, exactly as the depth *test*
        // is: the receiver survives when it is nearer the light than whatever
        // the map stored. `SamplerDesc::compare` carries the same warning.
        //
        // Clamped on every axis so a receiver just outside a cascade's box
        // reads the edge rather than wrapping into the opposite corner — the
        // shader's own bounds check is what actually handles that case, and this
        // is the second line of defence one texel wide.
        let shadow_sampler = device.create_sampler(&SamplerDesc {
            label: Some("shadow comparison"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            compare: Some(crcbl_hal::CompareOp::Greater),
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(shadow_sampler);

        // §3.3's cull, once per cascade. Each gets its own `DrawGen` and
        // therefore its own frustum, survivor list and indirect arguments —
        // which is what "one cull dispatch per cascade against the same
        // instance/geometry pools" means, and why the instance ring and the
        // mesh table below are the same handles the camera's cull was given.
        for _ in 0..shadow::CASCADES {
            // Into the rollback as each is built, on the same terms as the
            // camera's: a failure two cascades in has to release the first one,
            // and the rollback is the only thing that knows about it.
            let draws = DrawGen::new(
                device,
                queue,
                &DrawGenDesc {
                    label: Some("shadow cascade"),
                    instances: &instance_buffers,
                    mesh_table,
                    bucket_meshes: &bucket_meshes,
                    bucket_clusters: &bucket_clusters,
                    mesh_levels: &mesh_levels,
                    level_groups: &level_groups,
                    level_meshes: &level_meshes,
                    instance_capacity: POOL_INSTANCE_CAPACITY,
                },
            )?;
            rollback.shadow_draws.push(draws);
        }
        // The handles are `Copy` and are read out here for the same reason the
        // camera's are above: the pool they came from is the rollback's now.
        let cascade_buffers: Vec<CascadeBuffers> = rollback
            .shadow_draws
            .iter()
            .map(|draws| CascadeBuffers {
                runs: (0..instance_buffers.len())
                    .map(|frame| draws.runs(frame))
                    .collect(),
                args: (0..instance_buffers.len())
                    .map(|frame| draws.args(frame))
                    .collect(),
                cull_params: (0..instance_buffers.len())
                    .map(|frame| draws.cull_params(frame))
                    .collect(),
                cull_stats: (0..instance_buffers.len())
                    .map(|frame| draws.visible_count(frame))
                    .collect(),
                group_state: draws.group_state(),
            })
            .collect();

        let mut uniforms = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut mesh_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut shadow_uniforms = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut shadow_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut shadow_selection = Vec::with_capacity(FRAMES_IN_FLIGHT);
        // Everything a group of this layout names that is the same in all of
        // them. The per-group half is what `MeshGroup` below varies, and the two
        // exist so the colour pass's group and the shadow pass's are one
        // description rather than two that agree today.
        let shared = SharedBindings {
            vertices,
            draw_constants,
            mesh_table,
            materials: material_buffer,
            page: base_color_page.view,
            page_sampler: base_color_sampler,
            clusters: rollback.clusters.as_ref(),
            culls_clusters,
            shadow_sampler,
        };
        for (frame, &slot_instances) in instance_buffers.iter().enumerate() {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("mesh frame uniforms"),
                size: mesh::FRAME_UNIFORMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(buffer);
            let entries = MeshGroup {
                uniforms: buffer,
                instances: slot_instances,
                runs: runs[frame],
                args: args[frame],
                cull_params: cull_params[frame],
                cull_stats: cull_stats[frame],
                cluster_selection: cluster_selection.get(frame).copied(),
                group_state: culls_clusters.then(|| draws.group_state()),
                // The colour pass reads the finished atlas. Its own pass writes
                // nothing to it, so there is no conflict to avoid here.
                shadow_map: shadow_atlas_view,
            }
            .entries(&shared);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("mesh frame"),
                layout: mesh_layout,
                entries: &entries,
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            uniforms.push(buffer);
            mesh_groups.push(group);

            // The same layout again, once per cascade, differing in exactly the
            // things a cascade is: which matrix, which survivors, which frustum.
            let mut frame_shadow_uniforms = Vec::with_capacity(shadow::CASCADES);
            let mut frame_shadow_groups = Vec::with_capacity(shadow::CASCADES);
            let mut frame_shadow_selection = Vec::with_capacity(shadow::CASCADES);
            for (cascade, buffers) in cascade_buffers.iter().enumerate() {
                let cascade_uniforms = device.create_buffer(&BufferDesc {
                    label: Some("shadow cascade uniforms"),
                    size: mesh::FRAME_UNIFORMS_SIZE as u64,
                    usage: BufferUsage::UNIFORM,
                    memory: MemoryLocation::HostUpload,
                })?;
                rollback.buffers.push(cascade_uniforms);
                let entries = MeshGroup {
                    uniforms: cascade_uniforms,
                    instances: slot_instances,
                    runs: buffers.runs[frame],
                    args: buffers.args[frame],
                    // The amplification stage culls clusters against whatever
                    // frustum is in this block, and against
                    // `frame.camera_position` — which in a cascade's copy is the
                    // *light*. So the per-cluster cull rejects what faces away
                    // from the sun, which is the right question for a shadow map
                    // and the wrong one to have asked with the camera's frustum.
                    cull_params: buffers.cull_params[frame],
                    cull_stats: buffers.cull_stats[frame],
                    // **This cascade's own**, and not the colour pass's: that
                    // pass is recorded last and would write over it. See
                    // `ForwardRenderer::shadow_selection`.
                    cluster_selection: cascade_selection.get(cascade).map(|ring| ring[frame]),
                    // This cascade's own too — see `CascadeBuffers::group_state`,
                    // which is where the two budgets are argued.
                    group_state: culls_clusters.then_some(buffers.group_state),
                    shadow_map: shadow_placeholder_view,
                }
                .entries(&shared);
                let group = device.create_bind_group(&BindGroupDesc {
                    label: Some("shadow cascade"),
                    layout: mesh_layout,
                    entries: &entries,
                    variable_count: None,
                })?;
                rollback.bind_groups.push(group);
                frame_shadow_uniforms.push(cascade_uniforms);
                frame_shadow_groups.push(group);
                frame_shadow_selection
                    .extend(cascade_selection.get(cascade).map(|ring| ring[frame]));
            }
            shadow_uniforms.push(frame_shadow_uniforms);
            shadow_groups.push(frame_shadow_groups);
            shadow_selection.push(frame_shadow_selection);
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
        // The depth-only twin, built beside the colour pipeline out of the same
        // modules and the same layout, differing in exactly two things: no
        // fragment stage and no colour target.
        //
        // **The geometry stage is identical, and that is the design.** Topic 18
        // asks for a shadow pass "identical on every `GeometryPath` — depth pass
        // plus whatever emit tail the device selected", and the way to get that
        // without a second transform path is to leave `vertexMain` and
        // `meshMain` alone and hand them a frame block whose `view_proj` is the
        // cascade's matrix. A cascade that disagreed with the colour pass about
        // where a vertex is would produce shadows that do not line up with their
        // casters, which is indistinguishable from a bias problem.
        let (shadow_pipeline_result, mesh_pipeline, cluster_module) = if emit.is_mesh() {
            // **The mesh stage's module is `mesh_cluster.slang`; the fragment
            // stage's is still `mesh.slang`'s.** A pipeline takes a module per
            // stage, so the shading — Lambert, Blinn, the material row, the
            // page sample — is the same code both paths run rather than a copy
            // that agrees today. `mesh_cluster.slang`'s header carries the
            // argument, and its `VertexOutput` is what the two agree through.
            // **Named rather than looked up by stage**, because the module has
            // two mesh entry points and a stage lookup would refuse an
            // ambiguous one — see `named_entry`. Which of the two this builds
            // is the whole of the amplification decision.
            let cluster_entry = named_entry(
                &MESH_CLUSTER,
                if culls_clusters {
                    "amplifiedMeshMain"
                } else {
                    "meshMain"
                },
                Stage::Mesh,
            )?;
            let task_entry = culls_clusters
                .then(|| named_entry(&MESH_CLUSTER, "taskMain", Stage::Task))
                .transpose()?;
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
                // §3.5's per-cluster cull, where the device has the stage to
                // run it in. `None` is not a degradation: `meshMain` draws the
                // same picture out of the same clusters, having rejected none
                // of them, which is what a device with `Features::MESH_SHADER`
                // and no `Features::TASK_SHADER` gets.
                task: task_entry.map(|entry_point| ShaderEntry {
                    module: cluster_module,
                    entry_point,
                }),
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
            let shadow = device.create_mesh_pipeline(&MeshPipelineDesc {
                label: Some("shadow cascade mesh cluster"),
                layout: mesh_pipeline_layout,
                task: task_entry.map(|entry_point| ShaderEntry {
                    module: cluster_module,
                    entry_point,
                }),
                mesh: ShaderEntry {
                    module: cluster_module,
                    entry_point: cluster_entry,
                },
                fragment: None,
                primitive,
                depth_stencil,
                multisample: MultisampleState::default(),
                color_targets: &[],
            });
            (shadow, pipeline, Some(cluster_module))
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
            let shadow = device.create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("shadow cascade"),
                layout: mesh_pipeline_layout,
                vertex: ShaderEntry {
                    module: mesh_module,
                    entry_point: mesh_vertex,
                },
                fragment: None,
                primitive,
                depth_stencil,
                multisample: MultisampleState::default(),
                color_targets: &[],
            });
            (shadow, pipeline, None)
        };
        device.destroy_shader_module(mesh_module);
        if let Some(module) = cluster_module {
            device.destroy_shader_module(module);
        }
        let mesh_pipeline = mesh_pipeline?;
        rollback.pipelines.push(mesh_pipeline);
        let shadow_pipeline = shadow_pipeline_result?;
        rollback.pipelines.push(shadow_pipeline);

        // --- the tonemap pass ---
        let tonemap_entries = [
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
            culls_clusters,
            uniforms,
            draw_constants,
            dunes_instance: None,
            dunes_mesh,
            dunes_clusters,
            // One per level where the cull pass takes a uniform cut, and none
            // where the amplification stage takes a per-cluster one — the same
            // condition `dunes_buckets` was derived from, expressed off the
            // table that was actually built rather than re-derived from the
            // path.
            dunes_level_buckets: if emit.is_mesh() {
                Vec::new()
            } else {
                (DUNES_BUCKET..bucket_meshes.len())
                    .map(|bucket| {
                        u32::try_from(bucket)
                            .unwrap_or_else(|_| unreachable!("a table of a few buckets"))
                    })
                    .collect()
            },
            cluster_selection,
            lod_error_budget: LOD_ERROR_BUDGET,
            lod_hold_ratio: LOD_HOLD_RATIO,
            // Overwritten by the first `begin_frame`, which is the only thing
            // that can know the viewport. A zero scale with a budget of zero
            // selects nothing at all, and there is no frame yet to select for.
            lod_params: [0.0, 0.0, 0.0],
            shadow_lod_params: [0.0, 0.0, 0.0],
            mesh_groups,
            frame: 0,
            mesh_layout,
            mesh_pipeline_layout,
            mesh_pipeline,
            shadow_atlas,
            shadow_atlas_view,
            shadow_placeholder,
            shadow_placeholder_view,
            shadow_sampler,
            shadow_pipeline,
            shadow_draws: std::mem::take(&mut rollback.shadow_draws),
            shadow_uniforms,
            shadow_groups,
            shadow_selection,
            // Nothing has written either image yet, so the first frame's graph
            // is what gives them a layout.
            shadow_imported: ResourceState::Undefined,
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
    ) -> Result<(MeshPool, Residents), HalError> {
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
            Ok(residents) => Ok((pool, residents)),
            Err(error) => {
                pool.destroy(device);
                Err(error)
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
    ///
    /// The dunes patch is last, and it is **one upload per level of its DAG**.
    /// Every level was decimated separately, so a coarser level's vertices are
    /// wherever the collapses put them and belong to no vertex of the level
    /// below — a DAG is several vertex ranges, and the pool suballocates in
    /// vertices, so several ranges means several uploads. Level 0 goes first, so
    /// every other level starts past it and the offsets a cluster carries are
    /// non-negative; that is checked rather than assumed, because the pool's
    /// free list makes no promise about where a mesh lands.
    fn residents(
        device: &dyn Device,
        queue: QueueHandle,
        pool: &mut MeshPool,
    ) -> Result<Residents, HalError> {
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

        // `docs/plan/25-lod.md`'s model, level by level. The geometry of a
        // coarser level is positions and nothing else — the decimator carries no
        // attributes — so `dunes::vertex_at` is what turns each into a vertex,
        // by evaluating the surface rather than by interpolating an attribute
        // nothing recorded.
        let dag = crcbl_shaders::cluster_dag::dunes_dag();
        let mut dunes_levels = Vec::with_capacity(dag.levels.len());
        for (depth, level) in dag.levels.iter().enumerate() {
            let vertices: Vec<u8> = level
                .positions
                .iter()
                .flat_map(|&position| {
                    let vertex = crcbl_shaders::dunes::vertex_at(position);
                    let mut bytes = Vec::with_capacity(mesh::VERTEX_STRIDE);
                    for value in vertex
                        .position
                        .iter()
                        .chain(&vertex.normal)
                        .chain(&vertex.color)
                        .chain(&vertex.uv)
                    {
                        bytes.extend_from_slice(&value.to_le_bytes());
                    }
                    bytes
                })
                .collect();
            dunes_levels.push(pool.upload(
                device,
                queue,
                &format!("dunes level {depth}"),
                &vertices,
                &level.indices(),
            )?);
        }

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
        let base_vertex = |handle| match pool.mesh(handle) {
            Some(range) => Ok(range.base_vertex),
            None => Err(MeshPoolError::NotResident { handle }),
        };

        // **Relative to level 0's base**, because that is the base the instance
        // resolves through its mesh id and the one the stage adds this on top
        // of. A level that landed *below* level 0 would make the sum wrap, so it
        // is refused here rather than drawn as another mesh's vertices.
        let level_zero = base_vertex(dunes_levels[0])?;
        let mut dunes_vertex_bases = Vec::with_capacity(dunes_levels.len());
        for (depth, &handle) in dunes_levels.iter().enumerate() {
            let base = base_vertex(handle)?;
            dunes_vertex_bases.push(base.checked_sub(level_zero).ok_or_else(|| {
                HalError::InvalidDescriptor(format!(
                    "dunes level {depth} landed at vertex {base}, below level 0's \
                     {level_zero}, so its clusters cannot name their own geometry"
                ))
            })?);
        }

        Ok(Residents {
            cube: resolve(cube)?,
            pyramid: resolve(pyramid)?,
            open_box: resolve(open_box)?,
            dunes_levels: dunes_levels
                .iter()
                .map(|&handle| resolve(handle))
                .collect::<Result<_, _>>()?,
            dunes_vertex_bases,
        })
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
        // `docs/plan/25-lod.md`'s two selection numbers, from this frame's
        // viewport and this frame's camera. An orthographic projection has no
        // distance falloff for the metric to divide by, so it selects under a
        // budget nothing satisfies and draws the base level whole — see
        // [`LOD_BUDGET_NONE`].
        let lod_scale = camera.projection.pixels_per_unit(extent.1 as f32);
        let lod_budget = if camera.projection.is_orthographic() {
            LOD_BUDGET_NONE
        } else {
            self.lod_error_budget
        };
        self.lod_params = [lod_scale, lod_budget, lod_budget * self.lod_hold_ratio];
        // **The cascades select from this same camera at this same scale**, and
        // differ from it in the budgets alone — `docs/plan/25-lod.md`'s shadow
        // LOD bias, and see [`SHADOW_LOD_BIAS`] for why the bias is one factor
        // over the whole pass rather than a level count.
        //
        // `LOD_BUDGET_NONE` survives the scaling, which is what an orthographic
        // camera needs: negative infinity times a positive constant is still
        // negative infinity, so a camera with no distance falloff still draws
        // the base level into the shadow map rather than a level chosen by a
        // budget the metric cannot reach.
        self.shadow_lod_params = [
            lod_scale,
            self.lod_params[1] * SHADOW_LOD_BIAS,
            self.lod_params[2] * SHADOW_LOD_BIAS,
        ];
        // Topic 18's cascades. Built from the camera and the light alone, so a
        // frame that culls against them and a fragment that samples through them
        // cannot disagree about where they are.
        let cascades = Cascades::new(camera, direction);
        let mut shadow_view_proj = [[0.0f32; 16]; shadow::CASCADES];
        for (matrix, cascade) in shadow_view_proj.iter_mut().zip(&cascades.view_proj) {
            *matrix = cascade.to_cols_array();
        }
        let uniforms = mesh::FrameUniforms {
            view_proj: view_projection.to_cols_array(),
            camera_position: camera.eye.extend(1.0).to_array(),
            light_direction: direction.extend(0.0).to_array(),
            light_color: light.color.extend(0.0).to_array(),
            ambient: light.ambient.extend(0.0).to_array(),
            shadow_view_proj,
            cascade_far: cascades.far,
            shadow_params: Cascades::params(),
            lod_params: [
                self.lod_params[0],
                self.lod_params[1],
                self.lod_params[2],
                0.0,
            ],
        };
        device.write_buffer(self.uniforms[self.frame], 0, &uniforms.to_bytes())?;

        // Every element the pool has ever handed out, not its live count: a
        // removed instance leaves a hole and the live ones above it still have
        // to be tested. `InstancePool::slot_count` carries the difference.
        let instance_count = self.instances.slot_count();
        // The camera and the two selection numbers go to the cull/draw-argument
        // pair as well as into the block above, and they are handed over rather
        // than re-derived: `docs/plan/25-lod.md`'s uniform cut runs there, the
        // mesh path's per-cluster descent runs off the block, and a frame that
        // selected detail against one camera while drawing with another is a
        // difference nothing in the frame can see.
        self.draws.begin_frame(
            device,
            self.frame,
            &Frustum::from_view_projection(view_projection),
            instance_count,
            [camera.eye.x, camera.eye.y, camera.eye.z],
            self.lod_params,
        )?;

        // One cull per cascade, against that cascade's own frustum. The
        // orthographic box gives `Frustum::from_view_projection` six real planes
        // — unlike the camera's infinite perspective, whose far plane is
        // degenerate on purpose — so a caster outside the cascade is rejected
        // before it costs a vertex.
        for (cascade, draws) in self.shadow_draws.iter().enumerate() {
            // The same block the fragment stage will sample through, with the
            // cascade's matrix in `view_proj` and the *light* as the eye: the
            // amplification stage rejects clusters facing away from whatever is
            // at `camera_position`, and for a shadow map that must be the sun.
            let cascade_uniforms = mesh::FrameUniforms {
                view_proj: shadow_view_proj[cascade],
                camera_position: (camera.eye + direction * cascades.far[cascade])
                    .extend(1.0)
                    .to_array(),
                ..uniforms
            };
            device.write_buffer(
                self.shadow_uniforms[self.frame][cascade],
                0,
                &cascade_uniforms.to_bytes(),
            )?;
            // **The camera as the eye here, not the light**, and the two are
            // deliberately different questions asked of one pass.
            //
            // The block above puts the sun at `camera_position` because the
            // amplification stage's *normal cone* test asks which way a cluster
            // faces relative to the viewer, and a shadow map's viewer is the
            // sun. Detail is not that question. A directional sun has no
            // position for a distance metric to measure from — the point above
            // is the camera's own eye pushed along the sun's direction, so a
            // "distance to the light" taken from it is a fact about the camera
            // wearing the light's name, and it steps discontinuously from one
            // cascade to the next because `cascades.far` does.
            //
            // What a coarser caster actually costs is a shadow edge displaced by
            // the group's error, and that displacement is **seen by the camera**,
            // at the camera's pixels per unit and the camera's distance. So the
            // budget is denominated in the camera's pixels, and the eye that
            // makes the metric mean that is the camera's. The bias above is then
            // a statement about shadows rather than a side effect of where a
            // sun was placed.
            draws.begin_frame(
                device,
                self.frame,
                &Frustum::from_view_projection(cascades.view_proj[cascade]),
                instance_count,
                [camera.eye.x, camera.eye.y, camera.eye.z],
                self.shadow_lod_params,
            )?;
        }
        Ok(())
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

    /// Puts the dunes patch in the scene at `model`, or takes it out.
    ///
    /// `docs/plan/25-lod.md`'s model, and the only resident with a **cluster
    /// DAG**: a 64-unit height field whose far edge is many times further from
    /// a viewer standing at its near edge, so one cut through that DAG draws its
    /// two ends at different levels. That is the whole reason the mesh exists —
    /// a cube subtends one distance and asks the same question of every cluster.
    ///
    /// # One device cannot draw it, and says so rather than approximating
    ///
    /// A level gets chosen on every geometry path — per cluster in the
    /// amplification stage, and per instance in the cull pass where there is no
    /// such stage — **except** on a device that reports a mesh stage and no
    /// [`Features::TASK_SHADER`]. That one draws through `mesh_cluster.slang`'s
    /// un-amplified `meshMain`, which emits every cluster of the bucket, and for
    /// a DAG that is every level at once: several overlapping copies of one
    /// surface. So this refuses there rather than drawing that, and the return
    /// says which happened. The two indirect tails have no such stage to be
    /// missing a companion for — `draw_gen.slang` takes a uniform cut and the
    /// bucket it selects is one level's index range.
    ///
    /// Every other resident is a single level and is unaffected: their clusters
    /// carry [`ClusterSelect::ALWAYS`] and their `MeshLevels` record resolves to
    /// themselves, so both selections draw them from every camera.
    ///
    /// [`Features::TASK_SHADER`]: crcbl_hal::Features::TASK_SHADER
    /// [`ClusterSelect::ALWAYS`]: crcbl_shaders::cluster_select::ClusterSelect::ALWAYS
    pub fn set_dunes(&mut self, model: Option<Mat4>) -> bool {
        if model.is_some() && self.emit.is_mesh() && !self.culls_clusters {
            return false;
        }
        let instance = model.map(|model| mesh::GpuInstance {
            transform: model.to_cols_array(),
            mesh: self.dunes_mesh,
            material: self.untinted_material,
            ..mesh::GpuInstance::default()
        });
        place(
            &mut self.instances,
            &mut self.dunes_instance,
            instance.as_ref(),
            "the dunes patch",
        );
        true
    }

    /// The pixel budget the descent compares a group's projected error against.
    ///
    /// One pixel until this is called — see the constant this crate keeps it
    /// in. Larger is coarser: a group
    /// projecting *over* the budget is expanded and its children are drawn, so
    /// raising it stops the descent higher up the DAG.
    ///
    /// Takes effect at the next [`begin_frame`](Self::begin_frame), which is
    /// what writes it into the frame block.
    pub const fn set_lod_error_budget(&mut self, budget: f32) {
        self.lod_error_budget = budget;
    }

    /// The budget this renderer selects under unless
    /// [`set_lod_error_budget`](Self::set_lod_error_budget) says otherwise, and
    /// the ratio the hold budget sits at below whichever one is in force.
    ///
    /// Public because a test that drives a camera across a level boundary has to
    /// know where the boundary is, and where it is depends on both numbers —
    /// re-deriving either in the test would be a second copy to drift.
    pub const LOD_ERROR_BUDGET: f32 = LOD_ERROR_BUDGET;
    /// See [`LOD_ERROR_BUDGET`](Self::LOD_ERROR_BUDGET).
    pub const LOD_HOLD_RATIO: f32 = LOD_HOLD_RATIO;

    /// How far below the budget an already-expanded group is held before it
    /// collapses again — `docs/plan/25-lod.md`'s hysteresis, as a fraction of
    /// the budget.
    ///
    /// [`LOD_HOLD_RATIO`](Self::LOD_HOLD_RATIO) until this is called. **A ratio
    /// of one removes the band**, which makes the two budgets equal and the
    /// previous frame's answer stop mattering — the setting a test uses to show
    /// that the band is what stops a drifting camera flickering, on the same
    /// code path and one number apart.
    ///
    /// Takes effect at the next [`begin_frame`](Self::begin_frame), which is
    /// what writes it into the two params blocks.
    pub const fn set_lod_hold_ratio(&mut self, ratio: f32) {
        self.lod_hold_ratio = ratio;
    }

    /// What the last [`begin_frame`](Self::begin_frame) handed the descent:
    /// pixels per unit at one unit from the eye, and the pixel budget.
    ///
    /// Pixels per unit at one unit from the eye, the budget an unexpanded group
    /// starts expanding over, and the budget an expanded one is held down to.
    ///
    /// What `ClusterDag::expand` takes, so a caller reading
    /// [`cluster_selection`](Self::cluster_selection) back can run the same
    /// frames host-side rather than re-deriving the numbers from the camera and
    /// hoping they agree. `[0.0, 0.0, 0.0]` before the first frame.
    #[must_use]
    pub const fn lod_params(&self) -> [f32; 3] {
        self.lod_params
    }

    /// The same three numbers the **shadow cascades** selected under, which is
    /// [`lod_params`](Self::lod_params) with both budgets multiplied by
    /// [`SHADOW_LOD_BIAS`] — `docs/plan/25-lod.md`'s shadow LOD bias, as the
    /// parameters it actually reaches the GPU as.
    ///
    /// The pixels-per-unit is the camera's, unchanged, because the cascades
    /// select from the camera's eye at the camera's scale; see
    /// [`begin_frame`](Self::begin_frame). `[0.0, 0.0, 0.0]` before the first
    /// frame.
    #[must_use]
    pub const fn shadow_lod_params(&self) -> [f32; 3] {
        self.shadow_lod_params
    }

    /// Where the dunes DAG's clusters are in the cluster pool — every level, as
    /// one run — or `None` off the mesh path.
    ///
    /// The base a reader adds a `(level, cluster)` to in order to index
    /// [`cluster_selection`](Self::cluster_selection): the levels are laid down
    /// finest first and contiguously, so level `d` cluster `c` is at
    /// `base + (clusters of levels below d) + c`.
    #[must_use]
    pub const fn dunes_clusters(&self) -> Option<ClusterRange> {
        self.dunes_clusters
    }

    /// The buffer `frame`'s amplification stage recorded its chosen cut into:
    /// one `u32` per resident cluster, `1` where the cluster was drawn.
    ///
    /// `None` where there is no amplification stage, which is every device that
    /// reports no [`Features::TASK_SHADER`] and every non-mesh geometry path.
    ///
    /// **This is `docs/plan/25-lod.md`'s observable and nothing in the frame
    /// reads it.** A frame whose every cluster came from one level is a
    /// plausible picture and matches any golden blessed from it, so a golden
    /// cannot show per-cluster selection happening at all; this can. It is
    /// `TRANSFER_SRC`, and copying it out is the caller's to record.
    ///
    /// [`Features::TASK_SHADER`]: crcbl_hal::Features::TASK_SHADER
    #[must_use]
    pub fn cluster_selection(&self, frame: usize) -> Option<BufferHandle> {
        self.cluster_selection.get(frame).copied()
    }

    /// The same, for `cascade`'s shadow pass: the cut that cascade's
    /// amplification stage chose this frame.
    ///
    /// **The shadow LOD bias' observable.** A budget the cascades merely *were
    /// passed* is a parameter; the cut they reached under it is the behaviour,
    /// and this is the only thing that carries it out of a frame — the colour
    /// pass overwrites [`cluster_selection`](Self::cluster_selection) after the
    /// cascades have run. Indexed and read exactly as that buffer is, through
    /// [`dunes_clusters`](Self::dunes_clusters).
    ///
    /// `None` where there is no amplification stage, and where `cascade` is not
    /// one of [`shadow::CASCADES`].
    #[must_use]
    pub fn shadow_selection(&self, frame: usize, cascade: usize) -> Option<BufferHandle> {
        self.shadow_selection
            .get(frame)
            .and_then(|cascades| cascades.get(cascade))
            .copied()
    }

    /// The buckets the dunes patch's DAG levels draw through, finest first —
    /// element `d` is level `d`'s bucket, and it is empty on the mesh path.
    ///
    /// **This is the uniform cut's observable**, and it is the indirect
    /// arguments rather than a buffer of its own: `draw_gen.slang` scatters an
    /// instance into the bucket for the level it selected, so exactly one of
    /// these buckets comes out of a frame with a non-zero instance count and
    /// which one it is *is* the chosen level. A reader takes
    /// [`DrawGen::args_offset`](crate::draw_gen::DrawGen::args_offset) of each
    /// and looks at
    /// [`DrawIndexedArgs::instance_count`](crcbl_shaders::draw_gen::DrawIndexedArgs::instance_count).
    ///
    /// Empty on [`GeometryPath::MeshShader`], where the DAG is one bucket and
    /// the level is chosen per cluster instead — see
    /// [`cluster_selection`](Self::cluster_selection), which is that path's
    /// observable.
    #[must_use]
    pub fn dunes_level_buckets(&self) -> &[u32] {
        &self.dunes_level_buckets
    }

    /// `frame`'s indirect draw arguments, [`crcbl_shaders::draw_gen::DRAW_ARGS_SIZE`]
    /// bytes per bucket — what a reader of
    /// [`dunes_level_buckets`](Self::dunes_level_buckets) copies out.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this renderer was built with.
    #[must_use]
    pub fn draw_args(&self, frame: usize) -> BufferHandle {
        self.draws.args(frame)
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

        // `docs/plan/25-lod.md`'s record of the cut — the colour pass's, and one
        // per cascade beside it. Each is written by exactly one mesh pass, so
        // what the graph orders here is this frame's write against the next
        // frame's use of the same slot. Each arrives in the state the previous
        // frame left it in, which is the one declared as final.
        let import_selection = |graph: &mut RenderGraph<'_>, label: &str, buffer| {
            graph.import_buffer(
                label,
                ImportedBuffer {
                    buffer,
                    initial: ResourceState::ShaderReadWrite,
                    final_state: ResourceState::ShaderReadWrite,
                },
            )
        };
        let selection = self
            .cluster_selection
            .get(self.frame)
            .map(|&buffer| import_selection(graph, "cluster-selection", buffer));
        let cascade_selection: Vec<BufferId> = self
            .shadow_selection
            .get(self.frame)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(cascade, &buffer)| {
                import_selection(graph, &format!("shadow-selection-{cascade}"), buffer)
            })
            .collect();

        let imported = std::mem::replace(&mut self.shadow_imported, ResourceState::ShaderRead);
        let shadow_atlas = self.add_shadow_pass(graph, imported, &cascade_selection);

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
        let mesh_stride = crcbl_shaders::draw_gen::MESH_ARGS_SIZE as u32;
        // One call per bucket, always — the number the CPU records does not
        // depend on what is in the scene, which is the whole of what §3.3 asks
        // for. An empty bucket's arguments carry an instance count of zero.
        let calls: Vec<(u32, u64, u64, u64)> = self
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
                    self.draws.mesh_args_offset(bucket),
                )
            })
            .collect();

        let pass = graph
            .add_render_pass("forward")
            .clear_color(scene_color, SCENE_CLEAR)
            .clear_depth(scene_depth)
            // **The barrier out of the shadow pass's depth attachment.** The
            // atlas is in this pass's bind group at `SHADOW_ATLAS_BINDING`, and
            // without this declaration the graph leaves it in
            // `DepthStencilWrite` — which Vulkan reports as
            // `VUID-vkCmdDrawIndexedIndirectCount-imageLayout-00344` naming the
            // binding, and which every other backend reads as whatever the
            // depth writes left behind.
            .read_image(shadow_atlas)
            // The buffers the draws come out of. Declaring them is what makes
            // the graph transition them out of the compute pass's
            // `ShaderReadWrite` — the seam calls that the single most important
            // barrier in a GPU-driven frame, and its absence produces
            // "sometimes nothing draws".
            .read_buffer(generated.runs_id);
        let pass = if emit.is_mesh() {
            // **The same arguments, read as data rather than executed**, which
            // is what the stages use to bound the run of surviving instances
            // they index. The per-bucket draw *counts* are not read at all,
            // because nothing here is a draw whose count could come from
            // memory.
            //
            // The dispatch *extents* are a second buffer and a real indirect
            // read — one structure per bucket, written by the same pass. Two
            // buffers rather than one because a resource is in exactly one
            // state per pass and these two are in different ones.
            let pass = pass
                .read_buffer(generated.args_id)
                .use_buffer(generated.mesh_args_id, ResourceState::IndirectArgument);
            if self.culls_clusters {
                // The amplification stage counts its survivors into the
                // culling statistics, which the draw-argument pass read a
                // moment ago — so this is a write-after-read the graph has to
                // order, and declaring it is the whole of how it learns to.
                let pass =
                    pass.use_buffer(generated.visible_count_id, ResourceState::ShaderReadWrite);
                // `docs/plan/25-lod.md`'s hysteresis state, read here and
                // written by the draw-argument pass a moment ago. Declaring it
                // is what orders the two — and what puts it back into
                // `ShaderReadWrite` at the end of the graph, which is where the
                // next frame's draw-argument pass expects to find it.
                let pass = pass.read_buffer(generated.group_state_id);
                match selection {
                    // The colour pass's own, which no cascade writes — the
                    // cascades record into buffers of their own, so what
                    // survives a frame here is the camera's cut and what
                    // survives there is each cascade's.
                    Some(selection) => pass.use_buffer(selection, ResourceState::ShaderReadWrite),
                    None => pass,
                }
            } else {
                pass
            }
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
            for (constant_offset, args_offset, count_offset, mesh_args_offset) in calls {
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
                        // One workgroup per (cluster, **surviving** instance),
                        // and neither extent is the CPU's: they are the three
                        // words the draw-argument pass wrote for this bucket.
                        //
                        // That is the whole difference between culling that
                        // skips output and culling that skips work. A dispatch
                        // sized here would have to cover every slot the
                        // instance pool ever handed out — a removed instance
                        // leaves a hole and the live ones above it stay in the
                        // array — and launch a workgroup for each, which then
                        // reads the survivor count and returns.
                        //
                        // Recorded unconditionally, unlike a CPU-sized
                        // dispatch: an extent of zero is a legal indirect
                        // dispatch of no workgroups, so an empty scene needs no
                        // branch here and the recorded stream stays the same
                        // whatever the scene holds.
                        encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                            args: generated.mesh_args,
                            offset: mesh_args_offset,
                            draw_count: 1,
                            stride: mesh_stride,
                        });
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

    /// Adds the cull dispatches and the depth-only pass that fill the shadow
    /// atlas, and returns the atlas as the graph knows it.
    ///
    /// Every cascade is one viewport of one render pass. A pass per cascade
    /// would be [`shadow::CASCADES`] clears of the same image and
    /// [`shadow::CASCADES`] more barriers, and the graph would have to be told
    /// each of them only touches part of it — where one pass with a viewport per
    /// tile is what a shadow *atlas* is for in the first place.
    fn add_shadow_pass(
        &self,
        graph: &mut RenderGraph<'_>,
        imported: ResourceState,
        selection: &[BufferId],
    ) -> ImageId {
        let (atlas_width, atlas_height) = shadow::atlas_extent();
        let atlas = graph.import_image(
            "shadow-atlas",
            ImportedImage {
                image: self.shadow_atlas,
                view: self.shadow_atlas_view,
                format: Format::D32Float,
                extent: (atlas_width, atlas_height),
                // What the previous frame left it in — `Undefined` on the first,
                // which is what makes the graph give it a layout at all.
                initial: imported,
                final_state: ResourceState::ShaderRead,
            },
        );
        let placeholder = graph.import_image(
            "shadow-placeholder",
            ImportedImage {
                image: self.shadow_placeholder,
                view: self.shadow_placeholder_view,
                format: Format::D32Float,
                extent: (1, 1),
                initial: imported,
                final_state: ResourceState::ShaderRead,
            },
        );

        // One cull dispatch per cascade, before the pass that draws from them.
        let generated: Vec<_> = self
            .shadow_draws
            .iter()
            .map(|draws| draws.add_passes(graph, self.frame, self.instances.slot_count()))
            .collect();

        let mut pass = graph
            .add_render_pass("shadow")
            // **Stored, unlike the scene's depth.** This is the one depth buffer
            // in the engine that something downstream reads, which is what
            // `PassBuilder::clear_depth`'s docs said would need a `StoreOp` the
            // day a prepass existed.
            .depth(
                atlas,
                LoadOp::Clear,
                StoreOp::Store,
                crcbl_hal::ClearValue {
                    depth: crcbl_hal::depth::CLEAR,
                    ..crcbl_hal::ClearValue::default()
                },
            )
            // Declared so the graph gives it a shader-read layout: it is in
            // every cascade's bind group, standing in for the atlas this pass is
            // writing. See `ForwardRenderer::shadow_placeholder`.
            .read_image(placeholder);
        // Each cascade's mesh pass records the cut it descended to, into a
        // buffer of its own — see `ForwardRenderer::shadow_selection`. Empty
        // where there is no amplification stage to descend anything.
        for &buffer in selection {
            pass = pass.use_buffer(buffer, ResourceState::ShaderReadWrite);
        }
        for draws in &generated {
            pass = pass.read_buffer(draws.runs_id);
            pass = if self.emit.is_mesh() {
                let pass = pass
                    .read_buffer(draws.args_id)
                    .use_buffer(draws.mesh_args_id, ResourceState::IndirectArgument);
                if self.culls_clusters {
                    pass.use_buffer(draws.visible_count_id, ResourceState::ShaderReadWrite)
                        .read_buffer(draws.group_state_id)
                } else {
                    pass
                }
            } else {
                pass.use_buffer(draws.args_id, ResourceState::IndirectArgument)
                    .use_buffer(draws.counts_id, ResourceState::IndirectArgument)
            };
        }

        let groups = self.shadow_groups[self.frame].clone();
        let pipeline = self.shadow_pipeline;
        let layout = self.mesh_pipeline_layout;
        let indices = self.pool.index_buffer();
        let emit = self.emit;
        let stride = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u32;
        let mesh_stride = crcbl_shaders::draw_gen::MESH_ARGS_SIZE as u32;
        let calls: Vec<(u32, u64, u64, u64)> = self
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
                    self.draws.mesh_args_offset(bucket),
                )
            })
            .collect();

        pass.execute(move |ctx| {
            let encoder = ctx.encoder();
            encoder.bind_graphics_pipeline(pipeline);
            if !emit.is_mesh() {
                encoder.bind_index_buffer(indices, 0, IndexFormat::Uint32);
            }
            for (cascade, (group, draws)) in groups.iter().zip(&generated).enumerate() {
                // The tile this cascade owns. The graph set a viewport over the
                // whole atlas before this body ran, and this is what narrows it
                // — the same clip-space matrix mapped into a different sixth of
                // the image.
                let (origin_x, origin_y) = shadow::tile_origin(cascade);
                let rect = Rect2d {
                    x: i32::try_from(origin_x).unwrap_or(i32::MAX),
                    y: i32::try_from(origin_y).unwrap_or(i32::MAX),
                    width: shadow::TILE,
                    height: shadow::TILE,
                };
                encoder.set_viewport(&Viewport {
                    x: rect.x as f32,
                    y: rect.y as f32,
                    width: rect.width as f32,
                    height: rect.height as f32,
                    ..Viewport::from_size(rect.width, rect.height)
                });
                encoder.set_scissor(&rect);
                for (constant_offset, args_offset, count_offset, mesh_args_offset) in &calls {
                    encoder.bind_group(0, *group, &[*constant_offset], layout);
                    match emit {
                        EmitTail::Mesh => {
                            encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                                args: draws.mesh_args,
                                offset: *mesh_args_offset,
                                draw_count: 1,
                                stride: mesh_stride,
                            });
                        }
                        EmitTail::Count => {
                            encoder.draw_indexed_indirect_count(&DrawIndirectCount {
                                args: draws.args,
                                args_offset: *args_offset,
                                count_buffer: draws.counts,
                                count_offset: *count_offset,
                                max_draw_count: 1,
                                stride,
                            });
                        }
                        EmitTail::PerBatch => {
                            encoder.draw_indexed_indirect(&DrawIndirect {
                                args: draws.args,
                                offset: *args_offset,
                                draw_count: 1,
                                stride,
                            });
                        }
                    }
                }
            }
        });
        atlas
    }

    /// The shadow atlas, for a caller that wants to read it back.
    ///
    /// **The observable this whole slice can otherwise hide behind.** A shadow
    /// pass that renders nothing leaves every texel at
    /// [`depth::CLEAR`](crcbl_hal::depth::CLEAR) and produces a frame in which
    /// everything is lit — which is a perfectly plausible picture and matches
    /// any golden blessed from it. Copying this image back and finding a texel
    /// that is not the clear value is what distinguishes the two.
    ///
    /// It is created with
    /// [`ImageUsage::TRANSFER_SRC`](crcbl_hal::ImageUsage::TRANSFER_SRC) for
    /// exactly that, and its extent is [`shadow::atlas_extent`].
    #[must_use]
    pub const fn shadow_atlas(&self) -> crcbl_hal::ImageHandle {
        self.shadow_atlas
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

    /// Whether this renderer **built** §3.5's per-cluster cull — an
    /// amplification stage in front of the mesh stage, rejecting a surviving
    /// instance's back-facing and off-screen clusters.
    ///
    /// `false` on the two indirect tails, which draw index ranges and have no
    /// stage to put it in, and on a device with `Features::MESH_SHADER` and no
    /// `Features::TASK_SHADER` — a real and supported state, not a degradation:
    /// that device draws every cluster of every surviving instance and the
    /// picture is the same one.
    ///
    /// The instance cull runs on every path either way, and is unaffected by
    /// this.
    ///
    /// It is asked of the renderer rather than of the device for
    /// [`ForwardRenderer::geometry_path`]'s reason: this reports what was
    /// built, so `true` means
    /// [`DrawGen::visible_count`](crate::draw_gen::DrawGen::visible_count)'s
    /// cluster word is a number the frame produced rather than the zero the
    /// clearing pass left.
    #[must_use]
    pub const fn culls_clusters(&self) -> bool {
        self.culls_clusters
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

        device.destroy_graphics_pipeline(self.shadow_pipeline);
        for groups in self.shadow_groups {
            for group in groups {
                device.destroy_bind_group(group);
            }
        }
        for buffers in self.shadow_uniforms {
            for buffer in buffers {
                device.destroy_buffer(buffer);
            }
        }
        for buffers in self.shadow_selection {
            for buffer in buffers {
                device.destroy_buffer(buffer);
            }
        }
        for draws in self.shadow_draws {
            draws.destroy(device);
        }
        device.destroy_sampler(self.shadow_sampler);
        device.destroy_image_view(self.shadow_atlas_view);
        device.destroy_image(self.shadow_atlas);
        device.destroy_image_view(self.shadow_placeholder_view);
        device.destroy_image(self.shadow_placeholder);

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
        for buffer in self.cluster_selection {
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

/// The entry point called `name`, checked to be `stage`'s.
///
/// [`entry`] cannot serve `mesh_cluster.slang`: it has **two** mesh entry
/// points — one behind the amplification stage and one without it — and a
/// lookup by stage answers `None` for two matches rather than picking one, for
/// the good reason that picking one would draw the wrong geometry.
///
/// The stage is still checked rather than taken on trust: a name that resolved
/// to the wrong stage would be a pipeline built with an amplification stage in
/// its mesh slot, and the failure would arrive as a driver error rather than as
/// this sentence.
fn named_entry(
    shader: &crcbl_shaders::Shader,
    name: &'static str,
    stage: Stage,
) -> Result<&'static str, HalError> {
    let found = shader
        .entry_points()
        .iter()
        .find(|entry| entry.name() == name && entry.stage() == stage);
    found.map(crcbl_shaders::EntryPoint::name).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "{}.slang exposes no {stage:?} entry point called `{name}`; the committed SPIR-V and \
             its manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix",
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
            let expected: Vec<(u32, u64, u64)> = (0..renderer.bucket_constants.len())
                .map(|bucket| {
                    (
                        renderer.bucket_constants[bucket],
                        bucket as u64 * u64::from(stride),
                        bucket as u64 * 4,
                    )
                })
                .collect();

            // The dynamic offset last bound before each draw, paired with the
            // offsets that draw read its arguments and its count from.
            let mut offset = None;
            let mut seen: Vec<(u32, u64, u64)> = Vec::new();
            let mut dispatches = 0;
            for command in recorder.commands() {
                if matches!(command, Command::Dispatch { .. }) {
                    dispatches += 1;
                }
            }
            for command in commands_in_pass(&recorder, "forward") {
                match command {
                    Command::BindGroup {
                        slot: 0,
                        dynamic_offsets,
                        ..
                    } => offset = dynamic_offsets.first().copied(),
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
                dispatches,
                3 * (1 + shadow::CASCADES),
                "the clearing pass, the cull pass and the draw-argument pass, in front of \
                 the draws — once for the camera and once per shadow cascade"
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
            for command in commands_in_pass(&recorder, "forward") {
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
            let buckets = renderer.bucket_constants.len();
            let (expected_counted, expected_per_batch) =
                if counted { (buckets, 0) } else { (0, buckets) };
            assert_eq!(
                (counted_calls, per_batch_calls),
                (expected_counted, expected_per_batch),
                "{path:?} must record one call of its own kind per bucket and none of the other"
            );

            frame.finish(device.as_ref(), renderer);
        }
    }

    /// Opens a null device offering `optional` on top of the mesh path, and
    /// asserts it really selected that path.
    ///
    /// Asked for rather than merely reported, because a device grants what it
    /// enabled: leaving mesh shaders out of the optional set opens a device on
    /// an indirect tail and tests nothing.
    fn open_mesh_path(recorder: &Recorder, optional: Features) -> (Box<dyn Device>, QueueHandle) {
        let caps = crcbl_hal::DeviceCaps {
            features: Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
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
                optional_features: Features::GPU_DRIVEN | Features::MESH_SHADER | optional,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        assert_eq!(
            device.caps().geometry_path(),
            GeometryPath::MeshShader,
            "and the opened device still selects it"
        );
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    /// **`Features::TASK_SHADER` is what decides whether §3.5's per-cluster cull
    /// is built, and a device without it still draws.**
    ///
    /// The two capabilities are separate, and the adapter above offers both — so
    /// asking for one and not the other is a device state a real driver has and
    /// this can reach. Neither arm is a skip:
    ///
    /// * With the task stage, the renderer builds the amplification stage and
    ///   [`ForwardRenderer::culls_clusters`] says so. The null backend refuses a
    ///   task stage on a device without the flag, so an arm that computed this
    ///   wrongly would fail to build rather than pass quietly.
    /// * Without it, the renderer builds `meshMain` — the un-amplified entry
    ///   point — and draws every cluster of every surviving instance. That is
    ///   the path that existed before this slice and it is unchanged, dispatch
    ///   extents included.
    #[test]
    fn the_task_stage_is_built_only_where_the_device_has_one() {
        use crcbl_hal::null::Command;

        for (optional, expected) in [(Features::TASK_SHADER, true), (Features::empty(), false)] {
            let recorder = Recorder::new();
            let (device, queue) = open_mesh_path(&recorder, optional);
            let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("the forward renderer builds on both device shapes");
            assert_eq!(
                device.caps().supports(Features::TASK_SHADER),
                expected,
                "the device must actually differ between the arms, or both test the same thing"
            );
            assert_eq!(
                renderer.culls_clusters(),
                expected,
                "the amplification stage is built exactly where the device has one"
            );
            assert_eq!(
                renderer.geometry_path(),
                GeometryPath::MeshShader,
                "both arms are the mesh path; only the stage in front of it differs"
            );

            let frame = frame(device.as_ref(), &mut renderer, queue);
            // After the frame, because the buffer named is the slot the frame
            // rotated to — asking before it names the previous slot's ring
            // entry and the comparison is against a buffer nothing drew from.
            let expected = mesh_dispatch_calls(&renderer);

            // **The recorded stream is the same either way**, which is what
            // makes the cull a rejection inside the existing shape rather than
            // a second one: one indirect dispatch per bucket, reading that
            // bucket's own arguments, and the amplification stage is what turns
            // a group into no work.
            let dispatched: Vec<DrawIndirect> = commands_in_pass(&recorder, "forward")
                .into_iter()
                .filter_map(|command| match command {
                    Command::DrawMeshTasksIndirect(draw) => Some(draw),
                    _ => None,
                })
                .collect();
            assert_eq!(
                dispatched, expected,
                "one indirect dispatch per bucket on both arms, each reading its own \
                 argument structure"
            );

            frame.finish(device.as_ref(), renderer);
        }
    }

    /// The [`DrawIndirect`] the mesh path must record for each bucket, in bucket
    /// order.
    ///
    /// Built from the buffer the draw-argument pass writes and the stride the
    /// APIs fixed, so a test comparing against it is asserting that the call
    /// reads *this frame's* arguments at *this bucket's* offset — the two ways
    /// an indirect dispatch goes wrong while still dispatching something.
    ///
    /// **The offsets are multiplied out here rather than asked of
    /// [`DrawGen::mesh_args_offset`]**, which is the difference between a
    /// comparison and a tautology: an offset function that returned zero for
    /// every bucket would agree with itself, and three calls onto one structure
    /// is a frame that draws one bucket's geometry three times.
    fn mesh_dispatch_calls(renderer: &ForwardRenderer) -> Vec<DrawIndirect> {
        let stride = crcbl_shaders::draw_gen::MESH_ARGS_SIZE as u32;
        (0..renderer.bucket_constants.len())
            .map(|bucket| DrawIndirect {
                args: renderer.draws.mesh_args(renderer.frame),
                offset: bucket as u64 * u64::from(stride),
                draw_count: 1,
                stride,
            })
            .collect()
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
    /// **The extents are not here to be checked, and that is the point.** They
    /// are three words the draw-argument pass writes, so what this can pin is
    /// the call reading *this* bucket's structure out of *this* frame's buffer
    /// — and the CPU-side half of the x extent, the per-bucket cluster table
    /// the host uploaded, which the null backend keeps the bytes of. What the
    /// GPU then makes of it needs a GPU: `crcbl-vk`'s
    /// `the_mesh_dispatch_extent_is_the_culled_instance_count` reads the
    /// arguments back.
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
        let frame = frame(device.as_ref(), &mut renderer, queue);
        let expected = mesh_dispatch_calls(&renderer);
        // The table is written once at build, into a host-visible buffer the
        // null backend keeps the bytes of — so it is readable whether or not a
        // shader ever ran.
        let clusters: Vec<u32> = recorder
            .buffer_bytes(renderer.draws.bucket_clusters())
            .expect("the bucket cluster table is one of this recorder's buffers")
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("a four-byte chunk")))
            .collect();

        let mut dispatched = Vec::new();
        let mut index_binds = 0;
        for command in commands_in_pass(&recorder, "forward") {
            match command {
                Command::DrawMeshTasksIndirect(draw) => dispatched.push(draw),
                Command::DrawMeshTasks { .. } => panic!(
                    "the mesh path recorded a CPU-sized dispatch, which is the extent \
                     the instance cull is supposed to have taken over"
                ),
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
            dispatched, expected,
            "one indirect dispatch per bucket, each reading its own argument structure \
             out of this frame's buffer"
        );
        assert_eq!(
            clusters.len(),
            renderer.bucket_constants.len(),
            "one x extent per bucket: {clusters:?}"
        );
        assert!(
            clusters.iter().all(|&count| count > 0),
            "a bucket with no clusters dispatches nothing, whatever the arguments say: \
             {clusters:?}"
        );
        // **The x extents are not all the same number**, which is what makes
        // the table a statement about each bucket's own mesh. Two
        // single-cluster meshes would let a table hard-coded to one cluster
        // pass; the open box is five, one per face.
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
    /// It used to poison every counter with `0xAB` and assert `begin_frame`
    /// had written zeroes over them — the poison being the whole test, because
    /// the null backend runs no shader and a counter nothing wrote reads as the
    /// zero it was created with. The counters are device-local now, so nothing
    /// on the host writes them and [`Recorder::buffer_bytes`] holds nothing for
    /// them: the zero is a dispatch inside the frame, and a backend that runs no
    /// shader cannot observe it at all.
    ///
    /// So the poison moved to where a shader actually runs — `crcbl-vk`'s
    /// `draw_gen` end-to-end fills them with a sentinel and reads back the
    /// generated arguments — and what is checkable *here* is the schedule: that
    /// the frame contains the clearing pass, that it writes exactly those
    /// buffers, and that the graph orders the two accumulating passes after it.
    /// Each of the counts below goes red if a buffer is dropped from the
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
                4,
                "round {round}: the survivor count, the draw arguments, the draw counts \
                 and the mesh-dispatch arguments, and nothing else: {barriers:?}"
            );
            assert!(
                barriers
                    .iter()
                    .all(|barrier| barrier.to == ResourceState::ShaderReadWrite),
                "round {round}: every one of them is written by this pass: {barriers:?}"
            );
            // The ones the frame leaves as indirect arguments are the three a
            // driver reads; the survivor count rests in a shader read. That
            // split is what names them without a handle to compare.
            assert_eq!(
                barriers
                    .iter()
                    .filter(|barrier| barrier.from == ResourceState::IndirectArgument)
                    .count(),
                3,
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
                4,
                "round {round}: and so do the draw arguments, the draw counts and the \
                 mesh-dispatch arguments — plus `docs/plan/25-lod.md`'s hysteresis state, \
                 which the clearing pass does *not* zero and which is behind the same \
                 kind of barrier for the opposite reason: it is the one buffer here \
                 carrying a value out of the previous frame, so what it needs ordering \
                 against is that frame rather than this one's clear"
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
        // The camera's compute triple, then one per shadow cascade, then the
        // depth-only pass they feed and the colour pass that samples it.
        let mut expected: Vec<String> = Vec::new();
        for _ in 0..=shadow::CASCADES {
            expected.extend(
                ["clear-counters", "cull", "draw-args"]
                    .into_iter()
                    .map(str::to_string),
            );
        }
        expected.extend(
            ["shadow", "forward", "tonemap"]
                .into_iter()
                .map(str::to_string),
        );
        assert_eq!(
            passes, expected,
            "each cull's three compute passes come first, and in that order"
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

    /// One more frame than the uniform ring is deep, so at least one lap both
    /// reuses a slot and follows a lap that already sampled the atlas.
    const SHADOW_LAPS: usize = FRAMES_IN_FLIGHT + 1;

    /// **Every frame's first barrier on the shadow atlas names what the previous
    /// frame left it in**, and the same for its placeholder.
    ///
    /// # Why this cannot be read off one frame
    ///
    /// The atlas is neither a transient nor a swapchain image. The graph cannot
    /// look its state up in the [`TransientPool`], and no acquire semaphore sits
    /// between one frame's use of it and the next — so
    /// [`ForwardRenderer::add_passes`] *declares* it, out of
    /// [`ForwardRenderer::shadow_imported`], and the declaration is the only
    /// thing making the transition true.
    ///
    /// Declaring [`ResourceState::Undefined`] on a frame after the first is the
    /// failure this catches. It expands to `srcStageMask = NONE,
    /// srcAccessMask = NONE` (`crcbl_vk::conv::state_masks`), so the transition
    /// into the depth-only pass's attachment state orders against nothing — while
    /// the previous frame's colour pass is still sampling the very same image.
    /// Vulkan reports it as `SYNC-HAZARD-WRITE-AFTER-READ` with
    /// `read_barriers: 0`; every other backend resolves it however the driver
    /// likes.
    ///
    /// # Why the command stream is the observable
    ///
    /// Nothing above the seam can see a wrong declaration: the frame still
    /// renders, the golden still matches, and no state is ever read back. So this
    /// replays the recorded stream with a tracker, which is the same check a
    /// validation layer performs and needs no GPU at all — the shape
    /// `crcbl::screenshot`'s
    /// `every_readback_barrier_declares_the_state_the_image_is_actually_in` uses.
    /// One difference: `Undefined` is **not** waved through after the first
    /// touch. It is true for a swapchain image because the acquire's semaphore
    /// makes it true, and there is no such semaphore here.
    #[test]
    fn the_shadow_atlas_enters_each_frame_in_the_state_the_last_one_left_it() {
        use crcbl_hal::null::Command;

        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut renderer =
            ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
        let atlas = renderer.shadow_atlas();
        let placeholder = renderer.shadow_placeholder;
        let mut pool = crate::TransientPool::new();
        let imported = swapchain_image(device);
        let mut recorded = Vec::with_capacity(SHADOW_LAPS);

        for _ in 0..SHADOW_LAPS {
            renderer
                .begin_frame(
                    device,
                    &Camera::default(),
                    &DirectionalLight::default(),
                    Mat4::IDENTITY,
                    (64, 48),
                )
                .expect("write");
            let mut graph = crate::RenderGraph::new(queue);
            let target = graph.import_image("target", imported);
            renderer.add_passes(&mut graph, target, (64, 48));
            let compiled = graph.compile(&pool).expect("a legal frame");
            let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("shadow lap"),
                queue,
            });
            compiled
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("the graph executed");
            recorded.push(encoder.finish().expect("recording succeeded"));
        }

        // The two images the renderer owns across frames. A transient's state is
        // the pool's business and the fake swapchain image's is the acquire's, so
        // neither belongs in here.
        let mut tracked = [
            (atlas, ResourceState::Undefined),
            (placeholder, ResourceState::Undefined),
        ];
        let mut atlas_writes = 0usize;
        let mut named = 0usize;
        for command in recorder.commands() {
            let Command::Barrier { images, .. } = command else {
                continue;
            };
            for barrier in images {
                let Some((_, state)) = tracked
                    .iter_mut()
                    .find(|(handle, _)| *handle == barrier.image)
                else {
                    continue;
                };
                assert_eq!(
                    barrier.from, *state,
                    "a barrier declared {:?} -> {:?} on an image the frame before it left in \
                     {:?}. `Undefined` carries no source scope, so a transition declaring it \
                     orders against nothing and the previous frame's sampled read is still in \
                     flight underneath it.",
                    barrier.from, barrier.to, *state
                );
                if barrier.image == atlas && barrier.to == ResourceState::DepthStencilWrite {
                    atlas_writes += 1;
                }
                *state = barrier.to;
                named += 1;
            }
        }

        // Both halves of the loop above are silent on an empty stream, so the
        // shape it walked is asserted rather than assumed: one transition into
        // the depth-only pass's attachment state per lap, the last one included.
        assert_eq!(
            atlas_writes, SHADOW_LAPS,
            "{named} barrier(s) named the atlas or its placeholder across {SHADOW_LAPS} laps, \
             {atlas_writes} of them taking the atlas into DepthStencilWrite — the shadow pass \
             writes it every frame, so anything else means the loop above asserted on a stream \
             that does not contain the transition it is about"
        );
        let (_, ended) = tracked[0];
        assert_eq!(
            ended,
            ResourceState::ShaderRead,
            "the import promises to hand the atlas back in the state the next frame declares"
        );

        renderer.destroy(device);
        for commands in recorded {
            device.destroy_command_buffer(commands);
        }
        pool.destroy(device);
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

    /// Everything one labelled pass recorded, and nothing any other pass did.
    ///
    /// **Every draw assertion below is scoped through this**, and the reason is
    /// topic 18: the frame now records the same indirect call the colour pass
    /// records once per shadow cascade as well, out of that cascade's own
    /// arguments. A test filtering the whole stream for `DrawIndexedIndirect`
    /// would see one per cascade as well as the colour pass's, and could not
    /// say which pass any of them belonged to — so "the forward pass records one
    /// call per bucket" would become a claim about a total, which a shadow pass
    /// recording the wrong thing could satisfy.
    fn commands_in_pass(
        recorder: &crcbl_hal::null::Recorder,
        label: &str,
    ) -> Vec<crcbl_hal::null::Command> {
        use crcbl_hal::null::Command;

        let mut inside = false;
        let mut out = Vec::new();
        for command in recorder.commands() {
            if let Some((_, recorded)) = command.opens_pass() {
                inside = recorded == Some(label);
                continue;
            }
            if matches!(command, Command::EndRenderPass | Command::EndComputePass) {
                inside = false;
                continue;
            }
            if inside {
                out.push(command);
            }
        }
        assert!(!out.is_empty(), "no pass labelled `{label}` recorded work");
        out
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
