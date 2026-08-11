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
// **Neither extent is the CPU's.** They are two of the three words
// `draw_gen.slang` writes for this bucket, and the seam's
// `draw_mesh_tasks_indirect` is what reads them: the x extent is the bucket's
// mesh's cluster count and the y extent is how many instances survived the
// instance cull. So a culled instance is not a workgroup that returns early —
// it is a workgroup that is never launched, which is the difference between
// culling that skips output and culling that skips work.
//
// The bounds check below survives that and is now exactly that: a bound. The
// extents make it true for every group the driver launches, and it stays
// because the extents arrive from a buffer rather than from a caller — a wrong
// argument structure would otherwise reach `visible_instances` past what
// `draw_gen.slang` wrote, and an out-of-range instance index is a hang rather
// than a wrong picture.
//
// # Per-cluster culling lives in the amplification stage, and only there
//
// §3.5's second bullet — frustum and normal-cone rejection per cluster — is
// `taskMain` below, which runs one workgroup per (cluster, instance slot) and
// dispatches the mesh stage for the pairs that survive. **The instance cull is
// unchanged and still runs first**: `cull.slang` decides which instances exist
// at all, `draw_gen.slang` compacts them, and this rejects clusters *within* a
// survivor.
//
// `Features::TASK_SHADER` is a capability separate from `MESH_SHADER`, so a
// device that has the mesh stage and not the amplification stage must keep
// drawing. That is what `meshMain` is: the un-amplified entry point, which
// culls nothing and emits every cluster of every surviving instance, exactly as
// it did before this stage existed. `amplifiedMeshMain` is the same body behind
// a payload — see `emit_cluster`, which is the whole of both.
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
    /// Bounding sphere and normal cone, in the mesh's own space. Read by
    /// `cluster_survives`, which is the amplification stage's whole job.
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
/// how many instances survived culling into this bucket. The same number sizes
/// this dispatch's y extent, out of `draw_gen.slang`'s separate mesh-argument
/// structure — so what `group_is_live` does with it below is a range check
/// rather than the filter it once was.
struct DrawIndexedArgs
{
    uint index_count;
    uint instance_count;
    uint first_index;
    int vertex_offset;
    uint first_instance;
};

/// Planes in a frustum, as `cull.slang` declares it.
static const uint PLANE_COUNT = 6;

/// The frustum this frame culls against, matching `struct CullParams` in
/// `cull.slang` and `crcbl_shaders::cull::Params`.
///
/// **The same buffer the instance cull reads**, bound a second time rather than
/// copied: the two rejections are one camera's, and a frustum that reached the
/// two stages differently would keep an instance and drop every cluster of it.
/// `crcbl_shaders::cull`'s `the_shared_structs_are_declared_identically_in_
/// every_shader` is what holds this declaration to that one.
struct CullParams
{
    /// The camera frustum's six half-spaces, `xyz` the (unnormalized) outward
    /// plane normal and `w` its offset. A point is inside the plane when
    /// `dot(xyz, p) + w >= 0`.
    float4 planes[PLANE_COUNT];
    /// Instances the cull pass tested. Read by nothing here — the mesh path's
    /// instance bound is its own dispatch extent.
    uint instance_count;
    /// Elements the visible list can hold. Read by nothing here either.
    uint capacity;
};

/// Which element of `cull_stats` counts surviving clusters.
///
/// Element 0 is `cull.slang`'s surviving *instances*, so this is the one word
/// this shader owns. The same number as
/// `crcbl_shaders::cull::CLUSTER_SURVIVOR_WORD`, held in step with it by that
/// module's `the_shaders_index_the_culling_stats_at_the_words_this_crate_says`.
static const uint CLUSTER_SURVIVOR_WORD = 1;

/// What `taskMain` hands `amplifiedMeshMain`: which pair of the dispatch
/// survived.
///
/// The two group indices rather than a resolved cluster record, because a
/// payload is per surviving group and the mesh stage can re-read what it needs
/// from the same buffers — where a copied record would be a second layout to
/// keep in step with `struct Meshlet` above.
struct ClusterPayload
{
    /// The cluster's index within this bucket's mesh's run, which is
    /// `SV_GroupID.x` of the task group that kept it.
    uint cluster;
    /// The slot in this bucket's run of surviving instances, which is that
    /// group's `SV_GroupID.y`.
    uint instance;
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
    /// How many clusters that mesh has. The dispatch's x extent is this number
    /// too, reached through `draw_gen.slang`'s mesh-argument structure, and the
    /// bound is checked again here so an argument structure naming a larger one
    /// cannot draw another mesh's clusters.
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

/// This frame's frustum — the amplification stage's alone.
///
/// **Bound only when there is an amplification stage**: a device without
/// `Features::TASK_SHADER` builds a pipeline whose layout stops at binding 12,
/// and `meshMain` reads neither this nor `cull_stats` below.
[[vk::binding(13, 0)]]
ConstantBuffer<CullParams> cull;

/// The frame's culling statistics, which this stage adds its surviving-cluster
/// count to at `CLUSTER_SURVIVOR_WORD`.
///
/// The buffer `cull.slang` counts surviving instances into, one element along —
/// topic 03 §3.6's delayed readback ring is this one buffer, and a second
/// counter of its own would be a second readback. `Atomic<uint>` rather than
/// `InterlockedAdd` for `cull.slang`'s reason: spelled as a type it lowers on
/// every target.
[[vk::binding(14, 0)]]
RWStructuredBuffer<Atomic<uint> > cull_stats;

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

/// How many workgroups a group of the dispatch is worth: `1` while both its
/// indices are inside what the frame actually holds, and `0` past either.
///
/// **A bound rather than a filter**, since the dispatch extents became the same
/// two numbers: every group the driver launches is inside them. It is kept
/// because those extents are read out of a buffer — see the header — so this is
/// the range check on the only untrusted input either stage has, and what it
/// guards is `visible_instances` being indexed past the run `draw_gen.slang`
/// filled.
///
/// **Arithmetic rather than a comparison**, because both entry points below put
/// the result in front of a call the DXIL validator wants unbranched:
/// `SetMeshOutputCounts` refuses to be reached through a branch on dxc 1.9, and
/// Slang lowers every `?:` into an `if`/`else` pair. `min(1, max(a, b) - b)` is
/// `1` exactly when `a > b`, and the `max` is what stops the unsigned
/// subtraction wrapping.
uint group_is_live(uint3 group)
{
    uint drawn = draw_args[draw.bucket].instance_count;
    return min(1u, max(drawn, group.y) - group.y)
         * min(1u, max(draw.cluster_count, group.x) - group.x);
}

/// Whether a cluster of an instance may contribute a pixel: `1` to draw it, `0`
/// to reject it.
///
/// **The cull is in world space**, which is where the frustum already is: the
/// planes are `cull.slang`'s own for this frame, and `frame.camera_position` is
/// world-space too. `GpuInstance::transform` is rigid — its own documented
/// obligation, the same one the normal transform above rests on — so the
/// bounding sphere moves exactly: the centre by the whole matrix, the radius
/// not at all, and the cone axis by the 3x3 with its length intact.
///
/// # Frustum
///
/// The ordinary sphere-against-half-space test. The planes are **not**
/// normalized and must not be — `cull.slang`'s header says why, and the far
/// plane of an infinite reversed-Z projection has a zero normal — so the
/// radius is scaled by the plane normal's length rather than the plane by its
/// reciprocal. A zero normal makes the term zero and leaves `w >= 0`, which is
/// the degenerate plane rejecting nothing, exactly as it should.
///
/// # Normal cone
///
/// With `a = acos(cone_cutoff)` the cone's half angle, `d = center - camera`
/// and `r` the radius, every triangle of the cluster faces away when
///
/// ```text
/// cone_cutoff > 0 && dot(cone_axis, d) > sqrt(1 - cone_cutoff^2) * length(d) + r
/// ```
///
/// **The `+ r` is the whole of what makes this safe.** Without it the test
/// treats every triangle as sharing the centre's view direction, which is right
/// only for a cluster of zero radius: a cluster with a real radius close to the
/// camera can then hold a front-facing triangle and still be rejected, which is
/// dropped geometry. `crcbl_shaders::meshlet::ClusterBounds::cone_cutoff`
/// carries the derivation, and `crcbl_render::cull::cluster_survives_cull` is
/// the same rule in Rust with the cases that pin it.
///
/// The `cone_cutoff > 0` half is not redundant beside the radius term:
/// `sqrt(1 - cutoff^2)` is even in `cutoff`, so it cannot tell a narrow cone
/// from a cone wider than a hemisphere, and the latter holds a front-facing
/// normal for every possible view.
uint cluster_survives(Meshlet cluster, float4x4 transform)
{
    float3 center = mul(transform, float4(cluster.center_x, cluster.center_y, cluster.center_z, 1.0)).xyz;
    float radius = cluster.radius;

    for (uint plane = 0; plane < PLANE_COUNT; ++plane)
    {
        float4 half_space = cull.planes[plane];
        if (dot(half_space.xyz, center) + half_space.w < -radius * length(half_space.xyz))
        {
            return 0u;
        }
    }

    float3 axis = mul((float3x3)transform,
                      float3(cluster.cone_axis_x, cluster.cone_axis_y, cluster.cone_axis_z));
    float3 to_center = center - frame.camera_position.xyz;
    float sine = sqrt(max(0.0, 1.0 - cluster.cone_cutoff * cluster.cone_cutoff));
    if (cluster.cone_cutoff > 0.0
        && dot(axis, to_center) > sine * length(to_center) + radius)
    {
        return 0u;
    }

    return 1u;
}

/// One cluster of one instance, emitted — the body both mesh entry points are.
///
/// `[ForceInline]`, and it is load-bearing on two of the three targets for the
/// reason `mesh_shader.slang`'s `emit` records: mesh output arrays crossing a
/// function boundary abort Slang's Metal backend and produce HLSL dxc rejects
/// with "Not all elements of output SV_Position were written". Inlined, all
/// three take it — and the alternative is two copies of this body, which is the
/// drift the arrangement exists to prevent.
///
/// `active` is `0` for a group with nothing to draw, which emits no vertices
/// and no primitives. The amplification stage passes `1` unconditionally: a
/// group it dispatched is one it kept.
[ForceInline]
void emit_cluster(
    uint lane,
    uint cluster_index,
    uint instance_slot,
    uint active,
    out vertices VertexOutput verts[MAX_CLUSTER_VERTICES],
    out indices uint3 tris[MAX_CLUSTER_TRIANGLES])
{
    // An inactive group reads cluster `cluster_base`, which is in bounds
    // because a bucket names a mesh and a mesh has at least one cluster. It
    // reads *something* rather than nothing so the load stays in front of the
    // call too.
    //
    // **One `SetMeshOutputCounts`, with no control flow in front of it.** DXIL
    // validation refuses more than one call — "SetMeshOutputCounts cannot be
    // called multiple times" — and dxc 1.9 reports that for a *single* call
    // reached through a branch as well. Both bounds are workgroup-uniform, so
    // every thread agrees, which is what the call requires.
    Meshlet cluster = clusters[cluster_index];
    SetMeshOutputCounts(cluster.vertex_count * active, cluster.triangle_count * active);
    if (active == 0u)
    {
        return;
    }

    // The instance names its mesh and the mesh names its vertices, exactly as
    // in `mesh.slang`: nothing on this path came from the dispatch, which is
    // what lets one bucket cover whatever culling left it.
    GpuInstance instance = instances[visible_instances[draw.base + instance_slot]];
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

/// The un-amplified mesh stage: **every** cluster of every surviving instance,
/// culled by nothing.
///
/// What a device with `Features::MESH_SHADER` and no `Features::TASK_SHADER`
/// draws through, and it is unchanged by the amplification stage existing. A
/// mesh entry point that declared `in payload` would require a task stage
/// upstream, which is why this and `amplifiedMeshMain` are two entry points
/// rather than one with a parameter.
[shader("mesh")]
[outputtopology("triangle")]
[numthreads(THREADS, 1, 1)]
void meshMain(
    uint lane: SV_GroupThreadID,
    uint3 group: SV_GroupID,
    out vertices VertexOutput verts[MAX_CLUSTER_VERTICES],
    out indices uint3 tris[MAX_CLUSTER_TRIANGLES])
{
    uint active = group_is_live(group);
    emit_cluster(lane, draw.cluster_base + group.x * active, group.y, active, verts, tris);
}

/// §3.5's per-cluster cull: one workgroup per (cluster, instance slot), and a
/// mesh workgroup only for the pairs that survive.
///
/// One thread, because the decision is one cluster's and there is nothing to
/// parallelise over. `DispatchMesh` is called **exactly once and unbranched**,
/// with a group count of `1` or `0` — the DXIL rule `SetMeshOutputCounts` has,
/// and the reason nothing above it is a branch either.
///
/// A group past either bound — which the dispatch extents make unreachable, and
/// a wrong argument structure would not — reads the bucket's first cluster and
/// instance slot zero, which are in bounds and whose values it then multiplies
/// away. That is what keeps `visible_instances` — whose slots past the survivor
/// count hold whatever the buffer last had — from ever reaching `instances` as
/// an index.
///
/// The counter is added to with the survival flag itself rather than under an
/// `if`, so the add of zero is the branch: one instruction, no control flow in
/// front of the dispatch, and the same statement for every group.
[shader("amplification")]
[numthreads(1, 1, 1)]
void taskMain(uint3 group: SV_GroupID)
{
    uint active = group_is_live(group);
    Meshlet cluster = clusters[draw.cluster_base + group.x * active];
    uint instance_index = visible_instances[draw.base + group.y * active] * active;
    GpuInstance instance = instances[instance_index];

    uint keep = active * cluster_survives(cluster, instance.transform);
    cull_stats[CLUSTER_SURVIVOR_WORD].add(keep);

    ClusterPayload payload;
    payload.cluster = group.x;
    payload.instance = group.y;
    DispatchMesh(keep, 1, 1, payload);
}

/// The mesh stage behind `taskMain`: `meshMain`'s body, for a pair the
/// amplification stage already decided to keep.
///
/// `SV_GroupID` here counts within the one workgroup `DispatchMesh` launched
/// and says nothing about which cluster this is, which is what the payload is
/// for.
[shader("mesh")]
[outputtopology("triangle")]
[numthreads(THREADS, 1, 1)]
void amplifiedMeshMain(
    uint lane: SV_GroupThreadID,
    in payload ClusterPayload amplification,
    out vertices VertexOutput verts[MAX_CLUSTER_VERTICES],
    out indices uint3 tris[MAX_CLUSTER_TRIANGLES])
{
    emit_cluster(
        lane,
        draw.cluster_base + amplification.cluster,
        amplification.instance,
        1u,
        verts,
        tris);
}
