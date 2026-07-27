// The engine's first drawing shader: milestone 2 of
// `docs/plan/02-vulkan-backend.md`'s ladder — "Triangle (vertex pulling from a
// storage buffer — no vertex input state; this is the pattern stage 3 scales
// up)".
//
// There is no vertex input declaration anywhere in this file, and that is the
// architecture rather than a simplification: `crcbl-hal`'s `GraphicsPipelineDesc`
// has no vertex-buffer layout and there is no `bind_vertex_buffer`, so geometry
// can only ever arrive through a storage buffer the vertex stage indexes. Stage
// 3 replaces this one buffer with the global vertex pool and the index with
// `base_vertex + i`; the shader shape does not change.
//
// Both entry points live in one module. That is what `ShaderEntry`'s per-stage
// `entry_point` field is for, and it is the concrete reason the seam names an
// entry point instead of assuming `main`: Slang routinely emits a vertex and
// fragment pair into a single artifact, and one `OpEntryPoint` per stage is what
// comes out.

/// One pulled vertex.
///
/// Two `float4`s rather than a packed `float3` + `float3`: under `std430` a
/// `float4` member is 16-byte aligned with no tail padding, so the CPU-side
/// layout is exactly two 16-byte rows and cannot drift from this declaration.
/// `position` is clip space directly — there is no camera until milestone 3 —
/// and `color.a` is carried so the seam's `Rgba16Float` target gets a real
/// alpha rather than an implied one.
struct Vertex
{
    float4 position;
    float4 color;
};

/// The pulled geometry. `StructuredBuffer` (rather than `RWStructuredBuffer`)
/// is what makes this a read-only storage buffer, matching
/// `BindingKind::StorageBuffer { read_only: true }` on the Rust side — the
/// backend maps that to `NonWritable`, which lets the render graph merge
/// barriers it otherwise could not.
[[vk::binding(0, 0)]]
StructuredBuffer<Vertex> vertices;

struct VertexOutput
{
    float4 position : SV_Position;
    float4 color : COLOR0;
};

// `SV_VertexID` compiles to `gl_VertexIndex - gl_BaseVertex`, which declares the
// SPIR-V `DrawParameters` capability and therefore needs `shaderDrawParameters`
// on the device. `crcbl-vk` enables it: it is Vulkan 1.1 core-optional, present
// on both radv and lavapipe, and stage 3's indirect draws need the base-vertex
// and base-instance semantics for exactly the same reason a global pool does.
// Slang has no portable spelling for the raw `VertexIndex` builtin in this
// release — `[[vk::builtin("VertexIndex")]]` is accepted with a warning and then
// silently lowered to a *location-0 vertex input*, which would reintroduce the
// vertex input state this design exists to remove.
[shader("vertex")]
VertexOutput vertexMain(uint index: SV_VertexID)
{
    Vertex vertex = vertices[index];

    VertexOutput output;
    output.position = vertex.position;
    output.color = vertex.color;
    return output;
}

[shader("fragment")]
float4 fragmentMain(VertexOutput input) : SV_Target
{
    return input.color;
}
