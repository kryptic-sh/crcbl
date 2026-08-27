// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/43-render-standards.md` §8's second half: the pass that **draws**
// the gradient sky the frame already lights and reflects with.
//
// # Why the sky is a pass and not a clear colour
//
// `crcbl_shaders::sky` projects the gradient into an L1 probe for `mesh.slang`'s
// ambient term and `crcbl_render::forward` hands the raw gradient to
// `ssr.slang` for a reflection that hit nothing. Both of those are the sky seen
// *by a surface*; neither of them puts a single texel on the screen, so until
// this pass existed the background was `crcbl_render::SCENE_CLEAR` — one flat
// colour that has nothing to do with the sky lighting the scene in front of it.
//
// # It draws where the frame's depth is still the far plane, and the hardware
// is what decides that
//
// The triangle is emitted at `SV_Position.z = 0.0`, which is
// `crcbl_hal::depth::CLEAR` — the reversed-Z far plane — and the pipeline tests
// `GreaterOrEqual` with depth writes off
// (`crcbl_hal::DepthStencilState::equal_depth_read_only`). A pixel the forward
// pass covered holds a depth strictly greater than zero and fails the test; a
// pixel nothing covered holds exactly zero and passes. So there is no depth
// texture in this pass, no sampler, no `discard` and no comparison for four
// rasterisers to disagree about: the same silicon that rejected the hidden
// fragments rejects these.
//
// That is also why the colour attachment is *loaded* rather than cleared. This
// pass writes the background alone and must leave every shaded pixel exactly as
// the forward pass left it.
//
// # No transcendental reaches the colour
//
// `docs/plan/44-lighting.md`'s rule, and this file needs no escape from it: the
// gradient is a smoothstep, which is a cubic — multiplies and adds, IEEE-exact
// on every target. The one place floats are not bit-identical across backends is
// the ray, which comes out of two matrix products; the gradient is continuous in
// the ray, so a last-place difference there moves the colour by a last place too
// rather than flipping a decision.

/// The uniform block this pass reads. `crcbl_shaders::sky::SkyParams` is the
/// Rust mirror and `crcbl_render::sky_pass` is what writes it.
///
/// **Its own block rather than `ssr.slang`'s or `mesh.slang`'s**, on
/// `SsrParams`' reason exactly: a pass that draws whether or not the reflection
/// march ran cannot read the block that march fills in.
struct SkyParams
{
    /// Clip → view. Reversed-Z and possibly infinite, so `w` carries the depth
    /// and the divide below is not optional.
    float4x4 inv_proj;
    /// View → world, which turns the view-space ray into the direction the
    /// gradient is a function of.
    float4x4 inv_view;
    /// The gradient: zenith, horizon and ground, each a linear RGB radiance in
    /// `xyz` with `w` unread.
    ///
    /// The same three rows `SsrParams::sky` carries and in the same order —
    /// both passes want the radiance along one direction, so both take the
    /// gradient rather than the L1 projection `mesh.slang` is given.
    float4 sky[3];
};

[[vk::binding(0, 0)]]
ConstantBuffer<SkyParams> camera;

/// The reversed-Z **near** plane, which is where the ray is unprojected from.
static const float NDC_NEAR = 1.0;

/// A second depth to unproject at, anywhere strictly between the planes.
///
/// The ray is the difference of two unprojected points rather than the
/// normalised first one, and that is what makes this pass correct under an
/// **orthographic** camera as well: there the eye is not a point every ray
/// passes through, so `normalize(view_position)` would bend a parallel
/// projection's rays towards the view-space origin. A difference of two points
/// on the same ray is the direction under either projection.
static const float NDC_MID = 0.5;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// `crcbl_shaders::sky::SkyGradient::radiance`, and the second place in the
/// shader tree that spells it — `ssr.slang`'s `sky_radiance` is the first.
/// `crcbl_shaders::sky`'s `the_shader_spells_the_same_gradient` is what keeps
/// all three from drifting, since Slang has no `#include` for them to share.
float3 sky_radiance(float3 direction)
{
    float up = clamp(direction.y, -1.0, 1.0);
    float3 far = up >= 0.0 ? camera.sky[0].rgb : camera.sky[2].rgb;
    float u = abs(up);
    float blend = u * u * (3.0 - 2.0 * u);
    return camera.sky[1].rgb * (1.0 - blend) + far * blend;
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // `tonemap.slang`'s triangle exactly: index 0 → uv (0,0), 1 → (2,0),
    // 2 → (0,2), oversized so one primitive covers the viewport. The `0.0` in
    // `z` is the far plane the depth test above is written against, and is the
    // same zero every other full-screen pass in this tree emits.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    // The interpolated `uv` and not `SV_Position`: this is the inverse of the
    // vertex stage's own line, so it needs no extent and has no texel-centre
    // arithmetic in it. `+Y` is up in this engine's clip space on every target
    // — `crcbl-vk` submits a negative-height viewport so Vulkan agrees with the
    // other three — while `uv` counts down the framebuffer, hence the
    // subtraction on `y` and not on `x`.
    float2 ndc = float2(input.uv.x * 2.0 - 1.0, 1.0 - input.uv.y * 2.0);

    float4 near_plane = mul(camera.inv_proj, float4(ndc, NDC_NEAR, 1.0));
    float4 beyond = mul(camera.inv_proj, float4(ndc, NDC_MID, 1.0));
    float3 along = beyond.xyz / beyond.w - near_plane.xyz / near_plane.w;
    float3 direction = normalize(mul(camera.inv_view, float4(along, 0.0)).xyz);

    // Opaque: this is the furthest thing in the frame and nothing is behind it.
    return float4(sky_radiance(direction), 1.0);
}
