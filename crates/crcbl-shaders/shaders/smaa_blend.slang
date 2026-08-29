// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/49-antialiasing.md`'s SMAA 1x, third of three passes: the
// neighbourhood blend. `smaa_edges.slang`'s header describes the tier and what
// each pass hands the next.
//
// Every pixel gathers the four blend weights that point *at* it — its own two,
// its right neighbour's and its top neighbour's — picks the axis whose pair is
// stronger, and mixes itself with the one neighbour across that axis. The mix
// is a single bilinear tap placed off-centre by the weight rather than two taps
// and a lerp, which is the reference's own trick and the reason this pass takes
// a **linear** sampler.
//
// # Where it sits, and why that is after the tonemap and after the grid
//
// This is the pass that writes the caller's target, so it takes the resolve
// slot `crcbl_render::fxaa` takes when the cheap tier is the one running. That
// module's header carries the ordering argument in full and
// `crcbl_render::forward` carries it at the call site: the frame is
// `scene → bloom → tonemap → grid → resolve → UI`, the grid is in front of the
// filter because a field of thin high-contrast lines is what an edge filter is
// for, and the UI is behind it because topic 18 refuses to antialias text.
//
// # The mix is in the space the frame is already in
//
// The reference blends gamma-space colour, because that is what its pipeline
// hands it. What reaches here is linear — `tonemap.slang` writes linear values
// into an sRGB-format target and the hardware encodes — so this mixes linear
// values and writes linear values, and the encode happens once, at the end,
// where it always did. The place the difference between the two spaces actually
// matters is the *threshold* that decides what is an edge, and that is
// `smaa_edges.slang`'s `luma_of`, which corrects for it.
//
// # No `#include`
//
// `bloom_composite.slang`'s rule; `smaa_edges.slang` repeats it. The parameter
// block and the fullscreen triangle below are copies.

/// Everything the three SMAA passes take from the CPU — `smaa_edges.slang`
/// declares the same block and its header says why there is one.
struct SmaaParams
{
    /// One over the extent of the image being read — `SMAA_RT_METRICS.xy`.
    float2 inv_source;
    /// That extent in texels — `SMAA_RT_METRICS.zw`. Unread here, and present
    /// because one block serves the three passes.
    float2 source_size;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

/// The tonemapped frame, at this pass's own extent — the same image
/// `smaa_edges.slang` read.
[[vk::binding(0, 0)]]
Texture2D<float4> source;

/// The blend weights `smaa_weights.slang` wrote.
[[vk::binding(1, 0)]]
Texture2D<float4> blend;

/// A **linear**, clamp-to-edge sampler. The whole blend is one bilinear tap
/// placed off-centre by a weight, so nearest filtering would round every mix
/// back to one of its two ends and this pass would be a blit.
[[vk::binding(2, 0)]]
SamplerState sourceSampler;

[[vk::binding(3, 0)]]
ConstantBuffer<SmaaParams> params;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// Below this the four weights are nothing and the pixel is passed through
/// unchanged — the reference's own floor.
static const float BLEND_EPSILON = 1e-5;

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    float2 uv = input.uv;
    float2 texel = params.inv_source;
    float4 offset = float4(1.0, 0.0, 0.0, 1.0) * float4(texel, texel) + float4(uv, uv);

    // The four weights that point at this pixel: right, top, bottom, left — the
    // reference's `a`, whose lanes come from three different fetches because a
    // neighbour's weight lives in the neighbour's texel.
    float4 a;
    a.x = blend.SampleLevel(sourceSampler, offset.xy, 0.0).a;
    a.y = blend.SampleLevel(sourceSampler, offset.zw, 0.0).g;
    float4 own = blend.SampleLevel(sourceSampler, uv, 0.0);
    a.w = own.x;
    a.z = own.z;

    // Alpha 1.0 on every path out of this pass: the swapchain is composited
    // opaque, exactly as `tonemap.slang` says.
    if (dot(a, float4(1.0, 1.0, 1.0, 1.0)) < BLEND_EPSILON)
    {
        return float4(source.SampleLevel(sourceSampler, uv, 0.0).rgb, 1.0);
    }

    // Whichever axis carries the stronger pair is the one this pixel is moved
    // along.
    bool horizontal = max(a.x, a.z) > max(a.y, a.w);
    float4 blending_offset = float4(0.0, a.y, 0.0, a.w);
    float2 blending_weight = float2(a.y, a.w);
    if (horizontal)
    {
        blending_offset = float4(a.x, 0.0, a.z, 0.0);
        blending_weight = float2(a.x, a.z);
    }
    blending_weight /= dot(blending_weight, float2(1.0, 1.0));

    // One tap either side, each displaced by its own weight, so the hardware's
    // bilinear unit does the mix between this pixel and its neighbour.
    float4 blending_coord =
        blending_offset * float4(texel, -texel) + float4(uv, uv);
    float3 color = blending_weight.x * source.SampleLevel(sourceSampler, blending_coord.xy, 0.0).rgb;
    color += blending_weight.y * source.SampleLevel(sourceSampler, blending_coord.zw, 0.0).rgb;
    return float4(color, 1.0);
}
