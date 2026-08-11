// crcbl-targets: spirv, msl, dxil

// `docs/plan/03-gpu-driven-rendering.md` §3.5's geometry path, in the forward
// pass: the same lit mesh `mesh.slang` draws, with a **mesh stage** in place of
// its vertex stage and a cluster in place of an index range.
//
// # Why this is a second file rather than a second entry point on `mesh.slang`
//
// **`mesh.slang` must keep emitting WGSL**, because `crcbl-wgpu` is the web
// backend and the forward pass is what it draws. WGSL has no mesh shaders at
// all — Slang refuses `SetMeshOutputCounts` for that target by name, which is
// what `mesh_shader.slang`'s header records — so a mesh entry point added to
// `mesh.slang` would make the whole file fail to compile for a target its own
// `// crcbl-targets:` line requires. A target list is per source, so the only
// way to have one source with WGSL and one without is two sources.
//
// That is the whole of the argument. Everything else about the two files is
// deliberately identical, and where it could drift it is not duplicated at all:
// **the fragment stage of the mesh pipeline is `mesh.slang`'s own
// `fragmentMain`**, named as a second module at pipeline creation. So the
// shading — Lambert, Blinn, the material row, the page sample — exists once. A
// copy of it here would be the drift this arrangement exists to prevent, and it
// would be invisible: two shading models that agree today and differ after one
// of them is edited.
//
// What *is* restated here is the data layout: `struct MeshVertex`,
// `FrameUniforms`, `GpuInstance`, `GpuMesh` and `VertexOutput` are declared
// again, exactly as `mesh_shader.slang` restates `triangle.slang`'s `Vertex`.
// Slang has no shared header this build uses, and the layouts are pinned from
// the Rust side by `crcbl_shaders::mesh`'s offset tests — one source of bytes,
// two declarations of them.
//
// **`VertexOutput` is the one that has to match by *order*, not just by
// layout.** Its members take interface locations in declaration order, and the
// fragment stage reading them is the other file's, so a member inserted here
// and not there is a fragment stage reading the wrong varying. The pair is
// checked by rendering: `crcbl-vk`'s `the_mesh_shader_path_draws_the_indirect
// _path's_frame` compares this pipeline's frame against the raster pipeline's
// on one device, pixel for pixel.
//
// # A cluster is the unit of work, and the CPU never counts them per frame
//
// One workgroup draws one cluster of one instance:
//
//   * `SV_GroupID.x` is the cluster, within the mesh this bucket draws.
//   * `SV_GroupID.y` is the slot in the bucket's run of *surviving* instances,
//     which `cull.slang` filled and `draw_gen.slang` compacted.
//
// The dispatch is sized by the CPU from two upper bounds — the bucket's mesh's
// cluster count, and how many instances the pool holds — because
// `draw_mesh_tasks` takes its group counts as arguments and this engine's seam
// has no indirect form of it yet. **The bound is not the answer**: how many
// instances actually survived is a number only the GPU has, so every workgroup
// reads it out of the bucket's indirect-argument structure and the ones past it
// emit nothing. That is what makes this the same picture as the indirect tails
// rather than an approximation of it — a culled instance draws nothing here for
// the same reason an instance count of zero draws nothing there.
//
// Per-cluster culling — frustum, normal cone — is §3.5's second bullet and a
// slice of its own. `Meshlet::bounds` is read by nothing below.
//
// # The zero-base rule does not apply here, and that is worth saying
//
// `mesh.slang`'s header measures the four targets disagreeing about
// `SV_VertexID` and `SV_InstanceID`, and resolves it by passing zero for every
// draw's bases. A mesh shader has neither semantic: there is no vertex fetch to
// have a base vertex, and no instancing to have a base instance. Every index
// below comes from a buffer — the cluster's corner run, its vertex run, the
// mesh table's `base_vertex` — so there is no draw-call number in this file at
// all, and nothing for a target to disagree about.

/// The most vertices one cluster may reference, and the size of this stage's
/// vertex output array.
///
/// The same number as `crcbl_shaders::meshlet::MAX_CLUSTER_VERTICES`, which is
/// what the builder bounds a cluster by. That crate's
/// `the_shader_declares_the_same_cluster_bounds` reads this line and fails if
/// the two ever differ — a stage declaring fewer vertices than a cluster may
/// reference writes past its own array.
static const uint MAX_CLUSTER_VERTICES = 64;

/// The most triangles one cluster may hold, and the size of this stage's
/// primitive output array. Held in step with
/// `crcbl_shaders::meshlet::MAX_CLUSTER_TRIANGLES` by the same test.
static const uint MAX_CLUSTER_TRIANGLES = 124;

/// Threads per workgroup: one per vertex of a full cluster.
///
/// [`MAX_CLUSTER_VERTICES`] rather than a number of its own, so the vertex loop
/// below is one iteration for every cluster the builder can produce. The
/// triangle loop takes more than one, which is why both are loops rather than a
/// thread index used directly.
static const uint THREADS = MAX_CLUSTER_VERTICES;

/// One vertex, matching `struct MeshVertex` in `mesh.slang` and
/// `crcbl_shaders::mesh::MeshVertex`.
struct MeshVertex
{
    float4 position;
    float4 normal;
    float4 color;
    /// Base-colour texture coordinates in `xy`; `zw` unused.
    float4 uv;
};

/// Matching `struct FrameUniforms` in `mesh.slang`.
struct FrameUniforms
{
    float4x4 view_proj;
    float4 camera_position;
    float4 light_direction;
    float4 light_color;
    float4 ambient;
};

/// Matching `struct GpuInstance` in `mesh.slang`.
struct GpuInstance
{
    float4x4 transform;
    uint mesh;
    uint material;
    uint sector;
    uint flags;
};

/// Matching `struct GpuMesh` in `mesh.slang`.
struct GpuMesh
{
    uint base_vertex;
    uint base_index;
    uint index_count;
    float min_x;
    float min_y;
    float min_z;
    float max_x;
    float max_y;
    float max_z;
};

/// Matching `struct GpuMaterial` in `mesh.slang`. Declared and not read — see
/// the binding below.
struct GpuMaterial
{
    float4 base_color;
    uint base_color_texture;
    uint pad0;
    uint pad1;
    uint pad2;
};

/// One cluster, matching `crcbl_shaders::meshlet::Meshlet`.
///
/// Scalars rather than a `float3` for the bounds, for `GpuMesh`'s reason: under
/// `std430` a `float3` is 16-byte aligned, so a struct of scalars is the only
/// one whose stride is the sum of its members. `MESHLET_STRIDE` is 48 and this
/// is what makes it so.
struct Meshlet
{
    /// First entry of this cluster's run of `cluster_vertices`.
    uint vertex_offset;
    /// How many original vertex indices it references. At most
    /// [`MAX_CLUSTER_VERTICES`].
    uint vertex_count;
    /// First entry of its run of `cluster_corners`, **counted in corners** —
    /// not in the words they are packed into.
    uint triangle_offset;
    /// How many triangles it holds. At most [`MAX_CLUSTER_TRIANGLES`].
    uint triangle_count;
    /// Bounding sphere and normal cone. **Read by nothing here**: per-cluster
    /// culling is a slice of its own, and the record carries them because they
    /// share a cluster's lifetime.
    float center_x;
    float center_y;
    float center_z;
    float radius;
    float cone_axis_x;
    float cone_axis_y;
    float cone_axis_z;
    float cone_cutoff;
};

/// One indexed indirect draw's arguments, matching `draw_gen.slang`'s.
///
/// Read here as **data** rather than executed as a draw: `instance_count` is
/// how many instances survived culling into this bucket, and that number is the
/// GPU's alone. See the header on why a dispatch bound is not an answer.
struct DrawIndexedArgs
{
    uint index_count;
    uint instance_count;
    uint first_index;
    int vertex_offset;
    uint first_instance;
};

/// Where this bucket's run of visible instances starts, and which clusters its
/// mesh owns. Matching `crcbl_shaders::meshlet::ClusterDrawConstants`.
///
/// A block of its own rather than `mesh.slang`'s `DrawConstants`, because it
/// says three more things and the raster path would carry them unread. One
/// block per bucket, reached through a dynamic offset — the same mechanism the
/// raster path uses for its own.
struct ClusterDrawConstants
{
    /// This bucket's first slot in `visible_instances`, added to the group's
    /// instance slot.
    uint base;
    /// This bucket's mesh's first cluster in `clusters`.
    uint cluster_base;
    /// How many clusters that mesh has. The dispatch is sized to this, and the
    /// bound is checked again here so a caller that over-dispatched cannot
    /// draw another mesh's clusters.
    uint cluster_count;
    /// Which bucket this is, which is the element of `draw_args` holding the
    /// instance count culling produced.
    uint bucket;
};

[[vk::binding(0, 0)]]
ConstantBuffer<FrameUniforms> frame;

[[vk::binding(1, 0)]]
StructuredBuffer<MeshVertex> vertices;

[[vk::binding(2, 0)]]
StructuredBuffer<GpuInstance> instances;

[[vk::binding(3, 0)]]
ConstantBuffer<ClusterDrawConstants> draw;

[[vk::binding(4, 0)]]
StructuredBuffer<GpuMesh> meshes;

[[vk::binding(5, 0)]]
StructuredBuffer<uint> visible_instances;

/// The material table. **Declared here and read by nothing**, and the reason is
/// Metal: Slang's Metal target ignores `[[vk::binding]]` and hands each
/// resource the next index in its stage's flat table, in declaration order,
/// while `crcbl-mtl` computes an index by counting same-table descriptors in
/// the *layout*. The layout this pipeline is built with carries bindings 6, 7
/// and 8 because `mesh.slang`'s fragment stage reads them — so a source that
/// skipped them would put `clusters` at buffer index 6 in the MSL and have the
/// backend bind it at 9. `crcbl_shaders::declaration_order` is the lint that
/// checks the ordering half of this rule; the completeness half is this
/// comment.
[[vk::binding(6, 0)]]
StructuredBuffer<GpuMaterial> materials;

/// The base-colour page, on `materials`' terms exactly: the fragment stage
/// samples it, this stage does not, and the declaration is what keeps the
/// Metal tables aligned with the layout.
[[vk::binding(7, 0)]]
Texture2DArray<float4> base_color_textures;

/// The page's sampler, for the same reason.
[[vk::binding(8, 0)]]
SamplerState base_color_sampler;

/// Every resident mesh's clusters, back to back, in mesh order. A bucket's
/// mesh names its own run with `draw.cluster_base` and `draw.cluster_count`.
[[vk::binding(9, 0)]]
StructuredBuffer<Meshlet> clusters;

/// Original vertex indices, one run per cluster. **Mesh-relative**, like the
/// index buffer the raster path reads: `GpuMesh::base_vertex` is added here.
[[vk::binding(10, 0)]]
StructuredBuffer<uint> cluster_vertices;

/// Triangle corners, packed four to a word, least significant byte first.
///
/// A storage buffer has no byte addressing, so corner `n` is byte `n & 3` of
/// word `n >> 2` — which is exactly what a little-endian load of the host's
/// `u8` array reads. `crcbl_shaders::meshlet::corner_words` is the other half
/// of that statement. Packing by word rather than padding each cluster's run
/// is what lets a cluster closed early by the vertex bound end on any corner.
[[vk::binding(11, 0)]]
StructuredBuffer<uint> cluster_corners;

/// The per-bucket indirect arguments `draw_gen.slang` wrote. Read for
/// `instance_count` and nothing else.
[[vk::binding(12, 0)]]
StructuredBuffer<DrawIndexedArgs> draw_args;

/// What the mesh stage emits and `mesh.slang`'s `fragmentMain` consumes.
///
/// **Member order is the interface**, not just the layout: locations are
/// assigned in declaration order and the consumer is the other file's fragment
/// stage. See the header.
struct VertexOutput
{
    float4 position : SV_Position;
    float3 world_position : POSITION0;
    float3 world_normal : NORMAL0;
    float4 color : COLOR0;
    /// This primitive's material row. `nointerpolation` for the reason
    /// `mesh.slang` gives at length: an index is not a quantity, and
    /// interpolating one produces a fragment reading a row no vertex named.
    nointerpolation uint material : TEXCOORD0;
    float2 uv : TEXCOORD1;
};

/// Corner `corner` of the packed corner array.
uint corner_at(uint corner)
{
    uint word = cluster_corners[corner >> 2];
    return (word >> ((corner & 3) * 8)) & 0xffu;
}

[shader("mesh")]
[outputtopology("triangle")]
[numthreads(THREADS, 1, 1)]
void meshMain(
    uint lane: SV_GroupThreadID,
    uint3 group: SV_GroupID,
    out vertices VertexOutput verts[MAX_CLUSTER_VERTICES],
    out indices uint3 tris[MAX_CLUSTER_TRIANGLES])
{
    // A workgroup the dispatch bound covered and the frame does not: either its
    // instance did not survive culling, or the bucket's mesh has fewer clusters
    // than the dispatch was sized for. It emits nothing, which is what makes an
    // over-sized dispatch the same picture as an exact one.
    //
    // Both bounds are workgroup-uniform, so every thread agrees — which
    // `SetMeshOutputCounts` requires, and which is why the instance count is
    // read once here rather than per lane.
    uint drawn = draw_args[draw.bucket].instance_count;

    // **One `SetMeshOutputCounts`, with no control flow in front of it.** DXIL
    // validation refuses more than one call — "SetMeshOutputCounts cannot be
    // called multiple times" — and dxc 1.9 reports that for a *single* call
    // reached through a branch as well, which is what a `?:` becomes: Slang
    // lowers every select into an `if`/`else` pair, so the emitted HLSL had the
    // call in a joined block and the validator refused it. Hence arithmetic.
    //
    // `min(1, max(a, b) - b)` is `1` exactly when `a > b`, and it is
    // unsigned-safe because the `max` is what stops the subtraction wrapping.
    uint active = min(1u, max(drawn, group.y) - group.y)
                * min(1u, max(draw.cluster_count, group.x) - group.x);

    // An inactive group reads cluster `cluster_base`, which is in bounds
    // because a bucket names a mesh and a mesh has at least one cluster. It
    // reads *something* rather than nothing so the load stays in front of the
    // call too.
    Meshlet cluster = clusters[draw.cluster_base + group.x * active];
    SetMeshOutputCounts(cluster.vertex_count * active, cluster.triangle_count * active);
    if (active == 0u)
    {
        return;
    }

    // The instance names its mesh and the mesh names its vertices, exactly as
    // in `mesh.slang`: nothing on this path came from the dispatch, which is
    // what lets one bucket cover whatever culling left it.
    GpuInstance instance = instances[visible_instances[draw.base + group.y]];
    GpuMesh mesh = meshes[instance.mesh];

    for (uint v = lane; v < cluster.vertex_count; v += THREADS)
    {
        uint index = cluster_vertices[cluster.vertex_offset + v];
        MeshVertex vertex = vertices[index + mesh.base_vertex];

        float4 world = mul(instance.transform, float4(vertex.position.xyz, 1.0));

        VertexOutput output;
        output.position = mul(frame.view_proj, world);
        output.world_position = world.xyz;
        // The transform is rigid, so its 3x3 part is its own inverse-transpose
        // — the obligation `GpuInstance::transform` carries, and the same line
        // `mesh.slang`'s vertex stage writes.
        output.world_normal = mul((float3x3)instance.transform, vertex.normal.xyz);
        output.color = vertex.color;
        output.material = instance.material;
        output.uv = vertex.uv.xy;
        verts[v] = output;
    }

    for (uint t = lane; t < cluster.triangle_count; t += THREADS)
    {
        uint corner = cluster.triangle_offset + t * 3;
        // Corner indices are cluster-local, which is what the vertex array
        // above is indexed by too — so the triple needs no base added to it.
        tris[t] = uint3(corner_at(corner), corner_at(corner + 1), corner_at(corner + 2));
    }
}
