// crcbl-targets: spirv, wgsl, msl, dxil

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
// # The operator is exposure-and-clamp by default, and a curve on request
//
// Since 2026-08-27 the block carries a selector and this file carries two
// operators. What follows is the argument for the *default* being the clamp,
// which is unchanged and is why nothing in the tree re-blessed when the curve
// landed.
//
// A Reinhard curve would be just as short and is what "tonemap" usually means,
// and it was rejected: `c / (1 + c)` maps 0.5 to 0.333, so every LDR colour in
// the engine would shift the day this landed and shift again the day topic 18
// replaced it. Exposure-and-clamp is the identity on `[0, 1]`, so content that
// is already display-referred — which is all of it at P1 — reaches the swapchain
// unchanged, and the only pixels this pass moves are the ones a `Rgba8` target
// could not have held in the first place.
//
// That argument is about the *operator* and survives the exposure becoming a
// runtime value: at the default it is still the identity on `[0, 1]`, and a
// caller who moves it has asked for the picture to change. What it rules out is
// a curve that would shift every display-referred colour whatever the exposure.
//
// It is still a tonemap and not a no-op: the lit pass's specular highlight
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

/// Everything the pass takes from the CPU, which is one number.
///
/// `crcbl_shaders::tonemap::TonemapParams` is the Rust mirror and
/// `crcbl_render::forward` is what writes the buffer, once per frame.
struct TonemapParams
{
    /// The multiplier applied to the scene colour before the clamp.
    ///
    /// A **runtime** value rather than the compile-time constant this was until
    /// 2026-08. The operator is unchanged and so is its default —
    /// `crcbl_shaders::tonemap::DEFAULT_EXPOSURE` is 1.0, which is the value the
    /// constant held and the reason every golden image is untouched — but the
    /// number now arrives per frame, so an application can put it on a key or a
    /// slider without a second pipeline. Auto-exposure, which would compute it
    /// from the scene rather than take it from a caller, is still topic 18.
    float exposure;
    /// Which operator to run: [`CURVE_CLAMP`] or [`CURVE_ACES`].
    ///
    /// A `uint` selector rather than two pipelines, because the two operators
    /// differ by a handful of arithmetic instructions in a pass that is already
    /// one triangle over one texture — a second pipeline would cost a state
    /// object, a bind-group layout and a branch on the host to pick between
    /// them, to save a branch the whole warp takes the same way.
    ///
    /// `crcbl_shaders::tonemap::TonemapCurve` is the Rust side, and
    /// [`CURVE_CLAMP`] is what a renderer nobody has configured writes.
    uint curve;
    /// Non-zero to take the exposure from `measured[0]` instead of from
    /// [`TonemapParams::exposure`].
    ///
    /// `docs/plan/43-render-standards.md` §6's auto-exposure, and the switch is
    /// here rather than in a second pipeline for [`TonemapParams::curve`]'s
    /// reason exactly. Zero is what a frame that added no exposure passes
    /// writes, and on that frame this pass reads the number the host set and is
    /// the pass that ran before `exposure.slang` existed — which is what keeps
    /// every golden image in the tree untouched.
    uint auto_exposure;
};

/// [`TonemapParams::curve`]: exposure and clamp, the operator this pass shipped
/// with and the identity on `[0, 1]`.
static const uint CURVE_CLAMP = 0u;

/// [`TonemapParams::curve`]: the ACES filmic curve, fitted.
static const uint CURVE_ACES = 1u;

// The ACES fit, and why this one.
//
// Stephen Hill's fit of the ACES RRT and ODT: two 3x3 changes of primaries
// around a rational polynomial. Every operation in it is a multiply, an add or
// a divide.
//
// **That is the reason it is this curve and not AgX**, which is otherwise the
// newer answer and the one Blender and Filament moved to. AgX takes a `log2`
// and a `pow` per channel, and this workspace's whole determinism argument —
// the one `mesh.slang`'s `ggx_lobe` is held to and the one every cross-backend
// golden rests on — is that a shader uses no transcendental functions, because
// four platforms' implementations of them differ in the last place. A curve
// that cannot be blessed across the backends is not a curve this engine can
// ship, however good it looks on one of them.
//
// The constants are the published fit and are mirrored in
// `crcbl_shaders::tonemap`, which pins them against the specification and
// checks that this file spells the same numbers.

/// ACEScc-relative primaries in, for the fit's polynomial.
static const float3x3 ACES_INPUT = float3x3(
    0.59719, 0.35458, 0.04823,
    0.07600, 0.90834, 0.01566,
    0.02840, 0.13383, 0.83777);

/// And back out to display primaries afterwards.
static const float3x3 ACES_OUTPUT = float3x3(
    1.60475, -0.53108, -0.07367,
    -0.10208, 1.10813, -0.00605,
    -0.00327, -0.07276, 1.07602);

/// The rational polynomial standing in for the RRT and the ODT together.
float3 rrt_and_odt_fit(float3 v)
{
    float3 a = v * (v + 0.0245786) - 0.000090537;
    float3 b = v * (0.983729 * v + 0.432951) + 0.238081;
    return a / b;
}

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source. The
// block is numbered last and therefore declared last: it is the buffer table's
// only entry either way, and the rule is checked globally rather than per table.
[[vk::binding(0, 0)]]
Texture2D<float4> scene;

[[vk::binding(1, 0)]]
SamplerState sceneSampler;

[[vk::binding(2, 0)]]
ConstantBuffer<TonemapParams> params;

/// `[0]`: the exposure `exposure.slang`'s `reduceMain` measured from this
/// frame, read only when [`TonemapParams::auto_exposure`] says to.
///
/// **Always bound, on every frame, whether or not it holds anything.** A
/// binding that comes and goes is a second pipeline layout and a second bind
/// group cache keyed on which one is live, and the value it would save is four
/// bytes; `crcbl_render::exposure` creates the buffer with the renderer and
/// writes it only on a frame that measures.
[[vk::binding(3, 0)]]
StructuredBuffer<float> measured;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The operator, selected per frame.
///
/// **The clamp is still the default and still the identity on `[0, 1]`**, which
/// is what keeps display-referred content — every 2D sample in the tree — going
/// to the swapchain unchanged. A view that wants the filmic roll-off asks for
/// it by name; see `crcbl_render::ForwardRenderer::set_tonemap_curve`.
float3 tonemap(float3 color, float exposure, uint curve)
{
    float3 exposed = color * exposure;
    if (curve == CURVE_ACES)
    {
        float3 aces = rrt_and_odt_fit(mul(ACES_INPUT, exposed));
        return saturate(mul(ACES_OUTPUT, aces));
    }
    return saturate(exposed);
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
    // The measured exposure or the caller's, decided per frame by the block.
    // Reading the buffer unconditionally would be the same picture and a
    // dependency on a value no pass wrote on a frame that does not measure.
    float exposure = params.auto_exposure != 0u ? measured[0] : params.exposure;
    return float4(tonemap(color, exposure, params.curve), 1.0);
}
