// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s screen-space ambient occlusion: the pass
// that reads the depth prepass and writes the `Rgba8Unorm` channel the forward
// pass modulates and steers its **ambient** term with.
//
// # What the four channels are
//
// `r` is the visibility scalar this file has always written — `1.0` where
// nothing occludes — and every reader of it is unchanged. `gba` are the **bent
// direction**: the average unblocked direction over the hemisphere, in world
// space, encoded `xyz * 0.5 + 0.5`. `mesh.slang` samples its ambient irradiance
// along it instead of along the shading normal, which is the whole of what the
// widened target buys — a scalar can only dim the ambient term, where a
// direction says which part of the room is still reaching the surface.
//
// **A zero-length direction is the sentinel for "no bent direction here"**, and
// it is what a pixel with nothing to measure writes: an unoccluded surface, a
// march too short to run, a slice set that weighed nothing.
// `crcbl_shaders::ssao::BENT_NORMAL_MIN_LENGTH` is the length a consumer tests
// against, and `mesh.slang` answers the sentinel with its own shading normal.
// Encoding zero writes `0.5` into each of the three channels, which quantises
// to the same `0x80` the renderer's 1×1 placeholder holds — so "no occlusion
// pass ran" and "no bent direction at this pixel" are one case with one answer.
//
// **No octahedral encoding.** Three channels have no seam, no branch and no
// decode; and averaging-then-renormalising is then the obvious operation for
// the two filters behind this pass, where an octahedral pair would have to be
// decoded, averaged and re-folded at every tap.
//
// # It reads depth and nothing else
//
// There is no normal attachment and no G-buffer. The prepass this samples has no
// colour target at all, so a normal buffer would mean a third geometry pipeline
// per `GeometryPath`, a new fragment entry point compiled to four targets and a
// new `VertexOutput` consumer — for a buffer exactly one pass reads. The normal
// is reconstructed here instead, from four neighbouring depths.
//
// # The technique is GTAO — ground-truth ambient occlusion
//
// Not a hemisphere of depth comparisons. Each pixel takes `slice_count` planes
// through the eye, marches `SLICE_STEPS` pixels out along each side of each one,
// and keeps the **largest elevation** any sample reached — the horizon. What the
// visible hemisphere is then worth is a closed-form integral over those two
// angles, which `slice_visibility` evaluates.
//
// This file used to sum eight binary depth comparisons and its header used to
// say GTAO was too expensive for quality nobody could resolve at the golden
// suite's 256×192. `docs/plan/46-ambient-occlusion.md`'s 2026-08-27 decision is
// what reopened it, and the argument that did so is **determinism**, not
// quality: a binary comparison landing on its threshold resolves differently on
// two drivers and cliffs that pixel by an eighth, where a horizon angle
// disagreeing in the last bit moves the integral by a hair. The technique that
// looked riskier for the goldens is the safer one.
//
// # It runs at half the scene extent, and reads depth at full
//
// One invocation of this pass covers a [`RESOLUTION_DIVISOR`]-square block of
// the frame, so a frame marches a quarter of the horizons it used to. Occlusion
// is a low-frequency field and the reference implementations halve it for
// exactly that reason; `ssao_upsample.slang` is what puts it back, weighting the
// half-resolution samples by how near their depth is to the full-resolution
// pixel's so the field does not cross a silhouette on the way up.
//
// **Only the output is halved.** Every depth this file reads is the prepass's
// own, at full resolution, through the texel [`full_res_pixel`] maps this
// invocation to — so the march reaches the same distance through the same
// texels and the normal is still reconstructed from neighbours one
// full-resolution texel away. `depth_at`, `view_position` and `normal_at` are
// untouched by the change, which matters beyond taste: `ssr.slang` and
// `contact_shadows.slang` carry the same three bodies and
// `crcbl_shaders::ssr`'s drift guard compares them as text. A half-resolution
// depth of its own would be a second pass building a second pyramid, and
// `docs/backlog.md` carries that as a rung of its own.
//
// **The dither tile is indexed by the pass's own pixel**, not by the texel it
// stands for — see [`occlusion_at`], which is why the tile arrives there as an
// argument. So the sixteen phases still land one per pixel of this pass, and the
// tile they fill is [`RESOLUTION_DIVISOR`] times wider on screen than it was.
// That is expected rather than a regression to paper over: `ssao_blur.slang`'s
// footprint is this file's tile measured in the pixels the pair runs at, so it
// still averages exactly one tile and no more. What is wider is the tile
// itself, and therefore whatever residue of it the blur leaves.
//
// # Why the slice directions are a table and never a hash
//
// The determinism rule survives the technique change intact. Interleaved-
// gradient noise and `frac(sin(dot(...)))` hashes amplify float differences by
// construction, which is the opposite of what a golden needs; an integer index
// into a constant array is bit-identical by inspection. `SLICE_DIRECTIONS` is
// the same sixteen-entry Bayer-ordered table this file has always had, read for
// a marching direction now rather than for a tangent-frame spin.
//
// The slices after the first are that direction **turned on the table's own
// integers**, which is what keeps every one of them exact. A quarter turn is
// `(x, y) -> (-y, x)`, a swap and a negation. An eighth turn is
// `(x, y) -> (x - y, x + y)` — the 45 degree rotation scaled by `sqrt(2)`, and
// scale is the one thing `normalize` removes, so the `sqrt(2)/2` a rotation
// matrix would spell never has to be written down. Both send an entry of
// `SLICE_DIRECTIONS` to another pair of small integers, so the only rounding in
// a marching direction is the `normalize` this file already did for its first
// slice. [`SLICE_COUNT_MAX`] carries that bound as an angle, a texel and a
// channel level.
//
// **How many of the four a frame sweeps is `camera.params.y`**, and four is what
// ships since 2026-09-03 — two is the floor a frame that writes nothing falls
// to. See [`SLICE_COUNT_MAX`] for what the other two buy and cost.
//
// `STEP_OFFSETS` is the same rule applied to the other axis of the march — how
// far along a slice a pixel starts rather than which way it faces — and is an
// ordered-dither table for the same reason, never a random number.
//
// `ssao_blur.slang` is the other half of the rule and is **not optional**: its
// footprint is exactly this file's tile, so it averages the sixteen directions
// back into a smooth field.
//
// # The one approximation, and why it is one
//
// The integral needs an arc cosine, and no target specifies its `acos` to any
// accuracy — so two rasterisers would disagree by however much their libraries
// do, which is not a number anyone can bound. `acos_approx` is Abramowitz and
// Stegun 4.4.45 instead: a cubic and a `sqrt`, both exactly specified.
// `crcbl_shaders::ssao` carries the same polynomial, the measured error against
// `f64::acos`, and the drift test between the two.
//
// # The full-screen triangle is `tonemap.slang`'s
//
// Same three vertices from `SV_VertexID`, same `-2.0` on Y for the same reason.
// This pass wants pixel coordinates rather than a UV, so it reads them off
// `SV_Position` in the fragment stage — which is what `mesh.slang`'s `froxel_of`
// already does with the same value.

/// Everything this pass needs that is not in a texture.
///
/// Two matrices rather than one: the reconstruction goes clip → view and each
/// sample goes view → clip again, and there is no cheaper spelling that survives
/// an orthographic camera. `crcbl_shaders::ssao::SsaoParams` is the Rust mirror
/// and `crcbl_render::forward` is what writes it.
///
/// **Bound as `camera` rather than as `ssao`**, and that is the one thing here
/// that is about a *different* file: `ssr.slang` copies `view_position` and
/// `normal_at` out of this one verbatim, and
/// `crcbl_shaders::ssr`'s `the_shared_screen_space_helpers_have_not_drifted`
/// compares the bodies as text. Three passes reaching the same two matrices
/// through the same name is what lets that guard be an equality rather than a
/// substitution nobody would trust.
struct SsaoParams
{
    /// Clip → view. Reversed-Z and possibly infinite, so `w` is what carries the
    /// depth and the divide below is not optional.
    float4x4 inv_proj;
    /// View → clip, for projecting a sample point back to a pixel.
    float4x4 proj;
    /// View → world, for the bent direction alone.
    ///
    /// **Everything else in this file is view space and stays there.** The
    /// horizons, the tilt and the integral are all questions about the
    /// neighbourhood of a surface, and view space is where that neighbourhood
    /// is isotropic and the eye is at the origin — see `SsaoParams::inv_proj`.
    /// The bent direction is the one value that leaves this pass, and its
    /// consumer is `mesh.slang`'s ambient term, which evaluates a **world**-
    /// space L1 environment: `sky_irradiance` and `probe_irradiance` both take
    /// a world-space normal, and `FrameUniforms` carries no rotation that could
    /// take a view-space one there. So the rotation happens here, once per
    /// gathered pixel at [`RESOLUTION_DIVISOR`]-squared of the frame, rather
    /// than once per shaded fragment.
    ///
    /// `ssr.slang` carries the same member under the same name and reaches it
    /// the same way, for the same reason: a view-space ray that has to address
    /// the world-space probe grid.
    float4x4 inv_view;
    /// `x` is the sampling radius in **world units**, read through
    /// [`sampling_radius`], and `y` is how many planes each pixel sweeps — see
    /// [`slice_count`], which is what reads it. `z` is the intensity exponent
    /// `ssao_upsample.slang` raises its reconstruction to, written by the host
    /// into the block all three of these passes share and read by that one
    /// alone. `w` is the bent-direction switch — see [`bent_normals`], which is
    /// what reads it, and which is the one lane of this row *this* file reads
    /// besides `x` and `y`.
    ///
    /// **`y` is a count and not a quality enumeration**, so the shader has one
    /// number to clamp rather than a table to agree with a host about. It is
    /// whole and small, so it survives the trip through a float exactly.
    ///
    /// `y` used to be a depth bias, which the hemisphere-of-comparisons this
    /// file drew before GTAO needed: half of a flat surface's own samples land
    /// marginally in front of it once depth is quantised, and without a bias
    /// that is grey haze over every wall. A horizon integral has no such
    /// threshold to sit on — a sample in the surface's own plane lands exactly
    /// on the tangent, where the integral is stationary — so the bias measured
    /// as a no-op and was removed. The slot it left is what the slice count
    /// took, which is why the block is the width it always was.
    float4 params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsaoParams> camera;

/// The depth prepass's output, fetched by integer pixel and never filtered.
///
/// `DepthTexture2D`, not `Texture2D<float>`, and the distinction is the one this
/// engine has already been bitten by once: the two are the same HLSL declaration
/// and the same D3D12 SRV, and they are **not** the same WGSL.
/// `Texture2D<float>` lowers to `texture_2d<f32>`, which cannot be filled by a
/// `D32Float` view through a layout the seam spells `SampleType::Depth`.
/// `DepthTexture2D` lowers to `texture_depth_2d`, `depth2d<float,
/// access::sample>` in MSL and an `OpTypeImage` with `Depth = 1` in SPIR-V. See
/// `mesh.slang`'s `shadow_atlas`, which carries the same paragraph.
///
/// **No sampler.** Every read here is a `Load` at an integer pixel, so there is
/// no UV, no filtering and no texel-centre arithmetic for four rasterisers to
/// disagree about.
[[vk::binding(1, 0)]]
DepthTexture2D scene_depth;

/// The reversed-Z far plane: `crcbl_hal::depth::CLEAR`.
///
/// **The far plane has no surface**, and this is the value that says so. The
/// unprojection below divides by `clip.w`, which an infinite reversed-Z
/// projection takes to zero exactly here — so a pixel the geometry never covered
/// has to leave early rather than reconstruct a position from a division by
/// nothing.
///
/// `crcbl_shaders::ssao::DEPTH_FAR` is the Rust mirror, and
/// `crcbl_render::forward` is where it is asserted equal to the seam's own
/// constant.
static const float DEPTH_FAR = 0.0;

/// Planes through the eye a pixel may sweep for a horizon, at most.
///
/// The frame picks how many of them it wants — see [`slice_count`] — and two is
/// what ships. What the other two are, and what they are worth, is below.
///
/// # The four turns, and why none of them rounds
///
/// [`turned`] is the whole of it: a quarter turn is a swap and a negation, an
/// eighth turn is `(x, y) -> (x - y, x + y)`, and both are applied to an entry
/// of [`SLICE_DIRECTIONS`] rather than to a unit vector. Those entries have
/// components in `-2..=2`, the turns take them to `-3..=3`, and every integer in
/// that range is exactly representable — so a float add or subtract of two of
/// them lands on the exact answer and rounds nothing.
///
/// An eighth turn spelled as a rotation matrix would need `sqrt(2)/2` and would
/// round twice per component before anything else happened. This spelling is the
/// same rotation scaled by `sqrt(2)`, and `normalize` divides the scale back out
/// — which is why the constant never appears. `acos` was refused in this file
/// because no target specifies it; nothing here needs that argument, because
/// there is no transcendental to specify.
///
/// # The one rounding, bounded
///
/// It is `normalize`, and **it is not new**: it is the call the first slice has
/// made since the table was a table, on a vector this file now constructs
/// exactly rather than approximately. What the targets promise for it is a few
/// units in the last place rather than a correctly-rounded answer — which is a
/// different objection from the one this file makes to `acos`, where *no*
/// accuracy is stated at all — so bound it generously at eight units: a relative
/// `8 * 2^-24` on each component turns a unit vector by at most
/// `sqrt(2) * 8 * 2^-24`, under `7e-7` radians.
///
/// What that is worth in the three units this pass actually consumes:
///
/// * **As a horizon angle**, against the error budget the file already accepts:
///   `crcbl_shaders::ssao::MAX_ACOS_ERROR` is what [`acos_approx`] is allowed to
///   be off by, and this is two orders of magnitude under it.
/// * **As an occlusion level.** That constant's own doc does the conversion —
///   the integral's range is about `pi/2` into an `R8Unorm` channel, so one
///   level is roughly `6e-3` radians of horizon. Seven parts in ten million of a
///   radian is about a ten-thousandth of the smallest step the channel holds.
/// * **As a texel**, which is the only one that can bite, because
///   [`horizon_cosine`] truncates `step * distance` to whole pixels. A direction
///   off by `7e-7` radians displaces the furthest tap by that times `reach`, so
///   even a reach of a few thousand pixels moves it by under a hundredth of a
///   texel. A tap changes only where the exact product already sat that close to
///   an integer, and then by one pixel — which is the exposure the first slice
///   has always had.
///
/// One thing genuinely did move, and it is worth naming: the second slice used
/// to be an exact quarter turn of the *normalised* first, and is now the
/// `normalize` of an exactly quarter-turned integer vector. A target that
/// computes `normalize` as a reciprocal square root and a multiply gives
/// bit-identical answers either way — the sum of squares is the same and
/// negation is exact — and a target that does something else may differ in the
/// last place. It measured identical on both drivers here: the radial
/// measurement in `crcbl`'s `forward_e2e::occlusion` reports the same longest
/// run before and after, and every golden in the workspace is unmoved.
///
/// # What the extra two buy
///
/// Distinct plane *orientations* across the tile, which is what the tangential
/// banding is a shortage of. Reduced modulo a half turn, the sixteen entries of
/// [`SLICE_DIRECTIONS`] point along **eight** orientations, and the quarter turn
/// permutes that set onto itself — so the count that ships gives the blur's
/// footprint those eight and no more, however many slice *instances* it sweeps.
/// The eighth turn is the one that leaves the set: it adds four orientations the
/// table cannot reach, taking the neighbourhood to twelve. That is the whole of
/// what the second pair buys, and `crcbl`'s
/// `forward_e2e::occlusion::the_tangential_occlusion_line_does_not_step` is what
/// measures it as a picture rather than as a count.
static const uint SLICE_COUNT_MAX = 4;

/// The floor a frame that asks for nothing falls to — not what ships.
///
/// Two slices sweep the tile's own direction and its quarter turn, which is what
/// this file did before it could be asked for more. The engine has asked for
/// [`SLICE_COUNT_MAX`] since 2026-09-03 and the goldens were re-blessed there,
/// so this count is what a producer writing a zero gets rather than what a
/// player sees.
static const uint SLICE_COUNT_DEFAULT = 2;

/// Samples marched out along **each side** of each slice.
///
/// So a pixel reads `slice_count() * 2 * SLICE_STEPS` depths for its horizons,
/// sixteen at the count that ships, against the eight comparisons the
/// hemisphere kernel took. The
/// steps are spaced evenly in *pixels* out to the projected radius, which is
/// what keeps the march reading texels it can actually distinguish: spacing them
/// evenly in world units puts several of them inside one pixel whenever the
/// surface is near the eye, and those samples all return the same depth.
static const uint SLICE_STEPS = 4;

/// The shortest projected radius, in pixels, the march bothers with.
///
/// Below this the whole sweep falls inside a pixel or two and every sample reads
/// the centre's own depth, which is a horizon of nothing and an occlusion of
/// zero — the correct answer, arrived at through sixteen loads. The early return
/// is what skips them. A surface this far away is also one whose ambient term is
/// a handful of pixels wide.
static const float MIN_RADIUS_PIXELS = 2.0;

/// The sampling radius, in world units, a frame that asks for nothing falls to.
///
/// Half a metre: what every golden in this workspace was blessed at, and what
/// `crcbl_render::ssao::r_ssao_radius` defaults to. A producer that never writes
/// `camera.params.x` leaves a zero there, and a zero disc projects to a reach
/// under [`MIN_RADIUS_PIXELS`] at every distance — a frame with no occlusion in
/// it. So [`sampling_radius`] answers the zero with this rather than with
/// [`RADIUS_MIN`], which is `ao_intensity`'s reasoning in the shader beside
/// this one: the floor is a radius somebody chose, and an unwritten block is
/// nobody choosing.
///
/// **All three of the chain's shaders resolve the lane, not just this one.**
/// `ssao_blur.slang` and `ssao_upsample.slang` take their depth tolerance as a
/// share of the disc the gather swept, so each carries this constant and its
/// own copy of [`sampling_radius`]; `crcbl_shaders::ssao`'s
/// `the_occlusion_chain_resolves_one_radius` is what holds the three together.
static const float RADIUS_DEFAULT = 0.5;

/// The shortest sampling radius the march will honour, in world units.
///
/// An eighth of [`RADIUS_DEFAULT`]. Below it the disc is narrow enough that
/// [`MIN_RADIUS_PIXELS`] turns the march away across most of a room rather than
/// at the far end of one, and the lower half of a control that draws nothing on
/// screen is a control nobody can read.
static const float RADIUS_MIN = 0.0625;

/// The longest sampling radius the march will honour, in world units.
///
/// Eight times [`RADIUS_DEFAULT`], the same span the other way.
///
/// **The ceiling is a picture and not a budget.** The march takes
/// `slice_count() * 2 * SLICE_STEPS` samples whatever its reach, so a wider disc
/// costs nothing but cache locality; what it spends is fidelity. The steps are
/// spaced evenly in pixels out to the projected radius, so widening it walks
/// them apart, and past a few metres a step lands on the far side of the room
/// rather than on the neighbourhood the horizon is meant to be over.
static const float RADIUS_MAX = 4.0;

/// The factor the occlusion chain's gather extent is divided by, on each axis.
///
/// **Occlusion is low-frequency, so it is not gathered per pixel.** This pass
/// and `ssao_blur.slang` run over an image this many times smaller on each side
/// — a quarter of the invocations — and `ssao_upsample.slang` reconstructs the
/// full-resolution channel the forward pass binds. The header says what that
/// costs and what it does not.
///
/// `crcbl_shaders::ssao::RESOLUTION_DIVISOR` is the Rust mirror and
/// `crcbl_render::ssao::half_extent` is what sizes the images with it.
static const int RESOLUTION_DIVISOR = 2;

/// Half a turn's worth of horizon, which is where a hemisphere's edge is.
///
/// Written out rather than derived, because it is a bound in the integral rather
/// than an angle anything computes: a horizon may not rise past the plane of the
/// surface, and this is that plane.
static const float HALF_PI = 1.5707964;

/// How long the **mean** of a set of bent directions has to be before anything
/// in this chain calls it a direction.
///
/// It answers one question at three points — here over the slices, and in
/// `ssao_blur.slang` and `ssao_upsample.slang` over their taps — and the
/// question is always whether the directions being averaged agree enough to
/// have an average. Each of them is a unit vector, so the mean's length is a
/// coherence in `0..=1`: one where they all point the same way, zero where they
/// cancel exactly, a half where two of them are a third of a turn apart. It is
/// **not** a brightness, which is the whole reason it is a mean and not a sum —
/// a dark pixel with a well-defined way out is the case the bent direction
/// exists for, and it is coherent.
///
/// Below it there is no preferred direction and the zero sentinel goes out.
/// The same value also separates a decoded unit vector from a decoded sentinel,
/// which is the consumers' half of it: `ssao.slang`'s header states the
/// encoding, a sentinel decodes to under a hundredth of a unit, and half way
/// between the two is a threshold no rounding on any target can reach.
///
/// `crcbl_shaders::ssao::BENT_NORMAL_MIN_LENGTH` is the Rust mirror, and
/// `ssao_blur.slang`, `ssao_upsample.slang` and `mesh.slang` declare the same
/// value under this name.
static const float BENT_NORMAL_MIN_LENGTH = 0.5;

/// The polynomial [`acos_approx`] evaluates, index `i` the coefficient of `x^i`.
///
/// **Abramowitz and Stegun 4.4.45**, the degree-three minimax fit of
/// `acos(x) / sqrt(1 - x)` on `0..=1`. `crcbl_shaders::ssao::ACOS_KERNEL` is the
/// same four numbers and `the_shader_evaluates_this_modules_polynomial` is what
/// holds the copies together.
static const float ACOS_KERNEL[4] = {
    1.5707288, -0.2121144, 0.0742610, -0.0187293,
};

/// The slice tile: one screen-space marching direction per pixel of a 4×4 block,
/// indexed by `pixel.xy & 3`.
///
/// Sixteen integer directions at roughly even angles — the pairs `(±1, ±2)`,
/// `(±1, ±1)`, `(±2, ±1)` and the four axes doubled — laid out in the order a
/// 4×4 **Bayer** matrix gives, so two pixels next to each other sweep planes from
/// opposite sides of the circle rather than adjacent ones. Listed in tile order
/// rather than angular order for that reason: an angularly-ordered table indexed
/// this way would give neighbouring pixels almost the same slice, which is
/// banding rather than dither.
///
/// They are not unit vectors and do not need to be: `occlusion_at` normalises
/// each before it marches.
///
/// **The same sixteen numbers this file has always carried**, read differently.
/// They were a seed for a Gram-Schmidt tangent frame when the kernel was a
/// hemisphere; `docs/plan/46-ambient-occlusion.md` predicted the table would
/// survive the technique change, and it did.
static const float2 SLICE_DIRECTIONS[16] = {
    float2(2.0, 0.0),   float2(-2.0, 0.0),  float2(1.0, 1.0),   float2(-1.0, -1.0),
    float2(0.0, -2.0),  float2(0.0, 2.0),   float2(1.0, -1.0),  float2(-1.0, 1.0),
    float2(1.0, 2.0),   float2(-1.0, -2.0), float2(2.0, 1.0),   float2(-2.0, -1.0),
    float2(2.0, -1.0),  float2(-2.0, 1.0),  float2(1.0, -2.0),  float2(-1.0, 2.0),
};

/// The step tile: the fraction of a step each pixel of a 4×4 block starts its
/// march at, indexed by `pixel.xy & 3` exactly as [`SLICE_DIRECTIONS`] is.
///
/// **Without it a horizon can only sit at [`SLICE_STEPS`] distances.** Every
/// pixel would begin at the same fraction of a step, so an occluder draws that
/// many concentric terraces rather than a gradient — and the phase is identical
/// across the whole tile, which leaves `ssao_blur.slang` nothing to average:
/// its footprint is exactly this tile, and sixteen pixels that stepped in
/// lockstep average to the step they all took.
///
/// **A 4×4 ordered-dither (Bayer) threshold map over sixteenths**, entry `i`
/// being `(M[i] + 1) / 16` for the standard recursive `M` — the same
/// construction `SLICE_DIRECTIONS` is ordered by, and the hash-free spelling of
/// "spread sixteen phases as far apart as possible inside a 4×4 tile". The
/// map's defining property is that numerically adjacent values land as far
/// apart as the tile allows, so neighbouring pixels sample from opposite ends
/// of a step rather than from beside each other. Every entry is a multiple of
/// a sixteenth and therefore exact in binary floating point, which is the
/// determinism rule the header states: an integer table, never
/// `frac(sin(dot(...)))`.
///
/// **The range is `(0, 1]`, closed at the top.** The entry that is one is the
/// march this file made before there was a table: its last step lands exactly
/// on `reach`, and no pixel steps past the radius. At the bottom, a sixteenth
/// of a step rounds to the centre pixel once `reach` is small, and
/// [`horizon_cosine`]'s own zero-length rejection is what drops that sample —
/// the loop still runs [`SLICE_STEPS`] times, so the tap count and the cost are
/// unchanged.
static const float STEP_OFFSETS[16] = {
    0.0625, 0.5625, 0.1875, 0.6875,
    0.8125, 0.3125, 0.9375, 0.4375,
    0.2500, 0.7500, 0.1250, 0.6250,
    1.0000, 0.5000, 0.8750, 0.3750,
};

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The full-resolution texel a pixel of this pass stands for.
///
/// The **top-left** of the block rather than its centre, because a block of an
/// even side has no centre texel and every fetch in this pair is a `Load`: the
/// alternative is a filtered value four rasterisers would each round their own
/// way, which is the objection this file makes to everything else it refuses.
///
/// Every pass of the chain maps the same way, which is what lets
/// `ssao_blur.slang` weight a tap by the depth this file actually gathered it
/// at, and `ssao_upsample.slang` know where each half-resolution sample sits.
/// `crcbl_shaders::ssr`'s drift guard holds those copies together.
int2 full_res_pixel(int2 pixel)
{
    return pixel * RESOLUTION_DIVISOR;
}

/// The depth prepass's value at `pixel`, clamped into the image.
///
/// Clamped rather than bounds-checked at each call site: a `Load` outside the
/// image reads zero on some targets and is undefined on others, and zero is
/// [`DEPTH_FAR`] — so an unclamped edge tap would report "nothing there" and
/// quietly stop occluding along all four borders.
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

/// View-space position of the pixel at `pixel`, given the depth already fetched
/// for it.
///
/// The caller fetches the depth because it also has to test it against
/// [`DEPTH_FAR`], and fetching twice would be a second chance for the two to
/// disagree about which pixel is being asked about.
float3 view_position(int2 pixel, float depth, float2 extent)
{
    // Pixel centre → NDC. `+Y` is up in this engine's clip space on every target
    // — `crcbl-vk` submits a negative-height viewport so that Vulkan agrees with
    // the other three — while `SV_Position.y` counts down the framebuffer. Hence
    // the subtraction on `y` and not on `x`; `tonemap.slang`'s `-2.0` is the same
    // fact seen from the vertex stage.
    float2 ndc = float2(
        (float(pixel.x) + 0.5) / extent.x * 2.0 - 1.0,
        1.0 - (float(pixel.y) + 0.5) / extent.y * 2.0);
    float4 view = unproject(ndc, depth);
    return view.xyz / view.w;
}

/// The surface normal at `centre`, from the **closest** of the two neighbours on
/// each axis.
///
/// The two-tap `ddx`/`ddy` reconstruction is one line shorter and straddles the
/// depth discontinuity at every silhouette, which draws a dark rim around every
/// object in the frame. This one takes four taps and keeps, per axis, whichever
/// neighbour is nearer in depth to the centre — so at a silhouette it leans on
/// the side that is still the same surface.
///
/// Escalating past this — a normal attachment, or a full G-buffer — is contained
/// to the prepass pipeline and this function, and is deliberately not done here:
/// see the header.
float3 normal_at(int2 pixel, float3 centre, int2 extent, float2 size)
{
    float3 left = view_position(pixel + int2(-1, 0), depth_at(pixel + int2(-1, 0), extent), size);
    float3 right = view_position(pixel + int2(1, 0), depth_at(pixel + int2(1, 0), extent), size);
    float3 up = view_position(pixel + int2(0, -1), depth_at(pixel + int2(0, -1), extent), size);
    float3 down = view_position(pixel + int2(0, 1), depth_at(pixel + int2(0, 1), extent), size);

    float3 horizontal = abs(right.z - centre.z) < abs(centre.z - left.z)
        ? right - centre
        : centre - left;
    float3 vertical = abs(down.z - centre.z) < abs(centre.z - up.z)
        ? down - centre
        : centre - up;

    // `cross(vertical, horizontal)` and not the other order. View space is
    // right-handed with the eye looking down `-Z`; `horizontal` runs along `+X`
    // and `vertical` — one framebuffer row *down* — runs along `-Y`, so
    // `cross(-Y, +X)` is `+Z`, which faces the camera. The other order gives a
    // normal pointing into the surface, and a hemisphere built on it samples the
    // solid side of every wall: the frame goes uniformly dark rather than
    // obviously wrong, which is why `render_e2e`'s ratio exists.
    return normalize(cross(vertical, horizontal));
}

/// `acos(x)` for `x` in `-1..=1`, using only operations IEEE-754 specifies
/// exactly.
///
/// The header says why the intrinsic is refused. The fit is on `0..=1` only, and
/// a negative argument is answered through `acos(-x) = π - acos(x)`, which adds
/// no error beyond the one rounding of the subtraction. Exact at `x = 1`, where
/// the square root takes the whole expression to zero — the case a grazing
/// horizon lands on, and the one an approximation that merely got close would
/// turn into a rim of occlusion around every silhouette.
///
/// `crcbl_shaders::ssao::acos_approx` is the same function and
/// `MAX_ACOS_ERROR` there is what it was measured to be worth.
float acos_approx(float x)
{
    float magnitude = min(abs(x), 1.0);
    float kernel = ACOS_KERNEL[3];
    kernel = kernel * magnitude + ACOS_KERNEL[2];
    kernel = kernel * magnitude + ACOS_KERNEL[1];
    kernel = kernel * magnitude + ACOS_KERNEL[0];
    float positive = kernel * sqrt(1.0 - magnitude);
    return x < 0.0 ? (2.0 * HALF_PI - positive) : positive;
}

/// What fraction of one slice's half-disc the surface can see, given the two
/// horizon angles and the surface's own tilt within that slice.
///
/// # The integral
///
/// This is GTAO's inner form — Jimenez et al., *Practical Real-Time Strategies
/// for Accurate Indirect Occlusion*, SIGGRAPH 2016 — the cosine-weighted
/// visibility of the arc between two horizons, for a surface whose projected
/// normal sits at `gamma` from the view direction inside the slice plane:
///
/// ```text
/// 1/4 (-cos(2 h1 - gamma) + cos gamma + 2 h1 sin gamma)
///   + 1/4 (-cos(2 h2 - gamma) + cos gamma + 2 h2 sin gamma)
/// ```
///
/// **Every cosine and sine in it is already known**, which is why only the
/// horizons themselves need an arc cosine. `cos(2h - gamma)` expands by the
/// double-angle identities into `cos h`, `sin h`, `cos gamma` and `sin gamma`,
/// and the caller has all four: two from the dot products that found the
/// horizons, two from the dot product that found the projected normal.
///
/// # The clamp is why the sines come in rather than being derived here
///
/// A horizon may not rise past the plane of the surface, so `h1` is bounded
/// below by `gamma - π/2` and `h2` above by `gamma + π/2`. Where the bound
/// binds, the angle is no longer the one the dot product gave and its cosine and
/// sine are the bound's — `cos(gamma ∓ π/2)` is `±sin gamma`, and
/// `sin(gamma ∓ π/2)` is `∓cos gamma`. The caller does that selection because it
/// is the caller that knows which branch it took.
float slice_visibility(float h1, float cos_h1, float sin_h1,
                       float h2, float cos_h2, float sin_h2,
                       float cos_gamma, float sin_gamma)
{
    // cos(2h - gamma) = cos 2h cos gamma + sin 2h sin gamma, with the
    // double-angle forms written out.
    float cos_2h1 = 2.0 * cos_h1 * cos_h1 - 1.0;
    float sin_2h1 = 2.0 * sin_h1 * cos_h1;
    float cos_2h2 = 2.0 * cos_h2 * cos_h2 - 1.0;
    float sin_2h2 = 2.0 * sin_h2 * cos_h2;

    float first = -(cos_2h1 * cos_gamma + sin_2h1 * sin_gamma)
                + cos_gamma + 2.0 * h1 * sin_gamma;
    float second = -(cos_2h2 * cos_gamma + sin_2h2 * sin_gamma)
                 + cos_gamma + 2.0 * h2 * sin_gamma;
    return 0.25 * (first + second);
}

/// The largest elevation any sample along one side of a slice reached, as a
/// cosine against the view direction.
///
/// Marches out from `pixel` in whole-pixel steps to `reach` pixels, unprojects
/// each sample that has a surface, and keeps the largest `dot(direction, view)`
/// — which is the *smallest* angle from the eye, and therefore the highest
/// horizon.
///
/// **`offset` phases the march**, in `(0, 1]`. It is this pixel's entry in
/// [`STEP_OFFSETS`], and that table is where the reason lives: every pixel
/// starting at the same fraction of a step is what terraces the result.
///
/// **Starts at `-1.0`, which is a horizon behind the eye and not one flat
/// against it.** The caller clamps every horizon into the hemisphere over the
/// surface, so this value arrives there as exactly `±π/2` — the rim — and a side
/// that found nothing contributes no occlusion. Starting at `+1.0` looks like
/// the safe choice and is the opposite: it is a horizon *at* the eye, which
/// occludes the entire slice, and it makes an empty frame come out black.
///
/// **The falloff is a rejection, not a weight.** A surface further from the
/// centre than the radius is not part of this pixel's neighbourhood at all, and
/// letting it in with a small weight is what draws a halo around every object
/// standing in front of a distant wall. `radius` is in view-space units and the
/// test is on the sample's own distance, so it is the same surface either way it
/// is approached.
float horizon_cosine(int2 pixel, float2 step, float offset, float reach, float3 centre,
                     float3 view, float radius, int2 extent, float2 size)
{
    float cosine = -1.0;
    for (uint index = 0; index < SLICE_STEPS; ++index)
    {
        // Evenly spaced out to `reach` and phased by `offset`, so the sixteen
        // pixels of a tile march sixteen different sets of distances between
        // them. See [`STEP_OFFSETS`].
        float distance = reach * (float(index) + offset) / float(SLICE_STEPS);
        int2 tap = pixel + int2(step * distance);
        if (tap.x < 0 || tap.y < 0 || tap.x >= extent.x || tap.y >= extent.y)
        {
            break;
        }

        float depth = depth_at(tap, extent);
        if (depth <= DEPTH_FAR)
        {
            continue;
        }
        float3 delta = view_position(tap, depth, size) - centre;
        float length_squared = dot(delta, delta);
        if (length_squared > radius * radius || length_squared < 1e-10)
        {
            continue;
        }
        // `sqrt` and a divide rather than `rsqrt`: the reciprocal square root is
        // an *approximate* instruction on several targets and specified to no
        // accuracy anywhere, which is the same objection this file makes to
        // `acos`. Both of these are exactly specified.
        cosine = max(cosine, dot(delta, view) / sqrt(length_squared));
    }
    return cosine;
}

/// How many of [`SLICE_COUNT_MAX`]'s planes this frame asked for.
///
/// `camera.params.y`, and the host writes a whole number there — see
/// `crcbl_shaders::ssao::SsaoParams`. The floor is [`SLICE_COUNT_DEFAULT`]
/// rather than zero, and it is not defensive decoration: a producer that never
/// writes the field leaves the padding it used to be, which reads as zero, and a
/// zero here would run the loop no times and light every surface fully. The
/// floor makes that frame the frame this file drew before the field existed.
uint slice_count()
{
    return clamp(uint(camera.params.y), SLICE_COUNT_DEFAULT, SLICE_COUNT_MAX);
}

/// The disc this frame asked its horizons for, in view-space units.
///
/// `camera.params.x`, which the host writes from a console variable — see
/// `crcbl_render::ssao::r_ssao_radius`. That variable's own range is the
/// console's guard and this clamp is the one that holds if the two ever
/// disagree; the host clamps as well, so a value that never reached this
/// function is caught there.
///
/// **The unwritten block is turned away upwards, like `slice_count`'s lane and
/// unlike [`RADIUS_MIN`].** World units have no zero that means anything, so a
/// zero here is a producer that wrote no block rather than a frame asking for
/// the narrowest disc.
float sampling_radius()
{
    float asked = camera.params.x;
    if (asked <= 0.0)
    {
        return RADIUS_DEFAULT;
    }
    return clamp(asked, RADIUS_MIN, RADIUS_MAX);
}

/// Whether this frame asked for the bent direction beside the scalar.
///
/// `camera.params.w`, which the host writes from a console variable — see
/// `crcbl_render::ssao::r_ssao_bent_normals`.
///
/// **Zero is off, and that is the degenerate this way round on purpose.** A
/// producer that never writes the field leaves the padding this word used to
/// be, which reads as zero — and a frame with no bent direction is the frame
/// this file drew before the channel was widened, because every consumer
/// answers the sentinel with the shading normal it already had.
/// `slice_count`'s field cannot be defaulted that way (a zero there sweeps no
/// planes and lights every surface), which is why the two lanes turn an
/// unwritten block away in opposite directions.
bool bent_normals()
{
    return camera.params.w != 0.0;
}

/// One entry of [`SLICE_DIRECTIONS`] turned into slice `slice`'s marching
/// direction, still unnormalised.
///
/// Bit 1 is an eighth turn and bit 0 a quarter, so the slices come out at
/// `0`, `pi/2`, `pi/4`, `3pi/4` in that order — which is why raising the count
/// from two to four adds planes rather than reordering the two that ship.
///
/// **Every operation here is exact**, and [`SLICE_COUNT_MAX`] is where that is
/// argued and bounded: the entries are small integers, both turns keep them
/// integers, and the `sqrt(2)` an eighth turn leaves on the length is divided
/// out by the `normalize` the caller does anyway.
float2 turned(float2 seed, uint slice)
{
    float2 eighth = (slice & 2u) != 0u
        ? float2(seed.x - seed.y, seed.x + seed.y)
        : seed;
    return (slice & 1u) != 0u ? float2(-eighth.y, eighth.x) : eighth;
}

/// How much of the hemisphere over `centre` is blocked, in `x`; the **bent
/// direction** in `yzw`, in world space and either a unit vector or exactly
/// zero.
///
/// **Replace this function, and only this function, to upgrade the technique.**
/// It has been replaced once already, which is what that line was for.
///
/// # The bent direction is the sweep's own by-product
///
/// Jimenez et al., *Practical Real-Time Strategies for Accurate Indirect
/// Occlusion*, SIGGRAPH 2016 — the same course notes [`slice_visibility`]
/// transcribes its integral from — take the bent normal per slice as the
/// **bisector of the visible arc**, the direction half way between the two
/// horizons, and accumulate it over the slices weighted by what that slice
/// could see. So the two horizon angles this file already found are the whole
/// of the input: nothing marches again, nothing new is fetched, and no normal
/// attachment is wanted — which is what keeps this rung inside the refusal the
/// header states.
///
/// **The bisector's cosine and sine come out of the horizons' without a
/// trigonometric call**, which is the same determinism argument [`acos_approx`]
/// exists for. `cos(h1 + h2)` and `sin(h1 + h2)` are the angle-sum identities
/// over the four values `slice_visibility` is already handed, and the half
/// angle follows from `cos(2t) = 1 - 2 sin^2 t = 2 cos^2 t - 1` — two square
/// roots, both exactly specified. Both horizons are clamped into the hemisphere
/// over the surface, so their mean lies in `-pi/2 ..= pi/2`, where the cosine
/// is never negative and the sine takes the sign of `h1 + h2`.
///
/// # What each slice contributes is a *turn of the normal*, not a direction
///
/// **This is the one place the accumulation is not the obvious sum, and the
/// obvious sum is wrong.** A slice's bisector lives in the slice plane, which
/// contains the eye — so summing the bisectors of several slices pulls the
/// answer towards the view direction by an amount that depends on nothing but
/// where the pixel is on screen. Measured on this suite's own wall: at a column
/// two thirds across a 1920-wide frame, a *completely unoccluded* plane facing
/// the camera came back seventeen degrees off its own normal, and it would have
/// swung as the camera panned. Ambient light that follows the eye across flat
/// open geometry is exactly what a bent normal is supposed to stop.
///
/// So a slice contributes the surface normal **turned by the angle its bisector
/// sits from that slice's own unoccluded answer** — which is `gamma`, since an
/// unblocked slice's two horizons clamp to the plane of the surface and their
/// bisector is the surface's own tilt. The turn is about the slice's axis, by
/// Rodrigues, and its cosine and sine are angle differences over values the
/// loop already holds.
///
/// Two properties follow, and both are load-bearing:
///
/// * **An unoccluded pixel gets its own normal, exactly.** Every turn is zero,
///   every slice contributes `normal`, and the mean is `normal` whatever the
///   weights were. A frame with nothing occluding anything is therefore the
///   frame this pass produced before the channel was widened.
/// * **The answer does not depend on where the pixel sits on screen**, because
///   the frame it is accumulated in is the surface's rather than the eye's.
///
/// `crcbl`'s `forward_e2e::occlusion::the_bent_direction_leans_out_of_the_occluded_band`
/// is what holds both: it reads open wall and occluded wall in one frame, and
/// the sum-of-bisectors this replaced fails its open-wall half by an order of
/// magnitude.
///
/// `pixel` is the full-resolution texel this invocation marches from and `tile`
/// is the dither index of the pixel it was invoked for — two arguments rather
/// than one derivation, because the march wants full-resolution texels while the
/// tile wants one entry per pixel of *this pass*. Deriving the tile from `pixel`
/// here would index the tables with [`RESOLUTION_DIVISOR`]-strided coordinates,
/// which collapses the sixteen phases onto the quarter of them that lands on an
/// even row and column — a coarser dither than the file had before it was
/// halved, in both tables at once.
float4 occlusion_at(int2 pixel, uint tile, float3 centre, float3 normal, int2 extent, float2 size)
{
    // The sentinel: no occlusion measured and no direction to report. Every
    // early return below hands this back, and each of them is a pixel with
    // nothing to say — see [`BENT_NORMAL_MIN_LENGTH`].
    const float4 unoccluded = float4(0.0, 0.0, 0.0, 0.0);
    float radius = sampling_radius();

    // The radius in pixels, measured rather than derived from a projection
    // element: `proj` may be perspective or orthographic, and projecting the two
    // ends of a radius is the one spelling that is right for both. `+x` in view
    // space is `+x` on screen, so one axis answers for the whole disc.
    float4 near_clip = mul(camera.proj, float4(centre, 1.0));
    float4 far_clip = mul(camera.proj, float4(centre + float3(radius, 0.0, 0.0), 1.0));
    if (near_clip.w <= 0.0 || far_clip.w <= 0.0)
    {
        return unoccluded;
    }
    float reach = abs(far_clip.x / far_clip.w - near_clip.x / near_clip.w) * 0.5 * size.x;
    if (reach < MIN_RADIUS_PIXELS)
    {
        // Too small to march. See [`MIN_RADIUS_PIXELS`]: every sample would read
        // the centre's own texel, and the answer to that is no occlusion.
        return unoccluded;
    }

    // Towards the eye, which is the axis every horizon is measured from. View
    // space puts the eye at the origin, so this is the position negated.
    float3 view = normalize(-centre);

    float2 seed = SLICE_DIRECTIONS[tile];
    float offset = STEP_OFFSETS[tile];
    uint slices = slice_count();

    float visibility = 0.0;
    float weight = 0.0;
    // The bent direction's accumulator, in view space until the rotation below.
    // Each slice adds its bisector scaled by that slice's own weight *and* by
    // what it could see, which is the paper's weighting: a slice that sees
    // nothing has no unblocked direction to contribute.
    bool bent_wanted = bent_normals();
    float3 bent = float3(0.0, 0.0, 0.0);
    float bent_weight = 0.0;
    // **Bounded by the constant and left early by the count**, rather than
    // running to `slices` directly: the trip count stays something the compiler
    // can see, so the two slices that ship still unroll. The test is on a
    // uniform, so every lane in the draw takes the same side of it.
    for (uint slice = 0; slice < SLICE_COUNT_MAX; ++slice)
    {
        if (slice >= slices)
        {
            break;
        }

        // This slice's marching direction: the tile's entry turned, then
        // normalised — in that order, which is what makes the turn exact. See
        // [`turned`].
        float2 direction = normalize(turned(seed, slice));
        // Screen `y` counts down the framebuffer while view `y` counts up, which
        // is the flip `view_position` carries — so a screen-space step of
        // `(dx, dy)` is a view-space direction of `(dx, -dy)`.
        float3 in_plane = float3(direction.x, -direction.y, 0.0);
        // The slice plane contains the eye and this direction; its normal is the
        // axis both horizons turn about, and the surface normal projected into
        // the plane is what the integral is about.
        float3 axis = normalize(cross(in_plane, view));
        float3 projected = normal - axis * dot(normal, axis);
        float projected_length = length(projected);
        if (projected_length < 1e-6)
        {
            // The surface is edge-on to this slice: it has no orientation inside
            // the plane, so the plane has nothing to say about it. Weighted out
            // rather than counted as unoccluded.
            continue;
        }

        // `gamma`, the surface's tilt inside the slice, signed by which side of
        // the view direction the projected normal leans towards.
        //
        // **Signed against `tangent`, never against `in_plane`.** `in_plane` is
        // a screen-space direction lifted into view space with a zero `z`, so it
        // is perpendicular to the view *axis* and not to `view` — the two only
        // coincide at the exact centre of the frame. Sign the tilt with it and
        // every pixel away from that centre gets a `gamma` leaning the wrong way,
        // which puts both horizon clamps on the wrong sides: a flat floor stops
        // being unoccluded and picks up a smooth wash that grows towards the
        // frame's edges. `tangent` is `in_plane` orthogonalised against `view`,
        // and it is the direction the `+direction` march actually travels.
        float3 tangent = cross(view, axis);
        float cos_gamma = clamp(dot(projected, view) / projected_length, -1.0, 1.0);
        float sign_gamma = dot(tangent, projected) < 0.0 ? -1.0 : 1.0;
        float gamma = sign_gamma * acos_approx(cos_gamma);
        float sin_gamma = sign_gamma * sqrt(saturate(1.0 - cos_gamma * cos_gamma));

        // **Which side is the positive one matters**, because `gamma` is signed
        // against `in_plane`: a horizon found along `+direction` is at a positive
        // angle, and one along `-direction` at a negative. Swapping the two would
        // mirror the surface's tilt inside every slice, which draws a frame that
        // is occluded on the wrong side of each object and is still a picture.
        float cos_negative =
            horizon_cosine(pixel, -direction, offset, reach, centre, view, radius, extent, size);
        float cos_positive =
            horizon_cosine(pixel, direction, offset, reach, centre, view, radius, extent, size);

        // Each horizon clamped to the plane of the surface. Where the clamp binds
        // the angle is `gamma ∓ π/2`, whose cosine and sine are `gamma`'s swapped
        // and signed — see `slice_visibility` on why the caller selects rather
        // than the callee deriving.
        float raw_low = -acos_approx(cos_negative);
        float low = gamma - HALF_PI;
        bool clamped_low = raw_low < low;
        float h1 = clamped_low ? low : raw_low;
        float cos_h1 = clamped_low ? sin_gamma : cos_negative;
        float sin_h1 =
            clamped_low ? -cos_gamma : -sqrt(saturate(1.0 - cos_negative * cos_negative));

        float raw_high = acos_approx(cos_positive);
        float high = gamma + HALF_PI;
        bool clamped_high = raw_high > high;
        float h2 = clamped_high ? high : raw_high;
        float cos_h2 = clamped_high ? -sin_gamma : cos_positive;
        float sin_h2 =
            clamped_high ? cos_gamma : sqrt(saturate(1.0 - cos_positive * cos_positive));

        float slice_visible = slice_visibility(h1, cos_h1, sin_h1, h2, cos_h2, sin_h2,
                                               cos_gamma, sin_gamma);
        visibility += projected_length * slice_visible;
        weight += projected_length;

        if (bent_wanted)
        {
            // The bisector of the visible arc, at `(h1 + h2) / 2` from `view`
            // and turning towards `tangent` — the direction `gamma` and both
            // horizons are signed against, so the same sense answers for all
            // three. See this function's header for why the half angle needs no
            // `sin` or `cos` call.
            float cos_sum = cos_h1 * cos_h2 - sin_h1 * sin_h2;
            float cos_half = sqrt(saturate(0.5 * (1.0 + cos_sum)));
            float sin_half = sqrt(saturate(0.5 * (1.0 - cos_sum)));
            // Signed off the angles themselves rather than off `sin(h1 + h2)`:
            // the two agree everywhere except where the mean is exactly a
            // quarter turn, which is both horizons clamped hard to one side and
            // the one case where the sine of the doubled angle is zero while
            // the half angle's is not.
            sin_half = (h1 + h2) < 0.0 ? -sin_half : sin_half;
            // How far the bisector sits from this slice's unoccluded answer,
            // which is `gamma` — the angle-difference identities over the four
            // values above, so still no trigonometric call.
            float cos_turn = cos_half * cos_gamma + sin_half * sin_gamma;
            float sin_turn = sin_half * cos_gamma - cos_half * sin_gamma;
            // The normal turned by that angle about the slice's axis —
            // Rodrigues, with the cross term negated because the sense the
            // horizons are signed in is `view` towards `tangent`, and
            // `cross(view, tangent)` is `-axis`.
            float3 turned_normal = normal * cos_turn
                                 - cross(axis, normal) * sin_turn
                                 + axis * (dot(normal, axis) * (1.0 - cos_turn));
            float share = projected_length * slice_visible;
            bent += turned_normal * share;
            bent_weight += share;
        }
    }

    if (weight <= 0.0)
    {
        return unoccluded;
    }
    float occlusion = saturate(1.0 - visibility / weight);
    // No slice contributed, or the ones that did cancelled each other out —
    // see [`BENT_NORMAL_MIN_LENGTH`]. Either way there is no preferred
    // direction to report, and the sentinel is the honest answer.
    if (bent_weight <= 0.0 || length(bent / bent_weight) < BENT_NORMAL_MIN_LENGTH)
    {
        return float4(occlusion, 0.0, 0.0, 0.0);
    }
    // View → world for the one value that leaves this pass. `float4(_, 0.0)`
    // drops the translation, which is what a direction wants — `ssr.slang`
    // rotates its reflected ray through the same member the same way.
    return float4(occlusion, normalize(mul(camera.inv_view, float4(bent, 0.0)).xyz));
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // `tonemap.slang`'s triangle exactly: index 0 → uv (0,0), 1 → (2,0),
    // 2 → (0,2), oversized so one primitive covers the viewport.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

/// The bent direction as the three channels beside the scalar: `xyz * 0.5 +
/// 0.5`, and a zero direction lands on `0.5` in each — the value `Rgba8Unorm`
/// quantises to `0x80`, which is the byte the renderer's 1×1 placeholder holds.
/// See the header.
float3 encode_bent(float3 direction)
{
    return direction * 0.5 + 0.5;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    uint width;
    uint height;
    // Read off the image rather than passed in the block: the depth image is the
    // one this pass reads every value from, and a second copy of its size in a
    // uniform is a second thing that can be stale. It is **not** the size of the
    // target — see [`RESOLUTION_DIVISOR`] — and nothing here wants that size: a
    // pixel's dither entry comes off its own coordinate and everything else is
    // measured in the full-resolution texels below.
    scene_depth.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    float2 size = float2(float(width), float(height));

    // **Two pixel coordinates, and the difference is the whole of the halving.**
    // `target` is the pixel of this pass — the target is
    // [`RESOLUTION_DIVISOR`] times smaller on each side than the depth image
    // above — and it is what indexes the dither tables. `pixel` is the
    // full-resolution texel it stands for, and it is what every fetch below
    // reads.
    int2 target = int2(input.position.xy);
    int2 pixel = full_res_pixel(target);
    uint tile = (uint(target.y) & 3u) * 4u + (uint(target.x) & 3u);
    float depth = depth_at(pixel, extent);
    // Nothing was drawn here. Fully unoccluded, and — see [`DEPTH_FAR`] — the
    // only branch that keeps the unprojection from dividing by zero.
    if (depth <= DEPTH_FAR)
    {
        return float4(1.0, encode_bent(float3(0.0, 0.0, 0.0)));
    }

    float3 centre = view_position(pixel, depth, size);
    float3 normal = normal_at(pixel, centre, extent, size);
    float4 gathered = occlusion_at(pixel, tile, centre, normal, extent, size);
    return float4(saturate(1.0 - gathered.x), encode_bent(gathered.yzw));
}
