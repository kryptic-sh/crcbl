// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s screen-space ambient occlusion: the pass
// that reads the depth prepass and writes one `R8Unorm` channel the forward
// pass multiplies its **ambient** term by.
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
// **How many of the four a frame sweeps is `camera.params.y`**, and two is what
// ships. See [`SLICE_COUNT_MAX`] for what the other two buy and cost.
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
    /// `x` is the sampling radius in **world units** and `y` is how many planes
    /// each pixel sweeps — see [`slice_count`], which is what reads it. `z` is
    /// the intensity exponent `ssao_upsample.slang` raises its reconstruction
    /// to, written by the host into the block all three of these passes share
    /// and read by that one alone; `w` is what is left padding the row out to
    /// the sixteen bytes `std140` gives a uniform buffer. Nothing in *this*
    /// file reads either.
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

/// The default, and the count every golden in this workspace was blessed at.
///
/// Two slices sweep the tile's own direction and its quarter turn, which is what
/// this file did before it could be asked for more.
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
/// All three passes of the chain map the same way, which is what lets
/// `ssao_blur.slang` weight a tap by the depth this file actually gathered it
/// at, and `ssao_upsample.slang` know where each half-resolution sample sits.
/// `crcbl_shaders::ssr`'s drift guard holds the three copies together.
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
    float4 view = mul(camera.inv_proj, float4(ndc, depth, 1.0));
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

/// How much of the hemisphere over `centre` is blocked, in `0..=1`.
///
/// **Replace this function, and only this function, to upgrade the technique.**
/// It has been replaced once already, which is what that line was for.
///
/// `pixel` is the full-resolution texel this invocation marches from and `tile`
/// is the dither index of the pixel it was invoked for — two arguments rather
/// than one derivation, because the march wants full-resolution texels while the
/// tile wants one entry per pixel of *this pass*. Deriving the tile from `pixel`
/// here would index the tables with [`RESOLUTION_DIVISOR`]-strided coordinates,
/// which collapses the sixteen phases onto the quarter of them that lands on an
/// even row and column — a coarser dither than the file had before it was
/// halved, in both tables at once.
float occlusion_at(int2 pixel, uint tile, float3 centre, float3 normal, int2 extent, float2 size)
{
    float radius = camera.params.x;

    // The radius in pixels, measured rather than derived from a projection
    // element: `proj` may be perspective or orthographic, and projecting the two
    // ends of a radius is the one spelling that is right for both. `+x` in view
    // space is `+x` on screen, so one axis answers for the whole disc.
    float4 near_clip = mul(camera.proj, float4(centre, 1.0));
    float4 far_clip = mul(camera.proj, float4(centre + float3(radius, 0.0, 0.0), 1.0));
    if (near_clip.w <= 0.0 || far_clip.w <= 0.0)
    {
        return 0.0;
    }
    float reach = abs(far_clip.x / far_clip.w - near_clip.x / near_clip.w) * 0.5 * size.x;
    if (reach < MIN_RADIUS_PIXELS)
    {
        // Too small to march. See [`MIN_RADIUS_PIXELS`]: every sample would read
        // the centre's own texel, and the answer to that is no occlusion.
        return 0.0;
    }

    // Towards the eye, which is the axis every horizon is measured from. View
    // space puts the eye at the origin, so this is the position negated.
    float3 view = normalize(-centre);

    float2 seed = SLICE_DIRECTIONS[tile];
    float offset = STEP_OFFSETS[tile];
    uint slices = slice_count();

    float visibility = 0.0;
    float weight = 0.0;
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

        visibility += projected_length
                    * slice_visibility(h1, cos_h1, sin_h1, h2, cos_h2, sin_h2,
                                       cos_gamma, sin_gamma);
        weight += projected_length;
    }

    if (weight <= 0.0)
    {
        return 0.0;
    }
    return saturate(1.0 - visibility / weight);
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

[shader("fragment")]
float fragmentMain(FullscreenOutput input) : SV_Target
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
        return 1.0;
    }

    float3 centre = view_position(pixel, depth, size);
    float3 normal = normal_at(pixel, centre, extent, size);
    return saturate(1.0 - occlusion_at(pixel, tile, centre, normal, extent, size));
}
