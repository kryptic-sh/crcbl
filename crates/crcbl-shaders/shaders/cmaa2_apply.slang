// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/49-antialiasing.md`'s CMAA2 tier, last of its three passes: the
// deferred colour apply. `cmaa2_edges.slang`'s header describes the tier and
// what each pass hands the next.
//
// Every pixel reads the fixed-point total `cmaa2_shapes.slang` left for it,
// converts it **once**, and mixes it with its own colour. A pixel no run
// reached has a total of zero and is written through unchanged, which is most
// of the frame.
//
// # This one is a fullscreen draw where the two before it are dispatches
//
// It is the pass that writes the caller's target, and a caller's target is a
// swapchain image. **A swapchain image cannot be a storage image**: Vulkan has
// no storage-image view of an sRGB format at all, and WebGPU's storage-texture
// format list excludes every sRGB encoding for the same reason — the write
// would have to skip the encode the format exists to apply. So the two compute
// stages hand their result to a fullscreen triangle, which writes the target
// the way every other resolve in this engine does.
//
// The accumulation reaches it as a **read-only** storage buffer in the fragment
// stage, which is `ssr.slang`'s and `volumetric_composite.slang`'s shape — see
// `crcbl_render::ssr`, whose light list arrives the same way.
//
// # The mix is in the space the frame is already in
//
// `tonemap.slang` writes linear values into a target the hardware encodes, so
// what a fetch here decodes to is linear and what this writes is encoded once,
// at the end, where it always was. The place the two spaces differ is the
// *threshold* that decides what is an edge, and that is `cmaa2_edges.slang`'s
// `luma_of`.
//
// # No `#include`
//
// `bloom_composite.slang`'s rule; `cmaa2_edges.slang` repeats it. The parameter
// block and the fullscreen triangle below are copies.

// D3D12 takes each resource's register from its binding. A declaration's
// `D3D12_REGISTER` suffix is `register(<class><binding>, space<set>)`, the
// numbers its `[[vk::binding(binding, set)]]` names, and
// `crcbl_shaders::declaration_order` checks that the two agree. HLSL only,
// because Slang's Metal target would take its argument-table indices from a
// `register` as well.
#if defined(CRCBL_TARGET_HLSL)
#define D3D12_REGISTER(...) : register(__VA_ARGS__)
#else
#define D3D12_REGISTER(...)
#endif

/// Everything the CMAA2 passes take from the CPU — `cmaa2_edges.slang` declares
/// the same block and its header says why there is one.
struct Cmaa2Params
{
    /// Width of the image being filtered, in texels. The accumulation is
    /// indexed `y * viewport_x + x`, so this pass needs it even though it draws
    /// at that extent.
    uint viewport_x;
    /// Its height.
    uint viewport_y;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`. This file has a layout of its own: the
// two compute passes bind a writable edge buffer this stage has no use for, and
// a writable storage buffer is not something a vertex stage may be handed at
// all.

/// The tonemapped frame, read by `Load` for this pixel's own texel — the resolve
/// is 1:1 and a filtered tap would mix in a neighbour nothing asked for.
[[vk::binding(0, 0)]]
Texture2D<float4> source D3D12_REGISTER(t0, space0);

/// `[pixel * ACCUM_WORDS]`: three colour words and a weight word per pixel, as
/// `cmaa2_shapes.slang` left them.
///
/// `StructuredBuffer` rather than `RWStructuredBuffer`: nothing here writes it,
/// and read-only is what lets it be bound to a fragment stage on every backend.
[[vk::binding(1, 0)]]
StructuredBuffer<uint> accum D3D12_REGISTER(t1, space0);

[[vk::binding(2, 0)]]
ConstantBuffer<Cmaa2Params> params D3D12_REGISTER(b2, space0);

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// Words per pixel in `accum`, matching `crcbl_shaders::cmaa2::ACCUM_WORDS`.
static const uint ACCUM_WORDS = 4;

/// One over `crcbl_shaders::cmaa2::BLEND_FIXED_POINT_SCALE`.
///
/// A power of two, so the conversion back is exact: every accumulated word
/// divides by it without a rounding step, and four backends that agree on
/// integer addition therefore agree on the colour it produces.
static const float INV_BLEND_FIXED_POINT_SCALE = 1.0 / 1048576.0;

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // index 0 → uv (0,0), index 1 → (2,0), index 2 → (0,2). `tonemap.slang`
    // carries the *why* of this shape and of the -2.0 on Y.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    uint2 texel = uint2(input.position.xy);
    // Clamped rather than trusted: a render area is the attachment's, but a
    // half-open pixel centre on the far edge rounds to the extent on a
    // rasteriser that places it differently, and an index one row past the
    // accumulation is a read nothing else here would catch.
    texel = min(texel, uint2(params.viewport_x - 1, params.viewport_y - 1));
    uint pixel = texel.y * params.viewport_x + texel.x;

    // Alpha 1.0 on every path out of this pass: the swapchain is composited
    // opaque, exactly as `tonemap.slang` says.
    float3 own = source.Load(int3(int2(texel), 0)).rgb;

    float weight = float(accum[pixel * ACCUM_WORDS + 3]) * INV_BLEND_FIXED_POINT_SCALE;
    if (weight <= 0.0)
    {
        return float4(own, 1.0);
    }

    float3 blended = float3(
                         float(accum[pixel * ACCUM_WORDS + 0]),
                         float(accum[pixel * ACCUM_WORDS + 1]),
                         float(accum[pixel * ACCUM_WORDS + 2]))
                     * INV_BLEND_FIXED_POINT_SCALE;

    // The accumulated colour is already premultiplied by its own weights, so
    // the pixel keeps whatever share of itself the runs left over. A total
    // above one means more coverage was attributed to this pixel than it has —
    // several runs claiming the same pixel — and the honest answer there is the
    // weighted mean of what claimed it rather than a colour scaled past its
    // range.
    if (weight >= 1.0)
    {
        return float4(blended / weight, 1.0);
    }
    return float4(own * (1.0 - weight) + blended, 1.0);
}
