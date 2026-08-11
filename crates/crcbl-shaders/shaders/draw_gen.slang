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

/// One drawable object, matching `crcbl_shaders::mesh::GpuInstance` and
/// `mesh.slang`'s copy.
struct GpuInstance
{
    /// Model → sector. Rigid (rotation + translation), so its upper-left 3×3
    /// transforms normals correctly without an inverse-transpose.
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

/// Everything an invocation needs that is not buffer contents, matching
/// `crcbl_shaders::draw_gen::Params`.
///
/// A uniform buffer rather than push constants, for the reason
/// `compute_probe.slang` gives: WGSL has no push constants and no shader in this
/// crate may use one.
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
    /// Pads the block to a multiple of 16 bytes, which `std140` requires of a
    /// uniform buffer's size.
    uint pad0;
};

[[vk::binding(0, 0)]]
ConstantBuffer<DrawGenParams> gen;

/// The instance array, read only for `GpuInstance::mesh`: which bucket an
/// instance scatters into is the only thing this pass asks of it.
[[vk::binding(1, 0)]]
StructuredBuffer<GpuInstance> instances;

/// The mesh table, indexed by `bucket_meshes`. The index range in a bucket's
/// argument structure comes from here — which is the whole translation this
/// pass performs, and what makes the arguments checkable against the draws a
/// CPU would have recorded from the same table.
[[vk::binding(2, 0)]]
StructuredBuffer<GpuMesh> meshes;

/// `cull.slang`'s compacted survivors, as indices into `instances`. In whatever
/// order that pass's atomic handed slots out, which is not ascending — nothing
/// here depends on the order.
[[vk::binding(3, 0)]]
StructuredBuffer<uint> visible;

/// How many survivors `cull.slang` found, in element 0. The *true* count, so it
/// can exceed `DrawGenParams::visible_capacity` and is clamped below before it
/// indexes anything.
[[vk::binding(4, 0)]]
StructuredBuffer<uint> visible_count;

/// Which mesh each bucket draws, as an index into `meshes`. The bucket table,
/// built on the host when a mesh becomes resident and not rewritten per frame.
[[vk::binding(5, 0)]]
StructuredBuffer<uint> bucket_meshes;

/// The per-bucket runs of instance indices the vertex stage reads, bucket `b`'s
/// starting at `b * bucket_capacity`. `mesh.slang`'s `DrawConstants::base`
/// carries that offset and `SV_InstanceID` walks the run.
[[vk::binding(6, 0)]]
RWStructuredBuffer<uint> visible_instances;

/// The indirect arguments, `DRAW_ARGS_WORDS` per bucket, as a flat array of
/// words.
///
/// Flat and atomic rather than an array of structs because word
/// `ARG_INSTANCE_COUNT` **is** the slot allocator — see the header. The other
/// four words are plain stores through the same view, which is what
/// `Atomic<uint>::store` is for.
[[vk::binding(7, 0)]]
RWStructuredBuffer<Atomic<uint> > args;

/// One draw count per bucket: `1` for a bucket with something in it, and the
/// zero the caller wrote for a bucket with nothing.
///
/// What `GeometryPath::IndirectCount`'s call reads, and the reason the CPU never
/// learns whether a bucket drew. `IndirectPerBatch` passes a count of one and
/// leans on the empty bucket's zero instance count instead, which draws nothing
/// just as effectively — this buffer is what the two paths differ over.
[[vk::binding(8, 0)]]
RWStructuredBuffer<uint> draw_counts;

/// How many clusters each bucket's mesh has — the x extent of that bucket's
/// mesh dispatch, and the one number in `mesh_args` below that is not the
/// GPU's.
///
/// Host-written beside `bucket_meshes` and not rewritten per frame, because a
/// mesh's clusters are decided when it becomes resident. Zero on every
/// geometry path with no mesh stage, where nothing reads `mesh_args` at all.
[[vk::binding(9, 0)]]
StructuredBuffer<uint> bucket_clusters;

/// The per-bucket mesh-dispatch arguments, `MESH_ARGS_WORDS` per bucket, as a
/// flat array of words.
///
/// **This is what makes the cluster cull skip work rather than only output.**
/// `mesh_cluster.slang`'s stages run one workgroup per (cluster, surviving
/// instance), and how many instances survived is a number only this pass has —
/// so a CPU-sized dispatch has to use the instance *pool's* slot count and let
/// the groups past the survivors return early. Read from here instead, the
/// groups are never launched.
///
/// Flat and atomic for `args`' reason, and the same division of labour: word
/// `MESH_ARG_GROUP_Y` is incremented by each scattering invocation, exactly as
/// `ARG_INSTANCE_COUNT` is, and the other two are plain stores by the
/// invocation that owns the bucket.
[[vk::binding(10, 0)]]
RWStructuredBuffer<Atomic<uint> > mesh_args;

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
        GpuMesh mesh = meshes[bucket_meshes[index]];
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
        uint mesh_at = index * MESH_ARGS_WORDS;
        mesh_args[mesh_at + MESH_ARG_GROUP_X].store(bucket_clusters[index]);
        mesh_args[mesh_at + MESH_ARG_GROUP_Z].store(1);
    }

    // Clamped by both capacities, which is what makes a bucket's run unable to
    // overflow — see the header.
    uint count = min(visible_count[0], min(gen.visible_capacity, gen.bucket_capacity));
    if (index >= count)
    {
        return;
    }

    uint instance_index = visible[index];
    uint mesh_id = instances[instance_index].mesh;
    // A linear walk of the bucket table. The table is small and fixed — one
    // entry per resident mesh today — so this is a couple of comparisons; a
    // mesh → bucket map is what a table large enough to notice would want.
    for (uint bucket = 0; bucket < gen.bucket_count; ++bucket)
    {
        if (bucket_meshes[bucket] != mesh_id)
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
        mesh_args[bucket * MESH_ARGS_WORDS + MESH_ARG_GROUP_Y].add(1);
        // The mesh dispatch's y extent, counted by the same invocations and to
        // the same total. Two adds rather than one read of the other word,
        // because a word another invocation is still incrementing is not a
        // total yet — and the two are held equal by measurement rather than by
        // assertion: `crcbl-vk`'s `the_mesh_dispatch_extent_is_the_culled
        // _instance_count` reads both buffers back and compares them.

        visible_instances[bucket * gen.bucket_capacity + slot] = instance_index;
        // Exactly one invocation per bucket takes slot zero, so exactly one
        // writes this — a plain store rather than an atomic, and not a value
        // several invocations race to agree on.
        if (slot == 0)
        {
            draw_counts[bucket] = 1;
        }
        break;
    }
}
