// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/43-render-standards.md` §6's auto-exposure, first half: the
// luminance histogram of the scene target, built on the GPU with no readback.
//
// # Why a histogram and not an average
//
// A mean luminance is one reduction and it is the wrong statistic: a sky, a
// lamp or a specular highlight is orders of magnitude above everything a player
// is looking at, and a mean chases it — the picture darkens because one bright
// object entered frame. Every engine that ships auto-exposure bins the frame
// and takes a *quantile* of the population instead, which is stable under
// exactly that. This pass is the binning; the reduce that reads the bins is
// `docs/plan/48-post-processing.md`'s next rung.
//
// # The bins are read out of the float's exponent, not out of a `log2`
//
// A histogram over luminance has to be logarithmic — the interesting range is
// four or five orders of magnitude — and this workspace's rule is that no
// transcendental reaches a colour, because four platforms disagree in the last
// place. `log2` would be defensible here on `mesh.slang`'s `froxel_of` grounds
// (its result is floored into an integer, so a disagreement only moves a value
// sitting exactly on a bin edge), but the exponent field of an IEEE-754 float
// *is* the floor of its own base-two logarithm, and reading it is integer
// arithmetic: exact, identical on every target, and cheaper than the intrinsic
// it replaces. Two mantissa bits below it split each octave into four, which is
// the resolution `crcbl_shaders::exposure::BINS_PER_OCTAVE` documents.
//
// So there is no approximation to bless and no error term to bound. The host
// mirror in `crcbl_shaders::exposure::bin_of` is the same integer expression
// and lands on the same bin for every finite input, which is what
// `crcbl`'s `mesh_e2e/exposure.rs` checks against a real frame.
//
// # Three entry points, in the order a frame runs them
//
// `clearMain` zeroes the bins, `histogramMain` fills them, and `reduceMain`
// turns them into the one number the tonemap applies. The clear is a pass of
// its own on `clear_counters.slang`'s terms exactly: the accumulation is an
// atomic add, so the frame before it has to be cleared out first, and a compute
// dispatch is the only clear this engine's seam has for a storage buffer.
//
// All three run inside the frame they are about — the histogram reads the scene
// colour the passes before it produced, and the tonemap reads the exposure the
// reduce wrote — so nothing here is a frame behind and nothing is read back to
// the host. What it does not do yet is *adapt*: the exposure a frame is drawn
// with is the exposure that frame asked for, with no time constant on it, so a
// cut between two differently-lit shots lands in one frame rather than over a
// few tenths of a second. `docs/plan/48-post-processing.md` carries that as the
// next rung, and it is the one that needs a value to survive between frames.

/// What `crcbl_shaders::exposure::ExposureParams` writes: the extent of the
/// image being binned, and how far this frame's exposure is allowed to move.
///
/// The scene target's extent rather than the swapchain's, because this pass
/// runs before the upscale — see `crcbl_render::exposure`.
///
/// The two blends are already `rate * delta`, clamped to `[0, 1]` by the host:
/// this pass has no clock, and the multiply belongs where the frame time is. A
/// blend of one is the whole distance in one frame, which is the picture this
/// pass drew before adaptation existed, and it is what a block that says
/// nothing about adaptation carries.
struct ExposureParams
{
    uint viewport_x;
    uint viewport_y;
    float brighten_blend;
    float darken_blend;
};

[[vk::binding(0, 0)]]
ConstantBuffer<ExposureParams> params;

/// The frame as the passes before this one left it: linear `Rgba16Float`, HDR,
/// before the tonemap maps it into the swapchain.
///
/// `Texture2D<float4>` read by `Load`, so there is no sampler beside it — a
/// histogram wants each texel exactly once, and a filtered tap would bin an
/// average of four texels that is not any texel's value.
[[vk::binding(1, 0)]]
Texture2D<float4> scene;

/// `[bin]`: how many texels of the frame landed in each bin, matching
/// `crcbl_shaders::exposure::BIN_COUNT`.
///
/// `Atomic<uint>` rather than a plain `uint` with `InterlockedAdd`, for
/// `cull.slang`'s reason: WGSL has no such intrinsic and refuses the entry
/// point, while the type lowers on all four targets.
[[vk::binding(2, 0)]]
RWStructuredBuffer<Atomic<uint> > histogram;

/// `[0]`: the exposure `reduceMain` measured, and the value `tonemap.slang`
/// applies on a frame whose block says auto-exposure is on.
///
/// A buffer rather than a value handed back to the host, because a readback is
/// a stall: the reduce runs in the same frame as the histogram it reads and the
/// tonemap that reads it, so the exposure a frame is drawn with is measured
/// from that frame and never from the one before it.
[[vk::binding(3, 0)]]
RWStructuredBuffer<float> measured;

/// `[0]`: what the frame before this one was exposed by, which is the same ring
/// one slot back — `crcbl_render::exposure` binds it.
///
/// Read-only, and a *different* buffer from `measured` on every device with
/// more than one frame in flight, so this pass never reads a slot it is also
/// writing. `crcbl_render::exposure` fills the whole ring with
/// `DEFAULT_EXPOSURE` before any frame exists, which is what makes the first
/// frame's adaptation start somewhere defined rather than from whatever the
/// allocation came with.
[[vk::binding(4, 0)]]
StructuredBuffer<float> previous;

/// Bins per octave, matching `crcbl_shaders::exposure::BINS_PER_OCTAVE`.
static const uint BINS_PER_OCTAVE = 4;

/// How far the top of the mantissa is shifted down to become the sub-octave
/// index: 23 mantissa bits less the two this split uses.
static const uint MANTISSA_SHIFT = 21;

/// The exponent the first bin starts at, matching
/// `crcbl_shaders::exposure::MIN_EXPONENT`.
static const int MIN_EXPONENT = -12;

/// Bins in the buffer, matching `crcbl_shaders::exposure::BIN_COUNT`.
static const uint BIN_COUNT = 96;

/// The factor from a bin's lower edge to the luminance `reduceMain` weights it
/// at, matching `crcbl_shaders::exposure::BIN_CENTRE`: the geometric centre of a
/// quarter-octave bin, written out because computing it would be an `exp2` in
/// the arithmetic that produces the exposure.
static const float BIN_CENTRE = 1.0905077;

/// Middle grey, matching `crcbl_shaders::exposure::KEY`: the luminance the
/// measured average is exposed to.
static const float KEY = 0.18;

/// Where the reduce starts counting, matching
/// `crcbl_shaders::exposure::LOW_FRACTION`.
static const float LOW_FRACTION = 0.5;

/// Where it stops, matching `crcbl_shaders::exposure::HIGH_FRACTION`.
static const float HIGH_FRACTION = 0.95;

/// The darkest exposure the reduce will write, matching
/// `crcbl_shaders::exposure::MIN_EXPOSURE` and `crcbl_render::EXPOSURE_MIN`.
static const float MIN_EXPOSURE = 0.03125;

/// The brightest, matching `crcbl_shaders::exposure::MAX_EXPOSURE`.
static const float MAX_EXPOSURE = 32.0;

/// What a frame with nothing in the window is exposed by, matching
/// `crcbl_shaders::tonemap::DEFAULT_EXPOSURE`: the picture with no
/// auto-exposure in it at all.
static const float DEFAULT_EXPOSURE = 1.0;

/// Rec. 709 relative luminance of a **linear** colour, the same weights
/// `bloom_down.slang` declares.
///
/// Written out again rather than shared, because this repo has no include
/// mechanism by design — `crcbl_shaders::exposure`'s source guard holds the two
/// copies to the same constants.
float luma(float3 color)
{
    return dot(color, float3(0.2126, 0.7152, 0.0722));
}

/// The bin a luminance falls in: the octave above `MIN_EXPONENT` it sits in,
/// split four ways by the top of its mantissa.
///
/// Everything below the first bin — zero, a denormal, a negative value the
/// filtering upstream produced — lands in bin 0, and everything above the last,
/// an infinity included, lands in the last. The clamp is what makes the buffer
/// index safe rather than a bounds check somewhere else, and a saturating end
/// bin is also the right *model*: the reduce below wants "how much of the frame
/// is brighter than this", and a value past the top of the range is exactly
/// that.
uint bin_of(float luminance)
{
    uint bits = asuint(luminance);
    // The sign bit rides into the exponent here, so any negative value lands
    // far above the last bin rather than aliasing onto a positive one; the
    // clamp then puts it in the top bin. Negatives do not reach a frame this
    // engine draws, and this is what happens if one ever does.
    int exponent = int(bits >> 23) - 127;
    uint fraction = (bits >> MANTISSA_SHIFT) & (BINS_PER_OCTAVE - 1);
    int index = (exponent - MIN_EXPONENT) * int(BINS_PER_OCTAVE) + int(fraction);
    return uint(clamp(index, 0, int(BIN_COUNT) - 1));
}

/// Zeroes every bin, one invocation per bin.
///
/// Dispatched over `BIN_COUNT` rather than over the frame, so the extent is a
/// constant and the guard below is the only thing standing between a rounded-up
/// group count and a write past the end.
[shader("compute")]
[numthreads(64, 1, 1)]
void clearMain(uint3 thread: SV_DispatchThreadID)
{
    if (thread.x >= BIN_COUNT)
    {
        return;
    }
    histogram[thread.x].store(0);
}

/// The lower edge of a bin, built by writing its exponent field directly.
///
/// The inverse of `bin_of` on the values that survive it, and
/// `crcbl_shaders::exposure::bin_luminance` is the same shift. `mesh.slang`'s
/// `asfloat(uint(127 - int(n)) << 23)` is the same trick for the same reason: a
/// power of two written into the exponent is exact on every target, where the
/// intrinsic that computes it is exact on none of them.
float bin_luminance(uint bin)
{
    int exponent = MIN_EXPONENT + int(bin / BINS_PER_OCTAVE);
    uint fraction = (bin % BINS_PER_OCTAVE) << MANTISSA_SHIFT;
    return asfloat((uint(exponent + 127) << 23) | fraction);
}

/// The exposure the histogram asks for, written to `measured[0]`.
///
/// **One invocation for the whole reduce**, and that is a decision rather than
/// an oversight. The buffer is ninety-six words — a parallel reduction over it
/// would be a workgroup, a barrier and a tree, and it would sum the bins in an
/// order that depends on how a device schedules its lanes. Float addition is
/// not associative, so that order is visible in the last place of the exposure,
/// and the exposure multiplies every texel of the frame. A serial loop is the
/// same arithmetic in the same order on all four backends, which is what lets
/// `crcbl_shaders::exposure::measure` predict it and what lets a golden frame
/// hold still.
///
/// The window is the population between `LOW_FRACTION` and `HIGH_FRACTION` of
/// the frame, weighted at each bin's own luminance. Bin 0 — everything below
/// the histogram's floor — is left out: it is the black background, and
/// including it would expose for how much of the frame is empty.
///
/// **What the frame is exposed by is not that number but a step toward it**,
/// from what the frame before was exposed by, a fraction `blend` of the way.
/// The eye takes seconds to adapt and a cut that re-exposes in one frame reads
/// as a flicker; `crcbl_shaders::exposure::adapt` is this arithmetic on the
/// CPU, and the two blends the block carries are why the step is not the same
/// size in both directions.
///
/// **The two endpoints are exact rather than nearly so.** `prior + (target -
/// prior) * 1` is not `target` in floating point, so a blend of one — the
/// default, and the frame this pass drew before adaptation existed — takes the
/// branch that writes the target itself, and a blend of zero writes the prior
/// itself. Only a blend strictly between them interpolates.
[shader("compute")]
[numthreads(1, 1, 1)]
void reduceMain()
{
    float target = DEFAULT_EXPOSURE;
    uint total = 0;
    for (uint bin = 1; bin < BIN_COUNT; ++bin)
    {
        total += histogram[bin].load();
    }
    if (total > 0)
    {
        uint low = uint(float(total) * LOW_FRACTION);
        uint high = uint(float(total) * HIGH_FRACTION);
        uint seen = 0;
        float weighted = 0.0;
        float population = 0.0;
        for (uint bin = 1; bin < BIN_COUNT; ++bin)
        {
            uint start = seen;
            seen += histogram[bin].load();
            uint lower = max(start, low);
            uint upper = min(seen, high);
            if (upper > lower)
            {
                float part = float(upper - lower);
                weighted += part * bin_luminance(bin) * BIN_CENTRE;
                population += part;
            }
        }
        if (population > 0.0)
        {
            target = clamp(KEY / (weighted / population), MIN_EXPOSURE, MAX_EXPOSURE);
        }
    }

    float prior = previous[0];
    float rate = target > prior ? params.brighten_blend : params.darken_blend;
    float blend = clamp(rate, 0.0, 1.0);
    if (blend >= 1.0)
    {
        measured[0] = target;
    }
    else if (blend <= 0.0)
    {
        measured[0] = prior;
    }
    else
    {
        measured[0] = clamp(prior + (target - prior) * blend, MIN_EXPOSURE, MAX_EXPOSURE);
    }
}

/// One invocation per texel of the scene target, each adding one to its bin.
///
/// **A linear dispatch over the frame rather than a two-dimensional one**, so
/// the workgroup is the `WORKGROUP_SIZE` every other compute pass in this
/// engine declares and `crcbl_render` sizes it from the same constant. The
/// texel is recovered with a divide and a modulo, which is two integer
/// operations against a bound that would otherwise be two dispatch axes to keep
/// in step with two extents.
///
/// **Straight to the buffer, with no workgroup-local tile in front of it.** The
/// tile is the standard optimisation and it is deliberately not here yet: a
/// module-scope `groupshared` changes what Slang's Metal backend emits for
/// every entry point in the file — `zero_dispatch_probe.slang` carries what
/// that cost the mesh-shader sources — so it is a change to make with a profile
/// in hand rather than on the way past. `crcbl_render::timing` is what would
/// show it.
[shader("compute")]
[numthreads(64, 1, 1)]
void histogramMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= params.viewport_x * params.viewport_y)
    {
        return;
    }
    uint2 texel = uint2(index % params.viewport_x, index / params.viewport_x);
    float3 color = scene.Load(int3(int2(texel), 0)).rgb;
    histogram[bin_of(luma(color))].add(1);
}
