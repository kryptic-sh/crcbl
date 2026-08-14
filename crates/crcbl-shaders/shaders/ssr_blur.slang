// crcbl-targets: spirv, wgsl, msl, dxil

// The other half of `ssr.slang`: `ssao_blur.slang`'s 4×4 kernel over the raw
// reflection, weighted by view-space depth **and** by how sharp each tap's own
// surface is, added to the scene colour.
//
// # It is the composite, and that is what makes the blur possible at all
//
// The march writes the reflection by itself into an `Rgba16Float` image of its
// own. This pass reads that image, the scene colour and the depth prepass, and
// writes their sum into a second `Rgba16Float` target which the tonemap reads in
// place of the scene colour. One pass, no blend state, and no image that is both
// read and written. Had the march composited, filtering the reflection here
// would have meant filtering the whole frame with it.
//
// # Why a blur and not a mip chain
//
// `ssr.slang`'s [`ROUGHNESS_CUTOFF`] is the statement that a single ray stands
// for a narrow lobe. This kernel is what widens that a little — enough for a
// rough conductor to reflect faintly rather than not at all — and
// `docs/plan/18-render-features.md` refuses the alternative in as many words:
// cone tracing over a colour mip chain is the better technique and needs a
// `SampleLevel` at a computed LOD, which is a filtered read whose level four
// rasterisers select arithmetic for. Upgrading is contained to this file.
//
// # The second weight is the point of the file
//
// A depth-weighted kernel alone averages a mirror with the rough metal beside it
// and a reflecting surface with the matt floor it stands on: at a contact edge
// the two are at the same depth, so nothing in `ssao_blur.slang`'s weight
// separates them. The reflection then fades towards every such edge, which reads
// as dirt on the surface rather than as a filter.
//
// So a tap also carries **how sharp its own surface is** — the alpha the march
// wrote, which is [`ROUGHNESS_CUTOFF`]'s ramp evaluated at that pixel's
// roughness. Taps are weighted by how near that is to the centre's, over the
// centre's own value: a tap on a surface too rough to reflect at all weighs
// exactly nothing, which is the case that matters and the one a floor beside a
// metal block is.
//
// # What this pass does to the determinism argument
//
// `ssr.slang` says a march has no denominator. This gives it one, over sixteen
// taps, on the same terms `ssao_blur.slang` states: full strength where every
// tap counts, and falling towards a divisor of one exactly where taps are
// rejected. It does not make the goldens evidence — a rejected tap is rejected
// on both drivers or the pixel was borderline anyway — but a disagreement in one
// tap now costs a sixteenth of a pixel rather than all of it.
//
// **A surface at or above the cutoff carries only its probe fallback**, with
// zero alpha. The zero alpha is still the condition that keeps the sharpness
// kernel's denominator safe, so that fallback is added from the centre rather
// than filtered. With a zero probe volume the centre value is zero and the scene
// colour remains bit-identical.

/// Everything this pass needs that is not in a texture, and it is
/// `ssr.slang`'s block — same members, same order, same buffer.
///
/// Declared a second time rather than included, on this repo's terms: the
/// manifest hashes one source per artifact, so an `#include` would be a file
/// whose edits nothing downstream notices — `ssao_blur.slang` carries the same
/// paragraph about the occlusion pair's block. `crcbl_shaders::ssr::SsrParams`
/// is the Rust mirror both agree with, and `crcbl_render::ssr` is what binds the
/// one buffer to both passes.
///
/// **Bound as `camera`** for the reason `ssao.slang` gives at the same
/// declaration: [`depth_at`] is copied from that file and the guard that holds
/// the copies together compares their bodies as text.
struct SsrParams
{
    /// Clip → view. Reversed-Z and possibly infinite, so `w` is what carries the
    /// depth and the divide below is not optional.
    float4x4 inv_proj;
    /// View → clip. `ssr.slang` projects the reflected ray onto the screen with
    /// it; nothing here does, and it is declared because the block's layout is
    /// the block's layout.
    float4x4 proj;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsrParams> camera;

/// The depth prepass's output — the same image the march read, bound here to
/// weight the kernel rather than to walk it.
///
/// `DepthTexture2D` and the paragraph that goes with it are `ssao.slang`'s: the
/// two spellings are one HLSL declaration and one D3D12 SRV and are **not** the
/// same WGSL, and only this one lowers to a `texture_depth_2d` that a `D32Float`
/// view can fill through the seam's `SampleType::Depth`.
///
/// **No sampler anywhere in this pass**, for the march's reason exactly: every
/// read is a `Load` at an integer pixel, so there is no UV, no filtering and no
/// texel-centre arithmetic for four rasterisers to disagree about.
[[vk::binding(1, 0)]]
DepthTexture2D scene_depth;

/// The `Rgba16Float` scene target the forward pass wrote, passed through with
/// the filtered reflection added to it.
[[vk::binding(2, 0)]]
Texture2D<float4> scene_color;

/// The march's output: `rgb` is the reflection at that pixel and `a` is the
/// weight it was computed under.
///
/// `ssr.slang`'s return value is the other half of this description. The two
/// zeroes it can hold are different statements and the kernel below reads them
/// as such: an `rgb` of zero with a positive `a` is a surface that reflects and
/// whose ray found nothing, and an `a` of zero is a surface this pass has
/// nothing to say about at all.
[[vk::binding(3, 0)]]
Texture2D<float4> reflection;

/// The reversed-Z far plane: `crcbl_hal::depth::CLEAR`, and the value
/// `ssao.slang` and `ssr.slang` both declare under this name.
///
/// **The far plane has no surface**, and the unprojection divides by a `clip.w`
/// an infinite reversed-Z projection takes to zero exactly here — so a tap there
/// has no view-space position to weigh, and the test below is what keeps that
/// division out of the sum rather than letting a NaN into it. The march wrote
/// nothing at such a pixel anyway; this is about the arithmetic, not the value.
///
/// `crcbl_shaders::ssao::DEPTH_FAR` is the Rust mirror of all three.
static const float DEPTH_FAR = 0.0;

/// The least thickness the march credits any surface with, as a share of the
/// ray's own view depth — `ssr.slang`'s `THICKNESS_FLOOR`.
///
/// The length this pass's depth weight is placed on, and the only one the march
/// has that does not need a ray to evaluate: the bound `thickness_at` applies is
/// the ray's own depth advance over one step, floored by this, and a pass with
/// no ray keeps the floor. `crcbl_shaders::ssr`'s
/// `the_thickness_floor_matches_the_one_the_march_declares` holds the two
/// copies together.
static const float THICKNESS_FLOOR = 0.02;

/// How many of those thicknesses a tap's view depth may sit from the centre's
/// before it counts for nothing.
///
/// **The kernel is four pixels wide and the surfaces a reflection lives on
/// recede from the eye.** A floor seen at a shallow angle — which is most of
/// what reflects anything — covers several floor-thicknesses of view depth
/// across four pixel rows, so a tolerance of one thickness would switch this
/// filter off on exactly the surfaces it exists for, and the stepping the march
/// leaves behind would survive it. This is loose on purpose, and it can afford
/// to be: what it has to reject is a silhouette, where the two depths differ by
/// a large multiple of it, and the weight beside it rejects a different material
/// at the same depth.
///
/// A shader constant either way — not a fifth field the Rust mirror has to agree
/// about — and derived rather than tuned, on `ssao_blur.slang`'s
/// `DEPTH_TOLERANCE_RADII` terms: the march's own floor is the length, and this
/// is how many of them the kernel spans.
static const float DEPTH_TOLERANCE_THICKNESSES = 8.0;

/// The kernel's reach either side of the centre pixel.
///
/// `-1 ..= 2` on each axis, which is `ssao_blur.slang`'s window: sixteen taps
/// with a centre pixel, since an even-sided box has none. That file's footprint
/// argument is about `ssao.slang`'s rotation tile and does not transfer — what
/// this one has is that a wider kernel drags a reflection further past the
/// silhouette of the thing reflecting it, and the weights rather than the width
/// are what this pass leans on.
static const int KERNEL_FROM = -1;

/// One past the far edge of the kernel — see [`KERNEL_FROM`].
static const int KERNEL_TO = 3;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The depth prepass's value at `pixel`, clamped into the image.
///
/// `ssao.slang`'s function, for its reason: a `Load` outside the image reads
/// zero on some targets and is undefined on others, and zero is [`DEPTH_FAR`] —
/// so an unclamped edge tap would report "nothing there" and be rejected along
/// all four borders.
float depth_at(int2 pixel, int2 extent)
{
    int2 clamped = clamp(pixel, int2(0, 0), extent - int2(1, 1));
    return scene_depth.Load(int3(clamped, 0));
}

/// How far in front of the eye the pixel at `pixel` is, given the depth already
/// fetched for it.
///
/// `ssao_blur.slang`'s function, which is `ssao.slang`'s `view_position` with
/// everything but `z` dropped — including the subtraction on `y` alone, which is
/// where clip space's `+Y` up meets `SV_Position.y` counting down the
/// framebuffer. The caller fetches the depth because it also has to test it
/// against [`DEPTH_FAR`].
float view_z(int2 pixel, float depth, float2 extent)
{
    float2 ndc = float2(
        (float(pixel.x) + 0.5) / extent.x * 2.0 - 1.0,
        1.0 - (float(pixel.y) + 0.5) / extent.y * 2.0);
    float4 view = mul(camera.inv_proj, float4(ndc, depth, 1.0));
    return view.z / view.w;
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // `tonemap.slang`'s triangle, for the reason that file gives.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    uint width;
    uint height;
    // Read off the image rather than passed in the block, on the march's terms:
    // this pass renders at the depth buffer's own size by construction, and a
    // second copy of that number in a uniform is a second thing that can be
    // stale.
    scene_depth.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    float2 size = float2(float(width), float(height));
    int2 pixel = int2(input.position.xy);

    // The scene colour is passed through whatever happens below — this pass is
    // the composite, so every pixel of the target has to be written.
    float4 lit = scene_color.Load(int3(pixel, 0));
    float4 centre = reflection.Load(int3(pixel, 0));
    float sharpness = centre.a;
    float centre_depth = depth_at(pixel, extent);
    // Nothing was drawn here. A rough surface has no screen-space ray, but its
    // centre may carry a probe fallback; add that directly rather than dividing
    // the sharpness kernel by its zero alpha.
    if (centre_depth <= DEPTH_FAR)
    {
        return lit;
    }
    if (sharpness <= 0.0)
    {
        return float4(lit.rgb + centre.rgb, lit.a);
    }

    float centre_z = view_z(pixel, centre_depth, size);
    float tolerance = abs(centre_z) * THICKNESS_FLOOR * DEPTH_TOLERANCE_THICKNESSES;

    float3 total = float3(0.0, 0.0, 0.0);
    float weight = 0.0;
    for (int y = KERNEL_FROM; y < KERNEL_TO; ++y)
    {
        for (int x = KERNEL_FROM; x < KERNEL_TO; ++x)
        {
            // **Clamped, not skipped**, on `ssao_blur.slang`'s terms: a `Load`
            // outside the image is zero on some targets and undefined on others,
            // and clamping repeats the edge pixel, which is the honest answer
            // for a filter that has run out of evidence.
            int2 tap = clamp(pixel + int2(x, y), int2(0, 0), extent - int2(1, 1));
            float4 tapped = reflection.Load(int3(tap, 0));
            // **The centre tap's share is one by construction**, which is what
            // keeps the divisor off zero — rather than because the two ramps
            // below return one for differences the same expressions computed
            // twice ought to make exactly zero. The test is on the loop's own
            // integers and is not a thing two drivers can resolve differently.
            float share = 1.0;
            if (x != 0 || y != 0)
            {
                float depth = depth_at(tap, extent);
                float away = abs(view_z(tap, depth, size) - centre_z);
                // Over the centre's own ramp, so a tap on a surface the march
                // had nothing to say about — a floor under a metal block, at
                // exactly the same depth as its contact edge — weighs exactly
                // zero, and a mirror beside a rough conductor weighs almost
                // none of it.
                float apart = abs(tapped.a - sharpness);
                // A `select` rather than an `if` around the accumulation: the
                // far test is the only comparison in this pass that decides
                // anything, and it is the one [`DEPTH_FAR`] argues for.
                share = depth <= DEPTH_FAR
                    ? 0.0
                    : saturate(1.0 - away / tolerance) * saturate(1.0 - apart / sharpness);
            }
            total += tapped.rgb * share;
            weight += share;
        }
    }

    // The zero-sharpness branch above is this expression's limit: as sharpness
    // approaches zero, retain the centre's direct probe fallback rather than
    // abruptly replacing it with a fully filtered neighbourhood.
    float3 filtered = total / weight;
    return float4(lit.rgb + lerp(centre.rgb, filtered, sharpness), lit.a);
}
