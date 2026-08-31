// crcbl-targets: spirv, wgsl, msl, dxil

// The debug draw layer's one shader: world-space line segments, transformed by
// the camera and emitted in the colour they were appended with.
//
// `docs/plan/07-ui-debug.md` item 5 is what this belongs to — the immediate-mode
// buffer any system appends lines, boxes, spheres and frusta to — and
// `crcbl_render::debug_draw` is the Rust half that fills the buffer this reads.
// Every primitive in that module is a list of segments by the time it arrives
// here, so there is one topology and one entry-point pair rather than one per
// shape.
//
// There is no vertex input declaration, for `triangle.slang`'s reason: the seam
// has no vertex-buffer layout and no `bind_vertex_buffer`, so geometry arrives
// through a storage buffer the vertex stage indexes by `SV_VertexID`. Two
// consecutive elements are one segment, which is what
// `PrimitiveTopology::LineList` assembles them as.
//
// # Matrix layout
//
// As `sprite.slang`: a `float4x4` in a constant buffer comes out of `slangc`
// decorated `RowMajor` with `MatrixStride 16`, and `mul(m, v)` comes out as
// `OpVectorTimesMatrix`, which together put the HLSL matrix's column 0 in the
// first sixteen bytes — exactly `glam::Mat4::to_cols_array`. No transpose on the
// way in, and no `-matrix-layout-*` flag on the compile.

/// One endpoint of one segment.
///
/// Two `float4`s rather than a packed `float3` pair, for `triangle.slang`'s
/// reason: under `std430` a `float4` member is 16-byte aligned with no tail
/// padding, so the CPU-side layout is exactly two 16-byte rows and cannot drift
/// from this declaration. `position.w` is unused — the vertex stage supplies the
/// homogeneous one itself — and `color` is linear RGBA, written into the HDR
/// scene target before the tonemap reads it.
struct DebugVertex
{
    float4 position;
    float4 color;
};

/// The camera this frame's segments are transformed by: world → clip.
struct DebugConstants
{
    float4x4 view_proj;
};

// **Declaration order is binding order**, which Slang's Metal target makes
// load-bearing — see `crcbl_shaders::declaration_order` for the rule and for the
// frame it once cost.

/// This frame's segments, two elements per line. Read-only storage buffer.
[[vk::binding(0, 0)]]
StructuredBuffer<DebugVertex> vertices;

[[vk::binding(1, 0)]]
ConstantBuffer<DebugConstants> constants;

struct DebugOutput
{
    float4 position : SV_Position;
    float4 color : COLOR0;
};

[shader("vertex")]
DebugOutput vertexMain(uint index: SV_VertexID)
{
    DebugVertex vertex = vertices[index];

    DebugOutput output;
    output.position = mul(constants.view_proj, float4(vertex.position.xyz, 1.0));
    output.color = vertex.color;
    return output;
}

[shader("fragment")]
float4 fragmentMain(DebugOutput input) : SV_Target
{
    return input.color;
}
