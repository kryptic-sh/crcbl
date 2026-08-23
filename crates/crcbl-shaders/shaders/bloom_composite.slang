// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s bloom chain, last pass: the final tent
// upsample of mip 1 and the add that puts the whole chain back into the scene.
//
// # It is the last upsample *and* the composite, in one pass
//
// The alternative is an upsample into a full-resolution image followed by a
// pass that adds that image to the scene, and it costs a frame-sized
// `Rgba16Float` transient and a full-resolution write of it for nothing: the
// tent is nine taps of a half-resolution image either way, and the add is one
// more term in the expression. So the chain ends here, mip 1 is the last level
// anything reads, and the scene target is never written — the sum goes into an
// image of its own, which is what lets the tonemap read either it or the
// un-bloomed scene with no other difference in the frame.
//
// # `scene + bloom * strength`, and not `lerp(scene, bloom, strength)`
//
// Topic 18 says "additive with scalar" and this is that, but the choice is
// worth stating because the two look identical at a glance and differ where it
// matters. A `lerp` **removes** scene energy in proportion to how much bloom it
// adds, so a frame with a bright emitter in it gets a halo *and* a dimmer
// emitter — which is backwards: a lens scatters light from the highlight into
// its surroundings without taking anything away from what the sensor already
// recorded there. Additive is also the composition the chain's own arithmetic
// assumes: every level of the pyramid was summed, not blended, on the way up.
//
// What additive costs is that the total is not energy-preserving — the frame
// gets brighter overall — which is why `strength` is small and why the tonemap
// downstream is the thing that decides what a bright pixel looks like.
//
// # The scene is fetched, the chain is sampled
//
// The scene target is the same extent as this pass's own, so reading it is a
// 1:1 blit and it is read with a `Load` at the integer pixel — no sampler, no
// filtering, and none of the texel-centre arithmetic `tonemap.slang` refuses to
// leave a golden image depending on four rasterisers agreeing about. The chain
// is a genuine magnification and is sampled with the linear sampler
// `crcbl_render::bloom` creates for exactly this.

/// Everything the pass takes from the CPU — `bloom_down.slang`'s block, member
/// for member.
///
/// Declared a third time rather than included, on this repo's terms: the
/// manifest hashes one source per artifact, so an `#include` would be a file
/// whose edits nothing downstream notices. `crcbl_shaders::bloom::BloomParams`
/// is the Rust mirror all three bloom sources agree with.
struct BloomParams
{
    /// One over the extent of the image being **read** — here mip 1 of the
    /// chain, which is half the scene's extent rounded down. The scene target is
    /// fetched by integer pixel and needs no texel size of its own.
    float2 inv_source;
    /// `bloom_down.slang`'s Karis switch. Nothing here reads it; the block's
    /// layout is the block's layout.
    float karis;
    /// How much of the chain reaches the frame.
    /// `crcbl_shaders::bloom::DEFAULT_STRENGTH` is what a renderer nobody has
    /// configured writes here.
    float strength;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

/// The scene colour this pass adds the chain to: the forward pass's target, or
/// the reflection composite's where that pair ran.
///
/// `Texture2D<float4>` read by `Load`, so there is no sampler beside it — see
/// the header.
[[vk::binding(0, 0)]]
Texture2D<float4> scene;

/// Mip 1 of the chain, holding the sum of every level `bloom_up.slang` walked
/// back down.
///
/// **Named `source` and not `bloom`**, because `tap` and `tent` below are copied
/// verbatim out of that shader and the guard that holds the copies together
/// compares their bodies as text — see `crcbl_shaders::bloom`.
[[vk::binding(1, 0)]]
Texture2D<float4> source;

[[vk::binding(2, 0)]]
SamplerState sourceSampler;

[[vk::binding(3, 0)]]
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
/// Copied verbatim out of `bloom_up.slang`, whose header says what the filter is
/// and why it is a tent — `crcbl_shaders::bloom` is where the two copies are
/// held together.
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
    int2 pixel = int2(input.position.xy);
    float3 color = scene.Load(int3(pixel, 0)).rgb;
    // Alpha 1.0: this image stands in for the scene target from here on, and the
    // tonemap that reads it writes 1.0 into the swapchain whatever is here.
    return float4(color + tent(input.uv) * params.strength, 1.0);
}
