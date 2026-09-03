// crcbl-targets: spirv, wgsl, msl, dxil

// The other half of `ssao.slang`'s determinism rule: a 4×4 kernel over the raw
// occlusion, weighted by view-space depth, into a second `Rgba8Unorm` image.
//
// # The bent direction filters too, and it does not filter like a scalar
//
// `ssao.slang`'s header says what the four channels are: `r` is the visibility
// and `gba` are the bent direction, encoded `xyz * 0.5 + 0.5`. The kernel below
// applies **the same weights** to both — a tap that counts for the scalar
// counts for the direction, and a tap the depth ramp rejects counts for
// neither, because they are two readings of the same sample.
//
// What differs is what happens to the sum. A weighted mean of unit vectors is
// not a unit vector: it is shorter the more the taps disagree, and shortest
// exactly where the neighbourhood is busiest. So the direction is **decoded,
// summed and renormalised**, and only then re-encoded. A blur that skipped the
// renormalise would hand `mesh.slang` a direction whose length carries the
// filter's own disagreement, which is a term nothing in the shading means to
// have — and it would do so silently, because a short direction still points
// somewhere.
//
// A tap carrying the sentinel — a zero-length direction, see
// `crcbl_shaders::ssao::BENT_NORMAL_MIN_LENGTH` — contributes neither a
// direction nor a share, which is what it should: it has nothing to say. That
// is `decode_bent`'s job and not an accident of the arithmetic; the sentinel
// survives `Rgba8Unorm` as a short vector rather than as nothing, and a filter
// that let it in by its length alone hands back the unit diagonal on every
// pixel of a frame that asked for no direction. Where every tap is a sentinel,
// or where the ones that spoke cancel, the sentinel goes out again.
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
// # It runs where `ssao.slang` ran, which is not where the depth is
//
// The pair runs at a fraction of the scene extent — [`RESOLUTION_DIVISOR`] is
// the factor and `ssao.slang`'s header is the argument — so this pass reads and
// writes half-resolution images while the depth it weights by is the prepass's,
// at full resolution. Two extents, therefore, and each fetch takes the one that
// belongs to its image: the kernel walks and clamps in this pass's own pixels,
// and every depth is read at the texel [`full_res_pixel`] maps a tap to. That is
// the same texel `ssao.slang` gathered that tap's occlusion at, which is what
// makes the weight a statement about the sample rather than about a pixel near
// it.
//
// The kernel's footprint is unchanged in the pixels it is measured in, so it is
// still exactly `ssao.slang`'s dither tile — the property the section above
// rests on — and it is now [`RESOLUTION_DIVISOR`] times wider on screen.
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
    /// View → world. `ssao.slang` rotates the bent direction out of view space
    /// with it before it is ever written; nothing here does, and it is declared
    /// for `proj`'s reason exactly.
    float4x4 inv_view;
    /// `x` is the sampling radius in **world units**, read through
    /// [`sampling_radius`] because that is the disc the gather swept, and it is
    /// what [`DEPTH_TOLERANCE_RADII`] is a share of. `y` is the count of planes
    /// `ssao.slang` swept, `z` is the intensity exponent
    /// `ssao_upsample.slang` raises its reconstruction to, and `w` is that
    /// file's bent-direction switch; nothing here reads any of the four. This
    /// pass filters whatever the gather wrote, switch or no switch — the
    /// sentinel filters to the sentinel.
    float4 params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsaoParams> camera;

/// The raw occlusion `ssao.slang` wrote, fetched by integer pixel: the
/// visibility in `r` and the encoded bent direction in `gba`.
///
/// `Texture2D<float4>` and not `DepthTexture2D`: this one is an ordinary
/// four-channel colour image — `Rgba8Unorm` — so it lowers to
/// `texture_2d<f32>` and binds through the seam's `SampleType::Float`. The
/// depth spelling is the binding below.
///
/// **No sampler**, for `ssao.slang`'s reason exactly: every read here is a
/// `Load` at an integer pixel, so there is no filtering for four rasterisers to
/// disagree about.
[[vk::binding(1, 0)]]
Texture2D<float4> occlusion;

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

/// The factor the occlusion chain's gather extent is divided by, on each axis:
/// the constant `ssao.slang` declares under this name, and the argument for it
/// is that file's header.
static const int RESOLUTION_DIVISOR = 2;

/// How long a decoded bent direction has to be to be a direction at all.
///
/// **It separates two lengths that are nowhere near each other.** A pixel with
/// a direction carries a unit vector, which survives `Rgba8Unorm` to within
/// about a hundredth; a pixel with none carries the zero sentinel, which encodes
/// to `0.5` in each channel and decodes back to under a hundredth of a unit.
/// Half way between them is the one threshold no rounding on any target can
/// reach from either side, and it is where a consumer decides whether it was
/// handed a direction or told there was not one.
///
/// `crcbl_shaders::ssao::BENT_NORMAL_MIN_LENGTH` is the Rust mirror, and
/// `ssao_upsample.slang` and `mesh.slang` declare the same value under this
/// name — one length, one question, asked wherever the channel is decoded.
static const float BENT_NORMAL_MIN_LENGTH = 0.5;

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

/// The sampling radius, in world units, a frame that asks for nothing falls to.
///
/// **`ssao.slang`'s constant, and it has to be that one.** That file's
/// `sampling_radius` is what the gather resolved `camera.params.x` through, and
/// [`DEPTH_TOLERANCE_RADII`] is a share of the disc the gather actually swept —
/// so a filter reading the raw lane would take its tolerance from a number no
/// pass ever used. `crcbl_shaders::ssao`'s `RADIUS_DEFAULT` is the Rust mirror
/// and `the_occlusion_chain_resolves_one_radius` is what holds the copies
/// together.
static const float RADIUS_DEFAULT = 0.5;

/// The shortest sampling radius the gather will honour, in world units.
///
/// `ssao.slang`'s constant of the same name — see [`RADIUS_DEFAULT`].
static const float RADIUS_MIN = 0.0625;

/// The longest sampling radius the gather will honour, in world units.
///
/// `ssao.slang`'s constant of the same name — see [`RADIUS_DEFAULT`].
static const float RADIUS_MAX = 4.0;

/// The disc the gather swept, in view-space units, as this filter must read it.
///
/// `ssao.slang`'s `sampling_radius`, restated for the reason its constants are:
/// an unwritten block leaves a zero in `camera.params.x`, the gather answers
/// that with [`RADIUS_DEFAULT`], and a tolerance taken from the zero instead
/// would be zero — every tap rejected as off-surface, on a frame the gather
/// filled normally.
float sampling_radius()
{
    float asked = camera.params.x;
    if (asked <= 0.0)
    {
        return RADIUS_DEFAULT;
    }
    return clamp(asked, RADIUS_MIN, RADIUS_MAX);
}

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

/// The full-resolution texel a pixel of this pass stands for.
///
/// `ssao.slang`'s function and its whole argument: the top-left of the block,
/// mapped the same way by every pass of the chain, so the depth this kernel
/// weights a tap by is the depth that tap's occlusion was gathered at.
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

/// The two rows of the inverse projection that `depth` alone reaches: the
/// unprojected `z` and the `w` a caller divides it by.
///
/// **Split out of [`unproject`] so that no caller computes a component it
/// discards.** `ssr.slang`'s `view_z_of` and the two blurs' `view_z` want the
/// depth alone, and routing them through the full unprojection leaves the other
/// two components to a dead-code pass that may or may not run — a cost that
/// depends on the driver rather than on this file. This way every component
/// either function computes is one its caller reads, which is a property rather
/// than a hope.
///
/// **Not a measured win.** The split was made after the `ssr` pass read
/// 0.027 ms slower without it, and a paired re-measurement did not confirm
/// that: passes with no code change at all move by as much between builds on
/// the machine it was read on. What stands is the argument above, not a number.
float2 unproject_z(float depth)
{
    return float2(
        camera.inv_proj[2][2] * depth + camera.inv_proj[2][3],
        camera.inv_proj[3][2] * depth + camera.inv_proj[3][3]);
}

/// The inverse projection applied to one clip-space point, with the terms that
/// are structurally zero left out.
///
/// **The same value as `mul(camera.inv_proj, float4(ndc, depth, 1.0))`, not an
/// approximation of it.** Both projections `crcbl_render::camera::Projection`
/// builds — an infinite reversed-Z perspective and a reversed orthographic box
/// — invert to a matrix whose only non-zero entries are the eight read below,
/// so most of the general product's sixteen multiplies are against an exact
/// zero, and adding an exact zero is exact. What holds that true of a
/// projection added later is a pair of tests rather than this comment —
/// `crcbl_render::camera`'s
/// `every_projection_inverts_to_the_sparsity_the_screen_space_shaders_read`
/// for the pattern and
/// `the_screen_space_unprojection_is_the_full_product_for_that_sparsity` for
/// the four terms below being the whole of it — because a projection that
/// broke either would draw a wrong picture rather than fail to compile.
///
/// `camera.inv_proj[r][c]` is row `r`, column `c` — the same logical element
/// `mul` reads, whatever layout the target packs the block in.
///
/// **The call count is the reason it is worth writing out.** `ssao.slang`
/// unprojects once per horizon tap, once per neighbour the normal takes and
/// once for the centre, so a pixel pays for the dropped terms over and over
/// before the pass has shaded anything.
float4 unproject(float2 ndc, float depth)
{
    float2 depth_row = unproject_z(depth);
    return float4(
        camera.inv_proj[0][0] * ndc.x + camera.inv_proj[0][3],
        camera.inv_proj[1][1] * ndc.y + camera.inv_proj[1][3],
        depth_row.x,
        depth_row.y);
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
    float2 view = unproject_z(depth);
    return view.x / view.y;
}

/// The bent direction a texel of the occlusion channel carries: a **unit
/// vector**, or exactly zero where the texel holds the sentinel.
///
/// `ssao.slang`'s `encode_bent` undone — `gba * 2 - 1` — and then resolved to
/// one of those two, which is what the encoding can hold. The resolve is not
/// cosmetic: the zero sentinel survives `Rgba8Unorm` as a *short* vector rather
/// than as nothing, and a filter that weighted taps by their decoded length
/// would divide that residue by itself and hand back the unit diagonal
/// `(1, 1, 1)` normalised — a direction nothing in the frame suggested, on
/// every pixel of a frame that asked for no bent direction at all.
///
/// **The comparison decides nothing two drivers could decide differently.** It
/// separates a length of about one from a length under a hundredth — see
/// [`BENT_NORMAL_MIN_LENGTH`] — so it is the kind of comparison this pass's
/// headers argue is acceptable, next to the `DEPTH_FAR` equality.
float3 decode_bent(float4 texel)
{
    float3 decoded = texel.gba * 2.0 - 1.0;
    return length(decoded) < BENT_NORMAL_MIN_LENGTH ? float3(0.0, 0.0, 0.0) : normalize(decoded);
}

/// A summed bent direction back as the three channels beside the scalar.
///
/// **The mean and not the sum.** Each tap contributed a unit vector scaled by
/// its share, so dividing by the shares that carried a direction gives a vector
/// of length at most one — a coherence, which is what
/// [`BENT_NORMAL_MIN_LENGTH`] is a threshold on. Taps that carried the sentinel
/// contributed neither a direction nor a share, so they neither steer the mean
/// nor shorten it.
///
/// **The renormalise is here and not at the call site**, so the two filters
/// cannot differ about it: a weighted mean of unit vectors is not a unit vector,
/// and a direction whose length carries the filter's own disagreement is a term
/// the shading never meant to have.
float3 encode_bent(float3 summed, float weight)
{
    float3 mean = weight > 0.0 ? summed / weight : float3(0.0, 0.0, 0.0);
    float3 direction = length(mean) < BENT_NORMAL_MIN_LENGTH
        ? float3(0.0, 0.0, 0.0)
        : normalize(mean);
    return direction * 0.5 + 0.5;
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
    occlusion.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    // **The depth is the prepass's and is not this pass's size.** Read off its
    // own image for the reason `ssao.slang` reads its extent that way — a second
    // copy of a size is a second thing that can be stale — and kept apart from
    // `extent` above because the two genuinely differ now: see
    // [`RESOLUTION_DIVISOR`].
    uint depth_width;
    uint depth_height;
    scene_depth.GetDimensions(depth_width, depth_height);
    int2 depth_extent = int2(int(depth_width), int(depth_height));
    float2 depth_size = float2(float(depth_width), float(depth_height));
    int2 pixel = int2(input.position.xy);
    int2 centre_texel = full_res_pixel(pixel);

    float centre_depth = depth_at(centre_texel, depth_extent);
    // Nothing was drawn here — see [`DEPTH_FAR`]. `ssao.slang` returned 1.0 at
    // this pixel and there is neither a surface to filter nor a view-space
    // position to weight the neighbours against.
    if (centre_depth <= DEPTH_FAR)
    {
        // `ssao.slang` wrote full visibility and the sentinel at this pixel, and
        // this pass hands both on: `encode_bent` of a zero sum is that sentinel.
        return float4(1.0, encode_bent(float3(0.0, 0.0, 0.0), 0.0));
    }
    float centre_z = view_z(centre_texel, centre_depth, depth_size);
    float tolerance = sampling_radius() * DEPTH_TOLERANCE_RADII;

    float total = 0.0;
    // The bent direction's accumulator, on the scalar's own weights — see the
    // header. Decoded per tap, summed here, renormalised by `encode_bent`.
    float3 bent = float3(0.0, 0.0, 0.0);
    // What the direction's mean is over: the shares of the taps that carried a
    // direction at all — see [`decode_bent`], which is where a sentinel becomes
    // exactly nothing rather than something short.
    float bent_weight = 0.0;
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
                int2 texel = full_res_pixel(tap);
                float depth = depth_at(texel, depth_extent);
                float away = abs(view_z(texel, depth, depth_size) - centre_z);
                // A `select` rather than an `if` around the accumulation: the
                // far test is the only comparison in this pass that decides
                // anything, and it is the one [`DEPTH_FAR`] argues for.
                share = depth <= DEPTH_FAR ? 0.0 : saturate(1.0 - away / tolerance);
            }
            float4 sample = occlusion.Load(int3(tap, 0));
            float3 direction = decode_bent(sample);
            // One where the tap carried a direction and exactly zero where it
            // carried the sentinel, since `decode_bent` answers with a unit
            // vector or with nothing. It is what the direction's mean is over —
            // a sentinel tap neither steers it nor shortens it.
            float carried = dot(direction, direction);
            total += sample.r * share;
            bent += direction * share;
            bent_weight += carried * share;
            weight += share;
        }
    }
    // At least the centre tap's one, by the construction above. The direction
    // takes a divisor of its own, which is not this one — see [`encode_bent`].
    return float4(total / weight, encode_bent(bent, bent_weight));
}
