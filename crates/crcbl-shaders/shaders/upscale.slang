// crcbl-targets: spirv, wgsl, msl, dxil

// Topic 43's render-scale row: the spatial upscale
// that carries an internal render target to the caller's own extent.
//
// # Why this pass exists at all
//
// Render scale is the single largest performance knob a settings menu can
// offer — every pass before this one costs what its extent costs, so drawing
// the frame at 70% of the window is roughly half the shading work — and it is
// the knob the borderless definition in `docs/notes/backends.md` already assumes.
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
// # Nine fetches for sixteen taps
//
// The kernel spans a 4x4 neighbourhood, but it is fetched in nine. The two
// middle weights of each axis are both positive, so their pair of taps is one
// linear interpolation between the two middle texels — `w1 * a + w2 * b` is
// `(w1 + w2) * lerp(a, b, w2 / (w1 + w2))` — and a single bilinear fetch placed
// that far between them returns it. Per axis that is three taps instead of
// four, and nine instead of sixteen over the square. The pass is bound by its
// texture-sample rate, so the fetch count is its cost: measured on 2026-10-31
// on an RDNA2 iGPU at 1440x810 to 1920x1080 it went from 1.87 ms to 1.06 ms,
// and the output stayed within three 8-bit codes of the sixteen-tap filter on
// white noise. The difference is the sampler's sub-texel weight precision,
// which a fetch on a texel centre never meets and the merged one does.
//
// `crcbl_shaders::upscale::merged_pair_offset` mirrors `merged_pair_offset`
// below, and its tests show the three-tap row reproducing the four-tap one —
// at the border too, under the same clamp-to-edge the sampler applies.
//
// # Determinism
//
// Two cubic polynomials, a `floor`, a division per axis and nine weighted
// fetches: no transcendental function anywhere, which is this workspace's rule
// for arithmetic that reaches a colour. The fetches are a constant 3x3 pattern
// with no branch in the loop, so every pixel executes the same instructions in
// the same order.
//
// # No `#include`
//
// `bloom_composite.slang` states the rule: the manifest hashes one source per
// artifact, so an include would be a file whose edits nothing downstream
// notices. The fullscreen triangle below is `tonemap.slang`'s, copied.

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
    /// dividing once per fetch.
    float2 inv_source;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

/// The resolved frame at the internal render extent.
[[vk::binding(0, 0)]]
Texture2D<float4> source D3D12_REGISTER(t0, space0);

/// A **linear** sampler with `ClampToEdge`, and the renderer creates it that
/// way. Both halves are load-bearing.
///
/// Linear filtering *is* the middle tap of each axis: the fetch between the
/// two middle texels returns their weighted pair only because the sampler
/// interpolates them, and a nearest sampler would snap it onto one texel and
/// break the kernel everywhere. The outer fetches land on texel centres, where
/// a linear sampler returns that texel alone.
///
/// Clamp-to-edge is what the border gets. Every fetch is addressed by UV, so
/// the ring outside the source is what `ClampToEdge` decides: an outer fetch
/// past the edge returns the edge texel, and a merged fetch straddling the
/// edge interpolates the edge texel with its own clamped copy — which is the
/// edge texel again — exactly as the two separate taps it replaces would.
[[vk::binding(1, 0)]]
SamplerState sourceSampler D3D12_REGISTER(s1, space0);

[[vk::binding(2, 0)]]
ConstantBuffer<UpscaleParams> params D3D12_REGISTER(b2, space0);

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

/// How far past the first middle texel the one fetch standing in for the two
/// middle taps goes, in texels, for the weights `w` of one axis.
///
/// `w.y * a + w.z * b` is `(w.y + w.z) * lerp(a, b, w.z / (w.y + w.z))`, and
/// a linear sampler computes that `lerp`. The two middle weights are both
/// positive for every `f` in `[0, 1)` and sum to at least one — the outer pair
/// is never positive and the four sum to one — so the division is always
/// defined and the offset always lies in `[0, 1]`, between the two texels the
/// sampler interpolates.
float merged_pair_offset(float4 w)
{
    return w.z / (w.y + w.z);
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

    // Per axis, three fetches: the outer texel on each side at its centre, and
    // one between the two middle texels carrying both their weights. The +0.5
    // undoes the -0.5 above, turning a texel-centre coordinate back into UV.
    float3 tx = float3(wx.x, wx.y + wx.z, wx.w);
    float3 ty = float3(wy.x, wy.y + wy.z, wy.w);
    float3 ux = (base.x + float3(-1.0, merged_pair_offset(wx), 2.0) + 0.5) * params.inv_source.x;
    float3 uy = (base.y + float3(-1.0, merged_pair_offset(wy), 2.0) + 0.5) * params.inv_source.y;

    float3 sum = float3(0.0, 0.0, 0.0);
    for (int j = 0; j < 3; ++j)
    {
        for (int i = 0; i < 3; ++i)
        {
            float3 tap = source.SampleLevel(sourceSampler, float2(ux[i], uy[j]), 0.0).rgb;
            sum += tap * (tx[i] * ty[j]);
        }
    }

    // The ringing clamp, and alpha 1.0 on the way out: the swapchain is
    // composited opaque, exactly as `tonemap.slang` says.
    return float4(saturate(sum), 1.0);
}
