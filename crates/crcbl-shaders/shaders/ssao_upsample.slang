// crcbl-targets: spirv, wgsl, msl, dxil

// The third of the occlusion passes: the half-resolution channel `ssao.slang`
// gathered and `ssao_blur.slang` filtered, reconstructed at the scene's own
// extent for the forward pass to bind.
//
// # Why the pair runs small and this pass exists
//
// Occlusion is a low-frequency field, so gathering it per pixel spends four
// horizon marches where one carries almost the same picture. `ssao.slang`'s
// header is where that argument is made and [`RESOLUTION_DIVISOR`] is the factor
// it is made at; this file is the other half of the trade, because a quarter of
// the marches is only a saving if putting the channel back does not cost what
// was saved.
//
// # A bilinear upsample would draw a halo, and that is the whole design
//
// The samples this pass reads sit on a coarser grid than the pixels it writes,
// and a silhouette does not: a foreground pixel one texel from the edge of an
// object has half-resolution neighbours on the background behind it. Averaging
// those in with a distance weight alone is a rim of the wall's occlusion drawn
// around the object standing in front of it — the same halo `ssao_blur.slang`
// rejects taps to avoid, arriving through the upsample instead.
//
// So each tap is weighted by **both**: how near it is to this pixel, and how
// near its surface is. The second weight is `ssao_blur.slang`'s, ramp for ramp
// and constant for constant — see [`DEPTH_TOLERANCE_RADII`] — because it is
// answering the same question about the same two images, and two spellings of
// one filter is two things to keep in step.
//
// # What that leaves for the blur to do
//
// Less than it used to, and deliberately. A depth-aware upsample is most of what
// a depth-weighted blur was already doing, so the two together now smooth the
// dither tile at the resolution it exists at and then reconstruct across the
// silhouettes it does not cross. `ssao_blur.slang` is still not optional and its
// header still says why: its footprint is exactly `ssao.slang`'s tile, measured
// in the pixels the two of them run at, and nothing here widens to a tile.
//
// # Determinism
//
// The rule the pair is built on survives this pass intact. There is no noise, no
// hash and no filtering sampler: every fetch is a `Load` at an integer texel,
// every weight is a ratio of exactly-representable numbers, and the one
// comparison that decides anything is against [`DEPTH_FAR`] — an equality
// against an exact constant, which is the comparison `ssao_blur.slang` argues is
// the acceptable kind.

/// Everything this pass needs that is not in a texture, and it is
/// `ssao.slang`'s block — same members, same order, same buffer.
///
/// Declared a third time rather than included, on this repo's terms and
/// `ssao_blur.slang`'s paragraph: the manifest hashes one source per artifact,
/// so an `#include` would be a file whose edits nothing downstream notices.
/// `crcbl_shaders::ssao::SsaoParams` is the Rust mirror all three agree with,
/// and `crcbl_render::ssao` is what binds the one buffer to all three passes.
///
/// **Bound as `camera`** for the reason `ssao.slang` gives at the same
/// declaration: `depth_at` and `view_z` are copied between these files verbatim,
/// and the guard that holds the copies together compares their bodies as text.
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
    /// `ssao.slang` swept, which nothing here reads, and `z` is the exponent
    /// this pass raises its answer to — see [`ao_intensity`], the only reader
    /// of it in the chain. `w` pads the row.
    float4 params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsaoParams> camera;

/// The filtered occlusion `ssao_blur.slang` wrote, at the pair's own extent and
/// fetched by integer pixel.
///
/// `Texture2D<float>` and not `DepthTexture2D`, and **no sampler**: both are
/// `ssao_blur.slang`'s bindings exactly, for that file's reasons — this is an
/// ordinary single-channel `R8Unorm` colour image, and a filtering sampler is
/// four rasterisers rounding a texel centre their own way.
///
/// The three bindings of this pass are that file's three, in its order, which is
/// what lets `crcbl_render::ssao` build one bind group layout for both.
[[vk::binding(1, 0)]]
Texture2D<float> occlusion;

/// The depth prepass's output, at the **scene's** extent rather than the pair's.
///
/// This is the image the two grids are reconciled against: the pixel's own depth
/// comes from it and so does each tap's, through [`full_res_pixel`]. Bound as
/// `DepthTexture2D` for `ssao.slang`'s reason — the two spellings are one HLSL
/// declaration and are not the same WGSL, and only this one lowers to a
/// `texture_depth_2d` a `D32Float` view can fill.
[[vk::binding(2, 0)]]
DepthTexture2D scene_depth;

/// The reversed-Z far plane: `crcbl_hal::depth::CLEAR`, and the value
/// `ssao.slang` and `ssao_blur.slang` declare under this name.
///
/// **The far plane has no surface**, so it appears twice below and means the
/// same thing both times. A far *pixel* leaves early: `ssao.slang` wrote 1.0
/// there and there is no view-space position to weight anything against. A far
/// *tap* takes no share of the reconstruction, because a sample of the sky
/// averaged into the rim of what stands in front of it is the bright fringe
/// `ssao_blur.slang` names.
///
/// `crcbl_shaders::ssao::DEPTH_FAR` is the Rust mirror of all three.
static const float DEPTH_FAR = 0.0;

/// The factor the occlusion chain's gather extent is divided by, on each axis:
/// the constant `ssao.slang` declares under this name, and the argument for it
/// is that file's header.
static const int RESOLUTION_DIVISOR = 2;

/// How far a tap's view-space depth may sit from this pixel's before it counts
/// for nothing, as a share of `ssao.slang`'s sampling radius.
///
/// **`ssao_blur.slang`'s constant, and it has to be that one.** That file
/// carries the argument in full: the radius is the only length the occlusion
/// passes have, it is already their answer to "are these two pixels near enough
/// to be occluding each other", and a tuned number here would be a second
/// filter over the same two images disagreeing with the first about what a
/// surface is. `crcbl_shaders::ssao`'s
/// `the_two_occlusion_filters_share_one_depth_tolerance` is what holds the two
/// declarations together.
static const float DEPTH_TOLERANCE_RADII = 2.0;

/// The share the nearest half-resolution sample keeps however far its surface
/// turns out to be.
///
/// **Something has to keep the divisor off zero.** `ssao_blur.slang` does it by
/// giving its centre tap a share of one by construction, and this pass cannot:
/// its nearest sample is a *neighbouring* texel wherever the two grids do not
/// coincide, and a neighbour given an unconditional full share is precisely the
/// halo the depth weight is here to prevent.
///
/// So the nearest sample is never rejected outright — it is only ever outvoted.
/// Any tap that weighed in normally overwhelms this share by orders of
/// magnitude, which moves a pixel whose neighbourhood answered at all by a small
/// fraction of one level of the `R8Unorm` channel this lands in. What it buys is
/// the pixel whose neighbourhood answered not at all: a surface thinner than the
/// occlusion grid, with no sample of its own to recover, where the honest answer
/// is the nearest sample there is. It arrives there by the other ramps running
/// out rather than by a branch testing whether they did — a branch on a weight
/// that reached zero would put a binary decision on the output pixel, which is
/// what these headers spend their length keeping out.
///
/// A negative power of two, so the share and every ratio it forms are exact.
static const float NEAREST_TAP_FLOOR = 1.0 / 4096.0;

/// The exponent a frame that asked for nothing gets, and the one exponent that
/// changes nothing: `v` raised to it is `v`, for every visibility this pass can
/// reconstruct.
///
/// `crcbl_shaders::ssao::INTENSITY_DEFAULT` is the Rust mirror, and
/// `crcbl_render::ssao::r_ssao_intensity` is what a frame sets it from.
static const float INTENSITY_DEFAULT = 1.0;

/// The weakest exponent [`ao_intensity`] will answer with.
///
/// A quarter, which is [`INTENSITY_MAX`] reciprocated — the one symmetry an
/// exponent has, since `k` and `1/k` are the two curves that undo each other.
/// The argument for the number is the curve's slope at an unoccluded surface,
/// which is the exponent itself: at a quarter it takes four levels of the
/// `R8Unorm` channel `ssao_blur.slang` wrote to move this one by a level, so
/// below it the pass is discarding the measurement it just made.
///
/// **Zero is not the bottom of the range and cannot be.** `v` raised to zero is
/// one everywhere, which is a frame with no occlusion in it at all — see
/// [`ao_intensity`], which is where a zero is turned away rather than clamped
/// up to this.
static const float INTENSITY_MIN = 0.25;

/// The strongest exponent [`ao_intensity`] will answer with.
///
/// Four, by [`INTENSITY_MIN`]'s argument from the other end: the slope at an
/// unoccluded surface is the exponent, so four is where one level of the
/// channel the blur wrote becomes four levels of this one, and the terracing of
/// a half-resolution `R8Unorm` gather is what an artist would then be looking
/// at rather than the occlusion.
static const float INTENSITY_MAX = 4.0;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The full-resolution texel a pixel of the occlusion chain's gather stands
/// for.
///
/// `ssao.slang`'s function and its whole argument: the top-left of the block,
/// mapped the same way by every pass of the chain, so this one knows where each
/// half-resolution sample sits without either side writing the rule down twice.
int2 full_res_pixel(int2 pixel)
{
    return pixel * RESOLUTION_DIVISOR;
}

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
/// `ssao_blur.slang`'s function, and the reason this pass needs it is that
/// file's: a reversed-Z difference is not a distance, so a tolerance on the raw
/// value would be a different filter in every part of the frame. `extent` is the
/// **depth** image's, which is not this pass's target extent and not the
/// occlusion image's either.
float view_z(int2 pixel, float depth, float2 extent)
{
    float2 ndc = float2(
        (float(pixel.x) + 0.5) / extent.x * 2.0 - 1.0,
        1.0 - (float(pixel.y) + 0.5) / extent.y * 2.0);
    float4 view = mul(camera.inv_proj, float4(ndc, depth, 1.0));
    return view.z / view.w;
}

/// The exponent this frame asked the reconstructed visibility be raised to.
///
/// `camera.params.z`, which the host writes from a console variable — see
/// `crcbl_render::ssao::r_ssao_intensity`. The horizons measured the occlusion
/// and this is what lets an artist ask for more of it than they found: **a
/// power and not a blend towards one**, because a blend can only lift the
/// answer back towards unoccluded and the frame this knob exists for wants it
/// darker than the integral came out.
///
/// **Zero is the default and not the floor.** A producer that never writes the
/// field leaves the padding this word used to be, which reads as zero, and zero
/// is the one exponent that cannot be honoured: every visibility raised to it
/// is one, so the frame arrives with no occlusion in it whatsoever. A clamp on
/// its own would answer [`INTENSITY_MIN`] there — a picture nobody asked for
/// either — so the zero is answered with [`INTENSITY_DEFAULT`], which is the
/// frame this file drew before the field existed. `ssao.slang`'s `slice_count`
/// turns its own unwritten field away for that reason and this is the same
/// reasoning applied to a value whose degenerate case is at the other end.
float ao_intensity()
{
    float asked = camera.params.z;
    return asked == 0.0 ? INTENSITY_DEFAULT : clamp(asked, INTENSITY_MIN, INTENSITY_MAX);
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
    // **Three extents, and every one of them is read off its own image.** The
    // occlusion is the pair's, the depth is the scene's, and the target is the
    // scene's as well — which is why the pixel below needs no scaling and the
    // taps do.
    uint width;
    uint height;
    occlusion.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    uint depth_width;
    uint depth_height;
    scene_depth.GetDimensions(depth_width, depth_height);
    int2 depth_extent = int2(int(depth_width), int(depth_height));
    float2 depth_size = float2(float(depth_width), float(depth_height));

    int2 pixel = int2(input.position.xy);
    float centre_depth = depth_at(pixel, depth_extent);
    // Nothing was drawn here — see [`DEPTH_FAR`]. `ssao.slang` wrote 1.0 across
    // the whole block this pixel is in, and there is no view-space position to
    // weight its neighbours against.
    if (centre_depth <= DEPTH_FAR)
    {
        return 1.0;
    }
    float centre_z = view_z(pixel, centre_depth, depth_size);
    float tolerance = camera.params.x * DEPTH_TOLERANCE_RADII;

    // The sample this pixel sits **on or after**, and how far past it the pixel
    // is. `nearest` is the block's own sample, `offset` is zero on an axis where
    // the two grids coincide, and `blend` is that offset as the fraction of the
    // way to the next sample — which is the bilinear weight, written out rather
    // than folded away because it is only symmetric at the divisor this pair
    // ships at.
    int2 nearest = pixel / RESOLUTION_DIVISOR;
    int2 offset = pixel - full_res_pixel(nearest);
    float2 blend = float2(offset) / float(RESOLUTION_DIVISOR);
    // **One tap per axis where the grids coincide, two where they do not.** The
    // second tap's weight is `blend`, which is exactly zero on such an axis, so
    // this loop bound decides nothing the arithmetic would not — it saves the
    // fetch. The decision is on integers the rasteriser handed over, never on a
    // depth two drivers might round differently.
    int2 span = min(offset, int2(1, 1));

    float total = 0.0;
    float weight = 0.0;
    for (int y = 0; y <= span.y; ++y)
    {
        for (int x = 0; x <= span.x; ++x)
        {
            // **Clamped, not skipped**, for `ssao_blur.slang`'s reason: a `Load`
            // outside the image reads zero on some targets and is undefined on
            // others, and zero is fully occluded here. The clamp bites along the
            // right and bottom edges of an odd extent, where the pair's grid
            // runs out one sample before the frame does.
            int2 tap = clamp(nearest + int2(x, y), int2(0, 0), extent - int2(1, 1));
            int2 texel = full_res_pixel(tap);
            float depth = depth_at(texel, depth_extent);
            float away = abs(view_z(texel, depth, depth_size) - centre_z);
            // The two weights: how near the sample is, and how near its surface
            // is. A far tap has no surface at all and takes no share — see
            // [`DEPTH_FAR`] — and that `select` is the one comparison in this
            // pass that decides anything.
            float2 along = float2(x == 0 ? 1.0 - blend.x : blend.x,
                                  y == 0 ? 1.0 - blend.y : blend.y);
            float share = along.x * along.y
                        * (depth <= DEPTH_FAR ? 0.0 : saturate(1.0 - away / tolerance));
            // The nearest sample keeps a vote it cannot lose. See
            // [`NEAREST_TAP_FLOOR`], which is also why this is a `max` and not a
            // branch around the accumulation.
            share = (x == 0 && y == 0) ? max(share, NEAREST_TAP_FLOOR) : share;
            total += occlusion.Load(int3(tap, 0)) * share;
            weight += share;
        }
    }
    // At least the nearest sample's floor, by the construction above.
    float visibility = total / weight;
    float intensity = ao_intensity();
    // **The default is spelled as a branch rather than left to the power.**
    // `pow(x, 1.0)` is not specified to return `x` bit for bit on any of these
    // targets — it is a logarithm and an exponential on most of them — so every
    // golden in this workspace would move a level here and there for a knob
    // nobody had touched. The comparison is on a uniform, so a frame takes one
    // side of it for every pixel and it decides nothing per pixel.
    return intensity == INTENSITY_DEFAULT ? visibility : pow(visibility, intensity);
}
