// crcbl-targets: spirv, wgsl, msl, dxil

// The antialiasing ladder's second rung (`docs/notes/rendering.md`), first of
// the three passes CMAA2 records: the edge detect.
//
// # What is transcribed here, and what is this tree's own
//
// The algorithm is Intel's **Conservative Morphological Anti-Aliasing 2**
// (Filip Strugar, 2018). Its stages are the reference's own, and they are
// reproduced across the three `cmaa2_*`
// sources in this directory:
//
//  1. an **edge detect** — a luma delta per pixel boundary against a threshold,
//     with a **local-contrast adaptation** that drops the weaker half of a pair
//     of neighbouring boundaries — which writes one edge word per pixel and
//     zeroes that pixel's accumulation (this file);
//  2. a **shape classification** — the 2×2 `Z` and `U` shapes and the bounded
//     **long-edge search** along the run they belong to — which adds each
//     covered pixel's share straight into the accumulation
//     (`cmaa2_shapes.slang`);
//  3. the **deferred colour apply**, which resolves that accumulation into the
//     target (`cmaa2_apply.slang`).
//
// **What is not transcribed is the reference's tuned constants.** This engine
// has no network access to the reference source, and a constant recalled from
// memory is a number nothing here could check. So the shape *rules* are
// analytic — the coverage a straight line reconstructed across a run leaves
// over each of its pixels, which is the morphological family's own derivation
// and is written out at the site — and the two free numbers, `EDGE_THRESHOLD`
// and `LOCAL_CONTRAST_ADAPTATION_FACTOR`, are the ones this tree already
// measured for SMAA on the same fixture. `crates/crcbl/tests/mesh_e2e/cmaa2.rs`
// is what decides whether they hold, and `docs/backlog.md` records that they
// are this transcription's and not the reference's.
//
// # There is no candidate list and no blend-item list
//
// An earlier shape of this tier appended edge pixels and blend items to storage
// buffers through atomic counters, sized as a fraction of the frame, and
// **dropped** whatever arrived past the capacity. Which entries won the room
// was the device's scheduling of the work-groups that produced them, so a frame
// dense enough in edges to fill a list was a picture that was a function of the
// schedule rather than of its inputs —
// `crates/crcbl/tests/mesh_e2e/cmaa2.rs`'s
// `a_dense_edge_frame_resolves_to_the_same_bytes_every_time` reproduced it as
// thousands of pixels moving between runs of one frame. Nothing is appended
// now: `cmaa2_shapes.slang` runs one invocation per **pixel** and tests the
// same predicate this pass used to append on, and every share it derives goes
// straight into the accumulation as an integer atomic add. Integer addition
// commutes, so an accumulation that never drops is order-independent by
// construction.
//
// # Compute, not a fullscreen triangle
//
// **A linear dispatch over the frame** rather than a two-dimensional one, on
// `exposure.slang`'s terms: the workgroup is the one constant every compute
// pass here declares and the texel is recovered with a divide and a modulo. The
// resolve is a draw because it writes the caller's target, which is the one
// thing a compute pass cannot do — `cmaa2_apply.slang` says why.
//
// # The luma is corrected, because the image reaching here is linear
//
// `tonemap.slang` writes linear values into a target the hardware encodes, so a
// fetch of that target decodes straight back to linear — and an edge threshold
// wants display space, where equal steps look equally different. `luma_of`
// corrects it with one `sqrt`, exactly as `fxaa.slang` and the retired
// `smaa_edges.slang` did; `fxaa.slang`'s header carries the argument at length.
//
// The post stack's "pass fusion" would hand this pass the luma in the
// tonemap's alpha channel and save the fetches. That rung has not landed —
// `tonemap.slang` writes an opaque alpha — so this pass computes its own, and
// `docs/backlog.md` carries the fusion.
//
// # No `#include`
//
// `bloom_composite.slang` states the rule: the manifest hashes one source per
// artifact, so an include would be a file whose edits nothing downstream
// notices. The three `cmaa2_*` sources therefore each carry their own copy of
// the parameter block, and the two that share a bind-group layout declare the
// same four resources in the same order.

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

/// Everything the CMAA2 passes take from the CPU.
///
/// `crcbl_shaders::cmaa2::Cmaa2Params` is the Rust mirror and
/// `crcbl_render::cmaa2` is what writes the buffer, once per frame. One block
/// for every pass, on the retired SMAA tier's terms: they run at one extent and
/// a second block would be a second thing to keep in step.
struct Cmaa2Params
{
    /// Width of the image being filtered, in texels. Every buffer this tier
    /// owns is indexed `y * viewport_x + x`.
    uint viewport_x;
    /// Its height.
    uint viewport_y;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source. This
// file and `cmaa2_shapes.slang` declare the same four resources in the same
// order because one bind-group layout serves both of their compute entry
// points — `crcbl_render::cmaa2` builds exactly one.

[[vk::binding(0, 0)]]
ConstantBuffer<Cmaa2Params> params D3D12_REGISTER(b0, space0);

/// The tonemapped frame, at this tier's own extent.
///
/// `Texture2D<float4>` read by `Load` and with no sampler beside it: every
/// fetch this tier makes is a whole texel read for its own value, which is what
/// a filtered tap would destroy.
[[vk::binding(1, 0)]]
Texture2D<float4> source D3D12_REGISTER(t1, space0);

/// `[pixel]`: which of a pixel's two owned boundaries carry an edge.
///
/// Bit 0 is `EDGE_LEFT` — the boundary between this pixel and the one to its
/// west — and bit 1 is `EDGE_TOP`, the boundary to its north. A boundary
/// belongs to exactly one of the two pixels either side of it, and taking the
/// west and north sides is the morphological convention throughout.
///
/// Written whole by `edgesMain`, so nothing clears it.
[[vk::binding(2, 0)]]
RWStructuredBuffer<uint> edges D3D12_REGISTER(u2, space0);

/// `[pixel * ACCUM_WORDS]`: the fixed-point accumulation, three colour words
/// and a weight word per pixel.
///
/// Zeroed here rather than in a pass of its own: a pixel's four words are
/// written by the one invocation of this dispatch that owns that pixel, so
/// there is no race to separate out, and `cmaa2_shapes.slang` — the only thing
/// that adds to them — runs in a later dispatch.
[[vk::binding(3, 0)]]
RWStructuredBuffer<Atomic<uint> > accum D3D12_REGISTER(u3, space0);

/// Invocations per workgroup, matching `crcbl_shaders::cmaa2::WORKGROUP_SIZE`.
static const uint WORKGROUP_SIZE = 64;

/// Words per pixel in `accum`, matching `crcbl_shaders::cmaa2::ACCUM_WORDS`.
static const uint ACCUM_WORDS = 4;

/// Bit 0 of an `edges` word: an edge on the pixel's west boundary.
static const uint EDGE_LEFT = 1;

/// Bit 1: an edge on its north boundary.
static const uint EDGE_TOP = 2;

/// The luma delta across a pixel boundary, in display space, below which there
/// is no edge there — `crcbl_shaders::cmaa2::EDGE_THRESHOLD`.
///
/// **This tree's number rather than the reference's.** It is what
/// `smaa_edges.slang` used on the same fixture before it was retired, and the
/// observer in `crates/crcbl/tests/mesh_e2e/cmaa2.rs` is what holds it.
static const float EDGE_THRESHOLD = 0.1;

/// How much stronger a neighbouring boundary has to be before this one is
/// dropped — `crcbl_shaders::cmaa2::LOCAL_CONTRAST_ADAPTATION_FACTOR`.
///
/// The local-contrast adaptation CMAA2 and SMAA both carry, in the form this
/// tree measured: an edge whose neighbour has more than this much more contrast
/// is the quiet half of a pair and is not marked, so a step gets one line of
/// edge pixels rather than two. The factor is SMAA's own; the reference's is
/// not something this transcription can check.
static const float LOCAL_CONTRAST_ADAPTATION_FACTOR = 2.0;

/// Perceptual luma of a linear colour.
///
/// Rec. 709 weights, then one `sqrt` standing in for the sRGB encode. The
/// header says why the correction is here and what it approximates.
float luma_of(float3 color)
{
    return sqrt(dot(color, float3(0.2126, 0.7152, 0.0722)));
}

/// Luma of `texel`, clamped to the image.
///
/// Clamping rather than a border colour: a boundary at the frame's own edge has
/// no outside to step to, and repeating the edge texel makes that delta zero,
/// which is the answer that marks no edge there.
float luma_at(int2 texel)
{
    int2 inside = clamp(texel, int2(0, 0), int2(int(params.viewport_x) - 1, int(params.viewport_y) - 1));
    return luma_of(source.Load(int3(inside, 0)).rgb);
}

/// One invocation per pixel: mark this pixel's two boundaries and clear its
/// accumulation.
///
/// The two boundaries a pixel owns are its west and north ones, so the first
/// column can carry no `EDGE_LEFT` and the first row no `EDGE_TOP` — there is
/// no pixel on the other side for the boundary to separate it from.
[shader("compute")]
[numthreads(64, 1, 1)]
void edgesMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    uint pixels = params.viewport_x * params.viewport_y;
    if (index >= pixels)
    {
        return;
    }
    uint x = index % params.viewport_x;
    uint y = index / params.viewport_x;
    int2 texel = int2(int(x), int(y));

    // The accumulation this pixel owns, zeroed before any share can reach it —
    // the pass that adds to them runs after this one.
    for (uint word = 0; word < ACCUM_WORDS; ++word)
    {
        accum[index * ACCUM_WORDS + word].store(0);
    }

    float c = luma_at(texel);
    float w = luma_at(texel + int2(-1, 0));
    float n = luma_at(texel + int2(0, -1));
    float e = luma_at(texel + int2(1, 0));
    float s = luma_at(texel + int2(0, 1));

    // The west and north deltas, which are the two boundaries this pixel owns.
    float2 delta = abs(c - float2(w, n));
    float2 marked = step(float2(EDGE_THRESHOLD, EDGE_THRESHOLD), delta);

    // Local contrast adaptation: the strongest delta in the neighbourhood
    // against this pixel's own two.
    float2 other = abs(c - float2(e, s));
    float strongest = max(max(delta.x, delta.y), max(other.x, other.y));
    marked *= step(float2(strongest, strongest), LOCAL_CONTRAST_ADAPTATION_FACTOR * delta);

    uint bits = 0;
    if (x > 0 && marked.x > 0.0)
    {
        bits |= EDGE_LEFT;
    }
    if (y > 0 && marked.y > 0.0)
    {
        bits |= EDGE_TOP;
    }
    edges[index] = bits;
}
