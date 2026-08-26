// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s antialiasing row, cheap rung: FXAA 3.11
// over the tonemapped image, resolving into the swapchain.
//
// # Where it sits, and why that is after the tonemap
//
// The post chain is `scene → bloom → tonemap → AA → UI`. This pass reads what
// the tonemap wrote and writes what the UI then draws on top of, which puts two
// properties where they belong: the operator has already mapped the frame into
// the display's range, so an edge here is an edge a viewer would see rather than
// one hiding in the top of an HDR gradient; and the UI is composited afterwards
// at its own resolution, so text that was rasterised sharp stays sharp. Topic 18
// refuses AA after the UI pass in as many words, and this is the ordering that
// honours it.
//
// # The luma is corrected, because the image reaching here is linear
//
// FXAA is a *perceptual* filter: every threshold in it was chosen against
// gamma-space luma, which is where equal steps look equally different. What
// arrives here is not that. `tonemap.slang` writes linear values into an sRGB
// target and lets the hardware encode them, so a `Sample` of that target decodes
// straight back to linear — the sRGB round trip cancels and this pass sees the
// tonemapped image in a space its constants were never fitted to. Running FXAA
// on linear luma is a real defect and a quiet one: it under-blurs dark edges,
// where the eye is most sensitive, and over-blurs bright ones.
//
// `luma_of` corrects it with a single `sqrt`, which is the standard cheap
// stand-in for the sRGB encode — sRGB is close to a 2.2 power and this is 2.0.
// It is an approximation and is written down as one; what it buys is that the
// thresholds below mean what the algorithm's author meant by them.
//
// # Determinism: an edge search is the most driver-sensitive thing in the stack
//
// Every other pass in this chain is arithmetic. This one *decides* — whether a
// pixel is on an edge, which way the edge runs, and how far along it the search
// got — and each decision is a float comparison two rasterisers can land on
// opposite sides of. `crcbl-golden`'s rasteriser tolerance was measured against
// arithmetic disagreement, not against this.
//
// Three things keep it inside that tolerance, and they are the reason the code
// below looks more careful than a reference implementation:
//
// * **The early-out cannot cost much when it disagrees.** A pixel is only
//   filtered when its local luma range clears `edge_threshold`, and right at the
//   threshold the range *is* that threshold — so the two answers differ by a
//   blend of a contrast small enough to have nearly failed the test. The error a
//   disagreement can produce is bounded by the thing being tested.
// * **The search runs a constant number of steps.** There is no `break`: every
//   pixel executes `SEARCH_STEPS` iterations and a `done` flag stops the
//   accumulation. Same instructions, same count, every pixel — which removes
//   divergence as a variable and leaves only the comparisons themselves.
// * **The step sizes are a constant table indexed by an integer**, exactly as
//   `ssao.slang`'s rotation table is and for the same reason: an integer index
//   into a constant array is bit-identical by inspection, where anything
//   computed per pixel is one more thing four backends must agree about.
//
// What none of that removes is that this pass moves edge pixels on purpose. A
// golden of an antialiased frame is the most sensitive golden in the suite and
// should be read as one.
//
// # No `#include`
//
// `bloom_composite.slang` states the rule: the manifest hashes one source per
// artifact, so an include would be a file whose edits nothing downstream
// notices. The fullscreen triangle below is `tonemap.slang`'s, copied.

/// Everything the pass takes from the CPU.
///
/// `crcbl_shaders::fxaa::FxaaParams` is the Rust mirror and
/// `crcbl_render::forward` is what writes the buffer, once per frame.
struct FxaaParams
{
    /// One over the extent of the image being read, which is this pass's own
    /// extent — the resolve is 1:1 and the offsets below are in whole texels of
    /// it.
    float2 inv_source;
    /// Local luma range, as a fraction of the local maximum, below which a pixel
    /// is left alone. `crcbl_shaders::fxaa::DEFAULT_EDGE_THRESHOLD`.
    float edge_threshold;
    /// Absolute floor under the above, so that a dark region's tiny range does
    /// not read as an edge merely because its maximum is tiny too.
    /// `crcbl_shaders::fxaa::DEFAULT_EDGE_THRESHOLD_MIN`.
    float edge_threshold_min;
    /// How much of the subpixel (fine-detail) correction is applied, `0` to `1`.
    /// This is the knob that trades FXAA's characteristic softening against the
    /// shimmer it exists to remove.
    /// `crcbl_shaders::fxaa::DEFAULT_SUBPIXEL`.
    float subpixel;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

/// The tonemapped frame, at this pass's own extent.
[[vk::binding(0, 0)]]
Texture2D<float4> source;

/// A **linear** sampler, and the renderer creates it that way — unlike
/// `tonemap.slang`'s nearest one. This pass samples at fractional offsets by
/// construction: the whole filter is a bilinear fetch placed off-centre by an
/// amount it computed, so nearest filtering would round every one of those
/// offsets back to the pixel it started from and the pass would be a blit.
[[vk::binding(1, 0)]]
SamplerState sourceSampler;

[[vk::binding(2, 0)]]
ConstantBuffer<FxaaParams> params;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// How many steps the edge search takes along the edge, in each direction.
static const int SEARCH_STEPS = 12;

/// How far each step of the edge search reaches, in whole texels.
///
/// FXAA 3.11's `FXAA_QUALITY__PRESET 39` distances: the search starts at one
/// texel a step and lengthens once it is clear of the pixel it began at, so a
/// long edge is crossed in a bounded number of taps without a short one being
/// overshot. A constant table indexed by the loop counter, never a computed
/// progression — see the determinism note in the header.
static const float SEARCH_STEP[SEARCH_STEPS] = {
    1.0, 1.0, 1.0, 1.0, 1.0, 1.5, 2.0, 2.0, 2.0, 2.0, 4.0, 8.0
};

/// Perceptual luma of a linear colour.
///
/// Rec. 601 weights, then one `sqrt` standing in for the sRGB encode — the
/// header says why the correction is here at all and what it approximates.
float luma_of(float3 color)
{
    return sqrt(dot(color, float3(0.299, 0.587, 0.114)));
}

/// One bilinear tap, `offset` whole texels from `uv`.
///
/// **`SampleLevel` and not `Sample`, everywhere in this file.** WGSL refuses an
/// implicit-LOD sample reached from non-uniform control flow — `'textureSample'
/// must only be called from uniform control flow` — and every tap here is
/// reached from some: the early-out below returns before them, and the edge
/// search runs each of its steps under a flag. An implicit LOD would need a
/// derivative, and there is no derivative to take once the lanes of a quad stop
/// agreeing on where they are. There is nothing to lose by saying so: the source
/// is a single-mip image, so level zero is the only level there was.
///
/// Native targets accept the implicit form and this was compiled by all four of
/// them; what caught it was the browser gate, where a WGSL module that will not
/// parse is a device that refuses the pipeline and a scene that draws nothing.
float3 tap(float2 uv, float2 offset)
{
    return source.SampleLevel(sourceSampler, uv + offset * params.inv_source, 0.0).rgb;
}

/// Luma of one tap.
float luma_at(float2 uv, float2 offset)
{
    return luma_of(tap(uv, offset));
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // index 0 → uv (0,0), index 1 → (2,0), index 2 → (0,2). `tonemap.slang`
    // carries the *why* of this shape and of the -2.0 on Y.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    float2 uv = input.uv;
    float3 center = tap(uv, float2(0.0, 0.0));

    // The cross: the four neighbours that decide whether there is an edge here
    // at all, and which way it runs.
    float luma_c = luma_of(center);
    float luma_n = luma_at(uv, float2(0.0, -1.0));
    float luma_s = luma_at(uv, float2(0.0, 1.0));
    float luma_w = luma_at(uv, float2(-1.0, 0.0));
    float luma_e = luma_at(uv, float2(1.0, 0.0));

    float luma_min = min(luma_c, min(min(luma_n, luma_s), min(luma_w, luma_e)));
    float luma_max = max(luma_c, max(max(luma_n, luma_s), max(luma_w, luma_e)));
    float range = luma_max - luma_min;

    // Flat enough to leave alone. Alpha 1.0 on every path out of this pass: the
    // swapchain is composited opaque, exactly as `tonemap.slang` says.
    if (range < max(params.edge_threshold_min, luma_max * params.edge_threshold))
    {
        return float4(center, 1.0);
    }

    // The corners, needed for the orientation test and for the subpixel term.
    float luma_nw = luma_at(uv, float2(-1.0, -1.0));
    float luma_ne = luma_at(uv, float2(1.0, -1.0));
    float luma_sw = luma_at(uv, float2(-1.0, 1.0));
    float luma_se = luma_at(uv, float2(1.0, 1.0));

    float luma_ns = luma_n + luma_s;
    float luma_we = luma_w + luma_e;
    float luma_wcorners = luma_nw + luma_sw;
    float luma_ecorners = luma_ne + luma_se;
    float luma_ncorners = luma_nw + luma_ne;
    float luma_scorners = luma_sw + luma_se;

    // Which way the edge runs: compare the second derivative across the row
    // against the one down the column. The larger gradient is across the edge,
    // so the edge itself runs the other way.
    float edge_h = abs(-2.0 * luma_w + luma_wcorners)
        + abs(-2.0 * luma_c + luma_ns) * 2.0
        + abs(-2.0 * luma_e + luma_ecorners);
    float edge_v = abs(-2.0 * luma_n + luma_ncorners)
        + abs(-2.0 * luma_c + luma_we) * 2.0
        + abs(-2.0 * luma_s + luma_scorners);
    bool horizontal = edge_h >= edge_v;

    // The two neighbours across the edge, and which side of it is steeper.
    float luma_1 = horizontal ? luma_n : luma_w;
    float luma_2 = horizontal ? luma_s : luma_e;
    float gradient_1 = luma_1 - luma_c;
    float gradient_2 = luma_2 - luma_c;
    bool steeper_1 = abs(gradient_1) >= abs(gradient_2);
    float gradient_scaled = 0.25 * max(abs(gradient_1), abs(gradient_2));

    // Half a texel toward the steeper side, and the average luma of the pair
    // straddling the edge there. The search below asks how far that average
    // holds.
    float texel = horizontal ? params.inv_source.y : params.inv_source.x;
    float luma_local = steeper_1 ? 0.5 * (luma_1 + luma_c) : 0.5 * (luma_2 + luma_c);
    float step_length = steeper_1 ? -texel : texel;

    float2 edge_uv = uv;
    if (horizontal)
    {
        edge_uv.y += step_length * 0.5;
    }
    else
    {
        edge_uv.x += step_length * 0.5;
    }

    // Walk both ways along the edge until the local average stops holding.
    float2 along = horizontal
        ? float2(params.inv_source.x, 0.0)
        : float2(0.0, params.inv_source.y);

    float2 uv_neg = edge_uv - along;
    float2 uv_pos = edge_uv + along;
    float delta_neg = luma_of(source.SampleLevel(sourceSampler, uv_neg, 0.0).rgb) - luma_local;
    float delta_pos = luma_of(source.SampleLevel(sourceSampler, uv_pos, 0.0).rgb) - luma_local;
    bool done_neg = abs(delta_neg) >= gradient_scaled;
    bool done_pos = abs(delta_pos) >= gradient_scaled;
    float distance_neg = horizontal ? (uv.x - uv_neg.x) : (uv.y - uv_neg.y);
    float distance_pos = horizontal ? (uv_pos.x - uv.x) : (uv_pos.y - uv.y);

    // A constant trip count and a flag rather than a `break`, so every pixel in
    // the frame executes the same instructions — see the header.
    for (int i = 0; i < SEARCH_STEPS; ++i)
    {
        float reach = SEARCH_STEP[i];
        if (!done_neg)
        {
            uv_neg -= along * reach;
            delta_neg = luma_of(source.SampleLevel(sourceSampler, uv_neg, 0.0).rgb) - luma_local;
            done_neg = abs(delta_neg) >= gradient_scaled;
            distance_neg = horizontal ? (uv.x - uv_neg.x) : (uv.y - uv_neg.y);
        }
        if (!done_pos)
        {
            uv_pos += along * reach;
            delta_pos = luma_of(source.SampleLevel(sourceSampler, uv_pos, 0.0).rgb) - luma_local;
            done_pos = abs(delta_pos) >= gradient_scaled;
            distance_pos = horizontal ? (uv_pos.x - uv.x) : (uv_pos.y - uv.y);
        }
    }

    // The nearer end of the edge decides how far this pixel is from it, and the
    // fraction of the way across is how much to move.
    float span = distance_neg + distance_pos;
    float nearer = min(distance_neg, distance_pos);
    float offset = max(0.0, 0.5 - nearer / max(span, 1e-6)) * step_length;

    // The end this pixel is nearest has to be an end where the luma actually
    // crossed back over the centre's, or the edge ran off past the search and
    // this pixel is not on the part of it that needs moving.
    bool nearer_is_neg = distance_neg < distance_pos;
    float delta_nearer = nearer_is_neg ? delta_neg : delta_pos;
    if ((luma_c - luma_local < 0.0) == (delta_nearer < 0.0))
    {
        offset = 0.0;
    }

    // The subpixel term: fine detail smaller than the search can see, where the
    // centre's departure from its neighbourhood average is the whole signal.
    float average = (2.0 * (luma_ns + luma_we) + luma_wcorners + luma_ecorners) * (1.0 / 12.0);
    float subpixel_ratio = saturate(abs(average - luma_c) / max(range, 1e-6));
    float subpixel_weight = (-2.0 * subpixel_ratio + 3.0) * subpixel_ratio * subpixel_ratio;
    float subpixel_offset = subpixel_weight * subpixel_weight * params.subpixel * step_length;

    // Whichever correction is larger, not their sum: they are two estimates of
    // one displacement, and adding them double-counts an edge that is both.
    float final_offset = abs(subpixel_offset) > abs(offset) ? subpixel_offset : offset;

    float2 result_uv = uv;
    if (horizontal)
    {
        result_uv.y += final_offset;
    }
    else
    {
        result_uv.x += final_offset;
    }

    return float4(source.SampleLevel(sourceSampler, result_uv, 0.0).rgb, 1.0);
}
