// crcbl-targets: spirv, wgsl, msl, dxil

// The other half of `ssao.slang`'s determinism rule: a 4×4 box blur over the raw
// occlusion, into a second `R8Unorm` image.
//
// # It is not optional, and that is the whole point of the file
//
// `ssao.slang` rotates its hemisphere by a sixteen-entry table indexed by
// `pixel.xy & 3`, so the raw result carries the tile as visible banding — and
// each of its eight samples is a *binary* depth comparison, so a sample landing
// on the threshold flips on one driver and not another and swings that pixel by
// an eighth. This kernel's footprint is exactly that 4×4 tile, so it does both
// jobs at once: every pixel of the tile contributes to every output pixel, which
// removes the banding, and an isolated flipped sample is divided by sixteen —
// which brings a one-driver disagreement back under
// `crcbl_golden::Tolerance::RASTERISER`'s channel delta.
//
// # A box, not a bilateral filter
//
// A box blur bleeds occlusion across a silhouette as a halo. The fix is a
// bilateral kernel weighted by depth, which needs the depth buffer bound here as
// well, and it is **deliberately deferred to the slice after the first frame
// exists** — see `docs/plan/18-render-features.md`. Swapping it in is a change to
// this file and its bind group, and to nothing else.

/// The raw occlusion `ssao.slang` wrote, fetched by integer pixel.
///
/// `Texture2D<float>` and not `DepthTexture2D`: this one is an ordinary
/// single-channel colour image — `R8Unorm`, the same shape `ui.slang`'s glyph
/// atlas is — so it lowers to `texture_2d<f32>` and binds through the seam's
/// `SampleType::Float`. The depth spelling is `ssao.slang`'s problem and not
/// this file's.
///
/// **No sampler**, for `ssao.slang`'s reason exactly: every read is a `Load` at
/// an integer pixel, so there is no filtering for four rasterisers to disagree
/// about.
[[vk::binding(0, 0)]]
Texture2D<float> occlusion;

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
    int2 pixel = int2(input.position.xy);

    float total = 0.0;
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
            total += occlusion.Load(int3(tap, 0));
        }
    }
    float taps = float((KERNEL_TO - KERNEL_FROM) * (KERNEL_TO - KERNEL_FROM));
    return total / taps;
}
