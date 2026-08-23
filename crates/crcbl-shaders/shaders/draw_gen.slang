// crcbl-targets: spirv, wgsl, msl, dxil

// Draw generation on the GPU: `cull.slang`'s compacted visible list in, one
// indexed-indirect argument structure per draw bucket out.
//
// `docs/plan/03-gpu-driven-rendering.md` §3.3's second half — "compacted visible
// instance list → `draw_indexed_indirect` records + count buffer" — and its
// 2026-07-27 correction, which is the half that decides the shape here:
// "**draw binning is a fixed bucket table**… the cull shader scatters compacted
// instances into per-bucket indirect draws with per-bucket count buffers.
// `IndirectPerBatch` emits the same buckets as per-batch indirect draws." The
// scatter is here rather than in `cull.slang` because that pass's output is
// checked against a CPU oracle and consumed by more than this; nothing about a
// bucket belongs in it.
//
// # Level of detail, for the paths with no amplification stage
//
// `docs/plan/25-lod.md`'s "Runtime selection" splits by geometry path: the mesh
// path descends a mesh's cluster DAG per cluster in its amplification stage,
// and "on `IndirectCount` and `IndirectPerBatch` it runs in the cull compute
// pass and takes a **uniform cut** — every cluster at one depth, which is
// exactly a whole-mesh level — drawn as ordinary index ranges."
//
// That decision lives *here*, in the half of the cull/draw-argument pair that
// chooses an instance's bucket, because on this path a level **is** a bucket: a
// DAG's levels are separate mesh table entries, so selecting one is selecting a
// mesh, and the routing below already knows how to send an instance to the
// bucket for a mesh. `uniform_level` is the rule and
// `crcbl_shaders::level_select` is both its documentation and its host twin.
//
// A mesh with no hierarchy carries a record that resolves to itself, so nothing
// here branches on whether a mesh has levels — and the mesh path hands over a
// record whose top level is zero, which resolves to the mesh the instance
// already names and is how a device that already selects per cluster avoids a
// second, coarser cut on top of it.
//
// # And the hysteresis, for *both* paths
//
// That plan also asks for "**hysteresis** on the threshold (switch-up and
// switch-down differ)", which needs the previous frame's answer and therefore
// needs somewhere to keep it. `group_state` below is that somewhere: one word
// per (instance, group), written here and read by `mesh_cluster.slang`.
//
// It is written *here* on every geometry path, including the one that takes no
// uniform cut at all, because this is the only pass with one invocation per
// visible instance — so every element has exactly one writer, and the
// amplification stage's thousands of task groups per instance need only read.
// `select_level` therefore judges every group whatever level it returns, and a
// mesh-path record suppresses the cut with a top level of zero rather than with
// a group count of zero.
//
// # Why the arguments are per bucket and not per visible instance
//
// A draw's instances have to be *addressable* by the vertex stage, and the only
// per-draw number all four targets agree on is `SV_InstanceID` when the draw's
// first instance is zero — `mesh.slang`'s header measures the four lowerings and
// the disagreement is not small: SPIR-V and DXIL subtract the draw's base
// instance back out, WGSL and MSL do not. So an argument structure per visible
// instance, distinguished by its `first_instance`, reaches the shader as the
// instance index on two targets and as zero on the other two, and no source can
// be right on all four.
//
// What *is* portable is a draw of `instance_count` instances at first instance
// zero: `SV_InstanceID` is then `0..instance_count` everywhere. So each bucket
// gets one argument structure whose instance count this pass writes, plus a
// contiguous run of instance indices the vertex stage indexes with that id —
// which is what `visible_instances` below holds and what `mesh.slang`'s
// `DrawConstants::base` points into.
//
// The CPU therefore records **one draw per bucket**, a number fixed when the
// bucket table is built, whatever the scene holds — which is the property §3.3
// is about.
//
// # The instance count is the atomic, and that is deliberate
//
// A bucket's `instance_count` is the slot allocator: a scattering invocation
// takes its place in the run by incrementing it. Writing it afterwards would
// need a second dispatch, because nothing inside one dispatch can wait for every
// other invocation to have finished counting.
//
// The rest of each argument — index count, first index, and the two bases that
// are always zero — is written by the invocation that *owns* that bucket, one
// per bucket, at words the scattering invocations never touch. Different words,
// so there is no race between the two halves and no ordering to establish.
//
// The caller zeroes the argument buffer before the dispatch, exactly as it
// zeroes `cull.slang`'s counter, and for the same reason: this pass only ever
// adds.
//
// # The mesh path's dispatch extents are written here too
//
// §3.5's geometry path draws no indirect *draw* at all — it launches one
// workgroup per (cluster, surviving instance) — but the number of survivors is
// this pass's, so the extents are an indirect argument on exactly the same
// terms. `mesh_args` below is that structure, one per bucket, and it is what
// makes the instance cull reduce **work** rather than only output: a CPU-sized
// dispatch has to cover every slot the instance pool ever handed out and let
// the groups past the survivors return without emitting.
//
// It is a buffer of its own rather than three more words on the structure
// above, and that is a render-graph constraint rather than tidiness: a mesh
// pipeline reads `args` as data in the same pass, a resource is in one
// resource state at a time, and `IndirectArgument` and a shader read are two.
//
// # A bucket's run cannot overflow, by arithmetic rather than by assertion
//
// Every scattering invocation contributes exactly one instance to exactly one
// bucket, so the sum over buckets is the number of invocations that scattered —
// which `visible_count` below clamps to `min(visible_capacity, bucket_capacity)`
// before anything is read. A single bucket therefore cannot exceed
// `bucket_capacity`, and the write below needs no bounds check that would be
// unreachable if it did.
//
// An instance list too small for the scene is visible in *`cull.slang`'s*
// counter, which is the true survivor count and is deliberately unbounded; this
// pass draws the prefix that fits and the counter is where the caller sees that
// it did not all fit.
//
// # The structs are declared again here, and that is the rule
//
// `GpuInstance` and `GpuMesh` are also declared in `mesh.slang` and
// `cull.slang`. There is no shared header — `tools/compile-shaders.sh` hashes
// one source per artifact, so an `#include` would be a file whose edits nothing
// downstream notices — and what keeps the copies honest is
// `crcbl_shaders::cull`'s
// `the_shared_structs_are_declared_identically_in_every_shader`, which reads
// every source and compares the declarations character for character.
//
// # Nine bindings, because WebGPU guarantees eight storage buffers per stage
//
// **This pass bound fourteen storage buffers and could not be created on a
// WebGPU device at the guaranteed limit.** `maxStorageBuffersPerShaderStage` is
// 8 in the WebGPU specification's default limits; SwiftShader — every CI runner
// without a GPU — offers 10, and an RDNA-3 desktop offers 16. So the forward
// renderer ran in a browser only on generous hardware.
//
// Splitting the fourteen across two bind groups does **not** help and was tried:
// the limit is validated at `createPipelineLayout` as a sum over every bind
// group layout in the layout, per stage, and a 7+7 split was refused with the
// same message. The count itself has to come down, which means fewer *bindings*
// — so buffers that were separate are now regions of one buffer, and the host
// hands over where each region starts.
//
// Three merges bring fourteen to eight, and each one is a place where a reader
// has to know the layout rather than the name:
//
// * `tables` is every host-written table this pass reads — the bucket table, the
//   per-bucket cluster counts, and `docs/plan/25-lod.md`'s three selection
//   tables — as one flat array of words. **The cost is the typed
//   `StructuredBuffer`**: `LevelGroup`'s five floats are read back through
//   `asfloat`, and the word each field lands on is a named constant below
//   rather than a struct member Slang lays out. `crcbl_shaders::level_select` is
//   the host encoder and pins those constants.
// * `visible_instances` holds `cull.slang`'s survivor list **and** the per-bucket
//   runs this pass scatters into, in that order. One binding rather than two
//   because a read-only descriptor and a writable descriptor of one buffer in
//   one bind group is a usage conflict on WebGPU — so the survivors are read
//   back through the writable view.
// * `counts_and_mesh_args` holds the per-bucket draw counts and then the
//   per-bucket mesh-dispatch extents. Both are indirect arguments on every path
//   that reads either, so they are never in two resource states at once — which
//   is exactly why `args` is still a buffer of its own: the mesh path reads
//   *those* as data in the same pass that executes these.
//
// What a caller must not assume: that any of `DrawGen`'s buffer accessors hand
// back distinct buffers. `visible` and `runs` are one buffer, and so are `counts`
// and `mesh_args`; the byte offsets are `DrawGen::bucket_base`,
// `DrawGen::count_offset` and `DrawGen::mesh_args_offset`, and reading a region
// at offset zero because it used to be its own allocation reads the region in
// front of it.

/// One drawable object, matching `crcbl_shaders::mesh::GpuInstance` and
/// `mesh.slang`'s copy.
struct GpuInstance
{
    /// Model → sector. Rotation, translation and scale, **non-uniform
    /// included**: the raster stages carry a normal through `normal_basis`, the
    /// upper-left 3×3's cofactor matrix, rather than through the 3×3 itself.
    float4x4 transform;
    /// Which mesh to draw: an index into `meshes` below. The vertex stage
    /// resolves this instance's base vertex through it, which is what lets one
    /// draw cover instances of different meshes.
    uint mesh;
    /// Which material to shade with: an index into the material table
    /// `mesh.slang` shades through. Nothing here reads it — this pass buckets
    /// survivors by mesh, and a bucket key that included the material is the
    /// longer key §3.4's binning wants rather than anything this slice needs.
    uint material;
    /// Which sector `transform` is relative to. **Reserved**: the f64
    /// sector→camera offset table and the addition that would make this mean
    /// anything are §3.3's, so today every instance is in sector 0 and
    /// `transform` is effectively model → world.
    uint sector;
    /// Per-instance bits. Bit 0 is the live bit `cull.slang` reads; an index in
    /// the visible list is a live instance by construction, so nothing here
    /// asks.
    uint flags;
};

/// One resident mesh's range in the geometry pools **and its local-space
/// bounds**, matching `crcbl_shaders::mesh::GpuMesh` and `mesh.slang`'s copy.
struct GpuMesh
{
    /// This mesh's first vertex in the pool, added to every index a draw of it
    /// reads. The pool stores indices mesh-relative, so this is the only thing
    /// that says where the mesh actually is.
    uint base_vertex;
    /// This mesh's first index in the index pool.
    uint base_index;
    /// How many indices the mesh has. Zero for an entry naming no mesh — see
    /// `crcbl_render::MeshPool`, which clears an entry when its mesh is freed.
    uint index_count;
    /// Lowest corner of the mesh's **local-space** axis-aligned bounding box.
    float min_x;
    float min_y;
    float min_z;
    /// Highest corner of the same box.
    float max_x;
    float max_y;
    float max_z;
};

/// Words in one indexed-indirect argument structure, matching
/// `crcbl_shaders::draw_gen::DRAW_ARGS_WORDS`.
///
/// The layout is `VkDrawIndexedIndirectCommand`, which is also
/// `D3D12_DRAW_INDEXED_ARGUMENTS` and `wgpu`'s `DrawIndexedIndirectArgs`: five
/// 32-bit words, in the order the constants below name.
static const uint DRAW_ARGS_WORDS = 5;

/// Indices within one argument structure. Named rather than written as literals
/// at the stores below, for `PLANE_COUNT`'s reason in `cull.slang`: a bare `3`
/// in a store is a word nothing can be checked against.
static const uint ARG_INDEX_COUNT = 0;
static const uint ARG_INSTANCE_COUNT = 1;
static const uint ARG_FIRST_INDEX = 2;
static const uint ARG_VERTEX_OFFSET = 3;
static const uint ARG_FIRST_INSTANCE = 4;

/// Words in one mesh-dispatch argument structure, matching
/// `crcbl_shaders::draw_gen::MESH_ARGS_WORDS`.
///
/// The layout is `VkDrawMeshTasksIndirectCommandEXT`, which is also
/// `D3D12_DISPATCH_MESH_ARGUMENTS` and Metal's
/// `MTLDispatchThreadgroupsIndirectArguments`: three 32-bit workgroup counts,
/// in the order the constants below name.
static const uint MESH_ARGS_WORDS = 3;

/// Indices within one mesh-dispatch argument structure, on `ARG_*`'s terms.
static const uint MESH_ARG_GROUP_X = 0;
static const uint MESH_ARG_GROUP_Y = 1;
static const uint MESH_ARG_GROUP_Z = 2;

/// Which element of the culling statistics holds `cull.slang`'s survivor count.
///
/// The same number as `crcbl_shaders::cull::INSTANCE_SURVIVOR_WORD`, held in
/// step with it by that module's
/// `the_shaders_index_the_culling_stats_at_the_words_this_crate_says`. Named
/// here rather than written as a `0` because this shader reads a *shared*
/// statistics block: the element beside it is the amplification stage's.
static const uint INSTANCE_SURVIVOR_WORD = 0;

/// One group of a mesh's cluster DAG, matching
/// `crcbl_shaders::level_select::LevelGroup`.
///
/// Six scalars and not a `float3` in sight, for `GpuMesh`'s reason above: under
/// `std430` a `float3` is 16-byte aligned and would put four bytes of padding in
/// front of itself.
struct LevelGroup
{
    /// The level whose clusters this group grouped. An expanded group is
    /// therefore the deepest a uniform cut may go.
    uint level;
    /// How far this group's simplification may have moved the surface, in the
    /// mesh's own units of length.
    float error;
    /// Centre of the sphere the error is projected from, in the mesh's own
    /// space.
    float center_x;
    float center_y;
    float center_z;
    /// Distance from that centre to the furthest point the sphere contains.
    float radius;
};

/// Words one `LevelGroup` occupies in `tables`, matching
/// `crcbl_shaders::level_select::LEVEL_GROUP_STRIDE` divided by four.
static const uint LEVEL_GROUP_WORDS = 6;

/// Which word of a group's record holds which field.
///
/// Named rather than written as literals in `level_group_at` below, for
/// `ARG_INDEX_COUNT`'s reason: six scalars in a row permute silently — a radius
/// read as an error selects a level rather than crashing — and
/// `crcbl_shaders::level_select`'s
/// `the_shader_reads_the_selection_records_at_the_words_this_module_encodes`
/// is what holds these against the bytes the host writes.
static const uint LEVEL_GROUP_LEVEL = 0;
static const uint LEVEL_GROUP_ERROR = 1;
static const uint LEVEL_GROUP_CENTER_X = 2;
static const uint LEVEL_GROUP_CENTER_Y = 3;
static const uint LEVEL_GROUP_CENTER_Z = 4;
static const uint LEVEL_GROUP_RADIUS = 5;

/// Where one mesh's DAG is, matching `crcbl_shaders::level_select::MeshLevels`.
///
/// Indexed by `GpuInstance::mesh`, so `mesh_levels` below is parallel to the
/// mesh table and every entry an instance can name has to be filled. A mesh with
/// no hierarchy carries zero groups and a top level of zero, which makes
/// `uniform_level` answer zero without reading anything — and that is also how
/// the mesh-shader path suppresses this selection, since a device descending the
/// DAG per cluster must not have a coarser cut applied to the same instance
/// first.
struct MeshLevels
{
    /// This mesh's first entry in `level_groups`.
    uint first_group;
    /// How many it has. Zero for a mesh with no hierarchy.
    uint group_count;
    /// This mesh's first entry in `level_meshes`, whose element `d` is the mesh
    /// id that draws level `d`.
    uint first_level;
    /// The coarsest level this mesh has, so its run of `level_meshes` is
    /// `top_level + 1` long.
    ///
    /// The top *level* and not the level count: the search below starts here,
    /// and a count would have to be decremented — which on a zero written by
    /// mistake wraps to `0xffffffff` and indexes the level table with it.
    uint top_level;
};

/// Words one `MeshLevels` occupies in `tables`, matching
/// `crcbl_shaders::level_select::MESH_LEVELS_STRIDE` divided by four.
static const uint MESH_LEVELS_WORDS = 4;

/// Which word of a mesh's record holds which field, on `LEVEL_GROUP_LEVEL`'s
/// terms exactly.
static const uint MESH_LEVELS_FIRST_GROUP = 0;
static const uint MESH_LEVELS_GROUP_COUNT = 1;
static const uint MESH_LEVELS_FIRST_LEVEL = 2;
static const uint MESH_LEVELS_TOP_LEVEL = 3;

/// Everything an invocation needs that is not buffer contents, matching
/// `crcbl_shaders::draw_gen::Params`.
///
/// A uniform buffer rather than push constants, for the reason
/// `compute_probe.slang` gives: WGSL has no push constants, and this shader
/// declares that target.
struct DrawGenParams
{
    /// Buckets in the table: `bucket_meshes` has this many entries and so does
    /// the argument buffer.
    uint bucket_count;
    /// Instance indices one bucket's run can hold, which is also the stride
    /// between two buckets' runs in `visible_instances`.
    uint bucket_capacity;
    /// Elements of `visible` that hold anything — `cull.slang`'s capacity. Its
    /// counter can exceed this, which is how an overflow is visible at all.
    uint visible_capacity;
    /// How many groups one instance's run of `group_state` holds — every
    /// resident mesh's group count summed, and the stride between two instances
    /// in it. The same number `mesh_cluster.slang`'s
    /// `ClusterDrawConstants::group_stride` carries, because that stage reads
    /// what this one writes.
    uint group_stride;
    /// Where the per-bucket cluster counts start in `tables`, in words.
    ///
    /// The bucket table itself starts at word zero, so it needs no offset; every
    /// region after it does. **Host-computed, because only the host knows how
    /// long the region in front of each one is** — the bucket table is as long
    /// as there are buckets, the per-mesh records as long as the mesh table, and
    /// the group and level runs as long as the resident hierarchies came to.
    /// `crcbl_render::DrawGen` is what packs them and fills these in.
    uint bucket_clusters_at;
    /// Where the per-mesh `MeshLevels` records start in `tables`, in words.
    uint mesh_levels_at;
    /// Where the `LevelGroup` records start in `tables`, in words.
    uint level_groups_at;
    /// Where the level → mesh id table starts in `tables`, in words.
    uint level_meshes_at;
    /// The eye this frame's selection is projected from, in world space, in
    /// `xyz`. **The camera's**, on every pass this shader runs for, including the
    /// shadow cascades — unless the host pinned it, for which see the second
    /// half of this comment.
    ///
    /// A cascade's copy of `mesh.slang`'s `FrameUniforms::camera_position` holds
    /// the *sun* instead, and the two are answering different questions rather
    /// than disagreeing: that one feeds the amplification stage's normal-cone
    /// test, which asks which way a cluster faces relative to the viewer, and a
    /// shadow map's viewer is the light. Detail is not that question — what a
    /// coarser caster costs is a shadow edge displaced by the group's error, and
    /// that displacement is seen by the camera at the camera's distance. A
    /// directional sun has no position to measure a distance from at all.
    ///
    /// **The same split, read the other way, is what lets a host freeze the
    /// cut.** `crcbl_render::ForwardRenderer::set_frozen_selection_eye` pins
    /// this field at one viewpoint and changes nothing else: the frustum planes
    /// beside it in `cull.slang` come from the frame's view-projection, and
    /// `mesh_cluster.slang`'s normal cone asks `FrameUniforms::camera_position`
    /// which way a cluster faces the *viewer* — so a reviewer flies away from
    /// the pinned eye and the frame is still culled, faced and drawn from where
    /// they actually are, while the cut stays the one that eye chose. That is
    /// the only way a cut can be checked at all: from the eye that chose it, a
    /// cut too coarse and a cut exactly right have the same silhouette, which
    /// is what a screen-space error budget means.
    float4 camera_position;
    /// `docs/plan/25-lod.md`'s selection numbers: `x` is how many pixels one
    /// unit of length subtends one unit from the eye, `y` is the pixel budget an
    /// unexpanded group's projected error must exceed to expand, and `z` is the
    /// budget an already-expanded group is held down to before it collapses
    /// again — that plan's "switch-up and switch-down differ".
    ///
    /// The colour pass takes the camera's three; a shadow cascade takes the same
    /// `x` and both budgets scaled by `crcbl_render::SHADOW_LOD_BIAS`, which is
    /// that plan's shadow LOD bias. Scaling both by one positive constant is what
    /// keeps the cut a cover: the rule below is unchanged and its monotonicity
    /// turns on the projected error, not on the constants.
    float4 lod_params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<DrawGenParams> gen;

/// The instance array, read for `GpuInstance::mesh` and `transform`: which
/// bucket an instance scatters into, and — since `docs/plan/25-lod.md`'s uniform
/// cut runs here — where its mesh's group spheres are in world space.
[[vk::binding(1, 0)]]
StructuredBuffer<GpuInstance> instances;

/// The mesh table, indexed by `bucket_meshes`. The index range in a bucket's
/// argument structure comes from here — which is the whole translation this
/// pass performs, and what makes the arguments checkable against the draws a
/// CPU would have recorded from the same table.
[[vk::binding(2, 0)]]
StructuredBuffer<GpuMesh> meshes;

/// How many survivors `cull.slang` found, in element
/// `crcbl_shaders::cull::INSTANCE_SURVIVOR_WORD`. The *true* count, so it can
/// exceed `DrawGenParams::visible_capacity` and is clamped below before it
/// indexes anything.
///
/// The frame's whole culling-statistics block rather than a counter of its own —
/// see `cull.slang`, which writes it.
[[vk::binding(3, 0)]]
StructuredBuffer<uint> visible_count;

/// Every host-written table this pass reads, as one flat array of words.
///
/// **One binding for five tables**, because a WebGPU device guarantees only
/// eight storage buffers per shader stage — see the header. The regions, in
/// order, and what indexes each:
///
/// | region | starts at | stride | indexed by |
/// |---|---|---|---|
/// | bucket table: which mesh each bucket draws | word 0 | 1 | bucket |
/// | per-bucket cluster counts | `gen.bucket_clusters_at` | 1 | bucket |
/// | `MeshLevels` per mesh | `gen.mesh_levels_at` | `MESH_LEVELS_WORDS` | `GpuInstance::mesh` |
/// | `LevelGroup` per DAG group | `gen.level_groups_at` | `LEVEL_GROUP_WORDS` | `MeshLevels::first_group + i` |
/// | level → mesh id | `gen.level_meshes_at` | 1 | `MeshLevels::first_level + d` |
///
/// Every one of them is written when a mesh becomes resident and not rewritten
/// per frame, which is what lets them share one upload: they change together or
/// not at all.
///
/// The accessors below are the only readers. Nothing indexes this array
/// directly — a bare offset arithmetic at a use site is exactly the drift the
/// named word constants exist to prevent.
[[vk::binding(4, 0)]]
StructuredBuffer<uint> tables;

/// `cull.slang`'s compacted survivors, then the per-bucket runs of instance
/// indices the vertex stage reads.
///
/// **Two regions in one buffer, and read *and* written through one descriptor.**
/// The survivors occupy words `0..gen.visible_capacity`, in whatever order that
/// pass's atomic handed slots out — nothing here depends on the order — and
/// bucket `b`'s run starts at `gen.visible_capacity + b * gen.bucket_capacity`,
/// which is what `mesh.slang`'s `DrawConstants::base` carries and what
/// `SV_InstanceID` walks.
///
/// Writable rather than a read-only view of the survivors beside a writable view
/// of the runs: binding one buffer twice, once each way, in a single bind group
/// is a usage conflict on WebGPU. A `RWStructuredBuffer` reads perfectly well,
/// and the survivor half is read-only here by what this shader does with it.
[[vk::binding(5, 0)]]
RWStructuredBuffer<uint> visible_instances;

/// The indirect arguments, `DRAW_ARGS_WORDS` per bucket, as a flat array of
/// words.
///
/// Flat and atomic rather than an array of structs because word
/// `ARG_INSTANCE_COUNT` **is** the slot allocator — see the header. The other
/// four words are plain stores through the same view, which is what
/// `Atomic<uint>::store` is for.
///
/// **Still a buffer of its own**, unlike the counts and extents below: the mesh
/// path reads these as *data* in the same pass that executes those as indirect
/// arguments, and a resource is in one resource state at a time.
[[vk::binding(6, 0)]]
RWStructuredBuffer<Atomic<uint> > args;

/// One draw count per bucket, then the per-bucket mesh-dispatch arguments.
///
/// The counts occupy words `0..gen.bucket_count`: `1` for a bucket with something
/// in it, and the zero the clearing pass wrote for a bucket with nothing. That is
/// what `GeometryPath::IndirectCount`'s call reads, and the reason the CPU never
/// learns whether a bucket drew — `IndirectPerBatch` passes a count of one and
/// leans on the empty bucket's zero instance count instead, which draws nothing
/// just as effectively.
///
/// The extents follow at `gen.bucket_count`, `MESH_ARGS_WORDS` per bucket.
/// **They are what makes the cluster cull skip work rather than only output**:
/// `mesh_cluster.slang`'s stages run one workgroup per (cluster, surviving
/// instance), and how many instances survived is a number only this pass has —
/// so a CPU-sized dispatch has to use the instance *pool's* slot count and let
/// the groups past the survivors return early. Read from here instead, the
/// groups are never launched.
///
/// One buffer for both because **every path that reads either reads it as an
/// indirect argument**: the two indirect tails execute the counts and never look
/// at the extents, the mesh path executes the extents and never looks at the
/// counts, and no pass wants the buffer in two resource states at once.
///
/// Flat and atomic for `args`' reason, and the same division of labour: word
/// `MESH_ARG_GROUP_Y` is incremented by each scattering invocation, exactly as
/// `ARG_INSTANCE_COUNT` is, and the other two are plain stores by the
/// invocation that owns the bucket.
[[vk::binding(7, 0)]]
RWStructuredBuffer<Atomic<uint> > counts_and_mesh_args;

/// `docs/plan/25-lod.md`'s hysteresis state: one word per (instance, group),
/// `1` where that instance had that group expanded last frame.
///
/// **The one place either geometry path's expansion is decided**, and the reason
/// a decision that has to survive a frame can be made at all. `select_level`
/// below reads an element and writes it back; `mesh_cluster.slang`'s
/// amplification stage only reads, at `instance * group_stride + group`, which
/// is the same address this pass wrote.
///
/// **One buffer, not one per frame in flight**, and that is the whole
/// cross-frame story:
///
/// * *Within* a dispatch there is nothing to order — invocation `i` owns visible
///   instance `i`'s whole run and no other invocation touches it, so every
///   element has exactly one reader and one writer.
/// * *Between* frames the graph is what orders them. The buffer is imported in
///   `ResourceState::ShaderReadWrite` and declared the same here, and
///   `ResourceState::needs_barrier` answers `true` for any transition involving
///   a write — so the first barrier of every frame names a real source scope
///   covering the last frame's writes and the last frame's mesh-stage reads.
/// * A **ring** is what would break it, not what would save it: an instance the
///   frustum rejected writes nothing, so its slot in a fresh ring buffer would
///   hold whatever was there frames ago, and a state that is not this mesh's own
///   is a state that need not be monotone up the DAG — which is a crack. One
///   persistent buffer keeps a culled instance's history instead, and a history
///   is always monotone because every frame's update preserves it.
///
/// Zeroed once when the renderer builds it, which is where the induction starts.
[[vk::binding(8, 0)]]
RWStructuredBuffer<uint> group_state;

/// Which mesh bucket `bucket` draws, as an index into `meshes`.
uint bucket_mesh(uint bucket)
{
    return tables[bucket];
}

/// How many clusters bucket `bucket`'s mesh has — the x extent of that bucket's
/// mesh dispatch, and the one number in the extents that is not the GPU's.
///
/// Zero on every geometry path with no mesh stage, where nothing reads the
/// extents at all.
uint bucket_clusters(uint bucket)
{
    return tables[gen.bucket_clusters_at + bucket];
}

/// Where mesh `mesh`'s DAG is. Indexed by `GpuInstance::mesh`, so the region is
/// parallel to the mesh table and every entry an instance can name is filled.
MeshLevels mesh_levels_of(uint mesh)
{
    uint at = gen.mesh_levels_at + mesh * MESH_LEVELS_WORDS;
    MeshLevels levels;
    levels.first_group = tables[at + MESH_LEVELS_FIRST_GROUP];
    levels.group_count = tables[at + MESH_LEVELS_GROUP_COUNT];
    levels.first_level = tables[at + MESH_LEVELS_FIRST_LEVEL];
    levels.top_level = tables[at + MESH_LEVELS_TOP_LEVEL];
    return levels;
}

/// Group `group` of the concatenated DAG groups, its five floats read back
/// through `asfloat`.
///
/// The bit pattern the host wrote is the bit pattern a float load would have
/// produced from a typed buffer — `crcbl_shaders::level_select::LevelGroup::to_bytes`
/// writes `f32::to_le_bytes` and this reads the same word — so this is a
/// reinterpretation and not a conversion.
LevelGroup level_group_at(uint group)
{
    uint at = gen.level_groups_at + group * LEVEL_GROUP_WORDS;
    LevelGroup record;
    record.level = tables[at + LEVEL_GROUP_LEVEL];
    record.error = asfloat(tables[at + LEVEL_GROUP_ERROR]);
    record.center_x = asfloat(tables[at + LEVEL_GROUP_CENTER_X]);
    record.center_y = asfloat(tables[at + LEVEL_GROUP_CENTER_Y]);
    record.center_z = asfloat(tables[at + LEVEL_GROUP_CENTER_Z]);
    record.radius = asfloat(tables[at + LEVEL_GROUP_RADIUS]);
    return record;
}

/// The mesh id drawing entry `level` of the concatenated level table.
///
/// A DAG is several mesh table entries because every level is decimated
/// separately and its vertices belong to no vertex of the level below — so a
/// uniform cut selects a *mesh*, and the bucket table then routes it exactly as
/// it routes a mesh with no hierarchy at all.
uint level_mesh_at(uint level)
{
    return tables[gen.level_meshes_at + level];
}

/// Where bucket `bucket`'s run of surviving instance indices starts in
/// `visible_instances`, past the survivor list that shares the buffer.
///
/// The same arithmetic `crcbl_render::DrawGen::bucket_base` performs, and what
/// `mesh.slang`'s `DrawConstants::base` carries.
uint bucket_base(uint bucket)
{
    return gen.visible_capacity + bucket * gen.bucket_capacity;
}

/// Which word of `counts_and_mesh_args` holds bucket `bucket`'s draw count.
uint count_word(uint bucket)
{
    return bucket;
}

/// Which word of `counts_and_mesh_args` holds word `slot` of bucket `bucket`'s
/// mesh-dispatch extents, past the one draw count per bucket in front of them.
uint mesh_arg_word(uint bucket, uint slot)
{
    return gen.bucket_count + bucket * MESH_ARGS_WORDS + slot;
}

/// What `projected_error` answers for an eye inside the group's own sphere.
///
/// The largest finite `float`, standing in for the infinity
/// `crcbl_shaders::cluster_select::GroupCost::projected_error` returns there:
/// an infinity has no literal every target of this file spells the same way,
/// and every comparison the projection takes part in is against a budget a
/// caller set — a finite number, which this is above. So the two answer the
/// same decisions from the same inputs.
static const float INSIDE_THE_SPHERE = 3.4028235e38;

/// One group's simplification cost in **pixels**, from an eye at `eye`.
///
/// `docs/plan/25-lod.md`'s metric and the whole of it. The error is a length in
/// the mesh's own units, `pixels_per_unit` is how many pixels one such unit
/// subtends one unit from the eye, and the distance is measured to the *near
/// side* of the group's sphere rather than to its centre — so what comes out
/// bounds the cost anywhere in the group rather than naming it at one point.
///
/// **An eye inside the sphere has no distance to divide by** and answers
/// [`INSIDE_THE_SPHERE`]: as close as possible, which is both the conservative
/// reading and the monotone one, and monotone is what keeps the hysteresis band
/// from tearing a cut. The branch is taken before `error` is read at all, so a
/// group whose simplification costs nothing still answers it.
///
/// `crcbl_shaders::cluster_select::GroupCost::projected_error` is this function
/// in Rust — the same three operations over the same `f32`s. **The body below
/// also appears verbatim in `shaders/mesh_cluster.slang`**, whose mesh stage
/// shades a cluster by exactly this number for the screen-error heatmap: a
/// second spelling of one metric is a picture nobody can hold against the cut
/// it claims to describe. `crcbl_shaders::cluster_select`'s
/// `the_two_shaders_project_the_error_with_one_function` is what holds the two
/// texts equal.
float projected_error(float error, float3 center, float radius, float3 eye, float pixels_per_unit)
{
    float3 delta = eye - center;
    float separation = sqrt(delta.x * delta.x + delta.y * delta.y + delta.z * delta.z);
    float distance = separation - radius;
    if (distance <= 0.0)
    {
        return INSIDE_THE_SPHERE;
    }
    return error * pixels_per_unit / distance;
}

/// Whether a group is **expanded** — its children drawn rather than its parents
/// — from the eye this frame selects at, given whether it was last frame.
///
/// `docs/plan/25-lod.md`'s hysteresis, and
/// `crcbl_shaders::cluster_select::group_is_expanded` in Slang: [`projected_error`]
/// against `gen.lod_params.y` to *start* expanding, against `gen.lod_params.z`
/// to *keep* expanding, and the previous answer deciding between them. Equal
/// budgets are one threshold and `was` stops mattering, which is what a test
/// sets to show the band is doing the work.
uint group_is_expanded(float error, float3 center, float radius, float3 eye, uint was)
{
    float projected = projected_error(error, center, radius, eye, gen.lod_params.x);
    bool expanded = projected > gen.lod_params.y
        || (was != 0u && projected > gen.lod_params.z);
    return expanded ? 1u : 0u;
}

/// Judges every one of `instance`'s groups, records the answers, and returns the
/// level a uniform cut draws it at.
///
/// `crcbl_shaders::level_select::MeshLevels::select` is this loop in Rust, and
/// that module's docs carry why the returned number is the finest level the
/// per-cluster descent draws anywhere — which is what makes the coarse decision
/// the fine one's own floor rather than an approximation of it.
///
/// **Every group is judged, whatever the level answer is**, because the mesh
/// path descends these same groups per cluster and reads exactly this state. A
/// mesh whose record carries a `top_level` of zero — which is how that path
/// suppresses the uniform cut — still needs its groups decided, so there is no
/// early exit here even though the answer is a minimum.
///
/// **In world space**, exactly as `mesh_cluster.slang`'s descent is: the
/// sphere's centre goes through the instance's transform, its radius does not,
/// and the eye stays where it is. That is the same statement as putting the
/// camera through the inverse transform, and it needs no inverse.
///
/// **The radius staying in mesh space is a gap under a scaled instance, not a
/// property.** `GpuInstance::transform` may scale — see its declaration above —
/// and a group's radius and its error are both distances in the *mesh's* own
/// space, so a scaled instance is judged at the wrong size and selects a level
/// to match. It is the shape of the bug `mesh_cluster.slang`'s
/// `cluster_survives` closed one pass later with `max_stretch`. Written down
/// rather than changed here because it costs a level and does not drop
/// geometry, and because changing it moves every level-of-detail golden.
uint select_level(GpuInstance instance, uint instance_index)
{
    MeshLevels levels = mesh_levels_of(instance.mesh);
    float3 eye = gen.camera_position.xyz;
    uint base = instance_index * gen.group_stride;
    uint chosen = levels.top_level;
    for (uint i = 0; i < levels.group_count; ++i)
    {
        uint at = levels.first_group + i;
        LevelGroup group = level_group_at(at);
        float3 center = mul(instance.transform,
            float4(group.center_x, group.center_y, group.center_z, 1.0)).xyz;
        uint expanded = group_is_expanded(
            group.error, center, group.radius, eye, group_state[base + at]);
        group_state[base + at] = expanded;
        if (expanded == 1u && group.level < chosen)
        {
            chosen = group.level;
        }
    }
    return chosen;
}

/// Invocations per workgroup, matching
/// `crcbl_shaders::draw_gen::WORKGROUP_SIZE`.
///
/// One invocation covers *both* halves: it owns bucket `index` if there is one,
/// and it scatters visible instance `index` if there is one. A dispatch is
/// therefore sized for the larger of the two.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;

    // The static half of one bucket's arguments, from the mesh table. Every
    // word here is one a CPU-recorded `draw_indexed` would have passed, which
    // is what makes the two comparable.
    if (index < gen.bucket_count)
    {
        GpuMesh mesh = meshes[bucket_mesh(index)];
        uint at = index * DRAW_ARGS_WORDS;
        args[at + ARG_INDEX_COUNT].store(mesh.index_count);
        args[at + ARG_FIRST_INDEX].store(mesh.base_index);
        // **Zero for both bases, and it is not an omission.** The mesh's base
        // vertex reaches the vertex stage through the mesh table and the
        // instance reaches it through `visible_instances`, because the four
        // targets fold a draw's bases into `SV_VertexID` and `SV_InstanceID`
        // differently — `mesh.slang`'s header measures all four.
        args[at + ARG_VERTEX_OFFSET].store(0);
        args[at + ARG_FIRST_INSTANCE].store(0);

        // The static half of the same bucket's mesh dispatch. The y extent is
        // the scattering half's, below, for the reason `ARG_INSTANCE_COUNT` is:
        // nothing inside one dispatch can wait for every other invocation to
        // have finished counting.
        counts_and_mesh_args[mesh_arg_word(index, MESH_ARG_GROUP_X)]
            .store(bucket_clusters(index));
        counts_and_mesh_args[mesh_arg_word(index, MESH_ARG_GROUP_Z)].store(1);
    }

    // Clamped by both capacities, which is what makes a bucket's run unable to
    // overflow — see the header.
    uint count = min(visible_count[INSTANCE_SURVIVOR_WORD],
        min(gen.visible_capacity, gen.bucket_capacity));
    if (index >= count)
    {
        return;
    }

    // The survivor list is the front of the same buffer the runs are scattered
    // into — see `visible_instances`.
    uint instance_index = visible_instances[index];
    GpuInstance instance = instances[instance_index];
    // **`docs/plan/25-lod.md`'s uniform cut, and the one decision this pass
    // makes about detail.** The instance names level 0 of its mesh — that is
    // what the cull pass read a bounding box out of — and the level selected
    // here names the mesh that actually draws, which the bucket table then
    // routes exactly as it routes a mesh with no hierarchy. A mesh with none
    // resolves back to itself, so there is no branch for the ordinary case.
    uint mesh_id = level_mesh_at(
        mesh_levels_of(instance.mesh).first_level + select_level(instance, instance_index));
    // A linear walk of the bucket table. The table is small and fixed — one
    // entry per resident mesh today — so this is a couple of comparisons; a
    // mesh → bucket map is what a table large enough to notice would want.
    for (uint bucket = 0; bucket < gen.bucket_count; ++bucket)
    {
        if (bucket_mesh(bucket) != mesh_id)
        {
            continue;
        }
        uint slot = args[bucket * DRAW_ARGS_WORDS + ARG_INSTANCE_COUNT].add(1);
        // The mesh dispatch's y extent, counted by the same invocations and to
        // the same total. Two adds rather than one read of the other word,
        // because a word another invocation is still incrementing is not a
        // total yet — and the two are held equal by measurement rather than by
        // assertion: `crcbl-vk`'s `the_mesh_dispatch_extent_is_the_culled
        // _instance_count` reads both buffers back and compares them.
        counts_and_mesh_args[mesh_arg_word(bucket, MESH_ARG_GROUP_Y)].add(1);

        visible_instances[bucket_base(bucket) + slot] = instance_index;
        // Exactly one invocation per bucket takes slot zero, so exactly one
        // writes this — a plain store rather than an atomic, and not a value
        // several invocations race to agree on.
        if (slot == 0)
        {
            counts_and_mesh_args[count_word(bucket)].store(1);
        }
        break;
    }
}
