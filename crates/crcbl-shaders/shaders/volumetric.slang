// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/51-volumetrics.md`'s froxel column: what the air in front of every
// surface adds to the frame, and what it takes away, computed once per froxel
// instead of once per pixel.
//
// Two dispatches, and the split is what the rungs are built on:
//
// * `scatterMain` owns **one froxel** and answers two questions about the slab
//   of air in it — how much radiance it scatters towards the eye, and how much
//   of what is behind it gets through, with the sun's own contribution shadowed
//   through the cascade atlas. That is where a punctual light's contribution
//   goes when rung 2 of topic 51 arrives.
// * `integrateMain` owns **one screen tile** and walks its column from the eye
//   outward, turning each slice's own pair into the running pair in front of
//   it. `volumetric_composite.slang` then reads one froxel per pixel.
//
// # Why two passes and not one
//
// The prefix scan is sequential along `z` — slice `k`'s answer needs every
// slice in front of it — and there is no cross-workgroup ordering inside a
// dispatch to hang that on. A column per invocation is the arrangement that
// needs no ordering at all, and it is also what makes the scan's writes
// conflict-free: one invocation owns one tile's whole column and nothing else
// writes there. The scatter half is the opposite shape, one invocation per
// froxel and embarrassingly parallel, which is where the light loop wants to
// be. `clear_counters.slang`'s header argues the same shape for the same
// reason.
//
// # The buffer holds two different things, before and after
//
// `scatterMain` writes each froxel's **own** slice, `integrateMain` overwrites
// it with the **prefix in front of** that slice. One buffer rather than two
// because the scan reads a cell exactly once, immediately before writing it,
// from the single invocation that owns the whole column — so there is no
// version of this where a reader could see the wrong meaning.
//
// # Nothing here calls a transcendental
//
// `docs/plan/44-lighting.md`'s shading rule. The exponential is
// `crcbl_shaders::fog`'s construction, copied here for the third time because
// there is no `#include` in these shaders;
// `crcbl_shaders::fog::the_shader_spells_the_same_constants` enumerates every
// source carrying it and is what stops the copies drifting.

/// Depth slices a perspective grid has, matching
/// `crcbl_shaders::light::CLUSTER_DEPTH_SLICES`.
static const uint CLUSTER_DEPTH_SLICES = 24;

/// The view depth the first slice starts at, matching
/// `crcbl_shaders::light::CLUSTER_NEAR`.
static const float CLUSTER_NEAR = 0.1;

/// The view depth the last slice's *resolution* ends at, matching
/// `crcbl_shaders::light::CLUSTER_FAR`.
static const float CLUSTER_FAR = 1000.0;

/// The ratio between one slice's start and the next, matching
/// `crcbl_shaders::light::SLICE_RATIO`.
///
/// **A constant rather than the `pow` `light_cluster.slang` writes**, and the
/// reason is this workspace's shading rule: that shader's slice boundary is
/// floored into an integer index, where a last-place disagreement changes
/// nothing, while this one is an endpoint of an optical-depth integral and
/// reaches a colour. A chain of multiplies by a constant is exact arithmetic
/// on every target and has no add in it for a compiler to contract.
static const float CLUSTER_SLICE_RATIO = 1.4677993;

/// The reversed-Z near plane in NDC, matching `crcbl_hal::depth::NEAR`.
static const float DEPTH_NEAR = 1.0;

/// `log2(e)`, mirroring `crcbl_shaders::fog::LOG2_E`.
static const float FOG_LOG2_E = 1.44269504;

/// `ln 2` with its low mantissa bits cleared. `crcbl_shaders::fog::LN2_HI`.
static const float FOG_LN2_HI = 0.69311523;

/// What `FOG_LN2_HI` dropped. `crcbl_shaders::fog::LN2_LO`.
static const float FOG_LN2_LO = 3.1946183e-5;

/// The largest magnitude `fog_exp_neg` takes before it saturates.
/// `crcbl_shaders::fog::MAX_ARGUMENT`.
static const float FOG_MAX_ARGUMENT = 87.0;

/// Optical depth past which `fog_optical_depth` stops counting.
/// `crcbl_shaders::fog::MAX_OPTICAL_DEPTH`.
static const float FOG_MAX_OPTICAL_DEPTH = 32.0;

/// Below this reduced height difference the ratio below takes its series.
/// `crcbl_shaders::fog::SERIES_CUTOFF`.
static const float FOG_SERIES_CUTOFF = 0.125;

/// Reciprocal factorials, `1/i!`. `crcbl_shaders::fog::KERNEL`.
static const float FOG_KERNEL[8] = {
    1.0, 1.0, 0.5, 0.16666667, 0.041666668, 0.008333334, 0.0013888889, 0.00019841270
};

/// The same one factorial along, `1/(i+1)!`. `crcbl_shaders::fog::RATIO_KERNEL`.
static const float FOG_RATIO_KERNEL[5] = {
    1.0, 0.5, 0.16666667, 0.041666668, 0.008333334
};

/// How many cascades the sun's shadow map is split into, matching
/// `crcbl_shaders::mesh::SHADOW_CASCADES` and `mesh.slang`'s own copy.
///
/// The froxel volume asks the same atlas the same question a fragment does, so
/// a disagreement here would light a shaft against a cascade the surfaces in it
/// were never shadowed by.
static const uint SHADOW_CASCADES = 2;

/// Tiles across the shadow atlas, matching
/// `crcbl_shaders::mesh::SHADOW_ATLAS_COLUMNS`.
static const uint SHADOW_ATLAS_COLUMNS = 4;

/// Tiles down it, matching `crcbl_shaders::mesh::SHADOW_ATLAS_ROWS`.
static const uint SHADOW_ATLAS_ROWS = 4;

/// What neither dispatch can derive for itself, matching
/// `crcbl_shaders::volumetric::VolumetricParams`.
///
/// The same block is bound by `volumetric_composite.slang`, so the froxel a
/// pixel lands in and the froxel this pass wrote are computed from one set of
/// numbers rather than from two that could disagree.
struct VolumetricParams
{
    /// Clip → world, for unprojecting a tile's screen centre.
    float4x4 inverse_view_proj;
    /// World-space eye in `xyz`; `w` unused.
    float4 eye;
    /// Row 3 of the view-projection, so `dot(depth_row, float4(p, 1))` is `p`'s
    /// view depth — the same row `light_cluster.slang` is handed, from the same
    /// matrix, so the two grids cut the frustum in the same places.
    float4 depth_row;
    /// Exponential height fog: `x` the density at the reference height, `y` the
    /// scale height, `z` that reference height in world units, `w` unused.
    ///
    /// **The same numbers `FrameUniforms::fog_params` carries**, and the
    /// renderer hands them to exactly one of the two paths — see
    /// `crcbl_render::volumetric`, and `docs/plan/51-volumetrics.md` on why
    /// charging both squares the transmittance.
    float4 fog_params;
    /// The radiance the medium scatters towards the eye from every direction
    /// alike, in `rgb`; `w` unused. The environment term, and the whole of what
    /// `mesh.slang`'s closed form can answer.
    float4 fog_color;
    /// The unit vector **towards** the sun in `xyz` — `mesh.slang`'s
    /// `to_light` — and the medium's anisotropy in `w`.
    float4 sun_direction;
    /// What the medium scatters out of the sun per unit length, in `rgb`; `w`
    /// unused.
    ///
    /// **Zero is exactly no sun.** The source below is a sum, so a zero here
    /// leaves the environment term bit for bit — which is what keeps a column
    /// with no sun in it algebraically the closed form, and is the state every
    /// frame is in until a caller sets `Fog::sun_scattering`.
    float4 sun_radiance;
    /// World → cascade `i`'s shadow clip, one matrix per cascade — the same
    /// rows `mesh.slang`'s `FrameUniforms` carries, so a froxel and a fragment
    /// project into the same atlas tile.
    float4x4 shadow_view_proj[SHADOW_CASCADES];
    /// Component `i` is how far from the eye cascade `i` reaches, in world
    /// units; the first one that covers a point is the one it is looked up in.
    float4 cascade_far;
    /// `xy`: one atlas texel in `u` and in `v`, which is what the PCF kernel
    /// steps by. `zw` are `mesh.slang`'s receiver biases and **nothing here
    /// reads them**: a bias exists to stop a facet shadowing itself, and a
    /// froxel is a volume with no facet in it.
    float4 shadow_params;
    uint grid_x;
    uint grid_y;
    /// `CLUSTER_DEPTH_SLICES` for a perspective camera, `1` for an orthographic
    /// one — the grid `crcbl_render::light_grid`'s `Grid::for_frame` sized.
    uint slices;
    /// How many pixels wide and tall one tile is, matching
    /// `FrameUniforms::cluster_grid`'s `w`. A parameter for the reason
    /// `light_cluster.slang` gives: a viewport too large for the grid gets
    /// coarser tiles rather than an index off the end of a buffer.
    uint tile_pixels;
    uint viewport_x;
    uint viewport_y;
    /// Froxels the buffer holds, which both dispatches bound their writes by.
    uint froxel_count;
    uint pad0;
};

[[vk::binding(0, 0)]]
ConstantBuffer<VolumetricParams> params;

/// One `float4` per froxel, and it means two different things — see the header.
///
/// After `scatterMain`: `rgb` is the radiance that slice scatters towards the
/// eye and `a` is the fraction of what is behind it that gets through.
/// After `integrateMain`: `rgb` is the radiance accumulated **in front of**
/// that slice and `a` is the transmittance through all of it.
[[vk::binding(1, 0)]]
RWStructuredBuffer<float4> volumetrics;

/// The sun's cascades, in the atlas `crcbl_render::shadow` fills — the same
/// image `mesh.slang` reads at its own binding 15.
///
/// `DepthTexture2D` for that file's reason: WebGPU binds a `D32Float` view only
/// through a depth sample type, and the WGSL artifact declares
/// `texture_depth_2d`.
[[vk::binding(2, 0)]]
DepthTexture2D shadow_atlas;

/// `SamplerComparisonState`, so a tap returns the *fraction* the hardware
/// filtered rather than a depth to compare on this side.
///
/// **Bound to a compute stage**, which is what is new here: `SampleCmpLevelZero`
/// asks for no derivatives and is allowed outside a fragment shader on all four
/// targets, where the implicit-LOD `SampleCmp` is not.
[[vk::binding(3, 0)]]
SamplerComparisonState shadow_sampler;

/// What fraction of the sun each froxel can see, written by `scatterMain` and
/// read by `volumetric_composite.slang`.
///
/// **The cascade walk happens once per froxel and this is how the answer
/// leaves.** The composite integrates the partial slice a pixel sits inside
/// along that pixel's own ray, and it needs the same scattering source this
/// pass gave the whole froxel; without a number to read it would have to walk
/// the atlas again, per pixel, which is the cost the froxel grid exists to
/// avoid. `integrateMain` neither reads nor writes it — the scan is over the
/// column buffer alone, and this survives it untouched.
[[vk::binding(4, 0)]]
RWStructuredBuffer<float> visibilities;

/// `e^-x`, without calling a transcendental.
/// `crcbl_shaders::fog::exp_neg`; see `mesh.slang`'s copy for the derivation.
float fog_exp_neg(float x)
{
    float clamped = clamp(x, -FOG_MAX_ARGUMENT, FOG_MAX_ARGUMENT);

    float n = floor(clamped * FOG_LOG2_E + 0.5);
    float reduced = (clamped - n * FOG_LN2_HI) - n * FOG_LN2_LO;

    float u = -reduced;
    float kernel = FOG_KERNEL[7];
    for (int term = 6; term >= 0; --term)
    {
        kernel = kernel * u + FOG_KERNEL[term];
    }

    return kernel * asfloat(uint(127 - int(n)) << 23);
}

/// `(1 - e^-d) / d`, which tends to one as `d` does.
/// `crcbl_shaders::fog::one_minus_exp_over`.
float fog_one_minus_exp_over(float d)
{
    if (abs(d) < FOG_SERIES_CUTOFF)
    {
        float u = -d;
        float series = FOG_RATIO_KERNEL[4];
        for (int term = 3; term >= 0; --term)
        {
            series = series * u + FOG_RATIO_KERNEL[term];
        }
        return series;
    }
    return (1.0 - fog_exp_neg(d)) / d;
}

/// The optical depth of exponential height fog along one segment.
/// `crcbl_shaders::fog::optical_depth`.
float fog_optical_depth(float density, float falloff, float height_a, float height_b,
                        float distance)
{
    if (falloff <= 0.0)
    {
        return clamp(density * distance, 0.0, FOG_MAX_OPTICAL_DEPTH);
    }

    float a = height_a / falloff;
    float d = (height_b - height_a) / falloff;
    float tau = density * distance * fog_exp_neg(a) * fog_one_minus_exp_over(d);
    return clamp(tau, 0.0, FOG_MAX_OPTICAL_DEPTH);
}

/// The world-space point at NDC `(ndc, depth)`.
float3 volumetric_unproject(float2 ndc, float depth)
{
    float4 world = mul(params.inverse_view_proj, float4(ndc, depth, 1.0));
    return world.xyz / world.w;
}

/// The view depth slice `index` starts at, by the multiply chain
/// `CLUSTER_SLICE_RATIO` exists for.
///
/// `light_cluster.slang`'s `slice_start` is the same sequence written as a
/// `pow`; the two agree to within a unit in the last place, and neither grid
/// reads the other's boundaries. `volumetric_composite.slang` finds a pixel's
/// slice by running this same chain forward, so the boundary a column was cut
/// at and the boundary a pixel is measured from are the same number by
/// construction rather than by two roundings agreeing.
float volumetric_slice_start(uint index)
{
    float start = CLUSTER_NEAR;
    for (uint step = 0; step < index; ++step)
    {
        start *= CLUSTER_SLICE_RATIO;
    }
    return start;
}

/// A tile's ray: the point its screen centre unprojects to on the near plane,
/// and the view depth of that plane.
///
/// Everything a column needs about its own geometry comes from this pair. A
/// point at view depth `d` along the tile's centre ray is
/// `eye + (near_point - eye) * (d / near_depth)`, so the segment between two
/// view depths is `length(near_point - eye) / near_depth` times their
/// difference — **the secant factor**, which is what stops a froxel at the
/// corner of the frame being measured as if it sat on the axis. At a wide field
/// of view that is tens of per cent of the frame's edges, in the direction that
/// brightens them.
void volumetric_tile_ray(uint tile_x, uint tile_y, out float3 near_point, out float near_depth)
{
    float2 viewport = float2(float(max(params.viewport_x, 1u)), float(max(params.viewport_y, 1u)));
    float2 pixel = (float2(float(tile_x), float(tile_y)) + 0.5) * float(params.tile_pixels);
    // Y-up NDC against a pixel row zero that is the top of the screen, exactly
    // as `light_cluster.slang` builds its tile rectangle.
    float2 ndc = float2(pixel.x / viewport.x * 2.0 - 1.0, 1.0 - pixel.y / viewport.y * 2.0);
    near_point = volumetric_unproject(ndc, DEPTH_NEAR);
    near_depth = max(dot(params.depth_row, float4(near_point, 1.0)), 1e-6);
}

/// `1 / (4 pi)`. `crcbl_shaders::volumetric::INV_FOUR_PI`.
static const float VOLUMETRIC_INV_FOUR_PI = 0.079577470;

/// The largest anisotropy the phase function accepts, in either direction.
/// `crcbl_shaders::volumetric::MAX_ANISOTROPY`.
///
/// At `|g| = 1` the lobe is a delta function and the denominator is exactly
/// zero straight ahead, so this keeps a division by zero out of a colour rather
/// than being a tuning knob.
static const float VOLUMETRIC_MAX_ANISOTROPY = 0.99;

/// The Henyey-Greenstein phase function.
/// `crcbl_shaders::volumetric::phase`, whose tests are the proof this is right:
/// it integrates to one over the sphere at every anisotropy, so the medium
/// redistributes light rather than creating it.
///
/// `cos_theta` is the cosine between the direction the light **was travelling**
/// and the direction it leaves in, so `1.0` is straight on. Light from the sun
/// travels along `-sun_direction` and leaves towards the eye along
/// `-view_direction`, which makes the cosine `dot(sun_direction, view_direction)`
/// — a camera looking into the sun sees the forward lobe, which is what a shaft
/// is.
///
/// The three-halves power is `d * sqrt(d)` and not `pow(d, 1.5)`: IEEE-754
/// requires a correctly rounded `sqrt` and specifies nothing at all about
/// `pow`, and this result reaches a colour. See `docs/plan/44-lighting.md`.
float volumetric_phase(float g, float cos_theta)
{
    float a = clamp(g, -VOLUMETRIC_MAX_ANISOTROPY, VOLUMETRIC_MAX_ANISOTROPY);
    float d = 1.0 + a * a - 2.0 * a * clamp(cos_theta, -1.0, 1.0);
    return VOLUMETRIC_INV_FOUR_PI * (1.0 - a * a) / (d * sqrt(d));
}

/// What the medium sends towards the eye from a froxel whose view ray runs
/// along the unit vector `view_direction`.
///
/// A **sum**: the environment term `fog_color`, which is what the closed form
/// answers, plus the sun redistributed by the phase function. The sum is what
/// makes the sun an off-switch rather than a mode — `sun_radiance` at zero
/// leaves `fog_color` untouched bit for bit, and the column stays the closed
/// form the fragment stage was computing.
///
/// `visibility` is what `volumetric_sun_visibility` gave this froxel: it scales
/// the sun term alone, because the environment term is what reaches the medium
/// from everywhere and an occluder in front of the sun does not remove it.
/// **That is what makes this a shaft rather than a dimmer**: the shadowed air
/// keeps its ambient glow and loses the beam.
float3 volumetric_source(float3 view_direction, float visibility)
{
    float cos_theta = dot(params.sun_direction.xyz, view_direction);
    float3 sun = params.sun_radiance.rgb
               * volumetric_phase(params.sun_direction.w, cos_theta) * visibility;
    return params.fog_color.rgb + sun;
}

/// Where tile `tile` puts `tile_uv` in the whole atlas.
///
/// `mesh.slang`'s function of the same name, letter for letter — the grid is
/// row-major and the cascades are its first tiles, which is the arithmetic
/// `crcbl_render::shadow::tile_origin` performs on the host.
/// `crcbl_shaders::volumetric`'s `both_shaders_spell_the_same_atlas_walk` is
/// what holds the two copies together, there being no `#include` here.
float2 atlas_uv(uint tile, float2 tile_uv)
{
    float2 cell = float2(float(tile % SHADOW_ATLAS_COLUMNS), float(tile / SHADOW_ATLAS_COLUMNS));
    return (cell + tile_uv) / float2(float(SHADOW_ATLAS_COLUMNS), float(SHADOW_ATLAS_ROWS));
}

/// The shadow filter's tap count, reach, disc and rotations, which are
/// `mesh.slang`'s and are compared against it by
/// `crcbl_shaders::volumetric`'s `both_shaders_spell_the_same_atlas_walk`.
///
/// Why each is what it is lives in that file beside its own copy; a froxel
/// filters the atlas the same way a fragment does so that a shaft of light and
/// the surface behind it share one penumbra rather than two.
static const uint SHADOW_TAPS = 32;

static const float SHADOW_FILTER_TEXELS = 2.0;

static const float2 SHADOW_DISC[SHADOW_TAPS] = {
    float2(+0.125000, +0.000000), float2(-0.159645, +0.146248),
    float2(+0.024436, -0.278438), float2(+0.201222, +0.262459),
    float2(-0.369268, -0.065318), float2(+0.349802, -0.222516),
    float2(-0.117002, +0.435242), float2(-0.223136, -0.429634),
    float2(+0.484115, +0.176798), float2(-0.503641, +0.207896),
    float2(+0.242788, -0.518824), float2(+0.179414, +0.572001),
    float2(-0.540757, -0.313380), float2(+0.634370, -0.139464),
    float2(-0.387146, +0.550675), float2(-0.089440, -0.690200),
    float2(+0.549072, +0.462758), float2(-0.738878, +0.030555),
    float2(+0.538955, -0.536332), float2(-0.036058, +0.779792),
    float2(-0.512818, -0.614527), float2(+0.812360, +0.109302),
    float2(-0.688311, +0.478909), float2(+0.188086, -0.836061),
    float2(+0.435033, +0.759191), float2(-0.850448, -0.271316),
    float2(+0.826102, -0.381680), float2(-0.357888, +0.855156),
    float2(-0.319407, -0.888034), float2(+0.849909, +0.446688),
    float2(-0.944035, +0.248845), float2(+0.536596, -0.834530),
};

/// The probe the filter takes before its disc, and which of the disc's taps it
/// reads. `mesh.slang`'s, and why a probe exists at all lives there.
///
/// A froxel column pays this the same way a fragment does: most of the medium
/// is wholly lit or wholly in shadow, and the shaft's edge is the only place
/// the disc is what answers.
static const uint SHADOW_PROBE_TAPS = 5;

static const uint SHADOW_PROBE_INDEX[SHADOW_PROBE_TAPS] = { 0u, 23u, 25u, 27u, 29u };

static const float2 SHADOW_ROTATIONS[16] = {
    float2(+1.000000, +0.000000), float2(+0.923880, +0.382683),
    float2(+0.707107, +0.707107), float2(+0.382683, +0.923880),
    float2(+0.000000, +1.000000), float2(-0.382683, +0.923880),
    float2(-0.707107, +0.707107), float2(-0.923880, +0.382683),
    float2(-1.000000, +0.000000), float2(-0.923880, -0.382683),
    float2(-0.707107, -0.707107), float2(-0.382683, -0.923880),
    float2(-0.000000, -1.000000), float2(+0.382683, -0.923880),
    float2(+0.707107, -0.707107), float2(+0.923880, -0.382683),
};

static const uint SHADOW_DITHER[16] = {
    0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u,
};

/// Which of [`SHADOW_ROTATIONS`] this pixel takes, as a `(cos, sin)` pair.
///
/// `mesh.slang`'s helper. There it is shared between the blocker search and the
/// filter that search sizes; here there is only the filter, and it stays a
/// function so the two files' kernels remain the same text.
float2 shadow_rotation(float2 pixel)
{
    uint2 cell = uint2(pixel) & 3u;
    return SHADOW_ROTATIONS[SHADOW_DITHER[cell.y * 4u + cell.x]];
}

/// One tap of the filter, `mesh.slang`'s letter for letter: the spoke turned,
/// clamped half a texel inside the tile so the sampler's own 2×2 cannot reach
/// the neighbouring map, and compared.
float tile_tap(uint tile, float2 tile_uv, float2 spoke, float2 rotation, float reference)
{
    float2 texel = params.shadow_params.xy;
    // The grid's two extents, which is what turns an *atlas* texel into a step
    // in tile space: a tile is `1 / SHADOW_ATLAS_COLUMNS` of the atlas wide and
    // `1 / SHADOW_ATLAS_ROWS` of it tall, so scaling back up by them lands on
    // exactly one shadow-map texel on either axis.
    float2 grid = float2(float(SHADOW_ATLAS_COLUMNS), float(SHADOW_ATLAS_ROWS));
    float2 tile_min = float2(0.5, 0.5) * texel * grid;
    // The rotation, written out: a complex multiply by `(cos, sin)`.
    float2 turned = float2(spoke.x * rotation.x - spoke.y * rotation.y,
                           spoke.x * rotation.y + spoke.y * rotation.x);
    float2 tap = clamp(tile_uv + turned * texel * grid, tile_min, 1.0 - tile_min);
    return shadow_atlas.SampleCmpLevelZero(shadow_sampler, atlas_uv(tile, tap), reference);
}

/// Rotated-disc PCF over tile `tile`, at `tile_uv` in that tile's own `0..1`
/// space, dithered by `pixel`.
///
/// `mesh.slang`'s kernel, probe and all: [`SHADOW_PROBE_TAPS`] taps first, the
/// remaining twenty-seven only where those disagree.
///
/// `radius` is a parameter there because the sun's penumbra is measured per
/// fragment; the one caller here passes [`SHADOW_FILTER_TEXELS`] and always
/// will, for the reason `volumetric_sun_visibility` gives.
///
/// `pixel` is the froxel's column index where a fragment passes its own pixel
/// coordinate. It stands in for one exactly: the froxel grid is a tiling of the
/// screen, so neighbouring columns are neighbouring pixels made coarse, and the
/// lattice that picks a rotation wants nothing finer than an integer that
/// changes between neighbours.
float tile_pcf(uint tile, float2 tile_uv, float reference, float2 pixel, float radius)
{
    float2 rotation = shadow_rotation(pixel);

    float probe = 0.0;
    for (uint spot = 0u; spot < SHADOW_PROBE_TAPS; ++spot)
    {
        probe += tile_tap(tile, tile_uv, SHADOW_DISC[SHADOW_PROBE_INDEX[spot]] * radius,
                          rotation, reference);
    }
    // Unanimity, tested at both ends rather than by a tolerance: a comparison
    // sampler returns exactly zero or exactly one wherever the four texels it
    // filters agree, whatever weights it gave them, so the sum of five is an
    // exact integer on every fragment this early-out is meant for.
    if (probe <= 0.0)
    {
        return 0.0;
    }
    if (probe >= float(SHADOW_PROBE_TAPS))
    {
        return 1.0;
    }

    float visibility = 0.0;
    for (uint index = 0u; index < SHADOW_TAPS; ++index)
    {
        visibility += tile_tap(tile, tile_uv, SHADOW_DISC[index] * radius, rotation, reference);
    }
    return visibility / float(SHADOW_TAPS);
}

/// The fraction of the sun a point in the medium can see: `1.0` fully lit,
/// `0.0` fully shadowed, the values between being the PCF filter.
///
/// `mesh.slang`'s `sun_visibility` with **both of its biases dropped and its
/// early return with them**. Each of the three is about a receiving *surface*:
/// the depth and slope offsets exist so a facet does not shadow itself across
/// one texel of the map, and the `n_dot_l` return is "this fragment's diffuse
/// term is already zero". A froxel is a volume of air. It has no facet to
/// self-shadow and no normal to face away, and biasing it would push the
/// scattering it reports *out* of the shadow it is standing in — which is
/// visible as a lit rim along every shaft.
///
/// Outside the last cascade's box it is lit, which is that function's answer
/// and for its reason: a shadow map knows nothing about what it never
/// rendered, and inventing an occluder there is a black wall across everything
/// past the last split.
float volumetric_sun_visibility(float3 world_position, float2 pixel)
{
    // Distance to the eye rather than view-space depth, because the splits are
    // radii around the camera — the same selection a fragment runs, so a
    // froxel and the surface behind it land in the same cascade.
    float eye_distance = length(world_position - params.eye.xyz);
    uint cascade = SHADOW_CASCADES - 1;
    for (uint index = 0; index < SHADOW_CASCADES; ++index)
    {
        if (eye_distance < params.cascade_far[index])
        {
            cascade = index;
            break;
        }
    }

    float4 clip = mul(params.shadow_view_proj[cascade], float4(world_position, 1.0));
    // Orthographic, so `w` is one and the divide is a formality — written out
    // because nothing here may assume which constructor built a matrix.
    float3 ndc = clip.xyz / clip.w;
    if (any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0)
    {
        return 1.0;
    }

    // Y-up NDC to top-left texture space, the flip `crcbl_render`'s projections
    // push into the viewport rather than into the matrix.
    float2 tile_uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // The constant width, not the sun's measured one. A blocker search sizes a
    // penumbra from how far a *surface* stands below its caster, and a froxel is
    // a volume of air with no surface to stand: the height it would measure is
    // the distance to whatever the map holds behind it, which is not a shadow
    // this air is inside. The same reasoning that drops both biases here.
    return tile_pcf(cascade, tile_uv, ndc.z, pixel, SHADOW_FILTER_TEXELS);
}

/// What one slice of the medium does, given the segment of the tile's centre
/// ray it occupies.
///
/// `radiance` is what the slice scatters towards the eye and `transmittance` is
/// what survives it. **The environment term's single-scattering albedo is one**
/// — every unit of radiance the slab takes out of the view ray is a unit it
/// puts back in from every direction alike — which is what makes a column with
/// no sun in it algebraically the closed form `mesh.slang` composites, and is
/// the property `crcbl`'s `mesh_e2e` measures. The sun is added on top of that
/// and is zero until a caller asks for it, so the equality is an off-switch
/// rather than a rung that has been left behind.
float4 volumetric_slice(float3 from, float3 to, float visibility)
{
    float reference = params.fog_params.z;
    float3 segment = to - from;
    float length_of = length(segment);
    float tau = fog_optical_depth(params.fog_params.x, params.fog_params.y, from.y - reference,
                                  to.y - reference, length_of);
    float survives = fog_exp_neg(tau);
    // The direction the slice is looked along, for the phase function. A
    // degenerate segment cannot be normalised, and its `1 - survives` is zero
    // anyway, so the axis it falls back to is unobservable.
    float3 view_direction = length_of > 1e-6 ? segment / length_of : float3(0.0, 0.0, 1.0);
    return float4(volumetric_source(view_direction, visibility) * (1.0 - survives), survives);
}

/// Invocations per workgroup, matching
/// `crcbl_shaders::volumetric::WORKGROUP_SIZE`.
static const uint VOLUMETRIC_WORKGROUP_SIZE = 64;

/// One invocation per froxel: what the slab of air in it does on its own.
[shader("compute")]
[numthreads(64, 1, 1)]
void scatterMain(uint3 thread: SV_DispatchThreadID)
{
    uint froxel = thread.x;
    uint tiles = max(params.grid_x, 1u) * max(params.grid_y, 1u);
    uint slices = max(params.slices, 1u);
    if (froxel >= tiles * slices || froxel >= params.froxel_count)
    {
        return;
    }

    uint tile_x = froxel % max(params.grid_x, 1u);
    uint tile_y = (froxel / max(params.grid_x, 1u)) % max(params.grid_y, 1u);
    uint slice = froxel / tiles;

    float3 near_point;
    float near_depth;
    volumetric_tile_ray(tile_x, tile_y, near_point, near_depth);
    float3 along = (near_point - params.eye.xyz) / near_depth;

    // **The last slice's far side is the camera's, not `FLT_MAX`.**
    // `light_cluster.slang` leaves it unbounded so a light past `CLUSTER_FAR`
    // is still listed, and an unbounded *length* here would be an optical depth
    // of infinity in a colour. Nothing is behind the far plane to be lit.
    // **Slice zero starts at the eye, not at `CLUSTER_NEAR`.** The grid's near
    // plane is where its *resolution* begins; leaving the first tenth of a unit
    // out of the column would put a gap in the medium that the closed form does
    // not have, and every pixel in the frame is behind it.
    float from_depth = slice == 0 ? 0.0 : volumetric_slice_start(slice);
    float to_depth = slice + 1 == slices ? CLUSTER_FAR : volumetric_slice_start(slice + 1);

    float3 from = params.eye.xyz + along * from_depth;
    float3 to = params.eye.xyz + along * to_depth;

    // **The slice's midpoint, and one lookup for the whole of it.** A slab of
    // air has no single place the sun does or does not reach it, and the
    // alternative to picking one is marching the segment — which at 24 slices a
    // column is the shadow cost of a ray march, and is what rung 3's temporal
    // reprojection buys instead. The midpoint is where the error is smallest for
    // a slice that is entirely in or entirely out, which is what almost every
    // slice is: a shaft's edge is one froxel wide either way.
    // The column's own grid position stands in for a pixel coordinate, which is
    // what dithers the filter's rotation. Every slice of one column takes the
    // same rotation and neighbouring columns take different ones, which is the
    // property the lattice is built for.
    float visibility =
        volumetric_sun_visibility((from + to) * 0.5, float2(float(tile_x), float(tile_y)));
    visibilities[froxel] = visibility;
    volumetrics[froxel] = volumetric_slice(from, to, visibility);
}

/// One invocation per screen tile: the exclusive prefix scan down its column.
///
/// Exclusive rather than inclusive, and that is what makes the composite exact
/// rather than quantised to a slice boundary: a pixel reads the column **in
/// front of** its own slice and integrates the part of that slice it actually
/// sits in for itself. An inclusive scan would charge every pixel for the whole
/// of the slice it is inside, which at the far end of an exponential split is a
/// segment hundreds of metres long.
[shader("compute")]
[numthreads(64, 1, 1)]
void integrateMain(uint3 thread: SV_DispatchThreadID)
{
    uint tile = thread.x;
    uint tiles = max(params.grid_x, 1u) * max(params.grid_y, 1u);
    if (tile >= tiles)
    {
        return;
    }
    uint slices = max(params.slices, 1u);

    float3 accumulated = float3(0.0, 0.0, 0.0);
    float through = 1.0;
    for (uint slice = 0; slice < slices; ++slice)
    {
        uint froxel = tile + slice * tiles;
        if (froxel >= params.froxel_count)
        {
            break;
        }
        float4 own = volumetrics[froxel];
        volumetrics[froxel] = float4(accumulated, through);
        accumulated += through * own.rgb;
        through *= own.a;
    }
}
