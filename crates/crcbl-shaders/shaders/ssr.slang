// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s screen-space reflections: the pass that
// marches the depth prepass in **screen space** and writes what it finds.
//
// # It is the reflection alone; `ssr_blur.slang` is the composite
//
// It reads three images — the scene colour, the depth prepass and the
// reflectivity attachment `mesh.slang` writes beside its colour target — and
// writes the reflection into an `Rgba16Float` target of its own, with the weight
// it was computed under in the alpha. `ssr_blur.slang` filters that image and
// adds it to the scene colour, which is why nothing here loads this pixel's own
// lit value: a pass that composited would leave the blur no way to filter the
// reflection without filtering the whole frame with it.
//
// # This file's determinism argument is not `ssao.slang`'s
//
// That file can say a flipped sample costs a pixel an eighth and that its blur
// then divides it by sixteen. **A march has no such denominator**: the first tap
// whose comparison flips *is* the answer, and two rasterisers disagreeing in the
// last bit can tap a neighbouring pixel at the crossing or miss the crossing
// entirely at the last step — the second costs the whole reflection at that
// pixel. So the goldens here are review aids and every real check is a
// structural ratio between two blocks of one frame. What this file does instead,
// and what it deliberately does not:
//
// * **No jitter of any kind**, for the rotation table's reason applied where it
//   matters more. Stepping artefacts get lived with or blurred; they do not get
//   dithered away, because a dither is a per-pixel amplifier of exactly the
//   float differences a golden cannot absorb.
// * **Every weight is continuous and goes to zero exactly where the decision is
//   fragile.** A hit in the last steps of a ray's reach is on
//   [`DISTANCE_FADE`]'s ramp; a hit near the frame edge is on [`BORDER_FADE`]'s;
//   a tap that barely satisfies the thickness bound is on the low end of that
//   one; a ray nearly retracing the view ray is on [`BACKWARD_FADE`]'s. The pixels two drivers can disagree about are,
//   by construction, the pixels whose reflection is multiplied by almost
//   nothing. That is inspectable rather than measured, and it reduces the
//   exposure rather than bounding it.
// * **[`ROUGHNESS_CUTOFF`] makes screen-space marching skip most of every
//   existing frame.** `crcbl_shaders::mesh::GpuMaterial::UNTINTED`'s roughness
//   is 0.5 and the ramp is zero from there up, so a pixel shaded through that row
//   takes only its probe-environment fallback. With a zero probe volume that
//   fallback is zero, and `ssr_blur.slang` leaves the scene colour untouched.
//
// # The march
//
// Screen-space DDA at a fixed pixel stride, and the segment is clipped to the
// viewport *before* the walk so every tap is in bounds by construction. The
// stride is a fixed number of pixels within a frame and the loop bound is a
// constant, which is what keeps the cost a property of the screen rather than of
// the scene's scale — the thing that matters when the same constants have to
// trace a room and a planet, and when CI's rasterisers are software and the loop
// bound is the whole cost. The stride itself comes from [`REACH_FRACTION`], so a
// reflection is the same share of the frame at every resolution rather than
// shrinking as the window grows.
//
// Behind an object the depth buffer has no information, and that is where the
// plausible wrong answer lives: a tap says the ray is behind the *front*
// surface, not how thick that surface is. **A tap counts as a hit only within
// [`thickness_at`]'s bound; past it the tap is no evidence and the march
// continues.** Treating any "behind" as a hit reflects the nearest foreground
// object into every distant reflection and reads as a comet tail off every
// silhouette.
//
// # What is left to the rows after this one
//
// The rough end is **one ray and a blur**, which is honest for a lobe a few
// degrees wide and nothing like a cone trace. `ssr_blur.slang` is that blur —
// `ssao_blur.slang`'s kernel with a second weight on how sharp each tap's own
// surface is — and cone tracing over a colour mip chain is the better technique
// and stays refused, because it needs a `SampleLevel` at a computed LOD and that
// is a filtered read whose level four rasterisers select arithmetic for.
// Upgrading is contained to the blur pass.
//
// A ray that hits nothing returns the irradiance-probe environment. The march's
// confidence blends that environment into a screen hit, so a zeroed probe volume
// still returns zero bit-for-bit while a conductor reflects something outside the
// frame.
//
// # The three functions this file shares with `ssao.slang`
//
// [`depth_at`], [`view_position`] and [`normal_at`] are that file's, **copied
// verbatim** — this repo has no include mechanism by design, because the
// manifest hashes one source per artifact and an `#include` would be a file
// whose edits nothing downstream notices. `ssr_blur.slang` carries a fourth
// copy of the first of them. `crcbl_shaders::ssr`'s
// `the_shared_screen_space_helpers_have_not_drifted` is what holds the copies
// together; two copies is already the bug, and three without a guard is a drift
// with a schedule.

/// Everything this pass needs that is not in a texture.
///
/// **Its own block rather than `ssao.slang`'s**, and the two would have been the
/// same buffer: this pass needs no radius and no bias, and binding the occlusion
/// pair's block would make a pass that is meant to stand alone depend on that
/// pair having run. `crcbl_shaders::ssr::SsrParams` is the Rust mirror and
/// `crcbl_render::forward` is what writes it.
struct SsrParams
{
    /// Clip → view. Reversed-Z and possibly infinite, so `w` is what carries the
    /// depth and the divide below is not optional.
    float4x4 inv_proj;
    /// View → clip, for projecting the reflected ray onto the screen.
    float4x4 proj;
    /// View → world, for evaluating the world-space probe grid.
    float4x4 inv_view;
    /// Matching `crcbl_shaders::probe::ProbeVolume`; kept in this pass's block
    /// because a probe lookup needs it before it can address the storage table.
    float4 probe_origin;
    float4 probe_inv_spacing;
    uint4 probe_counts;
    /// `x` is how many levels of the Hi-Z pyramid this frame built, which is the
    /// deepest level the march may climb to; the other three lanes are the
    /// sixteen-byte row's padding.
    ///
    /// **Zero is a valid frame**: an extent too small to halve once gets no
    /// pyramid at all, the march never leaves level 0, and what it walks is the
    /// full-resolution prepass one texel at a time.
    uint4 hiz;
};

struct GpuProbe
{
    float4 sh_r;
    float4 sh_g;
    float4 sh_b;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsrParams> camera;

/// The depth prepass's output, fetched by integer pixel and never filtered.
///
/// `DepthTexture2D` and the paragraph that goes with it are `ssao.slang`'s: the
/// two spellings are one HLSL declaration and one D3D12 SRV and are **not** the
/// same WGSL, and only this one lowers to a `texture_depth_2d` that a `D32Float`
/// view can fill through the seam's `SampleType::Depth`.
///
/// **No sampler anywhere in this pass.** Every read is a `Load` at an integer
/// pixel, so there is no UV, no filtering and no texel-centre arithmetic for
/// four rasterisers to disagree about — which matters more here than in the
/// occlusion pass, because what a tap fetches is a *colour* rather than a
/// fraction that a blur will average.
[[vk::binding(1, 0)]]
DepthTexture2D scene_depth;

/// The `Rgba16Float` scene target the forward pass wrote.
///
/// Read **once**, at the crossing the march found, which is what the reflection
/// is made of. This pixel's own lit value is not read at all: the composite is
/// `ssr_blur.slang`'s, so what leaves here is the reflection by itself.
/// `Texture2D<float4>` and not `DepthTexture2D` — this is an ordinary colour
/// image, and it lowers to `texture_2d<f32>` against the seam's
/// `SampleType::Float`.
[[vk::binding(2, 0)]]
Texture2D<float4> scene_color;

/// The reflectivity attachment: `rgb` is the surface's `F0` and `a` is SSR's
/// already-evaluated sharpness ramp.
///
/// `mesh.slang` evaluates the ramp before writing its `Rgba8Unorm` attachment,
/// so the zero at [`ROUGHNESS_CUTOFF`] reloads exactly on every backend.
[[vk::binding(3, 0)]]
Texture2D<float4> reflectivity;

/// The same read-only irradiance table the forward pass binds to `mesh.slang`.
/// Appending it keeps Metal's declaration-order argument numbering aligned with
/// the renderer layout.
[[vk::binding(4, 0)]]
StructuredBuffer<GpuProbe> probes;

/// The Hi-Z pyramid `hiz.slang` builds, one binding per level.
///
/// **Level 0 is [`scene_depth`] itself** — the prepass, at full resolution — so
/// these are levels 1 to [`MAX_HIZ_LEVEL`], each half the extent of the one
/// before it and each texel the nearest surface in the block it covers.
///
/// Separate declarations and a `switch` in [`hiz_at`] rather than an array,
/// because WGSL has no arrays of textures and one image with mips is not
/// something the render graph can attach a level of — `crcbl_render::bloom`'s
/// header gives that second reason in full, and this pyramid is built the same
/// way for it.
///
/// **Every slot is always bound.** A frame whose extent is too small for a
/// level of a given depth binds the deepest level it does have into the slots
/// above it, and `SsrParams::hiz_levels` is what stops the march ever reading
/// them — see `crcbl_render::hiz`.
[[vk::binding(5, 0)]]
DepthTexture2D hiz_1;

[[vk::binding(6, 0)]]
DepthTexture2D hiz_2;

[[vk::binding(7, 0)]]
DepthTexture2D hiz_3;

[[vk::binding(8, 0)]]
DepthTexture2D hiz_4;

[[vk::binding(9, 0)]]
DepthTexture2D hiz_5;

/// The reversed-Z far plane: `crcbl_hal::depth::CLEAR`, and the value
/// `ssao.slang` declares under this name.
///
/// **The far plane has no surface.** The unprojection divides by a `clip.w` an
/// infinite reversed-Z projection takes to zero exactly here, so a pixel the
/// geometry never covered can neither start a ray nor stop one — a tap at this
/// depth is a tap at nothing, and the march passes through it.
///
/// `crcbl_shaders::ssao::DEPTH_FAR` is the Rust mirror of both.
static const float DEPTH_FAR = 0.0;

/// The roughness at which the reflection has faded to nothing.
///
/// **This is the statement that the pass is valid only where the lobe is
/// narrow**, not a gate bolted on for the goldens. One ray and
/// `ssr_blur.slang`'s four-pixel kernel stand for a lobe a few degrees wide and
/// for nothing wider, and the failure mode of pretending otherwise is a sharp
/// reflection on a rough surface. The ramp is linear from full strength at a
/// mirror to zero here, so nothing steps, and the value of that ramp is what
/// this pass writes into its alpha for the blur to weigh taps by.
///
/// Half, so `crcbl_shaders::mesh::GpuMaterial::UNTINTED`'s roughness lands on
/// the zero end exactly and every surface in this engine that nobody gave a
/// material to weighs nothing at all. **The blur widens the lobe this pass can
/// stand for and this number has deliberately not moved with it**: raising it
/// past a rough conductor's roughness takes that row in as well, which is a
/// change of blast radius rather than of filtering and is its own slice.
/// `crcbl_shaders::ssr::ROUGHNESS_CUTOFF` is the Rust mirror, and it is what
/// lets a sample's own tests say which of its materials this pass can see.
static const float ROUGHNESS_CUTOFF = 0.5;

/// How far a ray may reach, as a share of the frame's shorter axis.
///
/// **A share of the screen, not a length in the world and not a pixel count.**
/// A world-unit step is tens of pixels near the eye and a fraction of one far
/// away, so the same constants would be a different tracer in a room and on a
/// planet; that is the argument for working in screen space at all. A *fixed
/// pixel* reach has the mirror-image defect one level up — the same scene at
/// twice the resolution would grow twice as many pixels between a surface and
/// what it reflects, so the reflection would get shorter as the window got
/// bigger, and a golden and the review frame of the same fixture would not be
/// pictures of the same thing.
///
/// Three quarters, so a reflection can cross most of the frame it is in, and the
/// shorter axis so a wide window does not quietly buy a longer one.
static const float REACH_FRACTION = 0.75;

/// Iterations a ray may take before it gives up.
///
/// **The cost, and it is the same number at every resolution** — which is what
/// matters when CI's rasterisers are software and the loop bound is the whole
/// price of the pass. It is no longer a *stride*: a hierarchical march spends an
/// iteration either crossing one cell of the level it is on — which is `2^level`
/// texels wide — or dropping a level to look inside one. So the budget buys a
/// reach that grows with the pyramid rather than a fixed number of pixels, and
/// the number itself is the ceiling on the work, not the work.
static const uint MAX_STEPS = 96u;

/// The deepest level of the pyramid a march may climb to, and the number of
/// bindings above [`scene_depth`].
///
/// Five, so the coarsest cell is 32 texels across: with [`MAX_STEPS`] that is
/// three thousand texels of reach for a ray crossing empty space, which is more
/// than any frame this engine renders is wide. A sixth level would be another
/// binding, another pass and another image for reach nothing can use.
/// `crcbl_render::hiz::MAX_LEVELS` is the Rust mirror.
static const uint MAX_HIZ_LEVEL = 5u;

/// How far past a cell boundary a step lands, in texels of the level it is on.
///
/// **A step that lands exactly on the boundary has not left the cell**, and the
/// next iteration would test the same cell again and advance by nothing — a ray
/// running exactly along a texel edge would spend its whole budget standing
/// still. This is the nudge that puts the new position strictly inside the next
/// cell. Small enough that it cannot carry the ray across a second boundary at
/// level 0, where a cell is one texel.
static const float BOUNDARY_NUDGE = 0.00390625;

/// How far short of the vanishing point the segment stops, in pixels.
///
/// **One pixel, and it is a guard rather than a taste.** `w` grows without
/// bound towards the vanishing point, so a segment clipped to land exactly on
/// it solves for a ray parameter of infinity — and `over_w_end` is then a
/// product of an infinite parameter and a zero reciprocal, which is a NaN that
/// every depth along the segment inherits. Every comparison against a NaN is
/// false, so such a ray crosses nothing, finds no hit, and returns the
/// environment: a reflection with holes scattered through it wherever a pixel's
/// own vanishing point happened to land on the clip.
///
/// The fixed-stride march this replaced never met it, and not by design: it
/// floored the segment to a whole number of strides, which pulled the end back
/// off the clip by up to one stride every time.
///
/// A pixel is the right size because the march's finest tap is a texel: there is
/// no reflection to find in the last one before a point the projection cannot
/// describe.
static const float VANISH_MARGIN = 1.0;

/// How far off the surface a ray starts, as a share of the surface's view depth.
///
/// **Or the first tap self-intersects.** A ray leaving exactly on the surface is
/// within the depth buffer's own quantisation of it for the first few steps, and
/// a crossing found there reflects the surface into itself — a uniform
/// brightening that looks like a working reflection and is not one.
///
/// A share of the depth rather than a length, because depth precision and the
/// reconstruction error are both proportional to it: a fixed offset would be
/// invisible near the eye and enormous at the far end of the same frame.
static const float NORMAL_OFFSET = 0.005;

/// The least thickness any surface is credited with, as a share of the ray's own
/// view depth.
///
/// The bound is normally the ray's **own depth advance over one step** — see
/// [`thickness_at`] — which is the honest answer, because a tap that far behind
/// the front surface is a tap the previous step would already have caught. This
/// floor is what keeps a ray running nearly parallel to a surface from having a
/// bound of nothing and never registering a hit at all.
///
/// A share rather than a length, on [`NORMAL_OFFSET`]'s terms: a view-space
/// constant would be a scene-scale number, which is the thing the pixel stride
/// exists to avoid. It is a shader constant either way — not a fourth field the
/// Rust mirror has to agree about.
static const float THICKNESS_FLOOR = 0.02;

/// The share of a ray's reach over which its weight ramps out at the far end.
///
/// **A fragility ramp, not a falloff.** A mirror reflection does not dim with
/// distance — what dims is the confidence, because the last steps are where a
/// crossing is most likely to have been found on one driver and missed on
/// another. So the weight is one over the first three quarters of the reach and
/// falls to nothing over the last quarter, rather than sloping the whole way and
/// halving every honest hit in the middle.
static const float DISTANCE_FADE = 0.25;

/// The width of the border ramp, in NDC.
///
/// **A hard stop draws a visible line where reflections end.** The segment is
/// clipped to the viewport, so a ray that leaves the frame ends on the edge
/// rather than branching — and the weight has to reach zero there, or the edge
/// is a step. In NDC rather than pixels so the ramp is the same share of the
/// frame at every resolution the engine renders at.
static const float BORDER_FADE = 0.15;

/// How far from pointing straight back at the eye a ray may be before it fades.
///
/// A ray reflected back along the view direction asks about a point *behind* the
/// camera, and near that limit its projection barely moves at all: the march
/// covers a handful of pixels while crossing the whole depth of the scene, so
/// every step's advance — and with it [`thickness_at`]'s bound — grows without
/// limit. That is the fragile case, and this is the ramp that reaches zero
/// before it. Measured on `dot(ray, -towards)`, which is one exactly when the
/// ray retraces the view ray, so this width covers the last eighteen degrees.
///
/// **Narrow on purpose.** A surface facing the camera reflects backwards *past*
/// the eye rather than into it, and those rays do find things on screen — a wall
/// panel reflecting the floor at its foot is the case, and a wide ramp here
/// would fade out the one reflection such a surface has.
static const float BACKWARD_FADE = 0.05;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

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

/// NDC → the continuous pixel coordinate of the same point.
///
/// The inverse of the mapping [`view_position`] opens with, and it has to be
/// exactly that: the march reads a depth at `int2` of this and unprojects it
/// through that function, so a half-pixel disagreement between the two would put
/// the ray and the surface it is compared against on different pixels.
float2 pixel_of(float2 ndc, float2 size)
{
    return float2((ndc.x * 0.5 + 0.5) * size.x, (0.5 - ndc.y * 0.5) * size.y);
}

/// The pixel coordinate `at` as NDC — [`pixel_of`] the other way round.
float2 ndc_of(float2 at, float2 size)
{
    return float2(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
}

/// How far behind the front surface a tap may be and still count as a hit.
///
/// **Derived from the ray, not tuned.** `advance` is how much view depth this
/// ray covers in one step: a surface thinner than that would have been crossed
/// between the previous tap and this one, so anything further behind is a tap
/// the march has no evidence about — it is the far side of an object whose
/// thickness the depth buffer never recorded. [`THICKNESS_FLOOR`] is the floor
/// under it, for a ray running nearly parallel to what it is passing.
float thickness_at(float advance, float depth)
{
    return max(advance, abs(depth) * THICKNESS_FLOOR);
}

/// The nearest surface anywhere in the cell of `level` that `texel` falls in.
///
/// Level 0 is [`scene_depth`] and every level above it is one of the pyramid
/// bindings; `extent` is that level's own extent, so the clamp is
/// [`depth_at`]'s and for its reason — a fetch outside the image reads
/// [`DEPTH_FAR`] on some targets and is undefined on others, and either would
/// say "nothing here" along the border of every level.
///
/// A `switch` and not an index, because the five levels are five declarations —
/// see the bindings. The level is not uniform across a quad, and it does not
/// have to be: every read here is a `Load`, which has no implicit derivative and
/// no uniformity rule to break.
float hiz_at(uint level, int2 texel, int2 extent)
{
    int2 clamped = clamp(texel, int2(0, 0), max(extent - int2(1, 1), int2(0, 0)));
    int3 at = int3(clamped, 0);
    switch (level)
    {
    case 0u:
        return scene_depth.Load(at);
    case 1u:
        return hiz_1.Load(at);
    case 2u:
        return hiz_2.Load(at);
    case 3u:
        return hiz_3.Load(at);
    case 4u:
        return hiz_4.Load(at);
    default:
        return hiz_5.Load(at);
    }
}

/// View-space `z` of a point at reversed-Z depth `depth`.
///
/// [`view_position`] without the screen position, and it is the same
/// unprojection: for a perspective projection the view `z` a depth unprojects to
/// does not depend on where in the frame it was sampled, so a cell of the
/// pyramid — which is a *region* of the screen and has no one pixel — still has
/// a well-defined depth to compare a ray against.
float view_z_of(float depth)
{
    float4 view = mul(camera.inv_proj, float4(0.0, 0.0, depth, 1.0));
    return view.z / view.w;
}

/// How far along `forward` the point `at` is from leaving its cell of `size`
/// texels, in pixels.
///
/// The smaller of the two axes' boundary crossings, nudged past the edge by
/// [`BOUNDARY_NUDGE`] so the step lands strictly inside the next cell. An axis
/// the ray does not move along cannot bound the step, and its term is the
/// reach itself rather than a division by nothing.
float cell_exit(float2 at, float2 forward, float size, float reach)
{
    float2 edge = float2(
        forward.x > 0.0 ? (floor(at.x / size) + 1.0) * size : floor(at.x / size) * size,
        forward.y > 0.0 ? (floor(at.y / size) + 1.0) * size : floor(at.y / size) * size);
    float nudge = size * BOUNDARY_NUDGE;
    float along_x = abs(forward.x) < 1e-6
        ? reach
        : ((edge.x + (forward.x > 0.0 ? nudge : -nudge)) - at.x) / forward.x;
    float along_y = abs(forward.y) < 1e-6
        ? reach
        : ((edge.y + (forward.y > 0.0 ? nudge : -nudge)) - at.y) / forward.y;
    return max(min(along_x, along_y), nudge);
}

/// The clamped-cosine transfer of the stored irradiance probe's constant band:
/// `π`. Specular lookup undoes it to recover approximate L1 radiance.
static const float PROBE_TRANSFER_L0 = 3.14159265358979323846;

/// The clamped-cosine transfer of the stored irradiance probe's linear band:
/// `2π/3`. Specular lookup undoes it to recover approximate L1 radiance.
static const float PROBE_TRANSFER_L1 = 2.0943951023931954923;

/// The grid's manually trilinear L1 radiance evaluation in the reflection
/// direction. The stored coefficients are irradiance: undo the clamped-cosine
/// transfer per band before evaluating them. A zero volume addresses row zero of
/// the zeroed table and returns exactly zero.
float3 probe_environment(float3 world_position, float3 direction)
{
    float3 last = max(float3(camera.probe_counts.xyz) - 1.0, float3(0.0, 0.0, 0.0));
    float3 grid = clamp((world_position - camera.probe_origin.xyz) * camera.probe_inv_spacing.xyz,
                        float3(0.0, 0.0, 0.0), last);
    float3 base = floor(grid);
    float3 f = grid - base;
    uint3 lo = uint3(base);
    uint3 hi = uint3(min(base + 1.0, last));
    uint total = max(camera.probe_counts.w, 1u) - 1u;
    uint index000 = min((lo.z * camera.probe_counts.y + lo.y) * camera.probe_counts.x + lo.x, total);
    uint index100 = min((lo.z * camera.probe_counts.y + lo.y) * camera.probe_counts.x + hi.x, total);
    uint index010 = min((lo.z * camera.probe_counts.y + hi.y) * camera.probe_counts.x + lo.x, total);
    uint index110 = min((lo.z * camera.probe_counts.y + hi.y) * camera.probe_counts.x + hi.x, total);
    uint index001 = min((hi.z * camera.probe_counts.y + lo.y) * camera.probe_counts.x + lo.x, total);
    uint index101 = min((hi.z * camera.probe_counts.y + lo.y) * camera.probe_counts.x + hi.x, total);
    uint index011 = min((hi.z * camera.probe_counts.y + hi.y) * camera.probe_counts.x + lo.x, total);
    uint index111 = min((hi.z * camera.probe_counts.y + hi.y) * camera.probe_counts.x + hi.x, total);
    GpuProbe x00 = probes[index000];
    GpuProbe x10 = probes[index010];
    GpuProbe x01 = probes[index001];
    GpuProbe x11 = probes[index011];
    GpuProbe y00 = probes[index100];
    GpuProbe y10 = probes[index110];
    GpuProbe y01 = probes[index101];
    GpuProbe y11 = probes[index111];
    GpuProbe z0;
    z0.sh_r = lerp(lerp(x00.sh_r, y00.sh_r, f.x), lerp(x10.sh_r, y10.sh_r, f.x), f.y);
    z0.sh_g = lerp(lerp(x00.sh_g, y00.sh_g, f.x), lerp(x10.sh_g, y10.sh_g, f.x), f.y);
    z0.sh_b = lerp(lerp(x00.sh_b, y00.sh_b, f.x), lerp(x10.sh_b, y10.sh_b, f.x), f.y);
    GpuProbe z1;
    z1.sh_r = lerp(lerp(x01.sh_r, y01.sh_r, f.x), lerp(x11.sh_r, y11.sh_r, f.x), f.y);
    z1.sh_g = lerp(lerp(x01.sh_g, y01.sh_g, f.x), lerp(x11.sh_g, y11.sh_g, f.x), f.y);
    z1.sh_b = lerp(lerp(x01.sh_b, y01.sh_b, f.x), lerp(x11.sh_b, y11.sh_b, f.x), f.y);
    GpuProbe cell;
    cell.sh_r = lerp(z0.sh_r, z1.sh_r, f.z);
    cell.sh_g = lerp(z0.sh_g, z1.sh_g, f.z);
    cell.sh_b = lerp(z0.sh_b, z1.sh_b, f.z);
    return max(float3(
                   dot(cell.sh_r.xyz / PROBE_TRANSFER_L1, direction) + cell.sh_r.w / PROBE_TRANSFER_L0,
                   dot(cell.sh_g.xyz / PROBE_TRANSFER_L1, direction) + cell.sh_g.w / PROBE_TRANSFER_L0,
                   dot(cell.sh_b.xyz / PROBE_TRANSFER_L1, direction) + cell.sh_b.w / PROBE_TRANSFER_L0),
               float3(0.0, 0.0, 0.0));
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
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    uint width;
    uint height;
    // Read off the image rather than passed in the block: the pass renders at the
    // depth buffer's own size by construction, and a second copy of that number
    // in a uniform is a second thing that can be stale.
    scene_depth.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    float2 size = float2(float(width), float(height));
    int2 pixel = int2(input.position.xy);

    // **What a pixel with no reflection writes**, and every pixel of the target
    // has to be written. The alpha is the ramp `ssr_blur.slang` weighs taps by,
    // so a zero there says "this surface is not one of mine" to a neighbouring
    // reflector — which is the same statement as an rgb of zero for a surface
    // that reflects and found nothing, and deliberately a different one.
    const float4 NOTHING = float4(0.0, 0.0, 0.0, 0.0);

    float4 surface = reflectivity.Load(int3(pixel, 0));
    float sharpness = surface.a;
    float depth = depth_at(pixel, extent);
    // The far plane has no surface, so it has neither a probe lookup nor a ray.
    if (depth <= DEPTH_FAR)
    {
        return NOTHING;
    }

    float3 origin = view_position(pixel, depth, size);
    float3 normal = normal_at(pixel, origin, extent, size);
    // Eye → surface, and the eye is the origin of view space.
    float3 towards = normalize(origin);
    float3 ray = reflect(towards, normal);

    float3 world_position = mul(camera.inv_view, float4(origin, 1.0)).xyz;
    float3 reflection_direction = normalize(mul(camera.inv_view, float4(ray, 0.0)).xyz);
    float3 environment = probe_environment(world_position, reflection_direction);

    // Schlick, and it is `mesh.slang`'s expansion rather than a `pow`: the same
    // fragment evaluated the same Fresnel term for its lights, and a reflection
    // is the same lobe pointed at the room instead of at a light.
    float n_dot_v = saturate(dot(normal, -towards));
    float3 f0 = surface.rgb;
    float grazing = 1.0 - n_dot_v;
    float grazing2 = grazing * grazing;
    float3 fresnel = f0 + (float3(1.0, 1.0, 1.0) - f0) * (grazing2 * grazing2 * grazing);

    // A probe is a blurry environment, so it remains valid when a single
    // screen-space ray is not. Its zero alpha keeps it out of the sharpness
    // kernel; `ssr_blur.slang` composites this centre value directly.
    if (sharpness <= 0.0)
    {
        return float4(environment * fresnel, 0.0);
    }

    // A ray retracing the view ray asks about a point behind the camera.
    float facing = saturate((1.0 - dot(ray, -towards)) / BACKWARD_FADE);

    // The ray starts off the surface — see [`NORMAL_OFFSET`].
    float3 start = origin + normal * (abs(origin.z) * NORMAL_OFFSET);
    // Clip space is affine in the ray parameter, which is what lets the segment
    // be clipped and interpolated without a second projection per step.
    float4 clip_start = mul(camera.proj, float4(start, 1.0));
    float4 clip_ray = mul(camera.proj, float4(ray, 0.0));
    if (clip_start.w <= 0.0)
    {
        return float4(environment * fresnel, sharpness);
    }
    float2 at_start = pixel_of(clip_start.xy / clip_start.w, size);

    // The screen-space direction, from the derivative of the projected point at
    // the ray's origin. A ray whose projection does not move is one pointed
    // along the view ray, and there is no line to walk.
    float2 ndc_rate =
        (clip_ray.xy * clip_start.w - clip_start.xy * clip_ray.w) / (clip_start.w * clip_start.w);
    float2 screen_rate = float2(ndc_rate.x * 0.5 * size.x, -ndc_rate.y * 0.5 * size.y);
    float rate = length(screen_rate);
    if (rate < 1e-6)
    {
        return float4(environment * fresnel, sharpness);
    }
    float2 forward = screen_rate / rate;

    // --- clip the segment, before the walk ---
    //
    // Every tap is then in bounds by construction and a ray leaving the screen
    // stops being a branch. The reach starts at [`REACH_FRACTION`] of the frame
    // and is cut down by each viewport edge the segment would cross.
    float reach = REACH_FRACTION * min(size.x, size.y);
    float travel = reach;
    if (forward.x > 0.0)
    {
        travel = min(travel, (size.x - 1.0 - at_start.x) / forward.x);
    }
    else if (forward.x < 0.0)
    {
        travel = min(travel, -at_start.x / forward.x);
    }
    if (forward.y > 0.0)
    {
        travel = min(travel, (size.y - 1.0 - at_start.y) / forward.y);
    }
    else if (forward.y < 0.0)
    {
        travel = min(travel, -at_start.y / forward.y);
    }

    // And against one of the two ends the ray has in depth, which of them
    // depending on which way `w` runs along it. The two are exclusive: `w` is
    // affine in the ray parameter, so it either rises for ever or falls to zero.
    if (clip_ray.w > 0.0)
    {
        // **The vanishing point, and the walk must stop at it.** The whole
        // infinite ray projects into the segment between here and there, so a
        // pixel past it is the projection of a point *behind the eye* — the
        // screen line simply continues and the depths it implies are nonsense.
        // The viewport is a rectangle and knows nothing about that, so clipping
        // to the frame alone lets a ray whose vanishing point is on screen — a
        // reflection off a floor, which is most of them — walk straight through
        // infinity and keep marching.
        float2 at_vanish = pixel_of(clip_ray.xy / clip_ray.w, size);
        // [`VANISH_MARGIN`] short of it, because landing on it exactly is what
        // turns every depth along the segment into a NaN.
        travel = min(travel, max(dot(at_vanish - at_start, forward) - VANISH_MARGIN, 0.0));
    }
    else if (clip_ray.w < 0.0)
    {
        // The near plane, or a ray pointing towards the camera crosses `w <= 0`
        // and every projected coordinate after it is nonsense. The plane is read
        // off the inverse projection — reversed-Z puts it at depth 1 — rather
        // than carried in the block, so there is no fourth number to keep in
        // step.
        float4 on_near = mul(camera.inv_proj, float4(0.0, 0.0, 1.0, 1.0));
        float near = -on_near.z / on_near.w;
        // `clip_start.w` is a visible surface's view depth, so it is at or past
        // the near plane and this parameter is positive.
        float when = (near - clip_start.w) / clip_ray.w;
        float4 clip_near = clip_start + clip_ray * when;
        float2 at_near = pixel_of(clip_near.xy / clip_near.w, size);
        // The projected point moves monotonically along `forward` while `w` stays
        // positive, so the crossing is ahead rather than behind.
        travel = min(travel, max(dot(at_near - at_start, forward), 0.0));
    }

    travel = max(travel, 0.0);
    if (travel <= BOUNDARY_NUDGE)
    {
        return float4(environment * fresnel, sharpness);
    }

    // The ray parameter at the far end, solved on the screen axis the segment
    // moves fastest along: that axis has the largest denominator and so the
    // best-conditioned division.
    float2 at_end = at_start + forward * travel;
    float2 ndc_end = ndc_of(at_end, size);
    float when_end = abs(forward.x) >= abs(forward.y)
        ? (ndc_end.x * clip_start.w - clip_start.x) / (clip_ray.x - ndc_end.x * clip_ray.w)
        : (ndc_end.y * clip_start.w - clip_start.y) / (clip_ray.y - ndc_end.y * clip_ray.w);
    // Written to reject a NaN as well as a negative. The clip above stops the
    // segment *at* the vanishing point, so the denominator vanishes only when the
    // last step lands exactly on it — the segment reaching infinity. There is no
    // reflection to compute for such a ray, and the miss is the honest answer,
    // stated here rather than left to arrive as a NaN that every comparison below
    // happens to answer `false` to.
    if (!(when_end > 0.0) || !isfinite(when_end))
    {
        return float4(environment * fresnel, sharpness);
    }

    // `1/w` and `view/w` are the quantities linear in the *screen* parameter, so
    // the depth of the ray at each tap is a lerp of two endpoint values and one
    // divide — no reprojection inside the loop.
    float inverse_w_start = 1.0 / clip_start.w;
    float w_end = clip_start.w + when_end * clip_ray.w;
    float inverse_w_end = 1.0 / w_end;
    float over_w_start = start.z * inverse_w_start;
    float over_w_end = (start.z + when_end * ray.z) * inverse_w_end;

    // The march begins at the ray's own origin, which sits [`NORMAL_OFFSET`] in
    // front of the surface it left — so it starts on the near side of
    // everything, which is what a crossing is measured from.
    float3 reflection = environment * fresnel;
    uint max_level = min(camera.hiz.x, MAX_HIZ_LEVEL);

    // --- the hierarchical march ---
    //
    // One iteration crosses one cell of the level the ray is on, or drops a
    // level to look inside a cell that is not empty. A cell of level `n` is
    // `2^n` texels across, so a ray over open space climbs to the coarsest level
    // it has and crosses the frame in a handful of iterations, while a ray about
    // to hit something walks back down to a single texel before it decides.
    // **The walk starts at the far side of the ray's own texel.** That texel
    // holds the surface the ray left, and a cell of one texel is short enough
    // that the ray is still inside it — so testing it would compare the ray
    // against the very surface it is reflecting off, and a surface sloping away
    // under the ray would answer "behind" and reflect a pixel in itself.
    float at_travel = min(cell_exit(at_start, forward, 1.0, travel), travel);
    uint level = 0u;
    float entry_z = start.z;
    // **How far in front of the frame the ray was when it entered this cell**,
    // carried across iterations rather than recomputed from the cell it is now
    // in. That distinction is the whole crossing test: a ray reaching the first
    // texel of a surface is behind *that* texel at both ends of it — a texel is
    // one pixel and a ray barely changes depth crossing one — so a test that
    // compared both ends against the same cell would see no sign change and
    // march on through every surface it met. What changed sign is the gap
    // against the cell *before* this one, which is where the frame was still
    // showing whatever stood behind.
    //
    // It starts at the ray's own offset from the surface it left:
    // [`NORMAL_OFFSET`] puts the origin in front of that surface, which is the
    // near side of everything and what a crossing is measured from.
    float previous_gap = start.z - origin.z;

    for (uint step = 0u; step < MAX_STEPS; ++step)
    {
        float cell = float(1u << level);
        float2 at = at_start + forward * at_travel;
        float exit_travel = min(at_travel + cell_exit(at, forward, cell, travel), travel);
        float2 exit_at = at_start + forward * exit_travel;
        float along = exit_travel / travel;
        float exit_z =
            lerp(over_w_start, over_w_end, along) / lerp(inverse_w_start, inverse_w_end, along);

        // The level's own extent — the frame halved once per level, which is how
        // `crcbl_render::hiz` sizes the images — and the cell this point is in.
        int2 level_extent = int2(extent.x >> level, extent.y >> level);
        float cell_depth = hiz_at(level, int2(floor(at / cell)), level_extent);
        // The far plane has no surface, so a cell holding nothing else is a cell
        // the ray crosses whatever its own depth: a hole in the evidence rather
        // than the end of the ray.
        float gap = cell_depth <= DEPTH_FAR ? 1.0 : exit_z - view_z_of(cell_depth);
        // View `z` is negative ahead of the eye, so a **larger** `z` is nearer:
        // a positive gap is a ray still in front of what this cell holds. The
        // ray's `z` runs monotonically along the segment, so a ray in front of
        // the cell's nearest surface at the *exit* point was in front of it
        // everywhere in the cell — which is what makes one comparison stand for
        // the whole crossing.
        bool in_front = gap > 0.0;

        if (!in_front && level > 0u)
        {
            // Something in this cell is in the way and the cell is a region
            // rather than a texel. Look inside it instead of advancing — the
            // position does not move, so no part of the segment is skipped.
            level -= 1u;
            continue;
        }

        if (!in_front && previous_gap > 0.0)
        {
            // Level 0: the cell **is** one texel and `cell_depth` is that texel's
            // own prepass value, so this is the crossing itself rather than a
            // reduction of one.
            float behind = -gap;
            float thickness = thickness_at(abs(exit_z - entry_z), exit_z);
            if (behind <= thickness)
            {
                // Linear crossing between the last two gaps. A bisection is a
                // cascade of binary comparisons; this is arithmetic on two values
                // already computed.
                float crossed = previous_gap / max(previous_gap - gap, 1e-8);
                float2 hit_at = lerp(at, exit_at, crossed);
                int2 hit = clamp(int2(hit_at), int2(0, 0), extent - int2(1, 1));

                float2 hit_ndc = ndc_of(hit_at, size);
                float border =
                    saturate((1.0 - max(abs(hit_ndc.x), abs(hit_ndc.y))) / BORDER_FADE);
                // Against the ray's whole [`REACH_FRACTION`] reach rather than
                // against the segment the viewport clipped it to, so a ray that
                // ended early because it left the frame is faded by the border
                // ramp instead of twice — and over [`DISTANCE_FADE`] of that
                // reach rather than the whole of it, because what this ramps out
                // is the fragile end and not a falloff.
                float distance = saturate((1.0 - exit_travel / reach) / DISTANCE_FADE);
                float bound = saturate(1.0 - behind / thickness);

                float confidence = sharpness * facing * border * distance * bound;
                float3 hit_color = scene_color.Load(int3(hit, 0)).rgb;
                reflection = hit_color * fresnel * confidence
                    + environment * fresnel * (1.0 - confidence);
                break;
            }
            // Past the bound: the tap is no evidence and the march continues
            // through it — see [`thickness_at`].
        }

        previous_gap = gap;
        entry_z = exit_z;
        at_travel = exit_travel;
        if (at_travel >= travel)
        {
            break;
        }
        // Climb: the cell just crossed was empty of anything in the ray's way, so
        // the next one may be tested at twice the size. A march that never
        // climbed would be the fixed-stride walk this replaced.
        level = min(level + 1u, max_level);
    }

    // **The alpha is [`ROUGHNESS_CUTOFF`]'s ramp, not an opacity**, and it is
    // what `ssr_blur.slang` weighs its taps by: two pixels the march trusted
    // equally are two pixels whose reflections are the same kind of thing, and
    // the blur needs that without a second read of the reflectivity attachment
    // and a second copy of the constant that turns roughness into it.
    return float4(reflection, sharpness);
}
