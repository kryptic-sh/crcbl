// HDR from P1: the pass that maps the `Rgba16Float` scene target into the
// swapchain's format.
//
// `docs/plan/ROADMAP.md`'s correction is the whole reason this exists at P1
// rather than P7: "render to `RGBA16F` + trivial tonemap from the first lit mesh
// so breakout/asteroids goldens and web demos aren't re-blessed wholesale when
// 18's stack lands at P7". What P1 owes is the *shape* — a floating-point scene
// target and a pass that resolves it — not the operator. `topic 18`'s real stack
// replaces exactly one function in this file.
//
// # The operator is exposure-and-clamp, on purpose
//
// A Reinhard curve would be just as short and is what "tonemap" usually means,
// and it was rejected: `c / (1 + c)` maps 0.5 to 0.333, so every LDR colour in
// the engine would shift the day this landed and shift again the day topic 18
// replaced it. Exposure-and-clamp is the identity on `[0, 1]`, so content that
// is already display-referred — which is all of it at P1 — reaches the swapchain
// unchanged, and the only pixels this pass moves are the ones a `Rgba8` target
// could not have held in the first place.
//
// It is still a tonemap and not a no-op: the lit pass's Blinn highlight
// genuinely exceeds 1.0, and this is where that stops being representable. The
// `crcbl-vk` e2e suite reads the HDR target back and asserts a value above 1.0
// is in it, which is what makes "the RGBA16F path is real" a test result rather
// than a claim about a format enum.
//
// # No vertex buffer, and no full-screen quad either
//
// The classic full-screen *triangle*: three vertices generated from
// `SV_VertexID` alone, oversized so a single primitive covers the viewport. Two
// triangles would rasterise a seam down the diagonal where the two halves meet,
// and would need geometry — which `crcbl-hal` has no way to bind anyway.

[[vk::binding(0, 0)]]
Texture2D<float4> scene;

[[vk::binding(1, 0)]]
SamplerState sceneSampler;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// Exposure multiplier applied before the clamp.
///
/// `1.0` at P1: nothing in the engine produces a scene whose average luminance
/// needs correcting yet, and a value other than 1 would make this pass move
/// pixels for a reason no golden image could explain. Auto-exposure is topic 18.
static const float EXPOSURE = 1.0;

/// The operator. **Replace this function, and only this function, at P7.**
float3 tonemap(float3 color)
{
    return saturate(color * EXPOSURE);
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // index 0 → uv (0,0), index 1 → (2,0), index 2 → (0,2).
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    // uv (0,0) must land at the **top left**, because that is where texel (0,0)
    // of the scene target is: `crcbl-vk` submits a negative-height viewport so
    // the seam's +Y-up convention survives Vulkan's inverted clip space, and the
    // scene was rendered through that same flip. Hence the `-2.0` on Y.
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    // A **nearest** sampler, and the renderer creates it that way. This pass is
    // a 1:1 blit, so linear filtering would sample the same texel anyway on a
    // correct implementation — and would leave a golden image depending on two
    // rasterisers agreeing about texel-centre arithmetic, which is exactly the
    // class of difference `crcbl-golden`'s tolerance was measured against and
    // should not have to absorb.
    float3 color = scene.Sample(sceneSampler, input.uv).rgb;
    // Alpha 1.0 rather than the scene's: the swapchain is composited opaque
    // (`CompositeAlpha::Opaque`), and carrying a scene alpha into it would make
    // the window's transparency depend on the geometry that happened to be
    // drawn.
    return float4(tonemap(color), 1.0);
}
