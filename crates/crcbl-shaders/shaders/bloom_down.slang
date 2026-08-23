// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s bloom chain, downsampling half: one step
// of the thirteen-tap filter Jorge Jimenez presented as *Next Generation Post
// Processing in Call of Duty: Advanced Warfare* (SIGGRAPH 2014), reading one mip
// of the chain and writing the next.
//
// # Thirteen taps, five overlapping four-texel groups
//
// A box filter over a 2:1 reduction takes four texels and aliases everything
// between them; a wider box blurs the mip into mush. The thirteen taps are the
// four texels of the destination's own footprint (`j k l m`, at ±1 source
// texel) plus a 3×3 grid at ±2, and they are combined as **five overlapping
// four-texel groups** rather than thirteen weighted samples:
//
//     a . b . c        group 0 = a b d e     group 4 = j k l m
//     . j . k .        group 1 = b c e f
//     d . e . f        group 2 = d e g h
//     . l . m .        group 3 = e f h i
//     g . h . i
//
// The inner group carries half the weight and the four outer ones an eighth
// each. That is the talk's own split, and the property it buys is that every
// tap is a *bilinear* fetch between texels — the filter reads thirteen times and
// covers a 5×5 neighbourhood, where a separable Gaussian of the same reach costs
// more and still lets a single bright texel through.
//
// # The Karis average is what makes the chain threshold-free
//
// A classic bloom brightness-thresholds the scene before blurring, which is the
// step that puts a hard edge in the picture whenever a surface crosses the
// threshold. This chain has no threshold: everything blooms, in proportion to
// how bright it is. What that costs, and what the threshold was covering up, is
// **fireflies** — a single sub-pixel highlight of enormous intensity that the
// first reduction smears into a large, flickering, several-pixel blob.
//
// The fix is Brian Karis's weighted average, applied per four-texel group: each
// group is weighted by `1 / (1 + luma(group))`, so a group holding a firefly
// counts for almost nothing beside its neighbours, and the sum is renormalised
// by the weights it actually used. A firefly therefore contributes its *share*
// of a group rather than its absolute intensity, and a bright *area* — where
// every group is bright — is unaffected, because the weights come out of the
// normalisation together.
//
// It is applied on the **first** step alone, which is the "partial" in partial
// Karis average: after that reduction every texel is already an average of
// sixteen source texels, so there is no sub-pixel outlier left to catch and
// re-weighting later mips would only cost energy.
//
// # `karis` is a factor, not a branch
//
// The switch is a `float` multiplying the luma term rather than an `if`, and
// that is deliberate rather than cute. At `karis = 0` every weight is exactly
// its base — `0.125` four times and `0.5` — and the denominator sums to exactly
// `1.0` in binary floating point, so the plain filter is recovered *bit for
// bit* rather than approximately. One pipeline, one artifact per target, and no
// branch for four rasterisers to agree about.

/// Everything the pass takes from the CPU.
///
/// `crcbl_shaders::bloom::BloomParams` is the Rust mirror and
/// `crcbl_render::bloom` is what writes the buffer — one row per step of the
/// chain, per frame in flight.
///
/// Declared a second time in `bloom_up.slang` and a third in
/// `bloom_composite.slang` rather than included, on this repo's terms: the
/// manifest hashes one source per artifact, so an `#include` would be a file
/// whose edits nothing downstream notices — `ssao_blur.slang` carries the same
/// paragraph.
struct BloomParams
{
    /// One over the extent of the image being **read**, in texels.
    ///
    /// Every offset below is a whole number of *source* texels scaled by this,
    /// so a chain whose extents do not halve exactly — an odd extent floors on
    /// the way down, which `crcbl_render::bloom` states — changes the filter's
    /// footprint by the rounding fraction and nothing else. The alternative,
    /// offsets in destination texels, would double that error at every step.
    float2 inv_source;
    /// `1.0` on the first step of the chain and `0.0` on every later one — see
    /// the Karis paragraphs above.
    float karis;
    /// The composite's scalar. Nothing here reads it; the block's layout is the
    /// block's layout.
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

/// Rec. 709 relative luminance of a **linear** colour.
///
/// The scene target is `Rgba16Float` in linear light all the way to the tonemap,
/// so this is the luminance of the value as it is stored. `bloom_composite.slang`
/// does not need it and does not declare it.
float luma(float3 color)
{
    return dot(color, float3(0.2126, 0.7152, 0.0722));
}

/// One bilinear tap, `offset` whole source texels from `uv`.
float3 tap(float2 uv, float2 offset)
{
    return source.Sample(sourceSampler, uv + offset * params.inv_source).rgb;
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // index 0 → uv (0,0), index 1 → (2,0), index 2 → (0,2). `tonemap.slang`
    // carries the *why* of this shape; every full-screen pass in the engine
    // repeats it because `crcbl-hal` can bind no geometry at all.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    // uv (0,0) at the top left, for `tonemap.slang`'s reason: `crcbl-vk` submits
    // a negative-height viewport, and the chain is sampled in the same space the
    // scene was rendered in.
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    float2 uv = input.uv;

    float3 a = tap(uv, float2(-2.0, 2.0));
    float3 b = tap(uv, float2(0.0, 2.0));
    float3 c = tap(uv, float2(2.0, 2.0));
    float3 d = tap(uv, float2(-2.0, 0.0));
    float3 e = tap(uv, float2(0.0, 0.0));
    float3 f = tap(uv, float2(2.0, 0.0));
    float3 g = tap(uv, float2(-2.0, -2.0));
    float3 h = tap(uv, float2(0.0, -2.0));
    float3 i = tap(uv, float2(2.0, -2.0));
    float3 j = tap(uv, float2(-1.0, 1.0));
    float3 k = tap(uv, float2(1.0, 1.0));
    float3 l = tap(uv, float2(-1.0, -1.0));
    float3 m = tap(uv, float2(1.0, -1.0));

    // The five groups, each the mean of its four taps. The centre tap `e` is in
    // four of them, which is what makes the outer groups overlap and the filter
    // smooth rather than blocky.
    float3 g0 = (a + b + d + e) * 0.25;
    float3 g1 = (b + c + e + f) * 0.25;
    float3 g2 = (d + e + g + h) * 0.25;
    float3 g3 = (e + f + h + i) * 0.25;
    float3 g4 = (j + k + l + m) * 0.25;

    // The talk's weights, divided by the Karis term. At `karis = 0` these are
    // exactly `0.125, 0.125, 0.125, 0.125, 0.5` and the sum below is exactly
    // `1.0`.
    float w0 = 0.125 / (1.0 + params.karis * luma(g0));
    float w1 = 0.125 / (1.0 + params.karis * luma(g1));
    float w2 = 0.125 / (1.0 + params.karis * luma(g2));
    float w3 = 0.125 / (1.0 + params.karis * luma(g3));
    float w4 = 0.5 / (1.0 + params.karis * luma(g4));

    float3 sum = g0 * w0 + g1 * w1 + g2 * w2 + g3 * w3 + g4 * w4;
    float weight = w0 + w1 + w2 + w3 + w4;

    // Alpha 1.0: nothing in the chain reads it, and the mips are `Rgba16Float`
    // because the scene target is — a bloom that clipped at 1.0 before the
    // tonemap saw it would be a bloom of a picture the frame does not contain.
    return float4(sum / weight, 1.0);
}
