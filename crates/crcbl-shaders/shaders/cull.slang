// crcbl-targets: spirv, wgsl, msl, dxil

// Frustum culling on the GPU: one thread per instance, a compacted list of the
// survivors, and an atomic counter saying how many there are.
//
// `docs/plan/03-gpu-driven-rendering.md` §3.3's first half — "Compute pass:
// frustum cull against instance AABBs → compacted visible instance list". The
// second half, the `draw_indexed_indirect` records that list turns into, is a
// slice of its own and nothing here anticipates it beyond leaving the list in
// the form it wants: instance indices, ascending nowhere in particular, with a
// count beside them.
//
// **`draw_gen.slang` is what consumes the list**, scattering the survivors into
// per-bucket runs and writing the indirect arguments a draw reads. This pass is
// still checked on its own, by reading the list back and comparing it against a
// CPU reference — `crcbl_render::cull`, which is ordinary Rust and is the
// oracle — because a consumer that drew the right picture from the wrong list
// would say nothing about this one.
//
// # Every slot is tested, so every slot has to say whether it is one
//
// The dispatch walks `0..instance_count` and the array is a pool: an element
// between two live instances may be a slot whose instance was removed, still
// holding the transform and mesh id it had when it was. `GpuInstance::flags`
// bit 0 is what tells them apart, and the entry point asks it before it reads
// anything else in the record. Without that the pass culls — and keeps —
// removed instances on stale data, which is a contract "pass the right
// `instance_count`" cannot make a caller able to keep once slots are reused.
//
// # The structs are declared again here, and that is the rule
//
// `GpuInstance` and `GpuMesh` are also declared in `mesh.slang`. There is no
// shared header: `tools/compile-shaders.sh` hashes one source per artifact, so
// an `#include` would be a file whose edits nothing downstream notices.
// `mesh_shader.slang` re-declares its vertex struct for the same reason. What
// keeps the two copies honest is a test — `crcbl_shaders::cull`'s
// `the_shared_structs_are_declared_identically_in_both_shaders` — which reads
// both sources and compares the declarations character for character.
//
// # An AABB rotated by a transform is not an AABB
//
// The mesh's box is local-space, and the instance's transform rotates it. The
// tight bound of a rotated box is not a box, so this uses the standard
// conservative method: the transformed box's **centre** is the transform
// applied to the local centre, and its **half-extent** is the *absolute value*
// of the transform's 3x3 part applied to the local half-extent. The result
// contains the rotated box and is generally larger than it, which is the
// correct direction for a cull to err in — Ericson, *Real-Time Collision
// Detection* §4.2.6, and the reason a rotated instance is a test case rather
// than an assumption.
//
// # The planes are not normalized, and must not be
//
// The frustum arrives as six half-spaces where a point is inside when
// `dot(plane.xyz, p) + plane.w >= 0`. Both sides of the centre/extent test
// below scale linearly with the plane's normal, so an unnormalized plane gives
// the same answer as a normalized one — and under the engine's reversed-Z
// **infinite** projection one of Gribb-Hartmann's six planes comes out with a
// zero normal (the far plane, which is at infinity and rejects nothing).
// Normalizing would divide that one by zero and turn every test against it into
// a `NaN` comparison, which passes for "outside". `crcbl_render::cull::Frustum`
// therefore hands them over as extracted.

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
    /// `mesh.slang` shades through. Nothing here reads it — this pass decides
    /// what is on screen, not what colour it is.
    uint material;
    /// Which sector `transform` is relative to. **Reserved**: the f64
    /// sector→camera offset table and the addition that would make this mean
    /// anything are §3.3's, so today every instance is in sector 0 and
    /// `transform` is effectively model → world.
    uint sector;
    /// Per-instance bits. Bit 0 is `INSTANCE_LIVE` below: clear means this
    /// element holds a removed instance's leftovers, and the entry point
    /// rejects it before reading anything else. Bit 1 is the base-vertex
    /// override the raster stages read; nothing here does.
    uint flags;
    /// The pool vertex this instance's indices are relative to, read **instead
    /// of** the mesh entry's when `INSTANCE_BASE_VERTEX_OVERRIDE` is set in
    /// `flags` and ignored when it is not. A deformed instance names its
    /// **source** mesh above and carries the base of the region a skinning
    /// dispatch wrote it into here, so the bucket, the level tables and the
    /// bounding box all still resolve through the mesh it was made from.
    uint base_vertex;
    /// Pads the record to a multiple of the transform's alignment, matching
    /// `mesh.slang` — where the four targets' disagreement about the tail is
    /// written out.
    uint pad0;
    uint pad1;
    uint pad2;
};

/// `GpuInstance::flags` bit 0: this element is a live instance, matching
/// `crcbl_shaders::mesh::GpuInstance::LIVE`.
///
/// A named constant rather than a literal `1` at the test below, for the reason
/// `PLANE_COUNT` is one: a bare bit in an expression is a number the next bit
/// this word gains has no way to be checked against.
static const uint INSTANCE_LIVE = 1;

/// Which element of `visible_count` this pass adds to.
///
/// The same number as `crcbl_shaders::cull::INSTANCE_SURVIVOR_WORD`, held in
/// step with it by that module's
/// `the_shaders_index_the_culling_stats_at_the_words_this_crate_says` — which
/// is also what pins `mesh_cluster.slang` to the element beside this one.
static const uint INSTANCE_SURVIVOR_WORD = 0;

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

/// Planes in a frustum. Six, and the loop below is written against this rather
/// than against a literal.
static const uint PLANE_COUNT = 6;

/// Everything an invocation needs that is not buffer contents, matching
/// `crcbl_shaders::cull::Params`.
///
/// A uniform buffer rather than push constants, for the reason
/// `compute_probe.slang` gives: WGSL has no push constants, and this shader
/// declares that target.
struct CullParams
{
    /// The camera frustum's six half-spaces, `xyz` the (unnormalized) outward
    /// plane normal and `w` its offset. A point is inside the plane when
    /// `dot(xyz, p) + w >= 0`.
    float4 planes[PLANE_COUNT];
    /// Instances to test: the dispatch covers `0..instance_count`. A dispatch
    /// is a whole number of workgroups, so the last one runs past the end
    /// whenever this is not a multiple of the workgroup size.
    uint instance_count;
    /// Elements `visible` can hold. A survivor whose slot is at or past this
    /// still counts — see `visible_count` — but is not written.
    uint capacity;
};

[[vk::binding(0, 0)]]
ConstantBuffer<CullParams> cull;

/// The instance array. `StructuredBuffer`, so this is a read-only storage
/// buffer: the cull pass decides what is visible and never edits an instance.
[[vk::binding(1, 0)]]
StructuredBuffer<GpuInstance> instances;

/// The mesh table, indexed by `GpuInstance::mesh`. Read-only for the same
/// reason.
[[vk::binding(2, 0)]]
StructuredBuffer<GpuMesh> meshes;

/// The compacted survivors, as indices into `instances`. **Written in whatever
/// order the atomic below hands slots out**, which is not ascending and is not
/// reproducible between runs — a consumer reads `visible_count` of them and
/// must not assume anything else.
[[vk::binding(3, 0)]]
RWStructuredBuffer<uint> visible;

/// How many survivors there were, in element `INSTANCE_SURVIVOR_WORD`.
/// `clear_counters.slang` zeroes it before the dispatch.
///
/// The buffer is the frame's culling statistics rather than this pass's own
/// counter: `mesh_cluster.slang`'s amplification stage counts surviving
/// *clusters* into the element beside this one, so the frame has one readback
/// rather than two.
///
/// This is the *true* count, which can exceed `CullParams::capacity`: the
/// atomic increments unconditionally and only the write into `visible` is
/// bounded. So an overflow is a number a caller can see and act on rather than
/// a list that quietly stops growing — the "overflow counter" topic 03's
/// 2026-07-27 correction asks per-bucket capacity to carry.
///
/// **`Atomic<uint>` rather than a plain `uint` with `InterlockedAdd`**, which is
/// what this was first written as. WGSL has no such intrinsic — its atomics are
/// a distinct `atomic<u32>` *type* — and Slang refuses the entry point outright
/// for that target: "entrypoint 'computeMain' uses features that are not
/// available in 'compute' stage for 'wgsl' compilation target", pointing at
/// `InterlockedAdd`. Spelled as a type it lowers on all four: `atomicAdd` in
/// WGSL, `atomic_fetch_add_explicit` in MSL, and `InterlockedAdd` again in the
/// HLSL the DXIL is built from.
[[vk::binding(4, 0)]]
RWStructuredBuffer<Atomic<uint> > visible_count;

/// Invocations per workgroup, matching
/// `crcbl_shaders::cull::WORKGROUP_SIZE`. One instance each.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= cull.instance_count)
    {
        return;
    }

    GpuInstance instance = instances[index];
    // A slot whose instance has been removed keeps whatever transform and mesh
    // id it last had — `crcbl_render::InstancePool::remove` rewrites the flags
    // word and nothing else — so this is the first thing asked and the rest of
    // the record is never interpreted without it. First rather than second
    // because it is also what keeps a stale `instance.mesh` from indexing the
    // table below.
    if ((instance.flags & INSTANCE_LIVE) == 0)
    {
        return;
    }

    GpuMesh mesh = meshes[instance.mesh];
    // A mesh id whose entry has been cleared — `MeshPool::free` writes all
    // zeroes — names no geometry, and the all-zero bounds are a degenerate box
    // at the origin that would otherwise be "visible" whenever the origin is on
    // screen. The index count is the field that says an entry is empty, so it
    // is the field this asks.
    if (mesh.index_count == 0)
    {
        return;
    }

    float3 bounds_min = float3(mesh.min_x, mesh.min_y, mesh.min_z);
    float3 bounds_max = float3(mesh.max_x, mesh.max_y, mesh.max_z);
    float3 local_center = 0.5 * (bounds_max + bounds_min);
    float3 local_extent = 0.5 * (bounds_max - bounds_min);

    // The conservative transform the header names. `mul(m, v)` is row-wise —
    // `r[i] = dot(m[i], v)` — so `mul(abs(basis), e)` is
    // `r[i] = sum_j |m[i][j]| * e[j]`, which is the world-axis half-extent of
    // the rotated box.
    float3x3 basis = (float3x3)instance.transform;
    float3 center = mul(instance.transform, float4(local_center, 1.0)).xyz;
    float3 extent = mul(abs(basis), local_extent);

    for (uint plane = 0; plane < PLANE_COUNT; ++plane)
    {
        float4 half_space = cull.planes[plane];
        // How far the box can reach towards the plane from its centre, along
        // that plane's normal. The same `abs`-and-dot as the extent above, and
        // the reason neither the plane nor the normal needs to be unit length.
        float radius = dot(abs(half_space.xyz), extent);
        if (dot(half_space.xyz, center) + half_space.w < -radius)
        {
            return;
        }
    }

    uint slot = visible_count[INSTANCE_SURVIVOR_WORD].add(1);
    if (slot < cull.capacity)
    {
        visible[slot] = index;
    }
}
