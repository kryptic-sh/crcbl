// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/55-water.md` rung 1's first pass: the opaque frame's colour and
// depth, copied into two images of the water pass's own so it can read them
// while it draws into the originals.
//
// # Why a pass and not an image copy
//
// The render graph refuses to attach and sample one image in one pass, so the
// surface cannot read the frame it composites into, and needs a second image
// holding what was there. A `copy_image_to_image` would be the obvious way to
// fill it, and it is not taken, for three reasons: a depth image-to-image copy
// is exercised on no backend in this tree, and a browser is where it would first
// be tried; a copy pass carries no timer, where the water passes are priced one
// by one; and this pass is `hiz.slang`'s construction, which runs on all four
// backends already — a full-screen triangle writing its depth through
// `SV_Depth` under an always-passing compare.
//
// # Every read is a `Load`
//
// On `hiz.slang`'s terms: the destination is the source's own size, so a pixel
// fetches the texel it covers and nothing is filtered.

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

/// The HDR frame as the passes before water left it.
[[vk::binding(0, 0)]]
Texture2D<float4> source_color D3D12_REGISTER(t0, space0);

/// The opaque frame's depth. `DepthTexture2D` for the reason `hiz.slang` gives
/// under the same declaration: it is the spelling that lowers to a
/// `texture_depth_2d` a `D32Float` view can fill.
[[vk::binding(1, 0)]]
DepthTexture2D source_depth D3D12_REGISTER(t1, space0);

struct FullscreenOutput
{
    float4 position : SV_Position;
};

/// Both copies at once: the colour to the one colour target and the depth to
/// the depth attachment. A struct for `hiz.slang`'s reason — a bare `SV_Depth`
/// return lowers to a colour output on two targets.
struct CopyOutput
{
    float4 color : SV_Target;
    float depth : SV_Depth;
};

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // `hiz.slang`'s triangle: one oversized primitive covering the viewport,
    // with no interpolant — this pass addresses its source by `SV_Position`.
    float2 uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
CopyOutput fragmentMain(FullscreenOutput input)
{
    int3 texel = int3(int2(input.position.xy), 0);
    CopyOutput output;
    output.color = source_color.Load(texel);
    output.depth = source_depth.Load(texel);
    return output;
}
