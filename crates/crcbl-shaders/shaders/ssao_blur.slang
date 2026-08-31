// crcbl-targets: spirv, wgsl, msl, dxil

// The other half of `ssao.slang`'s determinism rule: a 4×4 kernel over the raw
// occlusion, weighted by view-space depth, into a second `R8Unorm` image.
//
// # It is not optional, and that is the whole point of the file
//
// `ssao.slang` rotates its hemisphere by a sixteen-entry table indexed by
// `pixel.xy & 3`, so the raw result carries the tile as visible banding — and
// each of its eight samples is a *binary* depth comparison, so a sample landing
// on the threshold flips on one driver and not another and swings that pixel by
// an eighth. This kernel's footprint is exactly that 4×4 tile, so every pixel of
// the tile contributes to every output pixel and the banding goes.
//
// # What the divisor is worth now
//
// The box blur this replaced divided an isolated flipped sample by sixteen,
// which is what brought a one-driver disagreement back under
// `crcbl_golden::Tolerance::RASTERISER`'s channel delta. A weighted kernel
// cannot promise sixteen everywhere, so here is what it does promise:
//
// * **Full strength on a flat surface.** The weight is a linear ramp on the
//   view-space depth difference over a tolerance derived from the AO radius, and
//   three pixels of a surface facing the camera differ by orders of magnitude
//   less than that. Every tap weighs one, the divisor is sixteen, and that is
//   the case over most of any frame — which is also where the tile banding and
//   the flipped samples are, because both are properties of `ssao.slang`'s
//   sampling rather than of the geometry.
// * **Weakened exactly where taps are rejected**, which is at a silhouette and
//   at the far plane. There the divisor falls towards one, and a sample two
//   drivers resolved differently counts for correspondingly more.
//
// That trade is deliberate rather than overlooked. The taps a box blur spent at
// a silhouette were a foreground pixel's occlusion averaged with a background
// that is not the same surface, which is a halo in every frame; the taps this
// one drops are the same ones, and what it gives up is margin against a
// disagreement that may not be in any of them.
//
// # The weight is on view-space Z, which is why this pass has a uniform block
//
// A reversed-Z difference is not a distance. The same one-metre gap is an
// enormous depth delta in front of the eye and almost none near the far plane,
// so a tolerance on the raw value would be a different filter in every part of
// the frame. This pass therefore unprojects, exactly as `ssao.slang` does — and
// through the same `SsaoParams` buffer, bound here as well rather than copied
// into a second block that could disagree with it.
//
// # The weight is smooth, and the one comparison left is against a constant
//
// `if (abs(dz) < threshold)` would put a *binary* decision on the output pixel,
// which is precisely what `ssao.slang`'s header spends four paragraphs keeping
// off its input samples: two drivers resolve the borderline case differently and
// the whole pixel jumps. Rejection here is a ramp with no branch in it, so a
// driver that computes `dz` a little differently moves the result a little.
//
// The `DEPTH_FAR` test stays, and it is not that kind of comparison: it is an
// equality against an exact constant, on the same value `ssao.slang` already
// tested against the same constant, so either both take that branch or neither
// does.

/// Everything this pass needs that is not in a texture, and it is
/// `ssao.slang`'s block — same members, same order, same buffer.
///
/// Declared a second time rather than included, on this repo's terms: the
/// manifest hashes one source per artifact, so an `#include` would be a file
/// whose edits nothing downstream notices — `cull.slang` carries the same
/// paragraph. `crcbl_shaders::ssao::SsaoParams` is the Rust mirror both agree
/// with, and `crcbl_render::ssao` is what binds the one buffer to both passes.
///
/// **Bound as `camera`** for the reason `ssao.slang` gives at the same
/// declaration: `depth_at` is copied into `ssr.slang` as well, and the guard
/// that holds the copies together compares their bodies as text.
struct SsaoParams
{
    /// Clip → view. Reversed-Z and possibly infinite, so `w` is what carries the
    /// depth and the divide below is not optional.
    float4x4 inv_proj;
    /// View → clip. `ssao.slang` projects a sample point back to a pixel with
    /// it; nothing here does, and it is declared because the block's layout is
    /// the block's layout.
    float4x4 proj;
    /// `x` is the sampling radius in **world units**, which is what
    /// [`DEPTH_TOLERANCE_RADII`] is a share of. `y` is the count of planes
    /// `ssao.slang` swept and `zw` pad the row; nothing here reads them.
    float4 params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsaoParams> camera;

/// The raw occlusion `ssao.slang` wrote, fetched by integer pixel.
///
/// `Texture2D<float>` and not `DepthTexture2D`: this one is an ordinary
/// single-channel colour image — `R8Unorm`, the same shape `ui.slang`'s glyph
/// atlas is — so it lowers to `texture_2d<f32>` and binds through the seam's
/// `SampleType::Float`. The depth spelling is the binding below.
///
/// **No sampler**, for `ssao.slang`'s reason exactly: every read here is a
/// `Load` at an integer pixel, so there is no filtering for four rasterisers to
/// disagree about.
[[vk::binding(1, 0)]]
Texture2D<float> occlusion;

/// The depth prepass's output — the same image `ssao.slang` read, bound here to
/// weight the kernel rather than to gather occlusion.
///
/// `DepthTexture2D` and the paragraph that goes with it are `ssao.slang`'s: the
/// two spellings are one HLSL declaration and one D3D12 SRV and are **not** the
/// same WGSL, and only this one lowers to a `texture_depth_2d` that a `D32Float`
/// view can fill through `SampleType::Depth`.
[[vk::binding(2, 0)]]
DepthTexture2D scene_depth;

/// The reversed-Z far plane: `crcbl_hal::depth::CLEAR`, and the value
/// `ssao.slang` declares under this name.
///
/// **The far plane has no surface, and it is where the halo comes from.** A
/// pixel the geometry never covered reads fully unoccluded, so a box blur
/// averages that 1.0 into the rim of anything standing in front of it and draws
/// a bright fringe along the silhouette. A far tap therefore takes no weight at
/// all below.
///
/// A far *centre* leaves early instead, for `ssao.slang`'s reason: an infinite
/// reversed-Z projection takes `clip.w` to zero exactly here, so there is no
/// view-space position to unproject and nothing to filter — `ssao.slang` wrote
/// 1.0 at that pixel and this pass hands the same value on.
///
/// `crcbl_shaders::ssao::DEPTH_FAR` is the Rust mirror of both.
static const float DEPTH_FAR = 0.0;

/// How far a tap's view-space depth may sit from the centre's before it counts
/// for nothing, as a share of `ssao.slang`'s sampling radius.
///
/// **Derived from the radius rather than tuned, and that is what makes it one
/// filter across the whole frame.** The radius is the only length this pair of
/// passes has: `ssao.slang` gathers occlusion within it, and its falloff is at
/// full strength while two surfaces are closer than that and cuts beyond it. So
/// the radius is already this pass's answer to "are these two pixels near enough
/// to be occluding each other", and the ramp below is placed on it — half weight
/// at exactly one radius, zero at this many.
///
/// A uniform field would be a knob nobody adjusts and a fourth thing the Rust
/// mirror has to agree about, for a number that is not free to move: it is
/// whatever the radius says it is.
static const float DEPTH_TOLERANCE_RADII = 2.0;

/// The kernel's reach either side of the centre pixel.
///
/// `-1 ..= 2` on each axis is sixteen taps whose footprint is exactly
/// `ssao.slang`'s rotation tile, which is the property the header rests on. An
/// even-sided box has no centre pixel, so the window is deliberately
/// asymmetric rather than rounded to 5×5 — a wider one would blur past the tile
/// and cost contact detail for nothing.
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
/// `ssao.slang`'s `view_position` with everything but `z` dropped, and the
/// arithmetic is that function's exactly — including the subtraction on `y`
/// alone, which is where clip space's `+Y` up meets `SV_Position.y` counting
/// down the framebuffer. The caller fetches the depth because it also has to
/// test it against [`DEPTH_FAR`].
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
float fragmentMain(FullscreenOutput input) : SV_Target
{
    uint width;
    uint height;
    occlusion.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    float2 size = float2(float(width), float(height));
    int2 pixel = int2(input.position.xy);

    float centre_depth = depth_at(pixel, extent);
    // Nothing was drawn here — see [`DEPTH_FAR`]. `ssao.slang` returned 1.0 at
    // this pixel and there is neither a surface to filter nor a view-space
    // position to weight the neighbours against.
    if (centre_depth <= DEPTH_FAR)
    {
        return 1.0;
    }
    float centre_z = view_z(pixel, centre_depth, size);
    float tolerance = camera.params.x * DEPTH_TOLERANCE_RADII;

    float total = 0.0;
    float weight = 0.0;
    for (int y = KERNEL_FROM; y < KERNEL_TO; ++y)
    {
        for (int x = KERNEL_FROM; x < KERNEL_TO; ++x)
        {
            // **Clamped, not skipped.** A `Load` outside the image reads zero on
            // some targets and is undefined on others, and zero is "fully
            // occluded" here — so an unclamped edge tap would draw a dark border
            // one kernel wide around every frame. Clamping repeats the edge
            // pixel, which is the honest answer for a filter that has run out of
            // evidence.
            int2 tap = clamp(pixel + int2(x, y), int2(0, 0), extent - int2(1, 1));
            // **The centre tap's share is one by construction**, which is what
            // keeps the divisor off zero — rather than because the ramp below
            // returns one for a difference the same expression computed twice
            // ought to make exactly zero. The test is on the loop's own integers
            // and is not a thing two drivers can resolve differently.
            float share = 1.0;
            if (x != 0 || y != 0)
            {
                float depth = depth_at(tap, extent);
                float away = abs(view_z(tap, depth, size) - centre_z);
                // A `select` rather than an `if` around the accumulation: the
                // far test is the only comparison in this pass that decides
                // anything, and it is the one [`DEPTH_FAR`] argues for.
                share = depth <= DEPTH_FAR ? 0.0 : saturate(1.0 - away / tolerance);
            }
            total += occlusion.Load(int3(tap, 0)) * share;
            weight += share;
        }
    }
    // At least the centre tap's one, by the construction above.
    return total / weight;
}
