// crcbl-targets: spirv, msl, dxil

// The mesh-shading path's bring-up shader — one triangle, emitted by a mesh
// stage, with an amplification stage in front of it.
//
// `docs/plan/03-gpu-driven-rendering.md` §3.5 makes mesh shaders the **primary**
// geometry path rather than an optimisation, and this is the smallest thing
// that proves the whole path exists: `Features::MESH_SHADER` was reported by
// `crcbl-vk` before anything could ask for it. What §3.5 actually wants —
// meshlets, per-cluster culling in the amplification stage, cluster LOD — needs
// a mesh asset system that does not exist yet, so none of it is here.
//
// # The first shader that is not all four targets
//
// **WGSL cannot express a mesh shader at all.** Slang 2026.14 refuses it by
// name — "entrypoint 'meshMain' uses features that are not available in '_mesh'
// stage for 'wgsl' compilation target", pointing at `SetMeshOutputCounts` — and
// that refusal is the whole reason `docs/plan/02-vulkan-backend.md`'s first
// shader-portability rule exists. Without the `// crcbl-targets:` line above,
// the compile script would have emitted a broken `.wgsl` and committed it.
//
// MSL and DXIL both take it: Slang lowers the output arrays to Metal's
// `metal::mesh<…>` and to HLSL's own `out vertices` / `out indices`, and `dxc`
// compiles those at `ms_6_6` and `as_6_6`. Neither `crcbl-mtl` nor `crcbl-dx12`
// can create a mesh pipeline yet — both refuse it — but a declared target is a
// claim about what the *compiler* accepts, and those two artifacts are what
// will catch it the day the compiler stops accepting it.
//
// # The geometry is pulled, exactly as the raster path's is
//
// `triangle.slang` pulls its vertices from a storage buffer because vertex
// pulling is the engine's only geometry path, and a mesh shader is not an
// exception to it: the buffer below is bound with `ShaderStages::MESH`
// visibility, which `crcbl-hal` now accepts and every backend refuses on a
// device without `Features::MESH_SHADER` (see `ShaderStages::check_supported`).
// What is *not* here is a meshlet — the vertices are one small buffer a caller
// uploads rather than a range of a global pool, because the mesh asset system
// §3.5 wants does not exist yet.
//
// The three vertices are deliberately **not** `triangle.slang`'s: apex down
// instead of up, so a golden image of this can never be satisfied by a copy of
// that one. `crcbl_shaders::mesh_shader` owns the values and the byte layout,
// the way `crcbl_shaders::triangle` owns the raster path's.
//
// # Winding
//
// `crcbl-vk` flips Y with a negative-height viewport so the seam's Y-up
// convention survives Vulkan's inverted clip space, which makes these three
// counter-clockwise in framebuffer space — matching `FrontFace::Ccw`, exactly
// as `crcbl_shaders::triangle::VERTICES` does.

/// One pulled vertex, in the `std430` layout
/// `crcbl_shaders::mesh_shader::vertex_bytes` writes: two `float4`s, 32 bytes,
/// no padding. The same shape `triangle.slang`'s `Vertex` has, and a separate
/// declaration for the same reason the two shaders are separate files — one
/// source, one layout, no shared header to keep in step.
struct Vertex
{
    float4 position;
    float4 color;
};

/// The pulled geometry, read by the **mesh** stage rather than a vertex stage.
/// `StructuredBuffer` rather than `RWStructuredBuffer` makes it a read-only
/// storage buffer, matching `BindingKind::StorageBuffer { read_only: true }`
/// with `ShaderStages::MESH` visibility on the Rust side.
[[vk::binding(0, 0)]]
StructuredBuffer<Vertex> vertices;

/// What the mesh stage emits: the same two fields, with the semantics the
/// rasteriser reads.
struct MeshVertex
{
    float4 position : SV_Position;
    float4 color : COLOR0;
};

/// What the amplification stage hands the mesh stage.
///
/// A tint rather than a count, because a payload the mesh stage *ignores* would
/// make the amplification stage unobservable: the picture would be identical
/// whether the stage ran, was dropped from the pipeline, or delivered nothing.
struct Amplification
{
    float4 tint;
};

/// One thread's share of the triangle: its vertex, and the index triple once.
///
/// **`[ForceInline]` is load-bearing on two of the three targets.** Left as an
/// ordinary call, Slang 2026.14 aborts the Metal backend outright ("Unknown
/// addressspace encountered", an internal error), and its HLSL comes out in a
/// shape `dxc` rejects — "Not all elements of output SV_Position were written",
/// because the mesh output arrays crossed a function boundary and DXC's writes
/// analysis stops there. Inlined, all three targets take it. Without the
/// attribute the alternative is two copies of this body, one per mesh entry
/// point, which is the drift this exists to prevent.
[ForceInline]
void emit(
    uint index,
    float4 tint,
    out indices uint3 triangles[1],
    out vertices MeshVertex verts[3])
{
    SetMeshOutputCounts(3, 1);

    Vertex pulled = vertices[index];

    MeshVertex vertex;
    vertex.position = pulled.position;
    vertex.color = pulled.color * tint;
    verts[index] = vertex;

    // One thread writes the single primitive; all three write a vertex.
    if (index == 0)
    {
        triangles[0] = uint3(0, 1, 2);
    }
}

// One workgroup of three threads, one thread per vertex. Not a meshlet size:
// three is what this triangle needs, and a size chosen for a cluster budget
// would be a number pretending to mean something.
[shader("mesh")]
[outputtopology("triangle")]
[numthreads(3, 1, 1)]
void meshMain(
    uint index: SV_DispatchThreadID,
    out indices uint3 triangles[1],
    out vertices MeshVertex verts[3])
{
    emit(index, float4(1.0, 1.0, 1.0, 1.0), triangles, verts);
}

/// The payload the amplification stage fills. `groupshared` is what
/// `DispatchMesh` requires of its payload argument.
groupshared Amplification amplification;

// Kills the red channel, which is what makes the stage observable downstream:
// the apex is red, so a frame drawn through this stage has a *black* apex and
// an unchanged green and blue corner. A payload that never arrived reads as
// zero and blackens all three, which is a different picture again.
[shader("amplification")]
[numthreads(1, 1, 1)]
void taskMain()
{
    amplification.tint = float4(0.0, 1.0, 1.0, 1.0);
    DispatchMesh(1, 1, 1, amplification);
}

// A second mesh entry point rather than a parameter on the first: a mesh shader
// declaring `in payload` requires a task stage upstream, so one entry point
// cannot serve both `MeshPipelineDesc::task: None` and `Some(…)`.
[shader("mesh")]
[outputtopology("triangle")]
[numthreads(3, 1, 1)]
void amplifiedMeshMain(
    uint index: SV_DispatchThreadID,
    in payload Amplification amplification,
    out indices uint3 triangles[1],
    out vertices MeshVertex verts[3])
{
    emit(index, amplification.tint, triangles, verts);
}

// Shared by both mesh entry points; there is nothing stage-specific about it.
[shader("fragment")]
float4 fragmentMain(MeshVertex input) : SV_Target
{
    return input.color;
}
