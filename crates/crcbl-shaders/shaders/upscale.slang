// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/43-render-standards.md`'s render-scale row: the spatial upscale
// that carries an internal render target to the caller's own extent.
//
// # Why this pass exists at all
//
// Render scale is the single largest performance knob a settings menu can
// offer — every pass before this one costs what its extent costs, so drawing
// the frame at 70% of the window is roughly half the shading work — and it is
// the knob `docs/plan/15-windowing.md`'s borderless definition already assumes.
// Until this pass existed the post chain's `[upscale]` stage was an ordering
// rule for something nothing implemented, and every internal target in the
// frame was the swapchain's size.
//
// This is a *spatial* upscale and not a temporal one. DLSS, FSR 3, XeSS and TSR
// all take motion vectors, and `crcbl_shaders::mesh::GpuInstance` carries no
// previous transform — the blocker topic 43's section 9 is about. What a
// spatial filter can do is carry a smaller frame to a larger one without the
// mush a bilinear stretch leaves, and that is what this does.
//
// # Catmull-Rom, and what it is checked against
//
// The filter is the Catmull-Rom cubic — the Mitchell-Netravali family at
// `B = 0`, `C = 0.5`, which is the spatial upscale essentially every engine
// reaches for. It is interpolating (a source texel survives unchanged when the
// sample lands on it) where a B-spline is not, and it keeps an edge that a
// bilinear stretch turns into a ramp.
//
// `crcbl_shaders::upscale::catmull_rom_weights` is the CPU mirror of
// `catmull_rom_weights` below and is pinned against the properties the family
// is defined by rather than against numbers this file produced: the four
// weights sum to one at every position, a sample landing exactly on a texel
// takes that texel and nothing else, and the half-way position is symmetric.
// Those are what a transcription slip in the two polynomials breaks.
//
// **It overshoots on purpose and the overshoot is clamped.** Two of the four
// weights are negative away from the sample point, which is where the filter's
// acutance comes from and is also how it rings past the range of its inputs.
// What reaches this pass is a tonemapped frame in display range, so `saturate`
// at the end is the whole of the answer — there is no headroom above one for a
// ringing lobe to be meaningful in.
//
// # Determinism
//
// Two cubic polynomials, a `floor` and sixteen weighted taps: no transcendental
// function anywhere, which is this workspace's rule for arithmetic that reaches
// a colour. The taps are a constant 4x4 neighbourhood with no branch in the
// loop, so every pixel executes the same instructions in the same order.
//
// # No `#include`
//
// `bloom_composite.slang` states the rule: the manifest hashes one source per
// artifact, so an include would be a file whose edits nothing downstream
// notices. The fullscreen triangle below is `tonemap.slang`'s, copied.

/// Everything the pass takes from the CPU.
///
/// `crcbl_shaders::upscale::UpscaleParams` is the Rust mirror and
/// `crcbl_render::forward` is what writes the buffer, once per frame.
struct UpscaleParams
{
    /// The extent of the image being read, in texels — which is the *internal*
    /// render extent and not this pass's own. The whole of what this pass does
    /// is that those two differ.
    float2 source_extent;
    /// One over [`source_extent`], so the tap loop multiplies rather than
    /// dividing sixteen times.
    float2 inv_source;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

/// The resolved frame at the internal render extent.
[[vk::binding(0, 0)]]
Texture2D<float4> source;

/// A **linear** sampler, and the renderer creates it that way.
///
/// The weights below are computed per tap, so nearest filtering would be the
/// defensible choice — and it is the wrong one at the border. Every tap of the
/// 4x4 neighbourhood is addressed by UV, so the ring outside the source is what
/// `ClampToEdge` decides, and a linear sampler reaching a clamped edge returns
/// the edge texel exactly, which is what the filter wants there.
[[vk::binding(1, 0)]]
SamplerState sourceSampler;

[[vk::binding(2, 0)]]
ConstantBuffer<UpscaleParams> params;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The four Catmull-Rom weights for a sample `f` of the way between the two
/// middle texels of a run of four, `f` in `[0, 1)`.
///
/// The kernel is `w(|x|)`, with `|x|` the distance from the sample to the
/// texel's centre, and the four distances are `1 + f`, `f`, `1 - f`, `2 - f`:
///
/// ```text
/// w(x) =  1.5|x|^3 - 2.5|x|^2 + 1              for |x| <= 1
/// w(x) = -0.5|x|^3 + 2.5|x|^2 - 4|x| + 2       for 1 < |x| < 2
/// ```
///
/// Written out per lane rather than as a loop over a distance, because each
/// lane knows at compile time which of the two arms it is on — the inner pair
/// is always within one texel and the outer pair is always beyond it.
float4 catmull_rom_weights(float f)
{
    float f2 = f * f;
    float f3 = f2 * f;
    return float4(
        // |x| = 1 + f, the far arm: -0.5(1+f)^3 + 2.5(1+f)^2 - 4(1+f) + 2,
        // multiplied out.
        -0.5 * f3 + f2 - 0.5 * f,
        // |x| = f, the near arm.
        1.5 * f3 - 2.5 * f2 + 1.0,
        // |x| = 1 - f, the near arm.
        -1.5 * f3 + 2.0 * f2 + 0.5 * f,
        // |x| = 2 - f, the far arm.
        0.5 * f3 - 0.5 * f2);
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // index 0 -> uv (0,0), index 1 -> (2,0), index 2 -> (0,2). `tonemap.slang`
    // carries the *why* of this shape and of the -2.0 on Y.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    // Texel space, with a texel's centre at its integer coordinate: the -0.5 is
    // what turns a UV whose 0 is the *corner* of the first texel into a
    // coordinate whose 0 is its middle. Getting this wrong shifts the whole
    // frame half an internal texel, which reads as softness rather than as a
    // shift and is why it is spelled out here.
    float2 pos = input.uv * params.source_extent - 0.5;
    float2 base = floor(pos);
    float2 f = pos - base;

    float4 wx = catmull_rom_weights(f.x);
    float4 wy = catmull_rom_weights(f.y);

    float3 sum = float3(0.0, 0.0, 0.0);
    for (int j = 0; j < 4; ++j)
    {
        for (int i = 0; i < 4; ++i)
        {
            float2 texel = base + float2(float(i) - 1.0, float(j) - 1.0) + 0.5;
            float3 tap = source.SampleLevel(sourceSampler, texel * params.inv_source, 0.0).rgb;
            sum += tap * (wx[i] * wy[j]);
        }
    }

    // The ringing clamp, and alpha 1.0 on the way out: the swapchain is
    // composited opaque, exactly as `tonemap.slang` says.
    return float4(saturate(sum), 1.0);
}
