// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/51-volumetrics.md`'s froxel column: what the air in front of every
// surface adds to the frame, and what it takes away, computed once per froxel
// instead of once per pixel.
//
// Two dispatches, and the split is what the rungs are built on:
//
// * `scatterMain` owns **one froxel** and answers two questions about the slab
//   of air in it — how much radiance it scatters towards the eye, and how much
//   of what is behind it gets through. That is where a light's contribution
//   goes when rung 2 of topic 51 arrives; today the medium scatters the frame's
//   fog colour and nothing else.
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
    /// The radiance the medium scatters towards the eye, in `rgb`; `w` unused.
    float4 fog_color;
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

/// What one slice of the medium does, given the segment of the tile's centre
/// ray it occupies.
///
/// `radiance` is what the slice scatters towards the eye and `transmittance` is
/// what survives it. **The medium's single-scattering albedo is one here** —
/// every unit of radiance the slab takes out of the view ray is a unit it puts
/// back in from the environment — which is what makes the whole column
/// algebraically the closed form `mesh.slang` composites, and is the property
/// `crcbl`'s `mesh_e2e` measures. Rung 2 of `docs/plan/51-volumetrics.md` is
/// where a light's own contribution joins `radiance` and the two stop being
/// equal.
float4 volumetric_slice(float3 from, float3 to)
{
    float reference = params.fog_params.z;
    float tau = fog_optical_depth(params.fog_params.x, params.fog_params.y, from.y - reference,
                                  to.y - reference, length(to - from));
    float survives = fog_exp_neg(tau);
    return float4(params.fog_color.rgb * (1.0 - survives), survives);
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

    volumetrics[froxel] = volumetric_slice(params.eye.xyz + along * from_depth,
                                           params.eye.xyz + along * to_depth);
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
