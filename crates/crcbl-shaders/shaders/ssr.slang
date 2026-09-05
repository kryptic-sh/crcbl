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
/// How many clipmap levels the irradiance volume's header carries room for,
/// matching `crcbl_shaders::probe::PROBE_LEVELS` and `mesh.slang`'s copy. Sizes
/// two arrays of the block below, so a file that thought it was smaller would
/// read every member after them at the wrong offset.
static const uint PROBE_LEVELS = 4;

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
    ///
    /// `probe_counts.xyz` is the probes one clipmap level holds on each axis and
    /// `.w` is the rows the whole table holds; `probe_levels.x` is how many
    /// levels are live and `.y` how many rows one of them owns. The per-level
    /// origin and reciprocal spacing are uploaded rather than derived here, for
    /// the reason `mesh.slang`'s copy of these rows gives.
    uint4 probe_counts;
    uint4 probe_levels;
    float4 probe_level_origin[PROBE_LEVELS];
    float4 probe_level_inv_spacing[PROBE_LEVELS];
    /// How far each level has scrolled, as whole probe steps already reduced
    /// into `0..probe_counts` — `mesh.slang`'s `probe_level_offset`, on this
    /// pass's own block. `probe_wrap` is where it is applied.
    uint4 probe_level_offset[PROBE_LEVELS];
    /// `x` is how many levels of the Hi-Z pyramid this frame built, which is the
    /// deepest level the march may climb to; the other three lanes are the
    /// sixteen-byte row's padding.
    ///
    /// **Zero is a valid frame**: an extent too small to halve once gets no
    /// pyramid at all, the march never leaves level 0, and what it walks is the
    /// full-resolution prepass one texel at a time.
    uint4 hiz;
    /// The gradient sky a ray that hit nothing sees: zenith, horizon and
    /// ground, each a linear RGB radiance in `xyz` with `w` unread.
    ///
    /// **The gradient and not its L1 projection**, which is where this block
    /// and `mesh.slang`'s deliberately disagree about how to carry one sky.
    /// The ambient term wants the environment's cosine-weighted integral and
    /// L1 *is* that integral; a reflection wants the radiance along one
    /// direction, and rebuilding that from four irradiance coefficients would
    /// blur a gradient this pass can evaluate exactly.
    ///
    /// `crcbl_shaders::sky::SkyGradient` is the host side; `sky_prefiltered`
    /// below weights these rows by `sky_prefilter`'s table at the surface's
    /// roughness, and a zeroed sky adds nothing to the probe environment a
    /// missed ray already had.
    ///
    /// **Last in the block**, so no member above it moves.
    float4 sky[3];
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

/// The reflectivity attachment: `rgb` is the surface's `F0` and `a` is the
/// lobe's roughness.
///
/// `mesh.slang` quantises the roughness to the attachment's own levels before
/// the store, so it reloads as the same level on every backend and the ramp
/// [`sharpness_of`] derives from it is the same number everywhere. A surface at
/// [`ROUGHNESS_CUTOFF`] — `GpuMaterial::UNTINTED` — reloads as the level above
/// it and derives exactly zero.
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

/// The gradient sky convolved against the lobe, `crcbl_shaders::sky_prefilter`'s
/// table as an `Rgba8Unorm` image: `rg` is `W_far` and `ba` is `W_opposite`,
/// each a 16-bit fixed-point share with its high byte first, over
/// `(|R.y|, roughness)`. **After the pyramid**, so Metal's declaration-order
/// argument numbering stays aligned with the renderer layout.
///
/// Read with four `Load`s and filtered here, on `mesh.slang`'s
/// `specular_albedo_at` terms: a hardware filter is not blessable across four
/// rasterisers, and the table is baked on texel centres.
[[vk::binding(10, 0)]]
Texture2D<float4> sky_prefilter;

/// The split-sum's other half, `crcbl_shaders::dfg`'s table as an `Rgba8Unorm`
/// image: `rg` is `scale` and `ba` is `bias`, each a 16-bit fixed-point share
/// with its high byte first, over `(N·V, roughness)` —
/// `crcbl_shaders::dfg::pair_texels`. `mesh.slang` reads the same table summed
/// to one channel for energy compensation; a reflection wants the pair,
/// because `f0 * scale + bias` is what the lobe's Fresnel and visibility
/// integrate to over the environment it gathers, and Schlick along one
/// direction is only that at a roughness of zero.
[[vk::binding(11, 0)]]
Texture2D<float4> dfg;

/// `docs/plan/50-irradiance-probes.md`'s per-probe **visibility map**, the very
/// image the forward pass binds to `mesh.slang`: one layer per row of
/// [`probes`], `r` the mean distance from that probe to the nearest static
/// surface in a direction and `g` the mean of its square.
/// `crcbl_shaders::probe_visibility` owns the layout and the octahedral mapping,
/// and `crcbl_render::probe_visibility` is what captures it.
///
/// # A reflection leaks through a wall without it
///
/// [`probe_level_environment`] reads the same eight rows `mesh.slang`'s diffuse
/// gather does, so a probe grid whose diffuse term is weighed by this map and
/// whose specular fallback is not still lights the room next door — through the
/// mirror rather than through the wall. [`probe_weight`] is the Chebyshev bound
/// that closes it, and it is `mesh.slang`'s body character for character;
/// `crcbl_shaders::probe_visibility`'s
/// `the_shaders_weigh_a_probe_the_way_this_module_does` is what holds the two
/// copies to each other.
///
/// # Read by four `Load`s and a blend written out
///
/// No sampler, like every other read in this pass: a hardware filter is
/// fixed-function arithmetic four rasterisers compute independently, and it also
/// means the image's format need not be filterable — `rg32float` is not, without
/// WebGPU's `float32-filterable`.
///
/// **A `float4` element on a two-channel image**, read as `.xy`, which is the
/// declaration `mesh.slang` makes of the same image.
///
/// # Bound on every path, and a one-texel image is the off switch
///
/// `crcbl_render::ssr` binds `crcbl_render::forward`'s 1×1×1 image holding
/// `crcbl_shaders::probe_visibility::FAR` when nothing has been captured or when
/// `r_probe_visibility` is off. No surface is further away than that, so every
/// probe keeps all of its weight and this file has no branch for the feature
/// being absent. The fetch is clamped against `GetDimensions`, which is what
/// lets the one-texel image be read by the same code as a captured one.
///
/// **Appended past [`dfg`], never inserted**: `crcbl-mtl` numbers a resource by
/// counting the same-table entries of the layout list, and Slang numbers a
/// stage's arguments by declaration order, so the two agree only while both
/// ascend.
[[vk::binding(12, 0)]]
Texture2DArray<float4> probe_visibility;

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

/// The sharpness ramp at `roughness`: full strength at a mirror, nothing at
/// [`ROUGHNESS_CUTOFF`], linear between.
///
/// Derived here from the roughness the attachment carries rather than stored by
/// `mesh.slang`, so the cutoff is one constant in one file and the attachment
/// keeps the roughness itself for the environment lookup.
float sharpness_of(float roughness)
{
    return saturate(1.0 - roughness / ROUGHNESS_CUTOFF);
}

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
    float2 view = unproject_z(depth);
    return view.x / view.y;
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

/// How much of a clipmap level's half-extent the blend into the next level out
/// occupies, matching `crcbl_shaders::probe::LEVEL_BAND` and `mesh.slang`'s
/// copy. That module is where the reason is argued.
static const float PROBE_LEVEL_BAND = 0.25;

/// Texels along one side of a visibility map's interior — the part that holds
/// directions. `crcbl_shaders::probe_visibility::SIDE`, and
/// `the_visibility_map_constants_match_the_ones_the_shaders_declare` is what
/// holds this file, `mesh.slang` and that module to one another.
static const float PROBE_VISIBILITY_SIDE = 16.0;

/// Texels of wrapped border on each side of that interior, which is what lets
/// [`probe_moments`] read four ordinary neighbours everywhere.
/// `crcbl_shaders::probe_visibility::BORDER`.
static const float PROBE_VISIBILITY_BORDER = 1.0;

/// How far along its own normal a shaded point is moved before its distance to a
/// probe is measured, in world units.
/// `crcbl_shaders::probe_visibility::SURFACE_BIAS`, where the reason is argued.
static const float PROBE_SURFACE_BIAS = 0.05;

/// The share of its trilinear weight a fully occluded corner keeps.
/// `crcbl_shaders::probe_visibility::OCCLUDED_WEIGHT`, where the reason is
/// argued: it is what stops the weighted sum's divisor reaching zero.
static const float PROBE_OCCLUDED_WEIGHT = 0.0001;

/// One texel of `sky_prefilter` or of `dfg` as the two shares it encodes.
///
/// Each channel pair arrives as the stored bytes over 255, a conversion every
/// backend defines exactly; multiplying back by 255 recovers the bytes, and
/// `high * 256 + low` over 65535 is the fixed point the cook wrote —
/// `crcbl_shaders::dfg::decode_albedo`'s arithmetic, once per share.
/// `crcbl_shaders::sky_prefilter`'s
/// `the_texels_encode_the_committed_table_to_a_quantisation_step` and
/// `crcbl_shaders::dfg`'s `the_pair_texels_encode_the_committed_table_to_a_
/// quantisation_step` hold the two encodings to it, and
/// `the_shader_decodes_the_texels_this_module_writes` holds this spelling.
float2 decode_fixed_pair(float4 texel)
{
    return float2(texel.r * 65280.0 + texel.g * 255.0, texel.b * 65280.0 + texel.a * 255.0)
         / 65535.0;
}

/// A fixed-point pair table at `at`, bilinear over the four texels around the
/// point with the addressing a clamped sampler would use — both tables are
/// baked on texel centres, so an axis maps to `value * size - 0.5` and both
/// ends clamp to the edge texel. `crcbl_shaders::sky_prefilter::sample` and
/// `crcbl_shaders::dfg::sample` are the same filter on the CPU, and
/// `mesh.slang`'s `specular_albedo_at` the same shape over one channel.
float2 fixed_pair_at(Texture2D<float4> table, float2 at)
{
    uint width;
    uint height;
    table.GetDimensions(width, height);
    float2 extent = float2(float(width), float(height));
    float2 scaled = saturate(at) * extent - 0.5;
    float2 low = clamp(floor(scaled), float2(0.0, 0.0), extent - 1.0);
    float2 high = min(low + 1.0, extent - 1.0);
    float2 weight = clamp(scaled - low, 0.0, 1.0);

    int2 lo = int2(low);
    int2 hi = int2(high);
    float2 top = lerp(decode_fixed_pair(table.Load(int3(lo.x, lo.y, 0))),
                      decode_fixed_pair(table.Load(int3(hi.x, lo.y, 0))),
                      weight.x);
    float2 bottom = lerp(decode_fixed_pair(table.Load(int3(lo.x, hi.y, 0))),
                         decode_fixed_pair(table.Load(int3(hi.x, hi.y, 0))),
                         weight.x);
    return lerp(top, bottom, weight.y);
}

/// `W_far` and `W_opposite` at `(up, roughness)`.
float2 sky_prefilter_at(float up, float roughness)
{
    return fixed_pair_at(sky_prefilter, float2(up, roughness));
}

/// `scale` and `bias` at `(n_dot_v, roughness)`.
float2 dfg_at(float n_dot_v, float roughness)
{
    return fixed_pair_at(dfg, float2(n_dot_v, roughness));
}

/// The gradient sky as a lobe of `roughness` about `direction` sees it:
/// `crcbl_shaders::sky_prefilter::prefiltered_radiance`, spelled again because
/// Slang has no `#include` for either side to share, and
/// `the_shader_spells_the_prefiltered_sum` is what keeps the two together.
///
/// The gradient is linear in its three colours, so the convolution is the
/// three bands weighted: the pole on `direction`'s side of the horizon takes
/// `W_far`, the opposite pole `W_opposite` and the horizon the rest. A mirror
/// reads the gradient itself — the table's first column is the sky's own
/// smoothstep — and the roughest lobe sees the horizon from every direction.
/// No `pow` anywhere: `docs/plan/44-lighting.md`'s rule forbids a
/// transcendental reaching a colour, and the lobe was integrated at the cook.
float3 sky_prefiltered(float3 direction, float roughness)
{
    float up = clamp(direction.y, -1.0, 1.0);
    float2 weights = sky_prefilter_at(abs(up), roughness);
    float3 far = up >= 0.0 ? camera.sky[0].rgb : camera.sky[2].rgb;
    float3 opposite = up >= 0.0 ? camera.sky[2].rgb : camera.sky[0].rgb;
    float horizon = 1.0 - weights.x - weights.y;
    return camera.sky[1].rgb * horizon + far * weights.x + opposite * weights.y;
}

/// `+1` for a positive or zero value and `-1` for a negative one — the
/// octahedral mapping's `signNotZero`, and `mesh.slang`'s body character for
/// character.
float sign_not_zero(float value)
{
    return value >= 0.0 ? 1.0 : -1.0;
}

/// A unit direction as a point of the octahedral square, each component in
/// `-1..1` — `crcbl_shaders::probe_visibility::oct_encode`, and `mesh.slang`'s
/// body character for character. Cigolle et al. 2014, *A Survey of Efficient
/// Representations for Independent Unit Vectors*; that file is where the fold
/// onto `+Y`/`-Y` is argued.
float2 oct_encode(float3 direction)
{
    float norm = abs(direction.x) + abs(direction.y) + abs(direction.z);
    float2 p = direction.xz / max(norm, 1e-20);
    if (direction.y < 0.0)
    {
        p = float2((1.0 - abs(p.y)) * sign_not_zero(p.x),
                   (1.0 - abs(p.x)) * sign_not_zero(p.y));
    }
    return p;
}

/// The two depth moments probe `index`'s map holds along `direction`, blended
/// bilinearly — `crcbl_shaders::probe_visibility::ProbeVisibility::moments`, and
/// `mesh.slang`'s body character for character.
///
/// The clamp is against `GetDimensions` rather than against the constants above,
/// which is what lets the one-texel off-switch be read by this same code.
float2 probe_moments(uint index, float3 direction)
{
    uint width;
    uint height;
    uint layers;
    probe_visibility.GetDimensions(width, height, layers);
    float2 extent = float2(float(width), float(height));
    float2 scaled = (oct_encode(direction) * 0.5 + 0.5) * PROBE_VISIBILITY_SIDE
                  + PROBE_VISIBILITY_BORDER - 0.5;
    float2 low = clamp(floor(scaled), float2(0.0, 0.0), extent - 1.0);
    float2 high = min(low + 1.0, extent - 1.0);
    float2 weight = clamp(scaled - low, 0.0, 1.0);
    int layer = int(min(index, max(layers, 1u) - 1u));

    float2 c00 = probe_visibility.Load(int4(int(low.x), int(low.y), layer, 0)).xy;
    float2 c10 = probe_visibility.Load(int4(int(high.x), int(low.y), layer, 0)).xy;
    float2 c01 = probe_visibility.Load(int4(int(low.x), int(high.y), layer, 0)).xy;
    float2 c11 = probe_visibility.Load(int4(int(high.x), int(high.y), layer, 0)).xy;
    return lerp(lerp(c00, c10, weight.x), lerp(c01, c11, weight.x), weight.y);
}

/// The Chebyshev bound on the probe at `probe_position`'s visibility of a
/// surface at `world_position` facing `normal` — [`probe_weight`] without its
/// floor, and `mesh.slang`'s body character for character.
///
/// **The Chebyshev bound**, cubed: Donnelly & Lauritzen 2006 is where it entered
/// graphics and Majercik et al. 2019 §3 is where it is applied to a probe.
/// `mesh.slang` is where the arithmetic and the constants it reads are argued in
/// full, this split included: [`PROBE_OCCLUDED_WEIGHT`] is a property of the
/// weighted mean's divisor, so a gather that has no divisor — the
/// reflective-shadow-map updater `docs/plan/50-irradiance-probes.md` describes,
/// summing thousands of texels into one probe — calls this instead and pays no
/// per-texel leak through the wall it stands behind.
float probe_chebyshev(uint index, float3 probe_position, float3 world_position, float3 normal)
{
    float3 to_probe = probe_position - (world_position + normal * PROBE_SURFACE_BIAS);
    float to_surface = length(to_probe);
    // The map is indexed by the direction the probe looks *out* along.
    float2 moments = probe_moments(index, -to_probe);
    // Clamped because the two moments are blended independently and a texel
    // between two disagreeing neighbours can land a hair under `mean²`, which
    // is a negative variance and a visibility above one.
    float variance = max(moments.y - moments.x * moments.x, 0.0);
    float behind = to_surface - moments.x;
    float bound = variance / (variance + behind * behind);
    return to_surface <= moments.x ? 1.0 : bound * bound * bound;
}

/// The share of its trilinear weight the probe at `probe_position` keeps for a
/// surface at `world_position` facing `normal` —
/// `crcbl_shaders::probe_visibility::ProbeVisibility::weight`, and
/// `mesh.slang`'s body character for character.
///
/// [`probe_chebyshev`] floored at [`PROBE_OCCLUDED_WEIGHT`], which is what keeps
/// the trilinear blend's divisor off zero.
///
/// **A reflection asks it about the surface, not about the ray.** What this
/// weighs out is a probe the shaded point cannot see — one behind the wall the
/// point stands against — and that probe holds no light this surface may
/// reflect, whichever way the ray leaves. The direction the ray takes is the
/// evaluation in [`probe_level_environment`], and it is a separate question.
float probe_weight(uint index, float3 probe_position, float3 world_position, float3 normal)
{
    return max(probe_chebyshev(index, probe_position, world_position, normal),
               PROBE_OCCLUDED_WEIGHT);
}

/// One axis of the toroidal wrap — `mesh.slang`'s `probe_wrap`, character for
/// character, and `crcbl_shaders::probe::wrap` is the host mirror both are held
/// to.
uint probe_wrap(uint cell, uint offset, uint count)
{
    uint at = cell + offset;
    return at >= count ? at - count : at;
}

/// Which row of the probe table a grid cell of clipmap level `level` is, clamped
/// into the buffer — `mesh.slang`'s `probe_row` against this pass's own block.
///
/// **It is also the layer of [`probe_visibility`]**, which is what makes the row
/// and the map one probe: `crcbl_render::probe_capture` writes its layers in the
/// order the table's rows are in.
///
/// **The cell is wrapped by the level's scroll offset first**, which is the
/// toroidal addressing `mesh.slang` describes: a level that stepped `k` probes
/// along an axis left the other slabs naming the probes they already held.
uint probe_row(uint level, uint3 cell)
{
    uint3 counts = camera.probe_counts.xyz;
    uint3 offset = camera.probe_level_offset[level].xyz;
    uint3 at = uint3(probe_wrap(cell.x, offset.x, counts.x),
                     probe_wrap(cell.y, offset.y, counts.y),
                     probe_wrap(cell.z, offset.z, counts.z));
    uint index = camera.probe_levels.y * level
               + (at.z * counts.y + at.y) * counts.x + at.x;
    return min(index, max(camera.probe_counts.w, 1u) - 1u);
}

/// One corner of the trilinear gather: its coefficients already scaled by the
/// weight it keeps, and that weight beside them. `crcbl_shaders::probe`'s
/// `WeightedProbe` is the host mirror and `mesh.slang` carries the same struct.
struct WeightedProbe
{
    /// The row, every coefficient already multiplied by `weight`.
    GpuProbe sh;
    /// What this corner counts for: its visibility, never below
    /// [`PROBE_OCCLUDED_WEIGHT`].
    float weight;
};

/// Two corners blended coefficient by coefficient and weight by weight — one
/// axis of the trilinear interpolation below.
WeightedProbe lerp_probe(WeightedProbe a, WeightedProbe b, float t)
{
    WeightedProbe blended;
    blended.sh.sh_r = lerp(a.sh.sh_r, b.sh.sh_r, t);
    blended.sh.sh_g = lerp(a.sh.sh_g, b.sh.sh_g, t);
    blended.sh.sh_b = lerp(a.sh.sh_b, b.sh.sh_b, t);
    blended.weight = lerp(a.weight, b.weight, t);
    return blended;
}

/// The corner at cell `cell` of clipmap level `level`: its row, scaled by what
/// that probe's visibility map says a surface at `world_position` facing
/// `normal` keeps of it.
///
/// `origin` and `spacing` are the level's own, which the caller reads and
/// inverts once rather than eight times here.
WeightedProbe probe_corner(uint level, uint3 cell, float3 origin, float3 spacing,
                           float3 world_position, float3 normal)
{
    uint row = probe_row(level, cell);
    GpuProbe stored = probes[row];
    float3 at = origin + float3(cell) * spacing;
    float weight = probe_weight(row, at, world_position, normal);

    WeightedProbe corner;
    corner.sh.sh_r = stored.sh_r * weight;
    corner.sh.sh_g = stored.sh_g * weight;
    corner.sh.sh_b = stored.sh_b * weight;
    corner.weight = weight;
    return corner;
}

/// How far out of the **finest** clipmap level `world_position` stands, in that
/// level's own half-extents: zero at its centre and one on its boundary.
///
/// `crcbl_shaders::probe::ProbeVolume::level_reach` is the host mirror, and this
/// body is character for character the one `mesh.slang` carries — that pass
/// evaluates the same clipmap, and `crcbl_shaders::probe`'s
/// `the_shaders_pick_a_level_the_way_this_module_does` is what holds the two
/// copies together.
///
/// **Only the finest level is measured**, because every level is concentric with
/// it and twice as wide: level `k`'s reach is this one halved `k` times, which
/// is what lets `probe_level_of` walk the levels without touching their rows.
///
/// **An axis with no extent is contained everywhere.** A grid one probe deep, or
/// one whose reciprocal spacing is zero, has no boundary on that axis to be
/// outside of — it clamps onto probe 0 for every point — so it contributes
/// nothing here rather than dividing by its zero extent.
float probe_level_reach(float3 world_position, float3 origin, float3 inv_spacing, float3 last)
{
    float reach = 0.0;
    for (uint axis = 0; axis < 3; ++axis)
    {
        if (last[axis] == 0.0 || inv_spacing[axis] == 0.0)
            continue;
        float grid = (world_position[axis] - origin[axis]) * inv_spacing[axis];
        reach = max(reach, abs(2.0 * grid / last[axis] - 1.0));
    }
    return reach;
}

/// Which clipmap level a point of `probe_level_reach` `reach` reads, and what
/// share of the read belongs to that level rather than to the next one out: `x`
/// the level, `y` the share.
///
/// `crcbl_shaders::probe::ProbeVolume::level_of` is the host mirror, and this
/// body is `mesh.slang`'s character for character. That module is where the
/// argument for why the two sides of a level switch meet is written.
float2 probe_level_of(float reach, uint levels)
{
    for (uint level = 0; level + 1 < levels; ++level)
    {
        float at = reach * exp2(-float(level));
        if (at < 1.0)
            return float2(float(level), saturate((1.0 - at) / PROBE_LEVEL_BAND));
    }
    return float2(float(levels - 1), 1.0);
}

/// One clipmap level's manually trilinear L1 radiance evaluation in the
/// reflection direction, each corner weighed by whether that probe can see the
/// surface the ray leaves. The stored coefficients are irradiance: undo the
/// clamped-cosine transfer per band before evaluating them.
///
/// `mesh.slang`'s `probe_level_irradiance` with a reflection where its normal
/// basis is: the cell, the eight corners, the weights and the seven blends are
/// that function's, and what differs is the last three lines — a surface asks
/// what a hemisphere gives it and a reflection asks what one direction holds.
///
/// # Each corner is weighed by whether it can see the surface
///
/// `docs/plan/50-irradiance-probes.md`'s rung, on the specular path: a corner's
/// weight is its trilinear share times [`probe_weight`], the Chebyshev bound of
/// that probe's visibility map in the direction of this surface. A probe on the
/// far side of a wall is further away than the wall it recorded, so it keeps
/// nothing and its light does not come through the reflection either. The
/// coefficients ride through the same seven blends already scaled by that weight
/// and the weights ride beside them, so the sum is a weighted mean and the
/// divide at the end is what makes it one.
///
/// **Every corner occluded is the plain trilinear result, not black**, on
/// `mesh.slang`'s terms: the weight never falls below [`PROBE_OCCLUDED_WEIGHT`],
/// so the divisor cannot reach zero, and when every corner sits at that floor it
/// divides straight back out.
///
/// # A zero volume is still exactly zero
///
/// A scene with no probes addresses row zero of the zeroed table at every
/// corner, and reads the one-texel placeholder — [`probe_visibility`]'s off
/// switch — for every weight, which no surface is further away than. So every
/// weight is exactly one, scaling a coefficient by it is exact, the divisor is
/// exactly one, and what comes out is the zero this returned before the
/// weighting landed.
float3 probe_level_environment(uint level, float3 world_position, float3 normal, float3 direction)
{
    float3 last = max(float3(camera.probe_counts.xyz) - 1.0, float3(0.0, 0.0, 0.0));
    // Where this level stands and how far apart its probes are, read rather than
    // derived: `crcbl_shaders::probe::ProbeVolume` wrote these rows, so there is
    // one place the clipmap's geometry is decided.
    float3 origin = camera.probe_level_origin[level].xyz;
    float3 inv = camera.probe_level_inv_spacing[level].xyz;
    float3 grid = clamp((world_position - origin) * inv, float3(0.0, 0.0, 0.0), last);
    float3 base = floor(grid);
    float3 f = grid - base;
    uint3 lo = uint3(base);
    uint3 hi = uint3(min(base + 1.0, last));
    // The spacing the block carries the reciprocal of, so a corner's world
    // position can be named — `mesh.slang`'s spelling and its reason: an axis
    // with no reciprocal has no step to take, and every probe on it stands at
    // the origin's coordinate.
    float3 spacing = float3(inv.x != 0.0 ? 1.0 / inv.x : 0.0,
                            inv.y != 0.0 ? 1.0 / inv.y : 0.0,
                            inv.z != 0.0 ? 1.0 / inv.z : 0.0);

    // Seven blends over the coefficients and their weights, in the order
    // `mesh.slang` makes them: the far corner's weight reaches exactly zero at
    // the boundary where the cell index changes, so a fragment landing in cell
    // `i` at `f≈1` and one landing in cell `i+1` at `f≈0` compute the same
    // value.
    WeightedProbe x00 = lerp_probe(probe_corner(level, uint3(lo.x, lo.y, lo.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, lo.y, lo.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe x10 = lerp_probe(probe_corner(level, uint3(lo.x, hi.y, lo.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, hi.y, lo.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe x01 = lerp_probe(probe_corner(level, uint3(lo.x, lo.y, hi.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, lo.y, hi.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe x11 = lerp_probe(probe_corner(level, uint3(lo.x, hi.y, hi.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, hi.y, hi.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe y0 = lerp_probe(x00, x10, f.y);
    WeightedProbe y1 = lerp_probe(x01, x11, f.y);
    WeightedProbe cell = lerp_probe(y0, y1, f.z);

    float3 radiance = float3(
        dot(cell.sh.sh_r.xyz / PROBE_TRANSFER_L1, direction) + cell.sh.sh_r.w / PROBE_TRANSFER_L0,
        dot(cell.sh.sh_g.xyz / PROBE_TRANSFER_L1, direction) + cell.sh.sh_g.w / PROBE_TRANSFER_L0,
        dot(cell.sh.sh_b.xyz / PROBE_TRANSFER_L1, direction) + cell.sh.sh_b.w / PROBE_TRANSFER_L0);
    return max(radiance / cell.weight, float3(0.0, 0.0, 0.0));
}

/// The whole clipmap's L1 radiance in the reflection direction: the finest level
/// that contains the point, faded into the next one out over a band at its edge.
///
/// `mesh.slang`'s `probe_irradiance` in the shape a reflection wants —
/// `probe_level_reach` and `probe_level_of` are shared with it character for
/// character, and the two-term composite is the same spelling this file already
/// blends a screen hit with, for the same reason: it returns either end exactly.
/// A volume of one level evaluates one gather and nothing else, which is what
/// keeps a scene that asks for a single grid bit-identical.
///
/// `normal` is the reflecting surface's and `direction` the ray leaving it, and
/// the two answer different halves of the lookup: the normal is what
/// [`probe_weight`] measures a probe's visibility of this point along, and the
/// direction is what the coefficients are evaluated in.
float3 probe_environment(float3 world_position, float3 normal, float3 direction)
{
    float3 last = max(float3(camera.probe_counts.xyz) - 1.0, float3(0.0, 0.0, 0.0));
    uint levels = clamp(camera.probe_levels.x, 1u, PROBE_LEVELS);
    float reach = probe_level_reach(world_position,
                                    camera.probe_level_origin[0].xyz,
                                    camera.probe_level_inv_spacing[0].xyz,
                                    last);
    float2 pick = probe_level_of(reach, levels);
    uint level = uint(pick.x);
    float share = pick.y;

    float3 fine = probe_level_environment(level, world_position, normal, direction);
    if (share >= 1.0)
        return fine;
    float3 coarse = probe_level_environment(level + 1, world_position, normal, direction);
    return coarse * (1.0 - share) + fine * share;
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
    float sharpness = sharpness_of(surface.a);
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
    // The reconstructed normal in the same space as the point it belongs to,
    // which is what `probe_weight` biases the query off the surface along. A
    // direction, so `w` is zero and the translation column does not reach it;
    // normalised because the inverse view is a rigid transform of a normal this
    // pass derived from three depth taps.
    float3 world_normal = normalize(mul(camera.inv_view, float4(normal, 0.0)).xyz);
    // The probe grid and the sky, **added**: both are what a ray leaving this
    // surface would find, and a scene may have bounce light around it and a sky
    // above it. A zeroed probe volume leaves the sky alone and a black sky
    // leaves the probes alone, which is what made each of them land switched
    // off. `docs/plan/43-render-standards.md` §8 is why the sky is here at all:
    // the environment a missed ray falls back to is the term a sky provides.
    //
    // The sky at the surface's roughness, not along the one mirror direction:
    // a rough lobe gathers the gradient from a cone about the reflection, and
    // `docs/plan/44-lighting.md`'s rung 3 cooked that gather into
    // `sky_prefilter`. This is the split-sum's prefiltered half; `env_brdf`
    // below is the other.
    float3 environment = probe_environment(world_position, world_normal, reflection_direction)
                       + sky_prefiltered(reflection_direction, surface.a);

    // The split-sum's second half: the `DFG` pair at `(N·V, roughness)`, and
    // `f0 * scale + bias` is what the lobe's Fresnel and visibility integrate
    // to over the environment it gathers. The table's roughness-zero row is
    // Schlick's expansion exactly, so a mirror takes what it took when this
    // was Schlick along the one reflected direction; a rough surface takes
    // less, which is the energy that single direction was over-counting.
    float n_dot_v = saturate(dot(normal, -towards));
    float3 f0 = surface.rgb;
    float2 dfg_terms = dfg_at(n_dot_v, surface.a);
    float3 env_brdf = f0 * dfg_terms.x + dfg_terms.y;

    // A probe is a blurry environment, so it remains valid when a single
    // screen-space ray is not. Its zero alpha keeps it out of the sharpness
    // kernel; `ssr_blur.slang` composites this centre value directly.
    if (sharpness <= 0.0)
    {
        return float4(environment * env_brdf, 0.0);
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
        return float4(environment * env_brdf, sharpness);
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
        return float4(environment * env_brdf, sharpness);
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
        return float4(environment * env_brdf, sharpness);
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
        return float4(environment * env_brdf, sharpness);
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
    float3 reflection = environment * env_brdf;
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
                reflection = hit_color * env_brdf * confidence
                    + environment * env_brdf * (1.0 - confidence);
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
