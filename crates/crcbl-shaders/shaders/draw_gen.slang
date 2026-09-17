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
// which is what `visible_instances` below holds, starting at the word
// `mesh.slang`'s `DrawConstants::start_at` names.
//
// The CPU therefore records **one draw per bucket**, a number fixed when the
// bucket table is built, whatever the scene holds — which is the property §3.3
// is about.
//
// # Three passes, because where a run starts is this frame's
//
// **Every bucket's run lives in one region sized by the instance capacity**, not
// in a region per bucket. Until 2026-09 each bucket reserved a whole capacity of
// words — which cannot overflow and costs `capacity * (1 + buckets)` words per
// frame in flight per generator. A scene of 17219 instances and 938 buckets made
// that 61.7 MiB a buffer and ran a 2 GiB device out of memory, although a
// surviving instance lands in exactly one bucket and the buckets between them
// never hold more than the survivor list does.
//
// Packing the runs end to end means a run's start depends on how many survivors
// every earlier bucket took, which is a number no invocation has until all of
// them have counted. So the work is three entry points, each dispatched by a
// compute pass of its own that `crcbl_render::DrawGen` records in this order —
// its own pass rather than a second dispatch inside one, because a dependency
// between two dispatches of one pass is declared to no device, and a driver
// that runs undeclared commands in any order ran the prefix sum before the
// routes existed (see `DrawGen::add_passes`):
//
// 1. `binMain`, one invocation per bucket and per survivor: writes each bucket's
//    static arguments, selects the survivor's level, and **routes** it — the
//    bucket it belongs to goes into the route region, and the bucket's mesh
//    dispatch y extent counts it.
// 2. `startsMain`, one invocation: walks the buckets in order and writes each
//    one's start as the sum of the counts in front of it — a prefix sum.
// 3. `scatterMain`, one invocation per survivor: reads its route and that
//    bucket's start, takes a slot in the run, and writes the instance index.
//
// The survivor list, the routes, the runs and the starts are all regions of
// `visible_instances`; its declaration below has the table. A region per bucket
// cost `capacity` words each, and now a bucket costs the one word of its start.
//
// **The routes are a region of their own**, and that is what a parallel scatter
// requires rather than a convenience: invocation `i` reads survivor `i`'s route
// while other invocations write into the runs, so the route cannot live in the
// region being scattered into, and the survivor list cannot hold it either —
// that is the instance index being scattered.
//
// **The prefix sum is serial**, in one invocation, for the reason
// `exposure.slang`'s reduce is: the bucket table is fixed at build and small —
// one entry per (resident mesh level, material mode) — so a walk of it is one
// addition per bucket, where a parallel scan would be several dispatches and an
// order a device schedules.
//
// # Draw regions: more than one set of draws out of one generator
//
// Every per-bucket word — an argument structure, a draw count, three extent
// words and a run start — is laid out once per **draw region**, a bucket count
// apart, and region 0 is the layout this pass always had. `DRAW_MODE_OCCLUSION`
// adds the camera's early and late regions: region 0's run for a bucket is sized
// for every routed survivor, the early scatter fills its front with the survivors
// `cull.slang`'s first phase did not mark, `lateScatterMain` appends the ones its
// second phase rescued, and `lateFinishMain` makes region 0 the two together —
// so the forward pass reads one contiguous run while the two depth prepasses
// read its halves. `DRAW_MODE_FACES` adds a region per point-light face, with
// runs of their own behind the starts, and scatters a survivor into every face
// its tags name. The hysteresis state is judged for every routed survivor in
// every mode, marked or not, so a cull that hides an instance leaves its level
// selection exactly where drawing it would have.
//
// # The instance count is the atomic, and that is deliberate
//
// A bucket's `instance_count` is `scatterMain`'s slot allocator: a scattering
// invocation takes its place in the run by incrementing it, and the run's start
// is added to the slot it gets back. Writing the count afterwards would need a
// fourth dispatch, because nothing inside one dispatch can wait for every other
// invocation to have finished counting.
//
// The rest of each argument — index count, first index, and the two bases that
// are always zero — is written by the `binMain` invocation that *owns* that
// bucket, one per bucket, at words no other invocation touches. Different words,
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
// Every routed survivor is counted into exactly one bucket by `binMain` and
// takes exactly one slot in that bucket by `scatterMain`, and both walk the same
// survivors: `visible_count` below is clamped to `visible_capacity` before
// anything is read, and nothing in the pass writes it between the dispatches. So
// bucket `b`'s slots are `0..count(b)`, its run is `start(b)..start(b + 1)`, and
// the last run ends at most `visible_capacity` words into the run region — the
// runs sum to the survivors, and the survivors fit the list. The writes below
// need no bounds check that would be unreachable if they did.
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
// * `tables` is every host-written table this pass reads — the bucket table and
//   its per-bucket modes, the per-bucket cluster counts, and
//   `docs/plan/25-lod.md`'s three selection tables — as one flat array of words. **The cost is the typed
//   `StructuredBuffer`**: `LevelGroup`'s five floats are read back through
//   `asfloat`, and the word each field lands on is a named constant below
//   rather than a struct member Slang lays out. `crcbl_shaders::level_select` is
//   the host encoder and pins those constants.
// * `visible_instances` holds `cull.slang`'s survivor list, each survivor's
//   route, the per-bucket runs this pass scatters into and each run's start, in
//   that order. One binding rather than four because a read-only descriptor and
//   a writable descriptor of one buffer in one bind group is a usage conflict on
//   WebGPU — so the survivors are read back through the writable view — and
//   because the starts are read by the geometry stages, whose own storage
//   bindings are spent too.
// * `counts_and_mesh_args` holds the per-bucket draw counts and then the
//   per-bucket mesh-dispatch extents. Both are indirect arguments on every path
//   that reads either, so they are never in two resource states at once — which
//   is exactly why `args` is still a buffer of its own: the mesh path reads
//   *those* as data in the same pass that executes these.
//
// What a caller must not assume: that any of `DrawGen`'s buffer accessors hand
// back distinct buffers. `visible` and `runs` are one buffer, and so are `counts`
// and `mesh_args`; the offsets are `DrawGen::bucket_start_word`,
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
    /// Where this instance was **last frame**, matching `mesh.slang`'s copy —
    /// the same model → sector matrix, one frame behind. `InstancePool` writes
    /// it; nothing reads it yet, and this pass never will: where a pixel came
    /// from is a raster-stage question.
    float4x4 previous_transform;
    /// Which mesh to draw: an index into `meshes` below. The vertex stage
    /// resolves this instance's base vertex through it, which is what lets one
    /// draw cover instances of different meshes.
    uint mesh;
    /// Which material to shade with: an index into the material table
    /// `mesh.slang` shades through. **Nothing here reads it**, and it is not
    /// what routes: the material table is not one of this pass's bindings and
    /// cannot become one — eight storage buffers per stage is what WebGPU
    /// guarantees and the merges in the header are what fit inside it. What the
    /// key grew instead is the material's *mode*, which the host writes into
    /// `flags` below. A bucket key on the material **id** is the longer key
    /// §3.4's binning wants, and is still not what this pass does.
    uint material;
    /// Which sector `transform` is relative to. **Reserved**: the f64
    /// sector→camera offset table and the addition that would make this mean
    /// anything are §3.3's, so today every instance is in sector 0 and
    /// `transform` is effectively model → world.
    uint sector;
    /// Per-instance bits. Bit 0 is the live bit `cull.slang` reads; an index in
    /// the visible list is a live instance by construction, so nothing here
    /// asks. Bit 1 is the base-vertex override the raster stages read, and this
    /// pass buckets by mesh rather than by base, so nothing here asks about that
    /// one either. Bits 2 and 3 are the **material mode** — see
    /// `instance_material_mode` below, which is the half of this pass's routing
    /// key that is not the mesh id.
    uint flags;
    /// The pool vertex this instance's indices are relative to, read **instead
    /// of** the mesh entry's when `INSTANCE_BASE_VERTEX_OVERRIDE` is set in
    /// `flags` and ignored when it is not. A deformed instance names its
    /// **source** mesh above and carries the base of the region a skinning
    /// dispatch wrote it into here, so the bucket, the level tables and the
    /// bounding box all still resolve through the mesh it was made from.
    uint base_vertex;
    /// The pool vertex the **previous frame's** deformation of this instance
    /// was written into, on the same terms as the field above — read by the two
    /// geometry stages when the override bit is set, and by nothing here:
    /// this pass buckets survivors by mesh, and neither question is where a fragment was.
    uint previous_base_vertex;
    /// Pads the record to a multiple of the transform's alignment, matching
    /// `mesh.slang` — where the four targets' disagreement about the tail is
    /// written out.
    uint pad1;
    uint pad2;
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
    /// The mesh's UV range, matching `mesh.slang`. Declared and not read: this
    /// pass resolves ranges and boxes, and a row short of four floats here puts
    /// every entry after it at the wrong offset.
    float uv_scale_u;
    float uv_scale_v;
    float uv_offset_u;
    float uv_offset_v;
    /// The mesh's flags word, matching `mesh.slang`. Declared and not read, for
    /// the UV range's reason exactly: a row short of it puts every entry after
    /// this one at the wrong offset, and nothing anywhere reports that.
    uint flags;
};

/// Where the material mode starts in `GpuInstance::flags`, matching
/// `crcbl_shaders::mesh::GpuInstance::MATERIAL_MODE_SHIFT`.
static const uint INSTANCE_MATERIAL_MODE_SHIFT = 2;

/// The material mode's bits in `GpuInstance::flags`, matching
/// `crcbl_shaders::mesh::GpuInstance::MATERIAL_MODE_MASK`.
///
/// Two bits, and both are spoken for — glTF's `alphaMode: "MASK"` and its
/// `doubleSided`. The host is what decides which modes exist at all; this pass
/// only compares.
static const uint INSTANCE_MATERIAL_MODE_MASK = 12;

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

/// The low bits of a survivor-list entry, which are the instance index, and the
/// tags `cull.slang` writes above them — matching the constants of the same
/// names there and in `crcbl_shaders::cull`.
static const uint ENTRY_INDEX_MASK = 16777215;
static const uint ENTRY_FACE_SHIFT = 24;
static const uint ENTRY_RESCUED = 1073741824;
static const uint ENTRY_OCCLUDED = 2147483648;

/// Point-light faces, and so how many face regions `DRAW_MODE_FACES` fills.
static const uint FACE_COUNT = 6;

/// How many **draw regions** every argument, count, dispatch-extent and start
/// word is laid out for, matching `crcbl_shaders::draw_gen::DRAW_REGIONS`.
///
/// A region is a whole copy of the per-bucket words — one argument structure,
/// one draw count, one dispatch extent and one run start per bucket — so one
/// generator can hand a pass more than one set of draws. Region 0 is what every
/// generator has always written: every survivor. `DRAW_MODE_OCCLUSION` adds the
/// early and late regions, and `DRAW_MODE_FACES` a region per point-light face.
/// A generator allocates the regions its mode can use and no more; the layout
/// of region 0 is the same in all of them.
static const uint DRAW_REGIONS = 7;

/// The survivors the first occlusion phase passed, drawn by the early prepass.
static const uint EARLY_REGION = 1;

/// The survivors the second phase rescued, drawn by the late prepass. Region 0
/// is the two together, which is what the forward pass draws.
static const uint LATE_REGION = 2;

/// Face `f` of a point light draws region `FACE_REGION_BASE + f`.
static const uint FACE_REGION_BASE = 1;

/// `DrawGenParams::mode`: every survivor, into region 0.
static const uint DRAW_MODE_PLAIN = 0;

/// `DrawGenParams::mode`: the camera's two-phase occlusion — region 0's runs
/// are sized for every survivor, the early scatter fills each run's front with
/// the unmarked ones and `lateScatterMain` appends the rescued ones behind them.
static const uint DRAW_MODE_OCCLUSION = 1;

/// `DrawGenParams::mode`: a point light's culls — each survivor scatters into
/// the region of every face its tags name, in runs packed from
/// `DrawGenParams::face_runs_at`.
static const uint DRAW_MODE_FACES = 2;

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
    /// Elements of the survivor list that hold anything — `cull.slang`'s
    /// capacity. Its counter can exceed this, which is how an overflow is
    /// visible at all.
    ///
    /// **Also the length of the route and run regions**, which is what places
    /// every region of `visible_instances` after the list: the runs between them
    /// hold every survivor the list does, so no bucket needs a stride of its own.
    uint visible_capacity;
    /// How many groups one instance's run of `group_state` holds — every
    /// resident mesh's group count summed, and the stride between two instances
    /// in it. The same number `mesh_cluster.slang`'s
    /// `ClusterDrawConstants::group_stride` carries, because that stage reads
    /// what this one writes.
    uint group_stride;
    /// Where the per-bucket **material modes** start in `tables`, in words.
    ///
    /// The other half of a bucket's key: bucket `b` draws mesh `bucket_mesh(b)`
    /// shaded in mode `bucket_mode(b)`, and `binMain` below routes an instance
    /// to the bucket that matches both. A scene whose materials are all one mode
    /// has one bucket per mesh exactly as it always did — the region is there,
    /// every word of it equal, and no draw is added.
    ///
    /// Host-computed on `bucket_clusters_at`'s terms below.
    uint bucket_modes_at;
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
    /// One of `DRAW_MODE_*`: which regions this frame's survivors scatter into.
    uint mode;
    /// Regions this generator's buffers hold, and so how many copies of each
    /// bucket's static arguments `binMain` writes.
    uint draw_regions;
    /// Where the face regions' runs start in `visible_instances`, in words —
    /// past every region's starts. Read only under `DRAW_MODE_FACES`.
    uint face_runs_at;
    uint mode_pad;
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
/// **One binding for every one of them**, because a WebGPU device guarantees only
/// eight storage buffers per shader stage — see the header. The regions, in
/// order, and what indexes each:
///
/// | region | starts at | stride | indexed by |
/// |---|---|---|---|
/// | bucket table: which mesh each bucket draws | word 0 | 1 | bucket |
/// | per-bucket material modes | `gen.bucket_modes_at` | 1 | bucket |
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

/// `cull.slang`'s compacted survivors, each survivor's route, the per-bucket runs
/// of instance indices the geometry stages read, and where each run starts.
///
/// **Four regions in one buffer, and read *and* written through one
/// descriptor.** With `C` for `gen.visible_capacity` and `B` for
/// `gen.bucket_count`:
///
/// | region | words | written by | read by |
/// |---|---|---|---|
/// | survivors | `0..C` | `cull.slang` | `binMain`, `scatterMain` |
/// | routes: survivor `i`'s bucket | `C..2C` | `binMain` | `scatterMain` |
/// | runs, every bucket's end to end | `2C..3C` | `scatterMain`, `lateScatterMain` | geometry stages |
/// | starts: region `r` bucket `b`'s first run word | `3C..3C+RB` | `startsMain` | the scatters, geometry stages |
/// | face runs, every face's buckets end to end | `face_runs_at..+6C+1` | `scatterMain` | geometry stages |
///
/// `R` is `DrawGenParams::draw_regions`. The face runs exist only in a generator
/// built for a point light's faces, and end with one spare word for the reason
/// the next paragraph gives.
///
/// The survivors are in whatever order `cull.slang`'s atomic handed slots out,
/// and each run in whatever order `scatterMain`'s did — nothing here depends on
/// either. A start is an **absolute** word index into this buffer, so a reader
/// adds `SV_InstanceID` to it and indexes again; `run_start_word` is the word
/// `mesh.slang`'s `DrawConstants::start_at` names.
///
/// **The starts are last so that every start is itself in bounds.** A bucket
/// that drew nothing after every survivor went into buckets in front of it
/// starts at `3C`, which is past the runs — and `mesh_cluster.slang`'s task
/// stage reads the word at a bucket's start even for a group it then multiplies
/// away. With the starts behind the runs that word is the first start rather
/// than the end of the buffer, and there is always one, because a table with no
/// buckets is refused at build.
///
/// Writable rather than a read-only view of the survivors beside a writable view
/// of the rest: binding one buffer twice, once each way, in a single bind group
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
/// `MESH_ARG_GROUP_Y` is incremented by each routing invocation, exactly as
/// `ARG_INSTANCE_COUNT` is by each scattering one, and the other two are plain
/// stores by the invocation that owns the bucket. The draw counts are plain
/// stores too, by `startsMain` alone.
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

/// Which material mode bucket `bucket` draws — the half of its key that is not
/// the mesh id.
///
/// Compared against `instance_material_mode` below, so a mesh drawn by an opaque
/// material and by a cutout one occupies two buckets and the depth passes bind a
/// pipeline per bucket rather than one per frame.
uint bucket_mode(uint bucket)
{
    return tables[gen.bucket_modes_at + bucket];
}

/// This instance's material mode, unpacked from `GpuInstance::flags` — the host
/// twin is `crcbl_shaders::mesh::GpuInstance::material_mode`.
///
/// Written when the instance's record was written, from the row
/// `GpuInstance::material` names. A material rewritten afterwards does not
/// re-key the instances that carry its old mode; `MATERIAL_MODE_SHIFT`'s Rust
/// doc is where that precondition is stated.
uint instance_material_mode(GpuInstance instance)
{
    return (instance.flags & INSTANCE_MATERIAL_MODE_MASK) >> INSTANCE_MATERIAL_MODE_SHIFT;
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

/// What `binMain` routes a survivor to when no bucket matches its mesh and its
/// mode — and what `scatterMain` then sends nowhere. No table reaches this many
/// buckets: `crcbl_render::DrawGen` counts them in a `u32`.
static const uint NO_BUCKET = 0xffffffff;

/// Which word of `visible_instances` holds survivor `survivor`'s route, past the
/// survivor list.
uint route_word(uint survivor)
{
    return gen.visible_capacity + survivor;
}

/// The first word of the run region, past the survivors and their routes — the
/// start of the first bucket's run in every frame.
uint runs_at()
{
    return 2 * gen.visible_capacity;
}

/// Which argument structure, and which element of every per-draw table, bucket
/// `bucket` of region `region` is: the regions sit end to end, a bucket count
/// apart. The number `mesh_cluster.slang`'s `ClusterDrawConstants::bucket`
/// carries, so its read of `draw_args` lands on the same structure.
uint draw_slot(uint region, uint bucket)
{
    return region * gen.bucket_count + bucket;
}

/// Which word of `visible_instances` holds where bucket `bucket`'s run of region
/// `region` starts this frame, past the runs.
///
/// The same arithmetic `crcbl_render::DrawGen::bucket_start_word` performs, and
/// the word `mesh.slang`'s `DrawConstants::start_at` names — fixed at build,
/// where the start it holds is `startsMain`'s every frame.
uint run_start_word(uint region, uint bucket)
{
    return 3 * gen.visible_capacity + draw_slot(region, bucket);
}

/// Which word of `counts_and_mesh_args` holds bucket `bucket`'s draw count in
/// region `region`. A region is one draw count and three extent words per
/// bucket, so region 0 is the layout the buffer always had.
uint count_word(uint region, uint bucket)
{
    return region * 4 * gen.bucket_count + bucket;
}

/// Which word of `counts_and_mesh_args` holds word `slot` of bucket `bucket`'s
/// mesh-dispatch extents in region `region`, past that region's draw counts.
uint mesh_arg_word(uint region, uint bucket, uint slot)
{
    return region * 4 * gen.bucket_count + gen.bucket_count + bucket * MESH_ARGS_WORDS + slot;
}

/// The word of `args` holding field `field` of bucket `bucket`'s structure in
/// region `region`.
uint arg_word(uint region, uint bucket, uint field)
{
    return draw_slot(region, bucket) * DRAW_ARGS_WORDS + field;
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

/// The largest factor by which `basis` can grow a vector's length, bounded from
/// above — what a length in the mesh's own space has to be multiplied by to be a
/// length in the space `basis` maps it into.
///
/// That factor is `basis`' largest singular value, and this bounds it rather
/// than computing it: `sigma_max^2 = lambda_max(B^T B)`, and a symmetric
/// matrix's spectral radius is at most any induced norm of it, so the largest
/// absolute row sum of `B^T B` is an upper bound on `sigma_max^2`. The bound is
/// **exact** whenever `basis`' columns are orthogonal — every rotation-then-scale,
/// which is every transform this engine builds — and an over-estimate on a
/// shear, which for a selection means expanding a group sooner than it had to
/// and drawing finer, never coarser than the metric asked for.
///
/// `1.0` for a rotation or a translation, so nothing that was already correct
/// moves.
///
/// **The body below also appears verbatim in `shaders/mesh_cluster.slang`**,
/// where the same bound carries a cluster's bounding sphere across the same
/// transform for the cull, and `crcbl_render::cull`'s `max_stretch` is it in
/// Rust. `crcbl_shaders::cluster_select`'s
/// `the_two_shaders_bound_a_stretched_length_with_one_function` is what holds
/// the two shader texts equal.
float max_stretch(float3x3 basis)
{
    float3x3 gram = mul(transpose(basis), basis);
    float bound = 0.0;
    for (uint row = 0; row < 3; ++row)
    {
        bound = max(bound, abs(gram[row][0]) + abs(gram[row][1]) + abs(gram[row][2]));
    }
    return sqrt(bound);
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
/// sphere's centre goes through the instance's transform, its radius and the
/// group's error through [`max_stretch`] of that transform's 3x3, and the eye
/// stays where it is. For a similarity — a rotation, a translation, a uniform
/// scale of `s` — that is exactly the same statement as putting the camera
/// through the inverse transform, and it needs no inverse: every term of the
/// projection is scaled by `s` and the eye at `eye/s` answers the same level.
/// `crcbl`'s `a_scaled_instance_of_the_patch_selects_a_finer_level` uses that
/// equivalence as its oracle on a real device.
///
/// **Both lengths are scaled, and neither is decoration.** A group's radius and
/// its error are distances in the *mesh's* own space and `GpuInstance::transform`
/// may scale — see its declaration above — so an instance drawn ten times the
/// size it was authored at has a simplification that moves the surface ten times
/// as far on screen, and judging it at the mesh's size selects a level for a
/// picture nobody is looking at. `max_stretch` bounds the factor from above, so
/// what it can do to a group is expand it *earlier* than the mesh's own size
/// would: finer geometry, never coarser than the metric asked for. At a rotation
/// or a translation it is exactly `1.0` and nothing moves.
/// `crcbl_shaders::level_select::MeshLevels::select` takes the same number as
/// its `stretch` argument, and `GroupCost::scaled` is where it lands.
uint select_level(GpuInstance instance, uint instance_index)
{
    MeshLevels levels = mesh_levels_of(instance.mesh);
    float3 eye = gen.camera_position.xyz;
    float stretch = max_stretch((float3x3)instance.transform);
    uint base = instance_index * gen.group_stride;
    uint chosen = levels.top_level;
    for (uint i = 0; i < levels.group_count; ++i)
    {
        uint at = levels.first_group + i;
        LevelGroup group = level_group_at(at);
        float3 center = mul(instance.transform,
            float4(group.center_x, group.center_y, group.center_z, 1.0)).xyz;
        uint expanded = group_is_expanded(group.error * stretch, center,
            group.radius * stretch, eye, group_state[base + at]);
        group_state[base + at] = expanded;
        if (expanded == 1u && group.level < chosen)
        {
            chosen = group.level;
        }
    }
    return chosen;
}

/// How many survivors this frame's dispatches walk: `cull.slang`'s true count,
/// clamped to the list that holds them.
///
/// One function for `binMain` and `scatterMain`, because the run arithmetic
/// rests on the two walking the same survivors — see the header. The counter is
/// a read-only binding here, so nothing between the two dispatches can move it.
uint survivor_count()
{
    return min(visible_count[INSTANCE_SURVIVOR_WORD], gen.visible_capacity);
}

/// Invocations per workgroup, matching
/// `crcbl_shaders::draw_gen::WORKGROUP_SIZE`.
///
/// One invocation covers *both* halves: it owns bucket `index` if there is one,
/// and it routes visible instance `index` if there is one. A dispatch is
/// therefore sized for the larger of the two.
///
/// The first of the three stages, the `draw-args` pass — see the header.
[shader("compute")]
[numthreads(64, 1, 1)]
void binMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;

    // The static half of one bucket's arguments, from the mesh table. Every
    // word here is one a CPU-recorded `draw_indexed` would have passed, which
    // is what makes the two comparable.
    if (index < gen.bucket_count)
    {
        GpuMesh mesh = meshes[bucket_mesh(index)];
        // Every region's copy: a region draws the same mesh through the same
        // index range, and only the instance counts differ between them.
        for (uint region = 0; region < gen.draw_regions; ++region)
        {
            args[arg_word(region, index, ARG_INDEX_COUNT)].store(mesh.index_count);
            args[arg_word(region, index, ARG_FIRST_INDEX)].store(mesh.base_index);
            // **Zero for both bases, and it is not an omission.** The mesh's
            // base vertex reaches the vertex stage through the mesh table and
            // the instance reaches it through `visible_instances`, because the
            // four targets fold a draw's bases into `SV_VertexID` and
            // `SV_InstanceID` differently — `mesh.slang`'s header measures all
            // four.
            args[arg_word(region, index, ARG_VERTEX_OFFSET)].store(0);
            args[arg_word(region, index, ARG_FIRST_INSTANCE)].store(0);

            // The static half of the same bucket's mesh dispatch. The y extent
            // is the routing half's, below, for the reason `ARG_INSTANCE_COUNT`
            // is: nothing inside one dispatch can wait for every other
            // invocation to have finished counting.
            counts_and_mesh_args[mesh_arg_word(region, index, MESH_ARG_GROUP_X)]
                .store(bucket_clusters(index));
            counts_and_mesh_args[mesh_arg_word(region, index, MESH_ARG_GROUP_Z)].store(1);
        }
    }

    // Clamped to the list, which is what makes the runs unable to overflow —
    // see the header.
    if (index >= survivor_count())
    {
        return;
    }

    // The survivor list is the front of the same buffer the routes are written
    // into — see `visible_instances`.
    uint entry = visible_instances[index];
    // The tags above the index are the cull's answers about the survivor — see
    // `ENTRY_INDEX_MASK` — and the index is what everything below names.
    uint instance_index = entry & ENTRY_INDEX_MASK;
    GpuInstance instance = instances[instance_index];
    // **`docs/plan/25-lod.md`'s uniform cut, and the one decision this pass
    // makes about detail.** The instance names level 0 of its mesh — that is
    // what the cull pass read a bounding box out of — and the level selected
    // here names the mesh that actually draws, which the bucket table then
    // routes exactly as it routes a mesh with no hierarchy. A mesh with none
    // resolves back to itself, so there is no branch for the ordinary case.
    //
    // Here and not in `scatterMain`, because `select_level` writes the
    // hysteresis state and every element of it has exactly one writer a frame.
    uint mesh_id = level_mesh_at(
        mesh_levels_of(instance.mesh).first_level + select_level(instance, instance_index));
    // The other half of the key, and the reason a cutout costs only its own
    // mesh: an opaque instance and a masked one of the same geometry go to
    // different buckets, so the depth prepass and the shadow atlas record the
    // first under a vertex-only pipeline and the second under the cutout twin.
    uint mode = instance_material_mode(instance);
    // A linear walk of the bucket table. The table is small and fixed — one
    // entry per (resident mesh, mode present in the scene) today — so this is a
    // couple of comparisons; a (mesh, mode) → bucket map is what a table large
    // enough to notice would want.
    uint routed = NO_BUCKET;
    for (uint bucket = 0; bucket < gen.bucket_count; ++bucket)
    {
        if (bucket_mesh(bucket) != mesh_id || bucket_mode(bucket) != mode)
        {
            continue;
        }
        routed = bucket;
        // Counted into the mesh dispatch's y extent, which is what `startsMain`
        // sums. `scatterMain` counts the same survivors again into
        // `ARG_INSTANCE_COUNT`, so the two words reach the same total — held
        // equal by measurement rather than by assertion: `crcbl-vk`'s
        // `the_mesh_dispatch_extent_is_the_culled_instance_count` reads both
        // buffers back and compares them.
        if (gen.mode == DRAW_MODE_FACES)
        {
            // One count per face the survivor reaches, into that face's region.
            for (uint face = 0; face < FACE_COUNT; ++face)
            {
                if (((entry >> (ENTRY_FACE_SHIFT + face)) & 1) != 0)
                {
                    counts_and_mesh_args[mesh_arg_word(FACE_REGION_BASE + face, bucket,
                        MESH_ARG_GROUP_Y)].add(1);
                }
            }
        }
        else
        {
            // Region 0 counts **every** routed survivor, marked or not, because
            // under occlusion its runs are sized for the late scatter to append
            // into; the early region counts the ones drawn first.
            counts_and_mesh_args[mesh_arg_word(0, bucket, MESH_ARG_GROUP_Y)].add(1);
            if (gen.mode == DRAW_MODE_OCCLUSION && (entry & ENTRY_OCCLUDED) == 0)
            {
                counts_and_mesh_args[mesh_arg_word(EARLY_REGION, bucket, MESH_ARG_GROUP_Y)]
                    .add(1);
            }
        }
        break;
    }
    // Invocation `index` owns word `index` of the routes, so this is a plain
    // store rather than an atomic. An instance matching no bucket is routed
    // nowhere and never drawn — see `crcbl_shaders::draw_gen`'s module docs.
    visible_instances[route_word(index)] = routed;
}

/// The second stage, the `draw-starts` pass: where every bucket's run starts
/// this frame, and whether it draws at all.
///
/// One invocation walking the table in bucket order, for the reason the header
/// gives. By the time it runs every `binMain` invocation has finished, so each
/// y extent is a total rather than a counter still moving — which is the whole
/// reason this is a pass of its own.
[shader("compute")]
[numthreads(1, 1, 1)]
void startsMain()
{
    if (gen.mode == DRAW_MODE_FACES)
    {
        // Each face's runs end to end, face by face, in their own region of the
        // buffer: a survivor can reach several faces, so the faces together can
        // hold more than one capacity of runs.
        uint face_start = gen.face_runs_at;
        for (uint face = 0; face < FACE_COUNT; ++face)
        {
            uint region = FACE_REGION_BASE + face;
            for (uint bucket = 0; bucket < gen.bucket_count; ++bucket)
            {
                visible_instances[run_start_word(region, bucket)] = face_start;
                uint reached =
                    counts_and_mesh_args[mesh_arg_word(region, bucket, MESH_ARG_GROUP_Y)].load();
                if (reached != 0)
                {
                    counts_and_mesh_args[count_word(region, bucket)].store(1);
                }
                face_start += reached;
            }
        }
        return;
    }

    uint start = runs_at();
    for (uint bucket = 0; bucket < gen.bucket_count; ++bucket)
    {
        visible_instances[run_start_word(0, bucket)] = start;
        uint routed = counts_and_mesh_args[mesh_arg_word(0, bucket, MESH_ARG_GROUP_Y)].load();
        // `1` for a bucket with something in it; an empty one keeps the zero
        // the clearing pass wrote, which is what `IndirectCount`'s call reads.
        if (routed != 0)
        {
            counts_and_mesh_args[count_word(0, bucket)].store(1);
        }
        if (gen.mode == DRAW_MODE_OCCLUSION)
        {
            // The early run is the front of region 0's, and the late run starts
            // where the early one ends — so the forward pass reads one
            // contiguous run of both.
            uint early =
                counts_and_mesh_args[mesh_arg_word(EARLY_REGION, bucket, MESH_ARG_GROUP_Y)].load();
            visible_instances[run_start_word(EARLY_REGION, bucket)] = start;
            visible_instances[run_start_word(LATE_REGION, bucket)] = start + early;
            if (early != 0)
            {
                counts_and_mesh_args[count_word(EARLY_REGION, bucket)].store(1);
            }
        }
        start += routed;
    }
}

/// The third stage, the `draw-scatter` pass: every routed survivor's instance
/// index, written into its bucket's run.
///
/// One invocation per survivor, and it reads three regions of
/// `visible_instances` it never writes — the survivor, its route and its
/// bucket's start — while writing only into the runs, so no invocation reads a
/// word another one writes.
[shader("compute")]
[numthreads(64, 1, 1)]
void scatterMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= survivor_count())
    {
        return;
    }
    uint bucket = visible_instances[route_word(index)];
    if (bucket == NO_BUCKET)
    {
        return;
    }
    uint entry = visible_instances[index];
    uint instance_index = entry & ENTRY_INDEX_MASK;
    if (gen.mode == DRAW_MODE_FACES)
    {
        for (uint face = 0; face < FACE_COUNT; ++face)
        {
            if (((entry >> (ENTRY_FACE_SHIFT + face)) & 1) == 0)
            {
                continue;
            }
            uint region = FACE_REGION_BASE + face;
            uint face_slot = args[arg_word(region, bucket, ARG_INSTANCE_COUNT)].add(1);
            visible_instances[visible_instances[run_start_word(region, bucket)] + face_slot] =
                instance_index;
        }
        return;
    }
    // A survivor the first occlusion phase marked waits for `lateScatterMain`.
    if (gen.mode == DRAW_MODE_OCCLUSION && (entry & ENTRY_OCCLUDED) != 0)
    {
        return;
    }
    uint region = gen.mode == DRAW_MODE_OCCLUSION ? EARLY_REGION : 0;
    uint slot = args[arg_word(region, bucket, ARG_INSTANCE_COUNT)].add(1);
    visible_instances[visible_instances[run_start_word(region, bucket)] + slot] = instance_index;
}

/// The late scatter: every survivor `cull.slang`'s `lateMain` rescued, written
/// into its bucket's late run — which starts where the early run ended, inside
/// region 0's run.
///
/// A pass of its own after that one, on `scatterMain`'s terms: it reads the tag
/// `lateMain` wrote, and a dependency between two dispatches of one pass is one
/// no device is told about.
[shader("compute")]
[numthreads(64, 1, 1)]
void lateScatterMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= survivor_count())
    {
        return;
    }
    uint entry = visible_instances[index];
    if ((entry & ENTRY_RESCUED) == 0)
    {
        return;
    }
    uint bucket = visible_instances[route_word(index)];
    if (bucket == NO_BUCKET)
    {
        return;
    }
    uint slot = args[arg_word(LATE_REGION, bucket, ARG_INSTANCE_COUNT)].add(1);
    visible_instances[visible_instances[run_start_word(LATE_REGION, bucket)] + slot] =
        entry & ENTRY_INDEX_MASK;
}

/// Closes the two phases: the late region's draw counts and extents, and region
/// 0 — the forward pass's — as the early and late regions together.
///
/// Each invocation owns one bucket. The graph dependency on the late scatter
/// makes both phase counts complete before this pass reads them.
/// Region 0's count and y extent are stored over every routed survivor,
/// including the ones never drawn.
[shader("compute")]
[numthreads(64, 1, 1)]
void lateFinishMain(uint3 thread: SV_DispatchThreadID)
{
    uint bucket = thread.x;
    if (bucket >= gen.bucket_count)
    {
        return;
    }
    uint early = args[arg_word(EARLY_REGION, bucket, ARG_INSTANCE_COUNT)].load();
    uint late = args[arg_word(LATE_REGION, bucket, ARG_INSTANCE_COUNT)].load();
    counts_and_mesh_args[mesh_arg_word(LATE_REGION, bucket, MESH_ARG_GROUP_Y)].store(late);
    if (late != 0)
    {
        counts_and_mesh_args[count_word(LATE_REGION, bucket)].store(1);
    }
    uint drawn = early + late;
    args[arg_word(0, bucket, ARG_INSTANCE_COUNT)].store(drawn);
    counts_and_mesh_args[mesh_arg_word(0, bucket, MESH_ARG_GROUP_Y)].store(drawn);
    counts_and_mesh_args[count_word(0, bucket)].store(drawn != 0 ? 1 : 0);
}
