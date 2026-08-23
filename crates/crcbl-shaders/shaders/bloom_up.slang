// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s bloom chain, upsampling half: the 3×3 tent
// filter that carries one mip of the chain onto the next larger one, **added**
// rather than written.
//
// # Progressive upsample-and-add, which is what the chain is for
//
// `bloom_down.slang` produced a pyramid where each level holds the scene at a
// coarser scale. This pass walks it back down — smallest level first — adding
// each level into the one above it, so by the time the chain reaches mip 1 that
// image holds the sum of every level. The result is a halo whose falloff spans
// several octaves rather than the single Gaussian a one-scale blur would give,
// which is the whole reason a bloom is a chain and not a blur.
//
// The add is a **blend**, not a load-modify-store: the pipeline is created with
// `One`/`One` on colour, so the destination keeps what the downsample wrote and
// this pass contributes on top. An attachment that a fragment shader both read
// and wrote would be undefined on every backend here.
//
// # A tent, not a Gaussian
//
// Nine taps one *source* texel apart, weighted `1 2 1 / 2 4 2 / 1 2 1` over
// sixteen. Three reasons it is the right filter for this step and a Gaussian is
// not:
//
// * A tent is the exact reconstruction filter for a 2:1 bilinear magnification,
//   so upsampling a level and adding it introduces no ringing and no blocking of
//   its own.
// * It is separable in effect but cheap unseparated at nine taps, where a
//   Gaussian wide enough to matter needs a second pass per level.
// * The chain's *smoothness* comes from summing octaves, not from any one
//   level's kernel — so spending taps on a better kernel per level buys much
//   less than another level does.

/// Everything the pass takes from the CPU — `bloom_down.slang`'s block, member
/// for member.
///
/// Declared a second time rather than included, on this repo's terms: the
/// manifest hashes one source per artifact, so an `#include` would be a file
/// whose edits nothing downstream notices. `crcbl_shaders::bloom::BloomParams`
/// is the Rust mirror all three bloom sources agree with.
struct BloomParams
{
    /// One over the extent of the image being **read** — here the smaller mip
    /// this pass is spreading over the larger one.
    float2 inv_source;
    /// `bloom_down.slang`'s Karis switch. Nothing here reads it; the block's
    /// layout is the block's layout.
    float karis;
    /// The composite's scalar. Nothing here reads it either.
    float strength;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.
[[vk::binding(0, 0)]]
Texture2D<float4> source;

[[vk::binding(1, 0)]]
SamplerState sourceSampler;

[[vk::binding(2, 0)]]
ConstantBuffer<BloomParams> params;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// One bilinear tap, `offset` whole source texels from `uv`.
float3 tap(float2 uv, float2 offset)
{
    return source.Sample(sourceSampler, uv + offset * params.inv_source).rgb;
}

/// The 3×3 tent, `1 2 1 / 2 4 2 / 1 2 1` over sixteen, one source texel apart.
///
/// Copied verbatim into `bloom_composite.slang`, which does this same
/// reconstruction on mip 1 as part of the pass that adds the chain to the scene
/// — `crcbl_shaders::bloom` is where the two copies are held together.
float3 tent(float2 uv)
{
    float3 sum = tap(uv, float2(-1.0, 1.0)) * 1.0;
    sum += tap(uv, float2(0.0, 1.0)) * 2.0;
    sum += tap(uv, float2(1.0, 1.0)) * 1.0;
    sum += tap(uv, float2(-1.0, 0.0)) * 2.0;
    sum += tap(uv, float2(0.0, 0.0)) * 4.0;
    sum += tap(uv, float2(1.0, 0.0)) * 2.0;
    sum += tap(uv, float2(-1.0, -1.0)) * 1.0;
    sum += tap(uv, float2(0.0, -1.0)) * 2.0;
    sum += tap(uv, float2(1.0, -1.0)) * 1.0;
    return sum * (1.0 / 16.0);
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // index 0 → uv (0,0), index 1 → (2,0), index 2 → (0,2). `tonemap.slang`
    // carries the *why* of this shape.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    // Alpha zero, and the pipeline's alpha blend is `Zero`/`One`: the
    // destination's alpha is left exactly as the downsample wrote it. Nothing
    // reads a chain mip's alpha, and an accumulating one would be a channel
    // whose value depends on how many levels the extent happened to allow.
    return float4(tent(input.uv), 0.0);
}
