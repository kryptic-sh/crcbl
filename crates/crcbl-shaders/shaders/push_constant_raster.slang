// crcbl-targets: spirv, msl, dxil

// The raster half of the push-constant surface: a vertex/fragment pair whose
// geometry *and* colour both come out of the block.
//
// `push_constant_probe.slang` beside this one drives `Features::PUSH_CONSTANTS`
// through a compute dispatch, and that is all any test in the tree did. The
// plumbing a graphics pipeline needs is not the same plumbing: D3D12 computes a
// root parameter's shader visibility from the range's stages, Metal has a
// separate argument table per stage and sends the block with
// `setVertexBytes:`/`setFragmentBytes:` rather than the compute call, and
// Vulkan's `vkCmdPushConstants` takes a stage mask a compute-only range never
// varies. A backend wrong for the graphics stages passed every gate this
// workspace had.
//
// # The two stages read different halves of the block
//
// `color` is read only by the fragment stage and `rect` only by the vertex
// stage, which is what makes a per-stage failure visible rather than folded
// into one verdict:
//
// * a block that reached the vertex stage alone draws the rectangle in black,
//   because `color` came back zero;
// * a block that reached the fragment stage alone draws **nothing at all** —
//   `rect` came back zero, so all four corners collapse onto the clip-space
//   origin and the two triangles have no area.
//
// A block that reached neither is the second of those, and the seam's exercise
// separates them by what the target holds rather than by which call was made.
//
// # There is no vertex buffer, and that is what the block is for
//
// Every other drawing shader here pulls its vertices from a
// `StructuredBuffer<Vertex>` — see `triangle.slang`, whose comment explains why
// the seam has no vertex input state. This one pulls nothing: the four corners
// come out of `rect` and the index the rasteriser hands each invocation. So a
// pixel of the expected colour in the expected column is a value the pipeline
// had no source for other than the block, where a shader that also read a bound
// buffer could have drawn the same picture from that instead.
//
// The consequence for the seam is that the pipeline layout carries **no bind
// group at all**, which is also the arithmetic each backend's slot computation
// gets asked for the first time here: Metal's block index is the buffer-table
// entry after the last binding, and with no bindings that is zero.
//
// # No WGSL, for `push_constant_probe.slang`'s reason
//
// WGSL has no push constants and Slang's WGSL target does not say so — it emits
// a module-scope `var<uniform>` with no `@group`/`@binding`, which naga rejects.
// The `// crcbl-targets:` line above is what keeps that artifact out of the
// tree, and `tests/wgsl_validation.rs` is what catches it being added back. See
// this crate's docs, "A push constant costs the WGSL target".

/// The clip-space depth every corner is emitted at.
///
/// Anywhere inside the clip volume would do — the seam's exercise binds no
/// depth attachment — and the middle is chosen so that neither plane is near
/// enough for a backend's depth convention to decide whether the quad survives.
static const float RECT_DEPTH = 0.5;

/// The push-constant block, matching
/// `crcbl_shaders::push_constant_raster::Constants`.
///
/// Two `float4`s, for the reason `push_constant_probe.slang`'s block is a
/// `uint4` rather than a `uint[4]`: Slang lays a push-constant block out as
/// `std430` where a vector is tightly packed, while an HLSL `cbuffer` gives an
/// *array* element a sixteen-byte register each. One `PushConstantRange`
/// describes every backend only while the block's layout does not vary by
/// target, and a pair of four-component vectors is sixteen bytes at offset 0
/// and sixteen at offset 16 under both rules — and under Metal's, which
/// aligns a `float4` to sixteen bytes too.
struct RasterConstants
{
    /// What the fragment stage writes to the colour target.
    float4 color;

    /// The clip-space rectangle the vertex stage spans, as
    /// `(min x, min y, max x, max y)`.
    ///
    /// A rectangle rather than a triangle so that the drawn region is symmetric
    /// about `y = 0`: the seam's raster exercises sample the target's centre row
    /// and must not depend on which sign convention clip-space Y carries, which
    /// is `RASTER_TRIANGLE`'s reason for being symmetric about `x = 0`.
    float4 rect;
};

/// The only resource this shader declares, so it is trivially last — the
/// position `crcbl_shaders::declaration_order` requires a push constant to hold
/// and the one `crcbl-mtl` leaves free past the numbered bindings.
[[vk::push_constant]]
ConstantBuffer<RasterConstants> constants;

struct VertexOutput
{
    float4 position : SV_Position;
};

/// One corner of `constants.rect`, for a draw of six vertices: the two
/// triangles `(0, 1, 2)` and `(0, 2, 3)` over the corners numbered
/// anticlockwise from the rectangle's minimum.
///
/// EACH COMPONENT IS NAMED, RATHER THAN INDEXED BY `corner`.
///
/// `push_constant_probe.slang` carries the measurement this follows: NIR
/// defines a `load_push_constant` offset as dynamically uniform, so ACO
/// scalarises a divergent one and RADV reads the block at lane 0's index for
/// every invocation. A vertex index is as divergent as a dispatch index. The
/// ternaries below keep the offset each invocation loads a compile-time
/// constant, which is what that rule asks for, and cost nothing else.
[shader("vertex")]
VertexOutput vertexMain(uint index: SV_VertexID)
{
    uint corner;
    switch (index)
    {
    case 1:
        corner = 1;
        break;
    case 2:
    case 4:
        corner = 2;
        break;
    case 5:
        corner = 3;
        break;
    // Indices 0 and 3 are the corner both triangles share. So is anything past
    // the six vertices a draw submits, which makes a seventh vertex a
    // degenerate triangle rather than a corner this switch does not name.
    default:
        corner = 0;
        break;
    }

    bool atMaxX = corner == 1 || corner == 2;
    bool atMaxY = corner == 2 || corner == 3;

    VertexOutput output;
    output.position = float4(
        atMaxX ? constants.rect.z : constants.rect.x,
        atMaxY ? constants.rect.w : constants.rect.y,
        RECT_DEPTH,
        1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(VertexOutput input) : SV_Target
{
    return constants.color;
}
