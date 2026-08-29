// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/49-antialiasing.md`'s antialiasing ladder, second rung, first of
// three passes: SMAA 1x luma edge detection over the tonemapped image.
//
// # The three passes, and what each one hands the next
//
// `smaa_edges.slang` reads the tonemapped frame and writes a two-channel mask —
// red is "there is an edge on this pixel's **west** side", green is "…on its
// **north** side". `smaa_weights.slang` reads that mask, searches along each
// edge for the pattern it belongs to, looks the pattern up in the two committed
// tables and writes a blend weight per side. `smaa_blend.slang` reads the frame
// and those weights and mixes each pixel with the one neighbour its weights
// point at. Nothing here is a function of any earlier frame — the whole tier is
// one frame's pixels and two constant tables, which is what makes it
// golden-safe where TAA is not.
//
// The transcription target is the reference `SMAA.hlsl` (Jimenez et al.,
// `iryoku/smaa`), `SMAA_PRESET_HIGH`, **1x only**: no subsample index, no
// reprojection, no predication. Those paths are not branches that happen to be
// off here, they are absent — a frame this engine draws has no jitter and no
// velocity buffer for them to read.
//
// # The luma is corrected, because the image reaching here is linear
//
// The reference says it in capitals: luma edge detection wants gamma-corrected
// colour. What arrives here is not that. `tonemap.slang` writes linear values
// into an sRGB-format target and lets the hardware encode, so a sample of that
// target decodes straight back to linear — and `SMAA_THRESHOLD` was fitted
// against display-space luma, where equal steps look equally different.
//
// `luma_of` corrects it with one `sqrt`, exactly as `fxaa.slang` does and for
// the same reason; that file's header carries the argument at length. It is an
// approximation of the sRGB encode (2.0 standing in for 2.2) and is written
// down as one.
//
// # A point sampler, unlike the two passes after it
//
// Every fetch here is `SMAASamplePoint` in the reference: a whole-texel offset
// from a pixel centre, read for its own value. The renderer creates a
// **nearest** sampler for this pass for that reason, where `smaa_weights.slang`
// and `smaa_blend.slang` both take a linear one because their whole method is
// reading between texels. `crcbl_render::smaa` is where both are built.
//
// # No `discard`
//
// The reference discards a pixel with no edge so the mask keeps its clear. This
// writes zero instead: the full-screen triangle covers every pixel of the
// target, so a written zero and a preserved clear are the same two bytes — and
// because every pixel is written, `crcbl_render::smaa` can attach the mask with
// `LoadOp::DontCare` and pay for neither. Not discarding also keeps WGSL's
// uniformity rules easy to satisfy: there is no early exit above the taps.
//
// `smaa_weights.slang` returns a zero weight the same way and for the same
// reason, so all three passes in this tier write every pixel of their target.
//
// # No `#include`
//
// `bloom_composite.slang` states the rule: the manifest hashes one source per
// artifact, so an include would be a file whose edits nothing downstream
// notices. The fullscreen triangle below is `tonemap.slang`'s, copied, and the
// three SMAA sources each carry their own copy of the parameter block.

/// Everything the three SMAA passes take from the CPU.
///
/// `crcbl_shaders::smaa::SmaaParams` is the Rust mirror and
/// `crcbl_render::smaa` is what writes the buffer, once per frame. One block
/// for the three sources, on `bloom_*.slang`'s terms: the three passes run at
/// one extent and a second block would be a second thing to keep in step.
///
/// It carries the extent and nothing else. The reference's quality constants
/// are `static const` below rather than fields here, because two of them are
/// loop trip counts — a uniform there would make the searches dynamically
/// bounded and take the determinism argument `fxaa.slang` spends its header on
/// with it — and the other two are a threshold and a rounding fraction that
/// nothing in this engine asks to move. A block carrying a knob with no caller
/// is machinery ahead of need.
struct SmaaParams
{
    /// One over the extent of the image being read, which is every SMAA pass's
    /// own extent — the reference's `SMAA_RT_METRICS.xy`.
    float2 inv_source;
    /// That extent in texels — the reference's `SMAA_RT_METRICS.zw`. Only
    /// `smaa_weights.slang` reads it, and it is in the shared block rather than
    /// in a second one for the reason above.
    float2 source_size;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

/// The tonemapped frame, at this pass's own extent.
[[vk::binding(0, 0)]]
Texture2D<float4> source;

/// A **nearest** sampler — see the header. Every tap below is a whole texel
/// from the pixel centre and wants that texel's own value.
[[vk::binding(1, 0)]]
SamplerState sourceSampler;

[[vk::binding(2, 0)]]
ConstantBuffer<SmaaParams> params;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// `SMAA_THRESHOLD` at `SMAA_PRESET_HIGH`: the luma delta across a pixel
/// boundary, in display space, below which there is no edge there.
static const float THRESHOLD = 0.1;

/// `SMAA_LOCAL_CONTRAST_ADAPTATION_FACTOR`. An edge is discarded when a
/// neighbouring boundary has more than this much more contrast than it does,
/// which is what keeps the filter off the quiet side of a strong edge.
static const float LOCAL_CONTRAST_ADAPTATION_FACTOR = 2.0;

/// Perceptual luma of a linear colour.
///
/// Rec. 709 weights — the reference's own — then one `sqrt` standing in for the
/// sRGB encode. The header says why the correction is here and what it
/// approximates.
float luma_of(float3 color)
{
    return sqrt(dot(color, float3(0.2126, 0.7152, 0.0722)));
}

/// Luma of the texel `offset` whole texels from `uv`.
///
/// **`SampleLevel` and not `Sample`.** WGSL refuses an implicit-LOD sample
/// reached from non-uniform control flow, and the early return below puts every
/// tap after it in exactly that position. `fxaa.slang`'s header carries the
/// whole account, including that all four native targets accepted the implicit
/// form and only the browser gate caught it. The source is single-mip, so level
/// zero is the only level there was.
float luma_at(float2 uv, float2 offset)
{
    return luma_of(source.SampleLevel(sourceSampler, uv + offset * params.inv_source, 0.0).rgb);
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

    // The west and north deltas, which are the two this pixel owns: an edge
    // belongs to exactly one of the pixels either side of it, and taking the
    // west and north sides is the reference's convention throughout.
    float l = luma_at(uv, float2(0.0, 0.0));
    float l_left = luma_at(uv, float2(-1.0, 0.0));
    float l_top = luma_at(uv, float2(0.0, -1.0));

    float2 delta = abs(l - float2(l_left, l_top));
    float2 edges = step(float2(THRESHOLD, THRESHOLD), delta);

    // No edge on either side. Zero rather than a `discard` — see the header.
    if (edges.x + edges.y == 0.0)
    {
        return float4(0.0, 0.0, 0.0, 1.0);
    }

    // Local contrast adaptation: the strongest delta anywhere in the
    // neighbourhood, against this pixel's own two. An edge whose neighbour is
    // more than `LOCAL_CONTRAST_ADAPTATION_FACTOR` times stronger is the weak
    // half of a pair and is dropped, so a step gets one line of edge pixels
    // rather than two.
    float l_right = luma_at(uv, float2(1.0, 0.0));
    float l_bottom = luma_at(uv, float2(0.0, 1.0));
    float2 max_delta = max(delta, abs(l - float2(l_right, l_bottom)));

    float l_leftleft = luma_at(uv, float2(-2.0, 0.0));
    float l_toptop = luma_at(uv, float2(0.0, -2.0));
    float2 far_delta = abs(float2(l_left, l_top) - float2(l_leftleft, l_toptop));
    max_delta = max(max_delta, far_delta);

    float final_delta = max(max_delta.x, max_delta.y);
    edges *= step(float2(final_delta, final_delta), LOCAL_CONTRAST_ADAPTATION_FACTOR * delta);

    // Blue and alpha are written and unread: the target is `Rg8Unorm`, so the
    // hardware drops them.
    return float4(edges, 0.0, 1.0);
}
